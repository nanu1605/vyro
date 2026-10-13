//! Fan and pump control on the NCT6687 (Phase 12a): the write path, what may be written, and
//! the BIOS state saved before the first write so any failure can hand cooling back.
//!
//! Registers and the request handshake follow the Linux `nct6687d` driver and
//! LibreHardwareMonitor's `Nct677X`: bit n of the mode register puts header n under software
//! control, the duty goes to its command register between a "request" and a "done" on the
//! request register, and the chip reports the duty it drives in the output register. Clearing
//! the mode bit returns the header to the BIOS curve.
//!
//! Safety rules kept here, not by callers: only allow-listed chip + revision + board + header
//! pairs are writable, duties are clamped to each header's floor, every write is read back,
//! and the EC's free/busy page handshake is never forced (another tool's transaction wins).

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{read_ec, Chip, PortIo, EC_DATA, EC_FREE, EC_INDEX, EC_PAGE};
use crate::snapshot::FanKind;

/// Bit n: header n under software control (clear = BIOS curve).
pub const CTRL_MODE: u16 = 0xA00;
/// Request register: [`REQ_BEGIN`] before a duty write, [`REQ_DONE`] after it.
pub const CTRL_REQUEST: u16 = 0xA01;
pub const REQ_BEGIN: u8 = 0x80;
pub const REQ_DONE: u8 = 0x40;
/// Duty command of header n (0–255) at `DUTY_COMMAND + n`.
pub const DUTY_COMMAND: u16 = 0xA28;
/// Duty the chip drives on header n now, at `DUTY_OUTPUT + n`.
pub const DUTY_OUTPUT: u16 = 0x160;
/// PWM headers of the NCT6687 (the mode register is one byte).
pub const HEADERS: u16 = 8;

/// A header Vyro may drive on an allow-listed board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub index: u16,
    pub name: &'static str,
    pub kind: FanKind,
    /// Lowest duty in percent Vyro will set (a pump never stops; fans keep spinning).
    pub min_pct: u8,
}

/// A board whose fan control has been checked on real hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Board {
    pub name: &'static str,
    /// SMBIOS baseboard product contains this.
    pub product: &'static str,
    pub chip: Chip,
    pub chip_rev: u8,
    pub headers: &'static [Header],
}

pub const FAN_MIN_PCT: u8 = 20;
pub const PUMP_MIN_PCT: u8 = 60;

const fn fan(index: u16, name: &'static str) -> Header {
    Header {
        index,
        name,
        kind: FanKind::Fan,
        min_pct: FAN_MIN_PCT,
    }
}

/// Decision 24: the launch set is the reference board only. Each further board gets an entry
/// here only after its own field check.
pub const ALLOW_LIST: &[Board] = &[Board {
    name: "MSI B650M GAMING PLUS WIFI",
    product: "MS-7E24",
    chip: Chip::Nct6687,
    chip_rev: 0x92,
    headers: &[
        fan(0, "CPU Fan"),
        Header {
            index: 1,
            name: "Pump Fan",
            kind: FanKind::Pump,
            min_pct: PUMP_MIN_PCT,
        },
        fan(2, "System Fan 1"),
        fan(3, "System Fan 2"),
        fan(4, "System Fan 3"),
    ],
}];

/// Why a detected chip stays read-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOnly {
    /// The chip's revision was not checked (NCT6687D and -R differ in fan control).
    UnvalidatedRevision,
    /// The board (or its maker) is not on the allow-list.
    UnvalidatedBoard,
}

impl ReadOnly {
    pub const fn reason(self) -> &'static str {
        match self {
            Self::UnvalidatedRevision => "this revision of the fan controller is not validated",
            Self::UnvalidatedBoard => "fan control is not validated on this motherboard",
        }
    }
}

/// Whether Vyro may write fan control on this chip and board.
pub fn capability(
    chip: Chip,
    rev: u8,
    manufacturer: Option<&str>,
    product: Option<&str>,
) -> Result<&'static Board, ReadOnly> {
    let msi = manufacturer.is_some_and(|m| m.contains("Micro-Star") || m.contains("MSI"));
    let product = product.unwrap_or("");
    let board = ALLOW_LIST
        .iter()
        .find(|b| b.chip == chip && msi && product.contains(b.product))
        .ok_or(ReadOnly::UnvalidatedBoard)?;
    if board.chip_rev != rev {
        return Err(ReadOnly::UnvalidatedRevision);
    }
    Ok(board)
}

