//! Reads that can hang (1.4.1 field check): a read of the device list asks Windows, and through
//! it the devices themselves, and one that does not answer held every later read of the list,
//! so the Devices tab never showed it. Each part is read on a thread of its own
//! ([`Bounded::start`]), all at once, and waited for a while ([`Bounded::wait`]); a read still
//! going is never started twice, and its answer is used once it comes.

use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// A read that takes longer than this is logged, with how long it took.
const SLOW: Duration = Duration::from_secs(2);

/// One kind of read: its latest answer and whether one is going.
pub struct Bounded<T> {
    /// What is read, for the log.
    what: &'static str,
    state: Mutex<State<T>>,
    done: Condvar,
}

struct State<T> {
    /// The latest answer; none before the first.
    last: Option<T>,
    /// When the read going now started.
    running: Option<Instant>,
    /// Reads finished so far.
    finished: u64,
    /// The read going now was already waited for in vain.
    late: bool,
}

/// The read a caller waits for: done once this many reads have finished.
#[derive(Debug, Clone, Copy)]
pub struct Ticket(u64);

impl<T: Clone + Default + Send + 'static> Bounded<T> {
    pub const fn new(what: &'static str) -> Self {
        Self {
            what,
            state: Mutex::new(State {
                last: None,
                running: None,
                finished: 0,
                late: false,
            }),
            done: Condvar::new(),
        }
    }

    /// Reads with `read` on its own thread, unless a read is already going: then that one
    /// counts.
    pub fn start(&'static self, read: impl FnOnce() -> T + Send + 'static) -> Ticket {
        let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if s.running.is_some() {
            return Ticket(s.finished + 1);
        }
        let spawned = std::thread::Builder::new()
            .name("vyro-device-read".into())
            .spawn(move || {
                let v = read();
                let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(at) = s.running {
                    let took = at.elapsed();
                    if s.late {
                        log::info!(
                            "devices: {} answered after {:.1} s",
                            self.what,
                            took.as_secs_f32()
                        );
                    } else if took > SLOW {
                        log::info!("devices: {} took {:.1} s", self.what, took.as_secs_f32());
                    }
                }
                s.last = Some(v);
                s.running = None;
                s.late = false;
                s.finished += 1;
                self.done.notify_all();
            });
        match spawned {
            Ok(_) => {
                s.running = Some(Instant::now());
                Ticket(s.finished + 1)
            }
            Err(e) => {
                log::warn!("devices: {} not read: {e}", self.what);
                Ticket(s.finished)
            }
        }
    }

    /// The answer of the read `ticket` names, waiting for it until `until`; else the answer
    /// before it (empty before any). A read already waited for in vain is not waited for again,
    /// so while it hangs each list goes on at once.
    pub fn wait(&self, ticket: Ticket, until: Instant) -> T {
        let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        while s.finished < ticket.0 && !s.late {
            let Some(left) = until.checked_duration_since(Instant::now()) else {
                s.late = true;
                log::info!(
                    "devices: {} has not answered in {:.1} s; the list goes on without it",
                    self.what,
                    s.running.map_or(0.0, |at| at.elapsed().as_secs_f32())
                );
                break;
            };
            s = self
                .done
                .wait_timeout(s, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        s.last.clone().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    fn run<T: Clone + Default + Send + 'static>(
        read: &'static Bounded<T>,
        wait: Duration,
        f: impl FnOnce() -> T + Send + 'static,
    ) -> T {
        let ticket = read.start(f);
        read.wait(ticket, Instant::now() + wait)
    }

    #[test]
    fn a_read_that_hangs_holds_nothing_up_and_is_not_started_twice() {
        static READ: Bounded<Vec<u8>> = Bounded::new("a test read");
        static STARTED: AtomicUsize = AtomicUsize::new(0);
        let wait = Duration::from_millis(100);

        // A read that answers at once.
        assert_eq!(run(&READ, wait, || vec![1]), [1]);

        // One that hangs: the caller gets the answer before it, in about `wait`.
        let (release, held) = mpsc::channel::<()>();
        let start = Instant::now();
        let got = run(&READ, wait, move || {
            STARTED.fetch_add(1, Ordering::SeqCst);
            held.recv().ok();
            vec![2]
        });
        assert_eq!(got, [1]);
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "{:?}",
            start.elapsed()
        );

        // While it hangs, a later call starts nothing, does not wait for it again and goes on
        // with the last answer.
        let start = Instant::now();
        assert_eq!(
            run(&READ, Duration::from_secs(5), || unreachable!(
                "a second read"
            )),
            [1]
        );
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "{:?}",
            start.elapsed()
        );
        assert_eq!(STARTED.load(Ordering::SeqCst), 1);

        // Once it answers, its answer counts, and the next call reads again.
        release.send(()).unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while READ.state.lock().unwrap().running.is_some() && Instant::now() < until {
            std::thread::yield_now();
        }
        assert_eq!(READ.state.lock().unwrap().last.as_deref(), Some(&[2][..]));
        assert_eq!(run(&READ, wait, || vec![3]), [3]);
    }

    #[test]
    fn callers_at_the_same_time_share_one_read() {
        static READ: Bounded<u32> = Bounded::new("a shared read");
        static STARTED: AtomicUsize = AtomicUsize::new(0);
        let slow = || {
            STARTED.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(200));
            7
        };
        let other = std::thread::spawn(move || run(&READ, Duration::from_secs(5), slow));
        while STARTED.load(Ordering::SeqCst) == 0 {
            std::thread::yield_now();
        }
        assert_eq!(run(&READ, Duration::from_secs(5), || 8), 7);
        assert_eq!(other.join().unwrap(), 7);
        assert_eq!(STARTED.load(Ordering::SeqCst), 1);
    }

    /// Reads started together run side by side: the list waits for the slowest, not the sum.
    #[test]
    fn reads_started_together_run_side_by_side() {
        static A: Bounded<u8> = Bounded::new("read a");
        static B: Bounded<u8> = Bounded::new("read b");
        let nap = Duration::from_millis(300);
        let start = Instant::now();
        let a = A.start(move || {
            std::thread::sleep(nap);
            1
        });
        let b = B.start(move || {
            std::thread::sleep(nap);
            2
        });
        let until = Instant::now() + Duration::from_secs(5);
        assert_eq!((A.wait(a, until), B.wait(b, until)), (1, 2));
        assert!(
            start.elapsed() < nap * 2,
            "{:?} for two reads of {nap:?}",
            start.elapsed()
        );
    }
}
