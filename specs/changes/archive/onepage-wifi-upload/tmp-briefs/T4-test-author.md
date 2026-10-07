# T4 test author brief — onepage-wifi-upload

你是 **test author**。只寫測試，**不得讀取或修改 production 實作**：`src/**`、`kernel/src/**`、`board-logic/src/**`、`Cargo.toml`（根）。可讀：`specs/changes/onepage-wifi-upload/{spec.md,proposal.md}`、`tmp-briefs/T4-contract.md`、`host/tests/*.rs`（特別是 `upload_connect.rs`、`upload_http.rs` 的寫法與 block_on／外層保險 timeout 慣例）、`host/Cargo.toml`。你的測試以**你自己寫的、獨立的**最小 DNS 解析器驗證回應（不要複製契約文字當作「解析」；依 RFC 1035／6762 的欄位布局寫）。repo `/Users/raiven_kao/dev/pulp-os`，不要 commit。

## Task
T4：恢復 mDNS 發現與 IP 顯示，驗收 UDP response。（R10／R11 的退出與重入驗收在 T5，本 task 不測。）

## Requirement（未標註 provenance；模糊就停下回報）
- R9: WHILE upload 已連線 THE SYSTEM SHALL 提供既有 `pulp.local` mDNS 回應。

## 契約
`/Users/raiven_kao/dev/pulp-os/specs/changes/onepage-wifi-upload/tmp-briefs/T4-contract.md`（完整讀；測試只能針對它與 RFC）。

## 交付
新檔 `host/tests/upload_mdns.rs`（`pulp_host::apps::upload_mdns`）：
A. `handle_packet`：
 1. 標準 A 查詢（id 0、flags 0、QD 1、`pulp.local` type 1 class 1）→ true；用你的獨立解析器逐欄驗證回應全部欄位（見契約「回應格式」）：長度 38、ID／flags／四個 count、name labels、type、class（IN 與 cache-flush 位分開驗）、TTL、RDLENGTH、RDATA；多組 IP（含 0.0.0.0、255.255.255.255）。
 2. 大小寫：`PULP.LOCAL`、`Pulp.Local`、`pUlP.lOcAl`；QU bit（class 0x8001）；type ANY（255）→ 回；type AAAA(28)／MX／TXT、class CH(3)／ANY(255) → 不回。
 3. 名稱不符：`other.local`、`xpulp.local`、`pulp.localx`、`pulp.local.evil`、`a.pulp.local`、`pulp.lan`、`pulp`、空 name → 不回。
 4. 不是查詢：QR=1 的封包、opcode 非 0（如 4 NOTIFY、5 UPDATE）→ 不回；QDCOUNT=0 → 不回。
 5. 多問題：`[other.local A][pulp.local A]` → 回；`[pulp.local AAAA][pulp.local A]`，其中第二題的 name 以壓縮指標（0xC00C）指回第一題 → 回；僅有 AAAA 的多題 → 不回；問題數宣告 > 實際 → 不回。
 6. 壓縮指標安全：自我指向、前向指標、指標鏈循環 → 不回、不 hang；qname 以指標開頭指向 header 內 → 不回。
 7. 附加記錄：ARCOUNT=1 帶 EDNS OPT（RR type 41）、ANCOUNT=1 帶 known-answer（接在 question 後）→ 仍回；附加記錄被截斷 → 仍回（只要 question 完整）。
 8. 強健性：把一個有效查詢的**每一個前綴**（長度 0..len）餵入 → 不 panic；長度 < 12／恰 12／剛好少一個 byte 都不回；一組固定種子偽隨機位元組（至少 20,000 個封包，長度 0..600）→ 不 panic、不 hang（外層保險）。
B. `ip_label`：0.0.0.0 → `(0.0.0.0)`、192.168.1.23 → `(192.168.1.23)`、255.255.255.255 → `(255.255.255.255)`、10.0.0.1 → `(10.0.0.1)`、各 octet 10／100／9 的位數邊界；長度 ≤ `IP_LABEL_MAX`。
C. `serve`（用你寫的 fake `Datagrams`：預先給定封包序列；序列耗盡後 `recv` 永遠 pending；`send` 記錄；可設定某次 send 失敗／某次 recv 失敗；用 `embassy_futures::block_on` 搭配 select／輪詢有界地等待到「預期的 send 數量已到」或外層保險 timeout）：
 1. `[noise, query, noise, query, query]` → 恰好 3 個回應、內容皆為對應 `handle_packet` 的合法 38-byte 回應；`serve` 仍未返回（證明雜訊封包不會讓它結束）。
 2. 一長串（至少 200）不相符封包後接一個查詢 → 仍回應。
 3. 某次 `send` 失敗 → serve 不返回，後續查詢仍被回應。
 4. `recv` 失敗 → serve 返回 `Err`，且失敗之前的查詢已被回應。
 5. 回應使用傳入的 `ip`；重新呼叫 `serve` 以另一個 ip → 回應 RDATA 是新 ip。

## 紀律
- red-proof：實作前 `scripts/host-test.sh --test upload_mdns`，完整貼失敗輸出（預期 unresolved import）。
- expected 只能來自 requirement／契約／RFC；不得抄實作輸出。不得 `#[ignore]`／skip。
- 你可以像 T3 的 test author 那樣自寫一份**暫存**參考 stub 來證明測試可通過、並做 mutation 檢查測試敏感度，但 stub 不得留在 repo。
- 報告（正體中文）：(1) 檔案，(2) 每項測試與 expected 來源（引用 RFC 章節），(3) 實作前完整失敗輸出，(4) 含糊之處，(5) 契約讓 requirement 無法測的地方，(6) 你的 mutation 檢查結果。
