# Final review — 2026-10-07

Review base: `8702340`; branch: `onepage`; implementation remains uncommitted.

## Verdict: BLOCK

1 unresolved MUST · 1 RECALL. Review is not an archive approval.

## Fixed finding

`POST /delete` clipped Content-Length to 13 before validating the body. An invalid body `BOOK.TXT     X` deleted `BOOK.TXT` and returned 200/Deleted. An independent test author reproduced whole/split input and declared-longer early EOF variants before the implementer changed production. The guard now rejects an oversized declared body before storage mutation. See `review-evidence/delete-red-proof.md` for the actual failing output and requirement-derived expectations. Green: `scripts/host-test.sh --test upload_regression` (18 passed), `scripts/host-test.sh --test upload_http` (24 passed). Fresh supporting suites: connect 9, mDNS 58, session 20, all passed. Re-review confirms the guard precedes storage mutation and existing test assertions are unchanged. Final `scripts/check-wifi-build.sh` exits 0, with statics 204,664 B / stack 51,960 B unchanged; see `review-evidence/final-build.log`. Detailed fix and re-review reports are retained alongside it.

## MUST — valid existing FAT filenames cannot be deleted

`src/apps/upload/http.rs` uses upload sanitizer equality to validate deletion. `A(B).TXT` is accepted by the pinned embedded-sdmmc FAT parser, listed by the firmware and sent by the browser Delete button, but the new guard returns 500. Before this change it could be deleted. This contradicts R6's existing delete behavior.

The T6 implementer brief also mandates a shared upload/delete validator. That mandate conflicts with existing upload sanitation's narrower character set. A user decision is pending: preserve exact FAT 8.3 deletion while retaining current upload sanitation, or explicitly narrow compatibility in the spec and record it as an accepted limitation. No implementation or spec change has been made for this unresolved choice.

## RECALL — Wi-Fi reservation boundary lacks behavioral coverage

`wifi_budget.rs` covers helper constants and historical ELF arithmetic, but not aggregate reserve/reject behavior of `MemoryBudget::for_build(true)`. Current wiring is correct by inspection. Add a behavioral boundary test as a follow-up; this is not an implementation defect.

## Evidence and limits

- Fresh pre-fix full host run: `scripts/host-test.sh --locked`, exit 0, 874 passed, 0 failed, 0 ignored.
- Fresh board run: `scripts/test-board-logic.sh`, exit 0, 331 passed, 0 failed, 0 ignored.
- Fresh pre-fix build: `scripts/check-wifi-build.sh`, exit 0; C61/X4 enabled/disabled checks and memory guards passed. C61 enabled statics 204,664 B (aligned image range), stack section 51,960 B; headroom above minimum 2,808 B.
- No OnePage hardware was accessed. Association/DHCP, radio runtime heap, stack high-water, physical mDNS, re-entry and power remain UNVERIFIED.
- Real SD close errors are swallowed in inherited storage macros (G5, explicitly deferred in the prior ledger). The host VirtualStorage seam does not cover FAT flush/close; full real-SD R7/R8 conformance cannot be claimed.
- Historical red-proof and expected-value provenance are carried by `progress.md`. Import-only red proofs establish absent interfaces, not independently demonstrated behavioral failures. Original task console logs were not recreated.
- Historical weakening gate: T6 removed nine bring_up tests, mapped to session tests; the immediate-completion threshold changed from 500 ms to 1.5 s. T6 snapshot comparison confirms original session assertions retained and two tests added. HTTP/mDNS files equal their earlier snapshots before the final hygiene cleanup. This is a disclosed weakening, not a clean no-change gate.
- Final fix tests are read-only for implementer except comment cleanup. New regression tests and optional peer-EOF seam preserve the default open-peer behavior. Re-review confirms no existing assertion removal/loosening, skips, tolerance widening or existing path substitution. Rustfmt-normalized executable content of four existing suites is identical after comment removal. The new peer-EOF seam defaults to false; existing test scenarios retain their prior open-peer path.

## Cherry-pick source

`feat/iansui-cjk-host-foundation` points to `445755f` and is already an ancestor of `onepage` (`8702340`). `git log onepage..feat/iansui-cjk-host-foundation` is empty. There is no missing commit to cherry-pick from the requested branch.

`improve/memory-cjk-font-build` contains one additional commit `26febe1` (Improve memory budgets and CJK font builds). The source correction is awaiting user confirmation; it has not been applied.

## Final hygiene gate

Spec IDs and change-folder oracle links were removed from source/test comments and script labels. `rg` over changed/new upload sources, tests, Cargo.toml and the new build script has no spec-ID hits. `git diff --check` and `bash -n scripts/check-wifi-build.sh` pass. Initial shell execution raced a non-atomic cleanup write; the implementer replaced the cleanup atomically from the original snapshot. Re-review confirms executable shell lines equal the original except display labels.
