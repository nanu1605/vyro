//! Diagnostics (P21), app side: the app adds what only it knows (how it reaches the engine,
//! the PC summary and the GPU drivers from its inventory), the engine works out and keeps the
//! report, and the app writes the exports to `Documents\Vyro Reports`. Serial numbers are read
//! only for an export that asks for them, and never kept.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::{AppHandle, Manager, State};
use vyro_core::diag::export::{self, ExportOptions};
use vyro_core::diag::{pdf, DiagAppInfo, DiagReport, DiagSummary, Field, GpuDriver};
use vyro_core::engine::BackendMode;
use vyro_core::units::TempUnit;

use crate::engine::Engine;

const FOLDER: &str = "Vyro Reports";

fn join(parts: &[Option<&str>]) -> Option<String> {
    let s = parts
        .iter()
        .flatten()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    (!s.is_empty()).then_some(s)
}

/// The PC summary and the drivers, from the inventory (no serial numbers).
pub(crate) fn app_info(app: &AppHandle, engine: &Engine) -> DiagAppInfo {
    let hw = engine.inventory.hardware();
    let sys = engine.inventory.system();
    let status = engine.status();
    let mut pc = Vec::new();
    let mut add = |label: &str, value: Option<String>| {
        if let Some(value) = value {
            pc.push(Field {
                label: label.into(),
                value,
            });
        }
    };
    add("CPU", hw.cpu.brand.clone());
    for g in &hw.gpus {
        add("GPU", Some(g.name.clone()));
    }
    add(
        "Board",
        join(&[
            hw.board.manufacturer.as_deref(),
            hw.board.product.as_deref(),
        ]),
    );
    add(
        "BIOS",
        join(&[
            hw.board.bios_version.as_deref(),
            hw.board.bios_date.as_deref(),
        ]),
    );
    let speed = hw
        .memory
        .dimms
        .iter()
        .filter_map(|d| d.configured_speed_mts.or(d.speed_mts))
        .max();
    add(
        "RAM",
        hw.memory.total_mb.map(|mb| match speed {
            Some(s) => format!("{:.0} GB at {s} MT/s", mb / 1024.0),
            None => format!("{:.0} GB", mb / 1024.0),
        }),
    );
    let windows = join(&[
        sys.os.name.as_deref(),
        sys.os.display_version.as_deref(),
        sys.os.build.as_deref(),
    ]);
    add("Windows", windows.clone());
    DiagAppInfo {
        app_version: app.package_info().version.to_string(),
        service: status.mode == BackendMode::Service && status.connected,
        service_version: status.service_version.clone(),
        service_detail: status.detail.clone(),
        pc,
        drivers: hw
            .gpus
            .iter()
            .map(|g| GpuDriver {
                name: g.name.clone(),
                version: g
                    .vendor_driver_version
                    .clone()
                    .or_else(|| g.driver_version.clone()),
                date: g.driver_date.clone(),
            })
            .collect(),
        windows,
    }
}

/// Runs the diagnostics off the UI thread; the engine keeps the report.
#[tauri::command]
pub async fn run_diagnostics(app: AppHandle) -> Result<DiagReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let engine = app.state::<Engine>();
        let info = app_info(&app, &engine);
        engine.api.run_diagnostics(info)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_diagnostic_runs(engine: State<'_, Engine>) -> Result<Vec<DiagSummary>, String> {
    let api = Arc::clone(&engine.api);
    tauri::async_runtime::spawn_blocking(move || api.diagnostic_runs())
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_diagnostic_run(
    engine: State<'_, Engine>,
    id: i64,
) -> Result<Option<DiagReport>, String> {
    let api = Arc::clone(&engine.api);
    tauri::async_runtime::spawn_blocking(move || api.diagnostic_run(id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_diagnostic(engine: State<'_, Engine>, id: i64) -> Result<bool, String> {
    let api = Arc::clone(&engine.api);
    tauri::async_runtime::spawn_blocking(move || api.delete_diagnostic(id))
        .await
        .map_err(|e| e.to_string())?
}

/// The board's and the drives' serial numbers, read now for one export.
fn serials(engine: &Engine) -> Vec<Field> {
    let mut v = Vec::new();
    if let Some(s) = engine
        .inventory
        .hardware()
        .board
        .serial
        .clone()
        .filter(|s| !s.trim().is_empty())
    {
        v.push(Field {
            label: "Board serial".into(),
            value: s,
        });
    }
    for d in engine.api.drives().unwrap_or_default() {
        if let Some(s) = d.serial.filter(|s| !s.0.trim().is_empty()) {
            v.push(Field {
                label: format!("Serial: {}", d.model.trim()),
                value: s.0.trim().to_owned(),
            });
        }
    }
    v
}

fn reports_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let docs = app.path().document_dir().map_err(|e| e.to_string())?;
    Ok(docs.join(FOLDER))
}

/// "20261015-1024" as the UI sends it, kept to digits and dashes.
fn clean_stamp(stamp: &str) -> String {
    let s: String = stamp
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '-')
        .take(20)
        .collect();
    if s.is_empty() {
        vyro_core::unix_now().to_string()
    } else {
        s
    }
}

/// Writes one export of a kept report and returns its path.
#[tauri::command]
pub async fn export_diagnostics(
    app: AppHandle,
    id: i64,
    format: String,
    serial_numbers: bool,
    unit: TempUnit,
    when: String,
    stamp: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let engine = app.state::<Engine>();
        let report = engine
            .api
            .diagnostic_run(id)?
            .ok_or("That report is no longer kept.")?;
        let opts = ExportOptions {
            unit,
            when,
            serials: if serial_numbers {
                serials(&engine)
            } else {
                Vec::new()
            },
        };
        let (ext, bytes) = match format.as_str() {
            "html" => ("html", export::html(&report, &opts).into_bytes()),
            "json" => ("json", export::json(&report, &opts).into_bytes()),
            "md" => ("md", export::markdown(&report, &opts).into_bytes()),
            "pdf" => ("pdf", pdf::pdf(&report, &opts)),
            other => return Err(format!("unknown format {other}")),
        };
        let dir = reports_dir(&app)?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = dir.join(format!("vyro-diagnostics-{}.{ext}", clean_stamp(&stamp)));
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        log::info!("diagnostics: exported {}", path.display());
        Ok(path.display().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Shows an exported report in Explorer; only files in the reports folder.
#[tauri::command]
pub fn reveal_report(app: AppHandle, path: String) -> Result<(), String> {
    reveal_in(&reports_dir(&app)?, &path).map_err(|()| "Not a Vyro report.".into())
}

/// Opens Explorer on a file, only when it sits directly in `dir`.
pub(crate) fn reveal_in(dir: &Path, path: &str) -> Result<(), ()> {
    let p = Path::new(path);
    if p.parent() != Some(dir) || !p.is_file() {
        return Err(());
    }
    #[cfg(windows)]
    {
        if let Err(e) = std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", p.display()))
            .spawn()
        {
            log::warn!("explorer: {e}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_keep_digits_and_dashes() {
        assert_eq!(clean_stamp("20261015-1024"), "20261015-1024");
        assert_eq!(clean_stamp("../../x 2026"), "2026");
        assert_eq!(
            join(&[Some(" MSI "), None, Some("B650M")]).as_deref(),
            Some("MSI B650M")
        );
        assert_eq!(join(&[None, Some("  ")]), None);
    }
}
