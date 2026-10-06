# Chapter identity regression RED

Independent test author added only `host/tests/cjk_chapter_identity.rs`. Production implementation bodies were not read. Public fixture data types, existing fixture usage, cjk_support literal packs, and Rig public method names supplied the harness. No production code or existing tests were changed.

The requirement-derived oracle is that forward chapter navigation enters chapter 1 at page 0, regardless of the raw anchor in chapter 0 when the active same-size pack changes. The test first reaches chapter 0 page 3 with a positive raw offset, installs a literal replacement 16px PFNT with a different font ID and 34px advances, then triggers the real next-chapter quick action. Remaining assertions check scalar conservation and the normal quick-action/ordinary backward destination semantics after the boundary is fixed.

Frozen SHA-256:

```text
5b921d2feb70f788e58f2b6adb412fc78587490d0ce84bba444a6791010f89fb  host/tests/cjk_chapter_identity.rs
```

Replay command:

```sh
cargo test -p pulp-host --target aarch64-apple-darwin --config 'unstable.build-std=["std","test"]' --test cjk_chapter_identity -- --nocapture
```

Actual RED after formatting (exit 101):

```text
thread 'replacing_pack_during_chapter_jump_does_not_reuse_the_departed_chapter_anchor' panicked at host/tests/cjk_chapter_identity.rs:66:5:
assertion `left == right` failed: forward chapter navigation starts the selected chapter, without the old raw anchor
  left: (1, 6)
 right: (1, 0)
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
```

This is an assertion failure in the intended behavior, not a compilation or fixture failure. The chapter 0 nonzero-page setup passed. No implementation fix was made before either RED run. The test is frozen for implementation.
