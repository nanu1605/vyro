//! `RamProvider`: memory module temperatures over the SMBus (AMD's FCH through `SmbusPIIX4`,
//! Intel's PCH through `SmbusI801`) and memory timings from the memory controller (AMD's UMC
//! over SMN through `AMDFamily17`, Intel's through `IntelMCHBAR`). Each half works without the
//! other; with neither `init` fails and the memory fields it owns stay empty.

use super::{
    channel_active, channel_offset, decode_ddr5, decode_intel, dram_type, has_sensor, intel_imc,
    is_spd5_hub, is_tse2004, slot_name, spd5_temp, tsod_temp, DramType, DDR5_REGS, INTEL_REGS,
    MAX_CHANNELS, MR_CAPABILITY, MR_DEVICE_TYPE_LSB, MR_DEVICE_TYPE_MSB, MR_TS_CONFIG, MR_TS_SENSE,
    SPD_FIRST, SPD_LAST, TSOD_FIRST, TSOD_LAST, TS_CAPABILITY, TS_CONFIG, TS_DEVICE,
    TS_MANUFACTURER, TS_TEMPERATURE, UMC_CONFIG, UMC_CTRL, UMC_DIMM_CS,
};
use crate::pawnio::{BusLock, Handle, Module, PawnIoError, MUTEX_PCI, MUTEX_SMBUS};
use crate::provider::{BoxError, Provider};
use crate::registry::{Group, Registry, SensorDesc, SensorKey, Source, Unit};
use crate::snapshot::{DimmSnapshot, MemTimings, SensorSnapshot};

/// Longest wait for another tool's SMBus access before skipping this tick.
const LOCK_MS: u32 = 50;
/// Ticks the last temperatures stand in while another tool keeps the bus (then `null`).
const MAX_STALE: u32 = 3;
/// Polls that retry the timings or the SPD probe when a bus lock was busy at start-up.
const RETRIES: u32 = 10;

/// `i2c_smbus_xfer` protocols (Linux numbering, as the modules take them).
const SMBUS_BYTE_DATA: u64 = 2;
const SMBUS_WORD_DATA: u64 = 3;
const SMBUS_READ: u64 = 1;

/// Which SMBus controller the platform has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bus {
    /// AMD's FCH, with a port mux in front of the SPD.
    Piix4,
    /// Intel's PCH (2.1).
    I801,
}

impl Bus {
    pub const fn module(self) -> Module {
        match self {
            Self::Piix4 => Module::SmbusPiix4,
            Self::I801 => Module::SmbusI801,
        }
    }

    /// The SMBus of a CPU's platform, from its CPUID vendor.
    pub fn of(vendor: &str) -> Option<Self> {
        match vendor {
            "AuthenticAMD" => Some(Self::Piix4),
            "GenuineIntel" => Some(Self::I801),
            _ => None,
        }
    }
}

fn read(io: &Handle, addr: u8, cmd: u8, proto: u64) -> Result<u64, PawnIoError> {
    io.exec1(
        "ioctl_smbus_xfer",
        &[u64::from(addr), SMBUS_READ, u64::from(cmd), proto],
    )
}

fn byte(io: &Handle, addr: u8, cmd: u8) -> Option<u8> {
    read(io, addr, cmd, SMBUS_BYTE_DATA)
        .ok()
        .map(|v| v.to_le_bytes()[0])
}

/// A word as the bus delivers it: the first byte received is the low one.
fn word(io: &Handle, addr: u8, cmd: u8) -> Option<u16> {
    read(io, addr, cmd, SMBUS_WORD_DATA)
        .ok()
        .map(|v| u16::from_le_bytes([v.to_le_bytes()[0], v.to_le_bytes()[1]]))
}

/// A JC-42.4 register: most significant byte first on the bus.
fn ts_word(io: &Handle, addr: u8, reg: u8) -> Option<u16> {
    word(io, addr, reg).map(u16::swap_bytes)
}

