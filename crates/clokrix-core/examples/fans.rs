//! Fan-control check for Phase 12a (and, from 2.3, a NCT679x board's first check; from 2.4 a
//! Gigabyte board's, on its first ITE chip), run as administrator with PawnIO installed.
//!
//! ```sh
//! clokrix-fans                 # read only: control registers, each header's mode / duty / RPM
//! clokrix-fans --try 2 50      # supervised test: header 2 at 50 % for 10 s, then back to the BIOS
//! clokrix-fans --restore       # hand every header back to the BIOS from clokrix-fans-restore.json
//! ```
//!
//! The default run writes nothing. `--try` works on an allow-listed board and header, and on an
//! ASUS board with a NCT679x or a Gigabyte board with an ITE chip that has not had its field check
//! yet (one PWM output at a time, with you watching: the first step of that check). It saves the
//! BIOS state next to the executable before writing and restores it afterwards. Quit MSI Center,
//! Armoury Crate / AI Suite, Gigabyte Control Center and FanControl first: two programs driving
//! the same chip fight each other.

#[cfg(windows)]
mod win {
    use std::io::{BufRead, IsTerminal};
    use std::path::PathBuf;
    use std::time::Duration;

    use clokrix_core::pawnio::{BusLock, Handle, Module, PawnIoError, MUTEX_ISABUS};
    use clokrix_core::sio::control::{self, BiosState, Board, ControlError};
    use clokrix_core::sio::{
        self, control679x, control_ite, ite, nct679x, Family, Found, FAN_BASE,
    };

    type Error = Box<dyn std::error::Error>;

    const LOCK_MS: u32 = 500;

