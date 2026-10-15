//! Share cards (P25): the page draws the PNG (`src/card.ts`) and the app saves it to
//! `Pictures\Vyro`. The bytes come as the raw request body; the file name comes in a header and
//! is checked, so a page can only write Vyro card PNGs into that folder.

use std::path::PathBuf;

use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Manager};

const FOLDER: &str = "Vyro";
/// A 1080 × 1920 card is about 1 MB; anything far bigger is not a card.
const MAX_BYTES: usize = 16 * 1024 * 1024;
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

fn cards_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let pictures = app.path().picture_dir().map_err(|e| e.to_string())?;
    Ok(pictures.join(FOLDER))
}

/// "vyro-card-20261015-1024-16x9.png": lower-case letters, digits and dashes only.
fn valid_name(name: &str) -> bool {
    name.len() <= 80
        && name.starts_with("vyro-card-")
        && name.strip_suffix(".png").is_some_and(|stem| {
            stem.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

fn checked(name: &str, bytes: &[u8]) -> Result<(), String> {
    if !valid_name(name) {
        return Err(format!("not a card file name: {name}"));
    }
    if !bytes.starts_with(PNG) || bytes.len() > MAX_BYTES {
        return Err("not a PNG card".into());
    }
    Ok(())
}

/// Saves one card and returns its path.
#[tauri::command]
pub async fn save_share_card(app: AppHandle, request: Request<'_>) -> Result<String, String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected the PNG as raw bytes".into());
    };
    let name = request
        .headers()
        .get("x-file-name")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    checked(&name, bytes)?;
    let bytes = bytes.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let dir = cards_dir(&app)?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = dir.join(&name);
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        log::info!("share card: saved {}", path.display());
        Ok(path.display().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Shows a saved card in Explorer; only files in the cards folder.
#[tauri::command]
pub fn reveal_card(app: AppHandle, path: String) -> Result<(), String> {
    crate::diag::reveal_in(&cards_dir(&app)?, &path).map_err(|()| "Not a Vyro card.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_card_pngs_are_written() {
        let png = [PNG, b"rest"].concat();
        assert!(checked("vyro-card-20261015-1024-16x9.png", &png).is_ok());
        assert!(checked("vyro-card-20261015-1024-story-transparent.png", &png).is_ok());
        assert!(checked("../vyro-card-1.png", &png).is_err());
        assert!(checked("vyro-card-1.exe", &png).is_err());
        assert!(checked("vyro-card-1/x.png", &png).is_err());
        assert!(checked("other.png", &png).is_err());
        assert!(checked("vyro-card-1.png", b"GIF89a").is_err());
        assert!(checked(&format!("vyro-card-{}.png", "1".repeat(80)), &png).is_err());
    }
}
