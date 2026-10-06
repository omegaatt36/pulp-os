# T8 implementation report

Starting point: the previous worker's unverified T8 tree (baseline snapshot
`/private/tmp/cjk-snap/T8-implementation-start`). Nothing was discarded. No tests,
specs, progress files or commits were touched. No mutants.

## Files changed by this pass (relative to the snapshot, above the earlier worker's edits)

| File | Why |
| --- | --- |
| `kernel/src/kernel/scheduler.rs` | rustfmt only: fixes the broken indentation of the four `prepare_render` call sites. |
| `src/apps/manager.rs` | Loading overlay reverted to the original `draw_loading_indicator` (mono FONT_9X18) and removed from visible-text collection (see inspection bullet 5); removed `loading_text`. rustfmt. |
| `src/fonts/cjk.rs` | `VisibleText::add` emits the style marker only when an unsupported scalar is actually appended (Latin-only labels add zero bytes, so the 4096 B bound is not consumed by markers). rustfmt. |
| `src/apps/reader/mod.rs` | `render_fonts_released` is now set on suspend only when the reader is `Ready`, and cleared on `on_enter` and on a font-size-changing `on_resume`, so it does not trigger a redundant restore (extra SD reads). rustfmt. |
| `src/apps/widgets/quick_menu.rs`, `button_feedback.rs` | `collect_text` now collects with the same default font that `draw` uses when no chrome font is set (was a different font than the one drawn). rustfmt. |
| `src/apps/files.rs`, `home.rs`, `settings.rs`, `widgets/bitmap_label.rs` | rustfmt only. |

rustfmt was applied to touched files only, with `rustfmt --edition 2024 --config skip_children=true`.
Baseline check: the snapshot and `git show HEAD:<file>` versions of all these files are rustfmt-clean
(0 diffs), so the repo does format this way. The reformat touched only the earlier worker's added
lines; untouched code is unchanged (`diff -u` against the snapshot shows only the prepare/draw hunks).

## Commands and results

All run from `/Users/raiven_kao/dev/pulp-os` with the pinned toolchain (`nightly-2026-09-22`, targets
`riscv32imc-unknown-none-elf` and `riscv32imac-unknown-none-elf` installed).

| Command | Exit | Key output |
| --- | --- | --- |
| `CARGO_NET_OFFLINE=true cargo check-x4` (= `check --release --target riscv32imc-unknown-none-elf --features board-x4`) | 0 | `Finished release` ; 1 warning (below) |
| `CARGO_NET_OFFLINE=true cargo check-c61` (= `check --release --target riscv32imac-unknown-none-elf --features board-onepage-c61`) | 0 | `Finished release` ; same 1 warning |
| `CARGO_NET_OFFLINE=true cargo check --release --target riscv32imc-unknown-none-elf --features board-x4,wifi` | 0 | `Finished release` |
| `cargo test --no-fail-fast -p pulp-host --target $host --config 'unstable.build-std=["std","test"]' --test cjk_surfaces --test cjk_reader --test cjk_identity --test utf8_boundary --test utf8_malformed` | 101 (expected) | see below |

Only `cargo check` was run. `cargo build` (link/flash image) was not run for either firmware, and no
hardware or emulator run exists.

The one warning (both boards): `associated items FLAG_BOLD, FLAG_ITALIC, FLAG_HEADING, style, pack_flags
are never used` at `src/apps/reader/mod.rs:162`. It comes from the earlier T7 reader work, not from T8; not fixed here.

Test results:
- `cjk_surfaces`: 7 passed, 0 failed.
- `cjk_reader`: 9 passed.
- `utf8_boundary`: 9 passed.
- `utf8_malformed`: 12 passed.
- `cjk_identity`: 1 passed, 3 failed. The failures are exactly the three expected T9 reds:
  `same_size_body_replacement_resume_rebuilds_pages_at_the_raw_anchor`,
  `same_size_heading_replacement_resume_refreshes_the_active_heading_bank`,
  `live_size_cycle_preserves_raw_anchor_and_rebuilds_txt_navigation`. Not touched.
- `board-logic` tests not run: `board-logic/src/memory.rs` was not modified by this pass.

The host does not compile or run the schedulers, manager, Files, Home or Settings. Everything about those paths
below is source inspection plus a successful firmware `cargo check`, not runtime evidence.

## Production inspection (contract "Required production inspection", 7 bullets)

