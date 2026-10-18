//! "Find my best" (1.6): raises the core offset step by step while the stability test passes,
//! keeps a margin below the last step that passed, does the same for the memory offset while its
//! bandwidth keeps up (memory that corrects its own errors gets slower before it fails), then
//! tests the result at length. Pure: the app runs each test and reports how it went.

use super::{Limits, Settings};

pub const CORE_STEP: i32 = 15;
/// The result keeps this margin below the last core step that passed (30 MHz below the one
/// that failed).
pub const CORE_MARGIN: i32 = 15;
/// Furthest the core climbs above where it starts, so a run ends in time.
pub const CORE_CLIMB: i32 = 300;
pub const MEM_STEP: i32 = 100;
pub const MEM_MARGIN: i32 = 100;
pub const MEM_CLIMB: i32 = 1500;
/// A memory step whose bandwidth is this much below the best so far counts as failed.
pub const MEM_LOSS: f64 = 0.01;
/// Each step's test, seconds.
pub const STEP_SECS: u32 = 45;
/// The result's test, seconds (as "Test this setting").
pub const CONFIRM_SECS: u32 = 300;
/// The result is lowered one step and tested again at most this many times.
pub const CONFIRM_TRIES: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Core,
    Memory,
    Confirm,
    Finished,
}

/// What to do next.
#[derive(Debug, Clone, PartialEq)]
pub enum Next {
    /// Apply these settings and run the stability test for `secs`.
    Test {
        settings: Settings,
        secs: u32,
        step: String,
    },
    /// The best settings found, tested at length.
    Done(Settings),
    /// Nothing above stock passed.
    GaveUp(String),
}

#[derive(Debug, Clone)]
pub struct Search {
    limits: Limits,
    phase: Phase,
    /// Where the climbs start (the Performance suggestion).
    start: Settings,
    /// On test now.
    current: Settings,
    /// The last core and memory offsets that passed (0 until one does).
    core_ok: i32,
    mem_ok: i32,
    best_bandwidth: Option<f64>,
    tries: u32,
}

impl Search {
    /// A search from `start` (clamped to `limits`), at the highest power limit allowed.
    pub fn new(start: Settings, limits: Limits) -> Self {
        let power_pct = limits.power.map_or(100, |r| r.max.max(100));
        let start = limits.clamp(Settings { power_pct, ..start });
        let phase = if limits.core.is_some() {
            Phase::Core
        } else {
            Phase::Memory
        };
        let current = match phase {
            Phase::Core => Settings {
                mem_mhz: 0,
                ..start
            },
            _ => start,
        };
        Self {
            limits,
            phase,
            start,
            current,
            core_ok: 0,
            mem_ok: 0,
            best_bandwidth: None,
            tries: 0,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The first test, or the end when the driver allows no offsets at all.
    pub fn first(&mut self) -> Next {
        if self.limits.core.is_none() && self.limits.mem.is_none() {
            self.phase = Phase::Finished;
            return Next::GaveUp("the driver allows no clock offsets on this card".into());
        }
        self.test()
    }

    fn test(&self) -> Next {
        let s = self.current;
        let (secs, step) = match self.phase {
            Phase::Core => (STEP_SECS, format!("Core {:+} MHz", s.core_mhz)),
            Phase::Memory => (STEP_SECS, format!("Memory {:+} MHz", s.mem_mhz)),
            _ => (CONFIRM_SECS, format!("Checking {}", super::describe(&s))),
        };
        Next::Test {
            settings: s,
            secs,
            step,
        }
    }

    /// How the last test went; `bandwidth` is the memory bandwidth it measured (GB/s), if any.
    pub fn result(&mut self, passed: bool, bandwidth: Option<f64>) -> Next {
        match self.phase {
            Phase::Core => {
                let top = self
                    .limits
                    .core
                    .map_or(0, |r| r.max.min(self.start.core_mhz + CORE_CLIMB));
                if passed {
                    self.core_ok = self.current.core_mhz;
                    if self.current.core_mhz + CORE_STEP <= top {
                        self.current.core_mhz += CORE_STEP;
                        return self.test();
                    }
                }
                let core = (self.core_ok - CORE_MARGIN).max(0);
                self.current = Settings {
                    core_mhz: core,
                    mem_mhz: self.start.mem_mhz,
                    ..self.current
                };
                self.phase = Phase::Memory;
                if self.limits.mem.is_none() {
                    return self.confirm();
                }
                self.test()
            }
            Phase::Memory => {
                let slower = match (bandwidth, self.best_bandwidth) {
                    (Some(b), Some(best)) => b < best * (1.0 - MEM_LOSS),
                    _ => false,
                };
                let top = self
                    .limits
                    .mem
                    .map_or(0, |r| r.max.min(self.start.mem_mhz + MEM_CLIMB));
                if passed && !slower {
                    self.mem_ok = self.current.mem_mhz;
                    if let Some(b) = bandwidth {
                        self.best_bandwidth = Some(self.best_bandwidth.map_or(b, |x| x.max(b)));
                    }
                    if self.current.mem_mhz + MEM_STEP <= top {
                        self.current.mem_mhz += MEM_STEP;
                        return self.test();
                    }
                }
                self.current.mem_mhz = (self.mem_ok - MEM_MARGIN).max(0);
                self.confirm()
            }
            Phase::Confirm => {
                if passed {
                    self.phase = Phase::Finished;
                    return Next::Done(self.current);
                }
                self.tries += 1;
                let lower = Settings {
                    core_mhz: (self.current.core_mhz - CORE_STEP).max(0),
                    mem_mhz: (self.current.mem_mhz - MEM_STEP).max(0),
                    ..self.current
                };
                if self.tries > CONFIRM_TRIES || lower == self.current {
                    self.phase = Phase::Finished;
                    return Next::GaveUp(
                        "no offset passed the long test: the card is best left at its own clocks"
                            .into(),
                    );
                }
                self.current = lower;
                self.test()
            }
            Phase::Finished => Next::GaveUp("the search has ended".into()),
        }
    }

    fn confirm(&mut self) -> Next {
        self.phase = Phase::Confirm;
        if self.current.core_mhz == 0 && self.current.mem_mhz == 0 {
            self.phase = Phase::Finished;
            return Next::GaveUp(
                "no offset passed: the card is best left at its own clocks".into(),
            );
        }
        self.test()
    }

    /// About how far along the search is, 0–1.
    pub fn progress(&self) -> f64 {
        let climb = |now: i32, from: i32, span: i32| {
            (f64::from(now - from) / f64::from(span.max(1))).clamp(0.0, 1.0)
        };
        match self.phase {
            Phase::Core => 0.4 * climb(self.current.core_mhz, self.start.core_mhz, CORE_CLIMB),
            Phase::Memory => {
                0.4 + 0.35 * climb(self.current.mem_mhz, self.start.mem_mhz, MEM_CLIMB)
            }
            Phase::Confirm => 0.75 + 0.08 * f64::from(self.tries),
            Phase::Finished => 1.0,
        }
    }
}
