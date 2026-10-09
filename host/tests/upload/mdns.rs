// mDNS responder of the upload app: `pulp.local` A / ANY queries, IP
// label, and the `serve` loop over a fake datagram socket.
//
// Expected values come only from:
//   service contract: connected upload mode answers pulp.local queries
//   RFC 1035  section 4.1 (message layout), 4.1.2 (question), 4.1.3 (RR),
//             4.1.4 (name compression)
//   RFC 6762  mDNS: 5.4 (QU bit), 6 / 10 / 10.2 (answer, TTL, cache-flush),
//             7.1 (known answers), 16 (case-insensitive names), 18 (header
//             field values)
// No expected value is copied from an implementation output. Responses are
// checked by the independent `parse_response` below, written from the RFC 1035
// field layout.
//
// Packets built here are plain Vec<u8> assembled byte by byte. Every loop that
// could hang on a regression (pointer loops, fuzzing, serve) runs on a worker
// thread under a wall-clock guard, so a regression fails instead of hanging.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration as StdDuration;

use embassy_futures::block_on;
use embassy_futures::select::{Either, select};
use embassy_futures::yield_now;
use pulp_host::apps::upload_mdns::{
    Datagrams, GROUP, IP_LABEL_MAX, PORT, RESPONSE_LEN, handle_packet, ip_label, serve,
};

const GUARD: StdDuration = StdDuration::from_secs(60);

const IP: [u8; 4] = [192, 168, 1, 23];

// RFC 1035 3.2.2 / 3.2.4 / 3.2.1
const TYPE_A: u16 = 1;
const TYPE_NS: u16 = 2;
const TYPE_CNAME: u16 = 5;
const TYPE_PTR: u16 = 12;
const TYPE_MX: u16 = 15;
const TYPE_TXT: u16 = 16;
const TYPE_AAAA: u16 = 28; // RFC 3596
const TYPE_SRV: u16 = 33; // RFC 2782
const TYPE_OPT: u16 = 41; // RFC 6891
const TYPE_ANY: u16 = 255;
const CLASS_IN: u16 = 1;
const CLASS_CH: u16 = 3;
const CLASS_ANY: u16 = 255;
const QU: u16 = 0x8000; // RFC 6762 5.4

// ------------------------------------------------------------- guard

fn bounded<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(GUARD) {
        Ok(v) => {
            handle.join().unwrap();
            v
        }
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("exceeded the {GUARD:?} wall-clock guard"),
        // sender dropped without a value: the worker panicked; re-raise it
        Err(mpsc::RecvTimeoutError::Disconnected) => match handle.join() {
            Err(p) => std::panic::resume_unwind(p),
            Ok(()) => panic!("worker ended without a result"),
        },
    }
}

// ------------------------------------------------------------- packet building

// RFC 1035 4.1.2: QNAME = labels, each length-prefixed, ended by a zero octet.
// An empty string is the root name (a single zero octet).
fn name(dotted: &str) -> Vec<u8> {
    let mut v = Vec::new();
    if !dotted.is_empty() {
        for label in dotted.split('.') {
            v.push(label.len() as u8);
            v.extend_from_slice(label.as_bytes());
        }
    }
    v.push(0);
    v
}

fn question(qname: &[u8], qtype: u16, qclass: u16) -> Vec<u8> {
    let mut v = qname.to_vec();
    v.extend_from_slice(&qtype.to_be_bytes());
    v.extend_from_slice(&qclass.to_be_bytes());
    v
}

// RFC 1035 4.1.1 header, counts = [QD, AN, NS, AR].
fn header(id: u16, flags: u16, counts: [u16; 4]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&id.to_be_bytes());
    v.extend_from_slice(&flags.to_be_bytes());
    for c in counts {
        v.extend_from_slice(&c.to_be_bytes());
    }
    v
}

fn packet(id: u16, flags: u16, counts: [u16; 4], body: &[u8]) -> Vec<u8> {
    let mut v = header(id, flags, counts);
    v.extend_from_slice(body);
    v
}

fn query(dotted: &str, qtype: u16, qclass: u16) -> Vec<u8> {
    packet(0, 0, [1, 0, 0, 0], &question(&name(dotted), qtype, qclass))
}

fn std_query() -> Vec<u8> {
    query("pulp.local", TYPE_A, CLASS_IN)
}

// ------------------------------------------------------------- response parser

// Independent RFC 1035 4.1 parser for the single answer RR the responder
// emits. Compression pointers are rejected: the contract says the answer NAME
// is not compressed.
#[derive(Debug)]
struct Parsed {
    id: u16,
    flags: u16,
    counts: [u16; 4],
    labels: Vec<Vec<u8>>,
    rtype: u16,
    class: u16,
    ttl: u32,
    rdlength: u16,
    rdata: Vec<u8>,
    consumed: usize,
}

fn parse_response(msg: &[u8]) -> Parsed {
    assert!(msg.len() >= 12, "shorter than a header: {}", msg.len());
    let u16at = |o: usize| u16::from_be_bytes([msg[o], msg[o + 1]]);
    let mut off = 12;
    let mut labels = Vec::new();
    loop {
        let l = msg[off] as usize;
        off += 1;
        assert_eq!(l & 0xC0, 0, "compressed or reserved label in answer name");
        if l == 0 {
            break;
        }
        labels.push(msg[off..off + l].to_vec());
        off += l;
    }
    let rtype = u16at(off);
    let class = u16at(off + 2);
    let ttl = u32::from_be_bytes([msg[off + 4], msg[off + 5], msg[off + 6], msg[off + 7]]);
    let rdlength = u16at(off + 8);
    off += 10;
    let rdata = msg[off..off + rdlength as usize].to_vec();
    off += rdlength as usize;
    Parsed {
        id: u16at(0),
        flags: u16at(2),
        counts: [u16at(4), u16at(6), u16at(8), u16at(10)],
        labels,
        rtype,
        class,
        ttl,
        rdlength,
        rdata,
        consumed: off,
    }
}

// Every field of a response, each against its own requirement.
fn check_response(resp: &[u8], ip: [u8; 4]) {
    // contract: 38 bytes
    assert_eq!(resp.len(), RESPONSE_LEN, "response length");
    let p = parse_response(resp);
    // RFC 6762 18.1: response ID is zero
    assert_eq!(p.id, 0, "ID");
    // RFC 6762 18.2 QR=1, 18.3 opcode 0, 18.4 AA=1, 18.5 TC=0, 18.6 RD=0,
    // 18.7 RA=0, 18.8 Z=0, 18.11 rcode 0 => 0x8400 exactly
    assert_eq!(p.flags & 0x8000, 0x8000, "QR bit");
    assert_eq!(p.flags & 0x7800, 0, "opcode");
    assert_eq!(p.flags & 0x0400, 0x0400, "AA bit");
    assert_eq!(p.flags & 0x0200, 0, "TC bit");
    assert_eq!(p.flags & 0x0100, 0, "RD bit");
    assert_eq!(p.flags & 0x0080, 0, "RA bit");
    assert_eq!(p.flags & 0x0070, 0, "Z bits");
    assert_eq!(p.flags & 0x000F, 0, "rcode");
    assert_eq!(p.flags, 0x8400, "flags");
    // RFC 6762 6: a response carries no questions, one answer, nothing else
    assert_eq!(p.counts, [0, 1, 0, 0], "QD/AN/NS/AR counts");
    // contract: NAME = pulp.local, written out uncompressed
    assert_eq!(
        p.labels,
        vec![b"pulp".to_vec(), b"local".to_vec()],
        "name labels"
    );
    // RFC 1035 3.2.2: TYPE A
    assert_eq!(p.rtype, TYPE_A, "type");
    // RFC 6762 10.2: cache-flush bit set; RFC 1035 3.2.4: class IN
    assert_eq!(p.class & 0x8000, 0x8000, "cache-flush bit");
    assert_eq!(p.class & 0x7FFF, CLASS_IN, "class IN");
    // RFC 6762 10: host-name records use a 120 s TTL
    assert_eq!(p.ttl, 120, "TTL");
    // RFC 1035 3.4.1: an A record is 4 octets
    assert_eq!(p.rdlength, 4, "RDLENGTH");
    assert_eq!(p.rdata, ip.to_vec(), "RDATA");
    assert_eq!(p.consumed, resp.len(), "no bytes beyond the answer");
}

