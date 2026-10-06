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

## Assumptions

- **[整體 review，resolved miss，2026-10-06 使用者選項「限縮到 CJK 章節」]** R10 邊界政策原寫「heading 持續到 closing marker」未限定範圍；獨立作者的全 Latin 跨頁 heading 測試與原始英文 golden pin（每頁起始重置樣式）衝突，強行通過會使 golden 變 3103 行並改動約 14 列 trace。決定：政策限縮為章節已出現 CJK 後才跨頁保持；全 Latin 測試改為特徵化（鎖定英文每頁重置），為使用者核准的 oracle 修訂，weakening gate 明列。另：TOC 開啟時 suspend 後 CJK cache 未重建（原列 Minor）經 review 升級並已修（`on_suspend` 納入 ShowToc）；heading 風格以 per-page `CARRIED_FALLBACK` 旗標承接。已知殘留：章節首個 CJK 視窗之前的純 Latin 續頁仍依英文行為重置樣式；承接 heading 的 closing-marker 行（及 quote 區塊）的 indent 於 emit 時而非行首蓋章，風格不受影響（reviewer 執行 probe 確認，與既有 fallback 視窗及英文 pin 行為相同）。

- **[T9/T10，resolved miss，2026-10-06 使用者選項 1]** 「保留既有 Latin 行為」原假設與 R15 的全字體失效契約衝突：舊英文 regression 故意固定即時變更字級後保留 stale offsets 的 bug。使用者決定所有文字重建分頁並保留 raw anchor；英文靜態 rendering golden 仍不變。獨立作者替換 tree 的 bug-preservation oracle，以 requirement-derived anchor／全文守恆測試驗收；head 歷史 baseline 保留。此 assertion 政策修訂必須在 weakening gate 明列，不可記成完全未更動測試。

- **[T4，使用者決定]** 畸形 UTF-8 的 U+FFFD 數量採 maximal subpart；替換在消費端解碼時進行，page buffer 保留原始位元組。理由：std 可直接當 oracle；buffer 位移須等於檔案位移，否則 R11 書籤／回頁定位會被 1 byte→3 bytes 的膨脹破壞。
- **[T4，主 session 預設，未經使用者逐項確認]** ①monospace 換行單位：每行至多 `CHARS_PER_LINE` 個 scalar 且不切開任何 scalar（以 byte 或 scalar 計都接受）；②read 回傳不足、結束在 scalar 中間（非 EOF）：不得顯示半個 scalar、不得出現 U+FFFD，真正 EOF 的截斷序列才顯示 U+FFFD；③書名超出欄位容量時保留「容量內最長的完整 scalar 前綴」，不加省略號。
- **[T5，主 session 依 tasks.md 授權的決定]** smol-epub 改為 vendor 進本 repo：`vendor/smol-epub` 取自 upstream `832609d967d4d06452dd49d35b2c9aeb578612de` 的 `git archive`（非髒的 sibling 工作樹；sibling 有 4 個只含 import 排序的未提交改動），根 `Cargo.toml` 的 path 指向 `vendor/smol-epub`；修補清單記在 `vendor/smol-epub/UPSTREAM.md`。理由：不依賴未追蹤的 sibling、revision 隨本 repo 固定、可在 repo 內測試與修補；不需推送任何遠端。`../smol-epub` 不再被使用。
- **[T5，主 session 預設，未經使用者逐項確認]** ①numeric entity：surrogate、>U+10FFFF、0、溢位／超長數字＝一個 entity 一個 U+FFFD；`&#xZZ;`、`&#;`、`&#x;`、缺分號者只要求輸出合法 UTF-8 且前後文不變；②`strip_html_inplace`（container／OPF／TOC 用）也須淨化畸形位元組並解碼 entity；③截斷與淨化並用時，輸出合法、不超過欄位容量、是淨化後全文的 scalar 前綴；④ZIP 超過 256 entries 僅「記錄」不修：索引只保留前 256 個，其後靜默忽略（spine 章節在其後會使開書失敗或 spine 靜默縮短），以 `known_limit` 特徵化測試釘住。
- **[T5，範圍擴張，需使用者知悉]** test author 在位移掃描中發現與 UTF-8 無關的 `zip::extract_entry` 缺陷：DEFLATE entry 壓縮大小為 4096k+1／+2 時誤報 `deflate output exceeds declared size`（同 entry 走 `stream_strip_entry` 正常），會讓特定大小的章節整章打不開。屬 read-buffer 邊界缺陷，已有紅燈測試；交 implementer 修，若修法非小改動（超過約 30 行或需改串流結構）則停下回報，不擴大範圍。
- **[T5，brief 筆誤更正]** 「臺」是 U+81FA＝十進位 33274（brief 誤寫 33218），測試採正確值。
- **[T2，主 session 決定，未經使用者逐項確認]** converter 為新 workspace crate `fontconv`（`pulp-fontconv`），僅用 `fontdue`／`sha2`／`pulp-fontpack`；SD 安裝佈局 `_PULP/FONTS/F000NN.PFN`（8.3，NN＝像素大小補零 5 位）；`font_id`＝f(字型 sha256, pixel_size, 光柵化慣例版本)；字庫收錄字型 cmap 全部非 control 字元，不補 ASCII（Latin 由既有 Bookerly 提供）；`--upstream-url` 必填且僅 ASCII、無前後空白；有 missing 時仍寫出 pack 並以退出碼 3 表示；Iansui 九個字級 pack 合計約 14.8 MB（SD 空間占用，裝置一次只讀目前字級者）。

