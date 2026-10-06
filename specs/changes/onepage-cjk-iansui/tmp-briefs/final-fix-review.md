# Final-fix review (architecture / correctness)

Scope: `final-fix.diff` (src/apps/reader/mod.rs `on_suspend`; src/apps/reader/paging.rs `CARRIED_FALLBACK`)
vs spec R7, R9, R10 (scoped 2026-10-06), R11, R15; tests `host/tests/cjk_lifecycle.rs`,
`host/tests/cjk_heading_latin_tail.rs`.

Architectural impact: Low-Medium (one RAM-only marker bit in a per-chapter page table; one-line state-flag change).

## Verdict: REQUEST CHANGES (one concrete R10 defect, not covered by the tests; everything else APPROVE)

The `on_suspend` fix is correct. The CARRIED_FALLBACK bit is safe as a flag. But the carried-style path
routes Latin-only windows through the English emit semantics, so the heading's closing line is drawn in body style.

## Defect 1 (R10): closing line of a carried heading on a Latin-only page is drawn in body style
Where: src/apps/reader/paging.rs `wrap_proportional`, the `emit!` macro (~L679-686) and the tail line (~L972-978):
`flags: if fonts.has_fallback() { line_flags } else { styles.flags() }`. The diff makes a Latin-only window
carry style/indent (paging.rs:156-167) but `fonts.has_fallback()` is still false for that window, so line flags
are `styles.flags()` sampled at emit time (after the closing marker was applied), not the start-of-line style
that `draw` needs (`StyleState::from_flags(span.flags)` then marker replay, mod.rs:1773).

Failing scenario (executed in a scratch copy under /tmp, repo untouched; copy deleted):
input `01 'H' "臺" + "A"*600 + 01 'h' "\n" "Body."`, rig width 48, font idx 0 (the
`latin_tail_input` fixture). Last Latin heading line, bytes `'A' 01 'h'`, ends with
`SPAN flags=0x00` (all 299 other Latin heading lines `flags=0x04`). `draw` therefore renders that heading 'A'
with the Regular glyph while layout measured it at heading advance. R10 says heading persists "until the
closing marker"; this is the last char before the marker. The same happens for any carried line containing
the closing marker, and for bold/italic in carried windows (a line that starts regular and opens bold mid-line
is emitted with bold flags and drawn bold from column 0).
Why tests miss it: `assert_latin_page_capacity` (heading_latin_tail.rs) checks only `lines[..n-1]`, and only
character counts, never the style of the drawn glyph on the last line.
Note: the same quirk exists in the original English path (golden pins it), so do not "fix" it for the no-carry
case. It is a defect only because the diff now moves CJK-chapter Latin pages onto that path, where a CJK window
(has_fallback) draws the same line correctly with `line_flags`.
Direction: when `carried` is true use start-of-line flags in the Latin branch too. That needs a parameter into
`wrap_proportional` (not `fonts.has_fallback()`), and the Latin break sites (paging.rs ~L778, ~L814) must record
the flags in effect at `last_space` (the CJK branch already does this with `legal_flags`), otherwise
`line_flags = styles.flags()` after a space-wrap is wrong when a marker sits between `last_space` and `i`.
Add a test that checks the drawn glyph (or span flags) of the last heading line on a Latin-only page.

## Checks requested

### (1) Bit 7 of `pg.style_flags`: SAFE
- Writers: paging.rs:208 (record_continuation), :300 and :461 (page 0 = 0). Readers: paging.rs:112, mod.rs:1462.
  Declared mod.rs:198/218 as a RAM array in `PageState`; no serialization, no SD page cache, no bookmark field,
  no identity/`==` comparison (grep over src, kernel, host/src, board-logic, scripts: only these sites).
- Consumers all go through `StyleState::from_flags` which masks `& 7` (src/fonts/mod.rs:95), i.e.
  `cjk::scan` (cjk.rs:554, used by `stage_metrics_with_flags` L231 and `mark_visible_with_flags` L305) and
  `wrap_proportional` (paging.rs:668). `record_continuation` re-derives from `from_flags(flags)` so the bit does
  not feed back through `styles`; it is re-added explicitly.
- Leak: `wrap_proportional` receives the unmasked `initial_flags` and seeds `line_flags`/`legal_flags` (L655-656),
  so `LineSpan.flags` of the first line(s) of a carried CJK-window page can contain bit 7. The only other
  `LineSpan` flag consumers are `FLAG_IMAGE` (bit 3) and bits 0-2 (mod.rs:169-182), and draw masks via
  `from_flags`. Harmless today; `& 7` at the call site would remove the latent coupling (low priority, not a
  blocking item).
