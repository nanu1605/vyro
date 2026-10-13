//! Automatic game profiles (P13): which profile, fan settings, overlay and update rate apply
//! while a game runs, and how everything goes back when it closes. Pure: the app feeds it what
//! its game watcher sees and the owner's picks, and applies what [`Switcher::effective`]
//! returns.
//!
//! The owner's chosen profile (the base) is never changed by a game, so going back on exit is
//! simply returning to it. A profile picked while a game runs becomes the new base and keeps
//! the game's settings out until that game closes: a manual choice always wins.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::profiles::Profile;

/// Update rates a game can ask for, milliseconds.
pub const POLL_CHOICES: [u32; 4] = [500, 1000, 2000, 5000];
/// Saved games, at most.
pub const MAX_RULES: usize = 200;
/// GPU load trigger: the app in front uses at least this much of the GPU…
pub const LOAD_GPU_PCT: f64 = 60.0;
/// …for this many samples (about one a second) in a row.
pub const LOAD_SAMPLES: u32 = 10;

/// One game's own settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct GameRule {
    /// Executable file name, lowercase: what the game is recognised by.
    pub exe: String,
    /// Shown name (the executable as first seen).
    pub name: String,
    /// Profile while it runs; `None`: what games without their own settings get.
    pub profile: Option<Profile>,
    /// Leave the profile as it is while this game runs.
    pub keep: bool,
    /// Whose fan settings apply; `None`: the profile's own.
    pub fans: Option<Profile>,
    /// Shows or hides the overlay for this game; `None`: the overlay setting.
    pub overlay: Option<bool>,
    /// Vyro's update rate while it runs, ms; `None`: the profile's.
    pub poll_ms: Option<u32>,
    /// Not a game: never detected, never switches anything.
    pub ignore: bool,
}

impl GameRule {
    /// A rule with nothing of its own yet, for `exe`.
    pub fn new(exe: &str) -> Self {
        Self {
            exe: exe.to_ascii_lowercase(),
            name: exe.to_owned(),
            ..Self::default()
        }
    }
}

/// How Vyro follows games: kept with the app's settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct GameAutomation {
    /// Switch profiles for games at all.
    pub enabled: bool,
    /// Profile for games without their own; `None`: leave the profile as it is.
    pub default_profile: Option<Profile>,
    /// Also treat the app in front as a game when it keeps the GPU busy (windowed games).
    pub load_trigger: bool,
    pub rules: Vec<GameRule>,
}

impl Default for GameAutomation {
    fn default() -> Self {
        Self {
            enabled: true,
            default_profile: Some(Profile::Gaming),
            load_trigger: false,
            rules: Vec::new(),
        }
    }
}

impl GameAutomation {
    /// The saved settings for `exe` (any case).
    pub fn rule(&self, exe: &str) -> Option<&GameRule> {
        self.rules.iter().find(|r| r.exe.eq_ignore_ascii_case(exe))
    }

    /// Marked "Not a game".
    pub fn ignored(&self, exe: &str) -> bool {
        self.rule(exe).is_some_and(|r| r.ignore)
    }

    /// A saved game (detected in a window too).
    pub fn known(&self, exe: &str) -> bool {
        self.rule(exe).is_some_and(|r| !r.ignore)
    }

    /// Adds or replaces the rule for its executable.
    pub fn upsert(&mut self, mut rule: GameRule) -> Result<(), String> {
        rule.exe = rule.exe.trim().to_ascii_lowercase();
        if rule.name.trim().is_empty() {
            rule.name.clone_from(&rule.exe);
        }
        match self.rules.iter_mut().find(|r| r.exe == rule.exe) {
            Some(r) => *r = rule,
            None => self.rules.push(rule),
        }
        self.validate()
    }

