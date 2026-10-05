# T6b author report — settings & bookmarks regression on the host

Author: test author, red tier. Deliverables: `host/tests/reader_settings.rs`
(18 tests), `host/tests/reader_bookmarks.rs` (14 tests). Nothing else was
created or edited; no `cargo fmt` was run (only `rustfmt --edition 2024` on the
two new files).

Oracle source: `scripts/reader-regression/os/tests/settings.rs` (19 tests) and
`scripts/reader-regression/os/tests/bookmarks.rs` (16 tests), plus the archived
harness `scripts/reader-regression/os/src/{rig,lib,fixtures}.rs` and
`scripts/reader-regression/kernel/src/kernel/mod.rs`. All line references below
are into those files.

---

## 1. Contract definitions (exact signatures, grouped by file)

### host/src/apps.rs — include the real settings app

```rust
#[path = "../../src/apps/settings.rs"]
#[allow(dead_code, unused_imports)]
pub mod settings;
```

`src/apps/settings.rs` is the production file and is expected to compile
unchanged: every `crate::` path it names is already exported by the host
surface (verified item by item):
- `crate::ui::{Alignment, BUTTON_BAR_H, BitmapLabel, CONTENT_TOP,
  FULL_CONTENT_W, LARGE_MARGIN, Region, SECTION_GAP, StackFmt, TITLE_Y,
  wrap_next, wrap_prev}` — `host/src/ui.rs` re-exports all of them
  (`crate::kernel_ui::*` for the layout/format items, widgets for
  `BitmapLabel`/`BUTTON_BAR_H`; `StackFmt` is the kernel's
  `kernel/src/ui/stack_fmt.rs`).
- `crate::fonts::{UiFonts, font_size_name, max_size_idx}` — real
  `src/fonts/mod.rs`, included by `host/src/lib.rs`.
- `crate::board::{SCREEN_H, SCREEN_W}` — `host/src/board.rs` consts.
- `crate::drivers::strip::StripBuffer` — real `kernel/src/drivers/strip.rs`.
- `crate::apps::{App, AppContext, AppId, Transition}` — `host/src/apps.rs` has
  the same `AppId` shape (Home/Files/Reader/Settings) and the real `App` trait.
- `crate::kernel::{KernelHandle, config::{...}}` — host kernel modules.

The archived host build (`scripts/reader-regression/os/src/apps/mod.rs:6-10`)
compiled exactly this file; only its `crate::ui` path differed there.

### host/src/kernel.rs — scheduler housekeeping next to `bookmarks_load`

```rust
impl Kernel {
    /// BookmarkCache::flush(&mut self, &self.sd) — the same call the firmware
    /// scheduler makes when the flush timer is due
    /// (kernel/src/kernel/scheduler.rs:282-283 housekeeping body).
    pub fn bookmarks_flush(&mut self);
}
```

Mirrors the archived harness's `Kernel::bookmarks_flush`
(`scripts/reader-regression/kernel/src/kernel/mod.rs:86-88`) and the existing
`Kernel::bookmarks_load` ("what the scheduler does at boot").

### host/src/reader.rs — `Rig` additions (forwarding only)

```rust
impl Rig {
    /// ReaderApp::save_position(&mut BookmarkCache): the real save the
    /// firmware's App::save_state makes, over the kernel's bookmark cache
    /// (the one Rig::new loaded at boot).
    pub fn save_position(&mut self);

    /// BookmarkCache::save(...) on the kernel's cache (the direct cache drive
    /// the archived harness did through Kernel::bookmarks()).
    pub fn bookmark_save(&mut self, filename: &[u8], byte_offset: u32, chapter: u16);

    /// BookmarkCache::remove(...) on the kernel's cache.
    pub fn bookmark_remove(&mut self, filename: &[u8]);

    /// BookmarkCache::find(...) on the kernel's cache.
    pub fn bookmark_find(&self, filename: &[u8]) -> Option<crate::kernel::bookmarks::BookmarkSlot>;

    /// BookmarkCache::load_all(out): fills `out` with at most out.len()
    /// entries in the cache's list order and returns how many were written
    /// (the archived short-buffer behaviour is part of the contract).
    pub fn bookmark_list_into(&self, out: &mut [crate::kernel::bookmarks::BmListEntry]) -> usize;

    /// BookmarkCache::flush(&mut self, &SdStorage): the scheduler's
    /// housekeeping save (writes _PULP/BKMK.BIN when dirty); delegates to
    /// Kernel::bookmarks_flush.
    pub fn bookmarks_flush(&mut self);

    /// Takes the card back out (power off): the test re-mounts the SAME
    /// VirtualStorage in a fresh Rig / SettingsRig — a reboot. The fresh rig
    /// boots through the production path (Rig::new loads the bookmark cache).
    pub fn into_storage(self) -> VirtualStorage;

    /// The reading-theme index stored in the real ReaderApp (the same
    /// read-only probe the archived harness exposed as probe::theme_idx,
    /// scripts/reader-regression/os/src/apps/probe.rs:64-66).
    pub fn theme_idx(&self) -> u8;
}
```

