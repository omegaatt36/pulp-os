use core::convert::Infallible;

use embedded_graphics::{pixelcolor::BinaryColor, prelude::*, primitives::PrimitiveStyle};

use crate::drivers::strip::StripBuffer;
use crate::fonts::{Style, bitmap::BitmapFont, cjk::PreparedFonts};
use crate::ui::{Alignment, Region};

pub struct BitmapLabel<'a> {
    region: Region,
    text: &'a str,
    font: &'static BitmapFont,
    alignment: Alignment,
    inverted: bool,
}

impl<'a> BitmapLabel<'a> {
    pub fn new(region: Region, text: &'a str, font: &'static BitmapFont) -> Self {
        Self {
            region,
            text,
            font,
            alignment: Alignment::CenterLeft,
            inverted: false,
        }
    }

    pub const fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    pub const fn inverted(mut self, inverted: bool) -> Self {
        self.inverted = inverted;
        self
    }

    pub fn draw_prepared(
        &self,
        strip: &mut StripBuffer,
        fonts: &PreparedFonts<'_>,
    ) -> Result<(), Infallible> {
        draw_prepared_text(
            strip,
            self.region,
            self.text,
            self.font,
            self.alignment,
            self.inverted,
            fonts,
        )
    }

    pub fn draw(&self, strip: &mut StripBuffer) -> Result<(), Infallible> {
        draw_bitmap_text(
            strip,
            self.region,
            self.text,
            self.font,
            self.alignment,
            self.inverted,
        )
    }
}

pub struct BitmapDynLabel<const N: usize> {
    region: Region,
    buffer: [u8; N],
    len: usize,
    font: &'static BitmapFont,
    alignment: Alignment,
    inverted: bool,
}

impl<const N: usize> BitmapDynLabel<N> {
    pub fn new(region: Region, font: &'static BitmapFont) -> Self {
        Self {
            region,
            buffer: [0u8; N],
            len: 0,
            font,
            alignment: Alignment::CenterLeft,
            inverted: false,
        }
    }

    pub const fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    pub const fn inverted(mut self, inverted: bool) -> Self {
        self.inverted = inverted;
        self
    }

    pub fn set_text(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let n = scalar_prefix(text, N);
        self.buffer[..n].copy_from_slice(&bytes[..n]);
        self.len = n;
    }

    pub fn clear_text(&mut self) {
        self.len = 0;
    }

    pub fn text(&self) -> &str {
        core::str::from_utf8(&self.buffer[..self.len]).unwrap_or("")
    }

    pub fn draw_prepared(
        &self,
        strip: &mut StripBuffer,
        fonts: &PreparedFonts<'_>,
    ) -> Result<(), Infallible> {
        draw_prepared_text(
            strip,
            self.region,
            self.text(),
            self.font,
            self.alignment,
            self.inverted,
            fonts,
        )
    }

    pub fn draw(&self, strip: &mut StripBuffer) -> Result<(), Infallible> {
        draw_bitmap_text(
            strip,
            self.region,
            self.text(),
            self.font,
            self.alignment,
            self.inverted,
        )
    }
}

impl<const N: usize> core::fmt::Write for BitmapDynLabel<N> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let available = N - self.len;
        let n = scalar_prefix(s, available);
        self.buffer[self.len..self.len + n].copy_from_slice(&bytes[..n]);
        self.len += n;
        Ok(())
    }
}

fn draw_bitmap_text(
    strip: &mut StripBuffer,
    region: Region,
    text: &str,
    font: &'static BitmapFont,
    alignment: Alignment,
    inverted: bool,
) -> Result<(), Infallible> {
    if !region.intersects(strip.logical_window()) {
        return Ok(());
    }

    let (bg, fg) = if inverted {
        (BinaryColor::On, BinaryColor::Off)
    } else {
        (BinaryColor::Off, BinaryColor::On)
    };

    region
        .to_rect()
        .into_styled(PrimitiveStyle::with_fill(bg))
        .draw(strip)?;

    font.draw_aligned(strip, region, text, alignment, fg);

    Ok(())
}

fn scalar_prefix(text: &str, capacity: usize) -> usize {
    let mut n = text.len().min(capacity);
    while !text.is_char_boundary(n) {
        n -= 1;
    }
    n
}

/// The widget owns Latin appearance; the immutable view supplies absent glyphs.
pub fn draw_prepared_text(
    strip: &mut StripBuffer,
    region: Region,
    text: &str,
    font: &'static BitmapFont,
    alignment: Alignment,
    inverted: bool,
    fonts: &PreparedFonts<'_>,
) -> Result<(), Infallible> {
    if !region.intersects(strip.logical_window()) {
        return Ok(());
    }
    let (bg, fg) = if inverted {
        (BinaryColor::On, BinaryColor::Off)
    } else {
        (BinaryColor::Off, BinaryColor::On)
    };
    region
        .to_rect()
        .into_styled(PrimitiveStyle::with_fill(bg))
        .draw(strip)?;
    draw_prepared_aligned(strip, region, text, font, alignment, fg, fonts);
    Ok(())
}

pub fn draw_prepared_aligned(
    strip: &mut StripBuffer,
    region: Region,
    text: &str,
    font: &'static BitmapFont,
    alignment: Alignment,
    fg: BinaryColor,
    fonts: &PreparedFonts<'_>,
) {
    let style = if core::ptr::eq(font, fonts.font(Style::Heading)) {
        Style::Heading
    } else {
        Style::Regular
    };
    let width: u32 = text
        .chars()
        .map(|ch| {
            if font.has_glyph(ch) {
                u32::from(font.advance(ch))
            } else {
                u32::from(fonts.advance(ch, style))
            }
        })
        .sum();
    let pos = alignment.position(region, Size::new(width, font.line_height.into()));
    let baseline = pos.y + i32::from(font.ascent);
    let mut x = pos.x;
    for ch in text.chars() {
        x += if font.has_glyph(ch) {
            i32::from(font.draw_char_fg(strip, ch, fg, x, baseline))
        } else {
            i32::from(fonts.draw_char_fg(strip, ch, style, fg, x, baseline))
        };
    }
}

pub fn draw_surface_error(
    strip: &mut StripBuffer,
    region: Region,
    font: &'static BitmapFont,
    error: crate::error::Error,
) {
    use core::fmt::Write;
    let mut text = crate::ui::stack_fmt::StackFmt::<48>::new();
    let _ = write!(text, "Font: {}", error);
    let _ = BitmapLabel::new(region, text.as_str(), font).draw(strip);
}
