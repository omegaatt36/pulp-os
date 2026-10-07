# T3 implementer brief — onepage-wifi-upload

你是 **implementer**。repo `/Users/raiven_kao/dev/pulp-os`，branch `onepage`，不要 commit。

## Task
T3：整合 network runner／TCP／UDP 與既有 HTTP upload／list／delete，驗收檔案內容及 SD failure。

## Requirements（未標註 provenance；與現實衝突就停下回報）
- R6: WHILE upload 已連線 THE SYSTEM SHALL 提供既有 HTTP upload／list／delete 行為。
- R7: WHEN 上傳成功 THE SYSTEM SHALL 在 SD 保存與輸入位元組一致的檔案。
- R8: IF HTTP／SD 處理失敗 THEN THE SYSTEM SHALL 回報失敗而不顯示上傳成功。

## 契約
`/Users/raiven_kao/dev/pulp-os/specs/changes/onepage-wifi-upload/tmp-briefs/T3-contract.md`。

## 唯讀測試
`host/tests/upload_http.rs`（test author 已寫；**不得修改**）。先跑 `scripts/host-test.sh --test upload_http` 保留紅燈輸出。

## 範圍
1. 從 `src/apps/upload/mod.rs` 抽出 HTTP 層到 `src/apps/upload/http.rs`（`serve_request`、`handle_upload`、`extract_path`／`find_boundary`／`extract_filename`／`sanitize_83`／`extract_content_length`／`find_subsequence`／`fmt_u32` 等與相關常數、`ServerEvent`）。`mod.rs` 只保留：accept（`TcpSocket::accept`、timeout）、呼叫 `http::serve_request(&mut socket, sd)`、關閉 socket（`close_socket`）、mDNS（T4 才動）、畫面、主迴圈。socket 的 `close`／timeout／accept-retry 不進 `http.rs`。刪除 `mod.rs` 中被取代的舊碼，不留相容層。
2. 若測試暴露現行實作對 R7／R8 的真實缺陷（例如 boundary 跨 read 邊界、work buffer 邊界、錯誤路徑仍回 200、寫入失敗後仍回報 Uploaded、失敗後 `close` 前沒寫完整回應等），修正它；每個修正在報告中標明：測試名、原因、修法。若你認為某測試本身錯了，**停下回報**，不要改測試。
3. `host/src/apps.rs` 加 `#[path = "../../src/apps/upload/http.rs"] #[allow(dead_code)] pub mod upload_http;`；`host/src/drivers/storage.rs` 補缺的 stand-in（簽名與 production 一致，轉呼叫 `VirtualStorage`；必要時 `VirtualStorage` 補對應方法——那是 host stand-in，允許，但只補不改既有行為，且不得在 stand-in 裡放入「讓測試過關」的邏輯）。`host/Cargo.toml` 若測試作者已加 `embedded-io-async` 到 dev-dependencies，需要時把它移到 `[dependencies]`（因為 `http.rs` 以 `#[path]` 被主 crate 引入時需要它）。確認 host 依賴圖仍沒有任何 esp-* crate（`scripts/check-host-boundary.sh --skip-firmware`）。
4. 不要新增大型 `static`（wifi 變體 statics 餘裕僅約 3,256 B；`scripts/check-wifi-build.sh` 的 memory baseline 會抓）；`serve_request` 的緩衝區沿用現有的 stack 陣列即可（hdr 1024／work 2048／64 筆 DirEntry），不要變大。
5. 不碰 mDNS（T4）、退出／重入（T5）。

## 紀律
- weakening gate：不得修改任何既有測試或 `scripts/*`。報告列出你對 `host/`、`scripts/`、`board-logic/` 的所有改動。
- 驗證並貼實際輸出：`scripts/host-test.sh --test upload_http`、`scripts/host-test.sh --locked`（全部）、`scripts/check-host-boundary.sh --skip-firmware`、`scripts/test-board-logic.sh`、`scripts/check-wifi-build.sh`、`scripts/check-offline-boundary.sh`、`cargo build-c61`、`cargo build-x4`、`cargo build-c61-wifi`、`cargo build-x4-wifi`、`rustfmt --edition 2024 --check <改過的每個 .rs>`（若 rustfmt 順著 `#[path]` 改到其他檔，用 `git checkout` 還原）。
- 報告（正體中文）：(1) 改動檔案與理由，(2) red-proof，(3) 驗證結果，(4) 對 host／scripts／board-logic 的改動，(5) 修掉的真實缺陷清單（若有）與 UNVERIFIED／阻塞（含 statics 餘裕變化、async future 大小），(6) 含糊處。