All 18 existing `Rig` methods stay unchanged (`new` / `storage` / `configure`
/ `open` / `press` / `phase` / `error_kind` / `page` / `total_pages` /
`fully_indexed` / `page_offsets` / `lines` / `max_lines` / `text_w` /
`text_margin` / `font_line_h` / `text_y` / `text_area_h` / `draw` / `chapter`
/ `spine_len` / `epub_title` / `epub_author` / `toc_entries` / `toc_selected`
/ `line_infos` / `page_image` / `quick_action_ids` / `quick_trigger` / `exit`
/ `idle` / `has_bg_work`).

### host/src/reader.rs — settings entry: `SettingsRig`

Decision (asked for by the brief): an **explicitly-scoped direct drive of the
real `SettingsApp`**, as a second rig in `pulp_host::reader`, NOT an
`AppManager`-navigation entry. Why:
1. The firmware's settings entry is `AppManager` navigation (home menu → app
   switch), and `host/src/apps.rs` deliberately excludes manager/home/files
   (firmware-only drivers). Pulling `src/apps/manager.rs` into the host build
   to get one transition would drag in `FilesApp`/`HomeApp` and the real
   display/SD board surface — a large, unrelated seam; re-implementing the
   navigation would violate the production-path requirement.
2. The archived oracle never tests menu navigation INTO settings: its own
   `SettingsRig` (`scripts/reader-regression/os/src/rig.rs:139-186`) is the
   same direct drive, with `boot()` documented as
   "`AppManager::load_eager_settings` (the `load_eager` half)" — `boot()`
   here is the same half, so scenario parity is preserved.
3. Fewest new seams: one `#[path]` module + one forwarding struct. The
   settings → reader half of `load_eager_settings` (`propagate_fonts`,
   `src/apps/manager.rs:199-204,578-593`) is already on the host as the
   `Rig::configure` forwarding (`set_book_font_size` / `set_reading_theme`),
   which the propagation test drives with the values the real app loaded.

```rust
pub struct SettingsRig;   // fields private, like Rig

impl SettingsRig {
    /// The real SettingsApp::new() over a fresh Kernel::new(SdStorage::new(storage))
    /// and a fresh AppContext. No bookmark load (the archived rig is the
    /// load_eager half; the scheduler loads bookmarks separately).
    pub fn new(storage: VirtualStorage) -> Self;

    /// SettingsApp::load_eager(&mut KernelHandle<'_>): the production load —
    /// reads _PULP/SETTINGS.TXT through the real handle (at most 512 bytes)
    /// or falls back to defaults, then applies the UI font size.
    pub fn boot(&mut self);

    /// SettingsApp::is_loaded().
    pub fn is_loaded(&self) -> bool;

    /// *SettingsApp::system_settings() (SystemSettings is Copy).
    pub fn settings(&self) -> SystemSettings;

    /// SettingsApp::wifi_config().ssid() as an owned string.
    pub fn wifi_ssid(&self) -> String;

    /// SettingsApp::on_enter(&mut AppContext, &mut KernelHandle<'_>).
    pub fn enter(&mut self);

    /// SettingsApp::on_event(ActionEvent::Press(a), &mut AppContext);
    /// the Transition is ignored (the settings app never navigates away
    /// under these keys, and the tests observe the settings only).
    pub fn press(&mut self, a: Action);

    /// One scheduler background tick: block_on(app.background(...)). A dirty
    /// app saves here (write_app_data -> _PULP/SETTINGS.TXT).
    pub fn tick(&mut self);

    /// The _PULP/SETTINGS.TXT bytes on the rig's card, None when absent.
    pub fn file(&self) -> Option<Vec<u8>>;

    /// Same reboot seam as Rig::into_storage.
    pub fn into_storage(self) -> VirtualStorage;
}
```

