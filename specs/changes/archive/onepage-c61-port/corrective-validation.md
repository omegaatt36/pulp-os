# MUST corrective validation — 2026-10-05

This records corrective-fix evidence, not reconstructed original spec-apply history.


## Independent session test author

# Session restoration test-author report

Scope: ONLY scripts/reader-regression/os/tests/session_restore.rs. No production implementation edited.

Requirement: R20 (spec has no provenance tag). R18 lifecycle behavior is used to model sleep accurately: scheduler flushes dirty bookmarks but does not save current ReaderApp position separately.

Oracle: requirement states valid persisted state restores reading position. Expected chapter/page are derived from explicit navigation (TXT seven Next presses; EPUB two NextJump and three Next). Equality of text/offset/pixels compares pre-sleep observable reading state to post-restart observable state, which is the requirement invariant; no expected values were generated from broken restart behavior. Chapter-start requires page/offset zero. One-shot case asserts normal open resumes the previously saved bookmark after restored state was consumed.

Coverage: existing TXT/EPUB same-position tests now run without a pre-sleep bookmark write; new TXT/EPUB stale-bookmark precedence, EPUB chapter-start offset zero, and one-shot normal reopen cases. Existing corruption/fallback and independent-bookmark assertions retained.

Test layer limitation: AppManager is excluded from this existing host harness, which has placeholder RtcSession and no Home/Files/display bindings. Tests exercise real ReaderApp restore_state → on_enter → background in actual manager order, real SD session encode/write/read/restore, and real bookmark loading. collect/apply field mapping remains the harness's existing mapping; it is not complete C61 AppManager/scheduler integration coverage. Suspended Reader beneath Settings does not receive on_enter in current AppManager::apply_session; review production handling separately.

## Red replay

Preparation: READER_REG_WORKDIR=/tmp/c61-session-fix-red scripts/check-reader-regression.sh tree --filter session prepared the harness but unconditional cargo fetch failed due DNS. That is NOT red-proof. A first cargo call from repository cwd picked embedded target configuration and failed compile; also NOT red-proof.

Actual red command, from /tmp/c61-session-fix-red/tree:

```
unset CARGO_BUILD_TARGET RUSTFLAGS
cargo +nightly-2026-09-22 test --offline -p pulp-os-host --features board-x4,tree --test session_restore
```

Exit 101 before production fix; 4 passed, 6 failed, 0 ignored. Raw output: /tmp/c61-session-fix-red-output.log. Failure excerpt (line payloads shortened after positional fields):

