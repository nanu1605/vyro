//! Fan control on the Nuvoton NCT6791D to NCT6799D (2.3): for the banked chips of ASUS boards, the
//! write path [`super::control`] has for the NCT6687. The registers are Linux `nct6775`'s. Each
//! PWM output n has a mode register whose bits 7:4 choose who drives it (0 is manual; 1 thermal
//! cruise, 2 speed cruise, 4 Smart Fan IV, which ASUS's Q-Fan curves are) and whose bits 3:0 hold
//! a tolerance, a duty register the chip drives in manual mode, and an output register with the
//! duty driven now.
//!
//! Taking an output: its mode bits cleared (the tolerance kept, as Linux keeps it), then the
//! duty. Giving it back: the recorded duty, then the recorded mode, which is what hands it back to
//! the firmware's curve. Every write is read back. Which outputs may be written, and how low, is
//! the board's, as on the NCT6687, and no board's fans are driven before its own field check
//! (`docs/control-safety.md`). Pure: the port I/O is behind [`PortIo`], tested on a fake chip.

use super::control::{duty, BiosState, Board, ControlError};
use super::nct679x::{read_reg, write_reg};
use super::{Chip, PortIo};

/// Mode, duty and output registers of PWM outputs 1 to 7 (bank << 8 | register).
pub const MODE: [u16; 7] = [0x102, 0x202, 0x302, 0x802, 0x902, 0xA02, 0xB02];
pub const DUTY: [u16; 7] = [0x109, 0x209, 0x309, 0x809, 0x909, 0xA09, 0xB09];
/// Outputs 6 and 7 report the duty they drive in their duty register.
pub const OUTPUT: [u16; 7] = [0x001, 0x003, 0x011, 0x013, 0x015, 0xA09, 0xB09];
/// Bits 7:4 of a mode register: 0 is manual (software) control.
const MODE_BITS: u8 = 0xF0;

/// PWM outputs of a chip (Linux `nct6775`'s count); 0 for chips without these registers.
pub const fn outputs(chip: Chip) -> u16 {
    match chip {
        Chip::Nct6791 | Chip::Nct6792 | Chip::Nct6793 | Chip::Nct6795 => 6,
        Chip::Nct6796 | Chip::Nct6797 | Chip::Nct6798 | Chip::Nct6799 | Chip::Nct6701 => 7,
        _ => 0,
    }
}

/// A mode register value that means software (manual) control.
pub const fn manual(mode: u8) -> bool {
    mode & MODE_BITS == 0
}

type Res<T, E> = Result<T, ControlError<E>>;

fn read<P: PortIo>(io: &mut P, base: u16, regs: &[u16]) -> Res<Vec<u8>, P::Error> {
    regs.iter()
        .map(|&a| read_reg(io, base, a).map_err(ControlError::Io))
        .collect()
}

fn write_checked<P: PortIo>(io: &mut P, base: u16, addr: u16, value: u8) -> Res<(), P::Error> {
    write_reg(io, base, addr, value)?;
    let read = read_reg(io, base, addr)?;
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

/// Bit n set: output n under software control.
fn mask(modes: &[u8]) -> u8 {
    modes
        .iter()
        .take(8)
        .enumerate()
        .filter(|(_, &m)| manual(m))
        .fold(0, |acc, (n, _)| acc | 1 << n)
}

/// Reads the firmware's control state of the chip's `n` outputs: every mode and duty register.
/// Call before Clokrix writes anything.
pub fn capture<P: PortIo>(
    io: &mut P,
    base: u16,
    (chip_id, chip_rev): (u8, u8),
    n: u16,
    now: u64,
) -> Res<BiosState, P::Error> {
    let n = usize::from(n.min(7));
    let modes = read(io, base, &MODE[..n])?;
    let commands = read(io, base, &DUTY[..n])?;
    Ok(BiosState {
        chip_id,
        chip_rev,
        base,
        mode: mask(&modes),
        commands,
        modes,
        saved_at: now,
    })
}

/// Puts `header` under software control at `pct` (clamped to its floor) and checks the mode and
/// duty registers kept what was written. Returns the duty written.
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
        .find(|h| h.index == header && h.index < 7)
        .ok_or(ControlError::NotAllowed(header))?;
    let raw = duty(h, pct);
    let n = usize::from(h.index);
    let mode = read_reg(io, base, MODE[n])?;
    if !manual(mode) {
        write_checked(io, base, MODE[n], mode & !MODE_BITS)?;
    }
    write_checked(io, base, DUTY[n], raw)?;
    Ok(raw)
}

