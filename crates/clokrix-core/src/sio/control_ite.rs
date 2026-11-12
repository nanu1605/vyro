//! Fan control on Gigabyte's ITE chips (2.4): for the first chip's PWM outputs, the write path
//! [`super::control679x`] has for the NCT679x. The registers are Linux `it87`'s (its out-of-tree
//! version's for the IT8686E to IT8698E): each output has a control register whose bit 7 hands it
//! to the chip's automatic mode (SmartGuardian, which Gigabyte's Smart Fan curves run on) and whose
//! low bits choose the temperature that mode follows, and a duty register (0–255) the chip drives
//! while bit 7 is clear; in automatic mode the duty register is read-only and reports the duty the
//! curve drives. Outputs 1–3 also have a bit in the fan main control register (0x13, bits 0–2):
//! clear, the output is in on/off mode, at full speed.
//!
//! Taking an output: its main-control bit set if it was clear, its control register's bit 7
//! cleared with the temperature mapping kept, as Linux clears it (LibreHardwareMonitor writes 0x7F
//! on the IT8689E instead), then the duty. Giving it back: the recorded duty while the output is
//! still manual, then the recorded control register, which hands it back to the curve, then the
//! main-control bit. Every write is read back, in bank 0 on the chips with banks (the bank found
//! is put back). A chip whose fan control register (0x14) has every fan off with active-low
//! polarity is not written: Linux takes that for a PWM setup the firmware got wrong.
//!
//! Only the first chip: the second chip's fans are driven by the board's embedded controller.
//! Which outputs may be written, and how low, is the board's, and no board's fans are driven
//! before its own field check (`docs/control-safety.md`). Pure: the port I/O is behind
//! [`PortIo`], tested on a fake chip.

use super::control::{duty, BiosState, Board, ControlError};
use super::ite::{self, in_bank0, read_reg, write_reg};
use super::{Chip, PortIo};

/// Control register of PWM outputs 1 to 6.
pub const CTRL: [u8; 6] = [0x15, 0x16, 0x17, 0x7F, 0xA7, 0xAF];
/// Duty register of each output.
pub const DUTY: [u8; 6] = [0x63, 0x6B, 0x73, 0x7B, 0xA3, 0xAB];
/// Fan main control: bits 0–2 put outputs 1–3 in PWM mode; bits 4–6 enable fan inputs 1–3.
pub const MAIN: u8 = ite::FAN_MAIN_CTRL;
/// Fan control: bit 7 the PWM polarity (set: active high), bits 0–2 outputs 1–3 on in on/off mode.
pub const FAN_CTL: u8 = 0x14;
/// Bit 7 of a control register: the chip's automatic mode.
const AUTO: u8 = 0x80;

const BROKEN: &str = "the chip's PWM setup looks broken (every fan off, active-low polarity), so \
                      its fans are left to the firmware";

/// PWM outputs of a first chip, the fewer where Linux and LibreHardwareMonitor differ (five on the
/// IT8620E, IT8686E and IT8688E); 0 on the second chips and the Nuvoton ones.
pub const fn outputs(chip: Chip) -> u16 {
    match chip {
        Chip::It8628 | Chip::It8689 | Chip::It8696 | Chip::It8698 => 6,
        Chip::It8620 | Chip::It8686 | Chip::It8688 => 5,
        _ => 0,
    }
}

/// A control register value that means manual (software) control.
pub const fn manual(ctrl: u8) -> bool {
    ctrl & AUTO == 0
}

/// Output `n` is driven by the duty register: manual, and for outputs 1–3 in PWM mode.
pub const fn software(n: usize, ctrl: u8, main: u8) -> bool {
    manual(ctrl) && (n >= 3 || main & (1 << n) != 0)
}

type Res<T, E> = Result<T, ControlError<E>>;

fn read<P: PortIo>(io: &mut P, base: u16, regs: &[u8]) -> Res<Vec<u8>, P::Error> {
    regs.iter()
        .map(|&r| read_reg(io, base, r).map_err(ControlError::Io))
        .collect()
}

fn write_checked<P: PortIo>(io: &mut P, base: u16, reg: u8, value: u8) -> Res<(), P::Error> {
    write_reg(io, base, reg, value)?;
    let read = read_reg(io, base, reg)?;
    if read == value {
        Ok(())
    } else {
        Err(ControlError::Mismatch {
            addr: u16::from(reg),
            wrote: value,
            read,
        })
    }
}

