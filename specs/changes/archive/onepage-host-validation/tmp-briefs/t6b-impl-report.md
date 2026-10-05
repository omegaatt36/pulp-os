# T6b implementer report — settings & bookmarks contract built

**Status: BLOCKED on exactly one test** (`reader_bookmarks::flush_writes_the_baseline_record_layout`),
which asserts something the production code makes impossible — a porting omission
in the test itself (evidence below). Per the brief I did not edit it; the main
session decides. Everything else the brief requires is done and green.

---

## 1. Files touched (host/src only — production src/**, kernel/** untouched)

| File | Change |
|---|---|
| `host/src/apps.rs` | `#[path = "../../src/apps/settings.rs"] pub mod settings;` (with `#[allow(dead_code, unused_imports)]`) + header comment updated (settings is pure UI + config, not firmware-only). The production `src/apps/settings.rs` compiled unchanged — no ui/fonts drift surfaced. |
| `host/src/kernel.rs` | `Kernel::bookmarks_flush()` → `self.bm_cache.flush(&self.sd)` — the archived-harness form; the dirty gate is `BookmarkCache::flush`'s own first line, matching the scheduler housekeeping at `kernel/src/kernel/scheduler.rs:282-283` (`if BOOKMARK_FLUSH_DUE && is_dirty() { flush(&self.sd) }` — the timer half is host-N/A, the rig calls flush explicitly). |
| `host/src/apps/probe.rs` | `probe::theme_idx(a: &ReaderApp) -> u8 { a.reading_theme_idx }` — read-only view of the `pub(super)` field, same forwarding pattern as the rest of the file. |
| `host/src/reader.rs` | 8 `Rig` methods (`save_position`, `bookmark_save`, `bookmark_remove`, `bookmark_find`, `bookmark_list_into`, `bookmarks_flush`, `into_storage`, `theme_idx`) + `SettingsRig` (10 methods), all forwarding to the real `ReaderApp` / `SettingsApp` / kernel `BookmarkCache` / `KernelHandle`. No logic beyond forwarding; no settings/bookmark behaviour lives in pulp-host. |

All 20 contract items of the author report §1 are implemented with the exact
signatures listed there. Formatting: `rustfmt --edition 2024` run per touched
file; `rustfmt --check` on all four is clean. No git commits.

## 2. Gate results

| Gate | Expected | Actual |
|---|---|---|
| `scripts/host-test.sh --test reader_settings` | 18 | **18 passed / 0 failed** |
| `scripts/host-test.sh --test reader_bookmarks` | 14 | **13 passed / 1 failed** (the blocked test) |
| `scripts/host-test.sh` (full, `--no-fail-fast`) | 227 (195 baseline + 32) | **226 passed / 1 failed** — lib 1, fixtures 46, paging 24, reader_bookmarks 13+1f, reader_epub 31, reader_settings 18, render 39, storage 36, utf8 18 |
| `scripts/check-host-boundary.sh` | all ok | R1 graph ok (81 packages, no esp-*), R3 `build-x4` ok, R3 `build-c61` ok; **FAIL only** at the host-test step, and the log shows the sole failure is the same blocked test (84 test oks before it) |
| `scripts/check-reader-regression.sh both` | head 83 / tree 90, golden sha = pin | **head 83 passed, tree 90 passed, golden sha256 `8cb6e31a…f454` = pinned value, both variants ok** |

## 3. The blocked test (STOP case — main session decides)

`flush_writes_the_baseline_record_layout` (`host/tests/reader_bookmarks.rs:291-320`)
fails at line 305 (`bkmk(&k).unwrap()` → `None`): after `k.bookmarks_flush()` the
card has no `_PULP/BKMK.BIN`.

Evidence chain (production sources, read-only):

1. The test saves through a `KernelHandle` on a **never-loaded** cache:
   `kernel_with(card())` → `h.bookmark_cache_mut().save(...)` — no
   `bookmarks_load()` call anywhere in the test.
2. `BookmarkCache::save` (`kernel/src/kernel/bookmarks.rs:270-276`): `if
   !self.loaded { log::warn!("...save called before load, ignoring"); return; }`.
   `loaded` is set only by `ensure_loaded`/`force_load` (`:183-211`), reached via
   `Kernel::bookmarks_load`.
