# T3 contract: random-access pack reader, missing glyph, converter roundtrip

Crates: `fontpack` (`pulp-fontpack`, `#![no_std]`, no alloc in reader, `forbid(unsafe_code)`, zero deps) and
`fontconv` (`pulp-fontconv`). Byte format = `t1-contract.md` (unchanged, v1). Firmware cannot hold a
pack in RAM (index of a 12,665-glyph pack ~ 278 KB), so the device reads the SD file through
random-access reads. `Pack::parse(&[u8])` (T1) stays as the whole-file validator and is the
cross-check oracle in roundtrip tests.

Requirements this task satisfies (from spec.md, verbatim):
- R2: WHEN 字元或 bitmap offset 超出16-bit範圍 THE SYSTEM SHALL 仍可正確索引該字形。
- R3: IF 字庫損壞、版本不符或索引越界 THEN THE SYSTEM SHALL 回報 font failure 而不越界存取。
- R4: IF 已選字體不含所需字元 THEN THE SYSTEM SHALL 顯示明確的 missing-glyph fallback。
- R5: WHEN 切換目前支援的 body／heading 字級 THE SYSTEM SHALL 使用該字級一致的量測與字形資料。
No provenance tags in spec.md: treat every requirement as unannotated; if the contract below contradicts a
requirement, stop and report.

## 1. Already public, in scope of direct tests (T1 gap: only covered via `Pack::parse` so far)

```rust
// pulp_fontpack
pub fn bitmap_size(width: u16, height: u16) -> u32;            // ceil(width/8) * height
pub struct Layout { pub index_len: u32, pub bitmap_offset: u32, pub total_len: u32 }
impl Header {                                                   // Header { info: FontInfo, glyph_count: u32, bitmap_len: u32 }
    pub fn layout(&self) -> Option<Layout>;                     // None iff any derived u32 overflows
    pub fn decode(bytes: &[u8], file_len: u64) -> Result<Header, PackError>;
}
pub struct Record { pub codepoint: u32, pub bitmap_offset: u32, pub bitmap_len: u32, pub metrics: Metrics }
impl Record {
    pub fn decode(bytes: &[u8]) -> Option<Record>;              // first 22 bytes; None if fewer
    pub fn validate(&self, index: u32, prev: Option<char>, region_len: u32) -> Result<char, PackError>;
}
```
Semantics (all from t1-contract.md section 1):
- `layout()`: `index_len = glyph_count*22`, `bitmap_offset = 44 + index_len`, `total_len = bitmap_offset + bitmap_len`;
  each step overflow-checked in u32 (`glyph_count*22 > u32::MAX` is None; `44 + index_len` overflow is None;
  `+ bitmap_len` overflow is None). Boundary values must be tested exactly (largest ok / smallest overflowing).
- `Header::decode(bytes, file_len)`: `bytes` starts with the header, more bytes may follow; checks in the order of
  t1 section 1.4 steps 1-5, with `file_len` instead of `bytes.len()` for step 4. `< 44` bytes -> `TooShort`.
- `Record::decode`: little-endian, 22-byte layout of t1 section 1.2; `bytes.len() > 22` fine (extra ignored).
- `Record::validate(index, prev, region_len)`: t1 step 6 a-d in that order; first failure wins, error carries
  `index`; `prev = Some(p)` means this codepoint must be `> p`; returns the codepoint as `char`.

## 2. New API (in `fontpack`, always available, no alloc)

