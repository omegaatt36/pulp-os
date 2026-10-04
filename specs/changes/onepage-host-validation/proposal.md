# onepage-host-validation

## Why

讓沒有 OnePage 的 agent 也能驗收正式閱讀程式碼，而不是只通過編譯或另寫一份模擬演算法。提供可重複使用的 host storage、framebuffer 與 fixtures，作為後續 CJK 的驗收入口。

## Scope

- In：host build boundary、正式 UTF-8／paging／rendering／EPUB path、virtual storage、portrait 畫面輸出、英文 fixtures／回歸與可重現指令。
- Out：完整 MCU emulator、類比電氣模型、e-paper 波形、效能／功耗結論、CJK 字庫實作、Wi-Fi。
- 前置：`onepage-c61-port` 軟體候選版完成；後續 integration 以當時正式程式碼為準，保持兩個 firmware targets 可編譯。

## Impact

workspace／build.rs 的 host boundary、kernel 的純邏輯與 strip renderer、fonts／reader 的可共用入口、smol fixtures 及 host test／preview tools。延續 C61 port 已建立的最小測試 seam，只抽出仍缺的接縫，不先重寫 kernel。參考 `kernel/src/util/utf8.rs`、`kernel/src/drivers/strip.rs`、`src/apps/reader/{paging,epubs}.rs`、`build.rs`。

回歸使用自行產生的測試書與可分發 fixture；preview 是 rendering 驗收，不代表 C61 電氣行為。參考入口：[handoff](../README.md)。
