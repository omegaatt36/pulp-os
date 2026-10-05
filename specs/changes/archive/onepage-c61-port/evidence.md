# OnePage C61 — Archive evidence (2026-10-05)

Result: **0 / 23 fully satisfy the archive evidence gate; 23 / 23 open.** All 23 requirements in spec.md are missing provenance tags and their original pre-implementation red-proof. Existing runnable tests and final validation are recorded below; they are useful acceptance evidence, but cannot recreate the missing history.

Original T1–T14 and the three corrective tasks are checked; corrective tests and independent review pass. The full final acceptance result is recorded below. Proposal has no Risk or Assumptions sections. progress.md describes fresh per-task subagents but does not preserve a test-author/implementer split or per-requirement expected-value derivation. Only T13 weakening gate is explicitly recorded: baseline.md line 1464 reports +1 UTF-8 test, no removed/loosened assertion, no skip, no widened tolerance and no new mock; two equivalent mutants removed. Other tasks' weakening evidence is MISSING.

Paths and commands are repo relative. Commands listed are from real files and historical reports; the audit mapping was prepared before the corrective acceptance run. Some test names still contain spec IDs despite the spec-apply hygiene rule. Do not invent tags or substitute a mutation kill for original red-proof.

| R-ID | Provenance | Test layer | Test path/name | Replay command | Red-proof / historical evidence |
|---|---|---|---|---|---|
| R1 | **MISSING in spec.md** | build | `Cargo.toml; rust-toolchain.toml; src/bin/main_c61.rs` | `cargo build-c61 --locked` | **Original red-proof MISSING.** baseline §1–9, §41; old build failures baseline lines 101/105 are environment diagnosis, not a recorded new-test pre-implementation failure. |
| R2 | **MISSING in spec.md** | build negative | `scripts/check-board-selection.sh` | `scripts/check-board-selection.sh` | **Original red-proof MISSING.** baseline §9 build-selection checks; missing/multiple board failure is intended final behavior, not recorded pre-implementation red. |
| R3 | **MISSING in spec.md** | build | `Cargo.toml; src/bin/main.rs` | `cargo build-x4 --locked` | **Original red-proof MISSING.** baseline §41; pre-port build equivalence/control is not red-proof. |
| R4 | **MISSING in spec.md** | artifact/dependency boundary | `scripts/check-offline-boundary.sh` | `scripts/check-offline-boundary.sh` | **Original red-proof MISSING.** baseline §10–12 |
| R5 | **MISSING in spec.md** | menu/artifact boundary | `scripts/check-offline-boundary.sh` | `scripts/check-offline-boundary.sh` | **Original red-proof MISSING.** baseline §10–12 |
| R6 | **MISSING in spec.md** | host state/trace | `board-logic/src/power.rs::r6_power_cycle_is_low_delay_high_delay_with_bsp_timing; board-logic/src/sd.rs::r6_probe_runs_only_after_the_power_cycle_events` | `scripts/test-board-logic.sh r6_` | **Original red-proof MISSING.** baseline §13–18 |
| R7 | **MISSING in spec.md** | host state/trace | `board-logic/src/power.rs::r7_rail_stays_high_from_sd_init_until_shutdown_cut` | `scripts/test-board-logic.sh r7_` | **Original red-proof MISSING.** baseline §13–18 |
| R8 | **MISSING in spec.md** | host SPI trace / artifact | `board-logic/src/spi.rs::r8_alternating_epd_and_sd_never_overlap; scripts/check-c61-no-c3-raw-gpio.sh` | `scripts/test-board-logic.sh r8_; scripts/run-software-acceptance.sh` | **Original red-proof MISSING.** baseline §16–18 |
| R9 | **MISSING in spec.md** | host SD fault/state | `board-logic/src/sd.rs::r9_no_card_is_a_recoverable_error_not_a_panic; r9_remove_then_reinsert_remounts_through_a_new_permit` | `scripts/test-board-logic.sh r9_` | **Original red-proof MISSING.** baseline §16–18; these tests did not detect actual SPI hot-insert missing prelude; corrective red will be new evidence only. |
| R10 | **MISSING in spec.md** | host wire trace/pixel mapping | `board-logic/src/ssd1677.rs::r10_portrait_four_corner_pixels_land_at_known_stream_offsets; r10_full_refresh_command_trace` | `scripts/test-board-logic.sh r10_` | **Original red-proof MISSING.** baseline §19–21; post-implementation mutation failures are not historical red. |
| R11 | **MISSING in spec.md** | host timeout/failure | `board-logic/src/ssd1677.rs::r11_stuck_busy_times_out_within_the_limit_and_does_not_hang; board-logic/src/lifecycle.rs::r11_refresh_stuck_panel_gives_up_after_exactly_two_attempts` | `scripts/test-board-logic.sh r11_` | **Original red-proof MISSING.** baseline §19–21, §40–41; mutation failures only. |
| R12 | **MISSING in spec.md** | host decoder/input trace | `board-logic/src/keys.rs::r12_ladder_window_edges_are_inclusive_and_one_mv_outside_is_none; r12_gpio_decode_priority_wake_prev_next_over_all_combinations` | `scripts/test-board-logic.sh r12_; scripts/check-x4-input-trace.sh` | **Original red-proof MISSING.** baseline §22–24 |
| R13 | **MISSING in spec.md** | host grace timing | `board-logic/src/keys.rs::r13_grace_boundary_one_microsecond_before_and_exactly_at_2500ms` | `scripts/test-board-logic.sh r13_` | **Original red-proof MISSING.** baseline §22–24 |
| R14 | **MISSING in spec.md** | host budget / ELF | `board-logic/src/memory.rs::r14_one_byte_over_the_class_limit_is_refused; scripts/report-c61-memory.sh` | `scripts/test-board-logic.sh r14_; scripts/run-software-acceptance.sh` | **Original red-proof MISSING.** baseline §25–30, §41, §43 |
| R15 | **MISSING in spec.md** | host memory policy / ELF | `board-logic/src/memory.rs::r15_dma_isr_runtime_never_allow_psram; r15_memory_map_matches_memory_x; scripts/report-c61-memory.sh` | `scripts/test-board-logic.sh r15_; scripts/run-software-acceptance.sh` | **Original red-proof MISSING.** baseline §25–30, §41, §43 |
| R16 | **MISSING in spec.md** | host battery sequence | `board-logic/src/battery.rs::r16_sequence_is_enable_pause_settle_16_reads_resume; r16_adc_failure_at_every_index_stops_reading_and_still_resumes` | `scripts/test-board-logic.sh r16_; scripts/check-x4-battery-equiv.sh` | **Original red-proof MISSING.** baseline §31–33; shared HAL ADC pending-state bug not exercised. |
| R17 | **MISSING in spec.md** | host polarity/source | `board-logic/src/usb.rs::r17_board_polarity_is_active_low_per_bsp_code_and_schematic; r17_mapping_table_for_both_polarities` | `scripts/test-board-logic.sh r17_` | **Original red-proof MISSING.** baseline §31–33; physical polarity still unverified. |
| R18 | **MISSING in spec.md** | host persistence/sleep sequence | `board-logic/src/sleep.rs::r18_save_is_the_first_event_and_runs_with_sd_active_and_rail_high; board-logic/src/session.rs` | `scripts/test-board-logic.sh r18_` | **Original red-proof MISSING.** baseline §34–39 |
| R19 | **MISSING in spec.md** | host shutdown/wake sequence | `board-logic/src/sleep.rs::r19_full_sequence_trace_in_bsp_order; r19_wake_config_failure_aborts_before_any_irreversible_step` | `scripts/test-board-logic.sh r19_` | **Original red-proof MISSING.** baseline §37–39; arm wake earlier than BSP is explicitly disclosed. |
| R20 | **MISSING in spec.md** | host persistent data + app restoration | `board-logic/src/session.rs::r20_round_trip_restores_the_same_position; board-logic/src/lifecycle.rs; scripts/reader-regression/os/tests/session_restore.rs::txt_session_restores_the_same_page_and_text` | `scripts/test-board-logic.sh r20_; scripts/check-reader-regression.sh tree` | **Original red-proof MISSING.** baseline §34–41. Additional genuine pre-corrective-implementation failure retained at /tmp/c61-session-review/repro.log: txt_session_restores_the_same_page_and_text FAILED, left page 0 offset 0 / right page 7 offset 4553; acceptance-review.md lines 6–18. This proves the MUST fix red, not initial port red. |
| R21 | **MISSING in spec.md** | host real-source reader regression/golden | `scripts/reader-regression/os/tests/{pagination,navigation,settings,bookmarks,smol_epub}.rs; scripts/reader-regression/os/src/golden.rs` | `scripts/check-reader-regression.sh both; scripts/check-reader-mutants.sh` | **Original red-proof MISSING.** baseline §43; UTF-8 mutant R16 (a mutant ID, not requirement R16) killed after adding a test is post-implementation mutation evidence, not original red-proof. |
| R22 | **MISSING in spec.md** | delivery/build/memory evidence | `scripts/run-software-acceptance.sh; baseline.md; acceptance-review.md` | `scripts/run-software-acceptance.sh --with-mutants` | **Original red-proof MISSING.** baseline §43; /tmp/c61-acceptance-review.log contains full prior run, acceptance-review.md lines 38–50 summarize it. |
| R23 | **MISSING in spec.md** | delivery status text | `bringup.md; bringup-record.md; scripts/run-software-acceptance.sh UNVERIFIED output` | `scripts/run-software-acceptance.sh --skip-builds` | **Original red-proof MISSING.** progress T14; bringup-record.md all hardware marked 未驗; acceptance-review.md line 48. |

