// ZIP writer for the fixture EPUBs. Layout is fully pinned so the bytes are a
// pure function of the entries: general-purpose flags 0 (sizes and CRC in the
// local header, no data descriptor), no extra field, no comments, DOS time
// 00:00:00 and date 1980-01-01, entries tightly packed, central directory and
// EOCD last. Deflate uses miniz_oxide at one fixed level.

use miniz_oxide::deflate::compress_to_vec;

const LOCAL_SIG: u32 = 0x0403_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const EOCD_SIG: u32 = 0x0605_4b50;

const VERSION: u16 = 20;
const DOS_TIME: u16 = 0x0000;
const DOS_DATE: u16 = 0x0021;
const METHOD_STORED: u16 = 0;
const METHOD_DEFLATE: u16 = 8;

// the one compression level every deflate stream of the fixtures uses
pub(super) const LEVEL: u8 = 6;

pub(super) struct Entry {
    pub name: String,
    pub data: Vec<u8>,
    pub deflate: bool,
}

// CRC-32 (IEEE 802.3, reflected), shared with the PNG chunk writer
pub(super) fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn put16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub(super) fn write(entries: &[Entry]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for e in entries {
        let body = if e.deflate {
            compress_to_vec(&e.data, LEVEL)
        } else {
            e.data.clone()
        };
        let method = if e.deflate {
            METHOD_DEFLATE
        } else {
            METHOD_STORED
        };
        let crc = crc32(&e.data);
        let (csize, usize_) = (body.len() as u32, e.data.len() as u32);
        let name = e.name.as_bytes();

        let offset = out.len() as u32;
        put32(&mut out, LOCAL_SIG);
        put16(&mut out, VERSION);
        put16(&mut out, 0); // flags
        put16(&mut out, method);
        put16(&mut out, DOS_TIME);
        put16(&mut out, DOS_DATE);
        put32(&mut out, crc);
        put32(&mut out, csize);
        put32(&mut out, usize_);
        put16(&mut out, name.len() as u16);
        put16(&mut out, 0); // extra length
        out.extend_from_slice(name);
        out.extend_from_slice(&body);

        put32(&mut central, CENTRAL_SIG);
        put16(&mut central, VERSION); // made by
        put16(&mut central, VERSION); // needed
        put16(&mut central, 0); // flags
        put16(&mut central, method);
        put16(&mut central, DOS_TIME);
        put16(&mut central, DOS_DATE);
        put32(&mut central, crc);
        put32(&mut central, csize);
        put32(&mut central, usize_);
        put16(&mut central, name.len() as u16);
        put16(&mut central, 0); // extra length
        put16(&mut central, 0); // comment length
        put16(&mut central, 0); // disk number start
        put16(&mut central, 0); // internal attributes
        put32(&mut central, 0); // external attributes
        put32(&mut central, offset);
        central.extend_from_slice(name);
    }

    let central_offset = out.len() as u32;
    out.extend_from_slice(&central);
    put32(&mut out, EOCD_SIG);
    put16(&mut out, 0); // this disk
    put16(&mut out, 0); // central directory disk
    put16(&mut out, entries.len() as u16);
    put16(&mut out, entries.len() as u16);
    put32(&mut out, central.len() as u32);
    put32(&mut out, central_offset);
    put16(&mut out, 0); // comment length
    out
}
