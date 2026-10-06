# T7 metadata failure RED and UTF-8 oracle amendment

Tests/reports only. No provider implementation was inspected or edited, no
mutants, no full-suite run. The worker held metadata-policy edits before RED
capture and was released afterward. Existing frozen snapshots/manifests remain
historical evidence; this amendment has its own manifest/snapshot.

## Compatibility oracle and weakening gate

Only `utf8_common::line_ink` advance expectations changed. Unsupported valid
scalars use literal body advances16/19/23/28/35 from the approved optional-pack
synthetic fallback policy/T3 metrics. Available native glyphs and U+FFFD retain
existing FontSet table advances. No production CJK output defines expectations.
Actual byte comparison against the saved original helper confirms changes are
confined to the advance oracle/comment: row scans, both ink checks, the +2
tolerance, failure messages and every other helper remain identical. No test
case/assertion/tolerance/skip changed.

Call-site audit: all `line_ink`/`check_drawn`/`drawn_line_matches` clients use
normal proportional `open_book`. Forced monospace is exercised separately by
`open_mono` and is never passed to this ink oracle; its scalar-column premise
and HAS_REGULAR-disabled helper setup remain unchanged. No general production
fallback/HAS_REGULAR behavior is inferred from these proportional draw tests.

Saved original helper:
`/private/tmp/cjk-snap/utf8-helper-before-metadata-amendment.rs`, SHA-256
`1c7991bb886fab1de05c1d1dbe533ad9865bb71443e20e17e61964785c7efcf4`.

Implementer supplied original actual observations (not rerun by this author):
boundary target7/9 passed; `a_line_of_every_length_is_drawn_whole` had `臺`x1,
pad0/font2 inkx35 beyond expected advance27. The other failure,
`drawn_pages_show_every_scalar_at_every_pad_offset`, had19 `臺` inkx449 beyond
expected advance225. Their original combined command used the standard host
flags below with targets utf8_boundary/malformed/measure/title/residue/epub,
and stopped at boundary. Standalone malformed9/12 passed; three draw cases
failed: maximal-subpart mixed text inkx196 beyond advance116; malformed text
fit no1..=1 replacement cells; prefix6bytes inkx60 beyond advance52.

Author reran only affected tests, using:

```sh
cargo test -p pulp-host --target aarch64-apple-darwin \
  --config 'unstable.build-std=["std","test"]' \
  --test utf8_boundary a_line_of_every_length_is_drawn_whole -- --test-threads=1
cargo test -p pulp-host --target aarch64-apple-darwin \
  --config 'unstable.build-std=["std","test"]' \
  --test utf8_boundary drawn_pages_show_every_scalar_at_every_pad_offset -- --test-threads=1
cargo test -p pulp-host --target aarch64-apple-darwin \
  --config 'unstable.build-std=["std","test"]' \
  --test utf8_malformed drawn_ -- --test-threads=1
```

All exit0, without warnings: boundary1/1 +1/1 and malformed3/3 passed.
These are5 affected draw tests, not claims about the filtered cases/full suite.

## Installed pack metadata failure

New `cjk_metadata_failure.rs` installs valid hand-packs and injects a23px
StorageOp::FileSize OpenFile failure. It requires injection consumption,
Phase::Error with a cause, drawable full/stitched parity and zero draw reads.
This keeps genuine metadata failure distinct from definitive optional absence.
The existing no-pack Ready/box test is unchanged. Separate contract:
`t7-metadata-tests-contract.md`.

Actual command on2026-10-06:

```sh
cargo test -p pulp-host --target aarch64-apple-darwin \
  --config 'unstable.build-std=["std","test"]' \
  --test cjk_metadata_failure -- --test-threads=1
```

Compiled without warnings. Exit101:0 passed/1 failed/0 ignored. Injection
consumption passed; at line21 actual Ready versus expected Error. Cause and
draw assertions did not execute. No metadata GREEN claim is made.

T8 contract table was corrected to the already-approved missing-title test name
and Ready/12x12-box expectation; original T8 snapshot remains untouched.
`rustfmt --edition 2024 --check host/tests/utf8_common/mod.rs host/tests/cjk_metadata_failure.rs`
exits0. Manifest: `t7-metadata-tests.sha256`. Immutable snapshot:
`/private/tmp/cjk-snap/T7-metadata-tests` includes amended helper, new test,
metadata contract, corrected T8 contract, report/manifest and original helper.
Independent review remains required before accepting this amendment.
