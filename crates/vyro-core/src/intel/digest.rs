//! The minute digest (P16): what the sensor history does not keep, one row a minute. The
//! busiest programs (names only), how much of the minute the CPU and GPU reported holding back
//! their clocks, the profile in force and the game being played. The poller feeds it every
//! tick; the history store keeps a week of it.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::gaming::{Bound, CaptureState};
use crate::profiles::Profile;
use crate::snapshot::SensorSnapshot;

/// Programs kept per minute…
pub const MAX_BUSY: usize = 5;
/// …if they used at least this much of the CPU (% of the whole machine)…
pub const BUSY_CPU: f64 = 3.0;
/// …or of the GPU, on average over the minute.
pub const BUSY_GPU: f64 = 5.0;

/// A program's average use over a minute (all its processes together).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Busy {
    pub name: String,
    pub cpu: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gpu: f64,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// Seconds of the minute each limit was reported; `None` when the part does not report it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Throttle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_heat: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_power: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_current: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_heat: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_power: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MinuteDigest {
    /// Start of the minute, unix seconds.
    pub ts: i64,
    /// Profile in force at the end of the minute (`None`: the app has not said yet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<Profile>,
    /// The game being captured, as its sessions name it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
    /// What held the frame rate for most of the minute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound: Option<Bound>,
    #[serde(default)]
    pub throttle: Throttle,
    /// Busiest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub busy: Vec<Busy>,
}

impl MinuteDigest {
    /// This program's use in the minute (0 when it was not among the busiest).
    pub fn use_of(&self, name: &str) -> (f64, f64) {
        self.busy
            .iter()
            .find(|b| b.name == name)
            .map_or((0.0, 0.0), |b| (b.cpu, b.gpu))
    }
}

/// Ticks with a flag reported, and with it set.
#[derive(Debug, Clone, Copy, Default)]
struct Flag {
    reported: u32,
    on: u32,
}

impl Flag {
    fn add(&mut self, v: Option<bool>) {
        if let Some(on) = v {
            self.reported += 1;
            self.on += u32::from(on);
        }
    }

    /// Seconds of a minute of `ticks` ticks.
    fn secs(self, ticks: u32) -> Option<u8> {
        (self.reported > 0).then(|| {
            let share = f64::from(self.on) / f64::from(ticks.max(1));
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let s = (share * 60.0).round().clamp(0.0, 60.0) as u8;
            s
        })
    }
}

/// Builds one [`MinuteDigest`] a minute from the ticks.
#[derive(Debug, Default)]
pub struct DigestBuilder {
    minute: Option<i64>,
    ticks: u32,
    /// CPU and GPU % summed over the ticks, by program name.
    use_sum: HashMap<String, (f64, f64)>,
    flags: [Flag; 5],
    bound: [u32; 3],
    game: Option<String>,
    profile: Option<Profile>,
}

/// Not programs: the idle process, and Vyro itself.
fn ignored(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "idle" || n == "system idle process" || n.starts_with("vyro")
}

