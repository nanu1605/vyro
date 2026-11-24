//! Work on a thread of its own, one piece at a time, so a slow piece never holds up the poller's
//! tick: every snapshot, and every reading in it, waits for each provider's poll. The owner's
//! log (2026-11-21) had the drive scan, every 5 s, at about 390 ms (327 to 1,312 ms) on a PC
//! with two NVMe drives and a spinning disk, so every fifth update came that much late; the
//! AMD CPU's reads at about 340 ms (up to 1,217 ms) and the network's up to 2,782 ms.
//!
//! [`Background`] is for work done every few seconds, used when it comes in. [`Sampler`] is for
//! a reading wanted at every tick: the tick waits a little for it, and one that takes longer is
//! used at the next tick instead.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
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

    /// As [`Self::take`], waiting up to `wait` for the piece still going.
    pub fn take_within(&mut self, wait: Duration) -> Option<Result<Res, String>> {
        if !self.busy {
            return None;
        }
        match self.from.recv_timeout(wait) {
            Ok(r) => {
                self.busy = false;
                Some(r)
            }
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => {
                self.busy = false;
                None
            }
        }
    }
}

/// What a tick got from a [`Sampler`].
#[derive(Debug, PartialEq, Eq)]
pub enum Sample<Res> {
    /// The newest reading that came in. An earlier tick's late one is passed over when this
    /// tick's own came in too: counters then give their rates over the whole span since the
    /// reading used before, not over the few milliseconds between the two.
    Came(Result<Res, String>),
    /// None came in: how many ticks in a row, this one counted.
    Late(u32),
    /// The thread is gone.
    Gone,
}

/// A reading wanted at every tick, taken on a thread of its own: each tick waits a little for
/// its reading, so it is as fresh as one taken in the tick, and one that takes longer is used
/// at the next tick instead of holding up this one.
pub struct Sampler<Req, Res> {
    work: Background<Req, Res>,
    late: u32,
}

impl<Req: Send + 'static, Res: Send + 'static> Sampler<Req, Res> {
    /// As [`Background::start`].
    pub fn start(
        thread: &'static str,
        what: &'static str,
        work: impl FnMut(Req) -> Res + Send + 'static,
    ) -> std::io::Result<Self> {
        Ok(Self {
            work: Background::start(thread, what, work)?,
            late: 0,
        })
    }

    /// Starts this tick's reading with `piece` (unless an earlier one is still going) and waits
    /// up to `wait` for it.
    pub fn tick(&mut self, piece: Req, wait: Duration) -> Sample<Res> {
        let late = self.work.take();
        let (own, gone) = match self.work.ask(piece) {
            // Busy: the earlier one is still going, and is this tick's to wait for.
            Asked::Started | Asked::Busy => (self.work.take_within(wait), false),
            Asked::Gone(_) => (None, true),
        };
        match own.or(late) {
            Some(r) => {
                self.late = 0;
                Sample::Came(r)
            }
            None if gone => Sample::Gone,
            None => {
                self.late += 1;
                Sample::Late(self.late)
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

    #[test]
    fn take_within_waits_for_the_piece_going_only() {
        let mut b = Background::start("test-within", "test", |n: u32| n + 1).unwrap();
        let asked = Instant::now();
        assert_eq!(
            b.take_within(Duration::from_secs(10)),
            None,
            "nothing asked"
        );
        assert!(
            asked.elapsed() < Duration::from_secs(5),
            "no wait for nothing"
        );
        assert_eq!(b.ask(1), Asked::Started);
        assert_eq!(b.take_within(Duration::from_secs(10)), Some(Ok(2)));
        assert_eq!(b.ask(2), Asked::Started, "free once taken");
    }

    #[test]
    fn a_quick_reading_comes_in_its_own_tick() {
        let mut s = Sampler::start("test-quick", "test", |n: u32| n * 10).unwrap();
        // The wait ends as the reading comes in, so a long one costs nothing here.
        for n in 1..=3 {
            assert_eq!(s.tick(n, Duration::from_secs(10)), Sample::Came(Ok(n * 10)));
        }
    }

    #[test]
    fn a_slow_reading_is_used_at_a_later_tick() {
        let (go, gate) = mpsc::channel::<()>();
        let mut s = Sampler::start("test-late", "test", move |n: u32| {
            if n == 1 || n == 5 {
                gate.recv().unwrap();
            }
            n
        })
        .unwrap();
        let (short, long) = (Duration::from_millis(20), Duration::from_secs(10));
        let ticked = Instant::now();
        assert_eq!(s.tick(1, short), Sample::Late(1));
        assert_eq!(s.tick(2, short), Sample::Late(2), "one reading at a time");
        assert!(
            ticked.elapsed() < Duration::from_secs(5),
            "the ticks went on"
        );
        go.send(()).unwrap();
        // If the late reading came back before tick 3, tick 3 starts its own and passes the late
        // one over; if not, tick 3 waits for the late one, and tick 4 starts the next.
        match s.tick(3, long) {
            Sample::Came(Ok(3)) => {}
            Sample::Came(Ok(1)) => assert_eq!(s.tick(4, long), Sample::Came(Ok(4))),
            other => panic!("{other:?}"),
        }
        assert_eq!(s.tick(5, short), Sample::Late(1), "the count starts again");
        go.send(()).unwrap();
        match s.tick(6, long) {
            Sample::Came(Ok(6)) => {}
            Sample::Came(Ok(5)) => assert_eq!(s.tick(7, long), Sample::Came(Ok(7))),
            other => panic!("{other:?}"),
        }
    }
}