```rust
pub trait ReadAt {
    type Error;
    // Fill `buf` completely with the bytes at [offset, offset+buf.len()), or return Err.
    // A read that cannot be satisfied in full is an Err; there are no short reads.
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutOfRange;                      // error type of the slice impl
impl ReadAt for &[u8] { type Error = OutOfRange; ... }   // Err iff offset+len exceeds the slice (no overflow panic)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontError<E> {
    Unsupported { found: u16 },             // header version != 1 (== PackError::UnsupportedVersion)
    Corrupt(PackError),                     // every other structural failure, with its cause
    Io(E),                                  // the ReadAt failed
    BufferTooSmall { needed: usize },       // caller buffer smaller than the bitmap (needed = bitmap_len)
}
impl<E: core::fmt::Display> core::fmt::Display for FontError<E>;   // non-empty, distinct text per variant

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphRef { pub metrics: Metrics, pub bitmap_len: u32, /* private: bitmap location */ }

pub struct PackReader<R: ReadAt> { /* private */ }
impl<R: ReadAt> PackReader<R> {
    pub fn open(src: R, file_len: u64) -> Result<Self, FontError<R::Error>>;
    pub fn header(&self) -> Header;
    pub fn info(&self) -> FontInfo;                                            // == header().info
    pub fn find(&mut self, c: char) -> Result<Option<GlyphRef>, FontError<R::Error>>;
    pub fn read_bitmap<'b>(&mut self, g: &GlyphRef, buf: &'b mut [u8])
        -> Result<&'b [u8], FontError<R::Error>>;
}
```
Behaviour:
- `open`: `file_len < 44` -> `Err(Corrupt(TooShort))` with **zero** reads (decided: the only accepted answer).
  Otherwise exactly **one** `read_at(0, <44-byte buf>)`. Validates with the header rules (t1
  steps 1-5, `file_len` is the size of the whole file). `UnsupportedVersion{found}` maps to `Unsupported{found}`,
  every other `PackError` to `Corrupt(that error)`. Read failure -> `Io(e)`. No whole-file scan.
- `find(c)`: binary search over the index; each probe is **one** `read_at` of exactly 22 bytes at
  `44 + 22*mid`. At most `ceil(log2(glyph_count+1))` reads per call, 0 reads when `glyph_count == 0`.
  Never touches the bitmap region. Every probed record is decoded and checked with the per-record rules of
  t1 step 6 a, c, d (codepoint is a scalar, bitmap range inside the region, bitmap_len == size); a probed
  record failing a rule -> `Err(Corrupt(<that PackError>))` with `index` = the probed record number. Order (b)
  can not be checked from one record: disorder only makes a glyph unfindable (`Ok(None)`), never an error and
  never an out-of-bounds access. Damage in records that are not probed is not detected (by design).
  `Ok(None)` = the pack does not contain `c` (this includes control characters the converter excluded, e.g. '\r').
  Binary search must stay correct for any `u32` ordering: it compares codepoints, never casts to u16. `Ok(Some(g))`: `g.metrics` = the record's metrics,
  `g.bitmap_len` = the record's bitmap_len. Read failure -> `Io(e)` (no retry).
- `read_bitmap(g, buf)`: `buf.len() < g.bitmap_len` -> `Err(BufferTooSmall{needed: g.bitmap_len as usize})`
  without any read. Else exactly **one** `read_at(44 + 22*glyph_count + bitmap_offset, &mut buf[..bitmap_len])`
  and returns `&buf[..bitmap_len]`; `bitmap_len == 0` -> `Ok(&[])` with **zero** reads.
  A `GlyphRef` whose bitmap range does not lie inside THIS reader's bitmap region (`offset + len <= bitmap_len`
  of this pack; e.g. it came from another, larger pack) -> `Err(Corrupt(PackError::BadLayout))` with **zero**
  reads (never a request outside `[0, file_len)`). Precedence: range outside the region -> `BadLayout`; range
  valid but `buf` too small -> `BufferTooSmall`. A blank glyph (len 0) with `offset == bitmap_len` is valid
  (`Ok(&[])`, 0 reads); `offset > bitmap_len` is `BadLayout`. `buf` bytes beyond
  `bitmap_len` are untouched. Read failure -> `Io(e)`.
- Offsets are u64 arithmetic; codepoints and bitmap offsets above 0xFFFF / 65,535 / 2^24 etc. are indexed
  correctly (R2). A pack whose bitmap region is > 64 KiB and > 16 MiB (sparse, built with the builder using
  large blank-ish glyphs, or hand-written bytes where the reader only needs index records and the offset
  validity, since the bitmap bytes themselves are only read on demand) must work.
- No input may panic, overflow-trap or read out of the declared file: for any `file_len`/bytes, any `c`.

## 3. Missing glyph (R4), pure function of `FontInfo`, no I/O, no alloc

```rust
pub fn missing_glyph_metrics(info: &FontInfo) -> Metrics;
// Writes the bitmap into `out[..bitmap_size(w, h)]` (other bytes untouched), returns its metrics.
// None iff out.len() < bitmap_size(w, h).
pub fn render_missing_glyph(info: &FontInfo, out: &mut [u8]) -> Option<Metrics>;
```
Definition (only `info.pixel_size` matters; `font_id`, `line_height`, `ascent` do not):
- `s = (pixel_size * 3 / 4)` in u32 integer arithmetic, clamped to `3..=255`.
- `width = height = s`; `advance = max(pixel_size, s + 2)`; `offset_x = (advance - s) / 2` (integer division);
  `offset_y = -(s as i16)` (the box sits on the baseline; negative = above baseline, same sign convention as
  the pack format: top row y = baseline + offset_y).
