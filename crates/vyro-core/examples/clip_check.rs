//! A voice clip pasted into a real browser (1.7): makes a clip as the app makes one (a 2 s tone
//! as the phone sends it, encoded to AAC, or kept as WAV where Windows has none), puts it on the
//! clipboard as the app puts it, and prints `clip: <path> <bytes>`. A `paste` line on stdin then
//! presses Ctrl+V as the phone's Paste key does (Windows' SendInput). `scripts/clip-check.mjs`
//! runs this and reads what the browser's page received. Windows only.
//!
//! ```sh
//! node scripts/clip-check.mjs   # builds and runs this
//! ```

#[cfg(windows)]
fn main() {
    use std::io::BufRead;

    use vyro_core::screen::voice::{self, win};
    use vyro_core::screen::{capture, inject, input};

    let tone: Vec<i16> = (0..voice::RATE * 2)
        .map(|i| {
            let t = f64::from(i) / f64::from(voice::RATE);
            ((t * 440.0 * std::f64::consts::TAU).sin() * 8000.0) as i16
        })
        .collect();
    let dir = std::env::temp_dir().join("vyro-clip-check");
    std::fs::create_dir_all(&dir).expect("a folder for the clip");
    let m4a = dir.join(voice::clip_name(win::local_now(), "m4a"));
    let path = match win::encode_m4a(&m4a, &tone) {
        Ok(()) => m4a,
        Err(e) => {
            println!("note: no AAC encoder here ({e}); the clip is a WAV");
            let wav = m4a.with_extension("wav");
            let mut bytes = Vec::with_capacity(44 + tone.len() * 2);
            let data = (tone.len() * 2) as u32;
            bytes.extend_from_slice(b"RIFF");
            bytes.extend_from_slice(&(36 + data).to_le_bytes());
            bytes.extend_from_slice(b"WAVEfmt ");
            for v in [
                16u32,
                0x0001_0001,
                voice::RATE,
                voice::RATE * 2,
                0x0010_0002,
            ] {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            bytes.extend_from_slice(b"data");
            bytes.extend_from_slice(&data.to_le_bytes());
            for s in &tone {
                bytes.extend_from_slice(&s.to_le_bytes());
            }
            std::fs::write(&wav, bytes).expect("the WAV");
            wav
        }
    };
    if let Err(e) = win::files_to_clipboard(std::slice::from_ref(&path)) {
        println!("error: {e}");
        std::process::exit(1);
    }
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    println!("clip: {} {bytes}", path.display());
    let on = capture::monitors()
        .first()
        .map(|m| m.rect)
        .expect("a monitor");
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim() != "paste" {
            continue;
        }
        // As the phone's Paste key sends it.
        for msg in [
            r#"{"t":"key","k":"Control","down":true}"#,
            r#"{"t":"press","c":"v"}"#,
            r#"{"t":"key","k":"Control","down":false}"#,
        ] {
            let i = input::parse(msg).expect("an input message");
            if let Err(e) = inject::apply(&i, on) {
                println!("error: {e}");
            }
        }
        println!("pasted");
    }
}

#[cfg(not(windows))]
fn main() {
    println!("note: the clipboard check is for Windows");
}
