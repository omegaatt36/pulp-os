// 4-level gray probe for the OSPTEK EPD0426A02 (SSD1677): not used by the reader.
//
// Everything here is the sequence of the vendor's own bring-up example
// (vendor/epd-4.26-480x800-spi-ssd1677, `epd_init_4g` / `epd_update_4g`), which
// drives this very panel: border 0x00, the 112 byte waveform table through
// 0x32, the gate / source / VCOM voltages from its tail through 0x03 / 0x04 /
// 0x2C, both RAM planes, then 0x22 <- 0xC7 with the table in the LUT register
// (no LOAD_LUT, no LOAD_TEMP: the table is not the OTP one).
//
// The vendor packs a 2-bit pixel (3 = white .. 0 = black) so that the
// {RED, BW} index of the pixel is 3 - level: white is waveform 0, black
// waveform 3. `vendor_plane_bits_match_three_minus_level` pins that to a port of
// the vendor's loop. The probe screen shows the four raw indexes in bands (so
// the mapping is read off the panel, not assumed) and a dithered gradient drawn
// with the vendor's mapping.
//
// Rows are stream rows: the 100 bytes (800 pixels, MSB = lowest x) written
// between the RAM window setup and the next command, in the order the
// controller takes them. How that lands on the glass depends on the data entry
// mode (see `ssd1677::set_ram_area`), so the orientation of the pattern on the
// panel is one of the things the probe tells.

use crate::power::DelayMs;
use crate::ssd1677::{
    BusyPin, DisplayError, EpdBus, HEIGHT, Rotation, WIDTH, cmd, set_ram_area, wait_busy_bounded,
};
use crate::strip::STRIP_COUNT;

/// Waveform table + voltages, as the vendor's `s_lut_4g`: VS 50 bytes (LUT0..4,
/// ten groups each), TP/RP 50 bytes (ten groups of TP[A..D] + RP), frame rate 5
/// bytes, then VGH, VSH1, VSH2, VSL, VCOM and two reserved bytes.
pub const LUT_4G: [u8; LUT_LEN] = [
    0x80, 0x48, 0x4A, 0x22, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x0A, 0x48, 0x68, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x88, 0x48, 0x60, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0xA8, 0x48, 0x45, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x07, 0x1E, 0x1C, 0x02, 0x00, //
    0x05, 0x01, 0x05, 0x01, 0x02, //
    0x08, 0x02, 0x01, 0x04, 0x04, //
    0x00, 0x02, 0x00, 0x02, 0x01, //
    0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x01, //
    0x22, 0x22, 0x22, 0x22, 0x22, //
    0x17, 0x41, 0xA8, 0x32, 0x30, //
    0x00, 0x00,
];

pub const LUT_LEN: usize = 112;

/// Byte offsets of the voltages in the tail of `LUT_4G`.
const VGH: usize = 105;
const VSH1: usize = 106;
const VSH2: usize = 107;
const VSL: usize = 108;
const VCOM: usize = 109;

/// Registers outside `ssd1677::cmd`.
const GATE_VOLTAGE: u8 = 0x03;
const SOURCE_VOLTAGE: u8 = 0x04;
const WRITE_VCOM: u8 = 0x2C;
const WRITE_LUT: u8 = 0x32;

/// 0x22 values: clock + analog on, and the 4-gray update (clock, analog, display
/// with the LUT register as it is, analog and oscillator off).
const POWER_ON: u8 = 0xC0;
const GRAY_UPDATE: u8 = 0xC7;

/// The vendor allows 4 s for the gray update, 100 ms for the power-on.
pub const POWER_ON_TIMEOUT_MS: u32 = 1_000;
pub const GRAY_TIMEOUT_MS: u32 = 10_000;

pub const ROW_BYTES: usize = WIDTH as usize / 8;
pub const ROWS: u16 = HEIGHT;
/// Rows per band of raw indexes at the top of the screen.
pub const BAND_ROWS: u16 = 60;
const BAND_COUNT: u16 = 4;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Plane {
    /// RAM 0x26: the high bit of the index.
    Red,
    /// RAM 0x24: the low bit of the index.
    Bw,
}

impl Plane {
    const fn command(self) -> u8 {
        match self {
            Plane::Red => cmd::WRITE_RAM_RED,
            Plane::Bw => cmd::WRITE_RAM_BW,
        }
    }

    const fn bit(self, index: u8) -> u8 {
        match self {
            Plane::Red => (index >> 1) & 1,
            Plane::Bw => index & 1,
        }
    }
}

