# onepage-wifi-upload — progress ledger

由 `/spec-apply` 維護。每個 task 完成後在此記錄證據；context 壓縮後以本檔為準，不重新派工已完成的 task。

## 前置判定

- `proposal.md` 沒有 `Risk` 欄 → 依 spec-apply 規則視為 **red**：每個 task 拆成 test author（只拿 requirement，不讀實作）與 implementer（測試唯讀）兩個 subagent。
- `spec.md` 沒有 provenance tag → 派工時一律視為**未標註**：subagent 發現實作與 requirement 衝突時停下回報，不自行解讀。
- 工作樹起點：branch `onepage`，`8702340`，乾淨。不 commit（使用者未要求）。
- 依 `specs/changes/README.md`，同一 worktree 的共用檔案不平行修改 → 依序 T1 → T2 → T3 → T4 → T5 → T6 → T7（即 tasks.md 依賴順序）。
- `subagent-driven-development` skill 不在可用清單；本 session 以 fresh test-author／implementer subagent ＋ `tmp-briefs/` 派工單 ＋ 本 ledger 自行實作其機制。
- weakening gate 基準：各 task 起點把 `scripts/`、`host/`、`board-logic/` 拷到 `/tmp/wifi-snap/Tn/`，`src/`、`kernel/` 以 `git diff` 對照。
- 不對整個 workspace 跑 `cargo fmt`（會動到 `../smol-epub`），只用 `rustfmt --edition 2024 <單一檔案>`。
- 無機器可跑 radio：所有「可用」宣稱都限於 compile／link／host service test；association／DHCP／HTTP 實測／mDNS／重入／電流全為 UNVERIFIED（R14）。

## 起點事實（讀碼所得，2026-10-07）

- `Cargo.toml` 已固定 esp-hal =1.2.0、esp-rtos =0.4.0、esp-alloc =0.11.0、esp-radio =1.0.0-beta.1（optional，C61／C3 chip feature 皆有）；`wifi` feature 已存在，預設關。
- `src/lib.rs` 有 `compile_error!`：`wifi` + `board-onepage-c61` 被拒。
- `src/apps/upload.rs`（990 行）已用 esp-radio 1.0 的 `WifiController::new`／`Interface::station()`（X4 wifi build 為正控制組），HTTP／mDNS 邏輯與 `embassy_net` socket 耦合，無 host 測試。
- C61 端：`kernel/src/kernel/scheduler_c61.rs` `upload_available: false`；`src/bin/main_c61.rs` 已 `esp_rtos::start`，heap 為 internal 兩段＋獨立 PSRAM heap（budget 檢查）。
- `scripts/check-offline-boundary.sh` 把 C61 當「離線唯一」並以 X4+wifi 為 positive control；C61+wifi 後需更新。
- 基準：`cargo build-c61` 通過（約 11 s，已有 cache）。

## Tasks

| Task | 狀態 | 起點 | 備註 |
|---|---|---|---|
| T1 | 完成 | 8702340 | check-wifi-build.sh 89 ok／0 FAIL；board-logic 321+7+3；host 全綠；離線 report 結論不變。詳見「T1b」 |
| T2 | 完成 | 見 tree.diff 快照 /tmp/wifi-snap/T2 | host 18 測試；詳見下方 |
| T3 | 完成 | /tmp/wifi-snap/T3 | host 24 測試；詳見下方 |
| T4 | 完成 | /tmp/wifi-snap/T4 | host 58 測試；詳見下方 |
| T5 | 完成 | /tmp/wifi-snap/T5 | host 18 測試（upload_session）；詳見下方 |
| T6 | 完成 | /tmp/wifi-snap/T6 | upload_regression 15、connect 9、session 20；詳見下方 |
| T7 | 完成 | | 文件交付；所有實機項目 UNVERIFIED |

## 每個 task 的證據

（task 完成後在此追加：red-proof 輸出、expected value 來源、weakening gate 結果、驗證指令。）

### T1 第一輪（2026-10-07）

