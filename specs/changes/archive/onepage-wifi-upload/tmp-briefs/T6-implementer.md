# T6 implementer brief — onepage-wifi-upload

你是 **implementer**。repo `/Users/raiven_kao/dev/pulp-os`，branch `onepage`，不要 commit；**不要使用 `git stash`**。長時間 build 請前景執行並設足夠 timeout；**不要寫 `pgrep -f` 等待迴圈**（會比對到自己的命令列而永不結束）。

## Task
T6：host service／timeout／storage-error 回歸，及 firmware link／heap／stack 預算報告。

## Requirements（無 provenance 標註；與現實衝突就停下回報）
- R4: 有效 credentials 啟動 upload 時，在期限內進入可取得 IPv4 的連線狀態。
- R8: HTTP／SD 處理失敗要回報失敗，不顯示上傳成功。
- R12: Wi-Fi enabled 時保持有證據的 internal heap／stack 與 PSRAM 預算。
- R13/R14: 交付 enabled／disabled link 與 host 測試證據；未做實機驗證者標為未驗。

## 使用者決定（fact）
1. `GET /files` 在 SD 未掛載／列表失敗時回 **500**（現況先寫 200 標頭再 list，失敗回 `[]`）。改成先 list、失敗回 500；`upload.html` 已有非 200 的「Could not list files」分支，client 不用改。
2. 刪除 `bring_up`（firmware 無呼叫者，現以 `#[allow(dead_code)]` 保留）。依「不保留相容層」，連同只服務它的註解／glue 一併清理；`within()`、`at_least`、`check_credentials`、`Limits`、`ConnectError` 保留（`session.rs`／`mod.rs` 使用）。
3. HTTP 單請求逾時**不改**（記為 UNVERIFIED，T7）。

## 唯讀測試
`host/tests/upload_regression.rs`（新）、`upload_connect.rs`（已改寫，不再 import `bring_up`）、`upload_session.rs`、`upload_http.rs`、`upload_mdns.rs`。**不得修改**；若認為測試有誤，停下回報。先跑 `scripts/host-test.sh --test upload_regression` 保留紅燈輸出（預期 list 失敗測試紅）。

