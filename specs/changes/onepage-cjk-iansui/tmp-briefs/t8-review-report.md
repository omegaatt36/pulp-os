# T8 source review (independent, read-only)

Reviewer scope: kernel/src/kernel/{app,scheduler,scheduler_c61}.rs, src/apps/{manager,files,home,settings}.rs,
src/apps/reader/mod.rs, src/apps/widgets/{bitmap_label,button_feedback,quick_menu}.rs, src/fonts/cjk.rs,
host/src/reader.rs, diffed against /private/tmp/cjk-snap/T8-implementation-start. No file edited except this report.

## Verdict: APPROVE-WITH-FIXES

No Critical finding. The draw path is storage-free and panic-free, every render entry prepares, Latin screens
do zero font storage work and are pixel-equivalent (one exception, I5). Five Important findings need a fix or an
explicit controller acceptance before T8 is closed. The controller decision on the loading overlay is correct.

## Findings (ranked)

| # | Sev | Where | Finding |
|---|-----|-------|---------|
| I1 | Important | src/apps/reader/mod.rs:1454-1457, 1478-1481 | `prepare_render` sets `self.error`/`State::Error` directly instead of `enter_error(ctx, e)` (mod.rs:497). `_ctx` is unused, so `ctx.clear_loading()` and `mark_dirty` are skipped. Every other failure site in the reader (mod.rs:975, 992, 1068, 1092, 1104, 1146, ...) uses `enter_error`. |
| I2 | Important | src/fonts/cjk.rs:150-156 (aux ceilings), files.rs:459, reader/mod.rs:1464-1477 | Aux ceilings (4 KiB metrics = 204 entries, 4 KiB slots = 170 slots, 16 KiB bitmaps per role) are smaller than one realistic CJK page, and exceeding them replaces the whole screen (Files/Home/Settings) or kills the reading session (Reader terminal `State::Error`). |
| I3 | Important | src/fonts/cjk.rs:24-28, board-logic/src/memory.rs INVENTORY (single 128 KiB row), kernel/src/kernel/bigbuf.rs imp (X4) | R8 accounting no longer bounds the real worst case. Verified the implementer's claim (see item 4). |
| I4 | Important (perf, unmeasured) | files.rs:459, home.rs:355, reader/mod.rs:1437, manager.rs:556, scheduler.rs:324/366/396 | No memoization: each render re-reads SD for every visible CJK glyph even when nothing visible changed. Cost is O(glyphs x log2(pack size)) file-open reads, twice per X4 partial refresh (phase1 and phase3). |
| I5 | Important (undisclosed Latin change) | src/apps/reader/mod.rs:1589,1591 | TOC row pitch and visible-row count changed from `font.line_height` to `self.font_line_h` (theme-scaled 100/120/140/160 percent). Pure-Latin TOCs now render differently under themes with spacing above 100 percent. Not mentioned in the impl report. The frozen test oracle (`r.font_line_h()`) dictates it, and it makes draw agree with the event handlers (mod.rs:1225,1240,1391), but it contradicts "Latin pixel-identical". |
| M1 | Minor | reader/mod.rs:910 | `render_fonts_released = (state == Ready)` skips `ShowToc`. Suspend in TOC, resume, press Back: state is `Ready`, `cjk` is empty, flag false, so no restore and the CJK body draws as synthetic boxes. Reachability is low (Reader is only suspended below another app via session restore), but the hole is real. Fix: `matches!(state, Ready \| ShowToc)` on suspend, restore only when `state == Ready`. |
| M2 | Minor | cjk.rs:553-571 `VisibleText::append` | `try_reserve_exact` per appended scalar means one realloc per CJK character (up to ~1300 reallocs per Files render on a 160 KiB internal heap, fragmentation). Use amortised `try_reserve` with the 4096 cap checked on `len`, not `capacity`. |
| M3 | Minor | cjk.rs:618, bitmap_label.rs:213-240 | Two sources of truth for the heading flag: `VisibleText::add(.., heading: bool)` (explicit) versus `draw_prepared_aligned` (inferred from `ptr::eq(font, fonts.font(Heading))`). They agree today at all call sites (static `BitmapFont`s, verified `pub static` in build.rs:641/656), but a future label that passes the wrong bool silently draws a wrong-size box. Derive the flag in `add` with the same pointer test, or pass `Style` in both places. |
| M4 | Minor | files.rs:459-480, home.rs:355-395, settings.rs:440-472, reader TOC | Prepare and draw iterate the same visible window through two hand-copied loops (take/skip/min arithmetic). They match today (item 2), but the next edit to either drifts silently. A shared `visible_rows()` per app would remove the class of bug. The three identical error-draw blocks (`draw_surface_error(Region::new(8, CONTENT_TOP, ...))`) should be one `SurfaceFonts::draw_error`. |
| M5 | Minor | scheduler.rs:156-170 comment | "SPI bus sharing invariant" says no SD I/O outside three sites; `prepare_render` now does SD I/O immediately before phase1, phase3, and full-frame writes. Electrically fine (each device transaction is exclusive via CriticalSectionDevice), but the invariant text is now false. Update it. |
| M6 | Minor | manager.rs:556-563 | `overlay_fonts` (quick menu, button bumps) is dead weight: every label in those widgets is a static Latin literal (verified quick_menu.rs format_value, item labels, help text, action_label). It adds a third aux surface to the R8 budget for no reachable CJK. Either delete it and keep `draw`, or keep it knowingly (R14 literal compliance) and budget it. |
| M7 | Minor | files.rs:471-486 and home.rs/settings.rs | On a font-prepare error the app draws only the error line and returns; the list, selection and title vanish. Navigation still works blind. Acceptable as "recoverable" but harsh; ties to I2. |
| M8 | Minor | reader/mod.rs:1487-1493 vs old draw_chrome_text | Title with `chrome_font == None` used to fall back to mono FONT_9X18, now uses `chrome_font()`. Unreachable after `propagate_fonts`, noted for completeness. quick_menu/button_feedback default to `REGULAR_BODY_SMALL` while the overlay surface is idx 0 (impl concern 5): unreachable, same reasoning. |
| M9 | Minor | bitmap_label.rs:140-149 `write_str` | After a multibyte piece is dropped at a scalar boundary, a later shorter piece can still fill the gap (e.g. "A" after dropped "臺"), producing out-of-order text. Same class of behavior as before for ASCII; harmless for current call sites (digits, ASCII). |
| M10 | Minor | scheduler.rs:366 | Phase 3 re-prepares. If that second prepare fails after phase 1 succeeded, phase 3 writes the error screen into RED RAM while BW shows content (one-frame flash). Edge case. |

