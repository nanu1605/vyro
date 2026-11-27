//! OpenRGB's SDK server (2.6): Clokrix sends it the colour, and OpenRGB, which the owner installs
//! and leaves running with its SDK server on, drives the lights of most brands' keyboards, mice,
//! memory, boards, graphics cards, fans and strips. Written from OpenRGB's own description of its
//! protocol (Documentation/OpenRGBSDK.md); none of OpenRGB's code is used.
//!
//! Every packet is a 16-byte header ("ORGB", the device's index, the packet's id, the size of
//! what follows; numbers little-endian) and its data. Clokrix speaks protocol 5 (OpenRGB 1.0) and
//! anything older the server speaks: the server says its own version, and the lower one is used.
//! Clokrix only reads the devices, puts them in their "Direct" mode and sends colours; it never
//! asks a device to save a mode, which writes its memory (docs/control-safety.md).

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use super::colour::Rgb8;

/// OpenRGB's port ("ORGB" on a phone's keypad).
pub const PORT: u16 = 6742;
const MAGIC: &[u8; 4] = b"ORGB";
/// The newest protocol this client speaks: 5, OpenRGB 1.0.
pub const VERSION: u32 = 5;

/// The packets this client sends or reads.
pub mod id {
    pub const CONTROLLER_COUNT: u32 = 0;
    pub const CONTROLLER_DATA: u32 = 1;
    pub const PROTOCOL_VERSION: u32 = 40;
    pub const CLIENT_NAME: u32 = 50;
    /// The server's word that its devices changed.
    pub const DEVICE_LIST_UPDATED: u32 = 100;
    pub const UPDATE_LEDS: u32 = 1050;
    pub const UPDATE_MODE: u32 = 1101;
    /// Saves a mode to the device's memory: never sent.
    pub const SAVE_MODE: u32 = 1102;
}

/// A mode's flag: it takes a colour for each LED.
pub const MODE_PER_LED: u32 = 1 << 5;
/// A mode's colour mode: a colour for each LED.
pub const COLORS_PER_LED: u32 = 1;

/// What a packet holds at most: a device's description with a large matrix is well under this.
const MAX_DATA: u32 = 4 << 20;
/// At most this many of anything a device lists.
const MAX_ITEMS: usize = 4096;

/// A packet's header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub device: u32,
    pub id: u32,
    pub size: u32,
}

pub fn header(device: u32, id: u32, size: u32) -> [u8; 16] {
    let mut h = [0u8; 16];
    h[..4].copy_from_slice(MAGIC);
    h[4..8].copy_from_slice(&device.to_le_bytes());
    h[8..12].copy_from_slice(&id.to_le_bytes());
    h[12..].copy_from_slice(&size.to_le_bytes());
    h
}

pub fn parse_header(h: &[u8; 16]) -> Option<Header> {
    let u = |i: usize| u32::from_le_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]);
    (&h[..4] == MAGIC).then(|| Header {
        device: u(4),
        id: u(8),
        size: u(12),
    })
}

/// A mode, as a device lists it; `raw` is its data as the server gave it, sent back as is to
/// choose it again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mode {
    pub name: String,
    pub flags: u32,
    pub color_mode: u32,
    pub raw: Vec<u8>,
}

/// A device ("controller") as OpenRGB describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Controller {
    /// Its index on the server, as of the last count.
    pub index: u32,
    /// OpenRGB's kind of device (keyboard 5, mouse 6, ...).
    pub kind: i32,
    pub name: String,
    pub vendor: String,
    /// Where it is (a USB path, an I2C bus and address): read, never kept, as it can hold a
    /// serial number (CONTRIBUTING.md, Privacy).
    pub location: String,
    pub modes: Vec<Mode>,
    pub active_mode: i32,
    pub leds: usize,
}

impl Controller {
    /// Its "Direct" mode, which takes a colour for each LED at once.
    pub fn direct_mode(&self) -> Option<usize> {
        self.modes.iter().position(|m| {
            m.name.eq_ignore_ascii_case("direct")
                && (m.flags & MODE_PER_LED != 0 || m.color_mode == COLORS_PER_LED)
        })
    }
}

