use pulp_board_logic::ssd1677::Rotation;
use pulp_board_logic::strip::STRIP_BUF_SIZE;
use pulp_host::drivers::strip::{STRIP_COUNT, StripBuffer};

fn draw(target: &mut StripBuffer<'_>) {
    target.fill_logical_rect(3, 5, 219, 301, true);
    target.fill_logical_rect(17, 31, 103, 155, false);
    target.blit_1bpp(&[0x81, 0x42, 0x24], 0, 8, 3, 1, 29, 50, true);
}

#[test]
fn one_pass_frame_matches_strip_rendering_in_every_rotation() {
    for rotation in [
        Rotation::Deg0,
        Rotation::Deg90,
        Rotation::Deg180,
        Rotation::Deg270,
    ] {
        let mut pixels = vec![0; 48_000];
        let mut frame = StripBuffer::frame(&mut pixels, rotation).unwrap();
        draw(&mut frame);
        let mut strip = StripBuffer::new();
        for idx in 0..STRIP_COUNT {
            strip.begin_strip(rotation, idx);
            draw(&mut strip);
            let at = idx as usize * STRIP_BUF_SIZE;
            assert_eq!(strip.data(), &frame.data()[at..at + STRIP_BUF_SIZE]);
        }
    }
}

#[test]
fn reader_page_has_identical_pixels_with_one_draw() {
    use pulp_host::{
        reader::Rig,
        render::{render_once, render_stitched},
        storage::VirtualStorage,
    };
    let text = "One pass drawing with punctuation, headings and paragraph wrapping. ".repeat(600);
    let mut rig = Rig::new(VirtualStorage::memory_with(&[(
        "BOOK.TXT",
        text.as_bytes(),
    )]));
    rig.open("BOOK.TXT");
    rig.prepare_render();
    let once = render_once(&|s| rig.draw(s));
    let stitched = render_stitched(&|s| rig.draw(s));
    assert_eq!(once.passes.len(), 1);
    assert_eq!(once.frame.to_pbm(), stitched.frame.to_pbm());
}
