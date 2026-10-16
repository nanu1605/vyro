//! Who may talk to the companion, and where it can be found.

use std::net::{IpAddr, Ipv4Addr};

/// Peers let in: this PC and the local network's private and link-local ranges. Everyone else
/// (the internet, carrier-grade NAT) is dropped before a byte is read.
pub fn peer_allowed(ip: IpAddr) -> bool {
    let v4 = match ip {
        IpAddr::V4(v4) => v4,
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4,
            None => return v6.is_loopback(),
        },
    };
    v4.is_loopback() || v4.is_private() || v4.is_link_local()
}

/// The Host header must name this server by address or by this PC's name, so a web page on
/// another site cannot reach it through DNS rebinding (its Host would be the site's name).
pub fn host_allowed(host: &str, names: &[String]) -> bool {
    let host = host.trim();
    let name = if let Some(rest) = host.strip_prefix('[') {
        // [::1]:port
        match rest.split_once(']') {
            Some((inside, _)) => return inside == "::1",
            None => return false,
        }
    } else {
        host.rsplit_once(':').map_or(host, |(h, port)| {
            if port.bytes().all(|b| b.is_ascii_digit()) {
                h
            } else {
                host
            }
        })
    };
    if name.parse::<Ipv4Addr>().is_ok() {
        return true;
    }
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    let bare = name.strip_suffix(".local").unwrap_or(&name);
    bare == "localhost"
        || names
            .iter()
            .any(|n| !n.is_empty() && n.eq_ignore_ascii_case(bare))
}

/// A POST must come from the page itself: its Origin, when sent, is this host.
pub fn origin_allowed(origin: Option<&str>, host: &str) -> bool {
    origin.is_none_or(|o| o.eq_ignore_ascii_case(&format!("http://{}", host.trim())))
}

/// An address of this PC on a network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalAddr {
    pub ip: Ipv4Addr,
    /// Carries the default route.
    pub primary: bool,
    /// A VPN, Hyper-V switch or other software adapter.
    pub virtual_adapter: bool,
    pub gateway: bool,
}

/// Addresses a phone on the local network can use, best first: the adapter with the default
/// route, then physical adapters with a gateway, then the rest. Link-local addresses come
/// last (they work only on the same cable or Wi-Fi without a router).
pub fn lan_addresses(addrs: &[LocalAddr]) -> Vec<Ipv4Addr> {
    let mut out: Vec<&LocalAddr> = addrs
        .iter()
        .filter(|a| a.ip.is_private() || a.ip.is_link_local())
        .collect();
    out.sort_by_key(|a| {
        (
            a.ip.is_link_local(),
            !a.primary,
            a.virtual_adapter,
            !a.gateway,
        )
    });
    let mut ips: Vec<Ipv4Addr> = Vec::new();
    for a in out {
        if !ips.contains(&a.ip) {
            ips.push(a.ip);
        }
    }
    ips
}

/// `http://192.168.1.20:8976/`.
pub fn url(ip: Ipv4Addr, port: u16) -> String {
    format!("http://{ip}:{port}/")
}

/// This PC's addresses (Windows); off Windows, the one the default route would use.
pub fn local_addresses() -> Vec<LocalAddr> {
    #[cfg(windows)]
    return crate::network::local_ipv4();
    #[cfg(not(windows))]
    {
        // Connecting a UDP socket sends nothing; it only picks the route's source address.
        let ip = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
            .and_then(|s| {
                s.connect((Ipv4Addr::new(192, 0, 2, 1), 9))?;
                s.local_addr()
            })
            .ok()
            .and_then(|a| match a.ip() {
                IpAddr::V4(v4) if !v4.is_unspecified() => Some(v4),
                _ => None,
            });
        ip.map(|ip| LocalAddr {
            ip,
            primary: true,
            virtual_adapter: false,
            gateway: true,
        })
        .into_iter()
        .collect()
    }
}

/// This PC's name, as the local network knows it.
pub fn pc_name() -> String {
    for var in ["COMPUTERNAME", "HOSTNAME"] {
        if let Some(n) = std::env::var_os(var) {
            let n = n.to_string_lossy().trim().to_owned();
            if !n.is_empty() {
                return n;
            }
        }
    }
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}
