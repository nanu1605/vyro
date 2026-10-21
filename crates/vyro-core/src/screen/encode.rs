//! H.264 with Windows' own encoder (Media Foundation's H.264 encoder MFT, 1.7): NV12 in, Annex B
//! out, constrained baseline (what every phone browser plays), low latency, a steady bit rate and
//! a keyframe whenever the phone asks. It runs on the CPU; the GPU's encoder is a later step if
//! the field check finds this too costly.

use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::sync::OnceLock;

use windows::core::{Interface, GUID, HRESULT};
use windows::Win32::Media::MediaFoundation::{
    eAVEncCommonRateControlMode_CBR, eAVEncH264VProfile_Base, eAVEncH264VProfile_ConstrainedBase,
    CLSID_MSH264EncoderMFT, CODECAPI_AVEncCommonMeanBitRate, CODECAPI_AVEncCommonRateControlMode,
    CODECAPI_AVEncMPVGOPSize, CODECAPI_AVEncVideoForceKeyFrame, CODECAPI_AVLowLatencyMode,
    ICodecAPI, IMFMediaBuffer, IMFMediaType, IMFSample, IMFTransform, MFMediaType_Video,
    MFVideoFormat_H264, MFVideoFormat_NV12, MFVideoInterlace_Progressive, MFSTARTUP_LITE,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_END_OF_STREAM,
    MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
    MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MF_E_NOTACCEPTING,
    MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE, MF_MT_AVG_BITRATE,
    MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
    MF_MT_MPEG2_PROFILE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE, MF_VERSION,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Variant::VARIANT;

use super::frames;
pub use super::producer::Encoded;
use crate::live::dll::Library;

/// Media Foundation's functions, from System32's `mfplat.dll` when there is one: Windows "N"
/// editions without the Media Feature Pack have none, and Vyro must still start there (only the
/// screen's direct view is then missing).
pub(crate) struct Mf {
    pub(crate) startup: unsafe extern "system" fn(u32, u32) -> HRESULT,
    pub(crate) shutdown: unsafe extern "system" fn() -> HRESULT,
    media_type: unsafe extern "system" fn(*mut *mut c_void) -> HRESULT,
    memory_buffer: unsafe extern "system" fn(u32, *mut *mut c_void) -> HRESULT,
    sample: unsafe extern "system" fn(*mut *mut c_void) -> HRESULT,
}

pub(crate) fn mf() -> Result<&'static Mf, String> {
    static MF: OnceLock<Option<Mf>> = OnceLock::new();
    MF.get_or_init(|| {
        let lib = Library::system32("mfplat.dll")?;
        // SAFETY: each type is the export's documented signature.
        unsafe {
            Some(Mf {
                startup: lib.get(c"MFStartup")?,
                shutdown: lib.get(c"MFShutdown")?,
                media_type: lib.get(c"MFCreateMediaType")?,
                memory_buffer: lib.get(c"MFCreateMemoryBuffer")?,
                sample: lib.get(c"MFCreateSample")?,
            })
        }
    })
    .as_ref()
    .ok_or_else(|| {
        "Windows' media features are not installed (a Windows \"N\" edition needs the Media \
         Feature Pack)"
            .into()
    })
}

impl Mf {
    /// A COM object an `MFCreate…` call made.
    ///
    /// # Safety
    /// `make` must store an owned `T` in its argument on success.
    unsafe fn make<T: Interface>(
        make: impl FnOnce(*mut *mut c_void) -> HRESULT,
    ) -> windows::core::Result<T> {
        let mut p = std::ptr::null_mut();
        make(&mut p).ok()?;
        // SAFETY: the call succeeded and handed over a reference.
        Ok(unsafe { T::from_raw(p) })
    }

    pub(crate) unsafe fn media_type(&self) -> windows::core::Result<IMFMediaType> {
        // SAFETY: MFCreateMediaType hands back an owned IMFMediaType.
        unsafe { Self::make(|p| (self.media_type)(p)) }
    }