### Pre-existing, outside the T8 diff, noted only
- reader/mod.rs:829-841 `msg.len().min(32)` and `self.title` copy truncate filenames at a byte count, mid-scalar for CJK. Not T8, but it feeds the title surface (display_name falls back via `from_utf8`).

## Per-item verification evidence

### 1. Draw path purity (R7): no defect
- Every `draw`/`draw_prepared` path read: `draw_prepared_text`/`draw_prepared_aligned` (bitmap_label.rs:190-255), `LayoutFonts::advance/draw_char_fg` (cjk.rs:395-470), quick_menu `draw_with_fonts`, button_feedback `draw_with_fonts`, manager `draw` (manager.rs:566-600), reader title/TOC/body draw, Files/Home/Settings draw.
- Views are `&CjkState`/`&PageCache` only; `LayoutFonts` holds no `KernelHandle`, so storage is unreachable by type. `view()` and `FontSet::for_size` allocate nothing (`StackFmt` for error text is stack).
- Missing prepared glyph: `draw_char_fg` falls back to a stroke rectangle from `missing_glyph_metrics` (no panic, no index).
- `.unwrap()` sites on `draw_prepared`: files.rs, home.rs, settings.rs, reader title all return `Result<(), Infallible>` (fine). manager.rs does not unwrap at all: `QuickMenu::draw_prepared` and `ButtonFeedback::draw_prepared` return `()`.
- `unreachable!` for `AppId::Upload` in `with_app!` is newly reachable from `prepare_render`, but `with_app_ref!` in `draw` has the same arm; render is never reached in Upload (special mode). No new exposure.
- Pre-existing `toc_ref ... .unwrap()` and `bm_entries[idx]` index in draw are unchanged.

