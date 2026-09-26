//! `SioProvider`: reads the Super-I/O hardware monitor through PawnIO's `LpcIO` module. Without
//! PawnIO or a supported chip `init` fails and fans, voltages and board temperatures stay empty.

use std::time::Duration;

use super::{
    decode, identify, layout, plausible, read_ec, valid_base, voltage_channel, BoardTemp, Chip,
    Decoded, Layout, PortIo, BLOCK_LEN, BLOCK_START, CONFIG_PORTS, ENTER_KEY, EXIT_KEY, LDN_HWM,
    REG_BASE, REG_CHIP_ID, REG_CHIP_REV, REG_LDN,
};
use crate::hwid::smbios;
use crate::pawnio::{BusLock, Handle, Module, PawnIoError, MUTEX_ISABUS};
use crate::provider::{BoxError, Provider};
use crate::registry::{Group, Registry, SensorDesc, SensorKey, Source, Unit};
use crate::snapshot::{BoardSnapshot, FanSnapshot, SensorSnapshot, VoltageReading};

/// Longest wait for another tool's ISA-bus access before skipping this tick.
const LOCK_MS: u32 = 50;
/// Ticks the last readings stand in while another tool keeps the bus (then empty).
const MAX_STALE: u32 = 3;

/// A detected chip and its hardware-monitor I/O base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    pub chip: Chip,
    pub id: u8,
    pub rev: u8,
    pub config_port: u16,
    pub base: u16,
}

fn outb(io: &Handle, port: u16, value: u8) -> Result<(), PawnIoError> {
    io.exec("ioctl_pio_outb", &[u64::from(port), u64::from(value)], 0)
        .map(drop)
}

fn inb(io: &Handle, port: u16) -> Result<u8, PawnIoError> {
    io.exec1("ioctl_pio_inb", &[u64::from(port)])
        .map(|v| v.to_le_bytes()[0])
}

fn sio_in(io: &Handle, reg: u8) -> Result<u8, PawnIoError> {
    io.exec1("ioctl_superio_inb", &[u64::from(reg)])
        .map(|v| v.to_le_bytes()[0])
}

fn sio_out(io: &Handle, reg: u8, value: u8) -> Result<(), PawnIoError> {
    io.exec("ioctl_superio_outb", &[u64::from(reg), u64::from(value)], 0)
        .map(drop)
}

/// Looks for a supported chip at 0x2E and 0x4E. Call with the ISA-bus mutex held. On success
/// the handle's module keeps the chip's I/O ranges unlocked for [`read_block`].
pub fn detect(io: &Handle) -> Result<Option<Found>, PawnIoError> {
    for (slot, port) in (0u64..).zip(CONFIG_PORTS) {
        io.exec("ioctl_select_slot", &[slot], 0)?;
        outb(io, port, ENTER_KEY)?;
        outb(io, port, ENTER_KEY)?;
        let probe = (|| {
            let id = sio_in(io, REG_CHIP_ID)?;
            let rev = sio_in(io, REG_CHIP_REV)?;
            let Some(chip) = identify(id, rev) else {
                return Ok(None);
            };
            io.exec("ioctl_find_bars", &[], 0)?;
            sio_out(io, REG_LDN, LDN_HWM)?;
            let base = u16::from_be_bytes([sio_in(io, REG_BASE)?, sio_in(io, REG_BASE + 1)?]);
            Ok(valid_base(base).then_some(Found {
                chip,
                id,
                rev,
                config_port: port,
                base,
            }))
        })();
        // Always leave configuration mode, whatever the probe found.
        let exit = outb(io, port, EXIT_KEY);
        if let Some(found) = probe? {
            exit?;
            return Ok(Some(found));
        }
    }
    Ok(None)
}

struct Ports<'a>(&'a Handle);

