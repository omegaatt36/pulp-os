# T6 test author brief — onepage-wifi-upload

你是 **test author**。只寫／改測試，**不得讀取或修改 production 實作**：`src/**`、`kernel/src/**`、`board-logic/src/**`、根 `Cargo.toml`、`host/src/**`。可讀：`specs/changes/onepage-wifi-upload/{spec.md,proposal.md}`、`assets/upload.html`（瀏覽器 client，oracle）、`host/tests/*.rs`（既有慣例）、`host/Cargo.toml`。可**執行**測試（黑箱），不可讀實作原始碼。repo `/Users/raiven_kao/dev/pulp-os`，不要 commit，不要 `git stash`。

## Task
T6：host service／timeout／storage-error 回歸。T1–T5 已有 `upload_connect`（18）、`upload_http`（24）、`upload_mdns`（58）、`upload_session`（18）。本次補缺口並移除 `bring_up` 的測試依賴。

## Requirements（無 provenance 標註；含糊就停下回報）
- R6: WHILE upload 已連線 THE SYSTEM SHALL 提供既有 HTTP upload／list／delete 行為。
- R7: WHEN 上傳成功 THE SYSTEM SHALL 在 SD 保存與輸入位元組一致的檔案。
- R8: IF HTTP／SD 處理失敗 THEN THE SYSTEM SHALL 回報失敗而不顯示上傳成功。
- R4/R5: 連線期限與錯誤（既有測試涵蓋）。

## 使用者決定（2026-10-07，是 fact）
`GET /files` 在 SD 未掛載或列表失敗時，必須回**非 2xx（≥400，預期 500）**，且 `ServerEvent` 不得顯示上傳成功／刪除成功之類事件；不得外洩檔名。`upload.html` 對非 200 已有「Could not list files」分支。

## 交付 A：新檔 `host/tests/upload_regression.rs`
介面沿用 `upload_http.rs` 的 `pulp_host::apps::upload_http` 與 host 儲存假件（讀 `upload_http.rs` 學慣例，包含失敗注入）。涵蓋：
1. **list 失敗（會紅）**：(a) 注入 `StorageOp::List` 失敗；(b) SD 未掛載。皆斷言 status ≥400、不含 `200 OK`、body 不含任何檔名、event 為 `Nothing`。**此測試實作前必須紅**，貼出失敗輸出（斷言失敗，不是編譯錯誤）。另加正向：SD 正常時 `GET /files` 仍回 200 與 JSON（防過度修正）。
2. **>64 筆目錄列表**：寫入 70 個檔後 `GET /files`；斷言行為由 requirement 推得——至少前 64 筆合法 JSON 且回 200、不 panic、回應為有效 JSON 陣列；若 requirement 不足以決定「第 65 筆之後是否截斷」，**只斷言有效 JSON 與不 panic**，並於報告列為含糊。
3. **delete 路徑安全**：`POST /delete` 名稱含 `..`、`/`、`\`、空字串、超長（>13）、SD 根目錄外目標 → 失敗（≥400）、**不刪除**任何既有檔（含根目錄下同名檔以外的檔）。
4. **upload 的兩條失敗路徑**：multipart `boundary` 超過 120 bytes → 失敗（≥400）、不寫檔；multipart part headers 超過 2048 bytes → 失敗（≥400，契約：500／`UploadFailed`）、不寫檔。
5. **大檔位元組一致（R7）**：1 MiB 與 3 MiB 全 256 值循環內容，多種 read slice（例如 1、1460、整包），上傳後讀回比對完全一致。VirtualStorage 若有容量限制，改為驗證到其上限且報告。
6. **`POST /upload?x=1`（帶 query）**：行為依 requirement 無法決定 → 只斷言不 panic 且 status 與「不帶 query」一致；若 `upload.html` 不送 query，報告為含糊並可跳過斷言（但仍不得 `#[ignore]`；直接不寫該測試並列入報告）。
每個失敗注入測試都要斷言**注入確實被觸發**（沿用既有測試的做法），避免空轉。

## 交付 B：改寫 `host/tests/upload_connect.rs`（`bring_up` 將被刪除）
`bring_up` 要被移除。`upload_connect.rs` 中用到 `bring_up` 的 9 個測試（`bring_up_success_returns_dhcp_value_and_calls_in_order_once`、`missing_credentials_never_touch_radio`、`invalid_credentials_never_touch_radio`、`association_error_maps_to_association_failed_without_dhcp`、`association_that_never_completes_times_out_within_bound`、`dhcp_that_never_completes_times_out_within_bound`、`dhcp_budget_starts_after_association_completes`、`timeout_duration_follows_configured_limits`、`completes_immediately_without_waiting_for_the_deadline`）處置如下：
- 逐一判斷 `host/tests/upload_session.rs` 是否已有**一對一語意相同**的覆蓋（斷言同樣的 requirement 性質）。有 → 刪除該測試。沒有 → 改寫為走 `pulp_host::apps::upload_session::run`（放進 `upload_session.rs` 或 `upload_connect.rs` 皆可，優先放 `upload_session.rs`；兩個檔案你都可以改，但**不得放寬或刪減 `upload_session.rs` 既有 18 個測試**）。
- `timeout_duration_follows_configured_limits`：必須保留（改走 `session::run`），確認 `Limits` 的 associate／dhcp 期限真的各自生效。
- 檔頭註解、import 同步整理；不再 import `bring_up`。`check_credentials`／`Limits`／`ConnectError::lines` 的 9 個測試**不變**。
- **weakening gate 報告**：列出被刪除的每個測試，以及它被哪個既有／新測試取代（寫出對應的測試名與斷言性質）；沒有對應者不得刪。

## 紀律
- red-proof：改寫 B 時 `bring_up` 仍存在，所以 B 先照樣綠（等 implementer 刪除 `bring_up` 後仍須綠）；交付 A 第 1 點必須在實作前紅燈。其餘（2–6）是既有行為的回歸，實作前就應綠：你必須證明它們不是空轉——對每個測試說明它「會在什麼壞行為下失敗」，並在 `/tmp/t6_stub/` 用**黑箱**方式（例如在測試內暫時反轉一個斷言、或注入失敗）做至少 4 個 mutation，報告結果。stub 不得留在 repo。
- expected value 只能來自 requirement／`upload.html`／上述使用者決定；不得抄實作輸出。不得 `#[ignore]`／skip。外層有界保險（沿用既有 worker thread＋牆鐘 timeout）。
- 驗證指令：`scripts/host-test.sh --test upload_regression`、`--test upload_connect`、`--test upload_session`、`--test upload_http`、`--test upload_mdns`。
- 報告（正體中文）：(1) 檔案，(2) 每項測試與 expected 來源，(3) 實作前完整失敗輸出（第 1 點紅燈），(4) 含糊之處，(5) requirement 無法測的地方，(6) mutation 結果，(7) B 的 weakening gate 對照表。
