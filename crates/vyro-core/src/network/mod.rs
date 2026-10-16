//! Network (spec §2 Network, Phase 5): adapters and their rates, Wi-Fi link quality, and an
//! ICMP ping / packet-loss probe. The Windows side reads raw counters and structures; the
//! arithmetic (rates, utilization, Wi-Fi channel / band / standard, ping statistics) is here so
//! it is tested on Linux.

#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::{collect, local_ipv4, set_ping_host, NetworkProvider};

use std::collections::{HashMap, VecDeque};

use crate::info::{NetAdapterInfo, WifiInfo};
use crate::snapshot::{NetKind, PingSnapshot};

/// Default ping target (Cloudflare's anycast resolver: close to everyone, answers ICMP).
pub const DEFAULT_PING_HOST: &str = "1.1.1.1";
/// Probes kept for loss / average / jitter (one per second → one minute).
pub const PING_WINDOW: usize = 60;

/// `IFTYPE` → kind.
pub fn kind(if_type: u32) -> NetKind {
    match if_type {
        6 => NetKind::Ethernet,
        71 => NetKind::Wifi,
        _ => NetKind::Other,
    }
}

/// Which interfaces the panel lists: physical NICs (not their filter-driver shadows), plus
/// connected virtual adapters that carry a default route (VPNs).
pub fn listed(if_type: u32, hardware: bool, filter: bool, up: bool, has_gateway: bool) -> bool {
    const LOOPBACK: u32 = 24;
    if if_type == LOOPBACK || filter {
        return false;
    }
    hardware || (up && has_gateway)
}

/// Turns cumulative octet counters into Mbit/s per interface.
#[derive(Debug, Default)]
pub struct Rates {
    prev: HashMap<String, (u64, u64, f64)>,
}

impl Rates {
    /// `t` is a monotonic time in seconds. Returns (down, up) Mbit/s; `None` on the first sample
    /// or after a counter reset.
    pub fn update(&mut self, id: &str, rx: u64, tx: u64, t: f64) -> (Option<f64>, Option<f64>) {
        let prev = self.prev.insert(id.to_owned(), (rx, tx, t));
        let Some((prx, ptx, pt)) = prev else {
            return (None, None);
        };
        let dt = t - pt;
        if dt <= 0.0 {
            return (None, None);
        }
        let rate =
            |now: u64, before: u64| now.checked_sub(before).map(|d| d as f64 * 8.0 / 1e6 / dt);
        (rate(rx, prx), rate(tx, ptx))
    }

    /// Forgets interfaces that disappeared.
    pub fn retain(&mut self, ids: &[String]) {
        self.prev.retain(|k, _| ids.contains(k));
    }
}

/// The busier direction against the link speed, %.
pub fn utilization(down: Option<f64>, up: Option<f64>, link_mbps: Option<f64>) -> Option<f64> {
    let link = link_mbps.filter(|&l| l > 0.0)?;
    let busiest = down
        .into_iter()
        .chain(up)
        .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))))?;
    Some((busiest / link * 100.0).clamp(0.0, 100.0))
}

/// Centre frequency (MHz) → (band, channel).
pub fn channel(mhz: u32) -> Option<(&'static str, u32)> {
    match mhz {
        2484 => Some(("2.4 GHz", 14)),
        2412..=2472 => Some(("2.4 GHz", (mhz - 2407) / 5)),
        5000..=5895 => Some(("5 GHz", (mhz - 5000) / 5)),
        5955..=7115 => Some(("6 GHz", (mhz - 5950) / 5)),
        _ => None,
    }
}

/// `DOT11_PHY_TYPE` → marketing name.
pub fn standard(phy: u32, band: Option<&str>) -> Option<&'static str> {
    Some(match phy {
        4 => "802.11a",
        5 => "802.11b",
        6 => "802.11g",
        7 => "Wi-Fi 4 (802.11n)",
        8 => "Wi-Fi 5 (802.11ac)",
        9 => "WiGig (802.11ad)",
        10 if band == Some("6 GHz") => "Wi-Fi 6E (802.11ax)",
        10 => "Wi-Fi 6 (802.11ax)",
        11 => "Wi-Fi 7 (802.11be)",
        _ => return None,
    })
}