/// Reads a device's description, field by field as protocol `version` lays it out; every length
/// is checked against what is there.
struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.b.len());
        let end = end.ok_or_else(|| format!("cut short at byte {}", self.at))?;
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16, String> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }
    /// A string: its length (with the ending zero), then its bytes.
    fn string(&mut self) -> Result<String, String> {
        let n = usize::from(self.u16()?);
        let s = self.take(n)?;
        let s = s.strip_suffix(&[0]).unwrap_or(s);
        Ok(String::from_utf8_lossy(s).into_owned())
    }
    fn count(&mut self) -> Result<usize, String> {
        let n = usize::from(self.u16()?);
        if n > MAX_ITEMS {
            return Err(format!("{n} items is too many"));
        }
        Ok(n)
    }
}

fn mode(r: &mut Reader<'_>, version: u32) -> Result<Mode, String> {
    let start = r.at;
    let name = r.string()?;
    if version < 6 {
        r.u32()?; // its value, the server's own
    }
    let flags = r.u32()?;
    r.take(8)?; // speed min and max
    if version >= 3 {
        r.take(8)?; // brightness min and max
    }
    r.take(8)?; // colours min and max
    r.u32()?; // speed
    if version >= 3 {
        r.u32()?; // brightness
    }
    r.u32()?; // direction
    let color_mode = r.u32()?;
    let colors = r.count()?;
    r.take(colors * 4)?;
    Ok(Mode {
        name,
        flags,
        color_mode,
        raw: r.b[start..r.at].to_vec(),
    })
}

/// A device's description, the reply to [`id::CONTROLLER_DATA`] under protocol `version`.
pub fn parse_controller(data: &[u8], index: u32, version: u32) -> Result<Controller, String> {
    let mut r = Reader { b: data, at: 0 };
    r.u32()?; // the size of it all
    let kind = r.i32()?;
    let name = r.string()?;
    let vendor = if version >= 1 {
        r.string()?
    } else {
        String::new()
    };
    r.string()?; // description
    r.string()?; // version
    r.string()?; // serial: not kept
    let location = r.string()?;
    let n_modes = r.count()?;
    let active_mode = r.i32()?;
    let modes = (0..n_modes)
        .map(|_| mode(&mut r, version))
        .collect::<Result<Vec<_>, _>>()?;
    let zones = r.count()?;
    for _ in 0..zones {
        r.string()?; // name
        r.i32()?; // type
        r.take(12)?; // LEDs min, max and count
        let matrix = usize::from(r.u16()?);
        r.take(matrix)?;
        if version >= 4 {
            let segments = r.count()?;
            for _ in 0..segments {
                r.string()?;
                r.i32()?;
                r.take(8)?;
            }
        }
        if version >= 5 {
            r.u32()?; // flags
        }
    }
    let leds = r.count()?;
    for _ in 0..leds {
        r.string()?;
        if version < 6 {
            r.u32()?;
        }
    }
    let colors = r.count()?;
    r.take(colors * 4)?;
    Ok(Controller {
        index,
        kind,
        name,
        vendor,
        location,
        modes,
        active_mode,
        leds,
    })
}

/// [`id::UPDATE_LEDS`]'s data: every LED one colour (red, green, blue, then a zero byte).
pub fn update_leds(colours: &[Rgb8]) -> Vec<u8> {
    let n = colours.len().min(usize::from(u16::MAX));
    let size = 4 + 2 + n * 4;
    let mut d = Vec::with_capacity(size);
    d.extend_from_slice(&(size as u32).to_le_bytes());
    d.extend_from_slice(&(n as u16).to_le_bytes());
    for c in &colours[..n] {
        d.extend_from_slice(&[c.r, c.g, c.b, 0]);
    }
    d
}