/// The FCH's primary SMBus port mux; SPD sits on port 0. Reading it (argument -1) changes
/// nothing. Another tool that left the mux elsewhere makes Clokrix skip rather than switch it. Intel's
/// SMBus has no mux.
fn on_spd_port(io: &Handle, bus: Bus) -> bool {
    bus == Bus::I801
        || io
            .exec1("ioctl_piix4_port_sel", &[u64::MAX])
            .is_ok_and(|p| p == 0)
}

/// What an SPD address answered: hub registers only, never the NVM (no serial numbers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpdRaw {
    pub addr: u8,
    pub mr0: u8,
    pub mr1: u8,
    pub mr5: Option<u8>,
    pub mr26: Option<u8>,
    pub temp: Option<u16>,
}

/// What a DDR4 module sensor address answered (JC-42.4 registers, most significant byte first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TsodRaw {
    pub addr: u8,
    pub capability: u16,
    pub config: u16,
    pub maker: u16,
    pub device: u16,
    pub temp: Option<u16>,
}

impl TsodRaw {
    pub fn is_sensor(&self) -> bool {
        is_tse2004(self.capability, self.config, self.maker, self.device)
    }
}

/// Everything an SPD probe found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Probe {
    /// DDR5 hubs (and whatever else answered at 0x50–0x57).
    pub spd: Vec<SpdRaw>,
    /// DDR4 module sensors, looked for only when no DDR5 hub answered.
    pub tsod: Vec<TsodRaw>,
}

/// Probes 0x50–0x57 with byte reads, then, without a DDR5 hub, the DDR4 sensor addresses
/// 0x18–0x1F. Call with the SMBus mutex held. `None` when the SMBus mux is not on the SPD port.
pub fn probe_spd(io: &Handle, bus: Bus) -> Option<Probe> {
    if !on_spd_port(io, bus) {
        return None;
    }
    let spd: Vec<SpdRaw> = (SPD_FIRST..=SPD_LAST)
        .filter_map(|addr| {
            let mr0 = byte(io, addr, MR_DEVICE_TYPE_MSB)?;
            let mr1 = byte(io, addr, MR_DEVICE_TYPE_LSB)?;
            let hub = is_spd5_hub(mr0, mr1);
            Some(SpdRaw {
                addr,
                mr0,
                mr1,
                mr5: hub.then(|| byte(io, addr, MR_CAPABILITY)).flatten(),
                mr26: hub.then(|| byte(io, addr, MR_TS_CONFIG)).flatten(),
                temp: hub.then(|| word(io, addr, MR_TS_SENSE)).flatten(),
            })
        })
        .collect();
    let ddr5 = spd.iter().any(|s| is_spd5_hub(s.mr0, s.mr1));
    let tsod = if ddr5 {
        Vec::new()
    } else {
        (TSOD_FIRST..=TSOD_LAST)
            .filter_map(|addr| {
                let mut raw = TsodRaw {
                    addr,
                    capability: ts_word(io, addr, TS_CAPABILITY)?,
                    config: ts_word(io, addr, TS_CONFIG)?,
                    maker: ts_word(io, addr, TS_MANUFACTURER)?,
                    device: ts_word(io, addr, TS_DEVICE)?,
                    temp: None,
                };
                if raw.is_sensor() {
                    raw.temp = ts_word(io, addr, TS_TEMPERATURE);
                }
                Some(raw)
            })
            .collect()
    };
    Some(Probe { spd, tsod })
}

/// One UMC channel's timing registers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UmcRaw {
    pub channel: u32,
    pub regs: Vec<(u32, u32)>,
}

fn smn(io: &Handle, addr: u32) -> Option<u32> {
    io.exec1("ioctl_read_smn", &[u64::from(addr)])
        .ok()
        .and_then(|v| u32::try_from(v & 0xFFFF_FFFF).ok())
}

