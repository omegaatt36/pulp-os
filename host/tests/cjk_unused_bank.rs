mod cjk_support;

use cjk_support::{BOOK, card, install, pack};
use pulp_host::reader::{Phase, Rig};
use pulp_host::render::{render_full, render_stitched};

const LATIN: &[u8] = b"A lighthouse keeper records the arrivals.\nShips return at dawn.";
// A present but truncated PFNT header: unused optional banks cannot affect Latin.
const BROKEN_PACK: &[u8] = b"PFNT\x01\x00";

fn latin_rig() -> Rig {
    let mut r = Rig::new(card(LATIN, false));
    r.configure(0, 0);
    r
}

fn assert_no_pack_reads(r: &Rig) {
    let reads = r.storage().read_log();
    assert!(
        reads.iter().all(|read| !read.path.starts_with("_PULP/FONTS/")),
        "Latin needs no fallback pack reads: {reads:?}"
    );
}

fn assert_original_latin_with_unused_packs(packs: &[(u16, Vec<u8>)]) {
    let mut original = latin_rig();
    original.open(BOOK);
    assert_eq!(original.phase(), Phase::Ready);
    original.prepare_render();
    let original_lines = original.lines();
    let original_frame = render_full(&|s| original.draw(s)).frame.to_pbm();

    let mut r = latin_rig();
    for (px, bytes) in packs {
        install(r.storage(), *px, bytes);
    }
    r.storage().reset_reads();
    r.open(BOOK);
    assert_eq!(r.phase(), Phase::Ready, "unused optional banks cannot prevent Latin reading");
    assert_no_pack_reads(&r);
    assert_eq!(r.lines(), original_lines, "original Latin text and wrapping survive");
    r.storage().reset_reads();
    r.prepare_render();
    assert_no_pack_reads(&r);
    r.storage().reset_reads();
    let frame = render_full(&|s| r.draw(s)).frame.to_pbm();
    assert_eq!(frame, original_frame, "original Latin glyph pixels survive");
    assert_eq!(frame, render_stitched(&|s| r.draw(s)).frame.to_pbm());
    assert_eq!(r.storage().read_count(), 0, "draw performs no storage reads");
}

#[test]
fn latin_ignores_a_corrupt_unused_body_pack() {
    assert_original_latin_with_unused_packs(&[(16, BROKEN_PACK.to_vec())]);
}

#[test]
fn latin_ignores_a_corrupt_unused_heading_pack() {
    assert_original_latin_with_unused_packs(&[(23, BROKEN_PACK.to_vec())]);
}

#[test]
fn latin_does_not_read_or_render_unused_valid_fallback_banks() {
    // These packs contain a literal A bitmap unlike the original Latin glyph.
    assert_original_latin_with_unused_packs(&[(16, pack(16, false)), (23, pack(23, false))]);
}
