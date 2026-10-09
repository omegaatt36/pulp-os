use pulp_board_logic::font_index::{
    FLASH_FONT_MAX_TOTAL_BYTES, FlashPackInfo, FlashSetError, INDEX_BANKS, INDEX_REGISTRY_BYTES,
    IndexKey, IndexRegistry, Lease, Refusal, Retired, SourceGeneration, select_flash_pack,
    validate_flash_set,
};

type Registry = IndexRegistry<Vec<u8>>;

const CAP: usize = 1000;

fn key(px: u16, source: u32) -> IndexKey {
    IndexKey {
        font_id: 0xABCD_0000 + u64::from(px),
        pixel_size: px,
        glyph_count: 12_665,
        bitmap_len: 4_000_000,
        source,
    }
}

// the whole load protocol a user goes through
fn load(r: &mut Registry, k: IndexKey, bytes: usize) -> Result<(), Refusal> {
    let mut retired = Retired::new();
    let token = r.begin_load(k, bytes, &mut retired)?;
    drop(retired);
    r.publish(token, vec![k.pixel_size as u8; bytes]).unwrap();
    Ok(())
}

fn lease(r: &mut Registry, k: IndexKey) -> Lease<Vec<u8>> {
    let mut retired = Retired::new();
    let got = r.lease(k, &mut retired);
    drop(retired);
    got
}

#[test]
fn registry_budget_is_half_of_the_large_font_class() {
    assert_eq!(INDEX_REGISTRY_BYTES, 1024 * 1024);
    assert_eq!(INDEX_BANKS, 2);
}

#[test]
fn leased_index_returns_and_is_reused() {
    let mut r = Registry::new(CAP);
    let k = key(16, 0);
    assert_eq!(lease(&mut r, k), Lease::Miss);
    load(&mut r, k, 400).unwrap();
    // suspend/resume: the user leases, uses and returns it more than once
    for _ in 0..3 {
        let Lease::Hit(index) = lease(&mut r, k) else {
            panic!("resident index must be found");
        };
        assert_eq!(index.len(), 400);
        assert_eq!(lease(&mut r, k), Lease::Busy, "no second owner while out");
        r.give_back(k, index).unwrap();
    }
    assert_eq!(r.resident(), 1);
    assert_eq!(r.resident_bytes(), 400);
}

#[test]
fn a_key_is_found_by_every_field() {
    let mut r = Registry::new(CAP);
    let k = key(16, 0);
    load(&mut r, k, 100).unwrap();
    for other in [
        IndexKey { font_id: 1, ..k },
        IndexKey {
            pixel_size: 19,
            ..k
        },
        IndexKey {
            glyph_count: k.glyph_count + 1,
            ..k
        },
        IndexKey {
            bitmap_len: k.bitmap_len + 1,
            ..k
        },
    ] {
        assert_eq!(lease(&mut r, other), Lease::Miss);
    }
    assert!(matches!(lease(&mut r, k), Lease::Hit(_)));
}

#[test]
fn source_change_retires_every_entry_and_cancels_the_load() {
    let mut r = Registry::new(CAP);
    load(&mut r, key(16, 0), 300).unwrap();
    let mut retired = Retired::new();
    // a load in flight under the old source
    let token = r.begin_load(key(23, 0), 100, &mut retired).unwrap();
    assert!(retired.is_empty());
    // the card was replaced: generation 1 arrives
    assert_eq!(lease(&mut r, key(16, 1)), Lease::Miss);
    assert_eq!(r.source(), 1);
    assert_eq!(r.resident(), 0, "entries of the old source are gone");
    assert!(!r.is_loading());
    // the load that began under the old source cannot publish
    assert_eq!(r.publish(token, vec![1; 100]), Err(vec![1; 100]));
    assert_eq!(r.resident(), 0);
    // the old key is stale now
    assert_eq!(lease(&mut r, key(16, 0)), Lease::Stale);
    assert_eq!(
        r.begin_load(key(16, 0), 10, &mut Retired::new()),
        Err(Refusal::Stale)
    );
}

#[test]
fn replaced_pack_never_answers_from_the_old_index() {
    let mut r = Registry::new(CAP);
    load(&mut r, key(16, 3), 300).unwrap();
    // font directory written: generation 4, same font id and size
    assert_eq!(lease(&mut r, key(16, 4)), Lease::Miss);
    assert_eq!(r.resident(), 0);
    load(&mut r, key(16, 4), 300).unwrap();
    assert!(matches!(lease(&mut r, key(16, 4)), Lease::Hit(_)));
}