```
---- txt_session_restores_the_same_page_and_text stdout ----
thread 'txt_session_restores_the_same_page_and_text' (5817790) panicked at os/tests/session_restore.rs:152:5:
assertion `left == right` failed
  left: Pos { chapter: 0, page: 0, offset: 0
 right: Pos { chapter: 0, page: 7, offset: 4553
---- restored_session_is_consumed_before_a_normal_reopen stdout ----
thread 'restored_session_is_consumed_before_a_normal_reopen' (5817787) panicked at os/tests/session_restore.rs:247:5:
assertion `left == right` failed
  left: Pos { chapter: 0, page: 2, offset: 1212
 right: Pos { chapter: 0, page: 7, offset: 4553
---- txt_session_position_takes_precedence_over_an_older_bookmark stdout ----
thread 'txt_session_position_takes_precedence_over_an_older_bookmark' (5817789) panicked at os/tests/session_restore.rs:188:5:
assertion `left == right` failed: the valid session is newer than the bookmark
  left: Pos { chapter: 0, page: 2, offset: 1212
 right: Pos { chapter: 0, page: 7, offset: 4553
---- epub_session_restores_chapter_page_and_text stdout ----
thread 'epub_session_restores_chapter_page_and_text' (5817786) panicked at os/tests/session_restore.rs:167:5:
assertion `left == right` failed
  left: Pos { chapter: 0, page: 0, offset: 0
 right: Pos { chapter: 2, page: 3, offset: 1993
---- epub_session_position_takes_precedence_over_an_older_bookmark stdout ----
thread 'epub_session_position_takes_precedence_over_an_older_bookmark' (5817785) panicked at os/tests/session_restore.rs:209:5:
assertion `left == right` failed: the valid session is newer than the bookmark
  left: Pos { chapter: 0, page: 2, offset: 1231
 right: Pos { chapter: 2, page: 3, offset: 1993
---- epub_session_at_chapter_start_overrides_a_later_bookmark stdout ----
thread 'epub_session_at_chapter_start_overrides_a_later_bookmark' (5817784) panicked at os/tests/session_restore.rs:227:5:
assertion `left == right` failed
  left: Pos { chapter: 2, page: 3, offset: 1993
 right: Pos { chapter: 2, page: 0, offset: 0
test result: FAILED. 4 passed; 6 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

## Weakening gate

No assertion removed or loosened; no ignores/skips/tolerance/case narrowing; no mocks or stubs introduced. New cases cfg(tree) because pre-port X4 behavior is baseline, and original baseline sleep bookmark write remains cfg(not(tree)). Two fallback tests explicitly save their bookmark instead of relying on the removed sleep side effect. Source spec-ID comment removed; test names use behavior.

## X4 pre-port compatibility

Existing head harness command (cwd $TMPDIR/pulp-reader-regression/head):
`unset CARGO_BUILD_TARGET RUSTFLAGS; cargo +nightly-2026-09-22 test --offline -p pulp-os-host --features board-x4 --test session_restore`
Exit 0, 6 passed, 0 failed. Raw output /tmp/c61-session-fix-head-output.log.

## Suspended restoration follow-up

Added two tree-only tests exercising actual ReaderApp restore_state → on_enter → on_suspend, with no background load:

- `suspended_restored_reader_retains_position_before_background_loading`: R20 invariant requires collection before another sleep to preserve persisted byte offset/chapter. Actual red before follow-up fix: chapter matches, byte_offset left `0` versus expected `1993`.
- `suspended_restored_epub_initializes_when_font_changes_before_resume`: actual public `set_book_font_size(3)` then on_resume/background. Already PASSES before follow-up fix. No red-proof exists for the alleged NeedInit skip through this public setter path: setter updates applied_font_idx through apply_font_metrics, hence on_resume font_changed is false. Retain as preventive behavior coverage, not red-proof. Expected page contains persisted byte (`offset[page] <= byte < next offset`) because changing font legitimately changes page boundaries.

Command from /tmp/c61-session-fix-red/tree:
`unset CARGO_BUILD_TARGET RUSTFLAGS; cargo +nightly-2026-09-22 test --offline -p pulp-os-host --features board-x4,tree --test session_restore suspended_restored`
Exit 101, 1 passed / 1 failed. Raw log /tmp/c61-suspended-red.log. No production changes. No test weakening; no artificial field mutation or fake manager logic introduced.


## Session implementation

# Session restore MUST fix

## Root cause and implementation

`ReaderApp::restore_state` previously wrote chapter and an optional positive offset, but `on_enter` then reset both. `NeedBookmark` subsequently restored an older bookmark. Zero was also treated as no restoration, so a session at chapter start could never outrank a later bookmark.

Production ownership: `src/apps/reader/mod.rs`, `src/apps/manager.rs` only. Added a one-shot `session_position: Option<(u16, u32)>`, bound to the restored filename and consumed by `on_enter`. The initialized chapter and `Some(offset)` survive reset, including offset zero; bookmark loading is skipped while the restored offset is pending. `on_exit` clears pending restoration and normal reopen uses the bookmark. No extra bookmark save was introduced.

`AppManager::apply_session` now initializes Reader also when suspended beneath Settings, then suspends it. Previously that stack branch never called `on_enter`, leaving the fresh reader without its normal opening lifecycle. Returning from Settings follows the existing background initialization.

## Verification

Prepared harness directory: `/tmp/c61-session-fix-red/tree`; includes repository production code via symlink.

- Pre-fix red provided by independent test-author worker: `/tmp/c61-session-fix-red-output.log`, six failing session cases. Implementer did not edit tests.
- `cd /tmp/c61-session-fix-red/tree && cargo +nightly-2026-09-22 test --offline -p pulp-os-host --features board-x4,tree --test session_restore`: exit 0, 10 passed, 0 failed/ignored.
- `cd /tmp/c61-session-fix-red/tree && cargo +nightly-2026-09-22 test --offline -p pulp-os-host --features board-x4,tree`: exit 0, 88 named tests passed, 0 failed/ignored.
- `cd /tmp/c61-session-fix-red/tree && cargo +nightly-2026-09-22 run -q --offline -p pulp-os-host --features board-x4,tree --bin golden > /tmp/c61-session-fix-golden.txt`: exit 0; SHA256 `8cb6e31a70a32cdc05d30fab45ee0c7983da8cde6a55ba8bb45a0540f8d3f454`, identical to baseline pin.
- Repository `cargo +nightly-2026-09-22 fmt --check`: exit 0.

## Oracle and weakening gate

Tests were authored independently before implementation and treated as read-only. Expected session position derives from the session contract: state collected before sleep must reopen at the same text position even without saving a bookmark; newer session position outranks an older bookmark, including zero offset; normal reopen uses existing bookmark after one-shot restore. No expected values were obtained by running this implementation. No test edit, assertion removal/loosening, skip, tolerance expansion, narrowed case table, or fake substitution by this implementer.

Limitation: current reader harness exercises actual Reader lifecycle but reproduces the manager/session glue. Suspended Reader manager branch was reviewed against real manager and reader sources, not executed by host harness. Full target builds are left to parent final verification.


## Independent ADC/SD test author

# C61 adapter regression report

## Oracle / test scope

Tests compile actual `kernel/src/board_c61/adc.rs` and `spi.rs` by `#[path]`; only esp-hal is replaced by a compact stateful host seam. critical-section, static_cell, embedded-hal and SpiArbiter are actual dependencies.

