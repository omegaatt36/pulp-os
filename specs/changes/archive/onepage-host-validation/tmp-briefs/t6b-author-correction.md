# T6b test-author correction

Status: DONE. Verified a porting omission against the independent archived oracle.

## Oracle and exact correction

`scripts/reader-regression/os/tests/bookmarks.rs:170-172` constructs the kernel and explicitly calls `k.bookmarks_load();` before saving records. The ported `flush_writes_the_baseline_record_layout` omitted that call. Restored exactly that one line immediately after `let mut k = kernel_with(card());` in `host/tests/reader_bookmarks.rs`.

R8 now states the explicit production cache-load boot precondition for bookmark persistence scenarios. Proposal Assumptions records the resolved porting omission. No production files, settings tests, assertions, expected values, or oracle files were edited. No commits or formatting commands were run.

## Red evidence before correction

Command: `scripts/host-test.sh --test reader_bookmarks flush_writes_the_baseline_record_layout`

Exit status: 101.

```text
running 1 test
test flush_writes_the_baseline_record_layout ... FAILED
thread 'flush_writes_the_baseline_record_layout' panicked at host/tests/reader_bookmarks.rs:305:22:
called `Option::unwrap()` on a `None` value
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 13 filtered out
```

## Green evidence after correction

Command: `scripts/host-test.sh --test reader_bookmarks --test reader_settings`

Exit status: 0. Bookmarks: 14 passed, 0 failed. Settings: 18 passed, 0 failed. The restored record-layout scenario passes; `not_loaded_means_no_lookup_and_no_save` also passes. Existing unused-mut warnings remain untouched.

## Weakening gate

`diff -u /tmp/t6b-bookmarks-before-correction.rs host/tests/reader_bookmarks.rs` reports exactly one added line: `k.bookmarks_load();`. All record length, hash, offset, chapter, flags, generation, name length, name bytes, padding, and second-generation assertions remain byte-identical to the pre-correction test. Their expected values still derive from the archived oracle, not observed production output. This restores the original boot precondition and does not weaken any assertion or alter the independently tested never-loaded-cache semantics.

## EPUB format coverage additions

Added `saved_font_and_theme_apply_to_generated_epub_formats` to `host/tests/reader_settings.rs` and `generated_epub_bookmarks_restore_chapter_page_and_text_after_reboot` to `host/tests/reader_bookmarks.rs`. Each loops all six generated standard EPUB2/3 fixtures (STORED, DEFLATE, Mixed) and holds a process-local mutex for the entire EPUB scenario, with only one Reader Rig alive at a time. These are the only EPUB-driving tests in their respective executables.

The settings scenario applies saved font 4/theme 3 through the existing explicit settings-load/configure seam. Expectations 10 maximum lines, 400 text width, theme 3 are the unchanged settings oracle already used by the TXT scenario; no production-derived numeric pins were introduced. The bookmark scenario navigates to chapter 1/page 1, saves and flushes, and constructs a new Rig on the same storage object. It requires exact equality of chapter, page and displayed text across reboot, extending the archived TXT roundtrip invariant to EPUB formats. The 400 idle ticks are a bounded setup budget copied from the established EPUB harness, not an asserted implementation timing result.

Baseline command: `scripts/host-test.sh --test reader_bookmarks --test reader_settings`. Exit status 0; bookmarks 15/15, settings 19/19. Both new scenarios started green against existing contract APIs. This is baseline coverage evidence, not red proof. Root coordinates separate targeted forwarding-seam mutations; no implementation files were edited by this author.

Weakening gate for additions: only new test functions were appended beside existing scenarios. All pre-existing assertions and expected values remain unchanged, apart from the independently verified cache-load setup restoration above. Review report was not yet present when the additions were made; root supplied the missing-format finding.

After reading `t6b-review.md`, expanded the same generated EPUB scenario to pin per-book isolation (a second EPUB has no saved position), most-recent-save list reorder, list order after same-card reboot, and deletion followed by reopening at chapter 0/page 0 while preserving the other book's bookmark. These expectations extend archived bookmark oracle semantics without production-output numeric pins. All six generated variants pass.

Root-owned mutation evidence: `/tmp/t6b-save-position-mutation.diff` changes only the `Rig::save_position` forwarding call to a no-op; `/tmp/t6b-epub-mutation.log` records the new EPUB roundtrip test failing (exit 101). Root reported byte-exact implementation restore. This is genuine mutation red evidence, separate from already-green baseline coverage; author made no implementation edits. Final combined command `scripts/host-test.sh --test reader_bookmarks --test reader_settings --test reader_failures --test reader_screens` exits 0: bookmarks 15, settings 19, failures 6, screens 1, all pass.