/// [`id::UPDATE_MODE`]'s data: mode `index`, as the device described it.
pub fn update_mode(index: i32, raw: &[u8]) -> Vec<u8> {
    let size = 4 + 4 + raw.len();
    let mut d = Vec::with_capacity(size);
    d.extend_from_slice(&(size as u32).to_le_bytes());
    d.extend_from_slice(&index.to_le_bytes());
    d.extend_from_slice(raw);
    d
}

/// A connection to OpenRGB's SDK server.
pub struct Client {
    stream: TcpStream,
    /// The protocol both speak.
    pub version: u32,
    /// The server said its devices changed: count and read them again.
    changed: bool,
}

/// How long the server has to answer.
const ANSWER: Duration = Duration::from_secs(3);
/// How long an old server (protocol 0, which does not say its version) is waited for.
const VERSION_WAIT: Duration = Duration::from_millis(800);

impl Client {
    /// Connects to `addr` (OpenRGB on this PC: 127.0.0.1:6742), says who it is and agrees the
    /// protocol.
    pub fn connect(addr: SocketAddr) -> io::Result<Self> {
        let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
        stream.set_nodelay(true)?;
        let mut c = Self {
            stream,
            version: 0,
            changed: false,
        };
        c.send(0, id::CLIENT_NAME, b"Clokrix\0")?;
        c.send(0, id::PROTOCOL_VERSION, &VERSION.to_le_bytes())?;
        c.version = match c.answer(id::PROTOCOL_VERSION, VERSION_WAIT) {
            Ok(d) if d.len() >= 4 => u32::from_le_bytes([d[0], d[1], d[2], d[3]]).min(VERSION),
            Ok(_) => 0,
            Err(e) if e.kind() == io::ErrorKind::TimedOut => 0,
            Err(e) => return Err(e),
        };
        Ok(c)
    }

    fn send(&mut self, device: u32, id: u32, data: &[u8]) -> io::Result<()> {
        debug_assert_ne!(id, id::SAVE_MODE, "a mode is never saved to a device");
        let mut p = header(device, id, data.len() as u32).to_vec();
        p.extend_from_slice(data);
        self.stream.write_all(&p)
    }

    /// The next packet, waiting at most `wait`.
    fn packet(&mut self, wait: Duration) -> io::Result<(Header, Vec<u8>)> {
        self.stream.set_read_timeout(Some(wait))?;
        let mut h = [0u8; 16];
        self.stream.read_exact(&mut h).map_err(|e| {
            if e.kind() == io::ErrorKind::WouldBlock {
                io::Error::new(io::ErrorKind::TimedOut, "no answer")
            } else {
                e
            }
        })?;
        let h = parse_header(&h).ok_or_else(|| bad("not OpenRGB"))?;
        if h.size > MAX_DATA {
            return Err(bad("a packet too large"));
        }
        let mut d = vec![0u8; h.size as usize];
        self.stream.set_read_timeout(Some(ANSWER))?;
        self.stream.read_exact(&mut d)?;
        Ok((h, d))
    }

