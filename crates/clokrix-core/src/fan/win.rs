//! The fan controller chip as a [`FanDevice`]: the NCT6687's EC transactions, a NCT679x's banked
//! registers (2.3) or an ITE chip's (2.4), PawnIO `LpcIO` port I/O, each call one transaction
//! under the ISA-bus mutex, only on an allow-listed board (`sio::control::capability`).

use super::controller::{ChipState, DevError, FanDevice};
use super::FanCapability;
use crate::hwid::smbios;
use crate::pawnio::{BusLock, Handle, Module, PawnIoError, MUTEX_ISABUS};
use crate::sio::control::{self, BiosState, Board, ControlError};
use crate::sio::{self, control679x, control_ite, ite, nct679x, Family, Found, Ports};

/// Longest wait for another tool's bus access before skipping this write (tried next tick).
const LOCK_MS: u32 = 200;

struct SioDevice {
    io: Handle,
    lock: BusLock,
    found: Found,
    board: &'static Board,
}

impl SioDevice {
    fn locked<R>(
        &self,
        f: impl FnOnce(&mut Ports<'_>) -> Result<R, ControlError<PawnIoError>>,
    ) -> Result<R, DevError> {
        self.lock
            .hold(LOCK_MS, || f(&mut Ports(&self.io)))
            .ok_or(DevError::Busy)?
            .map_err(DevError::from)
    }

    fn family(&self) -> Family {
        self.found.chip.family()
    }

    /// An ITE chip with register banks.
    fn banks(&self) -> bool {
        ite::has_banks(self.found.chip)
    }

    /// A saved state is this chip's: the same ID and revision.
    fn ours(&self, bios: &BiosState) -> Result<(), DevError> {
        if (bios.chip_id, bios.chip_rev) == (self.found.id, self.found.rev) {
            Ok(())
        } else {
            Err(DevError::Io(
                ControlError::<PawnIoError>::WrongState.to_string(),
            ))
        }
    }
}

impl FanDevice for SioDevice {
    fn board(&self) -> &'static Board {
        self.board
    }

    fn capture(&mut self, now: u64) -> Result<BiosState, DevError> {
        let (base, id, rev) = (self.found.base, self.found.id, self.found.rev);
        let (chip, banks) = (self.found.chip, self.banks());
        match self.family() {
            Family::Nct679x => {
                let n = control679x::outputs(chip);
                self.locked(|io| control679x::capture(io, base, (id, rev), n, now))
            }
            Family::Ite => {
                let n = control_ite::outputs(chip);
                self.locked(|io| control_ite::capture(io, base, banks, (id, rev), n, now))
            }
            Family::Nct6687 => self.locked(|io| control::capture(io, base, id, rev, now)),
        }
    }

    fn set(&mut self, header: u16, pct: f64) -> Result<u8, DevError> {
        let (base, board, banks) = (self.found.base, self.board, self.banks());
        match self.family() {
            Family::Nct679x => {
                self.locked(|io| control679x::set_duty(io, base, board, header, pct))
            }
            Family::Ite => {
                self.locked(|io| control_ite::set_duty(io, base, banks, board, header, pct))
            }
            Family::Nct6687 => self.locked(|io| control::set_duty(io, base, board, header, pct)),
        }
    }

    fn release(&mut self, header: u16, bios: &BiosState) -> Result<(), DevError> {
        self.ours(bios)?;
        let (base, banks) = (self.found.base, self.banks());
        match self.family() {
            Family::Nct679x => self.locked(|io| control679x::release(io, base, header, bios)),
            Family::Ite => self.locked(|io| control_ite::release(io, base, banks, header, bios)),
            Family::Nct6687 => self.locked(|io| control::release(io, base, header, bios)),
        }
    }

    fn restore(&mut self, bios: &BiosState) -> Result<(), DevError> {
        self.ours(bios)?;
        let (base, banks) = (self.found.base, self.banks());
        match self.family() {
            Family::Nct679x => self.locked(|io| control679x::restore(io, base, bios)),
            Family::Ite => self.locked(|io| control_ite::restore(io, base, banks, bios)),
            Family::Nct6687 => self.locked(|io| control::restore(io, base, bios)),
        }
    }

    fn read(&mut self) -> Result<ChipState, DevError> {
        let (base, chip, banks) = (self.found.base, self.found.chip, self.banks());
        match self.family() {
            Family::Nct679x => {
                let n = control679x::outputs(chip);
                return self.locked(|io| {
                    let (mode, outputs) = control679x::read_state(io, base, n)?;
                    Ok(ChipState { mode, outputs })
                });
            }
            Family::Ite => {
                let n = control_ite::outputs(chip);
                return self.locked(|io| {
                    let (mode, outputs) = control_ite::read_state(io, base, banks, n)?;
                    Ok(ChipState { mode, outputs })
                });
            }
            Family::Nct6687 => {}
        }
        self.locked(|io| {
            let read = |io: &mut Ports<'_>, start, len| {
                sio::read_ec(io, base, start, len)?.ok_or(ControlError::Busy)
            };
            let mode = read(io, control::CTRL_MODE, 1)?[0];
            let outputs = read(io, control::DUTY_OUTPUT, control::HEADERS)?;
            Ok(ChipState { mode, outputs })
        })
    }
}

/// A NCT679x whose firmware locked its monitor's ports answers nothing until the lock is
/// cleared, as the sensor reader clears it (`sio::win::unlock_monitor`).
fn unlock_if_needed(io: &Handle, lock: &BusLock, found: &Found) -> Result<(), String> {
    if found.chip.family() != Family::Nct679x {
        return Ok(());
    }
    let answers = || {
        lock.hold(500, || nct679x::vendor(&mut Ports(io), found.base))
            .and_then(Result::ok)
            == Some(nct679x::NUVOTON)
    };
    if answers() {
        return Ok(());
    }
    if found.io_locked {
        lock.hold(500, || sio::unlock_monitor(io, found))
            .ok_or("the ISA bus is busy")?
            .map_err(|e| e.to_string())?;
        if answers() {
            return Ok(());
        }
    }
    Err(format!("the {} does not answer", found.chip.name()))
}

/// The fan controller of this PC, if Clokrix may drive it.
pub fn open_device() -> Result<Box<dyn FanDevice>, FanCapability> {
    let unavailable = |reason: String| FanCapability::Unavailable { reason };
    let io = Handle::open(Module::LpcIo).map_err(|e| unavailable(e.to_string()))?;
    let lock = BusLock::new(MUTEX_ISABUS);
    let found = lock
        .hold(500, || sio::detect(&io))
        .ok_or_else(|| unavailable("the ISA bus is busy".into()))?
        .map_err(|e| unavailable(e.to_string()))?
        .ok_or_else(|| unavailable("no supported fan controller chip found".into()))?;
    let ids = crate::hwid::win::smbios_raw()
        .as_deref()
        .and_then(smbios::Smbios::from_raw)
        .map(|s| smbios::board(&s))
        .unwrap_or_default();
    let board = control::capability(
        found.chip,
        found.rev,
        ids.manufacturer.as_deref(),
        ids.product.as_deref(),
    )
    .map_err(|r| FanCapability::ReadOnly {
        reason: r.reason().into(),
    })?;
    unlock_if_needed(&io, &lock, &found).map_err(unavailable)?;
    log::info!(
        "fans: control available on {} ({} rev {:#04x})",
        board.name,
        found.chip.name(),
        found.rev
    );
    Ok(Box::new(SioDevice {
        io,
        lock,
        found,
        board,
    }))
}
