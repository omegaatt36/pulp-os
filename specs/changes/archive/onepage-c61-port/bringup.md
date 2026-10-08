# OnePage C61 實機 bring-up 程序（T14；R22／R23）

**狀態：沒有任何一項在實機上執行過。** 本文件只交付程序；結果記在 [`bringup-record.md`](bringup-record.md)（複製一份填寫，原檔保持全「未驗」）。軟體候選版的證據在 [`baseline.md`](baseline.md) §43（`scripts/run-software-acceptance.sh`）。

每一步都寫了：怎麼做、預期看到什麼（log 字串取自原始碼）、失敗時改哪裡。**沒有來源依據的數值門檻（睡眠電流、BUSY 時間、手感）不在此訂門檻，只記錄量測值**；是否合格由使用者／原理圖決定。

## 0. 前置

| 項目 | 要求 | 來源 |
|---|---|---|
| 工具鏈 | `rust-toolchain.toml` 的 pinned nightly（含 `rust-src`、`llvm-tools`），targets 含 `riscv32imac-unknown-none-elf` | baseline §1–6 |
| 燒錄 | `espflash` 最低 **4.4.0**（4.3.0 不認得 esp32c61），建議 4.6.0；本機目前沒有 `espflash` | baseline §9 |
| 旗標 | flash **40 MHz DIO／16 MB**（80 MHz 在此板會 image-hash boot loop）；PSRAM 40 MHz；已寫在 `.cargo/config.toml` 的 runner | BSP README、baseline §9／§25 |
| 序列 log | `esp-println`（`auto`）；`ESP_LOG=info`（`.cargo/config.toml`） | Cargo.toml、.cargo/config.toml |
| 卡 | FAT 格式 SD，內含至少一本英文 `.txt` 與一本 `.epub`；另備一張**空白／未插卡**情境 | R9、R21 |
| 量測 | 電流計（deep sleep µA 等級）、可調／可量電壓的電源或電池、萬用表（量 GPIO4／GPIO5 分壓點） | — |

建置（兩個映像一次建出）：

```
cargo build-c61 --locked
# target/riscv32imac-unknown-none-elf/release/pulp-os-c61-boot   bring-up 映像（逐項 log、無 UI 流程）
# target/riscv32imac-unknown-none-elf/release/pulp-os-c61        完整離線韌體
```

流程：**先 A（boot 映像）逐項驗硬體，A 全部有結論後再 B（完整韌體）**。A 失敗就不要往 B 走：B 疊在 A 驗過的硬體上。

---

## A. Boot 映像 `pulp-os-c61-boot`

燒錄並開監看：`cargo run-c61-boot`（= `espflash flash --monitor --chip esp32c61 --flash-mode dio --flash-freq 40mhz --flash-size 16mb`）。

### A1. Boot 與 log（R1）
- 預期：開機後有 log；出現 `boot: wake cause …`（冷開機為空 cause）、後面出現週期性 `pulp-os c61 boot: alive`。
- 失敗：無輸出 → 先查 flash 頻率（必須 40 MHz）、espflash 版本、USB 序列口。**GPIO10／GPIO11 是 C61 UART0 預設 RX／TX，而本板拿它們當 charge 控制／USB detect**：若 log channel 走 UART0 會與 A6 衝突，要確認 `esp-println`（`auto`）實際用的 channel（T9 必查）。
- 紀錄：log 前 20 行、`espflash` 版本、使用的序列口。

### A2. PSRAM（R14／R15）
- 預期：`psram: <N> B at 0x…, heap registered`。N 應對應 2 MiB 級 PSRAM（板規 2 MiB，`PSRAM_HW_BYTES = 2097152`）。
- 失敗樣態：`psram: budget refused status …` 或 `… running on internal memory`＝降級路徑（不 panic）。降級時完整韌體以 X4 規模運作，**不算 PSRAM 驗收通過**。
- 另記：偵測到的大小、冒煙測試是否有錯誤 log、memory_demo 的配置 log、40 MHz 是否穩定（連續開關機 ≥ 10 次）。
- 未驗風險：cache／DMA 一致性、dram2 可用性（baseline §30）。

### A3. 電源順序與 SD（R6／R7／R8／R9）
- 預期：開機流程在 SD 初始化前完成 GPIO27 power-cycle；`sd: card detect GPIO28 reads …`；有卡時 SD 掛載成功，無 `sd: init failed`／`volume mount failed`。
- 驗證項：(1) 插卡冷開機 (2) 無卡冷開機：應顯示可恢復的儲存錯誤、**不 panic**（R9）(3) 執行中拔卡：`sd: card removed` (4) 再插卡：恢復。
- **CD 極性**（BSP `board_c61.c:419`）：若插卡時讀到的狀態與預期相反，改 `board-logic/src/sd.rs` 中把 GPIO28 電平對應成「有卡」的函式（約 line 40；測試 `r9_cd_polarity_is_bsp_low_means_inserted` 會跟著要改）並重跑 `scripts/test-board-logic.sh`。
- 未驗風險：MISO pull-up 差異、SPI／DMA 在 400 kHz→10 MHz 切換、FAT 一致性（baseline §18）。

