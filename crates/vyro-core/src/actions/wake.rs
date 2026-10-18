//! Waking a PC (1.5). A phone's browser cannot send the wake packet itself (web pages cannot
//! send UDP), so another Vyro PC on the same network sends it when the phone asks. The PC to
//! wake tells its phones, sealed, its wired adapter's address while "Wake from phone" is on;
//! its readiness is read here (Windows settings only: the BIOS's cannot be read).

use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The ports wake packets are sent to (9 "discard", and 7 some cards listen on).
const PORTS: [u16; 2] = [9, 7];

/// A MAC address from `AA-BB-CC-DD-EE-FF`, `aa:bb:…` or 12 hex digits.
pub fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let hex: String = s
        .chars()
        .filter(|c| !matches!(c, '-' | ':' | '.' | ' '))
        .collect();
    if hex.len() != 12 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut mac = [0u8; 6];
    for (i, b) in mac.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    (mac != [0; 6] && mac != [0xff; 6] && mac[0] & 1 == 0).then_some(mac)
}

pub fn mac_text(mac: &[u8; 6]) -> String {
    mac.iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join("-")
}

/// The magic packet: six 0xFF bytes, then the address 16 times.
pub fn magic_packet(mac: &[u8; 6]) -> [u8; 102] {
    let mut p = [0xffu8; 102];
    for i in 0..16 {
        p[6 + i * 6..12 + i * 6].copy_from_slice(mac);
    }
    p
}

/// The broadcast address of `ip`'s network, from its prefix length.
pub fn broadcast(ip: Ipv4Addr, prefix: u8) -> Ipv4Addr {
    let bits = u32::from(ip);
    let host = if prefix >= 32 { 0 } else { u32::MAX >> prefix };
    Ipv4Addr::from(bits | host)
}

/// Sends the wake packet for `mac` from each of `sources` (this PC's addresses with their
/// prefix lengths) to its network's broadcast address and to 255.255.255.255. Gives how many
/// went out.
pub fn send(mac: &[u8; 6], sources: &[(Ipv4Addr, u8)]) -> Result<usize, String> {
    let packet = magic_packet(mac);
    let mut sent = 0;
    let mut last = None;
    let from: Vec<(Ipv4Addr, u8)> = if sources.is_empty() {
        vec![(Ipv4Addr::UNSPECIFIED, 0)]
    } else {
        sources.to_vec()
    };
    for (ip, prefix) in from {
        let socket = match UdpSocket::bind(SocketAddrV4::new(ip, 0)) {
            Ok(s) => s,
            Err(e) => {
                last = Some(e.to_string());
                continue;
            }
        };
        if let Err(e) = socket.set_broadcast(true) {
            last = Some(e.to_string());
            continue;
        }
        let mut targets = vec![Ipv4Addr::BROADCAST];
        if !ip.is_unspecified() && prefix > 0 {
            targets.push(broadcast(ip, prefix));
        }
        for to in targets {
            for port in PORTS {
                match socket.send_to(&packet, SocketAddrV4::new(to, port)) {
                    Ok(_) => sent += 1,
                    Err(e) => last = Some(e.to_string()),
                }
            }
        }
    }
    if sent == 0 {
        return Err(format!(
            "The wake packet did not go out ({}).",
            last.unwrap_or_else(|| "no network".into())
        ));
    }
    log::info!(
        "actions: wake packet for {} sent {sent} times",
        mac_text(mac)
    );
    Ok(sent)
}

/// What this PC tells its phones so another PC can wake it (only while "Wake from phone" is on).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WakeTarget {
    /// The wired adapter's address, `AA-BB-CC-DD-EE-FF`.
    pub mac: String,
}

/// How ready this PC is to be woken (Settings → Remote → Wake from phone).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WakeReadiness {
    /// The wired adapter that would be woken, by name ("Ethernet").
    pub adapter: Option<String>,
    /// Its "Wake on Magic Packet" setting; `None` when Windows does not say.
    pub magic_packet: Option<bool>,
    /// Fast Startup is on (some PCs then do not wake from shut down).
    pub fast_startup: Option<bool>,
    pub laptop: bool,
}

#[cfg(windows)]
mod win {
    use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;

    use crate::winutil::Key;

    const NET_CLASS: &str =
        r"SYSTEM\CurrentControlSet\Control\Class\{4d36e972-e325-11ce-bfc1-08002be10318}";

    /// An adapter's "Wake on Magic Packet" keyword, found by its driver's description.
    pub fn magic_packet(description: &str) -> Option<bool> {
        let class = Key::open(HKEY_LOCAL_MACHINE, NET_CLASS)?;
        class
            .subkeys()
            .iter()
            .filter_map(|k| class.subkey(k))
            .find(|k| {
                k.string("DriverDesc")
                    .is_some_and(|d| d.eq_ignore_ascii_case(description.trim()))
            })
            .and_then(|k| k.string("*WakeOnMagicPacket"))
            .map(|v| v.trim() == "1")
    }

    pub fn fast_startup() -> Option<bool> {
        Key::open(
            HKEY_LOCAL_MACHINE,
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Power",
        )?
        .dword("HiberbootEnabled")
        .map(|v| v != 0)
    }
}

/// The adapter's "Wake on Magic Packet" setting, by its driver description.
pub fn magic_packet_setting(description: &str) -> Option<bool> {
    #[cfg(windows)]
    return win::magic_packet(description);
    #[cfg(not(windows))]
    {
        let _ = description;
        None
    }
}

pub fn fast_startup() -> Option<bool> {
    #[cfg(windows)]
    return win::fast_startup();
    #[cfg(not(windows))]
    None
}
