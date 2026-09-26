//! Windows network collector: `GetIfTable2` (counters, link speed, state), `GetAdaptersAddresses`
//! (addresses, gateways, DNS), the WLAN API (Wi-Fi link) and an ICMP echo thread
//! (`IcmpSendEcho`, no elevation needed).
//!
//! Since Windows 11 24H2 the WLAN API only returns the SSID / BSSID (and the connection record
//! that carries signal quality) to apps allowed to use location; without it the channel and RSSI
//! queries still work and the rest is reported as unavailable with a note.

use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::core::GUID;
use windows::Win32::Foundation::{GetLastError, HANDLE};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetAdaptersAddresses, GetBestInterface, GetIfTable2, IcmpCloseHandle,
    IcmpCreateFile, IcmpSendEcho, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST,
    GAA_FLAG_SKIP_MULTICAST, ICMP_ECHO_REPLY, IP_ADAPTER_ADDRESSES_LH, MIB_IF_TABLE2,
};
use windows::Win32::NetworkManagement::WiFi::{
    dot11_BSS_type_infrastructure, wlan_interface_state_connected, wlan_intf_opcode_channel_number,
    wlan_intf_opcode_current_connection, wlan_intf_opcode_rssi, WlanCloseHandle,
    WlanEnumInterfaces, WlanFreeMemory, WlanGetNetworkBssList, WlanOpenHandle, WlanQueryInterface,
    WLAN_BSS_LIST, WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO_LIST,
};
use windows::Win32::Networking::WinSock::{
    AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR_IN, SOCKADDR_IN6, SOCKET_ADDRESS,
};

use super::{
    channel, kind, listed, rssi_quality, security, sort, standard, utilization, valid_host,
    PingWindow, Rates, DEFAULT_PING_HOST,
};
use crate::info::{LocalOnly, NetAdapterInfo, WifiInfo};
use crate::lock;
use crate::provider::{BoxError, Provider};
use crate::registry::{Group, Registry, SensorDesc, SensorKey, Source, Unit};
use crate::snapshot::{NetKind, NetworkSnapshot, PingSnapshot, SensorSnapshot};

const ERROR_ACCESS_DENIED: u32 = 5;
const LOCATION_NOTE: &str =
    "Windows hides the network name and link details unless location access is on for desktop apps";

/// One `MIB_IF_ROW2`, reduced to what we use.
#[derive(Debug, Clone)]
struct IfRow {
    index: u32,
    guid: u128,
    alias: String,
    description: String,
    mac: Option<String>,
    mtu: u32,
    if_type: u32,
    hardware: bool,
    filter: bool,
    up: bool,
    link_mbps: Option<f64>,
    rx: u64,
    tx: u64,
}

fn wide(w: &[u16]) -> String {
    let end = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..end])
}

fn if_rows() -> Vec<IfRow> {
    let mut table: *mut MIB_IF_TABLE2 = null_mut();
    // SAFETY: out-pointer to a table the API allocates; freed below.
    if unsafe { GetIfTable2(&mut table) }.is_err() || table.is_null() {
        return Vec::new();
    }
    // SAFETY: `table` is valid until FreeMibTable; NumEntries rows follow the header.
    let rows = unsafe {
        std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize)
    };
    let out = rows
        .iter()
        .map(|r| {
            let flags = r.InterfaceAndOperStatusFlags._bitfield;
            let speed = r.ReceiveLinkSpeed.max(r.TransmitLinkSpeed);
            let mac_len = (r.PhysicalAddressLength as usize).min(r.PhysicalAddress.len());
            IfRow {
                index: r.InterfaceIndex,
                guid: r.InterfaceGuid.to_u128(),
                alias: wide(&r.Alias),
                description: wide(&r.Description),
                mac: (mac_len == 6).then(|| {
                    r.PhysicalAddress[..6]
                        .iter()
                        .map(|b| format!("{b:02X}"))
                        .collect::<Vec<_>>()
                        .join("-")
                }),
                mtu: r.Mtu,
                if_type: r.Type,
                hardware: flags & 1 != 0,
                filter: flags & 2 != 0,
                // IfOperStatusUp = 1.
                up: r.OperStatus.0 == 1,
                link_mbps: (speed > 0 && speed != u64::MAX).then(|| speed as f64 / 1e6),
                rx: r.InOctets,
                tx: r.OutOctets,
            }
        })
        .collect();
    // SAFETY: allocated by GetIfTable2.
    unsafe { FreeMibTable(table.cast()) };
    out
}