    pub(crate) unsafe fn memory_buffer(&self, len: u32) -> windows::core::Result<IMFMediaBuffer> {
        // SAFETY: MFCreateMemoryBuffer hands back an owned IMFMediaBuffer.
        unsafe { Self::make(|p| (self.memory_buffer)(len, p)) }
    }

    pub(crate) unsafe fn sample(&self) -> windows::core::Result<IMFSample> {
        // SAFETY: MFCreateSample hands back an owned IMFSample.
        unsafe { Self::make(|p| (self.sample)(p)) }
    }
}

/// Two 32-bit values in one attribute, as `MFSetAttributeSize` packs them.
fn pack(hi: u32, lo: u32) -> u64 {
    (u64::from(hi) << 32) | u64::from(lo)
}

fn err(what: &str, e: &windows::core::Error) -> String {
    format!("{what}: {e}")
}

pub struct Encoder {
    mft: IMFTransform,
    codec: Option<ICodecAPI>,
    fps: u32,
    /// The output samples' size when the encoder does not make its own.
    out_size: u32,
    provides: bool,
    /// SPS and PPS, from the first keyframe, for keyframes that come without them.
    headers: Vec<u8>,
}

// SAFETY: the MFT is created in the multithreaded apartment and used from one thread at a time
// (the session's producer).
unsafe impl Send for Encoder {}

/// Low latency (a picture out for each picture in, as far as the encoder allows), a steady bit
/// rate and a keyframe at least every `gop` pictures. Set before the media types too: some
/// settings only count then.
///
/// # Safety
/// `c` is the encoder's live codec interface.
unsafe fn tune(c: &ICodecAPI, gop: u32) {
    // SAFETY: plain property sets; an encoder without one of these still encodes.
    unsafe {
        let _ = c.SetValue(&CODECAPI_AVLowLatencyMode, &VARIANT::from(true));
        let _ = c.SetValue(
            &CODECAPI_AVEncCommonRateControlMode,
            &VARIANT::from(eAVEncCommonRateControlMode_CBR.0 as u32),
        );
        let _ = c.SetValue(&CODECAPI_AVEncMPVGOPSize, &VARIANT::from(gop));
    }
}