3. Sibling test `not_loaded_means_no_lookup_and_no_save` (same file) **pins that
   exact no-op semantics** and is green — no host-side seam can satisfy both.
4. The archived oracle `scripts/reader-regression/os/tests/bookmarks.rs:171-172`
   has `k.bookmarks_load();` immediately after `kernel_with(card());`; the ported
   test dropped that line. Sibling ported test `save_find_update_and_remove`
   (line 249) does call `k.bookmarks_load();` — the omission is inconsistent
   with both the oracle and the sibling.

I verified no implementation choice can fix it without editing the test or
production: auto-loading in `Kernel::new` would break the already-green
`not_loaded_means_no_lookup_and_no_save`; loading inside `bookmarks_flush`
(`ensure_loaded` first) resets the cache and wipes the unsaved writes, still
writing nothing.

**Suggested unblock (one line, main session to apply):** restore the oracle line
`k.bookmarks_load();` after `let mut k = kernel_with(card());` in
`flush_writes_the_baseline_record_layout` (`host/tests/reader_bookmarks.rs:292`).
With that line, the other 13 bookmarks tests are unaffected and the expected
total is 227/227 with the boundary gate fully green. I ran no such edit.

## 4. Mutation program (19 mutations, one at a time, diff-confirmed, reverted)

Each mutation: applied by script to `host/src/**`, `diff` against a pristine
copy confirmed the exact hunk, targeted suite run, revert + `diff -q` confirmed
byte-identical restore. Kill evidence excludes the pre-blocked
`flush_writes_the_baseline_record_layout`.

| # | Mutation | Killed by | Verdict |
|---|---|---|---|
| M1 | `Kernel::bookmarks_flush` → `force_load` (wrong op / reboot loses data) | persisted_bookmarks_survive_a_reboot_exactly; reader_bookmark_round_trip_through_the_card; reader_save_position_does_not_touch_the_session_files | KILLED |
| M2 | `Rig::bookmarks_flush` → no-op | persisted_bookmarks_survive_a_reboot_exactly; reader_bookmark_round_trip_through_the_card; reader_save_position_does_not_touch_the_session_files | KILLED |
| M3 | `Rig::save_position` → no-op | bookmarks_are_per_book; reader_bookmark_round_trip_through_the_card; reader_save_position_does_not_touch_the_session_files | KILLED |
| M4 | `Rig::bookmark_save` → byte_offset/chapter swapped | damaged_bookmark_files_are_tolerated; persisted_bookmarks_survive_a_reboot_exactly | KILLED |
| M5 | `Rig::bookmark_remove` → no-op | persisted_bookmarks_survive_a_reboot_exactly (removed-stays-removed); removing_the_bookmark_reopens_at_the_first_page | KILLED |
| M6 | `Rig::bookmark_find` → always `None` | damaged_bookmark_files_are_tolerated; persisted_bookmarks_survive_a_reboot_exactly | KILLED |
| M7 | `Rig::bookmark_list_into` → always returns 0 | damaged_bookmark_files_are_tolerated (`== 1` count asserts) | KILLED |
| M8 | `Rig::into_storage` → fresh empty card | persisted_bookmarks_survive_a_reboot_exactly; reader_bookmark_round_trip_through_the_card | KILLED |
| M9 | `Rig::theme_idx` → 0 | saved_book_font_and_theme_decide_the_reader_layout | KILLED |
| M10 | `SettingsRig::boot` → no-op | settings_file_is_read_up_to_512_bytes; settings_app_loads_defaults_when_there_is_no_file; settings_app_loads_a_saved_file_and_sanitizes_it; settings_app_edits_steps_clamps_and_saves; saved_book_font_and_theme_decide_the_reader_layout | KILLED |
| M11 | `SettingsRig::press` → forwards as `LongPress` | settings_app_edits_steps_clamps_and_saves (no value changes) | KILLED |
| M12 | `SettingsRig::tick` → no-op | settings_app_edits_steps_clamps_and_saves (`expect("saved")`) | KILLED |
| M13 | `SettingsRig::press` → also runs a background tick (immediate save) | settings_app_edits_steps_clamps_and_saves ("nothing is written until the scheduler's background tick") | KILLED |
| M14 | `SettingsRig::settings` → `SystemSettings::defaults()` | settings_file_is_read_up_to_512_bytes; settings_app_loads_a_saved_file_and_sanitizes_it; settings_app_edits_steps_clamps_and_saves; saved_book_font_and_theme_decide_the_reader_layout | KILLED |
| M15 | `SettingsRig::wifi_ssid` → `String::new()` | settings_app_loads_a_saved_file_and_sanitizes_it (`"home"`) | KILLED |
| M16 | `SettingsRig::file` → always `None` | settings_app_edits_steps_clamps_and_saves; settings_app_keeps_wifi_credentials_when_saving_other_values | KILLED |
| M17 | `SettingsRig::into_storage` → fresh empty card | settings_app_edits_steps_clamps_and_saves (fresh-boot readback) | KILLED |
| M18 | `SettingsRig::new` → adds `bookmarks_load()` (contract deviation: the rig must NOT load) | none — reader_settings 18/18, reader_bookmarks unchanged | **SURVIVED (equivalent)** |
| M19 | `Rig::configure` → no-op (pre-existing propagate seam, brief menu "propagate to reader absent") | saved_book_font_and_theme_decide_the_reader_layout (max_lines/text_w) | KILLED |

