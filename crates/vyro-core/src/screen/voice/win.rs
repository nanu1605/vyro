//! The phone's voice on Windows (1.7): a clip as an AAC file (`.m4a`, what a chat app takes as
//! audio) through Media Foundation's own encoder, a file put on the clipboard as Explorer puts
//! one, and the live microphone: played into a virtual cable through Core Audio, with the
//! cable's recording side made Windows' default microphone while it plays.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows::core::{w, Interface, GUID, HRESULT, HSTRING, PCWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND, POINT};
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, eConsole, eRender, ERole, IAudioClient, IAudioRenderClient,
    IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
    DEVICE_STATE_ACTIVE, WAVEFORMATEX, WAVE_FORMAT_PCM,
};
use windows::Win32::Media::MediaFoundation::{
    IMFSinkWriter, MFAudioFormat_AAC, MFAudioFormat_PCM, MFMediaType_Audio, MFSTARTUP_LITE,
    MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT,
    MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
    MF_VERSION,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW,
    SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{CF_HDROP, DROPEFFECT_COPY};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Shell::{DragQueryFileW, DROPFILES, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
};

use super::{find_cable, to_48k, Jitter, FRAME_SAMPLES, RATE};
use crate::live::dll::Library;
use crate::screen::encode;

type CreateSinkWriter =
    unsafe extern "system" fn(PCWSTR, *mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT;

/// `samples` (16 kHz) as an AAC file at `path` (96 kbit/s, 48 kHz mono: what Windows' AAC
/// encoder takes). Fails where Media Foundation has no AAC encoder (Windows "N" editions).
pub fn encode_m4a(path: &Path, samples: &[i16]) -> Result<(), String> {
    let mf = encode::mf()?;
    let lib = Library::system32("mfreadwrite.dll").ok_or("Windows' media writer is missing")?;
    // SAFETY: the export's documented signature.
    let create: CreateSinkWriter = unsafe { lib.get(c"MFCreateSinkWriterFromURL") }
        .ok_or("Windows' media writer is missing")?;
    let pcm = to_48k(samples);
    // SAFETY: COM and Media Foundation calls; every object is owned by its wrapper, and
    // MFShutdown pairs the MFStartup.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        (mf.startup)(MF_VERSION, MFSTARTUP_LITE)
            .ok()
            .map_err(|e| format!("Media Foundation: {e}"))?;
        let written = (|| -> windows::core::Result<()> {
            let url = HSTRING::from(path.as_os_str());
            let mut raw = std::ptr::null_mut();
            create(
                PCWSTR(url.as_ptr()),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut raw,
            )
            .ok()?;
            let writer = IMFSinkWriter::from_raw(raw);
            let audio = |sub: &GUID| -> windows::core::Result<_> {
                let t = mf.media_type()?;
                t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
                t.SetGUID(&MF_MT_SUBTYPE, sub)?;
                t.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
                t.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, 48_000)?;
                t.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 1)?;
                Ok(t)
            };
            let out = audio(&MFAudioFormat_AAC)?;
            out.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 12_000)?;
            let stream = writer.AddStream(&out)?;
            let input = audio(&MFAudioFormat_PCM)?;
            input.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, 2)?;
            input.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 96_000)?;
            writer.SetInputMediaType(stream, &input, None)?;
            writer.BeginWriting()?;
            let mut at: i64 = 0;
            // A tenth of a second a sample.
            for chunk in pcm.chunks(4800) {
                let len = chunk.len() * 2;
                let buf = mf.memory_buffer(len as u32)?;
                let mut p = std::ptr::null_mut();
                buf.Lock(&mut p, None, None)?;
                std::ptr::copy_nonoverlapping(chunk.as_ptr().cast::<u8>(), p, len);
                buf.Unlock()?;
                buf.SetCurrentLength(len as u32)?;
                let s = mf.sample()?;
                s.AddBuffer(&buf)?;
                let dur = chunk.len() as i64 * 10_000_000 / 48_000;
                s.SetSampleTime(at)?;
                s.SetSampleDuration(dur)?;
                at += dur;
                writer.WriteSample(stream, &s)?;
            }
            writer.Finalize()
        })();
        let _ = (mf.shutdown)();
        written.map_err(|e| format!("the AAC file: {e}"))
    }
}

/// A window for this thread that only owns the clipboard while files go on it.
struct Owner(HWND);