    /// The answer to packet `want`, noting the server's word that its devices changed on the way.
    fn answer(&mut self, want: u32, wait: Duration) -> io::Result<Vec<u8>> {
        let until = Instant::now() + wait;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "no answer"));
            }
            let (h, d) = self.packet(left)?;
            if h.id == id::DEVICE_LIST_UPDATED {
                self.changed = true;
            } else if h.id == want {
                return Ok(d);
            }
        }
    }

    /// How many devices the server has.
    pub fn count(&mut self) -> io::Result<u32> {
        self.send(0, id::CONTROLLER_COUNT, &[])?;
        let d = self.answer(id::CONTROLLER_COUNT, ANSWER)?;
        d.get(..4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| bad("no count"))
    }

    /// Device `index`'s description.
    pub fn controller(&mut self, index: u32) -> io::Result<Controller> {
        let ask = if self.version >= 1 {
            self.version.to_le_bytes().to_vec()
        } else {
            Vec::new()
        };
        self.send(index, id::CONTROLLER_DATA, &ask)?;
        let d = self.answer(id::CONTROLLER_DATA, ANSWER)?;
        parse_controller(&d, index, self.version).map_err(|e| bad(&e))
    }

    /// Every device the server has.
    pub fn controllers(&mut self) -> io::Result<Vec<Controller>> {
        self.changed = false;
        let n = self.count()?.min(256);
        (0..n).map(|i| self.controller(i)).collect()
    }

    /// Device `index` in mode `mode` (as it described it).
    pub fn set_mode(&mut self, index: u32, mode: i32, raw: &[u8]) -> io::Result<()> {
        self.send(index, id::UPDATE_MODE, &update_mode(mode, raw))
    }

    /// Every LED of device `index` in `colour`.
    pub fn set_colour(&mut self, index: u32, leds: usize, colour: Rgb8) -> io::Result<()> {
        self.send(index, id::UPDATE_LEDS, &update_leds(&vec![colour; leds]))
    }

    /// Whether the server said its devices changed since they were last read. Reads what it sent
    /// meanwhile without waiting; a closed connection is an error.
    pub fn devices_changed(&mut self) -> io::Result<bool> {
        self.stream.set_nonblocking(true)?;
        let mut h = [0u8; 16];
        let peeked = self.stream.peek(&mut h);
        self.stream.set_nonblocking(false)?;
        match peeked {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "OpenRGB closed",
                ))
            }
            Ok(n) if n >= 16 => {
                let (h, _) = self.packet(ANSWER)?;
                if h.id == id::DEVICE_LIST_UPDATED {
                    self.changed = true;
                }
                return self.devices_changed();
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e),
        }
        Ok(self.changed)
    }
}