/// Bit n set: output n under software control.
fn mask(ctrls: &[u8], main: u8) -> u8 {
    ctrls
        .iter()
        .enumerate()
        .filter(|&(n, &c)| software(n, c, main))
        .fold(0, |acc, (n, _)| acc | 1 << n)
}

/// Reads the firmware's control state of the chip's `n` outputs: the main control register and
/// every control and duty register. Call before Clokrix writes anything; a chip whose PWM setup
/// looks broken is refused here.
pub fn capture<P: PortIo>(
    io: &mut P,
    base: u16,
    banks: bool,
    (chip_id, chip_rev): (u8, u8),
    n: u16,
    now: u64,
) -> Res<BiosState, P::Error> {
    let n = usize::from(n.min(6));
    in_bank0(io, base, banks, |io| {
        if read_reg(io, base, FAN_CTL)? & 0x87 == 0 {
            return Err(ControlError::Refused(BROKEN));
        }
        let main = read_reg(io, base, MAIN)?;
        let ctrls = read(io, base, &CTRL[..n])?;
        let commands = read(io, base, &DUTY[..n])?;
        Ok(BiosState {
            chip_id,
            chip_rev,
            base,
            mode: mask(&ctrls, main),
            commands,
            modes: ctrls,
            fan_main: Some(main),
            saved_at: now,
        })
    })
}

/// Puts `header` under software control at `pct` (clamped to its floor) and checks every register
/// kept what was written. Returns the duty written.
pub fn set_duty<P: PortIo>(
    io: &mut P,
    base: u16,
    banks: bool,
    board: &Board,
    header: u16,
    pct: f64,
) -> Res<u8, P::Error> {
    let h = board
        .headers
        .iter()
        .find(|h| h.index == header && h.index < 6)
        .ok_or(ControlError::NotAllowed(header))?;
    let raw = duty(h, pct);
    let n = usize::from(h.index);
    in_bank0(io, base, banks, |io| {
        if n < 3 {
            let main = read_reg(io, base, MAIN)?;
            if main & (1 << n) == 0 {
                write_checked(io, base, MAIN, main | 1 << n)?;
            }
        }
        let ctrl = read_reg(io, base, CTRL[n])?;
        if !manual(ctrl) {
            write_checked(io, base, CTRL[n], ctrl & !AUTO)?;
        }
        write_checked(io, base, DUTY[n], raw)?;
        Ok(raw)
    })
}

/// The saved state is an ITE chip's and covers output `n`: its duty, control register and the
/// main control register.
fn recorded(state: &BiosState, n: usize) -> Option<(u8, u8, u8)> {
    Some((
        *state.commands.get(n)?,
        *state.modes.get(n)?,
        state.fan_main?,
    ))
}

/// Puts output `n` back as recorded. On the firmware's curve: the recorded duty only where the
/// output is manual now (in automatic mode the duty register is read-only), then the control
/// register, which hands it back and is written even when the duty was not kept. Manual as
/// recorded: the control register, then the duty. Then, for outputs 1–3, the main control bit as
/// recorded, its other bits as they are. Registers that already hold the recorded value are not
/// written.
fn put_back<P: PortIo>(
    io: &mut P,
    base: u16,
    n: usize,
    (raw, ctrl, main): (u8, u8, u8),
) -> Res<(), P::Error> {
    let now = read_reg(io, base, CTRL[n])?;
    let control = if manual(ctrl) {
        let set = if now == ctrl {
            Ok(())
        } else {
            write_checked(io, base, CTRL[n], ctrl)
        };
        set.and_then(|()| write_checked(io, base, DUTY[n], raw))
    } else {
        let duty = if manual(now) {
            write_checked(io, base, DUTY[n], raw)
        } else {
            Ok(())
        };
        let set = if now == ctrl {
            Ok(())
        } else {
            write_checked(io, base, CTRL[n], ctrl)
        };
        duty.and(set)
    };
    let mode = if n < 3 {
        let bit = 1u8 << n;
        let cur = read_reg(io, base, MAIN)?;
        if cur & bit == main & bit {
            Ok(())
        } else {
            write_checked(io, base, MAIN, (cur & !bit) | (main & bit))
        }
    } else {
        Ok(())
    };
    control.and(mode)
}