// ------------------------------------------------------------- call helpers

// Run handle_packet with the out buffer pre-filled two different ways: the
// reply must not depend on stale bytes (every byte of a reply is written).
fn run(pkt: &[u8], ip: [u8; 4]) -> Option<[u8; RESPONSE_LEN]> {
    let mut a = [0xA5u8; RESPONSE_LEN];
    let mut b = [0x5Au8; RESPONSE_LEN];
    let ra = handle_packet(pkt, ip, &mut a);
    let rb = handle_packet(pkt, ip, &mut b);
    assert_eq!(ra, rb, "result depends on out buffer contents");
    if ra {
        assert_eq!(a, b, "response depends on out buffer contents");
        Some(a)
    } else {
        None
    }
}

#[track_caller]
fn assert_reply(pkt: &[u8], why: &str) {
    match run(pkt, IP) {
        Some(resp) => check_response(&resp, IP),
        None => panic!("expected a reply: {why}\npacket: {pkt:02x?}"),
    }
}

#[track_caller]
fn assert_silent(pkt: &[u8], why: &str) {
    if run(pkt, IP).is_some() {
        panic!("expected no reply: {why}\npacket: {pkt:02x?}");
    }
}

// ============================================================ A. handle_packet

#[test]
fn constants_match_the_contract() {
    // RFC 6762 3: UDP port 5353; RFC 5771 / RFC 6762 3: group 224.0.0.251
    assert_eq!(PORT, 5353);
    assert_eq!(GROUP, [224, 0, 0, 251]);
    // contract
    assert_eq!(RESPONSE_LEN, 38);
    assert_eq!(IP_LABEL_MAX, 17);
}

// ----------------------------------------------------------------
#[test]
fn standard_a_query_gets_a_fully_checked_reply_for_many_ips() {
    // contract answer format; RFC 6762 6 / 10 / 18
    let ips: [[u8; 4]; 9] = [
        [192, 168, 1, 23],
        [0, 0, 0, 0],
        [255, 255, 255, 255],
        [10, 0, 0, 1],
        [1, 2, 3, 4],
        [127, 0, 0, 1],
        [172, 16, 254, 3],
        [0, 255, 0, 255],
        [224, 0, 0, 251],
    ];
    let q = std_query();
    for ip in ips {
        let resp = run(&q, ip).unwrap_or_else(|| panic!("no reply for ip {ip:?}"));
        check_response(&resp, ip);
    }
}

#[test]
fn reply_matches_the_query_layout_assumed_by_the_parser() {
    // Sanity of the test's own builder: header 12 + name 12 + type/class 4.
    let q = std_query();
    assert_eq!(q.len(), 12 + 12 + 4);
    assert_eq!(&q[12..24], b"\x04pulp\x05local\x00");
}

#[test]
fn nonzero_query_id_and_benign_flag_bits_still_match_but_reply_id_is_zero() {
    // contract rules reject only QR=1 and opcode != 0, so ID / RD / TC in the
    // query do not matter; RFC 6762 18.1: the multicast reply ID is zero.
    // RFC 6762 7.2: TC in a query means more known answers follow.
    let body = question(&name("pulp.local"), TYPE_A, CLASS_IN);
    for id in [0u16, 1, 0xBEEF, 0xFFFF] {
        for flags in [0x0000u16, 0x0100, 0x0200, 0x0300] {
            let pkt = packet(id, flags, [1, 0, 0, 0], &body);
            assert_reply(&pkt, &format!("id {id:#x} flags {flags:#x}"));
        }
    }
}

// ----------------------------------------------------------------
#[test]
fn name_labels_compare_case_insensitively_exhaustively() {
    // RFC 6762 16 / RFC 1035 2.3.3: all 2^9 casings of "pulp.local" match
    let letters: Vec<usize> = "pulp.local"
        .char_indices()
        .filter(|(_, c)| c.is_ascii_alphabetic())
        .map(|(i, _)| i)
        .collect();
    assert_eq!(letters.len(), 9);
    for mask in 0u32..(1 << letters.len()) {
        let mut s = String::from("pulp.local");
        for (bit, &idx) in letters.iter().enumerate() {
            if mask & (1 << bit) != 0 {
                let up = s[idx..idx + 1].to_ascii_uppercase();
                s.replace_range(idx..idx + 1, &up);
            }
        }
        assert_reply(&query(&s, TYPE_A, CLASS_IN), &format!("casing {s}"));
    }
}

#[test]
fn named_casings_match() {
    for s in [
        "PULP.LOCAL",
        "Pulp.Local",
        "pUlP.lOcAl",
        "pulp.LOCAL",
        "PULP.local",
    ] {
        assert_reply(&query(s, TYPE_A, CLASS_IN), s);
    }
}

#[test]
fn reply_name_is_lowercase_whatever_the_query_case() {
    // contract: NAME = \x04pulp\x05local\x00 (checked by check_response)
    let resp = run(&query("PuLp.LoCaL", TYPE_A, CLASS_IN), IP).expect("reply");
    check_response(&resp, IP);
}

#[test]
fn qu_bit_class_matches() {
    // RFC 6762 5.4: the top bit of qclass is the unicast-response bit
    assert_reply(&query("pulp.local", TYPE_A, QU | CLASS_IN), "QU A");
    assert_reply(&query("pulp.local", TYPE_ANY, QU | CLASS_IN), "QU ANY");
    assert_reply(&query("pulp.local", TYPE_A, 0x8001), "class 0x8001");
}

#[test]
fn any_type_matches() {
    // contract: qtype 1 or 255
    assert_reply(&query("pulp.local", TYPE_ANY, CLASS_IN), "ANY");
}

#[test]
fn other_qtypes_do_not_match() {
    for t in [
        0u16, TYPE_NS, TYPE_CNAME, TYPE_PTR, TYPE_MX, TYPE_TXT, TYPE_AAAA, TYPE_SRV, TYPE_OPT, 254,
        256, 0x0101, 0x8001, 0xFFFF,
    ] {
        assert_silent(&query("pulp.local", t, CLASS_IN), &format!("qtype {t}"));
    }
}

#[test]
fn other_qclasses_do_not_match() {
    // contract: qclass minus the QU bit must be IN (1)
    for c in [
        0u16,
        CLASS_CH,
        2,
        4,
        CLASS_ANY,
        0x0100,
        0x0101,
        0x7FFF,
        QU,
        QU | CLASS_CH,
        QU | CLASS_ANY,
        QU | 0x0100,
        0xFFFF,
    ] {
        assert_silent(&query("pulp.local", TYPE_A, c), &format!("qclass {c:#x}"));
        assert_silent(
            &query("pulp.local", TYPE_ANY, c),
            &format!("ANY qclass {c:#x}"),
        );
    }
}

