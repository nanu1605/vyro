//! The window's look (P25). The page applies the theme itself (`src/themes.ts`); the app only
//! keeps the choice in the settings and paints the window in the theme's background, so a light
//! theme does not open on a dark frame. Unknown names (a newer version's settings) fall back to
//! the defaults instead of resetting the whole settings file.

use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tauri::window::{Color, Effect, EffectsBuilder};
use tauri::{AppHandle, Emitter, Manager};

/// A settings enum read from its camelCase name; an unknown name gives the default.
macro_rules! named {
    ($(#[$doc:meta])* $name:ident { $default:ident = $_d:literal $(, $variant:ident = $vname:literal)* $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", from = "String")]
        pub enum $name {
            #[default]
            $default,
            $($variant,)*
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                match s.as_str() {
                    $($vname => Self::$variant,)*
                    _ => Self::$default,
                }
            }
        }
    };
}

named!(Theme {
    Midnight = "midnight",
    Amoled = "amoled",
    Cyber = "cyber",
    Glass = "glass",
    Minimal = "minimal",
    Daylight = "daylight",
});

/// `Theme`: the theme's own accent. `Custom`: any colour (1.3), kept as `#rrggbb` in lowercase;
/// the page works out the shade text takes from it. An older version reads a colour as an
/// unknown name and falls back to `Theme`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum Accent {
    #[default]
    Theme,
    Cyan,
    Blue,
    Violet,
    Magenta,
    Green,
    Custom(String),
}

impl From<String> for Accent {
    fn from(s: String) -> Self {
        match s.as_str() {
            "theme" => Self::Theme,
            "cyan" => Self::Cyan,
            "blue" => Self::Blue,
            "violet" => Self::Violet,
            "magenta" => Self::Magenta,
            "green" => Self::Green,
            _ if is_colour(&s) => Self::Custom(s.to_ascii_lowercase()),
            _ => Self::Theme,
        }
    }
}

impl From<Accent> for String {
    fn from(a: Accent) -> Self {
        match a {
            Accent::Theme => "theme".into(),
            Accent::Cyan => "cyan".into(),
            Accent::Blue => "blue".into(),
            Accent::Violet => "violet".into(),
            Accent::Magenta => "magenta".into(),
            Accent::Green => "green".into(),
            Accent::Custom(c) => c,
        }
    }
}

/// `#rrggbb`.
fn is_colour(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s.bytes().skip(1).all(|b| b.is_ascii_hexdigit())
}

named!(
    /// Which tab the window opens on, at launch and when opened from the tray.
    OpenOn { Dashboard = "dashboard", Last = "last" }
);

/// Emitted when the window is shown after being hidden in the tray, so the page can go back to
/// the Dashboard.
pub const OPENED_EVENT: &str = "window-opened";

/// Asks the main window to show a view (a desktop widget's "Edit…" opens Widgets).
pub const OPEN_VIEW_EVENT: &str = "open-view";

/// The theme's page background (the same as `src/themes.ts`, checked by its tests).
pub fn background(theme: Theme) -> (u8, u8, u8) {
    match theme {
        Theme::Midnight => (0x0a, 0x0d, 0x12),
        Theme::Amoled => (0x00, 0x00, 0x00),
        Theme::Cyber => (0x0a, 0x06, 0x14),
        Theme::Glass => (0x0b, 0x12, 0x22),
        Theme::Minimal => (0x16, 0x18, 0x1c),
        Theme::Daylight => (0xee, 0xf1, 0xf5),
    }
}

/// The first Windows build whose own Acrylic (a system backdrop) window-vibrancy uses (2.5). On
/// older builds its Acrylic is an older effect that is slow to drag and resize, so Clokrix's
/// window stays opaque there.
const ACRYLIC_BUILD: u32 = 22_523;

/// Windows' build, read once.
fn windows_build() -> u32 {
    static BUILD: OnceLock<u32> = OnceLock::new();
    *BUILD.get_or_init(clokrix_core::sysinfo::windows_build)
}

