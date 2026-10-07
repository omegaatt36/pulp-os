# T4 implementer brief — onepage-wifi-upload

你是 **implementer**。repo `/Users/raiven_kao/dev/pulp-os`，branch `onepage`，不要 commit。

## Task
T4：恢復 mDNS 發現與 IP 顯示，驗收 UDP response。

## Requirement（未標註 provenance；與現實衝突就停下回報）
- R9: WHILE upload 已連線 THE SYSTEM SHALL 提供既有 `pulp.local` mDNS 回應。

## 契約
`/Users/raiven_kao/dev/pulp-os/specs/changes/onepage-wifi-upload/tmp-briefs/T4-contract.md`。

## 唯讀測試
`host/tests/upload_mdns.rs`（test author 已寫；**不得修改**）。先跑 `scripts/host-test.sh --test upload_mdns` 保留紅燈輸出。

## 範圍
1. 新增 `src/apps/upload/mdns.rs` 依契約；從 `mod.rs` 刪除舊的 `mdns_respond_once`／`is_mdns_query_for_pulp`／`build_mdns_response`／mDNS 常數，不留相容層。`host/src/apps.rs` 加 `upload_mdns` 膠水（`#[allow(dead_code)]`，並更新該檔頭註解）。
2. `mod.rs` 的 firmware 端：
   - 以 `impl mdns::Datagrams` 包 `embassy_net::udp::UdpSocket`（`send` 送 `GROUP:PORT`）；socket 在 upload 連線期間 **bind 一次**、`stack.join_multicast_group(GROUP)`；緩衝區放 stack（future 內），**不得新增大型 `static`**（wifi 變體 statics 餘裕僅約 3.2 KB；`scripts/check-wifi-build.sh` 會抓）。
   - 根 `Cargo.toml` 的 `embassy-net` 加 `multicast` feature（只有 wifi 建置才會編入）；確認 smoltcp 的 multicast group 容量足夠 1 個群組（預設值即可，說明你看到的值與來源），不得為此多加 feature。
   - **主迴圈重構（真實缺陷）**：現行每圈 `select(runner.run(), select(select(serve_one_request, mdns_respond_once), drain_until_back()))`；`mdns_respond_once` 收到任何封包就返回，會把進行中的 HTTP 請求 drop 掉，且 `runner.run()` 每圈重建。改成：`runner.run()`、HTTP accept 迴圈、mDNS `serve`、`drain_until_back()` 各自**持續**執行，只有 BACK 結束整個 session（`select` 包 `join`／等價結構；mDNS 的 `serve` 若返回 Err 就記 log 並讓該分支永久 pending，HTTP 照常服務）。HTTP 迴圈的 `ServerEvent` log 行為保留。
   - IP 顯示改用 `mdns::ip_label`（取代 `stack_fmt` 組字串）；畫面行為（"http://pulp.local/" 與 IP 標籤兩行）不變。
3. 不碰退出／重入（T5）。注意 `Interface::station()` singleton panic、`wifi_ctrl` 生命週期是 T5 的事，這裡只要不把情況變得更糟。

## 紀律
- weakening gate：不得修改任何既有測試或 `scripts/*`。報告列出你對 `host/`、`scripts/`、`board-logic/` 的所有改動。
- 驗證並貼實際輸出：`scripts/host-test.sh --test upload_mdns`、`scripts/host-test.sh --locked`（全部）、`scripts/check-host-boundary.sh --skip-firmware`、`scripts/test-board-logic.sh`、`scripts/check-wifi-build.sh`（報告 statics 餘裕變化）、`scripts/check-offline-boundary.sh`、`cargo build-c61`、`cargo build-x4`、`cargo build-c61-wifi`、`cargo build-x4-wifi`、`rustfmt --edition 2024 --check <改過的每個 .rs>`（若 rustfmt 順著 `#[path]` 動到其他檔，`git checkout` 還原）。
- 報告（正體中文）：(1) 改動檔案與理由，(2) red-proof，(3) 驗證結果，(4) 對 host／scripts／board-logic 的改動，(5) 修掉的缺陷／UNVERIFIED／阻塞（含 statics 餘裕變化；multicast 實機收包、Wi-Fi power-save 對 multicast 的影響等無法 host 驗證的點要明列），(6) 含糊處。
