//! The phone's voice on the PC (1.7): a clip recorded on the phone, put on the PC's clipboard as
//! a file to paste into a chat; and the phone as the PC's microphone, played into a virtual
//! cable (VB-CABLE and the like) that Windows then offers as a microphone. Windows lets only a
//! signed driver add a microphone, and Vyro ships none.
//!
//! What needs no Windows is here and tested: the phone's WAV, a clip's pieces, the clip's name,
//! the live frames' jitter buffer and finding a cable by name. The Windows parts (the AAC file,
//! the clipboard, the cable's playback, the default microphone) are in [`win`].

use std::collections::BTreeMap;

/// What the phone sends: 16 kHz, mono, 16-bit.
pub const RATE: u32 = 16_000;
/// The longest clip.
pub const MAX_CLIP_SECS: u32 = 300;
/// The largest clip, header included.
pub const MAX_CLIP_BYTES: usize = 44 + (RATE as usize * 2 * MAX_CLIP_SECS as usize);
/// A clip through the relay: at most this many pieces, each at most this large.
pub const MAX_PIECES: u32 = 600;
pub const MAX_PIECE: usize = 32 * 1024;
/// A live frame: 20 ms.
pub const FRAME_SAMPLES: usize = RATE as usize / 50;

/// The samples of a WAV the phone made (16-bit little-endian PCM, mono, 16 kHz).
pub fn read_wav(bytes: &[u8]) -> Result<&[u8], String> {
    let bad = |why: &str| Err(format!("not a voice clip: {why}"));
    if bytes.len() > MAX_CLIP_BYTES {
        return bad("longer than 5 minutes");
    }
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return bad("no WAV header");
    }
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32_at =
        |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    if &bytes[12..16] != b"fmt " || u32_at(16) != 16 {
        return bad("an unexpected format block");
    }
    if u16_at(20) != 1 || u16_at(22) != 1 || u32_at(24) != RATE || u16_at(34) != 16 {
        return bad("not 16 kHz mono 16-bit PCM");
    }
    if &bytes[36..40] != b"data" {
        return bad("no data block");
    }
    let len = u32_at(40) as usize;
    if !len.is_multiple_of(2) || 44 + len != bytes.len() {
        return bad("its length does not match");
    }
    Ok(&bytes[44..])
}

/// 16-bit little-endian PCM as samples.
pub fn samples(pcm: &[u8]) -> Vec<i16> {
    pcm.as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect()
}

/// 16 kHz to 48 kHz (what Windows' AAC encoder takes): two samples between each pair, on the line
/// between them.
pub fn to_48k(s: &[i16]) -> Vec<i16> {
    let mut out = Vec::with_capacity(s.len() * 3);
    for (i, &a) in s.iter().enumerate() {
        let b = s.get(i + 1).copied().unwrap_or(a);
        let (a32, b32) = (i32::from(a), i32::from(b));
        out.push(a);
        out.push(((a32 * 2 + b32) / 3) as i16);
        out.push(((a32 + b32 * 2) / 3) as i16);
    }
    out
}

/// A clip through the relay (the slow view), a piece a request.
#[derive(Debug)]
pub struct Pieces {
    pub id: String,
    n: u32,
    got: Vec<Option<Vec<u8>>>,
    bytes: usize,
}

impl Pieces {
    pub fn new(id: &str, n: u32) -> Result<Self, String> {
        if id.is_empty() || id.len() > 32 {
            return Err("The clip needs an id.".into());
        }
        if n == 0 || n > MAX_PIECES {
            return Err("The clip is too long.".into());
        }
        Ok(Self {
            id: id.to_owned(),
            n,
            got: vec![None; n as usize],
            bytes: 0,
        })
    }

    /// Piece `i` of `n`; the whole clip once every piece came.
    pub fn add(&mut self, i: u32, n: u32, piece: Vec<u8>) -> Result<Option<Vec<u8>>, String> {
        if n != self.n || i >= n {
            return Err("That piece is not of this clip.".into());
        }
        if piece.len() > MAX_PIECE {
            return Err("A piece of the clip is too large.".into());
        }
        let slot = &mut self.got[i as usize];
        if slot.is_none() {
            self.bytes += piece.len();
            if self.bytes > MAX_CLIP_BYTES {
                return Err("The clip is longer than 5 minutes.".into());
            }
            *slot = Some(piece);
        }
        if self.got.iter().all(Option::is_some) {
            return Ok(Some(
                self.got
                    .iter_mut()
                    .flat_map(|p| p.take().unwrap_or_default())
                    .collect(),
            ));
        }
        Ok(None)
    }
}

