# Final-fix round 2 report (production-only)

## Root cause
`wrap_proportional` chose per-line flags with `fonts.has_fallback()`: start-of-line `line_flags` when the window
stages fallback glyphs, otherwise `styles.flags()` sampled at emit time (after any closing marker was applied).
Round 1 (CARRIED_FALLBACK) makes Latin-only windows carry style/indent, but `has_fallback()` is false there, so the
line holding the closing marker got flags 0 while draw seeds from `span.flags`. Also, the ASCII/Latin break sites never
maintained `line_flags` (only CJK/newline/image sites did).

## Fix (src/apps/reader/paging.rs, `wrap_proportional` only; no new parameters)
- New local `line_start_flags = fonts.has_fallback() || initial_flags & CARRIED_FALLBACK != 0`. The caller already
  passes `initial_flags` (with the carried bit) only when `carried`, else 0, so the no-carry English path has
  `line_start_flags == false` and keeps `styles.flags()` at emit (unchanged behaviour). `emit!` and the tail-line
  emit use it instead of `has_fallback()`.
- New `space_flags` (mirror of `legal_flags` for the Latin branch): style in effect when `last_space` was set
  (ASCII space, NBSP, soft hyphen); reset to `line_flags` wherever `last_space = line_start`.
- Latin break sites now set `line_flags`: break at `last_space` -> `space_flags`; break at word start / char `i` /
  after a wrapping space / ASCII word overflow -> `styles.flags()`. `line_flags` is only read when
  `line_start_flags`, so these updates are inert for the no-carry path.
- Branch gating (`fonts.has_fallback()` at CJK vs Latin branches) is untouched.

## Results
- cjk_heading_latin_tail: before 6 pass / 2 fail (closing_marker_line_is_drawn_in_heading_style_on_latin_only_continuation_page,
  closing_marker_line_with_spaces_in_latin_words_is_drawn_in_heading_style); after 8 passed, 0 failed.
- cjk_lifecycle 2/2, cjk_heading_pages 1/1, cjk_identity 4/4 pass.
- check-reader-regression.sh both: exit 0; head 83 passed, tree 90 passed; golden 3105 lines sha
  8cb6e31a70a32cdc05d30fab45ee0c7983da8cde6a55ba8bb45a0540f8d3f454 (unchanged) for both.
- IANSUI_REQUIRED=1 check-host-boundary.sh: exit 0 (719 host tests, x4 + c61 builds ok, PASS).
- test-board-logic.sh: exit 0.
- Test files: host/tests/cjk_heading_latin_tail.rs sha256 36e2d72c...ab558 identical before and after (matches
  final-heading-tests.sha256). No file under host/tests touched (git shows none modified; scripts/reader-regression
  pagination.rs was already modified before this round, untouched by me).

## Diff
final-fix2.diff is `diff` of `git diff` (now) vs /private/tmp/final-fix-round2-start.diff; the only substantive
changes are in paging.rs `wrap_proportional` (hunk contexts shift line numbers, ignore those lines).

## Residual concerns
- Carried-window line spans from CJK-staged windows can still contain bit 7 in `LineSpan.flags` for the first lines
  (pre-existing, draw masks it); not changed.
- Spec/test gap noted in review remains: unsupported non-CJK scalars (Cyrillic, etc.) latch carry the same way.
