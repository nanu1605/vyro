//! Causes (P16, layer 4). What could explain a finding: PC changes (P19), profile switches,
//! fan control, busy programs and other measured findings. Fixed rules decide how strongly a
//! candidate is tied to an effect, so a coincidence never reads as a cause:
//!
//! - **Likely**: a fixed table says it can produce the effect, it falls in the window where
//!   the effect began, the effect lasted, and it is the only such candidate there; or both
//!   sides are measured together (a slower fan and a hotter CPU now). Timing alone is at most
//!   medium confidence.
//! - **Possible**: one of several candidates the table allows, or one whose effect has not
//!   been shown to last.
//! - **Related**: in the window but not something that can produce the effect. Listed apart.

use crate::changes::{ChangeKind, ComponentClass, HardwareChange};
use crate::fan::FanEventKind;
use crate::profiles::Profile;

use super::{Basis, Cause, CauseKind, CauseRole, Confidence, Evidence, Inputs};

/// What a candidate is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum What {
    Bios,
    Vbios,
    GpuDriver,
    DriveFirmware,
    Memory,
    Security,
    Windows,
    /// A part added, removed or replaced.
    Part(ComponentClass),
    /// A monitor, USB drive or network adapter coming or going.
    HotPlug,
    Profile,
    FanControl,
    Program,
}

/// What is being explained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Effect {
    HotterCpu,
    HotterGpu,
    Cooling,
    Throttling {
        gpu: bool,
    },
    GameSlower,
    /// A power step or a CPU-bound game: only a program's load explains it.
    Load,
}

/// The mechanism table: whether `what` can produce `effect` at all.
pub(super) fn allowed(effect: Effect, what: What) -> bool {
    use ComponentClass as C;
    use What as W;
    match effect {
        Effect::HotterCpu => matches!(
            what,
            W::Bios | W::Memory | W::Profile | W::FanControl | W::Part(C::Cpu | C::Board)
        ),
        Effect::HotterGpu => matches!(
            what,
            W::GpuDriver | W::Vbios | W::Profile | W::FanControl | W::Part(C::Gpu)
        ),
        Effect::Cooling => matches!(
            what,
            W::Bios | W::Profile | W::FanControl | W::Part(C::Board)
        ),
        Effect::Throttling { gpu: false } => {
            matches!(what, W::Bios | W::Memory | W::Profile | W::FanControl)
        }
        Effect::Throttling { gpu: true } => {
            matches!(what, W::GpuDriver | W::Vbios | W::Profile | W::FanControl)
        }
        Effect::GameSlower => matches!(
            what,
            W::GpuDriver
                | W::Vbios
                | W::Windows
                | W::Bios
                | W::Memory
                | W::Security
                | W::Profile
                | W::Program
                | W::Part(C::Cpu | C::Gpu | C::Memory)
        ),
        Effect::Load => what == W::Program,
    }
}

/// Something that could explain a finding.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Candidate {
    pub what: What,
    pub ts: i64,
    pub text: String,
    pub change_id: Option<i64>,
    pub evidence: Vec<Evidence>,
    /// The owner chose it (a profile, fan control on or off): what it explains is expected.
    pub chosen: bool,
}

impl Candidate {
    fn kind(&self) -> CauseKind {
        match self.what {
            What::Profile => CauseKind::Profile,
            What::FanControl => CauseKind::FanControl,
            What::Program => CauseKind::Program,
            _ => CauseKind::Change,
        }
    }

    pub fn cause(&self, role: CauseRole, confidence: Confidence) -> Cause {
        Cause {
            role,
            basis: Basis::Inferred,
            confidence,
            kind: self.kind(),
            text: self.text.clone(),
            ts: Some(self.ts),
            change_id: self.change_id,
            evidence: self.evidence.clone(),
            because: Vec::new(),
        }
    }
}

pub(super) fn profile_name(p: Profile) -> &'static str {
    match p {
        Profile::Desktop => "Desktop",
        Profile::Gaming => "Gaming",
        Profile::Work => "Work",
        Profile::Benchmark => "Benchmark",
        Profile::Silent => "Silent",
    }
}

fn class_name(c: ComponentClass) -> &'static str {
    match c {
        ComponentClass::Cpu => "Processor",
        ComponentClass::Board => "Motherboard",
        ComponentClass::Memory => "Memory module",
        ComponentClass::Gpu => "Graphics card",
        ComponentClass::Drive => "Drive",
        ComponentClass::RemovableDrive => "USB drive",
        ComponentClass::Monitor => "Monitor",
        ComponentClass::Network => "Network adapter",
        ComponentClass::Windows => "Windows",
        ComponentClass::Security => "Firmware security",
    }
}

