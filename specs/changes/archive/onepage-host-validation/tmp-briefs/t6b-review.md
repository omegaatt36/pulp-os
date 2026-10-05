# Settings/bookmarks review

Spec compliance: **Needs fixes**.
Code quality: **Approved** for the scoped implementation diff.

Reviewed `t6b-review.diff`, author/implementer briefs and reports, the current four changed implementation files, both new suites, production SettingsApp/BookmarkCache/ReaderApp call sites, and archived settings/bookmarks oracle scenarios. Applied the raiven-code-review skill in review-worker mode. Existing unrelated working-tree changes are outside this review. No implementation changes, commits, or suite reruns were performed.

## Findings

[HIGH] Required EPUB settings/bookmark regression is absent

Files: `host/tests/reader_bookmarks.rs:473`, `host/tests/reader_bookmarks.rs:484`, `host/tests/reader_settings.rs:540`.

Issue: Every reader-facing settings/bookmark scenario opens generated TXT only. Neither suite imports/builds an EPUB fixture. The cache-only persistence test saves chapter numbers but never opens an EPUB, so it cannot catch failure to restore an EPUB chapter plus position. The existing `reader_epub.rs` suite has no bookmark/settings scenarios and does not close this gap. This contradicts the author brief lines 44–48 (generated TXT and EPUB format copies) and spec R8 (TXT and EPUB2/3 settings/bookmark regression).

Fix: Add independently authored EPUB2/3 fixture scenarios using the public production-driven Rig: save a position beyond the first chapter, flush, move the same VirtualStorage into a fresh Rig, and assert restored chapter/page/text; delete a bookmark and reopen at the initial position; pin per-book isolation and list reorder/persistence for EPUB names. Exercise saved font/theme settings against EPUB reader layout as well as TXT. Use oracle-derived expectations or format-independent roundtrip invariants, with explicit boot cache loading for kernel-driven persistence tests. Cover the generated format copies rather than claiming TXT coverage also exercises the EPUB restore path.

## Checks and resolved concern

- Production path: the host includes the real SettingsApp, calls its eager load/entry/event/background methods, and forwards bookmark operations to the real kernel BookmarkCache. The new probe is read-only. No settings/bookmark algorithm was added at the host seam.
- Virtual storage: settings writes use the real KernelHandle storage path; bookmark flush calls the real cache over SdStorage backed by VirtualStorage. Both `into_storage` methods return the same owned card for remounting.
- Direct SettingsRig driving is explicitly allowed by the author contract and matches the archived oracle. Manual reader configure forwarding is the accepted scoped settings-to-reader seam.
- The previously missing `bookmarks_load()` is now present at `host/tests/reader_bookmarks.rs:297`, matching archived `scripts/reader-regression/os/tests/bookmarks.rs:172` and production save-before-load no-op semantics. This is a restored precondition, not weakened assertions. The correction report was not yet available at review time. Green status after correction must come from the parent/test author verification; the implementer report still records the earlier 226/227 run.
- The existing out-of-scope hardware/session and failure-injection exclusions are appropriate. The reported SettingsRig extra-bookmark-load mutant is unobservable to this contract's suites; the shipped constructor follows the required no-load contract. It does not explain or excuse the EPUB regression gap.
- No confident runtime/security/code-quality defect was found in the four scoped implementation changes.

## Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 0 | pass |
| HIGH | 1 | warn |
| MEDIUM | 0 | pass |
| LOW | 0 | pass |

Verdict: **WARNING — resolve the required EPUB regression coverage before marking this batch spec-complete.** Implementation quality is approved; spec compliance needs fixes.