/// Hands one output back as recorded.
pub fn release<P: PortIo>(
    io: &mut P,
    base: u16,
    banks: bool,
    header: u16,
    state: &BiosState,
) -> Res<(), P::Error> {
    let n = usize::from(header);
    let rec = recorded(state, n)
        .filter(|_| n < 6)
        .ok_or(ControlError::NotAllowed(header))?;
    in_bank0(io, base, banks, |io| put_back(io, base, n, rec))
}

/// Puts every recorded output back; the others are tried after one that fails. A state without
/// the main control register (a Nuvoton chip's) is refused before anything is written.
pub fn restore<P: PortIo>(
    io: &mut P,
    base: u16,
    banks: bool,
    state: &BiosState,
) -> Res<(), P::Error> {
    if state.fan_main.is_none()
        || state.modes.is_empty()
        || state.modes.len() != state.commands.len()
    {
        return Err(ControlError::WrongState);
    }
    in_bank0(io, base, banks, |io| {
        let mut first = None;
        for n in 0..state.modes.len().min(6) {
            if let Some(rec) = recorded(state, n) {
                if let Err(e) = put_back(io, base, n, rec) {
                    first.get_or_insert(e);
                }
            }
        }
        first.map_or(Ok(()), Err)
    })
}

/// What the chip does now with its `n` outputs: bit n set = output n under software control, and
/// each output's duty.
pub fn read_state<P: PortIo>(
    io: &mut P,
    base: u16,
    banks: bool,
    n: u16,
) -> Res<(u8, Vec<u8>), P::Error> {
    let n = usize::from(n.min(6));
    in_bank0(io, base, banks, |io| {
        let main = read_reg(io, base, MAIN)?;
        let ctrls = read(io, base, &CTRL[..n])?;
        let duties = read(io, base, &DUTY[..n])?;
        Ok((mask(&ctrls, main), duties))
    })
}

/// One output's registers, for `clokrix-fans`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pwm {
    pub index: u16,
    pub ctrl: u8,
    pub duty: u8,
    pub software: bool,
}

/// The fan main control and fan control registers and every output's, for `clokrix-fans`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dump {
    pub main: u8,
    pub fan_ctl: u8,
    pub outputs: Vec<Pwm>,
}

