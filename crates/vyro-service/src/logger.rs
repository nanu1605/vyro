//! A small file logger: info and above, one line per record, rotated at [`MAX_BYTES`] with
//! one older file kept (`service.log.1`).

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use vyro_core::lock;

pub const MAX_BYTES: u64 = 1_000_000;

struct Sink {
    path: PathBuf,
    file: File,
    written: u64,
}

struct Logger {
    file: Mutex<Option<Sink>>,
    stderr: bool,
}

impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }

    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let line = format!(
            "{} [{}] {}: {}\n",
            vyro_core::unix_now(),
            r.level(),
            r.target(),
            r.args()
        );
        if self.stderr {
            eprint!("{line}");
        }
        let mut guard = lock(&self.file);
        let Some(sink) = guard.as_mut() else {
            return;
        };
        if sink.written + line.len() as u64 > MAX_BYTES {
            let old = sink.path.with_extension("log.1");
            let _ = std::fs::rename(&sink.path, old);
            match File::create(&sink.path) {
                Ok(f) => {
                    sink.file = f;
                    sink.written = 0;
                }
                Err(_) => {
                    *guard = None;
                    return;
                }
            }
        }
        if sink.file.write_all(line.as_bytes()).is_ok() {
            sink.written += line.len() as u64;
        }
    }

    fn flush(&self) {
        if let Some(sink) = lock(&self.file).as_mut() {
            let _ = sink.file.flush();
        }
    }
}

/// Installs the logger. Without a usable `path`, only `stderr` logging (if asked) remains.
pub fn init(path: Option<PathBuf>, stderr: bool) {
    let sink = path.and_then(|path| {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()?;
        let written = file.metadata().map_or(0, |m| m.len());
        Some(Sink {
            path,
            file,
            written,
        })
    });
    // Lives for the rest of the process, like any installed logger.
    let logger: &'static Logger = Box::leak(Box::new(Logger {
        file: Mutex::new(sink),
        stderr,
    }));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}