- **[T6，主 session 的 cache 介面決定]** fontpack cache 接受泛型 caller-owned／borrowed slots 與 bitmap backing，預算包含 cache 物件＋全部 metadata capacity＋全部 bitmap capacity；核心不 alloc、不 unsafe，prepare 失敗後舊頁與部分新頁均不可見，同物件可重試。get 僅借用已準備資料，實際 strip 零 SD read 接入留 T7/T8 驗證。
- **[流程，2026-10-06 使用者同意]** 取消大量／重複 mutant campaign；保留 red-proof、作者分離、weakening gate 與必要 review。T3 剩餘70個不執行，不把未執行記成通過；關鍵風險抽查須有 baseline、單次與整批 timeout。

- **[T7，主 session 的 routine integration 決定]** Latin靜態字型仍優先，未安裝pack時純英文保留原行為；需要CJK fallback才開啟對應pack，缺pack／壞pack／SD錯誤回recoverable preparation failure，合法pack缺scalar才畫合成方框。窄到無法容纳不可分標點組時，允許該組overhang以保全文字與forward progress。geometry窄寬測試入口仅host Rig，沒有替代排版實作。

- **[T7，已解決的記憶體假設誤判]** 一般Box/Vec只使用C61internalheap，不能以256KiBcache上限假設可用2MiBPSRAM。新增FontGlyphs外部class256KiB（從原reserve448KiB挪用，reserve192KiB），降級16KiB；bitmap透過BigBuf進budgetedPSRAM，typedmetadata/scratch留有界internal。原pool-exhaustiontest的單一ChapterText填充前提因reserve改變失效，獨立作者只改setup分配，保留全部assertions。

- **[T7，已解決的行為假設衝突，主 session 暫定]** 原先 controller 決定「缺 pack 即 Error」與既有 UTF-8 fixtures 的「無pack仍可開書」衝突；已詢問使用者，未收到答覆後採相容方案：未安裝pack→尺寸一致合成方框，壞／已安裝pack讀取失敗→error。spec R4 policy 已更新；獨立作者須調整初版failure table的absent期待並新增明確方框assertions，corrupt/read assertions保持。這是行為期待修訂，weakening gate 必須揭露，不默默改assertion。
- **[T7，metadata 與舊 oracle 前提更正]** Firmware 一般 storage helper 把 NotFound 與真正 lookup failure 都包成 OpenFile／OpenDir；optional font lookup 必須用窄介面保留明確不存在與故障的區別。舊 UTF-8 draw helper 把有效 unsupported scalar 當問號 advance，與既定合成方框 advance=pixel_size 衝突；由獨立作者更正 helper 的 metrics 前提，保留原 assertions／cases／tolerance 並揭露修改。
- **[T7，review 邊界決定]** heading state 直到closing marker並跨頁保留；nestedbold/italic不終止heading。不可分標點組超過raw PAGE_BUF時可恢復BufferTooSmall，禁止falseEOF／丟後續文字。每一項以獨立作者單一red testcase交implementer修。
- **[T8，fixture setup 前提更正]** 未安裝pack的EPUB fixture也必須具正常boot建立的 `_PULP` root；初版缺root先觸發cache OpenDir，未走到字庫準備。由獨立作者只補fixture root setup，所有期待／assertions保持。依據實際main與C61scheduler的ensure_pulp_dir_async，不以全域Rig改動影響其他SD fault注入順序。
