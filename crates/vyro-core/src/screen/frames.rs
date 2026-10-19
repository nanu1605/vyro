//! Pictures between the monitor and the encoder (1.7): the size a session sends, BGRA (what
//! Windows captures) to NV12 (what H.264 encoders take) at that size, a test pattern, and the
//! NAL units of an encoded picture.

/// The size to send for a `w`×`h` monitor within `max_w`×`max_h`: the same shape, never larger
/// than the monitor, both sides even (NV12 halves them).
pub fn fit(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (2, 2);
    }
    let scale = (f64::from(max_w) / f64::from(w))
        .min(f64::from(max_h) / f64::from(h))
        .min(1.0);
    let even = |v: f64| ((v as u32) & !1).max(2);
    (even(f64::from(w) * scale), even(f64::from(h) * scale))
}

/// BT.601 limited range, as H.264 decoders expect by default.
fn yuv(b: u32, g: u32, r: u32) -> (u8, u8, u8) {
    let (r, g, b) = (r as i32, g as i32, b as i32);
    let y = 16 + ((66 * r + 129 * g + 25 * b + 128) >> 8);
    let u = 128 + ((-38 * r - 74 * g + 112 * b + 128) >> 8);
    let v = 128 + ((112 * r - 94 * g - 18 * b + 128) >> 8);
    (
        y.clamp(0, 255) as u8,
        u.clamp(0, 255) as u8,
        v.clamp(0, 255) as u8,
    )
}

/// For each of `d` positions across a side `s` long, the source span it covers (at least one).
fn spans(s: usize, d: usize) -> Vec<(usize, usize)> {
    (0..d)
        .map(|i| {
            let a = (i * s / d).min(s - 1);
            (a, ((i + 1) * s / d).clamp(a + 1, s))
        })
        .collect()
}

/// Each pixel of a `dw`×`dh` picture as the average blue, green and red of the source pixels it
/// covers, so text stays readable when a large monitor is sent smaller (picking one pixel in
/// three drops whole strokes). `None` when `src` is too short.
fn scaled(
    src: &[u8],
    sw: u32,
    sh: u32,
    pitch: usize,
    dw: usize,
    dh: usize,
) -> Option<Vec<[u8; 3]>> {
    let (sw, sh) = (sw as usize, sh as usize);
    if sw == 0 || sh == 0 || src.len() < pitch * (sh - 1) + sw * 4 {
        return None;
    }
    if (sw, sh) == (dw, dh) {
        // Sent as it is (a 1080p monitor): each pixel once, nothing to average.
        return Some(
            (0..sh)
                .flat_map(|y| src[y * pitch..y * pitch + sw * 4].as_chunks::<4>().0)
                .map(|p| [p[0], p[1], p[2]])
                .collect(),
        );
    }
    let xs = spans(sw, dw);
    let mut out = Vec::with_capacity(dw * dh);
    for (y0, y1) in spans(sh, dh) {
        for &(x0, x1) in &xs {
            let mut sum = [0u32; 3];
            for y in y0..y1 {
                let row = &src[y * pitch + x0 * 4..y * pitch + x1 * 4];
                for px in row.as_chunks::<4>().0 {
                    sum[0] += u32::from(px[0]);
                    sum[1] += u32::from(px[1]);
                    sum[2] += u32::from(px[2]);
                }
            }
            let n = ((x1 - x0) * (y1 - y0)) as u32;
            out.push(sum.map(|v| ((v + n / 2) / n) as u8));
        }
    }
    Some(out)
}