// ----------------------------------------------------------------
#[test]
fn non_matching_names_do_not_match() {
    for n in [
        "other.local",
        "xpulp.local",
        "pulp.localx",
        "pulp.local.evil",
        "a.pulp.local",
        "pulp.lan",
        "pulp",
        "local",
        "local.pulp",
        "pulp.pulp",
        "pulpp.local",
        "pul.local",
        "pulp.loca",
        "pulp.locall",
        "",
    ] {
        assert_silent(&query(n, TYPE_A, CLASS_IN), &format!("name {n:?}"));
        assert_silent(&query(n, TYPE_ANY, CLASS_IN), &format!("ANY name {n:?}"));
    }
}

#[test]
fn bytes_differing_only_by_high_bit_do_not_match() {
    // Case folding is ASCII only (RFC 6762 16): 'p'|0x80 and friends are not
    // letters of pulp.local.
    let good = b"\x04pulp\x05local\x00".to_vec();
    for i in 0..good.len() {
        for flip in [0x80u8, 0x20 | 0x80] {
            let mut bad = good.clone();
            bad[i] ^= flip;
            // changing a length octet changes the structure; either way it is
            // not pulp.local
            let pkt = packet(0, 0, [1, 0, 0, 0], &question(&bad, TYPE_A, CLASS_IN));
            assert_silent(&pkt, &format!("byte {i} xor {flip:#x}"));
        }
    }
}

#[test]
fn single_byte_substitutions_in_the_name_do_not_match() {
    // every letter replaced by every other byte value that is not its own
    // ASCII case-fold
    let good = b"\x04pulp\x05local\x00".to_vec();
    for i in 0..good.len() {
        if good[i] < 6 {
            continue; // length octets: covered elsewhere
        }
        for v in 0u16..=255 {
            let v = v as u8;
            if v.eq_ignore_ascii_case(&good[i]) {
                continue;
            }
            let mut bad = good.clone();
            bad[i] = v;
            let pkt = packet(0, 0, [1, 0, 0, 0], &question(&bad, TYPE_A, CLASS_IN));
            assert_silent(&pkt, &format!("byte {i} = {v:#x}"));
        }
    }
}

#[test]
fn over_long_names_do_not_match() {
    // RFC 1035 2.3.4: labels <= 63, names <= 255; such names are not pulp.local
    let label63 = "a".repeat(63);
    let long = [label63.as_str(); 5].join(".");
    assert_silent(&query(&long, TYPE_A, CLASS_IN), "5 x 63-byte labels");
    let pulp_tail = format!("{long}.pulp.local");
    assert_silent(
        &query(&pulp_tail, TYPE_A, CLASS_IN),
        "long prefix before pulp.local",
    );
}

#[test]
fn reserved_label_types_do_not_match() {
    // RFC 1035 4.1.4: only 00 (length) and 11 (pointer) label types are
    // defined; 01 and 10 are reserved.
    // The reserved bits are set on an otherwise correct length octet, so a
    // reader that masks them off would see pulp.local.
    for l1 in [0x04u8, 0x44, 0x84] {
        for l2 in [0x05u8, 0x45, 0x85] {
            if (l1, l2) == (0x04, 0x05) {
                continue;
            }
            let mut qname = vec![l1];
            qname.extend_from_slice(b"pulp");
            qname.push(l2);
            qname.extend_from_slice(b"local\x00");
            let pkt = packet(0, 0, [1, 0, 0, 0], &question(&qname, TYPE_A, CLASS_IN));
            assert_silent(&pkt, &format!("length octets {l1:#x} / {l2:#x}"));
        }
    }
    for lead in [0x40u8, 0x41, 0x7F, 0x80, 0x81, 0xBF] {
        let mut qname = vec![lead];
        qname.extend_from_slice(b"pulp\x05local\x00");
        let pkt = packet(0, 0, [1, 0, 0, 0], &question(&qname, TYPE_A, CLASS_IN));
        assert_silent(&pkt, &format!("label type byte {lead:#x}"));
    }
}

// ----------------------------------------------------------------
#[test]
fn responses_are_not_queries() {
    // RFC 6762 18.2: QR=1 is a response. Includes the exact bytes we emit.
    let body = question(&name("pulp.local"), TYPE_A, CLASS_IN);
    for flags in [0x8000u16, 0x8400, 0x8100, 0x8180, 0xFFFF, 0x8001] {
        assert_silent(
            &packet(0, flags, [1, 0, 0, 0], &body),
            &format!("flags {flags:#x}"),
        );
    }
    let own = run(&std_query(), IP).expect("reply");
    assert_silent(&own, "our own response looped back");
}

#[test]
fn nonzero_opcodes_are_not_queries() {
    // RFC 6762 18.3: opcode must be 0; 4 = NOTIFY, 5 = UPDATE (RFC 1996/2136)
    let body = question(&name("pulp.local"), TYPE_A, CLASS_IN);
    for opcode in 1u16..=15 {
        let flags = opcode << 11;
        assert_silent(
            &packet(0, flags, [1, 0, 0, 0], &body),
            &format!("opcode {opcode}"),
        );
    }
    assert_silent(&packet(0, 4 << 11, [1, 0, 0, 0], &body), "NOTIFY");
    assert_silent(&packet(0, 5 << 11, [1, 0, 0, 0], &body), "UPDATE");
}

#[test]
fn zero_qdcount_is_not_a_question() {
    // RFC 1035 4.1.1: QDCOUNT = number of questions; none => nothing to answer
    let body = question(&name("pulp.local"), TYPE_A, CLASS_IN);
    assert_silent(
        &packet(0, 0, [0, 0, 0, 0], &body),
        "QD 0 with trailing question bytes",
    );
    assert_silent(&packet(0, 0, [0, 0, 0, 0], &[]), "bare header");
    assert_silent(&packet(0, 0, [0, 1, 0, 1], &body), "QD 0 AN 1 AR 1");
}

// ----------------------------------------------------------------
#[test]
fn any_matching_question_wins_not_only_the_first() {
    // contract: any one of QDCOUNT questions matching is enough
    let mut body = question(&name("other.local"), TYPE_A, CLASS_IN);
    body.extend(question(&name("pulp.local"), TYPE_A, CLASS_IN));
    assert_reply(&packet(0, 0, [2, 0, 0, 0], &body), "[other A][pulp A]");

    let mut body = question(&name("pulp.local"), TYPE_A, CLASS_IN);
    body.extend(question(&name("other.local"), TYPE_A, CLASS_IN));
    assert_reply(&packet(0, 0, [2, 0, 0, 0], &body), "[pulp A][other A]");

    let mut body = question(&name("other.local"), TYPE_AAAA, CLASS_IN);
    body.extend(question(&name("another.local"), TYPE_A, CLASS_IN));
    body.extend(question(&name("PULP.LOCAL"), TYPE_ANY, QU | CLASS_IN));
    body.extend(question(&name("third.local"), TYPE_A, CLASS_IN));
    assert_reply(&packet(0, 0, [4, 0, 0, 0], &body), "match in third of four");
}

