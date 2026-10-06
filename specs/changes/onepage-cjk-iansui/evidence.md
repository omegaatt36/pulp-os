# Acceptance evidence

This table records executed evidence, not a completion claim. Final gate results and artifacts are recorded below; whole-branch independent review remains separate.

| Requirement | Evidence | Status |
| --- | --- | --- |
| R1 host conversion | fontconv tests; converter provenance/license contracts in tmp-briefs/t2-contract.md | Verified; nine real generated packs, PROV/OFL/COVERAGE and install.md |
| R2 Unicode/large offsets | fontpack golden/lookup/reader tests, converter roundtrip | Verified before integration |
| R3 malformed pack | fontpack corrupt/reader_faults; real reader corrupt/error tests | Verified; final boundary results below |
| R4 explicit missing glyph | fontpack missing tests; cjk_reader absent-scalar/optional-pack boxes | Verified under disclosed compatibility policy |
| R5 size consistency | literal five body/five heading pack metrics+pixels in cjk_reader | Verified |
| R6 prepare visible glyphs | page_cache focused8; reader literal bitmap assertions | Verified core+reader+widgets (cjk_surfaces, cjk_surface_fixes); Files/Home/Settings source-inspected only |
| R7 draw zero SD reads | cjk_reader/error/metadata draw read counts+full/stitched equality | Verified reader+title/TOC+widgets (draw read counts, full/stitched equality; cjk_surface_fixes memoization); other apps source-inspected only; real full/stitched artifacts below |
| R8 memory bounds | page_cache reserved capacity accounting; board font_memory3; bounded BigBuf class source review | Budget arithmetic + compile-time asserts: FontGlyphs worst case 224 KiB ≤ 256 KiB C61 PSRAM class, inventory updated; internal-heap metadata ~80–96 KiB unmeasured; X4 BigBuf unbudgeted; C61 degraded 16 KiB ⇒ recoverable OutOfMemory for dense CJK |
| R9 recoverable failure | page_cache capacity/I/O retries; reader corrupt/read/metadata errors and reopen | Verified layers incl. reader title failure clears loading and requests redraw (cjk_surface_fixes T-A, during-load case only); final matrix below |
| R10 kinsoku | narrow mixed literal fixtures+text conservation; long inseparable group recoverable error | Verified reader |
| R11 same raw position | reader page/back/bookmark and heading carry | Verified; cjk_identity 4 GREEN and chapter identity checks |
| R12 scalar boundaries | utf8 targets; smol targets; UI label tests | Reader/parser verified; BitmapDynLabel set_text/write_str verified (cjk_surfaces) |
| R13 malformed policy | maximal-subpart decoder/reader tests; numeric entity parser tests | Verified; policy in spec/proposal |
| R14 body/heading/TOC/title/UI | T7 reader coverage; frozen cjk_surfaces7 + production path inspection contract | T8 done: runtime evidence for reader title/TOC/widgets; Files/Home/Settings/manager/schedulers via source inspection + `cargo check-x4`/`check-c61` only (no link, no hardware); independent review APPROVE-WITH-FIXES, fixes verified |
| R15 incompatible layout cache | frozen cjk_identity4: actual3RED +1bookmark control PASS | Verified all-text policy [human]; frozen English anchor acceptance GREEN; unused-bank3 GREEN; full regression head83/tree90 PASS |
| R16 acceptance artifacts | Nine real packs; 20 full/stitched PBM + 20 PNG; fixture coverage, source/pack hashes, logical read counts and installation docs | Artifacts verified; gate results below |

Red-proof and weakening reports live in tmp-briefs and progress.md. Historical
mutants remain evidence only; 70 cancelled mutants were not executed or counted
as pass. No further mutant campaign is an acceptance condition.

## Real-font observations and provenance

Artifacts are under `target/accept-iansui/`: `packs/PROV.TXT`, `packs/OFL.TXT`,
`packs/COVERAGE.TXT`, `pack-manifest.json`, `fixture-coverage.json`,
`converter.log`, `export.log`, and `snapshots/`. Install/reproduction: `install.md`.
Font SHA256 `7f1aa62e9dcbf40d0ce41a5d3f1e5ea602e66c295778ac6fefb6b84d8ed08bd5`;
OFL SHA256 `5d6d9d02d598daa2f178d0b577c55594892c5cf9c38566e2343cea049a371ac3`.
Convention version 1, pack format 1. Nine packs total 14,792,301 bytes;
each contains 12,665 scalars. Fixture 25/26 covered; only absent scalar is
𪚥 U+2A6A5. Coverage does not mean all Unicode is supported.

Actual ReaderApp observations include body, chapter title + explicit Heading,
Chinese chrome title and selected Chinese TOC at all five sizes. Each full PBM
equals stitched PBM; all ten pairs are 480×800, with PNG copies inspected.
No blank page or clipping observed; largest body fixture spans two pages.
Snapshots are observations, not newly defined correctness oracles.

