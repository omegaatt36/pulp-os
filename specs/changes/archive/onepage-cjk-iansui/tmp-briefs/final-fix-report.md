# Final fix report (production-only)

Production edits: `src/apps/reader/mod.rs` (1 line), `src/apps/reader/paging.rs` (small). Diff: `final-fix.diff`
(edits only, relative to the state at my start; start snapshot `/private/tmp/final-fix-start.diff`). No test file edited.

## Defect A: TOC suspend -> resume -> Back leaves body CJK glyphs as boxes
Root cause: `src/apps/reader/mod.rs:925` (`on_suspend`): `render_fonts_released = self.state == State::Ready`.
`on_suspend` always clears `self.cjk`, but in `State::ShowToc` the flag stayed false, so the restore block at
`prepare_render` (mod.rs ~1452, guarded by `state == Ready && render_fonts_released`) never re-staged/prepared the
page's CJK glyph cache after Back. Body text (`lines`) was intact; only the glyph cache was empty.
Fix: `render_fonts_released = matches!(self.state, State::Ready | State::ShowToc)`. The flag survives the TOC
(restore guard requires Ready), so the restore runs on the first Ready prepare after Back. paging.rs:72
(`invalidate_layout` sets it false) is correct as is: a rebuild re-prepares fonts itself.

## Defect B: open heading loses heading style on Latin-only continuation pages
Root cause: `src/apps/reader/paging.rs:158-167` (original lines, `wrap_lines_counted`): the page-start marker state
(`initial_flags`, `initial_indent`) was passed to `wrap_proportional` only when `view.has_fallback()`, i.e. when the
staged window itself contains unsupported (CJK) scalars; otherwise 0/0. That gate exists because the pre-port English
layout resets style at every page start (the head baseline). So a page whose window holds only Latin text ignored the
carried heading flag even though the heading began on an earlier CJK page. `record_continuation` computed the flags
correctly all along; the second "source of truth" was this window-content gate (CJK-state vs Latin path).
Fix: new bit `CARRIED_FALLBACK` (1<<7) in `pg.style_flags[]`, set deterministically along the page chain by
`record_continuation` when the page's window staged fallback or the page itself started with the bit. A page whose
start flags carry the bit applies its carried style/indent even if its own window is Latin only. Bits 0..2 are still
what `StyleState::from_flags` reads (it masks `& 7`; `stage_metrics_with_flags`/`mark_visible_with_flags` also mask).
Deterministic for Prev/re-layout because a page's layout depends only on stored `style_flags[page]` + its window
(a book-wide latch on layout identity would have made page 2's layout depend on whether page 3 had been seen).
No new allocation, no new SD access, no memory-budget change.

## CONFLICT (reported, not papered over)
`oracle_latin_heading_is_consistent_across_pages` (all-Latin heading, no CJK anywhere) stays RED. Fixing it means
passing carried flags for pure-English windows. I tried that experiment (unconditional `initial_flags/indent`):
`check-reader-regression.sh both` -> head 83 pass, tree 90 pass but the golden trace changes: tree 3103 lines,
sha256 d1679ab3902b0f5d33aac704259de267ebc12ba11a29d0ae3d7b2fc6bb8ce747 != pin 8cb6e31a...; 14+ rows differ vs
head (e.g. `c0 p2 off3283 n35 hf77c4b66b52b0d18` -> `h97e3046740ec57b2`, `c3 p3 off4998 n17` -> `off4997 n18`).
I.e. the original English layout resets heading/bold/italic/indent at each page start and the golden pins that.
The experiment was reverted; the shipped fix is limited to chapters where the CJK bank participated earlier
(the stated acceptable scope). Resolution needs a spec/pin decision (all-Latin test vs English golden), not code.

## Before / after (frozen tests)
Before (production at start):
- cjk_lifecycle: 1 passed, 1 FAILED (`suspended_toc_resume_back_restores_original_body_glyphs_and_pixels`,
  panicked at host/tests/cjk_support/mod.rs:140:5, glyph pixels = boxes)
- cjk_heading_latin_tail: 2 passed, 3 FAILED:
  oracle_latin_heading_is_consistent_across_pages (line 0 "AAAA" left 4 right 2, test.rs:83),
  navigation_and_bookmark_restore_... (page 1 first visit "AAAA" 4 vs 2),
  latin_only_pages_inside_open_cjk_heading_keep_heading_style (page 1 at raw offset 74 "AAAA" 4 vs 2).
After:
- cjk_lifecycle: 2 passed, 0 failed.
- cjk_heading_latin_tail: 4 passed, 1 FAILED (only `oracle_latin_heading_is_consistent_across_pages`, same 4 vs 2; see conflict).

## Constraint commands
1. `CARGO_NET_OFFLINE=true scripts/check-reader-regression.sh both`: exit 0; head 83 passed, tree 90 passed; both
   goldens 3105 lines sha256 8cb6e31a70a32cdc05d30fab45ee0c7983da8cde6a55ba8bb45a0540f8d3f454 (unchanged).
2. `IANSUI_REQUIRED=1 CARGO_NET_OFFLINE=true scripts/host-test.sh --no-fail-fast`: exit 101; 715 passed, 1 failed
   (the oracle test above); 0 SKIPPED lines; cjk_identity, cjk_surfaces, cjk_surface_fixes, cjk_reader,
   cjk_unused_bank, cjk_chapter_identity, cjk_heading_pages, cjk_nested_styles, cjk_long_group, cjk_lifecycle,
   paging/render/reader_* all green. (Without --no-fail-fast cargo stops at the first red binary.)
3. `scripts/test-board-logic.sh`: exit 0; 324 passed, 0 failed.
4. `scripts/check-host-boundary.sh`: exit 1, solely because its host-test step contains the red oracle test
   (`target/accept-host.log`: only that one failure; 30 suites ok). Host graph has no esp-* crate (ok);
   `cargo build-x4` exit 0 (ok); `cargo build-c61` exit 0 (ok). So both firmware links PASS.
5. R7/R9/R8: no change to draw, no SD access, no allocations; compile-time budget asserts built in both firmware builds.

## Test-file integrity
`shasum -c` on final-lifecycle-tests.sha256 and final-heading-tests.sha256 before and after: all four files OK
(cjk_lifecycle.rs, cjk_support/mod.rs, cjk_heading_pages.rs, cjk_heading_latin_tail.rs). No file under
host/tests/ or scripts/reader-regression/**/tests was edited by me.

## Residual concerns
- The all-Latin oracle test cannot go green without breaking the English golden pin (above); needs a decision.
- A heading opened on pure-Latin pages before the first CJK page of a chapter still resets at page starts until a
  CJK window appears (head behaviour preserved by design); a heading opened in Latin and continuing into CJK is
  honored from the first CJK-participating window onward.
