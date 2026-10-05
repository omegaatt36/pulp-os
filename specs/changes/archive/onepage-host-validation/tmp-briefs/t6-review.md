# T6b corrections and T6c review

T6b spec compliance: **APPROVED**. T6b code quality: **APPROVED**.
T6c spec compliance: **APPROVED**. T6c code quality: **APPROVED**.

## Findings

No findings in the scoped changes. The previous T6b HIGH finding for absent EPUB settings/bookmark regression is resolved.

## Reviewed evidence

Applied raiven-code-review in review-worker mode. Read staged/working diff context, the scoped `/tmp/t6c-impl-diff.patch`, final source and tests, T6b/T6c briefs/reports/corrections, the prior T6b review, current R5/R8 text and production storage/paging boundaries. Unrelated working-tree changes are excluded. No source edits, commits, formatting commands, or suite reruns performed; this file is the requested review artifact.

- T6b `host/tests/reader_bookmarks.rs:511`: all six generated EPUB2/3 STORED/DEFLATE/Mixed variants save beyond chapter/page zero and restore exact chapter/page/text on a fresh Rig over the same card. The scenario also pins second-book isolation, most-recent-save reordering, persisted list order after reboot, and deletion reopening chapter/page zero while preserving the other bookmark. Serial execution and one live Reader Rig protect the process-global image worker. The restored cache-load boot precondition agrees with the archived oracle and does not weaken record-layout assertions.
- T6b `host/tests/reader_settings.rs:561`: all six variants load saved font/theme through the explicitly accepted SettingsRig/configure seam; layout values are inherited from the existing settings oracle. The suite claims that scoped seam, rather than full AppManager integration.
- T6c `host/src/reader.rs:116`, `host/src/drivers/sdcard.rs:15`, `host/src/drivers/storage.rs:14`: mount control changes Kernel.sd_ok and host card availability together. Every direct storage forwarder checks the borrowed mounted card before backend access; NoCard and storage::borrow match firmware `kernel/src/drivers/storage.rs:198`. Remount retains the same VirtualStorage. save_title retains existing validation order and reaches the mounted append boundary.
- T6c `host/src/apps/settings.rs:1`, `host/src/reader.rs:362`: the include wrapper retains the production SettingsApp body and exposes its private dirty flag read-only. Bookmark dirty state reads the production cache directly; neither probe changes application state.
- T6c `host/tests/reader_failures.rs:36`: successful Next then injected Prev establishes a mandatory current-page read; Error/ReadFailed/log/reopen assertions remain intact. Production `src/apps/reader/paging.rs:129` confirms forward prefetch cannot supply the previous page. The separate optional-prefetch scenario verifies Ready, injected read consumption/logging, and recovery, consistent with clarified R5. Settings/bookmark failure cases retain previous bytes and dirty state until a successful retry; short reads are observed without inventing a universal error requirement.
- T6c `host/tests/reader_screens.rs:6`: screen invariants cover PBM 480×800 payload, both ink and white, repeated-draw equality, navigation/reopen equality, and full-versus-stitched equality on TXT and all six EPUB variants including an image page. Expectations are differential/invariant checks, not output-derived hash pins.

## Verification limits

Author final combined evidence reports bookmarks 15, settings 19, failures 6, screens 1 passing. Inspected the restored T6c focused log (6 failure cases and 1 screen case passing) and the root EPUB save-position mutation failure log. Implementer reports 236 full-suite passes before the final T6b assertion extensions; final combined author evidence covers those extensions. API compile red, baseline-green screen invariants, and behavioral mutation sensitivity remain distinguished. No new concrete risk justified rerunning suites.

Hardware/session restoration, network behavior, full AppManager integration, every page/configuration screen combination, and physical SD behavior remain outside these batches. Existing companion suites supply TOC/image/other success-path coverage. These limits do not contradict the scoped briefs.

## Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 0 | pass |
| HIGH | 0 | pass |
| MEDIUM | 0 | pass |
| LOW | 0 | pass |

Verdict: **APPROVE — scoped T6b corrections and T6c forwarding/failure/screen batches satisfy their stated acceptance contracts.**
