# T8 contract: shared prepared fallback on visible reader and UI surfaces

Requirements are untagged in spec.md. R14 requires the same preparable regular
CJK fallback for traditional Chinese body, heading, TOC, book title and existing
UI labels. R5 binds each surface to its actual pixel size; R7 prohibits SD font
reads during any strip draw. R12 also applies to the existing BitmapDynLabel
set_text/write_str UTF-8 truncation defect.

T7 already tests headings across all five heading sizes and unchanged Latin
styles. T8 does not duplicate those tests. T8 builds on T7's CjkState and the T3
reader/missing-glyph and T6 cache contracts. The integration-map.md lifecycle
inspection is part of this contract: background alone cannot prepare TOC labels
made visible by a just-dispatched event.

## Interfaces approved by the controller

```rust
// production App: manager supplies its owned context
fn prepare_render(&mut self, ctx: &mut AppContext, k: &mut KernelHandle<'_>);
// production AppLayer: owns context; scheduler supplies only handle
fn prepare_render(&mut self, k: &mut KernelHandle<'_>);

// thin host Rig forwarding; no layout or font algorithm
pub fn prepare_render(&mut self);

// pulp_host::fonts::cjk (production src/fonts/cjk.rs)
CjkState::new();
CjkState::prepare_text(&mut self, k: &mut KernelHandle<'_>, text: &[u8],
    latin: FontSet, body_px: u16, heading_px: u16) -> Result<()>;
CjkState::view(&self, latin: FontSet, body_px: u16, heading_px: u16)
    -> PreparedFonts<'_>;

// both BitmapLabel and BitmapDynLabel
pub fn draw_prepared(&self, strip: &mut StripBuffer, fonts: &PreparedFonts<'_>)
    -> Result<(), core::convert::Infallible>;
```

The production hook must run after events and before render, including boot and
deferred/partial redraw paths on both boards. It handles preparation errors by
publishing a recoverable drawable error/previous/loading state. Draw receives an
immutable prepared view and performs no storage operations. Existing draw APIs
remain usable for static Latin-only rendering. A prepared widget retains its
own Latin BitmapFont, alignment, inversion and fill behavior; the view supplies
only unsupported fallback glyphs. T8's widget test prepares 16px body fallback,
then uses regular, bold and italic Latin body fonts with that same regular CJK
fallback. Pure Latin heading placement is checked against the existing API.

No new widget Rig seam is needed. The test creates the existing public host
Kernel/SdStorage, calls CjkState with Kernel::handle, releases that borrow and
draws real production widgets into StripBuffer.

## Seven focused tests and expected evidence

New host/tests/cjk_surfaces.rs uses the frozen read-only T7 cjk_support pack
fixtures. Their expectations are independently assembled PFNT v1 bytes with
header44/record22, advance px+1 and literal rows [0x81, px, scalar-low-byte].
Installed paths are _PULP/FONTS/F000NN.PFN. No expected CJK metrics, bitmaps or
breaks come from production provider output. Existing static Latin font tables
are authoritative data for the preservation requirement.

| Test | Expected behavior |
| --- | --- |
| book_title_uses_16px_chrome_independently_of_body_size_and_draw_reads_nothing | Real EPUB OPF title 臺灣𠮷 draws 16px fingerprints at x8/25/42, at body size indices0 and4; full/stitched render agrees and draw reads zero bytes. |
| visible_toc_uses_body_size_regular_fallback_and_selected_foreground | Real EPUB nav labels use each of five body pixel sizes; selected 臺灣 is inverse and second 𠮷 is normal; advances remain px+1; full/stitched equality and zero draw reads. |
| toc_scroll_prepares_newly_visible_glyph_before_the_next_draw | 36-entry TOC at35px initially prepares visible 臺 only; 𠮷 at index35 loads its literal bitmap only after scrolling makes it visible; next inverse render uses it without reads. |
| uninstalled_title_pack_draws_a_size_specific_box_and_reopen_uses_installed_pack | Latin body plus CJK title and absent16px pack remains Ready with a literal12x12 synthetic box; installing16px pack and reopening shows the literal title glyph. |
| dynamic_label_set_text_keeps_the_longest_complete_scalar_prefix | Capacity7 truncates A臺𠮷Z to A臺; exact7-byte supplementary+BMP text remains; capacity3 cannot skip an unfit leading supplementary scalar. |
| dynamic_label_write_str_keeps_existing_text_and_a_complete_scalar_prefix | Append into capacity7 retains A臺 instead of invalidating all text; separate4+3-byte writes fill exactly. |
| prepared_static_and_dynamic_labels_keep_latin_style_alignment_and_inversion | Real widgets use shared prepared regular fallback. Representative left/center/right and normal/inverse mixed labels have literal17px advances/fingerprints, static/dynamic parity, full/stitched parity and zero reads; pure Latin regular/bold/italic/heading equals old draw pixels. |

