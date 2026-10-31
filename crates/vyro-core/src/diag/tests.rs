use super::checks::{evaluate, DiagInputs};
use super::export::{html, json, markdown, ExportOptions};
use super::pdf::{pdf, text_runs};
use super::*;
use crate::health::HealthEvent;
use crate::info::{DriveInfo, PawnIoState, PawnIoStatus};
use crate::intel::{Basis, Evidence, Finding, FindingKind, Insights};
use crate::score::{Cooling, Headroom, Measures, Sag, StabilityCounts, StorageHealth};
use crate::snapshot::{FanSnapshot, SensorSnapshot, Status, StorageKind};

const NOW: i64 = 1_800_000_000;

fn app() -> DiagAppInfo {
    DiagAppInfo {
        app_version: "0.4.0".into(),
        service: true,
        service_version: Some("0.4.0".into()),
        service_detail: None,
        pc: vec![Field {
            label: "CPU".into(),
            value: "AMD Ryzen 7 9700X".into(),
        }],
        drivers: vec![GpuDriver {
            name: "NVIDIA GeForce RTX 5060 Ti".into(),
            version: Some("576.02".into()),
            date: Some("2025-09-30".into()),
        }],
        windows: Some("Windows 11 Pro 24H2 26100.4652".into()),
    }
}

fn healthy() -> Measures {
    Measures {
        thermals: vec![
            Headroom {
                device: "CPU".into(),
                median_c: 78.0,
                limit_c: 95.0,
            },
            Headroom {
                device: "GPU #0".into(),
                median_c: 62.0,
                limit_c: 85.0,
            },
        ],
        sag: vec![Sag {
            device: "CPU".into(),
            pct: 1.4,
        }],
        throttle: Some(0.0),
        cooling: Some(Cooling {
            hotter_c: Some(1.0),
            fan_ratio: Some(0.98),
            hotter_chosen: false,
            fan_chosen: false,
        }),
        storage: Some(StorageHealth {
            worst: Some(Status::Ok),
            life_pct: Some(96.0),
            system_free_pct: Some(41.0),
        }),
        memory_p95: Some(61.0),
        ping_p95_ms: Some(18.0),
        loss_pct: Some(0.1),
        stability: Some(StabilityCounts::default()),
        deviation: Some(Vec::new()),
        thin: None,
        power_cpu: Some(0.42),
        power_gpu: Some(0.12),
    }
}

fn snapshot() -> SensorSnapshot {
    let mut s = SensorSnapshot::default();
    s.cpu.temp_package = Some(48.0);
    s.cpu.power_pkg_w = Some(41.0);
    s.memory.used_mb = Some(12_000.0);
    s.memory.total_mb = Some(32_000.0);
    s.fans.push(FanSnapshot {
        name: "CPU fan".into(),
        rpm: Some(1100.0),
        status: Status::Ok,
        ..FanSnapshot::default()
    });
    s
}

fn drive(health: Status, life: f64, temp: f64) -> DriveInfo {
    DriveInfo {
        id: "disk0".into(),
        model: "Samsung SSD 990 PRO 2TB".into(),
        kind: StorageKind::Nvme,
        ssd: Some(true),
        health,
        health_pct: Some(life),
        temp_c: Some(temp),
        ..DriveInfo::default()
    }
}

fn pawnio(state: PawnIoState) -> PawnIoStatus {
    PawnIoStatus {
        state,
        ..PawnIoStatus::default()
    }
}

struct Fixture {
    measures: Option<Measures>,
    insights: Option<Insights>,
    events: Vec<HealthEvent>,
    snapshot: SensorSnapshot,
    drives: Vec<DriveInfo>,
    pawnio: PawnIoStatus,
    app: DiagAppInfo,
}

impl Fixture {
    fn healthy() -> Self {
        Self {
            measures: Some(healthy()),
            insights: Some(Insights {
                ts: NOW,
                ..Insights::default()
            }),
            events: Vec::new(),
            snapshot: snapshot(),
            drives: vec![drive(Status::Ok, 96.0, 41.0)],
            pawnio: pawnio(PawnIoState::Ready),
            app: app(),
        }
    }

    fn run(&self) -> Vec<DiagCheck> {
        evaluate(&DiagInputs {
            now: NOW,
            measures: self.measures.as_ref(),
            insights: self.insights.as_ref(),
            health_events: &self.events,
            snapshot: Some(&self.snapshot),
            drives: &self.drives,
            pawnio: Some(&self.pawnio),
            fans: None,
            app: &self.app,
        })
    }
}

fn status(checks: &[DiagCheck], id: CheckId) -> CheckStatus {
    checks
        .iter()
        .find(|c| c.id == id)
        .map(|c| c.status)
        .unwrap()
}

