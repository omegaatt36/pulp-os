use crate::cjk_support;

use cjk_support::{BOOK, card, path};
use pulp_host::ErrorKind;
use pulp_host::reader::{Phase, Rig};
use pulp_host::render::{render_full, render_stitched};
use pulp_host::storage::StorageOp;

#[test]
fn installed_pack_metadata_open_failure_remains_a_recoverable_error() {
    let storage = card("臺".as_bytes(), true);
    storage.inject_error(StorageOp::FileSize, &path(23), 1, ErrorKind::OpenFile);
    let mut r = Rig::new(storage);
    r.configure(2, 0);
    r.open(BOOK);
    assert_eq!(
        r.storage().pending_injections(),
        0,
        "metadata failure was exercised"
    );
    assert_eq!(
        r.phase(),
        Phase::Error,
        "failure to open an installed pack is not definitive optional-pack absence"
    );
    assert!(
        r.error_kind().is_some(),
        "recoverable failure retains a cause"
    );
    r.storage().reset_reads();
    let frame = render_full(&|s| r.draw(s)).frame.to_pbm();
    assert_eq!(frame, render_stitched(&|s| r.draw(s)).frame.to_pbm());
    assert_eq!(
        r.storage().read_count(),
        0,
        "error draw performs no source reads"
    );
}