Count: 1 module include + 1 `Kernel` method + 8 `Rig` methods + 1 struct with
10 methods = **20 contract items**. Every one forwards to a real app/kernel
call; none carries settings or bookmark logic.

---

## 2. Test list

### host/tests/reader_settings.rs — 18 tests

| # | test | pins | serves | oracle ref | proof state |
|---|------|------|--------|-----------|-------------|
| 1 | `defaults_are_the_baseline_values` | `SystemSettings::defaults` values (10/10/2/2/1/false) and empty-file parse giving defaults with no wifi credentials | settings regression, R2 (production `kernel::config`) | settings.rs:20-33 | **already green pre-implementation** |
| 2 | `theme_table_is_the_baseline` | the four reading themes (name/margin_h/margin_v/spacing) and out-of-range index → last theme | settings regression | settings.rs:35-44 | already green |
| 3 | `every_key_is_parsed_by_its_exact_name` | every key name, `value = everything after the first '='` (p@ss=word, spaces in ssid) | settings regression | settings.rs:46-58 | already green |
| 4 | `unknown_keys_comments_blank_lines_and_whitespace_are_tolerated` | comments/blank/unknown/garbage lines tolerated; trim around key/value; keys case-sensitive | settings regression | settings.rs:60-68 | already green |
| 5 | `corrupt_values_keep_the_previous_value` | unparsable values keep defaults (starting from defaults, as the firmware load does); last assignment wins | settings regression | settings.rs:70-84 | already green |
| 6 | `out_of_range_values_are_clamped_by_sanitize` | clamp table (120/5/4/4/3); ghost_clear u16→u8 truncation quirk (300 as u8 = 44); sleep 0 = never | settings regression | settings.rs:86-98 | already green |
| 7 | `wifi_credentials_are_kept_and_capped` | SSID cap 32, pass cap 63, empty ssid = no credentials | settings regression | settings.rs:100-108 | already green |
| 8 | `write_then_parse_round_trips_every_field_including_wifi` | write→parse roundtrip of all six fields + wifi; idempotent bytes | settings regression, R4 | settings.rs:110-130 | already green |
| 9 | `serialized_text_is_the_baseline_text` | the exact baseline SETTINGS.TXT text produced by `write_settings_txt` | settings regression | settings.rs:132-141 | already green |
| 10 | `maximum_credentials_still_fit_the_apps_512_byte_buffer` | max-clamped settings + capped credentials serialize < 512 B with the last key intact | settings regression | settings.rs:143-154 | already green |
| 11 | `settings_file_is_read_up_to_512_bytes` | production `SettingsApp::load` reads at most 512 bytes: a key after the window is not seen | settings regression, R2 (real app) | settings.rs:156-169 | red: `SettingsRig` missing; predicted green |
| 12 | `settings_app_loads_defaults_when_there_is_no_file` | fresh app: not loaded → boot loads → defaults, and loading never writes | settings regression, R2 | settings.rs:173-181 | red: `SettingsRig`; predicted green |
| 13 | `settings_app_loads_a_saved_file_and_sanitizes_it` | saved file values + sanitize (sleep 900→120) + wifi ssid visible through the app | settings regression, R2 | settings.rs:184-192 | red: `SettingsRig`; predicted green |
| 14 | `settings_app_edits_steps_clamps_and_saves` | the real editing loop: row navigation, step/clamp per row (both directions, swap toggles), nothing written until the scheduler tick, the saved bytes parse back, a fresh boot reads them | settings regression, R2, R4 | settings.rs:195-256 | red: `SettingsRig`; predicted green |
| 15 | `settings_app_keeps_wifi_credentials_when_saving_other_values` | a settings save re-writes the wifi keys it loaded (offline build keeps credentials) | settings regression, R4 | settings.rs:258-269 | red: `SettingsRig`; predicted green |
| 16 | `saved_book_font_and_theme_decide_the_reader_layout` | load_eager → propagate_fonts: saved font/theme values (4, 3) decide the reader layout (max_lines 10, text_w 400) and the theme index reaches the app | settings→reader, R2 | settings.rs:287-302 | red: `SettingsRig` + `Rig::theme_idx`; predicted green |
| 17 | `swap_buttons_setting_selects_the_button_mapping_x4` | the parsed `swap_buttons` drives the X4 `ButtonMapper` default↔swapped table (incl. LongPress mapping) | settings regression | settings.rs:304-339 | already green |
| 18 | `swap_buttons_setting_selects_the_button_mapping_c61` | the same value drives the C61 key table (`pulp-board-logic::keys`, pure logic) | settings regression | settings.rs:341-353 | already green |

