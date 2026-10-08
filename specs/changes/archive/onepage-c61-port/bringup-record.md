# OnePage C61 實機驗收記錄（模板）

**使用方式**：複製成 `bringup-record-<YYYY-MM-DD>-<板號>.md` 填寫，**本檔保持全「未驗」**。程序見 [`bringup.md`](bringup.md)。
狀態只能是：`未驗`（預設）／`已驗`（附證據）／`失敗`（附現象與處置）／`略過`（附原因）。沒有證據不得寫「已驗」。

## 環境

| 欄位 | 值 |
|---|---|
| 日期／測試者 | |
| 板子（版本／序號） | |
| commit（`git rev-parse HEAD`）與是否有未 commit 改動 | |
| rust nightly（`rust-toolchain.toml`） | nightly-2026-09-22 |
| espflash 版本（最低 4.4.0，建議 4.6.0） | |
| 映像 sha256（boot／full） | |
| SD 卡（廠牌／容量／格式） | |
| log channel／序列口 | |

## A. Boot 映像 `pulp-os-c61-boot`

| ID | 項目（需求） | 狀態 | 證據（log／量測／照片） | 備註／處置 |
|---|---|---|---|---|
| A1 | boot 與 log 輸出（R1） | 未驗 | | |
| A1b | GPIO10／GPIO11 與 log channel 無衝突（T9） | 未驗 | | |
| A2 | PSRAM 偵測大小／40 MHz／冒煙測試（R14） | 未驗 | | |
| A2b | PSRAM 連續開關機 ≥ 10 次穩定 | 未驗 | | |
| A2c | DMA／ISR 用的 internal buffer 正常（R15） | 未驗 | | |
| A3a | 冷開機 GPIO27 power-cycle 後 SD 掛載成功（R6） | 未驗 | | |
| A3b | SD 初始化後 GPIO27 保持供電（R7） | 未驗 | | |
| A3c | EPD／SD 共用 SPI2 無互相干擾（R8） | 未驗 | | |
| A3d | 無卡冷開機：可恢復錯誤、不 panic（R9） | 未驗 | | |
| A3e | 執行中拔卡／再插卡 | 未驗 | | |
| A3f | card detect 極性（GPIO28，BSP `:419`） | 未驗 | 實測電平： | |
| A4a | EPD init 與 full refresh 成功（R10） | 未驗 | | |
| A4b | 畫面方向／480×800 直向正確 | 未驗 | | |
| A4c | full refresh 耗時（ms） | 未驗 | 值： | |
| A4d | BUSY 異常時於上限內回 display failure（R11） | 未驗 | | |
| A4e | 殘影／對比 | 未驗 | | |
| A5a | ADC ladder 四鍵電壓落在窗口（BACK 2400–2800／LEFT 1780–2140／RIGHT 1140–1500／ENTER 0–250 mV）（R12） | 未驗 | 實測 mV： | |
| A5b | GPIO 三鍵（GPIO2／6／9）與優先序 WAKE>PREV>NEXT（R12） | 未驗 | | |
| A5c | 短按／長按／repeat 行為與手感、15 ms 去抖 | 未驗 | | |
| A5d | startup grace 2.5 s 內不輸出 front-ladder 事件；之後按住不產生假事件（R13） | 未驗 | | |
| A6a | 電池電壓讀值對照萬用表（R16） | 未驗 | log mV／量測 mV： | |
| A6b | 取樣期間 GPIO10 低 ≥ 30 ms 並恢復（R16） | 未驗 | | |
| A6c | USB 插拔事件與 polarity（R17） | 未驗 | 插入時 GPIO11 電平： | 是否需改 `USB_POLARITY` |
| A6d | keys（GPIO4）與 battery（GPIO5）共用 ADC1 無干擾 | 未驗 | | |
| A7a | 睡眠序列 log 順序完整（R19） | 未驗 | | |
| A7b | GPIO2 喚醒有效（arm 提前於 BSP 順序，T11） | 未驗 | | |
| A7c | 睡前保存、喚醒後恢復閱讀位置（R18／R20） | 未驗 | | |
| A7d | session 檔損壞（截斷／改位元組）時正常開機（R20） | 未驗 | | |
| A7e | 深睡電流 | 未驗 | 值（µA）： | 門檻待定 |
| A7f | 睡眠中 GPIO27／GPIO10 pad 狀態；EPD／SPI 腳無反灌 | 未驗 | | |
| A7g | 睡眠中 EPD 影像保留、無殘影 | 未驗 | | |
| A7h | 喚醒後 SD 重新初始化成功 | 未驗 | | |
| A7i | WAKE 鍵仍按著時進入睡眠的行為（`AlreadyAsserted`） | 未驗 | | |

