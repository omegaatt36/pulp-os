# Chapter anchor ownership fix

Changed only `src/apps/reader/paging.rs`. `mod.rs` remained unchanged relative to the implementation snapshot. Snapshot-relative change: `t9-chapter-fix.diff`.

During EPUB NeedIndex, the page table can belong to the departed chapter. Invalidation now preserves explicit session/bookmark raw restore positions, otherwise leaves the target without a restore anchor. Ready/NeedPage invalidation still captures the current raw anchor before reset. goto_last_page is untouched, preserving ordinary backward chapter last-page intent.

Verification command (exit 0):

```sh
cargo test -p pulp-host --target aarch64-apple-darwin --config 'unstable.build-std=["std","test"]' --test cjk_chapter_identity --test cjk_identity --test reader_epub --test reader_bookmarks -- --nocapture
```

Actual result summaries:

```text
cjk_chapter_identity: test result: ok. 1 passed; 0 failed
cjk_identity: test result: ok. 4 passed; 0 failed
reader_bookmarks: test result: ok. 15 passed; 0 failed
reader_epub: test result: ok. 31 passed; 0 failed
```

51 covering tests pass. Frozen independent chapter test checks chapter page zero, scalar conservation, quick previous-chapter page zero and ordinary backward last page. Other targets cover live size changes, same-size pack replacements, bookmarks, chapter navigation and TOC jumps. Author report establishes pre-fix RED `(1, 6)` versus `(1, 0)`; no mutants used. Existing unused_mut warnings in reader_bookmarks and unused goto in reader_epub remain.

`git diff --check`: exit 0.

Recomputed SHA-256, matching frozen reports:

```text
5b921d2feb70f788e58f2b6adb412fc78587490d0ce84bba444a6791010f89fb  host/tests/cjk_chapter_identity.rs
42376f2733b6e7051735a008214eba869495428d13b262bb8d64b7b2f6eef1a3  host/tests/cjk_identity.rs
c54f7693bfaec43d1c621ac935f79993b43b91c00e6c1b7ed034bb33a81d7393  host/tests/cjk_support/mod.rs
```

Weakening gate: no tests, fixtures, oracles, baselines or support changed. No commits, spec IDs in Rust, persistence or broad full suite. Other agents' work retained. Reviewer handoff is controller-owned.
