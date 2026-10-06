mod cjk_support;

use cjk_support::{assert_patch, rig, rows, text_lines};
use pulp_host::render::{render_full, render_stitched};

#[test]
fn nested_bold_and_italic_keep_heading_fallback_until_the_heading_closes() {
    let mut text = vec![1, b'H'];
    text.extend_from_slice("臺".as_bytes());
    text.extend_from_slice(&[1, b'B']);
    text.extend_from_slice("灣".as_bytes());
    text.extend_from_slice(&[1, b'b', 1, b'I']);
    text.extend_from_slice("𠮷".as_bytes());
    text.extend_from_slice(&[1, b'i', 1, b'h']);
    text.extend_from_slice("臺".as_bytes());
    let r = rig(&text, 0, 4 * 24);
    assert_eq!(text_lines(&r), ["臺灣𠮷臺"]);
    r.storage().reset_reads();
    // Nested emphasis cannot end a heading: its three advances remain24px.
    assert_patch(&r, r.text_margin(), 8, &rows(23, '臺'), 3);
    assert_patch(&r, r.text_margin() + 24, 8, &rows(23, '灣'), 3);
    assert_patch(&r, r.text_margin() + 48, 8, &rows(23, '𠮷'), 3);
    // Only the explicit closing heading marker returns to the16px body pack.
    assert_patch(&r, r.text_margin() + 72, 8, &rows(16, '臺'), 3);
    let full = render_full(&|s| r.draw(s)).frame.to_pbm();
    assert_eq!(full, render_stitched(&|s| r.draw(s)).frame.to_pbm());
    assert_eq!(
        r.storage().read_count(),
        0,
        "draw uses prepared glyphs only"
    );
}