### A4. EPD（R10／R11）
- 預期：`epd: full refresh done (480x800 portrait test card)`，畫面為 480×800 直向測試圖，方向正確。
- 記錄：實際方向（是否需調 `Rotation`）、full refresh 耗時（BUSY 上限 5000 ms，超過會回 `DisplayError::BusyTimeout`，log 為 `epd: full refresh failed: …`）、殘影／對比。
- 失敗樣態：`epd: init failed` → init 序列只對到 X4（BSP 的 moui 驅動原始碼本機沒有），要與 BSP 逐命令比對；`epd: display reset refused` → GPIO27 已為 SD 上電，display reset 只能用軟體 reset（R6 設計）。
- R11 逼近測試：拔掉／懸空 BUSY（GPIO29）腳或用示波器確認 BUSY 長時間為高時，應在上限內回錯、不卡死。

### A5. 按鍵（R12／R13）
- 預期：每次按鍵一行 `key: <event> -> <action>`。
- 逐鍵：ADC ladder 四鍵（GPIO4）BACK／LEFT／RIGHT／ENTER，GPIO 三鍵 WAKE(GPIO2)／PREV(GPIO6)／NEXT(GPIO9)；各測短按、長按、長按 repeat；同時按下的優先序（WAKE > PREV > NEXT）。
- **量電壓**：各鍵按下時 GPIO4 的實際電壓對照窗口 BACK 2400–2800 mV、LEFT 1780–2140、RIGHT 1140–1500、ENTER 0–250（BSP `board_keys.c`）。窗口外或重疊＝改 `board-logic/src/keys.rs` 的窗口並重跑測試。
- **Startup grace（R13）**：上電後 2.5 s 內按 front ladder 鍵**不得**出現 key 事件；2.5 s 後按住的鍵不得產生假事件。
- 記錄：手感（是否漏鍵／重複）、15 ms 去抖是否夠。

### A6. 電池與 USB（R16／R17）
- 預期：開機一次、之後每 30 s 一行 `battery: pin <mV> mV -> cell <mV> mV, <N>%`；失敗為 `battery: sample failed …`（**絕不當 0 V**）。取樣契約：GPIO10 拉低暫停充電 → 30 ms → 16 次 ADC → 還原。
- 驗證：用萬用表量電池端電壓，對照 `cell mV`（x2 分壓）；取樣期間用示波器看 GPIO10 確實低 ≥ 30 ms 並恢復。
- **USB polarity（R17）**：開機會印一行 `usb: GPIO11 reads <level> -> <plugged?> (polarity …, UNVERIFIED on hardware)`，用它對照「插著 USB 開機」與「不插開機」兩種實際電平；執行中插／拔則出 `usb: …` 事件。若插入顯示為拔出（相反），**只改** `board-logic/src/usb.rs:59` 的 `USB_POLARITY`（目前 `ActiveLow`；BSP 實作 `board_c61.c:306` 與 BSP README `:52` 互相矛盾，原理圖推論為 active-low，未經實測）。把實測電平與來源寫進 `bringup-record.md`。
- 另記：GPIO4／GPIO5 共用 ADC1，keys 與 battery 同時運作是否互相干擾（T9 必查）。

### A7. Deep sleep 與 wake（R18／R19／R20）
預設不睡眠。要演練：把 `src/bin/c61_boot.rs:95` 的 `DEMO_IDLE_SLEEP_SECS` 改為 N（秒）重建重燒，閒置 N 秒後序列只跑一次。
- 預期 log 順序：`session: saved to slot …` → `sleep: GPIO2 wake armed …` → `sleep: epd parked …` → `sleep: sd flushed and closed` → `sleep: shared lines driven low` → 斷電睡眠。
- 失敗樣態：`sleep: aborted, staying awake: …`（wake 未能 arm／WAKE 鍵仍按著＝`AlreadyAsserted`／rail 狀態不符）。
- 驗證：(1) 按 GPIO2 能否喚醒；喚醒後 log `boot: wake cause …` 與 `boot plan …`／`session[…]` 是否恢復上次位置（R20）(2) 睡眠中量電流（記錄值）(3) 睡眠中 EPD 影像是否保留、有無殘影 (4) 喚醒後 SD 是否重新初始化成功 (5) 手指仍按著 WAKE 時進入睡眠的行為。
- **T11 必查**：本實作把 arm wake 提前到所有不可逆步驟之前，**與 BSP 順序不同**（BSP 在斷電後才 arm，`board_c61.c:348-352`）；GPIO27 在 deep sleep 中的 pad 狀態（若浮接，SD／MIC／EPD 可能被重新供電而吃電）、GPIO10 睡眠中電位、EPD DC／CS／SPI 斷電後是否反灌電。若 GPIO2 不能喚醒，優先檢查 arm 時序與 LP pad hold。
- Session 檔：`_PULP/SESSA.BIN`／`SESSB.BIN`（雙槽、CRC、序號）。可在讀卡機上確認寫入；**損壞測試**：截斷或改一個位元組後重開機，應正常開機（`session[…]: normal boot (…)`）而非 panic（R20 的另一半）。