SD expected order derives from pinned embedded-sdmmc-rs commit 0bf1254 `src/sdcard/spi.rs:17–24`: caller supplies at least 74 clocks with SD CS high; SD probe <=400 kHz from actual board contract `board-logic/src/spi.rs`. Test expects 400 kHz then both selects high, ten 0xFF bytes (80 clocks), flush, every retry, and propagates prelude write failure. Expected values were not obtained by running the implementation.

ADC oracle derives from esp-hal 1.2.0 `src/analog/adc/riscv.rs:365–418`: different active channel returns WouldBlock; only completion of same channel clears pending conversion. The seam preserves this state. Test expects a timed-out battery conversion to be drained on the next key poll, followed by front conversion; persistent stall returns None within original 20,000 iteration bound and never mixes channels. Front=1200 and battery=1800 are deliberately distinct test stimulus, not observed product readings. No claim of real hardware timing/calibration.

## Pre-fix red proof

`bash scripts/check-c61-adapter-regression.sh` exit 101, actual unchanged production files compiled successfully. Full output `/tmp/c61-hardware-red.log`.

- `each_sd_probe_prepares_clock_deselects_and_flushes_before_cmd0`: panic `each probe needs at least 74 clocks, including hot insertion`. Calls original `slow_down()` so failure is behavioral, not compile error for missing new method.
- `shared_adc_recovers_on_next_key_poll_without_channel_mixing`: expected Some(1200), actual None at first front poll after battery conversion becomes ready.
- Summary `0 passed; 2 failed; 0 ignored`.

## Implementation brief

ADC: own both calibrated pins with shared ADC and pending-channel state. Provide read_front_mv/read_battery_mv; bounded drain of pending same channel before any new channel conversion, using public HAL API only. On persistent stall return None without channel mix, conversion remain pending. No raw registers / unsafe peripheral steal.

SD: SpiControl.prepare_sd_probe must set 400kHz, deselect both, write >=10 FF bytes, flush, propagate failure; call on every startup/retry/insert attempt in both scheduler and c61_boot. Do not attempt bring_up after preparation failure; restore operating clock after attempts as appropriate. Full firmware build verifies public HAL compatibility.

## Weakening gate

New tests were written independently before production edits. Assertions have not been removed/loosened, no ignores/skips, no wider tolerance, no fewer cases, no prior real call replaced by fake. Updating old read_mv/slow_down calls to newly named APIs is mechanical API adaptation; preserve every behavioral assertion. Adapter calls still import actual production source. Host shim is test seam from introduction, not replacement of prior exercised call path.


## ADC/SD implementation

# C61 hardware adapter MUST fixes

