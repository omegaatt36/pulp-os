# T5 test author brief — onepage-wifi-upload

你是 **test author**。只寫測試，**不得讀取或修改 production 實作**：`src/**`、`kernel/src/**`、`board-logic/src/**`、`Cargo.toml`（根）。可讀：`specs/changes/onepage-wifi-upload/{spec.md,proposal.md}`、`tmp-briefs/T5-contract.md`、`host/tests/*.rs`（特別是 `upload_connect.rs`、`upload_mdns.rs` 的 block_on／fake／外層保險慣例）、`host/Cargo.toml`。repo `/Users/raiven_kao/dev/pulp-os`，不要 commit。

## Task
T5：修正退出／失敗／再次進入的 runner、socket、interface／radio ownership，驗收 reader 返回。（本 task 的 host 驗收範圍：`session::run` 的階段順序、取消、所有權釋放與重入；真實 esp-radio deinit 與 reader 返回的 UI 只能 compile＋source 驗收，實機待驗。）

## Requirements（未標註 provenance；模糊就停下回報）
- R9: WHILE upload 已連線 THE SYSTEM SHALL 提供既有 `pulp.local` mDNS 回應。（本 task 的相關部分：連線期間持續服務的結構在被取消／失敗時不留下殘餘）
- R10: WHEN 使用者退出或連線失敗 THE SYSTEM SHALL 釋放 network／radio 資源並返回可操作的閱讀介面。
- R11: WHEN 再次啟動 upload THE SYSTEM SHALL 不因殘留 controller／interface ownership 失敗。

## 契約
`/Users/raiven_kao/dev/pulp-os/specs/changes/onepage-wifi-upload/tmp-briefs/T5-contract.md`（完整讀；測試只能針對它）。

## 交付
新檔 `host/tests/upload_session.rs`（`pulp_host::apps::upload_session` 與 `upload_connect`）。以你自己的 fake context `C` 驗證——`C` 帶一個共享的「所有權權杖」計數器（模擬 `Interface::try_station()` 的 singleton：`acquire` 在權杖已被持有時回 `Err(ConnectError::RadioUnavailable)`；`C` 的 `Drop` 歸還權杖並記錄事件順序）。階段 closure 用 `async |c| { ... }` 風格（AsyncFnOnce），可在內部持有會計數 Drop 的 guard，以便偵測「階段 future 是否在 run 返回前被 drop」。用 `embassy_futures::block_on`，外層有界保險（worker thread＋牆鐘 timeout，與既有測試一致；不要用 embassy 的 timer 佇列開太多 timer：generic-queue-8 只容 8 個）。涵蓋：
1. 順序與資料流：憑證有效 → acquire→associate→dhcp→serve 依序各被呼叫一次；dhcp 的回傳值 T 交給 serve；back 在 serve 開始後才完成 → `Exited(Serving)`。
2. 取消於每個階段：back 在 associate 懸而未決時完成 → `Exited(Associating)`、dhcp／serve 從未被呼叫；back 在 dhcp 懸而未決時完成 → `Exited(Dhcp)`、serve 未呼叫；back 在 serve 中完成 → `Exited(Serving)`。每種情況：返回時 C 已被 drop、權杖已歸還、階段 future 內的 guard 已 drop，且 drop 發生在 `run` 返回**之前**（用事件序列驗證：`階段 guard dropped` → `C dropped` → `run returned`）。
3. 憑證缺失／不合法 → `Failed(MissingCredentials／InvalidCredentials)`，acquire 與其後 closure 從未被呼叫。
4. acquire 失敗：`Err(RadioUnavailable)` → `Failed(RadioUnavailable)`，其後 closure 從未被呼叫；權杖被占用時的 acquire（fake 回 `RadioUnavailable`）同樣如此且不 panic。
5. 失敗路徑釋放：associate 回 `Err` → `Failed(AssociationFailed)`；associate 永不完成（極小 `Limits`）→ `Failed(AssociationTimeout)`；dhcp 永不完成 → `Failed(DhcpTimeout)`。每種：返回時 C 已 drop、權杖已歸還；後續階段 closure 未呼叫；耗時 ≥ 設定的 limit 且有界。
6. back 與階段的競爭：back 一開始就已完成（立即 ready）且 associate 懸而未決 → `Exited(Associating)`，C 若已建立必須 drop（權杖歸還）；back 立即 ready 時整個 `run` 在有界時間內返回。
7. **重入（R11）**：同一個權杖計數器下，連續執行 100 次 `run`，每次選用不同路徑（輪流：成功後 back、back 於 associate／dhcp、associate Err、timeout、DHCP timeout、憑證錯誤、acquire 失敗），**每一次 acquire 在應該成功時都成功**（不會因殘留權杖而 `RadioUnavailable`）；結束時權杖計數為 0、C 的建立數＝drop 數。
8. 取消不洩漏：serve 階段 future 在取消時被 drop（用 guard 計數）；`run` 返回後不再有任何 pending 工作（用 waker／poll 計數：返回後再也不被 poll）。
9. `ConnectError::RadioUnavailable.lines()`：非空、各行非空、第一行與其他 5 個 variant 相異（`"WiFi init failed!"` 為第一行）。
10. `bring_up`／`check_credentials`／`Limits` 行為不變：不需重測（`upload_connect.rs` 已涵蓋，唯讀）。

## 紀律
- red-proof：實作前執行 `scripts/host-test.sh --test upload_session`，完整貼失敗輸出（預期 unresolved import）。
- expected 只能來自 requirement／契約；不得抄實作輸出。不得 `#[ignore]`／skip。
- 你可以像之前的 test author 一樣自寫**暫存**參考 stub（放在 repo 外，例如 `/tmp/session_stub/`）證明測試可通過並做 mutation（例如：先 drop C 再 drop 階段 future；back 被忽略；acquire 失敗仍呼叫 associate；失敗路徑漏 drop C；重入時不歸還權杖），報告 mutation 結果；stub 不得留在 repo。
- 報告（正體中文）：(1) 檔案，(2) 每項測試與 expected 來源，(3) 實作前完整失敗輸出，(4) 含糊之處，(5) 契約讓 requirement 無法測的地方，(6) mutation 結果。
