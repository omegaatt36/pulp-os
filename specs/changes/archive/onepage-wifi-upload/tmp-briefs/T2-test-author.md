# T2 test author brief — onepage-wifi-upload

你是 **test author**。只寫測試，**不得讀取或修改實作檔**（`src/**`、`kernel/src/**`、`board-logic/src/**`、`host/src/**`、`Cargo.toml`）。可讀：`host/tests/*.rs`（寫法慣例；注意 host 現有慣例偏好有界、不長時間 wall-clock 等待的測試）、`host/Cargo.toml`（確認 embassy-time `std`／embassy-futures 可用）、`specs/changes/onepage-wifi-upload/{spec.md,proposal.md}`、同目錄 `T2-contract.md`。repo `/Users/raiven_kao/dev/pulp-os`，不要 commit。

## Task
T2：遷移 station controller／credentials／interface API，建立有界 association／DHCP 與錯誤返回。

## Requirements（未標註 provenance；模糊就停下回報）
- R4: WHEN 使用有效 station credentials 啟動 upload THE SYSTEM SHALL 在設定期限內進入可取得 IPv4 的連線狀態。
- R5: IF credentials 缺失／不合法、association 失敗或 DHCP 逾時 THEN THE SYSTEM SHALL 顯示可退出的連線錯誤。

## 契約
見 `/Users/raiven_kao/dev/pulp-os/specs/changes/onepage-wifi-upload/tmp-briefs/T2-contract.md`（完整讀它；測試只能針對這個 API）。

## 交付
新檔 `host/tests/upload_connect.rs`（integration test，經 `pulp_host::apps::upload_connect`）。至少涵蓋（expected value 只能來自 requirement／契約文字）：
1. `check_credentials` 表格：空 SSID（含各種密碼）→ MissingCredentials；SSID 32 bytes＋8 bytes 密碼 → Ok；SSID 33 bytes → InvalidCredentials；密碼 7／0／64 bytes → InvalidCredentials；密碼 8 與 63 bytes 邊界 → Ok；SSID 1 byte 邊界 → Ok；SSID／密碼含非 ASCII UTF-8 位元組只要長度合規仍 Ok（以 bytes 計）。
2. `bring_up` 成功：有效憑證、associate 立即 Ok、dhcp 回傳一個值 → `Ok(該值)`；associate 先於 dhcp 被呼叫（用順序記錄驗證）；各 closure 恰被呼叫一次。
3. 憑證缺失／不合法：回對應 error，且 associate 與 dhcp closure **都沒被呼叫**。
4. associate 回 Err → `AssociationFailed`，dhcp closure 未被呼叫。
5. associate 永不完成 → `AssociationTimeout`，且耗時 ≥ `limits.associate` 且在合理上界內（用很小的 limits，例如 30–100 ms；上界給足夠寬的 slack 避免 CI 抖動，但必須有界——整個測試以外層保險 timeout 包住，避免回歸時 hang）；dhcp closure 未呼叫。
6. dhcp 永不完成（associate 已 Ok）→ `DhcpTimeout`，耗時 ≥ `limits.dhcp`。
7. 期限是「設定的」：兩組不同 limits 得到不同的逾時耗時（短的明顯早於長的）；`Limits::DEFAULT` 的欄位等於 `ASSOCIATE_TIMEOUT`／`DHCP_TIMEOUT`；且二者皆為有限、非零（R4「設定期限」）。
8. `ConnectError::lines()`：5 個 variant 每個非空、每行非空字串、第一行彼此相異（R5「顯示」連線錯誤）。
9. 反應速度：associate 在期限內完成時 `bring_up` 不會等到期限才回（例如 limits 設 2 s，associate 立即 Ok、dhcp 立即完成，整體 < 500 ms）。

## 紀律
- red-proof：實作前執行 `scripts/host-test.sh --test upload_connect`，完整貼出失敗輸出（預期：unresolved import／module）。
- 每個 expected value 說明來源（requirement／契約）；不得由執行實作輸出得出。不得 `#[ignore]`／skip。
- 報告（正體中文）：(1) 檔案，(2) 每項測試與 expected 來源，(3) 實作前完整失敗輸出，(4) 含糊之處，(5) 若你認為契約本身讓某個 requirement 無法被測，指出。