### 2. Prepare/draw coverage: matches, with the heading-flag caveat (M3)
- Font identity: `UiFonts::for_size(i).body/heading` and `FontSet::for_size(i).font(Regular/Heading)` are the same statics; `SurfaceFonts::set_size` clamp (`<5 else 1`) equals `body_font/heading_font/FontSet::for_size` default (Small). Initial state: apps construct `UiFonts::for_size(0)` and `SurfaceFonts::new()` size 0: consistent. `propagate_fonts` (manager.rs:603-613) keeps them in lockstep.
- Files: prepare (files.rs:459) = heading "Files" + `entries.take(count.min(page_size))` (skipped when `self.error`); draw loops `0..page_size`, `i < count`, `entries[i]`. Status label digits only; error text Latin. Match.
- Home: Menu heading + `0..item_count` labels; Bookmarks heading + `skip(bm_scroll).take(visible_lines.min(count - scroll))`; draw identical window. Match. `item_label` includes no book name.
- Settings: heading + `scroll..scroll+min(visible_items, NUM_ITEMS-scroll)` labels and `format_value`; draw identical. All static Latin.
- Reader title: prepare uses `display_name()` + `chrome_font()` + SurfaceFonts idx 0 (16 px); draw uses `cf.unwrap_or(chrome_font())` + same view. Match, unconditional of state. TOC: prepare `skip(toc_scroll).take(text_area_h / font_line_h)` + `body_font(book idx)`, `toc_fonts.set_size(book idx)`; draw uses the same `font_line_h` row count and `Style::Regular`. Match. The `'>'` marker goes through the Latin font directly.
- Quick menu/bumps: collect with the same default font as draw; chrome is idx 0 = SurfaceFonts idx 0. Match. Both use `heading=false` and `ptr::eq(font, Heading)` is false: consistent.
- Chars that `VisibleText::add` collects but `needs()` ignores (U+FFFD, U+AD, U+A0, control) only waste buffer bytes; draw renders them through the label font's `?` fallback exactly as before.
- Text that changes between prepare and draw within one render: none found (status digits, selection, inverted flag do not alter the glyph set). Across X4 phase1 to phase3, background may mutate state but then `has_redraw()` is true and phase 3 is skipped; otherwise phase 3 re-prepares.

### 3. Render-entry coverage: complete
- grep of `full_refresh|write_full_frame|partial_phase|render_full|app_mgr.draw|AppStrips` over kernel/src, src, host/src.
- X4 app renders: boot (scheduler.rs:110), phase1 (324), phase3 (366), full (396). Non-app renders: `show_boot_console` (:59), `enter_sleep` (:546), upload special mode (own UI, uses static `bumps.draw`), all static Latin.
- C61: `render_full` (scheduler_c61.rs:249-251) is the only app render, called from boot (:193) and `render` (:241); `draw_sleep_screen` is static. Retries inside `refresh_with_recovery` reuse the prepared state.
- Ordering: input dispatch, `run_background`, housekeeping, then `render_ready()` then `render()` then prepare: after the last mutation. Deferred transitions are applied after the frame and re-mark dirty, so the next loop iteration prepares again.
- Borrow soundness: `app_mgr.prepare_render(&mut self.handle())` takes a temporary `KernelHandle<'_>` that ends at the statement; `app_mgr` is a separate `&mut A`; the draw closure is created afterwards. C61 `render_full(&mut A)` reborrows `&A` for `AppStrips`. `AppManager::prepare_render` borrows `&mut self.launcher.ctx` and `&mut *self.home` as disjoint fields. `cargo check-x4`, `cargo check-c61` and `--features board-x4,wifi` all finish (below).
- Suspended work cannot evict: only `ReaderApp::background_suspended -> bg_work_tick` runs while suspended; grep shows no `cjk`/`SurfaceFonts` access outside paging.rs and the lifecycle hooks. `AppManager::prepare_render` prepares only the active app plus overlay.

### 4. Memory (R8)
Claims verified:
- INVENTORY (memory.rs ~1014) has exactly one font row: "reader font bitmap caches", 128 KiB (body + heading). Aux caches are absent.
- The compile-time assert (cjk.rs:29-32) is `2*METRIC + 2*CACHE + SCRATCH + size_of::<CjkState>() <= CJK_STORAGE_BUDGET` (200 KiB): body state only.
- X4 `BigBuf::zeroed` is a plain `Vec::try_reserve_exact` with no class cap (bigbuf.rs imp); C61 enforces the FontGlyphs class (256 KiB PSRAM, 16 KiB degraded).
- Sizes: `Entry` 20 B, `PageGlyphSlot` 24 B (derived from `fontpack` field types: Metrics = 10 B).

