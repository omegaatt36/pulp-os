# T7 heading continuation test: behavioral RED

One focused independent test was added in `host/tests/cjk_heading_pages.rs`.
No evolving T7 implementation was inspected. Existing frozen T7 tests supplied
marker semantics and public Rig/navigation/bookmark observations; read-only
`cjk_support` supplies independently assembled pack bytes and literal rows.
No existing test, support, harness, production file or original contract was
changed by this test-author task. No mutants, reference algorithm or commits.

## Separate contract addendum

An opening `01 H` heading marker remains active until its corresponding `01 h`
closing marker, including across page windows, backward navigation and restored
positions. At body index0, a continued heading still uses heading23px metrics
and glyphs rather than body16px. This is the existing heading meaning combined
with R5 size consistency and R11 navigation/restore position consistency; it
does not revise the frozen original T7 contract or its hash.

## Assertions and observed RED

The fixture opens a heading, contains1000 `臺` scalars, closes the heading and
then contains Latin body text. Width48 accommodates two literal24px heading
advances. The test confirms page2 starts after the opening marker and before
the closing marker, and all its visible text remains nonempty `臺`. It then
requires the literal23px bitmap at the first glyph position. Subsequent checks
cover a third heading page, backward/forward identical page bytes/offsets and
bookmark reboot with the same bytes/offset and23px fingerprint.

Actual command on2026-10-06:

```sh
cargo test -p pulp-host --target aarch64-apple-darwin \
  --config 'unstable.build-std=["std","test"]' \
  --test cjk_heading_pages -- --test-threads=1
```

Compiled without warnings. Exit101:0 passed/1 failed/0 ignored. Failure was
the first page2 heading fingerprint assertion: `literal 8x3 glyph missing at
x=8 in line 0` (`cjk_support/mod.rs:140`). Page2 readiness, page index, raw
heading-span offset and visible scalar assertions all executed and passed.
Later navigation, third-page and bookmark checks have not executed yet.
The current concurrent production state was used; no pure historical baseline
or GREEN claim is made.

`rustfmt --edition 2024 --check host/tests/cjk_heading_pages.rs` exits0.

## Freeze

Test SHA-256:
`d3add6b8c7d34088a5ebde8ad9fd1aed119c992f1092513d129ddee9bf68c260`.
Manifest: `tmp-briefs/t7-heading-tests.sha256`.
Immutable snapshot: `/private/tmp/cjk-snap/T7-heading-tests` (test, report,
manifest). Independent review and full execution after implementation remain
required; this handoff establishes tests-only behavioral RED.
