# onepage-host-validation

## Why

讓沒有 OnePage 的 agent 也能驗收正式閱讀程式碼，而不是只通過編譯或另寫一份模擬演算法。提供可重複使用的 host storage、framebuffer 與 fixtures，作為後續 CJK 的驗收入口。

## Scope

- In：host build boundary、正式 UTF-8／paging／rendering／EPUB path、virtual storage、portrait 畫面輸出、英文 fixtures／回歸與可重現指令。
- Out：完整 MCU emulator、類比電氣模型、e-paper 波形、效能／功耗結論、CJK 字庫實作、Wi-Fi。
- 前置：`onepage-c61-port` 軟體候選版完成；後續 integration 以當時正式程式碼為準，保持兩個 firmware targets 可編譯。

## Impact

workspace／build.rs 的 host boundary、kernel 的純邏輯與 strip renderer、fonts／reader 的可共用入口、smol fixtures 及 host test／preview tools。延續 C61 port 已建立的最小測試 seam，只抽出仍缺的接縫，不先重寫 kernel。參考 `kernel/src/util/utf8.rs`、`kernel/src/drivers/strip.rs`、`src/apps/reader/{paging,epubs}.rs`、`build.rs`。

回歸使用自行產生的測試書與可分發 fixture；preview 是 rendering 驗收，不代表 C61 電氣行為。參考入口：[handoff](../../README.md)。

## Assumptions

- **Resolved author assumption（T6c）**：test author 把 `Next` 後第一個 storage read 當成顯示當頁的必要讀取，忽略當頁可能已 prefetch。Production `paging.rs` 對必要讀取傳遞錯誤，對後續 best-effort prefetch 只清除 prefetch 狀態。R5 明確區分這兩種邊界；必要讀取測試先 Next 到非首頁再 Prev，確保回讀的當頁不在 forward prefetch，Error／ReadFailed／read log／reopen assertions 不變。`NextJump` 曾因 lazy page index clamp 到 prefetched page 而不足以建立前置條件，保留失敗證據；另加 Next prefetch 失敗仍 Ready、注入被記錄消耗且可恢復的獨立測試。
- **Resolved porting omission（T6b）**：`flush_writes_the_baseline_record_layout` 移植時漏掉舊 oracle（`scripts/reader-regression/os/tests/bookmarks.rs:172`）的 `k.bookmarks_load();` boot 前置步驟，導致 save 被未載入 cache 忽略、flush 後無檔案。R8 補充明確 cache-load 前置條件；測試只恢復該行，所有 record layout assertions 與 expected values 保持原 oracle，未改 production 或未載入 cache 的 no-op 契約。
- **Resolved miss（T3，spec-apply）**：test author 只看 R4／R5 文字，把「缺檔」猜成 `ErrorKind::NotFound`，且假設 `offset >= len` 讀取回 `Ok(0)`。真實 `kernel/src/drivers/storage.rs` 缺檔回 `OpenFile`（delete 為 `DeleteFailed`），`offset > len` 回 `SeekFailed`；firmware 的 storage 層從不產生 `NotFound`。R4／R5 文字沒有錯，但沒有說明「可驗收的失敗結果」要與 firmware 的 ErrorKind 一致——這是 spec 的盲點。決定：virtual storage 以 firmware 真實語義為準（host 驗收不得驗到與 firmware 不同的行為）。
- **Resolved miss（T2，使用者決定）**：`kernel/src/util/utf8.rs` 的 `decode_utf8_char` 接受 overlong 編碼（`C0 80`→`'\0'`、`C0 AF`→`'/'`），違反 RFC 3629；host 的獨立 oracle（`core::str::from_utf8`）暴露此事。使用者決定**收緊 firmware decoder**（overlong 回 U+FFFD），而不是弱化測試。只影響非法輸入；須重跑 reader regression 與兩個 firmware build。這是 spec 的盲點：R2「使用正式演算法」沒有說明正式演算法本身有缺陷時要修正還是記錄。
- **Resolved miss（T6a，使用者決定：本 change 內修 firmware）**：host 驗收在真實 `ReaderApp` 上暴露兩個出貨 firmware 缺陷，R8「圖片回歸」在修好前無法通過；spec 的盲點是 Scope 把本 change 定位為「驗收」，沒有說明驗收發現 firmware 缺陷時是否在本 change 內修。
  1. **圖片章節頁表與顯示不一致**：`preindex_all_pages`（`src/apps/reader/paging.rs:215`）建頁表前不 prescan 圖高，沿用上一頁／別章殘留的 `img_heights`（計數 0 時退回 `DEFAULT_IMG_H=350` 且未套 inline 40% 上限）；逐頁顯示（`load_and_prefetch`）才用真實尺寸，因此重複／遺失文字，且同章頁數依進入方式而異（E2STORED 章 1：NextJump 5 頁、逐頁 6 頁）。一行 prescan 的最小修法不足：deflate 圖未快取時 prescan 回 `DEFAULT_IMG_H`（`images.rs:955`），快取完成後版面又變（E2DEFL／E3MIXED 實測重複）。決定：**根治**——版面不得依賴快取狀態，頁表與顯示共用同一份圖高來源。
  2. **損壞圖片使背景快取無限重試**：`ImageFailed` 分支（`images.rs:743`）只印 log，未寫失敗標記（其他失敗路徑 `:521/:550/:593` 皆寫空檔），`try_dispatch_nearby_image`（`:764`）每 tick 重派同一張圖，`bg_cache_step` 卡在 `WaitNearbyImage`（`epubs.rs:496`），後續章節快取也卡住；firmware 邏輯相同（`TICK_MS=10`）。決定：寫空標記（與其他失敗路徑一致）。
- **Resolved（T6a 修復，使用者確認）**：缺陷 1 以「每個內聯圖一律保留 `inline_img_max_h(text_area_h)`（文字區高度的 40%）」根治：版面成為「書文字＋設定」的純函數，刪除 prescan 整套機制（`prescan_image_heights`、`peek_cached_image_size`、`peek_source_dimensions`、`img_heights`、`img_height_count`、`MAX_IMAGES_PER_PAGE`、`DEFAULT_IMG_H`；`src/apps/reader/{images,mod,paging}.rs`，+21／−238）。**視覺行為改變（已獲使用者接受）**：小圖置中顯示於約 40% 文字區高的區塊內，上下留白。精確高度需 smol-epub（另一 repo）提供 deflate 圖尺寸窺探，或根 Cargo.toml 加 `miniz_oxide`，列為後續 change。缺陷 2 在 `ImageFailed` 分支寫空標記。驗證：host 193 測試全綠；`check-reader-regression.sh both` head 83／tree 90、golden sha 仍為 pin；X4／C61 build 通過；C61 memory budget ok（statics −16 B、image bytes −2954 B）。
