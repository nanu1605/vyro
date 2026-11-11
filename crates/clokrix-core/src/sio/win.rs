//! `SioProvider`: reads the Super-I/O hardware monitor through PawnIO's `LpcIO` module: a
//! Nuvoton NCT6687 or NCT679x, or an ITE chip (2.3). Without PawnIO or a supported chip `init`
//! fails and fans, voltages and board temperatures stay empty.

use std::time::Duration;

use super::nct679x::{self, CR_ENABLE, CR_GLOBAL_OPTION, HM_IO_LOCK, NUVOTON};
use super::{
    decode, identify, ite, layout, plausible, read_ec, valid_base, BoardTemp, Chip, Decoded,
    Family, Layout, PortIo, BLOCK_LEN, BLOCK_START, CONFIG_PORTS, ENTER_KEY, EXIT_KEY, LDN_HWM,
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
    /// Configuration registers 0x20 and 0x21: a Nuvoton chip's ID and revision, an ITE chip's two
    /// ID bytes.
    pub id: u8,
    pub rev: u8,
    pub config_port: u16,
    pub base: u16,
    /// The monitor's logical device is on (a NCT679x's enable bit; the NCT6687 is not checked).
    pub active: bool,
    /// The firmware locked the monitor's I/O ports (NCT679x only).
    pub io_locked: bool,
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

/// Looks for a supported chip at 0x2E and 0x4E: a Nuvoton one at either, then an ITE one at 0x2E
/// (the ITE chip at 0x4E of some Gigabyte boards is not read; see [`ite`]). Call with the ISA-bus
/// mutex held. On success the handle's module keeps the chip's I/O ranges unlocked for
/// [`read_block`] and the other readers.
pub fn detect(io: &Handle) -> Result<Option<Found>, PawnIoError> {
    for (slot, port) in (0u64..).zip(CONFIG_PORTS) {
        io.exec("ioctl_select_slot", &[slot], 0)?;
        if let Some(found) = probe_nuvoton(io, port)? {
            return Ok(Some(found));
        }
        if port == CONFIG_PORTS[0] {
            if let Some(found) = probe_ite(io, port)? {
                return Ok(Some(found));
            }
        }
    }
    Ok(None)
}

/// The Nuvoton key, ID and revision; on a chip Clokrix reads, its monitor's base.
fn probe_nuvoton(io: &Handle, port: u16) -> Result<Option<Found>, PawnIoError> {
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
        // A NCT679x may report its address port (base + 5); Linux `nct6775` masks the low
        // bits off, as PawnIO's `LpcIO` does when it lets the range through.
        let base = if chip.banked() { base & !7 } else { base };
        let (active, io_locked) = if chip.banked() {
            (
                sio_in(io, CR_ENABLE)? & 1 != 0,
                sio_in(io, CR_GLOBAL_OPTION)? & HM_IO_LOCK != 0,
            )
        } else {
            (true, false)
        };
        Ok(valid_base(base).then_some(Found {
            chip,
            id,
            rev,
            config_port: port,
            base,
            active,
            io_locked,
        }))
    })();
    // Always leave configuration mode, whatever the probe found.
    let exit = outb(io, port, EXIT_KEY);
    let found = probe?;
    if found.is_some() {
        exit?;
    }
    Ok(found)
}

/// The ITE key and ID; on a chip Clokrix reads, its environment controller's base.
fn probe_ite(io: &Handle, port: u16) -> Result<Option<Found>, PawnIoError> {
    for key in ite::ENTER_2E {
        outb(io, port, key)?;
    }
    let mut id = 0xFFFF;
    let probe = (|| {
        id = u16::from_be_bytes([sio_in(io, REG_CHIP_ID)?, sio_in(io, REG_CHIP_REV)?]);
        let Some(chip) = ite::identify(id) else {
            return Ok(None);
        };
        io.exec("ioctl_find_bars", &[], 0)?;
        sio_out(io, REG_LDN, ite::LDN_EC)?;
        let base = u16::from_be_bytes([sio_in(io, REG_BASE)?, sio_in(io, REG_BASE + 1)?]) & !7;
        let active = sio_in(io, ite::CR_ACTIVATE)? & 1 != 0;
        let [hi, lo] = id.to_be_bytes();
        Ok(valid_base(base).then_some(Found {
            chip,
            id: hi,
            rev: lo,
            config_port: port,
            base,
            active,
            io_locked: false,
        }))
    })();
    // Leave configuration mode: an ITE chip the ITE way; anything else that took the key (only
    // 0x55, an SMSC chip's key, can open another maker's) with the common exit key.
    let exit = if ite::is_ite(id) {
        sio_out(io, ite::CR_CONFIG_CONTROL, ite::EXIT)
    } else {
        outb(io, port, EXIT_KEY)
    };
    let found = probe?;
    if found.is_some() {
        exit?;
    }
    Ok(found)
}