/// `DOT11_AUTH_ALGORITHM` → label.
pub fn security(auth: u32) -> Option<&'static str> {
    Some(match auth {
        1 => "Open",
        2 => "WEP (shared key)",
        3 => "WPA-Enterprise",
        4 => "WPA-Personal",
        6 => "WPA2-Enterprise",
        7 => "WPA2-Personal",
        8 | 11 => "WPA3-Enterprise",
        9 => "WPA3-Personal",
        10 => "Enhanced Open (OWE)",
        _ => return None,
    })
}

/// RSSI (dBm) → the 0–100 % quality Windows shows (linear from −100 to −50 dBm).
pub fn rssi_quality(dbm: i32) -> f64 {
    f64::from((2 * (dbm + 100)).clamp(0, 100))
}

/// Rolling window of ICMP probe results (`None` = lost).
#[derive(Debug, Default, Clone)]
pub struct PingWindow {
    probes: VecDeque<Option<f64>>,
}

impl PingWindow {
    pub fn push(&mut self, rtt_ms: Option<f64>) {
        if self.probes.len() == PING_WINDOW {
            self.probes.pop_front();
        }
        self.probes.push_back(rtt_ms);
    }

    pub fn clear(&mut self) {
        self.probes.clear();
    }

    pub fn snapshot(&self, host: &str, error: Option<String>) -> PingSnapshot {
        let replies: Vec<f64> = self.probes.iter().flatten().copied().collect();
        let n = self.probes.len();
        let mean = |v: &[f64]| (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64);
        let diffs: Vec<f64> = replies.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        PingSnapshot {
            host: host.to_owned(),
            rtt_ms: self.probes.back().copied().flatten(),
            avg_ms: mean(&replies),
            min_ms: replies.iter().copied().reduce(f64::min),
            max_ms: replies.iter().copied().reduce(f64::max),
            jitter_ms: mean(&diffs),
            loss_pct: (n > 0).then(|| (n - replies.len()) as f64 / n as f64 * 100.0),
            samples: u32::try_from(n).unwrap_or(u32::MAX),
            error,
        }
    }
}

