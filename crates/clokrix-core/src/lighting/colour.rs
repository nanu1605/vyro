//! The colour glowing most on the screen (2.6; the owner, 2026-11-27: "Whatever is glowing in the
//! screen ... the RGB of the keyboard or mouse or any other devices ... should change
//! automatically with the colour which is glowing most on the screen. Clokrix should
//! intelligently figure out which colour is glowing the most or which colour to send").
//!
//! The screen comes as a small picture in linear light ([`Thumb`], 64x36). Each pixel is weighed
//! by how strongly it glows, its chroma times its lightness in OKLab, so a vivid red sign counts
//! and grey text, white pages and black bars do not. The weights fall into 24 hues round the
//! colour wheel; the busiest hue with its two neighbours wins, and its pixels' average is the
//! colour, made vivid enough for LEDs and as bright as the scene. A screen with no colour (a
//! document, the desktop) shows white, the accent colour or nothing, as the owner chooses.
//!
//! So that the lights do not flicker: a hue keeps winning until another clearly beats it, the
//! colour eases to each new one (in OKLab, the short way between hues), and the brightness never
//! swings 10% or more back and forth more than 3 times a second (WCAG 2.3.1's flash threshold),
//! however the screen flashes.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The picture's size: enough to tell a sign from its background, little to read or weigh.
pub const THUMB_W: usize = 64;
pub const THUMB_H: usize = 36;

/// The screen, small, in linear light: SDR white is 1.0, an HDR highlight more.
#[derive(Debug, Clone, PartialEq)]
pub struct Thumb {
    pub width: usize,
    pub height: usize,
    /// Red, green, blue, row by row.
    pub px: Vec<[f32; 3]>,
}

/// 8-bit sRGB to linear light, by table.
fn srgb_table() -> &'static [f32; 256] {
    static T: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0.0; 256];
        for (i, v) in t.iter_mut().enumerate() {
            let c = i as f32 / 255.0;
            *v = if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            };
        }
        t
    })
}

impl Thumb {
    /// A BGRA picture (rows packed, 8-bit sRGB) made small: each cell the average of the pixels
    /// in it, in linear light, looking at every `step`th pixel each way.
    pub fn from_bgra(bgra: &[u8], width: u32, height: u32, step: usize) -> Self {
        let (w, h) = (width as usize, height as usize);
        let step = step.max(1);
        let lin = srgb_table();
        let mut sum = vec![[0.0f32; 3]; THUMB_W * THUMB_H];
        let mut n = vec![0u32; THUMB_W * THUMB_H];
        if w > 0 && h > 0 && bgra.len() >= w * h * 4 {
            for y in (0..h).step_by(step) {
                let cy = y * THUMB_H / h;
                let row = &bgra[y * w * 4..(y + 1) * w * 4];
                for x in (0..w).step_by(step) {
                    let i = cy * THUMB_W + x * THUMB_W / w;
                    let p = &row[x * 4..x * 4 + 3];
                    sum[i][0] += lin[usize::from(p[2])];
                    sum[i][1] += lin[usize::from(p[1])];
                    sum[i][2] += lin[usize::from(p[0])];
                    n[i] += 1;
                }
            }
        }
        let px = sum
            .iter()
            .zip(&n)
            .map(|(s, &k)| {
                let k = k.max(1) as f32;
                [s[0] / k, s[1] / k, s[2] / k]
            })
            .collect();
        Self {
            width: THUMB_W,
            height: THUMB_H,
            px,
        }
    }

    /// One colour everywhere (tests and the settings' preview).
    pub fn solid(rgb: [f32; 3]) -> Self {
        Self {
            width: THUMB_W,
            height: THUMB_H,
            px: vec![rgb; THUMB_W * THUMB_H],
        }
    }
}

/// A colour as the lights take it: 8-bit sRGB.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Rgb8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// How fast the lights follow the screen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LightSpeed {
    /// Fast while a game is in front, smooth otherwise (the app decides).
    #[default]
    Auto,
    /// About a tenth of a second: games.
    Fast,
    /// About 0.4 s: films and videos.
    Smooth,
}

