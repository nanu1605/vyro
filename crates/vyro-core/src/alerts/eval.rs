//! Evaluating the rules each tick: holds, hysteresis, rates, cooldowns, what stays active and
//! which alerts repeat one just shown.

use std::collections::{HashMap, VecDeque};

use super::{
    diagnostic, part, show, validate, ActiveAlert, AlertBrief, AlertFired, AlertKind, AlertRule,
    Condition, ConditionTest, Reading, TickInputs, CLEAR_SECS, DUPLICATE_SECS, MAX_LOG,
};
use crate::health::HealthEvent;
use crate::snapshot::Status;

#[derive(Debug, Default, Clone)]
struct RuleState {
    /// When the conditions began to hold (for the hold time).
    since: Option<i64>,
    /// Fired, and the conditions have not cleared since.
    latched: bool,
    /// Last firing (for the cooldown).
    last: Option<i64>,
    /// When it fired, while its conditions still hold or cleared less than [`CLEAR_SECS`] ago.
    active: Option<i64>,
    /// Since when the conditions have not held, while active.
    clear_since: Option<i64>,
    /// Whether each condition holds (it clears only past its hysteresis).
    held: Vec<bool>,
}

#[derive(Debug)]
pub struct AlertEngine {
    rules: Vec<AlertRule>,
    state: HashMap<String, RuleState>,
    log: VecDeque<AlertFired>,
    /// Recent readings of the sensors rate conditions watch: (unix seconds, value).
    recent: HashMap<String, VecDeque<(i64, f64)>>,
    /// Alerts shown lately: (when, part, title).
    shown: VecDeque<(i64, String, String)>,
    brief: AlertBrief,
}

/// A condition measured against its limit: it trips when `x` passes `limit` (`strict`: goes
/// beyond it), and once held clears only when `x` falls `hysteresis` short of it.
struct Measure {
    x: f64,
    limit: f64,
    strict: bool,
    text: String,
}

fn measure(
    c: &Condition,
    r: &Reading,
    recent: Option<&VecDeque<(i64, f64)>>,
    usual: Option<f64>,
    ts: i64,
) -> Option<Measure> {
    let v = r.value;
    let at = |x: f64| show(x, r.unit);
    Some(match c.test {
        ConditionTest::Above { value } => Measure {
            x: v,
            limit: value,
            strict: true,
            text: format!("{} at {} (above {})", r.name, at(v), at(value)),
        },
        ConditionTest::Below { value } => Measure {
            x: -v,
            limit: -value,
            strict: true,
            text: format!("{} at {} (below {})", r.name, at(v), at(value)),
        },
        ConditionTest::Rises { by, secs } | ConditionTest::Falls { by, secs } => {
            let rises = matches!(c.test, ConditionTest::Rises { .. });
            let from = ts - i64::from(secs);
            let window = recent?.iter().filter(|(t, _)| *t >= from).map(|(_, x)| *x);
            let edge = if rises {
                window.fold(v, f64::min)
            } else {
                window.fold(v, f64::max)
            };
            Measure {
                x: if rises { v - edge } else { edge - v },
                limit: by,
                strict: false,
                text: format!(
                    "{} {} from {} to {} within {secs} s",
                    r.name,
                    if rises { "rose" } else { "fell" },
                    at(edge),
                    at(v)
                ),
            }
        }
        ConditionTest::AboveUsual { by } | ConditionTest::BelowUsual { by } => {
            let u = usual?;
            let above = matches!(c.test, ConditionTest::AboveUsual { .. });
            Measure {
                x: if above { v - u } else { u - v },
                limit: by,
                strict: false,
                text: format!("{} at {}, usually {}", r.name, at(v), at(u)),
            }
        }
    })
}

impl Measure {
    fn holds(&self, held: bool, hysteresis: f64) -> bool {
        let limit = if held {
            self.limit - hysteresis.max(0.0)
        } else {
            self.limit
        };
        if self.strict {
            self.x > limit
        } else {
            self.x >= limit
        }
    }
}

/// All (or any) of several conditions that are each true, false or unknown (no reading).
fn combine(any: bool, results: &[Option<bool>]) -> Option<bool> {
    let wanted = any;
    if results.is_empty() {
        None
    } else if results.contains(&Some(wanted)) {
        Some(wanted)
    } else if results.iter().all(|r| *r == Some(!wanted)) {
        Some(!wanted)
    } else {
        None
    }
}

impl AlertEngine {
    pub fn new(rules: Vec<AlertRule>) -> Self {
        Self {
            rules,
            state: HashMap::new(),
            log: VecDeque::new(),
            recent: HashMap::new(),
            shown: VecDeque::new(),
            brief: AlertBrief::default(),
        }
    }

    pub fn rules(&self) -> &[AlertRule] {
        &self.rules
    }