impl Owner {
    fn new() -> Result<Self, String> {
        // SAFETY: a message-only window of a system class, destroyed in Drop.
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("Vyro clipboard"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .map(Self)
        .map_err(|e| format!("the clipboard: {e}"))
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        // SAFETY: the window made in `new`; the clipboard keeps data already put on it.
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

/// Opens the clipboard, waiting a little for another program that holds it.
fn open(owner: Option<HWND>) -> Result<(), String> {
    for _ in 0..20 {
        // SAFETY: plain call; closed by the caller.
        if unsafe { OpenClipboard(owner) }.is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err("another program holds the clipboard".into())
}

/// Global memory holding `bytes`, for the clipboard (which then owns it).
fn global(bytes: &[u8]) -> Result<HGLOBAL, String> {
    // SAFETY: a fresh allocation, filled while locked; freed by the caller if not handed over.
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(|e| e.to_string())?;
        let p = GlobalLock(h).cast::<u8>();
        if p.is_null() {
            let _ = GlobalFree(Some(h));
            return Err("no memory for the clipboard".into());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        let _ = GlobalUnlock(h);
        Ok(h)
    }
}

/// Puts `files` on the clipboard as Explorer's Copy does, so Paste in a chat attaches them.
pub fn files_to_clipboard(files: &[PathBuf]) -> Result<(), String> {
    let mut list: Vec<u16> = Vec::new();
    for f in files {
        list.extend(f.as_os_str().encode_wide());
        list.push(0);
    }
    list.push(0);
    let head = DROPFILES {
        pFiles: size_of::<DROPFILES>() as u32,
        pt: POINT::default(),
        fNC: false.into(),
        fWide: true.into(),
    };
    let mut drop = Vec::with_capacity(size_of::<DROPFILES>() + list.len() * 2);
    // SAFETY: DROPFILES is plain data; its bytes as they are in memory.
    drop.extend_from_slice(unsafe {
        std::slice::from_raw_parts((&raw const head).cast::<u8>(), size_of::<DROPFILES>())
    });
    for u in &list {
        drop.extend_from_slice(&u.to_le_bytes());
    }
    let effect = DROPEFFECT_COPY.0.to_le_bytes();
    let owner = Owner::new()?;
    open(Some(owner.0))?;
    let put = (|| -> Result<(), String> {
        // SAFETY: the clipboard is open by this thread; handed-over memory is the clipboard's.
        unsafe {
            EmptyClipboard().map_err(|e| e.to_string())?;
            let files = global(&drop)?;
            if let Err(e) = SetClipboardData(u32::from(CF_HDROP.0), Some(HANDLE(files.0))) {
                let _ = GlobalFree(Some(files));
                return Err(e.to_string());
            }
            let format = RegisterClipboardFormatW(w!("Preferred DropEffect"));
            let how = global(&effect)?;
            if SetClipboardData(format, Some(HANDLE(how.0))).is_err() {
                let _ = GlobalFree(Some(how));
            }
        }
        Ok(())
    })();
    // SAFETY: opened above.
    let _ = unsafe { CloseClipboard() };
    put.map_err(|e| format!("the clipboard: {e}"))
}

/// The files on the clipboard now (for the tests).
pub fn clipboard_files() -> Result<Vec<PathBuf>, String> {
    open(None)?;
    // SAFETY: the clipboard is open; the handle is the clipboard's and read before closing.
    let files = unsafe {
        GetClipboardData(u32::from(CF_HDROP.0)).map(|h| {
            let drop = HDROP(h.0);
            let n = DragQueryFileW(drop, u32::MAX, None);
            (0..n)
                .map(|i| {
                    let len = DragQueryFileW(drop, i, None) as usize;
                    let mut buf = vec![0u16; len + 1];
                    let got = DragQueryFileW(drop, i, Some(&mut buf)) as usize;
                    PathBuf::from(String::from_utf16_lossy(&buf[..got]))
                })
                .collect::<Vec<_>>()
        })
    };
    // SAFETY: opened above.
    let _ = unsafe { CloseClipboard() };
    files.map_err(|e| format!("no files on the clipboard: {e}"))
}

/// A sound device: Windows' id for it and its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub id: String,
    pub name: String,
}

fn enumerator() -> Result<IMMDeviceEnumerator, String> {
    // SAFETY: COM for this thread (left on: the thread's other COM users share it), then the
    // standard Core Audio entry point.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())
    }
}

fn endpoint(dev: &IMMDevice) -> Option<Endpoint> {
    // SAFETY: COM calls; the strings are copied and freed.
    unsafe {
        let raw = dev.GetId().ok()?;
        let id = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0.cast()));
        let store = dev.OpenPropertyStore(STGM_READ).ok()?;
        let v = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let s = PropVariantToStringAlloc(&v).ok()?;
        let name = s.to_string().ok();
        CoTaskMemFree(Some(s.0.cast()));
        Some(Endpoint {
            id: id?,
            name: name?,
        })
    }
}

