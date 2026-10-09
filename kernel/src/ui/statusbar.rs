// battery status and stack-measurement utilities

use core::fmt::Write as _;

use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_8X13},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};

use crate::board::SCREEN_W;
use crate::drivers::strip::StripBuffer;
use crate::ui::{Region, StackFmt};

pub const BAR_HEIGHT: u16 = 4;

pub const BATTERY_W: u16 = 68;
pub const BATTERY_X: u16 = (SCREEN_W - BATTERY_W) / 2;
pub const BATTERY_Y: u16 = 6;
pub const BATTERY_H: u16 = 16;
pub const BATTERY_REGION: Region = Region::new(BATTERY_X, BATTERY_Y, BATTERY_W, BATTERY_H);

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct BatteryStatus {
    percent: Option<u8>,
}

impl BatteryStatus {
    pub const fn new() -> Self {
        Self { percent: None }
    }

    pub fn percent(&self) -> Option<u8> {
        self.percent
    }

    // A missing boot reading is encoded as 0 mV. Show an unknown value
    // instead of presenting a failed measurement as an empty battery.
    pub fn update(&mut self, cell_mv: u16) -> bool {
        let percent = (cell_mv != 0).then(|| pulp_board_logic::battery::percentage(cell_mv));
        if self.percent == percent {
            return false;
        }
        self.percent = percent;
        true
    }

    pub fn draw(&self, strip: &mut StripBuffer) {
        self.draw_in(strip, BATTERY_REGION);
    }

    pub fn draw_in(&self, strip: &mut StripBuffer, region: Region) {
        if !region.intersects(strip.logical_window()) {
            return;
        }
        let _ = strip.fill_solid(&region.to_rect(), BinaryColor::Off);

        let icon_x = region.x as i32 + 4;
        let icon_y = region.y as i32 + 2;
        let outline = Rectangle::new(Point::new(icon_x, icon_y), Size::new(22, 12));
        let _ = outline
            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
            .draw(strip);
        let _ = strip.fill_solid(
            &Rectangle::new(Point::new(icon_x + 22, icon_y + 3), Size::new(2, 6)),
            BinaryColor::On,
        );
        if let Some(percent) = self.percent {
            let fill_w = 16 * percent as u32 / 100;
            if fill_w > 0 {
                let _ = strip.fill_solid(
                    &Rectangle::new(Point::new(icon_x + 3, icon_y + 3), Size::new(fill_w, 6)),
                    BinaryColor::On,
                );
            }
        }

        let mut label = StackFmt::<4>::new();
        if let Some(percent) = self.percent {
            let _ = write!(label, "{}%", percent);
        } else {
            let _ = label.write_str("--%");
        }
        let style = TextStyleBuilder::new()
            .alignment(Alignment::Left)
            .baseline(Baseline::Top)
            .build();
        let _ = Text::with_text_style(
            label.as_str(),
            Point::new(icon_x + 28, region.y as i32 + 2),
            MonoTextStyle::new(&FONT_8X13, BinaryColor::On),
            style,
        )
        .draw(strip);
    }
}

const STACK_PAINT_WORD: u32 = 0xDEAD_BEEF;

const STACK_GUARD_SKIP: usize = 256;

pub fn paint_stack() {
    #[cfg(target_arch = "riscv32")]
    {
        let sp: usize;
        unsafe {
            core::arch::asm!("mv {}, sp", out(reg) sp);
        }

        unsafe extern "C" {
            static _stack_end_cpu0: u8;
        }
        let bottom = (&raw const _stack_end_cpu0) as usize;

        let paint_bottom = bottom + STACK_GUARD_SKIP;
        let paint_top = sp.saturating_sub(STACK_GUARD_SKIP);

        if paint_top <= paint_bottom {
            return;
        }

        let start = (paint_bottom + 3) & !3;

        let mut addr = start;
        while addr + 4 <= paint_top {
            unsafe {
                core::ptr::write_volatile(addr as *mut u32, STACK_PAINT_WORD);
            }
            addr += 4;
        }
    }
}

pub fn free_stack_bytes() -> usize {
    #[cfg(target_arch = "riscv32")]
    {
        let sp: usize;
        unsafe {
            core::arch::asm!("mv {}, sp", out(reg) sp);
        }

        unsafe extern "C" {
            static _stack_end_cpu0: u8;
        }
        let stack_bottom = (&raw const _stack_end_cpu0) as usize;
        sp.saturating_sub(stack_bottom)
    }

    #[cfg(not(target_arch = "riscv32"))]
    {
        0
    }
}

pub fn stack_high_water_mark() -> usize {
    #[cfg(target_arch = "riscv32")]
    {
        unsafe extern "C" {
            static _stack_end_cpu0: u8;
            static _stack_start_cpu0: u8;
        }
        let bottom = (&raw const _stack_end_cpu0) as usize;
        let top = (&raw const _stack_start_cpu0) as usize;

        let scan_bottom = bottom + STACK_GUARD_SKIP;

        let start = (scan_bottom + 3) & !3;

        let mut addr = start;
        while addr + 4 <= top {
            let val = unsafe { core::ptr::read_volatile(addr as *const u32) };
            if val != STACK_PAINT_WORD {
                break;
            }
            addr += 4;
        }

        top.saturating_sub(addr)
    }

    #[cfg(not(target_arch = "riscv32"))]
    {
        0
    }
}