#[test]
fn second_question_may_use_a_compression_pointer_to_the_first() {
    // RFC 1035 4.1.4 / RFC 6762 18.14: [pulp.local AAAA][<ptr 0xC00C> A]
    let mut body = question(&name("pulp.local"), TYPE_AAAA, CLASS_IN);
    body.extend(question(&[0xC0, 0x0C], TYPE_A, CLASS_IN));
    let pkt = packet(0, 0, [2, 0, 0, 0], &body);
    assert_eq!(&pkt[12..14], &[4, b'p'][..]); // sanity: first label at offset 12
    assert_reply(&pkt, "AAAA then pointer A");

    // same pointer, but the second question has a non-matching type/class
    for (t, c) in [
        (TYPE_AAAA, CLASS_IN),
        (TYPE_A, CLASS_CH),
        (TYPE_TXT, CLASS_IN),
    ] {
        let mut body = question(&name("pulp.local"), TYPE_AAAA, CLASS_IN);
        body.extend(question(&[0xC0, 0x0C], t, c));
        assert_silent(
            &packet(0, 0, [2, 0, 0, 0], &body),
            &format!("ptr type {t} class {c}"),
        );
    }
}

#[test]
fn pointer_to_a_label_suffix_of_an_earlier_question_is_followed() {
    // RFC 1035 4.1.4: a name may be labels followed by a pointer.
    // q1 = other.local at offset 12; its "local" label starts at 12 + 6 = 18.
    let mut body = question(&name("other.local"), TYPE_AAAA, CLASS_IN);
    let mut qname = vec![4];
    qname.extend_from_slice(b"pulp");
    qname.extend_from_slice(&[0xC0, 18]);
    body.extend(question(&qname, TYPE_A, CLASS_IN));
    let pkt = packet(0, 0, [2, 0, 0, 0], &body);
    assert_eq!(&pkt[18..24], b"\x05local");
    assert_reply(&pkt, "pulp + ptr -> local");

    // pointing at a different suffix: "lan" instead of "local"
    let mut body = question(&name("other.lan"), TYPE_AAAA, CLASS_IN);
    let mut qname = vec![4];
    qname.extend_from_slice(b"pulp");
    qname.extend_from_slice(&[0xC0, 18]);
    body.extend(question(&qname, TYPE_A, CLASS_IN));
    let pkt = packet(0, 0, [2, 0, 0, 0], &body);
    assert_eq!(&pkt[18..22], b"\x03lan");
    assert_silent(&pkt, "pulp + ptr -> lan");

    // q1 = a.pulp.local: "pulp" starts at 12 + 2 = 14
    let mut body = question(&name("a.pulp.local"), TYPE_AAAA, CLASS_IN);
    body.extend(question(&[0xC0, 14], TYPE_A, CLASS_IN));
    let pkt = packet(0, 0, [2, 0, 0, 0], &body);
    assert_eq!(&pkt[14..19], b"\x04pulp");
    assert_reply(&pkt, "pointer into a.pulp.local at pulp");
    // the same pointer at offset 12 would be a.pulp.local: no match
    let mut body = question(&name("a.pulp.local"), TYPE_AAAA, CLASS_IN);
    body.extend(question(&[0xC0, 12], TYPE_A, CLASS_IN));
    assert_silent(
        &packet(0, 0, [2, 0, 0, 0], &body),
        "pointer to a.pulp.local",
    );
}

#[test]
fn pointer_to_a_pointer_is_followed() {
    // RFC 1035 4.1.4: a pointer target may itself be a pointer.
    // q1 pulp.local AAAA (12..28), q2 name = ptr 12 at offset 28, q3 name =
    // ptr 28 (a pointer) at offset 34.
    let mut body = question(&name("pulp.local"), TYPE_AAAA, CLASS_IN);
    body.extend(question(&[0xC0, 12], TYPE_AAAA, CLASS_IN));
    body.extend(question(&[0xC0, 28], TYPE_A, CLASS_IN));
    let pkt = packet(0, 0, [3, 0, 0, 0], &body);
    assert_eq!(&pkt[28..30], &[0xC0, 12]);
    assert_reply(&pkt, "two-hop pointer chain");
}

#[test]
fn only_aaaa_questions_do_not_match() {
    // all questions name pulp.local but none asks for A / ANY
    let mut body = question(&name("pulp.local"), TYPE_AAAA, CLASS_IN);
    body.extend(question(&[0xC0, 0x0C], TYPE_AAAA, CLASS_IN));
    body.extend(question(&name("pulp.local"), TYPE_TXT, CLASS_IN));
    assert_silent(&packet(0, 0, [3, 0, 0, 0], &body), "AAAA, AAAA, TXT");
}

#[test]
fn declared_question_count_beyond_the_actual_does_not_reply() {
    // contract: any truncation / out-of-bounds => no reply
    let one = question(&name("pulp.local"), TYPE_A, CLASS_IN);
    assert_silent(
        &packet(0, 0, [2, 0, 0, 0], &one),
        "QD 2, one question present",
    );
    assert_silent(
        &packet(0, 0, [3, 0, 0, 0], &one),
        "QD 3, one question present",
    );
    assert_silent(
        &packet(0, 0, [0xFFFF, 0, 0, 0], &one),
        "QD 65535, one question present",
    );
    let aaaa = question(&name("pulp.local"), TYPE_AAAA, CLASS_IN);
    assert_silent(
        &packet(0, 0, [2, 0, 0, 0], &aaaa),
        "QD 2, only an AAAA present",
    );

    // a matching question followed by a cut-off one
    let mut body = one.clone();
    body.extend_from_slice(&name("other.local")); // name, but no type/class
    assert_silent(
        &packet(0, 0, [2, 0, 0, 0], &body),
        "second question missing type/class",
    );
    let mut body = one.clone();
    body.extend_from_slice(b"\x05oth"); // label cut short
    assert_silent(
        &packet(0, 0, [2, 0, 0, 0], &body),
        "second question label truncated",
    );
}

#[test]
fn fewer_questions_declared_than_present_is_fine() {
    // extra bytes after the declared questions are not questions
    let mut body = question(&name("pulp.local"), TYPE_A, CLASS_IN);
    body.extend(question(&name("other.local"), TYPE_A, CLASS_IN));
    assert_reply(
        &packet(0, 0, [1, 0, 0, 0], &body),
        "QD 1, two present, first matches",
    );
    let mut body = question(&name("other.local"), TYPE_A, CLASS_IN);
    body.extend(question(&name("pulp.local"), TYPE_A, CLASS_IN));
    assert_silent(
        &packet(0, 0, [1, 0, 0, 0], &body),
        "QD 1, two present, only second matches",
    );
}

// ----------------------------------------------------------------
#[test]
fn self_pointing_name_is_malformed_and_terminates() {
    // contract: loop => malformed => no reply, no hang
    bounded(|| {
        let mut body = question(&[0xC0, 0x0C], TYPE_A, CLASS_IN); // name at 12 -> 12
        body.extend_from_slice(&name("pulp.local"));
        assert_silent(&packet(0, 0, [1, 0, 0, 0], &body), "self pointer");
        assert_silent(
            &packet(
                0,
                0,
                [1, 0, 0, 0],
                &question(&[0xC0, 0x0C], TYPE_A, CLASS_IN),
            ),
            "self pointer, bare",
        );
    });
}

#[test]
fn forward_pointers_are_malformed() {
    // contract: a pointer must go strictly to an earlier position.
    // q1 name = ptr -> t, with a real pulp.local at offset 18.
    bounded(|| {
        for t in 0u8..=0x3F {
            let mut body = question(&[0xC0, t], TYPE_A, CLASS_IN);
            body.extend_from_slice(&name("pulp.local"));
            let pkt = packet(0, 0, [1, 0, 0, 0], &body);
            assert_eq!(&pkt[18..30], b"\x04pulp\x05local\x00");
            assert_silent(&pkt, &format!("pointer to {t}"));
        }
        // far out-of-bounds targets
        for t in [0xC0FFu16, 0xC100, 0xFFFF, 0xC3FF] {
            let pkt = packet(
                0,
                0,
                [1, 0, 0, 0],
                &question(&t.to_be_bytes(), TYPE_A, CLASS_IN),
            );
            assert_silent(&pkt, &format!("pointer {t:#x}"));
        }
    });
}

