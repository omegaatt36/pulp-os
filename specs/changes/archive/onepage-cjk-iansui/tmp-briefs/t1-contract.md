# T1 contract: SD font pack format (`pulp-fontpack`)

Crate: `/Users/raiven_kao/dev/pulp-os/fontpack` (package `pulp-fontpack`, lib `pulp_fontpack`),
`#![no_std]`, zero dependencies, edition 2024, rust-version 1.95, workspace member.
Features: `default = []`; `builder` (host-side writer, uses `alloc`). Firmware links the
reader only; host converter links with `builder`. Tests refuse to compile without `builder`
(`scripts/host-test.sh` passes it).

This file plus `fontpack/tests/common/mod.rs::GOLDEN` is the format definition. A second-language
implementation must reproduce `GOLDEN` byte for byte from the same input.

## 1. Byte-level format (version 1)

All multi-byte integers are little-endian, fixed width. Signed fields are two's complement.
The file is `header | index | bitmap` with no gaps and no trailing bytes.

### 1.1 Header, 44 bytes, at file offset 0

| offset | size | field            | type | meaning |
|-------:|-----:|------------------|------|---------|
|  0 | 4 | `magic`          | bytes | ASCII `PFNT` = 50 46 4E 54 |
|  4 | 2 | `version`        | u16 | format version; this contract = 1 |
|  6 | 2 | `pixel_size`     | u16 | font pixel size the pack was rasterised at |
|  8 | 8 | `font_id`        | u64 | opaque font identity, compared for equality only (page-cache invalidation); converter defines it (e.g. a hash of the font file); the reader never interprets it |
| 16 | 2 | `line_height`    | u16 | pixels |
| 18 | 2 | `ascent`         | u16 | pixels |
| 20 | 4 | `glyph_count`    | u32 | number of index records |
| 24 | 4 | `index_offset`   | u32 | must be 44 |
| 28 | 4 | `index_len`      | u32 | must be `glyph_count * 22` |
| 32 | 4 | `bitmap_offset`  | u32 | must be `index_offset + index_len` |
| 36 | 4 | `bitmap_len`     | u32 | size of bitmap region |
| 40 | 4 | `total_len`      | u32 | must equal `bitmap_offset + bitmap_len` and the actual input length |

`pixel_size`, `line_height`, `ascent`, `font_id` are not range-checked (any value is accepted).
The layout fields are redundant on purpose: they are cross-checked, so any disagreement is damage.

### 1.2 Index, `glyph_count` records of 22 bytes, at `index_offset`

Record `i` starts at file offset `44 + 22*i`.

| rec offset | size | field           | type | meaning |
|-----------:|-----:|-----------------|------|---------|
|  0 | 4 | `codepoint`     | u32 | Unicode scalar value (0..=0x10FFFF excluding D800..=DFFF) |
|  4 | 4 | `bitmap_offset` | u32 | byte offset **relative to the start of the bitmap region** |
|  8 | 4 | `bitmap_len`    | u32 | byte length of this glyph's bitmap |
| 12 | 2 | `advance`       | u16 | |
| 14 | 2 | `offset_x`      | i16 | |
| 16 | 2 | `offset_y`      | i16 | |
| 18 | 2 | `width`         | u16 | pixels |
| 20 | 2 | `height`        | u16 | pixels |

Records are sorted by `codepoint`, strictly increasing (binary-searchable, no duplicates).

### 1.3 Bitmap region, `bitmap_len` bytes, at `bitmap_offset`

Raw glyph bitmaps: 1 bpp, MSB-first, row-major, `stride = ceil(width/8)`, row padding bits are
unspecified. A glyph's `bitmap_len` must equal `stride * height`. Therefore `width == 0` or
`height == 0` means `bitmap_len == 0` (blank glyph; `offset` still must satisfy the range rule).
The writer emits bitmaps back to back in index order starting at relative offset 0, no padding,
no de-duplication. The reader additionally accepts bitmaps that overlap each other, appear in
any offset order, and bitmap bytes no glyph references (e.g. `glyph_count == 0` with a non-empty
bitmap region).

### 1.4 Validity rules (all arithmetic in u64 or checked u32; overflow = damage)

`Pack::parse(bytes)` returns `Ok` iff all of the following hold, checked in this order and
returning the first violation:

1. `bytes.len() >= 44`, else `TooShort`.
2. `bytes[0..4] == "PFNT"`, else `BadMagic`.
3. `version == 1`, else `UnsupportedVersion { found }` (both lower and higher; judged before any
   layout field, so a version mismatch is never masked by structural damage).
4. `total_len == bytes.len()` (rejects truncation and trailing bytes), else `LengthMismatch`.
5. `index_offset == 44`; `glyph_count * 22 == index_len` (computed without overflow);
   `index_offset + index_len` does not overflow u32 and `== bitmap_offset`;
   `bitmap_offset + bitmap_len` does not overflow u32 and `== total_len`; else `BadLayout`.
   (Covers index/bitmap out of bounds, overlap, gap, count/len mismatch.)
