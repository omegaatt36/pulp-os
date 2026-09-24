//! TTF to pack conversion (§9), rasterised exactly like the firmware's
//! `build.rs::rasterize_char`: fontdue at `pixel_size`, coverage >= 100 is ink,
//! rows packed MSB-first. Unlike `build.rs`, a metric that does not fit its
//! record type is an error, never a clamp (§4).
//!
//! Every pack is self-verified with the device loader (`pulp_render`), so a
//! pack that the device would reject, or that would draw a glyph other than
//! the one rasterised, fails generation on the host.

use pulp_render::font_pack::{FontPack, LoadError, LookupError, SliceReader};
use ttf_parser::PlatformId;

use crate::format::{Glyph, PackInput, WriteError, write_pack};

/// Same value as `build.rs` `THRESHOLD`.
pub const THRESHOLD: u8 = 100;

#[derive(Debug)]
pub enum ConvertError {
    Font(&'static str),
    NoUnicodeCmap,
    NoLineMetrics,
    MetricOverflow {
        code_point: Option<u32>,
        field: &'static str,
        value: i64,
    },
    Write(WriteError),
    /// The written pack failed self-verification: a converter bug.
    Verify(VerifyError),
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Font(e) => write!(f, "cannot parse font: {e}"),
            Self::NoUnicodeCmap => write!(f, "font has no Unicode cmap subtable"),
            Self::NoLineMetrics => write!(f, "font has no horizontal line metrics"),
            Self::MetricOverflow {
                code_point: Some(cp),
                field,
                value,
            } => write!(
                f,
                "U+{cp:04X}: {field} = {value} does not fit its record type"
            ),
            Self::MetricOverflow {
                code_point: None,
                field,
                value,
            } => write!(f, "{field} = {value} does not fit u16"),
            Self::Write(e) => write!(f, "{e}"),
            Self::Verify(e) => write!(f, "written pack fails self-verification: {e}"),
        }
    }
}

impl std::error::Error for ConvertError {}

impl From<WriteError> for ConvertError {
    fn from(e: WriteError) -> Self {
        Self::Write(e)
    }
}

/// Code points of the best Unicode cmap subtable, sorted ascending.
#[derive(Debug)]
pub struct CmapScan {
    /// `(code_point, glyph_id)` for every valid scalar mapped to a non-zero glyph.
    pub mapped: Vec<(u32, u16)>,
    /// Mapped entries that are not Unicode scalar values (skipped).
    pub invalid: Vec<u32>,
}

/// Preference among Unicode subtables, full repertoire first (the order used
/// by fontTools `getBestCmap`). Format 14 (variation sequences) is excluded.
const CMAP_PREFERENCE: [(PlatformId, u16); 8] = [
    (PlatformId::Windows, 10),
    (PlatformId::Unicode, 6),
    (PlatformId::Unicode, 4),
    (PlatformId::Windows, 1),
    (PlatformId::Unicode, 3),
    (PlatformId::Unicode, 2),
    (PlatformId::Unicode, 1),
    (PlatformId::Unicode, 0),
];

/// Read the best Unicode cmap. fontdue's own `chars()` merges every subtable,
/// including non-Unicode ones, so it is not used for the code point list.
pub fn scan_cmap(ttf: &[u8]) -> Result<CmapScan, ConvertError> {
    let face = ttf_parser::Face::parse(ttf, 0).map_err(|_| ConvertError::Font("invalid TTF"))?;
    let cmap = face.tables().cmap.ok_or(ConvertError::NoUnicodeCmap)?;
    let subtable = CMAP_PREFERENCE
        .iter()
        .find_map(|&(platform, encoding)| {
            cmap.subtables
                .into_iter()
                .find(|s| s.platform_id == platform && s.encoding_id == encoding)
        })
        .ok_or(ConvertError::NoUnicodeCmap)?;

    let mut mapped = Vec::new();
    let mut invalid = Vec::new();
    subtable.codepoints(|cp| match subtable.glyph_index(cp) {
        Some(gid) if gid.0 != 0 => {
            if char::from_u32(cp).is_some() {
                mapped.push((cp, gid.0));
            } else {
                invalid.push(cp);
            }
        }
        _ => {}
    });
    mapped.sort_unstable();
    mapped.dedup_by_key(|m| m.0);
    invalid.sort_unstable();
    invalid.dedup();
    Ok(CmapScan { mapped, invalid })
}

