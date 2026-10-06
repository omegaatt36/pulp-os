# T7 implementation and final verification

Final focused reader tests pass13/13; affected UTF-8 boundary/malformed tests
pass21/21. The English golden remains the original pinned3105-line transcript,
SHA256 `8cb6e31a70a32cdc05d30fab45ee0c7983da8cde6a55ba8bb45a0540f8d3f454`.
Implementation ownership is ready to transfer to T8; controller acceptance and
independent final source review remain separate.

## Resulting behavior

`src/fonts/cjk.rs` owns bounded metrics and body/heading PageCaches. Latin
`FontSet` remains Copy/static; real Latin glyphs and existing styles remain
selected from flash. Unsupported valid scalars resolve the selected regular
CJK body/heading size, including supplementary scalars, u16 advances and signed
bearings. U+FFFD retains the existing decoder replacement path. An explicitly
selected mono path retains its cell geometry. CJK does not require optional
flash Latin font assets; their absence uses built-in9x18 Latin drawing in a
mixed fallback view.

Metrics are staged per copied window before wrapping. Indexing, restored-offset
scans and visible pages share the same wrapper. Only final visible spans mark
bitmap requests; index/restore traversal performs no font bitmap reads. Get/draw
borrow owned RAM only. No KernelHandle, PackReader, unsafe borrowed source or
leaked storage lives in the app.

The controller explicitly amended optional no-pack policy after independent
RED: an uninstalled pack draws a synthetic size-specific hollow box with its
matching advance and publishes Ready. A valid pack lacking a scalar uses the
same T3 box. An installed corrupt pack or any real metadata/read failure remains
a recoverable error. The narrow optional metadata operation returns Option:
only actual FAT NotFound yields None. OpenFile/OpenDir and NoCard errors are not
absence signals. Synthetic preparation supplies a validated zero-record header
to PackReader and the same PageCache missing-glyph path, without any SD source.

Mixed wrapping tracks legal scalar boundaries for both prohibited punctuation
sets; minimal groups can overhang. When a trailing group is incomplete at the
read boundary, preceding completed lines can be published and the group is
retried from its own raw offset. A group filling the bounded window without any
safe progress returns recoverable BufferTooSmall. EOF/index completion is based
on consumed offsets relative to known total bytes, including underfilled windows.
Short reads are completed with bounded positioned reads before layout; a
premature zero read returns ReadFailed rather than false EOF or extra partial
pages.

## Style continuation and immutable drawing

Shared `fonts::StyleState` independently tracks bold/italic/heading markers and
keeps heading precedence until its own closing marker. Metrics scanning,
wrapping, continuation scanning and drawing use this parser. Nested emphasis
therefore never selects an unprepared body glyph during an active heading.

`PageState` adds style_flags and indents arrays, each512 bytes, alongside the
existing raw offset table. Scanning only the consumed raw prefix (skipping image
payloads) records state for the next offset. RAM chapter indexing selects each
page's index during its scan and returns to page0 afterward; backward loads use
the saved state; rebooted bookmark scans recreate it from byte0. Staging and CJK
wrapping receive exactly this inherited state. Latin-only windows use the legacy
wrapping initial state, preserving the pinned English layout and styles.

Mixed spans store state at their legal break, so closing heading markers do not
change the selected font for earlier characters. Visible marking validates every
needed staged key before Ready. Get/draw have no missing-preparation expect or
panic: an invariant failure remains drawable as a pure hollow box without I/O.

## Memory placement and limits

The controller extended ownership to the minimal board-memory/BigBuf seam after
architecture inspection identified the internal-only global allocator.

- Actual-sized bitmap backing uses `BigBuf`/`BufClass::FontGlyphs`, mapped to the
  private C61 PSRAM allocator by `MemClass::FontGlyphs`. Its global shared class
  limit is256 KiB in Ready mode and16 KiB in degraded mode. The PSRAM reserve
  changes448 to192 KiB; all existing class limits remain unchanged. Inventory
  identifies the reader's128 KiB maximum bitmap sum.
- Each of two bitmap roles is at most64 KiB; each typed internal slot table is
  at most16 KiB; temporary codepoints are at most4 KiB. Allocation uses actual
  visible counts/bitmap bytes, never fixed per-role buffers.
- Persistent metric Vec capacity is at most16 KiB. The scanner collects UNIQUE
  sorted `(pixel_size, scalar)` keys with fallible exact reserve and reuses the
  capacity across windows. Growth can temporarily hold old and new storage,
  bounded by32 KiB and included in the ceiling.
- A compile-time assertion includes state/cache owners, both full caches,
  metric growth peak and scratch within200 KiB per state. Internal peak is
  approximately68 KiB plus bookkeeping, with the two bitmap owners in PSRAM.
  Separate page-continuation bookkeeping adds1 KiB of bounded app state.
- Budget, allocation, missing-preparation and storage failures return errors.
  Both old caches are released before replacement. Exit/reopen releases all
  owned font storage. BigBuf AsRef/AsMut expose the complete backing; its X4
  FontGlyphs allocation includes actual Vec capacity in the exposed length.

