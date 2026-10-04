# onepage-cjk-iansui — Requirements

### R1: Host 字庫轉換
WHEN 提供 Iansui font-path 與字級設定 THE SYSTEM SHALL 產生可重現且附 provenance／OFL 的 SD bitmap 字庫。

### R2: Unicode／大 offset
WHEN 字元或 bitmap offset 超出16-bit範圍 THE SYSTEM SHALL 仍可正確索引該字形。

### R3: 字庫 failure
IF 字庫損壞、版本不符或索引越界 THEN THE SYSTEM SHALL 回報 font failure 而不越界存取。

### R4: 缺字 fallback
IF 已選字體不含所需字元 THEN THE SYSTEM SHALL 顯示明確的 missing-glyph fallback。

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

### R11: 文字位置一致
WHEN 回到前頁或恢復書籤 THE SYSTEM SHALL 定位至同一文字位置。

### R12: UTF-8 邊界
WHEN 文字跨 read-buffer、page 或 title 截斷邊界 THE SYSTEM SHALL 保留完整 Unicode scalar。

### R13: Malformed-input policy
IF UTF-8 或 numeric entity 含不合法 scalar／overlong 編碼 THEN THE SYSTEM SHALL 以 U+FFFD replacement policy 處理，而不輸出無效 UTF-8。

### R14: CJK 適用範圍
WHEN 正文、heading、目錄、書名或既有 UI label 含繁中 THE SYSTEM SHALL 使用同一套可準備的 CJK fallback。

### R15: 分頁失效
WHEN 字體識別、字級或排版版本改變 THE SYSTEM SHALL 重新建立不相容的分頁快取。

### R16: 字庫驗收證據
WHEN 使用中文 fixtures 驗收 THE SYSTEM SHALL 提供 coverage／missing-glyph、snapshot、cache／SD-read-budget 結果。
