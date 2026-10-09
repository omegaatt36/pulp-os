//! Exercise the firmware build's file validation before packs are embedded.
#[allow(dead_code)]
#[path = "../../build.rs"]
mod firmware;

use pulp_fontpack::{Header, PackError};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "pulp-flash-font-build-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        Self(dir)
    }

    fn write_pack(&self, bytes: &[u8]) -> PathBuf {
        let path = self.0.join("F00016.PFN");
        fs::write(&path, bytes).unwrap();
        path
    }

    fn output(&self) -> PathBuf {
        self.0.join("flash_fonts.rs")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

// Two 8x1 glyphs with a structurally valid header. Literal byte offsets keep
// corruption fixtures independent of the parser and do not duplicate its rules.
fn valid_pack() -> Vec<u8> {
    let mut bytes = b"PFNT".to_vec();
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(&42u64.to_le_bytes());
    bytes.extend_from_slice(&20u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    for n in [2u32, 44, 44, 88, 2, 90] {
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    for (codepoint, offset) in [(0x4e00u32, 0u32), (0x4e01, 1)] {
        for n in [codepoint, offset, 1] {
            bytes.extend_from_slice(&n.to_le_bytes());
        }
        for n in [16u16, 0, 0, 8, 1] {
            bytes.extend_from_slice(&n.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&[0x81, 0xff]);
    bytes
}

fn rejection(fixture: &Fixture, paths: &[PathBuf]) -> String {
    let panic = std::panic::catch_unwind(|| {
        firmware::generate_flash_fonts_in(&fixture.output(), paths);
    })
    .expect_err("invalid configured pack was embedded");
    assert!(!fixture.output().exists(), "rejected set emitted font data");
    if let Some(message) = panic.downcast_ref::<String>() {
        message.clone()
    } else {
        panic.downcast_ref::<&str>().unwrap().to_string()
    }
}

#[test]
fn valid_configured_pack_is_embedded() {
    let fixture = Fixture::new();
    let path = fixture.write_pack(&valid_pack());
    firmware::generate_flash_fonts_in(&fixture.output(), &[path.clone()]);
    let output = fs::read_to_string(fixture.output()).unwrap();
    assert!(output.contains("include_bytes!"));
    assert!(output.contains(path.canonicalize().unwrap().to_str().unwrap()));
    assert!(output.contains("(16, PACK_16)"));
}

#[test]
fn malformed_records_are_rejected_before_embedding() {
    // Damage the final record so validation must scan the entire index. Every
    // header still decodes: the old header-only build validation accepted them.
    for (offset, value, expected) in [
        (66, 0x110000u32, PackError::InvalidCodepoint { index: 1 }),
        (66, 0x4e00, PackError::CodepointOrder { index: 1 }),
        (70, 2, PackError::GlyphBitmapRange { index: 1 }),
        (74, 0, PackError::GlyphSizeMismatch { index: 1 }),
    ] {
        let fixture = Fixture::new();
        let mut bytes = valid_pack();
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(Header::decode(&bytes, bytes.len() as u64).is_ok());
        let path = fixture.write_pack(&bytes);
        let message = rejection(&fixture, &[path.clone()]);
        assert!(message.contains(path.to_str().unwrap()), "{message}");
        assert!(message.contains(&format!("{expected:?}")), "{message}");
    }
}

#[test]
fn missing_duplicate_and_oversized_configured_packs_still_fail() {
    let fixture = Fixture::new();
    let path = fixture.0.join("missing.PFN");
    assert!(rejection(&fixture, &[path]).contains("failed to read flash font pack file"));

    let path = fixture.write_pack(&valid_pack());
    assert!(rejection(&fixture, &[path.clone(), path]).contains("more than one 16 px pack"));

    // A valid pack may have unreferenced bitmap bytes. Expand that region and
    // its header together so the existing size limit, not malformed input,
    // remains the reason for rejection.
    let mut bytes = valid_pack();
    let total = pulp_board_logic::font_index::FLASH_FONT_MAX_TOTAL_BYTES + 1;
    bytes.resize(total, 0);
    bytes[36..40].copy_from_slice(&((total - 88) as u32).to_le_bytes());
    bytes[40..44].copy_from_slice(&(total as u32).to_le_bytes());
    let path = fixture.write_pack(&bytes);
    assert!(rejection(&fixture, &[path]).contains("byte limit"));
}