#[derive(Debug, Clone, Default)]
struct Addrs {
    ipv4: Vec<String>,
    ipv6: Vec<String>,
    gateways: Vec<String>,
    dns: Vec<String>,
    dhcp: Option<bool>,
    /// IP-level MTU (the interface row reports the NIC's jumbo-frame maximum instead).
    mtu: Option<u32>,
}

/// `SOCKET_ADDRESS` → IP address.
fn sockaddr(a: &SOCKET_ADDRESS) -> Option<std::net::IpAddr> {
    if a.lpSockaddr.is_null() {
        return None;
    }
    // SAFETY: the API guarantees lpSockaddr points at a sockaddr of iSockaddrLength bytes.
    unsafe {
        match (*a.lpSockaddr).sa_family {
            f if f == AF_INET => {
                let v4 = &*a.lpSockaddr.cast::<SOCKADDR_IN>();
                Some(Ipv4Addr::from(v4.sin_addr.S_un.S_addr.to_ne_bytes()).into())
            }
            f if f == AF_INET6 => {
                let v6 = &*a.lpSockaddr.cast::<SOCKADDR_IN6>();
                Some(Ipv6Addr::from(v6.sin6_addr.u.Byte).into())
            }
            _ => None,
        }
    }
}

fn addresses() -> HashMap<u32, Addrs> {
    let flags = GAA_FLAG_INCLUDE_GATEWAYS | GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST;
    let mut len = 16 * 1024u32;
    let mut buf: Vec<u64> = Vec::new();
    for _ in 0..4 {
        buf = vec![0u64; (len as usize).div_ceil(8)];
        // SAFETY: `buf` holds `len` writable, 8-byte aligned bytes.
        let rc = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_UNSPEC.0),
                flags,
                None,
                Some(buf.as_mut_ptr().cast()),
                &mut len,
            )
        };
        match rc {
            0 => break,
            111 => continue, // ERROR_BUFFER_OVERFLOW: `len` now holds the size needed
            _ => return HashMap::new(),
        }
    }
    let mut out = HashMap::new();
    let mut p = buf.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    while !p.is_null() {
        // SAFETY: a linked list inside `buf`, terminated by a null Next.
        let a = unsafe { &*p };
        let mut addrs = Addrs {
            // IP_ADAPTER_DHCP_ENABLED = 0x4.
            dhcp: Some(unsafe { a.Anonymous2.Flags } & 0x4 != 0),
            mtu: (a.Mtu != 0 && a.Mtu != u32::MAX).then_some(a.Mtu),
            ..Default::default()
        };
        let mut u = a.FirstUnicastAddress;
        while !u.is_null() {
            // SAFETY: list node inside `buf`.
            let node = unsafe { &*u };
            match sockaddr(&node.Address) {
                Some(std::net::IpAddr::V4(ip)) => {
                    addrs.ipv4.push(format!("{ip}/{}", node.OnLinkPrefixLength))
                }
                Some(std::net::IpAddr::V6(ip)) => {
                    addrs.ipv6.push(format!("{ip}/{}", node.OnLinkPrefixLength))
                }
                None => {}
            }
            u = node.Next;
        }
        let mut g = a.FirstGatewayAddress;
        while !g.is_null() {
            // SAFETY: list node inside `buf`.
            let node = unsafe { &*g };
            if let Some(ip) = sockaddr(&node.Address) {
                addrs.gateways.push(ip.to_string());
            }
            g = node.Next;
        }
        let mut d = a.FirstDnsServerAddress;
        while !d.is_null() {
            // SAFETY: list node inside `buf`.
            let node = unsafe { &*d };
            if let Some(ip) = sockaddr(&node.Address) {
                // Skip the site-local placeholders Windows lists on IPv6-less links.
                let ip = ip.to_string();
                if !ip.starts_with("fec0:") && !addrs.dns.contains(&ip) {
                    addrs.dns.push(ip);
                }
            }
            d = node.Next;
        }
        // SAFETY: union read; IfIndex is the documented member.
        let index = unsafe { a.Anonymous1.Anonymous.IfIndex };
        out.insert(index, addrs);
        p = a.Next;
    }
    out
}