/// The saved state is a NCT679x's (its modes were recorded) and covers output `n`.
fn recorded(state: &BiosState, n: usize) -> Option<(u8, u8)> {
    Some((*state.commands.get(n)?, *state.modes.get(n)?))
}

/// Puts output `n` back as recorded. On the firmware's curve: the recorded duty only where the
/// output is in manual mode now (an output on its curve may report the duty it drives in that
/// register, outputs 6 and 7 do), then the mode, which hands it back and is written even when the
/// duty was not kept. Manual as recorded: the mode, then the duty. Registers that already hold
/// the recorded value are not written.
fn put_back<P: PortIo>(io: &mut P, base: u16, n: usize, raw: u8, mode: u8) -> Res<(), P::Error> {
    let now = read_reg(io, base, MODE[n])?;
    let set_mode = |io: &mut P| {
        if now == mode {
            Ok(())
        } else {
            write_checked(io, base, MODE[n], mode)
        }
    };
    if manual(mode) {
        set_mode(io)?;
        write_checked(io, base, DUTY[n], raw)
    } else {
        let duty = if manual(now) {
            write_checked(io, base, DUTY[n], raw)
        } else {
            Ok(())
        };
        let mode = set_mode(io);
        duty.and(mode)
    }
}

/// Hands one output back as recorded.
pub fn release<P: PortIo>(
    io: &mut P,
    base: u16,
    header: u16,
    state: &BiosState,
) -> Res<(), P::Error> {
    let n = usize::from(header);
    let (raw, mode) = recorded(state, n)
        .filter(|_| n < 7)
        .ok_or(ControlError::NotAllowed(header))?;
    put_back(io, base, n, raw, mode)
}

/// Puts every recorded output back; the others are tried after one that fails. A state without
/// recorded modes (another chip's) is refused before anything is written.
pub fn restore<P: PortIo>(io: &mut P, base: u16, state: &BiosState) -> Res<(), P::Error> {
    if state.modes.is_empty() || state.modes.len() != state.commands.len() {
        return Err(ControlError::WrongState);
    }
    let mut first = None;
    for (n, (&raw, &mode)) in state.commands.iter().zip(&state.modes).enumerate().take(7) {
        if let Err(e) = put_back(io, base, n, raw, mode) {
            first.get_or_insert(e);
        }
    }
    first.map_or(Ok(()), Err)
}

/// What the chip does now with its `n` outputs: bit n set = output n under software control, and
/// each output's duty.
pub fn read_state<P: PortIo>(io: &mut P, base: u16, n: u16) -> Res<(u8, Vec<u8>), P::Error> {
    let n = usize::from(n.min(7));
    let modes = read(io, base, &MODE[..n])?;
    let outputs = read(io, base, &OUTPUT[..n])?;
    Ok((mask(&modes), outputs))
}

/// One output's registers, for `clokrix-fans`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputRegs {
    pub index: u16,
    pub mode: u8,
    pub duty: u8,
    pub output: u8,
}

