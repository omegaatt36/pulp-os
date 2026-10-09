use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_8X13},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
use pulp_host::apps::files::FilesApp;
use pulp_host::apps::home::HomeApp;
use pulp_host::apps::manager::AppManager;
use pulp_host::apps::reader::ReaderApp;
use pulp_host::apps::settings::SettingsApp;
use pulp_host::apps::widgets::{ButtonFeedback, QuickMenu};
use pulp_host::apps::{Launcher, Redraw};
use pulp_host::board::action::ButtonMapper;
use pulp_host::drivers::sdcard::SdStorage;
use pulp_host::kernel::Kernel;
use pulp_host::reader::Rig;
use pulp_host::render::{Framebuffer, render_full, render_stitched};
use pulp_host::storage::VirtualStorage;
use pulp_host::ui::Region;
use pulp_host::ui::statusbar::{BATTERY_REGION, BatteryStatus};

fn region_pixels(frame: &Framebuffer, r: Region) -> Vec<bool> {
    (r.y..r.y + r.h)
        .flat_map(|y| (r.x..r.x + r.w).map(move |x| frame.is_black(x, y)))
        .collect()
}

#[test]
fn percentage_and_icon_render_within_battery_region() {
    for (mv, label, fill) in [
        (0, "--%", 0),
        (3000, "0%", 0),
        (3830, "50%", 8),
        (4200, "100%", 16),
    ] {
        let mut status = BatteryStatus::new();
        status.update(mv);
        let frame = render_full(&|s| status.draw(s)).frame;
        assert_eq!(
            frame.to_pbm(),
            render_stitched(&|s| status.draw(s)).frame.to_pbm()
        );
        let overwritten = render_full(&|s| {
            s.fill_solid(&BATTERY_REGION.to_rect(), BinaryColor::On)
                .unwrap();
            status.draw(s);
        });
        assert_eq!(frame.to_pbm(), overwritten.frame.to_pbm());
        let text_region = Region::new(
            BATTERY_REGION.x + 28,
            BATTERY_REGION.y,
            36,
            BATTERY_REGION.h,
        );
        let reference = render_full(&|s| {
            Text::with_text_style(
                label,
                Point::new(
                    (BATTERY_REGION.x + 32) as i32,
                    (BATTERY_REGION.y + 2) as i32,
                ),
                MonoTextStyle::new(&FONT_8X13, BinaryColor::On),
                TextStyleBuilder::new()
                    .alignment(Alignment::Left)
                    .baseline(Baseline::Top)
                    .build(),
            )
            .draw(s)
            .unwrap();
        });
        assert_eq!(
            region_pixels(&frame, text_region),
            region_pixels(&reference.frame, text_region),
            "{label} must be fully visible"
        );
        let icon_x = BATTERY_REGION.x + 4;
        let icon_y = BATTERY_REGION.y + 2;
        for x in 0..16 {
            assert_eq!(frame.is_black(icon_x + 3 + x, icon_y + 4), x < fill);
        }
        for y in 0..800 {
            for x in 0..480 {
                if frame.is_black(x, y) {
                    assert!(
                        x >= BATTERY_REGION.x && x < BATTERY_REGION.x + BATTERY_REGION.w,
                        "pixel at ({x}, {y}) outside battery region x"
                    );
                    assert!(
                        y >= BATTERY_REGION.y && y < BATTERY_REGION.y + BATTERY_REGION.h,
                        "pixel at ({x}, {y}) outside battery region y"
                    );
                }
            }
        }
    }
}

#[test]
fn reader_shows_battery_in_top_center_between_title_and_progress() {
    let card = VirtualStorage::memory_with(&[("TEST.TXT", b"Hello world, this is a test book.")]);
    let mut rig = Rig::new(card);
    rig.set_battery_mv(4200);
    rig.open("TEST.TXT");
    let frame = render_full(&|s| rig.draw(s)).frame;

    // Check battery region contains battery drawing
    let mut expected_battery = BatteryStatus::new();
    expected_battery.update(4200);
    let battery_frame = render_full(&|s| expected_battery.draw(s)).frame;
    assert_eq!(
        region_pixels(&frame, BATTERY_REGION),
        region_pixels(&battery_frame, BATTERY_REGION)
    );

    // Title region on the left contains ink (the book title)
    let header_region = Region::new(8, 6, 190, 16);
    let title_ink = region_pixels(&frame, header_region).iter().any(|&b| b);
    assert!(title_ink, "header region must show book title");

    // Status region on the right contains ink (reading progress)
    let status_region = Region::new(282, 6, 190, 16);
    let status_ink = region_pixels(&frame, status_region).iter().any(|&b| b);
    assert!(status_ink, "status region must show reading progress");
}

#[test]
fn home_screen_does_not_display_battery() {
    let card = VirtualStorage::memory();
    card.ensure_pulp_dir().unwrap();
    let mut k = Kernel::new(SdStorage::new(card));
    k.set_battery_mv(4200);
    let mut apps = AppManager::new(
        Box::leak(Box::new(Launcher::new())),
        Box::leak(Box::new(HomeApp::new())),
        Box::leak(Box::new(FilesApp::new())),
        Box::leak(Box::new(ReaderApp::new())),
        Box::leak(Box::new(SettingsApp::new())),
        Box::leak(Box::new(QuickMenu::new())),
        Box::leak(Box::new(ButtonFeedback::new())),
        ButtonMapper::new(),
    );
    apps.load_eager_settings(&mut k.handle());
    apps.enter_initial(&mut k.handle());
    apps.prepare_render(&mut k.handle());
    let frame = render_full(&|s| apps.draw(s)).frame;

    let mut battery = BatteryStatus::new();
    battery.update(4200);
    let battery_frame = render_full(&|s| battery.draw(s)).frame;
    assert_ne!(
        region_pixels(&frame, BATTERY_REGION),
        region_pixels(&battery_frame, BATTERY_REGION)
    );
    for y in 0..4 {
        for x in 0..480 {
            assert!(
                !frame.is_black(x, y),
                "top status bar at y={y} should be white on home screen"
            );
        }
    }
}

#[test]
fn reader_refreshes_battery_region_when_percentage_changes() {
    let card = VirtualStorage::memory_with(&[("TEST.TXT", b"Some content for test.")]);
    let mut rig = Rig::new(card);
    rig.set_battery_mv(4200);
    rig.open("TEST.TXT");
    let _ = rig.take_redraw();

    // Still 4200 mV (100%): no redraw
    rig.tick();
    assert_eq!(rig.take_redraw(), Redraw::None);

    // Drop to 3000 mV (0%): percentage changed, marks BATTERY_REGION dirty!
    rig.set_battery_mv(3000);
    rig.tick();
    assert_eq!(rig.take_redraw(), Redraw::Partial(BATTERY_REGION));

    // 3050 mV (still 0%): percentage unchanged, no redraw
    rig.set_battery_mv(3050);
    rig.tick();
    assert_eq!(rig.take_redraw(), Redraw::None);
}