| # | Bullet | Verdict | Evidence |
| --- | --- | --- | --- |
| 1 | Files directory title and visible filename rows use the shared provider, including reload/reselection | Satisfied (source only) | `src/apps/files.rs:456-465` `prepare_render` adds heading "Files" and `entries[..count.min(page_size)]` (the same set `draw` iterates: `files.rs:~519`, `entries[i]` for `i < count`), then `label_fonts.prepare`. `draw` uses `draw_prepared` for title and rows. `prepare_render` runs after events/background in every render, so reload (background `dir_page`) and reselection are covered; `set_ui_font_size` -> `set_size`. |
| 2 | Home bookmark titles use it at the selected UiFonts size | Satisfied (source only) | `src/apps/home.rs:~348-366` collects bookmark `display_name` for `bm_scroll..+bm_visible_lines` (matches the draw loop at `home.rs:~560-575`) with `ui_fonts.body`; menu state collects item labels. `propagate_fonts` (`manager.rs:602`) calls `set_ui_font_size` -> `label_fonts.set_size` so px follows the selected index. Font-size clamp is identical (`UiFonts::for_size` default = Small, `SurfaceFonts::set_size` default = 1). |
| 3 | Settings labels and formatted values at their actual body/heading size | Satisfied (source only) | `src/apps/settings.rs:~434-445` collects the heading and, when loaded, each visible item label and `format_value` output with `ui_fonts.body`, same index range as `draw`. All Settings text is fixed Latin, so in practice preparation does no storage work. |
| 4 | Reader title chrome prepared even while body is loading/empty | Satisfied (source only; title drawing is also runtime-tested by `book_title_uses_16px_chrome...`) | `src/apps/reader/mod.rs:~1450-1456` prepares `title_fonts` unconditionally (outside the `State::Ready` branch) from `display_name()` with the chrome font. `draw` (`mod.rs:~1470`) draws the title via `draw_prepared` in every state. TOC labels prepared when `ShowToc` (`mod.rs:~1457-1468`), after the event that opened/scrolled the TOC, from the same `toc_scroll..+visible` window the draw loop uses. |
| 5 | Quick-menu labels, button feedback, loading overlay in the same pre-render lifecycle | Quick menu and button feedback: satisfied. Loading overlay: deliberately NOT prepared (deviation, needs controller decision) | `manager.rs` `prepare_render` collects `quick_menu.collect_text` (only when open, `quick_menu.rs:293`) and `bumps.collect_text` into `overlay_fonts`, then `draw` calls `draw_prepared` for both. Loading text comes only from literals (`"Opening"`, `"Loading"`, `"Caching"`, `"Indexing"`, `"Loading page"`, `"Caching N/M"`, `"Caching images ..."`, `reader/mod.rs:473-493,874-1119`) and is drawn by the built-in mono FONT_9X18, which cannot render CJK. The earlier worker had replaced it with a chrome-font label, changing Latin pixels with no test; I restored the original draw. If the controller wants loading text in the same prepared path, say so. |
| 6 | Every X4/C61 render entry prepares after last visibility mutation; suspended work cannot evict | Satisfied (source only; compiles, not run) | X4 `kernel/src/kernel/scheduler.rs`: boot full refresh `:110`, partial phase1 `:324` (before `partial_phase1_*`), partial phase3 `:366` (after `busy_wait_with_background` ran `run_background` and deferred `dispatch_event`; phase3 is skipped when `has_redraw()`/deferred, and the deferred `apply_transition` happens afterwards and re-marks dirty, so the next loop render prepares again), promoted/full `write_full_frame` `:396`. Non-app screens (`show_boot_console`, `enter_sleep`, `run_special_mode` upload) are static Latin or the wifi upload app's own UI and do not use the prepared path. C61 `kernel/src/kernel/scheduler_c61.rs:249-251`: `render_full` is the only render entry (called by `boot` `:193` and `render` `:241`; no strip/partial path) and calls `prepare_render` first; `refresh_with_recovery` retries reuse the already-prepared state. Suspension: the only suspended-background work is `ReaderApp::bg_work_tick` (image recv states, `reader/mod.rs:507-540`), which touches no font state; `AppManager::prepare_render` only prepares the active app plus overlay. Reader on suspend clears body/title/TOC caches and, if it was `Ready`, restores them at the next `prepare_render` (`mod.rs:~1437-1448`). |
| 7 | Missing pack, corrupt pack, SD failure, capacity failure recoverable; guards released before draw | Satisfied (source only for non-reader surfaces; reader title/TOC/body runtime-tested for missing-pack) | `SurfaceFonts::prepare` stores `error` and clears the state; every app's `draw` shows `draw_surface_error` (bounded `"Font: {err}"` line) instead of panicking (files/home/settings/manager). Reader surfaces publish `State::Error` with the error (`mod.rs:~1469-1472`; also if restore after resume fails). Absent optional pack -> `Source::Empty` synthetic-box path in `cjk.rs::open` (only `Ok(None)` from `optional_file_size_app_subdir`); `Err` propagates. `Source::Installed` borrows `&mut KernelHandle` only inside `open`/`prepare_*`, so no storage borrow outlives the prepare call; draw takes `&PreparedFonts`. Caveat: reader title/TOC/restore errors move the reader to the terminal `State::Error`; leaving the book is the recovery, there is no automatic retry. |

## Memory analysis (not a claim of hardware sufficiency)

Sizes on rv32: `Entry` 20 B, `PageGlyphSlot` 24 B (derived from `fontpack` field types, not measured).

Ceilings enforced by code budgets (reached only when CJK glyphs are visible):