- test author red-proof：`scripts/check-wifi-build.sh` 實作前失敗——C61+wifi 建置失敗（`compile_error!`＋`upload.rs` 用了 C61 `Epd` 沒有的 `full_refresh_async`／`partial_refresh_async`）、esp_radio 以 opt-level `s` 編譯（C61、X4 各一）、memory baseline 無 ELF。版本類檢查實作前即綠（Cargo.toml／lock 已是固定組），仍保留作退步防護。expected value 來源皆為 requirement 或 reference 文件。
- implementer 第一輪已完成：刪除 `compile_error!`；upload 刷新改走 `board::full_refresh_screen`／`partial_refresh_screen`（X4／C61 各一份，C61 無 partial → full）；`[profile.{dev,release}.package.esp-radio] opt-level = 3`；`build-c61-wifi`／`run-c61-wifi` alias；`baseline.md`。
- 阻塞：C61+wifi 連結失敗 `Main stack is smaller than 8192 bytes`；statics 249,280 B（預算 207,472 B）。implementer 依規定未放寬預算，回報。使用者選擇「Wi-Fi build 縮 main heap」。
- weakening gate（第一輪）：implementer 對 `scripts/` 零改動；測試完整。

### T1b（2026-10-07）— R12 預算變體

- test author：`board-logic/tests/wifi_budget.rs`（7 測試）；red-proof：`E0432 unresolved imports INTERNAL_HEAP_MAIN_BYTES_WIFI, internal_heap_bytes, internal_heap_main_bytes`。expected value 來源：proposal [human] 決定（52 KiB／96 KiB／48 KiB／64,000）與 T1 實測（離線 statics 180,932、wifi 增量 68,340）；無任何值取自實作輸出。
- implementer：`board-logic/src/memory.rs` 新增變體 API 與 `MemoryBudget::for_build(wifi)`；`kernel/Cargo.toml` 新增 `wifi` feature、根 `wifi` 轉送；`kernel/src/board_c61/memory.rs` 的 heap 與 budget 共用同一來源；`report-c61-memory.sh`／`memreport.rs` 變體感知。
- weakening gate（orchestrator 以 /tmp/wifi-snap/T1b 快照 diff 驗證）：測試檔未改；in-file 測試無斷言被刪或放寬（僅 `pool_limit` 改為委派 `pool_limit_for`、`MemoryBudget::new()` 委派 `for_build(false)`）；report 腳本只增加變體判定（nm 非 absolute `esp_radio|esp_wifi`），所有預算規則不動。
- orchestrator 獨立驗證：`scripts/check-wifi-build.sh` exit 0（89 ok，無 FAIL）；`scripts/test-board-logic.sh`（321／7／3 通過）；`scripts/host-test.sh --locked` 全綠。
- 數字：C61+wifi statics 204,224 B（預算 207,472，餘 3,248）；`.stack` 52,392 B（> STACK_MIN 49,152）；離線 statics 180,940、stack 75,684 不變。
- **約束**：wifi 變體 statics 餘裕僅 3,248 B；T2–T5 不得新增大型 `static`。
- UNVERIFIED：radio 執行期 internal heap 需求（52 KiB 為推算，可能不足）、PSRAM 分工、stack 高水位、`.rwtext.wifi` 能否縮減。

### T2（2026-10-07）— station／bring_up／錯誤返回（R4、R5）

- 契約：`tmp-briefs/T2-contract.md`（orchestrator 定義 `src/apps/upload/connect.rs` API）。
- test author：`host/tests/upload_connect.rs`（18 測試）。red-proof：`error[E0432]: unresolved import pulp_host::apps::upload_connect`（僅 import 層級——API 不存在，其他編譯錯誤被遮蔽；實作後的行為驗證靠 18 個測試實跑）。expected value 來源：requirement R4／R5 與契約的憑證規則、呼叫順序、期限；無值取自實作輸出。
- implementer：`src/apps/upload.rs` → `src/apps/upload/mod.rs`；新增 `connect.rs`（`check_credentials`／`bring_up`／`Limits`／`ConnectError::lines`）；`run_upload_mode` 改走 `bring_up`，連線與 DHCP 期間 BACK 可退出；`host/src/apps.rs` 加 `upload_connect`；`host/Cargo.toml` 的 embassy-time 加 `generic-queue-8`（host 上 block_on 的 waker 需要，否則 Timer panic）。
- weakening gate（orchestrator 對 /tmp/wifi-snap/T2）：`scripts/`、`board-logic/` 零差異；`host/` 只有上列兩處 glue；測試檔 mtime 早於實作檔、未被改。
- orchestrator 獨立驗證：`scripts/host-test.sh --test upload_connect` 18 通過；`scripts/check-wifi-build.sh` exit 0（statics 餘裕 3,256 B，stack 52,400 B）。implementer 另報：host 全套 766 通過、board-logic 全綠、check-offline-boundary 0 FAIL、c61／x4／c61-wifi build 通過、連跑 12 次不 flaky。
- 實作決定：`bring_up` 的兩段 timeout 各加 1 tick（時鐘向下取整會讓 deadline 提早最多 1 tick 觸發，否則「耗時 ≥ limit」測試約 6/8 失敗）。
- 行為變化（需知悉）：`WifiController::new` 失敗原本顯示 `WiFi init failed!`，現在歸為 `AssociationFailed`（`Connection failed!`），log 仍保留 init failed。契約沒有 init-failure variant。
- **未被 host 測試覆蓋（留給後續）**：R4 的「IPv4」語意（`bring_up` 對 `T` 泛型）；R5 的「可退出」（BACK 退出是 mod.rs 的 `select(…, drain_until_back())` 組合，只確認可編譯）→ T5 處理退出路徑的驗收。
- **T5 風險標記**：`Interface::station()` 是 singleton，已存在時 panic（R11 重入）；T5 必須處理，不可假設只呼叫一次。
- UNVERIFIED：實機 association／DHCP／畫面／連線中按 BACK；async future 的 stack 佔用（`report-c61-memory.sh` 只看靜態值）。

