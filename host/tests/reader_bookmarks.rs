// Bookmark regression on the host: the real BookmarkCache (16 slots, LRU by
// generation, 48-byte records in _PULP/BKMK.BIN) over the virtual card, and
// how the real ReaderApp saves and restores positions through it.
//
// Ported from the archived oracle tests, reader-regression/os/tests/
// bookmarks.rs: the production kernel/src/kernel/bookmarks.rs is exercised
// through the public pulp_host API (Kernel + its production KernelHandle
// cache accessors, and the production boot path at Rig::new).
//
// Run: cargo test-host --test reader_bookmarks
//
// ============================================================================
// CONTRACT (implementer must provide exactly this; the tests are the spec)
// ============================================================================
//
// New public items in pulp_host::reader (forwarding only; no bookmark logic
// may live in pulp-host):
//
//   impl Rig {
//       pub fn save_position(&mut self);
//            ReaderApp::save_position(&mut BookmarkCache): the real save the
//            firmware's App::save_state makes, over the kernel's bookmark
//            cache (the one Rig::new loaded at boot).
//       pub fn bookmark_save(&mut self, filename: &[u8], byte_offset: u32, chapter: u16);
//            BookmarkCache::save(...) on the kernel's cache (the direct
//            cache drive the archived harness did through Kernel::bookmarks()).
//       pub fn bookmark_remove(&mut self, filename: &[u8]);
//            BookmarkCache::remove(...) on the kernel's cache.
//       pub fn bookmark_find(&self, filename: &[u8]) -> Option<bookmarks::BookmarkSlot>;
//            BookmarkCache::find(...) on the kernel's cache.
//       pub fn bookmark_list_into(&self, out: &mut [bookmarks::BmListEntry]) -> usize;
//            BookmarkCache::load_all(out): fills `out` with at most out.len()
//            entries, newest first, and returns how many were written. A
//            short output buffer receives the newest out.len() bookmarks.
//       pub fn bookmarks_flush(&mut self);
//            BookmarkCache::flush(&mut self, &SdStorage): the scheduler's
//            housekeeping save (writes _PULP/BKMK.BIN when dirty).
//       pub fn into_storage(self) -> VirtualStorage;
//            takes the card back out (power off): the test re-mounts the SAME
//            VirtualStorage in a fresh Rig -- a reboot. The fresh rig boots
//            through the production path (Rig::new loads the bookmark cache).
//   }
//
// New public item in pulp_host::kernel (mirrors the scheduler's housekeeping,
// exactly like the existing Kernel::bookmarks_load):
//
//   impl Kernel {
//       pub fn bookmarks_flush(&mut self);
//            BookmarkCache::flush(&mut self, &self.sd) -- the same call the
//            firmware scheduler makes when the flush timer is due
//            (kernel/src/kernel/scheduler.rs housekeeping body).
//   }
//
// Existing items used here are unchanged: Kernel::new / bookmarks_load /
// sd(); KernelHandle::bookmark_cache_mut(); SdStorage::new(card) with the
// public `card` field; VirtualStorage::memory / memory_with / ensure_pulp_dir
// / read_chunk_in_pulp / file_size_in_pulp / write_in_pulp (the *_in_pulp path
// "NAME" is _PULP/NAME); Rig::new / storage / configure / open / press / page
// / chapter / total_pages / page_offsets / lines / exit; the bookmarks module
// constants and types (SLOTS, RECORD_LEN, FILE_LEN, BOOKMARK_FILE,
// BookmarkSlot, BmListEntry). All expectations below come from the oracle
// tests (including their independent FNV-1a copy), never from running the
// implementation.
// ============================================================================

use pulp_host::drivers::sdcard::SdStorage;
use pulp_host::fixtures::{Newline, TxtSpec, build_txt};
use pulp_host::kernel::Kernel;
use pulp_host::kernel::bookmarks::{BOOKMARK_FILE, BmListEntry, FILE_LEN, RECORD_LEN, SLOTS};
use pulp_host::reader::{Action, Rig};
use pulp_host::storage::VirtualStorage;

// a card as the firmware boots it: `_PULP/` exists
fn card() -> VirtualStorage {
    let card = VirtualStorage::memory();
    card.ensure_pulp_dir().expect("fresh card has no _PULP yet");
    card
}

// the firmware's boot: kernel over the card, then the scheduler loads the
// bookmark cache. A fresh Kernel has NOT loaded yet; tests call
// bookmarks_load explicitly, exactly like the archived harness did.
fn kernel_with(storage: VirtualStorage) -> Kernel {
    Kernel::new(SdStorage::new(storage))
}

