# onepage-wifi-upload

## Why

在離線 C61 移植後，獨立恢復 Pulp 的 station Wi-Fi 上傳，避免舊 radio API 阻塞閱讀／繁中。C61 有可編譯、可連結的 radio 路徑，但沒有實機 runtime 證據。

## Scope

- In：optional Wi-Fi build、radio API 遷移、STA association／DHCP／TCP／UDP、既有 HTTP upload／list／delete／mDNS、錯誤退出與再次進入、memory／hardware acceptance。
- Out：AP mode、新網路產品功能、OTA、BLE、無實機的穩定性／功耗保證。
- 前置：C61 port 軟體驗收完成；預設排在 CJK 後。讀 [版本與實際連結證據](../../references/onepage-wifi-support.md)。

## Impact

Cargo feature／radio package optimization、`src/apps/upload.rs`、manager／menu、network runner 與資源生命週期、測試及實機流程。

已知：原 radio0.17／RTOS0.2 無 C61；radio0.18／beta.0 對應 HAL1.1；本次 HAL1.2 + radio1.0.0-beta.1 + RTOS0.4 + alloc0.11 + embassy-net0.8 的 station／DHCP／TCP／UDP 最小 ELF link 通過。舊 upload config／init／start API 不可直接沿用。這是待實作 change，不要求第一階段連上網。