### T3（2026-10-07）— HTTP service 抽出與驗收（R6、R7、R8）

- 契約：`tmp-briefs/T3-contract.md`（`src/apps/upload/http.rs`：`serve_request<S: embedded_io_async::{Read,Write}>(&mut S, &SdStorage) -> ServerEvent`；不關 socket）。oracle：`assets/upload.html`（瀏覽器 client）＋契約。
- test author：`host/tests/upload_http.rs`（24 測試；fake socket 可設切片大小／輸入耗盡行為／單次 write 上限；虛擬 SD 失敗注入）。red-proof：`error[E0432]: unresolved import pulp_host::apps::upload_http`（import 層級）。test author 另以自寫參考 stub 驗證 24 測試可通過，並做 3 個 mutation（delimiter 少 CRLF、覆寫變附加、吞 append 錯誤）皆被抓到。expected value 來源：requirement／契約／upload.html，無值取自實作輸出。
- implementer：抽出 `http.rs`；`mod.rs` 只留 accept／close／mDNS／畫面；`host/src/apps.rs` 加 `upload_http`；`host/src/drivers/storage.rs` 補 `write_file`／`append_root_file` stand-in（同簽名、轉呼叫 VirtualStorage）；`host/Cargo.toml` `embedded-io-async` 進 `[dependencies]`。
- **修掉的真實缺陷（現行碼）**：(1) request header >1024 bytes 回 431 後未 flush；(2) **POST /delete 的 body 比 Content-Length 短時，舊碼用殘缺檔名去刪檔**（Content-Length 8、實到 `BOO` → 會刪掉真實存在的 `BOO` 並回報 `Deleted`），且會取超過 Content-Length 的 initial body 當檔名。修法：只取 `min(Content-Length,13)`，讀不滿 → 500＋`DeleteFailed`，不刪。附帶行為變化：POST /delete 無 Content-Length 現視為空檔名而失敗（瀏覽器端 client 一定送 Content-Length）。
- 契約澄清：request header >1024 → 431／`Nothing`；multipart part headers >2048（work buffer）→ 500／`UploadFailed`。
- weakening gate（/tmp/wifi-snap/T3）：`scripts/`、`board-logic/` 零差異；`host/` 僅上列 glue／stand-in／Cargo；測試檔 mtime（10:36）早於實作檔（10:41），未被改。
- orchestrator 獨立驗證：`upload_http` 24 通過；host 全套無 FAILED；`check-wifi-build.sh` exit 0。implementer 另報：check-host-boundary PASS、board-logic 全綠、check-offline-boundary 0 FAIL、c61／x4／c61-wifi／x4-wifi build 通過。
- **預算追蹤**：wifi 變體 statics 204,256 B，餘裕 3,216 B（T1b 3,248 → T2 3,256 → T3 3,216；`http.rs` 沒新增 static，變動來自程式碼／.rwtext 配置）。T4–T5 須持續盯緊。
- 設計觀察（不改）：`GET /files` 在 SD 不存在／列表失敗時仍回 200 `[]`（瀏覽器會顯示「No files」誤導使用者）；R8 的字面對象是上傳，故未改，列入產品決策待使用者決定。失敗後是否殘留半個檔案：requirement 未規定，未斷言。
- UNVERIFIED：async future 大小（generic 化後未量）、socket 實際 close 順序、真機 TCP 行為、大量檔案（>64 筆）列表。