/// Register value for `pct`, raised to the header's floor and capped at full speed.
pub fn duty(header: &Header, pct: f64) -> u8 {
    let pct = if pct.is_nan() { 100.0 } else { pct };
    let pct = pct.clamp(f64::from(header.min_pct), 100.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let raw = (pct * 255.0 / 100.0).round() as u8;
    raw
}

/// Percent from a duty register.
pub fn percent(raw: u8) -> f64 {
    f64::from(raw) * 100.0 / 255.0
}

#[derive(Debug, PartialEq, Eq)]
pub enum ControlError<E> {
    Io(E),
    /// Another program holds the EC; nothing was changed by this call after that point.
    Busy,
    /// A register did not keep what was written.
    Mismatch {
        addr: u16,
        wrote: u8,
        read: u8,
    },
    /// The header is not one of the board's controllable headers.
    NotAllowed(u16),
}

impl<E> From<E> for ControlError<E> {
    fn from(e: E) -> Self {
        Self::Io(e)
    }
}

impl<E: std::fmt::Display> std::fmt::Display for ControlError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "port I/O failed: {e}"),
            Self::Busy => f.write_str("another program is using the fan controller"),
            Self::Mismatch { addr, wrote, read } => {
                write!(
                    f,
                    "register {addr:#05x} read back {read:#04x} after writing {wrote:#04x}"
                )
            }
            Self::NotAllowed(n) => write!(f, "header {n} may not be controlled"),
        }
    }
}

type Res<T, E> = Result<T, ControlError<E>>;

/// Writes one byte of EC space: wait for the page register to be free, page, index, data,
/// free (the same transaction as [`read_ec`]). Call with the ISA-bus mutex held.
pub fn write_ec<P: PortIo>(io: &mut P, base: u16, addr: u16, value: u8) -> Res<(), P::Error> {
    let mut free = false;
    for _ in 0..5 {
        if io.inb(base + EC_PAGE)? == EC_FREE {
            free = true;
            break;
        }
        io.pause();
    }
    if !free {
        return Err(ControlError::Busy);
    }
    let [page, index] = addr.to_be_bytes();
    io.outb(base + EC_PAGE, page)?;
    let wrote = io
        .outb(base + EC_INDEX, index)
        .and_then(|()| io.outb(base + EC_DATA, value));
    let release = io.outb(base + EC_PAGE, EC_FREE);
    wrote?;
    release?;
    Ok(())
}

fn read1<P: PortIo>(io: &mut P, base: u16, addr: u16) -> Res<u8, P::Error> {
    read_ec(io, base, addr, 1)?
        .and_then(|b| b.first().copied())
        .ok_or(ControlError::Busy)
}

fn write_checked<P: PortIo>(io: &mut P, base: u16, addr: u16, value: u8) -> Res<(), P::Error> {
    write_ec(io, base, addr, value)?;
    let read = read1(io, base, addr)?;
    if read == value {
        Ok(())
    } else {
        Err(ControlError::Mismatch {
            addr,
            wrote: value,
            read,
        })
    }
}

/// Control registers of every header, as the BIOS left them. Saved (see [`save`]) before
/// Vyro's first write and written back by [`restore`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BiosState {
    pub chip_id: u8,
    pub chip_rev: u8,
    pub base: u16,
    pub mode: u8,
    /// Duty command of headers 0–7.
    pub commands: Vec<u8>,
    /// Unix seconds when captured.
    pub saved_at: u64,
}

/// Reads the BIOS control state. Call before Vyro writes anything.
pub fn capture<P: PortIo>(
    io: &mut P,
    base: u16,
    chip_id: u8,
    chip_rev: u8,
    now: u64,
) -> Res<BiosState, P::Error> {
    let mode = read1(io, base, CTRL_MODE)?;
    let commands = read_ec(io, base, DUTY_COMMAND, HEADERS)?.ok_or(ControlError::Busy)?;
    Ok(BiosState {
        chip_id,
        chip_rev,
        base,
        mode,
        commands,
        saved_at: now,
    })
}