// the _PULP/BKMK.BIN bytes on the kernel's card, None when absent
fn bkmk(k: &Kernel) -> Option<Vec<u8>> {
    let size = k.sd().card.file_size_in_pulp(BOOKMARK_FILE).ok()?;
    let mut buf = vec![0u8; size as usize];
    let n = k
        .sd()
        .card
        .read_chunk_in_pulp(BOOKMARK_FILE, 0, &mut buf)
        .ok()?;
    buf.truncate(n);
    Some(buf)
}

// independent FNV-1a (32 bit) over the lower-cased name (oracle value, not
// the production fnv1a_icase)
fn fnv(name: &str) -> u32 {
    let mut h = 0x811c_9dc5u32;
    for b in name.bytes() {
        h ^= b.to_ascii_lowercase() as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

fn names(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("BOOK{:02}.TXT", i)).collect()
}

#[test]
fn bookmark_cache_format_constants() {
    assert_eq!((SLOTS, RECORD_LEN, FILE_LEN), (16, 48, 768));
    assert_eq!(BOOKMARK_FILE, "BKMK.BIN");
}

#[test]
fn not_loaded_means_no_lookup_and_no_save() {
    let mut k = kernel_with(card());
    // the cache is only usable after the kernel loaded it at boot
    k.handle().bookmark_cache_mut().save(b"A.TXT", 5, 0);
    let mut h = k.handle();
    let bm = h.bookmark_cache_mut();
    assert!(!bm.is_dirty(), "save before load is ignored");
    assert!(bm.find(b"A.TXT").is_none());
    let mut out = [BmListEntry::EMPTY; 16];
    assert_eq!(bm.load_all(&mut out), 0);
    k.bookmarks_load();
    let mut h = k.handle();
    let bm = h.bookmark_cache_mut();
    assert!(bm.is_loaded());
    assert!(bm.find(b"A.TXT").is_none());
}

#[test]
fn list_is_most_recently_saved_first() {
    let mut k = kernel_with(card());
    k.bookmarks_load();
    let mut h = k.handle();
    let bm = h.bookmark_cache_mut();
    for (i, n) in names(5).iter().enumerate() {
        bm.save(n.as_bytes(), i as u32, i as u16);
    }
    bm.save(b"BOOK04.TXT", 111, 4); // re-save the newest: stays first, offset updated
    let mut out = [BmListEntry::EMPTY; 16];
    let n = bm.load_all(&mut out);
    let order: Vec<_> = out[..n]
        .iter()
        .map(|e| e.filename_str().to_string())
        .collect();
    assert_eq!(
        order,
        [
            "BOOK04.TXT",
            "BOOK03.TXT",
            "BOOK02.TXT",
            "BOOK01.TXT",
            "BOOK00.TXT"
        ]
    );
    assert_eq!(out[0].chapter, 4);
    // a short output buffer receives the newest bookmarks
    let mut two = [BmListEntry::EMPTY; 2];
    assert_eq!(bm.load_all(&mut two), 2);
    assert_eq!(two[0].filename_str(), "BOOK04.TXT");
    assert_eq!(two[1].filename_str(), "BOOK03.TXT");
}

#[test]
fn resaving_an_early_slot_makes_it_the_newest() {
    let mut k = kernel_with(card());
    k.bookmarks_load();
    let mut h = k.handle();
    let bm = h.bookmark_cache_mut();
    for (i, n) in names(5).iter().enumerate() {
        bm.save(n.as_bytes(), i as u32, 0);
    }
    assert_eq!(bm.find(b"BOOK04.TXT").unwrap().generation, 5);
    bm.save(b"BOOK01.TXT", 111, 0);
    let saved = bm.find(b"BOOK01.TXT").unwrap();
    assert_eq!(saved.generation, 6);
    assert_eq!(saved.byte_offset, 111);
    let mut out = [BmListEntry::EMPTY; 16];
    let n = bm.load_all(&mut out);
    let order: Vec<_> = out[..n]
        .iter()
        .map(|e| e.filename_str().to_string())
        .collect();
    assert_eq!(
        order,
        [
            "BOOK01.TXT",
            "BOOK04.TXT",
            "BOOK03.TXT",
            "BOOK02.TXT",
            "BOOK00.TXT"
        ]
    );
}

#[test]
fn resaving_the_oldest_book_keeps_it_out_of_the_next_eviction() {
    let mut r = Rig::new(card());
    let all = names(SLOTS + 1);
    for n in &all[..SLOTS] {
        r.bookmark_save(n.as_bytes(), 1, 0);
    }
    r.bookmark_save(all[0].as_bytes(), 99, 3);
    r.bookmark_save(all[SLOTS].as_bytes(), 2, 0);
    assert!(r.bookmark_find(all[1].as_bytes()).is_none());
    assert_eq!(r.bookmark_find(all[0].as_bytes()).unwrap().byte_offset, 99);
    let mut out = [BmListEntry::EMPTY; 2];
    assert_eq!(r.bookmark_list_into(&mut out), 2);
    assert_eq!(out[0].filename_str(), all[SLOTS]);
    assert_eq!(out[1].filename_str(), all[0]);
}

#[test]
fn full_cache_evicts_the_lowest_generation_book() {
    let mut k = kernel_with(card());
    k.bookmarks_load();
    let mut h = k.handle();
    let bm = h.bookmark_cache_mut();
    let all = names(SLOTS + 2);
    for (i, n) in all[..SLOTS].iter().enumerate() {
        bm.save(n.as_bytes(), 1000 + i as u32, 0);
    }
    let mut out = [BmListEntry::EMPTY; 32];
    assert_eq!(bm.load_all(&mut out), 16);
    // the 17th book replaces the oldest (BOOK00, generation 1), the 18th the next (BOOK01)
    bm.save(all[16].as_bytes(), 7, 0);
    assert!(bm.find(all[0].as_bytes()).is_none(), "oldest evicted");
    assert_eq!(bm.find(all[16].as_bytes()).unwrap().generation, 17);
    bm.save(all[17].as_bytes(), 8, 0);
    assert!(bm.find(all[1].as_bytes()).is_none());
    assert_eq!(bm.load_all(&mut out), 16, "never more than 16");
    for i in 2..16 {
        assert_eq!(
            bm.find(all[i].as_bytes()).unwrap().byte_offset,
            1000 + i as u32
        );
    }
    // the newest two are first in the list
    assert_eq!(out[0].filename_str(), all[17]);
    assert_eq!(out[1].filename_str(), all[16]);
}

#[test]
fn a_removed_slot_is_reused_before_anything_is_evicted() {
    let mut k = kernel_with(card());
    k.bookmarks_load();
    let mut h = k.handle();
    let bm = h.bookmark_cache_mut();
    let all = names(SLOTS + 1);
    for n in &all[..SLOTS] {
        bm.save(n.as_bytes(), 1, 0);
    }
    bm.remove(all[5].as_bytes());
    bm.save(all[16].as_bytes(), 2, 0);
    for (i, n) in all.iter().enumerate() {
        assert_eq!(bm.find(n.as_bytes()).is_some(), i != 5, "{n}");
    }
}

#[test]
fn save_find_update_and_remove() {
    let mut k = kernel_with(card());
    k.bookmarks_load();
    let mut h = k.handle();
    let bm = h.bookmark_cache_mut();
    bm.save(b"BOOK.TXT", 1234, 0);
    bm.save(b"NOVEL.EPU", 99, 7);
    assert!(bm.is_dirty());
    let a = bm.find(b"BOOK.TXT").unwrap();
    assert_eq!((a.byte_offset, a.chapter, a.valid), (1234, 0, true));
    assert_eq!(a.filename_str(), "BOOK.TXT");
    let b = bm.find(b"NOVEL.EPU").unwrap();
    assert_eq!((b.byte_offset, b.chapter), (99, 7));
    // lookup folds ASCII case (FAT names)
    assert_eq!(bm.find(b"book.txt").unwrap().byte_offset, 1234);
    assert!(bm.find(b"OTHER.TXT").is_none());

    // saving the same book updates in place and bumps its generation
    let g0 = a.generation;
    bm.save(b"BOOK.TXT", 4321, 0);
    let a2 = bm.find(b"BOOK.TXT").unwrap();
    assert_eq!(a2.byte_offset, 4321);
    assert!(a2.generation > g0);
    let mut out = [BmListEntry::EMPTY; 16];
    assert_eq!(bm.load_all(&mut out), 2, "still two books");

    bm.remove(b"BOOK.TXT");
    assert!(bm.find(b"BOOK.TXT").is_none());
    assert_eq!(bm.load_all(&mut out), 1);
    // removing something that is not there is a no-op: the flush clears the
    // dirty flag of the first remove, the no-op must not set it again
    k.bookmarks_flush();
    assert!(!k.handle().bookmark_cache_mut().is_dirty());
    k.handle().bookmark_cache_mut().remove(b"NOPE.TXT");
    assert!(!k.handle().bookmark_cache_mut().is_dirty());
}

#[test]
fn flush_writes_the_baseline_record_layout() {
    let mut k = kernel_with(card());
    k.bookmarks_load();
    assert!(bkmk(&k).is_none(), "a clean cache writes nothing");
    let mut h = k.handle();
    let bm = h.bookmark_cache_mut();
    bm.save(b"BOOK.TXT", 0x0102_0304, 7);
    bm.save(b"B2.EPU", 9, 0);
    k.bookmarks_flush();
    assert!(!k.handle().bookmark_cache_mut().is_dirty());
    let f = bkmk(&k).unwrap();
    assert_eq!(f.len(), 2 * RECORD_LEN, "file holds exactly the used slots");
    let rec = &f[..RECORD_LEN];
    assert_eq!(
        &rec[0..4],
        &fnv("BOOK.TXT").to_le_bytes(),
        "name hash (FNV-1a, case folded)"
    );
    assert_eq!(&rec[4..8], &0x0102_0304u32.to_le_bytes(), "byte offset");
    assert_eq!(&rec[8..10], &7u16.to_le_bytes(), "chapter");
    assert_eq!(&rec[10..12], &1u16.to_le_bytes(), "flags: valid");
    assert_eq!(&rec[12..14], &1u16.to_le_bytes(), "generation");
    assert_eq!(rec[14], 8, "name length");
    assert_eq!(rec[15], 0);
    assert_eq!(&rec[16..24], b"BOOK.TXT");
    assert!(rec[24..48].iter().all(|&b| b == 0));
    assert_eq!(
        &f[RECORD_LEN + 12..RECORD_LEN + 14],
        &2u16.to_le_bytes(),
        "second save: generation 2"
    );
}

#[test]
fn persisted_bookmarks_survive_a_reboot_exactly() {
    let mut r = Rig::new(card());
    let all = names(SLOTS);
    for (i, n) in all.iter().enumerate() {
        r.bookmark_save(n.as_bytes(), 100 * i as u32, i as u16);
    }
    r.bookmark_remove(all[3].as_bytes());
    r.bookmark_save(all[0].as_bytes(), 31337, 2);
    r.bookmarks_flush();
    let mut before = [BmListEntry::EMPTY; SLOTS];
    let na = r.bookmark_list_into(&mut before);

    // power cycle: a new rig re-mounts the same card and boots through the
    // production path (the bookmark cache loads at Rig::new)
    let mut r2 = Rig::new(r.into_storage());
    for (i, n) in all.iter().enumerate() {
        let got = r2.bookmark_find(n.as_bytes());
        match i {
            3 => assert!(got.is_none(), "removed stays removed"),
            0 => assert_eq!((got.unwrap().byte_offset, got.unwrap().chapter), (31337, 2)),
            _ => assert_eq!(
                (got.unwrap().byte_offset, got.unwrap().chapter),
                (100 * i as u32, i as u16)
            ),
        }
    }
    let mut after = [BmListEntry::EMPTY; SLOTS];
    let nb = r2.bookmark_list_into(&mut after);
    assert_eq!(na, nb);
    for i in 0..na {
        assert_eq!(
            before[i].filename_str(),
            after[i].filename_str(),
            "list order identical after reload"
        );
    }
    // saving again after the reload keeps generations increasing (no collision)
    r2.bookmark_save(b"NEW.TXT", 1, 0);
    assert_eq!(
        r2.bookmark_find(b"NEW.TXT").unwrap().generation,
        18,
        "resaving the first slot assigned generation 17"
    );
}

// a rig booted over a card whose _PULP/BKMK.BIN holds `file`: the production
// load path reads and decodes whatever is there
fn rig_from(file: &[u8]) -> Rig {
    Rig::new(VirtualStorage::memory_with(&[("_PULP/BKMK.BIN", file)]))
}

// one bookmark record in the baseline byte layout, built with the test's own
// FNV-1a (independent of the production hash)
fn record(name: &[u8], off: u32, ch: u16, flags: u16, generation: u16, name_len: u8) -> Vec<u8> {
    let mut rec = vec![0u8; RECORD_LEN];
    rec[0..4].copy_from_slice(&fnv(std::str::from_utf8(name).unwrap()).to_le_bytes());
    rec[4..8].copy_from_slice(&off.to_le_bytes());
    rec[8..10].copy_from_slice(&ch.to_le_bytes());
    rec[10..12].copy_from_slice(&flags.to_le_bytes());
    rec[12..14].copy_from_slice(&generation.to_le_bytes());
    rec[14] = name_len;
    rec[16..16 + name.len()].copy_from_slice(name);
    rec
}

#[test]
fn short_lists_select_the_newest_named_valid_books() {
    let mut file = record(b"OLD.TXT", 1, 1, 1, 1, 7);
    file.extend_from_slice(&record(b"GONE.TXT", 2, 2, 0, u16::MAX, 8));
    file.extend_from_slice(&record(b"", 3, 3, 1, 10, 0));
    file.extend_from_slice(&record(b"NEW.TXT", 4, 4, 1, 9, 7));
    file.extend_from_slice(&record(b"MID.TXT", 5, 5, 1, 5, 7));
    let r = rig_from(&file);
    assert_eq!(r.bookmark_list_into(&mut []), 0);
    let mut out = [BmListEntry::EMPTY; 2];
    assert_eq!(r.bookmark_list_into(&mut out), 2);
    assert_eq!(out[0].filename_str(), "NEW.TXT");
    assert_eq!(out[0].chapter, 4);
    assert_eq!(out[1].filename_str(), "MID.TXT");
    assert_eq!(out[1].chapter, 5);
}

#[test]
fn resaving_at_generation_exhaustion_preserves_recency_after_reboot() {
    // Physical order differs from recency, and the resaved slot comes first.
    let mut file = record(b"B.TXT", 10, 1, 1, u16::MAX - 2, 5);
    file.extend_from_slice(&record(b"A.TXT", 20, 2, 1, u16::MAX, 5));
    file.extend_from_slice(&record(b"C.TXT", 30, 3, 1, u16::MAX - 1, 5));
    let mut r = rig_from(&file);
    r.bookmark_save(b"B.TXT", 40, 4);
    assert_eq!(r.bookmark_find(b"B.TXT").unwrap().generation, 4);
    assert_eq!(r.bookmark_find(b"A.TXT").unwrap().generation, 3);
    assert_eq!(r.bookmark_find(b"C.TXT").unwrap().generation, 2);
    r.bookmarks_flush();
    assert_eq!(
        r.storage().file_size_in_pulp(BOOKMARK_FILE).unwrap(),
        3 * RECORD_LEN as u32
    );

    let mut rebooted = Rig::new(r.into_storage());
    let mut out = [BmListEntry::EMPTY; SLOTS];
    assert_eq!(rebooted.bookmark_list_into(&mut out), 3);
    assert_eq!(out[0].filename_str(), "B.TXT");
    assert_eq!(out[1].filename_str(), "A.TXT");
    assert_eq!(out[2].filename_str(), "C.TXT");
    for (name, offset, chapter, generation) in [
        (b"B.TXT", 40, 4, 4),
        (b"A.TXT", 20, 2, 3),
        (b"C.TXT", 30, 3, 2),
    ] {
        let saved = rebooted.bookmark_find(name).unwrap();
        assert_eq!(
            (saved.byte_offset, saved.chapter, saved.generation),
            (offset, chapter, generation)
        );
    }
    rebooted.bookmark_save(b"NEW.TXT", 50, 5);
    assert_eq!(rebooted.bookmark_find(b"NEW.TXT").unwrap().generation, 5);
    assert_eq!(rebooted.bookmark_list_into(&mut out), 4);
    assert_eq!(out[0].filename_str(), "NEW.TXT");
}

#[test]
fn generation_rollover_preserves_tied_recency_and_skips_invalid_slots() {
    let mut file = record(b"A.TXT", 1, 0, 1, u16::MAX, 5);
    file.extend_from_slice(&record(b"GONE.TXT", 2, 0, 0, u16::MAX, 8));
    file.extend_from_slice(&record(b"B.TXT", 3, 0, 1, u16::MAX, 5));
    file.extend_from_slice(&record(b"C.TXT", 4, 0, 1, 0, 5));
    let mut r = rig_from(&file);
    r.bookmark_save(b"NEW.TXT", 5, 0);
    let mut out = [BmListEntry::EMPTY; SLOTS];
    assert_eq!(r.bookmark_list_into(&mut out), 4);
    let order: Vec<_> = out[..4].iter().map(|e| e.filename_str()).collect();
    assert_eq!(order, ["NEW.TXT", "A.TXT", "B.TXT", "C.TXT"]);
    assert_eq!(r.bookmark_find(b"NEW.TXT").unwrap().generation, 4);
    assert_eq!(r.bookmark_find(b"A.TXT").unwrap().generation, 3);
    assert_eq!(r.bookmark_find(b"B.TXT").unwrap().generation, 2);
    assert_eq!(r.bookmark_find(b"C.TXT").unwrap().generation, 1);
    assert!(r.bookmark_find(b"GONE.TXT").is_none());
}

#[test]
fn tied_minimum_generations_evict_the_last_tied_book_in_recency_order() {
    let all = names(SLOTS + 1);
    let mut file = Vec::new();
    for (i, name) in all[..SLOTS].iter().enumerate() {
        let generation = if matches!(i, 1 | 5 | 9) {
            1
        } else {
            i as u16 + 2
        };
        file.extend_from_slice(&record(
            name.as_bytes(),
            i as u32,
            0,
            1,
            generation,
            name.len() as u8,
        ));
    }
    let mut r = rig_from(&file);
    let mut out = [BmListEntry::EMPTY; SLOTS];
    assert_eq!(r.bookmark_list_into(&mut out), SLOTS);
    assert_eq!(out[SLOTS - 3].filename_str(), all[1]);
    assert_eq!(out[SLOTS - 2].filename_str(), all[5]);
    assert_eq!(out[SLOTS - 1].filename_str(), all[9]);
    r.bookmark_save(all[SLOTS].as_bytes(), 99, 0);
    assert!(r.bookmark_find(all[9].as_bytes()).is_none());
    for (i, name) in all[..SLOTS].iter().enumerate() {
        if i != 9 {
            assert_eq!(
                r.bookmark_find(name.as_bytes()).unwrap().byte_offset,
                i as u32
            );
        }
    }
    assert_eq!(r.bookmark_list_into(&mut out), SLOTS);
    assert_eq!(out[0].filename_str(), all[SLOTS]);
    assert_eq!(out[SLOTS - 2].filename_str(), all[1]);
    assert_eq!(out[SLOTS - 1].filename_str(), all[5]);
}

#[test]
fn a_full_cache_of_max_generations_evicts_the_last_book_in_recency_order() {
    let all = names(SLOTS + 1);
    let mut file = Vec::new();
    for name in &all[..SLOTS] {
        file.extend_from_slice(&record(
            name.as_bytes(),
            1,
            0,
            1,
            u16::MAX,
            name.len() as u8,
        ));
    }
    let mut r = rig_from(&file);
    let mut out = [BmListEntry::EMPTY; SLOTS];
    assert_eq!(r.bookmark_list_into(&mut out), SLOTS);
    assert_eq!(out[SLOTS - 1].filename_str(), all[SLOTS - 1]);
    r.bookmark_save(all[SLOTS].as_bytes(), 2, 0);
    assert!(r.bookmark_find(all[SLOTS - 1].as_bytes()).is_none());
    for name in &all[..SLOTS - 1] {
        assert!(r.bookmark_find(name.as_bytes()).is_some());
    }
    assert_eq!(
        r.bookmark_find(all[SLOTS].as_bytes()).unwrap().generation,
        17
    );
    assert_eq!(r.bookmark_list_into(&mut out), SLOTS);
    assert_eq!(out[0].filename_str(), all[SLOTS]);
    for i in 1..SLOTS {
        assert_eq!(out[i].filename_str(), all[i - 1]);
    }
}

#[test]
fn rollover_evicts_the_oldest_book_and_preserves_eviction_order_after_reboot() {
    let all = names(SLOTS + 2);
    let mut file = Vec::new();
    for (i, name) in all[..SLOTS].iter().enumerate() {
        file.extend_from_slice(&record(
            name.as_bytes(),
            100 + i as u32,
            i as u16,
            1,
            u16::MAX - i as u16,
            name.len() as u8,
        ));
    }
    let mut r = rig_from(&file);
    r.bookmark_save(all[SLOTS].as_bytes(), 200, 0);
    assert!(r.bookmark_find(all[SLOTS - 1].as_bytes()).is_none());
    assert_eq!(
        r.bookmark_find(all[SLOTS].as_bytes()).unwrap().generation,
        17
    );
    for (i, name) in all[..SLOTS - 1].iter().enumerate() {
        let saved = r.bookmark_find(name.as_bytes()).unwrap();
        assert_eq!(
            (saved.byte_offset, saved.chapter, saved.generation),
            (100 + i as u32, i as u16, (SLOTS - i) as u16)
        );
    }
    r.bookmark_save(all[0].as_bytes(), 300, 0);
    r.bookmarks_flush();
    assert_eq!(
        r.storage().file_size_in_pulp(BOOKMARK_FILE).unwrap(),
        FILE_LEN as u32
    );
    let mut rebooted = Rig::new(r.into_storage());
    rebooted.bookmark_save(all[SLOTS + 1].as_bytes(), 400, 0);
    assert!(rebooted.bookmark_find(all[SLOTS - 2].as_bytes()).is_none());
    assert!(rebooted.bookmark_find(all[SLOTS - 1].as_bytes()).is_none());
    assert_eq!(
        rebooted
            .bookmark_find(all[0].as_bytes())
            .unwrap()
            .byte_offset,
        300
    );
    assert_eq!(
        rebooted
            .bookmark_find(all[SLOTS + 1].as_bytes())
            .unwrap()
            .generation,
        19
    );
    let mut out = [BmListEntry::EMPTY; SLOTS];
    assert_eq!(rebooted.bookmark_list_into(&mut out), SLOTS);
    let order: Vec<_> = out.iter().map(|e| e.filename_str().to_string()).collect();
    let expected: Vec<_> = [all[SLOTS + 1].clone(), all[0].clone(), all[SLOTS].clone()]
        .into_iter()
        .chain(all[1..SLOTS - 2].iter().cloned())
        .collect();
    assert_eq!(order, expected);
}

#[test]
fn damaged_bookmark_files_are_tolerated() {
    let mut out = [BmListEntry::EMPTY; 16];

    // empty file: no bookmarks, the cache is usable
    let mut r = rig_from(b"");
    assert_eq!(r.bookmark_list_into(&mut out), 0);
    r.bookmark_save(b"A.TXT", 1, 0);
    assert!(r.bookmark_find(b"A.TXT").is_some());

    // shorter than one record: nothing decoded
    let r = rig_from(&[0xAA; 47]);
    assert_eq!(r.bookmark_list_into(&mut out), 0);

    // trailing partial record is ignored
    let mut f = record(b"A.TXT", 10, 1, 1, 1, 5);
    f.extend_from_slice(&[0xFF; 20]);
    let r = rig_from(&f);
    assert_eq!(r.bookmark_find(b"A.TXT").unwrap().byte_offset, 10);
    assert_eq!(r.bookmark_list_into(&mut out), 1);

    // flags without the valid bit: slot present but not a bookmark
    let r = rig_from(&record(b"A.TXT", 10, 1, 0, 1, 5));
    assert!(r.bookmark_find(b"A.TXT").is_none());
    assert_eq!(r.bookmark_list_into(&mut out), 0);

    // name_len beyond the 32-byte field is clamped (no out-of-range panic,
    // the cache stays loaded); what a clamped name matches is not specified
    let r = rig_from(&record(b"A.TXT", 10, 1, 1, 1, 255));
    let _ = r.bookmark_list_into(&mut out);

    // wrong hash for the stored name: that slot never matches
    let mut f = record(b"A.TXT", 10, 1, 1, 1, 5);
    f[0] ^= 0xFF;
    let r = rig_from(&f);
    assert!(r.bookmark_find(b"A.TXT").is_none());

    // pure garbage of the maximum size and beyond: at most 16 slots are read
    let garbage: Vec<u8> = (0..2000u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect();
    let mut r = rig_from(&garbage);
    let n = r.bookmark_list_into(&mut out);
    assert!(n <= 16);
    r.bookmark_save(b"OK.TXT", 77, 3);
    assert_eq!(r.bookmark_find(b"OK.TXT").unwrap().byte_offset, 77);

    // 17 valid records in the file: only the first 16 are loaded
    let mut f = Vec::new();
    for (i, n) in names(17).iter().enumerate() {
        f.extend_from_slice(&record(
            n.as_bytes(),
            i as u32,
            0,
            1,
            i as u16 + 1,
            n.len() as u8,
        ));
    }
    let r = rig_from(&f);
    assert!(r.bookmark_find(b"BOOK15.TXT").is_some());
    assert!(r.bookmark_find(b"BOOK16.TXT").is_none());
}

// ----------------------------------------------------- reader integration

// one source line per displayed line: the page count is known by construction
fn book_bytes(lines: usize) -> Vec<u8> {
    let lines: Vec<String> = (0..lines)
        .map(|i| format!("page line {i} of the synthetic bookmark book"))
        .collect();
    build_txt(&TxtSpec {
        lines,
        newline: Newline::Lf,
        trailing_newline: true,
    })
}

fn txt_rig() -> Rig {
    let card = VirtualStorage::memory_with(&[("BOOK.TXT", &book_bytes(800))]);
    card.ensure_pulp_dir()
        .expect("card has _PULP/ the way the firmware boots it");
    let mut r = Rig::new(card);
    r.configure(2, 1);
    r.open("BOOK.TXT");
    r
}

#[test]
fn reader_bookmark_round_trip_through_the_card() {
    let mut r = txt_rig();
    for _ in 0..8 {
        r.press(Action::Next);
    }
    let want = (r.chapter(), r.page(), r.page_offsets()[r.page()], r.lines());
    r.save_position();
    r.bookmarks_flush();
    r.exit();

    // reboot: a new rig on the same card, the bookmark cache reloaded at boot
    let mut r2 = Rig::new(r.into_storage());
    r2.configure(2, 1);
    r2.open("BOOK.TXT");
    assert_eq!(
        (
            r2.chapter(),
            r2.page(),
            r2.page_offsets()[r2.page()],
            r2.lines()
        ),
        want,
        "same page and same text after a reboot"
    );
}

#[test]
fn generated_epub_bookmarks_restore_chapter_page_and_text_after_reboot() {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _serial = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for fixture in pulp_host::fixtures::standard() {
        if !matches!(fixture.spec, pulp_host::fixtures::Spec::Epub(_)) {
            continue;
        }
        let card = card();
        card.write_file(fixture.name, &fixture.bytes).unwrap();
        let mut reader = Rig::new(card);
        reader.configure(2, 1);
        reader.open(fixture.name);
        assert_eq!(
            reader.phase(),
            pulp_host::reader::Phase::Ready,
            "{}",
            fixture.name
        );
        reader.idle(400);
        reader.press(Action::NextJump);
        assert_eq!(reader.chapter(), 1, "{}", fixture.name);
        reader.press(Action::Next);
        assert_eq!(reader.page(), 1, "{}", fixture.name);
        let want = (reader.chapter(), reader.page(), reader.lines());
        reader.save_position();
        reader.bookmarks_flush();
        reader.exit();
        let mut rebooted = Rig::new(reader.into_storage());
        rebooted.configure(2, 1);
        rebooted.open(fixture.name);
        assert_eq!(
            rebooted.phase(),
            pulp_host::reader::Phase::Ready,
            "{}",
            fixture.name
        );
        assert_eq!(
            (rebooted.chapter(), rebooted.page(), rebooted.lines()),
            want,
            "{}: the same card restores the saved chapter, page and text",
            fixture.name
        );
        rebooted
            .storage()
            .write_file("OTHER.EPU", &fixture.bytes)
            .unwrap();
        rebooted.open("OTHER.EPU");
        assert_eq!(
            (rebooted.chapter(), rebooted.page()),
            (0, 0),
            "{}: another book has no bookmark",
            fixture.name
        );
        assert!(rebooted.bookmark_find(b"OTHER.EPU").is_none());
        rebooted.save_position();
        let mut entries = [BmListEntry::EMPTY; 16];
        assert_eq!(rebooted.bookmark_list_into(&mut entries), 2);
        let saved = rebooted.bookmark_find(fixture.name.as_bytes()).unwrap();
        assert_eq!(entries[0].filename_str(), "OTHER.EPU");
        rebooted.bookmark_save(fixture.name.as_bytes(), saved.byte_offset, saved.chapter);
        assert_eq!(rebooted.bookmark_list_into(&mut entries), 2);
        assert_eq!(entries[0].filename_str(), fixture.name);
        rebooted.bookmarks_flush();
        rebooted.exit();
        let mut remounted = Rig::new(rebooted.into_storage());
        assert_eq!(remounted.bookmark_list_into(&mut entries), 2);
        assert_eq!(entries[0].filename_str(), fixture.name);
        remounted.bookmark_remove(fixture.name.as_bytes());
        remounted.bookmarks_flush();
        remounted.configure(2, 1);
        remounted.open(fixture.name);
        assert_eq!(
            (remounted.chapter(), remounted.page()),
            (0, 0),
            "{}: deletion restores the initial position",
            fixture.name
        );
        assert!(remounted.bookmark_find(fixture.name.as_bytes()).is_none());
        assert!(remounted.bookmark_find(b"OTHER.EPU").is_some());
    }
}

#[test]
fn removing_the_bookmark_reopens_at_the_first_page() {
    let mut r = txt_rig();
    for _ in 0..4 {
        r.press(Action::Next);
    }
    r.save_position();
    r.exit();
    r.bookmark_remove(b"BOOK.TXT");
    r.open("BOOK.TXT");
    assert_eq!(r.page(), 0);
}

#[test]
fn bookmarks_are_per_book() {
    let card =
        VirtualStorage::memory_with(&[("A.TXT", &book_bytes(600)), ("B.TXT", &book_bytes(600))]);
    card.ensure_pulp_dir()
        .expect("card has _PULP/ the way the firmware boots it");
    let mut r = Rig::new(card);
    r.configure(2, 1);
    r.open("A.TXT");
    for _ in 0..5 {
        r.press(Action::Next);
    }
    r.save_position();
    r.exit();
    r.open("B.TXT");
    assert_eq!(r.page(), 0, "B has no bookmark");
    r.press(Action::Next);
    r.save_position();
    r.exit();
    r.open("A.TXT");
    assert_eq!(r.page(), 5);
    r.exit();
    r.open("B.TXT");
    assert_eq!(r.page(), 1);
    assert!(r.total_pages() > 1);
}

#[test]
fn reader_save_position_does_not_touch_the_session_files() {
    // the bookmark file and the session slots are separate files; a position
    // save must never write the neighbours
    let mut r = txt_rig();
    r.storage()
        .write_in_pulp("SESSA.BIN", &[1, 2, 3])
        .expect("seed SESSA.BIN");
    r.storage()
        .write_in_pulp("SESSB.BIN", &[4, 5, 6])
        .expect("seed SESSB.BIN");
    r.press(Action::Next);
    r.save_position();
    r.bookmarks_flush();
    let mut a = [0u8; 3];
    let n = r
        .storage()
        .read_chunk_in_pulp("SESSA.BIN", 0, &mut a)
        .expect("SESSA.BIN still there");
    assert_eq!(&a[..n], &[1, 2, 3]);
    let mut b = [0u8; 3];
    let n = r
        .storage()
        .read_chunk_in_pulp("SESSB.BIN", 0, &mut b)
        .expect("SESSB.BIN still there");
    assert_eq!(&b[..n], &[4, 5, 6]);
    assert!(
        r.storage().file_size_in_pulp(BOOKMARK_FILE).is_ok(),
        "the bookmark file was written"
    );
}