#[test]
fn a_healthy_pc_passes_with_power_limits_and_drivers_as_info() {
    let checks = Fixture::healthy().run();
    assert_eq!(checks.len(), CheckId::ALL.len());
    assert_eq!(
        checks.iter().map(|c| c.id).collect::<Vec<_>>(),
        CheckId::ALL
    );
    for c in &checks {
        let want = match c.id {
            CheckId::PowerLimits | CheckId::Drivers => CheckStatus::Info,
            _ => CheckStatus::Pass,
        };
        assert_eq!(c.status, want, "{:?}: {}", c.id, c.summary);
    }
    let thermals = &checks[0];
    assert_eq!(
        thermals.citations[0].metric.as_deref(),
        Some("cpu.temp.package")
    );
    assert_eq!(thermals.citations[3].value, Some(23.0), "GPU headroom");
    assert!(thermals.citations[3].difference);
}

#[test]
fn every_problem_is_attention_and_cites_its_telemetry() {
    let mut f = Fixture::healthy();
    let m = f.measures.as_mut().unwrap();
    m.thermals[0].median_c = 88.0;
    m.sag[0].pct = 9.8;
    m.throttle = Some(0.06);
    m.cooling = Some(Cooling {
        hotter_c: Some(7.0),
        fan_ratio: Some(0.6),
        hotter_chosen: false,
        fan_chosen: false,
    });
    m.memory_p95 = Some(95.0);
    m.storage = Some(StorageHealth {
        worst: Some(Status::Warn),
        life_pct: Some(4.0),
        system_free_pct: Some(6.0),
    });
    m.ping_p95_ms = Some(220.0);
    m.stability = Some(StabilityCounts {
        shutdowns: 1,
        corrected: 3,
        fatal: 0,
    });
    f.drives = vec![drive(Status::Warn, 4.0, 74.0)];
    f.insights = Some(Insights {
        ts: NOW,
        findings: vec![Finding {
            key: "hotter".into(),
            kind: FindingKind::Hotter,
            status: Status::Warn,
            basis: Basis::Measured,
            confidence: crate::intel::Confidence::High,
            title: "CPU runs hotter than usual".into(),
            detail: String::new(),
            ts: None,
            since: None,
            evidence: vec![Evidence {
                label: "CPU at 40 % load".into(),
                metric: Some("cpu.temp.package".into()),
                from: NOW - 1800,
                to: NOW,
                value: 81.0,
                baseline: Some(72.0),
                unit: "°C".into(),
            }],
            causes: Vec::new(),
            related: Vec::new(),
        }],
        ..Insights::default()
    });
    f.events.push(HealthEvent {
        ts: NOW - 3 * 86_400,
        subsystem: "Fans".into(),
        status: Status::Fail,
        message: "CPU fan stopped".into(),
    });
    f.events.push(HealthEvent {
        ts: NOW - 9 * 86_400,
        subsystem: "CPU".into(),
        status: Status::Warn,
        message: "CPU at 91 °C".into(),
    });
    f.snapshot.cpu.temp_package = None;
    f.pawnio = pawnio(PawnIoState::NotInstalled);
    f.app.service_version = Some("0.3.0".into());
    let checks = f.run();
    for id in [
        CheckId::Thermals,
        CheckId::ClockStability,
        CheckId::Throttling,
        CheckId::Cooling,
        CheckId::Memory,
        CheckId::Storage,
        CheckId::Network,
        CheckId::HardwareErrors,
        CheckId::Changes,
        CheckId::Sensors,
        CheckId::Service,
    ] {
        assert_eq!(status(&checks, id), CheckStatus::Attention, "{id:?}");
    }
    for c in checks.iter().filter(|c| c.status == CheckStatus::Attention) {
        assert!(!c.citations.is_empty(), "{:?} cites nothing", c.id);
        assert!(c.advice.is_some(), "{:?} says what to do", c.id);
        assert!(
            !c.summary.contains("°C"),
            "summaries carry no temperatures: {}",
            c.summary
        );
    }
    let cooling = checks.iter().find(|c| c.id == CheckId::Cooling).unwrap();
    assert!(cooling
        .citations
        .iter()
        .any(|c| c.source == Origin::HealthLog && c.ago == Some(3 * 86_400)));
    // The CPU warning 9 days ago is outside the week.
    let thermals = checks.iter().find(|c| c.id == CheckId::Thermals).unwrap();
    assert!(thermals
        .citations
        .iter()
        .all(|c| c.source != Origin::HealthLog));
    assert_eq!(thermals.summary, "CPU under load close to its limit.");
    let storage = checks.iter().find(|c| c.id == CheckId::Storage).unwrap();
    assert!(
        storage
            .summary
            .starts_with("Samsung SSD 990 PRO 2TB reports caution health"),
        "{}",
        storage.summary
    );
    let changes = checks.iter().find(|c| c.id == CheckId::Changes).unwrap();
    assert_eq!(
        changes.citations[1].metric.as_deref(),
        Some("cpu.temp.package")
    );
}