/// Reads [`Dump`] for the chip's `n` outputs.
pub fn dump<P: PortIo>(io: &mut P, base: u16, banks: bool, n: u16) -> Res<Dump, P::Error> {
    let n = n.min(6);
    in_bank0(io, base, banks, |io| {
        let main = read_reg(io, base, MAIN)?;
        let fan_ctl = read_reg(io, base, FAN_CTL)?;
        let k = usize::from(n);
        let ctrls = read(io, base, &CTRL[..k])?;
        let duties = read(io, base, &DUTY[..k])?;
        let outputs = (0..n)
            .zip(ctrls.into_iter().zip(duties))
            .map(|(index, (ctrl, duty))| Pwm {
                index,
                ctrl,
                duty,
                software: software(usize::from(index), ctrl, main),
            })
            .collect();
        Ok(Dump {
            main,
            fan_ctl,
            outputs,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::super::control::{Header, Maker, FAN_MIN_PCT};
    use super::super::ite::{ADDR_PORT, BANK, DATA_PORT};
    use super::*;
    use crate::snapshot::FanKind;
    use std::collections::HashMap;

    const BASE: u16 = 0xA40;
    /// What the fake chip's curve drives in automatic mode.
    const CURVE: u8 = 0x99;

    /// An ITE environment controller with four banks of 0x10–0xAF. A duty register is driven and
    /// written only while its output is manual; in automatic mode it reads the curve's duty and
    /// ignores writes, as Linux describes it.
    struct Fake {
        regs: HashMap<(u8, u8), u8>,
        bank_reg: u8,
        addr: u8,
        /// Registers that ignore writes.
        stuck: Vec<u8>,
        writes: Vec<(u8, u8)>,
        /// Writes made with a bank other than 0 selected.
        off_bank: usize,
    }

    impl Fake {
        fn new() -> Self {
            let mut regs = HashMap::new();
            // Fans 1–3 counted, outputs 1–3 in PWM mode; polarity active high.
            regs.insert((0, MAIN), 0x77);
            regs.insert((0, FAN_CTL), 0x80);
            for n in 0..6 {
                // Automatic, following temperature input n % 3; the last manual duty 0x40 + n.
                regs.insert((0, CTRL[n]), 0x80 | (n as u8 % 3));
                regs.insert((0, DUTY[n]), 0x40 + n as u8);
                // Another bank holds something else at the same addresses.
                regs.insert((2, CTRL[n]), 0x11);
            }
            Self {
                regs,
                bank_reg: 0x00,
                addr: 0,
                stuck: Vec::new(),
                writes: Vec::new(),
                off_bank: 0,
            }
        }

        fn bank(&self, reg: u8) -> u8 {
            if (0x10..=0xAF).contains(&reg) {
                (self.bank_reg & 0xE0) >> 5
            } else {
                0
            }
        }

        fn raw(&self, reg: u8) -> u8 {
            *self.regs.get(&(self.bank(reg), reg)).unwrap_or(&0)
        }

        /// What bank 0 holds, as the chip reports it.
        fn at(&self, reg: u8) -> u8 {
            if let Some(n) = DUTY.iter().position(|&d| d == reg) {
                if !manual(self.regs[&(0, CTRL[n])]) {
                    return CURVE;
                }
            }
            *self.regs.get(&(0, reg)).unwrap_or(&0)
        }
    }

    impl PortIo for Fake {
        type Error = ();
        fn inb(&mut self, port: u16) -> Result<u8, ()> {
            match port - BASE {
                DATA_PORT if self.addr == BANK => Ok(self.bank_reg),
                DATA_PORT if self.bank(self.addr) == 0 => Ok(self.at(self.addr)),
                DATA_PORT => Ok(self.raw(self.addr)),
                _ => Err(()),
            }
        }
        fn outb(&mut self, port: u16, value: u8) -> Result<(), ()> {
            match port - BASE {
                ADDR_PORT => self.addr = value,
                DATA_PORT if self.addr == BANK => self.bank_reg = value,
                DATA_PORT => {
                    let (bank, reg) = (self.bank(self.addr), self.addr);
                    if bank != 0 {
                        self.off_bank += 1;
                    }
                    self.writes.push((reg, value));
                    let duty_on_curve = DUTY
                        .iter()
                        .position(|&d| d == reg)
                        .is_some_and(|n| !manual(self.regs[&(0, CTRL[n])]));
                    if !self.stuck.contains(&reg) && !duty_on_curve {
                        self.regs.insert((bank, reg), value);
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
        name: "Constructed Gigabyte board",
        maker: Maker::Gigabyte,
        product: "TEST",
        chip: Chip::It8689,
        chip_rev: 0x89,
        headers: &[
            fan(0, "CPU Fan"),
            fan(1, "System Fan 1"),
            fan(3, "System Fan 3"),
            fan(6, "Not an output"),
        ],
    };

    fn bios(chip: &mut Fake) -> BiosState {
        capture(chip, BASE, true, (0x86, 0x89), 6, 7).expect("capture")
    }

    #[test]
    fn outputs_per_chip() {
        assert_eq!(outputs(Chip::It8689), 6);
        assert_eq!(outputs(Chip::It8686), 5);
        assert_eq!(outputs(Chip::It8620), 5);
        assert_eq!(outputs(Chip::It8792), 0, "the second chip is the EC's");
        assert_eq!(outputs(Chip::Nct6798), 0);
        assert!(manual(0x02));
        assert!(!manual(0x82));
        assert!(software(1, 0x01, 0x02));
        assert!(
            !software(1, 0x01, 0x05),
            "on/off mode: full speed, not the duty"
        );
        assert!(software(4, 0x00, 0x00));
    }

    #[test]
    fn capture_records_every_register_and_writes_nothing() {
        let mut chip = Fake::new();
        chip.regs.insert((0, CTRL[4]), 0x01);
        let s = bios(&mut chip);
        assert_eq!(s.modes, vec![0x80, 0x81, 0x82, 0x80, 0x01, 0x82]);
        // Outputs on their curve report the curve's duty; output 5, manual, its own.
        assert_eq!(s.commands, vec![CURVE, CURVE, CURVE, CURVE, 0x44, CURVE]);
        assert_eq!(s.fan_main, Some(0x77));
        assert_eq!(s.mode, 0b1_0000, "output 5 was already manual");
        assert_eq!(
            (s.chip_id, s.chip_rev, s.base, s.saved_at),
            (0x86, 0x89, BASE, 7)
        );
        assert!(chip.writes.is_empty(), "capture only reads");
    }

    #[test]
    fn a_broken_pwm_setup_is_refused() {
        let mut chip = Fake::new();
        chip.regs.insert((0, FAN_CTL), 0x00);
        assert_eq!(
            capture(&mut chip, BASE, true, (0x86, 0x89), 6, 0),
            Err(ControlError::Refused(BROKEN))
        );
        assert!(chip.writes.is_empty());
        // One fan on in on/off mode, or active-high polarity: a setup Linux accepts.
        chip.regs.insert((0, FAN_CTL), 0x01);
        assert!(capture(&mut chip, BASE, true, (0x86, 0x89), 6, 0).is_ok());
    }

    #[test]
    fn set_duty_takes_one_output_keeping_its_temperature_mapping() {
        let mut chip = Fake::new();
        let raw = set_duty(&mut chip, BASE, true, &BOARD, 1, 50.0).expect("set");
        assert_eq!(raw, 128);
        assert_eq!(chip.at(CTRL[1]), 0x01, "manual, mapping kept");
        assert_eq!(chip.at(DUTY[1]), 128, "the chip drives it");
        assert_eq!(chip.at(CTRL[0]), 0x80, "the other outputs untouched");
        assert_eq!(chip.writes, [(CTRL[1], 0x01), (DUTY[1], 128)]);
        // Already manual: only the duty; below the floor it is raised to it.
        let raw = set_duty(&mut chip, BASE, true, &BOARD, 1, 5.0).expect("floor");
        assert_eq!(raw, duty(&BOARD.headers[1], 20.0));
        assert_eq!(chip.writes.len(), 3);
    }

    #[test]
    fn an_output_in_on_off_mode_is_put_in_pwm_mode_first() {
        let mut chip = Fake::new();
        chip.regs.insert((0, MAIN), 0x75);
        set_duty(&mut chip, BASE, true, &BOARD, 1, 60.0).expect("set");
        assert_eq!(chip.writes[0], (MAIN, 0x77), "bit 1 set, the others kept");
        assert_eq!(chip.at(MAIN), 0x77);
        // Outputs 4–6 have no such bit.
        let mut chip = Fake::new();
        chip.regs.insert((0, MAIN), 0x70);
        set_duty(&mut chip, BASE, true, &BOARD, 3, 60.0).expect("set");
        assert!(chip.writes.iter().all(|w| w.0 != MAIN));
    }

    #[test]
    fn writes_go_to_bank_zero_and_the_bank_found_is_put_back() {
        let mut chip = Fake::new();
        chip.bank_reg = 0x41;
        set_duty(&mut chip, BASE, true, &BOARD, 0, 70.0).expect("set");
        assert_eq!(chip.off_bank, 0, "nothing written in bank 2");
        assert_eq!(chip.at(CTRL[0]), 0x00);
        assert_eq!(chip.regs[&(2, CTRL[0])], 0x11, "bank 2 untouched");
        assert_eq!(chip.bank_reg, 0x41, "bank 2 selected again");
    }

    #[test]
    fn outputs_not_on_the_board_are_refused() {
        let mut chip = Fake::new();
        for header in [2, 6] {
            assert_eq!(
                set_duty(&mut chip, BASE, true, &BOARD, header, 50.0),
                Err(ControlError::NotAllowed(header))
            );
        }
        assert!(chip.writes.is_empty());
    }

    #[test]
    fn a_register_that_does_not_keep_the_write_is_reported() {
        let mut chip = Fake::new();
        chip.stuck.push(CTRL[0]);
        assert_eq!(
            set_duty(&mut chip, BASE, true, &BOARD, 0, 60.0),
            Err(ControlError::Mismatch {
                addr: u16::from(CTRL[0]),
                wrote: 0x00,
                read: 0x80
            })
        );
        assert!(
            chip.writes.iter().all(|w| w.0 != DUTY[0]),
            "no duty after it"
        );
    }

    #[test]
    fn release_and_restore_hand_back_what_was_recorded() {
        let mut chip = Fake::new();
        chip.regs.insert((0, MAIN), 0x76);
        let s = bios(&mut chip);
        set_duty(&mut chip, BASE, true, &BOARD, 0, 70.0).expect("set 0");
        set_duty(&mut chip, BASE, true, &BOARD, 3, 40.0).expect("set 3");
        assert_eq!(chip.at(MAIN), 0x77);
        release(&mut chip, BASE, true, 0, &s).expect("release");
        assert_eq!(chip.at(CTRL[0]), 0x80);
        assert_eq!(chip.at(DUTY[0]), CURVE, "the firmware's curve again");
        assert_eq!(chip.at(MAIN), 0x76, "on/off mode again, as recorded");
        assert!(manual(chip.at(CTRL[3])), "release takes one output only");
        restore(&mut chip, BASE, true, &s).expect("restore");
        for (&reg, &recorded) in CTRL.iter().zip(&s.modes) {
            assert_eq!(chip.at(reg), recorded);
        }
        let (mode, duties) = read_state(&mut chip, BASE, true, 6).expect("read");
        assert_eq!(mode, 0);
        assert!(duties.iter().all(|&d| d == CURVE));
    }

    #[test]
    fn restore_writes_only_the_outputs_clokrix_took() {
        let mut chip = Fake::new();
        let s = bios(&mut chip);
        set_duty(&mut chip, BASE, true, &BOARD, 1, 40.0).expect("set");
        let before = chip.writes.len();
        restore(&mut chip, BASE, true, &s).expect("restore");
        assert_eq!(
            chip.writes[before..],
            [(DUTY[1], CURVE), (CTRL[1], 0x81)],
            "the duty while manual, then automatic again"
        );
        // An output the firmware had manual goes back to it, control register then duty.
        let mut chip = Fake::new();
        chip.regs.insert((0, CTRL[4]), 0x01);
        chip.regs.insert((0, DUTY[4]), 0xFF);
        let s = bios(&mut chip);
        chip.regs.insert((0, CTRL[4]), 0x81);
        release(&mut chip, BASE, true, 4, &s).expect("release");
        assert_eq!((chip.at(CTRL[4]), chip.at(DUTY[4])), (0x01, 0xFF));
    }

    #[test]
    fn restore_hands_every_output_back_even_when_a_duty_fails() {
        let mut chip = Fake::new();
        let s = bios(&mut chip);
        set_duty(&mut chip, BASE, true, &BOARD, 0, 70.0).expect("set");
        chip.stuck.push(DUTY[0]);
        assert!(matches!(
            restore(&mut chip, BASE, true, &s),
            Err(ControlError::Mismatch { addr, .. }) if addr == u16::from(DUTY[0])
        ));
        assert_eq!(chip.at(CTRL[0]), 0x80, "back on the firmware's curve");
    }

    #[test]
    fn a_nuvoton_state_is_refused() {
        let mut chip = Fake::new();
        let nct679x = BiosState {
            chip_id: 0xD4,
            chip_rev: 0x2B,
            base: 0x290,
            mode: 0,
            commands: vec![0x66; 7],
            modes: vec![0x42; 7],
            fan_main: None,
            saved_at: 0,
        };
        assert_eq!(
            restore(&mut chip, BASE, true, &nct679x),
            Err(ControlError::WrongState)
        );
        assert_eq!(
            release(&mut chip, BASE, true, 0, &nct679x),
            Err(ControlError::NotAllowed(0))
        );
        assert!(chip.writes.is_empty());
    }

    #[test]
    fn dump_lists_each_output() {
        let mut chip = Fake::new();
        chip.regs.insert((0, CTRL[1]), 0x01);
        chip.regs.insert((0, DUTY[1]), 0xFF);
        let d = dump(&mut chip, BASE, true, 5).expect("dump");
        assert_eq!((d.main, d.fan_ctl, d.outputs.len()), (0x77, 0x80, 5));
        assert_eq!(
            d.outputs[1],
            Pwm {
                index: 1,
                ctrl: 0x01,
                duty: 0xFF,
                software: true
            }
        );
        assert_eq!(d.outputs[0].duty, CURVE);
        assert!(!d.outputs[0].software);
        assert!(chip.writes.is_empty());
    }
}