/// The working playback and recording devices.
pub fn endpoints() -> Result<(Vec<Endpoint>, Vec<Endpoint>), String> {
    let en = enumerator()?;
    let list = |flow| -> Vec<Endpoint> {
        // SAFETY: COM calls with indexes below the count.
        unsafe {
            let Ok(c) = en.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE) else {
                return Vec::new();
            };
            let n = c.GetCount().unwrap_or(0);
            (0..n)
                .filter_map(|i| c.Item(i).ok())
                .filter_map(|d| endpoint(&d))
                .collect()
        }
    };
    Ok((list(eRender), list(eCapture)))
}

/// A cable's two sides as Windows knows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CableDevices {
    /// What Vyro plays into.
    pub render: Endpoint,
    /// The microphone Windows then offers.
    pub capture: Endpoint,
}

/// The virtual cable on this PC, if one is installed ([`find_cable`]).
pub fn cable() -> Result<Option<CableDevices>, String> {
    let (render, capture) = endpoints()?;
    let names = |l: &[Endpoint]| l.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
    let Some(found) = find_cable(&names(&render), &names(&capture)) else {
        return Ok(None);
    };
    let by = |l: &[Endpoint], name: &str| l.iter().find(|e| e.name == name).cloned();
    Ok(by(&render, &found.render)
        .zip(by(&capture, &found.capture))
        .map(|(render, capture)| CableDevices { render, capture }))
}

/// The local time now, as [year, month, day, hour, minute, second] (a clip's name).
pub fn local_now() -> [u16; 6] {
    // SAFETY: plain call.
    let t = unsafe { GetLocalTime() };
    [t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond]
}

/// The default microphone's id, for console use and for calls.
pub fn default_microphones() -> Result<(Option<String>, Option<String>), String> {
    let en = enumerator()?;
    let of = |role| {
        // SAFETY: COM call.
        unsafe { en.GetDefaultAudioEndpoint(eCapture, role) }
            .ok()
            .and_then(|d| endpoint(&d))
            .map(|e| e.id)
    };
    Ok((of(eConsole), of(eCommunications)))
}

/// Windows' own interface for the default sound devices, `IPolicyConfig` (Settings and the
/// volume mixer use it; undocumented, but unchanged since Windows 7, and EarTrumpet and
/// SoundSwitch rely on it). Only `SetDefaultEndpoint` is called: the eleventh method after
/// IUnknown's three.
#[repr(C)]
struct PolicyConfigVtbl {
    base: windows::core::IUnknown_Vtbl,
    before: [usize; 10],
    set_default_endpoint: unsafe extern "system" fn(*mut c_void, PCWSTR, ERole) -> HRESULT,
}

const IID_POLICY_CONFIG: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);
const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
/// Makes `id` the default microphone for console use and for calls (`None` leaves that role).
pub fn set_default_microphone(console: Option<&str>, calls: Option<&str>) -> Result<(), String> {
    // SAFETY: COM for this thread, then the interface's own calls with live strings.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let unknown: windows::core::IUnknown =
            CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL)
                .map_err(|e| format!("Windows' sound settings: {e}"))?;
        let mut raw = std::ptr::null_mut();
        unknown
            .query(&IID_POLICY_CONFIG, &mut raw)
            .ok()
            .map_err(|e| format!("Windows' sound settings: {e}"))?;
        // Owned from here: released when it goes.
        let _policy = windows::core::IUnknown::from_raw(raw);
        let vtbl = &**raw.cast::<*const PolicyConfigVtbl>();
        // Each role on its own: one that fails (its device unplugged) leaves the other done.
        let mut failed = None;
        for (id, role) in [(console, eConsole), (calls, eCommunications)] {
            if let Some(id) = id {
                let id = HSTRING::from(id);
                if let Err(e) = (vtbl.set_default_endpoint)(raw, PCWSTR(id.as_ptr()), role).ok() {
                    failed.get_or_insert(format!("the default microphone: {e}"));
                }
            }
        }
        failed.map_or(Ok(()), Err)
    }
}