/// Interface that carries traffic to the internet (the default route).
fn primary_index() -> Option<u32> {
    let mut index = 0u32;
    let dest = u32::from_ne_bytes([1, 1, 1, 1]);
    // SAFETY: plain out-pointer.
    (unsafe { GetBestInterface(dest, &mut index) } == 0).then_some(index)
}

/// WLAN client handle, closed on drop.
struct Wlan {
    handle: HANDLE,
    /// BSSID → centre frequency (MHz); the BSS list is costly, so it is cached.
    freq: HashMap<[u8; 6], u32>,
    denied_logged: bool,
}

// SAFETY: WLAN client handles may be used from any thread; each Wlan is owned by one provider.
unsafe impl Send for Wlan {}

impl Drop for Wlan {
    fn drop(&mut self) {
        // SAFETY: handle from WlanOpenHandle.
        unsafe { WlanCloseHandle(self.handle, None) };
    }
}

impl Wlan {
    fn open() -> Option<Self> {
        let mut version = 0u32;
        let mut handle = HANDLE::default();
        // SAFETY: out-pointers; fails cleanly when the WLAN service is not running.
        (unsafe { WlanOpenHandle(2, None, &mut version, &mut handle) } == 0).then(|| Self {
            handle,
            freq: HashMap::new(),
            denied_logged: false,
        })
    }

    /// Queries one opcode; returns the WLAN-allocated buffer, or the error code.
    fn query(
        &self,
        guid: &GUID,
        opcode: windows::Win32::NetworkManagement::WiFi::WLAN_INTF_OPCODE,
    ) -> Result<WlanBuf, u32> {
        let mut size = 0u32;
        let mut data: *mut core::ffi::c_void = null_mut();
        // SAFETY: out-pointers; the buffer is freed by WlanBuf.
        let rc = unsafe {
            WlanQueryInterface(self.handle, guid, opcode, None, &mut size, &mut data, None)
        };
        if rc != 0 || data.is_null() {
            return Err(rc);
        }
        Ok(WlanBuf(data, size as usize))
    }

