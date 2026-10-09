mod cjk_support;

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

use cjk_support::*;
use pulp_host::ErrorKind;
use pulp_host::reader::{Phase, Rig};
use pulp_host::storage::StorageOp;

thread_local! {
    static LINES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

struct Capture;
impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        LINES.with(|lines| lines.borrow_mut().push(record.args().to_string()));
    }
    fn flush(&self) {}
}
static CAPTURE: Capture = Capture;
static CLOCK: AtomicU64 = AtomicU64::new(0);
fn paced_now() -> u64 {
    CLOCK.fetch_add(200_000, Ordering::Relaxed)
}

fn take_lines() -> Vec<String> {
    LINES.with(|lines| std::mem::take(&mut *lines.borrow_mut()))
}

#[cfg(feature = "sd-metrics")]
fn field(line: &str, key: &str) -> u64 {
    line.split_whitespace()
        .find_map(|s| s.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("missing {key}: {line}"))
        .parse()
        .unwrap()
}

#[test]
fn reader_profiles_are_gated_and_account_for_success_failure_and_resumption() {
    log::set_logger(&CAPTURE).unwrap();
    log::set_max_level(log::LevelFilter::Info);
    let mut r = Rig::new(card(SAMPLE.as_bytes(), true));
    r.open(BOOK);
    assert_eq!(r.phase(), Phase::Ready);
    let bitmap_start = 44 + 22 * alphabet().len() as u32;
    let bitmap_read = r
        .storage()
        .read_log()
        .iter()
        .filter(|read| read.path == path(16))
        .position(|read| read.offset >= bitmap_start)
        .unwrap()
        + 1;
    let success = take_lines();
    r.idle(10);
    assert!(
        take_lines()
            .iter()
            .all(|line| !line.starts_with("reader-sd")),
        "settled TXT ticks must not emit preparation profiles"
    );

    let mut r = Rig::new(card(SAMPLE.as_bytes(), true));
    install(r.storage(), 16, b"bad pack");
    r.open(BOOK);
    assert_eq!(r.phase(), Phase::Error);
    let failure = take_lines();

    let c = card(SAMPLE.as_bytes(), true);
    c.inject_error(StorageOp::Read, BOOK, 1, ErrorKind::ReadFailed);
    let mut r = Rig::new(c);
    r.open(BOOK);
    assert_eq!(r.phase(), Phase::Error);
    let text_failure = take_lines();

    let c = card(SAMPLE.as_bytes(), true);
    c.inject_error(
        StorageOp::Read,
        &path(16),
        bitmap_read,
        ErrorKind::ReadFailed,
    );
    let mut r = Rig::new(c);
    r.open(BOOK);
    assert_eq!(r.phase(), Phase::Error);
    assert_eq!(r.storage().pending_injections(), 0);
    let visible_failure = take_lines();

    let mut r = Rig::new(card(SAMPLE.as_bytes(), true));
    r.set_cjk_pace(paced_now, 500_000);
    r.open(BOOK);
    assert_eq!(r.phase(), Phase::Ready);
    let sliced = take_lines();

    #[cfg(not(feature = "sd-metrics"))]
    for line in success
        .iter()
        .chain(&failure)
        .chain(&text_failure)
        .chain(&visible_failure)
        .chain(&sliced)
    {
        assert!(!line.starts_with("reader-sd"));
        assert!(!line.starts_with("cjk-sd"));
    }

    #[cfg(feature = "sd-metrics")]
    {
        assert!(
            success
                .iter()
                .any(|s| s.starts_with("reader-sd step=") && s.contains("outcome=ready"))
        );
        assert!(
            failure
                .iter()
                .any(|s| s.starts_with("reader-sd step=") && s.contains("outcome=error"))
        );
        assert!(
            sliced
                .iter()
                .any(|s| s.starts_with("reader-sd step=") && s.contains("outcome=slice"))
        );
        assert!(sliced.iter().any(|s| s.starts_with("reader-sd resume")
            && s.contains("resumed=true")
            && s.contains("retained_window=true")));
        for (lines, layout_calls, visible_calls) in [
            (&success, 1, 1),
            (&failure, 1, 0),
            (&text_failure, 0, 0),
            (&visible_failure, 1, 1),
        ] {
            let step = lines
                .iter()
                .find(|s| s.starts_with("reader-sd step="))
                .unwrap();
            assert!(field(step, "text_calls") > 0);
            assert_eq!(field(step, "layout_calls"), layout_calls, "{step}");
            assert_eq!(field(step, "visible_calls"), visible_calls, "{step}");
        }
        for line in success
            .iter()
            .chain(&failure)
            .chain(&text_failure)
            .chain(&visible_failure)
            .chain(&sliced)
            .filter(|s| s.starts_with("reader-sd step="))
        {
            assert_eq!(
                field(line, "wall_us"),
                [
                    "text_us",
                    "layout_us",
                    "visible_us",
                    "pause_us",
                    "logging_us",
                    "other_us"
                ]
                .iter()
                .map(|key| field(line, key))
                .sum::<u64>(),
                "{line}"
            );
        }
    }
}