impl Encoder {
    /// An encoder for `w`×`h` NV12 pictures (even sides) at `fps`, `bitrate` bits a second.
    pub fn new(w: u32, h: u32, fps: u32, bitrate: u32) -> Result<Self, String> {
        let mf = mf()?;
        // SAFETY: plain COM and Media Foundation calls; every object is owned by its wrapper.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            (mf.startup)(MF_VERSION, MFSTARTUP_LITE)
                .ok()
                .map_err(|e| err("Media Foundation", &e))?;
            let mft: IMFTransform =
                CoCreateInstance(&CLSID_MSH264EncoderMFT, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| err("the H.264 encoder", &e))?;
            let codec: Option<ICodecAPI> = mft.cast().ok();
            if let Some(c) = &codec {
                tune(c, fps * 10);
            }
            let video = |sub: &GUID| -> windows::core::Result<IMFMediaType> {
                let t = mf.media_type()?;
                t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
                t.SetGUID(&MF_MT_SUBTYPE, sub)?;
                t.SetUINT64(&MF_MT_FRAME_SIZE, pack(w, h))?;
                t.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1))?;
                t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
                t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
                Ok(t)
            };
            // The output type comes first; constrained baseline, or plain baseline on Windows
            // versions without it (both play everywhere WebRTC does).
            let mut set = Err(String::new());
            for profile in [
                eAVEncH264VProfile_ConstrainedBase.0,
                eAVEncH264VProfile_Base.0,
            ] {
                let out = video(&MFVideoFormat_H264).map_err(|e| err("output type", &e))?;
                out.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)
                    .map_err(|e| err("bit rate", &e))?;
                out.SetUINT32(&MF_MT_MPEG2_PROFILE, profile as u32)
                    .map_err(|e| err("profile", &e))?;
                set = mft
                    .SetOutputType(0, &out, 0)
                    .map_err(|e| err("output type", &e));
                if set.is_ok() {
                    break;
                }
            }
            set?;
            let input = video(&MFVideoFormat_NV12).map_err(|e| err("input type", &e))?;
            mft.SetInputType(0, &input, 0)
                .map_err(|e| err("input type", &e))?;
            if let Some(c) = &codec {
                tune(c, fps * 10);
            }
            mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .map_err(|e| err("starting", &e))?;
            mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .map_err(|e| err("starting", &e))?;
            let info = mft.GetOutputStreamInfo(0).map_err(|e| err("output", &e))?;
            let provides = info.dwFlags
                & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32
                    | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0 as u32)
                != 0;
            Ok(Self {
                mft,
                codec,
                fps: fps.max(1),
                out_size: info.cbSize.max(w * h * 3 / 2),
                provides,
                headers: Vec::new(),
            })
        }
    }

    /// Encodes one NV12 picture taken at `time_100ns`; returns the pictures that came out (often
    /// one, sometimes none while the encoder fills up), each with its own picture's time. With
    /// `keyframe`, the next one is a keyframe.
    pub fn encode(
        &mut self,
        nv12: &[u8],
        time_100ns: i64,
        keyframe: bool,
    ) -> Result<Vec<Encoded>, String> {
        // SAFETY: the buffer is locked only while copied into, within its length.
        let sample = unsafe {
            if keyframe {
                if let Some(c) = &self.codec {
                    let _ = c.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &VARIANT::from(1u32));
                }
            }
            let len = u32::try_from(nv12.len()).map_err(|_| "picture too large")?;
            let mf = mf()?;
            let buf = mf.memory_buffer(len).map_err(|e| err("buffer", &e))?;
            let mut p = std::ptr::null_mut();
            buf.Lock(&mut p, None, None)
                .map_err(|e| err("buffer", &e))?;
            std::ptr::copy_nonoverlapping(nv12.as_ptr(), p, nv12.len());
            buf.Unlock().map_err(|e| err("buffer", &e))?;
            buf.SetCurrentLength(len).map_err(|e| err("buffer", &e))?;
            let s = mf.sample().map_err(|e| err("sample", &e))?;
            s.AddBuffer(&buf).map_err(|e| err("sample", &e))?;
            s.SetSampleTime(time_100ns).map_err(|e| err("sample", &e))?;
            s.SetSampleDuration(10_000_000 / i64::from(self.fps))
                .map_err(|e| err("sample", &e))?;
            s
        };
        let mut out = Vec::new();
        // SAFETY: plain MFT call with a live sample.
        match unsafe { self.mft.ProcessInput(0, &sample, 0) } {
            Ok(()) => {}
            Err(e) if e.code() == MF_E_NOTACCEPTING => {
                self.drain(&mut out)?;
                // SAFETY: as above.
                unsafe { self.mft.ProcessInput(0, &sample, 0) }.map_err(|e| err("encoding", &e))?;
            }
            Err(e) => return Err(err("encoding", &e)),
        }
        self.drain(&mut out)?;
        Ok(out)
    }

    /// Changes the bit rate from the next picture on (Windows' encoder takes it while encoding
    /// since Windows 8); false when the encoder refuses it.
    pub fn set_bitrate(&mut self, bitrate: u32) -> bool {
        let Some(c) = &self.codec else {
            return false;
        };
        // SAFETY: a plain property set on the live encoder.
        unsafe { c.SetValue(&CODECAPI_AVEncCommonMeanBitRate, &VARIANT::from(bitrate)) }.is_ok()
    }

    /// Every picture the encoder has ready.
    fn drain(&mut self, out: &mut Vec<Encoded>) -> Result<(), String> {
        loop {
            // SAFETY: the output buffer struct owns at most one sample and one collection, taken
            // back out right after the call; a sample's buffer is locked only while copied.
            unsafe {
                let mine = if self.provides {
                    None
                } else {
                    let mf = mf()?;
                    let buf = mf
                        .memory_buffer(self.out_size)
                        .map_err(|e| err("buffer", &e))?;
                    let s = mf.sample().map_err(|e| err("sample", &e))?;
                    s.AddBuffer(&buf).map_err(|e| err("sample", &e))?;
                    Some(s)
                };
                let mut db = [MFT_OUTPUT_DATA_BUFFER {
                    dwStreamID: 0,
                    pSample: ManuallyDrop::new(mine),
                    dwStatus: 0,
                    pEvents: ManuallyDrop::new(None),
                }];
                let mut status = 0u32;
                let r = self.mft.ProcessOutput(0, &mut db, &mut status);
                let sample = ManuallyDrop::take(&mut db[0].pSample);
                drop(ManuallyDrop::take(&mut db[0].pEvents));
                match r {
                    Ok(()) => {
                        if let Some(s) = sample {
                            let data = bytes(&s)?;
                            if !data.is_empty() {
                                // The input's time comes through; failing that, the latest one.
                                let time_100ns = s
                                    .GetSampleTime()
                                    .unwrap_or_else(|_| out.last().map_or(0, |e| e.time_100ns));
                                out.push(Encoded {
                                    data: self.complete(data),
                                    time_100ns,
                                });
                            }
                        }
                    }
                    Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(()),
                    Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                        let t = self
                            .mft
                            .GetOutputAvailableType(0, 0)
                            .map_err(|e| err("output type", &e))?;
                        self.mft
                            .SetOutputType(0, &t, 0)
                            .map_err(|e| err("output type", &e))?;
                    }
                    Err(e) => return Err(err("encoding", &e)),
                }
            }
        }
    }

    /// A keyframe gets the SPS and PPS in front if it lacks them; the first one supplies them.
    fn complete(&mut self, data: Vec<u8>) -> Vec<u8> {
        if self.headers.is_empty() && frames::is_keyframe(&data) {
            self.headers = parameter_sets(&data);
        }
        frames::with_headers(data, &self.headers)
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // SAFETY: plain calls on a live MFT; MFShutdown pairs the MFStartup in `new`.
        unsafe {
            let _ = self.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            if let Ok(mf) = mf() {
                let _ = (mf.shutdown)();
            }
        }
    }
}