    /// Wi-Fi details per interface GUID, for connected interfaces.
    fn links(&mut self) -> HashMap<u128, WifiInfo> {
        let mut out = HashMap::new();
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = null_mut();
        // SAFETY: out-pointer; freed below.
        if unsafe { WlanEnumInterfaces(self.handle, None, &mut list) } != 0 || list.is_null() {
            return out;
        }
        // SAFETY: dwNumberOfItems entries follow the header.
        let items = unsafe {
            std::slice::from_raw_parts(
                (*list).InterfaceInfo.as_ptr(),
                (*list).dwNumberOfItems as usize,
            )
        };
        for it in items
            .iter()
            .filter(|i| i.isState == wlan_interface_state_connected)
        {
            let guid = it.InterfaceGuid;
            let mut info = WifiInfo::default();
            let rssi = self
                .query(&guid, wlan_intf_opcode_rssi)
                .ok()
                .and_then(|b| b.read::<i32>());
            let chan = self
                .query(&guid, wlan_intf_opcode_channel_number)
                .ok()
                .and_then(|b| b.read::<u32>());
            info.rssi_dbm = rssi.map(f64::from);
            info.channel = chan;
            match self.query(&guid, wlan_intf_opcode_current_connection) {
                Ok(b) if b.1 >= size_of::<WLAN_CONNECTION_ATTRIBUTES>() => {
                    // SAFETY: size checked; the buffer holds a WLAN_CONNECTION_ATTRIBUTES.
                    let c = unsafe { &*b.0.cast::<WLAN_CONNECTION_ATTRIBUTES>() };
                    let assoc = &c.wlanAssociationAttributes;
                    let ssid_len = (assoc.dot11Ssid.uSSIDLength as usize).min(32);
                    let ssid =
                        String::from_utf8_lossy(&assoc.dot11Ssid.ucSSID[..ssid_len]).into_owned();
                    let bssid = assoc.dot11Bssid;
                    info.ssid = (!ssid.is_empty()).then_some(LocalOnly(ssid));
                    info.bssid = Some(LocalOnly(
                        bssid
                            .iter()
                            .map(|b| format!("{b:02X}"))
                            .collect::<Vec<_>>()
                            .join(":"),
                    ));
                    info.signal_pct = Some(f64::from(assoc.wlanSignalQuality.min(100)));
                    info.rx_mbps = Some(f64::from(assoc.ulRxRate) / 1000.0);
                    info.tx_mbps = Some(f64::from(assoc.ulTxRate) / 1000.0);
                    info.security = security(c.wlanSecurityAttributes.dot11AuthAlgorithm.0 as u32)
                        .map(str::to_owned);
                    let freq = match self.freq.get(&bssid) {
                        Some(&f) => Some(f),
                        None => self.bss_frequency(&guid, &assoc.dot11Ssid, bssid),
                    };
                    info.frequency_mhz = freq;
                    let band = freq.and_then(channel);
                    info.band = band.map(|(b, _)| b.to_owned());
                    if let Some((_, ch)) = band {
                        info.channel = Some(ch);
                    }
                    info.standard = standard(assoc.dot11PhyType.0 as u32, info.band.as_deref())
                        .map(str::to_owned);
                }
                Err(ERROR_ACCESS_DENIED) => {
                    if !self.denied_logged {
                        log::info!("network: WLAN connection details denied (location access off)");
                        self.denied_logged = true;
                    }
                    info.note = Some(LOCATION_NOTE.into());
                    info.signal_pct = rssi.map(rssi_quality);
                }
                _ => info.signal_pct = rssi.map(rssi_quality),
            }
            out.insert(guid.to_u128(), info);
        }
        // SAFETY: allocated by WlanEnumInterfaces.
        unsafe { WlanFreeMemory(list.cast()) };
        out
    }

    fn bss_frequency(
        &mut self,
        guid: &GUID,
        ssid: &windows::Win32::NetworkManagement::WiFi::DOT11_SSID,
        bssid: [u8; 6],
    ) -> Option<u32> {
        let mut list: *mut WLAN_BSS_LIST = null_mut();
        // SAFETY: out-pointer; freed below. Security flag ignored for a specific SSID.
        let rc = unsafe {
            WlanGetNetworkBssList(
                self.handle,
                guid,
                Some(ssid),
                dot11_BSS_type_infrastructure,
                true,
                None,
                &mut list,
            )
        };
        if rc != 0 || list.is_null() {
            return None;
        }
        // SAFETY: dwNumberOfItems entries follow the header.
        let entries = unsafe {
            std::slice::from_raw_parts(
                (*list).wlanBssEntries.as_ptr(),
                (*list).dwNumberOfItems as usize,
            )
        };
        let khz = entries
            .iter()
            .find(|e| e.dot11Bssid == bssid)
            .map(|e| e.ulChCenterFrequency);
        // SAFETY: allocated by WlanGetNetworkBssList.
        unsafe { WlanFreeMemory(list.cast()) };
        let mhz = khz.map(|k| k / 1000)?;
        self.freq.insert(bssid, mhz);
        Some(mhz)
    }
}

/// A WLAN-allocated buffer (pointer, size), freed on drop.
struct WlanBuf(*mut core::ffi::c_void, usize);

impl WlanBuf {
    fn read<T: Copy>(&self) -> Option<T> {
        // SAFETY: size checked; unaligned read of a plain value.
        (self.1 >= size_of::<T>()).then(|| unsafe { std::ptr::read_unaligned(self.0.cast::<T>()) })
    }
}

impl Drop for WlanBuf {
    fn drop(&mut self) {
        // SAFETY: allocated by WlanQueryInterface.
        unsafe { WlanFreeMemory(self.0) };
    }
}

