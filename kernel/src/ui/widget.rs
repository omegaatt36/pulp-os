// wrap helpers, progress bar, loading indicator
// (Region and Alignment live in pulp_render::geometry)

use embedded_graphics::{
    mono_font::MonoTextStyle, mono_font::ascii::FONT_9X18, pixelcolor::BinaryColor, prelude::*,
    primitives::PrimitiveStyle, primitives::Rectangle, text::Text,
};

use crate::ui::stack_fmt::BorrowedFmt;
use pulp_render::geometry::Region;
use pulp_render::strip::StripBuffer;

#[inline]
pub fn wrap_next(current: usize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    if current + 1 >= count { 0 } else { current + 1 }
}

#[inline]
pub fn wrap_prev(current: usize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    if current == 0 { count - 1 } else { current - 1 }
}

// horizontal progress bar for 1-bit e-paper
// draws a 1px black border around the full track and fills
// proportionally from the left; pct is clamped to 0..=100
// region should be at least 4px wide and 4px tall
pub fn draw_progress_bar(strip: &mut StripBuffer, region: Region, pct: u8) {
    let pct = pct.min(100) as u32;

    // clear region
    region
        .to_rect()
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
        .draw(strip)
        .unwrap();

    // 1px border shows full extent even at 0%
    region
        .to_rect()
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(strip)
        .unwrap();

    // filled portion inside the border
    if pct > 0 && region.w > 2 && region.h > 2 {
        let inner_w = (region.w - 2) as u32;
        let fill_w = (inner_w * pct / 100).max(1);
        Rectangle::new(
            Point::new((region.x + 1) as i32, (region.y + 1) as i32),
            Size::new(fill_w, (region.h - 2) as u32),
        )
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(strip)
        .unwrap();
    }
}

// loading indicator for 1-bit e-paper
// draws "msg...pct%" centered vertically in the region using the
// built-in FONT_9X18 mono font; works without any custom bitmap
// fonts loaded, usable from any app or the kernel itself
//
// typical usage:
//   draw_loading_indicator(strip, region, "Loading", 25)  => "Loading...25%"
//   draw_loading_indicator(strip, region, "Caching 3/15", 20)  => "Caching 3/15...20%"
pub fn draw_loading_indicator(strip: &mut StripBuffer, region: Region, msg: &str, pct: u8) {
    use core::fmt::Write;

    // clear region
    region
        .to_rect()
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
        .draw(strip)
        .unwrap();

    // format "msg...pct%"
    let mut buf = [0u8; 48];
    let mut fmt = BorrowedFmt::new(&mut buf);
    let _ = write!(fmt, "{}...{}%", msg, pct.min(100));
    let text = fmt.as_str();

    // FONT_9X18: 9px wide, 18px tall, ~14px ascent
    // center vertically; baseline = region.y + (h + 9) / 2
    let style = MonoTextStyle::new(&FONT_9X18, BinaryColor::On);
    let baseline_y = region.y as i32 + (region.h as i32 + 9) / 2;
    Text::new(text, Point::new(region.x as i32 + 2, baseline_y), style)
        .draw(strip)
        .unwrap();
}
