# Evidence — onepage-pre-hardware-hardening

HEAD `3fcbbea9d2f1252afcc4044743f504292792c683`（所有改動為未 commit 的工作樹）；日期 2026-10-08；macOS aarch64。

## `task acceptance` 結果（R15）

- exit **0**；耗時 2:38.26（wall；user 267.62 s）。
- 彙總（`grep -E "^test result" | awk` 加總，與基準同法）：**passed 1394／failed 0／ignored 1**。基準 1366／0／1；+28 = 本 change 新增測試（T2 10、T3 3、T4 2、T5 2、T6 7、T7 4）。
- 唯一 ignored：`qemu_riscv32_executes_target_binary`（`requires qemu-riscv32 (Linux qemu-user) on PATH`）。
- 五個映像（x4、x4-wifi、c61、c61-wifi、c61-partial）build 兩輪皆 `Finished`、log 無 `^error`；`cargo fmt --check` 通過；`cargo test-harness --locked` 通過。
- 完整 log：`/tmp/acceptance-final.log`（**不在 repo 內**，不隨 change 保存）。重跑：`(time task acceptance) > /tmp/acceptance-final.log 2>&1`。
- `git diff harness/tests/goldens` 為空（X4 golden 未變）。R1 指令另於 T11 重跑：`cargo run -p pulp-fontconv --release --target host-tuple --config 'unstable.build-std=["std","test"]' -- bundle` exit 0，`target/cjk-sd/_PULP/FONTS/` 在。
- 工作樹變更：20 個既有檔（+520／−108）＋新增 `fontconv/build.rs`、4 個測試檔、`specs/hardware-records/TEMPLATE.md`、本 change 的 progress／evidence。`target/` 以外無意外產物。

## 逐項證據（`spec-archive` 格式）

`C` = `cargo test-tools`（fontconv）、`H` = `cargo test-harness --locked`、`B` = `cargo test-board-logic`、`U` = `cargo test-host --locked`。證據等級僅四種：host 測試／link-level／文件核對／無法主機驗證。

