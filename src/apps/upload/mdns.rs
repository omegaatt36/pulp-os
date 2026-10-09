// mDNS responder for `pulp.local`: query parsing, the fixed A answer, the IP
// label and the serve loop. Radio-free (only `core`) so it is exercised on the
// host; the firmware implements `Datagrams` over its UDP socket.

use core::convert::Infallible;

pub const PORT: u16 = 5353;
pub const GROUP: [u8; 4] = [224, 0, 0, 251];
pub const RESPONSE_LEN: usize = 38;
// "(255.255.255.255)"
pub const IP_LABEL_MAX: usize = 17;

// Largest datagram `serve` reads. The firmware sizes the socket receive buffer
// to this, so a datagram that does not fit is dropped by the stack before
// `recv` can report it truncated.
pub const RECV_BUF_LEN: usize = 512;

const HEADER_LEN: usize = 12;
const NAME_MAX: usize = 255;
const HOST: [&[u8]; 2] = [b"pulp", b"local"];

const TYPE_A: u16 = 1;
const TYPE_ANY: u16 = 255;
const CLASS_IN: u16 = 1;
const CLASS_MASK: u16 = 0x7FFF; // top bit: unicast-response request

// QR bit and opcode of the flags word; a query has both zero.
const FLAGS_QR_OPCODE: u16 = 0xF800;

// ID 0, flags 0x8400 (response, authoritative), one answer, `pulp.local` A IN
// with the cache-flush bit, TTL 120, RDLENGTH 4, RDATA patched in.
const RESPONSE: [u8; RESPONSE_LEN] = [
    0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0, //
    4, b'p', b'u', b'l', b'p', 5, b'l', b'o', b'c', b'a', b'l', 0, //
    0, 1, 0x80, 1, 0, 0, 0, 120, 0, 4, //
    0, 0, 0, 0,
];

fn be16(pkt: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*pkt.get(at)?, *pkt.get(at + 1)?]))
}

// Reads the name at `start`. Returns the offset just past it in the stream and
// whether it spells `pulp.local` (ASCII case-insensitive). A compression
// pointer must lead strictly before the run of labels it ends and not into the
// header, so every jump moves backwards and the walk terminates. Reserved label
// types, truncation and names over 255 bytes are malformed (None).
fn read_name(pkt: &[u8], start: usize) -> Option<(usize, bool)> {
    let mut pos = start;
    let mut run_start = start;
    let mut end = None;
    let mut wire_len = 1; // the terminating zero
    let mut labels = 0;
    let mut matches = true;
    loop {
        let len = *pkt.get(pos)?;
        match len & 0xC0 {
            0x00 => {
                pos += 1;
                if len == 0 {
                    break;
                }
                let len = usize::from(len);
                let label = pkt.get(pos..pos + len)?;
                wire_len += len + 1;
                if wire_len > NAME_MAX {
                    return None;
                }
                matches &= HOST
                    .get(labels)
                    .is_some_and(|h| h.eq_ignore_ascii_case(label));
                labels += 1;
                pos += len;
            }
            0xC0 => {
                let low = *pkt.get(pos + 1)?;
                let target = usize::from(len & 0x3F) << 8 | usize::from(low);
                if target < HEADER_LEN || target >= run_start {
                    return None;
                }
                end.get_or_insert(pos + 2);
                run_start = target;
                pos = target;
            }
            _ => return None,
        }
    }
    Some((end.unwrap_or(pos), matches && labels == HOST.len()))
}

// Every declared question is parsed; the packet is answered only when the whole
// question section is well-formed and one question asks for `pulp.local` A / ANY.
fn is_query_for_host(pkt: &[u8]) -> Option<bool> {
    if be16(pkt, 2)? & FLAGS_QR_OPCODE != 0 {
        return Some(false);
    }
    let questions = be16(pkt, 4)?;
    let mut pos = HEADER_LEN;
    let mut hit = false;
    for _ in 0..questions {
        let (after, is_host) = read_name(pkt, pos)?;
        let qtype = be16(pkt, after)?;
        let qclass = be16(pkt, after + 2)?;
        hit |= is_host && (qtype == TYPE_A || qtype == TYPE_ANY) && qclass & CLASS_MASK == CLASS_IN;
        pos = after + 4;
    }
    Some(hit)
}

/// Writes the answer into `out` and returns true when `pkt` is an mDNS query
/// for `pulp.local`; otherwise returns false and leaves `out` unspecified.
pub fn handle_packet(pkt: &[u8], ip: [u8; 4], out: &mut [u8; RESPONSE_LEN]) -> bool {
    if is_query_for_host(pkt) != Some(true) {
        return false;
    }
    *out = RESPONSE;
    out[RESPONSE_LEN - 4..].copy_from_slice(&ip);
    true
}

/// "(a.b.c.d)", decimal without leading zeros.
pub fn ip_label(ip: [u8; 4], buf: &mut [u8; IP_LABEL_MAX]) -> &str {
    let mut n = 0;
    let mut put = |b: u8| {
        buf[n] = b;
        n += 1;
    };
    put(b'(');
    for (i, octet) in ip.into_iter().enumerate() {
        if i > 0 {
            put(b'.');
        }
        if octet >= 100 {
            put(b'0' + octet / 100);
        }
        if octet >= 10 {
            put(b'0' + octet / 10 % 10);
        }
        put(b'0' + octet % 10);
    }
    put(b')');
    core::str::from_utf8(&buf[..n]).unwrap_or("")
}

/// The datagram socket seen by `serve`.
#[allow(async_fn_in_trait)]
pub trait Datagrams {
    type Error;
    /// Receives the next datagram into `buf` and returns its length.
    async fn recv(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error>;
    /// Sends `data` to the mDNS multicast group (`GROUP`:`PORT`).
    async fn send(&mut self, data: &[u8]) -> Result<(), Self::Error>;
}

/// Answers every query for `pulp.local`. Other datagrams are ignored and a
/// failed send is skipped; only a receive error ends it.
pub async fn serve<D: Datagrams>(socket: &mut D, ip: [u8; 4]) -> Result<Infallible, D::Error> {
    let mut buf = [0u8; RECV_BUF_LEN];
    let mut reply = [0u8; RESPONSE_LEN];
    loop {
        let n = socket.recv(&mut buf).await?;
        if handle_packet(&buf[..n], ip, &mut reply) {
            let _ = socket.send(&reply).await;
        }
    }
}
