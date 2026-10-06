# English anchor production repair

Status: focused acceptance GREEN; identity/chapter/bookmark coverage GREEN; one full offline reader regression both GREEN. No tests, fixtures, or golden changed. No commits or mutants. Snapshot: `/private/tmp/english-anchor-fix-start/{mod.rs,paging.rs}`. Scoped task diff: `english-anchor-fix.diff`.

## Root cause and repair

`wrap_lines_counted` established layout identity only when a CJK fallback was active. Pure English therefore had no identity for `check_layout_identity` to compare. The public font setter applied metrics and updated `applied_font_idx` before resume checked it, so English resume retained old offsets and old current lines. Revisiting the same offset wrapped with the new metrics, violating adjacent navigation consistency.

Identity is now established before selecting any wrapping path. Font pixel size, installed bank identity and layout version participate for English and CJK alike. Explicit invalidation clears the previous identity, because the rebuild already captured its raw anchor. Without that clearing, quick-cycle invalidation followed by the background identity check would invalidate a second time and overwrite the saved TXT anchor with reset offset zero. The ordinary identity-mismatch path still installs the newly compared identity immediately after invalidating. Chapter-specific anchor selection and session/bookmark code are unchanged. `mod.rs` is unchanged.

## Verification

Commands use actual live production and the unchanged frozen acceptance test.

```
CARGO_NET_OFFLINE=true READER_REG_WORKDIR=/private/tmp/english-anchor-fix-check bash scripts/check-reader-regression.sh tree --filter txt_live_font_change_preserves_raw_anchor_and_complete_source
```
Before repair: exit 1; 0 passed, 1 failed at pagination.rs:256 (`back returns to the same source and layout`). Captured stdout `/private/tmp/english-anchor-fix-red.log`, full test output `/private/tmp/english-anchor-fix-red-tests.log`.
After repair: exit 0; 1 passed; golden 3105 lines with immutable SHA256 below. Output `/private/tmp/english-anchor-fix-focused.log`.

```
CARGO_NET_OFFLINE=true cargo test -p pulp-host --target aarch64-apple-darwin --config 'unstable.build-std=["std","test"]' --test cjk_chapter_identity --test cjk_identity --test reader_epub --test reader_bookmarks -- --nocapture
```
Exit 0; chapter identity 1 passed, identity 4 passed, bookmarks 15 passed, EPUB 31 passed; 0 failures. Output `/private/tmp/english-anchor-fix-host.log`.

An initial `check-reader-regression.sh tree --filter cjk_identity` invocation exited 1 with `FAIL no test ran [tree]`; those tests belong to the main host harness, so the correct command above was run. No claim of coverage is based on the empty invocation.

```
CARGO_NET_OFFLINE=true READER_REG_WORKDIR=/private/tmp/english-anchor-fix-check bash scripts/check-reader-regression.sh both
```
Exit 0. Head 83 passed; tree 90 passed. Full reader checks include pagination, settings, bookmarks, navigation and session restoration. Both immutable golden traces: 3105 lines, SHA256 `8cb6e31a70a32cdc05d30fab45ee0c7983da8cde6a55ba8bb45a0540f8d3f454`. Output `/private/tmp/english-anchor-fix-both.log`.

## Hashes and weakening audit

| File | Before SHA256 | After SHA256 |
|---|---|---|
| src/apps/reader/mod.rs | b6ef27f68655778a35183500939ca6adfbf5056a0a350df732fc3c365aa8389a | b6ef27f68655778a35183500939ca6adfbf5056a0a350df732fc3c365aa8389a |
| src/apps/reader/paging.rs | 4f6716cdcd492c82fc99cfd3dc0e627c03f05c8b6d5e062693ca53dbc128e066 | c34bf930c9f82985c7911fa9dccf90e5aeb02deff6c30731bb3f2d5921144c87 |
| scripts/reader-regression/os/tests/pagination.rs | 454d1a92c96c0749d6ec82bcb70493a898d925e22f5f41424f3d1163168ce06e | 454d1a92c96c0749d6ec82bcb70493a898d925e22f5f41424f3d1163168ce06e |

No assertions were removed or weakened; no tests/oracles/fixtures edited, no skips or tolerances added. The frozen requirement-derived acceptance now passes in both callback variants. No outstanding observed failures. Production identity establishment now reads optional bank identity even for English; valid or absent bank behavior is covered by host tests and English regression. Hardware execution was not performed. Independent code review remains controller-owned.