#[test]
fn the_owners_choice_does_not_count_and_missing_data_says_why() {
    let mut f = Fixture::healthy();
    f.measures.as_mut().unwrap().cooling = Some(Cooling {
        hotter_c: Some(7.0),
        fan_ratio: Some(0.5),
        hotter_chosen: true,
        fan_chosen: true,
    });
    assert_eq!(status(&f.run(), CheckId::Cooling), CheckStatus::Pass);

    let mut thin = Fixture::healthy();
    thin.measures = Some(Measures {
        thin: Some("Vyro has 3 h of history; the score needs about a day.".into()),
        ..Measures::default()
    });
    thin.insights = None;
    let checks = thin.run();
    for id in [
        CheckId::Thermals,
        CheckId::ClockStability,
        CheckId::PowerLimits,
        CheckId::Throttling,
        CheckId::Memory,
        CheckId::Network,
    ] {
        let c = checks.iter().find(|c| c.id == id).unwrap();
        assert_eq!(c.status, CheckStatus::Unavailable, "{id:?}");
        assert!(
            c.advice.as_deref().unwrap().contains("3 h of history"),
            "{id:?}: {:?}",
            c.advice
        );
    }
    assert_eq!(
        status(&checks, CheckId::HardwareErrors),
        CheckStatus::Unavailable
    );
    assert_eq!(status(&checks, CheckId::Changes), CheckStatus::Unavailable);

    let mut alone = Fixture::healthy();
    alone.app.service = false;
    alone.app.service_detail = Some("The Vyro service is not running.".into());
    let c = alone.run();
    let s = c.iter().find(|c| c.id == CheckId::Service).unwrap();
    assert_eq!(s.status, CheckStatus::Attention);
    assert_eq!(checks::engine_label(&alone.app), "the app's own engine");
}

/// A CPU Vyro does not read (an Intel one) is no PawnIO problem: PawnIO is there (2.0.4).
#[test]
fn a_cpu_vyro_does_not_read_is_not_sent_to_pawnio() {
    let mut f = Fixture::healthy();
    f.snapshot.cpu.temp_package = None;
    let c = f.run();
    let s = c.iter().find(|c| c.id == CheckId::Sensors).unwrap();
    assert_eq!(s.status, CheckStatus::Attention);
    assert!(s.advice.as_deref().unwrap().contains("PawnIO"));

    f.pawnio.cpu_unread = true;
    let c = f.run();
    let s = c.iter().find(|c| c.id == CheckId::Sensors).unwrap();
    assert_eq!(s.status, CheckStatus::Info);
    assert_eq!(s.advice, None);
    assert!(
        s.summary.contains("Vyro reads AMD Ryzen CPUs"),
        "{}",
        s.summary
    );
    assert!(!s.citations.is_empty());
}

#[test]
fn citations_read_the_same_everywhere() {
    let t = Citation::number("CPU median under load", 78.0, "°C", 0, Origin::History);
    assert_eq!(fmt_citation(&t, TempUnit::C), "78 °C");
    assert_eq!(fmt_citation(&t, TempUnit::F), "172 °F");
    let d = Citation::number("headroom", 17.0, "°C", 0, Origin::History).diff();
    assert_eq!(fmt_citation(&d, TempUnit::F), "31 °F");
    assert_eq!(
        fmt_citation(
            &Citation::number("x", 0.06, "%", 1, Origin::Digests),
            TempUnit::C
        ),
        "0.1%"
    );
    assert_eq!(
        fmt_citation(
            &Citation::number("n", 3.0, "", 0, Origin::EventLog),
            TempUnit::C
        ),
        "3"
    );
    assert_eq!(
        fmt_citation(
            &Citation::text("h", "CPU at 91 °C", Origin::HealthLog),
            TempUnit::F
        ),
        "CPU at 196 °F"
    );
    let half = Citation::number("median", 78.5, "°C", 0, Origin::History);
    assert_eq!(
        fmt_citation(&half, TempUnit::C),
        "79 °C",
        "halves round up, as in the UI"
    );
    assert_eq!(
        fmt_citation(
            &Citation::number("x", -0.25, "°C", 1, Origin::Insights).diff(),
            TempUnit::C
        ),
        "-0.3 °C"
    );
    assert_eq!(fmt_ago(30), "1 min before");
    assert_eq!(fmt_ago(7200), "2 h before");
    assert_eq!(fmt_ago(3 * 86_400), "3 d before");
}

fn report() -> DiagReport {
    let mut f = Fixture::healthy();
    f.measures.as_mut().unwrap().thermals[0].median_c = 88.0;
    DiagReport {
        id: 3,
        ts: NOW,
        version: CHECKS_VERSION,
        app_version: "0.4.0".into(),
        engine: checks::engine_label(&f.app),
        pc: f.app.pc.clone(),
        checks: f.run(),
    }
}

