# T8 review fixes: implementation report

No frozen file edited. No commits. Tests were not modified.

## Files changed and why

| File | Change |
|---|---|
| src/apps/reader/mod.rs | `prepare_render` takes `ctx` (was `_ctx`). The restore-failure branch calls `self.enter_error(ctx, error)`. The title/TOC aux-failure branch calls `enter_error` only when `self.state != State::Error`, so a persistent aux error requests no redraw per render. Terminal-error recovery is unchanged. |
| src/fonts/cjk.rs | Named aux constants `AUX_SURFACES`=3, `AUX_METRIC_BUDGET`=4 KiB, `AUX_SLOT_BUDGET`, `AUX_BITMAP_BUDGET`=32 KiB (was 16 KiB), `AUX_CACHE_BUDGET`. `CjkState` gets `heading_role` (true for `new()`, false for `auxiliary()`); `prepare_visible` skips the heading role when it is false. `SurfaceFonts` gets `prepared: Option<(u8, usize, u32)>` (size, byte length, `smol_epub::cache::fnv1a`) and `prepare` returns early when the key equals `prepared` and `error` is None. `clear()` resets it, and any error leaves it None. Compile-time asserts extended (see numbers below). `CJK_STORAGE_BUDGET` raised 200 to 336 KiB. |
| kernel/src/kernel/bigbuf.rs, kernel/src/kernel/mod.rs, host/src/kernel.rs | New `FONT_GLYPHS_PSRAM_BYTES` (= `pulp_board_logic::memory::PSRAM_FONT_GLYPHS_BYTES`), re-exported next to `BigBuf`. cjk.rs asserts against it. The host stand-in kernel mod re-exports it too, because it includes bigbuf.rs by path. |
| board-logic/src/memory.rs | INVENTORY FontGlyphs row: 128 KiB changed to 224 KiB, name and note updated. The note records the internal-heap metrics/slot numbers; no new class and no new row. |
| kernel/src/kernel/scheduler.rs | Comment item 4 of the SPI invariant now says `prepare_render` may read font packs immediately before each draw. |
| src/apps/manager.rs | Comment at `draw_loading_indicator`: loading text must stay Latin literals (built-in font cannot render CJK); user content goes through the prepared path. |

## Decisions

1. **Aux heading role removed (structural).** The conservative sum with both roles per aux surface is 128 + 3 x 2 x 32 = 320 KiB, which is over the 256 KiB class. Aux surfaces now prepare only the body role. Every heading `add(.., true)` call site passes a static Latin literal ("Files", "pulp-os", "Bookmarks", "Settings"), so no CJK heading exists in an aux surface. If CJK heading text ever reached an aux surface, it would draw as the missing-glyph box, with no failure and no storage access. The aux ceiling stays at 32 KiB, which the 150-glyph test needs.
2. **Aux slot budget is now expressed as a slot count.** The brief said to leave the 4096-byte slot budget unchanged, but T-C failed on that. `PageGlyphSlot` is 24 B on riscv32 (170 slots in 4 KiB) and 32 B on the 64-bit host (128 slots). 150 slots need 4800 B on the host, so `buffer()` returned `BufferTooSmall`. The constant is now `170 * size_of::<PageGlyphSlot>()`. Target behaviour is identical to before (170 slots, 4080 B <= 4096) and the host mirrors it. This is not a test-oracle problem. It is a host-vs-target size difference the brief did not account for. Please confirm you accept it.
3. **Memo uses a 32-bit FNV-1a** (`smol_epub::cache::fnv1a`, already in the repo) plus byte length and size index. A collision would leave a stale glyph set: missing glyphs draw as boxes. There is no memory-safety impact, because draw uses only prepared slots and falls back to a rectangle for a missing glyph.
4. `CJK_STORAGE_BUDGET` is 336 KiB instead of the exact sum. Target sum is about 316 KiB plus structs. The 64-bit host adds a few KiB because of larger structs and slots, and the assert is evaluated on the host too.
5. memory.rs was not rustfmt-clean before my edit (pre-existing diff in a test at ~line 1438), so I did not format it. cjk.rs was formatted. The other touched files were already clean and are still clean.

## Memory numbers (R8)

- FontGlyphs bitmaps, worst case with the reader on the TOC: 2 x 64 KiB (reader body + heading) + 3 x 32 KiB (reader title, reader TOC, overlay) = 224 KiB. The class is 256 KiB (`PSRAM_FONT_GLYPHS_BYTES`), so headroom is 32 KiB. A compile-time assert in cjk.rs enforces `2*BITMAP_BUDGET + AUX_SURFACES*AUX_BITMAP_BUDGET <= FONT_GLYPHS_PSRAM_BYTES`.
- Files, Home and Settings clear their surfaces on exit and suspend, so they are not added. A comment in cjk.rs says so.
- Internal-heap metadata (not a class):
  - Body: metrics <= 16 KiB (32 KiB transient while growing) + 2 x 16 KiB slot tables.
  - Aux: 3 x (4 KiB metrics + ~4 KiB slots) = 24 KiB.
  - Shared: VisibleText 4 KiB + scratch 4 KiB.
  - Total about 80 KiB persistent, about 96 KiB with the transient.
  - It is recorded in the INVENTORY note and the cjk.rs comment only. Not budgeted on X4, where BigBuf has no class cap (unchanged, recorded risk).
- Total CJK assert: 2x16 + 2x(16+64) + 4 + 3 x (4 + 4 + 32) = 316 KiB, plus struct sizes, <= 336 KiB.

## Verification

- `cargo test -p pulp-host ... --test cjk_surface_fixes --test cjk_surfaces --test cjk_reader --test utf8_boundary --test utf8_malformed --test reader_failures`: exit 0.
  - cjk_reader 9/9, cjk_surface_fixes 3/3, cjk_surfaces 7/7, reader_failures 6/6, utf8_boundary 9/9, utf8_malformed 12/12.
  - The three previously red tests (T-A, T-B, T-C) are green.
- `cjk_identity`: 1 passed (`bookmark_reboot_with_replaced_pack_locates_the_same_raw_text`) and 3 failed. The failures are the T9 reds, untouched:
  - same_size_body_replacement_resume_rebuilds_pages_at_the_raw_anchor
  - same_size_heading_replacement_resume_refreshes_the_active_heading_bank
  - live_size_cycle_preserves_raw_anchor_and_rebuilds_txt_navigation
- `cargo check-x4`: exit 0, Finished. `cargo check-c61`: exit 0, Finished. Both have only the existing `LineSpan` dead-code warning, which is out of scope. The riscv32 compile-time asserts pass.
- `scripts/test-board-logic.sh`: exit 0, 321 + 3 passed, no memory test edited and none failed on the new 224 KiB row.
- Weakening gate: the five frozen files match the manifests exactly.
  - cjk_surface_fixes.rs 6683fc6d...
  - host/src/reader.rs 1df56547...
  - cjk_surfaces.rs 4fbf2233...
  - cjk_support/mod.rs c54f7693...
  - cjk_reader.rs 9812b73c...

## Unverified

- Scheduler, AppManager, Files, Home and Settings behaviour at runtime. The host does not cover them, so only `cargo check` and source reading apply.
- Memory numbers are budget arithmetic and compile-time asserts, not measured on hardware. The X4 BigBuf stays unbudgeted. The C61 degraded 16 KiB FontGlyphs class cannot hold dense CJK pages and fails recoverably.
- The X4 phase-1/phase-3 double prepare now hits the memo, but the real scheduler was not run.