| R-ID | Provenance | Test layer | Test path / name | Replay command | Red-proof（含預期值來源） |
|---|---|---|---|---|---|
| R1 | `[derived]` | 文件指令實際執行（host） | `README.txt:22`、`specs/changes/archive/onepage-cjk-iansui/install.md:13,44`、`specs/references/hardware-acceptance.md:246`（文件指令本身，無測試檔） | 照文件逐字執行 bundle（與 install.md 的 `pbm-to-png`）；核對：`grep -n 'host-tuple' README.txt specs/changes/archive/onepage-cjk-iansui/install.md specs/references/hardware-acceptance.md` | 有（指令級）：改前原指令 exit 101，`E0463 can't find crate for 'std'`，243 個錯誤；改後 exit 0（T1、T11 各一次）；預期值：需求文字（指令須成功） |
| R2 | `[derived]` | host 測試；另實測 bundle 第一次 built、第二次 cache hit、build.rs 納入輸入後 hit 變 built | `fontconv/tests/build_identity.rs`：`changed_build_identity_changes_cache_key`、`unchanged_manifest_rustc_and_build_identity_keep_cache_key`、`changed_manifest_or_rustc_still_changes_cache_key`、`embedded_build_id_is_a_sha256_digest`、`unchanged_inputs_keep_identity`、`changed_converter_source_changes_identity`、`changed_nested_fontpack_source_changes_identity`、`changed_dependency_lock_changes_identity`、`added_source_file_changes_identity`、`renamed_source_file_changes_identity`；實作 `fontconv/build.rs`、`fontconv/src/bundle.rs` | `cargo test-tools --test build_identity` | 有：測試先寫，編譯失敗（`couldn't find file fontconv/tests/../build.rs`，`BUILD_ID`／`cache_key` 不存在）；預期值：需求文字（全為 `assert_ne!`／`assert_eq!`，無硬編碼雜湊） |
| R3 | `[derived]` | host 測試；對真實 `pulp-os-c61` 手動驗證 exit 2、stdout 空 | `harness/tests/stack_analyzer.rs`：`missing_stack_sizes_exits_nonzero_and_reports_unknown`、`missing_stack_sizes_prints_no_stack_verdict`、`present_stack_sizes_still_analyzes`（手寫最小 ELF32 fixture） | `cargo test-harness --locked --test stack_analyzer` | 有：前兩個失敗（exit 0，印出 worst 0 B／headroom 51888 B／100.0% margin）；第三個本來就過（證明 fixture 有效，屬迴歸保護，無 red 可證）；預期值：需求文字（非零 exit、stderr 含 unknown、stdout 無 Worst／Headroom／margin／%） |
| R4 | `[derived]` | host 測試（依賴圖 feature）；ELF 反組譯：`Printer::write_bytes` 0x14c B → 0xb6 B，SOF 讀取與 ROM UART 分支消失（輔助）。log 實際去向見下方 UNVERIFIED | `harness/tests/esp_println_features.rs`：`c61_console_is_jtag_serial_only`、`x4_console_keeps_original_configuration`；`Cargo.toml` | `cargo test-harness --locked --test esp_println_features` | `c61_console_is_jtag_serial_only` 有（列出 auto／default feature 而失敗）；**X4 守護測試本來就過：迴歸保護，無 red 可證**；預期值：需求文字（C61 只用 `jtag-serial`；X4 維持原設定） |
| R5 | `[derived]` | host 測試（wire trace）；對實機面板無法主機驗證 | `board-logic/src/ssd1677.rs`：`r5_c61_init_booster_is_otp_order`、`r5_x4_init_booster_is_unchanged`、`x4_init_sequence_is_unchanged_golden`、`r10_epd_init_trace_is_bounded_wait_soft_reset_wait_configure`；`harness/tests/goldens/x4_driver_trace.txt` | `cargo test-board-logic ssd1677`；`cargo test-harness --locked --test x4_driver_trace`；`git diff harness/tests/goldens` | `r5_c61_…` 有（left `[174,199,195,192,128]` vs right `[174,199,195,128,192]`），既有 `r10_…` 同樣失敗；**`r5_x4_…` 與 X4 golden 本來就過：迴歸保護，無 red 可證**；預期值：需求文字（`AE C7 C3 80 C0`／`AE C7 C3 C0 80`） |
| R6 | `[derived]` | **link-level**（`% (periodic)` 字串在 c61、c61-wifi 的 .rodata，boot 映像沒有）；每 30 s 真的出現無法主機驗證 | `harness/tests/elf_c61_battery_log.rs`：`full_firmware_c61_images_link_the_periodic_battery_success_log`、`boot_image_keeps_its_own_battery_log_and_has_no_periodic_piece`；實作 `kernel/src/kernel/scheduler_c61.rs` `poll_battery` | `cargo test-harness --locked --test elf_c61_battery_log` | 有：`no '% (periodic)' literal in the image`；預期值：需求文字（成功 log 含 mV 與百分比；失敗訊息不變） |
| R7 | `[assumed]` | host 測試（邏輯）；`key:` 行格式只有源碼核對；mV 準確度無法主機驗證 | `board-logic/src/keys.rs`：`r7_press_and_release_carry_the_mv_of_the_sample_that_caused_them`、`r7_each_ladder_window_event_reports_an_mv_inside_its_own_window`、`r7_queued_release_and_press_of_a_direct_key_change_share_the_new_sample`、`r7_side_key_events_have_no_ladder_mv`、`r7_no_mv_during_grace_or_when_the_adc_fails`；輸出端 `src/bin/c61_boot.rs` `key:` 行 | `cargo test-board-logic r7_` | 有：以回傳 `None` 的 stub，4 個失敗（如 `left: Some((Press(Back), None))`）；五個測試中 1 個在 stub 下本來就過（progress 未指明是哪一個，推測為 `None` 預期的負向案例；該個為迴歸保護，無 red 可證）；預期值：需求文字推導（事件附帶的 mV＝造成該事件那次輪詢的值） |
| R8 | `[derived]` | 文件核對 | `specs/references/hardware-acceptance.md` §0 規則 3、§3 G2 列（:148）、G1 通過條件（:218） | `grep -nE 'USB-only gate' specs/references/hardware-acceptance.md`；狀態詞：`grep -nE '`DEFERRED`\|`SKIPPED`' …` 逐處核對同一組詞 | 無 red 可證（文件，無自動化）；預期值：需求文字 |
| R9 | `[derived]` | 文件核對 | `hardware-acceptance.md` A3d（:190）、A7c-1（:208）、A7c-2（:209）、A7d-1（:210）、A7d-2（:211） | `grep -nE '^\| (A3d\|A7c-1\|A7c-2\|A7d-1\|A7d-2) ' specs/references/hardware-acceptance.md`；log 字串對照 `grep -n 'session saved before power-off\|boot plan\|restore slot' src/bin/c61_boot.rs kernel/src/board_c61/sleep.rs` | 無 red 可證（文件）；預期值：需求文字；log 字串取自程式碼 grep |
| R10 | `[derived]` | 文件核對 | `hardware-acceptance.md` A4d（:196）、P10（:326）、§6「BUSY 逾時的可控注入」列 | `grep -nE '^\| (A4d\|P10) ' specs/references/hardware-acceptance.md`；原因對照 `grep -n 'Pull::None\|is_high' kernel/src/board_c61/epd.rs` | 無 red 可證（文件）；預期值：需求文字；`epd.rs:70,85` 源碼 |
| R11 | `[derived]` | 文件核對 | `hardware-acceptance.md` §5.5（:330）；`specs/hardware-records/TEMPLATE.md`（82 個 ID，頭部欄位含規則 2 全部項目） | `grep -n '^## 5.5' specs/references/hardware-acceptance.md`；`diff` 模板 ID 與第 5 節 ID（T9 已為空） | 無 red 可證（文件）；預期值：需求文字（A5 公開規格頁，未實測） |
| R12 | `[derived]` | 文件核對 | `Taskfile.yml:76`；`specs/changes/archive/onepage-c61-port/bringup.md:12`、`bringup-record.md:14`、`onepage-wifi-upload/hardware-acceptance.md:35`、`hardware-acceptance.md` §1／§2.2／:43／:74；B5a（:236）；`specs/changes/README.md:13` | `grep -n 'hardware-acceptance' Taskfile.yml`；`grep -rn '4\.4\.0' specs/changes/archive/onepage-c61-port specs/references/hardware-acceptance.md`；`grep -n '4785a36' specs/changes/README.md`；`task --list` | 無 red 可證（文件／設定文字）；預期值：需求文字；B5a 字串對照 `scheduler_c61.rs:419` |
| R13 | `[assumed]` | host 測試（FakeSocket＋虛擬 SD）；真實瀏覽器／curl 無法主機驗證 | `host/tests/upload_http.rs`：`upload_delimiter_not_followed_by_dashes_is_a_failure`、`upload_delimiter_with_too_little_after_it_is_a_failure`、`upload_final_delimiter_completes_the_upload`、`upload_final_dashes_split_across_reads_still_complete`；實作 `src/apps/upload/http.rs` `handle_upload` | `cargo test-host --locked --test upload_http` | 前兩個有（`failure response contains "200 OK"`）；**後兩個（最終 `--` 成功、切 read）本來就過：迴歸保護，無 red 可證**；預期值：需求文字（`--` 才算完成；`\r\n`／不足即錯誤且無 Uploaded event） |
| R14 | `[human]` | 文件核對 | `hardware-acceptance.md` 狀態欄；`specs/hardware-records/TEMPLATE.md` | `grep -cE '\| `PASS` \|$' specs/references/hardware-acceptance.md`（T11 結果 0）；`grep -cE '\| `UNVERIFIED` \|$' …`（82） | 無 red 可證（文件）；預期值：需求文字 |
| R15 | `[derived]` | host 測試＋build（1394／0／1，exit 0） | 本次 `task acceptance`（見上） | `task acceptance` | 無 red 可證（基線不退步；非新行為）；預期值：需求文字（≥1366 passed、ignored ≤1） |