/// The phone's microphone played into a cable's playback side, from a jitter buffer the
/// session fills, until dropped.
pub struct CablePlayer {
    pub frames: Arc<Mutex<Jitter>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl CablePlayer {
    /// Plays into the playback device `render_id`.
    pub fn start(render_id: &str) -> Result<Self, String> {
        let frames = Arc::new(Mutex::new(Jitter::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let thread = {
            let (frames, stop, id) = (Arc::clone(&frames), Arc::clone(&stop), render_id.to_owned());
            std::thread::Builder::new()
                .name("vyro-phone-mic".into())
                .spawn(move || {
                    if let Err(e) = play(&id, &frames, &stop, &ready_tx) {
                        log::warn!("phone microphone: {e}");
                        let _ = ready_tx.send(Err(e));
                    }
                })
                .map_err(|e| e.to_string())?
        };
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self {
                frames,
                stop,
                thread: Some(thread),
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("the cable did not start".into()),
        }
    }
}

impl Drop for CablePlayer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn play(
    id: &str,
    frames: &Mutex<Jitter>,
    stop: &AtomicBool,
    ready: &std::sync::mpsc::Sender<Result<(), String>>,
) -> Result<(), String> {
    let en = enumerator()?;
    let e = |what: &str, err: windows::core::Error| format!("{what}: {err}");
    // SAFETY: Core Audio calls on live objects; the buffer is written within the size Windows
    // gave and released before the next.
    unsafe {
        let dev = en
            .GetDevice(&HSTRING::from(id))
            .map_err(|x| e("the cable", x))?;
        let client: IAudioClient = dev
            .Activate(CLSCTX_ALL, None)
            .map_err(|x| e("the cable's audio", x))?;
        let format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: 1,
            nSamplesPerSec: RATE,
            nAvgBytesPerSec: RATE * 2,
            nBlockAlign: 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        // 200 ms of buffer; Windows converts 16 kHz mono to what the cable runs at.
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                2_000_000,
                0,
                &format,
                None,
            )
            .map_err(|x| e("the cable's format", x))?;
        let size = client.GetBufferSize().map_err(|x| e("the cable", x))?;
        let render: IAudioRenderClient = client.GetService().map_err(|x| e("the cable", x))?;
        client.Start().map_err(|x| e("the cable", x))?;
        let _ = ready.send(Ok(()));
        let mut pending: Vec<i16> = Vec::new();
        while !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(10));
            let padding = client.GetCurrentPadding().unwrap_or(size);
            let free = (size - padding) as usize;
            // Keep about 60 ms queued: enough to ride out the 10 ms sleeps, little delay.
            while pending.len() < free.min(FRAME_SAMPLES * 3) {
                match crate::lock(frames).pop() {
                    Some(f) => pending.extend(f),
                    None => break,
                }
            }
            let n = pending.len().min(free);
            if n == 0 {
                continue;
            }
            let Ok(buf) = render.GetBuffer(n as u32) else {
                continue;
            };
            std::ptr::copy_nonoverlapping(pending.as_ptr().cast::<u8>(), buf, n * 2);
            let _ = render.ReleaseBuffer(n as u32, 0);
            pending.drain(..n);
        }
        let _ = client.Stop();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A second of a 440 Hz tone, as the phone sends it.
    fn tone() -> Vec<i16> {
        (0..RATE)
            .map(|i| {
                ((i as f64 * 440.0 * std::f64::consts::TAU / f64::from(RATE)).sin() * 8000.0) as i16
            })
            .collect()
    }

    #[test]
    fn makes_an_aac_file() {
        let path = std::env::temp_dir().join(format!("vyro-clip-{}.m4a", std::process::id()));
        match encode_m4a(&path, &tone()) {
            Ok(()) => {
                let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                println!("screen: a 1 s voice clip as AAC is {len} bytes");
                assert!(len > 1000, "an AAC file with something in it ({len} bytes)");
                let _ = std::fs::remove_file(&path);
            }
            Err(e) => println!("screen: no AAC encoder here ({e}); clips are kept as WAV"),
        }
    }

    #[test]
    fn puts_a_file_on_the_clipboard() {
        let path = std::env::temp_dir().join(format!("vyro-clip-{}.wav", std::process::id()));
        std::fs::write(&path, b"RIFF").unwrap();
        match files_to_clipboard(std::slice::from_ref(&path)) {
            Ok(()) => {
                let back = clipboard_files().expect("the files just put there");
                println!("screen: the clipboard holds {back:?}");
                assert_eq!(back, vec![path.clone()]);
            }
            Err(e) => println!("screen: no clipboard here ({e})"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lists_sound_devices() {
        match endpoints() {
            Ok((render, capture)) => {
                println!(
                    "screen: {} playback and {} recording devices",
                    render.len(),
                    capture.len()
                );
                println!("screen: a cable: {:?}", cable());
            }
            Err(e) => println!("screen: no sound devices here ({e})"),
        }
    }
}
