// OnePage C61 sleep wallpaper: `SLEEP.BMP` from the SD root, decoded to a 1 bit
// image in PSRAM before the sleep refresh starts (the SD card must be idle
// while the panel refreshes). The format, dithering and bit order are in
// pulp_board_logic::wallpaper (host-tested); this file only reads the file and
// places the buffers.
//
// Any failure (no file, wrong format, read error, no PSRAM budget) gives
// `None` and one log line, and the caller draws the text sleep screen.
//
// The X4 has no PSRAM for a 48 KB image and keeps the text screen.
//
// With the `gray-wallpaper` feature the file is decoded to 4 gray levels instead
// (`load_gray`): two 48 KB plane images (96 KB, PSRAM) that the 4-gray waveform
// (pulp_board_logic::gray) streams to the panel's two RAMs.

use embassy_time::Instant;
use log::{info, warn};
#[cfg(feature = "gray-wallpaper")]
use pulp_board_logic::gray::{Plane, PlaneSource};
#[cfg(feature = "gray-wallpaper")]
use pulp_board_logic::ssd1677::Rotation;
use pulp_board_logic::wallpaper::{self, BmpSource, ReadError};

use super::bigbuf::{BigBuf, BufClass};
use crate::drivers::sdcard::SdStorage;
use crate::drivers::storage;
#[cfg(feature = "gray-wallpaper")]
use crate::drivers::strip::StripBuffer;

const FILE: &str = "SLEEP.BMP";

// the decoded image, in the first `OUT_BYTES` of a PSRAM block
#[cfg(not(feature = "gray-wallpaper"))]
pub struct Wallpaper {
    buf: BigBuf,
}

#[cfg(not(feature = "gray-wallpaper"))]
impl Wallpaper {
    // 1 = ink, 60 bytes per row, 480x800
    pub fn image(&self) -> &[u8] {
        &self.buf[..wallpaper::OUT_BYTES]
    }
}

struct SdBmp<'a>(&'a SdStorage);

impl BmpSource for SdBmp<'_> {
    fn read_at(&mut self, offset: u32, buf: &mut [u8]) -> Result<usize, ReadError> {
        storage::read_file_chunk(self.0, FILE, offset, buf).map_err(|_| ReadError)
    }
}

#[cfg(not(feature = "gray-wallpaper"))]
// Blocking (SD reads, ~1 s for a 24-bit file). The image is `DisplayFrame`
// class memory: PSRAM only, refused (not taken from the internal heap) when
// PSRAM is degraded. Dropping the `Wallpaper` frees it.
pub fn load(sd: &SdStorage) -> Option<Wallpaper> {
    // a missing file is the normal case: no allocation, no SD traffic beyond
    // the directory lookup
    if storage::file_size(sd, FILE).is_err() {
        info!("sleep: no {}, text screen", FILE);
        return None;
    }
    let mut buf = match BigBuf::zeroed(BufClass::DisplayFrame, wallpaper::WORK_BYTES) {
        Ok(b) => b,
        Err(_) => {
            warn!("sleep: {} skipped, no PSRAM budget, text screen", FILE);
            return None;
        }
    };
    let started = Instant::now();
    let (image, batch) = buf.split_at_mut(wallpaper::OUT_BYTES);
    if let Err(e) = wallpaper::decode(&mut SdBmp(sd), image, batch) {
        warn!("sleep: {} unusable ({}), text screen", FILE, e.as_str());
        return None;
    }
    info!(
        "sleep: {} decoded in {} ms",
        FILE,
        started.elapsed().as_millis()
    );
    Some(Wallpaper { buf })
}

// the decoded 4 gray image: the RED plane image, then the BW plane image, in the
// first `GRAY_OUT_BYTES` of a PSRAM block (the layout is wallpaper.rs's)
#[cfg(feature = "gray-wallpaper")]
pub struct GrayWallpaper {
    buf: BigBuf,
}

#[cfg(feature = "gray-wallpaper")]
impl GrayWallpaper {
    pub fn red(&self) -> &[u8] {
        &self.buf[..wallpaper::OUT_BYTES]
    }

    pub fn bw(&self) -> &[u8] {
        &self.buf[wallpaper::OUT_BYTES..wallpaper::GRAY_OUT_BYTES]
    }
}

// as `load`, to 4 gray levels
#[cfg(feature = "gray-wallpaper")]
pub fn load_gray(sd: &SdStorage) -> Option<GrayWallpaper> {
    if storage::file_size(sd, FILE).is_err() {
        info!("sleep: no {}, text screen", FILE);
        return None;
    }
    let mut buf = match BigBuf::zeroed(BufClass::DisplayFrame, wallpaper::GRAY_WORK_BYTES) {
        Ok(b) => b,
        Err(_) => {
            warn!("sleep: {} skipped, no PSRAM budget, text screen", FILE);
            return None;
        }
    };
    let started = Instant::now();
    let (planes, batch) = buf.split_at_mut(wallpaper::GRAY_OUT_BYTES);
    if let Err(e) = wallpaper::decode_gray(&mut SdBmp(sd), planes, batch) {
        warn!("sleep: {} unusable ({}), text screen", FILE, e.as_str());
        return None;
    }
    info!(
        "sleep: {} decoded to 4 gray in {} ms",
        FILE,
        started.elapsed().as_millis()
    );
    Some(GrayWallpaper { buf })
}

// strips of the two plane images, for `gray::show_planes`: the same blit the
// 1 bit wallpaper uses, once per plane
#[cfg(feature = "gray-wallpaper")]
pub struct GrayStrips<'a> {
    pub strip: &'a mut StripBuffer,
    pub image: &'a GrayWallpaper,
}

#[cfg(feature = "gray-wallpaper")]
impl PlaneSource for GrayStrips<'_> {
    fn render_strip(&mut self, plane: Plane, rotation: Rotation, idx: u16) -> &[u8] {
        self.strip.begin_strip(rotation, idx);
        let img = match plane {
            Plane::Red => self.image.red(),
            Plane::Bw => self.image.bw(),
        };
        wallpaper::draw_strip(self.strip, img);
        self.strip.data()
    }
}