### A 階段通過條件
A1–A7 每項在 `bringup-record.md` 都有「已驗（附 log／量測）」或「失敗（附現象與修正）」；**不得留未驗就進 B**，除非記錄中寫明原因。

---

## B. 完整離線韌體 `pulp-os-c61`

燒錄：`cargo run-c61`。

### B1. 啟動與恢復（R20）
- 冷開機進 Home；有有效 session 時恢復到上次閱讀位置（restore 成功**不刪** session，失敗才刪並正常開機）。
- 預設決定仍待你確認（baseline §42）：睡眠只靠 idle timeout（系統設定 `sleep_timeout`，分鐘）、Reader 內**長按 ENTER 開 quick menu**（Files 刪除在 C61 不可用）、一律 full refresh、無 boot console。

### B2. 英文 TXT／EPUB 閱讀（R21）
軟體側已證明與移植前 X4 的分頁／導航／設定／書籤行為一致（baseline §43，host）。實機要補的是 host 測不到的：
- 開啟 TXT、EPUB：分頁正確、翻頁／跳 10 頁／跳章、TOC 可用、書籤新增與跨重開機保留、設定（字型、主題、`swap_buttons`）生效。
- **頁面載入速度**（PSRAM 熱路徑：章節快取／prefetch）：記錄換頁、開書、章節切換的實測時間。章節載入明顯變慢時，才考慮新增 `alloc_external_uninit`（baseline §41／40f）。
- full refresh 阻塞期間（最壞約 5 s）的短按會遺失（已知限制）；記錄實際 refresh 耗時與是否影響操作。
- 按鍵標籤（button feedback）的 bezel 位置沿用 X4，**未驗證**：確認位置是否對得上實體鍵。

### B3. 儲存錯誤與恢復（R9）
- 開機無卡 → 顯示儲存錯誤、不 panic；插卡後 Files 的 error 欄位**不會自動清除**（已知限制），確認操作上的影響。

### B4. 睡眠（R18／R19）
- 閒置超過 `sleep_timeout` 後：先畫 `(sleep)` 畫面 → 儲存位置 → 睡眠；按 GPIO2 喚醒並回到原位置。
- 序列中止時要多一次 full refresh（baseline §40c）。記錄睡眠電流（與 A7 對照）。

### B5. 電池／USB 顯示與長時間穩定
- 電池 30 s 週期更新；`USB_PLUGGED` 目前**無 UI**（只進 log）。
- 連續閱讀／翻頁 ≥ 30 分鐘：heap／stack 是否穩定（目前**只有靜態預算**：statics 178,080 B、stack headroom 29,392 B；實際高水位未量，可用 stack painting 或 log 記錄）。

---

## C. X4 實機回歸（R3 的硬體側）

X4 為遷移 HAL 1.2 改了 SPI（`SpiDmaBus`→`SpiDma`）、deep sleep（`LowPower::sleep_deep` + GPIO3 wake）、esp-rtos 啟動與 Wi-Fi upload（esp-radio beta、WPA2-Personal 固定）。目前只證明可連結且 wire trace／input／battery 與移植前等價。有 X4 時：`cargo run-x4` 驗證開機、閱讀、睡眠喚醒；`cargo run-x4-wifi` 驗證 upload。

---

## D. 失敗時改哪裡（速查）

| 症狀 | 檔案／常數 |
|---|---|
| 無法開機／image-hash loop | `.cargo/config.toml` runner 的 flash 參數；espflash 版本 |
| PSRAM 不穩 | `kernel/src/board_c61/memory.rs`（PSRAM 40 MHz 設定）；`board-logic/src/memory.rs` 預算常數 |
| card detect 相反 | `board-logic/src/sd.rs`（GPIO28 電平→有卡的對應函式、去抖） |
| 按鍵窗口不符 | `board-logic/src/keys.rs`（ladder mV 窗口、優先序） |
| USB 插拔相反 | `board-logic/src/usb.rs:59` `USB_POLARITY` |
| 畫面方向錯 | `board-logic/src/ssd1677.rs`（`Rotation`；BSP `board_c61.c:50,211,216,222`） |
| BUSY 逾時／卡住 | `board-logic/src/ssd1677.rs`（上限 5000 ms，可配置） |
| 睡眠無法喚醒／吃電 | `board-logic/src/sleep.rs`（序列順序）、`kernel/src/board_c61/sleep.rs`（wake 配置） |

改完任何 `board-logic` 常數：`scripts/test-board-logic.sh`，再 `scripts/run-software-acceptance.sh`，確認軟體契約沒壞，**並把原因與來源寫回 `baseline.md`**（不要只改常數）。
