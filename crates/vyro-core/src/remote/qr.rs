//! The pairing QR code as an SVG path, drawn by Settings in its own colours.

use qrcodegen::{QrCode, QrCodeEcc};

/// Quiet zone around the code, in modules.
pub const BORDER: i32 = 4;

/// One `M x y h1 v1 h-1 z` square per dark module, in a `size` × `size` box that includes the
/// border; `None` when the text is too long for a QR code.
pub fn path(text: &str) -> Option<(String, u32)> {
    let qr = QrCode::encode_text(text, QrCodeEcc::Medium).ok()?;
    let n = qr.size();
    let mut d = String::with_capacity((n * n) as usize * 4);
    for y in 0..n {
        let mut x = 0;
        while x < n {
            if !qr.get_module(x, y) {
                x += 1;
                continue;
            }
            // A run of dark modules in a row is one rectangle.
            let start = x;
            while x < n && qr.get_module(x, y) {
                x += 1;
            }
            d.push_str(&format!(
                "M{} {}h{}v1h-{}z",
                start + BORDER,
                y + BORDER,
                x - start,
                x - start
            ));
        }
    }
    Some((d, u32::try_from(n + 2 * BORDER).unwrap_or(0)))
}
