//! The 13 checks of checks v1 (`docs/diagnostics.md`) over what the engine and the app hold.
//! Pure: the same inputs always give the same report. Summaries carry no temperatures (the
//! citations do, so they follow the chosen unit).

use super::limits::{
    CORRECTED, DRIVE_C, FAN_PCT, FREE_PCT, HEADROOM_C, HOTTER_C, LIFE_PCT, LOSS_PCT, MEMORY_PCT,
    PING_MS, SAG_PCT, THROTTLE_PCT, WINDOW_SECS,
};
use super::{CheckId, CheckStatus, Citation, DiagAppInfo, DiagCheck, Origin};
use crate::fan::FanStatus;
use crate::health::HealthEvent;
use crate::info::{DriveInfo, PawnIoState, PawnIoStatus};
use crate::intel::{Confidence, Insights};
use crate::score::Measures;
use crate::snapshot::{FanKind, SensorSnapshot, Status};

/// Health events cited per check, newest first.
const MAX_EVENTS: usize = 5;
/// Insights warnings cited.
const MAX_FINDINGS: usize = 5;

pub struct DiagInputs<'a> {
    /// When the report is worked out (Unix seconds).
    pub now: i64,
    /// The week's measures, as the Performance Score reads them; `None` without a history
    /// store.
    pub measures: Option<&'a Measures>,
    pub insights: Option<&'a Insights>,
    /// Health changes, newest or oldest first, any age (the window is applied here).
    pub health_events: &'a [HealthEvent],
    pub snapshot: Option<&'a SensorSnapshot>,
    pub drives: &'a [DriveInfo],
    pub pawnio: Option<&'a PawnIoStatus>,
    pub fans: Option<&'a FanStatus>,
    pub app: &'a DiagAppInfo,
}

pub fn evaluate(i: &DiagInputs) -> Vec<DiagCheck> {
    vec![
        thermals(i),
        clocks(i),
        power(i),
        throttling(i),
        cooling(i),
        memory(i),
        storage(i),
        network(i),
        hardware(i),
        changes(i),
        drivers(i),
        sensors(i),
        service(i),
    ]
}

fn check(
    id: CheckId,
    status: CheckStatus,
    summary: impl Into<String>,
    advice: Option<&str>,
    citations: Vec<Citation>,
) -> DiagCheck {
    DiagCheck {
        id,
        status,
        summary: summary.into(),
        advice: advice.map(str::to_owned),
        citations,
    }
}

fn unavailable(id: CheckId, why: impl Into<String>) -> DiagCheck {
    check(
        id,
        CheckStatus::Unavailable,
        "No data for this check yet.",
        None,
        Vec::new(),
    )
    .with_advice(why)
}

impl DiagCheck {
    fn with_advice(mut self, advice: impl Into<String>) -> Self {
        self.advice = Some(advice.into());
        self
    }
}

const NO_STORE: &str = "Vyro's history store is not available.";

/// Why history-based data is missing: too little history, or the given reason.
fn why(i: &DiagInputs, otherwise: &str) -> String {
    i.measures
        .and_then(|m| m.thin.clone())
        .unwrap_or_else(|| otherwise.to_owned())
}

/// The sensor a device's temperature / clock is kept as ("CPU", "GPU #1").
fn device_metric(device: &str, what: &str) -> Option<String> {
    if device == "CPU" {
        return Some(match what {
            "temp" => "cpu.temp.package".into(),
            _ => "cpu.clock.effective".into(),
        });
    }
    let n = device.strip_prefix("GPU #")?;
    Some(match what {
        "temp" => format!("gpu.{n}.temp.edge"),
        _ => format!("gpu.{n}.clock.core"),
    })
}

/// Health warnings and problems from `subsystems` in the window, newest first.
fn events<'a>(
    i: &'a DiagInputs,
    subsystems: &[&str],
    need: impl Fn(&HealthEvent) -> bool,
) -> Vec<&'a HealthEvent> {
    let mut v: Vec<&HealthEvent> = i
        .health_events
        .iter()
        .filter(|e| {
            e.status != Status::Ok
                && i.now - e.ts <= WINDOW_SECS
                && e.ts <= i.now
                && subsystems.contains(&e.subsystem.as_str())
                && need(e)
        })
        .collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.ts));
    v.truncate(MAX_EVENTS);
    v
}