/// The window is see-through, with Windows' Acrylic behind it (2.5): from build 22523, with
/// Glass effects and Windows' Transparency effects on, and never on AMOLED, whose black stays
/// black.
pub fn see_through(build: u32, glass: bool, transparency: bool, theme: Theme) -> bool {
    build >= ACRYLIC_BUILD && glass && transparency && theme != Theme::Amoled
}

/// The window's glass, as the page needs it (`window-look`, `window_look`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowLook {
    /// Windows' Acrylic is behind a see-through window.
    pub see_through: bool,
    /// Windows' own Transparency effects are on; off turns Clokrix's glass off too.
    pub transparency: bool,
}

/// Emitted with the [`WindowLook`] when it changes.
pub const WINDOW_LOOK_EVENT: &str = "window-look";

/// The theme and look last put on the window.
static LAST: Mutex<Option<(Theme, WindowLook)>> = Mutex::new(None);

/// The main window, from `tauri.conf.json` (which leaves it to this): able to be see-through
/// only where Windows has its own Acrylic, so a window on an older Windows is exactly 2.4's.
pub fn main_window(app: &AppHandle) -> tauri::Result<()> {
    let Some(config) = app.config().app.windows.iter().find(|w| w.label == "main") else {
        return Ok(());
    };
    tauri::WebviewWindowBuilder::from_config(app, config)?
        .transparent(windows_build() >= ACRYLIC_BUILD)
        .build()?;
    Ok(())
}

/// Paints the main window in the theme's background, with a title bar to match; see-through
/// over Windows' Acrylic instead when [`see_through`] holds (2.5). Tells the page when that
/// changes.
pub fn paint(app: &AppHandle, theme: Theme, glass: bool) {
    let transparency = clokrix_core::sysinfo::transparency_effects();
    let look = WindowLook {
        see_through: see_through(windows_build(), glass, transparency, theme),
        transparency,
    };
    let before = clokrix_core::lock(&LAST).replace((theme, look));
    // Nothing changed (another setting, or the window got focus again): nothing to do.
    if before == Some((theme, look)) {
        return;
    }
    let before = before.map(|(_, l)| l);
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    // The effect only when it changes: none was there to start with.
    if before.map_or(look.see_through, |b| b.see_through != look.see_through) {
        let effects = look
            .see_through
            .then(|| EffectsBuilder::new().effect(Effect::Acrylic).build());
        if let Err(e) = w.set_effects(effects) {
            log::debug!("window effects: {e}");
        }
    }
    // Clear behind a see-through page, so the Acrylic shows; else the theme's background.
    let (r, g, b) = background(theme);
    let colour = if look.see_through {
        Color(0, 0, 0, 0)
    } else {
        Color(r, g, b, 255)
    };
    if let Err(e) = w.set_background_color(Some(colour)) {
        log::debug!("window colour: {e}");
    }
    // Acrylic's tint follows the title bar's dark or light.
    let bar = if theme == Theme::Daylight {
        tauri::Theme::Light
    } else {
        tauri::Theme::Dark
    };
    if let Err(e) = w.set_theme(Some(bar)) {
        log::debug!("title bar theme: {e}");
    }
    if before != Some(look) {
        if let Err(e) = app.emit(WINDOW_LOOK_EVENT, look) {
            log::debug!("emit {WINDOW_LOOK_EVENT}: {e}");
        }
    }
}

/// Paints the window again from the settings, as Windows' Transparency effects may have
/// changed (the window got focus).
pub fn refresh(app: &AppHandle) {
    if let Some(store) = app.try_state::<crate::tray::SettingsStore>() {
        let s = store.get();
        paint(app, s.theme, s.glass);
    }
}

