# Optional no-pack policy amendment: RED evidence

Controller explicitly changed absent-pack policy to preserve UTF-8 compatibility:
an uninstalled optional pack yields Ready text with a synthetic size-specific
missing box. An installed corrupt pack or storage read failure still yields a
recoverable error. This report documents a behavior-policy amendment; original
T7/T8 frozen snapshots and historical manifests were preserved.

## Exact changes

`cjk_reader.rs`: removed only the absent case from the failure table; its corrupt
and read-failure assertions are unchanged. Relocated absent coverage into one
new test requiring Ready, conserved `臺𠮷` text, literal23px17x17 boxes at x+3
and x+26, zero draw reads, then real hand-pack fingerprints after install/reopen.
The failure table uses installed packs for both remaining cases.

`cjk_surfaces.rs`: renamed only the absent-title test, requiring Ready and a
literal16px12x12 box at x10 in the header, zero draw reads, then the existing
real16px title fingerprint after install/reopen. Actual byte comparison with
the original T8 snapshot confirms the other six tests and shared setup remain
unchanged. Frozen `cjk_support` was not edited.

Both contracts have appended no-pack policy addenda, explicitly superseding
their historical absent-pack Error clauses. T8 also retains the previously
authorized AppLayer signature correction. The old snapshots/manifests are not
rewritten. No evolving implementation was inspected or edited, no mutants,
skips, reference algorithm or commits. The implementation worker was notified
to hold no-pack policy edits before RED and released after capture.

## Literal box expectations

T3 contract section3 defines the synthetic fallback independently of provider
output:23px gives side17/advance23/offset_x3;16px gives side12/advance16/
offset_x2. Tests use explicit hollow MSB-first bitmap rows. They do not call
production missing-glyph helpers to construct expected output.

## Actual RED on2026-10-06

```sh
cargo test -p pulp-host --target aarch64-apple-darwin \
  --config 'unstable.build-std=["std","test"]' \
  --test cjk_reader \
  uninstalled_pack_keeps_ready_text_with_size_specific_boxes_until_install_and_reopen \
  -- --test-threads=1
```

Compiled without warnings. Exit101:0 passed/1 failed/8 filtered out. At
`cjk_reader.rs:205`, actual phase Error versus expected Ready. Box, zero-read
and install/reopen assertions were not reached.

```sh
cargo test -p pulp-host --target aarch64-apple-darwin \
  --config 'unstable.build-std=["std","test"]' \
  --test cjk_surfaces \
  uninstalled_title_pack_draws_a_size_specific_box_and_reopen_uses_installed_pack \
  -- --test-threads=1
```

Exit101 with exactly9 E0599 errors:5 missing Rig::prepare_render references
and4 missing widget draw_prepared references. This is existing approved
interface RED, not runtime title-policy evidence. No assertion was executed.

`rustfmt --edition 2024 --check host/tests/cjk_reader.rs host/tests/cjk_surfaces.rs`
exits0. No GREEN claim is made; focused missing-pack tests and unchanged
corrupt/read cases must execute after implementation, alongside full suites.

## Freeze

Amended test/contract hashes: `tmp-briefs/no-pack-policy-tests.sha256`.
Immutable snapshot: `/private/tmp/cjk-snap/no-pack-policy-tests`, containing
amended tests/contracts, this report and the new manifest. Independent review
remains required before accepting this amendment.
