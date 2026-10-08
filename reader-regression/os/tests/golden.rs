// The golden trace must reproduce the sha256 produced by the pre-port X4 commit
// (3bb911af): pagination, navigation, settings text, bookmark bytes and rendered
// pixels of English TXT/EPUB are unchanged since before the C61 port.
use sha2::{Digest, Sha256};

const PINNED_SHA256: &str = "8cb6e31a70a32cdc05d30fab45ee0c7983da8cde6a55ba8bb45a0540f8d3f454";

#[test]
fn golden_trace_matches_the_pre_port_pin() {
    let trace = pulp_os_host::golden::trace();
    let sha: String = Sha256::digest(trace.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(sha, PINNED_SHA256, "golden trace changed ({} lines)", trace.lines().count());
}