impl PortIo for Ports<'_> {
    type Error = PawnIoError;
    fn inb(&mut self, port: u16) -> Result<u8, PawnIoError> {
        inb(self.0, port)
    }
    fn outb(&mut self, port: u16, value: u8) -> Result<(), PawnIoError> {
        outb(self.0, port, value)
    }
    fn pause(&mut self) {
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Reads NCT6687 EC space through PawnIO; see [`super::read_ec`]. Call with the ISA-bus
/// mutex held, after [`detect`] on the same handle.
pub fn read_block(
    io: &Handle,
    base: u16,
    start: u16,
    len: u16,
) -> Result<Option<Vec<u8>>, PawnIoError> {
    read_ec(&mut Ports(io), base, start, len)
}

#[derive(Default)]
struct Keys {
    temps: Vec<Option<SensorKey>>,
    volts: Vec<Option<SensorKey>>,
    fans: Vec<Option<SensorKey>>,
}

fn key(
    slot: &mut Vec<Option<SensorKey>>,
    n: u16,
    reg: &mut Registry,
    desc: impl FnOnce() -> SensorDesc,
) -> SensorKey {
    let i = usize::from(n);
    if slot.len() <= i {
        slot.resize(i + 1, None);
    }
    *slot[i].get_or_insert_with(|| reg.register(desc()))
}

#[derive(Default, Clone)]
struct Readings {
    board: BoardSnapshot,
    fans: Vec<FanSnapshot>,
    voltages: Vec<VoltageReading>,
}

pub struct SioProvider {
    io: Option<Handle>,
    isa: BusLock,
    found: Option<Found>,
    layout: Option<Layout>,
    keys: Keys,
    last: Readings,
    stale: u32,
}

impl SioProvider {
    pub fn new() -> Self {
        Self {
            io: None,
            isa: BusLock::new(MUTEX_ISABUS),
            found: None,
            layout: None,
            keys: Keys::default(),
            last: Readings::default(),
            stale: 0,
        }
    }

    fn named(&mut self, reg: &mut Registry, d: &Decoded) -> Readings {
        let layout = self.layout;
        let mut out = Readings::default();
        for &(n, t) in &d.temps {
            let named = layout.and_then(|l| l.temps.iter().find(|c| c.0 == n));
            // Unnamed channels that read nothing (or 0 °C, an unconnected input) are skipped.
            if named.is_none() && t.is_none_or(|t| t == 0.0) {
                continue;
            }
            let (name, target) = named.map_or_else(
                || (format!("TMPIN{n}"), BoardTemp::None),
                |c| (c.1.to_owned(), c.2),
            );
            let k = key(&mut self.keys.temps, n, reg, || {
                SensorDesc::new(
                    format!("board.temp{n}"),
                    Group::Board,
                    name,
                    Unit::Celsius,
                    Source::Driver,
                )
                .with_history()
            });
            let t = reg.set(k, t);
            let b = &mut out.board;
            match target {
                BoardTemp::None => {}
                BoardTemp::System => b.temp_system = t,
                BoardTemp::VrmMos => b.temp_vrm_mos = t,
                BoardTemp::Chipset => b.temp_chipset = t,
                BoardTemp::Socket => b.temp_socket = t,
                BoardTemp::Pcie => b.temp_pcie = t,
                BoardTemp::M2 => b.temp_m2 = t,
            }
        }
        for &(n, v) in &d.volts {
            let known = self
                .keys
                .volts
                .get(usize::from(n))
                .is_some_and(Option::is_some);
            if v <= 0.0 && !known {
                continue;
            }
            let name = layout
                .and_then(|l| l.volts.iter().find(|c| c.0 == n))
                .map_or_else(|| voltage_channel(n).0, |c| c.1.to_owned());
            let k = key(&mut self.keys.volts, n, reg, || {
                SensorDesc::new(
                    format!("voltage.vin{n}"),
                    Group::Voltages,
                    name.clone(),
                    Unit::Volt,
                    Source::Driver,
                )
            });
            let value = reg.set(k, Some(v));
            out.voltages.push(VoltageReading { name, value });
        }
        for &(n, r) in &d.fans {
            let named = layout.and_then(|l| l.fans.iter().find(|c| c.0 == n));
            let known = self
                .keys
                .fans
                .get(usize::from(n))
                .is_some_and(Option::is_some);
            if named.is_none() && !known && r.is_none_or(|r| r <= 0.0) {
                continue;
            }
            let (name, kind) = named.map_or_else(
                || (format!("Fan #{}", n + 1), crate::snapshot::FanKind::Fan),
                |c| (c.1.to_owned(), c.2),
            );
            let k = key(&mut self.keys.fans, n, reg, || {
                SensorDesc::new(
                    format!("fan.{n}"),
                    Group::Fans,
                    name.clone(),
                    Unit::Rpm,
                    Source::Driver,
                )
                .with_history()
            });
            let rpm = reg.set(k, r);
            out.fans.push(FanSnapshot {
                name,
                rpm,
                kind,
                ..Default::default()
            });
        }
        out
    }
}

impl Default for SioProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for SioProvider {
    fn name(&self) -> &'static str {
        "board-sio"
    }

    fn init(&mut self, _reg: &mut Registry) -> Result<(), BoxError> {
        let io = Handle::open(Module::LpcIo)?;
        let found = self
            .isa
            .hold(500, || detect(&io))
            .ok_or("ISA bus busy")??
            .ok_or("no supported Super-I/O chip")?;
        let board = crate::hwid::win::smbios_raw()
            .as_deref()
            .and_then(smbios::Smbios::from_raw)
            .map(|s| smbios::board(&s))
            .unwrap_or_default();
        self.layout = layout(
            board.manufacturer.as_deref(),
            board.product.as_deref(),
            found.chip,
        );
        log::info!(
            "sio: {} (id {:#04x} rev {:#04x}) at {:#x}, monitor base {:#06x}, board layout {}",
            found.chip.name(),
            found.id,
            found.rev,
            found.config_port,
            found.base,
            if self.layout.is_some() {
                "known"
            } else {
                "generic"
            },
        );
        self.found = Some(found);
        self.io = Some(io);
        Ok(())
    }

    fn poll(&mut self, reg: &mut Registry, snap: &mut SensorSnapshot) -> Result<(), BoxError> {
        let (Some(io), Some(found)) = (&self.io, self.found) else {
            return Err("poll before init".into());
        };
        let block = self
            .isa
            .hold(LOCK_MS, || {
                read_block(io, found.base, BLOCK_START, BLOCK_LEN)
            })
            .transpose()?
            .flatten()
            .filter(|b| plausible(b));
        if let Some(block) = block {
            self.last = self.named(reg, &decode(&block));
            self.stale = 0;
        } else if self.stale < MAX_STALE {
            // Another monitor holds the bus or the EC this tick: keep the last reading.
            self.stale += 1;
        } else {
            self.last = Readings::default();
        }
        let r = self.last.clone();
        snap.board = r.board;
        snap.fans = r.fans;
        snap.voltages = r.voltages;
        Ok(())
    }
}