    /// Forgets the game's settings.
    pub fn remove(&mut self, exe: &str) {
        self.rules.retain(|r| !r.exe.eq_ignore_ascii_case(exe));
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.rules.len() > MAX_RULES {
            return Err(format!("at most {MAX_RULES} games can be saved"));
        }
        for (i, r) in self.rules.iter().enumerate() {
            if r.exe.is_empty() || r.exe.contains(['\\', '/']) {
                return Err(format!("\"{}\" is not an executable name", r.exe));
            }
            if r.exe != r.exe.to_ascii_lowercase() {
                return Err(format!("{}: the executable name must be lowercase", r.exe));
            }
            if self.rules[..i].iter().any(|o| o.exe == r.exe) {
                return Err(format!("{} is saved twice", r.name));
            }
            if let Some(ms) = r.poll_ms {
                if !POLL_CHOICES.contains(&ms) {
                    return Err(format!("{}: update rate {ms} ms is not offered", r.name));
                }
            }
        }
        Ok(())
    }
}

/// What applies right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Effective {
    /// The owner's chosen profile, which applies again when the game closes.
    pub base: Profile,
    /// The profile in force.
    pub profile: Profile,
    /// Whose fan settings are in force.
    pub fans: Profile,
    /// Vyro's update rate, ms.
    pub poll_ms: u32,
    /// The game's overlay choice; `None`: the overlay setting.
    pub overlay: Option<bool>,
    /// The game being played (its executable), if any.
    pub game: Option<String>,
    /// Profile, fans or update rate differ from the base because of the game.
    pub auto: bool,
    /// The owner kept their own profile for this game.
    pub suppressed: bool,
}

/// Follows the game being played and the owner's picks.
#[derive(Debug, Clone, Default)]
pub struct Switcher {
    game: Option<String>,
    suppressed: bool,
}

impl Switcher {
    /// The game the watcher follows (its executable), or `None`. Returns whether it changed;
    /// a new game starts without the previous game's suppression.
    pub fn set_game(&mut self, exe: Option<&str>) -> bool {
        if self.game.as_deref() == exe {
            return false;
        }
        self.game = exe.map(str::to_owned);
        self.suppressed = false;
        true
    }

    pub fn game(&self) -> Option<&str> {
        self.game.as_deref()
    }

    /// The owner picked a profile (tray, Settings, "Keep …"): while a game runs, its own
    /// settings stay out until it closes.
    pub fn manual(&mut self) {
        if self.game.is_some() {
            self.suppressed = true;
        }
    }

    pub fn effective(&self, base: Profile, auto: &GameAutomation) -> Effective {
        let plain = Effective {
            base,
            profile: base,
            fans: base,
            poll_ms: base.spec().poll_ms,
            overlay: None,
            game: None,
            auto: false,
            suppressed: false,
        };
        let Some(game) = self.game.as_deref() else {
            return plain;
        };
        let rule = auto.rule(game);
        if rule.is_some_and(|r| r.ignore) {
            return plain;
        }
        let playing = Effective {
            game: Some(game.to_owned()),
            overlay: rule.and_then(|r| r.overlay),
            ..plain
        };
        if self.suppressed {
            return Effective {
                suppressed: true,
                ..playing
            };
        }
        if !auto.enabled {
            return playing;
        }
        let profile = match rule {
            Some(r) if r.keep => base,
            Some(r) => r.profile.or(auto.default_profile).unwrap_or(base),
            None => auto.default_profile.unwrap_or(base),
        };
        let fans = rule.and_then(|r| r.fans).unwrap_or(profile);
        let poll_ms = rule
            .and_then(|r| r.poll_ms)
            .unwrap_or(profile.spec().poll_ms);
        Effective {
            profile,
            fans,
            poll_ms,
            auto: profile != base || fans != base || poll_ms != base.spec().poll_ms,
            ..playing
        }
    }
}

/// Counts how long the app in front has kept the GPU busy (the opt-in load trigger).
#[derive(Debug, Clone, Default)]
pub struct LoadWatch {
    pid: Option<u32>,
    busy: u32,
}