| Size index | Reader prepare font ops / bytes | TOC prepare font ops / bytes | Full / stitched draw reads |
| --- | --- | --- | --- |
| 0 | 1216 / 27335 | 229 / 5139 | 0 / 0 |
| 1 | 1216 / 27767 | 229 / 5290 | 0 / 0 |
| 2 | 1216 / 28119 | 229 / 5415 | 0 / 0 |
| 3 | 1216 / 28887 | 229 / 5681 | 0 / 0 |
| 4 | 1510 / 36641 | 229 / 6059 | 0 / 0 |

Reader preparation includes opening/layout/visible preparation, not only bitmap
fetches. Requested bytes equal returned bytes in these runs. Counts are logical
VirtualStorage adapter calls, not physical SD transactions; there is no hardware
latency measurement. Body+UI bitmap budget is 224 KiB within C61 256 KiB class.
Combined CJK compile-time total <=336 KiB; internal metadata ~80 KiB persistent,
~96 KiB transient remains unmeasured. Degraded C61 16 KiB and X4 unbudgeted BigBuf
limitations remain. Host tests do not prove device live heap sufficiency.

## Exercised failure matrix

| Case | Independent exercised evidence | Outcome |
| --- | --- | --- |
| Missing scalar | fontpack missing/page_cache hollow-square tests; cjk_reader absent_scalar; real 𪚥 snapshots | Explicit size-specific box |
| Optional absent pack | cjk_reader uninstalled_pack; no-pack-policy-tests-report.md | Ready with boxes; install/reopen recovers |
| Corrupt/version mismatch | fontpack corrupt/reader_faults; cjk_reader required_pack_failures | Recoverable error, no invisible Ready text |
| Metadata capacity | page_cache metadata_capacity_failure; t6-impl-report.md actual GREEN | Old/partial pages hidden; retry recovers |
| Bitmap capacity | page_cache bitmap_capacity_failure; board font_memory | Bounded allocation/read; retry recovers |
| SD bitmap/lookup failure | page_cache lookup_and_bitmap_io_failures; cjk_reader required_pack_failures | Recoverable error; prepared draw source-free |
| SD metadata failure | cjk_metadata_failure; t7-metadata-tests-report.md actual RED, t7-impl-report.md GREEN | Genuine errors propagate, absence distinguished |
| Title load error / retry | cjk_surface_fixes; t8-fixes-tests-report.md RED, t8-fixes-impl-report.md GREEN | Loading clears, redraw requested; reopen recovers |
| Incompatible body/heading layout | cjk_identity; t9-tests-report.md actual RED, t9-impl-report.md GREEN | Rebuild from raw anchor, preserve chapter |

## English golden causality and weakening gate

Five render-strip differences predated T9 and reproduced its task-start trace.
Header-only isolated restoration reproduced the unchanged original golden pin,
3105 lines SHA256 `8cb6e31a70a32cdc05d30fab45ee0c7983da8cde6a55ba8bb45a0540f8d3f454`.
Root cause was changing absent chrome font from FONT_9X18 to generated bitmap
chrome. The controller authorized preserving legacy ASCII/None header drawing;
prepared CJK title drawing remains. `header-causality.log`,
`header-golden-green.log`, `header-cjk-green.log` record actual results.
No TOC spacing attribution or expected golden hash update occurred.

Tests/oracles were read-only for the acceptance exporter and production repairs.
The independent user-approved English oracle amendment is disclosed below. Separately
approved comment-only hygiene is disclosed in `tmp-briefs/hygiene-report.md`;
comment-stripped executable equivalence was checked. Existing actual RED/GREEN
reports supply correctness evidence; no fresh mutants or snapshot oracle was
created. No commits or spec IDs were added to code.

## Initial gate history and current follow-up

Initial `IANSUI_REQUIRED=1 CARGO_NET_OFFLINE=true scripts/check-host-boundary.sh`: exit 0,
706 host tests, no forbidden host dependencies/linker scripts, both X4 and C61
firmware links PASS. Logs: `target/accept-iansui/final-boundary.log`,
`target/accept-host.log`, `target/accept-host-x4.log`, `target/accept-host-c61.log`.
`CARGO_NET_OFFLINE=true scripts/test-board-logic.sh`: exit 0, 321+3 tests;
`target/accept-iansui/final-board.log`.
`CARGO_NET_OFFLINE=true cargo check -p pulp-fontpack --no-default-features
--target riscv32imac-unknown-none-elf`: exit 0; `final-no-std.log`.

Initial `scripts/check-reader-regression.sh both` failed the historical English
stale-offset oracle (`final-regression.log`). The user selected all-text rebuild
and R15 now records this [human] policy. Independent author retained the complete
historical function for head under cfg(not(tree)), and replaced tree stale-page
and stale-offset expectations with raw-anchor containment, scalar boundaries,
repeatable navigation and complete source conservation for both callbacks.
This is an explicitly approved oracle amendment; the original golden pin remains
unchanged. Frozen tree test actually failed current production before repair,
then passed after repair. A separate unused-bank3 RED caught eager validation of
unused CJK banks; its reviewed repair tracks only participating bank identities.