- Bits 3..6 are not used in `style_flags`, so no collision with `LineSpan::FLAG_IMAGE`.

### (2) Other suspend/resume paths: no gap found
- `on_suspend` always clears cjk/title_fonts/toc_fonts. `title_fonts` is re-prepared unconditionally each
  `prepare_render`; `toc_fonts` is re-prepared whenever state == ShowToc (mod.rs:1477-1490). Neither depends on the flag.
- States other than Ready/ShowToc (NeedBookmark..NeedCache, NeedIndex, NeedPage, Error): no displayed page glyph
  cache exists yet. NeedIndex/NeedPage re-run `load_and_prefetch(.., visible=true)` -> `prepare_page_fonts`
  (paging.rs:333, :399; mod.rs:1163) on resume, so glyphs are rebuilt by the state machine itself. The NeedPage
  restore loop is synchronous (no await), so a suspend cannot split it. Error: draws no body glyphs.
- ShowToc + suspend: flag stays true; the restore block is guarded by `state == Ready` (mod.rs:1452) so it runs
  on the first Ready `prepare_render` after Back. Verified the flag has no other reader (grep).
- Resume with a font change/identity change -> `invalidate_layout` clears the flag (paging.rs:72-77) and
  rebuilds; consistent.
- Non-blocking observation: ShowToc -> Select -> NeedIndex leaves the flag true, so the first Ready
  `prepare_render` after the jump redoes `stage_metrics_with_flags` + `prepare_page_fonts` on the freshly
  prepared page (redundant SD reads in prepare, not draw; idempotent). Not worth a code change; if desired,
  clear the flag where the TOC jump sets `State::NeedIndex` (mod.rs:~1281).

### (3) Prev / bookmark / font-size change; English behaviour
- Determinism: `style_flags[p+1]` is a pure function of (`style_flags[p]`, window bytes at `offsets[p]`,
  staged fallback state of that window), and every chain restarts at page 0 with flags 0 (`reset_paging`,
  `preindex_all_pages`). Prev re-wrap rewrites the same value; bookmark restore and `invalidate_layout` walk from
  page 0 (NeedPage loop); `scan_to_last_page` is sequential. Stale entries beyond the walked page are overwritten
  before use. The bit is set and cleared consistently per page (set iff this page's window staged fallback or the
  page started carried; never cleared mid-chapter; cleared by `reset_paging` for page 0 only, which is correct).
- Pure English (no scalar missing from the Latin font in any window): `cjk.has_fallback()` is false for every
  window, so the bit is never set and `carried` is false -> identical to the old gate; consistent with the
  unchanged golden (sha 8cb6e31a...). Boundary: the trigger is "fallback staged", i.e. ANY scalar absent from
  the Latin font (Cyrillic, Greek, symbols, emoji), not only CJK. An English chapter with one such character on
  page k now carries style/indent over page boundaries for pages > k (previously reset at each Latin-only page
  start), and the staged window is the whole read buffer, so the latch can start a page or two before the
  character's own page. This matches the spec wording ("CJK glyph window") only loosely; it is consistent
  with how the old code already treated that window and is not covered by any test or golden. Worth one line in
  the spec/tests so it is a declared behaviour rather than an accident.
- No-flash-font builds (`!HAS_REGULAR`): Latin windows take `wrap_monospace`; the bit is still propagated by
  `record_continuation(consumed, initial_flags, ..)` (preserves carried bit through Latin-only pages). Fine.
- R11: byte offsets/consumed counts are unchanged in form (scalar boundaries); only break points of carried
  Latin windows change. R15: rebuild path unchanged.

### (4) Draw-time SD / allocation: none introduced
The diff touches `on_suspend` (a bool) and layout-time code only. `draw` is untouched; `prepare_render` restore
now also runs after ShowToc -> Back (an existing code path, same allocations as the Ready path). No new Vec,
no new read; R7/R9 unaffected. Error path of the restore (`enter_error` + flag clear) unchanged.

## Test assessment
- `cjk_lifecycle.rs`: asserts text, glyph patches and full/stitched pixels with zero draw reads after
  suspend -> resume -> Back in ShowToc. Good for Defect A. Not covered: TOC label glyphs after resume;
  suspend in NeedPage/Error; TOC Select after suspend.