## 範圍
1. `src/apps/upload/http.rs`：GET /files 失敗 → 500（見上）。確認「不外洩檔名」、`ServerEvent::Nothing`、現有成功路徑 byte 對 byte 不變。
1b. **POST /delete 名稱驗證（新發現的缺陷）**：`http.rs` 的 `/delete` 把 trim 後的名稱原封不動交給 `storage::delete_file`，沒有 `/upload` 那套名稱驗證。host 實測：body `../DECOY.TXT`（12 bytes）回 200 並刪掉 SD 根目錄外的檔案；`/KEEP.TXT`、`KEEP.TXT/`、`./KEEP.TXT` 被正規化後刪到真檔。要求：delete 與 upload **共用同一個名稱驗證函式**（不要複製規則）；含 `/`、`\\`、`..`、`.`、空字串、超過 12 bytes 者一律回失敗（≥400，event `DeleteFailed`）且不呼叫 storage。`delete_refuses_empty_dot_and_over_long_names`、`delete_refuses_names_with_path_separators`、`delete_never_reaches_outside_the_card_root` 皆須轉綠，且 `upload_http.rs` 既有 24 項與 `delete_sandbox_control_removes_a_plain_root_file`（合法 8.3 名仍能刪）不得退步。報告中說明共用驗證的位置，以及 `/upload` 的既有行為是否因共用而改變（必須不變）。
2. `src/apps/upload/connect.rs`：刪 `bring_up`；修正相關註解。`host/src/apps.rs` 的 `upload_connect` 模組與 `use self::upload_connect as connect;` 保留（session.rs 需要）。
3. **G1（R4 IPv4）**：`mod.rs` 的 dhcp 階段在 `wait_config_up()` 後若 `config_v4()` 為 `None`，不得以 `0.0.0.0` 進入 Serving 並顯示該 IP。**不得改 `session::run` 的簽名**（`upload_session.rs` 唯讀）。作法例：在 dhcp closure 內以輪詢直到 `config_v4()` 為 `Some`（受 session 的 `limits.dhcp` 上限約束）。這部分 host 無法測，以 source 證據＋compile 驗收；另在 `scripts/check-wifi-build.sh` 新增一個斷言：wifi 建置依賴圖中 smoltcp 沒有 `proto-ipv6`（以 `cargo tree -e features` 或同等方式），讓「IPv4-only」成為被守門的事實。**注意：此為對 `scripts/` 的新增，不得放寬或刪除任何既有檢查。**
4. **接線（R13）**：`scripts/run-software-acceptance.sh` 納入 `scripts/check-wifi-build.sh`（C61+wifi 的 link／預算）；若該腳本有 hardware status 區塊，加入 Wi-Fi 實機項目為 UNVERIFIED（association／DHCP／HTTP 內容／mDNS 實收／重入／電流／HTTP 靜默對端逾時）。只做接線與清單，不重寫流程（完整實機流程是 T7）。`README.txt` 的 `upload.rs` 過期描述（約第 184 行）與 wifi 段落（約 62–70 行：補 `check-wifi-build.sh`、`build-c61-wifi`、wifi 變體預算 52 KiB）更新為現況。
5. **預算報告**：新檔 `specs/changes/onepage-wifi-upload/budget-report.md`。以**最終**工作樹重新實測（`scripts/report-c61-memory.sh`／`check-wifi-build.sh`／離線 ELF），表格比較離線 vs wifi：statics、.stack、stack 餘裕、main heap static、internal heap、.dram2_uninit、.rwtext.wifi、`embassy_main POOL`、async future 大小（`-Zprint-type-sizes`，只報事實：`run_upload_mode`／`session::run`／serve closure／`serve_http`，及 `entries:[DirEntry;64]` 5,376 B 的占比）。列出 R12 的 UNVERIFIED（radio 執行期 heap、stack 高水位、PSRAM 分工、重入高水位、52 KiB 對閱讀路徑的影響）。同步修正 `baseline.md` 中已過期的 T1b 預算表（改為指向 budget-report.md，或更新數字，擇一，不要兩邊各存一份數字）。
6. 預算約束：stack 餘裕目前 2,808 B。任何改動不得使 `check-wifi-build.sh` 失敗；不得新增大型 `static`；盡量不要讓 async future 變大（G3 的 list 順序調整若放大 future，改用不增加跨 await 大型局部變數的寫法，並在報告說明）。

## 紀律
- weakening gate：不得修改既有測試；對 `scripts/` 只允許上述兩處**新增**。報告列出你對 `host/`、`scripts/`、`board-logic/`、`kernel/` 的所有改動。
- 驗證並貼實際輸出：`scripts/host-test.sh --test upload_regression`（紅→綠）、`scripts/host-test.sh --locked`（全部）、`scripts/check-host-boundary.sh --skip-firmware`、`scripts/test-board-logic.sh`、`scripts/check-wifi-build.sh`（statics 與 stack 餘裕）、`scripts/check-offline-boundary.sh`（離線 statics 180,940／stack 75,684 應不變）、`cargo build-c61`、`cargo build-x4`、`cargo build-c61-wifi`、`cargo build-x4-wifi`、`rustfmt --edition 2024 --check <改過的每個 .rs>`（只 `--check`；若順著 `#[path]` 報到其他檔的既有差異，不要寫入）。
- 報告（正體中文）：(1) 改動檔案與理由，(2) red-proof，(3) 驗證結果，(4) 對 host／scripts／board-logic／kernel 的改動，(5) 缺陷／UNVERIFIED／阻塞，(6) 含糊處。

## Final review 修復決定（2026-10-07）

使用者已要求修復 FAT delete 相容性 MUST。上文 1b 的「delete 與 upload 共用同一個名稱驗證函式」撤回；以更新後 spec R6 為準：保留既有 upload sanitize，delete 獨立驗證無路徑的合法 FAT 8.3 名，不改名、不正規化路徑，仍拒絕超長 body。