/// Puts `header` under software control at `pct` (clamped to its floor) and checks the mode
/// and command registers kept what was written. Returns the duty written.
pub fn set_duty<P: PortIo>(
    io: &mut P,
    base: u16,
    board: &Board,
    header: u16,
    pct: f64,
) -> Res<u8, P::Error> {
    let h = board
        .headers
        .iter()
        .find(|h| h.index == header)
        .ok_or(ControlError::NotAllowed(header))?;
    let raw = duty(h, pct);
    let bit = 1u8 << h.index;
    let mode = read1(io, base, CTRL_MODE)?;
    if mode & bit == 0 {
        write_checked(io, base, CTRL_MODE, mode | bit)?;
    }
    write_ec(io, base, CTRL_REQUEST, REQ_BEGIN)?;
    io.settle();
    let written = write_checked(io, base, DUTY_COMMAND + h.index, raw);
    // Always close the request, also after a failed write, so the EC does not wait for it.
    let done = write_ec(io, base, CTRL_REQUEST, REQ_DONE);
    io.settle();
    written?;
    done?;
    Ok(raw)
}

/// Writes the BIOS state back: duty commands first, then the mode register, which hands every
/// header Vyro took back to the BIOS curve. Headers another tool put under software control
/// before Vyro started stay as the BIOS state recorded them.
pub fn restore<P: PortIo>(io: &mut P, base: u16, state: &BiosState) -> Res<(), P::Error> {
    write_ec(io, base, CTRL_REQUEST, REQ_BEGIN)?;
    io.settle();
    let mut first_err = None;
    for (n, &raw) in (0u16..).zip(&state.commands) {
        if let Err(e) = write_checked(io, base, DUTY_COMMAND + n, raw) {
            first_err.get_or_insert(e);
        }
    }
    let done = write_ec(io, base, CTRL_REQUEST, REQ_DONE);
    io.settle();
    // The mode register is what returns control to the BIOS: write it even if a duty failed.
    let mode = write_checked(io, base, CTRL_MODE, state.mode);
    match first_err {
        Some(e) => Err(e),
        None => done.and(mode),
    }
}

/// Hands one header back to the BIOS: its duty command as recorded, then its mode bit as
/// recorded (the other headers' bits are left as they are now).
pub fn release<P: PortIo>(
    io: &mut P,
    base: u16,
    header: u16,
    state: &BiosState,
) -> Res<(), P::Error> {
    if header >= HEADERS {
        return Err(ControlError::NotAllowed(header));
    }
    let bit = 1u8 << header;
    let raw = state
        .commands
        .get(usize::from(header))
        .copied()
        .unwrap_or(0);
    write_ec(io, base, CTRL_REQUEST, REQ_BEGIN)?;
    io.settle();
    let duty = write_checked(io, base, DUTY_COMMAND + header, raw);
    let done = write_ec(io, base, CTRL_REQUEST, REQ_DONE);
    io.settle();
    let mode = read1(io, base, CTRL_MODE)
        .and_then(|m| write_checked(io, base, CTRL_MODE, (m & !bit) | (state.mode & bit)));
    duty.and(done).and(mode)
}

/// Mode, command and output registers of each header, for the read-only dump and the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderState {
    pub index: u16,
    pub software: bool,
    pub command: u8,
    pub output: u8,
}

pub fn headers(mode: u8, commands: &[u8], outputs: &[u8]) -> Vec<HeaderState> {
    (0..HEADERS)
        .map(|n| {
            let i = usize::from(n);
            HeaderState {
                index: n,
                software: mode & (1 << n) != 0,
                command: commands.get(i).copied().unwrap_or(0),
                output: outputs.get(i).copied().unwrap_or(0),
            }
        })
        .collect()
}

