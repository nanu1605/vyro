//! Fan-control check for Phase 12a, run as administrator with PawnIO installed.
//!
//! ```sh
//! vyro-fans                 # read only: control registers, each header's mode / duty / RPM
//! vyro-fans --try 2 50      # supervised test: header 2 at 50 % for 10 s, then back to the BIOS
//! vyro-fans --restore       # hand every header back to the BIOS from vyro-fans-restore.json
//! ```
//!
//! The default run writes nothing. `--try` works only on an allow-listed board and header; it
//! saves the BIOS state next to the executable before writing and restores it afterwards. Quit
//! MSI Center / FanControl first: two programs driving the same chip fight each other.

#[cfg(windows)]
mod win {
    use std::io::{BufRead, IsTerminal};
    use std::path::PathBuf;
    use std::time::Duration;

    use vyro_core::pawnio::{BusLock, Handle, Module, PawnIoError, MUTEX_ISABUS};
    use vyro_core::sio::control::{self, BiosState, Board, ControlError};
    use vyro_core::sio::{self, Found, FAN_BASE};

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
            Ok(Self { io, lock, found })
        }

        /// One locked access: reads `len` EC bytes from `start`.
        fn read(&self, start: u16, len: u16) -> Result<Vec<u8>, Error> {
            let r = self
                .lock
                .hold(LOCK_MS, || {
                    sio::read_block(&self.io, self.found.base, start, len)
                })
                .ok_or("ISA bus busy")??;
            Ok(r.ok_or("another program is using the fan controller")?)
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
    }

    /// Mode, duty command, duty output and RPM of each header, as a table and as JSON.
    fn snapshot(chip: &Chip, board: Option<&Board>) -> Result<serde_json::Value, Error> {
        let mode = chip.read(control::CTRL_MODE, 1)?[0];
        let commands = chip.read(control::DUTY_COMMAND, control::HEADERS)?;
        let outputs = chip.read(control::DUTY_OUTPUT, control::HEADERS)?;
        let fans = chip.read(FAN_BASE, control::HEADERS * 2)?;
        let mut rows = Vec::new();
        println!("  hdr  name            mode      command   output      RPM");
        for h in control::headers(mode, &commands, &outputs) {
            let i = usize::from(h.index) * 2;
            let rpm = sio::rpm(fans[i], fans[i + 1]);
            let name = board
                .and_then(|b| b.headers.iter().find(|x| x.index == h.index))
                .map_or("-", |x| x.name);
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

    fn exe_dir() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn restore_path() -> PathBuf {
        exe_dir().join("vyro-fans-restore.json")
    }

    fn restore(chip: &Chip, state: &BiosState) -> Result<(), Error> {
        chip.control(|io| control::restore(io, chip.found.base, state))?;
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
        println!("If this window is closed early, run `vyro-fans --restore`.\n");
        let now = u64::try_from(vyro_core::unix_now()).unwrap_or(0);
        let (id, rev, base) = (chip.found.id, chip.found.rev, chip.found.base);
        let state = chip.control(|io| control::capture(io, base, id, rev, now))?;
        control::save(&restore_path(), &state)?;
        let result = chip.control(|io| control::set_duty(io, base, board, header, pct));
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
        let out = exe_dir().join("vyro-fans-try.json");
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
        let hw = vyro_core::hwid::collect();
        let (maker, product) = (
            hw.board.manufacturer.as_deref(),
            hw.board.product.as_deref(),
        );
        let f = chip.found;
        println!(
            "{} (id {:#04x} rev {:#04x}) at {:#x}, EC base {:#06x}",
            f.chip.name(),
            f.id,
            f.rev,
            f.config_port,
            f.base,
        );
        println!("Board: {} {}", maker.unwrap_or("?"), product.unwrap_or("?"));
        let board = control::capability(f.chip, f.rev, maker, product);
        match board {
            Ok(b) => println!("Fan control: allowed on {} (validation pending)", b.name),
            Err(r) => println!("Fan control: read-only ({})", r.reason()),
        }

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
            let board = board.map_err(|r| r.reason())?;
            return try_duty(&chip, board, header, pct);
        }

        // Read only: registers and three samples a second apart (the BIOS curve at work).
        let regs = serde_json::json!({
            "controlA00": hex(&chip.read(0xA00, 0x40)?),
            "output160": hex(&chip.read(control::DUTY_OUTPUT, 0x10)?),
            "fans140": hex(&chip.read(FAN_BASE, 0x20)?),
        });
        println!("\nEC 0xA00-0xA3F:");
        for row in regs["controlA00"].as_array().into_iter().flatten() {
            println!("  {}", row.as_str().unwrap_or(""));
        }
        let mut samples = Vec::new();
        for s in 0..3 {
            if s > 0 {
                std::thread::sleep(Duration::from_secs(1));
            }
            println!();
            samples.push(snapshot(&chip, board.ok())?);
        }
        let out = exe_dir().join("vyro-fans.json");
        let json = serde_json::json!({
            "chip": f.chip.name(), "id": format!("{:#04x}", f.id), "rev": format!("{:#04x}", f.rev),
            "base": format!("{:#06x}", f.base), "boardMaker": maker, "boardProduct": product,
            "allowed": board.is_ok(), "registers": regs, "samples": samples,
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
    println!("vyro-fans runs on Windows only.");
}
