//! `RamProvider`: DDR5 module temperatures over the AMD FCH SMBus (`SmbusPIIX4`) and memory
//! timings from the UMC over SMN (`AMDFamily17`). Each half works without the other; with
//! neither `init` fails and the memory fields it owns stay empty.

use super::{
    channel_active, channel_offset, decode_ddr5, dram_type, has_sensor, is_spd5_hub, slot_name,
    spd5_temp, DramType, DDR5_REGS, MAX_CHANNELS, MR_CAPABILITY, MR_DEVICE_TYPE_LSB,
    MR_DEVICE_TYPE_MSB, MR_TS_CONFIG, MR_TS_SENSE, SPD_FIRST, SPD_LAST, UMC_CONFIG, UMC_CTRL,
    UMC_DIMM_CS,
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

/// `i2c_smbus_xfer` protocols (Linux numbering, as the module takes them).
const SMBUS_BYTE_DATA: u64 = 2;
const SMBUS_WORD_DATA: u64 = 3;
const SMBUS_READ: u64 = 1;

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

fn word(io: &Handle, addr: u8, cmd: u8) -> Option<u16> {
    read(io, addr, cmd, SMBUS_WORD_DATA)
        .ok()
        .map(|v| u16::from_le_bytes([v.to_le_bytes()[0], v.to_le_bytes()[1]]))
}

/// The FCH's primary SMBus port mux; SPD sits on port 0. Reading it (argument -1) changes
/// nothing. Another tool that left the mux elsewhere makes Vyro skip rather than switch it.
fn on_spd_port(io: &Handle) -> bool {
    io.exec1("ioctl_piix4_port_sel", &[u64::MAX])
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

/// Probes 0x50–0x57 with byte reads. Call with the SMBus mutex held. `None` when the SMBus
/// mux is not on the SPD port.
pub fn probe_spd(io: &Handle) -> Option<Vec<SpdRaw>> {
    if !on_spd_port(io) {
        return None;
    }
    let found = (SPD_FIRST..=SPD_LAST)
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
    Some(found)
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

/// Timings of the first populated channel (channels run the same profile).
fn timings(io: &Handle) -> Option<MemTimings> {
    read_umc(io)
        .iter()
        .find_map(|c| decode_ddr5(|a| c.regs.iter().find(|r| r.0 == a).map_or(0, |r| r.1)))
}

struct Hub {
    addr: u8,
    slot: String,
    key: SensorKey,
}

pub struct RamProvider {
    smn: Option<Handle>,
    smbus: Option<Handle>,
    pci: BusLock,
    bus: BusLock,
    hubs: Vec<Hub>,
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
            smn: None,
            smbus: None,
            pci: BusLock::new(MUTEX_PCI),
            bus: BusLock::new(MUTEX_SMBUS),
            hubs: Vec::new(),
            probes: Some(0),
            timings: None,
            tries: 0,
            last: Vec::new(),
            stale: 0,
        }
    }

    fn read_timings(&mut self, wait_ms: u32) {
        if let Some(io) = &self.smn {
            self.tries += 1;
            self.timings = self.pci.hold(wait_ms, || timings(io)).flatten();
        }
    }

    /// Finds DIMMs with a live thermal sensor and registers one sensor each. A probe the bus
    /// lock or port mux blocked is retried on later polls.
    fn probe(&mut self, reg: &mut Registry, wait_ms: u32) {
        let (Some(io), Some(n)) = (&self.smbus, self.probes) else {
            return;
        };
        let Some(spd) = self.bus.hold(wait_ms, || probe_spd(io)).flatten() else {
            self.probes = (n + 1 < RETRIES).then_some(n + 1);
            if self.probes.is_none() {
                log::info!("ram: SPD not probed (SMBus busy or mux on another port)");
            }
            return;
        };
        self.probes = None;
        for s in &spd {
            let live = s.mr5.zip(s.mr26).is_some_and(|(c, t)| has_sensor(c, t));
            let Some(slot) = slot_name(s.addr).filter(|_| live) else {
                continue;
            };
            let key = reg.register(
                SensorDesc::new(
                    format!("memory.dimm{:02x}", s.addr),
                    Group::Memory,
                    format!("{slot} temperature"),
                    Unit::Celsius,
                    Source::Driver,
                )
                .with_history(),
            );
            self.hubs.push(Hub {
                addr: s.addr,
                slot,
                key,
            });
        }
        log::info!(
            "ram: {} SPD device(s), {} DIMM temperature sensor(s)",
            spd.len(),
            self.hubs.len()
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
        if crate::amd::supported(&id.vendor, id.family) {
            match Handle::open(Module::AmdFamily17) {
                Ok(io) => self.smn = Some(io),
                Err(e) => log::info!("ram: no SMN access ({e})"),
            }
        }
        self.read_timings(500);

        match Handle::open(Module::SmbusPiix4) {
            Ok(io) => self.smbus = Some(io),
            Err(e) => log::info!("ram: no SMBus access ({e})"),
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
        if self.smn.is_none() && self.smbus.is_none() {
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
        if let Some(io) = self.smbus.as_ref().filter(|_| !self.hubs.is_empty()) {
            let hubs = &self.hubs;
            let words = self
                .bus
                .hold(LOCK_MS, || {
                    on_spd_port(io).then(|| {
                        hubs.iter()
                            .map(|h| word(io, h.addr, MR_TS_SENSE))
                            .collect::<Vec<_>>()
                    })
                })
                .flatten();
            if let Some(words) = words {
                self.last = hubs
                    .iter()
                    .zip(words)
                    .map(|(h, w)| DimmSnapshot {
                        slot: h.slot.clone(),
                        temp_c: reg.set(h.key, w.and_then(spd5_temp)),
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