impl LightSpeed {
    /// The time constant: the lights cover about two thirds of a change in this long.
    pub fn seconds(self, game: bool) -> f32 {
        match self {
            Self::Fast => 0.1,
            Self::Smooth => 0.4,
            Self::Auto if game => 0.1,
            Self::Auto => 0.4,
        }
    }
}

/// What the lights show when the screen has no colour to speak of.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum NoColour {
    /// White, as bright as the screen.
    #[default]
    White,
    /// Clokrix's accent colour.
    Accent,
    /// Off.
    Off,
}

/// How the picker is set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PickSettings {
    /// The time constant, from [`LightSpeed::seconds`].
    pub seconds: f32,
    /// 0 to 1: the lights at their brightest.
    pub brightness: f32,
    pub no_colour: NoColour,
    /// For [`NoColour::Accent`].
    pub accent: Rgb8,
}

impl Default for PickSettings {
    fn default() -> Self {
        Self {
            seconds: 0.1,
            brightness: 1.0,
            no_colour: NoColour::White,
            accent: Rgb8 {
                r: 0x25,
                g: 0x63,
                b: 0xeb,
            },
        }
    }
}

// OKLab (Björn Ottosson's, from linear sRGB), the space colours are weighed and eased in.

type Lab = [f32; 3];

fn to_oklab(c: [f32; 3]) -> Lab {
    let [r, g, b] = c;
    let l = 0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_99 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
    let (l, m, s) = (l.max(0.0).cbrt(), m.max(0.0).cbrt(), s.max(0.0).cbrt());
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn from_oklab(c: Lab) -> [f32; 3] {
    let [ll, a, b] = c;
    let l = ll + 0.396_337_78 * a + 0.215_803_76 * b;
    let m = ll - 0.105_561_346 * a - 0.063_854_17 * b;
    let s = ll - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ]
}