EPUB fixture body and chapter headings are Latin to isolate title/TOC behavior
from T7 body/heading behavior. Both chapters exist; TOC labels target chapter1
while chapter0 is displayed, so the current-chapter marker does not alter label
positions. Title glyphs are searched only in the existing header band; TOC
glyphs only in their actual row bands. Vertical baselines remain a production
policy; literal horizontal advances and size-specific fingerprint rows enforce
the required size and fallback behavior.

## Required production inspection beyond host tests

The host intentionally excludes Files/Home/manager and does not execute real
schedulers. Do not fake those algorithms or expand the host broadly to claim
coverage. Before accepting T8, controller/reviewer must inspect and record:

* Files directory title and visible filename rows use the shared provider,
  including reload/reselection after events.
* Home bookmark titles use that provider at the selected UiFonts size.
* Settings labels and formatted values use it at their actual body/heading size.
* Reader title chrome is prepared even while body content is loading or empty,
  rather than depending on a completed body page.
* Manager quick-menu labels, button feedback and loading overlays are included
  in the same pre-render visible-text preparation lifecycle.
* Every X4 and C61 render entry invokes preparation after last visibility
  mutation, including boot and X4 deferred input/partial redraws; suspended
  background work cannot evict the prepared active render's glyphs.
* Missing pack, corrupt pack, SD failure and capacity failure remain recoverable
  on these surfaces; preparations release all storage guards before strip draw.

Widget/reader tests establish runtime evidence only for their real included
production paths. Source inspection is required evidence for omitted surfaces,
not a claim that these seven host tests execute those apps.

No production source, T7 tests/support/harness, mutants, reference algorithm,
exhaustive sweep, spec IDs in Rust, or commits are part of test-author scope.

Controller interface amendment (2026-10-06): AppLayer preparation takes only KernelHandle because the manager owns launcher.ctx; App preparation retains ctx+handle. This prevents a scheduler double mutable borrow. User requirements and test assertions are unchanged. Original frozen snapshot/manifest preserved; amended contract hash recorded separately.

## Controller no-pack compatibility amendment (2026-10-06)

An uninstalled optional title/UI pack now preserves Ready text with the
synthetic T3 missing box at the surface's actual pixel size. Installed corrupt
packs, storage failures and capacity failures remain recoverable errors.
The title test replaces absent-pack Error with a literal16px12x12 box and
zero draw reads, then still requires the installed16px fingerprint on reopen.
The other six tests are unchanged. Original frozen tests/contracts/manifests
remain historical evidence; amended hashes are recorded separately.

## Controller real-boot fixture premise (2026-10-06)

Reader surface fixtures create the `_PULP` root for both installed and
uninstalled packs. The controller verified firmware boot creates this root
(`src/bin/main.rs:114`, `kernel/src/kernel/scheduler_c61.rs:299`). Font absence
therefore means an absent optional FONTS directory/pack, not an absent app data
root. The fixture correction changes no assertion, expected glyph or test case;
original frozen snapshots/manifests remain historical evidence.

## Controller loading-overlay amendment (2026-10-06, after implementer pass)

Inspection bullet 5 is narrowed: the loading overlay stays on the original
built-in mono `draw_loading_indicator`. Every `set_loading` message in
`src/apps/reader/mod.rs` is a Latin literal (including formatted `Caching N/M`),
and that font cannot render CJK, so no fallback glyph can ever be needed there.
The previous worker's switch to a chrome-font label changed Latin pixels with no
test and is reverted. Quick menu and button feedback remain in the prepared
overlay lifecycle. If a future change lets loading text carry user content
(titles, filenames), it must be routed through the prepared path then.