/// Checks a user-supplied ping target: an IPv4 address or a host name.
pub fn valid_host(host: &str) -> bool {
    let h = host.trim();
    !h.is_empty()
        && h.len() <= 253
        && h.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// Primary first, then connected, then by name.
pub fn sort(adapters: &mut [NetAdapterInfo]) {
    adapters.sort_by(|a, b| {
        (!a.primary, !a.connected, a.virtual_adapter, &a.name).cmp(&(
            !b.primary,
            !b.connected,
            b.virtual_adapter,
            &b.name,
        ))
    });
}

/// Two made-up adapters (wired primary + Wi-Fi) for the browser build and non-Windows runs.
pub fn sample() -> Vec<NetAdapterInfo> {
    vec![
        NetAdapterInfo {
            id: "if12".into(),
            name: "Ethernet".into(),
            description: "Realtek Gaming 2.5GbE Family Controller".into(),
            kind: NetKind::Ethernet,
            connected: true,
            primary: true,
            link_mbps: Some(1000.0),
            mtu: Some(1500),
            dhcp: Some(true),
            ipv4: vec!["192.168.1.24/24".into()],
            gateways: vec!["192.168.1.1".into()],
            dns: vec!["192.168.1.1".into(), "1.1.1.1".into()],
            ..Default::default()
        },
        NetAdapterInfo {
            id: "if18".into(),
            name: "Wi-Fi".into(),
            description: "MediaTek Wi-Fi 6E MT7922 160MHz Wireless LAN Card".into(),
            kind: NetKind::Wifi,
            connected: true,
            link_mbps: Some(1201.0),
            mtu: Some(1500),
            dhcp: Some(true),
            ipv4: vec!["192.168.1.31/24".into()],
            gateways: vec!["192.168.1.1".into()],
            dns: vec!["192.168.1.1".into()],
            wifi: Some(WifiInfo {
                standard: standard(10, Some("5 GHz")).map(str::to_owned),
                band: Some("5 GHz".into()),
                channel: Some(36),
                frequency_mhz: Some(5180),
                signal_pct: Some(86.0),
                rssi_dbm: Some(-52.0),
                rx_mbps: Some(1201.0),
                tx_mbps: Some(960.0),
                security: security(9).map(str::to_owned),
                ..Default::default()
            }),
            ..Default::default()
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_from_counters() {
        let mut r = Rates::default();
        assert_eq!(r.update("if1", 1_000, 500, 10.0), (None, None));
        let (down, up) = r.update("if1", 1_000 + 12_500_000, 500 + 125_000, 11.0);
        assert_eq!((down, up), (Some(100.0), Some(1.0)));
        // Counter reset (adapter re-enabled): no bogus spike.
        assert_eq!(r.update("if1", 10, 10, 12.0), (None, None));
        assert_eq!(r.update("if1", 10, 10, 12.0), (None, None));
        r.retain(&[]);
        assert_eq!(r.update("if1", 20, 20, 13.0), (None, None));
    }

    #[test]
    fn utilization_uses_busier_direction() {
        assert_eq!(
            utilization(Some(250.0), Some(10.0), Some(1000.0)),
            Some(25.0)
        );
        assert_eq!(utilization(None, Some(10.0), Some(100.0)), Some(10.0));
        assert_eq!(utilization(Some(1.0), None, None), None);
        assert_eq!(utilization(None, None, Some(1000.0)), None);
    }

    #[test]
    fn wifi_channels_bands_and_names() {
        assert_eq!(channel(2412), Some(("2.4 GHz", 1)));
        assert_eq!(channel(2484), Some(("2.4 GHz", 14)));
        assert_eq!(channel(5180), Some(("5 GHz", 36)));
        assert_eq!(channel(5825), Some(("5 GHz", 165)));
        assert_eq!(channel(5955), Some(("6 GHz", 1)));
        assert_eq!(channel(6115), Some(("6 GHz", 33)));
        assert_eq!(channel(900), None);
        assert_eq!(standard(10, Some("6 GHz")), Some("Wi-Fi 6E (802.11ax)"));
        assert_eq!(standard(8, None), Some("Wi-Fi 5 (802.11ac)"));
        assert_eq!(security(7), Some("WPA2-Personal"));
        assert_eq!(rssi_quality(-52), 96.0);
        assert_eq!(rssi_quality(-40), 100.0);
        assert_eq!(rssi_quality(-110), 0.0);
    }

    #[test]
    fn adapter_filter() {
        assert!(listed(6, true, false, true, true));
        assert!(!listed(6, true, true, true, true), "filter-driver shadow");
        assert!(!listed(24, false, false, true, false), "loopback");
        assert!(
            listed(53, false, false, true, true),
            "VPN with default route"
        );
        assert!(
            !listed(6, false, false, true, false),
            "Hyper-V switch without route"
        );
    }

    #[test]
    fn ping_statistics() {
        let mut w = PingWindow::default();
        for rtt in [Some(10.0), Some(14.0), None, Some(12.0)] {
            w.push(rtt);
        }
        let s = w.snapshot("1.1.1.1", None);
        assert_eq!(s.rtt_ms, Some(12.0));
        assert_eq!(s.avg_ms, Some(12.0));
        assert_eq!((s.min_ms, s.max_ms), (Some(10.0), Some(14.0)));
        assert_eq!(s.jitter_ms, Some(3.0));
        assert_eq!(s.loss_pct, Some(25.0));
        assert_eq!(s.samples, 4);
        for _ in 0..PING_WINDOW {
            w.push(None);
        }
        let s = w.snapshot("1.1.1.1", Some("timed out".into()));
        assert_eq!((s.loss_pct, s.avg_ms, s.samples), (Some(100.0), None, 60));
        assert_eq!(PingWindow::default().snapshot("x", None).loss_pct, None);
    }

    #[test]
    fn host_validation() {
        assert!(valid_host("1.1.1.1"));
        assert!(valid_host("one.one.one.one"));
        assert!(!valid_host(""));
        assert!(!valid_host("1.1.1.1; rm -rf"));
        assert!(!valid_host("::1"));
    }

    #[test]
    fn primary_sorts_first() {
        let mut v = sample();
        v.reverse();
        sort(&mut v);
        assert!(v[0].primary);
    }
}
