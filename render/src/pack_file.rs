// pack file names (§7) and the discovery scan that finds one on the SD card
//
// embedded-sdmmc opens short names only, so every name here is a valid 8.3
// name: `<FAMILY><SS>.PFP`, e.g. `IANSUI24.PFP`. The converter emits exactly
// this spelling, so parsing it back is how the device recognises a pack
// without opening it.
//
// a pack carries one pixel size of one face, so the name is the whole index:
// there is no per-glyph lookup by name, only "is there a pack for the size I
// am about to lay text out at". That is what `find` answers.

/// Pixel sizes a pack may be rasterised at. Mirrors the converter's
/// `PIXEL_SIZES`; a file claiming anything else is not a pack we wrote.
pub const PIXEL_SIZES: core::ops::RangeInclusive<u16> = 8..=96;

/// Extension every pack file ends in, uppercase: SD cards are FAT and the
/// converter only ever writes uppercase, so this match is exact.
pub const PACK_EXT: &[u8; 4] = b".PFP";

/// Longest family the converter accepts, so `<FAM><SS>` always fits 8.3.
pub const MAX_FAMILY: usize = 6;

/// A parsed pack file name: which family, at which pixel size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackName {
    /// 1..=6 uppercase ASCII letters or digits, exactly as the file spells it.
    pub family: [u8; MAX_FAMILY],
    /// Number of valid bytes in `family`; the rest are padding.
    pub family_len: u8,
    pub pixel_size: u16,
}

impl PackName {
    pub fn family(&self) -> &[u8] {
        &self.family[..self.family_len as usize]
    }

    /// Re-render the canonical spelling. This is what the device compares a
    /// scanned name against, so it never has to rebuild a string to search.
    pub fn file_name(&self) -> ([u8; 12], usize) {
        let mut out = [0u8; 12];
        let fam = self.family();
        out[..fam.len()].copy_from_slice(fam);
        let ss = self.pixel_size;
        out[fam.len()] = b'0' + (ss / 10) as u8;
        out[fam.len() + 1] = b'0' + (ss % 10) as u8;
        out[fam.len() + 2..fam.len() + 6].copy_from_slice(PACK_EXT);
        (out, fam.len() + 6)
    }
}

fn is_family_byte(b: u8) -> bool {
    b.is_ascii_uppercase() || b.is_ascii_digit()
}

/// Parse an 8.3 file name into a pack name, or `None` if it is not one.
///
/// Accepts only the exact spelling the converter writes: uppercase family, two
/// zero-padded digits, uppercase `.PFP`. Lowercase or unpadded input is
/// rejected rather than normalised, because FAT long-name entries can put
/// either on the card and guessing would make the scan depend on a
/// normalisation the user never performed.
pub fn parse_pack_name(name: &[u8]) -> Option<PackName> {
    // 1..=6 family + 2 digits + 4 ext = 7..=12 bytes
    if name.len() < 7 || name.len() > 12 {
        return None;
    }
    let (stem, _ext) = name.split_at(name.len() - 4);
    if _ext != PACK_EXT {
        return None;
    }
    // the stem is <FAMILY><SS>: the size is always the last two bytes, so it
    // splits off the same way whether the family is 1 or 6 characters
    let (fam, digits) = stem.split_at(stem.len() - 2);
    if !fam.iter().all(|&b| is_family_byte(b)) {
        return None;
    }
    if !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    // exactly two digits, so 8..=96 is 08..=96 and nothing is truncated
    let pixel_size = u16::from(digits[0] - b'0') * 10 + u16::from(digits[1] - b'0');
    if !PIXEL_SIZES.contains(&pixel_size) {
        return None;
    }
    let mut family = [0u8; MAX_FAMILY];
    family[..fam.len()].copy_from_slice(fam);
    Some(PackName {
        family,
        family_len: fam.len() as u8,
        pixel_size,
    })
}

/// Search a scanned SD root listing for the pack for one family and pixel
/// size. `names` are 8.3 entries as bytes; the first match wins, so the
/// listing order decides ties (there is exactly one file per family+size).
///
/// Returns the canonical file name to open, so the caller never reassembles
/// a string from a parsed struct.
pub fn find(names: &[&[u8]], family: &[u8], pixel_size: u16) -> Option<([u8; 12], usize)> {
    names
        .iter()
        .filter_map(|n| parse_pack_name(n))
        .find(|p| p.family() == family && p.pixel_size == pixel_size)
        .map(|p| p.file_name())
}
