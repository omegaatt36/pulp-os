# Storage failure test-author correction

Status: DONE. No implementation edits, commits or formatting commands.

## Verified boundary and red evidence

Original focused command: `scripts/host-test.sh --test reader_failures failed_open_and_page_read_can_be_retried`. Test fails at line 50: actual Ready, expected Error; test summary 0 passed / 1 failed. The shell command bundled subsequent reads, so its overall exit status was 0; Cargo's failure output is the red evidence.

Read-only verification of `src/apps/reader/paging.rs:129-200` shows that a prefetched current page is copied without reading, required current-page reads propagate errors with `?`, and subsequent prefetch errors clear the prefetch state instead of failing the displayed page. The original assumption that the next injected read after `Next` was mandatory was wrong.

First correction attempt used `NextJump`, preserving all original assertions. Combined failures/screens command exited 101: failures 5 passed / 1 failed, same Ready versus Error. Per STOP rule author reported the mismatch to root. Root identified lazy indexing: the jump clamps to the already-prefetched next page. No production change or assertion relaxation followed.

## Exact final setup correction

After successful reopen and `total_pages > 1`, press `Next` and assert `page() > 0` before resetting the read log and injecting. Press `Prev` instead of `Next` after injection, so the required displayed page is not in the forward prefetch. Original Error, ReadFailed, injected-error log and reopen-Ready assertions are unchanged.

Added `failed_best_effort_prefetch_keeps_the_displayed_page_ready_and_recovers`: Next displays its prefetched page, remains Ready, advances exactly one page, consumes and logs the injected error; subsequent Next advances another page and remains Ready, and reopen succeeds. Expected outcomes derive from the production storage error boundary and navigation invariants, not observed output pins. R5 now distinguishes mandatory displayed-page reads from best-effort future-page reads; proposal Assumptions records the corrected author assumption and unsuccessful jump setup.

## Green and weakening gate

`scripts/host-test.sh --test reader_bookmarks --test reader_settings --test reader_failures --test reader_screens`: exit 0, failures 6/6, screens 1/1, bookmarks 15/15, settings 19/19. New prefetch scenario started green against existing APIs; this is coverage evidence, not invented red proof.

`diff -u /tmp/t6c-failures-before-correction.rs host/tests/reader_failures.rs` shows only two added mandatory-load setup lines, Next-to-Prev action replacement, and the new independent prefetch test. No existing assertion or expected value changed; no other original failure scenario changed.