    /// Replaces the rule set. State carries over for rules whose id and conditions are
    /// unchanged.
    pub fn set_rules(&mut self, rules: Vec<AlertRule>) -> Result<(), String> {
        validate(&rules)?;
        let old: HashMap<&str, &AlertRule> =
            self.rules.iter().map(|r| (r.id.as_str(), r)).collect();
        self.state.retain(|key, _| {
            let id = key.split('\u{1f}').next().unwrap_or(key);
            rules
                .iter()
                .any(|r| r.id == id && old.get(id).is_some_and(|o| o.kind == r.kind))
        });
        self.rules = rules;
        self.brief = self.build_brief();
        Ok(())
    }

    /// Alerts fired recently, oldest first.
    pub fn log(&self) -> Vec<AlertFired> {
        self.log.iter().cloned().collect()
    }

    /// What holds now, as of the last tick.
    pub fn brief(&self) -> AlertBrief {
        self.brief.clone()
    }

    /// Sensors whose usual reading an enabled rule compares with.
    pub fn usual_sensors(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for r in self.rules.iter().filter(|r| r.enabled) {
            if let AlertKind::Conditions { conditions, .. } = &r.kind {
                for c in conditions.iter().filter(|c| c.test.needs_usual()) {
                    if !out.contains(&c.sensor) {
                        out.push(c.sensor.clone());
                    }
                }
            }
        }
        out
    }

    fn cooled(last: Option<i64>, ts: i64, cooldown: u32) -> bool {
        last.is_none_or(|last| ts - last >= i64::from(cooldown))
    }

    /// Checks every enabled rule against this tick's readings and new health events.
    pub fn evaluate(
        &mut self,
        ts: i64,
        reading: impl Fn(&str) -> Option<Reading>,
        health: &[HealthEvent],
    ) -> Vec<AlertFired> {
        self.evaluate_with(
            ts,
            reading,
            &TickInputs {
                health,
                ..TickInputs::default()
            },
        )
    }

    /// Checks every enabled rule against this tick: readings, health changes, usual readings,
    /// whether Vyro drives fans and the programs running.
    pub fn evaluate_with(
        &mut self,
        ts: i64,
        reading: impl Fn(&str) -> Option<Reading>,
        inp: &TickInputs,
    ) -> Vec<AlertFired> {
        let reading: &dyn Fn(&str) -> Option<Reading> = &reading;
        self.track_rates(ts, reading);
        let mut fired = Vec::new();
        let mut parts: Vec<(Vec<String>, bool)> = Vec::new();
        for rule in &self.rules {
            if !rule.enabled {
                if let Some(st) = self.state.get_mut(&rule.id) {
                    st.active = None;
                    st.clear_since = None;
                }
                continue;
            }
            if let AlertKind::Health { problems } = &rule.kind {
                for e in inp.health {
                    let wanted = match e.status {
                        Status::Fail => true,
                        Status::Warn => !problems,
                        _ => false,
                    };
                    if !wanted {
                        continue;
                    }
                    let key = format!("{}\u{1f}{}", rule.id, e.subsystem);
                    let st = self.state.entry(key).or_default();
                    if !Self::cooled(st.last, ts, rule.cooldown_secs) {
                        continue;
                    }
                    st.last = Some(ts);
                    let mut f = AlertFired::new(
                        ts,
                        &rule.id,
                        format!(
                            "{} {}",
                            e.subsystem,
                            if e.status == Status::Fail {
                                "problem"
                            } else {
                                "warning"
                            }
                        ),
                        e.message.clone(),
                        rule.sound,
                    );
                    f.notify = rule.actions.notify;
                    if rule.actions.record {
                        f.diagnostic = Some(diagnostic(rule, reading, inp.processes));
                    }
                    parts.push((vec![e.subsystem.clone()], e.status == Status::Fail));
                    fired.push(f);
                }
                continue;
            }

            let (any, conditions) = rule.conditions();
            let st = self.state.entry(rule.id.clone()).or_default();
            st.held.resize(conditions.len(), false);
            let mut results = Vec::with_capacity(conditions.len());
            let mut texts = Vec::new();
            for (c, held) in conditions.iter().zip(st.held.iter_mut()) {
                let Some(r) = reading(&c.sensor) else {
                    results.push(None);
                    continue;
                };
                let usual = if c.test.needs_usual() {
                    inp.usual.and_then(|u| u(&c.sensor))
                } else {
                    None
                };
                let Some(m) = measure(c, &r, self.recent.get(&c.sensor), usual, ts) else {
                    results.push(None);
                    continue;
                };
                *held = m.holds(*held, c.hysteresis);
                if *held {
                    texts.push(m.text);
                }
                results.push(Some(*held));
            }
            let hit = combine(any, &results);
            match hit {
                Some(true) => {
                    let since = *st.since.get_or_insert(ts);
                    st.clear_since = None;
                    if !st.latched
                        && ts - since >= i64::from(rule.for_secs)
                        && Self::cooled(st.last, ts, rule.cooldown_secs)
                    {
                        st.latched = true;
                        st.last = Some(ts);
                        st.active = Some(ts);
                        let held = if rule.for_secs > 0 {
                            format!(" for {} s", rule.for_secs)
                        } else {
                            String::new()
                        };
                        let message = match &rule.kind {
                            // P7's wording: "CPU (Tdie) at 96.4 °C (above 95.0 °C for 10 s)".
                            AlertKind::Threshold { .. } => {
                                let t = texts.join("");
                                match t.strip_suffix(')') {
                                    Some(t) => format!("{t}{held})"),
                                    None => t,
                                }
                            }
                            _ => format!("{}{held}", texts.join(if any { "; " } else { " and " })),
                        };
                        let mut f =
                            AlertFired::new(ts, &rule.id, rule.name.clone(), message, rule.sound);
                        f.notify = rule.actions.notify;
                        f.actions = actions_text(rule, inp.fans_on);
                        if rule.actions.record {
                            f.diagnostic = Some(diagnostic(rule, reading, inp.processes));
                        }
                        let mut ps: Vec<String> = Vec::new();
                        for s in rule.sensors() {
                            let p = part(s);
                            if !ps.contains(&p) {
                                ps.push(p);
                            }
                        }
                        parts.push((ps, false));
                        fired.push(f);
                    }
                }
                Some(false) => {
                    st.since = None;
                    st.latched = false;
                }
                None => {}
            }
            if hit != Some(true) && st.active.is_some() {
                let c = *st.clear_since.get_or_insert(ts);
                if ts - c >= CLEAR_SECS {
                    st.active = None;
                    st.clear_since = None;
                }
            }
        }
        self.suppress_repeats(ts, &mut fired, &parts);
        for f in &fired {
            if self.log.len() == MAX_LOG {
                self.log.pop_front();
            }
            self.log.push_back(f.clone());
        }
        self.brief = self.build_brief();
        fired
    }

