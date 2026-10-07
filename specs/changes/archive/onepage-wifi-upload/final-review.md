# Final review — 2026-10-07

## Verdict: APPROVE

0 unresolved MUST · 0 unresolved RECALL. Software review only; hardware remains UNVERIFIED.

Branch `onepage`, base `8702340`; changes remain uncommitted. Independent final re-review: `review-evidence/remaining-fixes-review.md`. Prior review retained as `review-evidence/initial-final-review.md`.

## Resolved findings

- Delete body clipping: oversized declared bodies are refused before storage mutation. Independent red proof reproduced destructive 200/Deleted responses; guard fix and re-review passed.
- FAT deletion compatibility: exact printable-ASCII root FAT 8.3 names, including `A(B).TXT`, can be listed/deleted. Paths, malformed names and surrounding whitespace are refused without normalization. Upload sanitation is unchanged: `A(B).TXT` uploads as `AB.TXT`. R6 clarification and proposal resolved-miss supersede the earlier shared-validator mandate.
- Wi-Fi reservation coverage: new behavioral tests cover exact aggregate capacity across multiple classes, PoolExhausted rejection/accounting, release/refill, offline capacity above the Wi-Fi ceiling, and NotInitialised/Degraded/Ready PSRAM states. Production budget logic was already correct; no memory implementation change was needed.

## Fresh verification

| Command | Result |
|---|---|
| `scripts/host-test.sh --locked` | exit 0; 880 passed, 0 failed, 0 ignored |
| `scripts/test-board-logic.sh` | exit 0; 333 passed, 0 failed, 0 ignored |
| `scripts/check-wifi-build.sh` | exit 0; enabled/disabled C61/X4 link, radio optimization and memory guards pass |
| `rustfmt --edition 2024 --check src/apps/upload/http.rs board-logic/tests/wifi_budget.rs host/tests/upload_regression.rs` | exit 0 |
| `git diff --check` | exit 0 |

C61 enabled statics 204,664 B (aligned image range), stack section 51,960 B, headroom above minimum 2,808 B: unchanged by final fixes. Raw final test/build output is retained in `review-evidence/remaining-*.log`.

## Red-proof and weakening gate

- Historical per-task proof/expected-value provenance remains in `progress.md`. Import-only reds prove missing interfaces, not faulty behavioral assertions; those limits remain disclosed.
- Actual final HTTP reds: `review-evidence/delete-red-proof.md` and `fat-name-red-proof.md`. Expectations come from written requirements, existing client behavior and pinned FAT parser, not implementation output.
- Memory tests passed immediately against correct production. Two isolated mutations were both rejected: wrong build variant (162,304 instead of 117,248 B), and missing aggregate enforcement (actual reserve exceeds pool). See `wifi-budget-mutation-report.md` and its raw logs. No fabricated memory implementation red is claimed.
- Independent final test author and production implementer were separated. Existing regression prefix and seven budget tests retained byte-for-byte; implementer test snapshots unchanged. No new skip, removed/loosened assertion, widened tolerance or replacement of existing exercised call paths in these fixes.
- Historical T6 weakening remains disclosed: nine bring_up tests migrated to session coverage; immediate-completion threshold changed 500 ms to 1.5 s. Final fixes do not erase that history.
- Spec-ID hygiene passes for changed/new upload sources, tests and build script. Traceability stays in this change folder.

## Evidence limits

No OnePage hardware was accessed. Association/DHCP, actual radio heap, stack high-water, physical multicast, re-entry and power remain UNVERIFIED, as required by R14. Static budget arithmetic does not prove runtime adequacy.

Inherited G5: real SD close errors are swallowed in existing storage macros; host VirtualStorage does not exercise FAT flush/close. This was explicitly deferred and remains a known limitation of R7/R8 evidence.

## Cherry-pick source

Requested `feat/iansui-cjk-host-foundation` (`445755f`) is already an ancestor of `onepage`; there is no missing commit to cherry-pick. Different branch `improve/memory-cjk-font-build` contains `26febe1`, but the user has not confirmed that replacement source; it has not been applied.