fn event_citation(i: &DiagInputs, e: &HealthEvent) -> Citation {
    Citation::text(
        format!("Health: {}", e.subsystem),
        e.message.clone(),
        Origin::HealthLog,
    )
    .ago(i.now - e.ts)
}

fn thermals(i: &DiagInputs) -> DiagCheck {
    let id = CheckId::Thermals;
    let Some(m) = i.measures else {
        return unavailable(id, NO_STORE);
    };
    let mut cites = Vec::new();
    let mut close = Vec::new();
    for h in &m.thermals {
        let head = h.limit_c - h.median_c;
        let mut c = Citation::number(
            format!("{} median under load", h.device),
            h.median_c,
            "°C",
            0,
            Origin::History,
        );
        if let Some(metric) = device_metric(&h.device, "temp") {
            c = c.metric(metric);
        }
        cites.push(c);
        cites.push(
            Citation::number(
                format!("{} headroom to its limit", h.device),
                head,
                "°C",
                0,
                Origin::History,
            )
            .diff(),
        );
        if head < HEADROOM_C {
            close.push(h.device.clone());
        }
    }
    let warned = events(i, &["CPU", "GPU", "Board thermals"], |e| {
        e.message.contains("°C")
    });
    cites.extend(warned.iter().map(|e| event_citation(i, e)));
    if m.thermals.is_empty() && warned.is_empty() {
        return unavailable(
            id,
            why(i, "No loaded minutes in the past week: use the PC for something demanding, then run diagnostics again."),
        );
    }
    if close.is_empty() && warned.is_empty() {
        return check(
            id,
            CheckStatus::Pass,
            "Every device stays well below its limit under load.",
            None,
            cites,
        );
    }
    let summary = if close.is_empty() {
        "Health warned about temperatures in the past week.".to_owned()
    } else {
        format!(
            "{} under load close to {} limit.",
            close.join(" and "),
            if close.len() == 1 { "its" } else { "their" }
        )
    };
    check(
        id,
        CheckStatus::Attention,
        summary,
        Some("Check that the cooler sits well and that fans and filters are clean, then compare History during a long load."),
        cites,
    )
}

fn clocks(i: &DiagInputs) -> DiagCheck {
    let id = CheckId::ClockStability;
    let Some(m) = i.measures else {
        return unavailable(id, NO_STORE);
    };
    if m.sag.is_empty() {
        return unavailable(
            id,
            why(i, "No stretch of 5 minutes or more at full load in the past week, so sag cannot be measured."),
        );
    }
    let mut cites = Vec::new();
    let mut sagging = Vec::new();
    for s in &m.sag {
        let mut c = Citation::number(
            format!("{} clock sag over long full load", s.device),
            s.pct,
            "%",
            1,
            Origin::History,
        );
        if let Some(metric) = device_metric(&s.device, "clock") {
            c = c.metric(metric);
        }
        cites.push(c);
        if s.pct >= SAG_PCT {
            sagging.push(s.device.clone());
        }
    }
    if sagging.is_empty() {
        return check(
            id,
            CheckStatus::Pass,
            "Clocks hold steady through long full load.",
            None,
            cites,
        );
    }
    check(
        id,
        CheckStatus::Attention,
        format!("The {} clock drops as long full load goes on.", sagging.join(" and ")),
        Some("Usually heat soak: compare Thermals and Throttling, and the cooling during a long load."),
        cites,
    )
}

fn power(i: &DiagInputs) -> DiagCheck {
    let id = CheckId::PowerLimits;
    let Some(m) = i.measures else {
        return unavailable(id, NO_STORE);
    };
    if m.thin.is_some() {
        return unavailable(id, why(i, ""));
    }
    let mut cites = Vec::new();
    if let Some(c) = m.power_cpu {
        cites.push(Citation::number(
            "CPU at its power limit (PPT), share of loaded time",
            c * 100.0,
            "%",
            1,
            Origin::Digests,
        ));
    }
    if let Some(g) = m.power_gpu {
        cites.push(Citation::number(
            "GPU at its power target, share of loaded time",
            g * 100.0,
            "%",
            1,
            Origin::Digests,
        ));
    }
    if cites.is_empty() {
        return unavailable(id, "Neither the CPU nor the GPU reports power-limit flags.");
    }
    check(
        id,
        CheckStatus::Info,
        "How much of the loaded time power limits held the clocks. This is how CPUs and GPUs normally keep to their power budget, not a fault.",
        None,
        cites,
    )
}