fn opts(unit: TempUnit, serials: bool) -> ExportOptions {
    ExportOptions {
        unit,
        when: "15 Oct 2026, 10:24".into(),
        serials: if serials {
            vec![Field {
                label: "Serial: Samsung SSD 990 PRO 2TB".into(),
                value: "S6Z2NJ0W123456".into(),
            }]
        } else {
            Vec::new()
        },
    }
}

#[test]
fn every_export_shows_the_same_values() {
    let r = report();
    for unit in [TempUnit::C, TempUnit::F] {
        let o = opts(unit, false);
        let md = markdown(&r, &o);
        let page = html(&r, &o);
        let runs = text_runs(&pdf(&r, &o)).join(" ");
        for c in &r.checks {
            assert!(md.contains(export::name(c)), "{:?}", c.id);
            for x in &c.citations {
                let v = fmt_citation(x, unit);
                assert!(md.contains(&v), "markdown lacks {v}");
                assert!(page.contains(&v.replace('\'', "&#39;")), "html lacks {v}");
                assert!(runs.contains(&v), "pdf lacks {v}");
            }
        }
        let sym = if unit == TempUnit::F { "°F" } else { "°C" };
        assert!(md.contains(&format!("88 {sym}")) || md.contains("190 °F"));
        assert!(
            md.contains("1 attention · 10 pass · 2 info · 0 unavailable"),
            "{md}"
        );
        assert!(page.starts_with("<!doctype html>"));
    }
    // Serial numbers only when asked.
    let plain = opts(TempUnit::C, false);
    let with = opts(TempUnit::C, true);
    for (out_plain, out_with) in [
        (markdown(&r, &plain), markdown(&r, &with)),
        (html(&r, &plain), html(&r, &with)),
        (json(&r, &plain), json(&r, &with)),
        (
            text_runs(&pdf(&r, &plain)).join(" "),
            text_runs(&pdf(&r, &with)).join(" "),
        ),
    ] {
        assert!(!out_plain.contains("S6Z2NJ0W123456"));
        assert!(out_with.contains("S6Z2NJ0W123456"));
    }
    let back: DiagReport = serde_json::from_str(&json(&r, &plain)).unwrap();
    assert_eq!(back, r);
    // Markup in text is escaped.
    let mut odd = r.clone();
    odd.checks[0].summary = "a <b> & 'c' | d".into();
    assert!(html(&odd, &plain).contains("a &lt;b&gt; &amp; &#39;c&#39; | d"));
    assert!(markdown(&odd, &plain).contains("a <b> & 'c' | d"));
}

#[test]
fn the_pdf_is_well_formed() {
    let bytes = pdf(&report(), &opts(TempUnit::C, false));
    assert!(bytes.starts_with(b"%PDF-1.4\n"));
    assert!(bytes.ends_with(b"%%EOF\n"));
    let s: String = bytes.iter().map(|b| char::from(*b)).collect();
    // Every cross-reference entry points at its object (byte offsets in the file).
    let find = |needle: &[u8]| {
        bytes
            .windows(needle.len())
            .rposition(|w| w == needle)
            .unwrap()
    };
    let xref = find(b"\nxref\n") + 1;
    let tail = &bytes[find(b"startxref\n") + 10..];
    let start: usize = std::str::from_utf8(tail)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(start, xref);
    let table = std::str::from_utf8(&bytes[xref..]).unwrap();
    let entries: Vec<&str> = table
        .lines()
        .skip(3)
        .take_while(|l| l.ends_with(" n "))
        .collect();
    assert!(entries.len() >= 7);
    for (i, e) in entries.iter().enumerate() {
        let off: usize = e[..10].parse().unwrap();
        assert!(
            bytes[off..].starts_with(format!("{} 0 obj", i + 1).as_bytes()),
            "object {}",
            i + 1
        );
        assert_eq!(e.len() + 1, 20, "entries are 20 bytes");
    }
    let pages = s.matches("/Type /Page ").count();
    assert!(s.contains(&format!("/Count {pages}")));
    assert!(pages >= 2, "13 checks fill more than a page");
    let runs = text_runs(&bytes);
    assert!(runs.iter().any(|r| r == "Vyro diagnostics"));
    assert!(runs.iter().any(
        |r| r == &format!("Vyro diagnostics · page {pages} of {pages}").replace('·', "\u{b7}")
    ));
    // Lines stay inside the page.
    for line in s.lines().filter(|l| l.contains(" Td (")) {
        let x: f32 = line
            .split(" rg ")
            .nth(1)
            .unwrap()
            .split(' ')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!((50.0..545.0).contains(&x), "{line}");
    }
}
