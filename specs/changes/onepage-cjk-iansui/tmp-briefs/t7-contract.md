# T7 contract: reader CJK metrics, visible glyphs and mixed wrapping

Scope: actual ReaderApp through host Rig and VirtualStorage. T3 supplies
PackReader/missing glyphs; T6 supplies bounded visible-page PageCaches. Keep
FontSet Copy/static for Latin. Owned CjkState may stage bounded metrics for the
copied PAGE_BUF; an immutable LayoutFonts descriptor may select those metrics.
No T8 TOC/title/UI expansion or T9 font-ID invalidation work is included.

Requirements are untagged in spec.md:

- R4: IF 已選字體不含所需字元 THEN THE SYSTEM SHALL 顯示明確的 missing-glyph fallback。
- R5: WHEN 切換目前支援的 body／heading 字級 THE SYSTEM SHALL 使用該字級一致的量測與字形資料。
- R10: WHEN 橫排繁中與 Latin 混合內容換行 THE SYSTEM SHALL 依明確列出的禁則規則避免禁止的行首／行尾標點。
- R11: WHEN 回到前頁或恢復書籤 THE SYSTEM SHALL 定位至同一文字位置。

## Behavior

- Body indices 0..4 select 16/19/23/28/35px; heading selects
  23/27/32/38/46px. Packs live at _PULP/FONTS/F000NN.PFN.
- Existing static Latin availability and regular/bold/italic/heading selection
  remain authoritative for supported Latin characters. The CJK pack supplies
  unsupported scalars, including U+20BB7, without narrowing codepoints or
  u16 advances. Bearings retain their signed pack-format meaning.
- Resolve pack metrics before wrapping. Indexing, restored-offset scans and
  visible layout use the same metrics and break decisions. Load bitmap data
  only for final visible spans. Cache get/draw never reads storage.
- If a valid selected pack lacks a scalar, use the T3 hollow missing-glyph box
  and its matching advance. Missing/corrupt packs or an SD error become a
  recoverable preparation error when CJK fallback is actually required; no
  invisible successful page. English without packs retains existing behavior.
  Reopening after installing/fixing the pack or clearing a one-shot SD failure
  retries successfully.
- No line begins with ，。、？！）」』】 or ends with （「『【.
  Preserve all text scalars through mixed wrapping and page boundaries. The
  sample 臺灣「繁體中文」，𠮷。 is exercised at narrow widths and shifted scalar
  boundaries, alongside Latin text and every listed punctuation character.
- When width cannot fit a minimally inseparable punctuation group, permit
  group overhang. Preserve text and forward progress; never loop or drop
  scalars. Ordinary cases use widths of at least two glyph advances.
- Going forward then backward returns the same page bytes and text offset;
  bookmark save/reboot/restore locates that same position. All page offsets
  remain UTF-8 scalar boundaries. Font-identity cache invalidation is T9.

## Thin host instrumentation required

```rust
impl pulp_host::reader::Rig {
    pub fn set_text_width(&mut self, width: u32);
}
```

This host-only method sets ReaderApp text geometry without implementing any
layout. The Rig retains an optional override and applies it after open/configure
and before settling; it also reapplies it before ticks and after input. Firmware
geometry/reset logic is unchanged. Persist width through reopen, including
restored-position scans. Widths are positive and within
the physical text area in these tests. Production paging/wrapping/draw must
remain the code under test; no alternate host wrapper or copied algorithm.
Existing Rig lines/page offsets/storage log/full and stitched rendering suffice
for observations. The controller authorized only this Rig addition and a probe
setter of the existing text_w field; the test author changes no firmware code.

## Fixtures and eight focused tests

New host/tests/cjk_support/mod.rs assembles v1 bytes independently: header44,
record22, ascending scalars, 8x3 MSB-first glyphs, bearing(0,-3), advance px+1,
bitmap [0x81, px, low byte of scalar]. Every size and glyph therefore has a
distinct literal pixel fingerprint. Font ID is a literal prefix plus pixel size.
An alternate 23px pack uses advance301 to catch u8 truncation. No converter,
production reader output or production metrics define the expected values.
The 23px missing glyph expectation is the hand-drawn 17x17 hollow square.

host/tests/cjk_reader.rs covers:

1. All five body sizes, literal advances/pixels, supplementary scalar, advance301.
2. All five heading sizes and preservation of English Latin styles without packs.
3. Explicit missing box for absent U+2A6A5 instead of an ASCII question mark.
4. Mixed kinsoku boundaries, both punctuation sets, scalar conservation.
5. Multi-page conservation, backward navigation and rebooted bookmark position.
6. Visible-only bitmap reads and zero storage reads in repeated full/strip draws.
7. Missing/corrupt/read-failed required pack, failure then successful retry.
8. Inseparable-group overhang and progress at one-glyph width.

Existing tests remain unchanged. No spec IDs are placed in production source.
No mutations, reference implementation or test additions for equivalent mutants.
The initial compile RED identified absent geometry instrumentation. After the
authorized host-only addition, capture behavioral RED against unchanged firmware.

Replay: cargo test -p pulp-host --target aarch64-apple-darwin
--config 'unstable.build-std=["std","test"]' --test cjk_reader -- --test-threads=1

## Controller no-pack compatibility amendment (2026-10-06)

An uninstalled optional pack now preserves Ready text using the synthetic
pixel-size-specific T3 missing box with matching advance, rather than an error.
Installed corrupt packs and storage read failures remain recoverable errors.
The absent case is relocated from the failure table into an explicit runtime
test of23px boxes, zero draw reads and real glyphs after install/reopen.
Original frozen contracts/tests/manifests remain historical evidence; amended
hashes are recorded in the no-pack policy manifest.