impl DigestBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a tick. Returns the previous minute's digest when this tick opens a new minute.
    pub fn add(&mut self, snap: &SensorSnapshot, profile: Option<Profile>) -> Option<MinuteDigest> {
        let minute = snap.ts.div_euclid(60) * 60;
        let closed = match self.minute {
            Some(m) if m != minute => self.close(),
            _ => None,
        };
        self.minute = Some(minute);
        self.ticks += 1;
        let game = snap
            .game
            .as_ref()
            .filter(|g| g.state == CaptureState::Capturing);
        let game_pid = game.map(|g| g.pid);
        // A program's processes together; the game itself is not background load.
        let mut tick: HashMap<&str, (f64, f64)> = HashMap::new();
        for p in &snap.processes {
            if Some(p.pid) == game_pid || ignored(&p.name) {
                continue;
            }
            let e = tick.entry(p.name.as_str()).or_default();
            e.0 += p.cpu_pct.unwrap_or(0.0);
            e.1 += p.gpu_pct.unwrap_or(0.0);
        }
        for (name, (cpu, gpu)) in tick {
            if cpu > 0.0 || gpu > 0.0 {
                let e = self.use_sum.entry(name.to_owned()).or_default();
                e.0 += cpu;
                e.1 += gpu;
            }
        }
        let gpu0 = snap.gpu.first();
        self.flags[0].add(snap.cpu.throttle_thermal);
        self.flags[1].add(snap.cpu.throttle_power_limit);
        self.flags[2].add(snap.cpu.throttle_current_limit);
        self.flags[3].add(gpu0.and_then(|g| g.throttle_thermal));
        self.flags[4].add(gpu0.and_then(|g| g.throttle_power_limit));
        if let Some(g) = game {
            self.game = Some(g.app.clone());
            match g.bound {
                Some(Bound::Gpu) => self.bound[0] += 1,
                Some(Bound::Cpu) => self.bound[1] += 1,
                Some(Bound::Capped) => self.bound[2] += 1,
                None => {}
            }
        }
        if profile.is_some() {
            self.profile = profile;
        }
        closed
    }

    fn close(&mut self) -> Option<MinuteDigest> {
        let ts = self.minute?;
        let ticks = self.ticks.max(1);
        let n = f64::from(ticks);
        let round = |v: f64| (v * 10.0).round() / 10.0;
        let mut busy: Vec<Busy> = std::mem::take(&mut self.use_sum)
            .into_iter()
            .map(|(name, (cpu, gpu))| Busy {
                name,
                cpu: round(cpu / n),
                gpu: round(gpu / n),
            })
            .filter(|b| b.cpu >= BUSY_CPU || b.gpu >= BUSY_GPU)
            .collect();
        busy.sort_by(|a, b| {
            (b.cpu + b.gpu)
                .total_cmp(&(a.cpu + a.gpu))
                .then_with(|| a.name.cmp(&b.name))
        });
        busy.truncate(MAX_BUSY);
        let f = std::mem::take(&mut self.flags);
        let bound = std::mem::take(&mut self.bound);
        let most = bound.iter().copied().max().unwrap_or(0);
        let bound = (most > 0).then(|| {
            [Bound::Gpu, Bound::Cpu, Bound::Capped]
                [bound.iter().position(|&b| b == most).unwrap_or(0)]
        });
        let digest = MinuteDigest {
            ts,
            profile: self.profile,
            game: self.game.take(),
            bound,
            throttle: Throttle {
                cpu_heat: f[0].secs(ticks),
                cpu_power: f[1].secs(ticks),
                cpu_current: f[2].secs(ticks),
                gpu_heat: f[3].secs(ticks),
                gpu_power: f[4].secs(ticks),
            },
            busy,
        };
        self.ticks = 0;
        Some(digest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gaming::GameSnapshot;
    use crate::snapshot::{GpuSnapshot, ProcessSnapshot};

    fn proc(pid: u32, name: &str, cpu: f64, gpu: Option<f64>) -> ProcessSnapshot {
        ProcessSnapshot {
            pid,
            name: name.into(),
            cpu_pct: Some(cpu),
            ram_mb: Some(100.0),
            gpu_pct: gpu,
        }
    }

    fn game(pid: u32, bound: Bound) -> GameSnapshot {
        GameSnapshot {
            state: CaptureState::Capturing,
            detail: None,
            app: "Game.exe".into(),
            pid,
            runtime: None,
            present_mode: None,
            fps: Some(100.0),
            frame_ms: Some(10.0),
            low1_fps: None,
            low01_fps: None,
            cpu_busy_ms: None,
            gpu_busy_ms: None,
            latency_ms: None,
            displayed_fps: None,
            bound: Some(bound),
            started: 0,
            frames: 0,
            session_fps: None,
            session_low1_fps: None,
            timeline: Vec::new(),
        }
    }

    fn snap(ts: i64, processes: Vec<ProcessSnapshot>) -> SensorSnapshot {
        let mut s = SensorSnapshot {
            ts,
            processes,
            ..Default::default()
        };
        s.cpu.throttle_thermal = Some(false);
        s
    }

    #[test]
    fn averages_each_program_over_the_minute_and_keeps_the_busiest() {
        let mut b = DigestBuilder::new();
        let t0 = 1_800_000_000; // a whole minute
        for i in 0..60 {
            let mut list = vec![
                proc(10, "MsMpEng.exe", 20.0, None),
                // Two processes of one program count together.
                proc(11, "chrome.exe", 2.0, Some(1.0)),
                proc(12, "chrome.exe", 2.0, Some(5.0)),
                proc(13, "explorer.exe", 1.0, None),
                proc(14, "vyro-service.exe", 9.0, None),
                proc(99, "Game.exe", 60.0, Some(95.0)),
            ];
            // Busy for the first half only: 4 % on average.
            if i >= 30 {
                list[0].cpu_pct = Some(0.0);
            }
            let mut s = snap(t0 + i, list);
            s.cpu.throttle_thermal = Some(i < 15);
            s.game = Some(game(99, if i < 40 { Bound::Gpu } else { Bound::Cpu }));
            assert!(b.add(&s, Some(Profile::Gaming)).is_none());
        }
        let d = b.add(&snap(t0 + 60, Vec::new()), None).expect("closed");
        assert_eq!(d.ts, t0);
        assert_eq!(d.profile, Some(Profile::Gaming));
        assert_eq!(d.game.as_deref(), Some("Game.exe"));
        assert_eq!(d.bound, Some(Bound::Gpu));
        assert_eq!(d.throttle.cpu_heat, Some(15));
        assert_eq!(d.throttle.gpu_heat, None, "not reported");
        let names: Vec<(&str, f64, f64)> = d
            .busy
            .iter()
            .map(|x| (x.name.as_str(), x.cpu, x.gpu))
            .collect();
        // The game and Vyro are not background programs; explorer is under the bar.
        assert_eq!(
            names,
            [("MsMpEng.exe", 10.0, 0.0), ("chrome.exe", 4.0, 6.0)]
        );
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.len() < 220, "{json}");
        assert_eq!(serde_json::from_str::<MinuteDigest>(&json).unwrap(), d);
    }

    #[test]
    fn a_quiet_minute_is_small_and_keeps_the_profile() {
        let mut b = DigestBuilder::new();
        b.add(
            &snap(120, vec![proc(1, "a.exe", 0.5, None)]),
            Some(Profile::Silent),
        );
        b.add(&snap(150, Vec::new()), None);
        let d = b.add(&snap(180, Vec::new()), None).unwrap();
        assert_eq!(d.ts, 120);
        assert_eq!(d.profile, Some(Profile::Silent));
        assert!(d.busy.is_empty() && d.game.is_none() && d.bound.is_none());
        for (ts, on) in [(240, true), (270, false)] {
            let mut s = snap(ts, Vec::new());
            s.gpu.push(GpuSnapshot {
                throttle_thermal: Some(on),
                ..Default::default()
            });
            b.add(&s, None);
        }
        let d = b.add(&snap(300, Vec::new()), None).unwrap();
        // One tick of two: half the minute.
        assert_eq!(d.throttle.gpu_heat, Some(30));
        assert_eq!(d.profile, Some(Profile::Silent));
    }
}