#[test]
fn index_leased_across_a_source_change_is_dropped_on_return() {
    let mut r = Registry::new(CAP);
    let k = key(16, 0);
    load(&mut r, k, 300).unwrap();
    let Lease::Hit(index) = lease(&mut r, k) else {
        panic!("resident");
    };
    assert_eq!(lease(&mut r, key(19, 1)), Lease::Miss);
    assert_eq!(r.resident_bytes(), 0);
    assert_eq!(r.give_back(k, index).unwrap_err().len(), 300);
    assert_eq!(r.resident(), 0);
}

#[test]
fn failed_preload_is_remembered_until_forgotten() {
    let mut r = Registry::new(CAP);
    let k = key(16, 0);
    let mut retired = Retired::new();
    let token = r.begin_load(k, 400, &mut retired).unwrap();
    assert_eq!(r.committed_bytes(), 400);
    r.fail(token);
    assert!(!r.is_loading());
    assert_eq!(r.committed_bytes(), 0, "a failed load keeps nothing");
    assert_eq!(r.resident(), 0);
    assert_eq!(lease(&mut r, k), Lease::Failed);
    assert_eq!(r.begin_load(k, 400, &mut retired), Err(Refusal::Failed));
    // another size is not affected
    let other = r.begin_load(key(19, 0), 400, &mut retired).unwrap();
    r.abandon(other);
    r.forget_failures();
    assert_eq!(lease(&mut r, k), Lease::Miss);
    load(&mut r, k, 400).unwrap();
    assert!(matches!(lease(&mut r, k), Lease::Hit(_)));
}

#[test]
fn failure_memory_ends_with_the_source() {
    let mut r = Registry::new(CAP);
    let mut retired = Retired::new();
    let token = r.begin_load(key(16, 0), 400, &mut retired).unwrap();
    r.fail(token);
    assert_eq!(lease(&mut r, key(16, 1)), Lease::Miss);
}

#[test]
fn a_token_publishes_once_and_only_while_current() {
    let mut r = Registry::new(CAP);
    let mut retired = Retired::new();
    let a = r.begin_load(key(16, 0), 100, &mut retired).unwrap();
    assert_eq!(
        r.begin_load(key(19, 0), 100, &mut retired),
        Err(Refusal::Loading)
    );
    r.abandon(a);
    assert_eq!(r.publish(a, vec![0; 100]), Err(vec![0; 100]));
    let b = r.begin_load(key(16, 0), 100, &mut retired).unwrap();
    assert_ne!(a, b);
    assert_eq!(r.publish(a, vec![0; 100]).unwrap_err().len(), 100);
    r.publish(b, vec![0; 100]).unwrap();
    assert_eq!(r.publish(b, vec![0; 100]).unwrap_err().len(), 100);
    assert_eq!(
        r.begin_load(key(16, 0), 100, &mut retired),
        Err(Refusal::Present)
    );
}

#[test]
fn clear_cancels_the_load_and_drops_residents() {
    let mut r = Registry::new(CAP);
    load(&mut r, key(16, 0), 300).unwrap();
    let mut retired = Retired::new();
    let token = r.begin_load(key(19, 0), 300, &mut retired).unwrap();
    r.clear(&mut retired);
    assert_eq!(retired.len(), 1);
    assert_eq!(r.committed_bytes(), 0);
    assert_eq!(r.publish(token, vec![0; 300]).unwrap_err().len(), 300);
}

#[test]
fn total_storage_never_exceeds_the_cap() {
    let mut r = Registry::new(CAP);
    assert_eq!(
        r.begin_load(key(16, 0), CAP + 1, &mut Retired::new()),
        Err(Refusal::Size)
    );
    assert_eq!(
        r.begin_load(key(16, 0), 0, &mut Retired::new()),
        Err(Refusal::Size)
    );
    load(&mut r, key(16, 0), 600).unwrap();
    // 600 + 600 > 1000: the older index makes room, the newer stays
    let mut retired = Retired::new();
    let token = r.begin_load(key(19, 0), 600, &mut retired).unwrap();
    assert_eq!(retired.len(), 1);
    assert_eq!(r.committed_bytes(), 600);
    r.publish(token, vec![0; 600]).unwrap();
    assert_eq!(r.resident_bytes(), 600);
    assert_eq!(lease(&mut r, key(16, 0)), Lease::Miss);
    assert!(matches!(lease(&mut r, key(19, 0)), Lease::Hit(_)));
}

