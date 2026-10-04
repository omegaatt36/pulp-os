# onepage-host-validation — Requirements

### R1: Host build
WHEN 執行 host 驗收命令 THE SYSTEM SHALL 不載入 Espressif 硬體 runtime 或 embedded linker scripts。

### R2: 共用正式邏輯
WHEN host 分頁／rendering／EPUB fixture 執行 THE SYSTEM SHALL 使用 firmware 共用的正式演算法。

### R3: Firmware build 回歸
WHEN 建立 host boundary 後 THE SYSTEM SHALL 保持 OnePage 與 X4 離線 firmware 可編譯。

### R4: Virtual storage
WHEN host fixture 使用儲存 THE SYSTEM SHALL 提供可記錄讀取的 memory／host-file backend。

### R5: Storage error 注入
IF fixture 注入 short read 或 storage error THEN THE SYSTEM SHALL 回傳可驗收的失敗結果。

### R6: Portrait artifact
WHEN host preview 渲染一頁 THE SYSTEM SHALL 輸出固定 480×800 的 monochrome artifact。

### R7: Strip equality
WHEN 比較 full-frame 與 stitched-strip render THE SYSTEM SHALL 產生相同的像素結果。

### R8: 英文閱讀 fixtures
WHEN 使用生成的 TXT 與 EPUB2／3 fixtures THE SYSTEM SHALL 通過前後翻頁、TOC、圖片、設定與書籤回歸。

### R9: 可重現驗收
WHEN 新 agent 執行已記錄的命令 THE SYSTEM SHALL 產生確定性的測試結果與 preview artifacts。

### R10: 覆蓋缺口
IF fixture 功能或硬體行為尚未涵蓋 THEN THE SYSTEM SHALL 在驗收報告標記該缺口。