/// Relative luminance of linear sRGB.
fn luminance(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn to_srgb8(c: [f32; 3]) -> Rgb8 {
    let enc = |v: f32| {
        let v = if v.is_finite() {
            v.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let s = if v <= 0.003_130_8 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round() as u8
    };
    Rgb8 {
        r: enc(c[0]),
        g: enc(c[1]),
        b: enc(c[2]),
    }
}

fn from_srgb8(c: Rgb8) -> [f32; 3] {
    let lin = srgb_table();
    [
        lin[usize::from(c.r)],
        lin[usize::from(c.g)],
        lin[usize::from(c.b)],
    ]
}

/// Hues round the wheel.
const BINS: usize = 24;
/// Darker than this (OKLab lightness) is black: bars, shadows, an empty background.
const DARK: f32 = 0.15;
/// A screen whose glow, per pixel, is less than this has no colour to speak of (a red mark on
/// about 1.5% of it has this much).
const COLOURFUL: f32 = 0.0025;
/// The hue shown stays until another's weight is this many times its own.
const KEEP: f32 = 1.33;
/// Chroma is raised this much (LEDs look paler than screens), within what sRGB can show.
const BOOST: f32 = 1.5;
/// A swing in brightness counts as a flash from this much (relative luminance).
const FLASH: f32 = 0.1;
/// Swings allowed in any second: 3 flashes, each a swing up and one down.
const SWINGS: usize = 6;

/// A highlight above SDR white, eased so a bright HDR scene does not hold the lights at full.
fn knee(v: f32) -> f32 {
    if !v.is_finite() || v <= 0.0 {
        0.0
    } else if v <= 1.0 {
        v
    } else {
        let over = v - 1.0;
        1.0 + over / (1.0 + over)
    }
}

/// What a picture asks the lights to show, before easing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    /// The colour, in linear light.
    pub rgb: [f32; 3],
    /// The winning hue (0 to 23), or none when the screen has no colour.
    pub hue: Option<usize>,
}

/// Finds and eases the colour, picture after picture.
#[derive(Debug, Clone, Default)]
pub struct Picker {
    /// The hue shown.
    hue: Option<usize>,
    /// The colour shown, in OKLab.
    shown: Option<Lab>,
    /// Seconds since the picker started.
    clock: f64,
    /// The brightness at the last turn, which way it went since, and when it turned.
    turn: f32,
    rising: Option<bool>,
    swings: std::collections::VecDeque<f64>,
}

impl Picker {
    pub fn new() -> Self {
        Self::default()
    }

    /// What `thumb` asks for, keeping the hue shown unless another clearly beats it.
    pub fn target(&mut self, thumb: &Thumb, s: &PickSettings) -> Target {
        let mut weight = [0.0f32; BINS];
        let mut sum = [[0.0f32; 2]; BINS];
        let mut lightness = 0.0f32;
        let n = thumb.px.len().max(1) as f32;
        let pixels: Vec<(Lab, f32, usize)> = thumb
            .px
            .iter()
            .map(|p| {
                let lab = to_oklab([knee(p[0]), knee(p[1]), knee(p[2])]);
                let chroma = lab[1].hypot(lab[2]);
                let w = if lab[0] < DARK { 0.0 } else { chroma * lab[0] };
                let angle = lab[2].atan2(lab[1]);
                let bin = ((angle + std::f32::consts::PI) / std::f32::consts::TAU * BINS as f32)
                    .floor() as usize
                    % BINS;
                (lab, w, bin)
            })
            .collect();
        for &(lab, w, bin) in &pixels {
            lightness += lab[0].min(1.0);
            weight[bin] += w;
            sum[bin][0] += lab[1] * w;
            sum[bin][1] += lab[2] * w;
        }
        // The scene's brightness: a dark scene dims the lights, never below a glimmer.
        let scene = (lightness / n / 0.55).clamp(0.15, 1.0) * s.brightness.clamp(0.0, 1.0);
        let total: f32 = weight.iter().sum();
        let around = |i: usize| {
            weight[(i + BINS - 1) % BINS] * 0.5 + weight[i] + weight[(i + 1) % BINS] * 0.5
        };
        let best = (0..BINS).max_by(|&a, &b| around(a).total_cmp(&around(b)));
        let hue = if total / n < COLOURFUL {
            None
        } else {
            match (self.hue, best) {
                (Some(h), Some(b)) if around(h) * KEEP >= around(b) => Some(h),
                (_, b) => b,
            }
        };
        self.hue = hue;
        let rgb = match hue {
            Some(h) => {
                let (mut a, mut b, mut w) = (0.0, 0.0, 0.0);
                for i in [(h + BINS - 1) % BINS, h, (h + 1) % BINS] {
                    a += sum[i][0];
                    b += sum[i][1];
                    w += weight[i];
                }
                let (a, b) = (a / w.max(1e-9), b / w.max(1e-9));
                let chroma = a.hypot(b);
                let angle = b.atan2(a);
                let rgb = vivid(chroma * BOOST, angle);
                rgb.map(|v| v * scene)
            }
            None => match s.no_colour {
                NoColour::White => [scene; 3],
                NoColour::Accent => from_srgb8(s.accent).map(|v| v * scene),
                NoColour::Off => [0.0; 3],
            },
        };
        Target { rgb, hue }
    }

    /// The colour to show `dt` seconds after the last, easing towards `t`.
    pub fn step(&mut self, t: &Target, dt: f32, s: &PickSettings) -> Rgb8 {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.clock += f64::from(dt);
        let want = to_oklab(t.rgb);
        let lab = match self.shown {
            None => want,
            Some(was) => {
                let k = 1.0 - (-dt / s.seconds.max(0.01)).exp();
                [0, 1, 2].map(|i| was[i] + (want[i] - was[i]) * k)
            }
        };
        self.shown = Some(lab);
        let mut rgb = from_oklab(lab).map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 });
        let y = luminance(rgb);
        let held = self.limit(y);
        if y > 1e-6 && (held - y).abs() > 1e-6 {
            rgb = rgb.map(|v| v * held / y);
        } else if y <= 1e-6 && held > 1e-6 {
            // Held bright though the colour went dark: as bright, in grey.
            rgb = [held; 3];
        }
        to_srgb8(rgb)
    }

    /// The brightness `y`, unless it would make a 4th flash within a second: then held within a
    /// flash of the last turn.
    fn limit(&mut self, y: f32) -> f32 {
        let now = self.clock;
        match self.rising {
            Some(true) if y >= self.turn => {
                self.turn = y;
                return y;
            }
            Some(false) if y <= self.turn => {
                self.turn = y;
                return y;
            }
            _ => {}
        }
        // Counted from a little under a flash, so that 8-bit rounding never makes one uncounted.
        if (y - self.turn).abs() < FLASH * 0.9 {
            return y;
        }
        while self.swings.front().is_some_and(|&t| now - t >= 1.0) {
            self.swings.pop_front();
        }
        if self.swings.len() >= SWINGS {
            return self.turn + (y - self.turn).signum() * (FLASH * 0.8);
        }
        self.swings.push_back(now);
        self.rising = Some(y > self.turn);
        self.turn = y;
        y
    }
}

