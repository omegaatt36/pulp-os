# T7 nested heading styles: behavioral RED

One focused independent test was added in `host/tests/cjk_nested_styles.rs`.
Read-only `cjk_support` supplies independently assembled pack bytes and literal
fingerprint rows. No evolving production implementation was inspected. No
existing tests, harness, support, contracts, production files or commits were
changed. The implementation worker was notified to hold nested rendering edits
before both RED runs, then notified that RED was captured.

Controller policy: bold/italic markers inside a heading do not end that heading.
Heading size takes precedence until explicit `01 h`. At index0 the three CJK
scalars in `01H臺01B灣01b01I𠮷01i01h臺` require regular23px heading fallback,
then the final scalar requires regular16px body fallback. Expectations derive
from correct role sizes and existing marker semantics; literal horizontal
positions use the hand-pack24px heading advances. No production metrics or
output define expected CJK pixels. No spec IDs are placed in Rust.

The test requires exact one-line visible text,23px `臺`/`灣`/`𠮷` fingerprints at
x8/32/56,16px final `臺` at x80, successful rendering, full/stitched pixel parity
and zero SD reads during every draw. These address consistent role sizing and
regular CJK fallback under nested emphasis without adding a new matrix.

Actual command on2026-10-06:

```sh
cargo test -p pulp-host --target aarch64-apple-darwin \
  --config 'unstable.build-std=["std","test"]' \
  --test cjk_nested_styles -- --test-threads=1
```

Final formatted file compiled without warnings. Exit101:0 passed/1 failed/
0 ignored. The exact visible text assertion passed, then rendering inside the
first fingerprint assertion panicked at `src/fonts/cjk.rs:385` with
`visible font glyph prepared`. Pixel assertions, strip parity and zero-read
assertion did not complete. An earlier pre-format run failed identically.
`rustfmt --edition 2024 --check host/tests/cjk_nested_styles.rs` exits0.

Test SHA-256:
`3ec1dfb17086b17b763a1631ff80693cb69d80f9793a943b00f84b885437b59c`.
Manifest: `tmp-briefs/t7-nested-tests.sha256`.
Immutable snapshot: `/private/tmp/cjk-snap/T7-nested-tests` (test, report,
manifest). Independent review and execution after implementation remain
required. This handoff establishes behavioral RED, not GREEN.
