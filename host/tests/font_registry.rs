//! Host integration tests for C61 font registry, font source generations,
//! and flash-backed pack source selection.
mod cjk_support;

use pulp_board_logic::font_index::{
    FLASH_FONT_MAX_TOTAL_BYTES, FONT_SOURCE, FlashPackInfo, FlashSetError, IndexKey, Lease,
    Refusal, Retired, select_flash_pack, validate_flash_set,
};
use pulp_fontpack::{Header, PackError, PackReader};
use pulp_host::drivers::sdcard::SdStorage;
use pulp_host::fonts::{
    FontSet,
    cjk::{CjkState, FONT_REGISTRY, bank_identity},
};
use pulp_host::kernel::Kernel;
use pulp_host::storage::VirtualStorage;
use std::sync::Mutex;

static TEST_LOCK: Mutex<()> = Mutex::new(());

const PX: u16 = 16;
const HEAD_PX: u16 = 23;
const RECORDS: u32 = 150;
const INDEX_START: u32 = 44;
const INDEX_END: u32 = INDEX_START + 22 * RECORDS;

fn latin() -> FontSet {
    FontSet::for_size(0)
}

fn kernel(card: VirtualStorage) -> Kernel {
    Kernel::new(SdStorage::new(card))
}

fn reset_registry() {
    FONT_REGISTRY.with(|r| {
        let mut ret = Retired::new();
        r.clear(&mut ret);
    });
    FONT_SOURCE.bump();
}

/// Generate a valid font pack with `records` glyphs starting at codepoint 0x4e00.
fn generate_pack(px: u16, font_id: u64, records: u32) -> Vec<u8> {
    let bitmap_len = 3; // 8x3 bitmap
    let region = bitmap_len * records;
    let index_end = 44 + 22 * records;
    let total_len = index_end + region;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"PFNT");
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&px.to_le_bytes());
    bytes.extend_from_slice(&font_id.to_le_bytes());
    bytes.extend_from_slice(&(px + 4).to_le_bytes());
    bytes.extend_from_slice(&px.to_le_bytes());
    for n in [records, 44, 22 * records, index_end, region, total_len] {
        bytes.extend_from_slice(&n.to_le_bytes());
    }
    for i in 0..records {
        for n in [0x4e00 + i, bitmap_len * i, bitmap_len] {
            bytes.extend_from_slice(&n.to_le_bytes());
        }
        for n in [
            px + 1,         // advance
            0u16,           // offset_x
            (-3i16) as u16, // offset_y
            8u16,           // width
            3u16,           // height
        ] {
            bytes.extend_from_slice(&n.to_le_bytes());
        }
    }
    for i in 0..records {
        bytes.extend_from_slice(&[0x81, px as u8, (i & 0xff) as u8]);
    }
    bytes
}

fn install_pack(card: &VirtualStorage, px: u16, bytes: &[u8]) {
    card.ensure_pulp_dir().unwrap();
    card.ensure_pulp_subdir("FONTS").unwrap();
    card.write_in_pulp_subdir("FONTS", &format!("F{px:05}.PFN"), bytes)
        .unwrap();
}

fn text_for_records(count: u32) -> String {
    (0..count)
        .map(|i| char::from_u32(0x4e00 + i).unwrap())
        .collect()
}

// ---------------------------------------------------------------------------
// 1. Source selection tests
// ---------------------------------------------------------------------------

