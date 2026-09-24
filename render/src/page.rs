// page glyph preparation: fetch a page's glyphs once, draw from RAM
//
// the SSD1677 driver runs a page's draw closure once per strip (12 per
// full refresh) or per partial-window chunk, on the SPI bus it shares
// with the SD card. so a page is drawn in three steps:
//
//   1. layout: layout::wrap with PackMeasure (SD reads: one index sector
//      per measured scalar)
//   2. prepare: PageGlyphs::prepare resolves every drawn scalar of the
//      page's spans once and reads each distinct bitmap once into the
//      cache (SD reads)
//   3. draw: PageGlyphs::draw, per strip, from the cache alone. it takes
//      no reader and the cache holds none, so a draw cannot touch the SD
//      card (R10)
//
// the cache is bounded by its const parameters: GLYPHS slots (one per
// distinct drawn scalar; absent scalars each take a slot but share the
// fallback bitmap) and BYTES of bitmap data (one copy per distinct
// resolved code point). a page that does not fit fails to prepare and
// leaves the cache empty and not drawable (R11); so does a read failure
//
// the pack is one face: bold, italic and heading spans draw with it too
// (their style is tracked, as layout measured it, but selects nothing)

use crate::font_pack::{FontPack, LookupError, PackGlyph, PackReader};
use crate::layout::{LineSpan, Markup, Measure, Style, line_glyphs};
use crate::strip::StripBuffer;

// layout metrics from the pack: every scalar resolves through
// FontPack::resolve, so layout and drawing agree on the fallback (R4).
// one index-sector read per call
pub struct PackMeasure<'a, R> {
    pack: &'a FontPack,
    reader: &'a mut R,
}

impl<'a, R: PackReader> PackMeasure<'a, R> {
    pub fn new(pack: &'a FontPack, reader: &'a mut R) -> Self {
        Self { pack, reader }
    }
}