Who can be alive together: the reader (body `cjk` + `title_fonts` + `toc_fonts` only while `ShowToc`, cleared otherwise) plus the manager overlay. Files/Home/Settings clear on exit and suspend (files.rs:293-301, home.rs:283-288, settings.rs:345-350), so they never coexist with the reader's caches. Worst case, reader on TOC:
- Bitmaps (FontGlyphs): 128 + 3 x 32 = 224 KiB, under the 256 KiB C61 PSRAM cap (the class limit is enforced, so overflow is a recoverable error, not corruption). PSRAM sum is exactly 2048 KiB (reserve was cut from 448 to 192 KiB in an earlier task), so no spare pool.
- Internal heap metadata (not in any class): body metrics <= 16 KiB (32 KiB transient), body slots 2 x 16 KiB, aux 3 x (4 + 2 x 4) = 36 KiB, VisibleText 4 KiB, scratch 4 KiB, about 92 KiB of 162,304 B C61 internal heap. By the cjk.rs metric the declared 200 KiB CJK budget becomes 196 + 3 x 48 = 340 KiB in the worst case, so the assert no longer describes the bound.
- X4: heap is 174,592 B; 92 KiB metadata plus up to 224 KiB bitmaps can exceed it. Protection is `try_reserve_exact` only, and a failing title prepare (small, made after the body) turns into terminal `State::Error` (I1/I2).
- C61 degraded (no PSRAM): one shared 16 KiB FontGlyphs class: a dense CJK page cannot fit and title/TOC compete with the body. Recoverable, not a crash (not changed by T8, but T8 adds more consumers).

Smallest correct fix (not implemented):
1. cjk.rs: name the aux ceilings (`AUX_METRIC/SLOT/BITMAP_BUDGET`), use them in `auxiliary()`, add `const AUX_SURFACES: usize = 3` (title, TOC, overlay; or 2 if M6 deletes the overlay) and extend the `const _: () = assert!` to include `AUX_SURFACES * (2*AUX_METRIC + 2*AUX_CACHE)`; set `CJK_STORAGE_BUDGET` to the honest sum.
2. memory.rs: change the INVENTORY row to `128 + AUX_SURFACES*32 = 224 KiB` and its note; the existing test `r14_the_current_reader_allocations_fit...` still passes (224 <= 256).
3. If a smaller footprint is wanted, delete `overlay_fonts` (M6), saving one surface and 32 KiB of budget. Do not shrink aux ceilings further: I2 shows they are already too small for real pages.

### 5. Latin preservation: confirmed
- `VisibleText::add` emits the style marker only when an unsupported scalar is appended (cjk.rs:541-551); Latin labels add zero bytes. `SurfaceFonts::prepare` then runs `stage_metrics` with an empty window (no `open`) and `prepare_visible` with count 0 for both roles: no directory open, no read. Prior CJK buffers are freed (`body = None`), `metrics` keeps capacity (<= 4 KiB per surface) until lifecycle `clear()`. Reader restore on a Latin page: same, zero I/O.
- Pixel equivalence for Latin: `draw_prepared_aligned` computes the same width (sum of advances), the same `alignment.position(region, Size(width, line_height))`, same baseline `pos.y + ascent`, same fill as `draw_bitmap_text`/`draw_chrome_text`. The host test `prepared_static_and_dynamic_labels_keep_latin_style_alignment_and_inversion` covers regular/bold/italic/heading. Exception: I5 (TOC pitch).
- Per-render cost (I4): reader title/TOC, Files rows, Home bookmarks re-prepare on every render including selection-only changes and loading-percent updates. Each `ReadAt` probe is `read_app_subdir_chunk` = open _PULP, open FONTS, open file, read, close (storage.rs:485-499), and `find` does one probe per binary-search step (fontpack reader.rs:110-134). A 10-glyph title is on the order of 250 file-open reads per prepare, twice per X4 partial. Not measured; the structure is verified. Smallest fix: keep a hash of `(size, VisibleText bytes)` in `SurfaceFonts` and skip `prepare` when equal and `error.is_none()`; clear it in `clear()`. Do the same for the reader body restore (already guarded by the flag).

### 6. Dead-code warning (src/apps/reader/mod.rs:162): harmless
- Style info is consumed. Body draw (mod.rs:1749-1765) rebuilds `StyleState::from_flags(span.flags)` per line and applies markers; layout (`paging.rs:584-601`, `:892-895`) writes `span.flags` from `styles.flags()`; `prepare_page_fonts` (paging.rs:143-145) passes `span.flags` to `mark_visible_with_flags`. `StyleState::from_flags` masks with `& 7`, so `FLAG_IMAGE` (1<<3) is ignored by it and still handled by `is_image()`.
- The warned items (`FLAG_BOLD/ITALIC/HEADING`, `LineSpan::style`, `pack_flags`) are superseded helpers with no callers in firmware. The host build does not warn because host/src/apps.rs wraps the included source in `#[allow(dead_code)]` (host/src/apps.rs:6-12); host tests exercise the same `StyleState` path as firmware. No T7 defect. Per project policy (remove obsolete paths), delete them.