## B. 完整韌體 `pulp-os-c61`

| ID | 項目（需求） | 狀態 | 證據 | 備註／處置 |
|---|---|---|---|---|
| B1 | 冷開機進 Home；有效 session 恢復位置（R20） | 未驗 | | |
| B2a | 英文 TXT：分頁／翻頁／跳 10 頁／跳頭尾（R21） | 未驗 | | |
| B2b | 英文 EPUB：分頁／跳章／TOC（R21） | 未驗 | | |
| B2c | 書籤新增、跨重開機保留（R21） | 未驗 | | |
| B2d | 設定（字型、主題、swap_buttons）生效（R21） | 未驗 | | |
| B2e | 換頁／開書／換章耗時（PSRAM 熱路徑） | 未驗 | 值： | 明顯變慢才考慮 `alloc_external_uninit` |
| B2f | full refresh 阻塞期間短按遺失的實際影響 | 未驗 | | |
| B2g | 按鍵標籤 bezel 位置對得上實體鍵 | 未驗 | | |
| B2h | Reader 長按 ENTER 開 quick menu（待確認決定 #2） | 未驗 | | |
| B3 | 無卡開機顯示儲存錯誤、不 panic；插卡後行為（R9） | 未驗 | | |
| B4a | idle timeout 後睡眠，GPIO2 喚醒回原位置（R18／R19） | 未驗 | | |
| B4b | 完整韌體睡眠電流 | 未驗 | 值（µA）： | |
| B5a | 電池週期更新；USB 插拔進 log | 未驗 | | |
| B5b | 連續閱讀 ≥ 30 分鐘穩定；stack／heap 高水位 | 未驗 | 值： | 靜態預算：statics 178,080 B、headroom 29,392 B |

## C. X4 實機回歸（R3 硬體側）

| ID | 項目 | 狀態 | 證據 | 備註／處置 |
|---|---|---|---|---|
| C1 | X4 開機與閱讀（HAL 1.2：SPI／DMA、esp-rtos 啟動） | 未驗 | | |
| C2 | X4 deep sleep／GPIO3 喚醒 | 未驗 | | |
| C3 | X4 Wi-Fi upload（`build-x4-wifi`） | 未驗 | | |

## 待使用者確認的決定（baseline §42）

| # | 決定 | 結論 |
|---|---|---|
| 1 | 睡眠只靠 idle timeout（無手動睡眠鍵） | 未確認 |
| 2 | Reader 長按 ENTER 開 quick menu；Files 刪除在 C61 不可用 | 未確認 |
| 3 | restore 成功不刪 session、失敗才刪 | 未確認 |
| 4 | 熱路徑配置不加 uninit 版本 | 未確認 |
| 5 | 無 boot console；C61 一律 full refresh | 未確認 |

## 結論

| 欄位 | 值 |
|---|---|
| A 階段整體 | 未驗 |
| B 階段整體 | 未驗 |
| 需回頭修改的 `board-logic` 常數與依據 | |
| 後續 change（partial refresh 波形、async BUSY、CJK、Wi-Fi 等） | |