/// What a PC change is, for the mechanism table (`None`: not a cause of anything here).
fn what_of(c: &HardwareChange) -> Option<What> {
    let class = c.class?;
    let field = c.field.as_deref().unwrap_or("");
    Some(match (c.kind, class) {
        (ChangeKind::Tracking, _) => return None,
        (ChangeKind::FirstSeen | ChangeKind::Missing, _)
        | (_, ComponentClass::RemovableDrive | ComponentClass::Monitor | ComponentClass::Network) => {
            What::HotPlug
        }
        (ChangeKind::Changed, ComponentClass::Board) if field == "BIOS" => What::Bios,
        (ChangeKind::Changed, ComponentClass::Gpu) if field == "Driver" => What::GpuDriver,
        (ChangeKind::Changed, ComponentClass::Gpu) if field == "VBIOS" => What::Vbios,
        (ChangeKind::Changed, ComponentClass::Drive) => What::DriveFirmware,
        (ChangeKind::Changed, ComponentClass::Memory) => What::Memory,
        (ChangeKind::Changed, ComponentClass::Security) => What::Security,
        (ChangeKind::Changed, ComponentClass::Windows) => What::Windows,
        (ChangeKind::Changed, _) => return None,
        (ChangeKind::Added | ChangeKind::Removed | ChangeKind::Replaced, c) => What::Part(c),
    })
}

fn change_text(c: &HardwareChange, what: What) -> String {
    let field = c.field.as_deref().unwrap_or("");
    let label = match what {
        What::Bios => "BIOS".to_owned(),
        What::Vbios => "Graphics card BIOS".to_owned(),
        What::GpuDriver => "Graphics driver".to_owned(),
        What::DriveFirmware => format!("Drive firmware ({})", c.name),
        What::Memory => format!("Memory {}", field.to_lowercase()),
        What::Windows if field == "Build" => "Windows update".to_owned(),
        What::Windows => format!("Windows {}", field.to_lowercase()),
        What::Security => field.to_owned(),
        What::Part(class) => {
            let how = match c.kind {
                ChangeKind::Added => "added",
                ChangeKind::Removed => "removed",
                _ => "replaced",
            };
            return match c.kind {
                ChangeKind::Replaced => format!(
                    "{} replaced: {} → {}",
                    class_name(class),
                    c.before.as_deref().unwrap_or("?"),
                    c.after.as_deref().unwrap_or(&c.name)
                ),
                _ => format!("{} {how}: {}", class_name(class), c.name),
            };
        }
        What::HotPlug => {
            let class = c.class.map_or("Device", class_name);
            return match c.kind {
                ChangeKind::Missing => format!("{class} not seen for 30 days: {}", c.name),
                _ => format!("New {} connected: {}", class.to_lowercase(), c.name),
            };
        }
        What::Profile | What::FanControl | What::Program => String::new(),
    };
    match (&c.before, &c.after) {
        (Some(b), Some(a)) => format!("{label}: {b} → {a}"),
        _ => label,
    }
}

/// Changes, profile switches and fan control events: the candidates for every finding.
pub(super) fn candidates(inp: &Inputs) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = inp
        .changes
        .iter()
        .filter_map(|c| {
            let what = what_of(c)?;
            Some(Candidate {
                what,
                ts: c.ts,
                text: change_text(c, what),
                change_id: Some(c.id),
                evidence: Vec::new(),
                chosen: false,
            })
        })
        .collect();
    let mut last: Option<Profile> = None;
    for d in inp.digests {
        let Some(p) = d.profile else { continue };
        if let Some(prev) = last.filter(|&prev| prev != p) {
            out.push(Candidate {
                what: What::Profile,
                ts: d.ts,
                text: format!(
                    "Profile switched from {} to {}",
                    profile_name(prev),
                    profile_name(p)
                ),
                change_id: None,
                evidence: Vec::new(),
                chosen: true,
            });
        }
        last = Some(p);
    }
    for e in inp.fan_events {
        let chosen = match e.kind {
            FanEventKind::On | FanEventKind::Off | FanEventKind::Bios | FanEventKind::Profile => {
                true
            }
            FanEventKind::Fault | FanEventKind::Watchdog | FanEventKind::Recovered => false,
            _ => continue,
        };
        out.push(Candidate {
            what: What::FanControl,
            ts: e.ts,
            text: e.message.clone(),
            change_id: None,
            evidence: Vec::new(),
            chosen,
        });
    }
    out
}

/// Where an effect's cause must lie, and whether the effect has been shown to last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Window {
    pub lo: i64,
    pub hi: i64,
    pub lasted: bool,
}

/// A measured finding that explains another, with whether its own cause was the owner's
/// choice and the candidates it already accounts for.
pub(super) struct Link {
    pub cause: Cause,
    pub chosen: bool,
    pub uses: Vec<(What, i64)>,
}

#[derive(Debug, Default)]
pub(super) struct Explained {
    pub causes: Vec<Cause>,
    pub related: Vec<Cause>,
    /// The likely cause was the owner's own choice: the effect is expected.
    pub chosen: bool,
    /// Candidates used as causes (so a finding that builds on this one does not count them
    /// again).
    pub uses: Vec<(What, i64)>,
}

