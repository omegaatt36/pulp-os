# onepage-cjk-iansui — Requirements

### R1: Host 字庫轉換
WHEN 提供 Iansui font-path 與字級設定 THE SYSTEM SHALL 產生可重現且附 provenance／OFL 的 SD bitmap 字庫。

### R2: Unicode／大 offset
WHEN 字元或 bitmap offset 超出16-bit範圍 THE SYSTEM SHALL 仍可正確索引該字形。

### R3: 字庫 failure
IF 字庫損壞、版本不符或索引越界 THEN THE SYSTEM SHALL 回報 font failure 而不越界存取。

### R4: 缺字 fallback
IF 已選字體不含所需字元 THEN THE SYSTEM SHALL 顯示明確的 missing-glyph fallback。

Policy（2026-10-06 主 session 暫定；選項詢問後未收到回覆，保留既有可開書相容性）：未安裝目前字級的 CJK pack 時，仍可開啟文字，以該字級合成缺字方框顯示；pack 已安裝但損壞、版本不符或讀取失敗時回報 font preparation failure。

查找政策：只有 storage 明確證明 NotFound 才視為未安裝；OpenFile／OpenDir 不代表不存在，metadata／directory I/O 失敗必須傳遞。合成方框沿用既有 missing-glyph 尺寸契約，advance 等於 pixel_size；既有 Latin 與 U+FFFD 字形仍使用原生靜態 metrics。舊 UTF-8 測試 helper 對有效但未支援 scalar 的問號 advance 假設須依此更正，原字元完整性與繪製 assertion 不變。

### R5: 字級量測一致
WHEN 切換目前支援的 body／heading 字級 THE SYSTEM SHALL 使用該字級一致的量測與字形資料。

### R6: Page preparation
WHEN 準備可見頁面 THE SYSTEM SHALL 先載入該頁所需的 metrics 與字形。

### R7: Draw 零 SD read
WHILE 繪製任何 strip THE SYSTEM SHALL 不執行 SD 字庫讀取。

### R8: Cache 預算
WHILE 字形 cache 存在 THE SYSTEM SHALL 不超過設定的記憶體預算。

### R9: Preparation failure
IF cache 無法容納必要頁面資料或 SD 讀取失敗 THEN THE SYSTEM SHALL 回報可恢復的 font preparation failure。

### R10: 中文禁則
WHEN 橫排繁中與 Latin 混合內容換行 THE SYSTEM SHALL 依明確列出的禁則規則避免禁止的行首／行尾標點。

Scenario (R10): Given 包含 `臺灣「繁體中文」，𠮷。` 的窄行寬 fixture，When 在任一字元位置接近行界，Then 行首不出現 `，。、？！）」』】`，行尾不出現 `（「『【`；排版前後文字內容不變。

Boundary policy（主 session 的有界實作決定；2026-10-06 使用者選項 1 限縮）：章節中一旦已出現 CJK 字形視窗，heading 狀態持續到 closing marker，不因頁界、僅含 Latin 的續頁或 nested bold/italic 而結束。「CJK 字形視窗」指任何需要 fallback 字庫的 scalar（含非 CJK 的 Latin 字型缺字，如西里爾字母、符號），觸發範圍以視窗為單位，可能比該 scalar 所在頁早一至兩頁；此為已知且接受的行為。純英文章節維持原始英文行為（每頁起始重置 heading／bold／italic／indent），由原始 golden pin 釘住，不屬本規則。若不可分標點組超出 raw page buffer 容量，回報可恢復的 BufferTooSmall，不發布部分 Ready 頁、不宣稱已讀完整份內容。

### R11: 文字位置一致
WHEN 回到前頁或恢復書籤 THE SYSTEM SHALL 定位至同一文字位置。

### R12: UTF-8 邊界
WHEN 文字跨 read-buffer、page 或 title 截斷邊界 THE SYSTEM SHALL 保留完整 Unicode scalar。

### R13: Malformed-input policy
IF UTF-8 或 numeric entity 含不合法 scalar／overlong 編碼 THEN THE SYSTEM SHALL 以 U+FFFD replacement policy 處理，而不輸出無效 UTF-8。

Policy (R13，使用者於 T4 決定)：替換數量依 maximal subpart（同 Rust `String::from_utf8_lossy`／WHATWG；`C0 80`→2 個、`E0 80 80`→3 個、檔尾截斷的 `E8 87`→1 個）。替換發生在**消費端解碼**（`Utf8Iter`／`decode_utf8_char`／繪製／文字取出）；page buffer 維持原始位元組，使檔案位移＝buffer 位移（R11）。

### R14: CJK 適用範圍
WHEN 正文、heading、目錄、書名或既有 UI label 含繁中 THE SYSTEM SHALL 使用同一套可準備的 CJK fallback。

Host 驗收前提：模擬已完成正常 boot 的 card，包含 boot 建立的 `_PULP` 根目錄；未安裝 optional pack 指沒有 `FONTS` 或相應 pack，不指省略 EPUB cache 所需的 app 根目錄。缺 root 的 storage failure 與缺 optional pack 是不同情境。

### R15: 分頁失效
WHEN 字體識別、字級或排版版本改變 THE SYSTEM SHALL 重新建立不相容的分頁快取。

Policy [human]（2026-10-06 使用者選項 1）：本規則適用所有文字，包含純英文；即時字級變更與 resume 都須重建分頁，並選取包含變更前 raw byte anchor 的新頁。新頁碼可改變；舊英文 stale-offset 行為不保留。測試以 anchor containment、scalar 邊界、前後頁一致及全文內容守恆驗收，不以舊頁碼或舊 offsets 為 oracle。

### R16: 字庫驗收證據
WHEN 使用中文 fixtures 驗收 THE SYSTEM SHALL 提供 coverage／missing-glyph、snapshot、cache／SD-read-budget 結果。