### T4（2026-10-07）— mDNS 與 IP 顯示（R9；R10／R11 的驗收在 T5）

- 契約：`tmp-briefs/T4-contract.md`（`src/apps/upload/mdns.rs`：`handle_packet`／`ip_label`／`Datagrams` trait／`serve`；oracle 為 RFC 1035 §4.1／§4.1.4、RFC 6762 §5／§6／§10，不是現行碼）。
- test author：`host/tests/upload_mdns.rs`（58 測試；獨立 DNS 解析器；fake `Datagrams`；30k 隨機封包 fuzz）。red-proof：`error[E0432]: unresolved import pulp_host::apps::upload_mdns`（import 層級）。test author 以暫存 stub＋約 40 個 mutant 驗證：38 被擊殺；存活 2 個為等價 mutant（指標指向 header 內與「不可達」等價；保留 label 位元走 skip 路徑）；另「指標跳數無上限」僅被 fuzz 的 60 s 保險擋下，屬弱點。
- implementer：新增 `mdns.rs`；`mod.rs` 刪舊 mDNS；新增 `MdnsSocket`（impl `Datagrams`）／`serve_mdns`（bind 一次、`join_multicast_group(224.0.0.251)`）／`serve_http`；主迴圈改 `select4(runner.run(), serve_http, serve_mdns, drain_until_back)`；`Cargo.toml` 的 `embassy-net` 加 `multicast` feature；`host/src/apps.rs` 加 `upload_mdns`。smoltcp 0.12 multicast group 預設容量 4（`build.rs` 內建），只用 1 個。
- **修掉的真實缺陷**：(1) mDNS 收任何封包就返回 → 把進行中的 HTTP 請求 drop；(2) `runner.run()` 每圈重建；(3) 從不 `join_multicast_group`（smoltcp `has_multicast_group` 對未加入的 224.0.0.251 回 false，現行碼實機收不到查詢；source 證據，未實測）；(4) 舊 parser 只看固定 offset，不支援壓縮指標／多 question／EDNS／known-answer。
- weakening gate（/tmp/wifi-snap/T4）：`scripts/`、`board-logic/` 零差異；`host/` 只多 `upload_mdns` 膠水與註解；測試檔 mtime 早於實作檔。
- orchestrator 獨立驗證：`upload_mdns` 58 通過；host 全套無 FAILED；`check-wifi-build.sh` exit 0（statics 204,264 B，餘裕 3,208 B）；`check-offline-boundary.sh` 0 FAIL。implementer 另報：board-logic 331 通過、check-host-boundary PASS、四個 build 通過。
- 索引操作註記：implementer 中途用過 `git stash`／`pop`，index 被改動；orchestrator 驗證工作樹檔案齊全、stash 為空、內容未丟失（index 僅為 rename 標記）。
- UNVERIFIED：實機是否真收到 multicast（IGMP join 被 AP 接受、Wi-Fi 驅動放行 `01:00:5e:00:00:fb`、smoltcp multicast 路徑）；Wi-Fi power-save／DTIM 對 multicast 的影響；>512 bytes 的查詢被丟棄（刻意取捨）；自送回應是否回環（QR=1 不會被重答，host 已覆蓋）。

### T5（2026-10-07）— 退出／失敗／再進入的所有權（R10、R11；R9 相關部分）