6. For each record `i` in index order, first failing check wins:
   a. `codepoint` is a Unicode scalar (`char::from_u32` succeeds), else `InvalidCodepoint { index: i }`;
   b. `codepoint` > previous record's codepoint (for `i > 0`), else `CodepointOrder { index: i }`;
   c. `bitmap_offset + bitmap_len` does not overflow u32 and is `<= header.bitmap_len`, else `GlyphBitmapRange { index: i }`;
   d. `bitmap_len == ceil(width/8) * height`, else `GlyphSizeMismatch { index: i }`.

Consequence: every proper prefix of a valid pack is rejected (`TooShort` below 44 bytes,
`LengthMismatch` from 44 up). A 0-glyph pack (44 bytes: header only, `bitmap_len == 0`) is **valid**.
No input may cause a panic, overflow trap or out-of-bounds access; no slice returned by the
reader lies outside the input.

## 2. Public API (reader, always available)

```rust
pub const MAGIC: [u8; 4];            // *b"PFNT"
pub const FORMAT_VERSION: u16;       // 1
pub const HEADER_LEN: usize;         // 44
pub const INDEX_RECORD_LEN: usize;   // 22

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontInfo { pub pixel_size: u16, pub font_id: u64, pub line_height: u16, pub ascent: u16 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header { pub info: FontInfo, pub glyph_count: u32, pub bitmap_len: u32 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metrics { pub advance: u16, pub offset_x: i16, pub offset_y: i16, pub width: u16, pub height: u16 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph<'a> { pub metrics: Metrics, pub bitmap: &'a [u8] }

pub struct Pack<'a> { /* borrows the input; zero-copy */ }
impl<'a> Pack<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Pack<'a>, PackError>;   // full validation, section 1.4
    pub fn header(&self) -> Header;
    pub fn find(&self, c: char) -> Option<Glyph<'a>>;               // binary search; None if absent
    pub fn glyph_at(&self, index: u32) -> Option<(char, Glyph<'a>)>; // None if index >= glyph_count
}
```

`Pack` need not implement `Debug` (tests do not require it). `find`/`glyph_at` never panic on a
`Pack` obtained from a successful `parse`; the returned `bitmap` is exactly the glyph's
`bitmap_len` bytes inside the input (empty slice for blank glyphs). Missing-glyph policy is not
part of this crate's contract (`find` returns `None`).

## 3. Errors

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackError {
    TooShort,                                // input < 44 bytes
    BadMagic,                                // magic != "PFNT"
    UnsupportedVersion { found: u16 },       // version != 1 (the only non-structural error)
    LengthMismatch,                          // total_len != input length (truncated or trailing bytes)
    BadLayout,                               // index/bitmap region fields inconsistent, overlap, gap, overflow
    InvalidCodepoint { index: u32 },         // record codepoint is not a Unicode scalar
    CodepointOrder { index: u32 },           // record codepoint <= previous (duplicate or descending)
    GlyphBitmapRange { index: u32 },         // offset+len overflows u32 or exceeds bitmap_len
    GlyphSizeMismatch { index: u32 },        // bitmap_len != ceil(width/8)*height
}
impl core::fmt::Display for PackError { /* non-empty, distinct text per kind */ }
```

Diagnosability: `UnsupportedVersion` is the only variant meaning "recognisably a pack of another
version"; every other variant is structural damage. (Caller maps both to "font failure" with the
variant as the cause.) `index` is the 0-based record number.

## 4. Public API (writer, feature `builder`)

```rust
pub struct GlyphEntry { pub codepoint: char, pub metrics: Metrics, pub bitmap: Vec<u8> }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildError {
    NotStrictlyIncreasing { index: u32 },   // entry index whose codepoint <= previous
    BitmapSizeMismatch { index: u32 },      // bitmap.len() != ceil(width/8)*height
    TooLarge,                               // any u32 field would overflow
}

pub fn build_pack(info: &FontInfo, glyphs: &[GlyphEntry]) -> Result<Vec<u8>, BuildError>;
```

`build_pack` writes exactly the layout in section 1 and the output always passes `Pack::parse`.
Entries must already be sorted strictly ascending by codepoint (the builder does not sort).
Bitmaps are concatenated in entry order from relative offset 0. Reader and writer share the same
constants and codec in this crate; no other crate may define the layout.

## 5. Golden pack (139 bytes), pinned in tests

Font info `pixel_size 16, font_id 0x0123_4567_89AB_CDEF, line_height 20, ascent 15`; glyphs
U+0041 (8x2, adv 9, ox 1, oy 12, bitmap 3C 42), U+FFFF (1x1, adv 1, bitmap 80),
U+20BB7 (9x2, adv 16, oy -2, bitmap FF 80 00 80), U+10FFFF (blank, adv 5). Layout: header
[0,44), index [44,132), bitmap [132,139); `total_len` = 0x8B. The per-byte annotated bytes live in
`fontpack/tests/common/mod.rs` (`GOLDEN`; `GOLDEN_EMPTY` for the 44-byte 0-glyph pack).