/// Converts a BGRA picture (`pitch` bytes a row) to NV12 at `dw`×`dh` (even), each pixel the
/// average of the source pixels it covers; chroma is the average of each 2×2 block.
pub fn bgra_to_nv12(src: &[u8], sw: u32, sh: u32, pitch: usize, dw: u32, dh: u32) -> Vec<u8> {
    let (dw, dh) = (dw as usize, dh as usize);
    let mut out = vec![0u8; dw * dh * 3 / 2];
    let Some(px) = scaled(src, sw, sh, pitch, dw, dh) else {
        return out;
    };
    let (luma, chroma) = out.split_at_mut(dw * dh);
    for y in (0..dh).step_by(2) {
        for x in (0..dw).step_by(2) {
            let (mut su, mut sv) = (0u32, 0u32);
            for dy in 0..2 {
                for dx in 0..2 {
                    let [b, g, r] = px[(y + dy) * dw + x + dx];
                    let (yy, u, v) = yuv(u32::from(b), u32::from(g), u32::from(r));
                    luma[(y + dy) * dw + x + dx] = yy;
                    su += u32::from(u);
                    sv += u32::from(v);
                }
            }
            let c = (y / 2) * dw + x;
            chroma[c] = ((su + 2) / 4) as u8;
            chroma[c + 1] = ((sv + 2) / 4) as u8;
        }
    }
    out
}

/// Converts a BGRA picture (`pitch` bytes a row) to packed BGR at `dw`×`dh`, each pixel the
/// average of the source pixels it covers (the slow view's pictures, before JPEG).
pub fn bgra_to_bgr(src: &[u8], sw: u32, sh: u32, pitch: usize, dw: u32, dh: u32) -> Vec<u8> {
    let (dw, dh) = (dw as usize, dh as usize);
    match scaled(src, sw, sh, pitch, dw, dh) {
        Some(px) => px.concat(),
        None => vec![0u8; dw * dh * 3],
    }
}

/// How bright an NV12 picture is (its mean luma, 0–255) and how much that varies across it (the
/// standard deviation; 0 is one flat colour), from a grid of its pixels: a black picture reads
/// (16, 0) or so.
pub fn brightness(nv12: &[u8], w: u32, h: u32) -> (u8, u8) {
    let (w, h) = (w as usize, h as usize);
    if w == 0 || h == 0 || nv12.len() < w * h {
        return (0, 0);
    }
    let (cols, rows) = (w.min(64), h.min(36));
    let mut values = Vec::with_capacity(cols * rows);
    for r in 0..rows {
        let y = (r * h + h / 2) / rows;
        for c in 0..cols {
            let x = (c * w + w / 2) / cols;
            values.push(f64::from(nv12[y.min(h - 1) * w + x.min(w - 1)]));
        }
    }
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let spread = (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt();
    (mean.round() as u8, spread.round().min(255.0) as u8)
}

/// A moving test picture (BGRA, `w`×`h`, tightly packed): colour bars and a bar that slides with
/// `n`, so a stalled stream shows.
pub fn test_pattern(w: u32, h: u32, n: u32) -> Vec<u8> {
    const BARS: [[u8; 3]; 7] = [
        [255, 255, 255],
        [0, 255, 255],
        [255, 255, 0],
        [0, 255, 0],
        [255, 0, 255],
        [0, 0, 255],
        [255, 0, 0],
    ];
    let (w, h) = (w as usize, h as usize);
    let mut px = vec![0u8; w * h * 4];
    let slider = (n as usize * 8) % w.max(1);
    for y in 0..h {
        for x in 0..w {
            let c = if (x + w - slider) % w < w / 16 && y > h * 3 / 4 {
                [0, 0, 0]
            } else {
                BARS[x * BARS.len() / w.max(1)]
            };
            let p = (y * w + x) * 4;
            px[p..p + 3].copy_from_slice(&c);
            px[p + 3] = 255;
        }
    }
    px
}

/// The type of each NAL unit in Annex B data (5 = IDR slice, 7 = SPS, 8 = PPS).
pub fn nal_types(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 < data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            out.push(data[i + 3] & 0x1f);
            i += 3;
        } else {
            i += 1;
        }
    }
    out
}

/// Whether the picture is a keyframe (an IDR slice).
pub fn is_keyframe(data: &[u8]) -> bool {
    nal_types(data).contains(&5)
}

/// A keyframe with its SPS and PPS in front, if it lacks them (`headers` holds them, Annex B).
pub fn with_headers(frame: Vec<u8>, headers: &[u8]) -> Vec<u8> {
    let types = nal_types(&frame);
    if headers.is_empty() || !types.contains(&5) || types.contains(&7) {
        return frame;
    }
    let mut v = headers.to_vec();
    v.extend_from_slice(&frame);
    v
}
