//! Dashboard layouts (P24): the owner's widget layouts for the Dashboard, kept by the app in
//! `dashboard.json`. The UI owns the widget kinds and the presets; this module checks what is
//! stored, so a hand-edited or damaged file can only ever load as a tidy layout.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const VERSION: u32 = 1;
pub const MAX_LAYOUTS: usize = 12;
pub const MAX_WIDGETS: usize = 40;
pub const NAME_MAX: usize = 40;
/// Widths on the 12-column grid.
pub const WIDTHS: [u16; 5] = [3, 4, 6, 8, 12];
pub const MAX_ROWS: u16 = 4;
const ID_MAX: usize = 40;
const SOURCE_MAX: usize = 120;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct Dashboard {
    /// File format version.
    pub version: u32,
    /// The layout the Dashboard shows.
    pub active: String,
    pub layouts: Vec<Layout>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct Layout {
    pub id: String,
    pub name: String,
    /// The preset it was made from, for "Reset".
    pub preset: Option<String>,
    /// No editing until unlocked.
    pub locked: bool,
    /// In grid order.
    pub widgets: Vec<Widget>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct Widget {
    pub id: String,
    /// "gauge", "reading", "graph", "history", "game", "health", "events", "card"; kinds a
    /// later version adds are kept and skipped by this one.
    pub kind: String,
    /// Columns of 12: one of [`WIDTHS`].
    pub w: u16,
    /// Rows, 1 to [`MAX_ROWS`].
    pub h: u16,
    /// What it shows: a reading id, a sensor id, "cpu", "gpu.0", a card name.
    pub source: Option<String>,
}

fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= ID_MAX
        && s.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
        })
}

