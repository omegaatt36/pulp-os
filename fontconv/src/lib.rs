// pulp-fontconv: TTF/OTF -> SD font packs (pulp-fontpack format) plus
// provenance, licence copy and coverage report. Pure bytes in, bytes out; the
// binary owns all file system access. Output is a function of the inputs only.

mod raster;
mod report;

use std::collections::BTreeSet;
use std::fmt;

use fontdue::{Font, FontSettings};
use pulp_fontpack::{FontInfo, GlyphEntry, Pack, build_pack, pack_file_name};
use report::{CoverageInput, PackInfo, ProvInput};

pub const CONVERTER_NAME: &str = "pulp-fontconv";
pub const CONVERTER_VERSION: &str = env!("CARGO_PKG_VERSION");
// rasterisation convention version, mixed into every font_id
pub const CONVENTION_VERSION: u32 = 1;
pub const LICENSE_NAME: &str = "SIL OFL 1.1";
// union of firmware body 16/19/23/28/35 and heading 23/27/32/38/46
pub const DEFAULT_SIZES: [u16; 9] = [16, 19, 23, 27, 28, 32, 35, 38, 46];

pub const LICENSE_FILE: &str = "OFL.TXT";
pub const PROV_FILE: &str = "PROV.TXT";
pub const COVERAGE_FILE: &str = "COVERAGE.TXT";

pub struct Input<'a> {
    pub font: &'a [u8],
    pub sizes: &'a [u16],
    pub license: &'a [u8],
    pub upstream_url: &'a str,
    pub require_chars: Option<&'a str>,
}

pub struct OutFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

pub struct Output {
    // ascending by name
    pub files: Vec<OutFile>,
    // required chars that are not in the packs; ascending, distinct
    pub missing: Vec<char>,
}

impl Output {
    pub fn file(&self, name: &str) -> Option<&[u8]> {
        self.files
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.bytes.as_slice())
    }
}

#[derive(Debug)]
pub enum ConvError {
    Sizes(&'static str),
    UpstreamUrl(&'static str),
    EmptyLicense,
    LicenseName,
    Font(&'static str),
    NoMetrics,
    NoGlyphs,
    Pack(pulp_fontpack::BuildError),
    InvalidPack(pulp_fontpack::PackError),
}

impl fmt::Display for ConvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sizes(why) => write!(f, "invalid sizes: {why}"),
            Self::UpstreamUrl(why) => write!(f, "invalid upstream url: {why}"),
            Self::LicenseName => f.write_str("invalid license name: use non-empty printable ASCII without surrounding whitespace"),
            Self::EmptyLicense => f.write_str("licence text is empty"),
            Self::Font(why) => write!(f, "cannot parse font: {why}"),
            Self::NoMetrics => f.write_str("font has no horizontal line metrics"),
            Self::NoGlyphs => f.write_str("font has no usable character"),
            Self::Pack(e) => write!(f, "cannot build font pack: {e:?}"),
            Self::InvalidPack(e) => write!(f, "built font pack is invalid: {e}"),
        }
    }
}

impl std::error::Error for ConvError {}

// each size 1..=255, no duplicates, non-empty
pub fn validate_sizes(sizes: &[u16]) -> Result<(), ConvError> {
    if sizes.is_empty() {
        return Err(ConvError::Sizes("no size given"));
    }
    if sizes.iter().any(|&s| !(1..=255).contains(&s)) {
        return Err(ConvError::Sizes("each size must be 1..=255"));
    }
    let distinct: BTreeSet<u16> = sizes.iter().copied().collect();
    if distinct.len() != sizes.len() {
        return Err(ConvError::Sizes("duplicate size"));
    }
    Ok(())
}

// the url becomes one value of a key=value line in an ASCII file
pub fn validate_upstream_url(url: &str) -> Result<(), ConvError> {
    if url.is_empty() {
        return Err(ConvError::UpstreamUrl("empty"));
    }
    if url.chars().any(|c| !c.is_ascii() || c.is_ascii_control()) {
        return Err(ConvError::UpstreamUrl(
            "must be printable ASCII without control characters",
        ));
    }
    if url.trim() != url {
        return Err(ConvError::UpstreamUrl("surrounding whitespace"));
    }
    Ok(())
}