- 契約：`tmp-briefs/T5-contract.md`（`src/apps/upload/session.rs`：`run(...) -> SessionEnd`；`ConnectError::RadioUnavailable`）。oracle：R10／R11 與契約的「階段 future 先 drop、C 後 drop、皆在 run 返回前」。
- test author：`host/tests/upload_session.rs`（18 測試；fake `C` 帶 singleton 所有權權杖，事件序列驗證 drop 順序；重入 100 次輪流 10 條路徑）。red-proof：`E0432 unresolved import upload_session`＋5 個 `E0599 no variant RadioUnavailable`（orchestrator 重跑確認 6 個錯誤）。test author 以自寫參考 stub 做 12 個 mutation，全數被抓（每個至少 2 項測試失敗）。expected value 來源：requirement／契約，無值取自實作輸出。
- implementer（中途被使用者中斷一次，以 SendMessage 接續）：新增 `session.rs`；`connect.rs` 新增 `RadioUnavailable` 與共用 `within()`；`mod.rs` 以 `Net { runner, stack, controller }`（欄位序＝drop 序）走 `session::run`，`acquire` 用 `try_station()`（None → `RadioUnavailable`，不 panic）；`WIFI::steal()` 由 `manager.rs` 搬進 `acquire`；錯誤畫面改在 `run` 返回後顯示；`baseline.md` 新增「T5 所有權審查」；`scheduler_c61.rs` 只改 `upload_available` 註解。
- 第二次紅燈（implementer 自報）：`acquire` 放進 select 內時 `one_hundred_consecutive_sessions…` 失敗（acquire 60 次 < 門檻 70）；改為 `acquire` 在 select 之前同步執行後轉綠。測試檔未改。測試註解（「BackImmediate 可不 acquire」）與 `>= 70` 門檻互相矛盾，實作兩邊都滿足。
- weakening gate（orchestrator 對 /tmp/wifi-snap/T5）：`scripts/`、`board-logic/` 零差異；`host/` 只多 `upload_session.rs`（新測試）與 `host/src/apps.rs` 膠水；`scheduler_c61.rs` 對快照只增註解；測試檔 mtime（19:16）早於實作檔（19:25），既有測試未動。
- orchestrator 獨立驗證：`upload_session` 18／`upload_connect` 18／`upload_http` 24／`upload_mdns` 58 全通過；`check-wifi-build.sh` exit 0。implementer 另報：host 全套（866 測試）、check-host-boundary、board-logic（321／3／7）、check-offline-boundary、四個 cargo build 皆通過。
- **預算追蹤**：wifi 變體 statics 204,664 B，stack 51,960 B，**stack 餘裕僅 2,808 B**（T4 3,208 → 2,808）。增量主要來自 `embassy_main POOL`（19,496 → 19,704 B）。T6 不得再增加 static／async future 大小。
- 修掉的缺陷：`Interface::station()` 在 interface 已被占用時 panic → 改 `try_station()`；`WIFI::steal()` 原在 manager 層不受 session 約束；失敗畫面原在資源釋放前顯示。
- **待決（需使用者決定）**：`bring_up` 在 firmware 已無呼叫者，只剩唯讀的 `upload_connect.rs` 使用，現以 `#[allow(dead_code)]` 保留。依「不保留相容層」原則應移除，但需連同改寫 `upload_connect.rs`（既有測試）。
- UNVERIFIED：esp-radio deinit 是否把 radio 內部 allocation 全數還給 heap、重複 N 次的 heap 高水位、重新 init 耗時、esp-rtos 任務是否殘留、`wifi_init` 中途失敗是否回滾、X4 的 refresh 被 BACK 取消時 panel 狀態。（source 證據見 `baseline.md`「T5 所有權審查」。）R10 的「返回可操作閱讀介面」只做 compile＋source 審查（special-mode Pop＋full redraw），實機待驗。

### T6 起點（2026-10-07）— 缺口審計與使用者決定

- 唯讀審計（R1–R14 證據對照、缺口、預算素材）完成；wifi 變體 statics 204,664 B、stack 餘裕 2,808 B；離線 statics 180,940、stack 75,684 不變。`baseline.md` 的 T1b 預算表已過期，T6 重寫。
- 審計實跑：check-wifi-build 89 ok／0 FAIL；check-offline-boundary 27 ok／0 FAIL；upload_connect 18／upload_http 24／upload_mdns 58／upload_session 18；wifi_budget 7。
- **使用者決定（2026-10-07）**：
  1. `GET /files` 在 SD 未掛載／列表失敗時改回 500（以 R8 為準；現況 200 `[]` 使 upload.html 的「Could not list files」分支不可達）。
  2. 刪除 `bring_up`（firmware 無呼叫者）；其 9 個測試改走 `session::run`，由 test author 處理並單獨記錄 weakening gate。
  3. HTTP 單請求逾時**不改**（G9）：標 UNVERIFIED，交 T7 實機確認 smoltcp 對靜默對端的行為。
- 不需決定、由 T6 直接處理：G1（DHCP 階段 `config_v4()` 為 None 時不得以 0.0.0.0 進 Serving）、G6／G7／G8 補測試、G12 接線（run-software-acceptance 納入 check-wifi-build；README 過期描述）。
- 刻意不做（記為已知）：G4 上傳失敗的殘留檔語意（spec 未規定；先釘現況會鎖死未決的產品語意）；G5 SD close 錯誤被吞（既有碼 `kernel/src/drivers/storage.rs`，非本 change 範圍）；G2 錯誤畫面 BACK（host 無接縫）。

