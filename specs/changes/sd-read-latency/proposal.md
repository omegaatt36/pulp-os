# sd-read-latency

## Why

成品機實測（`specs/hardware-records/2026-10-08-product.md` K2／F3）：開 14 字的 `ZH1.TXT` 要約 70 秒，400 字的 `ZH2.TXT` 超過 10 分鐘未完成，全程無 loading 畫面、按鍵無回應，使用者看起來像死機。字形本身正確。估算每個不重複字約 5 秒（兩個資料點，未量測）；ZH2 超過 28 分鐘仍未完成。

**T1 結果（`timed-eng.log` 加上使用者口述）：英文路徑基本正常，瓶頸是 CJK 字形準備。** 純英文 `ENG.TXT` 秒開、`ENG.EPUB` 第二次開近乎秒開；第一次開 EPUB 的 ZIP→OPF 4 秒與 6 章快取 12 秒是一次性成本，只有幾秒。Files 列表約 20 秒、從閱讀返回 Files 卡幾秒，與書名 `集合體`（3 個字，約 5 秒／字）的字形準備吻合，是 CJK 造成的。

## Scope

- In：量測 CJK 字庫準備的 SD 讀取次數與單次耗時；依量測消除主要瓶頸（可能在儲存層的每次讀取重開檔，也可能在字庫索引走法）；長時間準備期間的使用者回饋與按鍵回應；Files 列表與返回 Files 時的 CJK 書名字形準備（F5）。英文 EPUB 第一次開啟的幾秒只做量測、不列為目標。
- Out：字庫格式（`.PFN`）與分頁／排版邏輯變更；Wi-Fi；partial refresh；CJK 檔名支援（F2）；按鍵提示版面（F1）；睡眠喚醒（F4）。
- 前置：T1 已完成（見 Why）。

## Impact

`src/fonts/cjk.rs`（`Source::read_at`、`bank_identity`、`open`）、`kernel/src/drivers/storage.rs`（整個儲存層：`read_chunk_in_pulp_subdir` 每次讀取重新 `open_dir(_PULP)` → `open_dir(FONTS)` → 開檔 → seek → read → 關檔）、`src/apps/reader/paging.rs`、`src/apps/files.rs`。

已知：`read_at` 對每個小讀取都走上述完整路徑（已讀碼）；單次耗時與每字讀取次數**尚未量測**，改法依量測結果決定。

## Risk

SD 與 EPD 共用 SPI2，長時間阻塞或改動共享匯流排的時序可能影響 EPD（見記錄檔 F9，原因未明）。快取檔案 handle 會改變 SD 熱插拔時的生命週期；heap／PSRAM 預算已緊（`CJK_STORAGE_BUDGET`）。