/// A clip over its own data channel: `{"t":"clip","bytes":N}`, the bytes, `{"t":"end"}`.
#[derive(Debug, Default)]
pub struct ClipStream {
    expected: Option<usize>,
    data: Vec<u8>,
}

impl ClipStream {
    /// A text message; the whole clip after `end`.
    pub fn text(&mut self, msg: &str) -> Result<Option<Vec<u8>>, String> {
        let v: serde_json::Value =
            serde_json::from_str(msg).map_err(|_| "not a clip message".to_string())?;
        match v.get("t").and_then(serde_json::Value::as_str) {
            Some("clip") => {
                let n = v
                    .get("bytes")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok())
                    .filter(|&n| n > 0 && n <= MAX_CLIP_BYTES)
                    .ok_or("The clip is too long, or empty.")?;
                self.expected = Some(n);
                self.data = Vec::with_capacity(n);
                Ok(None)
            }
            Some("end") => {
                let n = self
                    .expected
                    .take()
                    .ok_or("A clip ended before it began.")?;
                if self.data.len() != n {
                    return Err(format!(
                        "The clip came short ({} of {n} bytes).",
                        self.data.len()
                    ));
                }
                Ok(Some(std::mem::take(&mut self.data)))
            }
            _ => Err("not a clip message".into()),
        }
    }

    pub fn binary(&mut self, b: &[u8]) -> Result<(), String> {
        let n = self
            .expected
            .ok_or("Clip bytes came before the clip began.")?;
        if self.data.len() + b.len() > n {
            return Err("The clip is longer than it said.".into());
        }
        self.data.extend_from_slice(b);
        Ok(())
    }
}

/// The clip's file name, chosen by Vyro (never the phone): "Clip 2026-10-21 14-03-22.m4a".
pub fn clip_name(t: [u16; 6], ext: &str) -> String {
    format!(
        "Clip {:04}-{:02}-{:02} {:02}-{:02}-{:02}.{ext}",
        t[0], t[1], t[2], t[3], t[4], t[5]
    )
}

/// UTC from Unix seconds, as [year, month, day, hour, minute, second] (for builds without
/// Windows' local time).
pub fn utc_parts(unix: i64) -> [u16; 6] {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let p = |v: i64| u16::try_from(v).unwrap_or(0);
    [
        p(y),
        p(m),
        p(d),
        p(secs / 3600),
        p(secs % 3600 / 60),
        p(secs % 60),
    ]
}

/// A live frame from the phone: its number (u32, little-endian), then 20 ms of samples.
pub fn read_frame(b: &[u8]) -> Result<(u32, Vec<i16>), String> {
    if b.len() != 4 + FRAME_SAMPLES * 2 {
        return Err("not a microphone frame".into());
    }
    let seq = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    Ok((seq, samples(&b[4..])))
}

/// The phone's microphone as it arrives (out of order, some lost, unevenly), played out evenly:
/// frames wait in order until `delay` frames are in hand, a late one is dropped, a missing one
/// is silence, and when it runs dry it waits for `delay` frames again.
#[derive(Debug)]
pub struct Jitter {
    delay: usize,
    frames: BTreeMap<u32, Vec<i16>>,
    next: Option<u32>,
    playing: bool,
}

/// Frames held before playing (80 ms).
pub const JITTER_FRAMES: usize = 4;
/// A frame further ahead than this is a phone that started again: start from it.
const RESTART_GAP: u32 = 50;

impl Default for Jitter {
    fn default() -> Self {
        Self::new(JITTER_FRAMES)
    }
}

impl Jitter {
    pub fn new(delay: usize) -> Self {
        Self {
            delay: delay.max(1),
            frames: BTreeMap::new(),
            next: None,
            playing: false,
        }
    }

    pub fn push(&mut self, seq: u32, samples: Vec<i16>) {
        if let Some(next) = self.next {
            if seq < next && next - seq < RESTART_GAP {
                return; // too late: its time has passed
            }
            if (seq >= next && seq - next > RESTART_GAP) || seq < next {
                // The phone started again (or a long gap): from this frame on.
                self.frames.clear();
                self.next = None;
                self.playing = false;
            }
        }
        self.frames.insert(seq, samples);
        if self.frames.len() > self.delay * 8 {
            // Far too much waiting (the player stalled): keep only the newest.
            let keep = self
                .frames
                .split_off(&(seq.saturating_sub(self.delay as u32)));
            self.frames = keep;
            self.next = None;
        }
    }