    fn hex(bytes: &[u8]) -> Vec<String> {
        bytes
            .chunks(16)
            .map(|row| {
                row.iter()
                    .map(|x| format!("{x:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }

    struct Chip {
        io: Handle,
        lock: BusLock,
        found: Found,
    }

    impl Chip {
        fn open() -> Result<Self, Error> {
            let io = Handle::open(Module::LpcIo)?;
            let lock = BusLock::new(MUTEX_ISABUS);
            let found = lock
                .hold(LOCK_MS, || sio::detect(&io))
                .ok_or("ISA bus busy")??
                .ok_or("no supported Super-I/O chip found")?;
            // A NCT679x whose firmware locked its monitor's ports answers nothing until the
            // lock is cleared, as the app clears it.
            if found.chip.family() == Family::Nct679x {
                let vendor = lock
                    .hold(LOCK_MS, || {
                        nct679x::vendor(&mut sio::Ports(&io), found.base)
                    })
                    .ok_or("ISA bus busy")??;
                if vendor != nct679x::NUVOTON && found.io_locked {
                    println!(
                        "The firmware locked the monitor's ports; unlocking, as the app does."
                    );
                    lock.hold(LOCK_MS, || sio::unlock_monitor(&io, &found))
                        .ok_or("ISA bus busy")??;
                }
            }
            Ok(Self { io, lock, found })
        }

        fn banked(&self) -> bool {
            self.found.chip.family() == Family::Nct679x
        }

        fn ite(&self) -> bool {
            self.found.chip.family() == Family::Ite
        }

        /// An ITE chip with register banks.
        fn banks(&self) -> bool {
            ite::has_banks(self.found.chip)
        }

        /// One locked access: reads `len` EC bytes from `start` (the NCT6687).
        fn read(&self, start: u16, len: u16) -> Result<Vec<u8>, Error> {
            let r = self
                .lock
                .hold(LOCK_MS, || {
                    sio::read_block(&self.io, self.found.base, start, len)
                })
                .ok_or("ISA bus busy")??;
            Ok(r.ok_or("another program is using the fan controller")?)
        }

        /// One locked read of a NCT679x's registers.
        fn read_banked(&self, regs: &[u16]) -> Result<Vec<(u16, u8)>, Error> {
            let r = self
                .lock
                .hold(LOCK_MS, || {
                    nct679x::read(&mut sio::Ports(&self.io), self.found.base, regs)
                })
                .ok_or("ISA bus busy")??;
            Ok(r)
        }

        /// One locked control operation.
        fn control<R>(
            &self,
            f: impl FnOnce(&mut sio::Ports<'_>) -> Result<R, ControlError<PawnIoError>>,
        ) -> Result<R, Error> {
            let r = self
                .lock
                .hold(LOCK_MS, || f(&mut sio::Ports(&self.io)))
                .ok_or("ISA bus busy")?;
            r.map_err(|e| e.to_string().into())
        }

        fn outputs(&self) -> u16 {
            if self.ite() {
                control_ite::outputs(self.found.chip)
            } else {
                control679x::outputs(self.found.chip)
            }
        }

        fn capture(&self, now: u64) -> Result<BiosState, Error> {
            let (id, rev, base) = (self.found.id, self.found.rev, self.found.base);
            let (n, banks) = (self.outputs(), self.banks());
            if self.ite() {
                self.control(|io| control_ite::capture(io, base, banks, (id, rev), n, now))
            } else if self.banked() {
                self.control(|io| control679x::capture(io, base, (id, rev), n, now))
            } else {
                self.control(|io| control::capture(io, base, id, rev, now))
            }
        }

        fn set(&self, board: &Board, header: u16, pct: f64) -> Result<u8, Error> {
            let (base, banks) = (self.found.base, self.banks());
            if self.ite() {
                self.control(|io| control_ite::set_duty(io, base, banks, board, header, pct))
            } else if self.banked() {
                self.control(|io| control679x::set_duty(io, base, board, header, pct))
            } else {
                self.control(|io| control::set_duty(io, base, board, header, pct))
            }
        }

        /// Puts a saved state back, if it is this chip's (the same ID and revision).
        fn restore(&self, state: &BiosState) -> Result<(), Error> {
            if (state.chip_id, state.chip_rev) != (self.found.id, self.found.rev) {
                return Err(format!(
                    "the saved fan settings are another chip's ({:#04x} {:#04x}), not this one's",
                    state.chip_id, state.chip_rev
                )
                .into());
            }
            let (base, banks) = (self.found.base, self.banks());
            if self.ite() {
                self.control(|io| control_ite::restore(io, base, banks, state))
            } else if self.banked() {
                self.control(|io| control679x::restore(io, base, state))
            } else {
                self.control(|io| control::restore(io, base, state))
            }
        }
    }

    fn name(board: Option<&Board>, index: u16) -> &'static str {
        board
            .and_then(|b| b.headers.iter().find(|x| x.index == index))
            .map_or("-", |x| x.name)
    }

    /// Mode, duty command, duty output and RPM of each header, as a table and as JSON.
    fn snapshot(chip: &Chip, board: Option<&Board>) -> Result<serde_json::Value, Error> {
        if chip.ite() {
            return snapshot_ite(chip, board);
        }
        if chip.banked() {
            return snapshot_banked(chip, board);
        }
        let mode = chip.read(control::CTRL_MODE, 1)?[0];
        let commands = chip.read(control::DUTY_COMMAND, control::HEADERS)?;
        let outputs = chip.read(control::DUTY_OUTPUT, control::HEADERS)?;
        let fans = chip.read(FAN_BASE, control::HEADERS * 2)?;
        let mut rows = Vec::new();
        println!("  hdr  name            mode      command   output      RPM");
        for h in control::headers(mode, &commands, &outputs) {
            let i = usize::from(h.index) * 2;
            let rpm = sio::rpm(fans[i], fans[i + 1]);
            let name = name(board, h.index);
            println!(
                "  {:<4} {:<15} {:<9} {:>5.1} %   {:>5.1} %   {:>6}",
                h.index,
                name,
                if h.software { "software" } else { "BIOS" },
                control::percent(h.command),
                control::percent(h.output),
                rpm.map_or("-".to_owned(), |r| format!("{r:.0}")),
            );
            rows.push(serde_json::json!({
                "header": h.index, "name": name, "software": h.software,
                "command": h.command, "output": h.output, "rpm": rpm,
            }));
        }
        Ok(serde_json::json!({ "mode": format!("{mode:#04x}"), "headers": rows }))
    }

    /// The same for a NCT679x: each PWM output's mode register, duty, output and the speed of the
    /// fan input with the same number (usually, not always, the header's own).
    fn snapshot_banked(chip: &Chip, board: Option<&Board>) -> Result<serde_json::Value, Error> {
        let base = chip.found.base;
        let n = chip.outputs();
        let dump = chip.control(|io| control679x::dump(io, base, n))?;
        let fan_regs: Vec<u16> = nct679x::tables(chip.found.chip)
            .map(|t| t.fans.iter().flat_map(|&f| [f, f + 1]).collect())
            .unwrap_or_default();
        let fans = chip.read_banked(&fan_regs)?;
        let rpm = |i: usize| {
            let r = |a: u16| fans.iter().find(|x| x.0 == a).map(|x| x.1);
            let reg = *fan_regs.get(i * 2)?;
            let v = u16::from_be_bytes([r(reg)?, r(reg + 1)?]);
            (v != 0xFFFF).then_some(f64::from(v))
        };
        let mut rows = Vec::new();
        println!("  pwm  name            mode            duty      output      RPM");
        for control679x::OutputRegs {
            index: h,
            mode,
            duty,
            output,
        } in dump
        {
            let software = control679x::manual(mode);
            let name = name(board, h);
            let rpm = rpm(usize::from(h));
            println!(
                "  {:<4} {:<15} {:<15} {:>5.1} %   {:>5.1} %   {:>6}",
                h,
                name,
                if software {
                    "software".to_owned()
                } else {
                    format!("BIOS ({mode:#04x})")
                },
                control::percent(duty),
                control::percent(output),
                rpm.map_or("-".to_owned(), |r| format!("{r:.0}")),
            );
            rows.push(serde_json::json!({
                "header": h, "name": name, "software": software, "mode": format!("{mode:#04x}"),
                "command": duty, "output": output, "rpm": rpm,
            }));
        }
        Ok(serde_json::json!({ "headers": rows }))
    }

    /// The same for an ITE chip (2.4): each PWM output's control register (bit 7: the chip's own
    /// curve), duty and the speed of the fan input with the same number (usually, not always, the
    /// header's own), with the fan main control and fan control registers.
    fn snapshot_ite(chip: &Chip, board: Option<&Board>) -> Result<serde_json::Value, Error> {
        let (base, banks, n) = (chip.found.base, chip.banks(), chip.outputs());
        let dump = chip.control(|io| control_ite::dump(io, base, banks, n))?;
        let fan_regs: Vec<u8> = ite::FAN_LOW.iter().chain(&ite::FAN_HIGH).copied().collect();
        let fans = chip
            .lock
            .hold(LOCK_MS, || {
                ite::read(&mut sio::Ports(&chip.io), base, banks, &fan_regs)
            })
            .ok_or("ISA bus busy")??;
        let rpm = |i: usize| {
            let r = |a: u8| fans.iter().find(|x| x.0 == a).map(|x| x.1);
            ite::rpm(u16::from_le_bytes([
                r(ite::FAN_LOW[i])?,
                r(ite::FAN_HIGH[i])?,
            ]))
        };
        println!(
            "  fan main control {:#04x}, fan control {:#04x}",
            dump.main, dump.fan_ctl
        );
        let mut rows = Vec::new();
        println!("  pwm  name            control         duty        RPM");
        for p in &dump.outputs {
            let name = name(board, p.index);
            let rpm = rpm(usize::from(p.index));
            println!(
                "  {:<4} {:<15} {:<15} {:>5.1} %   {:>6}",
                p.index,
                name,
                if p.software {
                    format!("software ({:#04x})", p.ctrl)
                } else {
                    format!("BIOS ({:#04x})", p.ctrl)
                },
                control::percent(p.duty),
                rpm.map_or("-".to_owned(), |r| format!("{r:.0}")),
            );
            rows.push(serde_json::json!({
                "header": p.index, "name": name, "software": p.software,
                "control": format!("{:#04x}", p.ctrl), "duty": p.duty, "rpm": rpm,
            }));
        }
        Ok(serde_json::json!({
            "fanMain": format!("{:#04x}", dump.main),
            "fanCtl": format!("{:#04x}", dump.fan_ctl),
            "headers": rows,
        }))
    }

    fn exe_dir() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn restore_path() -> PathBuf {
        exe_dir().join("clokrix-fans-restore.json")
    }

    fn restore(chip: &Chip, state: &BiosState) -> Result<(), Error> {
        chip.restore(state)?;
        std::fs::remove_file(restore_path())?;
        println!("BIOS control restored.");
        Ok(())
    }

    fn try_duty(chip: &Chip, board: &Board, header: u16, pct: f64) -> Result<(), Error> {
        let h = board
            .headers
            .iter()
            .find(|h| h.index == header)
            .ok_or_else(|| format!("header {header} is not controllable on {}", board.name))?;
        println!(
            "\nTest: {} (header {header}) at {:.0} % for 10 s, then back to the BIOS.",
            h.name,
            control::percent(control::duty(h, pct)),
        );
        println!("If this window is closed early, run `clokrix-fans --restore`.\n");
        let now = u64::try_from(clokrix_core::unix_now()).unwrap_or(0);
        let state = chip.capture(now)?;
        control::save(&restore_path(), &state)?;
        let result = chip.set(board, header, pct);
        let mut log = Vec::new();
        if let Err(e) = &result {
            println!("Write failed ({e}); restoring.");
        } else {
            for s in 1..=10 {
                std::thread::sleep(Duration::from_secs(1));
                println!("after {s} s:");
                log.push(snapshot(chip, Some(board))?);
            }
        }
        restore(chip, &state)?;
        std::thread::sleep(Duration::from_secs(3));
        println!("3 s after restoring:");
        log.push(snapshot(chip, Some(board))?);
        result?;
        let out = exe_dir().join("clokrix-fans-try.json");
        std::fs::write(
            &out,
            serde_json::to_string_pretty(&serde_json::json!({
                "header": header, "percent": pct, "bios": state, "samples": log,
            }))?,
        )?;
        println!("\nSaved {}", out.display());
        Ok(())
    }

    pub fn run(args: &[String]) -> Result<(), Error> {
        let chip = Chip::open()?;
        let hw = clokrix_core::hwid::collect();
        let (maker, product) = (
            hw.board.manufacturer.as_deref(),
            hw.board.product.as_deref(),
        );
        let f = chip.found;
        println!(
            "{} (id {:#04x} rev {:#04x}) at {:#x}, base {:#06x}",
            f.chip.name(),
            f.id,
            f.rev,
            f.config_port,
            f.base,
        );
        println!("Board: {} {}", maker.unwrap_or("?"), product.unwrap_or("?"));
        let board = control::capability(f.chip, f.rev, maker, product);
        // Before a NCT679x or ITE board's field check, only this tool's supervised test writes it.
        let candidate = control::candidate(f.chip, f.rev, maker);
        match (&board, &candidate) {
            (Ok(b), _) => println!("Fan control: allowed on {} (validation pending)", b.name),
            (Err(r), Some(_)) => println!(
                "Fan control: read-only in the app ({}); --try can test one output at a time",
                r.reason()
            ),
            (Err(r), None) => println!("Fan control: read-only ({})", r.reason()),
        }
        let tryable: Option<Board> = board.ok().copied().or(candidate);

        if args.iter().any(|a| a == "--restore") {
            let state = control::load(&restore_path())?.ok_or("nothing to restore")?;
            return restore(&chip, &state);
        }
        if let Some(i) = args.iter().position(|a| a == "--try") {
            let header: u16 = args
                .get(i + 1)
                .and_then(|a| a.parse().ok())
                .ok_or("--try HEADER PERCENT")?;
            let pct: f64 = args
                .get(i + 2)
                .and_then(|a| a.parse().ok())
                .ok_or("--try HEADER PERCENT")?;
            if let Some(state) = control::load(&restore_path())? {
                println!("A previous test did not finish; restoring it first.");
                restore(&chip, &state)?;
            }
            let board = tryable.ok_or("fan control is not validated on this motherboard")?;
            return try_duty(&chip, &board, header, pct);
        }

        // Read only: registers and three samples a second apart (the BIOS curve at work).
        let regs = if chip.ite() {
            let mut regs = vec![control_ite::MAIN, control_ite::FAN_CTL];
            regs.extend(control_ite::CTRL.iter().chain(&control_ite::DUTY));
            regs.sort_unstable();
            let r = chip
                .lock
                .hold(LOCK_MS, || {
                    ite::read(&mut sio::Ports(&chip.io), f.base, chip.banks(), &regs)
                })
                .ok_or("ISA bus busy")??;
            println!("\nPWM registers (bank 0):");
            for (a, v) in &r {
                println!("  {a:#04x} = {v:#04x}");
            }
            serde_json::json!({
                "pwm": r.iter().map(|(a, v)| (format!("{a:#04x}"), serde_json::Value::from(format!("{v:#04x}")))).collect::<serde_json::Map<_, _>>(),
            })
        } else if chip.banked() {
            let mut regs: Vec<u16> = control679x::MODE
                .iter()
                .chain(&control679x::DUTY)
                .chain(&control679x::OUTPUT)
                .copied()
                .collect();
            regs.sort_unstable();
            regs.dedup();
            let r = chip.read_banked(&regs)?;
            println!("\nPWM registers:");
            for (a, v) in &r {
                println!("  {a:#05x} = {v:#04x}");
            }
            serde_json::json!({
                "pwm": r.iter().map(|(a, v)| (format!("{a:#05x}"), serde_json::Value::from(format!("{v:#04x}")))).collect::<serde_json::Map<_, _>>(),
            })
        } else {
            let regs = serde_json::json!({
                "controlA00": hex(&chip.read(0xA00, 0x40)?),
                "output160": hex(&chip.read(control::DUTY_OUTPUT, 0x10)?),
                "fans140": hex(&chip.read(FAN_BASE, 0x20)?),
            });
            println!("\nEC 0xA00-0xA3F:");
            for row in regs["controlA00"].as_array().into_iter().flatten() {
                println!("  {}", row.as_str().unwrap_or(""));
            }
            regs
        };
        let mut samples = Vec::new();
        for s in 0..3 {
            if s > 0 {
                std::thread::sleep(Duration::from_secs(1));
            }
            println!();
            samples.push(snapshot(&chip, tryable.as_ref())?);
        }
        let out = exe_dir().join("clokrix-fans.json");
        let json = serde_json::json!({
            "chip": f.chip.name(), "id": format!("{:#04x}", f.id), "rev": format!("{:#04x}", f.rev),
            "base": format!("{:#06x}", f.base), "boardMaker": maker, "boardProduct": product,
            "allowed": board.is_ok(), "tryable": tryable.is_some(),
            "registers": regs, "samples": samples,
        });
        std::fs::write(&out, serde_json::to_string_pretty(&json)?)?;
        println!(
            "\nSaved {} (nothing was written to the chip)",
            out.display()
        );
        Ok(())
    }

    pub fn wait(args: &[String]) {
        if !args.iter().any(|a| a == "--no-wait") && std::io::stdin().is_terminal() {
            println!("\nPress Enter to close.");
            let _ = std::io::stdin().lock().lines().next();
        }
    }
}

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Err(e) = win::run(&args) {
        println!("Error: {e}");
    }
    win::wait(&args);
}

#[cfg(not(windows))]
fn main() {
    println!("clokrix-fans runs on Windows only.");
}
