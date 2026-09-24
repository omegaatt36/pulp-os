// build-time rasterised bitmap fonts for e-ink rendering
// TTFs rasterised by build.rs via fontdue into 1-bit tables in flash
// zero heap, zero parsing at runtime
//
// five size tiers: 0=XSmall  1=Small  2=Medium  3=Large  4=XLarge

pub mod bitmap;

#[allow(clippy::all)]
pub mod font_data {
    include!(concat!(env!("OUT_DIR"), "/font_data.rs"));
}

use bitmap::BitmapFont;
use core::convert::Infallible;
use pulp_render::layout::Measure;
use pulp_render::strip::StripBuffer;

pub const FONT_SIZE_COUNT: usize = 5;

pub const FONT_SIZE_NAMES: &[&str] = &["XSmall", "Small", "Medium", "Large", "XLarge"];

// pre-resolved body + heading font pair for a given size index
#[derive(Clone, Copy)]
pub struct UiFonts {
    pub body: &'static BitmapFont,
    pub heading: &'static BitmapFont,
}

impl UiFonts {
    pub fn for_size(idx: u8) -> Self {
        Self {
            body: body_font(idx),
            heading: heading_font(idx),
        }
    }
}

// human-readable name for size index (clamped to valid range)
#[inline]
pub fn font_size_name(idx: u8) -> &'static str {
    FONT_SIZE_NAMES
        .get(idx as usize)
        .copied()
        .unwrap_or("Small")
}

#[inline]
pub const fn max_size_idx() -> u8 {
    (FONT_SIZE_COUNT - 1) as u8
}

pub fn body_font(idx: u8) -> &'static BitmapFont {
    match idx {
        0 => &font_data::REGULAR_BODY_XSMALL,
        1 => &font_data::REGULAR_BODY_SMALL,
        2 => &font_data::REGULAR_BODY_MEDIUM,
        3 => &font_data::REGULAR_BODY_LARGE,
        4 => &font_data::REGULAR_BODY_XLARGE,
        _ => &font_data::REGULAR_BODY_SMALL,
    }
}

// chrome font (button labels, quick-menu items, loading text)
// always the XSmall body font, compact for UI chrome
pub fn chrome_font() -> &'static BitmapFont {
    body_font(0)
}

pub fn heading_font(idx: u8) -> &'static BitmapFont {
    match idx {
        0 => &font_data::REGULAR_HEADING_XSMALL,
        1 => &font_data::REGULAR_HEADING_SMALL,
        2 => &font_data::REGULAR_HEADING_MEDIUM,
        3 => &font_data::REGULAR_HEADING_LARGE,
        4 => &font_data::REGULAR_HEADING_XLARGE,
        _ => &font_data::REGULAR_HEADING_SMALL,
    }
}

pub use pulp_render::layout::Style;

// complete set of four style variants at a single size tier
// missing weights fall back to regular automatically
#[derive(Clone, Copy)]
pub struct FontSet {
    regular: &'static BitmapFont,
    bold: &'static BitmapFont,
    italic: &'static BitmapFont,
    heading: &'static BitmapFont,
}

// measurer for pulp_render::layout: flash-resident tables, so it
// cannot fail
impl Measure for FontSet {
    type Error = Infallible;

    #[inline]
    fn advance(&mut self, ch: char, style: Style) -> Result<u32, Infallible> {
        Ok(self.font(style).advance(ch) as u32)
    }

    #[inline]
    fn line_height(&self, style: Style) -> u16 {
        self.font(style).line_height
    }
}

impl FontSet {
    fn from_fonts(
        regular: &'static BitmapFont,
        bold_candidate: &'static BitmapFont,
        italic_candidate: &'static BitmapFont,
        heading: &'static BitmapFont,
    ) -> Self {
        let bold = if bold_candidate.glyph('A').advance > 0 {
            bold_candidate
        } else {
            regular
        };
        let italic = if italic_candidate.glyph('A').advance > 0 {
            italic_candidate
        } else {
            regular
        };
        Self {
            regular,
            bold,
            italic,
            heading,
        }
    }

