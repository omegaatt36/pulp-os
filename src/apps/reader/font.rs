// which font a book page is laid out and drawn with
//
// two sources, one decision point at open time:
//   BuiltIn  the build-time rasterised TTF tables (src/fonts). Always
//            present, Latin/Greek/Cyrillic only, zero RAM.
//   Pack     a .PFP font pack on the SD card, read a sector at a time. This
//            is the only path that can draw CJK, and the only one that costs
//            RAM: a FontPack holds an index table and a PageGlyphs holds one
//            page of glyph bitmaps.
//
// discovery is by name, not by scan. The card root listing is cached and
// filtered to book extensions (TXT/EPUB/EPU/MD) by the file browser, so a
// .PFP is invisible in it, and walking the raw directory a second time just
// to look for font files would be a whole extra SD pass per book open. Since
// a pack carries exactly one pixel size of one face, the name IS the index:
// the reader asks for the file its current size tier implies and falls back
// to the built-in fonts when that file is not on the card.
//
// the pack path is prepare-then-draw, not draw-as-you-go. `prepare_page`
// issues every SD read the page needs in one pass, from the app's background
// phase where input can still interrupt; `draw_text` is then pure and runs
// from the &self render path. That split is why PageGlyphs is separate from
// FontPack, and why the cache is rebuilt per page instead of carried across.

use alloc::boxed::Box;

use pulp_render::font_pack::FontPack;
use pulp_render::layout::{LineSpan, Markup};
use pulp_render::pack_file;
use pulp_render::page::{PageGeometry, PageGlyphs};
use pulp_render::strip::StripBuffer;

use crate::fonts;
use crate::kernel::handle::KernelHandle;
use crate::kernel::pack::HandlePackReader;

/// Font family to look for on the card. The converter takes 1..=6 uppercase
/// ASCII letters or digits; see tools/fontpack/src/names.rs.
pub const PACK_FAMILY: &[u8] = b"IANSUI";

/// Pixel size per book-font size tier (0=XSmall .. 4=XLarge), matching the
/// body sizes build.rs rasterises. A pack must be built for one of these to
/// be picked up; the converter takes 8..=96 px, so a pack at some other
/// size is simply never named here.
const TIER_PX: [u16; 5] = [16, 19, 23, 28, 35];

/// Distinct scalars one page may draw. A 16 px CJK page holds 30 glyphs per
/// line and the reader shows at most 32 lines, so this is headroom.
pub const PAGE_GLYPHS: usize = 256;

/// Glyph bitmap bytes one page may hold. A full-width CJK glyph is up to
/// 35x35/8 = 154 bytes, so this covers ~100 glyphs at the largest tier --
/// past PAGE_GLYPHS, so the glyph count binds first and an overflow is a
/// real "page too unusual" rather than a byte-budget surprise.
pub const PAGE_BYTES: usize = 24 * 1024;

type Cache = PageGlyphs<PAGE_GLYPHS, PAGE_BYTES>;

/// The pack file for one size tier, in the converter's spelling.
///
/// Returns the name and its length so callers can hand them straight to
/// `HandlePackReader` without building a `&str`.
pub fn pack_file_name(tier: u8, out: &mut [u8; 12]) -> Option<usize> {
    let px = *TIER_PX.get(tier as usize)?;
    // build the name the converter would have written, then parse it back so
    // the file this module opens is spelled exactly the way the name parser
    // accepts -- one spelling, checked at compile time by round-tripping
    let mut buf = [0u8; 12];
    let mut n = 0;
    buf[..PACK_FAMILY.len()].copy_from_slice(PACK_FAMILY);
    n += PACK_FAMILY.len();
    buf[n] = b'0' + (px / 10) as u8;
    buf[n + 1] = b'0' + (px % 10) as u8;
    n += 2;
    buf[n..n + 4].copy_from_slice(pack_file::PACK_EXT);
    n += 4;

    let parsed = pack_file::parse_pack_name(&buf[..n])?;
    let (name, len) = parsed.file_name();
    out[..len].copy_from_slice(&name[..len]);
    Some(len)
}

pub enum BookFont {
    BuiltIn(fonts::FontSet),
    Pack {
        name: [u8; 12],
        name_len: usize,
        pack: Box<FontPack>,
        cache: Box<Cache>,
    },
}

