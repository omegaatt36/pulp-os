// tests/goldens/check.rs — shared golden-trace lock for the X4 harness tests:
// byte-exact comparison against a committed file, plus UPDATE_GOLDEN=1 refresh.

/// Compare `actual_lines` against `golden` (include_str! of the committed
/// file, `path` relative to the harness crate). On mismatch, panic with the
/// first differing line. With UPDATE_GOLDEN=1 the actual output is written
/// over the golden and the test panics so the diff is reviewed deliberately.
pub fn check(path: &str, golden: &str, actual_lines: &[String]) {
    let actual = if actual_lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", actual_lines.join("\n"))
    };
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        let full = format!("{}/{path}", env!("CARGO_MANIFEST_DIR"));
        std::fs::write(&full, &actual).unwrap_or_else(|e| panic!("cannot write {full}: {e}"));
        panic!("golden updated; review the diff");
    }
    let golden_lines: Vec<&str> = golden.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();
    for i in 0..golden_lines.len().max(actual_lines.len()) {
        let g = golden_lines.get(i).copied();
        let a = actual_lines.get(i).copied();
        if g == a {
            continue;
        }
        panic!(
            "{path}:{}: trace changed\n  golden: {:?}\n  actual: {:?}\n  \
             (an intentional change refreshes the golden: UPDATE_GOLDEN=1 \
             cargo test -p board-harness, then review the diff)",
            i + 1,
            g.unwrap_or("(end of file)"),
            a.unwrap_or("(end of file)"),
        );
    }
}