    pub fn for_size(idx: u8) -> Self {
        match idx {
            0 => Self::from_fonts(
                &font_data::REGULAR_BODY_XSMALL,
                &font_data::BOLD_BODY_XSMALL,
                &font_data::ITALIC_BODY_XSMALL,
                &font_data::REGULAR_HEADING_XSMALL,
            ),
            1 => Self::from_fonts(
                &font_data::REGULAR_BODY_SMALL,
                &font_data::BOLD_BODY_SMALL,
                &font_data::ITALIC_BODY_SMALL,
                &font_data::REGULAR_HEADING_SMALL,
            ),
            2 => Self::from_fonts(
                &font_data::REGULAR_BODY_MEDIUM,
                &font_data::BOLD_BODY_MEDIUM,
                &font_data::ITALIC_BODY_MEDIUM,
                &font_data::REGULAR_HEADING_MEDIUM,
            ),
            3 => Self::from_fonts(
                &font_data::REGULAR_BODY_LARGE,
                &font_data::BOLD_BODY_LARGE,
                &font_data::ITALIC_BODY_LARGE,
                &font_data::REGULAR_HEADING_LARGE,
            ),
            4 => Self::from_fonts(
                &font_data::REGULAR_BODY_XLARGE,
                &font_data::BOLD_BODY_XLARGE,
                &font_data::ITALIC_BODY_XLARGE,
                &font_data::REGULAR_HEADING_XLARGE,
            ),
            _ => Self::from_fonts(
                &font_data::REGULAR_BODY_SMALL,
                &font_data::BOLD_BODY_SMALL,
                &font_data::ITALIC_BODY_SMALL,
                &font_data::REGULAR_HEADING_SMALL,
            ),
        }
    }

    #[inline]
    pub fn font(&self, style: Style) -> &'static BitmapFont {
        match style {
            Style::Regular => self.regular,
            Style::Bold => self.bold,
            Style::Italic => self.italic,
            Style::Heading => self.heading,
        }
    }

    #[inline]
    pub fn line_height(&self, style: Style) -> u16 {
        self.font(style).line_height
    }

    #[inline]
    pub fn ascent(&self, style: Style) -> u16 {
        self.font(style).ascent
    }

    #[inline]
    pub fn advance(&self, ch: char, style: Style) -> u8 {
        self.font(style).advance(ch)
    }

    #[inline]
    pub fn advance_byte(&self, b: u8, style: Style) -> u8 {
        self.font(style).advance(bitmap::byte_to_char(b))
    }

    #[inline]
    pub fn draw_char(
        &self,
        strip: &mut StripBuffer,
        ch: char,
        style: Style,
        cx: i32,
        baseline: i32,
    ) -> u8 {
        self.font(style).draw_char(strip, ch, cx, baseline)
    }

    pub fn draw_bytes(
        &self,
        strip: &mut StripBuffer,
        text: &[u8],
        style: Style,
        cx: i32,
        baseline: i32,
    ) -> i32 {
        self.font(style).draw_bytes(strip, text, cx, baseline)
    }

    pub fn draw_str(
        &self,
        strip: &mut StripBuffer,
        text: &str,
        style: Style,
        cx: i32,
        baseline: i32,
    ) -> i32 {
        self.font(style).draw_str(strip, text, cx, baseline)
    }

    /// Draw one laid-out line, resolving the markup markers inside it the
    /// same way `layout::line_glyphs` does when a pack lays the page out.
    ///
    /// This lives here rather than in the reader so that the built-in and
    /// pack paths share one page-driven interface: the reader picks a
    /// `BookFont` once and then only hands it lines. The two must agree on
    /// what a line contains, or a page wrapped by one would be drawn as if it
    /// had been wrapped by the other.
    pub fn draw_span(
        &self,
        strip: &mut StripBuffer,
        buf: &[u8],
        span: pulp_render::layout::LineSpan,
        markup: pulp_render::layout::Markup,
        cx: i32,
        baseline: i32,
    ) {
        use pulp_render::layout::Style as LStyle;

        if span.is_image() {
            return;
        }

        // walk markup and scalars together with layout's scanner, so the
        // bytes consumed and the style at each glyph are identical
        let mut x = cx;
        for (ch, style) in pulp_render::layout::line_glyphs(buf, &span, markup) {
            let style = match style {
                LStyle::Bold => Style::Bold,
                LStyle::Italic => Style::Italic,
                LStyle::Heading => Style::Heading,
                LStyle::Regular => Style::Regular,
            };
            x += self.draw_char(strip, ch, style, x, baseline) as i32;
        }
    }
}