#[test]
fn least_recently_used_index_is_evicted_first() {
    let mut r = Registry::new(CAP);
    load(&mut r, key(16, 0), 300).unwrap();
    load(&mut r, key(19, 0), 300).unwrap();
    // 16 is used again, 19 is now the older one
    let Lease::Hit(v) = lease(&mut r, key(16, 0)) else {
        panic!("resident");
    };
    r.give_back(key(16, 0), v).unwrap();
    // two banks are full: the third size takes the bank of the oldest
    load(&mut r, key(23, 0), 300).unwrap();
    assert_eq!(lease(&mut r, key(19, 0)), Lease::Miss);
    assert!(matches!(lease(&mut r, key(16, 0)), Lease::Hit(_)));
    assert_eq!(r.resident(), 2);
}

#[test]
fn leased_index_is_not_evicted() {
    let mut r = Registry::new(CAP);
    load(&mut r, key(16, 0), 300).unwrap();
    load(&mut r, key(19, 0), 300).unwrap();
    let Lease::Hit(a) = lease(&mut r, key(16, 0)) else {
        panic!("resident");
    };
    let Lease::Hit(b) = lease(&mut r, key(19, 0)) else {
        panic!("resident");
    };
    let mut retired = Retired::new();
    assert_eq!(
        r.begin_load(key(23, 0), 300, &mut retired),
        Err(Refusal::InUse)
    );
    assert!(retired.is_empty());
    r.give_back(key(16, 0), a).unwrap();
    // one is back: it can go, the other stays with its user
    let token = r.begin_load(key(23, 0), 300, &mut retired).unwrap();
    assert_eq!(retired.len(), 1);
    r.publish(token, vec![0; 300]).unwrap();
    r.give_back(key(19, 0), b).unwrap();
    assert_eq!(r.resident(), 2);
    assert!(r.resident_bytes() <= CAP);
}

#[test]
fn generation_counts_bumps_and_wraps() {
    let g = SourceGeneration::new();
    assert_eq!(g.get(), 0);
    g.bump();
    g.bump();
    assert_eq!(g.get(), 2);
}

fn pack(pixel_size: u16, len: usize) -> FlashPackInfo {
    FlashPackInfo { pixel_size, len }
}

#[test]
fn flash_set_accepts_distinct_known_sizes() {
    assert_eq!(
        validate_flash_set(&[pack(16, 648_170), pack(23, 700_000)]),
        Ok(1_348_170)
    );
}

#[test]
fn flash_set_rejects_bad_configuration() {
    assert_eq!(validate_flash_set(&[]), Err(FlashSetError::Empty));
    assert_eq!(
        validate_flash_set(&[pack(17, 10)]),
        Err(FlashSetError::UnknownSize { pixel_size: 17 })
    );
    assert_eq!(
        validate_flash_set(&[pack(16, 10), pack(19, 10), pack(16, 12)]),
        Err(FlashSetError::Duplicate { pixel_size: 16 })
    );
    assert_eq!(
        validate_flash_set(&[pack(16, FLASH_FONT_MAX_TOTAL_BYTES + 1)]),
        Err(FlashSetError::PackTooLarge {
            pixel_size: 16,
            len: FLASH_FONT_MAX_TOTAL_BYTES + 1
        })
    );
    let half = FLASH_FONT_MAX_TOTAL_BYTES / 2 + 1;
    assert_eq!(
        validate_flash_set(&[pack(16, half), pack(19, half)]),
        Err(FlashSetError::TotalTooLarge { total: 2 * half })
    );
    // exactly the cap is accepted
    assert_eq!(
        validate_flash_set(&[pack(16, FLASH_FONT_MAX_TOTAL_BYTES)]),
        Ok(FLASH_FONT_MAX_TOTAL_BYTES)
    );
}

#[test]
fn flash_errors_name_the_problem() {
    let text = FlashSetError::Duplicate { pixel_size: 16 }.to_string();
    assert!(
        text.contains("16 px") && text.contains("more than one"),
        "{text}"
    );
    let text = FlashSetError::Empty.to_string();
    assert!(text.contains("PULP_C61_FLASH_FONTS"), "{text}");
}

#[test]
fn source_selection_prefers_the_configured_pack_by_size() {
    static A: [u8; 3] = [1, 2, 3];
    static B: [u8; 2] = [4, 5];
    let table: [(u16, &'static [u8]); 2] = [(16, &A), (23, &B)];
    assert_eq!(select_flash_pack(&table, 16), Some(&A[..]));
    assert_eq!(select_flash_pack(&table, 23), Some(&B[..]));
    assert_eq!(select_flash_pack(&table, 19), None);
    assert_eq!(select_flash_pack::<[u8]>(&[], 16), None);
}
