# onepage-c61-port

## Why

將目前只支援 Xteink X4 的 Pulp 移植至 OnePage C61，先交付可編譯、可供實機 bring-up 的離線閱讀版本。沒有實機期間，以編譯、操作紀錄與 host 邏輯測試驗收軟體契約。

## Scope

- In：可重現工具鏈、board selection、C61 HAL API、SPI／SD／EPD、按鍵、PSRAM、電池／USB、休眠與持久化、英文閱讀回歸。
- Out：CJK、Wi-Fi 上傳實作、BLE、音訊、OTA；partial refresh 波形調校留待實機後另定。
- 前置：讀 [handoff](../README.md) 與 [Wi-Fi check](../../references/onepage-wifi-support.md)，不使用其他分支。

## Impact

Cargo／toolchain／runner、`src/bin/main.rs`、`kernel/src/board`／`drivers`／scheduler／RTC session、upload 的 feature boundary、閱讀入口及必要測試 seam。BSP 是行為 reference，不作 IDF dependency。

硬體依據（repo 根目錄相對路徑）：`../bsp_onepage_c61/board_c61.c:37`、`:80`、`:156`、`:335`，`board_keys.c:19`／`:32`；`../crosspoint-onepage/boards/onepage-c61.json`。SPI22/23/24；EPD CS25/DC8/RST27/BUSY29；SD CS26/CD28；front ADC4；wake2/prev6/next9；battery ADC5/charge10/USB11。

GPIO27 兼 SD/MIC power，SD 初始化後的 display reset 只能用軟體 reset。C61 無現有 HAL `.rtc_fast.persistent` 配置；先以 SD 保存 session。USB11 polarity 及 flash clock 文件有矛盾：bring-up 用 flash40／PSRAM40，USB polarity 待 schematic／硬體確認。

完成定義是「軟體候選版準備好」，不是實機 port 宣告成功；T14 交付未驗項目的硬體驗收流程。