fn fit<T: TryFrom<i64>>(
    code_point: Option<u32>,
    field: &'static str,
    value: i64,
) -> Result<T, ConvertError> {
    T::try_from(value).map_err(|_| ConvertError::MetricOverflow {
        code_point,
        field,
        value,
    })
}

/// Convert one fontdue raster into a record + bitmap (`build.rs::rasterize_char`).
pub fn glyph_from_raster(
    code_point: u32,
    m: &fontdue::Metrics,
    coverage: &[u8],
) -> Result<Glyph, ConvertError> {
    let cp = Some(code_point);
    let width: u8 = fit(cp, "width", m.width as i64)?;
    let height: u8 = fit(cp, "height", m.height as i64)?;
    // build.rs rounds half up with `(advance_width + 0.5) as u8`
    let advance: u8 = fit(cp, "advance", (m.advance_width + 0.5).floor() as i64)?;
    let offset_x: i8 = fit(cp, "offset_x", m.xmin as i64)?;
    // baseline to top row, y down: -ymin - height
    let offset_y: i8 = fit(cp, "offset_y", -(m.ymin as i64) - m.height as i64)?;

    let (w, h) = (m.width, m.height);
    let row_bytes = w.div_ceil(8);
    let mut bitmap = vec![0u8; row_bytes * h];
    for y in 0..h {
        for x in 0..w {
            if coverage[y * w + x] >= THRESHOLD {
                bitmap[y * row_bytes + x / 8] |= 0x80 >> (x % 8);
            }
        }
    }
    Ok(Glyph {
        code_point,
        width,
        height,
        advance,
        offset_x,
        offset_y,
        bitmap,
    })
}

/// `(line_height, ascent)` = `(ceil(new_line_size), ceil(ascent))` as in `build.rs`.
pub fn line_metrics(lm: &fontdue::LineMetrics) -> Result<(u16, u16), ConvertError> {
    let line_height = fit(None, "line_height", lm.new_line_size.ceil() as i64)?;
    let ascent = fit(None, "ascent", lm.ascent.ceil() as i64)?;
    Ok((line_height, ascent))
}

#[derive(Debug)]
pub struct Conversion {
    pub pack: Vec<u8>,
    /// Packed code points, ascending.
    pub code_points: Vec<u32>,
    /// Entries seen in the best cmap (packed + invalid).
    pub cmap_count: usize,
    pub invalid: Vec<u32>,
    pub line_height: u16,
    pub ascent: u16,
    pub fallback_cp: u32,
    /// Bitmap section length in bytes.
    pub bitmap_len: usize,
    /// Largest glyph bitmap, as the device loader reports it.
    pub max_glyph_len: usize,
}

