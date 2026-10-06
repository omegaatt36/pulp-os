# T8 review-fix tests report (independent test author)

File added: `host/tests/cjk_surface_fixes.rs` (tests are auto-discovered; `host/Cargo.toml` lists no test targets, so no registration). No existing test or support file modified. No production code read or edited.

Replay: `host=$(rustc -vV | sed -n 's/^host: //p'); CARGO_NET_OFFLINE=true cargo test -p pulp-host --target "$host" --config 'unstable.build-std=["std","test"]' --test cjk_surface_fixes`

## Harness accessors added (host/src/reader.rs, Rig only, forwarders, no logic)
- `loading_active(&self) -> bool` -> `AppContext::loading_active()`
- `has_redraw(&self) -> bool` -> `AppContext::has_redraw()`
- `take_redraw(&mut self)` -> `AppContext::take_redraw()` (result discarded)
- `enter(&mut self, name)` -> `on_enter` exactly as `Rig::open` does it, minus the final `settle()`; lets the test interleave `prepare_render` and `idle(1)` the way the scheduler renders between background ticks while loading.
The AppContext method names were not read from kernel/src; they were guessed from the task vocabulary and confirmed by compiling. Their semantics (loading flag, pending-redraw flag, consume redraw) were confirmed empirically: after `take_redraw`, `has_redraw` is false.

## T-A corrupt_installed_title_pack_during_load_ends_in_error_with_loading_cleared_and_redraw_requested
Requirement: R9/R3 (installed but corrupt pack -> recoverable font preparation failure); reviewer finding I1 (every other reader failure path clears loading and marks dirty).
Scenario: real EPUB with OPF title 臺灣𠮷, Latin body/TOC, installed `_PULP/FONTS/F00016.PFN` = 200 bytes of 0xA5. `enter`, then loop {take_redraw; prepare_render; stop if phase != Loading; idle(1)} so the redraw flag observed at the end can only come from the failing prepare.
Expected values: all boolean, from requirement text (phase == Error, error present, loading_active == false, has_redraw == true). No numbers from the implementation. Sanity observed while building: title is known at the third render; the reader goes to Error with loading still true.
Both conditions are evaluated before one assertion so the red output shows both.
Captured failure:
```
after the font failure: loading_active=true (must be false, the overlay must not stay painted over the error), redraw_requested=false (must be true, the error screen has to be painted)
```
Phase == Error and error_kind().is_some() assertions pass today; only loading/redraw fail (right reason).

## T-B unchanged_text_and_size_do_not_reread_the_pack_but_changes_do
Requirement: R7 spirit, reviewer finding I4 (no memoization). Fixtures: cjk_support `pack(px,false)` for all SIZES; `Kernel`/`SdStorage`; `SurfaceFonts` size idx 0 (16px), `VisibleText` with body font.
Expected values: literal fingerprints from `cjk_support::rows(px, ch)` (rows [0x81, px, scalar-low-byte], 8x3), x positions from label at x16 with advance px+1 (臺 x16, 灣 x33, 𠮷 x50 at 16px; x16/36/56 at 19px). Zero reads is `read_count()==0` and bytes returned == 0.
Sequence: prepare (reads>0, glyphs drawn) -> identical fresh VisibleText + same size: zero reads, identical PBM -> add 𠮷: reads>0, new glyph drawn -> set_size(1) (19px): reads>0, 19px glyphs drawn.
Captured failure:
```
assertion `left == right` failed: re-preparing unchanged text at an unchanged size must not touch the SD card
  left: (20, 446)
 right: (0, 0)
```
Validation of the later steps: with the step-2 assertion temporarily relaxed, steps 3-4 passed against current code (so only the memoization expectation is red). Original restored.

## T-C a_page_of_150_distinct_35px_glyphs_prepares_and_draws_from_the_pack
Requirement: R9/R14 realistic page must prepare; reviewer finding I2. Fixture: local `big_pack()` using the same v1 layout as cjk_support (44-byte header, 22-byte sorted records) with 150 consecutive scalars U+4E00..U+4E95 and real 35x35 bitmaps: stride ceil(35/8)=5, 5*35=175 bytes per glyph, 26,250 bytes total (hand arithmetic from the format, not from production). Advance 36, bearing (0,-30). Each bitmap is a per-scalar/per-row fingerprint ([cp low, cp high^0x5A, row|0x80, 0xA5^row, 0xE0]), so a synthetic missing box cannot match. 15 labels of 10 scalars (`VisibleText::add` per label, same body font idx 4), prepared once at size idx 4, then all 150 glyphs are fingerprint-checked in a full render at x = column*36 within each 50px row band, and draw reads are asserted zero.
Captured failure:
```
150 distinct 35px glyphs (26250 bitmap bytes) are one realistic page; got Some(BufferTooSmall)
```
Fixture validation: temporarily set N=90 (15,750 bytes, under the 16 KiB bitmap ceiling named in the review); prepare succeeded and all 90 glyphs matched at the expected positions, so the draw/position/fingerprint logic is correct and the only red reason at 150 is capacity. Original restored (N=150).

## Full red run
```
test corrupt_installed_title_pack_during_load_ends_in_error_with_loading_cleared_and_redraw_requested ... FAILED
test a_page_of_150_distinct_35px_glyphs_prepares_and_draws_from_the_pack ... FAILED
test unchanged_text_and_size_do_not_reread_the_pack_but_changes_do ... FAILED
test result: FAILED. 0 passed; 3 failed
```

## Caveats
- T-A's red condition covers the load-time path (prepare fails while the loading overlay is up). A Ready-state failure (prepare after load finished) would only exercise the dirty flag; not tested separately.
- T-B asserts a re-prepare after set_size also reads; it does not constrain how an implementation memoizes (any scheme with zero reads on identical text/size passes).
- host/src/reader.rs already carried uncommitted changes from earlier tasks; its sha256 in the manifest covers the whole file.