#[test]
fn forward_pointer_in_a_later_question_is_malformed() {
    // q1 = other.local; q2 = ptr to the pulp.local that sits after q2
    bounded(|| {
        let q1 = question(&name("other.local"), TYPE_A, CLASS_IN); // 12..29
        let q2_at = 12 + q1.len();
        let target = (q2_at + 6) as u8; // after q2's pointer(2) + type/class(4)
        let mut body = q1;
        body.extend(question(&[0xC0, target], TYPE_A, CLASS_IN));
        body.extend_from_slice(&name("pulp.local"));
        let pkt = packet(0, 0, [2, 0, 0, 0], &body);
        assert_eq!(&pkt[target as usize..target as usize + 5], b"\x04pulp");
        assert_silent(&pkt, "forward pointer in q2");
    });
}

#[test]
fn label_followed_by_pointer_back_to_its_own_start_loops_and_terminates() {
    // a . <ptr -> start of this name> expands forever: a.a.a....
    bounded(|| {
        let qname = [1u8, b'a', 0xC0, 0x0C];
        let pkt = packet(0, 0, [1, 0, 0, 0], &question(&qname, TYPE_A, CLASS_IN));
        assert_silent(&pkt, "label + backward pointer loop");

        // "pulp" . <ptr -> itself>: never reaches "local"
        let qname = [4u8, b'p', b'u', b'l', b'p', 0xC0, 0x0C];
        let pkt = packet(0, 0, [1, 0, 0, 0], &question(&qname, TYPE_ANY, CLASS_IN));
        assert_silent(&pkt, "pulp + loop");

        // loop reached through an earlier question's name
        let mut body = question(&[1u8, b'a', 0xC0, 0x0C], TYPE_AAAA, CLASS_IN); // 12..20
        body.extend(question(&[0xC0, 0x0C], TYPE_A, CLASS_IN));
        assert_silent(
            &packet(0, 0, [2, 0, 0, 0], &body),
            "pointer into a looping name",
        );
    });
}

#[test]
fn pointer_into_the_header_is_not_a_name() {
    // contract: qname starting with a pointer into the header => no reply
    bounded(|| {
        for t in 0u8..12 {
            let pkt = packet(0, 0, [1, 0, 0, 0], &question(&[0xC0, t], TYPE_A, CLASS_IN));
            assert_silent(&pkt, &format!("pointer to header offset {t}"));
            let pkt = packet(
                0,
                0,
                [1, 0, 0, 0],
                &question(&[0xC0, t], TYPE_ANY, QU | CLASS_IN),
            );
            assert_silent(&pkt, &format!("QU ANY pointer to header offset {t}"));
        }
        // ID bytes 04 'p' form the start of a label if a pointer lands there
        let mut pkt = header(0x0470, 0, [1, 0, 0, 0]);
        pkt.extend(question(&[0xC0, 0x00], TYPE_A, CLASS_IN));
        assert_silent(&pkt, "pointer to ID bytes 04 'p'");
    });
}

#[test]
fn truncated_or_dangling_pointers_do_not_reply() {
    bounded(|| {
        // pointer needs two octets; only one present at end of packet
        let mut pkt = header(0, 0, [1, 0, 0, 0]);
        pkt.push(0xC0);
        assert_silent(&pkt, "half a pointer");
        // pointer present but no type / class
        let mut body = question(&name("pulp.local"), TYPE_AAAA, CLASS_IN);
        body.extend_from_slice(&[0xC0, 0x0C]);
        assert_silent(
            &packet(0, 0, [2, 0, 0, 0], &body),
            "pointer name, no type/class",
        );
        // pointer target beyond the end of the packet
        let pkt = packet(
            0,
            0,
            [1, 0, 0, 0],
            &question(&[0xC1, 0x00], TYPE_A, CLASS_IN),
        );
        assert_silent(&pkt, "pointer past end");
        // label length runs past the end of the packet
        let mut pkt = header(0, 0, [1, 0, 0, 0]);
        pkt.extend_from_slice(b"\x3Fpulp");
        assert_silent(&pkt, "label longer than packet");
    });
}

// ----------------------------------------------------------------
fn opt_rr() -> Vec<u8> {
    // RFC 6891 6.1.2: root name, TYPE 41, CLASS = UDP payload size, TTL 0, RDLEN 0
    let mut v = vec![0u8];
    v.extend_from_slice(&TYPE_OPT.to_be_bytes());
    v.extend_from_slice(&4096u16.to_be_bytes());
    v.extend_from_slice(&0u32.to_be_bytes());
    v.extend_from_slice(&0u16.to_be_bytes());
    v
}

fn known_answer_rr() -> Vec<u8> {
    // RFC 6762 7.1: known-answer = full RR; name by pointer to the question
    let mut v = vec![0xC0, 0x0C];
    v.extend_from_slice(&TYPE_A.to_be_bytes());
    v.extend_from_slice(&CLASS_IN.to_be_bytes());
    v.extend_from_slice(&60u32.to_be_bytes());
    v.extend_from_slice(&4u16.to_be_bytes());
    v.extend_from_slice(&[192, 168, 1, 99]);
    v
}

fn query_with_extras() -> (Vec<u8>, usize) {
    let q = question(&name("pulp.local"), TYPE_A, CLASS_IN);
    let qend = 12 + q.len();
    let mut body = q;
    body.extend(known_answer_rr());
    body.extend(opt_rr());
    (packet(0, 0, [1, 1, 0, 1], &body), qend)
}

#[test]
fn edns_opt_and_known_answer_do_not_prevent_a_reply() {
    // contract: ANCOUNT / NSCOUNT / ARCOUNT may be non-zero
    let q = question(&name("pulp.local"), TYPE_A, CLASS_IN);

    let mut body = q.clone();
    body.extend(opt_rr());
    assert_reply(&packet(0, 0, [1, 0, 0, 1], &body), "ARCOUNT 1 with OPT");

    let mut body = q.clone();
    body.extend(known_answer_rr());
    assert_reply(&packet(0, 0, [1, 1, 0, 0], &body), "ANCOUNT 1 known-answer");

    let (pkt, _) = query_with_extras();
    assert_reply(&pkt, "known-answer + OPT");

    // NSCOUNT: authority record (RFC 6762 8.2 probe style) - any RR bytes
    let mut body = q.clone();
    body.extend(known_answer_rr());
    assert_reply(&packet(0, 0, [1, 0, 1, 0], &body), "NSCOUNT 1");
}

#[test]
fn record_counts_are_not_validated_against_the_bytes_that_follow() {
    // contract: records after the question section need not be verified
    let q = question(&name("pulp.local"), TYPE_A, CLASS_IN);
    for counts in [
        [1u16, 1, 0, 0],
        [1, 0, 1, 0],
        [1, 0, 0, 1],
        [1, 0xFFFF, 0xFFFF, 0xFFFF],
        [1, 7, 7, 7],
    ] {
        assert_reply(
            &packet(0, 0, counts, &q),
            &format!("counts {counts:?}, no records"),
        );
    }
}

