//! System Health (Phase 7): per-subsystem status from each snapshot, plus a log of changes
//! (thermal limits, throttling, stopped fans, rails out of tolerance, drive health).
//!
//! Thresholds are fixed and deliberately conservative: a warning means "worth a look", not
//! "normal under load". Each one has hysteresis, so a reading hovering at a limit produces one
//! event, not one per tick. User-set limits with notifications are the Alerts feature.

use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::info::{HealthReport, SubsystemHealth};
use crate::snapshot::{SensorSnapshot, Status, StorageKind};

/// Events kept in memory for the Health view.
pub const MAX_EVENTS: usize = 200;
/// CPU package and GPU edge temperature limits (warning, problem), °C; the intelligence engine
/// measures thermal headroom against the problem limit.
pub const CPU_TEMP: (f64, f64) = (90.0, 99.0);
pub const GPU_TEMP: (f64, f64) = (85.0, 92.0);
/// Ticks a fan must have spun before 0 RPM counts as "stopped" (not an empty header).
const SPIN_TICKS: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Subsystem {
    Cpu,
    Gpu,
    Memory,
    Storage,
    Thermals,
    Fans,
    Voltages,
}

impl Subsystem {
    pub const ALL: [Self; 7] = [
        Self::Cpu,
        Self::Gpu,
        Self::Memory,
        Self::Storage,
        Self::Thermals,
        Self::Fans,
        Self::Voltages,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Gpu => "GPU",
            Self::Memory => "Memory",
            Self::Storage => "Storage",
            Self::Thermals => "Board thermals",
            Self::Fans => "Fans",
            Self::Voltages => "Voltages",
        }
    }
}

/// A change in a health condition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HealthEvent {
    /// Unix seconds.
    pub ts: i64,
    pub subsystem: String,
    /// `ok` when a condition cleared.
    pub status: Status,
    pub message: String,
}

/// One condition found in a snapshot. `key` identifies it across ticks.
#[derive(Debug, Clone, PartialEq)]
struct Finding {
    subsystem: Subsystem,
    key: String,
    status: Status,
    message: String,
}

const fn rank(s: Status) -> u8 {
    match s {
        Status::Fail => 3,
        Status::Warn => 2,
        Status::Ok => 1,
        Status::Unknown => 0,
    }
}

fn worst(a: Status, b: Status) -> Status {
    if rank(b) > rank(a) {
        b
    } else {
        a
    }
}

/// Grades a reading that is bad when high. While a condition is active its thresholds drop by
/// `hyst`, so it clears only once the value has clearly come back.
pub fn grade_high(value: f64, warn: f64, fail: f64, hyst: f64, prev: Status) -> Status {
    let w = if rank(prev) >= rank(Status::Warn) {
        warn - hyst
    } else {
        warn
    };
    let f = if prev == Status::Fail {
        fail - hyst
    } else {
        fail
    };
    if value >= f {
        Status::Fail
    } else if value >= w {
        Status::Warn
    } else {
        Status::Ok
    }
}

/// Same for a reading that is bad when low.
pub fn grade_low(value: f64, warn: f64, fail: f64, hyst: f64, prev: Status) -> Status {
    grade_high(-value, -warn, -fail, hyst, prev)
}

/// Supply rails by the name the board layout gives them: nominal volts. ATX allows ±5 %.
fn rail(name: &str) -> Option<f64> {
    match name {
        "+12V" => Some(12.0),
        "+5V" => Some(5.0),
        "+3.3V" | "+3.3V Standby" => Some(3.3),
        _ => None,
    }
}

#[derive(Debug, Default)]
pub struct HealthMonitor {
    active: HashMap<String, (Status, String, Subsystem)>,
    spun: HashMap<String, u32>,
    events: VecDeque<HealthEvent>,
    /// Events logged by the latest `update` (fed to alert rules).
    fresh: Vec<HealthEvent>,
}

struct Ctx<'a> {
    prev: &'a HashMap<String, (Status, String, Subsystem)>,
    out: Vec<Finding>,
}