### host/tests/reader_bookmarks.rs — 14 tests

| # | test | pins | serves | oracle ref | proof state |
|---|------|------|--------|-----------|-------------|
| 1 | `bookmark_cache_format_constants` | (SLOTS, RECORD_LEN, FILE_LEN) = (16, 48, 768), BOOKMARK_FILE = "BKMK.BIN" | bookmarks regression, R2 | bookmarks.rs:33-37 | **already green pre-implementation** |
| 2 | `not_loaded_means_no_lookup_and_no_save` | a cache not yet loaded: save ignored (no dirty), find None, load_all 0; the boot load makes it usable and empty | bookmarks regression, R2 | bookmarks.rs:39-51 | already green |
| 3 | `save_find_update_and_remove` | save/find/update-in-place with generation bump, case-folded lookup, remove, no-op remove keeps the cache clean | bookmarks regression | bookmarks.rs:53-86 | red: `Kernel::bookmarks_flush` missing; predicted green |
| 4 | `list_is_most_recently_saved_first` | load_all order = most-recently-saved first; re-save keeps first place and updates the offset; short buffer = first N slots in file order sorted by recency among themselves | bookmarks regression (reorder) | bookmarks.rs:88-107 | already green |
| 5 | `resaving_an_early_slot_does_not_always_make_it_the_newest` | the scan-stops-at-match generation quirk: re-saving slot 1 gets max(gens up to it)+1 = 3 and only ties with slot 2 | bookmarks regression | bookmarks.rs:109-127 | already green |
| 6 | `full_cache_evicts_the_lowest_generation_book` | 16-slot LRU by generation: 17th/18th saves evict generation 1 then 2; never more than 16; newest two first in the list | bookmarks regression | bookmarks.rs:129-152 | already green |
| 7 | `a_removed_slot_is_reused_before_anything_is_evicted` | a freed slot is refilled before any eviction | bookmarks regression (delete) | bookmarks.rs:154-167 | already green |
| 8 | `flush_writes_the_baseline_record_layout` | a clean cache writes nothing; a dirty one writes exactly the used slots with the baseline 48-byte record layout (FNV-1a case-folded name hash, offset, chapter, valid flag, generation, name_len, name, zero pad) | bookmarks regression, R4 | bookmarks.rs:169-191 | red: `Kernel::bookmarks_flush`; predicted green |
| 9 | `persisted_bookmarks_survive_a_reboot_exactly` | flush → reboot (new Rig, same VirtualStorage) → every slot's offset/chapter/generation, removed-stays-removed, list order identical, and the next save continues generations from max+1 (17) | bookmarks regression, R4 | bookmarks.rs:213-251 | red: `bookmark_save`/`remove`/`find`/`list_into`/`bookmarks_flush`/`into_storage`; predicted green |
| 10 | `damaged_bookmark_files_are_tolerated` | empty / 47-byte / trailing-partial / invalid-flag / clamped name_len / wrong-hash / 2000-byte garbage / 17 records: no panic, at most 16 slots, the cache stays usable and saves work | bookmarks regression, R2 | bookmarks.rs:273-328 | red: `bookmark_find`/`bookmark_list_into`; predicted green |
| 11 | `reader_bookmark_round_trip_through_the_card` | reader-driven save_position → flush → exit → reboot (new Rig, same card) → open restores the same chapter, page, byte offset and page text | bookmarks regression (add/jump), R2, R4 | bookmarks.rs:343-370 | red: `save_position`/`bookmarks_flush`/`into_storage`; predicted green |
| 12 | `removing_the_bookmark_reopens_at_the_first_page` | removing the bookmark, then re-entering the book, opens at page 0 | bookmarks regression (delete) | bookmarks.rs:372-383 | red: `save_position`/`bookmark_remove`; predicted green |
| 13 | `bookmarks_are_per_book` | two books on one card keep independent positions across interleaved open/exit; a book with no bookmark opens at page 0; the page table covers more than one page | bookmarks regression, R4 | bookmarks.rs:385-409 | red: `save_position`; predicted green |
| 14 | `reader_save_position_does_not_touch_the_session_files` | save_position + flush never write the _PULP neighbours (SESSA.BIN/SESSB.BIN stay byte-identical) while BKMK.BIN appears | bookmarks regression, R4 | bookmarks.rs:411-430 | red: `save_position`/`bookmarks_flush`; predicted green |

