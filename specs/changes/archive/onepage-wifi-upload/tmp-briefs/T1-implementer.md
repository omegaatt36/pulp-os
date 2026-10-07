# T1 implementer brief — onepage-wifi-upload

你是 **implementer**。repo：`/Users/raiven_kao/dev/pulp-os`，branch `onepage`，不要 commit。

## Task

T1：固定 C61 radio set／optimization，建立 enabled／disabled build／link smoke，記錄版本與 memory 起點。

## Requirements（未標註 provenance；若實作發現 requirement 與現實衝突，停下回報，不要自行解讀）

- R1: WHERE Wi-Fi enabled THE SYSTEM SHALL 使用有 C61 支援且相容的固定 radio／RTOS dependencies 連結 release ELF。
- R2: WHERE Wi-Fi enabled THE SYSTEM SHALL 以 radio 要求的 optimization level 2 或 3 建置 radio package。
- R3: WHERE Wi-Fi disabled THE SYSTEM SHALL 保持不連入 radio driver 的離線版本。
- R12（本 task：起點）: WHILE Wi-Fi enabled THE SYSTEM SHALL 保持有證據的 internal heap／stack 與 PSRAM 預算。

## 測試（唯讀，已存在）

- `scripts/check-wifi-build.sh`（新）與 `scripts/check-offline-boundary.sh`（已擴充 C61+wifi 正控制）。**你不得編輯這兩支，也不得編輯 `scripts/report-c61-memory.sh`、`scripts/lib/**`。** 若你認為其中一個檢查錯了，停下回報，不要改它。
- 目標：`scripts/check-wifi-build.sh` 全綠。它目前失敗在：(a) `src/lib.rs` 的 `compile_error!`（wifi + board-onepage-c61）；(b) `src/apps/upload.rs` 用了 C61 `Epd` 沒有的 `full_refresh_async`／`partial_refresh_async`（`pulp_board_logic::ssd1677::Epd` 的 API 與 X4 不同，參考 C61 其他 app／scheduler_c61 怎麼刷新與 `kernel/src/board_c61/api.rs` 的 `board::Epd` 說明；upload 的畫面渲染要改用兩個板子共同可用的路徑，不得只為 C61 加分支複製一份）；(c) esp-radio 以 opt-level `s` 編譯，要求 2 或 3；(d) 因為 (a)(b) enabled C61 ELF 不存在，memory baseline 無法產生。

## 範圍界線

- 本 task 只要 **compile／link 與 memory 起點**：不要實作 C61 上 upload 的 session restore（`scheduler_c61.rs` 的 `upload_available`）、不要改 HTTP／mDNS／連線流程（那是 T2–T5）。若 `src/bin/main_c61.rs` 需要最少的改動才能讓 enabled ELF 真的連入 radio（symbol 必須存在，測試會探 `esp_wifi_start`／`esp_wifi_init_internal`／`esp_wifi_connect_internal`），做最少改動並在報告說明；Upload 選單與 `run_special_mode` 已經是 `#[cfg(feature = "wifi")]`，從那條路徑連入。
- C61 上 `unsafe { WIFI::steal() }` 與 esp-rtos／radio 啟動順序：本 task 保持現有 `manager.rs` 作法即可，T5 處理生命週期。
- 全域 dev／release profile 是 `opt-level='s'`，**不得改全域**。為 esp-radio 設 package 專用 profile override（`[profile.release.package.esp-radio]` 與必要的 dev 對應）。radio 要求見 `~/.cargo/registry/src/*/esp-radio-1.0.0-beta.1/src/lib.rs` 的文件段（opt-level 2 或 3）。是否也要提升 esp-phy／esp-wifi-sys 等依賴的 level：requirement 只說 radio package，不要擴大，除非你有 upstream 文件證據，並在報告中引用。
- 不要新增 backward-compat 層、不要留 stub（user 規則：不保留相容路徑，移除過時分支）。`src/lib.rs` 的 `compile_error!` 刪除；同時修正因它而過時的註解／文件字串（`Cargo.toml` wifi feature 註解、`.cargo/config.toml` alias 註解、`src/bin/main_c61.rs` 檔頭「`wifi` + this board is a compile_error!」、`README.txt` 若有提到）。`.cargo/config.toml` 要有 C61+wifi 的 alias：`build-c61-wifi`、`run-c61-wifi`（比照 `build-x4-wifi`；run 對 `pulp-os-c61` bin）。`check-board-selection.sh` 若斷言 wifi+C61 會 compile_error，需要說明並處理——它屬於 scripts/，若必須改動，先停下回報而不是直接改（它可能是測試）。
- memory：把 enabled C61 ELF 的 `size`、`report-c61-memory.sh` 結果寫入 `specs/changes/onepage-wifi-upload/baseline.md`（日期、指令、版本表、radio opt-level 取得方式、text/data/bss、report 結論，以及**哪些是 UNVERIFIED**：radio runtime 的 internal heap 需求、PSRAM 分工、stack 高水位）。如果 `report-c61-memory.sh` 對 enabled ELF 預算檢查失敗：**不要改腳本或放寬 board-logic 的預算常數**，把失敗輸出與數字（statics 大小、stack headroom、哪一條規則）完整貼到報告並停下，由 orchestrator 決定。

## 紀律

- **red-proof**：實作前先執行 `scripts/check-wifi-build.sh`，保留失敗輸出（test author 已有一份，你要自己再跑一次確認 tree 沒變）。
- **weakening gate**：不得修改任何測試。你的最終報告要列出你對 `scripts/` 的任何改動（應為零）。
- 必須驗證（貼實際輸出尾段）：`scripts/check-wifi-build.sh`、`scripts/check-offline-boundary.sh`、`cargo build-c61`（離線 C61，不得退步）、`cargo build-x4`、`scripts/test-board-logic.sh`、`scripts/host-test.sh --locked`（host crate 會 `#[path]` 引入 production 檔，改 `src/apps/upload.rs`／`manager.rs` 時要確認 host 仍可編譯與通過）、`rustfmt --edition 2024 --check <你改過的每個 .rs 檔>`。
- 長時間建置：用較長 timeout（可到 600000 ms），必要時 `run_in_background`。
- 報告格式（正體中文）：(1) 改動檔案與理由，(2) red-proof 輸出，(3) 每項驗證指令的實際結果，(4) 對 scripts/ 的改動（應為無），(5) UNVERIFIED／阻塞項，(6) requirement 含糊或想重新解讀的地方。