| Holder | Internal heap (metrics Vec + slot boxes) | BigBuf `FontGlyphs` bitmaps |
| --- | --- | --- |
| Reader body `CjkState` | metrics <= 16 KiB (up to ~32 KiB transient while growing), slots <= 16 KiB x 2 (body + heading) | <= 64 KiB x 2 = 128 KiB |
| Reader title `SurfaceFonts` | metrics <= 4 KiB, slots <= 4 KiB x 2 | <= 16 KiB x 2 |
| Reader TOC `SurfaceFonts` (ShowToc only) | same as title | same as title |
| Manager overlay `SurfaceFonts` | same as title | same as title |
| Files / Home / Settings `SurfaceFonts` | same as title, only for the active app; cleared on exit and suspend, so never alive together with the reader's | same as title |
| `VisibleText` (transient) | <= 4096 B (+ allocator headroom), dropped before the next surface is prepared | none |
| `prepare_visible` scratch (transient) | <= 4 KiB | none |

Worst case with the reader active on its TOC: internal 16 + 32 + 12 + 12 + 12 = 84 KiB persistent, plus up to ~8 KiB transient, i.e. about 92 KiB.
Title, TOC and overlay only use the body role in practice (no heading text), so the realistic ceiling is about 16 + 32 + 8 + 8 + 8 = 72 KiB + transient.
`FontGlyphs` worst case is 128 + 32 + 32 + 32 = 224 KiB (176 KiB realistic: heading role unused by the three aux surfaces).

Comparison with budgets:
- C61 PSRAM: `PSRAM_FONT_GLYPHS_BYTES` = 256 KiB (`board-logic/src/memory.rs`), so the 224 KiB ceiling fits, with 32 KiB margin. The class sum plus reserve is asserted equal to 2 MiB, so there is no spare pool.
- C61 degraded (no PSRAM): `INTERNAL_FONT_GLYPHS_BYTES` = 16 KiB is shared by ALL FontGlyphs allocations. The reader body+heading caches (up to 128 KiB each role limit) cannot fit, and the title/TOC/overlay caches compete with them for the same 16 KiB. A dense CJK page at 35 px (about 175 B per glyph, so about 100 distinct glyphs) already exceeds 16 KiB. This degrades to the recoverable `OutOfMemory` font failure, not a crash, but CJK reading in degraded mode is effectively unsupported.
- C61 internal heap: `INTERNAL_HEAP_BYTES` = 98,304 + 64,000 = 162,304 B. About 84-92 KiB of CJK metadata is roughly 53-57 percent of it, on top of everything else the reader already holds. `INVENTORY` in `memory.rs` lists only the 128 KiB FontGlyphs bitmap item; the CJK `Vec`/`Box` metadata (metrics, slot tables, VisibleText, scratch) and the three auxiliary surface caches are NOT in the inventory, and `cjk.rs`'s compile-time assert (`CJK_STORAGE_BUDGET` 200 KiB) covers only the body `CjkState`, not the auxiliary ones. So the budget tables understate worst-case use. I did not edit `memory.rs`; the controller should decide whether to inventory these or lower the auxiliary ceilings.
- X4: `BigBuf` is a plain heap `Vec` with no class limit; heap is 110,592 + 64,000 = 174,592 B. The 84-92 KiB internal plus up to 224 KiB bitmaps can exceed the whole heap. Protection is only the fallible `try_reserve_exact` (recoverable `OutOfMemory`), which could starve other allocations. Not verified on hardware.

Nothing was measured on hardware; all figures are code ceilings.

## Weakening gate

`shasum -a 256 host/tests/cjk_surfaces.rs host/tests/cjk_support/mod.rs host/tests/cjk_reader.rs` equals the first three lines
of `specs/changes/onepage-cjk-iansui/tmp-briefs/t8-tests-boot-root.sha256`:

```
4fbf2233c553c08b0f2c2aded9574c34366f368a0ef61800be7f9923f78c3a47  host/tests/cjk_surfaces.rs
c54f7693bfaec43d1c621ac935f79993b43b91c00e6c1b7ed034bb33a81d7393  host/tests/cjk_support/mod.rs
9812b73ceb45af30853011bccacd909c05c094a549fd36c42b69adba75daa82b  host/tests/cjk_reader.rs
```

Result: byte-identical (match).

## Concerns and open items

1. Loading overlay is intentionally not routed through the prepared path (bullet 5). Decision needed from the controller.
2. Reader title/TOC/restore failures set the sticky `State::Error`, including transient SD faults during a title prepare mid-load. Acceptable as "recoverable by leaving the book", but not auto-retrying.
3. `LineSpan` dead-code warning (T7 leftover) on both boards.
4. Memory ceilings above (degraded-mode 16 KiB shared class; internal heap share; metadata missing from the inventory/assert).
5. `quick_menu`/`button_feedback` fall back to `REGULAR_BODY_SMALL` when no chrome font is set while the overlay surface is sized at index 0; in practice `propagate_fonts` always sets the chrome font first, so this is unreachable, but the two defaults are inconsistent in principle.
6. Unverified: no real scheduler, manager, Files, Home or Settings execution (no host coverage by design); no firmware link/flash build; no hardware timing or heap measurement.
