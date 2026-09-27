//! Runs a frame source (PresentMon, or the simulator) for one process and feeds its frames to
//! a callback on a reader thread.

use std::io::{self, BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use super::csv::{Frame, Header};
use crate::lock;

/// A started frame source: CSV text to read, and how to stop it.
pub struct Source {
    pub reader: Box<dyn Read + Send>,
    /// Stops the source; the reader then reaches its end.
    pub stop: Box<dyn FnOnce() + Send>,
    /// Why the source ended, asked once the reader has ended (e.g. PresentMon's error output).
    pub failure: Box<dyn FnOnce() -> Option<String> + Send>,
}

/// Starts a frame source for a process id.
pub type Launch = Arc<dyn Fn(u32) -> io::Result<Source> + Send + Sync>;

/// How a capture ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum End {
    /// Stopped on request, or the process exited after presenting frames.
    Done,
    /// Ended before producing frames: the reason, for the UI.
    Failed(String),
}

/// A running capture; stopping or dropping it ends the source.
pub struct Capture {
    stop: Option<Box<dyn FnOnce() + Send>>,
    stopped: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Capture {
    /// Starts `launch` for `pid`; `on_frame` gets every parsed frame, `on_end` runs once when
    /// the source ends by itself (not after [`Capture::stop`]).
    pub fn start(
        launch: &Launch,
        pid: u32,
        on_frame: impl Fn(Frame) + Send + 'static,
        on_end: impl FnOnce(End) + Send + 'static,
    ) -> io::Result<Self> {
        let source = launch(pid)?;
        let stopped = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stopped);
        let reader = source.reader;
        let failure = source.failure;
        let thread = std::thread::Builder::new()
            .name("vyro-frames".into())
            .spawn(move || {
                let (frames, junk) = read(reader, &on_frame);
                if flag.load(Ordering::SeqCst) {
                    return;
                }
                let end = if frames > 0 {
                    End::Done
                } else {
                    End::Failed(
                        failure()
                            .or(junk)
                            .unwrap_or_else(|| "frame capture ended without data".into()),
                    )
                };
                on_end(end);
            })?;
        Ok(Self {
            stop: Some(source.stop),
            stopped,
            thread: Some(thread),
        })
    }

