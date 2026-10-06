# Unused fallback bank acceptance evidence

Owned new file: host/tests/cjk_unused_bank.rs. Production and support read-only. No production bodies read. No existing test edits, mutants, reference implementation, assertion weakening, skips, tolerances, or commits.

Frozen SHA256: 72d094e6b9b2acb6b04d492af20d7236440206aee4321a9bbc2185cb3bc64b25
Start state: new file absent. Frozen diff: unused-bank-tests.diff.
Observed production SHA256 after run:
- src/apps/reader/mod.rs: b6ef27f68655778a35183500939ca6adfbf5056a0a350df732fc3c365aa8389a
- src/apps/reader/paging.rs: c34bf930c9f82985c7911fa9dccf90e5aeb02deff6c30731bb3f2d5921144c87
- src/fonts/cjk.rs: 42f2be4e24026be1de6071ededc6462bc6c770a865e9be646ed4a4fdc8db20dc

Requirement: existing pure Latin availability, glyph rendering, and absence of unnecessary CJK fallback work remain unchanged. The approved all-text size/layout invalidation policy does not authorize consulting unused fallback banks for pure Latin text.

Fixtures: literal English TXT; literal six-byte truncated PFNT header. Install only corrupt body16, only corrupt heading23, or both valid existing cjk_support literal banks. Existing valid banks contain a literal A bitmap different from the original Latin A; the Latin first-glyph preference must stay original.

Oracle: Ready availability; original no-pack Latin lines and frame as an observational control for unchanged existing glyph behavior, not an expected new-pagination algorithm; no FONT read records through opening/preparation; zero storage reads during drawing; exact full/stitched frame equality. No spec IDs appear in test names/comments/messages. Three separate tests ensure corrupt body, corrupt heading and unnecessary successful reads all receive actual RED evidence. The control frame is not a fixed page number or derived reference layout.

Actual current-production RED command:
```
cargo test --offline -p pulp-host --target aarch64-apple-darwin --config 'unstable.build-std=["std","test"]' --test cjk_unused_bank > /private/tmp/unused-bank-red.log 2>&1
```
Verified rustc host: aarch64-apple-darwin; rustc 1.100.0-nightly (1303417c4 2026-09-21). Exit 101; 0 passed, 3 failed. Corrupt unused body and heading each change Ready into Error. Valid unused banks read both headers despite pure Latin requiring no fallback.

Full command output:
```
   Compiling pulp-host v0.1.0 (/Users/raiven_kao/dev/pulp-os/host)
    Finished `test` profile [optimized + debuginfo] target(s) in 0.72s
     Running tests/cjk_unused_bank.rs (target/aarch64-apple-darwin/debug/build/pulp-host/a43ab1e35b1da00c/out/cjk_unused_bank-a43ab1e35b1da00c)

running 3 tests
test latin_ignores_a_corrupt_unused_heading_pack ... FAILED
test latin_does_not_read_or_render_unused_valid_fallback_banks ... FAILED
test latin_ignores_a_corrupt_unused_body_pack ... FAILED

failures:

---- latin_ignores_a_corrupt_unused_heading_pack stdout ----

thread 'latin_ignores_a_corrupt_unused_heading_pack' (499037) panicked at host/tests/cjk_unused_bank.rs:39:5:
assertion `left == right` failed: unused optional banks cannot prevent Latin reading
  left: Error
 right: Ready
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

---- latin_does_not_read_or_render_unused_valid_fallback_banks stdout ----

thread 'latin_does_not_read_or_render_unused_valid_fallback_banks' (499035) panicked at host/tests/cjk_unused_bank.rs:19:5:
Latin needs no fallback pack reads: [ReadRecord { path: "CJK.TXT", offset: 0, requested: 8192, returned: 63, outcome: Ok }, ReadRecord { path: "_PULP/FONTS/F00016.PFN", offset: 0, requested: 44, returned: 44, outcome: Ok }, ReadRecord { path: "_PULP/FONTS/F00023.PFN", offset: 0, requested: 44, returned: 44, outcome: Ok }]

---- latin_ignores_a_corrupt_unused_body_pack stdout ----

thread 'latin_ignores_a_corrupt_unused_body_pack' (499036) panicked at host/tests/cjk_unused_bank.rs:39:5:
assertion `left == right` failed: unused optional banks cannot prevent Latin reading
  left: Error
 right: Ready


failures:
    latin_does_not_read_or_render_unused_valid_fallback_banks
    latin_ignores_a_corrupt_unused_body_pack
    latin_ignores_a_corrupt_unused_heading_pack

test result: FAILED. 0 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: test failed, to rerun pass `-p pulp-host --test cjk_unused_bank`
```

GREEN pending production repair. Test frozen after actual behavioral RED. Cargo slot released.
