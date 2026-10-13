//! The NCT6687 as a [`FanDevice`]: PawnIO `LpcIO` port I/O, each call one transaction under
//! the ISA-bus mutex, only on an allow-listed board (`sio::control::capability`).

use super::controller::{ChipState, DevError, FanDevice};
use super::FanCapability;
use crate::hwid::smbios;
use crate::pawnio::{BusLock, Handle, Module, PawnIoError, MUTEX_ISABUS};
use crate::sio::control::{self, BiosState, Board, ControlError};
use crate::sio::{self, Found, Ports};

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
}

impl FanDevice for SioDevice {
    fn board(&self) -> &'static Board {
        self.board
    }

    fn capture(&mut self, now: u64) -> Result<BiosState, DevError> {
        let (base, id, rev) = (self.found.base, self.found.id, self.found.rev);
        self.locked(|io| control::capture(io, base, id, rev, now))
    }

    fn set(&mut self, header: u16, pct: f64) -> Result<u8, DevError> {
        let (base, board) = (self.found.base, self.board);
        self.locked(|io| control::set_duty(io, base, board, header, pct))
    }

    fn release(&mut self, header: u16, bios: &BiosState) -> Result<(), DevError> {
        let base = self.found.base;
        self.locked(|io| control::release(io, base, header, bios))
    }

    fn restore(&mut self, bios: &BiosState) -> Result<(), DevError> {
        let base = self.found.base;
        self.locked(|io| control::restore(io, base, bios))
    }

    fn read(&mut self) -> Result<ChipState, DevError> {
        let base = self.found.base;
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

/// The fan controller of this PC, if Vyro may drive it.
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