Current full regression: exit0, head83/tree90 PASS, both 3105-line original golden
hashes unchanged. Log `/private/tmp/unused-bank-fix-both.log`; 75 covering host
checks PASS in `/private/tmp/unused-bank-fix-host.log`. Independent tests, repair
and review reports: `tmp-briefs/english-anchor-{tests,fix}-report.md`,
`english-anchor-tests-review.md`, `unused-bank-{tests,fix}-report.md` and
`unused-bank-fix-review.md`. Corrected historical provenance labels the isolated
RED original-reader/current-font compatibility, not exact-original production;
actual isolated cjk.rs hash was 42f2be4e24026be1de6071ededc6462bc6c770a865e9be646ed4a4fdc8db20dc.
The actual current-production pre-repair RED supplies the required red proof.

Current controller gate: exit0,709 host tests and both X4/C61 firmware links
PASS (`target/accept-iansui/final-boundary-current.log`). Current exporter exit0:
`export-current.log`; regenerated20PBM+20PNG retain every prior pixel/hash and
logical preparation count listed above; every full/stitched draw reads0.
Durable current full regression log: `final-regression-current.log`;
covering75 log: `unused-bank-host-green.log`. Board324 and no_std PASS remain
recorded above. Historical failed/cancelled logs are retained as history, not
counted as current passes. All execution gates pass; final broad independent
review remains controller-owned.

Snapshot reproduction:

```sh
CARGO_NET_OFFLINE=true cargo run -p pulp-host --bin iansui-acceptance \
  --target aarch64-apple-darwin \
  --config 'unstable.build-std=["std","panic_unwind"]' -- \
  target/accept-iansui/snapshots target/accept-iansui/packs
python3 scripts/pbm-to-png.py target/accept-iansui/snapshots
```

Artifact hashes: `target/accept-iansui/SHA256SUMS` refreshed with current logs
and verified from its containing directory. The initial root-directory check
failed because manifest paths are relative; corrected directory check passes.
Use `(cd target/accept-iansui && shasum -a 256 -c SHA256SUMS)`.
Final execution gates and generated observations pass; broad final review pending.

## Whole-change review follow-up (2026-10-06)

Two defects found by whole-change review, each with an independent RED test, a production-only fix, and re-verification. Reports: `tmp-briefs/final-heading-tests-report.md`, `final-fix-report.md`, `final-fix2-report.md`, `final-fix-review.md` (APPROVE). Frozen hashes: `final-lifecycle-tests.sha256`, `final-heading-tests.sha256`.

| Defect | RED test (actual failure) | Fix | GREEN |
| --- | --- | --- | --- |
| TOC suspend→resume→Back leaves body CJK glyphs as boxes (R14/R7) | `cjk_lifecycle::suspended_toc_resume_back_restores_original_body_glyphs_and_pixels`: "literal 8x3 glyph missing at x=8 in line 2" | `on_suspend` sets `render_fonts_released` for `Ready` and `ShowToc` | 2/2; draw reads 0, full == stitched |
| Open heading loses style on Latin-only continuation pages (R10 boundary) | `cjk_heading_latin_tail`: heading capacity 2 vs 4 chars on page 1 (3 RED) | per-page `CARRIED_FALLBACK` bit in `style_flags` | 5/5 |
| Closing-marker line of a carried heading drawn body-style (found by review of the fix) | pixel comparison vs reference rigs: "single-letter run: page 8 line 20 \"A\" … not drawn in heading style"; "suffix \"\": page 6 line 14 \"BBB \" …" | `wrap_proportional`: one `line_start_flags` predicate, `space_flags` captured at last_space | `cjk_heading_latin_tail` 8/8 |

Final gates after round 2 (controller re-run): `IANSUI_REQUIRED=1 scripts/check-host-boundary.sh` exit 0, 719 host tests, X4 and C61 firmware links PASS (`target/final-boundary2.log`); `scripts/check-reader-regression.sh both` exit 0, head 83 / tree 90, both goldens 3105 lines sha `8cb6e31a…` unchanged (`target/final-regression2.log`); `scripts/test-board-logic.sh` exit 0. All frozen test hashes verified unchanged.

Weakening gate: one assertion inverted by user-approved spec decision. `oracle_latin_heading_is_consistent_across_pages` (pure-Latin heading keeps heading style across pages) became `pure_latin_heading_resets_style_at_page_start`, because it contradicts the original English golden pin; spec R10 boundary policy was scoped to chapters that have staged fallback glyphs. No skip, tolerance, case-table or mock change otherwise.

Known residuals (not claims): the quote-indent of the closing-marker line in a carried `Q` block is stamped at emit time, not line start (reviewer-executed probe; indent is outside the scoped R10 style policy; same behaviour already exists in fallback windows and the pinned English path); carry latches on any fallback-staged scalar (Cyrillic, symbols), at window granularity, declared in spec R10; pure-Latin pages before the chapter's first fallback window keep English per-page reset; bit 7 can appear in a first line's `LineSpan.flags` (draw masks it).