/// Most causes and related items shown per finding.
const MAX_CAUSES: usize = 4;
const MAX_RELATED: usize = 3;

pub(super) fn explain(
    effect: Effect,
    w: Window,
    cands: &[Candidate],
    links: Vec<Link>,
) -> Explained {
    let skip: Vec<(What, i64)> = links.iter().flat_map(|l| l.uses.iter().copied()).collect();
    let near: Vec<&Candidate> = cands
        .iter()
        .filter(|c| c.ts >= w.lo && c.ts <= w.hi && !skip.contains(&(c.what, c.ts)))
        .collect();
    let (able, other): (Vec<&Candidate>, Vec<&Candidate>) =
        near.into_iter().partition(|c| allowed(effect, c.what));
    let mut out = Explained::default();
    let measured = !links.is_empty();
    for l in links {
        out.chosen |= l.chosen;
        out.uses.extend(l.uses);
        out.causes.push(l.cause);
    }
    let single = able.len() == 1 && !measured;
    for c in &able {
        let cause = if single && w.lasted {
            out.chosen |= c.chosen;
            c.cause(CauseRole::Likely, Confidence::Medium)
        } else {
            c.cause(CauseRole::Possible, Confidence::Low)
        };
        out.uses.push((c.what, c.ts));
        out.causes.push(cause);
    }
    let rank = |c: &Cause| match c.role {
        CauseRole::Likely => 0,
        CauseRole::Possible => 1,
        CauseRole::Related => 2,
    };
    out.causes
        .sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| b.ts.cmp(&a.ts)));
    out.causes.truncate(MAX_CAUSES);
    let mut related: Vec<&Candidate> = other;
    related.sort_by_key(|c| std::cmp::Reverse(c.ts));
    out.related = related
        .into_iter()
        .take(MAX_RELATED)
        .map(|c| c.cause(CauseRole::Related, Confidence::Low))
        .collect();
    out
}

/// Keeps a chain to three causes deep.
pub(super) fn trim(mut causes: Vec<Cause>, depth: usize) -> Vec<Cause> {
    if depth == 0 {
        return Vec::new();
    }
    for c in &mut causes {
        c.because = trim(std::mem::take(&mut c.because), depth - 1);
    }
    causes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(what: What, ts: i64) -> Candidate {
        Candidate {
            what,
            ts,
            text: format!("{what:?}"),
            change_id: None,
            evidence: Vec::new(),
            chosen: what == What::Profile,
        }
    }

    const W: Window = Window {
        lo: 100,
        hi: 200,
        lasted: true,
    };

    #[test]
    fn one_able_candidate_in_the_window_is_likely_but_never_high() {
        let e = explain(Effect::HotterCpu, W, &[cand(What::Bios, 150)], Vec::new());
        assert_eq!(e.causes.len(), 1);
        assert_eq!(e.causes[0].role, CauseRole::Likely);
        assert_eq!(e.causes[0].confidence, Confidence::Medium);
        assert!(!e.chosen);
        // The same, but the effect has not been shown to last: only possible.
        let short = Window { lasted: false, ..W };
        let e = explain(
            Effect::HotterCpu,
            short,
            &[cand(What::Bios, 150)],
            Vec::new(),
        );
        assert_eq!(e.causes[0].role, CauseRole::Possible);
    }

    #[test]
    fn several_able_candidates_are_each_possible_and_the_rest_related() {
        let c = [
            cand(What::Bios, 120),
            cand(What::Memory, 130),
            cand(What::HotPlug, 140),
            cand(What::GpuDriver, 150),
            cand(What::Bios, 260),
        ];
        let e = explain(Effect::HotterCpu, W, &c, Vec::new());
        assert!(e.causes.iter().all(|c| c.role == CauseRole::Possible));
        assert_eq!(e.causes.len(), 2);
        // A monitor and a graphics driver cannot heat the CPU; the late BIOS is out of the window.
        assert_eq!(e.related.len(), 2);
        assert!(e.related.iter().all(|c| c.role == CauseRole::Related));
        assert!(e.related.iter().all(|c| c.confidence == Confidence::Low));
    }

    #[test]
    fn a_profile_switch_is_the_owners_choice() {
        let e = explain(Effect::Cooling, W, &[cand(What::Profile, 150)], Vec::new());
        assert_eq!(e.causes[0].kind, CauseKind::Profile);
        assert!(e.chosen);
    }

    #[test]
    fn a_change_never_explains_what_it_cannot_produce() {
        for w in [What::HotPlug, What::DriveFirmware] {
            for e in [
                Effect::HotterCpu,
                Effect::HotterGpu,
                Effect::Cooling,
                Effect::GameSlower,
            ] {
                assert!(!allowed(e, w), "{w:?} {e:?}");
            }
        }
        assert!(!allowed(Effect::Load, What::Bios));
        assert!(allowed(Effect::GameSlower, What::GpuDriver));
    }
}