#[test]
fn source_selection_flash_pack_preferred_and_fallback_to_sd_or_empty() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_registry();

    let flash_bytes_16 = generate_pack(16, 0x1111_2222, 10);
    let flash_set: &[(u16, &'static [u8])] =
        &[(16, Box::leak(flash_bytes_16.clone().into_boxed_slice()))];

    // select_flash_pack picks 16 from flash table
    let selected_16 = select_flash_pack(flash_set, 16);
    assert_eq!(selected_16, Some(&flash_bytes_16[..]));

    // size not in flash falls back to None (which allows SD lookup)
    let selected_23 = select_flash_pack(flash_set, 23);
    assert_eq!(selected_23, None);

    // Fallback on SD: when card has pack, bank_identity sees installed = true
    let card = VirtualStorage::memory();
    let sd_pack = generate_pack(16, 0x3333_4444, 10);
    install_pack(&card, 16, &sd_pack);
    let mut k = kernel(card);
    let ident = bank_identity(&mut k.handle(), 16).unwrap();
    assert!(ident.installed);
    assert_eq!(ident.font_id, 0x3333_4444);

    // Fallback when absent: bank_identity returns uninstalled empty pack (font_id 0)
    let ident_absent = bank_identity(&mut k.handle(), 23).unwrap();
    assert!(!ident_absent.installed);
    assert_eq!(ident_absent.font_id, 0);
}

// ---------------------------------------------------------------------------
// 2. Malformed pack tests
// ---------------------------------------------------------------------------

#[test]
fn malformed_packs_rejected_during_build_validation() {
    // Empty set
    assert_eq!(validate_flash_set(&[]), Err(FlashSetError::Empty));

    // Unknown size (not in PACK_PIXEL_SIZES)
    let invalid_size = [FlashPackInfo {
        pixel_size: 15,
        len: 1000,
    }];
    assert_eq!(
        validate_flash_set(&invalid_size),
        Err(FlashSetError::UnknownSize { pixel_size: 15 })
    );

    // Duplicate size
    let duplicate = [
        FlashPackInfo {
            pixel_size: 16,
            len: 1000,
        },
        FlashPackInfo {
            pixel_size: 16,
            len: 1200,
        },
    ];
    assert_eq!(
        validate_flash_set(&duplicate),
        Err(FlashSetError::Duplicate { pixel_size: 16 })
    );

    // Pack exceeds max flash bytes
    let too_large = [FlashPackInfo {
        pixel_size: 16,
        len: FLASH_FONT_MAX_TOTAL_BYTES + 1,
    }];
    assert_eq!(
        validate_flash_set(&too_large),
        Err(FlashSetError::PackTooLarge {
            pixel_size: 16,
            len: FLASH_FONT_MAX_TOTAL_BYTES + 1,
        })
    );

    // Total exceeds max flash bytes
    let total_too_large = [
        FlashPackInfo {
            pixel_size: 16,
            len: FLASH_FONT_MAX_TOTAL_BYTES - 100,
        },
        FlashPackInfo {
            pixel_size: 23,
            len: 200,
        },
    ];
    assert_eq!(
        validate_flash_set(&total_too_large),
        Err(FlashSetError::TotalTooLarge {
            total: FLASH_FONT_MAX_TOTAL_BYTES + 100,
        })
    );
}

#[test]
fn malformed_packs_rejected_during_header_decode_and_reader_open() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // Too short
    let short_data = [0u8; 20];
    assert_eq!(Header::decode(&short_data, 20), Err(PackError::TooShort));
    assert!(PackReader::open(&short_data[..], 20).is_err());

    // Bad magic
    let mut bad_magic = generate_pack(16, 0x1234, 10);
    bad_magic[0..4].copy_from_slice(b"BAD!");
    assert_eq!(
        Header::decode(&bad_magic, bad_magic.len() as u64),
        Err(PackError::BadMagic)
    );
    assert!(PackReader::open(&bad_magic[..], bad_magic.len() as u64).is_err());

    // Unsupported version
    let mut bad_version = generate_pack(16, 0x1234, 10);
    bad_version[4..6].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        Header::decode(&bad_version, bad_version.len() as u64),
        Err(PackError::UnsupportedVersion { found: 2 })
    );
    assert!(PackReader::open(&bad_version[..], bad_version.len() as u64).is_err());

    // Length mismatch
    let good_pack = generate_pack(16, 0x1234, 10);
    assert_eq!(
        Header::decode(&good_pack, (good_pack.len() - 5) as u64),
        Err(PackError::LengthMismatch)
    );

    // Corrupted pack on SD card rejected by with_pack
    let card = VirtualStorage::memory();
    install_pack(&card, 16, &bad_magic);
    let mut k = kernel(card);
    assert!(bank_identity(&mut k.handle(), 16).is_err());
}