impl Ctx<'_> {
    fn prev(&self, key: &str) -> Status {
        self.prev.get(key).map_or(Status::Ok, |p| p.0)
    }

    fn push(&mut self, subsystem: Subsystem, key: String, status: Status, message: String) {
        if rank(status) >= rank(Status::Warn) {
            self.out.push(Finding {
                subsystem,
                key,
                status,
                message,
            });
        }
    }

    /// High-is-bad temperature with the usual 3 °C hysteresis.
    fn temp(
        &mut self,
        sub: Subsystem,
        key: &str,
        what: &str,
        t: Option<f64>,
        warn: f64,
        fail: f64,
    ) {
        let Some(t) = t else { return };
        let s = grade_high(t, warn, fail, 3.0, self.prev(key));
        let msg = match s {
            Status::Fail => format!("{what} at {t:.0} °C, at or past its limit ({fail:.0} °C)"),
            _ => format!("{what} running hot: {t:.0} °C"),
        };
        self.push(sub, key.to_owned(), s, msg);
    }
}

impl HealthMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Recent changes, oldest first.
    pub fn events(&self) -> Vec<HealthEvent> {
        self.events.iter().cloned().collect()
    }

    /// Changes logged by the latest [`Self::update`].
    pub fn fresh(&self) -> &[HealthEvent] {
        &self.fresh
    }

    fn findings(&mut self, s: &SensorSnapshot) -> Vec<Finding> {
        let mut c = Ctx {
            prev: &self.active,
            out: Vec::new(),
        };

        // CPU. Ryzen 7000/9000 hold 95 °C under full load by design (a warning, not a problem);
        // Intel parts throttle at 100 °C.
        c.temp(
            Subsystem::Cpu,
            "cpu.temp",
            "CPU",
            s.cpu.temp_package,
            CPU_TEMP.0,
            CPU_TEMP.1,
        );
        if s.cpu.throttle_thermal == Some(true) {
            c.push(
                Subsystem::Cpu,
                "cpu.throttle".into(),
                Status::Warn,
                "CPU is thermal throttling".into(),
            );
        }

        // GPU
        for (i, g) in s.gpu.iter().enumerate() {
            let k = |w: &str| format!("gpu.{i}.{w}");
            c.temp(
                Subsystem::Gpu,
                &k("temp"),
                &g.name,
                g.temp_edge,
                GPU_TEMP.0,
                GPU_TEMP.1,
            );
            let hot = format!("{} hotspot", g.name);
            c.temp(
                Subsystem::Gpu,
                &k("hotspot"),
                &hot,
                g.temp_hotspot,
                100.0,
                108.0,
            );
            let mem = format!("{} memory", g.name);
            c.temp(Subsystem::Gpu, &k("mem"), &mem, g.temp_mem, 95.0, 105.0);
            if g.throttle_thermal == Some(true) {
                c.push(
                    Subsystem::Gpu,
                    k("throttle"),
                    Status::Warn,
                    format!("{} is thermal throttling", g.name),
                );
            }
        }

        // Memory
        let m = &s.memory;
        if let (Some(used), Some(total)) = (m.used_mb, m.total_mb.filter(|&t| t > 0.0)) {
            let pct = used / total * 100.0;
            let st = grade_high(pct, 95.0, 101.0, 3.0, c.prev("memory.used"));
            c.push(
                Subsystem::Memory,
                "memory.used".into(),
                st,
                format!("RAM {pct:.0}% in use"),
            );
        }
        if let (Some(used), Some(limit)) = (m.committed_mb, m.commit_limit_mb.filter(|&l| l > 0.0))
        {
            let pct = used / limit * 100.0;
            let st = grade_high(pct, 95.0, 101.0, 3.0, c.prev("memory.commit"));
            c.push(
                Subsystem::Memory,
                "memory.commit".into(),
                st,
                format!("Commit charge at {pct:.0}% of the limit; programs may fail to allocate"),
            );
        }
        for d in &m.dimms {
            let key = format!("memory.{}", d.slot);
            c.temp(Subsystem::Memory, &key, &d.slot, d.temp_c, 75.0, 85.0);
        }

        // Storage
        for d in &s.storage {
            let (warn, fail) = if d.kind == StorageKind::Nvme {
                (75.0, 85.0)
            } else {
                (60.0, 70.0)
            };
            c.temp(
                Subsystem::Storage,
                &format!("storage.{}.temp", d.id),
                &d.model,
                d.temp_c,
                warn,
                fail,
            );
            if matches!(d.health, Status::Warn | Status::Fail) {
                let what = if d.health == Status::Fail {
                    "reports a failing health status"
                } else {
                    "reports a health warning"
                };
                c.push(
                    Subsystem::Storage,
                    format!("storage.{}.health", d.id),
                    d.health,
                    format!("{} {what} (details in Storage)", d.model),
                );
            }
        }

        // Board thermals
        let b = &s.board;
        for (key, what, t, warn, fail) in [
            (
                "board.vrm",
                "VRM",
                b.temp_vrm_mos.or(b.temp_vrm),
                95.0,
                105.0,
            ),
            (
                "board.chipset",
                "Chipset",
                b.temp_chipset.or(b.temp_pch),
                80.0,
                90.0,
            ),
            ("board.system", "System", b.temp_system, 55.0, 65.0),
        ] {
            c.temp(Subsystem::Thermals, key, what, t, warn, fail);
        }

        // Fans: a fan that was spinning and now reads 0 RPM has stopped (pump failure if it is
        // the pump). Headers that never spun are empty, not failed.
        for f in &s.fans {
            let Some(rpm) = f.rpm else { continue };
            let key = format!("fan.{}", f.name);
            let spun = self.spun.entry(f.name.clone()).or_default();
            if rpm > 0.0 {
                *spun = (*spun + 1).min(SPIN_TICKS);
            } else if *spun >= SPIN_TICKS {
                let pump = f.name.to_lowercase().contains("pump");
                c.push(
                    Subsystem::Fans,
                    key,
                    Status::Fail,
                    if pump {
                        format!("{} stopped: check the liquid cooler", f.name)
                    } else {
                        format!("{} stopped (0 RPM)", f.name)
                    },
                );
            }
        }

        // Voltages
        for v in &s.voltages {
            // 0 V means the input is not sampled (see `sio`), never a measured rail.
            let Some(value) = v.value.filter(|v| *v > 0.0) else {
                continue;
            };
            let key = format!("voltage.{}", v.name);
            if let Some(nominal) = rail(&v.name) {
                let dev = (value / nominal - 1.0).abs() * 100.0;
                let st = grade_high(dev, 5.0, 10.0, 1.0, c.prev(&key));
                c.push(
                    Subsystem::Voltages,
                    key,
                    st,
                    format!(
                        "{} at {value:.2} V, {dev:.1}% off nominal (ATX allows 5%)",
                        v.name
                    ),
                );
            } else if v.name == "CMOS Battery" {
                let st = grade_low(value, 2.8, 2.5, 0.05, c.prev(&key));
                c.push(
                    Subsystem::Voltages,
                    key,
                    st,
                    format!("CMOS battery low: {value:.2} V (replace the coin cell)"),
                );
            }
        }
        c.out
    }

    /// Grades `snap`, logs what changed since the last call, and returns the report.
    pub fn update(&mut self, snap: &SensorSnapshot) -> HealthReport {
        self.fresh.clear();
        let findings = self.findings(snap);
        let ts = snap.ts;
        let mut next = HashMap::new();
        for f in &findings {
            let prev = self.active.get(&f.key).map(|p| p.0);
            if prev != Some(f.status) {
                self.log(ts, f.subsystem, f.status, f.message.clone());
            }
            next.insert(f.key.clone(), (f.status, f.message.clone(), f.subsystem));
        }
        let mut cleared: Vec<_> = self
            .active
            .iter()
            .filter(|(k, _)| !next.contains_key(*k))
            .map(|(_, (_, msg, sub))| (*sub, msg.clone()))
            .collect();
        cleared.sort();
        for (sub, msg) in cleared {
            self.log(ts, sub, Status::Ok, format!("Cleared: {msg}"));
        }
        self.active = next;
        report(snap, &findings)
    }

    fn log(&mut self, ts: i64, sub: Subsystem, status: Status, message: String) {
        if self.events.len() == MAX_EVENTS {
            self.events.pop_front();
        }
        let e = HealthEvent {
            ts,
            subsystem: sub.name().to_owned(),
            status,
            message,
        };
        self.fresh.push(e.clone());
        self.events.push_back(e);
    }
}