## Actual dependency evidence

Read pinned esp-hal 1.2.0 analog/adc/riscv.rs:350–418: a different active channel yields WouldBlock, and only a successful original-channel read resets/clears state. Read pinned embedded-sdmmc 0bf1254 sdcard/spi.rs:17–24: the caller must supply >=74 clocks at 400 kHz with SD deselected.

## Fixes

ADC shared cell now owns the ADC and both calibrated pins plus pending channel. On timeout it retains that channel. Every next measurement boundedly drains it through public HAL read_oneshot before starting a fresh requested-channel conversion; completed pending data is discarded even for the same channel, preserving the current charger measurement window. Persistent stalls return None boundedly; no unsafe stealing or raw reset. Keys/battery binaries use read_front_mv/read_battery_mv via shared handle.

SD shared prepare_sd_probe changes to 400 kHz, drives both selects high, writes ten FF bytes and flushes, all under one shared-bus lock. Clock/write/flush errors propagate. Every CardProbe.probe invokes preparation before num_bytes; failure returns None without card commands. This central seam covers startup, every retry, scheduler insertion and demo insertion automatically. Removed obsolete warning-only scheduler slow_down and ignored boot-only prelude. Original operating-clock restoration in callers retained.

## Verification and evidence scope

Independent test author pre-fix red: /tmp/c61-hardware-red.log and /tmp/c61-hardware-tests-report.md. Both real adapter source tests compiled before failing behaviorally (not missing APIs). Expected values derive from dependency/state contracts, not implementation output.

Fresh bash scripts/check-c61-adapter-regression.sh: 2 passed / 0 failed / 0 ignored. Actual adc.rs/spi.rs compile into host harness; shim preserves active ADC channel. ADC timeout/recovery/persistent stalls/no channel mix and SD repeated preparation/order/clocks/flush/write failure pass.

cargo build-c61 --locked: exit 0, release full + bring-up images built using actual esp-hal/embedded-sdmmc. Final repeated build log /tmp/c61-hardware-build.log.

Weakening gate: implementer did not edit regression assertions or harness files. Root mechanically migrated API calls after independent red. No skips, removed assertions, widened tolerance, narrowed cases, or call-path replacement. Existing full acceptance is root-owned. Adapter harness does not exercise sd.rs/CardProbe directly; automatic preparation call and early error return are actual source-reviewed, with hardware insertion/ADC electrical timing still unverified. This report does not claim missing original historical red-proof for unrelated requirements.


## Suspended position follow-up implementation

The independent author captured byte_offset 0 vs expected 1993 before this change. Root updated ReaderApp::byte_offset to return a pending session_position or restore_offset until loading consumes it. No test assertions changed.

Fresh command in /tmp/c61-session-fix-red/tree:
`cargo +nightly-2026-09-22 test --offline -p pulp-os-host --features board-x4,tree --test session_restore`
Exit 0; 12 passed, 0 failed, 0 ignored. Raw log /tmp/c61-session-final-green.log.

The font-before-resume finding was independently disproved through the public set_book_font_size API; its preventive test passed before this follow-up and is not red-proof. No production font change was made.

## Final independent review

Spec compliance APPROVE / code quality APPROVE, no outstanding MUST. Reviewer read actual final Reader/manager, ADC/SPI/SD consumers, harness and acceptance wiring. Previously alleged font-change defect retracted after checking public setter side effect.

Coverage limits: actual Reader lifecycle is host-executed, but AppManager is source-reviewed rather than host-executed. Actual adc.rs/spi.rs are host-executed against a stateful HAL seam; CardProbe caller and flush-error propagation are source-reviewed plus target-built. Adapter tests inject write failure and observe flush ordering, not flush failure. All physical hardware remains unverified.

## Full final acceptance

`scripts/run-software-acceptance.sh --with-mutants`: exit 0, all 16 stages pass. Board logic 321 tests; actual C61 adapter 2 tests; reader head83/tree90 tests; golden SHA unchanged; 16 mutants killed; X4 offline/wifi and C61 full/boot release link; boundaries/equivalence, memory budget and fmt all pass. C61 statics178032 B, stack78592 B, headroom29440 B above48KiB. Permanent summary: [evidence.md](evidence.md).
