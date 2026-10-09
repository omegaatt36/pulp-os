// Page-table sizing, capacity-vs-EOF decisions and the persistent record.
use pulp_board_logic::memory::{MIB, PsramFault, PsramStatus};
use pulp_board_logic::page_index::{
    BANK_BODY_INSTALLED, BANK_BODY_USED, BANK_HEADING_USED, ENTRY_BYTES, FOOTER_BYTES,
    HEADER_BYTES, HR8_PAGES, LayoutKey, PageStep, Reader, Reject, SMALL_PAGES, Writer,
    acquire_table, encode_header, header_banks, next_page, parse_header, record_len, record_name,
    table_plan,
};
use pulp_board_logic::source_id::{Fnv64, SourceId};

fn ready(bytes: usize) -> PsramStatus {
    PsramStatus::Ready { bytes }
}

#[test]
fn table_plan_is_large_then_small_only_on_a_validated_hr8() {
    assert_eq!(table_plan(ready(8 * MIB)), &[HR8_PAGES, SMALL_PAGES]);
    assert_eq!(HR8_PAGES, 4096);
    assert_eq!(SMALL_PAGES, 512);
    for status in [
        ready(2 * MIB),
        ready(8 * MIB - 1),
        PsramStatus::NotInitialised,
        PsramStatus::Degraded(PsramFault::NotDetected),
    ] {
        assert_eq!(table_plan(status), &[SMALL_PAGES]);
    }
}

#[test]
fn a_refused_large_table_falls_back_and_a_refused_small_one_fails() {
    let mut asked = Vec::new();
    let got = acquire_table(ready(8 * MIB), |n| {
        asked.push(n);
        if n == HR8_PAGES {
            Err("over budget")
        } else {
            Ok(n)
        }
    });
    assert_eq!(got, Ok((SMALL_PAGES, SMALL_PAGES)));
    assert_eq!(asked, [HR8_PAGES, SMALL_PAGES]);
    assert_eq!(
        acquire_table::<usize, _>(ready(8 * MIB), |_| Err("no memory")),
        Err("no memory")
    );
    assert_eq!(
        acquire_table(ready(2 * MIB), |n| Ok::<_, ()>(n)),
        Ok((512, 512))
    );
}

#[test]
fn eof_completion_and_capacity_truncation_are_different_outcomes() {
    // text continues, room left
    assert_eq!(next_page(100, 1000, 100, 10, 512), PageStep::Append);
    // text continues, table full: truncation, not completion
    assert_eq!(next_page(100, 1000, 100, 512, 512), PageStep::Truncated);
    assert_eq!(next_page(100, 1000, 100, 4096, 4096), PageStep::Truncated);
    // text ended exactly at the end of the table: still EOF completion
    assert_eq!(next_page(1000, 1000, 100, 512, 512), PageStep::Done);
    // no progress cannot extend the index
    assert_eq!(next_page(0, 1000, 0, 3, 512), PageStep::Done);
    // the actual capacity decides: 512 entries truncate where 4096 would not
    assert_eq!(next_page(100, 1000, 100, 512, 4096), PageStep::Append);
}

fn key() -> LayoutKey {
    LayoutKey {
        source: SourceId::from_raw(0x1122_3344_5566_7788),
        chapter: 7,
        text_size: 90_000,
        layout_version: 1,
        latin_sig: 0xDEAD_BEEF,
        body_px: 23,
        heading_px: 32,
        body_font: 0x0102_0304_0506_0708,
        heading_font: 0,
        banks: BANK_BODY_USED | BANK_BODY_INSTALLED,
        text_w: 784,
        text_area_h: 440,
        line_h: 22,
        max_lines: 20,
    }
}

fn record(k: &LayoutKey, offsets: &[u32]) -> Vec<u8> {
    let mut out = encode_header(k, offsets.len()).to_vec();
    let mut w = Writer::new();
    let mut e = [0u8; ENTRY_BYTES];
    for (i, &o) in offsets.iter().enumerate() {
        w.push(&mut e, o, (i % 5) as u8, (i % 3) as u8);
        out.extend_from_slice(&e);
    }
    out.extend_from_slice(&w.footer(offsets.len()).unwrap());
    out
}

fn offsets(n: usize) -> Vec<u32> {
    (0..n as u32).map(|i| i * 50).collect()
}

fn load(bytes: &[u8], want: &LayoutKey, capacity: usize) -> Result<Vec<(u32, u8, u8)>, Reject> {
    let count = parse_header(bytes, want, bytes.len() as u64, capacity)?;
    let payload = &bytes[HEADER_BYTES..bytes.len() - FOOTER_BYTES];
    let mut reader = Reader::new(count, want.text_size);
    let mut got = vec![(0, 0, 0); count];
    // feed in odd-sized runs of whole entries
    for run in payload.chunks(ENTRY_BYTES * 7) {
        reader.feed(run, &mut |i, o, s, n| got[i] = (o, s, n))?;
    }
    reader.finish(&bytes[bytes.len() - FOOTER_BYTES..])?;
    Ok(got)
}

