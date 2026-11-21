//! Work on a thread of its own, one piece at a time, so a slow piece never holds up the poller's
//! tick: every snapshot, and every reading in it, waits for each provider's poll. The owner's
//! log (2026-11-21) had the drive scan, every 5 s, at about 390 ms (327 to 1,312 ms) on a PC
//! with two NVMe drives and a spinning disk, so every fifth update came that much late.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

/// A piece longer than this is logged, with how long it took.
const SLOW: Duration = Duration::from_millis(250);
/// At most one such line this often.
const SLOW_LOG_EVERY: Duration = Duration::from_secs(300);

/// What [`Background::ask`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum Asked<Req> {
    Started,
    /// A piece is still going; ask again once it is taken.
    Busy,
    /// The thread is gone: the piece is handed back, to be done some other way.
    Gone(Req),
}

pub struct Background<Req, Res> {
    to: Sender<Req>,
    from: Receiver<Result<Res, String>>,
    busy: bool,
}

impl<Req: Send + 'static, Res: Send + 'static> Background<Req, Res> {
    /// Starts the thread, named `thread`, that does each piece with `work`; `what` names a slow
    /// piece in the log.
    pub fn start(
        thread: &'static str,
        what: &'static str,
        mut work: impl FnMut(Req) -> Res + Send + 'static,
    ) -> std::io::Result<Self> {
        let (to, pieces) = mpsc::channel::<Req>();
        let (done, from) = mpsc::channel();
        std::thread::Builder::new()
            .name(thread.into())
            .spawn(move || {
                let mut slow_logged: Option<Instant> = None;
                for piece in pieces {
                    let started = Instant::now();
                    // A panic in one piece is that piece's failure, not the thread's end.
                    let r = catch_unwind(AssertUnwindSafe(|| work(piece))).map_err(|p| {
                        p.downcast_ref::<&str>()
                            .map(|s| (*s).to_owned())
                            .or_else(|| p.downcast_ref::<String>().cloned())
                            .unwrap_or_else(|| "panicked".into())
                    });
                    let took = started.elapsed();
                    if took > SLOW && slow_logged.is_none_or(|t| t.elapsed() >= SLOW_LOG_EVERY) {
                        log::info!("{what}: a pass took {} ms", took.as_millis());
                        slow_logged = Some(Instant::now());
                    }
                    if done.send(r).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            to,
            from,
            busy: false,
        })
    }

    /// Starts `piece`, unless one is still going.
    pub fn ask(&mut self, piece: Req) -> Asked<Req> {
        if self.busy {
            return Asked::Busy;
        }
        match self.to.send(piece) {
            Ok(()) => {
                self.busy = true;
                Asked::Started
            }
            Err(mpsc::SendError(piece)) => Asked::Gone(piece),
        }
    }

    /// The piece that finished since the last call, if one did (`Err`: it panicked).
    pub fn take(&mut self) -> Option<Result<Res, String>> {
        match self.from.try_recv() {
            Ok(r) => {
                self.busy = false;
                Some(r)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.busy = false;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    fn wait<Req: Send + 'static, Res: Send + 'static>(
        b: &mut Background<Req, Res>,
    ) -> Result<Res, String> {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(r) = b.take() {
                return r;
            }
            assert!(Instant::now() < until, "no answer in 10 s");
            std::thread::yield_now();
        }
    }

    #[test]
    fn a_slow_piece_does_not_hold_up_the_caller() {
        let gate = Arc::new(Barrier::new(2));
        let g = Arc::clone(&gate);
        let mut b = Background::start("test-slow", "test", move |n: u32| {
            g.wait();
            n * 2
        })
        .unwrap();
        let asked = Instant::now();
        assert_eq!(b.ask(21), Asked::Started);
        assert!(b.take().is_none(), "still going");
        assert_eq!(b.ask(5), Asked::Busy, "one piece at a time");
        assert!(
            asked.elapsed() < Duration::from_secs(1),
            "the caller went on"
        );
        gate.wait();
        assert_eq!(wait(&mut b), Ok(42));
        assert!(b.take().is_none(), "taken once");
        assert_eq!(b.ask(5), Asked::Started, "free again once taken");
        gate.wait();
        assert_eq!(wait(&mut b), Ok(10));
    }

    #[test]
    fn a_panic_fails_its_piece_only() {
        let mut b = Background::start("test-panic", "test", |n: u32| {
            assert!(n != 0, "no drive zero");
            n
        })
        .unwrap();
        assert_eq!(b.ask(0), Asked::Started);
        assert_eq!(wait(&mut b), Err("no drive zero".into()));
        assert_eq!(b.ask(3), Asked::Started, "the thread lives on");
        assert_eq!(wait(&mut b), Ok(3));
    }
}