### 7. Lifecycle
- Set only when `Ready` on suspend (mod.rs:910); cleared by `on_enter` (:860) and by a font-size-changing `on_resume` (:929). Restore runs in `prepare_render` before any draw (every render path prepares first), so a released cache is never drawn while `Ready`. The restore stages `buf[..buf_len]`, a superset of the `text_len`-trimmed window used at load (only an incomplete UTF-8 tail differs, ignored by `needs`), so it cannot exceed the metric budget the load already satisfied.
- Holes: M1 (suspend in `ShowToc`); I1 (error path skips `clear_loading`/`mark_dirty`, and when `State::Error` persists every render re-prepares the title and overwrites `self.error`, replacing an earlier non-font error with the font error); a title or restore failure is terminal for the book (recoverable only by leaving), which is harsh for a chrome-only failure (see I2).
- Resume with a theme change but no size change does not re-wrap (pre-existing, outside T8).

### 8. BitmapDynLabel truncation (R12): correct
`scalar_prefix`: `n = min(len, cap)`, then back off to `is_char_boundary`; `0` is always a boundary so it terminates. Hand-checked: capacity 7 on `A臺𠮷Z` (offsets A0, 臺1-3, 𠮷4-7): 7 inside 𠮷, back to 4, yields `A臺`; exact 7 bytes kept; capacity 3 against a leading 4-byte scalar yields 0; `write_str` uses `N - self.len` (invariant `len <= N`). `text()` uses `from_utf8(..).unwrap_or("")` and cannot panic. Frozen tests `dynamic_label_*` pass.

## Commands run
- `shasum -a 256 host/tests/cjk_surfaces.rs host/tests/cjk_support/mod.rs host/tests/cjk_reader.rs` equals the first three lines of tmp-briefs/t8-tests-boot-root.sha256 (4fbf2233..., c54f7693..., 9812b73c...): MATCH.
- `CARGO_NET_OFFLINE=true cargo check-x4`: exit 0, "Finished release", 1 warning (item 6). `cargo check-c61`: same. `cargo check --release --target riscv32imc-unknown-none-elf --features board-x4,wifi`: Finished. (Cargo cache hits; sources unchanged since the implementer's run.)
- `cargo test -p pulp-host --target <host> --config 'unstable.build-std=["std","test"]'`: `cjk_surfaces` 7/7, `cjk_reader` 9/9, `utf8_boundary` 9/9, `utf8_malformed` 12/12; `cjk_identity` 1 pass, 3 fail (exactly the three T9 reds named by the implementer: same_size_body_replacement_resume..., same_size_heading_replacement_resume..., live_size_cycle_preserves_raw_anchor...). `cargo test -p pulp-board-logic`: 321 + 3 passed.
- `rustfmt --check --edition 2024 --config skip_children=true` on all 13 changed files: clean.

## Implementer claims I could NOT verify
- Any runtime behavior of the schedulers, `AppManager`, Files, Home, Settings (host excludes them): source reading plus `cargo check` only.
- Firmware link/flash build, hardware heap/timing, SD read latency (I4 numbers are structural estimates).
- "Snapshot and git HEAD versions are rustfmt-clean" (I checked only the current tree).
- The implementer's per-surface byte ceilings are code ceilings I recomputed from the constants; none measured.
- That `propagate_fonts` runs before the first render on every boot path (manager.rs:203/340/511 exist; I did not trace each boot branch). It matters only for M8.

## Controller decision on the loading overlay
Agree. All `set_loading` messages are Latin literals or `"Caching N/M"` built from integers (reader/mod.rs:473-493, 875-1121); `ctx.message()` filenames are never shown in the overlay. The original FONT_9X18 draw is pixel-preserving. Leave a one-line comment at `manager.rs` where `draw_loading_indicator` is called stating the overlay is Latin-only by construction, since nothing enforces it.

## Suggested fix list (smallest set)
1. I1: in reader `prepare_render`, use `ctx` and call `self.enter_error(ctx, e)` guarded by `self.state != State::Error` (avoids a dirty-loop); keep the earlier error if one exists.
2. I2: raise aux ceilings to what a page needs or make aux failures non-terminal (TOC failure returns to `Ready` with an error line; title failure draws boxes and logs). Pair with I3 so the raised ceilings are budgeted.
3. I3: inventory plus assert update as above.
4. I4: memoize per surface on `(size, bytes hash)`.
5. I5: controller to accept explicitly or restore `font.line_height` and adjust the frozen oracle through the amendment process.
