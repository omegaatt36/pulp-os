# onepage-wifi-upload — Requirements

### R1: Enabled build
WHERE Wi-Fi enabled THE SYSTEM SHALL 使用有 C61 支援且相容的固定 radio／RTOS dependencies 連結 release ELF。

### R2: Radio optimization
WHERE Wi-Fi enabled THE SYSTEM SHALL 以 radio 要求的 optimization level2或3建置 radio package。

### R3: Disabled build
WHERE Wi-Fi disabled THE SYSTEM SHALL 保持不連入 radio driver 的離線版本。

### R4: Station／DHCP
WHEN 使用有效 station credentials 啟動 upload THE SYSTEM SHALL 在設定期限內進入可取得 IPv4 的連線狀態。

### R5: 連線 failure
IF credentials 缺失／不合法、association 失敗或 DHCP 逾時 THEN THE SYSTEM SHALL 顯示可退出的連線錯誤。

### R6: HTTP service
WHILE upload 已連線 THE SYSTEM SHALL 提供既有 HTTP upload／list／delete 行為。

### R7: 上傳內容
WHEN 上傳成功 THE SYSTEM SHALL 在 SD 保存與輸入位元組一致的檔案。

### R8: HTTP／SD failure
IF HTTP／SD 處理失敗 THEN THE SYSTEM SHALL 回報失敗而不顯示上傳成功。

### R9: mDNS
WHILE upload 已連線 THE SYSTEM SHALL 提供既有 `pulp.local` mDNS 回應。

### R10: 退出與釋放
WHEN 使用者退出或連線失敗 THE SYSTEM SHALL 釋放 network／radio 資源並返回可操作的閱讀介面。

### R11: 再次進入
WHEN 再次啟動 upload THE SYSTEM SHALL 不因殘留 controller／interface ownership 失敗。

### R12: 記憶體預算
WHILE Wi-Fi enabled THE SYSTEM SHALL 保持有證據的 internal heap／stack 與 PSRAM 預算。

### R13: 軟體驗收
WHEN 交付 Wi-Fi 實作 THE SYSTEM SHALL 提供 enabled／disabled link 與 host service-test 證據。

### R14: 實機驗收狀態
IF 尚未在 OnePage 測試 association／DHCP／HTTP／mDNS／重入／電流 THEN THE SYSTEM SHALL 將這些實機項目標為未驗。
