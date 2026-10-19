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

/// Converts a BGRA picture (`pitch` bytes a row) to NV12 at `dw`×`dh` (even), sampling the
/// nearest source pixel; chroma is the average of each 2×2 block.
pub fn bgra_to_nv12(src: &[u8], sw: u32, sh: u32, pitch: usize, dw: u32, dh: u32) -> Vec<u8> {
    let (dw, dh) = (dw as usize, dh as usize);
    let mut out = vec![0u8; dw * dh * 3 / 2];
    if sw == 0 || sh == 0 || src.len() < pitch * (sh as usize - 1) + sw as usize * 4 {
        return out;
    }
    let xs: Vec<usize> = (0..dw)
        .map(|x| (x * sw as usize / dw).min(sw as usize - 1) * 4)
        .collect();
    let (luma, chroma) = out.split_at_mut(dw * dh);
    for y in (0..dh).step_by(2) {
        let rows = [y, y + 1].map(|yy| (yy * sh as usize / dh).min(sh as usize - 1) * pitch);
        for x in (0..dw).step_by(2) {
            let (mut su, mut sv) = (0u32, 0u32);
            for (dy, row) in rows.iter().enumerate() {
                for dx in 0..2 {
                    let p = row + xs[x + dx];
                    let (yy, u, v) = yuv(
                        u32::from(src[p]),
                        u32::from(src[p + 1]),
                        u32::from(src[p + 2]),
                    );
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

/// Converts a BGRA picture (`pitch` bytes a row) to packed BGR at `dw`×`dh`, sampling the
/// nearest source pixel (the slow view's pictures, before JPEG).
pub fn bgra_to_bgr(src: &[u8], sw: u32, sh: u32, pitch: usize, dw: u32, dh: u32) -> Vec<u8> {
    let (dw, dh) = (dw as usize, dh as usize);
    let mut out = vec![0u8; dw * dh * 3];
    if sw == 0 || sh == 0 || src.len() < pitch * (sh as usize - 1) + sw as usize * 4 {
        return out;
    }
    let xs: Vec<usize> = (0..dw)
        .map(|x| (x * sw as usize / dw).min(sw as usize - 1) * 4)
        .collect();
    for (y, line) in out.chunks_exact_mut(dw * 3).enumerate() {
        let row = (y * sh as usize / dh).min(sh as usize - 1) * pitch;
        for (i, x) in xs.iter().enumerate() {
            line[i * 3..i * 3 + 3].copy_from_slice(&src[row + x..row + x + 3]);
        }
    }
    out
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