/// Registers of every populated DDR5 channel. Call with the PCI mutex held.
pub fn read_umc(io: &Handle) -> Vec<UmcRaw> {
    (0..MAX_CHANNELS)
        .filter_map(|ch| {
            let off = channel_offset(ch);
            let r = |a: u32| smn(io, off | a);
            let active = channel_active(r(UMC_CTRL)?, r(UMC_DIMM_CS[0])?, r(UMC_DIMM_CS[1])?);
            (active && dram_type(r(UMC_CONFIG)?) == DramType::Ddr5).then(|| UmcRaw {
                channel: ch,
                regs: DDR5_REGS.iter().map(|&a| (a, r(a).unwrap_or(0))).collect(),
            })
        })
        .collect()
}

/// The Intel memory controller's registers [`decode_intel`] reads (MCHBAR, memory-mapped: no
/// shared port, so no mutex).
pub fn read_imc(io: &Handle) -> Vec<(u32, u32)> {
    INTEL_REGS
        .iter()
        .filter_map(|&a| {
            let v = io.exec1("ioctl_read_dword", &[u64::from(a)]).ok()?;
            Some((a, u32::try_from(v & 0xFFFF_FFFF).ok()?))
        })
        .collect()
}

/// Which memory controller the timings come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Imc {
    Amd,
    Intel,
}

/// Timings of the first populated channel (channels run the same profile).
fn timings(io: &Handle, imc: Imc) -> Option<MemTimings> {
    match imc {
        Imc::Amd => read_umc(io)
            .iter()
            .find_map(|c| decode_ddr5(|a| c.regs.iter().find(|r| r.0 == a).map_or(0, |r| r.1))),
        Imc::Intel => {
            let regs = read_imc(io);
            decode_intel(|a| regs.iter().find(|r| r.0 == a).map(|r| r.1))
        }
    }
}

/// A module temperature sensor: a DDR5 hub's or a DDR4 module's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Hub,
    Tsod,
}

struct Sensor {
    addr: u8,
    kind: Kind,
    slot: String,
    key: SensorKey,
}

pub struct RamProvider {
    imc: Option<(Handle, Imc)>,
    smbus: Option<(Handle, Bus)>,
    pci: BusLock,
    bus: BusLock,
    sensors: Vec<Sensor>,
    /// SPD probe attempts; `None` once a probe completed.
    probes: Option<u32>,
    timings: Option<MemTimings>,
    tries: u32,
    last: Vec<DimmSnapshot>,
    stale: u32,
}

impl RamProvider {
    pub fn new() -> Self {
        Self {
            imc: None,
            smbus: None,
            pci: BusLock::new(MUTEX_PCI),
            bus: BusLock::new(MUTEX_SMBUS),
            sensors: Vec::new(),
            probes: Some(0),
            timings: None,
            tries: 0,
            last: Vec::new(),
            stale: 0,
        }
    }

    fn read_timings(&mut self, wait_ms: u32) {
        if let Some((io, imc)) = &self.imc {
            self.tries += 1;
            self.timings = match imc {
                // SMN goes through a PCI index/data pair other tools share.
                Imc::Amd => self.pci.hold(wait_ms, || timings(io, *imc)).flatten(),
                Imc::Intel => timings(io, *imc),
            };
        }
    }