#[test]
fn round_trip_is_exact() {
    let offs = offsets(300);
    let bytes = record(&key(), &offs);
    assert_eq!(bytes.len(), record_len(300).unwrap());
    let got = load(&bytes, &key(), 4096).unwrap();
    assert_eq!(got.len(), 300);
    for (i, (o, s, n)) in got.into_iter().enumerate() {
        assert_eq!((o, s, n), (offs[i], (i % 5) as u8, (i % 3) as u8));
    }
    // a one-page chapter
    assert_eq!(load(&record(&key(), &[0]), &key(), 512).unwrap().len(), 1);
}

#[test]
fn every_key_field_invalidates() {
    let bytes = record(&key(), &offsets(10));
    let variants: Vec<(&str, LayoutKey)> = vec![
        (
            "source",
            LayoutKey {
                source: SourceId::from_raw(9),
                ..key()
            },
        ),
        (
            "chapter",
            LayoutKey {
                chapter: 8,
                ..key()
            },
        ),
        (
            "text size",
            LayoutKey {
                text_size: 90_001,
                ..key()
            },
        ),
        (
            "layout version",
            LayoutKey {
                layout_version: 2,
                ..key()
            },
        ),
        (
            "latin font",
            LayoutKey {
                latin_sig: 1,
                ..key()
            },
        ),
        (
            "body px",
            LayoutKey {
                body_px: 28,
                ..key()
            },
        ),
        (
            "heading px",
            LayoutKey {
                heading_px: 38,
                ..key()
            },
        ),
        (
            "body pack id",
            LayoutKey {
                body_font: 5,
                ..key()
            },
        ),
        (
            "heading pack id",
            LayoutKey {
                heading_font: 5,
                ..key()
            },
        ),
        (
            "bank use",
            LayoutKey {
                banks: BANK_BODY_USED | BANK_BODY_INSTALLED | BANK_HEADING_USED,
                ..key()
            },
        ),
        (
            "text width",
            LayoutKey {
                text_w: 700,
                ..key()
            },
        ),
        (
            "text area",
            LayoutKey {
                text_area_h: 400,
                ..key()
            },
        ),
        (
            "line height",
            LayoutKey {
                line_h: 25,
                ..key()
            },
        ),
        (
            "lines per page",
            LayoutKey {
                max_lines: 19,
                ..key()
            },
        ),
    ];
    for (what, other) in variants {
        assert_eq!(
            parse_header(&bytes, &other, bytes.len() as u64, 4096),
            Err(Reject::KeyMismatch),
            "{what}"
        );
    }
}