fn adapter(
    row: &IfRow,
    addrs: Option<&Addrs>,
    primary: Option<u32>,
    wifi: Option<&WifiInfo>,
) -> NetAdapterInfo {
    let a = addrs.cloned().unwrap_or_default();
    NetAdapterInfo {
        id: format!("if{}", row.index),
        name: row.alias.clone(),
        description: row.description.clone(),
        kind: kind(row.if_type),
        connected: row.up,
        primary: primary == Some(row.index),
        virtual_adapter: !row.hardware,
        mac: row.mac.clone().map(LocalOnly),
        link_mbps: row.up.then_some(row.link_mbps).flatten(),
        mtu: a.mtu.or(Some(row.mtu)),
        dhcp: addrs.and(a.dhcp),
        // A disconnected adapter keeps a self-assigned 169.254.x.x address; not useful.
        ipv4: a
            .ipv4
            .into_iter()
            .filter(|ip| row.up || !ip.starts_with("169.254."))
            .collect(),
        ipv6: a.ipv6.into_iter().map(LocalOnly).collect(),
        gateways: a.gateways,
        dns: a.dns,
        wifi: wifi.cloned(),
    }
}

fn is_listed(row: &IfRow, addrs: &HashMap<u32, Addrs>) -> bool {
    let gw = addrs
        .get(&row.index)
        .is_some_and(|a| !a.gateways.is_empty());
    listed(row.if_type, row.hardware, row.filter, row.up, gw)
}

/// Every listed adapter with its configuration (for `get_network`).
pub fn collect() -> Vec<NetAdapterInfo> {
    let rows = if_rows();
    let addrs = addresses();
    let primary = primary_index();
    let wifi = Wlan::open().map(|mut w| w.links()).unwrap_or_default();
    let mut out: Vec<NetAdapterInfo> = rows
        .iter()
        .filter(|r| is_listed(r, &addrs))
        .map(|r| adapter(r, addrs.get(&r.index), primary, wifi.get(&r.guid)))
        .collect();
    sort(&mut out);
    for a in &out {
        log::info!(
            "network: {} {} [{:?}{}{}] link {:?} Mbps, {} IPv4, gateway {}, wifi {}",
            a.id,
            a.name,
            a.kind,
            if a.connected { ", up" } else { ", down" },
            if a.primary { ", primary" } else { "" },
            a.link_mbps,
            a.ipv4.len(),
            !a.gateways.is_empty(),
            a.wifi.as_ref().map_or("-".to_owned(), |w| format!(
                "{:?} ch {:?} {:?} dBm {:?}%{}",
                w.band,
                w.channel,
                w.rssi_dbm,
                w.signal_pct,
                if w.note.is_some() {
                    " (details hidden)"
                } else {
                    ""
                }
            )),
        );
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Ping
// ---------------------------------------------------------------------------------------------

struct PingShared {
    host: Mutex<String>,
    state: Mutex<(PingWindow, Option<String>)>,
}

static PING: OnceLock<Arc<PingShared>> = OnceLock::new();

fn ping_shared() -> &'static Arc<PingShared> {
    PING.get_or_init(|| {
        Arc::new(PingShared {
            host: Mutex::new(DEFAULT_PING_HOST.to_owned()),
            state: Mutex::new((PingWindow::default(), None)),
        })
    })
}

/// Changes the ping target (IPv4 address or host name); the window restarts.
pub fn set_ping_host(host: &str) -> Result<(), String> {
    let host = host.trim();
    if !valid_host(host) {
        return Err("Enter an IPv4 address or a host name".into());
    }
    let shared = ping_shared();
    *lock(&shared.host) = host.to_owned();
    let mut st = lock(&shared.state);
    st.0.clear();
    st.1 = None;
    Ok(())
}

fn resolve(host: &str) -> Result<Ipv4Addr, String> {
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return Ok(ip);
    }
    (host, 0)
        .to_socket_addrs()
        .map_err(|_| "host not found".to_owned())?
        .find_map(|a| match a {
            SocketAddr::V4(v4) => Some(*v4.ip()),
            SocketAddr::V6(_) => None,
        })
        .ok_or_else(|| "host has no IPv4 address".to_owned())
}

fn icmp_status(code: u32) -> String {
    match code {
        11002 => "destination network unreachable".into(),
        11003 => "destination host unreachable".into(),
        11010 => "request timed out".into(),
        11013 => "TTL expired in transit".into(),
        11050 => "general failure (no network?)".into(),
        c => format!("ICMP error {c}"),
    }
}