- `cjk_heading_latin_tail.rs`: covers capacity of non-last lines only (see Defect 1); does cover Prev and
  bookmark reopen. Not covered: carried bold/italic/blockquote indent, a heading closing mid-page, a Latin-only
  carried page containing spaces (word-wrap path), live font-size change with a carried page, English chapter
  with a single non-CJK unsupported scalar.

---

# Round 2 re-review (final-fix2): APPROVE

Read live `src/apps/reader/paging.rs` `wrap_proportional` (L643-L1040) and diffed the function body against
`git show HEAD:` (not the diff-of-diffs). Defect 1 is resolved. Ran the frozen
`cjk_heading_latin_tail` (8 passed) plus two scratch probes in a /tmp copy (deleted; repo untouched).

## (1) Emit paths: all yield start-of-line flags when `line_start_flags`
`line_start_flags = fonts.has_fallback() || initial_flags & CARRIED_FALLBACK != 0` (L664). Walked every site:
- `emit!` (L671) and tail emit (L1030): `line_flags` when `line_start_flags`.
- Image (L726-L731): text before the image emitted with the line's flags; line after the image starts with
  `styles.flags()`; `space_flags` reset.
- Newline (L763-L770), wrapping ASCII space (L897-L906) and wrapping NBSP/space in the multibyte branch
  (L826-L838): next line starts after the space with `styles.flags()`, which equals the style at its first byte
  (markers before the space belong to the previous line's replay).
- Break at `last_space` (multibyte-Latin L862-L866, ASCII word overflow L943-L946): `line_flags = space_flags`,
  captured at the space/NBSP/soft hyphen (L812, L823, L891) before any markers that follow it; replayed markers
  inside the new line are applied by draw. Correct.
- Break at char `i` (L867-L870) and word start (L950-L952): `styles.flags()` at that byte; markers just before it
  stay in the previous line. Correct.
- CJK branch unchanged (`legal_flags`). Every place that sets `last_space = line_start` also sets
  `space_flags = line_flags` (no stale value).
Executed check: nested probe (heading opened on a CJK page, then 60 x `AA \x01B BBB \x01b CC \x01I DD \x01i ` with
spaces and mid-line bold/italic markers inside the carried heading, Latin-only continuation pages): independent
python replay (flags of line k+1 == flags of line k replayed over its bytes) over 235 line transitions: 0
mismatches. Nested bold/italic/heading precedence is fine because the state is the 3-bit mask and draw uses the
same `from_flags` + replay.

## (2) No-carry English path unchanged
`line_start_flags` is false for a window with no fallback and no carried bit (caller passes 0 flags, see
paging.rs:156-167), so `emit!`/tail use `styles.flags()` exactly as HEAD. All new `line_flags`/`space_flags`/
`legal_flags` assignments are write-only on that path (read only inside `if line_start_flags` / the CJK
`legal_flags` branch). Function-body diff vs HEAD shows no layout-affecting change from this round; remaining
differences are earlier-round items (R13 decode, CJK branch, `BufferTooSmall`), which the unchanged golden
(8cb6e31a...) and regression head/tree runs cover.

## (3) Remaining carried-path scenarios
- Flags: none found (see executed check above; frozen test `closing_marker_line_*` cover closing-marker lines
  with and without spaces).
- Indent (non-blocking, pre-existing, identical on the CJK-window and original English paths): `emit!` stamps
  `indent` as of emit time, not at line start. Input `01 'Q' "臺" + "A"*300 + 01 'q' "\n"` (width 48): the last
  quote line `AA 01 'q'` gets `indent=0` while all 150 other quote lines have `indent=1` (executed). Draw positions
  the whole line by `span.indent`, so the final quote line shifts left by one indent; mid-line `QUOTE_ON`
  similarly indents the text before the marker. This is the indent twin of Defect 1, but it also occurs in
  windows that stage fallback glyphs (round 1 and earlier) and in the pinned English path, and R10's scoped
  policy is about heading/style, so I do not block on it. Fix, if wanted later, is a `line_indent` captured at each
  line start the same way as `line_flags`, applied only under `line_start_flags`.
- Carried indent across page starts itself is correct (`initial_indent` passed when `carried`, replayed in
  `record_continuation`).
- Still open from round 1 (non-blocking): unsupported non-CJK scalars also latch carry; bit 7 can appear in the
  first line's `LineSpan.flags` (draw masks it).

## Draw/SD/allocation
Round 2 touches only layout locals in `wrap_proportional`; no allocation, no SD access, no draw change.