// ---------------------------------------------------------------------------
// 3. Suspend reuse & auxiliary surface tests
// ---------------------------------------------------------------------------

#[test]
fn suspend_reuse_and_auxiliary_surfaces_reuse_resident_index() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_registry();

    let card = VirtualStorage::memory();
    let pack_data = generate_pack(PX, 0x5555_6666, RECORDS);
    install_pack(&card, PX, &pack_data);
    let mut k = kernel(card);

    let text = text_for_records(RECORDS); // 150 glyphs >= RAM_INDEX_MIN_PENDING (128)

    let mut state = CjkState::new();
    state.set_ram_index(true);

    // First prepare: loads index into FONT_REGISTRY
    state
        .prepare_text(&mut k.handle(), text.as_bytes(), latin(), PX, HEAD_PX)
        .unwrap();

    let resident_count = FONT_REGISTRY.with(|r| r.resident());
    assert_eq!(resident_count, 1, "index must be resident in registry");

    let expected_key = IndexKey {
        font_id: 0x5555_6666,
        pixel_size: PX,
        glyph_count: RECORDS,
        bitmap_len: 3 * RECORDS,
        source: FONT_SOURCE.get(),
    };
    assert!(FONT_REGISTRY.with(|r| r.contains(&expected_key)));

    // Simulate suspend / app exit: clear CjkState
    state.clear();

    // Resident index survived clear()
    assert_eq!(FONT_REGISTRY.with(|r| r.resident()), 1);
    assert!(FONT_REGISTRY.with(|r| r.contains(&expected_key)));

    // Subsequent prepare_text reuses resident index with 0 SD index reads
    k.sd().card.reset_reads();
    state
        .prepare_text(&mut k.handle(), text.as_bytes(), latin(), PX, HEAD_PX)
        .unwrap();

    let log = k.sd().card.read_log();
    let index_probes = log
        .iter()
        .filter(|r| r.offset >= INDEX_START && r.offset < INDEX_END)
        .count();
    assert_eq!(
        index_probes, 0,
        "resident index must require 0 SD index reads"
    );

    // Auxiliary surfaces: CjkState::auxiliary() has ram_index == false,
    // but reuses the resident index already in FONT_REGISTRY!
    let mut aux = CjkState::auxiliary();
    assert!(!aux.ram_index());

    let short_text = text_for_records(10); // only 10 scalars, well below 128
    k.sd().card.reset_reads();
    aux.prepare_text(&mut k.handle(), short_text.as_bytes(), latin(), PX, HEAD_PX)
        .unwrap();

    let aux_log = k.sd().card.read_log();
    let aux_index_probes = aux_log
        .iter()
        .filter(|r| r.offset >= INDEX_START && r.offset < INDEX_END)
        .count();
    assert_eq!(
        aux_index_probes, 0,
        "auxiliary surface must reuse resident index with 0 SD index reads"
    );

    // Index is returned to registry after auxiliary borrow ends
    assert_eq!(FONT_REGISTRY.with(|r| r.resident()), 1);
    assert!(FONT_REGISTRY.with(|r| r.contains(&expected_key)));
}

// ---------------------------------------------------------------------------
// 4. Source change tests
// ---------------------------------------------------------------------------