impl BookFont {
    /// Open the pack for `tier` if the card has it, else the built-in fonts.
    ///
    /// Every failure -- no card, absent file, bad magic, failed CRC -- lands
    /// on the same fallback, so the reader makes exactly one decision here
    /// and never has to reason about why a pack was rejected.
    pub fn open(k: &mut KernelHandle<'_>, tier: u8) -> Self {
        let mut name = [0u8; 12];
        if let Some(len) = pack_file_name(tier, &mut name)
            && let Ok(pack) = HandlePackReader::load(k, &name, len, TIER_PX[tier as usize])
        {
            log::info!(
                "book font: pack {} ({} px)",
                core::str::from_utf8(&name[..len]).unwrap_or("?"),
                TIER_PX[tier as usize]
            );
            return BookFont::Pack {
                name,
                name_len: len,
                pack: Box::new(pack),
                cache: Box::new(Cache::new()),
            };
        }
        log::info!("book font: no pack for tier {tier}, using built-in");
        BookFont::BuiltIn(fonts::FontSet::for_size(tier))
    }

    /// Draw a literal string (the "[image]" placeholder), in the pack's
    /// regular face. Used for chrome-like text the reader injects itself,
    /// which is rare enough not to be worth its own cache.
    pub fn draw_str(
        &self,
        strip: &mut StripBuffer,
        text: &str,
        style: fonts::Style,
        cx: i32,
        baseline: i32,
    ) {
        if let BookFont::BuiltIn(fs) = self {
            fs.draw_str(strip, text, style, cx, baseline);
        }
    }

    pub fn is_pack(&self) -> bool {
        matches!(self, BookFont::Pack { .. })
    }

    /// Line height in px, before the reading theme's spacing percentage.
    pub fn native_line_height(&self) -> u16 {
        match self {
            BookFont::BuiltIn(fs) => fs.line_height(fonts::Style::Regular),
            BookFont::Pack { pack, .. } => pack.line_height(),
        }
    }

    pub fn ascent(&self) -> u16 {
        match self {
            BookFont::BuiltIn(fs) => fs.ascent(fonts::Style::Regular),
            BookFont::Pack { pack, .. } => pack.ascent(),
        }
    }

    /// Drop any prepared page. Call on every page change so a stale cache can
    /// never be drawn against new text.
    pub fn invalidate(&mut self) {
        if let BookFont::Pack { cache, .. } = self {
            cache.clear();
        }
    }

    /// Read every glyph the page will draw. Pack path only; the built-in
    /// fonts need no preparation because their bitmaps are already in flash.
    pub fn prepare_page(
        &mut self,
        k: &mut KernelHandle<'_>,
        buf: &[u8],
        lines: &[LineSpan],
        markup: Markup,
    ) {
        let BookFont::Pack {
            name,
            name_len,
            pack,
            cache,
        } = self
        else {
            return;
        };

        let Ok(mut reader) = HandlePackReader::open(k, name, *name_len) else {
            // card went away between open and page turn; leave the cache
            // empty rather than drawing half a page
            cache.clear();
            return;
        };
        // all or nothing: prepare() leaves the cache empty on any error
        if cache
            .prepare(pack, &mut reader, buf, lines, markup)
            .is_err()
        {
            log::info!("book font: page needs more than {PAGE_GLYPHS} glyphs; drawing without it");
        }
    }

    /// Draw the page's text lines. Images are the caller's: line_glyphs
    /// yields nothing for an image span, so both paths leave them alone.
    pub fn draw_text(
        &self,
        strip: &mut StripBuffer,
        buf: &[u8],
        lines: &[LineSpan],
        markup: Markup,
        geom: &PageGeometry,
    ) {
        match self {
            BookFont::BuiltIn(fs) => {
                for (row, span) in lines.iter().enumerate() {
                    if span.is_image() {
                        continue;
                    }
                    let baseline = geom.top + row as i32 * geom.line_height + geom.ascent;
                    let x = geom.left + geom.indent_px * span.indent as i32;
                    fs.draw_span(strip, buf, *span, markup, x, baseline);
                }
            }
            BookFont::Pack { cache, .. } => {
                // reads nothing; a cache that was never prepared leaves the
                // page blank rather than drawing a partial line
                if let Err(e) = cache.draw(strip, buf, lines, markup, geom) {
                    log::info!("book font: draw failed: {e:?}");
                }
            }
        }
    }
}