fn bad(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("OpenRGB: {what}"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::mpsc;

    /// A string as OpenRGB writes one.
    fn s(out: &mut Vec<u8>, v: &str) {
        out.extend_from_slice(&((v.len() + 1) as u16).to_le_bytes());
        out.extend_from_slice(v.as_bytes());
        out.push(0);
    }

    /// A mode's data under `version` (constructed).
    fn mode_data(version: u32, name: &str, flags: u32, color_mode: u32) -> Vec<u8> {
        let mut m = Vec::new();
        s(&mut m, name);
        if version < 6 {
            m.extend_from_slice(&7u32.to_le_bytes());
        }
        m.extend_from_slice(&flags.to_le_bytes());
        m.extend_from_slice(&[0; 8]);
        if version >= 3 {
            m.extend_from_slice(&[0; 8]);
        }
        m.extend_from_slice(&[0; 8]);
        m.extend_from_slice(&[0; 4]);
        if version >= 3 {
            m.extend_from_slice(&[0; 4]);
        }
        m.extend_from_slice(&[0; 4]);
        m.extend_from_slice(&color_mode.to_le_bytes());
        m.extend_from_slice(&1u16.to_le_bytes());
        m.extend_from_slice(&[1, 2, 3, 0]);
        m
    }

    /// A keyboard's description as OpenRGB gives it under `version` (constructed): two modes,
    /// Static first and Direct, one zone with a matrix and a segment, `leds` LEDs.
    pub(crate) fn keyboard(version: u32, leds: u16, location: &str) -> Vec<u8> {
        let mut d = vec![0; 4];
        d.extend_from_slice(&5i32.to_le_bytes());
        s(&mut d, "Constructed Keyboard");
        if version >= 1 {
            s(&mut d, "Constructed Co");
        }
        s(&mut d, "A keyboard");
        s(&mut d, "1.0");
        s(&mut d, "SERIAL-0001");
        s(&mut d, location);
        d.extend_from_slice(&2u16.to_le_bytes());
        d.extend_from_slice(&0i32.to_le_bytes());
        d.extend(mode_data(version, "Static", 1 << 6, 2));
        d.extend(mode_data(version, "Direct", MODE_PER_LED, COLORS_PER_LED));
        d.extend_from_slice(&1u16.to_le_bytes());
        s(&mut d, "Keys");
        d.extend_from_slice(&1i32.to_le_bytes());
        for v in [u32::from(leds); 3] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        let mut matrix = Vec::new();
        matrix.extend_from_slice(&1u32.to_le_bytes());
        matrix.extend_from_slice(&2u32.to_le_bytes());
        matrix.extend_from_slice(&[0; 8]);
        d.extend_from_slice(&(matrix.len() as u16).to_le_bytes());
        d.extend(matrix);
        if version >= 4 {
            d.extend_from_slice(&1u16.to_le_bytes());
            s(&mut d, "All");
            d.extend_from_slice(&0i32.to_le_bytes());
            d.extend_from_slice(&[0; 8]);
        }
        if version >= 5 {
            d.extend_from_slice(&0u32.to_le_bytes());
        }
        d.extend_from_slice(&leds.to_le_bytes());
        for i in 0..leds {
            s(&mut d, &format!("Key {i}"));
            if version < 6 {
                d.extend_from_slice(&u32::from(i).to_le_bytes());
            }
        }
        d.extend_from_slice(&leds.to_le_bytes());
        d.extend(std::iter::repeat_n(0u8, usize::from(leds) * 4));
        let n = d.len() as u32;
        d[..4].copy_from_slice(&n.to_le_bytes());
        d
    }

    #[test]
    fn reads_a_device_under_every_protocol_it_speaks() {
        for version in 0..=VERSION {
            let d = keyboard(version, 3, "HID: \\\\?\\hid#vid_1234");
            let c = parse_controller(&d, 2, version).unwrap();
            assert_eq!(c.name, "Constructed Keyboard", "v{version}");
            assert_eq!(c.vendor, if version >= 1 { "Constructed Co" } else { "" });
            assert_eq!((c.kind, c.index, c.leds, c.active_mode), (5, 2, 3, 0));
            assert_eq!(c.modes.len(), 2);
            assert_eq!(c.direct_mode(), Some(1), "v{version}");
            // A mode's data goes back exactly as it came.
            assert_eq!(
                c.modes[1].raw,
                mode_data(version, "Direct", MODE_PER_LED, 1)
            );
        }
    }

    #[test]
    fn refuses_a_description_cut_short_or_too_large() {
        let d = keyboard(VERSION, 3, "USB");
        for cut in [0, 10, 40, d.len() / 2, d.len() - 1] {
            assert!(
                parse_controller(&d[..cut], 0, VERSION).is_err(),
                "cut at {cut}"
            );
        }
        // A count no device has.
        let mut huge = d.clone();
        let modes_at = huge
            .windows(2)
            .position(|w| w == 2u16.to_le_bytes())
            .unwrap();
        huge[modes_at..modes_at + 2].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(parse_controller(&huge, 0, VERSION).is_err());
        assert!(parse_header(b"XRGB\0\0\0\0\0\0\0\0\0\0\0\0").is_none());
    }

    #[test]
    fn says_each_colour_and_mode_as_openrgb_reads_them() {
        let d = update_leds(&[Rgb8 { r: 1, g: 2, b: 3 }; 2]);
        assert_eq!(d, [14, 0, 0, 0, 2, 0, 1, 2, 3, 0, 1, 2, 3, 0]);
        assert_eq!(update_mode(1, &[9, 9]), [10, 0, 0, 0, 1, 0, 0, 0, 9, 9]);
        let h = header(3, id::UPDATE_LEDS, 14);
        assert_eq!(
            parse_header(&h),
            Some(Header {
                device: 3,
                id: 1050,
                size: 14
            })
        );
        assert_eq!(&h[..4], b"ORGB");
    }

    /// What the stand-in server was sent: each packet's header and data.
    type Got = Vec<(Header, Vec<u8>)>;

    /// A stand-in OpenRGB on protocol `server`, with `devices` keyboards. Answers counts,
    /// descriptions and its version; tells of changed devices once, after the first colour.
    pub(crate) fn server(server: u32, devices: u32) -> (SocketAddr, mpsc::Receiver<Got>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut got = Got::new();
            let mut told = false;
            loop {
                let mut h = [0u8; 16];
                if s.read_exact(&mut h).is_err() {
                    break;
                }
                let h = parse_header(&h).unwrap();
                let mut d = vec![0u8; h.size as usize];
                s.read_exact(&mut d).unwrap();
                let reply = |s: &mut TcpStream, dev: u32, id: u32, data: &[u8]| {
                    let mut p = header(dev, id, data.len() as u32).to_vec();
                    p.extend_from_slice(data);
                    s.write_all(&p).unwrap();
                };
                match h.id {
                    id::PROTOCOL_VERSION if server > 0 => {
                        reply(&mut s, 0, id::PROTOCOL_VERSION, &server.to_le_bytes())
                    }
                    id::CONTROLLER_COUNT => reply(&mut s, 0, 0, &devices.to_le_bytes()),
                    id::CONTROLLER_DATA => {
                        let v = d
                            .get(..4)
                            .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
                        let leds = 2 + h.device as u16;
                        reply(&mut s, h.device, 1, &keyboard(v, leds, "USB: 1-2"));
                    }
                    id::UPDATE_LEDS if !told => {
                        told = true;
                        reply(&mut s, 0, id::DEVICE_LIST_UPDATED, &[]);
                    }
                    _ => {}
                }
                got.push((h, d));
            }
            let _ = tx.send(got);
        });
        (addr, rx)
    }

    #[test]
    fn talks_to_openrgb_in_the_protocol_both_speak() {
        for (server_version, agreed) in [(5, 5), (6, 5), (3, 3), (0, 0)] {
            let (addr, rx) = server(server_version, 2);
            let mut c = Client::connect(addr).unwrap();
            assert_eq!(c.version, agreed, "server {server_version}");
            let devices = c.controllers().unwrap();
            assert_eq!(devices.len(), 2);
            assert_eq!(devices[1].leds, 3);
            let direct = devices[0].direct_mode().unwrap();
            c.set_mode(0, direct as i32, &devices[0].modes[direct].raw)
                .unwrap();
            c.set_colour(0, devices[0].leds, Rgb8 { r: 255, g: 0, b: 7 })
                .unwrap();
            // The server's word that its devices changed, read when it comes.
            let mut changed = false;
            for _ in 0..50 {
                changed |= c.devices_changed().unwrap();
                if changed {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(changed);
            assert_eq!(c.controllers().unwrap().len(), 2);
            assert!(!c.devices_changed().unwrap());
            drop(c);
            let got = rx.recv().unwrap();
            assert_eq!(got[0].0.id, id::CLIENT_NAME);
            assert_eq!(got[0].1, b"Clokrix\0");
            assert!(got.iter().all(|(h, _)| h.id != id::SAVE_MODE));
            let leds = got.iter().find(|(h, _)| h.id == id::UPDATE_LEDS).unwrap();
            assert_eq!(leds.1, update_leds(&[Rgb8 { r: 255, g: 0, b: 7 }; 2]));
            let mode = got.iter().find(|(h, _)| h.id == id::UPDATE_MODE).unwrap();
            assert_eq!(mode.1, update_mode(1, &devices[0].modes[1].raw));
            // Descriptions asked in the agreed protocol (none named to a server on protocol 0).
            let asked = got
                .iter()
                .find(|(h, _)| h.id == id::CONTROLLER_DATA)
                .unwrap();
            let want = if agreed > 0 {
                agreed.to_le_bytes().to_vec()
            } else {
                vec![]
            };
            assert_eq!(asked.1, want);
        }
    }

    #[test]
    fn an_openrgb_that_closes_or_is_not_there_says_so() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let t = std::thread::spawn(move || {
            // Takes the connection, says nothing (as protocol 0), then goes.
            let (s, _) = l.accept().unwrap();
            std::thread::sleep(Duration::from_millis(1200));
            drop(s);
        });
        let mut c = Client::connect(addr).unwrap();
        assert_eq!(c.version, 0);
        t.join().unwrap();
        let mut closed = false;
        for _ in 0..50 {
            if c.devices_changed().is_err() {
                closed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(closed);
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let gone = l.local_addr().unwrap();
        drop(l);
        assert!(Client::connect(gone).is_err());
    }
}