## Actual commands and evidence

All Cargo host commands below use:

```
cargo test -p pulp-host --target aarch64-apple-darwin \
  --config 'unstable.build-std=["std","test"]'
```

Pre-implementation `--test cjk_reader -- --test-threads=1`: exit101,
0 passed/8 failed for the original missing behaviors. Initial implementation
7/8 exposed the final-heading-line marker issue; correction reached8/8.

Independent authors added and froze heading-page, nested-style, long-group and
metadata-fault cases before fixes. Implementer independently replayed each RED:
heading-page missing23px bitmap; nested rendering panic from wrong style;
long-group Ready instead of Error; metadata injection consumed but Ready instead
of Error. The separate optional-pack amendment also had independently replayed
RED: Error versus its new Ready+literal box expectation.

Latest focused replay after the metadata seam:

```
[host command above] --test cjk_metadata_failure --test cjk_reader \
  --test cjk_heading_pages --test cjk_nested_styles --test cjk_long_group \
  -- --test-threads=1
```

Exit0, metadata1/1 + amended reader9/9 + heading1/1 + nested1/1 + long-group1/1.
No warnings. All box, install/reopen, corrupt/read, literal advance/pixel,
conservation, navigation/bookmark and source-free draw assertions execute.

Latest affected legacy replay after the independent literal box-advance helper
amendment and final metadata seam:

```
[host command above] --test utf8_boundary --test utf8_malformed \
  -- --test-threads=1
```

Exit0, boundary9/9 + malformed12/12, no warnings. Before the author amendment,
exactly two boundary and three malformed pixel assertions used obsolete static
question-mark advances for valid unsupported CJK; raw/scalar/replacement tests
already passed. Author changed only the helper's unsupported-character advance
oracle, retaining all tolerances, cases and assertion expressions; see
`t7-metadata-tests-report.md`. Implementer did not edit any test or helper.

Additional pertinent replay on the resulting policy/style source:
`--test utf8_measure --test utf8_title --test utf8_residue --test utf8_epub
-- --test-threads=1`: exit0, measure7/7 + title12/12 + residue4/4 + epub2/2.
The metadata seam subsequently changed no exercised layout/decoder path.
Earlier pertinent paging24/24, bookmarks15/15, settings19/19 and render39/39
passed. A new EOF-gate short-read paging failure was then independently observed
and fixed by completing short reads; targeted
`--test paging a_short_read_never_corrupts_the_text_that_is_shown
-- --test-threads=1` exits0,1/1. Existing unrelated unused_mut test warnings remain
in bookmarks/settings. No exhaustive unchanged decoder suite was repeated.

Memory tests:

```
scripts/test-board-logic.sh --test font_memory
scripts/test-board-logic.sh --lib memory::tests
```

Before mapping, first command RED exit101 with26 absent public API diagnostics;
post-mapping GREEN3/3. Latest existing memory tests GREEN49/49. An independent
author corrected the old pool-fill setup (768 KiB ChapterText plus48 KiB ZipToc,
same816 KiB total) after its RED; all assertions remain unchanged.

Latest final English pin check after the optional metadata shim:

```
CARGO_NET_OFFLINE=true scripts/check-reader-regression.sh tree --golden-only
```

Exit0,3105 lines, exact original SHA256 above. Earlier style carry applied to
Latin-only windows changed the transcript; restoring their legacy initial state
corrected it. No pin, golden source or expected transcript was changed. Each pin
rerun followed an actual source/interface correction, not an unchanged repeat.

## Freeze and scope audit

Latest metadata, heading, nested and long-group SHA256 manifests verify OK.
The amended no-pack manifest's tests and T7 contract verify OK. Its historical
T8 contract digest differs because the independent metadata author appended a
later approved contract addendum; the latest metadata manifest's T8 contract
verifies OK. Historical snapshots/manifests were preserved, not overwritten.
Original CJK support and frozen Reader Rig/probe files were not modified by this
implementer. `git diff --check` passes.

No mutants, reference implementation, skips, weakened tolerances, spec IDs in
new production code or commits. Test-policy/helper amendments and the old pool
fixture correction belong to independent authors under explicit controller
instructions. No T8 UI/TOC/title/scheduler work or T9 identity work was added.

## Public seams for the next tasks

`CjkState::new`, `clear`, `stage_metrics`, `stage_metrics_with_flags`,
`begin_visible`, `mark_visible`, `mark_visible_with_flags`, `prepare_visible`,
`prepare_text`, `view` and `get(pixel_size,char)`. Visible marking returns Result
and validates stage completeness. `prepare_text` aggregates a single visible
text request; T8 must aggregate requests or keep separate body/chrome states.

`LayoutFonts` (`PreparedFonts` alias) offers static Latin font access, line
height, widened advance and foreground-aware draw_char/draw_char_fg.
`StyleState::from_flags`, `apply_marker`, `flags` and `style` supply shared marker
semantics. No view operation can reach SD. KernelHandle's new
`optional_file_size_app_subdir` returns Result<Option<u32>> with definitive
absence separated from real metadata failures.