// first 8 bytes (little endian) of sha-256(font hash || pixel size le16 || convention le32)
pub fn derive_font_id(font_sha256: &[u8; 32], pixel_size: u16, convention_version: u32) -> u64 {
    let mut msg = Vec::with_capacity(38);
    msg.extend_from_slice(font_sha256);
    msg.extend_from_slice(&pixel_size.to_le_bytes());
    msg.extend_from_slice(&convention_version.to_le_bytes());
    let digest = report::sha256(&msg);
    u64::from_le_bytes([
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
    ])
}

/// Explicit metadata for fonts with a license other than the legacy OFL default.
pub struct ConversionOptions<'a> {
    pub license_name: &'a str,
}

pub fn validate_license_name(name: &str) -> Result<(), ConvError> {
    if name.is_empty()
        || name.trim() != name
        || name.chars().any(|c| !c.is_ascii() || c.is_ascii_control())
    {
        return Err(ConvError::LicenseName);
    }
    Ok(())
}

/// Legacy OFL conversion, retained for existing callers.
pub fn convert(input: &Input) -> Result<Output, ConvError> {
    convert_with_options(
        input,
        &ConversionOptions {
            license_name: LICENSE_NAME,
        },
    )
}

pub fn convert_with_options(
    input: &Input,
    options: &ConversionOptions,
) -> Result<Output, ConvError> {
    validate_license_name(options.license_name)?;
    let license_file = if options.license_name == LICENSE_NAME {
        LICENSE_FILE
    } else {
        "LICENSE.TXT"
    };
    validate_sizes(input.sizes)?;
    validate_upstream_url(input.upstream_url)?;
    if input.license.is_empty() {
        return Err(ConvError::EmptyLicense);
    }
    let font = Font::from_bytes(input.font, FontSettings::default()).map_err(ConvError::Font)?;

    // included set = cmap minus control characters, ascending
    let mut cmap: Vec<char> = font.chars().keys().copied().collect();
    cmap.sort_unstable();
    let (excluded, included): (Vec<char>, Vec<char>) = cmap.iter().partition(|c| c.is_control());
    if included.is_empty() {
        return Err(ConvError::NoGlyphs);
    }

    let mut sizes = input.sizes.to_vec();
    sizes.sort_unstable();
    let font_sha256 = report::sha256(input.font);

    let mut packs = Vec::with_capacity(sizes.len());
    for &px in &sizes {
        let (line_height, ascent) =
            raster::line_metrics(&font, f32::from(px)).ok_or(ConvError::NoMetrics)?;
        let info = FontInfo {
            pixel_size: px,
            font_id: derive_font_id(&font_sha256, px, CONVENTION_VERSION),
            line_height,
            ascent,
        };
        let glyphs: Vec<GlyphEntry> = included
            .iter()
            .map(|&c| raster::glyph(&font, c, f32::from(px)))
            .collect();
        let bytes = build_pack(&info, &glyphs).map_err(ConvError::Pack)?;
        Pack::parse(&bytes).map_err(ConvError::InvalidPack)?;
        packs.push(PackInfo {
            pixel_size: px,
            font_id: info.font_id,
            glyph_count: included.len(),
            bytes,
        });
    }

    // required set: distinct non-whitespace scalars of the requirement text
    let required: Option<BTreeSet<char>> = input
        .require_chars
        .map(|t| t.chars().filter(|c| !c.is_whitespace()).collect());
    let missing: Vec<char> = required
        .iter()
        .flatten()
        .copied()
        .filter(|c| included.binary_search(c).is_err())
        .collect();

    let prov = report::provenance(&ProvInput {
        font_sha256: &font_sha256,
        font_size: input.font.len(),
        upstream_url: input.upstream_url,
        license: input.license,
        license_name: options.license_name,
        license_file,
        packs: &packs,
    });
    let coverage = report::coverage(&CoverageInput {
        font_sha256: &font_sha256,
        cmap_total: cmap.len(),
        excluded_control: &excluded,
        included_total: included.len(),
        packs: &packs,
        required: required.as_ref().map(|r| (r.len(), missing.as_slice())),
    });

    let mut files: Vec<OutFile> = packs
        .into_iter()
        .map(|p| OutFile {
            name: pack_file_name(p.pixel_size).to_string(),
            bytes: p.bytes,
        })
        .collect();
    files.push(OutFile {
        name: PROV_FILE.into(),
        bytes: prov.into_bytes(),
    });
    files.push(OutFile {
        name: license_file.into(),
        bytes: input.license.to_vec(),
    });
    files.push(OutFile {
        name: COVERAGE_FILE.into(),
        bytes: coverage.into_bytes(),
    });
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Output { files, missing })
}