## Surviving assumptions / decisions

There are zero literal `[assumed]` tags because **no provenance tags exist at all**. This is a missing ledger, not evidence that all requirements were confirmed. Do not reclassify original requirements from prose after the fact.

Five implementation choices explicitly await the user in baseline.md §42 (lines 1412–1418):

1. Sleep only via idle timeout, no manual sleep key; render `(sleep)` first and full refresh again if aborted.
2. Reader long ENTER opens quick menu; C61 Files deletion unavailable; future bookmark long Select needs another key.
3. Successful restore keeps session; unsuccessful apply clears it and normal boots.
4. No uninitialized buffer allocation path; smol-epub internals are not moved to PSRAM.
5. No boot console; always full refresh including requests marked Partial.

Other disclosed implementation/source uncertainty: wake arming moved before all irreversible shutdown actions relative to BSP (§37b); USB ActiveLow chosen from BSP implementation and schematic with README conflict (§31–33); card-detect polarity and PSRAM/flash clocks need hardware confirmation. All physical checks remain 未驗, which itself satisfies the delivery status intent R23 but does not fill missing provenance/red history.

## Accessible artifacts inspected

- `spec.md`, `tasks.md`, `proposal.md`, `progress.md`, `baseline.md`, `acceptance-review.md`, bring-up files.
- Real board-logic test names and acceptance script replay entry points.
- `/private/tmp/c61-session-review/repro.log` and `session_restore.original`: genuine regression failure before corrective changes, separate from original port red-proof.
- `/private/tmp/c61-acceptance-review.log`: previous acceptance result (referenced by permanent review).
- Scoped temp listing found these existing C61 artifacts; no original per-task historical red report was located. This is limited to accessible repository/change files and identified temporary artifacts; inaccessible prior agent conversation history cannot be reconstructed.