- Bitmap: 1 bpp, MSB-first, row-major, stride `ceil(s/8)`; a hollow square: pixel (x, y) is ink (bit 1) iff
  `x == 0 || y == 0 || x == s-1 || y == s-1`; every other pixel and every row padding bit is 0.
  Therefore it is non-blank and the interior is empty; `render` is deterministic and identical for equal
  `pixel_size`.
- `render_missing_glyph` returns exactly `missing_glyph_metrics(info)` on success.
Examples (hand-derived): `pixel_size 16` -> s=12, advance 16, offset_x 2, offset_y -12, 12x12;
`pixel_size 23` -> s=17, advance 23, offset_x 3; `pixel_size 1` -> s=3 (clamp), advance 5, offset_x 1;
`pixel_size 0` -> s=3, advance 5; `pixel_size 400` -> s=255 (clamp), advance 400, offset_x 72.

## 4. Converter roundtrip (in `fontconv/tests/`, not in fontpack)

`fontconv` produces packs; the new reader reads them back. For Bookerly (`assets/fonts/Bookerly-Regular.ttf`,
all nine `DEFAULT_SIZES`) and the real Iansui (`$IANSUI_TTF`; skip protocol and `SKIPPED(no-iansui-font):`
line exactly as in `fontconv/tests/common/mod.rs`, honour `IANSUI_REQUIRED=1`):
- Run the converter (`pulp_fontconv::Input/Output` as in the existing tests), then `PackReader::open(&bytes[..],
  len)` for each pack. `header()/info()` agree with `Pack::parse` and with the independent oracle in
  `fontconv/tests/common/mod.rs` (pixel_size = requested size; line_height/ascent per oracle).
- For **every** character the oracle rasterises: `find` returns `Some` and `metrics` + `read_bitmap` bytes equal the
  oracle's; for characters the font lacks (e.g. U+2A6A5 `𪚥` in Iansui, an unassigned scalar, U+FFFF): `Ok(None)`.
  The oracle in common/mod.rs is the independent source; do not derive expectations from converter output.
  Cross-check also that `PackReader` and `Pack::parse(..).glyph_at` agree on every glyph (all indices).
- Iansui contains `𠮷` (U+20BB7, 4-byte UTF-8, > 0xFFFF): found, with correct metrics/bitmap (R2).
  At least one Iansui pack has bitmap region > 64 KiB and glyph bitmap offsets > 65,535 (assert that fact in the
  test so R2 is really exercised: 23px bitmap region is 772,597 bytes).
- R5: for each of the nine sizes, the pack opened for that size reports that size, and the metrics of a given
  character differ across sizes consistently with the oracle at that size (a size-N glyph is never served from a
  size-M pack: assert `info().pixel_size == N` and `font_id` distinct across sizes).
- SD failure injection on a converter-produced pack (wrapper `ReadAt` defined in the test): error on the k-th
  read for k = 1..=small (open, probes, bitmap) -> `Io(e)` with the injected error and no panic; a wrapper that
  flips one byte of a probed record or of the header -> `Corrupt(...)`/`Unsupported` per contract, never a panic,
  never an out-of-bounds read request (the wrapper records `offset+len` of every request and the test asserts
  every request lies inside `[0, file_len)`).
- Read budget: counting wrapper; `open` = 1 read of 44 B at 0; a `find` of a present char on a 12,665-glyph
  pack = <= 14 reads of 22 B each and 0 reads past the index; `read_bitmap` = 1 read.

## 5. Where tests live
- `fontpack/tests/header_record.rs` (section 1, direct), `fontpack/tests/reader.rs`, `fontpack/tests/reader_faults.rs`
  (failure injection, budget, no-panic fuzz-ish sweeps), `fontpack/tests/missing_glyph.rs`. Add shared helpers
  only by appending to `fontpack/tests/common/mod.rs` (existing contents untouched).
- `fontconv/tests/roundtrip.rs` (+ append-only helpers in `fontconv/tests/common/mod.rs`).
- Existing tests in all crates must not be edited, loosened or skipped.
