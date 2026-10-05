# Implementer report — DONE after author precondition correction

Implemented host-only mount borrowing and read-only probes. Owned reader.rs, drivers/sdcard.rs, drivers/storage.rs; parent expanded scope to apps.rs and new apps/settings.rs for an include wrapper exposing the private production save_needed flag read-only. Production SettingsApp body remains unchanged. Mount state restores the same card. Every direct storage forwarder borrows the mounted card and returns firmware-origin NoCard/storage::borrow before backend access. save_title delegates to the mounted append boundary, preserving existing title validation ordering.

Contract correction approved by parent: AppContext has no sd_ok. Kernel.sd_ok and SdStorage.mounted are changed together. No application logic, algorithms, test assertions, scripts or commits changed.

Independent API red command: scripts/host-test.sh --test reader_failures --test reader_screens; exit 101, missing methods, /tmp/t6c-impl-red.log. This is API red, not behavioral red. Existing screens are author-reported regression invariants.

Focused after implementation: same command; reader_failures 4 passed, 1 failed. missing_card_is_a_storage_boundary_failure_and_mount_recovers, short_read_is_recorded_and_reader_settles, settings_write_failure_preserves_bytes_and_next_tick_retries, bookmark_flush_failure_preserves_bytes_and_dirty_state pass. failed_open_and_page_read_can_be_retried fails at host/tests/reader_failures.rs:50: phase Ready rather than Error after injected Next-page read. /tmp/t6c-impl-focused.log. Cargo stops before reader_screens executable. Gate explicitly requires STOP on production mismatch; no further app inspection/fix or test relaxation performed.

Requirements: R4/R5 storage/probe APIs now compile; missing-card, short read, settings retry and bookmark retry verified. R5 paging acceptance remains unresolved. R2/R6/R7/R8 screen author evidence remains green regression evidence; implementer completion cannot claim green. Full host and mutation passes remain outstanding because STOP gate triggered. Parent notified before continuing.

Expectation origins remain author report/task brief and firmware borrow contract, no new provenance or output-derived pins. Tests unchanged; weakening gate maintained. Actual scoped comparison against /tmp/hv-snap/T6c-start-current captured /tmp/t6c-impl-diff.patch. New settings wrapper is additional parent-approved file. No assertion edits, no cargo fmt, no commits.

## Resume and final evidence

Parent independently identified the displayed next page as prefetched, so the injection targeted optional future prefetch rather than mandatory page loading. Test author corrected the mandatory-read precondition: Next first, inject, then Prev; mandatory Error/ReadFailed and retry expectations remain. Author also added a separate optional-prefetch failure test. Implementer did not edit tests. The initial STOP record above remains evidence of the gate being followed.

scripts/host-test.sh completed exit 0: 236 tests passed across all test executables, 0 failed; /tmp/t6c-impl-full.log. This establishes shared algorithm and companion EPUB/TOC/settings/bookmark regressions, storage memory/file cases, PBM and strip invariants under the current suite.

Targeted mutations, each actual source change checked before execution and each exact baseline byte-restored in finally:
- SdStorage::borrow_card changed mounted.then_some(&card) to Some(&card): missing_card_is_a_storage_boundary_failure_and_mount_recovers fails Ready vs Error, exit 101. /tmp/t6c-mutation-mount.log.
- Rig::bookmarks_dirty changed cache.is_dirty to false: bookmark_flush_failure_preserves_bytes_and_dirty_state fails assert r.bookmarks_dirty at line 134, exit 101. /tmp/t6c-mutation-bookmarks-dirty.log.
- Settings host probe changed save_needed to false: settings_write_failure_preserves_bytes_and_next_tick_retries fails assert r.is_dirty at line 112, exit 101. /tmp/t6c-mutation-settings-dirty.log.

Restored focused command scripts/host-test.sh --test reader_failures --test reader_screens exits 0: 6 failure cases and 1 screen case pass, /tmp/t6c-impl-restored.log. Parent performed save_position mutation independently; it was not repeated here. Mutations are behavioral sensitivity evidence, distinguished from original missing-API compile red. Original invariant screens had no behavior red claim.

R2/R4/R5/R6/R7/R8 now have green current-suite evidence, subject to the author-declared scope gaps. Missing-card semantics are forwarded at all direct backend calls. Algorithms and production bodies untouched. No output-derived pinned expectations introduced. Original mandatory-read failure expectation preserved, precondition corrected by independent author; optional-prefetch Ready expectation separately exercises observable injected read failure and recovery. Weakening gate comparison remains /tmp/t6c-impl-diff.patch, plus parent-authorized apps/settings.rs include wrapper; mutation changes all precisely restored. Root asked to include host seams in independent code review. No commits or cargo fmt.

Exact tested state: full suite included reader_epub 31 cases and reader_failures 6 cases; later T6b deletion/isolation additions were still being authored and are outside this 236-test count. Final scoped diff refreshed after exact mutation restoration, including new settings wrapper, at /tmp/t6c-impl-diff.patch.
