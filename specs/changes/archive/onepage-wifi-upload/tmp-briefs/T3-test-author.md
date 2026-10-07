# T3 test author brief — onepage-wifi-upload

你是 **test author**。只寫測試，**不得讀取或修改 production 實作**：`src/**`、`kernel/src/**`、`board-logic/src/**`、`Cargo.toml`（根）。可讀：`assets/upload.html`（client，獨立 oracle）、`specs/changes/onepage-wifi-upload/{spec.md,proposal.md}`、`tmp-briefs/T3-contract.md`、`host/tests/*.rs`、**host 的測試設施**：`host/src/storage.rs`、`host/src/storage/fs.rs`、`host/src/drivers/sdcard.rs`、`host/src/drivers/storage.rs`（你需要它們來建立虛擬 SD 與注入失敗；這些是 host stand-in，不是 production）、`host/Cargo.toml`。你**可以**在 `host/Cargo.toml` 的 `[dev-dependencies]` 加 `embedded-io-async = "0.7"`（根 crate 已用同版本）作為測試設施；其他 Cargo 變更不要做。repo `/Users/raiven_kao/dev/pulp-os`，不要 commit。

## Task
T3：整合 network runner／TCP／UDP 與既有 HTTP upload／list／delete，驗收檔案內容及 SD failure（本 task 的 host 驗收範圍為 HTTP 服務層；TCP／UDP 組裝只能編譯驗收）。

## Requirements（未標註 provenance；模糊就停下回報）
- R6: WHILE upload 已連線 THE SYSTEM SHALL 提供既有 HTTP upload／list／delete 行為。
- R7: WHEN 上傳成功 THE SYSTEM SHALL 在 SD 保存與輸入位元組一致的檔案。
- R8: IF HTTP／SD 處理失敗 THEN THE SYSTEM SHALL 回報失敗而不顯示上傳成功。

## 契約
`/Users/raiven_kao/dev/pulp-os/specs/changes/onepage-wifi-upload/tmp-briefs/T3-contract.md`（完整讀；測試只能針對它）。

## 交付
新檔 `host/tests/upload_http.rs`，以 `pulp_host::apps::upload_http`、host 的虛擬 SD（`VirtualStorage` + `SdStorage`）與一個你自己實作的 in-memory fake socket（實作 `embedded_io_async::{ErrorType, Read, Write}`：輸入是預先給定的位元組、可設定每次 read 的切片大小，寫出全收集；可選擇在輸入耗盡時回 `Ok(0)`）。用 `embassy_futures::block_on` 驅動（host 已有 `generic-queue-8`，不要自己再動）。每個 serve 呼叫包外層有界保險（避免回歸時 hang）。涵蓋（expected value 只能來自 requirement／契約／upload.html，不得抄實作輸出）：

R6：
1. `GET /` → 200、text/html、body == `include_bytes!("../../assets/upload.html")`（相對測試檔路徑自行確認），event `Nothing`。
2. `GET /files`：0 檔 → `[]`；3 檔（不同大小）→ 陣列含恰好這 3 筆的 name／size（精確字串，順序以虛擬 SD 的 list 順序為準；你可以先用虛擬 SD 的 `list_root_files` 取得預期順序）；`GET /files?x=1` 結果相同；`Content-Type: application/json`。
3. `POST /delete` 成功：200，檔案消失，event `Deleted` 帶正確 name；body 前後有空白（`" BOOK.TXT\n"`）仍可刪（client 以純檔名送出，現行 trim）——若你認為這屬實作細節而非 requirement，不要測，並在報告說明。
4. 其他路徑 → 404；header 超過 1024 bytes 無終止 → 431；對端在 header 前關閉 → `Nothing`、不 panic。
5. 切片無關：同一組請求（`GET /files`、一次小上傳）分別以切片 1／3／64／1460／整包送入，回應與結果逐位元組相同。

R7：
6. `POST /upload` 內容位元組一致：內容長度 0、1、2047、2048、2049（work buffer 邊界附近）、100 KiB；全 256 種位元組值重複；含看似 boundary 的片段（`\r\n--`、`\r\n--bound`、boundary 的前綴、`--` 結尾字樣）；CRLF 結尾內容；每種都以切片 1／7／512／1460／整包送入，並要求寫入檔案逐位元組等於輸入、回應 200 `OK`、event `Uploaded` 且 name 正確。boundary 長度含 1、40、70（現行上限 120）。
7. 覆寫：同名檔已存在時，上傳後內容為新內容（不是附加）。
8. 檔名：已是合法 8.3（`BOOK.TXT`）→ 原名寫入；不得寫出根目錄以外（含 `../`、`/`、`\` 的 filename 要嘛被安全化成單純名字，要嘛失敗；兩者擇一都不能在虛擬 SD 留下目錄外檔案，也不能 panic）。

R8：
9. 儲存寫入失敗：用 `inject_error(StorageOp::Write, ...)` 與 `StorageOp::Append`（第 1 次、第 N 次——例如大檔中途）注入 → 回應狀態行**不是 200**、不含 `HTTP/1.0 200`、event `UploadFailed`、絕不 `Uploaded`。
10. SD 不存在（`set_mounted(false)`）→ 上傳失敗、列表不 panic、刪除 `DeleteFailed`。
11. 上傳中途連線斷（body 不完整即 `Ok(0)`）→ 非 200、`UploadFailed`。缺 boundary、缺 filename、空 filename → 非 200、`UploadFailed`。
12. `StorageOp::Delete` 注入錯誤與刪除不存在檔案、空檔名、13 bytes 檔名 → 非 200、`DeleteFailed`，且既有檔案保持不變。
13. 回應內容不洩漏成功：上述所有失敗案例中，回應 bytes 不含 `200 OK`。

## 紀律
- red-proof：實作前執行 `scripts/host-test.sh --test upload_http`，完整貼失敗輸出（預期：unresolved import）。
- 每個 expected 說明來源（requirement／契約／upload.html）；任何值抄自實作輸出就是缺陷。不得 `#[ignore]`／skip。
- 契約的 delete trim、sanitize 行為若與你從 requirement 的推導不一致，不要自行調整期望，回報。
- 報告（正體中文）：(1) 檔案，(2) 每項測試與 expected 來源，(3) 實作前完整失敗輸出，(4) 含糊之處（含：list 在 SD 失敗時現行回 200 `[]`，你認為 R8 是否涵蓋），(5) 契約讓某 requirement 無法測的地方。
