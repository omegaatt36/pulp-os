# T7 over-buffer inseparable group: behavioral RED

One focused independent test was added in `host/tests/cjk_long_group.rs`.
It uses read-only frozen `cjk_support` installed hand-packs and actual Rig.
No evolving implementation was inspected; no earlier tests, support, harness,
contract, production files or commits were changed. The implementation worker
was notified to hold this error-path change before RED and released afterward.

Separate controller policy: when an inseparable punctuation group exceeds the
documented8192-byte raw page buffer, publish recoverable `BufferTooSmall`
instead of partial Ready/false EOF or a forbidden line-head split. This is the
bounded resource policy for text preservation/progress; the original frozen
T7 contract/hash is unchanged.

Fixture: `「` followed by2738 `」` scalars and final `臺` (8220 UTF-8 bytes).
Opening brackets cannot end a line and closing brackets cannot begin a line.
The test installs valid hand-packs, selects index0 and width34, then requires
`Phase::Error` with `Some(ErrorKind::BufferTooSmall)`. Expectations are the
controller's explicit literal policy, independent of production output.

Actual command on2026-10-06:

```sh
cargo test -p pulp-host --target aarch64-apple-darwin \
  --config 'unstable.build-std=["std","test"]' \
  --test cjk_long_group -- --test-threads=1
```

Compiled without warnings. Exit101:0 passed/1 failed/0 ignored. At line18,
actual phase was `Ready`, expected `Error`. The error-kind assertion did not
execute. `rustfmt --edition 2024 --check host/tests/cjk_long_group.rs` exits0.

Test SHA-256:
`6b3a4550bb79d9ed4226ef321d1361dc1bc75db7881d71054ca1bd8000de8b11`.
Manifest: `tmp-briefs/t7-long-group-tests.sha256`.
Immutable snapshot: `/private/tmp/cjk-snap/T7-long-group-tests` (test, report,
manifest). Independent review and execution after implementation remain
required. This handoff establishes behavioral RED, not GREEN.
