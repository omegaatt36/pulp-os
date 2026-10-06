# Unused fallback bank independent test review

Spec verdict: PASS. Quality/evidence verdict: APPROVE. No findings.

Reviewed `host/tests/cjk_unused_bank.rs`, its frozen diff/evidence report, the preserved Latin requirement in the original layout-identity contract, existing literal pack support, host forwarding methods, and the actual saved RED log. No cargo runs or source/test modifications performed.

Frozen SHA256 verified: `72d094e6b9b2acb6b04d492af20d7236440206aee4321a9bbc2185cb3bc64b25`. The live file matches the reported frozen content. The amendment adds only this test file; it uses existing support and forwarding interfaces. Other pre-existing workspace changes to the host rig/support are outside this amendment.

The requirement oracle is independent of the repair: literal English source and literal truncated PFNT bytes establish the unused-bank cases. Separate corrupt body16 and heading23 tests assert that an optional unused resource cannot change Ready to Error. The valid-bank case rejects unnecessary FONT reads even when parsing succeeds. Existing hand-built valid banks contain a deliberately different Latin A bitmap, so the paired no-pack control checks that installed fallback content cannot replace original Latin text/wrapping or glyph pixels. This observational control is appropriate for unchanged Latin behavior; it does not calculate new pagination expectations from the repair.

Real call paths verified: the host app registry includes production `src/apps/reader/mod.rs`; `Rig::open` calls actual `on_enter` and normal background progression; `prepare_render` and `draw` forward to actual app methods. No test-only pagination or font selection path is introduced. Read logs are reset before opening, preparation and drawing separately, so each phase is observed without fixture-install noise. Whole-frame equality and full/stitched equality exercise actual rendering; zero drawing reads strengthens the existing storage-free drawing constraint.

The actual `/private/tmp/unused-bank-red.log` matches the report: 0 passed, 3 failed. Both corrupt-bank cases fail Ready availability; the valid-bank case fails because opening reads both PFNT headers. These are runtime behavioral failures, not build failures. They establish RED before repair for all three independent cases. Subsequent line/frame/preparation checks have not executed past those failures and still require GREEN.

Scope limit: these cases cover opening/preparing/drawing a short pure Latin TXT at size 0. They do not independently test live-size repagination, which remains required and covered by the separate English anchor amendment. Avoiding unused CJK work does not revoke the approved all-text invalidation policy.

## Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 0 | pass |
| HIGH | 0 | pass |
| MEDIUM | 0 | pass |
| LOW | 0 | pass |

Verdict: APPROVE — frozen tests and actual behavioral RED are sound. GREEN remains required before claiming production acceptance.