#[test]
fn truncated_additional_records_still_reply() {
    // contract: truncated records after a complete question section do not
    // matter. Every prefix from the end of the question section on replies.
    let (pkt, qend) = query_with_extras();
    for len in qend..=pkt.len() {
        assert_reply(&pkt[..len], &format!("prefix {len} of {}", pkt.len()));
    }
    // garbage tail
    let mut junk = pkt[..qend].to_vec();
    junk.extend_from_slice(&[0xFF; 40]);
    junk[4..12].copy_from_slice(&[0, 1, 0, 2, 0, 3, 0, 4]);
    assert_reply(&junk, "garbage after the question section");
}

// ----------------------------------------------------------------
#[test]
fn every_prefix_of_a_valid_query_is_rejected_without_panic() {
    let q = std_query();
    for len in 0..q.len() {
        assert_silent(&q[..len], &format!("prefix of length {len}"));
    }
    assert_reply(&q, "full query");
}

#[test]
fn short_packets_do_not_reply() {
    // contract: header is 12 bytes. < 12, exactly 12, and one byte short.
    let q = std_query();
    for len in [0usize, 1, 2, 11] {
        assert_silent(&q[..len], &format!("{len} bytes"));
    }
    assert_silent(&q[..12], "header only, QD 1");
    assert_silent(&header(0, 0, [1, 0, 0, 0]), "header only built directly");
    assert_silent(&q[..q.len() - 1], "one byte short");
    assert_silent(&q[..q.len() - 4], "no type/class");
    assert_silent(&q[..q.len() - 2], "no class");
    assert_silent(&[], "empty datagram");
}

#[test]
fn prefixes_of_multi_question_and_compressed_packets_are_rejected_until_complete() {
    // contract: the question section must be complete; all QDCOUNT parsed
    let mut cases: Vec<(Vec<u8>, usize, &str)> = Vec::new();

    let mut body = question(&name("other.local"), TYPE_A, CLASS_IN);
    body.extend(question(&name("pulp.local"), TYPE_A, CLASS_IN));
    let end = 12 + body.len();
    cases.push((packet(0, 0, [2, 0, 0, 0], &body), end, "other then pulp"));

    let mut body = question(&name("pulp.local"), TYPE_AAAA, CLASS_IN);
    body.extend(question(&[0xC0, 0x0C], TYPE_A, CLASS_IN));
    let end = 12 + body.len();
    cases.push((
        packet(0, 0, [2, 0, 0, 0], &body),
        end,
        "AAAA then pointer A",
    ));

    let (pkt, qend) = query_with_extras();
    cases.push((pkt, qend, "query with extras"));

    for (pkt, end, label) in cases {
        for len in 0..=pkt.len() {
            let expect = len >= end;
            let got = run(&pkt[..len], IP).is_some();
            assert_eq!(
                got, expect,
                "{label}: prefix {len} (question section ends at {end})"
            );
        }
    }
}

// Deterministic xorshift64* generator (fixed seed; no extra dependency).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn byte(&mut self) -> u8 {
        (self.next() >> 24) as u8
    }
}

// Any reply the responder gives under fuzzing must still be the one valid
// 38-byte answer carrying the supplied IP.
fn fuzz_one(pkt: &[u8], ip: [u8; 4], replies: &mut usize) {
    let mut out = [0u8; RESPONSE_LEN];
    if handle_packet(pkt, ip, &mut out) {
        check_response(&out, ip);
        *replies += 1;
    }
}

#[test]
fn random_bytes_never_panic_or_hang() {
    let replies = bounded(|| {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let mut replies = 0usize;
        for _ in 0..30_000 {
            let len = rng.below(601); // 0..=600
            let pkt: Vec<u8> = (0..len).map(|_| rng.byte()).collect();
            fuzz_one(
                &pkt,
                [rng.byte(), rng.byte(), rng.byte(), rng.byte()],
                &mut replies,
            );
        }
        replies
    });
    // uniformly random bytes are essentially never a pulp.local query
    assert_eq!(replies, 0, "random bytes produced replies");
}

#[test]
fn random_bodies_behind_a_query_header_never_panic_or_hang() {
    // Valid query header with random counts, then bytes biased toward the
    // structural ones (length octets, pointer markers, letters of the name),
    // so name / pointer parsing is actually exercised.
    bounded(|| {
        let mut rng = Rng(0xD1B5_4A32_D192_ED03);
        let alphabet: [u8; 16] = [
            0, 0, 4, 5, 0xC0, 0xC0, 0x0C, 12, b'p', b'u', b'l', b'o', b'c', b'a', 0x00, 0x01,
        ];
        let mut replies = 0usize;
        for _ in 0..30_000 {
            let len = rng.below(120);
            let mut pkt = header(
                0,
                0,
                [
                    1 + rng.below(4) as u16,
                    rng.below(3) as u16,
                    rng.below(3) as u16,
                    rng.below(3) as u16,
                ],
            );
            for _ in 0..len {
                pkt.push(if rng.below(4) == 0 {
                    rng.byte()
                } else {
                    alphabet[rng.below(alphabet.len())]
                });
            }
            fuzz_one(&pkt, IP, &mut replies);
        }
    });
}

#[test]
fn mutated_real_queries_never_panic_or_hang() {
    // Flip / truncate / extend byte-for-byte copies of real queries.
    bounded(|| {
        let mut bases: Vec<Vec<u8>> = vec![std_query(), query_with_extras().0];
        let mut body = question(&name("pulp.local"), TYPE_AAAA, CLASS_IN);
        body.extend(question(&[0xC0, 0x0C], TYPE_A, CLASS_IN));
        bases.push(packet(0, 0, [2, 0, 0, 0], &body));
        let mut body = question(&name("other.local"), TYPE_AAAA, CLASS_IN);
        let mut qn = vec![4];
        qn.extend_from_slice(b"pulp");
        qn.extend_from_slice(&[0xC0, 18]);
        body.extend(question(&qn, TYPE_A, CLASS_IN));
        bases.push(packet(0, 0, [2, 0, 0, 0], &body));

        let mut rng = Rng(0x0123_4567_89AB_CDEF);
        let mut replies = 0usize;
        for round in 0..40_000 {
            let mut pkt = bases[round % bases.len()].clone();
            for _ in 0..1 + rng.below(4) {
                let i = rng.below(pkt.len());
                pkt[i] = rng.byte();
            }
            match rng.below(4) {
                0 => pkt.truncate(rng.below(pkt.len() + 1)),
                1 => pkt.extend((0..rng.below(40)).map(|_| rng.byte())),
                _ => {}
            }
            fuzz_one(&pkt, IP, &mut replies);
        }
        // mutations of valid queries must leave at least some valid ones, or
        // the fuzzer is not testing the reply path
        assert!(replies > 0, "no mutated packet produced a reply");
    });
}

// ============================================================ B. ip_label

fn label_of(ip: [u8; 4]) -> String {
    let mut buf = [b'#'; IP_LABEL_MAX];
    ip_label(ip, &mut buf).to_owned()
}

#[test]
fn ip_label_named_examples() {
    assert_eq!(label_of([0, 0, 0, 0]), "(0.0.0.0)");
    assert_eq!(label_of([192, 168, 1, 23]), "(192.168.1.23)");
    assert_eq!(label_of([255, 255, 255, 255]), "(255.255.255.255)");
    assert_eq!(label_of([10, 0, 0, 1]), "(10.0.0.1)");
}

