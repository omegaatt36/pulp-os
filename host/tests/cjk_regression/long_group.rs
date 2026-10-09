use crate::cjk_support;

use cjk_support::{BOOK, card};
use pulp_host::ErrorKind;
use pulp_host::reader::{Phase, Rig};

#[test]
fn inseparable_group_beyond_page_buffer_reports_recoverable_capacity_error() {
    // The documented raw page buffer is8192bytes; none of these punctuation
    // boundaries is legal: opening bracket cannot end a line, closers cannot
    // begin one. The trailing scalar must not become unreachable as false EOF.
    let input = format!("「{}臺", "」".repeat(8192 / 3 + 8));
    assert!(input.len() > 8192);
    let mut r = Rig::new(card(input.as_bytes(), true));
    r.configure(0, 0);
    r.set_text_width(2 * 17);
    r.open(BOOK);
    assert_eq!(
        r.phase(),
        Phase::Error,
        "an inseparable group beyond the bounded buffer cannot publish a partial Ready page"
    );
    assert_eq!(
        r.error_kind(),
        Some(ErrorKind::BufferTooSmall),
        "capacity exhaustion remains a recoverable resource error"
    );
}