/// Saves the BIOS state before the first write (temporary file, then rename, so a crash never
/// leaves half a file). The service restores from it at start if it exists.
pub fn save(path: &Path, state: &BiosState) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(state).map_err(io::Error::other)?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(tmp, path)
}

/// The saved BIOS state, `None` when there is none (nothing to restore).
pub fn load(path: &Path) -> io::Result<Option<BiosState>> {
    match std::fs::read(path) {
        Ok(b) => serde_json::from_slice(&b)
            .map(Some)
            .map_err(io::Error::other),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: u16 = 0xA20;
    const MSI: Option<&str> = Some("Micro-Star International Co., Ltd.");
    const B650: Option<&str> = Some("B650M GAMING PLUS WIFI (MS-7E24)");

    /// NCT6687 EC with writable memory: a request starts when the page register goes from
    /// free to a page, and the data register then reads or writes that address once.
    struct Ec {
        mem: Vec<u8>,
        page_reg: u8,
        addr: Option<u16>,
        armed: bool,
        /// Addresses that ignore writes (a register the chip does not keep).
        stuck: Vec<u16>,
        /// Fail every port access after this many (a crash mid-sequence).
        fail_after: Option<usize>,
        ops: usize,
        writes: Vec<(u16, u8)>,
    }

    impl Ec {
        fn new() -> Self {
            let mut mem = vec![0u8; 0x10000];
            mem[usize::from(CTRL_MODE)] = 0x00;
            for n in 0..HEADERS {
                mem[usize::from(DUTY_COMMAND + n)] = 0x40 + n as u8;
                mem[usize::from(DUTY_OUTPUT + n)] = 0x40 + n as u8;
            }
            Self {
                mem,
                page_reg: EC_FREE,
                addr: None,
                armed: false,
                stuck: Vec::new(),
                fail_after: None,
                ops: 0,
                writes: Vec::new(),
            }
        }

        fn at(&self, addr: u16) -> u8 {
            self.mem[usize::from(addr)]
        }

        fn tick(&mut self) -> Result<(), ()> {
            self.ops += 1;
            match self.fail_after {
                Some(n) if self.ops > n => Err(()),
                _ => Ok(()),
            }
        }
    }

    impl PortIo for Ec {
        type Error = ();
        fn inb(&mut self, port: u16) -> Result<u8, ()> {
            self.tick()?;
            match port - BASE {
                EC_PAGE => Ok(self.page_reg),
                EC_DATA => Ok(self.addr.map_or(0xFF, |a| self.at(a))),
                _ => Err(()),
            }
        }
        fn outb(&mut self, port: u16, value: u8) -> Result<(), ()> {
            self.tick()?;
            match port - BASE {
                EC_PAGE => {
                    self.armed = self.page_reg == EC_FREE && value != EC_FREE;
                    self.page_reg = value;
                    if value == EC_FREE {
                        self.addr = None;
                    }
                }
                EC_INDEX if self.armed => {
                    self.addr = Some(u16::from_be_bytes([self.page_reg, value]));
                    self.armed = false;
                }
                EC_INDEX => {}
                EC_DATA => {
                    if let Some(a) = self.addr {
                        self.writes.push((a, value));
                        if !self.stuck.contains(&a) {
                            self.mem[usize::from(a)] = value;
                        }
                    }
                }
                _ => return Err(()),
            }
            Ok(())
        }
    }

    fn board() -> &'static Board {
        capability(Chip::Nct6687, 0x92, MSI, B650).expect("allow-listed")
    }

    #[test]
    fn only_the_reference_board_and_revision_are_writable() {
        assert_eq!(board().product, "MS-7E24");
        assert_eq!(
            capability(Chip::Nct6687, 0x93, MSI, B650),
            Err(ReadOnly::UnvalidatedRevision)
        );
        let tomahawk = Some("MAG X670E TOMAHAWK WIFI (MS-7E12)");
        assert_eq!(
            capability(Chip::Nct6687, 0x92, MSI, tomahawk),
            Err(ReadOnly::UnvalidatedBoard)
        );
        assert_eq!(
            capability(Chip::Nct6687, 0x92, Some("ASRock"), B650),
            Err(ReadOnly::UnvalidatedBoard)
        );
        assert_eq!(
            capability(Chip::Nct6687, 0x92, None, None),
            Err(ReadOnly::UnvalidatedBoard)
        );
    }

    #[test]
    fn duty_is_bounded_by_the_header_floor() {
        let b = board();
        let (cpu, pump) = (&b.headers[0], &b.headers[1]);
        assert_eq!(pump.kind, FanKind::Pump);
        assert_eq!(duty(cpu, 100.0), 255);
        assert_eq!(duty(cpu, 150.0), 255);
        assert_eq!(duty(cpu, 50.0), 128);
        assert_eq!(duty(cpu, 0.0), duty(cpu, 20.0));
        assert_eq!(duty(cpu, -5.0), 51);
        assert_eq!(duty(pump, 0.0), 153);
        assert_eq!(duty(pump, 30.0), 153);
        // An unknown request (a failed sensor upstream) means full speed, never a stop.
        assert_eq!(duty(pump, f64::NAN), 255);
        assert!((percent(255) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn write_ec_is_one_released_transaction() {
        let mut ec = Ec::new();
        write_ec(&mut ec, BASE, 0xA2A, 0x99).unwrap();
        assert_eq!(ec.at(0xA2A), 0x99);
        assert_eq!(ec.page_reg, EC_FREE);
        assert_eq!(ec.writes, vec![(0xA2A, 0x99)]);
    }

    #[test]
    fn a_busy_ec_is_never_forced() {
        let mut ec = Ec::new();
        ec.page_reg = 0x01; // another tool mid-transaction
        assert_eq!(
            write_ec(&mut ec, BASE, 0xA2A, 0x99),
            Err(ControlError::Busy)
        );
        assert_eq!(ec.page_reg, 0x01);
        assert!(ec.writes.is_empty());
        let before = ec.at(CTRL_MODE);
        assert_eq!(
            set_duty(&mut ec, BASE, board(), 0, 50.0),
            Err(ControlError::Busy)
        );
        assert_eq!(ec.at(CTRL_MODE), before);
    }

    #[test]
    fn set_duty_takes_one_header_and_follows_the_handshake() {
        let mut ec = Ec::new();
        ec.mem[usize::from(CTRL_MODE)] = 0b0100_0000; // another tool owns header 6
        let raw = set_duty(&mut ec, BASE, board(), 2, 50.0).unwrap();
        assert_eq!(raw, 128);
        assert_eq!(ec.at(CTRL_MODE), 0b0100_0100);
        assert_eq!(ec.at(DUTY_COMMAND + 2), 128);
        assert_eq!(
            ec.writes,
            vec![
                (CTRL_MODE, 0b0100_0100),
                (CTRL_REQUEST, REQ_BEGIN),
                (DUTY_COMMAND + 2, 128),
                (CTRL_REQUEST, REQ_DONE),
            ]
        );
        // Already under software control: no second mode write.
        ec.writes.clear();
        set_duty(&mut ec, BASE, board(), 2, 100.0).unwrap();
        assert_eq!(ec.writes.first(), Some(&(CTRL_REQUEST, REQ_BEGIN)));
    }

    #[test]
    fn pump_and_unlisted_headers_are_protected() {
        let mut ec = Ec::new();
        assert_eq!(set_duty(&mut ec, BASE, board(), 1, 0.0), Ok(153));
        assert_eq!(ec.at(DUTY_COMMAND + 1), 153);
        assert_eq!(
            set_duty(&mut ec, BASE, board(), 5, 50.0),
            Err(ControlError::NotAllowed(5))
        );
        assert_eq!(
            set_duty(&mut ec, BASE, board(), 9, 50.0),
            Err(ControlError::NotAllowed(9))
        );
    }

    #[test]
    fn a_register_that_does_not_keep_the_write_is_reported_and_the_request_closed() {
        let mut ec = Ec::new();
        ec.stuck.push(DUTY_COMMAND);
        let err = set_duty(&mut ec, BASE, board(), 0, 100.0).unwrap_err();
        assert_eq!(
            err,
            ControlError::Mismatch {
                addr: DUTY_COMMAND,
                wrote: 255,
                read: 0x40
            }
        );
        assert_eq!(ec.writes.last(), Some(&(CTRL_REQUEST, REQ_DONE)));
        assert_eq!(ec.page_reg, EC_FREE);
    }

    #[test]
    fn capture_then_restore_returns_every_register() {
        let mut ec = Ec::new();
        let state = capture(&mut ec, BASE, 0xD5, 0x92, 1_700_000_000).unwrap();
        assert_eq!(state.mode, 0);
        assert_eq!(
            state.commands,
            (0..8).map(|n| 0x40 + n).collect::<Vec<u8>>()
        );
        for n in [0, 1, 2, 4] {
            set_duty(&mut ec, BASE, board(), n, 100.0).unwrap();
        }
        assert_eq!(ec.at(CTRL_MODE), 0b0001_0111);
        restore(&mut ec, BASE, &state).unwrap();
        assert_eq!(ec.at(CTRL_MODE), 0);
        for n in 0..HEADERS {
            assert_eq!(ec.at(DUTY_COMMAND + n), 0x40 + n as u8);
        }
        // The mode register (hand-back to the BIOS) is the last write.
        assert_eq!(ec.writes.last(), Some(&(CTRL_MODE, 0)));
    }

    #[test]
    fn release_hands_back_one_header_only() {
        let mut ec = Ec::new();
        let state = capture(&mut ec, BASE, 0xD5, 0x92, 0).unwrap();
        set_duty(&mut ec, BASE, board(), 0, 100.0).unwrap();
        set_duty(&mut ec, BASE, board(), 2, 100.0).unwrap();
        release(&mut ec, BASE, 0, &state).unwrap();
        assert_eq!(ec.at(CTRL_MODE), 0b0000_0100);
        assert_eq!(ec.at(DUTY_COMMAND), 0x40);
        assert_eq!(ec.at(DUTY_COMMAND + 2), 255);
        assert_eq!(
            release(&mut ec, BASE, 8, &state),
            Err(ControlError::NotAllowed(8))
        );
    }

    #[test]
    fn restore_still_hands_back_control_when_a_duty_write_fails() {
        let mut ec = Ec::new();
        let state = capture(&mut ec, BASE, 0xD5, 0x92, 0).unwrap();
        set_duty(&mut ec, BASE, board(), 0, 100.0).unwrap();
        ec.stuck.push(DUTY_COMMAND);
        assert!(matches!(
            restore(&mut ec, BASE, &state),
            Err(ControlError::Mismatch {
                addr: DUTY_COMMAND,
                ..
            })
        ));
        assert_eq!(ec.at(CTRL_MODE), 0);
    }

    #[test]
    fn port_failure_mid_sequence_is_an_error_not_a_panic() {
        for n in 0..40 {
            let mut ec = Ec::new();
            ec.fail_after = Some(n);
            let r = set_duty(&mut ec, BASE, board(), 0, 60.0);
            if n < 30 {
                assert!(r.is_err(), "failing after {n} port accesses");
            }
        }
    }

    #[test]
    fn header_states_decode_the_mode_bits() {
        let s = headers(0b0000_0101, &[10, 20, 30], &[11, 21, 31]);
        assert_eq!(s.len(), 8);
        assert!(s[0].software && !s[1].software && s[2].software);
        assert_eq!((s[1].command, s[1].output), (20, 21));
        assert_eq!((s[7].command, s[7].output), (0, 0));
    }

    #[test]
    fn bios_state_file_round_trips() {
        let dir = std::env::temp_dir().join(format!("vyro-fan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fan-restore.json");
        assert_eq!(load(&path).unwrap(), None);
        let state = BiosState {
            chip_id: 0xD5,
            chip_rev: 0x92,
            base: 0xA20,
            mode: 0x02,
            commands: vec![1, 2, 3, 4, 5, 6, 7, 8],
            saved_at: 42,
        };
        save(&path, &state).unwrap();
        assert_eq!(load(&path).unwrap(), Some(state));
        assert!(!path.with_extension("tmp").exists());
        std::fs::write(&path, b"{").unwrap();
        assert!(load(&path).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