The supplied archive skill explicitly says: “covered by tests” with no path, no runnable command, or no red-proof is not evidence. Missing history requires reporting an open item; it cannot be fabricated by re-running current code or rolling it back after the fact.

## Corrective red-proof (2026-10-05)

These are genuine failures captured by independent test authors before the corrective implementation. They do not replace missing red-proof from the original port. Durable test-author, implementer and weakening-gate reports are in [corrective-validation.md](corrective-validation.md).

| Requirement scope | Test layer / path | Replay command | Actual pre-fix failure | Result after fix |
|---|---|---|---|---|
| R20 | Actual Reader lifecycle, `scripts/reader-regression/os/tests/session_restore.rs` | `scripts/check-reader-regression.sh tree` | 4 passed / 6 failed; TXT page 0 vs 7; stale bookmark page 2 vs 7; EPUB chapter0/page0 vs chapter2/page3; chapter-start offset1993 vs0 | Focused session suite: 12 passed / 0 failed |
| R20 suspended collection | Same file, `suspended_restored_reader_retains_position_before_background_loading` | `scripts/check-reader-regression.sh tree --filter suspended_restored` | byte_offset 0 vs 1993 before background loading; 1 passed / 1 failed | Both suspended cases pass; font-before-resume preventive test passed before fix and is not red-proof |
| R8/R9 SD preparation | Actual `kernel/src/board_c61/spi.rs`, `scripts/c61-adapter-regression/tests.rs::each_sd_probe_prepares_clock_deselects_and_flushes_before_cmd0` | `bash scripts/check-c61-adapter-regression.sh` | Actual old slow_down compiled, then failed: each probe needs at least 74 clocks, including hot insertion | Adapter suite: 2 passed / 0 failed; CardProbe call and failure short-circuit source-reviewed |
| R12/R16 shared ADC | Actual `kernel/src/board_c61/adc.rs`, `scripts/c61-adapter-regression/tests.rs::shared_adc_recovers_on_next_key_poll_without_channel_mixing` | `bash scripts/check-c61-adapter-regression.sh` | Actual old adapter compiled, then returned None vs expected Some(1200) after pending battery completion | Adapter suite: 2 passed / 0 failed |