/// Rasterise every best-cmap code point at `pixel_size`, write the pack, and
/// self-verify it (`verify`) before returning it.
pub fn convert(
    ttf: &[u8],
    pixel_size: u16,
    fallback_cp: u32,
    license: &[u8],
) -> Result<Conversion, ConvertError> {
    let scan = scan_cmap(ttf)?;
    let font = fontdue::Font::from_bytes(ttf, fontdue::FontSettings::default())
        .map_err(ConvertError::Font)?;
    let px = pixel_size as f32;
    let lm = font
        .horizontal_line_metrics(px)
        .ok_or(ConvertError::NoLineMetrics)?;
    let (line_height, ascent) = line_metrics(&lm)?;

    let glyphs = scan
        .mapped
        .iter()
        .map(|&(cp, gid)| {
            let (m, coverage) = font.rasterize_indexed(gid, px);
            glyph_from_raster(cp, &m, &coverage)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let input = PackInput {
        pixel_size,
        line_height,
        ascent,
        fallback_cp,
        glyphs: &glyphs,
        license,
    };
    let pack = write_pack(&input)?;
    let loaded = verify(&pack, &input).map_err(ConvertError::Verify)?;

    Ok(Conversion {
        pack,
        code_points: scan.mapped.iter().map(|m| m.0).collect(),
        cmap_count: scan.mapped.len() + scan.invalid.len(),
        invalid: scan.invalid,
        line_height,
        ascent,
        fallback_cp,
        bitmap_len: glyphs.iter().map(|g| g.bitmap.len()).sum(),
        max_glyph_len: loaded.max_glyph_len(),
    })
}

/// How a pack disagrees with what was written into it (§9 self-verify).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyError {
    /// The device loader rejects the file (§6).
    Load(LoadError),
    /// A header value differs from the conversion's.
    Header {
        field: &'static str,
        expected: u32,
        actual: u32,
    },
    /// A glyph lookup failed after a successful load.
    Lookup { code_point: u32, error: LookupError },
    /// A packed code point resolves to the fallback.
    Absent(u32),
    /// A packed code point resolves to a record with other metrics.
    Metrics(u32),
    /// A packed code point's bitmap bytes differ from its raster.
    Bitmap(u32),
    /// The license section is not the license bytes.
    License,
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Load(e) => write!(f, "the device loader rejects it: {e:?}"),
            Self::Header {
                field,
                expected,
                actual,
            } => write!(f, "header {field} is {actual}, expected {expected}"),
            Self::Lookup { code_point, error } => {
                write!(f, "U+{code_point:04X}: lookup failed: {error:?}")
            }
            Self::Absent(cp) => write!(f, "U+{cp:04X} resolves to the fallback"),
            Self::Metrics(cp) => write!(f, "U+{cp:04X} metrics differ from its raster"),
            Self::Bitmap(cp) => write!(f, "U+{cp:04X} bitmap differs from its raster"),
            Self::License => write!(f, "license section differs from the license"),
        }
    }
}

impl std::error::Error for VerifyError {}

/// Load `pack` with the device loader at `input.pixel_size`, then check that
/// its header matches `input` and that every glyph in `input` resolves to
/// itself with the same metrics and bitmap bytes. This catches what the §6
/// checks cannot see (bitmaps carry no checksum), so the device never gets
/// a pack that loads but draws the wrong pixels.
pub fn verify(pack: &[u8], input: &PackInput<'_>) -> Result<FontPack, VerifyError> {
    let mut reader = SliceReader(pack);
    let loaded = FontPack::load(&mut reader, input.pixel_size).map_err(VerifyError::Load)?;

    let header = |field, expected: u32, actual: u32| {
        if expected == actual {
            Ok(())
        } else {
            Err(VerifyError::Header {
                field,
                expected,
                actual,
            })
        }
    };
    header(
        "line_height",
        input.line_height.into(),
        loaded.line_height().into(),
    )?;
    header("ascent", input.ascent.into(), loaded.ascent().into())?;
    header("fallback_cp", input.fallback_cp, loaded.fallback_cp())?;
    header(
        "glyph_count",
        input.glyphs.len() as u32,
        loaded.glyph_count(),
    )?;

    let mut buf = vec![0u8; loaded.max_glyph_len()];
    for g in input.glyphs {
        let cp = g.code_point;
        let ch = char::from_u32(cp).ok_or(VerifyError::Absent(cp))?;
        let (res, bitmap) =
            loaded
                .lookup(&mut reader, ch, &mut buf)
                .map_err(|error| VerifyError::Lookup {
                    code_point: cp,
                    error,
                })?;
        let r = res.glyph;
        if res.fallback || r.code_point != cp {
            return Err(VerifyError::Absent(cp));
        }
        if (r.width, r.height, r.advance, r.offset_x, r.offset_y)
            != (g.width, g.height, g.advance, g.offset_x, g.offset_y)
        {
            return Err(VerifyError::Metrics(cp));
        }
        if bitmap != g.bitmap.as_slice() {
            return Err(VerifyError::Bitmap(cp));
        }
    }

    // §2: the license is the last section. Matching only the tail would
    // accept an extra prefix hidden inside a larger license section.
    let license_len = u32::from_le_bytes(pack[48..52].try_into().expect("loaded header"));
    if license_len as usize != input.license.len() || !pack.ends_with(input.license) {
        return Err(VerifyError::License);
    }
    Ok(loaded)
}
