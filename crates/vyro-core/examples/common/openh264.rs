//! OpenH264, standing in for Windows' encoder in the examples that send a screen off Windows
//! (tests and examples only, never in the app).

use vyro_core::screen::producer;

/// OpenH264, standing in for Windows' encoder where there is none: NV12 in as I420.
pub struct OpenH264 {
    enc: openh264::encoder::Encoder,
    width: usize,
    height: usize,
}

pub fn openh264(width: u32, height: u32, fps: u32, bitrate: u32) -> Result<OpenH264, String> {
    use openh264::encoder::{BitRate, EncoderConfig, FrameRate, UsageType};
    let config = EncoderConfig::new()
        .bitrate(BitRate::from_bps(bitrate))
        .max_frame_rate(FrameRate::from_hz(fps as f32))
        .usage_type(UsageType::ScreenContentRealTime);
    let enc =
        openh264::encoder::Encoder::with_api_config(openh264::OpenH264API::from_source(), config)
            .map_err(|e| format!("OpenH264: {e}"))?;
    Ok(OpenH264 {
        enc,
        width: width as usize,
        height: height as usize,
    })
}

impl producer::Encode for OpenH264 {
    fn encode(
        &mut self,
        nv12: &[u8],
        time_100ns: i64,
        keyframe: bool,
    ) -> Result<Vec<producer::Encoded>, String> {
        let (w, h) = (self.width, self.height);
        let (y, uv) = nv12.split_at(w * h);
        let u: Vec<u8> = uv.iter().step_by(2).copied().collect();
        let v: Vec<u8> = uv.iter().skip(1).step_by(2).copied().collect();
        let picture = openh264::formats::YUVSlices::new((y, &u, &v), (w, h), (w, w / 2, w / 2));
        if keyframe {
            self.enc.force_intra_frame();
        }
        let data = self
            .enc
            .encode(&picture)
            .map_err(|e| format!("OpenH264: {e}"))?
            .to_vec();
        Ok(if data.is_empty() {
            Vec::new()
        } else {
            vec![producer::Encoded { data, time_100ns }]
        })
    }
}