    /// The next 20 ms to play: a frame, silence for a lost one, or `None` while it fills.
    pub fn pop(&mut self) -> Option<Vec<i16>> {
        if !self.playing {
            if self.frames.len() < self.delay {
                return None;
            }
            self.playing = true;
        }
        let Some((&first, _)) = self.frames.first_key_value() else {
            self.playing = false;
            self.next = None;
            return None;
        };
        let at = *self.next.get_or_insert(first);
        self.next = Some(at.wrapping_add(1));
        Some(
            self.frames
                .remove(&at)
                .unwrap_or_else(|| vec![0; FRAME_SAMPLES]),
        )
    }
}

/// A virtual cable: what Vyro plays into, and the microphone Windows then offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cable {
    /// The playback device's name ("CABLE Input (VB-Audio Virtual Cable)").
    pub render: String,
    /// The recording device's name ("CABLE Output (VB-Audio Virtual Cable)").
    pub capture: String,
}

/// The cable among the PC's playback and recording devices, by the names cables have: VB-CABLE
/// (and its A/B), VoiceMeeter, Virtual Audio Cable.
pub fn find_cable(render: &[String], capture: &[String]) -> Option<Cable> {
    let lower = |s: &str| s.to_lowercase();
    // (playback name holds, recording name holds)
    const PAIRS: &[(&str, &str)] = &[
        ("cable input", "cable output"),
        ("cable-a input", "cable-a output"),
        ("cable-b input", "cable-b output"),
        ("voicemeeter input", "voicemeeter output"),
        ("voicemeeter aux input", "voicemeeter aux output"),
        (
            "line 1 (virtual audio cable)",
            "line 1 (virtual audio cable)",
        ),
    ];
    for (r, c) in PAIRS {
        let Some(rn) = render.iter().find(|n| lower(n).contains(r)) else {
            continue;
        };
        if let Some(cn) = capture.iter().find(|n| lower(n).contains(c)) {
            return Some(Cable {
                render: rn.clone(),
                capture: cn.clone(),
            });
        }
    }
    None
}