/// Whether the snapshot carries any reading for a subsystem (else its status is `unknown`).
fn monitored(s: &SensorSnapshot, sub: Subsystem) -> bool {
    match sub {
        Subsystem::Cpu => s.cpu.usage_total.is_some() || s.cpu.temp_package.is_some(),
        Subsystem::Gpu => !s.gpu.is_empty(),
        Subsystem::Memory => s.memory.total_mb.is_some(),
        Subsystem::Storage => !s.storage.is_empty(),
        Subsystem::Thermals => {
            let b = &s.board;
            [
                b.temp_system,
                b.temp_vrm_mos,
                b.temp_vrm,
                b.temp_chipset,
                b.temp_pch,
            ]
            .iter()
            .any(Option::is_some)
        }
        Subsystem::Fans => s.fans.iter().any(|f| f.rpm.is_some()),
        Subsystem::Voltages => s.voltages.iter().any(|v| v.value.is_some()),
    }
}

/// Plain-language gaps worth stating even when nothing is wrong.
fn coverage(s: &SensorSnapshot, sub: Subsystem) -> Option<&'static str> {
    match sub {
        Subsystem::Cpu if s.cpu.temp_package.is_none() => {
            Some("CPU temperature not available (needs PawnIO)")
        }
        Subsystem::Storage if s.storage.iter().any(|d| d.health == Status::Unknown) => {
            Some("Some drives do not report health (run as administrator)")
        }
        Subsystem::Thermals | Subsystem::Fans | Subsystem::Voltages if !monitored(s, sub) => {
            Some("Not reported: needs PawnIO and a supported sensor chip")
        }
        _ => None,
    }
}

