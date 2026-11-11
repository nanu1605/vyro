//! Profiles (P9c): one choice that sets how often the engine polls, which alerts notify, how
//! much the dashboard animates and how much of it shows. Fixed presets; the app keeps the chosen
//! one in its settings and applies it on start and on change.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::alerts::AlertFired;

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS,
)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Profile {
    #[default]
    Desktop,
    Gaming,
    Work,
    Benchmark,
    Silent,
}

/// Which fired alerts become Windows notifications. Every alert is evaluated and logged
/// whatever the policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum NotifyPolicy {
    /// Every alert, with the sound its rule asks for.
    All,
    /// Every alert, without sound.
    Quiet,
    /// Hardware problems only (the System Health problems rule), with sound.
    Critical,
    /// None; alerts are only logged.
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Motion {
    /// Gauges ease smoothly toward each reading.
    Full,
    /// Shorter easing: fewer frames per reading.
    Reduced,
    /// Values jump; no animation frames.
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Density {
    /// Every dashboard card.
    Full,
    /// Gauges, tiles and the essentials; per-thread and detail cards hidden.
    Compact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProfileSpec {
    pub id: Profile,
    pub name: String,
    pub description: String,
    /// Poll interval while the window shows (the tray rate applies while it does not).
    pub poll_ms: u32,
    pub notifications: NotifyPolicy,
    pub motion: Motion,
    pub dashboard: Density,
}

/// Alert rule whose firings count as hardware problems.
pub const PROBLEMS_RULE: &str = "health-problems";

impl Profile {
    pub const ALL: [Profile; 5] = [
        Profile::Desktop,
        Profile::Gaming,
        Profile::Work,
        Profile::Benchmark,
        Profile::Silent,
    ];

    pub fn spec(self) -> ProfileSpec {
        let (name, description, poll_ms, notifications, motion, dashboard) = match self {
            Profile::Desktop => (
                "Desktop",
                "Everyday use: live gauges every second, every alert.",
                1000,
                NotifyPolicy::All,
                Motion::Full,
                Density::Full,
            ),
            Profile::Gaming => (
                "Gaming",
                "Light while you play: readings every 2 s, only hardware problems notify.",
                2000,
                NotifyPolicy::Critical,
                Motion::Reduced,
                Density::Compact,
            ),
            Profile::Work => (
                "Work",
                "Readings every 2 s, alerts without sound, calmer animation.",
                2000,
                NotifyPolicy::Quiet,
                Motion::Reduced,
                Density::Full,
            ),
            Profile::Benchmark => (
                "Benchmark",
                "Readings twice a second for test runs; alerts silent, no animation.",
                500,
                NotifyPolicy::Quiet,
                Motion::Off,
                Density::Full,
            ),
            Profile::Silent => (
                "Silent",
                "Smallest footprint: readings every 5 s, no notifications (alerts still logged).",
                5000,
                NotifyPolicy::Off,
                Motion::Off,
                Density::Compact,
            ),
        };
        ProfileSpec {
            id: self,
            name: name.into(),
            description: description.into(),
            poll_ms,
            notifications,
            motion,
            dashboard,
        }
    }
}

impl NotifyPolicy {
    /// Whether `alert` should notify, and with sound.
    pub fn notify(self, alert: &AlertFired) -> Option<bool> {
        match self {
            NotifyPolicy::All => Some(alert.sound),
            NotifyPolicy::Quiet => Some(false),
            NotifyPolicy::Critical => (alert.rule_id == PROBLEMS_RULE).then_some(alert.sound),
            NotifyPolicy::Off => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fired(rule: &str) -> AlertFired {
        AlertFired::new(0, rule, "t".into(), "m".into(), true)
    }

    #[test]
    fn presets_are_consistent() {
        for p in Profile::ALL {
            let s = p.spec();
            assert_eq!(s.id, p);
            assert!((500..=5000).contains(&s.poll_ms), "{p:?}");
        }
        assert_eq!(Profile::default().spec().poll_ms, 1000);
        assert_eq!(
            serde_json::to_string(&Profile::Benchmark).unwrap(),
            r#""benchmark""#
        );
    }

    #[test]
    fn notification_policies() {
        let cpu = fired("cpu-temp");
        let problems = fired(PROBLEMS_RULE);
        assert_eq!(NotifyPolicy::All.notify(&cpu), Some(true));
        assert_eq!(NotifyPolicy::Quiet.notify(&cpu), Some(false));
        assert_eq!(NotifyPolicy::Critical.notify(&cpu), None);
        assert_eq!(NotifyPolicy::Critical.notify(&problems), Some(true));
        assert_eq!(NotifyPolicy::Off.notify(&problems), None);
    }
}
