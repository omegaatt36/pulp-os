# T8 test-author report

Authored seven focused tests in host/tests/cjk_surfaces.rs and the approved
surface/lifecycle contract in t8-contract.md. Applied test-driven-development
and verification-before-completion skill instructions. No forthcoming T7
implementation was inspected to derive expected behavior. Existing production
widget/reader interfaces and integration-map.md supplied integration context.

No production source, frozen T7 tests/support/harness, mutants, reference
algorithm, exhaustive sweep or commits were changed by the T8 test author.
The cjk_support fixture SHA-256 matches the immutable T7 snapshot exactly:
c54f7693bfaec43d1c621ac935f79993b43b91c00e6c1b7ed034bb33a81d7393.

## Actual RED evidence

Command, 2026-10-06:

```sh
cargo test -p pulp-host --target aarch64-apple-darwin \
  --config 'unstable.build-std=["std","test"]' \
  --test cjk_surfaces -- --test-threads=1
```

An initial six-test version used only existing APIs, before adding the approved
pre-render hook and seventh prepared-widget test. It compiled and ran: exit101,
0 passed / 6 failed / 0 ignored. Two interim unused-import warnings corresponded
to the not-yet-authored seventh widget test and disappeared in the final file.
This run used the concurrently changing T7 state; it is not claimed as a pure
pre-T7 firmware baseline.

| Test | Actual initial failure |
| --- | --- |
| book_title_uses_16px_chrome_independently_of_body_size_and_draw_reads_nothing | Literal16px 臺 fingerprint missing at x8 in header band. |
| visible_toc_uses_body_size_regular_fallback_and_selected_foreground | Literal16px inverse 臺 fingerprint missing at x8 in first TOC row. |
| toc_scroll_prepares_newly_visible_glyph_before_the_next_draw | Visible 臺 TOC bitmap was never read. |
| missing_title_pack_reports_recoverable_failure_and_reopen_retries | Initial Error/cause and reopen Ready assertions passed; recovered title fingerprint still missing. |
| dynamic_label_set_text_keeps_the_longest_complete_scalar_prefix | Actual empty string; expected A臺. |
| dynamic_label_write_str_keeps_existing_text_and_a_complete_scalar_prefix | Actual empty string; expected A臺. |

The final seven-test version calls the controller-approved Rig::prepare_render
after opening/reopening and after every TOC visibility event, and uses approved
BitmapLabel/BitmapDynLabel::draw_prepared. A first compile caught an unrelated
VirtualStorage::clone assumption; the test now observes the existing
Kernel::sd().card instead. That test-author defect is corrected.

Final RED rerun: exit101 with exactly nine E0599 missing-method errors: five
references to Rig::prepare_render and four to widget draw_prepared. No warnings.
CjkState::new/prepare_text/view and the direct Kernel::handle setup compile.
These absent APIs are intentional interface RED, not runtime behavior evidence
for the seventh test. No production or frozen host seam was edited to add them.

rustfmt --edition2024 --check host/tests/cjk_surfaces.rs succeeds. A search for
R-number/REQ-/SPEC- markers in the Rust test returns no matches.

Assertions after earlier failing assertions have not all executed. Controller
must rerun all seven tests after implementation and inspect the omitted UI and
scheduler surfaces listed in t8-contract.md. T7's heading/style cases remain the
separate heading evidence; the host does not execute Files/Home/manager or real
schedulers.

## Freeze and review

Immutable snapshot: /private/tmp/cjk-snap/T8-tests (also available through
/tmp/cjk-snap/T8-tests). Includes the new test, read-only shared T7 support,
t8-contract.md and SHA256SUMS. Files are0444; directories are0555. Actual
byte-for-byte comparison confirmed the repository files match this snapshot.

Repository manifest: tmp-briefs/t8-tests.sha256.
Manifest SHA-256:
80a0182228115a22cb5e02dfaf705b141cc95a6771018cea07c99ce066b51fc2.

Independent code review remains pending at handoff. The author released the
agent slot so the controller can run the required code-reviewer. Any resulting
test corrections must be an explicit documented amendment compared with this
pre-T8 implementation baseline. Controller must verify frozen test expectations
byte-for-byte before accepting GREEN and review the separately authorized
Rig::prepare_render forwarding addition against frozen T7 harness evidence.

The test author claims tests-only RED, not production GREEN.