/// The most chroma sRGB can show at lightness `l` and hue `angle`.
fn most_chroma(l: f32, angle: f32) -> f32 {
    let inside = |c: f32| {
        from_oklab([l, c * angle.cos(), c * angle.sin()])
            .iter()
            .all(|x| (-1e-4..=1.0001).contains(x))
    };
    let (mut lo, mut hi) = (0.0f32, 0.4f32);
    for _ in 0..18 {
        let mid = (lo + hi) / 2.0;
        if inside(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// The hue `angle` as LEDs show it best: at the lightness where sRGB holds the most of it (red
/// stays red, blue stays blue, neither turns pastel), with as much of `chroma` as fits there.
fn vivid(chroma: f32, angle: f32) -> [f32; 3] {
    let (l, most) = (35..=92)
        .map(|i| {
            let l = i as f32 / 100.0;
            (l, most_chroma(l, angle))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((0.7, 0.0));
    let c = chroma.max(0.0).min(most);
    from_oklab([l, c * angle.cos(), c * angle.sin()]).map(|v| v.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [f32; 3] = [1.0, 0.0, 0.0];
    const BLUE: [f32; 3] = [0.0, 0.0, 1.0];
    const GREY: [f32; 3] = [0.2, 0.2, 0.2];

    /// A picture of `bg` with `fg` on the first `share` of its pixels.
    fn with(bg: [f32; 3], fg: [f32; 3], share: f32) -> Thumb {
        let mut t = Thumb::solid(bg);
        let k = (t.px.len() as f32 * share) as usize;
        for p in &mut t.px[..k] {
            *p = fg;
        }
        t
    }

    /// The colour shown once the lights settle on `t`.
    fn settled(t: &Thumb, s: &PickSettings) -> Rgb8 {
        let mut p = Picker::new();
        let mut c = Rgb8::default();
        for _ in 0..60 {
            let target = p.target(t, s);
            c = p.step(&target, 0.05, s);
        }
        c
    }

    fn reddish(c: Rgb8) -> bool {
        c.r > 60 && u16::from(c.g) * 3 < u16::from(c.r) && u16::from(c.b) * 3 < u16::from(c.r)
    }

    fn bluish(c: Rgb8) -> bool {
        c.b > 60 && u16::from(c.r) * 3 < u16::from(c.b) && u16::from(c.g) * 3 < u16::from(c.b)
    }

    #[test]
    fn a_red_screen_lights_red() {
        let c = settled(&Thumb::solid(RED), &PickSettings::default());
        assert!(reddish(c) && c.r > 240, "{c:?}");
        // The brightness set scales it.
        let half = PickSettings {
            brightness: 0.5,
            ..PickSettings::default()
        };
        let dim = settled(&Thumb::solid(RED), &half);
        assert!(reddish(dim) && dim.r < c.r, "{dim:?}");
    }

    #[test]
    fn a_red_sign_in_the_dark_lights_red_and_dimmer() {
        let s = PickSettings::default();
        let sign = settled(&with([0.0; 3], RED, 0.1), &s);
        let full = settled(&Thumb::solid(RED), &s);
        assert!(reddish(sign), "{sign:?}");
        assert!(sign.r < full.r, "{sign:?} not dimmer than {full:?}");
    }

    #[test]
    fn a_little_vivid_blue_beats_a_lot_of_grey() {
        let c = settled(&with(GREY, BLUE, 0.05), &PickSettings::default());
        assert!(bluish(c), "{c:?}");
    }

    #[test]
    fn hues_either_side_of_the_wheels_start_are_one_colour() {
        // Two reds, one on each side of where the hue bins wrap, and some green that would win
        // if they were counted apart.
        let mut t = Thumb::solid(GREY);
        let n = t.px.len();
        for (i, p) in t.px.iter_mut().enumerate() {
            *p = match i * 10 / n {
                0 | 1 => [1.0, 0.0, 0.08],
                2 | 3 => [1.0, 0.08, 0.0],
                4..=6 => [0.0, 0.35, 0.0],
                _ => GREY,
            };
        }
        let c = settled(&t, &PickSettings::default());
        assert!(reddish(c), "{c:?}");
    }

    #[test]
    fn two_near_equal_colours_do_not_flip_flop() {
        let s = PickSettings::default();
        // Red and blue on about half each, the larger in glow swapping every picture.
        let picture = |i: usize| {
            let mut t = Thumb::solid(BLUE);
            let k = t.px.len() * if i.is_multiple_of(2) { 45 } else { 50 } / 100;
            for px in &mut t.px[..k] {
                *px = RED;
            }
            t
        };
        // Looked at afresh, each picture's winner differs from the last's.
        let fresh: Vec<_> = (0..2)
            .map(|i| Picker::new().target(&picture(i), &s).hue)
            .collect();
        assert_ne!(fresh[0], fresh[1]);
        // Looked at in turn, the hue shown stays.
        let mut p = Picker::new();
        let shown: Vec<_> = (0..40).map(|i| p.target(&picture(i), &s).hue).collect();
        assert!(shown.windows(2).all(|w| w[0] == w[1]), "{shown:?}");
    }

    #[test]
    fn eases_from_one_colour_to_the_next_at_the_speed_set() {
        // How far to blue (0 to 1) after `secs` of blue following red.
        let reach = |seconds: f32, secs: f32| {
            let s = PickSettings {
                seconds,
                ..PickSettings::default()
            };
            let mut p = Picker::new();
            let red = p.target(&Thumb::solid(RED), &s);
            let from = to_oklab(red.rgb);
            p.step(&red, 0.0, &s);
            let blue = p.target(&Thumb::solid(BLUE), &s);
            let to = to_oklab(blue.rgb);
            let steps = (secs / 0.01).round() as usize;
            let mut c = Rgb8::default();
            for _ in 0..steps {
                c = p.step(&blue, 0.01, &s);
            }
            let now = to_oklab(from_srgb8(c));
            let d = |x: Lab, y: Lab| ((x[1] - y[1]).powi(2) + (x[2] - y[2]).powi(2)).sqrt();
            1.0 - d(now, to) / d(from, to)
        };
        let fast = reach(LightSpeed::Fast.seconds(false), 0.1);
        let smooth = reach(LightSpeed::Smooth.seconds(false), 0.1);
        assert!((0.5..0.8).contains(&fast), "fast {fast}");
        assert!(smooth < 0.35, "smooth {smooth}");
        assert!(reach(LightSpeed::Smooth.seconds(false), 2.0) > 0.98);
        assert_eq!(
            LightSpeed::Auto.seconds(true),
            LightSpeed::Fast.seconds(false)
        );
        assert_eq!(
            LightSpeed::Auto.seconds(false),
            LightSpeed::Smooth.seconds(false)
        );
    }

    #[test]
    fn a_screen_without_colour_shows_what_the_owner_chose() {
        let page = Thumb::solid([0.9, 0.9, 0.9]);
        let s = |no_colour| PickSettings {
            no_colour,
            ..PickSettings::default()
        };
        let white = settled(&page, &s(NoColour::White));
        assert!(
            white.r > 200 && white.r == white.g && white.g == white.b,
            "{white:?}"
        );
        let accent = settled(&page, &s(NoColour::Accent));
        assert!(accent.b > accent.r && accent.b > accent.g, "{accent:?}");
        assert_eq!(settled(&page, &s(NoColour::Off)), Rgb8::default());
        // A tiny coloured mark (a taskbar icon) is not colour enough.
        let icon = settled(&with([0.9; 3], RED, 0.005), &s(NoColour::White));
        assert!(icon.r.abs_diff(icon.g) < 8, "{icon:?}");
    }

    #[test]
    fn hdr_highlights_and_bad_values_do_no_harm() {
        let mut t = with([0.0; 3], [8.0, 0.2, 0.2], 0.3);
        t.px[2000] = [f32::NAN, f32::INFINITY, -3.0];
        t.px[2001] = [-0.5, -0.5, 2.0];
        let c = settled(&t, &PickSettings::default());
        assert!(reddish(c), "{c:?}");
        let mut p = Picker::new();
        let s = PickSettings::default();
        let target = p.target(&Thumb::solid([f32::NAN; 3]), &s);
        assert!(target.rgb.iter().all(|v| v.is_finite()));
        let _ = p.step(&target, f32::NAN, &s);
    }

    #[test]
    fn the_brightness_never_flashes_more_than_3_times_a_second() {
        let s = PickSettings::default();
        let mut p = Picker::new();
        let (black, white) = (Thumb::solid([0.0; 3]), Thumb::solid([1.0; 3]));
        let mut ys = Vec::new();
        // White and black, swapping 15 times a second, for 4 s.
        for i in 0..120 {
            let t = if (i / 2) % 2 == 0 { &white } else { &black };
            let target = p.target(t, &s);
            let c = p.step(&target, 1.0 / 30.0, &s);
            ys.push(luminance(from_srgb8(c)));
        }
        // Swings of 10% or more, back and forth, in every second.
        let mut turns = Vec::new();
        let (mut turn, mut rising) = (ys[0], None::<bool>);
        for (i, &y) in ys.iter().enumerate() {
            match rising {
                Some(true) if y >= turn => turn = y,
                Some(false) if y <= turn => turn = y,
                _ if (y - turn).abs() >= FLASH => {
                    turns.push(i);
                    rising = Some(y > turn);
                    turn = y;
                }
                _ => {}
            }
        }
        for start in 0..ys.len() {
            let n = turns
                .iter()
                .filter(|&&i| i >= start && i < start + 30)
                .count();
            assert!(n <= SWINGS, "{n} swings from picture {start}: {turns:?}");
        }
        assert!(
            !turns.is_empty(),
            "the lights should still follow the screen"
        );
    }

    #[test]
    fn a_small_picture_of_a_bgra_screen() {
        // 128x72: the left half pure red, the right black.
        let (w, h) = (128u32, 72u32);
        let mut bgra = vec![0u8; (w * h * 4) as usize];
        for y in 0..h as usize {
            for x in 0..64 {
                bgra[(y * w as usize + x) * 4 + 2] = 255;
            }
        }
        let t = Thumb::from_bgra(&bgra, w, h, 1);
        assert_eq!(
            (t.width, t.height, t.px.len()),
            (THUMB_W, THUMB_H, THUMB_W * THUMB_H)
        );
        assert_eq!(t.px[0], [1.0, 0.0, 0.0]);
        assert_eq!(t.px[THUMB_W - 1], [0.0, 0.0, 0.0]);
        // A picture smaller than said is all black, not a panic.
        assert!(Thumb::from_bgra(&bgra[..10], w, h, 4)
            .px
            .iter()
            .all(|p| *p == [0.0; 3]));
    }
}
