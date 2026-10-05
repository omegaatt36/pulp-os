# OnePage C61 驗收審查 — 2026-10-05

原始審查 Verdict: BLOCK。下列內容保留修正前的三個問題及當時證據。

2026-10-05 更新：三個 MUST 已修正，包含暫停 Reader 再睡眠的位置保存；獨立 review APPROVE，16 階段 acceptance 全通過。見 [corrective-validation.md](corrective-validation.md) 與 [evidence.md](evidence.md)。使用者明確授權保留歷史證據缺失與待確認決定後，已例外封存；硬體仍未驗。
審查範圍：`3bb911a..3007dc6`，125 files、23,198 insertions、1,095 deletions。
由三個唯讀 review agents 分別審查 HAL、共用邏輯／驗證設施、apps／integration，再核對實際原始碼。

## 阻擋驗收

### 1. Session 恢復位置被 Reader 初始化覆蓋

- `src/apps/manager.rs:327` 呼叫 restore_state，`:366` 隨後呼叫 on_enter。
- `src/apps/reader/mod.rs:847,852` 將章節與 restore_offset 清除；`:920` 從書籤重新載入。
- `kernel/src/kernel/scheduler_c61.rs:340` 只 flush 已 dirty 的書籤，沒有保存當下 reader 位置。
- 因此新閱讀位置雖寫入 SD session，重啟後仍回第一頁／舊書籤，違反 R20。
- 現有 host test `scripts/reader-regression/os/tests/session_restore.rs:102` 額外呼叫 `save_position`，掩蓋此缺陷。
- 實際重現：在暫存 host harness 移除這個額外呼叫，執行
  `cargo +nightly-2026-09-22 test --offline -p pulp-os-host --features board-x4,tree --test session_restore txt_session_restores_the_same_page_and_text`。
  exit 101；預期 page 7，實際 page 0。暫存測試檔已還原；repo 實作未改。
- 修正方向：正常開書與 session restore 分開初始化，保留 session chapter／offset，並讓 session 優先於舊書籤。
  回歸測試應呼叫正式 AppManager／sleep 接線，避免另寫一套 glue。

### 2. SD 熱插卡缺少初始化 prelude

- `kernel/src/board_c61/spi.rs:260` 只在開機送 >=74 clocks、CS high、400 kHz。
- `kernel/src/kernel/scheduler_c61.rs:286` 熱插卡僅 slow_down → bring_up，未重送 prelude。
- 真實鎖定 dependency `embedded-sdmmc-rs/0bf1254/src/sdcard/spi.rs:17–24` 明確要求 caller 提供；acquire 直接送 CMD0。
- 開機無卡再插卡的新卡從未收到此序列，不能保證初始化成功。
- `src/bin/c61_boot.rs:303` 熱插卡另缺 slow_down，仍以 operating clock 重試。
- 修正方向：共用 prepare_sd_probe，設定 400 kHz、deselect、送至少 10 bytes 0xFF 並完成傳輸；正式／bring-up 每次初始化皆使用。

### 3. ADC timeout 卡住另一通道

- `kernel/src/board_c61/adc.rs:70–77` spin timeout 回傳 None，未完成／清理 pending conversion。
- 真實 esp-hal 1.2.0 `analog/adc/riscv.rs:365–370` 在 active_channel 不同時持續 WouldBlock；`:418` 僅原通道完成讀取後才清除。
- 電池通道超時後，front key 通道可能持續失敗，直到約 30 秒後再次讀取電池通道。
- 修正方向：支援 timeout recovery，或追蹤並 drain 原 pending channel，再切換通道。測試必須涵蓋 shared converter 的狀態，不能只測無狀態 ADC fake。

## 本次實際驗證

`scripts/run-software-acceptance.sh --with-mutants`：exit 0，全部 15 stages 通過。

