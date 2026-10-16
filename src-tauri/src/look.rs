//! The window's look (P25). The page applies the theme itself (`src/themes.ts`); the app only
//! keeps the choice in the settings and paints the window in the theme's background, so a light
//! theme does not open on a dark frame. Unknown names (a newer version's settings) fall back to
//! the defaults instead of resetting the whole settings file.

use serde::{Deserialize, Serialize};
use tauri::window::Color;
use tauri::{AppHandle, Manager};

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

named!(
    /// `Theme`: the theme's own accent.
    Accent {
        Theme = "theme",
        Cyan = "cyan",
        Blue = "blue",
        Violet = "violet",
        Magenta = "magenta",
        Green = "green",
    }
);

named!(
    /// Which tab the window opens on, at launch and when opened from the tray.
    OpenOn { Dashboard = "dashboard", Last = "last" }
);

/// Emitted when the window is shown after being hidden in the tray, so the page can go back to
/// the Dashboard.
pub const OPENED_EVENT: &str = "window-opened";

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

/// Paints the main window in the theme's background, with a title bar to match.
pub fn paint(app: &AppHandle, theme: Theme) {
    let (r, g, b) = background(theme);
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    if let Err(e) = w.set_background_color(Some(Color(r, g, b, 255))) {
        log::debug!("window colour: {e}");
    }
    let bar = if theme == Theme::Daylight {
        tauri::Theme::Light
    } else {
        tauri::Theme::Dark
    };
    if let Err(e) = w.set_theme(Some(bar)) {
        log::debug!("title bar theme: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