impl<R: PackReader> Measure for PackMeasure<'_, R> {
    type Error = LookupError;

    fn advance(&mut self, ch: char, _style: Style) -> Result<u32, LookupError> {
        Ok(self.pack.advance(self.reader, ch)? as u32)
    }

    fn line_height(&self, _style: Style) -> u16 {
        self.pack.line_height()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrepareError {
    // the page draws more distinct scalars than the cache has slots
    TooManyGlyphs { limit: usize },
    // the page's distinct bitmaps exceed the cache's byte budget
    TooManyBytes { limit: usize },
    // an index or bitmap read failed
    Lookup(LookupError),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrawError {
    // no successful prepare since creation, clear or a failed prepare
    NotPrepared,
    // the text holds a scalar the cache was not prepared with
    NotInCache(char),
}

// where a page's lines go, in logical pixels
#[derive(Clone, Copy, Debug)]
pub struct PageGeometry {
    // pen x of an unindented line
    pub left: i32,
    // top of line 0
    pub top: i32,
    // distance between line tops (the layout's line height)
    pub line_height: i32,
    // line top to baseline
    pub ascent: i32,
    // pen x shift per block-quote level (the layout's indent_px)
    pub indent_px: i32,
}

// one cached scalar: its resolved record, with bitmap_offset rewritten
// to the offset of its bitmap in the cache's byte store
#[derive(Clone, Copy)]
struct Entry {
    ch: char,
    glyph: PackGlyph,
}

impl Entry {
    const EMPTY: Self = Self {
        ch: '\0',
        glyph: PackGlyph {
            code_point: 0,
            bitmap_offset: 0,
            width: 0,
            height: 0,
            advance: 0,
            offset_x: 0,
            offset_y: 0,
        },
    };
}

// the glyphs of one prepared page. ~20 * GLYPHS + BYTES bytes: keep it in
// a static or a Box, not on a task stack
pub struct PageGlyphs<const GLYPHS: usize, const BYTES: usize> {
    // entries[..len], sorted by ch
    entries: [Entry; GLYPHS],
    len: usize,
    bytes: [u8; BYTES],
    used: usize,
    ready: bool,
}

impl<const GLYPHS: usize, const BYTES: usize> PageGlyphs<GLYPHS, BYTES> {
    pub const fn new() -> Self {
        Self {
            entries: [Entry::EMPTY; GLYPHS],
            len: 0,
            bytes: [0; BYTES],
            used: 0,
            ready: false,
        }
    }

    // empty and not drawable
    pub fn clear(&mut self) {
        self.len = 0;
        self.used = 0;
        self.ready = false;
    }

    // true after a successful prepare, until the next prepare or clear
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    // distinct scalars cached
    pub fn glyph_count(&self) -> usize {
        self.len
    }

    // bitmap bytes cached
    pub fn bytes_used(&self) -> usize {
        self.used
    }

    // cache every glyph the page's lines draw (layout::line_glyphs over
    // buf, the buffer the spans index). pass only populated lines, e.g.
    // &lines[..wrapped.line_count] when reusing a fixed-size scratch array.
    // all or nothing: on error the cache is left empty and not drawable
    pub fn prepare<R: PackReader>(
        &mut self,
        pack: &FontPack,
        reader: &mut R,
        buf: &[u8],
        lines: &[LineSpan],
        markup: Markup,
    ) -> Result<(), PrepareError> {
        self.clear();
        let filled = lines
            .iter()
            .flat_map(|span| line_glyphs(buf, span, markup))
            .try_for_each(|(ch, _)| self.add(pack, reader, ch));
        match filled {
            Ok(()) => self.ready = true,
            Err(_) => self.clear(),
        }
        filled
    }

    fn add<R: PackReader>(
        &mut self,
        pack: &FontPack,
        reader: &mut R,
        ch: char,
    ) -> Result<(), PrepareError> {
        let Err(pos) = self.entries[..self.len].binary_search_by_key(&ch, |e| e.ch) else {
            return Ok(());
        };
        if self.len == GLYPHS {
            return Err(PrepareError::TooManyGlyphs { limit: GLYPHS });
        }
        let mut glyph = pack
            .resolve(reader, ch)
            .map_err(PrepareError::Lookup)?
            .glyph;
        let shared = self.entries[..self.len]
            .iter()
            .find(|e| e.glyph.code_point == glyph.code_point);
        let offset = match shared {
            Some(e) => e.glyph.bitmap_offset,
            None => {
                let end = self.used + glyph.bitmap_len();
                if end > BYTES {
                    return Err(PrepareError::TooManyBytes { limit: BYTES });
                }
                pack.read_bitmap(reader, &glyph, &mut self.bytes[self.used..end])
                    .map_err(PrepareError::Lookup)?;
                let offset = self.used as u32;
                self.used = end;
                offset
            }
        };
        glyph.bitmap_offset = offset;
        self.entries[pos..=self.len].rotate_right(1);
        self.entries[pos] = Entry { ch, glyph };
        self.len += 1;
        Ok(())
    }

    // draw the prepared page's text lines into the strip in black; image
    // lines are the caller's. buf, lines and markup must be the ones
    // prepared; pass only populated lines, e.g. &lines[..wrapped.line_count]
    // when reusing a fixed-size scratch array. reads nothing but the cache
    pub fn draw(
        &self,
        strip: &mut StripBuffer,
        buf: &[u8],
        lines: &[LineSpan],
        markup: Markup,
        geom: &PageGeometry,
    ) -> Result<(), DrawError> {
        if !self.ready {
            return Err(DrawError::NotPrepared);
        }
        for (row, span) in lines.iter().enumerate() {
            let baseline = geom.top + row as i32 * geom.line_height + geom.ascent;
            let mut cx = geom.left + geom.indent_px * span.indent as i32;
            for (ch, _) in line_glyphs(buf, span, markup) {
                let g = self.glyph(ch).ok_or(DrawError::NotInCache(ch))?;
                strip.blit_1bpp(
                    &self.bytes,
                    g.bitmap_offset as usize,
                    g.width as usize,
                    g.height as usize,
                    (g.width as usize).div_ceil(8),
                    cx + g.offset_x as i32,
                    baseline + g.offset_y as i32,
                    true,
                );
                cx += g.advance as i32;
            }
        }
        Ok(())
    }

    fn glyph(&self, ch: char) -> Option<&PackGlyph> {
        let entries = &self.entries[..self.len];
        let i = entries.binary_search_by_key(&ch, |e| e.ch).ok()?;
        Some(&entries[i].glyph)
    }
}

impl<const GLYPHS: usize, const BYTES: usize> Default for PageGlyphs<GLYPHS, BYTES> {
    fn default() -> Self {
        Self::new()
    }
}