**Summary: 18 killed / 1 survived (equivalent).**

### Survived mutants — findings for the ledger

- **M18 (equivalent):** adding a bookmark load to `SettingsRig::new` is
  unobservable by the current suite: no settings test reads bookmark state
  through the SettingsRig, and the load itself is side-effect-free on a card
  without `BKMK.BIN`. The contract explicitly excludes it; the suite does not
  pin that exclusion. Cheap pin if wanted: assert the cache not-loaded / a
  later `Rig` boot still works on a card first booted by a SettingsRig. The
  shipped implementation follows the contract (no load).

### Brief-menu items not expressible at the host seam (production-internal)

Generation bump, eviction pick, record layout, case-folding in `find`, list
order, short-buffer cap (bookmarks.rs), the 512-byte load cap, sanitize clamp
table, edit step direction, keep-wifi-on-save (config.rs / settings.rs) — these
behaviours live in files this batch must not modify; host code is
forwarding-only by contract, so there is no host-side knob to mutate. They are
exercised (not mutated) by the green tests: e.g. case-folded find
(`save_find_update_and_remove`), short-buffer semantics
(`list_is_most_recently_saved_first`), record layout byte order
(`flush_writes_the_baseline_record_layout` — the save/flush half of that test is
blocked only by its missing boot line, the byte assertions themselves are
production-pinned), 512-byte cap (`settings_file_is_read_up_to_512_bytes`),
clamp table (`out_of_range_values_are_clamped_by_sanitize`), keep-wifi-on-save
(`settings_app_keeps_wifi_credentials_when_saving_other_values`).

## 5. Weakening / boundary re-verification

- The two test files match the author's state **byte-for-byte** — I never edited
  anything under `host/tests/` or `scripts/`:
  - `host/tests/reader_settings.rs` sha256 `e9a646a7e0b643649e9499393ad005023647754e676b5557d6438c14533cee69`
  - `host/tests/reader_bookmarks.rs` sha256 `1dad90d2f24e00d8f00eadbad13b012bdeb90acdc2ad93690228b1ebe5f710c7`
- `diff -r /tmp/hv-snap/T6b/scripts scripts` → **identical**.
- `diff -r /tmp/hv-snap/T6b/host host` → exactly: the four `host/src` files
  above + the two author test files. Nothing else.
- Production firmware sources (`src/**`, `kernel/**`): untouched by this batch
  (the pre-existing worktree changes belong to earlier batches).

## 6. For the main session to re-verify

1. The one-line test fix proposed in §3 (or a different resolution of the
   ambiguity) — then re-run the three gates; expected 227/227 + boundary all-ok.
2. M18 survival decision: acceptable as a recorded coverage gap, or add a pin.
3. `reader_save_position_does_not_touch_the_session_files` and
   `persisted_bookmarks_survive_a_reboot_exactly` currently carry the flush /
   reboot kill weight that the blocked test would have shared once unblocked.
