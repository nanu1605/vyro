//! The slow view's pictures as JPEG (1.7), through Windows' own imaging (WIC): no new library.

use windows::core::{w, PWSTR};
use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR,
    IWICImagingFactory, WICBitmapEncoderNoCache,
};
use windows::Win32::System::Com::StructuredStorage::{CreateStreamOnHGlobal, PROPBAG2};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, STATFLAG_NONAME,
    STATSTG, STREAM_SEEK_SET,
};
use windows::Win32::System::Variant::VARIANT;

fn err(what: &str) -> impl Fn(windows::core::Error) -> String + '_ {
    move |e| format!("JPEG {what}: {e}")
}

/// `bgr` (packed, `w`×`h`) as a JPEG file at `quality` (0 to 1).
pub fn encode(bgr: &[u8], w: u32, h: u32, quality: f32) -> Result<Vec<u8>, String> {
    let stride = w * 3;
    if w == 0 || h == 0 || bgr.len() < (stride * h) as usize {
        return Err("JPEG: no picture".into());
    }
    // SAFETY: COM calls on objects made here; the pixel slice is the size checked above, and
    // the stream is read into a buffer of the length it reports.
    unsafe {
        // Already initialised on this thread (either model) is fine.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(err("imaging"))?;
        // In memory, freed with the stream.
        let stream = CreateStreamOnHGlobal(HGLOBAL::default(), true).map_err(err("stream"))?;
        let encoder = factory
            .CreateEncoder(&GUID_ContainerFormatJpeg, std::ptr::null())
            .map_err(err("encoder"))?;
        encoder
            .Initialize(&stream, WICBitmapEncoderNoCache)
            .map_err(err("encoder"))?;
        let mut frame = None;
        let mut bag = None;
        encoder
            .CreateNewFrame(&mut frame, &mut bag)
            .map_err(err("frame"))?;
        let frame = frame.ok_or("JPEG frame: none")?;
        if let Some(bag) = &bag {
            let option = PROPBAG2 {
                pstrName: PWSTR(w!("ImageQuality").as_ptr().cast_mut()),
                ..Default::default()
            };
            let value = VARIANT::from(quality.clamp(0.0, 1.0));
            bag.Write(1, &option, &value).map_err(err("quality"))?;
        }
        frame.Initialize(bag.as_ref()).map_err(err("frame"))?;
        frame.SetSize(w, h).map_err(err("frame"))?;
        let mut format = GUID_WICPixelFormat24bppBGR;
        frame.SetPixelFormat(&mut format).map_err(err("frame"))?;
        if format != GUID_WICPixelFormat24bppBGR {
            return Err("JPEG: the encoder wants another pixel format".into());
        }
        frame
            .WritePixels(h, stride, &bgr[..(stride * h) as usize])
            .map_err(err("pixels"))?;
        frame.Commit().map_err(err("frame"))?;
        encoder.Commit().map_err(err("encoder"))?;
        let mut stat = STATSTG::default();
        stream
            .Stat(&mut stat, STATFLAG_NONAME)
            .map_err(err("stream"))?;
        let len = stat.cbSize;
        stream
            .Seek(0, STREAM_SEEK_SET, None)
            .map_err(err("stream"))?;
        let mut buf = vec![0u8; usize::try_from(len).map_err(|_| "JPEG: too large")?];
        let mut read = 0u32;
        stream
            .Read(
                buf.as_mut_ptr().cast(),
                u32::try_from(buf.len()).map_err(|_| "JPEG: too large")?,
                Some(&mut read),
            )
            .ok()
            .map_err(err("stream"))?;
        buf.truncate(read as usize);
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::frames;

    #[test]
    fn the_test_pattern_becomes_a_jpeg() {
        let (w, h) = (320, 180);
        let bgra = frames::test_pattern(w, h, 3);
        let bgr = frames::bgra_to_bgr(&bgra, w, h, w as usize * 4, w, h);
        let jpeg = encode(&bgr, w, h, 0.6).expect("WIC's JPEG encoder");
        assert_eq!(&jpeg[..3], &[0xff, 0xd8, 0xff], "a JPEG file");
        assert_eq!(&jpeg[jpeg.len() - 2..], &[0xff, 0xd9]);
        let small = encode(&bgr, w, h, 0.2).unwrap();
        eprintln!(
            "screen: JPEG {w}x{h}: {} bytes at 0.6, {} at 0.2",
            jpeg.len(),
            small.len()
        );
        assert!(small.len() < jpeg.len());
        assert!(encode(&bgr[..10], w, h, 0.6).is_err());
    }
}