    pub fn stop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(stop) = self.stop.take() {
            stop();
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Reads the header, then frames. Returns the frame count and, if no header came, the first
/// line of other output (an error message, usually).
fn read(reader: Box<dyn Read + Send>, on_frame: &dyn Fn(Frame)) -> (u64, Option<String>) {
    let mut reader = BufReader::new(reader);
    // PresentMon writes UTF-16 when its output is a console or a file; through a pipe it is
    // normally 8-bit. Accept both.
    let utf16 = reader
        .fill_buf()
        .map(|b| b.starts_with(&[0xFF, 0xFE]) || (b.len() >= 2 && b[0] != 0 && b[1] == 0))
        .unwrap_or(false);
    let mut reader: Box<dyn BufRead> = if utf16 {
        Box::new(BufReader::new(Utf16Reader {
            inner: reader,
            out: Vec::new(),
            pos: 0,
        }))
    } else {
        Box::new(reader)
    };
    let mut header: Option<Header> = None;
    let mut junk: Option<String> = None;
    let mut frames = 0;
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match &header {
            Some(h) => {
                if let Some(f) = h.row(line) {
                    frames += 1;
                    on_frame(f);
                }
            }
            None => {
                header = Header::parse(line);
                if header.is_none() && junk.is_none() {
                    junk = Some(line.chars().take(200).collect());
                }
            }
        }
    }
    (frames, junk)
}

/// Re-encodes UTF-16LE text (BOM dropped) as UTF-8.
struct Utf16Reader<R> {
    inner: R,
    out: Vec<u8>,
    pos: usize,
}

impl<R: Read> Read for Utf16Reader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.out.len() {
            let mut raw = [0u8; 4096];
            let mut n = self.inner.read(&mut raw)?;
            if n == 0 {
                return Ok(0);
            }
            if n % 2 == 1 {
                // Keep code units whole.
                n += self.inner.read(&mut raw[n..=n])?;
            }
            self.out = utf16le(&raw[..n - n % 2]).into_bytes();
            self.pos = 0;
        }
        let n = buf.len().min(self.out.len() - self.pos);
        buf[..n].copy_from_slice(&self.out[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// UTF-16LE code units to text, without byte-order marks.
fn utf16le(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&c| u16::from_le_bytes(c))
        .filter(|&u| u != 0xFEFF)
        .collect();
    String::from_utf16_lossy(&units)
}

/// Text in either of PresentMon's encodings (UTF-16LE with a BOM, or 8-bit).
pub fn decode(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) || (bytes.len() >= 2 && bytes[0] != 0 && bytes[1] == 0) {
        utf16le(bytes)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// PresentMon's ETW session name; one capture at a time.
pub const SESSION: &str = "VyroCapture";

/// Runs the bundled PresentMon console app (`PresentMon.exe` beside the running executable,
/// or `VYRO_PRESENTMON`).
pub fn presentmon() -> Launch {
    Arc::new(|pid| {
        let exe = presentmon_path()?;
        let mut cmd = std::process::Command::new(&exe);
        cmd.args([
            "--process_id",
            &pid.to_string(),
            "--output_stdout",
            "--v2_metrics",
            "--no_track_input",
            "--stop_existing_session",
            "--session_name",
            SESSION,
            "--terminate_on_proc_exit",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
        hide_window(&mut cmd);
        let mut child = cmd.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("no output"))?;
        let errors = Arc::new(Mutex::new(String::new()));
        if let Some(mut stderr) = child.stderr.take() {
            let errors = Arc::clone(&errors);
            let _ = std::thread::Builder::new()
                .name("vyro-frames-err".into())
                .spawn(move || {
                    let mut bytes = Vec::new();
                    let _ = stderr.read_to_end(&mut bytes);
                    *lock(&errors) = decode(&bytes).chars().take(2000).collect();
                });
        }
        let child = Arc::new(Mutex::new(child));
        let stop_child = Arc::clone(&child);
        Ok(Source {
            reader: Box::new(stdout),
            stop: Box::new(move || {
                let _ = lock(&stop_child).kill();
                let _ = lock(&stop_child).wait();
                // A killed PresentMon leaves its ETW session running; end it.
                let mut end = std::process::Command::new(&exe);
                end.args(["--terminate_existing_session", "--session_name", SESSION])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                hide_window(&mut end);
                let _ = end.status();
            }),
            failure: Box::new(move || {
                let status = lock(&child).wait().ok();
                // Give the error reader a moment to finish.
                std::thread::sleep(Duration::from_millis(100));
                let text = lock(&errors).replace("\r\n", "\n");
                // "error: failed to start trace session: access denied." plus an indented
                // explanation: one line of text.
                let first = text
                    .split("\n\n")
                    .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
                    .find(|p| !p.is_empty() && !p.starts_with("warning"));
                first.or_else(|| {
                    status
                        .filter(|s| !s.success())
                        .map(|s| format!("PresentMon exited with {s}"))
                })
            }),
        })
    })
}

fn presentmon_path() -> io::Result<std::path::PathBuf> {
    if let Some(p) = std::env::var_os("VYRO_PRESENTMON") {
        return Ok(p.into());
    }
    let dir = std::env::current_exe()?
        .parent()
        .map(std::path::Path::to_path_buf)
        .ok_or_else(|| io::Error::other("no program folder"))?;
    let exe = dir.join("PresentMon.exe");
    if exe.is_file() {
        Ok(exe)
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "PresentMon.exe is not installed next to Vyro; reinstall Vyro",
        ))
    }
}

#[cfg(windows)]
fn hide_window(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_window(_: &mut std::process::Command) {}

/// Simulated frames: a game running near 144 FPS with an occasional hitch, streamed in real
/// time (for the simulator, the browser-free UI checks and tests).
pub fn simulated(name: &'static str) -> Launch {
    Arc::new(move |pid| {
        let stop = Arc::new(AtomicBool::new(false));
        Ok(Source {
            reader: Box::new(SimFrames {
                name,
                pid,
                at: 0.0,
                n: 0,
                buf: Vec::new(),
                pos: 0,
                stop: Arc::clone(&stop),
                header: false,
            }),
            stop: Box::new(move || stop.store(true, Ordering::SeqCst)),
            failure: Box::new(|| None),
        })
    })
}

struct SimFrames {
    name: &'static str,
    pid: u32,
    at: f64,
    n: u64,
    buf: Vec<u8>,
    pos: usize,
    stop: Arc<AtomicBool>,
    header: bool,
}

/// Simulated frames are produced in 50 ms batches.
const SIM_BATCH_MS: f64 = 50.0;

impl Read for SimFrames {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.buf.len() {
            if self.stop.load(Ordering::SeqCst) {
                return Ok(0);
            }
            self.buf.clear();
            self.pos = 0;
            if !self.header {
                self.header = true;
                self.buf.extend_from_slice(
                    b"Application,ProcessID,SwapChainAddress,PresentRuntime,SyncInterval,PresentFlags,AllowsTearing,PresentMode,CPUStartTime,FrameTime,CPUBusy,CPUWait,GPULatency,GPUTime,GPUBusy,GPUWait,DisplayLatency,DisplayedTime\r\n",
                );
            } else {
                std::thread::sleep(Duration::from_millis(SIM_BATCH_MS as u64));
                let end = self.at + SIM_BATCH_MS;
                while self.at < end {
                    self.n += 1;
                    let wobble = ((self.n as f64) * 0.37).sin() * 0.35;
                    let ms = if self.n.is_multiple_of(900) {
                        24.0
                    } else {
                        6.9 + wobble
                    };
                    let gpu = ms * 0.93;
                    let cpu = ms * 0.55;
                    let line = format!(
                        "{},{},0x1,DXGI,0,0,1,Hardware: Independent Flip,{:.3},{ms:.3},{cpu:.3},{:.3},0.4,{gpu:.3},{gpu:.3},{:.3},{:.3},{ms:.3}\r\n",
                        self.name,
                        self.pid,
                        self.at,
                        ms - cpu,
                        ms - gpu,
                        ms + 2.5,
                    );
                    self.buf.extend_from_slice(line.as_bytes());
                    self.at += ms;
                }
            }
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn text_source(text: &'static str, failure: Option<&'static str>) -> Launch {
        Arc::new(move |_| {
            Ok(Source {
                reader: Box::new(text.as_bytes()),
                stop: Box::new(|| {}),
                failure: Box::new(move || failure.map(str::to_owned)),
            })
        })
    }

    #[test]
    fn feeds_frames_then_reports_the_end() {
        let (tx, rx) = mpsc::channel();
        let (etx, erx) = mpsc::channel();
        let mut c = Capture::start(
            &text_source(super::super::csv::SAMPLE, None),
            4242,
            move |f| tx.send(f).unwrap(),
            move |e| etx.send(e).unwrap(),
        )
        .unwrap();
        assert_eq!(erx.recv_timeout(Duration::from_secs(5)).unwrap(), End::Done);
        assert_eq!(rx.try_iter().count(), 2);
        c.stop();
    }

    #[test]
    fn a_source_without_frames_fails_with_its_message() {
        let (etx, erx) = mpsc::channel();
        let _c = Capture::start(
            &text_source("error: access denied\r\n", None),
            1,
            |_| {},
            move |e| etx.send(e).unwrap(),
        )
        .unwrap();
        assert_eq!(
            erx.recv_timeout(Duration::from_secs(5)).unwrap(),
            End::Failed("error: access denied".into())
        );
        let (etx, erx) = mpsc::channel();
        let _c = Capture::start(
            &text_source("", Some("PresentMon requires elevated privilege")),
            1,
            |_| {},
            move |e| etx.send(e).unwrap(),
        )
        .unwrap();
        assert_eq!(
            erx.recv_timeout(Duration::from_secs(5)).unwrap(),
            End::Failed("PresentMon requires elevated privilege".into())
        );
    }

    #[test]
    fn simulated_frames_stream_until_stopped() {
        let (tx, rx) = mpsc::channel();
        let mut c = Capture::start(
            &simulated("sim.exe"),
            9,
            move |f| {
                let _ = tx.send(f);
            },
            |_| panic!("stopped captures do not report an end"),
        )
        .unwrap();
        let f = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!((f.app.as_str(), f.pid), ("sim.exe", 9));
        c.stop();
    }

    #[test]
    fn reads_utf16_output() {
        let text = format!("\u{FEFF}{}", super::super::csv::SAMPLE);
        let bytes: &'static [u8] = Box::leak(
            text.encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<u8>>()
                .into_boxed_slice(),
        );
        let (tx, rx) = mpsc::channel();
        let (etx, erx) = mpsc::channel();
        let launch: Launch = Arc::new(move |_| {
            Ok(Source {
                reader: Box::new(bytes),
                stop: Box::new(|| {}),
                failure: Box::new(|| None),
            })
        });
        let _c = Capture::start(
            &launch,
            1,
            move |f| tx.send(f).unwrap(),
            move |e| {
                etx.send(e).unwrap();
            },
        )
        .unwrap();
        assert_eq!(erx.recv_timeout(Duration::from_secs(5)).unwrap(), End::Done);
        assert_eq!(rx.try_iter().count(), 2);

        let err = "\u{FEFF}error: failed to start trace session: access denied.\r\n       PresentMon requires either administrative privileges.\r\n";
        let err: Vec<u8> = err.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert!(decode(&err).starts_with("error: failed to start trace session"));
        assert_eq!(decode(b"plain"), "plain");
    }

    #[test]
    fn missing_presentmon_is_an_error() {
        std::env::set_var("VYRO_PRESENTMON", "/nonexistent/PresentMon.exe");
        assert!(presentmon()(1).is_err());
        std::env::remove_var("VYRO_PRESENTMON");
    }
}
