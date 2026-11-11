//! PresentMon console CSV (`--output_stdout --v2_metrics`): one row per frame. Columns are
//! found by name, so optional ones (GPU, display tracking) may be missing.

/// One frame of the captured application. Times in milliseconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub app: String,
    pub pid: u32,
    /// Graphics API ("DXGI", "D3D9", "Vulkan", …).
    pub runtime: String,
    pub present_mode: Option<String>,
    /// CPU start of this frame, since the capture began.
    pub start_ms: f64,
    /// CPU start of this frame to the next one's: the frame time that FPS is made of.
    pub frame_ms: f64,
    pub cpu_busy_ms: Option<f64>,
    pub gpu_busy_ms: Option<f64>,
    /// Frame start to on screen; `None` for a frame that was never displayed.
    pub display_latency_ms: Option<f64>,
    /// How long the frame stayed on screen; `None` if it was dropped.
    pub displayed_ms: Option<f64>,
}

/// Column positions from the header line.
#[derive(Debug, Clone)]
pub struct Header {
    len: usize,
    app: usize,
    pid: usize,
    runtime: Option<usize>,
    present_mode: Option<usize>,
    start: usize,
    frame: usize,
    cpu_busy: Option<usize>,
    gpu_busy: Option<usize>,
    display_latency: Option<usize>,
    displayed: Option<usize>,
}

impl Header {
    /// `None` unless the line is a v2-metrics header (application, process id, CPU start and
    /// frame time are required).
    pub fn parse(line: &str) -> Option<Self> {
        let cols: Vec<&str> = line.trim_end().split(',').map(str::trim).collect();
        let at = |name: &str| cols.iter().position(|c| c.eq_ignore_ascii_case(name));
        Some(Self {
            len: cols.len(),
            app: at("Application")?,
            pid: at("ProcessID")?,
            runtime: at("PresentRuntime"),
            present_mode: at("PresentMode"),
            start: at("CPUStartTime").or_else(|| at("CPUStartQPCTime"))?,
            frame: at("FrameTime")?,
            cpu_busy: at("CPUBusy"),
            gpu_busy: at("GPUBusy"),
            display_latency: at("DisplayLatency"),
            displayed: at("DisplayedTime"),
        })
    }

    /// One data row. Executable names are written unquoted, so a name containing commas makes
    /// extra fields at the front; they are folded back into the name.
    pub fn row(&self, line: &str) -> Option<Frame> {
        let raw: Vec<&str> = line.trim_end().split(',').collect();
        if raw.len() < self.len || (raw.len() > self.len && self.app != 0) {
            return None;
        }
        let extra = raw.len() - self.len;
        let mut cols: Vec<String> = Vec::with_capacity(self.len);
        cols.push(raw[..=extra].join(","));
        cols.extend(raw[extra + 1..].iter().map(|s| (*s).to_owned()));
        let text = |i: usize| cols.get(i).map(|s| s.trim());
        let num = |i: Option<usize>| {
            i.and_then(text)
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v >= 0.0)
        };
        let frame_ms = num(Some(self.frame))?;
        Some(Frame {
            app: text(self.app)?.to_owned(),
            pid: text(self.pid)?.parse().ok()?,
            runtime: self.runtime.and_then(text).unwrap_or("Other").to_owned(),
            present_mode: self
                .present_mode
                .and_then(text)
                .filter(|s| !s.is_empty() && *s != "NA" && *s != "Other")
                .map(str::to_owned),
            start_ms: num(Some(self.start))?,
            frame_ms,
            cpu_busy_ms: num(self.cpu_busy),
            gpu_busy_ms: num(self.gpu_busy),
            display_latency_ms: num(self.display_latency),
            displayed_ms: num(self.displayed),
        })
    }
}

#[cfg(test)]
pub(crate) const SAMPLE: &str = "\
Application,ProcessID,SwapChainAddress,PresentRuntime,SyncInterval,PresentFlags,AllowsTearing,PresentMode,CPUStartTime,FrameTime,CPUBusy,CPUWait,GPULatency,GPUTime,GPUBusy,GPUWait,DisplayLatency,DisplayedTime,AnimationError,AnimationTime,MsFlipDelay,AllInputToPhotonLatency,ClickToPhotonLatency\r
game.exe,4242,0x1F2E3D4C,DXGI,0,512,1,Hardware: Independent Flip,1000.000,6.944,2.100,4.844,0.500,6.200,6.100,0.844,9.100,6.944,0.000,1000.000,0.000,NA,NA\r
game.exe,4242,0x1F2E3D4C,DXGI,0,512,1,Hardware: Independent Flip,1006.944,6.900,2.050,4.850,0.480,6.150,6.000,0.900,9.000,NA,NA,1006.944,0.000,NA,NA\r
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_header_and_rows() {
        let mut lines = SAMPLE.lines();
        let h = Header::parse(lines.next().unwrap()).unwrap();
        let a = h.row(lines.next().unwrap()).unwrap();
        assert_eq!(a.app, "game.exe");
        assert_eq!(a.pid, 4242);
        assert_eq!(a.runtime, "DXGI");
        assert_eq!(
            a.present_mode.as_deref(),
            Some("Hardware: Independent Flip")
        );
        assert!((a.frame_ms - 6.944).abs() < 1e-9);
        assert_eq!(a.gpu_busy_ms, Some(6.1));
        assert_eq!(a.displayed_ms, Some(6.944));
        let b = h.row(lines.next().unwrap()).unwrap();
        assert_eq!(b.displayed_ms, None, "dropped frame");
        assert_eq!(b.display_latency_ms, Some(9.0));
    }

    #[test]
    fn tolerates_commas_in_names_and_missing_columns() {
        let h =
            Header::parse("Application,ProcessID,PresentRuntime,CPUStartTime,FrameTime").unwrap();
        let f = h.row("my,odd,game.exe,7,Vulkan,12.5,16.7").unwrap();
        assert_eq!(f.app, "my,odd,game.exe");
        assert_eq!(f.pid, 7);
        assert_eq!(f.gpu_busy_ms, None);
        assert!(h.row("short,row").is_none());
        assert!(
            h.row("x.exe,7,Vulkan,12.5,NA").is_none(),
            "frame time required"
        );
        assert!(Header::parse("error: access denied").is_none());
        assert!(Header::parse("Application,ProcessID,TimeInSeconds,MsBetweenPresents").is_none());
    }
}