const BAYER4: [[u32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// The raw `{RED, BW}` index of pixel `x` in stream row `row`.
///
/// Rows 0..240: four bands of `BAND_ROWS`, indexes 0, 1, 2, 3 in that order.
/// Rows 240..480: a left-to-right gradient, black to white, quantised to the four
/// levels with a 4x4 ordered dither and mapped as the vendor does (index = 3 -
/// level), so the left edge is index 3 and the right edge index 0.
pub fn probe_index(x: u16, row: u16) -> u8 {
    if row < BAND_ROWS * BAND_COUNT {
        return (row / BAND_ROWS) as u8;
    }
    let t = u32::from(x) * 255 / u32::from(WIDTH - 1);
    let v = t * 3;
    let base = v / 255;
    let frac = (v % 255) * 16 / 255;
    let threshold = BAYER4[usize::from(row & 3)][usize::from(x & 3)];
    let level = if frac > threshold { base + 1 } else { base }.min(3);
    (3 - level) as u8
}

/// One stream row of `plane`.
pub fn fill_row(plane: Plane, row: u16, out: &mut [u8; ROW_BYTES]) {
    for (byte, slot) in out.iter_mut().enumerate() {
        let mut b = 0u8;
        for bit in 0..8u16 {
            let x = byte as u16 * 8 + bit;
            b |= plane.bit(probe_index(x, row)) << (7 - bit);
        }
        *slot = b;
    }
}

/// Table, gate / source / VCOM voltages. The table goes out whole (112 bytes), as
/// the vendor does, although the datasheet lists 105 for 0x32.
pub fn load_lut<B: EpdBus>(b: &mut B) -> Result<(), DisplayError> {
    b.command(WRITE_LUT)?;
    b.data(&LUT_4G)?;

    b.command(GATE_VOLTAGE)?;
    b.data(&[LUT_4G[VGH]])?;

    b.command(SOURCE_VOLTAGE)?;
    b.data(&[LUT_4G[VSH1], LUT_4G[VSH2], LUT_4G[VSL]])?;

    b.command(WRITE_VCOM)?;
    b.data(&[LUT_4G[VCOM]])
}

/// Both planes, RED then BW (the vendor's order), each after a full window
/// setup, eight rows per transfer.
pub fn write_planes<B: EpdBus>(b: &mut B) -> Result<(), DisplayError> {
    const BATCH: usize = 8;
    for plane in [Plane::Red, Plane::Bw] {
        set_ram_area(b, 0, 0, WIDTH, HEIGHT)?;
        b.command(plane.command())?;
        let mut batch = [0u8; ROW_BYTES * BATCH];
        let mut row = 0u16;
        while row < ROWS {
            let rows = BATCH.min(usize::from(ROWS - row));
            for i in 0..rows {
                let mut r = [0u8; ROW_BYTES];
                fill_row(plane, row + i as u16, &mut r);
                batch[i * ROW_BYTES..(i + 1) * ROW_BYTES].copy_from_slice(&r);
            }
            b.data(&batch[..rows * ROW_BYTES])?;
            row += rows as u16;
        }
    }
    Ok(())
}

/// Rendered strips of the two planes (for an image that is already in `{RED,
/// BW}` plane form, see `wallpaper::decode_gray`): strip `idx` of `plane`,
/// 40 physical rows, rotated like `ssd1677::StripSource`.
pub trait PlaneSource {
    fn render_strip(&mut self, plane: Plane, rotation: Rotation, idx: u16) -> &[u8];
}

/// Both planes from `src`, RED then BW, each after a full window setup.
pub fn write_strip_planes<B: EpdBus, S: PlaneSource>(
    b: &mut B,
    rotation: Rotation,
    src: &mut S,
) -> Result<(), DisplayError> {
    for plane in [Plane::Red, Plane::Bw] {
        set_ram_area(b, 0, 0, WIDTH, HEIGHT)?;
        b.command(plane.command())?;
        for idx in 0..STRIP_COUNT {
            b.data(src.render_strip(plane, rotation, idx))?;
        }
    }
    Ok(())
}

/// Kick the gray waveform over the whole panel.
pub fn start_gray_update<B: EpdBus>(b: &mut B) -> Result<(), DisplayError> {
    b.command(cmd::DISPLAY_UPDATE_CONTROL_1)?;
    b.data(&[0x00, 0x00])?;

    b.command(cmd::DISPLAY_UPDATE_CONTROL_2)?;
    b.data(&[GRAY_UPDATE])?;

    b.command(cmd::MASTER_ACTIVATION)
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ProbeTimes {
    /// Table + both planes over SPI.
    pub write_ms: u64,
    /// The gray waveform (BUSY high).
    pub update_ms: u64,
}

/// The whole probe on a controller that was just initialised (soft reset and
/// `ssd1677::configure`). Leaves the table in the LUT register and the panel in
/// the gray state: the caller re-initialises and runs a Clean full refresh to get
/// a black-and-white panel back.
pub fn run_probe<P: EpdBus + DelayMs + BusyPin>(p: &mut P) -> Result<ProbeTimes, DisplayError> {
    run(p, write_planes)
}

/// The same sequence with the planes of an image (`PlaneSource`) instead of the
/// probe pattern: the 4-gray sleep wallpaper. Same precondition and aftermath as
/// `run_probe`.
pub fn show_planes<P: EpdBus + DelayMs + BusyPin, S: PlaneSource>(
    p: &mut P,
    rotation: Rotation,
    src: &mut S,
) -> Result<ProbeTimes, DisplayError> {
    run(p, |p| write_strip_planes(p, rotation, src))
}

fn run<P: EpdBus + DelayMs + BusyPin>(
    p: &mut P,
    write: impl FnOnce(&mut P) -> Result<(), DisplayError>,
) -> Result<ProbeTimes, DisplayError> {
    // the 4-gray border waveform (the black-and-white init uses 0x01)
    p.command(cmd::BORDER_WAVEFORM)?;
    p.data(&[0x00])?;

    let started = p.now_ms();
    load_lut(p)?;
    write(p)?;
    let written = p.now_ms();

    p.command(cmd::DISPLAY_UPDATE_CONTROL_2)?;
    p.data(&[POWER_ON])?;
    p.command(cmd::MASTER_ACTIVATION)?;
    wait_busy_bounded(p, POWER_ON_TIMEOUT_MS)?;

    start_gray_update(p)?;
    let kicked = p.now_ms();
    wait_busy_bounded(p, GRAY_TIMEOUT_MS)?;
    let done = p.now_ms();

    Ok(ProbeTimes {
        write_ms: written.saturating_sub(started),
        update_ms: done.saturating_sub(kicked),
    })
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::string::String;
    use std::vec::Vec;

    #[derive(Debug, PartialEq, Eq)]
    enum Ev {
        Cmd(u8),
        Data(Vec<u8>),
    }

    /// Wire recorder; BUSY is high after each MASTER_ACTIVATION for the next
    /// duration of `busy_ms` (the last one repeats).
    struct Fake {
        log: Vec<Ev>,
        now: u64,
        busy_until: u64,
        busy_ms: Vec<u64>,
        activations: usize,
    }

    impl Fake {
        fn new(busy_ms: u64) -> Self {
            Self::with_busy(&[busy_ms])
        }

        fn with_busy(busy_ms: &[u64]) -> Self {
            Self {
                log: Vec::new(),
                now: 0,
                busy_until: 0,
                busy_ms: busy_ms.to_vec(),
                activations: 0,
            }
        }
    }

    impl EpdBus for Fake {
        fn command(&mut self, c: u8) -> Result<(), DisplayError> {
            if c == cmd::MASTER_ACTIVATION {
                let i = self.activations.min(self.busy_ms.len() - 1);
                self.busy_until = self.now + self.busy_ms[i];
                self.activations += 1;
            }
            self.log.push(Ev::Cmd(c));
            Ok(())
        }
        fn data(&mut self, d: &[u8]) -> Result<(), DisplayError> {
            self.log.push(Ev::Data(d.to_vec()));
            Ok(())
        }
    }

    impl DelayMs for Fake {
        fn delay_ms(&mut self, ms: u32) {
            self.now += u64::from(ms);
        }
    }

    impl BusyPin for Fake {
        fn is_busy(&mut self) -> bool {
            self.now < self.busy_until
        }
        fn now_ms(&mut self) -> u64 {
            self.now
        }
    }

    /// The vendor's `epd_write_image_4g_plane` for one pixel: the bit it sends
    /// to a plane for the 2-bit level `level` (3 = white), after its final `~`.
    fn vendor_plane_bit(level: u8, invert_grey: bool) -> u8 {
        let nibble = level << 6;
        let bit = if nibble == 0xC0 {
            1
        } else if nibble == 0x00 {
            0
        } else if nibble >= 0x80 {
            if invert_grey { 0 } else { 1 }
        } else if invert_grey {
            1
        } else {
            0
        };
        !bit & 1
    }

    #[test]
    fn lut_has_the_vendor_shape() {
        assert_eq!(LUT_4G.len(), 112);
        // VGH 20 V, VSH1 15 V, VSH2 5 V, VSL -15 V, VCOM -1.2 V
        assert_eq!(&LUT_4G[VGH..=VCOM], &[0x17, 0x41, 0xA8, 0x32, 0x30]);
        // frame rate block
        assert_eq!(&LUT_4G[100..105], &[0x22; 5]);
        // the five VS rows (LUT0..LUT4) and the first TP/RP group
        assert_eq!(&LUT_4G[..4], &[0x80, 0x48, 0x4A, 0x22]);
        assert_eq!(&LUT_4G[50..55], &[0x07, 0x1E, 0x1C, 0x02, 0x00]);
        assert_eq!(LUT_4G[99], 0x01);
    }

    #[test]
    fn vendor_plane_bits_match_three_minus_level() {
        for level in 0..4u8 {
            let red = vendor_plane_bit(level, false);
            let bw = vendor_plane_bit(level, true);
            assert_eq!(red << 1 | bw, 3 - level, "level {level}");
        }
    }

    #[test]
    fn band_rows_are_the_four_raw_indexes() {
        for (band, &index) in [0u8, 1, 2, 3].iter().enumerate() {
            let row = band as u16 * BAND_ROWS;
            for x in [0u16, 1, 399, 799] {
                assert_eq!(probe_index(x, row), index);
                assert_eq!(probe_index(x, row + BAND_ROWS - 1), index);
            }
        }
        let mut red = [0u8; ROW_BYTES];
        let mut bw = [0u8; ROW_BYTES];
        let expect = [(0x00, 0x00), (0x00, 0xFF), (0xFF, 0x00), (0xFF, 0xFF)];
        for (band, &(r, b)) in expect.iter().enumerate() {
            let row = band as u16 * BAND_ROWS;
            fill_row(Plane::Red, row, &mut red);
            fill_row(Plane::Bw, row, &mut bw);
            assert!(red.iter().all(|&v| v == r), "band {band} red");
            assert!(bw.iter().all(|&v| v == b), "band {band} bw");
        }
    }

    #[test]
    fn gradient_runs_from_black_index_3_to_white_index_0() {
        let row = BAND_ROWS * BAND_COUNT;
        // far left: black (3), far right: white (0)
        assert!((0..4).all(|r| probe_index(0, row + r) == 3));
        assert!((0..4).all(|r| probe_index(799, row + r) == 0));
        // the mean level rises left to right
        let mean = |x0: u16| -> u32 {
            let mut sum = 0u32;
            for x in x0..x0 + 100 {
                for r in 0..4 {
                    sum += 3 - u32::from(probe_index(x, row + r));
                }
            }
            sum
        };
        let means: Vec<u32> = (0..8).map(|i| mean(i * 100)).collect();
        assert!(means.windows(2).all(|w| w[0] < w[1]), "{means:?}");
        // all four levels are used
        let used: std::collections::BTreeSet<u8> = (0..800u16)
            .flat_map(|x| (0..4u16).map(move |r| probe_index(x, row + r)))
            .collect();
        assert_eq!(used.len(), 4);
    }

    #[test]
    fn plane_rows_are_msb_first() {
        // the gradient row: bit 7 of byte 0 is x = 0, bit 0 of byte 99 is x = 799
        let row = BAND_ROWS * BAND_COUNT;
        let mut bw = [0u8; ROW_BYTES];
        fill_row(Plane::Bw, row, &mut bw);
        for x in [0u16, 1, 2, 7, 8, 400, 799] {
            let byte = bw[usize::from(x / 8)];
            let bit = (byte >> (7 - x % 8)) & 1;
            assert_eq!(bit, probe_index(x, row) & 1, "x {x}");
        }
    }

    #[test]
    fn load_lut_sends_the_table_then_the_voltages() {
        let mut f = Fake::new(0);
        load_lut(&mut f).unwrap();
        assert_eq!(
            f.log,
            [
                Ev::Cmd(0x32),
                Ev::Data(LUT_4G.to_vec()),
                Ev::Cmd(0x03),
                Ev::Data(std::vec![0x17]),
                Ev::Cmd(0x04),
                Ev::Data(std::vec![0x41, 0xA8, 0x32]),
                Ev::Cmd(0x2C),
                Ev::Data(std::vec![0x30]),
            ]
        );
    }

    #[test]
    fn write_planes_sends_48000_bytes_to_each_plane() {
        let mut f = Fake::new(0);
        write_planes(&mut f).unwrap();
        let mut plane_cmd = None;
        let mut bytes = std::collections::BTreeMap::new();
        for e in &f.log {
            match e {
                Ev::Cmd(c @ (0x26 | 0x24)) => plane_cmd = Some(*c),
                Ev::Cmd(_) => {}
                Ev::Data(d) if d.len() > 16 => {
                    *bytes.entry(plane_cmd.unwrap()).or_insert(0usize) += d.len();
                }
                Ev::Data(_) => {}
            }
        }
        assert_eq!(bytes.get(&0x26), Some(&48_000));
        assert_eq!(bytes.get(&0x24), Some(&48_000));
        // RED is written before BW
        let first = |c: u8| f.log.iter().position(|e| *e == Ev::Cmd(c)).unwrap();
        assert!(first(0x26) < first(0x24));
    }

    #[test]
    fn probe_command_trace_follows_the_vendor_sequence() {
        // power-on ~100 ms, then the 4 s gray waveform
        let mut f = Fake::with_busy(&[100, 4000]);
        let t = run_probe(&mut f).unwrap();
        let trace: Vec<String> = f
            .log
            .iter()
            .filter_map(|e| match e {
                Ev::Cmd(c) => Some(std::format!("C{c:02X}")),
                Ev::Data(d) if d.len() <= 4 => Some(std::format!("D{d:02X?}")),
                Ev::Data(d) => Some(std::format!("S{}", d.len())),
            })
            .collect();
        let joined = trace.join(" ");
        // border, table + voltages, planes, power on, gray update
        assert!(joined.starts_with("C3C D[00] C32 S112 C03 D[17] C04 D[41, A8, 32] C2C D[30] C11"));
        assert!(
            joined.ends_with("C22 D[C0] C20 C21 D[00, 00] C22 D[C7] C20"),
            "{joined}"
        );
        assert!(t.update_ms >= 4000 && t.update_ms < 4100, "{t:?}");
    }

    /// Strips of a constant byte per plane.
    struct Flat {
        buf: [u8; 4000],
        asked: Vec<(Plane, u16)>,
    }

    impl PlaneSource for Flat {
        fn render_strip(&mut self, plane: Plane, _r: Rotation, idx: u16) -> &[u8] {
            self.asked.push((plane, idx));
            self.buf.fill(if plane == Plane::Red { 0xF0 } else { 0x0F });
            &self.buf
        }
    }

    #[test]
    fn show_planes_streams_twelve_strips_per_plane_between_the_table_and_the_update() {
        let mut f = Fake::with_busy(&[100, 4000]);
        let mut src = Flat {
            buf: [0; 4000],
            asked: Vec::new(),
        };
        let t = show_planes(&mut f, Rotation::Deg270, &mut src).unwrap();
        let asked: Vec<(Plane, u16)> = (0..12)
            .map(|i| (Plane::Red, i))
            .chain((0..12).map(|i| (Plane::Bw, i)))
            .collect();
        assert_eq!(src.asked, asked);
        let strips: Vec<&Ev> = f
            .log
            .iter()
            .filter(|e| matches!(e, Ev::Data(d) if d.len() == 4000))
            .collect();
        assert_eq!(strips.len(), 24);
        assert_eq!(strips[0], &Ev::Data(std::vec![0xF0; 4000]));
        assert_eq!(strips[12], &Ev::Data(std::vec![0x0F; 4000]));
        let cmds: Vec<u8> = f
            .log
            .iter()
            .filter_map(|e| if let Ev::Cmd(c) = e { Some(*c) } else { None })
            .collect();
        // table first, then RED before BW, then power on and the update
        let at = |c: u8| cmds.iter().position(|&x| x == c).unwrap();
        assert!(at(0x32) < at(0x26) && at(0x26) < at(0x24));
        assert_eq!(&cmds[cmds.len() - 5..], &[0x22, 0x20, 0x21, 0x22, 0x20]);
        assert!(t.update_ms >= 4000, "{t:?}");
    }

    #[test]
    fn probe_gives_up_on_a_stuck_busy_line() {
        let mut f = Fake::new(u64::MAX / 4);
        assert_eq!(run_probe(&mut f), Err(DisplayError::BusyTimeout));
    }
}
