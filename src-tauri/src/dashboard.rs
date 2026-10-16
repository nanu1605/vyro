//! Dashboard layouts (P24), kept in `dashboard.json` next to `settings.json` but apart from it:
//! a damaged layout file is set aside and the Dashboard starts from the presets, while the other
//! settings stay as they were.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::{AppHandle, Manager, State};
use vyro_core::dashboard::{sanitize, Dashboard};
use vyro_core::{files, lock};

pub struct DashboardStore {
    path: Option<PathBuf>,
    /// `None` until the owner saves a layout: the UI then starts from the presets.
    current: Mutex<Option<Dashboard>>,
}

/// Reads a layout file; one that does not parse is set aside as `dashboard.bad.json` (see
/// `vyro_core::files`).
fn read(path: &Path) -> Option<Dashboard> {
    match files::read_json::<Dashboard>(path, |_| Ok(())) {
        Ok(d) => d.map(sanitize),
        Err(e) => {
            log::warn!(
                "dashboard: {} does not load ({e}); starting from the presets",
                path.display()
            );
            None
        }
    }
}

/// Writes through a temporary file, so a crash mid-write never leaves half a layout.
fn write(path: &Path, d: &Dashboard) -> Result<(), String> {
    files::write_json(path, d).map_err(|e| format!("{}: {e}", path.display()))
}

impl DashboardStore {
    pub fn load(app: &AppHandle) -> Self {
        let path = app
            .path()
            .app_config_dir()
            .ok()
            .map(|d| d.join("dashboard.json"));
        let current = path.as_deref().and_then(read);
        Self {
            path,
            current: Mutex::new(current),
        }
    }
}

/// The saved layouts, or `None` before the first save.
#[tauri::command]
pub fn get_dashboard(store: State<'_, DashboardStore>) -> Option<Dashboard> {
    lock(&store.current).clone()
}

/// Saves the layouts and returns them as stored (checked and tidied).
#[tauri::command]
pub fn set_dashboard(
    store: State<'_, DashboardStore>,
    dashboard: Dashboard,
) -> Result<Dashboard, String> {
    let d = sanitize(dashboard);
    let path = store.path.as_ref().ok_or("no config folder")?;
    write(path, &d)?;
    *lock(&store.current) = Some(d.clone());
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vyro_core::dashboard::{Layout, Widget};

    #[test]
    fn saves_and_sets_a_damaged_file_aside() {
        let dir = std::env::temp_dir().join(format!("vyro-dash-{}", std::process::id()));
        let path = dir.join("dashboard.json");
        let d = Dashboard {
            version: 1,
            active: "a".into(),
            layouts: vec![Layout {
                id: "a".into(),
                name: "Mine".into(),
                widgets: vec![Widget {
                    id: "w".into(),
                    kind: "reading".into(),
                    w: 3,
                    h: 1,
                    source: Some("cpu.temp".into()),
                }],
                ..Layout::default()
            }],
        };
        write(&path, &d).unwrap();
        assert_eq!(read(&path), Some(d));
        assert!(!path.with_extension("json.tmp").exists());

        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(read(&path), None);
        assert!(!path.exists());
        assert!(dir.join("dashboard.bad.json").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