fn report(s: &SensorSnapshot, findings: &[Finding]) -> HealthReport {
    let subsystems: Vec<SubsystemHealth> = Subsystem::ALL
        .iter()
        .map(|&sub| {
            let mine: Vec<&Finding> = findings.iter().filter(|f| f.subsystem == sub).collect();
            let status = mine.iter().fold(
                if monitored(s, sub) {
                    Status::Ok
                } else {
                    Status::Unknown
                },
                |acc, f| worst(acc, f.status),
            );
            let mut mine = mine;
            mine.sort_by_key(|f| std::cmp::Reverse(rank(f.status)));
            SubsystemHealth {
                name: sub.name().to_owned(),
                status,
                messages: mine.iter().map(|f| f.message.clone()).collect(),
                notes: coverage(s, sub).map(str::to_owned).into_iter().collect(),
            }
        })
        .collect();
    let overall = subsystems.iter().fold(Status::Unknown, |acc, h| {
        if h.status == Status::Unknown {
            acc
        } else {
            worst(acc, h.status)
        }
    });
    HealthReport {
        overall,
        subsystems,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{DimmSnapshot, FanSnapshot, GpuSnapshot, VoltageReading};

    fn snap(ts: i64) -> SensorSnapshot {
        let mut s = SensorSnapshot {
            ts,
            ..Default::default()
        };
        s.cpu.usage_total = Some(10.0);
        s.cpu.temp_package = Some(55.0);
        s.memory.total_mb = Some(32768.0);
        s.memory.used_mb = Some(12000.0);
        s
    }

    fn sub<'a>(r: &'a HealthReport, name: &str) -> &'a SubsystemHealth {
        r.subsystems.iter().find(|s| s.name == name).unwrap()
    }

    #[test]
    fn grades_with_hysteresis() {
        assert_eq!(grade_high(89.0, 90.0, 95.0, 3.0, Status::Ok), Status::Ok);
        assert_eq!(grade_high(90.0, 90.0, 95.0, 3.0, Status::Ok), Status::Warn);
        // Active warning holds until 87.
        assert_eq!(
            grade_high(88.0, 90.0, 95.0, 3.0, Status::Warn),
            Status::Warn
        );
        assert_eq!(grade_high(86.9, 90.0, 95.0, 3.0, Status::Warn), Status::Ok);
        assert_eq!(
            grade_high(93.0, 90.0, 95.0, 3.0, Status::Fail),
            Status::Fail
        );
        assert_eq!(grade_low(2.7, 2.8, 2.5, 0.05, Status::Ok), Status::Warn);
        assert_eq!(grade_low(2.4, 2.8, 2.5, 0.05, Status::Ok), Status::Fail);
    }

    #[test]
    fn healthy_machine_is_ok_and_unmonitored_parts_unknown() {
        let mut h = HealthMonitor::new();
        let r = h.update(&snap(0));
        assert_eq!(r.overall, Status::Ok);
        assert_eq!(sub(&r, "CPU").status, Status::Ok);
        assert_eq!(sub(&r, "Fans").status, Status::Unknown);
        assert!(h.events().is_empty());
    }

    #[test]
    fn cpu_at_its_limit_logs_one_event_then_clears() {
        let mut h = HealthMonitor::new();
        let mut s = snap(0);
        s.cpu.temp_package = Some(99.5);
        let r = h.update(&s);
        assert_eq!(r.overall, Status::Fail);
        assert!(sub(&r, "CPU").messages[0].contains("100 °C"));
        // Hovering around the limit does not add events.
        for (ts, t) in [(1, 98.5), (2, 97.0), (3, 96.5)] {
            s.ts = ts;
            s.cpu.temp_package = Some(t);
            h.update(&s);
        }
        assert_eq!(h.events().len(), 1);
        // Back to a Ryzen's designed 95 °C: a warning, not a problem.
        s.ts = 4;
        s.cpu.temp_package = Some(95.0);
        assert_eq!(h.update(&s).overall, Status::Warn);
        assert_eq!(h.events().len(), 2);
        s.ts = 6;
        s.cpu.temp_package = Some(60.0);
        let r = h.update(&s);
        assert_eq!(r.overall, Status::Ok);
        let ev = h.events();
        assert_eq!(ev.len(), 3);
        assert_eq!(ev[2].status, Status::Ok);
        assert!(ev[2].message.starts_with("Cleared: CPU"));
    }

    #[test]
    fn a_pump_that_stops_fails_but_an_empty_header_does_not() {
        let mut h = HealthMonitor::new();
        let mut s = snap(0);
        let fans = |pump: f64| {
            vec![
                FanSnapshot {
                    name: "Pump Fan".into(),
                    rpm: Some(pump),
                    ..Default::default()
                },
                FanSnapshot {
                    name: "System Fan 1".into(),
                    rpm: Some(0.0),
                    ..Default::default()
                },
            ]
        };
        for ts in 0..6 {
            s.ts = ts;
            s.fans = fans(4400.0);
            assert_eq!(sub(&h.update(&s), "Fans").status, Status::Ok);
        }
        s.fans = fans(0.0);
        let r = h.update(&s);
        assert_eq!(sub(&r, "Fans").status, Status::Fail);
        assert_eq!(sub(&r, "Fans").messages.len(), 1);
        assert!(sub(&r, "Fans").messages[0].contains("liquid cooler"));
    }

    #[test]
    fn rails_and_cmos_battery() {
        let mut h = HealthMonitor::new();
        let mut s = snap(0);
        s.voltages = vec![
            VoltageReading {
                name: "+12V".into(),
                value: Some(12.096),
            },
            VoltageReading {
                name: "+5V".into(),
                value: Some(4.6),
            },
            VoltageReading {
                name: "CMOS Battery".into(),
                value: Some(3.09),
            },
            VoltageReading {
                name: "Vcore".into(),
                value: Some(1.4),
            },
        ];
        let r = h.update(&s);
        let v = sub(&r, "Voltages");
        assert_eq!(v.status, Status::Warn);
        assert_eq!(v.messages.len(), 1);
        assert!(v.messages[0].starts_with("+5V at 4.60 V"));
    }

    #[test]
    fn inputs_reading_zero_are_not_graded() {
        // Field (MS-7E24): +3.3V Standby and the CMOS battery read 0.00 V on a working PC.
        let mut h = HealthMonitor::new();
        let mut s = snap(0);
        s.voltages = vec![
            VoltageReading {
                name: "+12V".into(),
                value: Some(12.07),
            },
            VoltageReading {
                name: "+3.3V Standby".into(),
                value: Some(0.0),
            },
            VoltageReading {
                name: "CMOS Battery".into(),
                value: Some(0.0),
            },
        ];
        let r = h.update(&s);
        let v = sub(&r, "Voltages");
        assert_eq!(v.status, Status::Ok);
        assert!(v.messages.is_empty());
        assert!(r.overall != Status::Fail);
    }

    #[test]
    fn gpu_dimm_and_memory_pressure() {
        let mut h = HealthMonitor::new();
        let mut s = snap(0);
        s.gpu.push(GpuSnapshot {
            name: "RTX".into(),
            temp_edge: Some(70.0),
            throttle_thermal: Some(true),
            ..Default::default()
        });
        s.memory.dimms.push(DimmSnapshot {
            slot: "DIMMA2".into(),
            temp_c: Some(80.0),
            ..Default::default()
        });
        s.memory.used_mb = Some(32000.0);
        let r = h.update(&s);
        assert_eq!(sub(&r, "GPU").messages, ["RTX is thermal throttling"]);
        assert_eq!(sub(&r, "Memory").status, Status::Warn);
        assert_eq!(sub(&r, "Memory").messages.len(), 2);
    }

    #[test]
    fn keeps_a_bounded_event_log() {
        let mut h = HealthMonitor::new();
        let mut s = snap(0);
        for ts in 0..(MAX_EVENTS as i64 + 50) {
            s.ts = ts;
            s.cpu.temp_package = Some(if ts % 2 == 0 { 99.0 } else { 40.0 });
            h.update(&s);
        }
        assert_eq!(h.events().len(), MAX_EVENTS);
    }
}
