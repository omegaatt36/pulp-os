# onepage-wifi-upload

## Why

在離線 C61 移植後，獨立恢復 Pulp 的 station Wi-Fi 上傳，避免舊 radio API 阻塞閱讀／繁中。C61 有可編譯、可連結的 radio 路徑，但沒有實機 runtime 證據。

## Scope

- In：optional Wi-Fi build、radio API 遷移、STA association／DHCP／TCP／UDP、既有 HTTP upload／list／delete／mDNS、錯誤退出與再次進入、memory／hardware acceptance。
- Out：AP mode、新網路產品功能、OTA、BLE、無實機的穩定性／功耗保證。
- 前置：C61 port 軟體驗收完成；預設排在 CJK 後。讀 [版本與實際連結證據](../../../references/onepage-wifi-support.md)。

## Impact

Cargo feature／radio package optimization、`src/apps/upload/`、manager／menu、network runner 與資源生命週期、測試及實機流程。

已知：原 radio0.17／RTOS0.2 無 C61；radio0.18／beta.0 對應 HAL1.1；本次 HAL1.2 + radio1.0.0-beta.1 + RTOS0.4 + alloc0.11 + embassy-net0.8 的 station／DHCP／TCP／UDP 最小 ELF link 通過。舊 upload config／init／start API 不可直接沿用。這是待實作 change，不要求第一階段連上網。

## Risk

未評估（原 proposal 缺此欄）；`/spec-apply` 依規則視為 **red**。

## Assumptions

- [human, 2026-10-07] **R12 預算以 build-time 變體處理**：wifi 建置（`--features wifi`）永久縮小 `INTERNAL_HEAP_MAIN_BYTES`（96 KiB → 52 KiB，不論使用者是否進入 Upload）；離線建置維持 96 KiB，預算與 ELF 不變。`STACK_MIN_BYTES`／`STATIC_RAM_MAX_BYTES` 不放寬。
- 已解決的 spec 落差（resolved miss）：R1（C61+wifi 要能連結）與 R12（預算不變）在原預算下互斥——T1 實測 C61+wifi statics 249,280 B > 預算 207,472 B（stack 剩 7,344 B，離線版 75,692 B）；radio 增加 .rwtext.wifi 30,704／.bss 28,500／.data 6,436 B。原 spec 沒有預見 radio 的靜態 RAM 成本。
- [assumed] 52 KiB 是由「statics 需再省 ≥ 41.8 KB」推得的最小切割加餘裕，**不是**由 radio 實際 heap 需求量測得出；radio 執行期 internal heap、PSRAM 分工、stack 高水位均 UNVERIFIED，待實機。

- [human, 2026-10-07] **resolved miss：delete 相容性**：使用者要求修復 final review 的 MUST／RECALL。R6 的既有 delete 相容性優先；T6 原派工「delete 與 upload 共用同一個名稱驗證函式」未預見合法 FAT 名的字元集合比 upload sanitizer 寬。保留 upload sanitize，delete 驗證無路徑的合法 FAT 8.3 名，不藉 sanitize 改名。另補 Wi-Fi aggregate reservation 邊界測試，預算不變。