#[test]
fn ip_label_digit_boundaries_in_every_octet_position() {
    // 1, 2 and 3 digit octets: 9 / 10 / 99 / 100 / 255, no leading zeros
    let values = [0u8, 1, 9, 10, 11, 99, 100, 101, 199, 200, 254, 255];
    for a in values {
        for b in values {
            for c in values {
                for d in values {
                    let ip = [a, b, c, d];
                    let want = format!("({a}.{b}.{c}.{d})");
                    let got = label_of(ip);
                    assert_eq!(got, want, "ip {ip:?}");
                    assert!(got.len() <= IP_LABEL_MAX, "{got} longer than IP_LABEL_MAX");
                }
            }
        }
    }
}

#[test]
fn ip_label_has_no_leading_zeros() {
    for ip in [
        [1, 2, 3, 4],
        [9, 9, 9, 9],
        [10, 10, 10, 10],
        [100, 9, 10, 0],
    ] {
        let got = label_of(ip);
        for part in got.trim_matches(|c| c == '(' || c == ')').split('.') {
            assert!(part == "0" || !part.starts_with('0'), "{got}");
        }
    }
}

#[test]
fn ip_label_ignores_stale_buffer_contents() {
    // the returned str is exactly the label, whatever the buffer held
    let mut buf = [0xFFu8; IP_LABEL_MAX];
    assert_eq!(
        ip_label([255, 255, 255, 255], &mut buf),
        "(255.255.255.255)"
    );
    assert_eq!(ip_label([0, 0, 0, 0], &mut buf), "(0.0.0.0)");
    assert_eq!(ip_label([10, 0, 0, 1], &mut buf), "(10.0.0.1)");
    assert_eq!(ip_label([192, 168, 1, 23], &mut buf), "(192.168.1.23)");
    let mut buf = [b'9'; IP_LABEL_MAX];
    assert_eq!(ip_label([1, 1, 1, 1], &mut buf), "(1.1.1.1)");
}

#[test]
fn longest_label_fits_exactly() {
    let got = label_of([255, 255, 255, 255]);
    assert_eq!(got.len(), IP_LABEL_MAX);
}

// ============================================================ C. serve

#[derive(Debug, PartialEq, Clone, Copy)]
enum FakeError {
    Send(usize), // index of the failed send attempt
    Recv(u32),   // caller-chosen tag
}

enum Step {
    Packet(Vec<u8>),
    RecvFail(u32),
}

struct State {
    steps: VecDeque<Step>,
    exhausted: bool,
    attempts: Vec<Vec<u8>>,
    fail_sends: Vec<usize>,
    loopback: bool,
}

// Datagram socket double: serves a scripted packet sequence; once the script
// is exhausted `recv` stays pending forever (a quiet network). `send` records
// every attempt, can be told to fail, and can loop the data back as received.
struct FakeDatagrams {
    st: Rc<RefCell<State>>,
}

impl Datagrams for FakeDatagrams {
    type Error = FakeError;

    async fn recv(&mut self, buf: &mut [u8]) -> Result<usize, FakeError> {
        let step = self.st.borrow_mut().steps.pop_front();
        match step {
            Some(Step::Packet(p)) => {
                let n = p.len().min(buf.len());
                buf[..n].copy_from_slice(&p[..n]);
                Ok(n)
            }
            Some(Step::RecvFail(tag)) => Err(FakeError::Recv(tag)),
            None => {
                self.st.borrow_mut().exhausted = true;
                core::future::pending().await
            }
        }
    }

    async fn send(&mut self, data: &[u8]) -> Result<(), FakeError> {
        let mut st = self.st.borrow_mut();
        let idx = st.attempts.len();
        st.attempts.push(data.to_vec());
        if st.loopback {
            st.steps.push_back(Step::Packet(data.to_vec()));
        }
        if st.fail_sends.contains(&idx) {
            Err(FakeError::Send(idx))
        } else {
            Ok(())
        }
    }
}

struct Harness {
    st: Rc<RefCell<State>>,
    sock: FakeDatagrams,
}

#[derive(Debug)]
struct Outcome {
    // every send attempt, in order
    attempts: Vec<Vec<u8>>,
    // Some(e): serve returned Err(e); None: serve was still running when the
    // script ran dry
    ended: Option<FakeError>,
    // scripted steps never consumed
    unread: usize,
}

impl Harness {
    fn new(steps: Vec<Step>) -> Self {
        let st = Rc::new(RefCell::new(State {
            steps: steps.into(),
            exhausted: false,
            attempts: Vec::new(),
            fail_sends: Vec::new(),
            loopback: false,
        }));
        Harness {
            sock: FakeDatagrams { st: st.clone() },
            st,
        }
    }

    fn with_fail_sends(self, idx: &[usize]) -> Self {
        self.st.borrow_mut().fail_sends = idx.to_vec();
        self
    }

    fn with_loopback(self) -> Self {
        self.st.borrow_mut().loopback = true;
        self
    }

    fn push(&self, steps: Vec<Step>) {
        let mut st = self.st.borrow_mut();
        st.steps.extend(steps);
        st.exhausted = false;
    }

    // Run `serve` until it returns or the script is exhausted (then it must
    // be parked in recv).
    fn run(&mut self, ip: [u8; 4]) -> Outcome {
        let start = self.st.borrow().attempts.len();
        let waiter = {
            let st = self.st.clone();
            async move {
                while !st.borrow().exhausted {
                    yield_now().await;
                }
            }
        };
        // no timer here (the embassy generic queue is tiny and tests run in
        // parallel); `bounded` supplies the wall-clock guard
        let res = block_on(select(serve(&mut self.sock, ip), waiter));
        let ended = match res {
            Either::First(Err(e)) => Some(e),
            Either::First(Ok(never)) => match never {},
            Either::Second(()) => None,
        };
        let st = self.st.borrow();
        Outcome {
            attempts: st.attempts[start..].to_vec(),
            ended,
            unread: st.steps.len(),
        }
    }
}

fn pkts(v: &[&[u8]]) -> Vec<Step> {
    v.iter().map(|p| Step::Packet(p.to_vec())).collect()
}

fn noise() -> Vec<u8> {
    vec![0xDE, 0xAD, 0xBE, 0xEF, 0x00]
}

#[test]
fn serve_answers_each_query_and_survives_noise() {
    // contract: non-matching packets are ignored without returning
    let q1 = query("pulp.local", TYPE_A, CLASS_IN);
    let q2 = query("PULP.LOCAL", TYPE_ANY, QU | CLASS_IN);
    let q3 = query("Pulp.Local", TYPE_A, QU | CLASS_IN);
    let n = noise();
    let steps = pkts(&[&n, &q1, &n, &q2, &q3]);
    let out = bounded(move || Harness::new(steps).run(IP));
    assert_eq!(out.ended, None, "serve returned on noise");
    assert_eq!(out.unread, 0);
    assert_eq!(
        out.attempts.len(),
        3,
        "exactly one reply per matching query"
    );
    for (resp, q) in out.attempts.iter().zip([&q1, &q2, &q3]) {
        check_response(resp, IP);
        let mut want = [0u8; RESPONSE_LEN];
        assert!(handle_packet(q, IP, &mut want));
        assert_eq!(
            resp.as_slice(),
            &want[..],
            "reply equals handle_packet for the query"
        );
    }
}