    /// Keeps the last minutes of every sensor a rate condition watches.
    fn track_rates(&mut self, ts: i64, reading: &dyn Fn(&str) -> Option<Reading>) {
        let mut windows: HashMap<&str, i64> = HashMap::new();
        for r in self.rules.iter().filter(|r| r.enabled) {
            if let AlertKind::Conditions { conditions, .. } = &r.kind {
                for c in conditions {
                    if let Some(secs) = c.test.window() {
                        let w = windows.entry(c.sensor.as_str()).or_default();
                        *w = (*w).max(i64::from(secs));
                    }
                }
            }
        }
        self.recent
            .retain(|id, _| windows.contains_key(id.as_str()));
        for (id, secs) in windows {
            let buf = self.recent.entry(id.to_owned()).or_default();
            while buf.front().is_some_and(|(t, _)| *t < ts - secs || *t > ts) {
                buf.pop_front();
            }
            if let Some(r) = reading(id).filter(|r| r.value.is_finite()) {
                buf.push_back((ts, r.value));
            }
        }
    }

    /// An alert about a part another alert was shown for within [`DUPLICATE_SECS`] is logged
    /// but not shown again; a hardware problem always is.
    fn suppress_repeats(
        &mut self,
        ts: i64,
        fired: &mut [AlertFired],
        parts: &[(Vec<String>, bool)],
    ) {
        while self
            .shown
            .front()
            .is_some_and(|(t, _, _)| ts - t >= DUPLICATE_SECS || *t > ts)
        {
            self.shown.pop_front();
        }
        for (f, (ps, problem)) in fired.iter_mut().zip(parts) {
            if !f.notify {
                continue;
            }
            if !problem {
                if let Some((_, _, title)) = self.shown.iter().find(|(_, p, _)| ps.contains(p)) {
                    f.notify = false;
                    f.also = Some(title.clone());
                    continue;
                }
            }
            for p in ps {
                self.shown.push_back((ts, p.clone(), f.title.clone()));
            }
        }
    }

    fn build_brief(&self) -> AlertBrief {
        AlertBrief {
            active: self
                .rules
                .iter()
                .filter(|r| r.enabled)
                .filter_map(|r| {
                    let since = self.state.get(&r.id)?.active?;
                    Some(ActiveAlert {
                        rule_id: r.id.clone(),
                        name: r.name.clone(),
                        since,
                        tray: r.actions.tray,
                        profile: r.actions.profile,
                        fan_boost: r.actions.fan_boost,
                    })
                })
                .collect(),
        }
    }
}

/// What a firing rule does besides notifying, in words for the log.
fn actions_text(rule: &AlertRule, fans_on: bool) -> Vec<String> {
    let a = &rule.actions;
    let mut out = Vec::new();
    if let Some(p) = a.profile {
        out.push(format!("{} profile while it holds", p.spec().name));
    }
    if let Some(b) = a.fan_boost {
        out.push(if fans_on {
            format!("Fans Vyro drives +{b} % while it holds")
        } else {
            "Fans not raised: fan control is off".into()
        });
    }
    if a.tray {
        out.push("Shown in the tray while it holds".into());
    }
    if !a.notify {
        out.push("No notification (logged only)".into());
    }
    out
}