### T6 test author（2026-10-07）

- 新增 `host/tests/upload_regression.rs`（15 測試）；`upload_connect.rs` 移除 9 個 `bring_up` 測試（9 個 `check_credentials`／`Limits`／`lines` 不變）；`upload_session.rs` 新增 2 個（`empty_password_fails_before_acquire`、`timeout_duration_follows_configured_limits`），原 18 個未動。
- red-proof（orchestrator 重跑）：`upload_regression` 15 個中 11 綠、4 紅。紅燈 2 個為預期（`get_files_with_a_failing_listing_is_refused`／`get_files_without_a_mounted_card_is_refused`：`failure response contains "200 OK"`）；**另 2 個為非預期的真實缺陷**：`delete_never_reaches_outside_the_card_root`（body `../DECOY.TXT` 回 200 並刪掉根目錄外的檔案）、`delete_refuses_names_with_path_separators`（`/KEEP.TXT` 等被正規化後刪到真檔）。原因（orchestrator 讀 `http.rs:155-210`）：`/delete` 只檢查空與長度，未驗證名稱即交給 `storage::delete_file`；`/upload` 才有名稱驗證。真實 FAT 上 `..` 是否生效未驗；HTTP 層驗證與之無關，屬必要防護。已補入 implementer 派工單 1b。
- mutation（黑箱，10 個）全被抓；expected value 來源：requirement／upload.html／使用者決定，無取自實作輸出。stub 已清。
- weakening gate（B）：9 個被移除的 `bring_up` 測試皆有對應（逐一列表見 test author 報告）；`session` 既有 18 個未放寬。**需知悉**：`completes_immediately_without_waiting_for_the_deadline` 由 `stages_run_in_order_once…` 取代，門檻由 500 ms（期限 2 s）放寬為 1.5 s（期限 20 s）——仍遠小於期限，語意保留，但不是一對一。空密碼案例原本只在 `upload_connect`，現補進 `upload_session`。
- 含糊（未斷言）：第 65 筆之後是否截斷；`POST /upload?x=1`（upload.html 不送 query，未寫測試）；name 前後空白被 trim 且在長度檢查前；boundary 120／part headers 2048 的恰好邊界；其他失敗路徑的殘留檔語意。

### T6 implementer 與驗收（2026-10-07）— 回歸、缺陷修正、預算報告（R1–R14）

