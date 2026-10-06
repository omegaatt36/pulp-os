// PROV.TXT and COVERAGE.TXT: ASCII, LF, one `key=value` per line, fixed key
// order. Built as plain strings; everything is a function of the inputs.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

pub struct PackInfo {
    pub pixel_size: u16,
    pub font_id: u64,
    pub glyph_count: usize,
    pub bytes: Vec<u8>,
}

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn u_plus(c: char) -> String {
    format!("U+{:04X}", u32::from(c))
}

pub struct ProvInput<'a> {
    pub font_sha256: &'a [u8; 32],
    pub font_size: usize,
    pub upstream_url: &'a str,
    pub license: &'a [u8],
    pub packs: &'a [PackInfo],
}

pub fn provenance(p: &ProvInput) -> String {
    let mut s = String::new();
    let mut line = |k: &str, v: &dyn std::fmt::Display| {
        let _ = writeln!(s, "{k}={v}");
    };
    line("format", &"pulp-fontconv-provenance");
    line("format_version", &1);
    line("converter", &crate::CONVERTER_NAME);
    line("converter_version", &crate::CONVERTER_VERSION);
    line("convention_version", &crate::CONVENTION_VERSION);
    line("pack_format_version", &pulp_fontpack::FORMAT_VERSION);
    line("font_sha256", &hex(p.font_sha256));
    line("font_size", &p.font_size);
    line("font_upstream_url", &p.upstream_url);
    line("license_name", &crate::LICENSE_NAME);
    line("license_file", &crate::LICENSE_FILE);
    line("license_sha256", &hex(&sha256(p.license)));
    line("pack_count", &p.packs.len());
    for pk in p.packs {
        let px = pk.pixel_size;
        line(
            &format!("pack.{px}.file"),
            &pulp_fontpack::pack_file_name(px),
        );
        line(&format!("pack.{px}.pixel_size"), &px);
        line(
            &format!("pack.{px}.font_id"),
            &format!("{:016x}", pk.font_id),
        );
        line(&format!("pack.{px}.glyph_count"), &pk.glyph_count);
        line(&format!("pack.{px}.size"), &pk.bytes.len());
        line(&format!("pack.{px}.sha256"), &hex(&sha256(&pk.bytes)));
    }
    s
}

pub struct CoverageInput<'a> {
    pub font_sha256: &'a [u8; 32],
    pub cmap_total: usize,
    // excluded cmap chars, ascending
    pub excluded_control: &'a [char],
    pub included_total: usize,
    pub packs: &'a [PackInfo],
    // (required total, missing ascending), only when a requirement was given
    pub required: Option<(usize, &'a [char])>,
}

pub fn coverage(c: &CoverageInput) -> String {
    let mut s = String::new();
    let mut line = |k: &str, v: &dyn std::fmt::Display| {
        let _ = writeln!(s, "{k}={v}");
    };
    line("format", &"pulp-fontconv-coverage");
    line("format_version", &1);
    line("font_sha256", &hex(c.font_sha256));
    line("cmap_total", &c.cmap_total);
    line("excluded_total", &c.excluded_control.len());
    line("excluded.control", &c.excluded_control.len());
    line("included_total", &c.included_total);
    for &ch in c.excluded_control {
        line("excluded", &format!("{}:control", u_plus(ch)));
    }
    for pk in c.packs {
        line(
            &format!("pack.{}.glyph_count", pk.pixel_size),
            &pk.glyph_count,
        );
    }
    if let Some((total, missing)) = c.required {
        line("require_total", &total);
        line("require_missing", &missing.len());
        for &ch in missing {
            line("missing", &u_plus(ch));
        }
    }
    s
}