#[cfg(windows)]
pub mod win;

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(pcm: &[i16]) -> Vec<u8> {
        let mut b = Vec::new();
        let data = (pcm.len() * 2) as u32;
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&RATE.to_le_bytes());
        b.extend_from_slice(&(RATE * 2).to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data.to_le_bytes());
        for s in pcm {
            b.extend_from_slice(&s.to_le_bytes());
        }
        b
    }

    #[test]
    fn takes_only_the_wav_the_phone_makes() {
        let good = wav(&[0, 1000, -1000]);
        assert_eq!(samples(read_wav(&good).unwrap()), vec![0, 1000, -1000]);
        let mut stereo = good.clone();
        stereo[22] = 2;
        assert!(read_wav(&stereo).is_err());
        let mut rate = good.clone();
        rate[24..28].copy_from_slice(&44_100u32.to_le_bytes());
        assert!(read_wav(&rate).is_err());
        assert!(read_wav(&good[..good.len() - 1]).is_err(), "a short one");
        assert!(read_wav(b"RIFF").is_err());
        assert!(read_wav(&vec![0; MAX_CLIP_BYTES + 1]).is_err(), "too long");
    }

    #[test]
    fn makes_48k_for_the_encoder() {
        assert_eq!(to_48k(&[0, 300]), vec![0, 100, 200, 300, 300, 300]);
        assert_eq!(to_48k(&[]).len(), 0);
    }

    #[test]
    fn puts_a_relay_clip_together_in_any_order() {
        let mut p = Pieces::new("abc", 3).unwrap();
        assert_eq!(p.add(2, 3, vec![5]).unwrap(), None);
        assert_eq!(p.add(0, 3, vec![1, 2]).unwrap(), None);
        assert_eq!(
            p.add(0, 3, vec![9, 9]).unwrap(),
            None,
            "a repeat is ignored"
        );
        assert_eq!(p.add(1, 3, vec![3, 4]).unwrap(), Some(vec![1, 2, 3, 4, 5]));
        assert!(p.add(3, 3, vec![]).is_err());
        assert!(p.add(0, 4, vec![]).is_err());
        assert!(Pieces::new("", 1).is_err());
        assert!(Pieces::new("x", MAX_PIECES + 1).is_err());
        let mut big = Pieces::new("big", 2).unwrap();
        assert!(big.add(0, 2, vec![0; MAX_PIECE + 1]).is_err());
    }

    #[test]
    fn reads_a_clip_from_its_channel() {
        let mut c = ClipStream::default();
        assert!(c.binary(&[1]).is_err(), "bytes before the start");
        assert_eq!(c.text(r#"{"t":"clip","bytes":3}"#).unwrap(), None);
        c.binary(&[1, 2]).unwrap();
        assert!(c.binary(&[3, 4]).is_err(), "more than it said");
        c.binary(&[3]).unwrap();
        assert_eq!(c.text(r#"{"t":"end"}"#).unwrap(), Some(vec![1, 2, 3]));
        assert!(c.text(r#"{"t":"end"}"#).is_err());
        assert!(c.text(r#"{"t":"clip","bytes":999999999}"#).is_err());
        c.text(r#"{"t":"clip","bytes":2}"#).unwrap();
        c.binary(&[1]).unwrap();
        assert!(c.text(r#"{"t":"end"}"#).is_err(), "short");
    }

    #[test]
    fn names_a_clip() {
        assert_eq!(
            clip_name([2026, 10, 21, 9, 3, 7], "m4a"),
            "Clip 2026-10-21 09-03-07.m4a"
        );
        assert_eq!(utc_parts(0), [1970, 1, 1, 0, 0, 0]);
        assert_eq!(utc_parts(1_792_616_000), [2026, 10, 21, 20, 53, 20]);
    }

    #[test]
    fn reads_a_live_frame() {
        let mut b = 9u32.to_le_bytes().to_vec();
        b.extend(std::iter::repeat_n(0u8, FRAME_SAMPLES * 2));
        let (seq, s) = read_frame(&b).unwrap();
        assert_eq!((seq, s.len()), (9, FRAME_SAMPLES));
        assert!(read_frame(&b[..10]).is_err());
    }

    fn f(v: i16) -> Vec<i16> {
        vec![v; FRAME_SAMPLES]
    }

    #[test]
    fn plays_frames_evenly_in_order() {
        let mut j = Jitter::new(3);
        j.push(1, f(1));
        j.push(0, f(0));
        assert_eq!(j.pop(), None, "filling");
        j.push(2, f(2));
        assert_eq!(j.pop().unwrap()[0], 0);
        assert_eq!(j.pop().unwrap()[0], 1);
        // 3 is lost; 4 came: silence where 3 was.
        j.push(4, f(4));
        assert_eq!(j.pop().unwrap()[0], 2);
        assert_eq!(j.pop().unwrap()[0], 0, "silence for the lost one");
        assert_eq!(j.pop().unwrap()[0], 4);
        // 3 turns up late: dropped.
        j.push(3, f(3));
        assert_eq!(j.pop(), None, "dry: it fills again");
    }

    #[test]
    fn starts_again_with_a_phone_that_did() {
        let mut j = Jitter::new(1);
        j.push(500, f(5));
        assert_eq!(j.pop().unwrap()[0], 5);
        j.push(0, f(7));
        assert_eq!(j.pop().unwrap()[0], 7, "a new start, not a late frame");
    }

    #[test]
    fn finds_a_cable_by_its_names() {
        let render = vec![
            "Speakers (Realtek(R) Audio)".to_string(),
            "CABLE Input (VB-Audio Virtual Cable)".to_string(),
        ];
        let capture = vec![
            "Microphone Array (Realtek(R) Audio)".to_string(),
            "CABLE Output (VB-Audio Virtual Cable)".to_string(),
        ];
        assert_eq!(
            find_cable(&render, &capture),
            Some(Cable {
                render: render[1].clone(),
                capture: capture[1].clone()
            })
        );
        assert_eq!(find_cable(&render[..1], &capture), None, "no playback side");
        assert_eq!(
            find_cable(&render, &capture[..1]),
            None,
            "no recording side"
        );
        let vm = find_cable(
            &["VoiceMeeter Input (VB-Audio VoiceMeeter VAIO)".into()],
            &["VoiceMeeter Output (VB-Audio VoiceMeeter VAIO)".into()],
        );
        assert!(vm.is_some());
    }
}
