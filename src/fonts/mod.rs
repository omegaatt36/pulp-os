// build-time rasterised bitmap fonts for e-ink rendering
// TTFs rasterised by build.rs via fontdue into 1-bit tables in flash
// zero heap, zero parsing at runtime
//
// five size tiers: 0=XSmall  1=Small  2=Medium  3=Large  4=XLarge

pub mod bitmap;
pub mod cjk;

#[allow(clippy::all)]
pub mod font_data {
    include!(concat!(env!("OUT_DIR"), "/font_data.rs"));
}

use crate::drivers::strip::StripBuffer;
use bitmap::BitmapFont;

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    Regular,
    Bold,
    Italic,
    Heading,
}

/// Independent style markers retain their state until their own closing marker.
/// Heading has precedence over nested bold and italic emphasis.
#[derive(Clone, Copy)]
pub struct StyleState {
    flags: u8,
}
impl StyleState {
    pub const fn from_flags(flags: u8) -> Self {
        Self { flags: flags & 7 }
    }
    pub fn flags(self) -> u8 {
        self.flags
    }
    pub fn apply_marker(&mut self, marker: u8) {
        use smol_epub::html_strip::{
            BOLD_OFF, BOLD_ON, HEADING_OFF, HEADING_ON, ITALIC_OFF, ITALIC_ON,
        };
        match marker {
            BOLD_ON => self.flags |= 1,
            BOLD_OFF => self.flags &= !1,
            ITALIC_ON => self.flags |= 2,
            ITALIC_OFF => self.flags &= !2,
            HEADING_ON => self.flags |= 4,
            HEADING_OFF => self.flags &= !4,
            _ => {}
        }
    }
    pub fn style(self) -> Style {
        if self.flags & 4 != 0 {
            Style::Heading
        } else if self.flags & 1 != 0 {
            Style::Bold
        } else if self.flags & 2 != 0 {
            Style::Italic
        } else {
            Style::Regular
        }
    }
}

// complete set of four style variants at a single size tier
// missing weights fall back to regular automatically
#[derive(Clone, Copy)]
pub struct FontSet {
    regular: &'static BitmapFont,
    bold: &'static BitmapFont,
    italic: &'static BitmapFont,
    heading: &'static BitmapFont,
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

    /// Fingerprint of everything the layout reads from this set: per style the
    /// line metrics and the advance of every ASCII and extended glyph. A page
    /// index stored for other font data (another firmware build, other assets)
    /// does not match.
    pub fn signature(&self) -> u32 {
        let mut h = pulp_board_logic::source_id::Fnv64::new();
        for style in [Style::Regular, Style::Bold, Style::Italic, Style::Heading] {
            let font = self.font(style);
            h.update(&font.line_height.to_le_bytes());
            h.update(&font.ascent.to_le_bytes());
            for glyph in font.glyphs.iter() {
                h.update(&[glyph.advance]);
            }
            for (cp, glyph) in font.ext_codepoints.iter().zip(font.ext_glyphs.iter()) {
                h.update_u32(*cp);
                h.update(&[glyph.advance]);
            }
        }
        let v = h.finish();
        (v ^ (v >> 32)) as u32
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
}
