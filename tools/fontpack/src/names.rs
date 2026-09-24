//! 8.3 file names (§7). embedded-sdmmc opens short names only, so every name
//! the converter emits must already be a valid 8.3 name.

use crate::format::PIXEL_SIZES;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameError {
    Family(String),
    PixelSize(u16),
}

impl std::fmt::Display for NameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Family(fam) => write!(
                f,
                "family {fam:?} must be 1-6 uppercase ASCII letters or digits"
            ),
            Self::PixelSize(px) => write!(f, "pixel size {px} is outside 8..=96"),
        }
    }
}

impl std::error::Error for NameError {}

fn check_family(family: &str) -> Result<(), NameError> {
    let ok = (1..=6).contains(&family.len())
        && family
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
    if ok {
        Ok(())
    } else {
        Err(NameError::Family(family.to_owned()))
    }
}

fn stem(family: &str, pixel_size: u16) -> Result<String, NameError> {
    check_family(family)?;
    if !PIXEL_SIZES.contains(&pixel_size) {
        return Err(NameError::PixelSize(pixel_size));
    }
    Ok(format!("{family}{pixel_size:02}"))
}

/// `<FAMILY><SS>.PFP`, e.g. `IANSUI24.PFP`.
pub fn pack_file_name(family: &str, pixel_size: u16) -> Result<String, NameError> {
    Ok(format!("{}.PFP", stem(family, pixel_size)?))
}

/// `<FAM5>OFL.TXT`, e.g. `IANSUOFL.TXT`.
pub fn license_file_name(family: &str) -> Result<String, NameError> {
    check_family(family)?;
    Ok(format!("{}OFL.TXT", &family[..family.len().min(5)]))
}

/// `<FAMILY><SS>.TXT`: host-side build manifest (source SHA-256, coverage).
/// Not part of the pack format; the device never opens it.
pub fn manifest_file_name(family: &str, pixel_size: u16) -> Result<String, NameError> {
    Ok(format!("{}.TXT", stem(family, pixel_size)?))
}