R14 另一面：本 change 沒有任何一處把編譯、link、host 測試通過寫成實機通過；R6 的 link-level 證據在 `hardware-acceptance.md` B5a 明寫「目前只有 link-level 證據」。

## Weakening gate 總結

| Task | 既有測試／斷言改動 |
|---|---|
| T1、T8、T9、T10 | 無測試檔改動（文件／設定） |
| T2 | 無（新增 `build_identity.rs`；既有測試檔 diff 為空） |
| T3 | 無（新增 `stack_analyzer.rs`） |
| T4 | 無（新增 `esp_println_features.rs`） |
| T5 | **改了一個既有斷言的預期值**：`r10_epd_init_trace_is_bounded_wait_soft_reset_wait_configure`（走 C61 `Epd::init`）由 `init_trace_expected()[2..]`（X4 trace）改為 `c61_init_trace_expected()[2..]`；僅 booster 資料不同，仍整段序列逐項比對，強度未減。X4 golden 未動 |
| T6 | 無移除／放寬／skip；僅 `KeyScanner::sample()` 最後一行等價改寫（production） |
| T7 | `host/tests/upload_http.rs` diff 僅新增（+159／−0）；既有 fixture 皆已寫 `--\r\n`，無需放寬 |

全程無新增 `#[ignore]`／skip。

## 仍 UNVERIFIED／無法在主機證明（R14）