fn throttling(i: &DiagInputs) -> DiagCheck {
    let id = CheckId::Throttling;
    let Some(m) = i.measures else {
        return unavailable(id, NO_STORE);
    };
    let Some(share) = m.throttle else {
        return unavailable(
            id,
            why(i, "Nothing on this PC reports heat or current limiting."),
        );
    };
    let pct = share * 100.0;
    let cites = vec![Citation::number(
        "Loaded time limited by heat or current",
        pct,
        "%",
        1,
        Origin::Digests,
    )];
    if pct < THROTTLE_PCT {
        return check(
            id,
            CheckStatus::Pass,
            "The hardware rarely or never limited itself for heat or current.",
            None,
            cites,
        );
    }
    check(
        id,
        CheckStatus::Attention,
        "The hardware limited itself for heat or current during loaded time.",
        Some("Heat: check cooling and airflow (see Thermals). Current: often a board limit or an undervolt."),
        cites,
    )
}

fn cooling(i: &DiagInputs) -> DiagCheck {
    let id = CheckId::Cooling;
    let mut cites = Vec::new();
    let mut issues: Vec<String> = Vec::new();
    if let Some(s) = i.snapshot {
        for f in &s.fans {
            let named = f.name.to_ascii_lowercase().contains("pump");
            let label = match f.kind {
                FanKind::Pump if !named => format!("{} (pump) now", f.name),
                _ => format!("{} now", f.name),
            };
            if let Some(rpm) = f.rpm {
                cites.push(Citation::number(label, rpm, "RPM", 0, Origin::Live));
            }
            if f.status == Status::Fail {
                issues.push(format!("{} has stopped", f.name));
            }
        }
    }
    let stopped = events(i, &["Fans"], |_| true);
    if !stopped.is_empty() && issues.is_empty() {
        issues.push("Health logged a fan or pump problem in the past week".into());
    }
    cites.extend(stopped.iter().map(|e| event_citation(i, e)));
    if let Some(c) = i.measures.and_then(|m| m.cooling.as_ref()) {
        if let Some(h) = c.hotter_c {
            cites.push(
                Citation::number(
                    "CPU against its usual for the load",
                    h,
                    "°C",
                    1,
                    Origin::Insights,
                )
                .diff()
                .metric("cpu.temp.package"),
            );
            if h >= HOTTER_C && !c.hotter_chosen {
                issues.push("the CPU runs hotter than usual for its load".into());
            }
        }
        if let Some(r) = c.fan_ratio {
            cites.push(Citation::number(
                "CPU fan against its usual at that temperature",
                r * 100.0,
                "%",
                0,
                Origin::Insights,
            ));
            if r * 100.0 <= FAN_PCT && !c.fan_chosen {
                issues.push("the CPU fan spins slower than usual".into());
            }
        }
    }
    if let Some(f) = i.fans.filter(|f| f.active) {
        if let Some(r) = &f.override_reason {
            cites.push(Citation::text(
                "Fan control safety rule",
                r.clone(),
                Origin::Live,
            ));
            issues.push("a fan control safety rule runs the fans at full speed".into());
        }
    }
    if cites.is_empty() {
        return unavailable(
            id,
            "No fan readings, and no cooling baseline yet (it needs about 3 days of history).",
        );
    }
    if issues.is_empty() {
        return check(
            id,
            CheckStatus::Pass,
            "Fans and pump spin, and cooling keeps up as usual.",
            None,
            cites,
        );
    }
    let mut summary = issues.join("; ");
    if let Some(first) = summary.get(..1) {
        summary = format!("{}{}.", first.to_uppercase(), &summary[1..]);
    }
    check(
        id,
        CheckStatus::Attention,
        summary,
        Some("Check the fan and pump cables and that nothing blocks them; if a curve is set, check it in Cooling."),
        cites,
    )
}