    /// Finds DIMMs with a live thermal sensor and registers one sensor each. A probe the bus
    /// lock or port mux blocked is retried on later polls.
    fn probe(&mut self, reg: &mut Registry, wait_ms: u32) {
        let (Some((io, bus)), Some(n)) = (&self.smbus, self.probes) else {
            return;
        };
        let Some(found) = self.bus.hold(wait_ms, || probe_spd(io, *bus)).flatten() else {
            self.probes = (n + 1 < RETRIES).then_some(n + 1);
            if self.probes.is_none() {
                log::info!("ram: SPD not probed (SMBus busy or mux on another port)");
            }
            return;
        };
        self.probes = None;
        let hubs = found
            .spd
            .iter()
            .filter(|s| s.mr5.zip(s.mr26).is_some_and(|(c, t)| has_sensor(c, t)))
            .map(|s| (s.addr, Kind::Hub));
        let tsods = found
            .tsod
            .iter()
            .filter(|t| t.is_sensor())
            .map(|t| (t.addr, Kind::Tsod));
        for (addr, kind) in hubs.chain(tsods).collect::<Vec<_>>() {
            let Some(slot) = slot_name(addr) else {
                continue;
            };
            // Both kinds keep the SPD address's id, so a slot's history is one series.
            let spd = if kind == Kind::Tsod {
                addr - TSOD_FIRST + SPD_FIRST
            } else {
                addr
            };
            let key = reg.register(
                SensorDesc::new(
                    format!("memory.dimm{spd:02x}"),
                    Group::Memory,
                    format!("{slot} temperature"),
                    Unit::Celsius,
                    Source::Driver,
                )
                .with_history(),
            );
            self.sensors.push(Sensor {
                addr,
                kind,
                slot,
                key,
            });
        }
        log::info!(
            "ram: {} SPD device(s), {} DDR4 sensor address(es) answering, {} DIMM temperature sensor(s)",
            found.spd.len(),
            found.tsod.len(),
            self.sensors.len()
        );
    }
}

impl Default for RamProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for RamProvider {
    fn name(&self) -> &'static str {
        "ram"
    }

    fn init(&mut self, reg: &mut Registry) -> Result<(), BoxError> {
        let id = crate::hwid::cpuid::read_live();
        let imc = if crate::amd::supported(&id.vendor, id.family) {
            Some((Module::AmdFamily17, Imc::Amd))
        } else if intel_imc(id.family, id.model) && id.vendor == "GenuineIntel" {
            Some((Module::IntelMchbar, Imc::Intel))
        } else {
            None
        };
        if let Some((module, imc)) = imc {
            match Handle::open(module) {
                Ok(io) => self.imc = Some((io, imc)),
                Err(e) => log::info!("ram: no memory controller access ({e})"),
            }
        }
        self.read_timings(500);

        if let Some(bus) = Bus::of(&id.vendor) {
            match Handle::open(bus.module()) {
                Ok(io) => self.smbus = Some((io, bus)),
                Err(e) => log::info!("ram: no SMBus access ({e})"),
            }
        }
        self.probe(reg, 500);
        log::info!(
            "ram: timings {}",
            if self.timings.is_some() {
                "read"
            } else {
                "unavailable"
            }
        );
        if self.imc.is_none() && self.smbus.is_none() {
            return Err("no memory sensors reachable".into());
        }
        Ok(())
    }

    fn poll(&mut self, reg: &mut Registry, snap: &mut SensorSnapshot) -> Result<(), BoxError> {
        if self.timings.is_none() && self.tries < RETRIES {
            self.read_timings(LOCK_MS);
        }
        snap.memory.timings.clone_from(&self.timings);

        self.probe(reg, LOCK_MS);
        if let Some((io, bus)) = self.smbus.as_ref().filter(|_| !self.sensors.is_empty()) {
            let sensors = &self.sensors;
            let temps = self
                .bus
                .hold(LOCK_MS, || {
                    on_spd_port(io, *bus).then(|| {
                        sensors
                            .iter()
                            .map(|s| match s.kind {
                                Kind::Hub => word(io, s.addr, MR_TS_SENSE).and_then(spd5_temp),
                                Kind::Tsod => {
                                    ts_word(io, s.addr, TS_TEMPERATURE).and_then(tsod_temp)
                                }
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .flatten();
            if let Some(temps) = temps {
                self.last = sensors
                    .iter()
                    .zip(temps)
                    .map(|(s, t)| DimmSnapshot {
                        slot: s.slot.clone(),
                        temp_c: reg.set(s.key, t),
                        ..Default::default()
                    })
                    .collect();
                self.stale = 0;
            } else if self.stale < MAX_STALE {
                // Another tool holds the SMBus this tick: keep the last reading.
                self.stale += 1;
            } else {
                for d in &mut self.last {
                    d.temp_c = None;
                }
            }
        }
        snap.memory.dimms.clone_from(&self.last);
        Ok(())
    }
}
