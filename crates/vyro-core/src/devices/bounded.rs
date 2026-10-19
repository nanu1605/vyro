//! Reads that can hang (1.4.1 field check): a Bluetooth battery read waits on the device, and
//! one that does not answer could hold every later read of the list, so the Devices tab never
//! showed it. [`Bounded::run`] reads on a thread of its own and waits a while; a read still
//! going is never started twice, and its answer is used once it comes.

use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

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
    /// The read going now was already reported late.
    late: bool,
}

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

    /// Reads with `read` on its own thread, unless a read is already going (then that one
    /// counts), and waits at most `wait` for it. Gives the answer it waited for, or the one
    /// before while this one is late (empty before any).
    pub fn run(&'static self, wait: Duration, read: impl FnOnce() -> T + Send + 'static) -> T {
        let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let want = s.finished + 1;
        if s.running.is_none() {
            let spawned = std::thread::Builder::new()
                .name("vyro-device-read".into())
                .spawn(move || {
                    let v = read();
                    let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                    if s.late {
                        if let Some(at) = s.running {
                            log::info!(
                                "devices: {} answered after {:.1} s",
                                self.what,
                                at.elapsed().as_secs_f32()
                            );
                        }
                    }
                    s.last = Some(v);
                    s.running = None;
                    s.late = false;
                    s.finished += 1;
                    self.done.notify_all();
                });
            match spawned {
                Ok(_) => s.running = Some(Instant::now()),
                Err(e) => {
                    log::warn!("devices: {} not read: {e}", self.what);
                    return s.last.clone().unwrap_or_default();
                }
            }
        }
        let until = Instant::now() + wait;
        while s.finished < want {
            let Some(left) = until.checked_duration_since(Instant::now()) else {
                if !s.late {
                    s.late = true;
                    log::info!(
                        "devices: {} has not answered in {:.1} s; the list goes on without it",
                        self.what,
                        wait.as_secs_f32()
                    );
                }
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

    #[test]
    fn a_read_that_hangs_holds_nothing_up_and_is_not_started_twice() {
        static READ: Bounded<Vec<u8>> = Bounded::new("a test read");
        static STARTED: AtomicUsize = AtomicUsize::new(0);
        let wait = Duration::from_millis(100);

        // A read that answers at once.
        assert_eq!(READ.run(wait, || vec![1]), [1]);

        // One that hangs: the caller gets the answer before it, in about `wait`.
        let (release, held) = mpsc::channel::<()>();
        let start = Instant::now();
        let got = READ.run(wait, move || {
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

        // While it hangs, a later call starts nothing and also goes on with the last answer.
        assert_eq!(READ.run(wait, || unreachable!("a second read")), [1]);
        assert_eq!(STARTED.load(Ordering::SeqCst), 1);

        // Once it answers, its answer counts, and the next call reads again.
        release.send(()).unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while READ.state.lock().unwrap().running.is_some() && Instant::now() < until {
            std::thread::yield_now();
        }
        assert_eq!(READ.state.lock().unwrap().last.as_deref(), Some(&[2][..]));
        assert_eq!(READ.run(wait, || vec![3]), [3]);
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
        let other = std::thread::spawn(move || READ.run(Duration::from_secs(5), slow));
        while STARTED.load(Ordering::SeqCst) == 0 {
            std::thread::yield_now();
        }
        assert_eq!(READ.run(Duration::from_secs(5), || 8), 7);
        assert_eq!(other.join().unwrap(), 7);
        assert_eq!(STARTED.load(Ordering::SeqCst), 1);
    }
}
