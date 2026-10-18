//! A STUN binding request and the address in its answer (RFC 5389), enough to learn the public
//! address a UDP socket is seen from: the phone may then reach the PC at it directly.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

const BINDING_REQUEST: u16 = 0x0001;
const BINDING_SUCCESS: u16 = 0x0101;
const MAGIC: u32 = 0x2112_A442;
const MAPPED_ADDRESS: u16 = 0x0001;
const XOR_MAPPED_ADDRESS: u16 = 0x0020;

/// A binding request with this transaction id.
pub fn request(txid: [u8; 12]) -> [u8; 20] {
    let mut m = [0u8; 20];
    m[..2].copy_from_slice(&BINDING_REQUEST.to_be_bytes());
    // Length 0: no attributes.
    m[4..8].copy_from_slice(&MAGIC.to_be_bytes());
    m[8..].copy_from_slice(&txid);
    m
}

/// The address a binding success for `txid` reports; `None` for anything else.
pub fn mapped_address(msg: &[u8], txid: [u8; 12]) -> Option<SocketAddr> {
    if msg.len() < 20
        || u16::from_be_bytes([msg[0], msg[1]]) != BINDING_SUCCESS
        || u32::from_be_bytes([msg[4], msg[5], msg[6], msg[7]]) != MAGIC
        || msg[8..20] != txid
    {
        return None;
    }
    let len = usize::from(u16::from_be_bytes([msg[2], msg[3]]));
    let body = msg.get(20..20 + len)?;
    let mut plain = None;
    let mut at = 0;
    while at + 4 <= body.len() {
        let kind = u16::from_be_bytes([body[at], body[at + 1]]);
        let n = usize::from(u16::from_be_bytes([body[at + 2], body[at + 3]]));
        let value = body.get(at + 4..at + 4 + n)?;
        match kind {
            XOR_MAPPED_ADDRESS => return address(value, Some(&msg[4..20])),
            MAPPED_ADDRESS => plain = address(value, None),
            _ => {}
        }
        at += 4 + n.div_ceil(4) * 4;
    }
    plain
}

/// An address attribute; `xor` is the magic cookie and transaction id when XOR-ed.
fn address(v: &[u8], xor: Option<&[u8]>) -> Option<SocketAddr> {
    if v.len() < 4 {
        return None;
    }
    let mask = |i: usize| xor.map_or(0, |x| x[i]);
    let port = u16::from_be_bytes([v[2] ^ mask(0), v[3] ^ mask(1)]);
    let ip = match v[1] {
        0x01 if v.len() >= 8 => {
            let b: [u8; 4] = std::array::from_fn(|i| v[4 + i] ^ mask(i));
            IpAddr::V4(Ipv4Addr::from(b))
        }
        0x02 if v.len() >= 20 => {
            let b: [u8; 16] = std::array::from_fn(|i| v[4 + i] ^ mask(i));
            IpAddr::V6(Ipv6Addr::from(b))
        }
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}