#[test]
fn truncated_and_extended_files_are_rejected() {
    let bytes = record(&key(), &offsets(40));
    for cut in [
        0,
        10,
        HEADER_BYTES - 1,
        HEADER_BYTES,
        bytes.len() - FOOTER_BYTES,
        bytes.len() - 1,
    ] {
        let r = load(&bytes[..cut], &key(), 4096);
        assert!(r.is_err(), "cut {cut}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert_eq!(load(&longer, &key(), 4096), Err(Reject::Size));
    // the header alone with a footer-less tail: torn write before commit
    let torn = &bytes[..bytes.len() - FOOTER_BYTES];
    assert!(load(torn, &key(), 4096).is_err());
}

#[test]
fn corruption_anywhere_is_detected() {
    let bytes = record(&key(), &offsets(40));
    for i in 0..bytes.len() {
        let mut bad = bytes.clone();
        bad[i] ^= 0x01;
        assert!(
            load(&bad, &key(), 4096).is_err(),
            "flip in byte {i} went unnoticed"
        );
    }
}

#[test]
fn a_missing_commit_footer_is_not_a_record() {
    let mut bytes = record(&key(), &offsets(5));
    let n = bytes.len();
    bytes[n - 4..].copy_from_slice(b"XXXX");
    assert_eq!(load(&bytes, &key(), 4096), Err(Reject::Footer));
}

#[test]
fn implausible_but_checksummed_entries_are_rejected() {
    // first page not at the start
    assert_eq!(
        load(&record(&key(), &[5, 100, 200]), &key(), 4096),
        Err(Reject::Order)
    );
    // not increasing
    assert_eq!(
        load(&record(&key(), &[0, 200, 200]), &key(), 4096),
        Err(Reject::Order)
    );
    assert_eq!(
        load(&record(&key(), &[0, 300, 200]), &key(), 4096),
        Err(Reject::Order)
    );
    // beyond the text
    assert_eq!(
        load(&record(&key(), &[0, 100, 90_000]), &key(), 4096),
        Err(Reject::Order)
    );
}

#[test]
fn a_record_bigger_than_the_table_is_a_capacity_miss_not_a_hit() {
    let bytes = record(&key(), &offsets(1000));
    assert_eq!(load(&bytes, &key(), 512), Err(Reject::Capacity));
    assert_eq!(load(&bytes, &key(), 1000).unwrap().len(), 1000);
    // the writer cannot produce a record with the wrong count either
    let mut w = Writer::new();
    let mut e = [0u8; ENTRY_BYTES];
    w.push(&mut e, 0, 0, 0);
    assert!(w.footer(2).is_none());
    assert!(w.footer(1).is_some());
}

#[test]
fn zero_pages_and_unknown_versions_are_rejected() {
    let mut hdr = encode_header(&key(), 0);
    assert_eq!(parse_header(&hdr, &key(), 0, 512), Err(Reject::Size));
    hdr = encode_header(&key(), 1);
    hdr[4] = 9;
    // the header checksum guards the version byte as well
    assert_eq!(
        parse_header(&hdr, &key(), 1, 512),
        Err(Reject::HeaderChecksum)
    );
    assert_eq!(
        parse_header(&hdr[..10], &key(), 1, 512),
        Err(Reject::Truncated)
    );
    let mut magic = encode_header(&key(), 1);
    magic[0] = b'X';
    assert_eq!(parse_header(&magic, &key(), 1, 512), Err(Reject::Magic));
}

#[test]
fn an_unknown_version_with_a_valid_header_checksum_is_rejected_as_a_version() {
    use pulp_board_logic::page_index::FORMAT_VERSION;
    let mut hdr = encode_header(&key(), 3);
    let version = FORMAT_VERSION + 1;
    hdr[4..6].copy_from_slice(&version.to_le_bytes());
    // re-seal the header: the checksum covers everything before itself
    let at = HEADER_BYTES - 4;
    let mut h = Fnv64::new();
    h.update(&hdr[..at]);
    let sum = h.finish();
    hdr[at..].copy_from_slice(&((sum ^ (sum >> 32)) as u32).to_le_bytes());

    assert_eq!(
        parse_header(&hdr, &key(), record_len(3).unwrap() as u64, 512),
        Err(Reject::Version)
    );
    assert_eq!(header_banks(&hdr), Err(Reject::Version));
    // control: the same header with the current version is accepted
    let ok = encode_header(&key(), 3);
    assert_eq!(
        parse_header(&ok, &key(), record_len(3).unwrap() as u64, 512),
        Ok(3)
    );
}

#[test]
fn record_names_are_8_3_and_per_chapter() {
    assert_eq!(&record_name(0), b"PG000.IDX");
    assert_eq!(&record_name(7), b"PG007.IDX");
    assert_eq!(&record_name(255), b"PG255.IDX");
}

#[test]
fn source_identity_depends_on_the_whole_central_directory() {
    let cd: Vec<u8> = (0..2000u32).map(|i| (i * 7 + 3) as u8).collect();
    let a = SourceId::from_archive(50_000, 40_000, &cd);
    assert_eq!(a, SourceId::from_archive(50_000, 40_000, &cd));
    assert!(!a.is_none());
    // same name and same archive size, different entry bytes (e.g. a CRC)
    let mut changed = cd.clone();
    changed[1500] ^= 1;
    assert_ne!(a, SourceId::from_archive(50_000, 40_000, &changed));
    assert_ne!(a, SourceId::from_archive(50_001, 40_000, &cd));
    assert_ne!(a, SourceId::from_archive(50_000, 40_001, &cd));
    assert_ne!(a, SourceId::from_archive(50_000, 40_000, &cd[..1999]));
    let mut inc = Fnv64::new();
    inc.update(b"ab");
    inc.update(b"cd");
    let mut whole = Fnv64::new();
    whole.update(b"abcd");
    assert_eq!(inc.finish(), whole.finish());
}

#[test]
fn header_banks_reads_exactly_the_banks_field_of_a_checked_header() {
    for banks in [
        0u8,
        BANK_BODY_USED,
        BANK_HEADING_USED | BANK_BODY_USED | 0b1100,
    ] {
        let k = LayoutKey { banks, ..key() };
        let hdr = encode_header(&k, 3);
        assert_eq!(header_banks(&hdr), Ok(banks));
    }
    let mut bad = encode_header(&key(), 3);
    bad[8 + 40] ^= 1;
    assert_eq!(header_banks(&bad), Err(Reject::HeaderChecksum));
    assert_eq!(header_banks(&bad[..20]), Err(Reject::Truncated));
}