Proof-state summary:
- **18 tests green before any implementation change** (12 settings + 6
  bookmarks): they use only existing public API and derive every expectation
  from the oracle text / the production constants the oracle pins.
- **14 tests red, blocked exactly on the missing contract APIs** (compile
  errors only — no behavior failure), predicted green after implementation
  because they are ported scenarios that were green on the archived harness
  (labeled predicted-green; not a weakened red-proof).

---

## 3. Red-proof evidence

Order of runs, all with `scripts/host-test.sh` (the repo's default target is
bare-metal RISC-V, so the plain `cargo test -p pulp-host` form selects the
wrong sysroot; the wrapper is the sanctioned host form):

**Run A (baseline, before any new file):** `scripts/host-test.sh` → 195 pass:
lib 1, fixtures 46, paging 24, reader_epub 31, render 39, storage 36, utf8 18.

**Run B (stage 1 — the two new files containing only the tests that need no
new API):** 18 pass / 0 fail:

```
test bookmark_cache_format_constants ... ok
test not_loaded_means_no_lookup_and_no_save ... ok
test list_is_most_recently_saved_first ... ok
test resaving_an_early_slot_does_not_always_make_it_the_newest ... ok
test full_cache_evicts_the_lowest_generation_book ... ok
test a_removed_slot_is_reused_before_anything_is_evicted ... ok
test result: ok. 6 passed; 0 failed (test "reader_bookmarks")
test defaults_are_the_baseline_values ... ok
... (8 more config/mapper tests) ...
test result: ok. 12 passed; 0 failed (test "reader_settings")
```

**Run C (final files — all 32 tests, no implementation yet):** compile errors
only, and only missing-API items; no test failed for a behavior reason:

```
error[E0432]: unresolved import `pulp_host::reader::SettingsRig`
  --> host/tests/reader_settings.rs:94:30
   |
94 | use pulp_host::reader::{Rig, SettingsRig};
   |                              ^^^^^^^^^^^ no `SettingsRig` in `reader`

error[E0599]: no method named `theme_idx` found for struct `Rig` in the current scope
   --> host/tests/reader_settings.rs:557:18

error[E0599]: no method named `bookmarks_flush` found for struct `Kernel` in the current scope
   --> host/tests/reader_bookmarks.rs:288:7
    |
288 |     k.bookmarks_flush();
    |       ^^^^^^^^^^^^^^^
    |
help: there is a method `bookmarks_load` with a similar name
```

(`scripts/host-test.sh --test reader_settings` → "due to 2 previous errors";
`--test reader_bookmarks` → "due to 36 previous errors", 10 distinct missing
items). The complete item set matches the contract 1:1:

```
`SettingsRig`                        (E0432, reader_settings.rs:94)
`theme_idx`                          (E0599, reader_settings.rs:557)
`bookmarks_flush` on Kernel          (E0599, reader_bookmarks.rs:288, 303)
`save_position`                      (E0599, reader_bookmarks.rs x5)
`bookmark_save`                      (E0599, reader_bookmarks.rs x5)
`bookmark_remove`                    (E0599, reader_bookmarks.rs x2)
`bookmark_find`                      (E0599, reader_bookmarks.rs x9)
`bookmark_list_into`                 (E0599, reader_bookmarks.rs x8)
`bookmarks_flush` on Rig             (E0599, reader_bookmarks.rs x3)
`into_storage`                       (E0599, reader_bookmarks.rs x2)
```

**Run D (current tree, the pre-existing suites explicitly, with the new files
present):** still 195 — lib 1, fixtures 46, paging 24, reader_epub 31, render
39, storage 36, utf8 18. Nothing outside the two new files was touched.

**Derivation statement (required by the brief):** every expected value in both
files comes from the oracle test text (including its independent FNV-1a copy
and the exact baseline SETTINGS.TXT text) or from the production constants the
oracle pins; none was obtained by running or reading the implementation. Two
draft-time over-specifications were caught and removed BEFORE the red-proof
run: (a) an extra `find` assertion on the name_len=255 record — the oracle
asserts only "no panic, cache still loaded" there, so the final test matches;
(b) two "the book has enough pages" invariant asserts on `total_pages()`
immediately after open — the existing paging contract pins that the TXT page
table is built lazily as pages are turned, so those asserts were invalid as
written and were removed (the walks themselves prove the pages exist).