/// One probe per second on its own thread, until `stop`.
fn ping_loop(shared: &PingShared, stop: &AtomicBool) {
    // SAFETY: plain handle creation; closed at the end.
    let Ok(icmp) = (unsafe { IcmpCreateFile() }) else {
        lock(&shared.state).1 = Some("ICMP unavailable".into());
        return;
    };
    let payload = *b"vyro-ping-0123456789abcdefghijkl";
    let mut reply = vec![0u64; (size_of::<ICMP_ECHO_REPLY>() + payload.len() + 64).div_ceil(8)];
    let mut target: Option<(String, Ipv4Addr, Instant)> = None;
    while !stop.load(Ordering::Relaxed) {
        let started = Instant::now();
        let host = lock(&shared.host).clone();
        let stale = target
            .as_ref()
            .is_none_or(|(h, _, at)| *h != host || at.elapsed() > Duration::from_secs(300));
        if stale {
            target = match resolve(&host) {
                Ok(ip) => Some((host.clone(), ip, Instant::now())),
                Err(e) => {
                    let mut st = lock(&shared.state);
                    st.0.push(None);
                    st.1 = Some(e);
                    None
                }
            };
        }
        if let Some((h, ip, _)) = &target {
            let t = Instant::now();
            // SAFETY: valid handle; payload and reply buffers outlive the call.
            let n = unsafe {
                IcmpSendEcho(
                    icmp,
                    u32::from_ne_bytes(ip.octets()),
                    payload.as_ptr().cast(),
                    payload.len() as u16,
                    None,
                    reply.as_mut_ptr().cast(),
                    (reply.len() * 8) as u32,
                    1000,
                )
            };
            let rtt = t.elapsed().as_secs_f64() * 1000.0;
            let result = if n > 0 {
                // SAFETY: the reply buffer starts with an ICMP_ECHO_REPLY when n > 0.
                let r = unsafe { &*reply.as_ptr().cast::<ICMP_ECHO_REPLY>() };
                if r.Status == 0 {
                    Ok(rtt)
                } else {
                    Err(icmp_status(r.Status))
                }
            } else {
                // SAFETY: no preconditions.
                Err(icmp_status(unsafe { GetLastError() }.0))
            };
            let mut st = lock(&shared.state);
            if *lock(&shared.host) == *h {
                match result {
                    Ok(ms) => {
                        st.0.push(Some(ms));
                        st.1 = None;
                    }
                    Err(e) => {
                        st.0.push(None);
                        st.1 = Some(e);
                    }
                }
            }
        }
        let rest = Duration::from_secs(1).saturating_sub(started.elapsed());
        std::thread::sleep(rest);
    }
    // SAFETY: handle from IcmpCreateFile.
    let _ = unsafe { IcmpCloseHandle(icmp) };
}

fn ping_snapshot() -> PingSnapshot {
    let shared = ping_shared();
    let host = lock(&shared.host).clone();
    let st = lock(&shared.state);
    st.0.snapshot(&host, st.1.clone())
}

// ---------------------------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------------------------

struct Keys {
    down: SensorKey,
    up: SensorKey,
}

/// Live network: counters every tick, addresses / primary route / Wi-Fi every few seconds.
pub struct NetworkProvider {
    rates: Rates,
    started: Instant,
    addrs: HashMap<u32, Addrs>,
    primary: Option<u32>,
    wlan: Option<Wlan>,
    wifi: HashMap<u128, WifiInfo>,
    last_refresh: Option<Instant>,
    keys: HashMap<u32, Keys>,
    ping_keys: Option<(SensorKey, SensorKey)>,
    stop: Arc<AtomicBool>,
    pinger: Option<JoinHandle<()>>,
}

impl Default for NetworkProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkProvider {
    pub fn new() -> Self {
        Self {
            rates: Rates::default(),
            started: Instant::now(),
            addrs: HashMap::new(),
            primary: None,
            wlan: None,
            wifi: HashMap::new(),
            last_refresh: None,
            keys: HashMap::new(),
            ping_keys: None,
            stop: Arc::new(AtomicBool::new(false)),
            pinger: None,
        }
    }