- **R5 booster 對實機面板**：BSP 值是否適用使用者面板未確認（D10）；實機畫面為 S4。
- **R4 冷開機無 USB**：log 的實際去向與 jtag printer 在無主機下是否不卡死（A1b），主機無行為可驗。
- **R6 週期 log**：每 30 s 真的出現只有 link-level 證據；成功／失敗兩路徑的實機行為未驗。
- **R7 ladder mV**：mV 數值是否準確、是否落在 A5a 窗口，需實機；`key:` 行格式只有源碼核對。
- **R13**：對真實瀏覽器／curl 的行為（host 以 FakeSocket＋虛擬 SD 驗證）；真實 SD 寫入／close 錯誤路徑未驗。
- **5.5 儀器對照所列延後／跳過項目**：A6b `SKIPPED`；A7e、B4b、W6 `DEFERRED`；A6a、A6c、B5a、A4d、P10、A7c-2 `DEFERRED`；A5a／A7f／W1(d)／W2／W5C 為條件式。`hardware-acceptance.md` 狀態欄 82 項仍全為 `UNVERIFIED`（PASS 數 0）。
- **未決（使用者）**：D8（電流量測儀器、量程、量測點）；OnePage 是否有 USB-C 以外的 5V 入口（決定 Debug Mate 能否用於 W6）。

## 已知限制與後續（不在本 change 範圍）

- R13 錯誤路徑（malformed、EOF 不足、既有 "upload incomplete"）會在 SD 留下已寫入的檔案（前者為完整內容但判失敗，後者為被截斷）；不影響 `upload: file saved as` log。建議另案決定是否清除殘檔。
- stack analyzer 對現有映像只能回報 unknown（映像無 `.stack_sizes`）；要有 worst-case 數字須改用 `-Z emit-stack-sizes` 建置，另案。
- BSP 的 `0x1A [5A]`（fast_temp）未採納，另案。
- 明確不做：upload 認證／session token、async BUSY。
- A4d／P10 的可控 BUSY 逾時測試模式待另案設計。
- R6 實作的週期 log 為 `battery: cell <mV> mV, <N>% (periodic)`，比開機第一次的 `battery: cell <mV> mV, <N>%` 多 ` (periodic)` 後綴（`log!` 會拆格式字串，無後綴則無法以 ELF 字串區分週期那行）；前綴相同。
- 過程備註：T7 的 subagent 曾用 `git stash`／`pop` 跑基準，事後核對 stash 為空、T1–T6 改動仍在；`tasks.md` 的 index 狀態因此由 `AM` 變 `A `，內容完好。

## Archive 閘門（`/spec-archive`，2026-10-08）

**evidence 表結果：8 verified／7 open。**

- **verified（路徑、可執行指令、red-proof 齊全）**：R1（指令級 red）、R2、R3、R4（C61 半邊；X4 守護屬迴歸保護）、R5（C61 半邊；X4 與 golden 屬迴歸保護）、R6、R7、R13（負向案例；成功路徑屬迴歸保護）。
- **open（沒有自動化 red-proof，依 skill 不得默默歸檔）**：R8、R9、R10、R11、R12（文件／設定文字，證據只有 grep 核對）、R14（狀態欄 PASS 數＝0 的核對）、R15（基線不退步，非新行為，無 red 可言）。這些需求的性質本來就沒有「實作前失敗的測試」；前例：先前封存的 change 都以「red-proof 例外經使用者接受」處理。
- **仍標 `[assumed]` 的需求**：**R7**（boot 映像 ladder mV 輸出，只要求 boot 映像、格式自訂）、**R13**（multipart 結尾一律要求 `--`，非最終 delimiter 一律錯誤）。這兩項由 agent 解讀、使用者未逐字確認。
- tasks：11／11 已勾選，無未完成項。
- **使用者決定（[human, 2026-10-08]）**：①接受上述 7 項 open（R8–R12、R14、R15）為「無 red 可證」的例外並歸檔，替代證據為本表列出的 grep 指令、狀態欄 PASS 數＝0、完整 `task acceptance` 1394／0／1；②接受 R7、R13 帶著 `[assumed]` 標記歸檔（標記保留，待實機或日後確認）。

## 收尾

- 請執行 `/spec-archive onepage-pre-hardware-hardening`。本 change 全程未 commit。
- `proposal.md` Assumptions 核對結果：A1 未解（維持 UNVERIFIED）；A2、A3、A5 與結果相符（A3：既有 fixture 無缺 `--`）。**A4 有一處不精確**：寫「不增減驗收項目的總數」，但 R9 要求把 A7c、A7d 各拆為兩項，`hardware-acceptance.md` 狀態列因此由 80 增為 82（`git show HEAD:… | grep -cE '\| `UNVERIFIED` \|$'` = 80；現為 82），A4d／P10 則確實保留編號。屬 proposal 文字需補註，不影響任何需求。