/// The bytes of a sample.
///
/// # Safety
/// `s` is a live sample.
unsafe fn bytes(s: &IMFSample) -> Result<Vec<u8>, String> {
    // SAFETY: the contiguous buffer is locked only while copied from, within its length.
    unsafe {
        let buf = s
            .ConvertToContiguousBuffer()
            .map_err(|e| err("output", &e))?;
        let mut p = std::ptr::null_mut();
        let mut len = 0u32;
        buf.Lock(&mut p, None, Some(&mut len))
            .map_err(|e| err("output", &e))?;
        let v = std::slice::from_raw_parts(p, len as usize).to_vec();
        let _ = buf.Unlock();
        Ok(v)
    }
}

/// The SPS and PPS NAL units of `data`, with their start codes.
fn parameter_sets(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 < data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            starts.push(i);
            i += 3;
        } else {
            i += 1;
        }
    }
    for (n, &s) in starts.iter().enumerate() {
        let end = starts.get(n + 1).map_or(data.len(), |&e| {
            // A four-byte start code's leading zero belongs to the next unit.
            if e > 0 && data[e - 1] == 0 {
                e - 1
            } else {
                e
            }
        });
        if matches!(data[s + 3] & 0x1f, 7 | 8) {
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(&data[s + 3..end]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows' encoder makes a keyframe with its SPS and PPS first, then smaller pictures,
    /// and a keyframe again when asked.
    #[test]
    fn encodes_the_test_pattern() {
        let (w, h) = (320, 180);
        let mut enc = Encoder::new(w, h, 30, 1_000_000).expect("the H.264 encoder");
        let mut pictures = Vec::new();
        let mut times = Vec::new();
        let mut late = 0;
        for n in 0..30u32 {
            let bgra = frames::test_pattern(w, h, n);
            let nv12 = frames::bgra_to_nv12(&bgra, w, h, w as usize * 4, w, h);
            let want_key = n == 20;
            let out = enc
                .encode(&nv12, i64::from(n) * 333_333, n == 0 || want_key)
                .unwrap();
            late = late.max((n as usize + 1).saturating_sub(pictures.len() + out.len()));
            for e in out {
                times.push(e.time_100ns);
                pictures.push(e.data);
            }
        }
        eprintln!("screen: the encoder held back at most {late} pictures");
        assert!(
            times.windows(2).all(|t| t[0] < t[1]),
            "each picture has its own time, in order: {times:?}"
        );
        assert!(pictures.len() >= 20, "{} pictures", pictures.len());
        let first = frames::nal_types(&pictures[0]);
        assert!(
            first.contains(&7) && first.contains(&8) && first.contains(&5),
            "{first:?}"
        );
        let keys = pictures.iter().filter(|p| frames::is_keyframe(p)).count();
        eprintln!(
            "screen: encoded {} pictures, {keys} keyframes, first {} bytes, total {} bytes",
            pictures.len(),
            pictures[0].len(),
            pictures.iter().map(Vec::len).sum::<usize>()
        );
        assert!(keys >= 2, "{keys} keyframes");
        assert!(pictures
            .iter()
            .filter(|p| frames::is_keyframe(p))
            .all(|p| frames::nal_types(p).contains(&7)));
    }

    /// The bit rate changes while encoding, without a new encoder or a keyframe: a moving
    /// picture comes out far smaller at a fifth of the rate.
    #[test]
    fn the_bit_rate_changes_while_encoding() {
        let (w, h) = (640, 360);
        let mut enc = Encoder::new(w, h, 30, 2_500_000).expect("the H.264 encoder");
        let mut bytes = [0usize; 2];
        let mut keys = 0;
        // Noise: no picture compresses, so what comes out follows the rate given.
        let mut seed = 0x9e37_79b9u32;
        for n in 0..120u32 {
            if n == 60 {
                assert!(enc.set_bitrate(500_000), "the encoder takes a new rate");
            }
            let bgra: Vec<u8> = (0..w * h * 4)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    seed as u8
                })
                .collect();
            let nv12 = frames::bgra_to_nv12(&bgra, w, h, w as usize * 4, w, h);
            for e in enc.encode(&nv12, i64::from(n) * 333_333, n == 0).unwrap() {
                // The second half's last 30 pictures: the new rate has taken hold.
                if n < 60 {
                    bytes[0] += e.data.len();
                } else if n >= 90 {
                    bytes[1] += e.data.len() * 2;
                }
                keys += usize::from(frames::is_keyframe(&e.data));
            }
        }
        eprintln!(
            "screen: 2.5 Mbit/s gave {} bytes in 2 s, 0.5 Mbit/s about {}",
            bytes[0], bytes[1]
        );
        assert!(bytes[1] * 2 < bytes[0], "{bytes:?}");
        assert_eq!(keys, 1, "no keyframe for the change");
    }

    #[test]
    fn parameter_sets_are_found() {
        let data = [
            0, 0, 0, 1, 0x67, 1, 2, 0, 0, 0, 1, 0x68, 3, 0, 0, 1, 0x65, 9, 9,
        ];
        assert_eq!(
            parameter_sets(&data),
            [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 0, 1, 0x68, 3]
        );
    }
}