/// Every output's mode, duty and output registers, for `clokrix-fans`.
pub fn dump<P: PortIo>(io: &mut P, base: u16, n: u16) -> Res<Vec<OutputRegs>, P::Error> {
    let n = n.min(7);
    let k = usize::from(n);
    let modes = read(io, base, &MODE[..k])?;
    let duties = read(io, base, &DUTY[..k])?;
    let outputs = read(io, base, &OUTPUT[..k])?;
    Ok((0..n)
        .zip(modes)
        .zip(duties.into_iter().zip(outputs))
        .map(|((index, mode), (duty, output))| OutputRegs {
            index,
            mode,
            duty,
            output,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::super::control::{Header, FAN_MIN_PCT};
    use super::super::nct679x::{ADDR_PORT, BANK_SELECT, DATA_PORT};
    use super::*;
    use crate::snapshot::FanKind;
    use std::collections::HashMap;

    const BASE: u16 = 0x290;

    /// A banked NCT679x: bank select at 0x4E, address then data ports. In manual mode an output
    /// drives its duty register; otherwise it drives the firmware's curve (here 0x99).
    struct Fake {
        regs: HashMap<u16, u8>,
        bank: u8,
        addr: u8,
        /// Registers that ignore writes.
        stuck: Vec<u16>,
        writes: Vec<(u16, u8)>,
    }

    impl Fake {
        fn new() -> Self {
            let mut regs = HashMap::new();
            for n in 0..7 {
                // Smart Fan IV with a tolerance of 2; the last manual duty 0x40 + n.
                regs.insert(MODE[n], 0x42);
                regs.insert(DUTY[n], 0x40 + n as u8);
            }
            Self {
                regs,
                bank: 0,
                addr: 0,
                stuck: Vec::new(),
                writes: Vec::new(),
            }
        }

        fn at(&self, a: u16) -> u8 {
            if let Some(n) = OUTPUT.iter().position(|&o| o == a) {
                return if manual(self.at_raw(MODE[n])) {
                    self.at_raw(DUTY[n])
                } else {
                    0x99
                };
            }
            self.at_raw(a)
        }

        fn at_raw(&self, a: u16) -> u8 {
            *self.regs.get(&a).unwrap_or(&0)
        }
    }

    impl PortIo for Fake {
        type Error = ();
        fn inb(&mut self, port: u16) -> Result<u8, ()> {
            match port - BASE {
                DATA_PORT if self.addr == BANK_SELECT => Ok(self.bank),
                DATA_PORT => Ok(self.at(u16::from_be_bytes([self.bank, self.addr]))),
                _ => Err(()),
            }
        }
        fn outb(&mut self, port: u16, value: u8) -> Result<(), ()> {
            match port - BASE {
                ADDR_PORT => self.addr = value,
                DATA_PORT if self.addr == BANK_SELECT => self.bank = value,
                DATA_PORT => {
                    let a = u16::from_be_bytes([self.bank, self.addr]);
                    self.writes.push((a, value));
                    if !self.stuck.contains(&a) {
                        self.regs.insert(a, value);
                    }
                }
                _ => return Err(()),
            }
            Ok(())
        }
    }

    const fn fan(index: u16, name: &'static str) -> Header {
        Header {
            index,
            name,
            kind: FanKind::Fan,
            min_pct: FAN_MIN_PCT,
        }
    }

    const BOARD: Board = Board {
        name: "Constructed ASUS board",
        maker: super::super::control::Maker::Asus,
        product: "TEST",
        chip: Chip::Nct6798,
        chip_rev: 0x2B,
        headers: &[fan(0, "Chassis Fan 1"), fan(1, "CPU Fan")],
    };

    #[test]
    fn outputs_per_chip() {
        assert_eq!(outputs(Chip::Nct6798), 7);
        assert_eq!(outputs(Chip::Nct6791), 6);
        assert_eq!(outputs(Chip::Nct6701), 7);
        assert_eq!(outputs(Chip::Nct6687), 0);
        assert_eq!(outputs(Chip::It8689), 0);
        assert!(manual(0x02));
        assert!(!manual(0x42));
    }

    #[test]
    fn capture_records_every_mode_and_duty() {
        let mut chip = Fake::new();
        chip.regs.insert(MODE[2], 0x00);
        let s = capture(&mut chip, BASE, (0xD4, 0x2B), 7, 9).expect("capture");
        assert_eq!(s.modes, vec![0x42, 0x42, 0x00, 0x42, 0x42, 0x42, 0x42]);
        // Outputs 6 and 7 on their curve report the duty they drive (0x99 here).
        assert_eq!(s.commands, vec![0x40, 0x41, 0x42, 0x43, 0x44, 0x99, 0x99]);
        assert_eq!(s.mode, 0b100, "output 3 was already manual");
        assert_eq!(
            (s.chip_id, s.chip_rev, s.base, s.saved_at),
            (0xD4, 0x2B, BASE, 9)
        );
        assert!(chip.writes.is_empty(), "capture only reads");
        assert_eq!(chip.bank, 0, "bank 0 left selected");
    }

    #[test]
    fn set_duty_takes_one_output_keeping_its_tolerance() {
        let mut chip = Fake::new();
        let raw = set_duty(&mut chip, BASE, &BOARD, 1, 50.0).expect("set");
        assert_eq!(raw, 128);
        assert_eq!(chip.at(MODE[1]), 0x02, "manual, tolerance kept");
        assert_eq!(chip.at(DUTY[1]), 128);
        assert_eq!(chip.at(OUTPUT[1]), 128, "the chip drives it");
        assert_eq!(chip.at(MODE[0]), 0x42, "the other output untouched");
        assert_eq!(chip.writes, [(MODE[1], 0x02), (DUTY[1], 128)]);
        // Already manual: only the duty is written; below the floor it is raised to it.
        let raw = set_duty(&mut chip, BASE, &BOARD, 1, 5.0).expect("floor");
        assert_eq!(raw, duty(&BOARD.headers[1], 20.0));
        assert_eq!(chip.writes.len(), 3);
        assert_eq!(chip.bank, 0);
    }

    #[test]
    fn outputs_not_on_the_board_are_refused() {
        let mut chip = Fake::new();
        assert_eq!(
            set_duty(&mut chip, BASE, &BOARD, 2, 50.0),
            Err(ControlError::NotAllowed(2))
        );
        assert!(chip.writes.is_empty());
    }

    #[test]
    fn a_register_that_does_not_keep_the_write_is_reported() {
        let mut chip = Fake::new();
        chip.stuck.push(MODE[0]);
        assert_eq!(
            set_duty(&mut chip, BASE, &BOARD, 0, 60.0),
            Err(ControlError::Mismatch {
                addr: MODE[0],
                wrote: 0x02,
                read: 0x42
            })
        );
        assert_eq!(chip.at(DUTY[0]), 0x40, "no duty after the mode failed");
    }

    #[test]
    fn release_and_restore_hand_back_what_was_recorded() {
        let mut chip = Fake::new();
        let bios = capture(&mut chip, BASE, (0xD4, 0x2B), 7, 0).expect("capture");
        set_duty(&mut chip, BASE, &BOARD, 0, 70.0).expect("set 0");
        set_duty(&mut chip, BASE, &BOARD, 1, 40.0).expect("set 1");
        release(&mut chip, BASE, 0, &bios).expect("release");
        assert_eq!((chip.at(MODE[0]), chip.at(DUTY[0])), (0x42, 0x40));
        assert_eq!(chip.at(OUTPUT[0]), 0x99, "the firmware's curve again");
        assert!(manual(chip.at(MODE[1])), "release takes one output only");
        restore(&mut chip, BASE, &bios).expect("restore");
        for n in 0..7 {
            assert_eq!(chip.at(MODE[n]), bios.modes[n]);
            assert_eq!(chip.at(DUTY[n]), bios.commands[n]);
        }
        let (mode, out) = read_state(&mut chip, BASE, 7).expect("read");
        assert_eq!(mode, 0);
        assert!(out.iter().all(|&o| o == 0x99));
    }

    #[test]
    fn outputs_left_on_their_curve_are_not_written() {
        let mut chip = Fake::new();
        let bios = capture(&mut chip, BASE, (0xD4, 0x2B), 7, 0).expect("capture");
        set_duty(&mut chip, BASE, &BOARD, 1, 40.0).expect("set");
        let before = chip.writes.len();
        restore(&mut chip, BASE, &bios).expect("restore");
        assert_eq!(
            chip.writes[before..],
            [(DUTY[1], 0x41), (MODE[1], 0x42)],
            "only the output Clokrix took"
        );
        // An output the firmware had in manual mode goes back to it, mode then duty.
        let mut chip = Fake::new();
        chip.regs.insert(MODE[0], 0x00);
        chip.regs.insert(DUTY[0], 0xFF);
        let bios = capture(&mut chip, BASE, (0xD4, 0x2B), 7, 0).expect("capture");
        chip.regs.insert(MODE[0], 0x42);
        release(&mut chip, BASE, 0, &bios).expect("release");
        assert_eq!((chip.at(MODE[0]), chip.at(DUTY[0])), (0x00, 0xFF));
    }

    #[test]
    fn restore_writes_every_mode_even_when_a_duty_fails() {
        let mut chip = Fake::new();
        let bios = capture(&mut chip, BASE, (0xD4, 0x2B), 7, 0).expect("capture");
        set_duty(&mut chip, BASE, &BOARD, 0, 70.0).expect("set");
        chip.stuck.push(DUTY[0]);
        assert!(matches!(
            restore(&mut chip, BASE, &bios),
            Err(ControlError::Mismatch { addr, .. }) if addr == DUTY[0]
        ));
        assert_eq!(chip.at(MODE[0]), 0x42, "back on the firmware's curve");
    }

    #[test]
    fn another_chips_state_is_refused() {
        let mut chip = Fake::new();
        let nct6687 = BiosState {
            chip_id: 0xD5,
            chip_rev: 0x92,
            base: 0xA20,
            mode: 0,
            commands: vec![0x66; 8],
            modes: Vec::new(),
            saved_at: 0,
        };
        assert_eq!(
            restore(&mut chip, BASE, &nct6687),
            Err(ControlError::WrongState)
        );
        assert_eq!(
            release(&mut chip, BASE, 0, &nct6687),
            Err(ControlError::NotAllowed(0))
        );
        assert!(chip.writes.is_empty());
    }

    #[test]
    fn dump_lists_each_output() {
        let mut chip = Fake::new();
        chip.regs.insert(MODE[1], 0x01);
        chip.regs.insert(DUTY[1], 0xFF);
        let d = dump(&mut chip, BASE, 6).expect("dump");
        assert_eq!(d.len(), 6);
        let row = |index, mode, duty, output| OutputRegs {
            index,
            mode,
            duty,
            output,
        };
        assert_eq!(d[1], row(1, 0x01, 0xFF, 0xFF));
        assert_eq!(d[0], row(0, 0x42, 0x40, 0x99));
    }
}