#[test]
fn source_change_invalidates_resident_indices_and_subsequent_lease_misses() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_registry();

    let card = VirtualStorage::memory();
    let pack_data = generate_pack(PX, 0x7777_8888, RECORDS);
    install_pack(&card, PX, &pack_data);
    let mut k = kernel(card);

    let text = text_for_records(RECORDS);
    let mut state = CjkState::new();
    state.set_ram_index(true);

    state
        .prepare_text(&mut k.handle(), text.as_bytes(), latin(), PX, HEAD_PX)
        .unwrap();
    assert_eq!(FONT_REGISTRY.with(|r| r.resident()), 1);

    let old_key = IndexKey {
        font_id: 0x7777_8888,
        pixel_size: PX,
        glyph_count: RECORDS,
        bitmap_len: 3 * RECORDS,
        source: FONT_SOURCE.get(),
    };
    assert!(FONT_REGISTRY.with(|r| r.contains(&old_key)));

    // Card or font directory change bumps FONT_SOURCE
    FONT_SOURCE.bump();

    // Lease with new generation key returns Miss and evicts stale residents
    let new_key = IndexKey {
        source: FONT_SOURCE.get(),
        ..old_key
    };
    let miss_lease = FONT_REGISTRY.with(|r| {
        let mut ret = Retired::new();
        r.lease(new_key, &mut ret)
    });
    assert!(matches!(miss_lease, Lease::Miss));
    assert_eq!(
        FONT_REGISTRY.with(|r| r.resident()),
        0,
        "stale residents evicted"
    );

    // Lease with old key is now stale
    let stale_lease = FONT_REGISTRY.with(|r| {
        let mut ret = Retired::new();
        r.lease(old_key, &mut ret)
    });
    assert!(matches!(stale_lease, Lease::Stale));

    // Subsequent prepare_text reloads the index under new generation
    state.clear();
    state
        .prepare_text(&mut k.handle(), text.as_bytes(), latin(), PX, HEAD_PX)
        .unwrap();
    assert_eq!(FONT_REGISTRY.with(|r| r.resident()), 1);
    assert!(FONT_REGISTRY.with(|r| r.contains(&new_key)));
}

// ---------------------------------------------------------------------------
// 5. Failed preload tests
// ---------------------------------------------------------------------------

#[test]
fn failed_preload_marks_refusal_failed_and_cleared_on_source_change_or_forget() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_registry();

    let key = IndexKey {
        font_id: 0x9999_aaaa,
        pixel_size: PX,
        glyph_count: RECORDS,
        bitmap_len: 3 * RECORDS,
        source: FONT_SOURCE.get(),
    };

    // Simulate load failure in registry
    let token = FONT_REGISTRY.with(|r| {
        let mut ret = Retired::new();
        r.begin_load(key, 4000, &mut ret).unwrap()
    });
    FONT_REGISTRY.with(|r| r.fail(token));

    // Subsequent lease returns Lease::Failed
    let failed_lease = FONT_REGISTRY.with(|r| {
        let mut ret = Retired::new();
        r.lease(key, &mut ret)
    });
    assert!(matches!(failed_lease, Lease::Failed));

    // Subsequent begin_load is refused with Refusal::Failed (no repeat attempt)
    let refusal = FONT_REGISTRY.with(|r| {
        let mut ret = Retired::new();
        r.begin_load(key, 4000, &mut ret)
    });
    assert_eq!(refusal, Err(Refusal::Failed));

    // When forget_failures is called, failure memory is cleared
    FONT_REGISTRY.with(|r| r.forget_failures());
    let cleared_lease = FONT_REGISTRY.with(|r| {
        let mut ret = Retired::new();
        r.lease(key, &mut ret)
    });
    assert!(matches!(cleared_lease, Lease::Miss));

    // Fail it again to verify clearing on source change
    let token2 = FONT_REGISTRY.with(|r| {
        let mut ret = Retired::new();
        r.begin_load(key, 4000, &mut ret).unwrap()
    });
    FONT_REGISTRY.with(|r| r.fail(token2));
    assert!(matches!(
        FONT_REGISTRY.with(|r| r.lease(key, &mut Retired::new())),
        Lease::Failed
    ));

    // Source generation bump clears failure memory
    FONT_SOURCE.bump();
    let new_key = IndexKey {
        source: FONT_SOURCE.get(),
        ..key
    };
    let after_bump = FONT_REGISTRY.with(|r| {
        let mut ret = Retired::new();
        r.lease(new_key, &mut ret)
    });
    assert!(matches!(after_bump, Lease::Miss));
}