fn memory(i: &DiagInputs) -> DiagCheck {
    let id = CheckId::Memory;
    let Some(m) = i.measures else {
        return unavailable(id, NO_STORE);
    };
    let mut cites = Vec::new();
    if let Some(mem) = i.snapshot.map(|s| &s.memory) {
        if let (Some(used), Some(total)) = (mem.used_mb, mem.total_mb.filter(|t| *t > 0.0)) {
            cites.push(
                Citation::number("RAM in use now", used / total * 100.0, "%", 0, Origin::Live)
                    .metric("memory.used"),
            );
        }
    }
    let Some(p95) = m.memory_p95 else {
        return unavailable(id, why(i, "Too little memory history in the past week."));
    };
    cites.insert(
        0,
        Citation::number("RAM in use, 95th percentile", p95, "%", 0, Origin::History)
            .metric("memory.used"),
    );
    if p95 < MEMORY_PCT {
        return check(
            id,
            CheckStatus::Pass,
            "There is memory to spare most of the time.",
            None,
            cites,
        );
    }
    check(
        id,
        CheckStatus::Attention,
        "RAM gets nearly full: Windows then pages to disk and everything slows down.",
        Some("Check Processes for what holds memory, close what you do not need, or add RAM."),
        cites,
    )
}

fn health_word(s: Status) -> &'static str {
    match s {
        Status::Ok => "good",
        Status::Warn => "caution",
        Status::Fail => "bad",
        Status::Unknown => "unknown",
    }
}

fn storage(i: &DiagInputs) -> DiagCheck {
    let id = CheckId::Storage;
    let drives: Vec<&DriveInfo> = i.drives.iter().filter(|d| !d.removable).collect();
    if drives.is_empty() {
        return unavailable(id, "No internal drives were found.");
    }
    let mut cites = Vec::new();
    let mut issues = Vec::new();
    for d in &drives {
        let name = d.model.trim();
        cites.push(
            Citation::text(
                format!("{name}: health"),
                health_word(d.health),
                Origin::Smart,
            )
            .metric(format!("storage.{}.health", d.id)),
        );
        if matches!(d.health, Status::Warn | Status::Fail) {
            issues.push(format!("{name} reports {} health", health_word(d.health)));
        }
        if d.ssd == Some(true) {
            if let Some(life) = d.health_pct {
                cites.push(Citation::number(
                    format!("{name}: life left"),
                    life,
                    "%",
                    0,
                    Origin::Smart,
                ));
                if life < LIFE_PCT {
                    issues.push(format!("{name} is near the end of its rated life"));
                }
            }
        }
        let live = i
            .snapshot
            .and_then(|s| s.storage.iter().find(|x| x.id == d.id))
            .and_then(|x| x.temp_c);
        if let Some(t) = live.or(d.temp_c) {
            cites.push(
                Citation::number(format!("{name}: temperature now"), t, "°C", 0, Origin::Live)
                    .metric(format!("storage.{}.temp", d.id)),
            );
            if t >= DRIVE_C {
                issues.push(format!("{name} runs hot"));
            }
        }
    }
    if let Some(free) = i
        .measures
        .and_then(|m| m.storage.as_ref())
        .and_then(|s| s.system_free_pct)
    {
        cites.push(Citation::number(
            "Free on the system drive",
            free,
            "%",
            0,
            Origin::Live,
        ));
        if free < FREE_PCT {
            issues.push("the system drive is nearly full".into());
        }
    }
    if issues.is_empty() {
        return check(
            id,
            CheckStatus::Pass,
            "Drives report good health, with life, room and temperatures to spare.",
            None,
            cites,
        );
    }
    let mut summary = issues.join("; ");
    if let Some(first) = summary.get(..1) {
        summary = format!("{}{}.", first.to_uppercase(), &summary[1..]);
    }
    check(
        id,
        CheckStatus::Attention,
        summary,
        Some("Back up what matters first. Storage shows each drive's details; a hot drive needs airflow or a heatsink."),
        cites,
    )
}

fn network(i: &DiagInputs) -> DiagCheck {
    let id = CheckId::Network;
    let Some(m) = i.measures else {
        return unavailable(id, NO_STORE);
    };
    let mut cites = Vec::new();
    if let Some(p) = m.ping_p95_ms {
        cites.push(
            Citation::number("Ping, 95th percentile", p, "ms", 0, Origin::History)
                .metric("network.ping"),
        );
    }
    if let Some(l) = m.loss_pct {
        cites.push(
            Citation::number("Lost replies", l, "%", 1, Origin::History).metric("network.loss"),
        );
    }
    if cites.is_empty() {
        return unavailable(
            id,
            why(
                i,
                "Ping is off or the PC was offline: turn ping on in Network.",
            ),
        );
    }
    let slow = m.ping_p95_ms.is_some_and(|p| p >= PING_MS);
    let lossy = m.loss_pct.is_some_and(|l| l >= LOSS_PCT);
    if !slow && !lossy {
        return check(
            id,
            CheckStatus::Pass,
            "The connection answers quickly and reliably.",
            None,
            cites,
        );
    }
    let summary = match (slow, lossy) {
        (true, true) => "The connection is slow and loses replies.",
        (true, false) => "The connection is slow at times.",
        _ => "The connection loses replies.",
    };
    check(
        id,
        CheckStatus::Attention,
        summary,
        Some("Compare with another device on the same network: Wi-Fi signal, the router or the provider are the usual causes."),
        cites,
    )
}