---

## 4. Gaps (out of scope, for the coverage report)

1. **`a_failed_save_is_retried_on_the_next_tick`** (settings.rs:271-283):
   the save-failure retry path needs storage failure injection (card
   eject/remount juggling). Storage failure injection belongs to the later
   error-path batch; excluded here.
2. **`flush_is_skipped_when_clean_and_retried_when_the_card_fails`**
   (bookmarks.rs:193-211): the flush-on-failed-write retry half needs storage
   failure injection — excluded, same reason. Note: the test's first half
   ("clean cache writes nothing") is pinned by
   `flush_writes_the_baseline_record_layout`'s opening assertion
   (`bkmk(&k).is_none()` before any save).
3. **`a_read_error_while_loading_leaves_an_empty_usable_cache`**
   (bookmarks.rs:330-339): read-error injection (`fail_next_reads`) — storage
   failure injection, excluded.
4. **SD session restore** (the archived `session_restore.rs`, cfg(feature =
   "tree")): C61 session-slot hardware logic — excluded per the brief; named
   here so the coverage report records it. Only the non-interference half
   (save_position never writes SESSA/SESSB.BIN) is covered — see test 14.
5. **Wi-Fi hardware/network behavior**: excluded entirely. Only the
   `WifiConfig` bytes inside SETTINGS.TXT are exercised (parse caps, roundtrip,
   app-load and the keep-credentials-on-save scenario) — no network code runs.
6. **Cover rendering / image chapters / EPUB navigation internals**: out of
   this batch (already covered by `host/tests/reader_epub.rs`).
7. **`SettingsApp::is_loaded` through the reader rig**: the loaded flag is
   directly asserted at cache level (`not_loaded_means_no_lookup_and_no_save`,
   via the kernel handle); the Rig does not expose it. Port detail, not a
   behavior gap: the damaged-file scenario asserts cache usability through
   save/find instead.

## 5. Findings

1. **The C61 swap-mapping oracle test does not need its cfg gate.** The old
   harness gates it `#[cfg(feature = "tree")]` (settings.rs:341), but it only
   drives `pulp_board_logic::keys` — pure logic, and that crate's stated rule
   is "no cfg on board features" (board-logic/src/lib.rs). Ported ungated; it
   passes on the host.
2. **The host "reboot" is a stronger oracle than the archived one.** The
   archived tests reconstruct a fresh card seeded with the flushed bytes
   (bookmarks.rs:227-230, 361-365); the brief's same-object re-mount keeps the
   identical card state (settings file, bookmark file and any caches the first
   boot wrote), so persistence is validated without byte copying. Same intent,
   strictly less fixture coincidence.
3. **Case-sensitivity migration is a non-issue for these scenarios.**
   `VirtualStorage` is case-sensitive where the archived `FakeFs` emulated
   case-insensitive FAT — but no ported scenario relies on storage-level case
   folding: the `find(b"book.txt")` assertion is cache-internal (FNV-1a over
   the lower-cased name), so it ports unchanged. Fixture names are 8.3
   uppercase throughout.
4. **`Rig::press` returns `()` on the host** (the archived rig returned
   `Transition`); every ported scenario ignores the return value, so nothing
   changes.
5. **SettingsApp inclusion reverses a deliberate host boundary.**
   `host/src/apps.rs` documents the other apps as firmware-only; the settings
   app specifically is pure UI + config (no esp/display/SD board deps) and the
   archived host build compiled it from the same source path. The implementer
   should still expect the first compile of `pub mod settings` to surface any
   drift between the host ui/fonts stand-ins and `src/ui/mod.rs` (none found
   by inspection; listed in §1).
6. **Oracle-vs-requirements mismatches: none found.** Every oracle scenario in
   the two files is hardware-independent and portable; the three exclusions are
   exactly the categories the brief names (storage failure injection, C61
   session hardware, Wi-Fi beyond the config bytes).
7. **`gen` is a reserved keyword under edition 2024** — the archived record
   builder already spelled it `gen_`; the port uses `generation`. Noted only
   because the same trap will hit future ports of older test code.

## Status

Test authoring complete. The two suites compile-block exactly on the 20
contract items of §1; the implementer's acceptance run is:

```
scripts/host-test.sh --test reader_settings --test reader_bookmarks
```

with 32 expected passes after the contract is built.