- implementer：`http.rs` GET /files 先 list、失敗回 500（成功路徑 byte 不變）；`/delete` 以 `is_plain_83`（內部用 `/upload` 已有的 `sanitize_83`，名稱須與清理後結果不分大小寫相等）驗證，`/`、`\`、`..`、`.`、空字串、>12 bytes 皆回 500／`DeleteFailed` 且不呼叫 storage，`/upload` 行為不變；`connect.rs` 刪 `bring_up`；`mod.rs` dhcp 階段改為 `select(runner.run(), ipv4)`，`config_v4()` 為 None 時每 100 ms 輪詢，受 `limits.dhcp` 約束（不再以 0.0.0.0 進 Serving，`session::run` 簽名不變）；`scripts/check-wifi-build.sh` 新增「smoltcp 無 proto-ipv6」斷言；`scripts/run-software-acceptance.sh` 納入 check-wifi-build stage 並列 7 項 Wi-Fi UNVERIFIED；README 更新；新增 `budget-report.md`，`baseline.md` 的 T1b 預算表改指向它（只留一份數字）。
- red-proof：實作前 `upload_regression` 11 綠 4 紅（orchestrator 與 implementer 各自重跑一致）；實作後 15／15。
- weakening gate（orchestrator 對 /tmp/wifi-snap/T6）：`host/src`、`board-logic`、`kernel` 零差異；`host/tests` 只有 test author 階段的改動（`upload_regression.rs` 新、`upload_connect.rs` 刪 9 個 `bring_up` 測試、`upload_session.rs` 增 2 個），implementer 階段未動測試；`scripts/` 對快照只有**新增**（一條斷言、一個 stage、7 行 UNVERIFIED、註解重新編號）；`src` 僅 `connect.rs`／`http.rs`／`mod.rs`。
- orchestrator 獨立驗證：`upload_regression` 15、`upload_connect` 9、`upload_session` 20、`upload_http` 24、`upload_mdns` 58 全通過；`check-wifi-build.sh` exit 0（`smoltcp lacks feature proto-ipv6` ok；statics 204,664 B、stack 51,960 B、餘裕 2,808 B，與 T5 相同）；`bring_up` 無殘留（僅 SD 驅動同名函式）。implementer 另報：host 全套（874）、check-host-boundary、board-logic（321／3／7）、check-offline-boundary（離線 statics 180,940／stack 75,684 不變）、四個 cargo build、rustfmt --check 皆通過；async future 只有 dhcp closure 48 → 56 B，外層不變。
- **新發現並修掉的缺陷**：`POST /delete` 路徑穿越（`../X` 刪到 SD 根目錄外；`/X`、`X/`、`./X` 被正規化後刪到真檔）。真實 FAT 上 `..` 是否生效未驗；HTTP 層驗證與之無關。
- **本 task 之外的已知項目（刻意不處理）**：G4 上傳失敗的殘留檔語意、G5 SD close 錯誤被吞（`kernel/src/drivers/storage.rs`）、G2 錯誤畫面 BACK（host 無接縫）、G9 HTTP 靜默對端逾時（使用者決定不改，T7 實機）、`POST /upload?x=1`、第 65 筆以後截斷。
- UNVERIFIED：G1 的 IPv4 輪詢對真實 router 的行為（host 測不到，僅 source＋compile）；radio 執行期 heap、stack 高水位、PSRAM 分工、重入高水位、52 KiB 對閱讀路徑的影響（詳見 `budget-report.md`）。
- 備註：兩個 statics 數字（204,664 vs 204,656）是同一份輸出內的口徑差（含對齊），已於 budget-report 註明。orchestrator 未逐行審閱 `budget-report.md` 的敘述，數字以 `check-wifi-build.sh` 輸出交叉核對。

### T7（2026-10-07）— 實機驗收流程與未驗記錄（R13、R14）

- 性質：文件交付，無程式碼，故無 red-proof／mutation。**不宣稱任何實機結果。**
- 新增 `hardware-acceptance.md`：R13 軟體證據指令；前置條件；W1 association、W2 DHCP、W3 HTTP 內容、W4 mDNS、W5 重入、W6 功耗、W7 HTTP 靜默對端、W8 radio heap／stack／PSRAM；追蹤表（18 處 UNVERIFIED，無任何 PASS；文中的 PASS 僅為判定準則）；風險表；log／畫面字串對照（附檔案:行號）。`run-software-acceptance.sh` 僅改結尾 heredoc 措辭（電流涵蓋退出後）＋指向文件一行，`bash -n` 通過；README 加 3 行指向。
- orchestrator 抽查：`mod.rs:116`、`:145`、`http.rs:99`、`connect.rs:42-54` 的字串與文件一致；`scheduler.rs:608` 的 `stats:` 格式存在；`src`／`kernel`／`host/src`／`board-logic` 對 T6 快照僅有 T6 已驗證的 `connect.rs`／`http.rs`／`mod.rs` 差異（T7 零程式改動）。
- **文件標示「不存在 instrumentation」**：upload 期間 heap 時間曲線（主迴圈暫停，`stats:` 只在退出後出現）、radio／RTOS task stack、裝置端收到 mDNS query 的 log、PSRAM／internal 分項、radio 內部配置與 esp-rtos 任務殘留、RSSI、log 時間戳。
- **文件自訂、非 spec 數值（需使用者確認）**：重入 N=10；mDNS 10 次中 9 次且 5 s 內；`hwm` 門檻 48K（警示）／50K（fail）；`used` 連續 5 次遞增視為疑似洩漏。
- **T7 作者發現的產品／文件問題**：(1) SD 未掛載時 `GET /files` 回 500 在實機無法由正常流程達成（拔卡後設定重載、憑證被清，只會看到「No WiFi credentials!」），僅 host 證據。(2) 憑證載入時 SSID 截 32、密碼截 63，故 `check_credentials` 的超長分支不可由 SETTINGS.TXT 觸發；**超長密碼被靜默截斷後以錯誤密碼連線**。(3) `.epub` 上傳後存為 `.EPU`（8.3）；`GET /files` 只列 TXT／EPUB／EPU／MD，其他副檔名可寫入但不顯示，與 upload.html 的 any file 有落差。(4) embassy-net 與 smoltcp 兩份上游文件對靜默連線是否被回收說法矛盾（W7 實測）。(5) README 的 stack 高水位描述不準；`proposal.md` 的 Impact 仍寫 `src/apps/upload.rs`；progress T2 備註的 `AssociationFailed` 已過期（T5 後為 `RadioUnavailable`）。(6) 所有 T1–T6 改動未 commit，韌體 commit 欄需先 commit 才能精確重現。

### Final review continuation（2026-10-07）

- 三個獨立 reviewer 完成整體／security／budget review；完整 synthesis 見 `final-review.md`，原始 review 與新 red-proof 保存在 `review-evidence/`。
- 修正：超長 delete body 被截斷後刪掉有效檔名。獨立 test author 先重現 200/Deleted 與資料刪除；implementer 只修改 length guard，測試唯讀（comment hygiene 除外）。新增 3 tests；HTTP 24／regression 18／connect 9／mDNS 58／session 20 全綠，無既有斷言放寬。
- hygiene：清除 source／tests／scripts 的 spec ID；proposal Impact 更新為 `src/apps/upload/`。
- 尚未解決 MUST：合法 FAT 8.3 名 `A(B).TXT` 列得出但新 validator 刪不掉；R6 與 T6 共用 upload validator 派工規則衝突，已詢問使用者，不自行改 spec 或 naming 行為。Final review **BLOCK**，尚不可 `/spec-archive`。
- RECALL：Wi-Fi reservation 邊界缺 behavioral test；目前 wiring 正確，列 follow-up。
- fresh 全套（修正前）：host 874／board 331／check-wifi-build exit0；執行期／實機仍 UNVERIFIED。歷史 weakening gate 的刪9 tests／500ms→1.5s 均保留，不宣稱完全無 weakening。
- 第二項：`feat/iansui-cjk-host-foundation` 的全部提交已在 `onepage`；無可 cherry-pick 的新 commit。`26febe1` 在另一分支，待使用者確認來源，不擅自採用。
- 修正後完整 `check-wifi-build.sh` exit0，statics 204,664／stack 51,960 B 不變；原始 build／HTTP／support 測試輸出與 re-review 已保存 `review-evidence/`。

### Remaining MUST／RECALL 修復（2026-10-07）

- 使用者授權兩項修復；spec R6 與 proposal 記錄 FAT delete 相容性，撤回 T6 shared-validator 派工限制。
- 獨立 test author 寫 FAT punctuation／exact-name refusal／upload compatibility tests；production implementer 測試唯讀，僅改 http delete validator／移除 trim。實際 pre-fix HTTP red 與 expected 來源保存在 `review-evidence/fat-name-red-proof.md`。
- RECALL：新增2個 aggregate budget behavioral tests，預算邏輯本來正確，coverage-only。隔離副本 wrong-variant 與 bypass-enforcement mutations 各 exit101（8綠1紅），證明新測試會抓到真退步；未修改 repo production memory。
- fresh host 880／board 333 全通過，check-wifi-build exit0；statics204,664／stack51,960 B 不變。新測試與原測試 weakening gate 無放寬／skip／替代呼叫路徑；歷史 weakening 記錄仍保留。
- 獨立 final re-review APPROVE，0 unresolved MUST／RECALL。完整報告與原始 console 輸出已保存 `final-review.md`／`review-evidence/`；實機與既有G5限制維持原狀。

### Spec-archive audit（2026-10-07）

- 完成14列 evidence.md；11列有記錄 red-proof，3列缺歷史 red（R3 preexisting-green、R13 artifact、R14 documentation）。獨立 audit 同意；不補造red、不自動豁免。
- 所有7 tasks checked；13條原 requirement 無 provenance tag，R6只有clarification帶human；spec無assumed requirement，但proposal52KiB runtime sizing仍assumed。
- Skill要求missing red-proof stop/open item；尚未move或promote，待使用者接受明示的證據例外與未標註來源。軟體review APPROVE不等於archive gate已過。

### Archive accepted（2026-10-07）

- 使用者在解釋實機UNVERIFIED與歷史red-proof缺口是不同狀態後，以「好」接受以軟體候選版封存。
- evidence.md：11列有歷史red、3列accepted exceptions（R3／R13／R14）、0 unresolved archive item。13條未標provenance與R6部分human標註保留，不補造來源；proposal52KiB runtime假設仍未量測。
- 移入 `specs/changes/archive/onepage-wifi-upload/`；硬體W1–W8與已知G5維持未驗／限制，不宣稱完成實機驗收。未promote living spec。