/// Clears a NCT679x's monitor I/O lock (Super-I/O register 0x28, bit 4) and nothing else, as
/// Linux `nct6775` and LibreHardwareMonitor do on these chips: with it set the monitor's ports
/// answer nothing. Call with the ISA-bus mutex held, after [`detect`] on the same handle.
/// Selecting the slot clears the module's list of ports it lets through, so the chip's ranges are
/// found again before configuration mode ends, as [`detect`] finds them.
pub fn unlock_monitor(io: &Handle, found: &Found) -> Result<(), PawnIoError> {
    let slot = u64::from(found.config_port != CONFIG_PORTS[0]);
    io.exec("ioctl_select_slot", &[slot], 0)?;
    outb(io, found.config_port, ENTER_KEY)?;
    outb(io, found.config_port, ENTER_KEY)?;
    let write = sio_in(io, CR_GLOBAL_OPTION)
        .and_then(|v| sio_out(io, CR_GLOBAL_OPTION, v & !HM_IO_LOCK))
        .and_then(|()| io.exec("ioctl_find_bars", &[], 0).map(drop));
    let exit = outb(io, found.config_port, EXIT_KEY);
    write?;
    exit
}

/// Port I/O through PawnIO, for [`super::control`]. Use with the ISA-bus mutex held, after
/// [`detect`] on the same handle.
pub struct Ports<'a>(pub &'a Handle);

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
    fn settle(&mut self) {
        // The `nct6687d` driver waits 50 ms around each request.
        std::thread::sleep(Duration::from_millis(50));
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
    /// What an ITE chip's firmware enabled, read at start.
    ite: Option<ite::Setup>,
    layout: Option<Layout>,
    keys: Keys,
    last: Readings,
    stale: u32,
}

impl SioProvider {
    /// An ITE chip's monitor must be on and answer; then what its firmware enabled.
    fn ite_setup(&self, io: &Handle, found: &Found) -> Result<ite::Setup, BoxError> {
        let name = found.chip.name();
        if !found.active {
            return Err(format!("{name}: the firmware left its hardware monitor off").into());
        }
        let regs = [ite::VENDOR, ite::CONFIG]
            .into_iter()
            .chain(ite::SETUP_REGS)
            .collect::<Vec<_>>();
        let got = self
            .isa
            .hold(500, || {
                ite::read(
                    &mut Ports(io),
                    found.base,
                    ite::has_banks(found.chip),
                    &regs,
                )
            })
            .ok_or("ISA bus busy")??;
        let r = |a: u8| got.iter().find(|x| x.0 == a).map_or(0, |x| x.1);
        ite::answers(r(ite::VENDOR), r(ite::CONFIG)).map_err(|e| format!("{name}: {e}"))?;
        let s = ite::setup(
            found.chip,
            r(ite::FAN_MAIN_CTRL),
            r(ite::FAN_16BIT),
            r(ite::TEMP456),
        );
        log::info!(
            "sio: {name} fans enabled {:#08b}, temperatures {:#08b}",
            s.fans,
            s.temps
        );
        Ok(s)
    }

    pub fn new() -> Self {
        Self {
            io: None,
            isa: BusLock::new(MUTEX_ISABUS),
            found: None,
            ite: None,
            layout: None,
            keys: Keys::default(),
            last: Readings::default(),
            stale: 0,
        }
    }

