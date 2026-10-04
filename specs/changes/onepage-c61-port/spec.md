# onepage-c61-port — Requirements

### R1: 可重現建置
WHEN 選擇 OnePage C61 build THE SYSTEM SHALL 以固定且版本一致的工具鏈／依賴連結 `riscv32imac-unknown-none-elf` release ELF。

### R2: 無效 board 選擇
IF 未選 board 或同時選多個 board THEN THE SYSTEM SHALL 在建置時回報可理解的選擇錯誤。

### R3: X4 建置回歸
WHEN 選擇 X4 build THE SYSTEM SHALL 保持 X4 離線韌體可連結。

### R4: 離線 radio 邊界
WHERE Wi-Fi 未啟用 THE SYSTEM SHALL 產生不依賴 radio driver 的閱讀韌體。

### R5: 離線選單
WHERE Wi-Fi 未啟用 THE SYSTEM SHALL 不提供可進入 upload mode 的選單入口。

### R6: 啟動電源順序
WHEN OnePage 開始 board 初始化 THE SYSTEM SHALL 在 SD 初始化前完成 GPIO27 power-cycle。

### R7: SD 供電不變條件
WHILE SD 已初始化且系統尚未進入外設 shutdown THE SYSTEM SHALL 保持 GPIO27 的外設供電。

### R8: SPI wiring／互斥
WHEN 存取 EPD 或 SD THE SYSTEM SHALL 使用 BSP 指定的 SPI2 pins 與互斥交易。

### R9: SD failure
IF SD 缺卡或存取失敗 THEN THE SYSTEM SHALL 顯示可恢復的儲存錯誤而不 panic。

### R10: Portrait full refresh
WHEN 要求 full refresh THE SYSTEM SHALL 輸出對應 480×800 portrait 畫面的 SSD1677 strip 資料。

### R11: BUSY timeout
IF BUSY 超過設定上限 THEN THE SYSTEM SHALL 回報 display failure 而不無限等待。

### R12: 語意按鍵
WHEN OnePage 實體按鍵產生有效操作 THE SYSTEM SHALL 將一組 ADC ladder 與三個 GPIO keys 轉換為既有語意 actions。

### R13: ADC startup grace
WHILE ADC startup grace 2.5秒尚未結束 THE SYSTEM SHALL 不輸出 front-ladder 按鍵事件。

### R14: PSRAM 預算
WHEN 配置章節／圖片資料 THE SYSTEM SHALL 使用受預算限制的 PSRAM 配置。

### R15: Internal-memory 要求
WHILE 進行 DMA 或執行 runtime／ISR THE SYSTEM SHALL 保留所需的 internal-memory buffers 與資料。

### R16: 電池取樣
WHEN 量測電池 THE SYSTEM SHALL 依 BSP 的充電暫停／恢復契約取樣 GPIO5。

### R17: USB polarity
WHEN USB 偵測訊號改變 THE SYSTEM SHALL 依明確設定且有來源依據的 polarity 回報插入狀態。

### R18: 睡前保存位置
WHEN 進入 deep sleep THE SYSTEM SHALL 在外設斷電前保存可恢復的閱讀位置。

### R19: Shutdown／wake
WHEN 進入 deep sleep THE SYSTEM SHALL 依 BSP 順序關閉外設並配置 GPIO2 喚醒。

### R20: 恢復閱讀位置
WHEN 重新啟動且持久化狀態有效 THE SYSTEM SHALL 恢復閱讀位置。

### R21: 英文閱讀回歸
WHEN 在 OnePage 候選版開啟既有支援的英文 TXT／EPUB THE SYSTEM SHALL 保持分頁、導航、設定與書籤行為。

### R22: 軟體驗收證據
WHEN 軟體候選版交付 THE SYSTEM SHALL 提供實際 build／host-test 結果與 ELF memory 預算。

### R23: 硬體驗收狀態
IF 尚未在 OnePage 執行 THEN THE SYSTEM SHALL 將硬體驗收結果標為未驗。