#[test]
fn serve_ignores_a_variety_of_non_queries() {
    // contract: unmatched packets (junk, other names, AAAA, responses,
    // zero-length datagrams, short packets) are ignored, serve keeps running
    let own_response = run(&std_query(), IP).expect("reply");
    let steps = vec![
        Step::Packet(vec![]),
        Step::Packet(noise()),
        Step::Packet(query("other.local", TYPE_A, CLASS_IN)),
        Step::Packet(query("pulp.local", TYPE_AAAA, CLASS_IN)),
        Step::Packet(query("pulp.local", TYPE_A, CLASS_CH)),
        Step::Packet(own_response.to_vec()),
        Step::Packet(std_query()[..11].to_vec()),
        Step::Packet(std_query()[..12].to_vec()),
        Step::Packet(vec![0u8; 12]),
        Step::Packet(vec![0xFFu8; 300]),
        Step::Packet(std_query()),
    ];
    let out = bounded(move || Harness::new(steps).run(IP));
    assert_eq!(out.ended, None);
    assert_eq!(out.unread, 0);
    assert_eq!(out.attempts.len(), 1, "only the final query is answered");
    check_response(&out.attempts[0], IP);
}

#[test]
fn serve_answers_after_a_long_run_of_non_matching_packets() {
    let mut rng = Rng(0xA5A5_A5A5_1234_5678);
    let mut steps = Vec::new();
    for i in 0..300 {
        let p = match i % 4 {
            0 => noise(),
            1 => query("other.local", TYPE_A, CLASS_IN),
            2 => (0..rng.below(100)).map(|_| rng.byte()).collect(),
            _ => query("pulp.local", TYPE_AAAA, CLASS_IN),
        };
        // random packets are exceedingly unlikely to be queries for pulp.local;
        // make that certain by forcing the QR bit when they have a header
        let mut p = p;
        if i % 4 == 2 && p.len() >= 12 {
            p[2] |= 0x80;
        }
        steps.push(Step::Packet(p));
    }
    steps.push(Step::Packet(std_query()));
    let out = bounded(move || Harness::new(steps).run(IP));
    assert_eq!(out.ended, None, "serve returned during the noise run");
    assert_eq!(out.unread, 0);
    assert_eq!(out.attempts.len(), 1);
    check_response(&out.attempts[0], IP);
}

#[test]
fn serve_answers_a_query_between_long_noise_stretches() {
    // 250 noise, query, 250 noise, query
    let mut steps = Vec::new();
    for _ in 0..250 {
        steps.push(Step::Packet(noise()));
    }
    steps.push(Step::Packet(std_query()));
    for _ in 0..250 {
        steps.push(Step::Packet(query("a.pulp.local", TYPE_A, CLASS_IN)));
    }
    steps.push(Step::Packet(std_query()));
    let out = bounded(move || Harness::new(steps).run(IP));
    assert_eq!(out.ended, None);
    assert_eq!(out.attempts.len(), 2);
    for r in &out.attempts {
        check_response(r, IP);
    }
}

#[test]
fn a_failed_send_does_not_end_serve_and_later_queries_are_answered() {
    // contract: send failure does not terminate serve
    let steps = (0..5)
        .map(|_| Step::Packet(std_query()))
        .collect::<Vec<_>>();
    let out = bounded(move || Harness::new(steps).with_fail_sends(&[0]).run(IP));
    assert_eq!(out.ended, None, "serve returned after a send failure");
    assert_eq!(out.unread, 0);
    assert_eq!(out.attempts.len(), 5, "every query attempted a reply");
    for r in &out.attempts {
        check_response(r, IP);
    }
}

#[test]
fn failures_in_the_middle_and_every_send_failing_do_not_end_serve() {
    let steps = (0..6)
        .map(|_| Step::Packet(std_query()))
        .collect::<Vec<_>>();
    let out = bounded(move || Harness::new(steps).with_fail_sends(&[1, 2, 4]).run(IP));
    assert_eq!(out.ended, None);
    assert_eq!(out.attempts.len(), 6);

    let steps = (0..10)
        .map(|_| Step::Packet(std_query()))
        .collect::<Vec<_>>();
    let all: Vec<usize> = (0..10).collect();
    let out = bounded(move || Harness::new(steps).with_fail_sends(&all).run(IP));
    assert_eq!(out.ended, None, "serve gave up when every send failed");
    assert_eq!(out.attempts.len(), 10);
}

#[test]
fn recv_failure_ends_serve_with_that_error_after_earlier_queries_were_answered() {
    // contract: only a recv error makes serve return, with that Err
    let q = std_query();
    let steps = vec![
        Step::Packet(q.clone()),
        Step::Packet(noise()),
        Step::Packet(q.clone()),
        Step::RecvFail(77),
        Step::Packet(q.clone()),
    ];
    let out = bounded(move || Harness::new(steps).run(IP));
    assert_eq!(out.ended, Some(FakeError::Recv(77)));
    assert_eq!(
        out.attempts.len(),
        2,
        "queries before the failure were answered"
    );
    for r in &out.attempts {
        check_response(r, IP);
    }
    assert_eq!(out.unread, 1, "serve read past the failed recv");
}

#[test]
fn recv_failure_on_the_very_first_call_returns_immediately() {
    let out = bounded(|| Harness::new(vec![Step::RecvFail(1), Step::Packet(std_query())]).run(IP));
    assert_eq!(out.ended, Some(FakeError::Recv(1)));
    assert!(out.attempts.is_empty());
    assert_eq!(out.unread, 1);
}

#[test]
fn recv_failure_after_a_failed_send_still_reports_the_recv_error() {
    // a send error must not be reported in place of, or mask, the recv error
    let steps = vec![Step::Packet(std_query()), Step::RecvFail(9)];
    let out = bounded(move || Harness::new(steps).with_fail_sends(&[0]).run(IP));
    assert_eq!(out.ended, Some(FakeError::Recv(9)));
    assert_eq!(out.attempts.len(), 1);
}

#[test]
fn replies_carry_the_ip_passed_to_serve_and_a_new_call_uses_the_new_ip() {
    // contract: reply RDATA = ip given to serve; calling serve again with
    // another ip answers with that one.
    let ip2 = [10, 1, 2, 3];
    let ip3 = [0, 0, 0, 0];
    let (a, b, c) = bounded(move || {
        let mut h = Harness::new(pkts(&[&std_query()]));
        let a = h.run(IP);
        h.push(pkts(&[&std_query(), &std_query()]));
        let b = h.run(ip2);
        h.push(pkts(&[&std_query()]));
        let c = h.run(ip3);
        (a, b, c)
    });
    assert_eq!(a.ended, None);
    assert_eq!(a.attempts.len(), 1);
    check_response(&a.attempts[0], IP);
    assert_eq!(b.attempts.len(), 2);
    for r in &b.attempts {
        check_response(r, ip2);
        assert_ne!(&r[34..38], &IP[..], "stale ip in reply");
    }
    assert_eq!(c.attempts.len(), 1);
    check_response(&c.attempts[0], ip3);
}

#[test]
fn serve_does_not_answer_its_own_looped_back_responses() {
    // RFC 6762 multicast loopback: our reply is delivered back to our own
    // socket. It is a response (QR=1), so it must not trigger another reply.
    let steps = (0..3)
        .map(|_| Step::Packet(std_query()))
        .collect::<Vec<_>>();
    let out = bounded(move || Harness::new(steps).with_loopback().run(IP));
    assert_eq!(out.ended, None);
    assert_eq!(out.unread, 0);
    assert_eq!(out.attempts.len(), 3, "replies were re-answered");
}

#[test]
fn serve_with_no_traffic_never_returns_and_never_sends() {
    let out = bounded(|| Harness::new(vec![]).run(IP));
    assert_eq!(out.ended, None);
    assert!(out.attempts.is_empty());
}