No corrective assertion removed/loosened, no new ignore/skip, no widened tolerance or narrowed case table. Old head-only sleep save remains to run the historical baseline; current tree sleep no longer adds an unimplemented bookmark-save side effect. ADC/SPI harness calls were mechanically migrated to new APIs without changing behavioral assertions.

Coverage limits: host Reader tests do not execute AppManager directly. Host adapter tests execute real ADC/SPI adapters against a stateful HAL seam; they exercise write failure and flush ordering, but not flush failure or CardProbe itself. These production call paths were source-reviewed and target-built. Hardware remains unverified.

## Final corrective acceptance result

Command: `scripts/run-software-acceptance.sh --with-mutants`
Date: 2026-10-05, current macOS checkout, pre-archive paths. Exit 0; all 16 stages pass. Full raw log `/tmp/c61-corrective-acceptance.log` is temporary; durable results follow.

| Check | Actual result |
|---|---|
| HAL-free board logic | 321 passed / 0 failed / 0 ignored |
| Actual C61 ADC/SPI adapter harness | 2 passed / 0 failed / 0 ignored |
| Reader regression | Pre-port 83 passed; current tree 90 passed (including 12 session tests) |
| Golden trace | Both 3105 lines, SHA256 `8cb6e31a70a32cdc05d30fab45ee0c7983da8cde6a55ba8bb45a0540f8d3f454` |
| Reader mutation checks | 16 run / 16 killed / 0 survived; each log reports actual named assertion failures, not harness errors |
| Release build matrix | X4 offline, X4 wifi, C61 full and boot all linked |
| Selection / boundaries / equivalence | Board-selection errors, offline radio boundary, C61 no-C3-register check, X4 input/display/battery equivalence all pass |
| C61 memory budget | statics 178032 B, stack 78592 B, 29440 B above 48 KiB minimum |
| Format / diff hygiene | cargo fmt check and git diff --check pass; corrective Rust diff adds no spec IDs |
| Independent review | APPROVE; no surviving MUST |
| Physical hardware | All unverified; bring-up status unchanged |

Historical-evidence result remains **0 complete / 23 open**: corrected software and runnable checks are validated, but original provenance and original pre-implementation red-proof remain unavailable. The user explicitly authorized archiving with these gaps retained; the archive move is complete.

## Explicit archive exception

On 2026-10-05, after the assistant disclosed 23 requirements missing original provenance/red-proof and five decisions still marked unconfirmed, the user replied: **「可以，封存吧」**. This authorizes the requested exception to spec-archive's missing-evidence stop gate. It does not supply the missing history, confirm each implementation decision, or verify physical hardware.

Archived path: `specs/changes/archive/onepage-c61-port/`. All 14 original tasks and 3 corrective tasks are checked. Software acceptance and independent review pass; the historical evidence table remains 0 complete / 23 open. The five decisions and all hardware 未驗 statuses are retained. No living spec was promoted.
