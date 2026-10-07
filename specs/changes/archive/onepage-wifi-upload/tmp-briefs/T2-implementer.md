# T2 implementer brief — onepage-wifi-upload

你是 **implementer**。repo `/Users/raiven_kao/dev/pulp-os`，branch `onepage`，不要 commit。

## Task
T2：遷移 station controller／credentials／interface API，建立有界 association／DHCP 與錯誤返回。

## Requirements（未標註 provenance；與現實衝突就停下回報）
- R4: WHEN 使用有效 station credentials 啟動 upload THE SYSTEM SHALL 在設定期限內進入可取得 IPv4 的連線狀態。
- R5: IF credentials 缺失／不合法、association 失敗或 DHCP 逾時 THEN THE SYSTEM SHALL 顯示可退出的連線錯誤。

## 契約
`/Users/raiven_kao/dev/pulp-os/specs/changes/onepage-wifi-upload/tmp-briefs/T2-contract.md`。

## 唯讀測試
`host/tests/upload_connect.rs`（test author 已寫；不得修改）。先跑 `scripts/host-test.sh --test upload_connect` 保留紅燈輸出，再實作到全綠。

## 實作範圍
1. `git mv src/apps/upload.rs src/apps/upload/mod.rs`（若 `include_bytes!` 相對路徑因此變動，修正），新增 `src/apps/upload/connect.rs` 依契約。`ConnectError::lines()` 的文字沿用現有畫面用語（"No WiFi credentials!"／"WiFi config error!"／"Connection failed!"…），並為 timeout 等補上簡短英文行；注意 `scripts/check-offline-boundary.sh` 的 `STR_RE`（`WiFi config error!|No WiFi credentials!|Connection failed!` 與 `pulp.local`）要求離線 ELF 不含這些字串；這些字串只在 wifi 建置存在。`lines()` 文字需包含 "Press BACK to exit" 之外的提示行，BACK 提示由畫面另外給（沿用 `render_screen(..., Some("Press BACK to exit"), ...)`）。
2. `run_upload_mode` 改用 `connect::bring_up`：`associate` closure = 建立 `WifiController` 的 `connect_async()`（已是 esp-radio 1.0 API，確認不是舊 API 殘留）；`dhcp` closure = 建 `embassy_net` stack 並 `select(runner.run(), stack.wait_config_up())`，回傳取得的 IPv4 octets。憑證檢查與 `Ssid::try_from`／`Password::try_from` 轉換：`bring_up` 的 `check_credentials` 先擋；之後轉成 esp-radio 型別失敗視為不可能（可用 `ConnectError::InvalidCredentials` 返回，不要 unwrap／panic）。
3. 連線期間（association 與 DHCP）按 BACK 必須能退出（`select` 加 `drain_until_back()`）；錯誤畫面顯示 `error.lines()` 並可按 BACK 返回。**不得新增大型 `static`**（wifi 變體 statics 餘裕只有 3,248 B；`scripts/check-wifi-build.sh` 的 memory baseline 會抓）。
4. `host/src/apps.rs` 加 `#[path = "../../src/apps/upload/connect.rs"]` 的 `pub mod upload_connect;`（含必要 `#[allow]`）；`host/Cargo.toml` 若需要依賴（embassy-futures／embassy-time 皆已有）才改。不得為了通過測試而在 host 側複製邏輯。
5. 不要動 HTTP／mDNS／T3 以後範圍；不要保留相容層或舊 API 殘留。

## 紀律
- weakening gate：不得修改任何既有測試或 scripts/*（包含 `check-wifi-build.sh`）。報告要列出你對 `host/`、`scripts/`、`board-logic/` 的所有改動。
- 驗證並貼實際輸出：`scripts/host-test.sh --test upload_connect`、`scripts/host-test.sh --locked`（全部）、`scripts/test-board-logic.sh`、`scripts/check-wifi-build.sh`（確認 statics 餘裕與 symbol 仍通過）、`scripts/check-offline-boundary.sh`、`cargo build-c61`、`cargo build-x4`、`rustfmt --edition 2024 --check <改過的每個 .rs>`。
- 報告（正體中文）：(1) 改動檔案與理由，(2) red-proof，(3) 驗證結果，(4) 對 host／scripts／board-logic 的改動，(5) UNVERIFIED／阻塞（特別是：statics 餘裕變化、任何無法在 host 驗證的部分），(6) 含糊處。