    fn refresh(&mut self) {
        self.addrs = addresses();
        self.primary = primary_index();
        self.wifi = self.wlan.as_mut().map(Wlan::links).unwrap_or_default();
        self.last_refresh = Some(Instant::now());
    }
}

impl Drop for NetworkProvider {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.pinger.take() {
            let _ = t.join();
        }
    }
}

const REFRESH: Duration = Duration::from_secs(3);

impl Provider for NetworkProvider {
    fn name(&self) -> &'static str {
        "network"
    }

    fn init(&mut self, reg: &mut Registry) -> Result<(), BoxError> {
        self.wlan = Wlan::open();
        self.refresh();
        let r = |reg: &mut Registry, id: &str, name: &str, unit| {
            reg.register(SensorDesc::new(id, Group::Network, name, unit, Source::Os).with_history())
        };
        self.ping_keys = Some((
            r(reg, "network.ping", "Ping", Unit::Millisecond),
            r(reg, "network.loss", "Packet Loss", Unit::Percent),
        ));
        let shared = Arc::clone(ping_shared());
        let stop = Arc::clone(&self.stop);
        self.pinger = Some(
            std::thread::Builder::new()
                .name("vyro-ping".into())
                .spawn(move || ping_loop(&shared, &stop))?,
        );
        if if_rows().is_empty() {
            return Err("GetIfTable2 returned no interfaces".into());
        }
        Ok(())
    }

    fn poll(&mut self, reg: &mut Registry, snap: &mut SensorSnapshot) -> Result<(), BoxError> {
        if self.last_refresh.is_none_or(|t| t.elapsed() >= REFRESH) {
            self.refresh();
        }
        let t = self.started.elapsed().as_secs_f64();
        let rows: Vec<IfRow> = if_rows()
            .into_iter()
            .filter(|r| is_listed(r, &self.addrs))
            .collect();
        let ids: Vec<String> = rows.iter().map(|r| format!("if{}", r.index)).collect();
        self.rates.retain(&ids);

        snap.ping = ping_snapshot();
        if let Some((k_rtt, k_loss)) = self.ping_keys {
            reg.set(k_rtt, snap.ping.rtt_ms);
            reg.set(k_loss, snap.ping.loss_pct);
        }

        for (row, id) in rows.iter().zip(ids) {
            let (down, up) = self.rates.update(&id, row.rx, row.tx, t);
            let keys = self.keys.entry(row.index).or_insert_with(|| {
                let r = |reg: &mut Registry, what: &str, label: &str| {
                    reg.register(
                        SensorDesc::new(
                            format!("network.{id}.{what}"),
                            Group::Network,
                            format!("{} {label}", row.alias),
                            Unit::Megabit,
                            Source::Os,
                        )
                        .with_history(),
                    )
                };
                Keys {
                    down: r(reg, "down", "Download"),
                    up: r(reg, "up", "Upload"),
                }
            });
            let wifi = self.wifi.get(&row.guid);
            let primary = self.primary == Some(row.index);
            let link = row.up.then_some(row.link_mbps).flatten();
            snap.network.push(NetworkSnapshot {
                id,
                adapter: row.alias.clone(),
                kind: kind(row.if_type),
                connected: row.up,
                primary,
                down_mbps: reg.set(keys.down, down),
                up_mbps: reg.set(keys.up, up),
                utilization_pct: utilization(down, up, link),
                link_mbps: link,
                received_mb: Some(row.rx as f64 / 1e6),
                sent_mb: Some(row.tx as f64 / 1e6),
                ping_ms: primary.then_some(snap.ping.rtt_ms).flatten(),
                packet_loss_pct: primary.then_some(snap.ping.loss_pct).flatten(),
                wifi_signal_pct: wifi.and_then(|w| w.signal_pct),
                wifi_rssi_dbm: wifi.and_then(|w| w.rssi_dbm),
                wifi_channel: wifi.and_then(|w| w.channel),
                wifi_band: wifi.and_then(|w| w.band.clone()),
            });
        }
        snap.network.sort_by_key(|n| {
            (
                !n.primary,
                !n.connected,
                n.kind != NetKind::Ethernet,
                n.adapter.clone(),
            )
        });
        Ok(())
    }
}
