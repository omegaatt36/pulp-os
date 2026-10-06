# T9 review

Spec-compliance verdict: REQUEST CHANGES.
Code-quality verdict: REQUEST CHANGES.

Scope: task-only snapshot-relative `t9-review.diff`, with surrounding production reader/font code and thin regression harness export inspected. No production/test edits, test reruns, commits, or agents spawned.

## Important

### I1 — A replaced pack transfers a previous chapter's offset into the next chapter

File: `src/apps/reader/mod.rs:952` (identity check), `src/apps/reader/paging.rs:30` (anchor capture).

Chapter navigation updates `epub.chapter` and enters `NeedIndex` while the page table still belongs to the previous chapter. The new unconditional pre-index identity check detects a changed font pack and calls `invalidate_layout()`, which captures `byte_offset()` from that old page table and saves it as `restore_offset`. The ordinary chapter index then resets the table, but does not clear `restore_offset`; `NeedPage` consequently seeks that old chapter-local offset in the newly selected chapter instead of starting at its beginning. For example, replace the active body pack after reading a multi-page chapter, then press Next at its last page: the next chapter opens at the old last-page raw offset (or at its end if shorter), skipping its initial text. Next/previous chapter quick actions and chapter jumps have the same mixed chapter/table interval.

Verified by tracing `page_forward()` lines 484–490, `jump_forward()`/quick chapter actions, `background()` lines 952–957 and 1116–1142, `epub_index_chapter()` and `invalidate_layout()`. Neither indexing nor identity checking associates the saved offset with its originating chapter. Frozen tests cover TXT replacements and heading/body metrics, not this chapter-transition interaction.

Fix: distinguish an existing chapter's layout rebuild from a pending chapter transition. Do not create a restore anchor from the previous chapter's table when entering a newly selected chapter; preserve explicit bookmark/session restore offsets and previous-chapter last-page intent. Associate the indexed table/anchor with its chapter or perform the identity check after the target chapter's initial reset while respecting pending restore intent.

## Critical / Minor

No findings.

## Evidence and compliant paths

- Body and heading identities include `font_id`, pixel size and installed/absent discrimination. `LAYOUT_VERSION` is compiled and included in `current_layout_identity()`; reviewed by source as contracted, without runtime mutation claims.
- Identity construction is lazy until fallback is encountered. Optional metadata and validated PFNT header errors propagate through `?`; absence uses a validated empty pack and cannot alias installed font ID zero.
- Existing-chapter invalidation captures its raw anchor before effective page-table, continuation state, fully-indexed and prefetch reset. Existing restore scanning selects the containing page; prepared glyphs are refreshed. No retained source/KernelHandle or strip-draw reads introduced.
- EPUB stripped byte-cache format and storage are unchanged; existing reindex reload mechanics remain in use.
- Harness change reexports the actual board-budget constant through the existing tree-only BigBuf module; no duplicated budget or runtime logic.
- Independently read existing logs: 4 identity tests and 99 scoped regression tests passed. No tests rerun; source trace established the uncovered chapter interaction.
- Recomputed frozen SHA256: identity `42376f2733b6e7051735a008214eba869495428d13b262bb8d64b7b2f6eef1a3`; support `c54f7693bfaec43d1c621ac935f79993b43b91c00e6c1b7ed034bb33a81d7393`. Both match frozen report.
- `cmp` of accessible task-start and final English golden transcripts exits 0. Both existing logs report hash `d5830acbcbd368c8cbbcb739ebe970b3a3867fef03034ea18955f72679e02d21`; the original pin mismatch predates T9 and is not a new T9 finding. It remains an overall acceptance limitation.
- `git diff --check`: exit 0.

## Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| Critical | 0 | pass |
| Important | 1 | fix |
| Minor | 0 | pass |

Verdict: REQUEST CHANGES — fix chapter/anchor ownership before acceptance.

# Superseding review after chapter fix

Spec-compliance verdict: APPROVE.
Code-quality verdict: APPROVE.

The earlier I1 finding is resolved by the snapshot-relative `t9-chapter-fix.diff`. Source inspection confirms that EPUB `NeedIndex` invalidation retains only explicit pending restore positions, rather than deriving an anchor from the departed chapter's page table. Chapter navigation therefore enters the selected chapter at its ordinary destination.

Ready/NeedPage invalidation still captures the current raw anchor before resetting. If a live size or resume change first invalidates an existing chapter, the saved `restore_offset` survives a subsequent identity check in NeedIndex. Bookmark/session restore offsets are retained through indexing and consumed by the existing containment scan. `goto_last_page` is unchanged by invalidation/reset and remains consumed by NeedIndex's `scan_to_last_page()` branch, so ordinary backward navigation retains its last-page destination. Quick previous-chapter navigation retains its first-page destination.

Reviewed the frozen chapter regression independently: its literal replacement changes font identity/advance, its initial chapter-zero page-three anchor is nonzero, and its assertions establish chapter-one page zero, full selected-chapter scalar conservation, quick previous chapter page zero, and ordinary backward last page. The author report records the matching pre-fix RED `(1, 6)` versus `(1, 0)`. The fix report records 51 passing covering tests; no suite was rerun in this review. The prior original T9 test logs were inspected in the first review. No concrete unanswered source doubt justified further reruns.

Recomputed chapter-test, identity-test and support SHA256 values match all three frozen report values. `git diff --check` exits 0. Source/tests remain unmodified by the reviewer; only this review report is updated. Earlier English golden baseline limitation remains unchanged and belongs to overall acceptance.

## Superseding Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| Critical | 0 | pass |
| Important | 0 | pass |
| Minor | 0 | pass |

Verdict: APPROVE — no remaining findings in the T9 task-only changes and chapter fix.