- board logic：321 passed。
- reader：pre-port 83／tree 84 passed；3105 行 golden trace 同 SHA256 `8cb6e31a70a32cdc05d30fab45ee0c7983da8cde6a55ba8bb45a0540f8d3f454`。
- reader mutants：16 killed／0 survived。
- X4 offline、X4 wifi、C61 full／boot release link 通過；board selection、offline boundary、X4 input／display／battery equivalence、fmt 通過。
- C61 statics 178,080 B，stack 78,544 B，比 48 KiB 下限多 29,392 B。
- 以上綠燈未涵蓋前述 runtime 接線問題。硬體項目全部未驗。
- 完整當次 log：`/tmp/c61-acceptance-review.log`；額外 session 重現：`/tmp/c61-session-review/repro.log`（暫存路徑，不是永久證據）。

## Rust 行數與刪減判斷

Physical lines，含註解／空白；不是機器碼大小。

| 範圍 | 行數 | 判斷 |
|---|---:|---|
| board-logic/src | 11,319 | HAL-free 邏輯與測試 |
| 其中 cfg(test) | 6,383 | 56.4%；不進 release ELF |
| 其中 cfg(test) 外 | 4,936 | 含 1,526 行註解／空白 |
| board-logic/examples/memreport.rs | 197 | host 報表 |
| kernel/src/board_c61 | 1,993 | 真正 HAL adapters |
| scripts Rust | 3,979 | host harness／fixtures，其中 reader-regression 3,672 |
| c61_boot.rs | 447 | 獨立 bring-up／demo |

新增量來自 port、HAL 1.2 遷移、X4 邏輯抽出、PSRAM／SD session 的新契約，以及大量 host 驗證設施。
不建議按總行數刪掉 board-logic 或測試；先刪沒有正式 consumer 的 API，再消除重複 glue。

### 優先整理

1. `board-logic/src/memory.rs:703–1116` 約 414 行 ELF report／inventory 只有 memreport 與自身 tests 使用：移至 host reporting 模組；其中約 327 行 inventory 可改為報表資料。移動本身不減 repo 總行數。
2. `power.rs:161` 的 hardware_reset_display 永遠回 Forbidden，只有 tests 呼叫：可刪方法、variant、相關人工 API tests，保留真實 power/reset trace 測試與 pin ownership。
3. `ssd1677.rs:350` 的 initial_refresh／needs_initial_refresh 沒有正式 consumer：可刪欄位、getter、更新與僅驗證該欄位的 assertions；保留 init_done。
4. `lifecycle.rs:58` 的 Periodic::interval_ms 無 consumer：可刪。
5. `scheduler_c61.rs:54–90` 的 AppStrips／FnStrips 合併為單一 closure adapter；button_feedback 相同 board constants 合併。
6. c61_boot 保留到首次硬體 bring-up 完成，再評估移出預設 build 或刪除。拿掉後可同步刪只有 demo 使用的 alloc_external／alloc_dma／save_session wrappers 與部分 by-value sleep adapters。
7. 用正式 session lifecycle seam 取代 host test 手寫 collect／sleep／wake；其餘 pagination、SD corruption、power ordering 等行為測試應保留。

## 非阻擋問題與交付缺口

- mutation runner 把任何非零 exit（包括 harness compile／fetch error）當 KILLED；應只接受具名 test failure 或 golden mismatch。
- reader harness 固定 board-x4，未執行 C61 BigBuf／PSRAM adapter；不能稱為完整 C61 runtime 驗證。
- 插拔卡只 invalidate kernel cache／redraw，Files app 不立即 reload；缺卡錯誤需 Back → Files 才恢復，拔卡可能仍顯示舊清單。建議 app storage-change event。
- 圖片 decode 先配置 internal Vec，再複製到 PSRAM；BigBuf 有用途，但尚未解決 decode 高峰 internal-memory 使用。
- 原始碼／測試名仍有大量 R/T IDs，違反 spec-apply hygiene；應將 traceability 留在 change 文件。
- proposal 未列 Risk，依提供的 spec-apply 規則應按 red tier；現有文件無法證明每項都有 test-author／implementer split、實作前 red-proof、expected-value 來源與 weakening gate。
- spec 無 provenance tags；無逐 requirement evidence.md。不能事後把 mutation failure 當成實作前 red-proof；無法存取的歷史證據應明確標記缺失。
- progress.md 的未 commit／舊 commit 說明與當前 HEAD 不符，需更新。
- baseline §42 的五項行為決定仍待確認；硬體驗收維持未驗，不執行封存。