/// `id` if it is a valid id not taken yet, else `fallback`, numbered until unique.
fn unique(seen: &mut HashSet<String>, id: &str, fallback: &str) -> String {
    let base = if valid_id(id) { id } else { fallback };
    let mut candidate = base.to_owned();
    let mut n = 2;
    while !seen.insert(candidate.clone()) {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    candidate
}

/// Trimmed, without control characters, at most `max` characters.
fn clip(s: &str, max: usize) -> String {
    s.trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect()
}

/// The nearest allowed width (the narrower on a tie).
pub fn snap_width(w: u16) -> u16 {
    WIDTHS
        .iter()
        .copied()
        .min_by_key(|x| x.abs_diff(w))
        .unwrap_or(WIDTHS[0])
}

/// What the app stores and serves: sizes in range, counts capped, ids valid and unique, names
/// present, and an active layout that exists.
pub fn sanitize(mut d: Dashboard) -> Dashboard {
    d.version = VERSION;
    d.layouts.truncate(MAX_LAYOUTS);
    let mut layout_ids = HashSet::new();
    for (i, l) in d.layouts.iter_mut().enumerate() {
        l.id = unique(&mut layout_ids, &l.id, &format!("layout-{}", i + 1));
        l.name = clip(&l.name, NAME_MAX);
        if l.name.is_empty() {
            l.name = format!("Layout {}", i + 1);
        }
        l.preset = l.preset.take().filter(|p| valid_id(p));
        l.widgets.truncate(MAX_WIDGETS);
        let mut widget_ids = HashSet::new();
        for (j, w) in l.widgets.iter_mut().enumerate() {
            w.id = unique(&mut widget_ids, &w.id, &format!("w{}", j + 1));
            w.kind = clip(&w.kind, ID_MAX);
            w.w = snap_width(w.w);
            w.h = w.h.clamp(1, MAX_ROWS);
            w.source = w
                .source
                .take()
                .map(|s| clip(&s, SOURCE_MAX))
                .filter(|s| !s.is_empty());
        }
        l.widgets.retain(|w| !w.kind.is_empty());
    }
    if !d.layouts.iter().any(|l| l.id == d.active) {
        d.active = d.layouts.first().map(|l| l.id.clone()).unwrap_or_default();
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widget(id: &str, kind: &str, w: u16, h: u16) -> Widget {
        Widget {
            id: id.into(),
            kind: kind.into(),
            w,
            h,
            source: None,
        }
    }

    fn layout(id: &str, widgets: Vec<Widget>) -> Layout {
        Layout {
            id: id.into(),
            name: "Mine".into(),
            widgets,
            ..Layout::default()
        }
    }

    #[test]
    fn sizes_snap_to_the_grid() {
        assert_eq!(
            [0, 3, 5, 7, 9, 10, 11, 99].map(snap_width),
            [3, 3, 4, 6, 8, 8, 12, 12]
        );
        let d = sanitize(Dashboard {
            layouts: vec![layout(
                "a",
                vec![widget("x", "reading", 5, 0), widget("y", "gauge", 13, 9)],
            )],
            ..Dashboard::default()
        });
        let w = &d.layouts[0].widgets;
        assert_eq!((w[0].w, w[0].h, w[1].w, w[1].h), (4, 1, 12, 4));
    }

    #[test]
    fn ids_are_valid_and_unique_and_the_active_layout_exists() {
        let mut bad = layout(
            "Bad Id!",
            vec![widget("", "reading", 3, 1), widget("w1", "reading", 3, 1)],
        );
        bad.name = "  \u{7}  ".into();
        let d = sanitize(Dashboard {
            version: 0,
            active: "gone".into(),
            layouts: vec![
                layout(
                    "a",
                    vec![widget("x", "reading", 3, 1), widget("x", "graph", 4, 2)],
                ),
                layout("a", vec![]),
                bad,
            ],
        });
        assert_eq!(d.version, VERSION);
        let ids: Vec<&str> = d.layouts.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["a", "a-2", "layout-3"]);
        assert_eq!(d.active, "a");
        assert_eq!(d.layouts[0].widgets[1].id, "x-2");
        // The first widget's empty id falls back to its place, which the next one also wants.
        let w: Vec<&str> = d.layouts[2].widgets.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(w, ["w1", "w1-2"]);
        assert_eq!(d.layouts[2].name, "Layout 3");
    }

    #[test]
    fn counts_and_text_are_capped() {
        let many = (0..60)
            .map(|i| widget(&format!("w{i}"), "reading", 3, 1))
            .collect();
        let mut l = layout("a", many);
        l.name = "x".repeat(100);
        l.preset = Some("Not An Id".into());
        l.widgets[0].source = Some(format!("  {}  ", "s".repeat(300)));
        l.widgets[1].source = Some("   ".into());
        l.widgets[2].kind = " ".into();
        let d = sanitize(Dashboard {
            layouts: vec![l; 20],
            ..Dashboard::default()
        });
        assert_eq!(d.layouts.len(), MAX_LAYOUTS);
        let l = &d.layouts[0];
        assert_eq!(l.name.chars().count(), NAME_MAX);
        assert_eq!(l.preset, None);
        // 40 kept, then the one without a kind dropped.
        assert_eq!(l.widgets.len(), MAX_WIDGETS - 1);
        assert_eq!(
            l.widgets[0].source.as_ref().map(String::len),
            Some(SOURCE_MAX)
        );
        assert_eq!(l.widgets[1].source, None);
    }

    #[test]
    fn files_from_later_versions_load() {
        let json = r#"{"version":2,"active":"g","future":true,"layouts":[{"id":"g","name":"Gaming","preset":"gaming",
            "widgets":[{"id":"a","kind":"hologram","w":6,"h":2,"source":"gpu.0","glow":1},{"id":"b","kind":"reading","w":3,"h":1}]}]}"#;
        let d = sanitize(serde_json::from_str(json).unwrap());
        assert_eq!(d.layouts[0].widgets.len(), 2);
        assert_eq!(d.layouts[0].widgets[0].kind, "hologram");
        assert_eq!(d.layouts[0].preset.as_deref(), Some("gaming"));
        // Missing fields take their defaults.
        let d: Dashboard =
            serde_json::from_str(r#"{"layouts":[{"widgets":[{"kind":"gauge"}]}]}"#).unwrap();
        let d = sanitize(d);
        assert_eq!(d.active, "layout-1");
        assert_eq!(
            (d.layouts[0].widgets[0].w, d.layouts[0].widgets[0].h),
            (3, 1)
        );
        assert_eq!(sanitize(Dashboard::default()).active, "");
    }
}
