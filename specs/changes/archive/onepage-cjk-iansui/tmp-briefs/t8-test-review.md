# T8 test review: PASS

No important defects found in the seven frozen tests or their contract.

Reviewed `host/tests/cjk_surfaces.rs`, read-only T7 `cjk_support`, T7/T8
contracts, T8 RED report, the relevant spec requirements and integration map.
Repository test/support/contract SHA-256 values match both `t8-tests.sha256`
and `/private/tmp/cjk-snap/T8-tests`.

The tests exercise actual ReaderApp through Rig and actual static/dynamic
widgets. Fixture pack bytes, CJK advances and size/scalar bitmap fingerprints
are independently defined; no production fallback or layout algorithm supplies
the expected output. Existing Latin font data/output is used only to check
preservation. Title and TOC assertions constrain the intended size, horizontal
advance and foreground in the relevant surface bands. Scroll preparation and
zero draw-read assertions check separate lifecycle obligations. UTF-8 tests
assert complete scalar prefixes and retention of existing appended text.

The approved missing `Rig::prepare_render` and widget `draw_prepared` methods
are intentional interface RED. The report correctly distinguishes initial
runtime failures from final compile RED; neither establishes forthcoming GREEN.
Controller preflight correction: App retains `prepare_render(ctx, k)`, while
AppLayer takes only `prepare_render(k)` because its manager owns the app context.
This interface correction changes no test expectations; the controller will
amend the contract/manifest explicitly while preserving the original snapshot.
No tests were rerun, no production source was inspected, and no frozen test or
support file was edited for this review.

Acceptance still requires all seven tests to execute after implementation and
the explicitly listed source inspection of omitted UI/manager/scheduler paths,
including title preparation while loading/empty. The host tests do not claim
that coverage.

## Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 0 | pass |
| HIGH | 0 | pass |
| MEDIUM | 0 | pass |
| LOW | 0 | pass |

Verdict: APPROVE — tests and contract are suitable to proceed with implementation.
