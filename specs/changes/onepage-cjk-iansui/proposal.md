# onepage-cjk-iansui

## Why

在 OnePage 移植軟體基線完成後，以 SD 字庫支援繁體中文閱讀。Iansui 在 host 轉換，裝置只載入受預算限制的字形，避免將多字級字庫寫入 firmware 或在 strip rendering 中讀 SD。

## Scope

- In：Iansui host converter、versioned SD bitmap packs、Unicode metrics／glyph lookup、bounded page cache、橫排繁中禁則、中英混排、正文／標題／TOC／UI fallback、UTF-8／parser 回歸。
- Out：完整 Unicode／所有 CJK 覆蓋保證、直排、ruby、完整 shaping、裝置 runtime TTF rasterizer、繁中 UI 翻譯、Wi-Fi／其他系統功能。
- 前置：`onepage-c61-port` 軟體驗收及 `onepage-host-validation` 完成；可在實機尚未到貨時做本 change。

## Impact

font conversion tool／fonts API、reader paging／page preparation、UI labels、SD storage adapters 與 host fixtures；smol 的 UTF-8-safe TOC／entity 修正需要明確的 upstream revision 或 fork dependency，不依賴未追蹤的 sibling 改動。

來源：`build.rs` 只讀 assets/fonts；`src/fonts/bitmap.rs:42` 是 u16 bitmap offset；`kernel/src/kernel/app.rs` 的 draw 是同步；`kernel/src/drivers/sdcard.rs:197` 的 poll_once 不能遇 Pending。先保持現有 blocking storage 契約。

輸入為外部 font-path（本機有 `Iansui-Regular.ttf`）。本地檔案9,447,424 bytes／12,666 Unicode mappings；23px bitmap772,597 bytes，不含索引；有 `𠮷`、無 `𪚥`。Iansui 不含完整 Big5：[上游及 OFL](https://github.com/ButTaiwan/iansui)。現有 body sizes16/19/23/28/35、heading23/27/32/38/46；以 regular CJK fallback 保留 Latin bold／italic。