    fn named(&mut self, reg: &mut Registry, d: &Decoded) -> Readings {
        let layout = self.layout;
        let Some(chip) = self.found.map(|f| f.chip) else {
            return Readings::default();
        };
        let mut out = Readings::default();
        for &(n, t) in &d.temps {
            let named = layout.and_then(|l| l.temps.iter().find(|c| c.0 == n));
            // Unnamed channels that read nothing (or 0 °C, an unconnected input) are skipped.
            if named.is_none() && t.is_none_or(|t| t == 0.0) {
                continue;
            }
            let (name, target) = named.map_or_else(
                || (chip.temp_name(n), BoardTemp::None),
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
                BoardTemp::Vrm => b.temp_vrm = t,
                BoardTemp::Pcie => b.temp_pcie = t,
                BoardTemp::M2 => b.temp_m2 = t,
            }
        }
        for &(n, v) in &d.volts {
            let named = layout.and_then(|l| l.volts.iter().find(|c| c.0 == n));
            let (name, v) = match named {
                Some(c) => (c.1.to_owned(), v * c.2),
                None if layout.is_some_and(|l| l.volts_only) => continue,
                None => {
                    let known = self
                        .keys
                        .volts
                        .get(usize::from(n))
                        .is_some_and(Option::is_some);
                    if v <= 0.0 && !known {
                        continue;
                    }
                    (chip.voltage_name(n), v)
                }
            };
            let k = key(&mut self.keys.volts, n, reg, || {
                SensorDesc::new(
                    format!("voltage.vin{n}"),
                    Group::Voltages,
                    name.clone(),
                    Unit::Volt,
                    Source::Driver,
                )
                .with_history()
            });
            // Exactly 0 V is an input the chip is not sampling (on MS-7E24 the standby rail
            // and the CMOS battery read 0 unless something turned their monitoring on), not a
            // dead rail: a PC that runs has standby power.
            let value = reg.set(k, (v > 0.0).then_some(v));
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
        if found.chip.family() == Family::Ite {
            self.ite = Some(self.ite_setup(&io, &found)?);
        }
        if found.chip.banked() {
            if !found.active {
                return Err(format!(
                    "{}: the firmware left its hardware monitor off",
                    found.chip.name()
                )
                .into());
            }
            let answers = || {
                self.isa
                    .hold(500, || nct679x::vendor(&mut Ports(&io), found.base))
                    .and_then(Result::ok)
                    == Some(NUVOTON)
            };
            if !answers() {
                if !found.io_locked {
                    return Err(
                        format!("{}: its monitor does not answer", found.chip.name()).into(),
                    );
                }
                log::info!(
                    "sio: {} monitor I/O locked by the firmware; unlocking",
                    found.chip.name()
                );
                self.isa
                    .hold(500, || unlock_monitor(&io, &found))
                    .ok_or("ISA bus busy")??;
                if !answers() {
                    return Err(format!(
                        "{}: its monitor does not answer after unlocking",
                        found.chip.name()
                    )
                    .into());
                }
            }
        }
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
            "sio: {} (id {:#04x} rev {:#04x}) at {:#x}, monitor base {:#06x}{}, board layout {}",
            found.chip.name(),
            found.id,
            found.rev,
            found.config_port,
            found.base,
            if found.io_locked {
                " (I/O lock bit set by the firmware)"
            } else {
                ""
            },
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
        let decoded = if let Some(t) = nct679x::tables(found.chip) {
            let regs = nct679x::registers(t);
            self.isa
                .hold(LOCK_MS, || nct679x::read(&mut Ports(io), found.base, &regs))
                .transpose()?
                .filter(|dump| plausible(&dump.iter().map(|r| r.1).collect::<Vec<_>>()))
                .map(|dump| {
                    nct679x::decode(t, |a| {
                        dump.binary_search_by_key(&a, |r| r.0)
                            .ok()
                            .map(|i| dump[i].1)
                    })
                })
        } else if let Some(s) = self.ite {
            let regs = ite::registers(s);
            let banks = ite::has_banks(found.chip);
            self.isa
                .hold(LOCK_MS, || {
                    ite::read(&mut Ports(io), found.base, banks, &regs)
                })
                .transpose()?
                .filter(|dump| plausible(&dump.iter().map(|r| r.1).collect::<Vec<_>>()))
                .map(|dump| {
                    ite::decode(s, |a| {
                        dump.binary_search_by_key(&a, |r| r.0)
                            .ok()
                            .map(|i| dump[i].1)
                    })
                })
        } else {
            self.isa
                .hold(LOCK_MS, || {
                    read_block(io, found.base, BLOCK_START, BLOCK_LEN)
                })
                .transpose()?
                .flatten()
                .filter(|b| plausible(b))
                .map(|b| decode(&b))
        };
        if let Some(d) = decoded {
            self.last = self.named(reg, &d);
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