fn hardware(i: &DiagInputs) -> DiagCheck {
    let id = CheckId::HardwareErrors;
    let Some(s) = i.measures.and_then(|m| m.stability) else {
        return unavailable(id, "The Windows System log could not be read.");
    };
    let cites = vec![
        Citation::number(
            "Unexpected shutdowns (Kernel-Power 41), 30 days",
            f64::from(s.shutdowns),
            "",
            0,
            Origin::EventLog,
        ),
        Citation::number(
            "Corrected hardware errors (WHEA), 30 days",
            f64::from(s.corrected),
            "",
            0,
            Origin::EventLog,
        ),
        Citation::number(
            "Fatal hardware errors (WHEA), 30 days",
            f64::from(s.fatal),
            "",
            0,
            Origin::EventLog,
        ),
    ];
    if s.shutdowns == 0 && s.fatal == 0 && s.corrected < CORRECTED {
        return check(
            id,
            CheckStatus::Pass,
            "No crashes and no serious hardware errors in 30 days.",
            None,
            cites,
        );
    }
    let summary = if s.fatal > 0 || s.corrected >= CORRECTED {
        "The CPU or chipset reported hardware errors."
    } else {
        "The PC stopped without shutting down."
    };
    check(
        id,
        CheckStatus::Attention,
        summary,
        Some("An unstable overclock or undervolt, memory or heat are the usual causes: the Benchmark's Full mode tests stability. Unexpected shutdowns can also be power cuts."),
        cites,
    )
}

fn confidence(c: Confidence) -> &'static str {
    match c {
        Confidence::High => "high confidence",
        Confidence::Medium => "medium confidence",
        Confidence::Low => "low confidence",
    }
}

fn changes(i: &DiagInputs) -> DiagCheck {
    let id = CheckId::Changes;
    let Some(ins) = i.insights else {
        return unavailable(id, "Insights need about a day of history.");
    };
    let warnings: Vec<_> = ins
        .findings
        .iter()
        .filter(|f| f.status == Status::Warn)
        .collect();
    if warnings.is_empty() {
        return check(
            id,
            CheckStatus::Pass,
            "Nothing has changed from usual.",
            None,
            Vec::new(),
        );
    }
    let mut cites = Vec::new();
    for f in warnings.iter().take(MAX_FINDINGS) {
        cites.push(Citation::text(
            f.title.clone(),
            confidence(f.confidence),
            Origin::Insights,
        ));
        for e in &f.evidence {
            let mut c = Citation::number(
                e.label.clone(),
                e.value,
                &e.unit,
                if e.value.abs() < 10.0 { 1 } else { 0 },
                Origin::History,
            )
            .ago(i.now - e.to);
            c.metric.clone_from(&e.metric);
            cites.push(c);
        }
    }
    check(
        id,
        CheckStatus::Attention,
        format!(
            "{} finding{} worth a look in Insights.",
            warnings.len(),
            if warnings.len() == 1 { "" } else { "s" }
        ),
        Some("Insights shows each finding with its evidence and the likely causes."),
        cites,
    )
}

fn drivers(i: &DiagInputs) -> DiagCheck {
    let id = CheckId::Drivers;
    let mut cites: Vec<Citation> = i
        .app
        .drivers
        .iter()
        .map(|d| {
            let text = match (&d.version, &d.date) {
                (Some(v), Some(date)) => format!("{v} ({date})"),
                (Some(v), None) => v.clone(),
                (None, Some(date)) => format!("dated {date}"),
                (None, None) => "unknown".into(),
            };
            Citation::text(format!("{} driver", d.name), text, Origin::Inventory)
        })
        .collect();
    if let Some(w) = &i.app.windows {
        cites.push(Citation::text("Windows", w.clone(), Origin::Inventory));
    }
    if cites.is_empty() {
        return unavailable(id, "No driver information was found.");
    }
    check(
        id,
        CheckStatus::Info,
        "The versions in use, for comparing with a later report or a support request.",
        None,
        cites,
    )
}