impl LoadWatch {
    /// One sample: the foreground process and its GPU use. True once it has stayed at or above
    /// [`LOAD_GPU_PCT`] for [`LOAD_SAMPLES`] samples in a row.
    pub fn step(&mut self, pid: Option<u32>, gpu_pct: Option<f64>) -> bool {
        if pid != self.pid {
            self.pid = pid;
            self.busy = 0;
        }
        if pid.is_some() && gpu_pct.is_some_and(|g| g >= LOAD_GPU_PCT) {
            self.busy = self.busy.saturating_add(1);
        } else {
            self.busy = 0;
        }
        self.busy >= LOAD_SAMPLES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auto_with(rules: Vec<GameRule>) -> GameAutomation {
        GameAutomation {
            rules,
            ..GameAutomation::default()
        }
    }

    #[test]
    fn a_game_without_settings_gets_gaming_and_the_base_returns_on_exit() {
        let auto = GameAutomation::default();
        let mut s = Switcher::default();
        assert_eq!(
            s.effective(Profile::Desktop, &auto).profile,
            Profile::Desktop
        );
        assert!(s.set_game(Some("game.exe")));
        assert!(!s.set_game(Some("game.exe")), "same game: no change");
        let e = s.effective(Profile::Desktop, &auto);
        assert_eq!(
            (e.profile, e.fans, e.poll_ms),
            (Profile::Gaming, Profile::Gaming, 2000)
        );
        assert!(e.auto && !e.suppressed);
        assert_eq!(e.game.as_deref(), Some("game.exe"));
        assert!(s.set_game(None));
        let e = s.effective(Profile::Desktop, &auto);
        assert_eq!(
            (e.profile, e.poll_ms, e.auto),
            (Profile::Desktop, 1000, false)
        );
        assert_eq!(e.game, None);
    }

    #[test]
    fn leave_it_alone_and_turned_off() {
        let mut s = Switcher::default();
        s.set_game(Some("game.exe"));
        let keep = GameAutomation {
            default_profile: None,
            ..GameAutomation::default()
        };
        let e = s.effective(Profile::Work, &keep);
        assert_eq!((e.profile, e.auto), (Profile::Work, false));
        assert_eq!(
            e.game.as_deref(),
            Some("game.exe"),
            "still a game for capture"
        );
        let off = GameAutomation {
            enabled: false,
            ..GameAutomation::default()
        };
        assert_eq!(s.effective(Profile::Work, &off).profile, Profile::Work);
    }

    #[test]
    fn a_game_with_its_own_settings() {
        let auto = auto_with(vec![GameRule {
            profile: Some(Profile::Silent),
            fans: Some(Profile::Benchmark),
            poll_ms: Some(1000),
            overlay: Some(true),
            ..GameRule::new("Quiet.exe")
        }]);
        let mut s = Switcher::default();
        s.set_game(Some("quiet.exe"));
        let e = s.effective(Profile::Desktop, &auto);
        assert_eq!(
            (e.profile, e.fans, e.poll_ms, e.overlay),
            (Profile::Silent, Profile::Benchmark, 1000, Some(true))
        );
        assert!(auto.known("QUIET.EXE"));

        // "Don't switch" with its own update rate still counts as automatic.
        let keep = auto_with(vec![GameRule {
            keep: true,
            poll_ms: Some(500),
            ..GameRule::new("bench.exe")
        }]);
        s.set_game(Some("bench.exe"));
        let e = s.effective(Profile::Work, &keep);
        assert_eq!(
            (e.profile, e.fans, e.poll_ms, e.auto),
            (Profile::Work, Profile::Work, 500, true)
        );
    }

    #[test]
    fn not_a_game() {
        let auto = auto_with(vec![GameRule {
            ignore: true,
            ..GameRule::new("editor.exe")
        }]);
        assert!(auto.ignored("Editor.exe") && !auto.known("editor.exe"));
        let mut s = Switcher::default();
        s.set_game(Some("editor.exe"));
        let e = s.effective(Profile::Desktop, &auto);
        assert_eq!((e.profile, e.game), (Profile::Desktop, None));
    }

    #[test]
    fn a_manual_pick_wins_until_the_game_closes() {
        let auto = GameAutomation::default();
        let mut s = Switcher::default();
        s.manual();
        s.set_game(Some("game.exe"));
        assert_eq!(
            s.effective(Profile::Desktop, &auto).profile,
            Profile::Gaming,
            "no game: nothing to suppress"
        );
        // Picking Work mid-game: the app saves it as the base.
        s.manual();
        let e = s.effective(Profile::Work, &auto);
        assert_eq!(
            (e.profile, e.auto, e.suppressed),
            (Profile::Work, false, true)
        );
        // Picking the old base (Desktop) also wins.
        let e = s.effective(Profile::Desktop, &auto);
        assert_eq!(e.profile, Profile::Desktop);
        s.set_game(None);
        assert_eq!(
            s.effective(Profile::Work, &auto).profile,
            Profile::Work,
            "stays after exit"
        );
        // The next game switches again.
        s.set_game(Some("game.exe"));
        assert_eq!(s.effective(Profile::Work, &auto).profile, Profile::Gaming);
    }

    #[test]
    fn handover_between_games() {
        let auto = auto_with(vec![GameRule {
            profile: Some(Profile::Benchmark),
            ..GameRule::new("bench.exe")
        }]);
        let mut s = Switcher::default();
        s.set_game(Some("game.exe"));
        s.manual();
        assert!(s.set_game(Some("bench.exe")));
        let e = s.effective(Profile::Desktop, &auto);
        assert_eq!((e.profile, e.suppressed), (Profile::Benchmark, false));
    }

    #[test]
    fn rules_are_checked() {
        let mut auto = GameAutomation::default();
        auto.upsert(GameRule::new("Game.exe")).unwrap();
        assert_eq!(auto.rules[0].exe, "game.exe");
        assert_eq!(auto.rules[0].name, "Game.exe");
        auto.upsert(GameRule {
            profile: Some(Profile::Work),
            ..GameRule::new("GAME.EXE")
        })
        .unwrap();
        assert_eq!(auto.rules.len(), 1, "replaced, not added");
        assert!(auto
            .upsert(GameRule {
                poll_ms: Some(750),
                ..GameRule::new("x.exe")
            })
            .unwrap_err()
            .contains("750 ms"));
        auto.remove("x.exe");
        assert!(auto.upsert(GameRule::new("C:\\Games\\x.exe")).is_err());
        auto.remove("c:\\games\\x.exe");
        auto.remove("game.exe");
        assert!(auto.rules.is_empty());
    }

    #[test]
    fn gpu_load_needs_ten_busy_samples_from_the_same_app() {
        let mut w = LoadWatch::default();
        for _ in 0..9 {
            assert!(!w.step(Some(7), Some(80.0)));
        }
        assert!(w.step(Some(7), Some(80.0)));
        assert!(!w.step(Some(7), Some(30.0)), "a quiet sample starts over");
        for _ in 0..9 {
            w.step(Some(7), Some(90.0));
        }
        assert!(!w.step(Some(8), Some(90.0)), "another app starts over");
        assert!(!w.step(None, Some(90.0)));
    }

    #[test]
    fn json_round_trip_and_old_settings() {
        let auto = auto_with(vec![GameRule {
            profile: Some(Profile::Silent),
            poll_ms: Some(1000),
            ..GameRule::new("Game.exe")
        }]);
        let json = serde_json::to_string(&auto).unwrap();
        assert!(json.contains(r#""defaultProfile":"gaming""#), "{json}");
        assert!(json.contains(r#""pollMs":1000"#), "{json}");
        assert_eq!(serde_json::from_str::<GameAutomation>(&json).unwrap(), auto);
        let empty: GameAutomation = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, GameAutomation::default());
        let partial: GameRule = serde_json::from_str(r#"{"exe":"a.exe"}"#).unwrap();
        assert_eq!(
            (partial.keep, partial.ignore, partial.profile),
            (false, false, None)
        );
    }
}