/// The window's glass now, for the page's first paint.
#[tauri::command]
pub fn window_look() -> WindowLook {
    clokrix_core::lock(&LAST).map_or(
        WindowLook {
            see_through: false,
            transparency: true,
        },
        |(_, l)| l,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2.5: see-through only on a Windows with its own Acrylic, with both switches on, and
    /// never on AMOLED.
    #[test]
    fn the_window_is_see_through_only_where_it_should_be() {
        assert!(!see_through(22_522, true, true, Theme::Midnight));
        assert!(see_through(22_523, true, true, Theme::Midnight));
        assert!(see_through(26_100, true, true, Theme::Daylight));
        assert!(!see_through(26_100, false, true, Theme::Midnight));
        assert!(!see_through(26_100, true, false, Theme::Glass));
        assert!(!see_through(26_100, true, true, Theme::Amoled));
        assert!(!see_through(0, true, true, Theme::Midnight));
        assert_eq!(
            serde_json::to_string(&WindowLook {
                see_through: true,
                transparency: true
            })
            .unwrap(),
            r#"{"seeThrough":true,"transparency":true}"#
        );
    }

    #[test]
    fn unknown_names_fall_back() {
        let t: Theme = serde_json::from_str("\"daylight\"").unwrap();
        assert_eq!(t, Theme::Daylight);
        let t: Theme = serde_json::from_str("\"aurora\"").unwrap();
        assert_eq!(t, Theme::Midnight);
        let a: Accent = serde_json::from_str("\"gold\"").unwrap();
        assert_eq!(a, Accent::Theme);
        let o: OpenOn = serde_json::from_str("\"last\"").unwrap();
        assert_eq!(o, OpenOn::Last);
        // Every name reads back as itself.
        for t in [
            Theme::Midnight,
            Theme::Amoled,
            Theme::Cyber,
            Theme::Glass,
            Theme::Minimal,
            Theme::Daylight,
        ] {
            let json = serde_json::to_string(&t).unwrap();
            assert_eq!(serde_json::from_str::<Theme>(&json).unwrap(), t, "{json}");
        }
        for a in [
            Accent::Theme,
            Accent::Cyan,
            Accent::Blue,
            Accent::Violet,
            Accent::Magenta,
            Accent::Green,
        ] {
            let json = serde_json::to_string(&a).unwrap();
            assert_eq!(serde_json::from_str::<Accent>(&json).unwrap(), a, "{json}");
        }
        assert_eq!(serde_json::to_string(&OpenOn::Last).unwrap(), "\"last\"");
    }

    /// 1.3: any colour, kept as `#rrggbb` in lowercase; anything else that is not a name falls
    /// back to the theme's own.
    #[test]
    fn a_custom_accent_is_any_colour() {
        let a: Accent = serde_json::from_str("\"#1A237E\"").unwrap();
        assert_eq!(a, Accent::Custom("#1a237e".into()));
        assert_eq!(serde_json::to_string(&a).unwrap(), "\"#1a237e\"");
        for bad in [
            "\"#12345\"",
            "\"#1234567\"",
            "\"#gg0000\"",
            "\"1a237e\"",
            "\"red\"",
        ] {
            assert_eq!(
                serde_json::from_str::<Accent>(bad).unwrap(),
                Accent::Theme,
                "{bad}"
            );
        }
    }
}

/// My PC (P23): drawn in 3-D or as a flat plan, and the owner's corrections to each GPU's shape
/// by card key. The UI checks the shapes; they are kept as written, so a newer shape loads
/// unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MyPc {
    pub three_d: bool,
    pub gpu_shapes: std::collections::BTreeMap<String, serde_json::Value>,
    /// Desktop or laptop model (1.1): `Auto` follows what the firmware says.
    pub form: Form,
}

named!(
    /// My PC's model: as the firmware says, or the owner's pick when it says wrong.
    Form { Auto = "auto", Desktop = "desktop", Laptop = "laptop" }
);

impl Default for MyPc {
    fn default() -> Self {
        Self {
            three_d: true,
            gpu_shapes: std::collections::BTreeMap::new(),
            form: Form::Auto,
        }
    }
}

/// Reads a settings section, falling back to its default when it does not parse, so one
/// damaged section never resets the whole file.
pub fn lenient<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}