fn pawnio_word(s: PawnIoState) -> &'static str {
    match s {
        PawnIoState::Ready => "ready",
        PawnIoState::NotInstalled => "not installed",
        PawnIoState::Outdated => "outdated",
        PawnIoState::NotRunning => "not running",
        PawnIoState::NoAccess => "no access",
        PawnIoState::Error => "error",
        PawnIoState::Unsupported => "not applicable",
    }
}

fn sensors(i: &DiagInputs) -> DiagCheck {
    let id = CheckId::Sensors;
    let Some(s) = i.snapshot else {
        return unavailable(id, "No readings yet.");
    };
    let read = |ok: bool| if ok { "read" } else { "not read" };
    let mut cites = Vec::new();
    if let Some(p) = i.pawnio {
        cites.push(Citation::text("PawnIO", pawnio_word(p.state), Origin::Live));
    }
    let cpu_temp = s.cpu.temp_package.is_some();
    cites.push(Citation::text(
        "CPU temperature",
        read(cpu_temp),
        Origin::Live,
    ));
    cites.push(Citation::text(
        "CPU power",
        read(s.cpu.power_pkg_w.is_some()),
        Origin::Live,
    ));
    cites.push(Citation::number(
        "Board fans and pumps read",
        s.fans.iter().filter(|f| f.rpm.is_some()).count() as f64,
        "",
        0,
        Origin::Live,
    ));
    cites.push(Citation::number(
        "Voltages read",
        s.voltages.len() as f64,
        "",
        0,
        Origin::Live,
    ));
    cites.push(Citation::number(
        "RAM modules with a temperature",
        s.memory.dimms.iter().filter(|d| d.temp_c.is_some()).count() as f64,
        "",
        0,
        Origin::Live,
    ));
    let smart = i
        .drives
        .iter()
        .filter(|d| d.health != Status::Unknown)
        .count();
    cites.push(Citation::number(
        "Drives with health data",
        smart as f64,
        "",
        0,
        Origin::Smart,
    ));
    if cpu_temp {
        return check(
            id,
            CheckStatus::Pass,
            "The key sensors report.",
            None,
            cites,
        );
    }
    // An Intel CPU's: PawnIO, which the advice below is about, was there all along (2.0.4).
    if i.pawnio.is_some_and(|p| p.cpu_unread) {
        return check(
            id,
            CheckStatus::Info,
            "This CPU's temperature is not read (Vyro reads AMD Ryzen CPUs), so the checks that need it lack data.",
            None,
            cites,
        );
    }
    check(
        id,
        CheckStatus::Attention,
        "The CPU temperature is not read, so most checks lack data.",
        Some("Install or repair PawnIO (the banner at the top shows how); the Vyro service needs it for CPU, board and RAM sensors."),
        cites,
    )
}

fn service(i: &DiagInputs) -> DiagCheck {
    let id = CheckId::Service;
    let app = i.app;
    let mut cites = vec![Citation::text(
        "Vyro app",
        app.app_version.clone(),
        Origin::Service,
    )];
    if !app.service {
        if let Some(d) = &app.service_detail {
            cites.push(Citation::text("Service", d.clone(), Origin::Service));
        }
        return check(
            id,
            CheckStatus::Attention,
            "Vyro runs its own engine, without the service's sensors (CPU temperature, SMART, fan control).",
            Some("Start the Vyro service or reinstall Vyro; the service starts with Windows."),
            cites,
        );
    }
    let ver = app
        .service_version
        .clone()
        .unwrap_or_else(|| "unknown".into());
    cites.push(Citation::text("Vyro service", ver.clone(), Origin::Service));
    if app
        .service_version
        .as_deref()
        .is_some_and(|v| v != app.app_version)
    {
        return check(
            id,
            CheckStatus::Attention,
            "The app and the service have different versions.",
            Some("Reinstall Vyro so that both match."),
            cites,
        );
    }
    check(
        id,
        CheckStatus::Pass,
        "The app reaches the engine through the Vyro service.",
        None,
        cites,
    )
}

/// "Vyro service 0.4.0" or "the app's own engine".
pub fn engine_label(app: &DiagAppInfo) -> String {
    if app.service {
        format!(
            "Vyro service {}",
            app.service_version.as_deref().unwrap_or("?")
        )
    } else {
        "the app's own engine".into()
    }
}
