# T5 implementer brief — onepage-wifi-upload

你是 **implementer**。repo `/Users/raiven_kao/dev/pulp-os`，branch `onepage`，不要 commit；**不要使用 `git stash`**（會動 index）。

## Task
T5：修正退出／失敗／再次進入的 runner、socket、interface／radio ownership，驗收 reader 返回。

## Requirements（未標註 provenance；與現實衝突就停下回報）
- R9（相關部分）: WHILE upload 已連線 THE SYSTEM SHALL 提供既有 `pulp.local` mDNS 回應。
- R10: WHEN 使用者退出或連線失敗 THE SYSTEM SHALL 釋放 network／radio 資源並返回可操作的閱讀介面。
- R11: WHEN 再次啟動 upload THE SYSTEM SHALL 不因殘留 controller／interface ownership 失敗。

## 契約
`/Users/raiven_kao/dev/pulp-os/specs/changes/onepage-wifi-upload/tmp-briefs/T5-contract.md`。

## 唯讀測試
`host/tests/upload_session.rs`（test author 已寫；**不得修改**）。`host/tests/upload_connect.rs`、`upload_http.rs`、`upload_mdns.rs` 同樣唯讀且必須仍全綠。先跑 `scripts/host-test.sh --test upload_session` 保留紅燈輸出。

## 範圍
1. 新增 `src/apps/upload/session.rs`、`ConnectError::RadioUnavailable`（`connect.rs`；只加 variant 與其 `lines()`，不改既有行為）；`host/src/apps.rs` 加膠水，讓 session.rs 在 host 與 firmware 都能編譯、且不複製 `connect.rs`。`bring_up` 內的 timeout 邏輯若需要共用，抽成內部 helper，但 `bring_up` 的公開行為與 `upload_connect.rs` 的 18 個測試不得受影響。
2. `mod.rs` 重構 `run_upload_mode` 走 `session::run`（見契約「firmware 端要求」）：
   - `C` struct 持有 `WifiController`、`StackResources`、`Stack`、`Runner`（或等價）；欄位宣告順序＝期望的 drop 順序，並用註解說明（network 先於 controller）。
   - 以 `Interface::try_station()` 取代 `Interface::station()`（不得 panic）；`WifiController::new` 失敗 → `RadioUnavailable`。
   - `runner.run()` 與 HTTP／mDNS 一樣，在 serve 階段持續執行（沿用 T4 的 `select4` 結構，改放在 serve closure 內；associate／dhcp 階段 runner 的使用方式要保持正確）。
   - 錯誤畫面在 `session::run` 返回之後才顯示（資源已釋放）。
   - 檢視 `wifi: WIFI::steal()`（`manager.rs`）：在 RAII 語意下是否仍安全；若不安全或可更好，說明並修正（例如把 steal 搬進 acquire、或證明目前作法已足夠）。
3. **Source 證據（寫入 `specs/changes/onepage-wifi-upload/baseline.md` 新增「T5 所有權審查」段）**：引用 esp-radio 1.0.0-beta.1 的 `Interface::try_station`／`Drop for Interface`、`WifiController` 的 `WifiRefGuard` drop → `wifi_deinit`、embassy-net `Runner`／`Stack` 對 `Interface` 的持有方式（檔案與行號），說明 RAII 如何保證釋放；並標明哪些仍 UNVERIFIED（deinit 是否把 radio 內部 allocation 全數還給 heap、重複 N 次後的 heap 高水位、Wi-Fi 重新 init 的耗時、esp-rtos 任務是否殘留），這些交給 T7 的實機流程。
4. 檢查 C61 的 special-mode 路徑（`kernel/src/kernel/scheduler.rs` `handle_special_mode`、`scheduler_c61.rs`）：確認 upload 返回後 Pop＋full redraw 在 C61 上適用（`self.epd` 型別、display health、SPI bus 共用、`reinit_display`）；如果發現缺口（例如返回後 C61 顯示未 re-init、`hw.display` 狀態未更新），修正並在報告中說明；`scheduler_c61.rs` 的 `upload_available: false`（註解「offline-only」）若已不實，改成正確的值與註解（Upload 只在 special-mode 期間為 active、不會被存進 session；請檢查這個推論是否成立，說明依據）。
5. 不得新增大型 `static`（wifi 變體 statics 餘裕約 3.2 KB）；`check-wifi-build.sh` 會抓。

## 紀律
- weakening gate：不得修改任何既有測試或 `scripts/*`。報告列出你對 `host/`、`scripts/`、`board-logic/`、`kernel/` 的所有改動。
- 驗證並貼實際輸出：`scripts/host-test.sh --test upload_session`、`scripts/host-test.sh --locked`（全部）、`scripts/check-host-boundary.sh --skip-firmware`、`scripts/test-board-logic.sh`、`scripts/check-wifi-build.sh`（報告 statics 餘裕）、`scripts/check-offline-boundary.sh`、`cargo build-c61`、`cargo build-x4`、`cargo build-c61-wifi`、`cargo build-x4-wifi`、`rustfmt --edition 2024 --check <改過的每個 .rs>`（若 rustfmt 順著 `#[path]` 動到其他檔，用 `git checkout -- <該檔>` 還原，不要用 stash）。
- 報告（正體中文）：(1) 改動檔案與理由，(2) red-proof，(3) 驗證結果，(4) 對 host／scripts／board-logic／kernel 的改動，(5) 缺陷／UNVERIFIED／阻塞，(6) 含糊處。
