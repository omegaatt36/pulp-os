# OnePage C61 實機驗收總表

**狀態：沒有任何一項在實機上執行過；第 5 節所有項目都是 `UNVERIFIED`。** 本文件把四個已封存 change（`onepage-c61-port`、`onepage-host-validation`、`onepage-cjk-iansui`、`onepage-wifi-upload`）所有「硬體未驗」的項目整合成單一入口，目的是拿到實機後，agent 可以照這份文件從刷機一路驗到 Wi-Fi，不必再回頭翻四個 archive。

- 逐步程序、log 字串行號、curl 腳本等細節仍在原 archive 文件（第 9 節列出對照），本文件只放：關卡順序、每項的指令／判定／失敗時改哪裡、需要人手的步驟、需要先加 instrumentation 的項目。
- 本文件撰寫時沒有重跑任何 build 或測試；引用的數字取自 archive 的 `baseline.md`／`progress.md`／`budget-report.md`，**不是**實測。

## 0. agent 執行規則

1. **誠實規則**：狀態只能是 `UNVERIFIED`（預設）、`PASS`（附 log／量測／照片）、`FAIL`（附現象與處置）、`SKIPPED`（附原因）。沒有證據不得寫 `PASS`。編譯、link、host 測試通過不算實機通過。
2. **記錄檔**：開始前複製第 5 節的表格到 `specs/hardware-records/<YYYY-MM-DD>-<板號>.md` 填寫；本檔保持全 `UNVERIFIED`。記錄檔頭部必填：`git rev-parse HEAD`、`git status --porcelain` 全文、各映像 `shasum -a 256`、`espflash --version`、SD 卡廠牌／容量／格式、log 通道（見 A1／A1b）、AP 型號。
3. **順序即依賴**：關卡 G0 → G1 → G2 → G3／G4。上一關有 `FAIL` 或未結論，下一關的失敗無法歸因，不得往下走（除非記錄寫明原因）。見第 3 節。
4. **哪些 agent 能自己做、哪些要人手**：agent 可執行建置、燒錄、監看序列輸出、`curl`、`shasum`、解析 log。**需要人手**的：進入下載模式（按鍵）、插拔 SD 卡、按實體鍵、肉眼判讀螢幕（方向、殘影、手感；請人拍照）、示波器／電流計／萬用表量測、改 AP／DHCP 環境。遇到時停下，明確列出要人做的動作與要回報的內容。
5. **失敗時改哪裡**：見第 7 節。改 `board-logic` 任何常數後，先 `cargo test-board-logic` 再 `task acceptance`，並把原因與來源寫回 `specs/changes/archive/onepage-c61-port/baseline.md`（不要只改常數）。
6. **不要擴大範圍**：async BUSY、OTA、BLE、音訊都不是本驗收的範圍（`onepage-c61-port/proposal.md` Out）。需要時另開 change。partial refresh 預設**關閉**（cargo feature `partial-refresh`），G0–G5 全程用預設映像驗收；它自成一關 G6（第 5 節），G2 通過後才做。

## 0.5 實際到貨配置與其影響

到貨：(1) **單主板 SKU**（L 形一體主板，Rev 0.2，不含螢幕、電池、外殼；開發用）；(2) **玻璃墨水屏面板（白色邊框）**（開發用，接在 SKU 的 FPC 座上）；(3) **成品機**一台（金屬背殼、橘色側鍵與前緣三鍵，實際使用）。**開發階段不接電池，只由 USB-C 供電。**

| 面向 | 影響 | 對應處置 |
|---|---|---|
| 面板型號 | BSP 與 EPD 驅動是為 **Osptek EPD0426A02（4.26"、800×480、SSD1677）** 寫的（`bsp_onepage_c61/README.md:22`），pulp-os 的 init 序列只對到 X4 而沒有逐命令比對 BSP。你買的裸面板是否就是這型，文件沒有證據 | 接上前**人手**看面板 FPC 上的型號絲印／向賣家確認；型號或解析度不同，A4 的失敗不能算軟體問題 |
| FPC 接線 | 裸面板靠 FPC 座連接，易損 | **斷電（拔 USB）後再插拔 FPC**；不要在通電時接面板 |
| 無電池 | 韌體對電量**只顯示、不做低電量關機**（`scheduler_c61.rs` 的 `poll_battery`；取樣失敗保留前值、不當 0 mV；`kernel` 內沒有低電量 cutoff），所以無電池不會觸發保護性關機。但 GPIO5 在無電池時讀到什麼取決於充電 IC／電源路徑（原理圖：LGS4056HDA、LM66200），文件沒有記載 | A6a／A6b／B5a 改為**只記錄**（見表內備註），不判 FAIL；電池讀值與電量曲線等拿到電池或成品機再驗 |
| USB 恆接 | `usb: GPIO11` 開機永遠讀到「插著」；A6c 的插拔事件要**人手拔插**才能看到兩個電平 | 這反而是定案 D6 極性的便宜機會：插著開機＝已知電平，拔掉 USB 後（需電池或電源）才有另一個電平 → 極性的「拔出」半邊要等有電池／成品機。**無電池時 USB 一拔機器就斷電**，不能在不重啟的情況下看到拔出事件 |
| 深睡與 USB | deep sleep 時 USB-Serial-JTAG 外設斷電，主機端的 `/dev/cu.usbmodem*` 會消失（即使 USB 仍供電）→ 睡眠後 log 中斷、重刷要走手動下載模式或先按 WAKE | 先刷 boot 映像（預設不睡眠）；A7 驗睡眠時預期 log 在睡眠序列最後一行後中斷，喚醒後埠重新出現；記錄這個現象不算失敗 |
| 睡眠電流 | USB 供電時量到的是整個板子加 USB 側 LDO／充電 IC 的靜態電流，**不是**深睡電流 | A7e／B4b／W6 的電流量測**不在 USB 供電下做**。路徑：SKU 板上有電池座（板上方白色 JST），可用**可調電源（約 3.7–4.2 V）＋串接電流計**自電池座供電、斷開 USB；**接之前必須用原理圖核對電池座極性**（接反會毀板），且斷開 USB 後就沒有序列 log，靠螢幕狀態標記。這項需要人手與儀器，可延到成品機＋電池 |
| 按鍵 | 單板 SKU 的 L 形長臂是側鍵板（照片絲印可見 `RESET`、`NEXT` 與上下箭頭）；底緣 K1–K4 看起來是 4 顆前鍵（對應 ADC ladder 四鍵）。成品機的橘色鍵是同一組鍵的外殼版本 | 實際鍵位與 GPIO 對應以實機＋原理圖為準；A5 測試時先逐鍵按、看 `key:` log 建立「實體鍵→Action」對照表並記錄 |
| 供電 | Mac USB 埠供電足夠 EPD 刷新；Wi-Fi 發射電流峰值是否造成 USB 掉電重啟未知 | W1–W5 若出現無預警重啟（開機行重現），先懷疑供電（換有源 USB hub／直接接 Mac 後方埠）再懷疑軟體 |
| 成品機 | 成品機用於日常使用，原廠 CrossPoint 韌體在上面 | **最後才刷**：在 SKU 上通過 G2（建議 G3、G4）、且**已在 SKU 上驗證過「備份 → 刷回原廠」的還原路徑**之後，才動成品機；成品機刷前同樣完整備份 16 MB 與 SD 卡。成品機的下載模式按鍵是否可從殼外按到、USB 口位置，到手後確認 |

建議順序：**SKU＋面板（USB 供電）走 G0 → G1（跳過需電池的項目）→ G2 → G3 → G4**（partial refresh 的 G6 在 G2 之後任何時候可做，不依賴 G3／G4）；A6／A7e／B4b／W6 與電池相關項目等取得電池或改用成品機再補；成品機最後。

## 1. 前置：軟體基準（在主機上，先於任何實機操作）

```sh
task acceptance        # 一鍵（Taskfile.yml）：五個映像（含 C61 partial refresh）的 build、host 測試、reader 回歸、ELF／依賴圖／記憶體預算檢查（harness/tests）、fmt
```

預期：所有 stage `ok`，結尾印出 UNVERIFIED 清單（對應本文件第 5 節）。不綠就先修，不要拿一個軟體基線已壞的映像去刷機。

工具鏈：`rust-toolchain.toml` 固定 `nightly-2026-09-22`（含 `rust-src`、`llvm-tools`，targets 含 `riscv32imac-unknown-none-elf`）。燒錄工具 `espflash` **≥ 4.4.0**（4.4.0 起支援 esp32c61，4.3.0 不認得；建議 4.6.0，專案以它驗證過 `save-image`；archive 與 `.cargo/config.toml` 寫的「≥ 4.6.0」已過時，見 2.2）。本機目前**沒有裝** `espflash`。

建置（兩個映像一次建出）：

```sh
cargo build-c61 --locked          # pulp-os-c61-boot（bring-up 映像）＋ pulp-os-c61（完整離線韌體）
cargo build-c61-wifi --locked     # 完整韌體 + Wi-Fi upload（G4 用）
cargo build-c61-partial --locked  # 完整韌體 + partial refresh（G6 用；feature `partial-refresh`，預設不開）
# 產物：target/riscv32imac-unknown-none-elf/release/{pulp-os-c61-boot,pulp-os-c61}
```

## 2. G0：刷機與第一次開機

本節整合 `../crosspoint-onepage`、`../onepage-reader`、`../bsp_onepage_c61` 與 pulp-os 本身的調查。標記：【文件】＝檔案明載；【推論】＝由證據推導，沒有文件直接寫；【待驗】＝只有實機能確認。

### 2.1 硬體事實

| 事實 | 標記 | 依據 |
|---|---|---|
| USB 是**原生 USB-Serial-JTAG**（D−=GPIO12、D+=GPIO13；macOS 上是 `/dev/cu.usbmodem*`，VID:PID 303a:1001），沒有 UART 橋接晶片 | 【文件】＋【推論】 | `crosspoint-onepage/platformio.ini:24-25`（`ARDUINO_USB_MODE=1`、`USB_CDC_ON_BOOT=1`）、`bsp_onepage_c61/README.md:159`；BOM 只有 USB1 Type-C、沒有 CH340／CP210x（`onepage-reader/electronics/c61`） |
| **沒有獨立 BOOT 鍵：BOOT ＝ 側鍵 NEXT（GPIO9，strapping pin）**；RESET 在 SKU 板側鍵長臂靠近 SD 座的一端，板上絲印 `RESET`，旁邊就是 `NEXT` 鍵 | 【文件】（GPIO9）＋【照片】（`RESET`／`NEXT` 絲印，低解析度，實機確認）。**更正**：先前由原理圖文字層推論「K4 是 RESET」，與照片不符（底緣 K1–K4 較像 4 顆前鍵），以照片為準，仍【待驗】 | `bsp_onepage_c61/README.md:55`；SKU 商品照 |
| 下載模式要求：GPIO9 低、**GPIO8 高**（EPD DC 就是 GPIO8，BSP 稱需外部上拉） | 【文件】 | esptool C61 boot-mode 文件、`bsp_onepage_c61/README.md:45` |
| SPI flash 是 `PY25Q128HA`（Puya，U5）；CrossPoint 文件載明 Puya 板 **stub 壓縮寫入會 crash，必須 `--no-stub`**（約 380 s，115200 baud） | 【文件】（stub 問題）＋【推論】（本板是 Puya） | `crosspoint-onepage/README.md:98-107`、BOM `BOM_Board1_PCB_OnePage_V1_2026-08-19.csv`；你手上的板是否同一批料【待驗】 |
| flash／PSRAM 頻率：CrossPoint 用 flash 80 MHz（不穩時退 40）；BSP 與 pulp-os 用 **40 MHz DIO／16 MB**（BSP：80 MHz 會 image-hash boot loop）；PSRAM 在 C61HR2 只在 ≤ 40 MHz 穩定 | 【文件】（互相矛盾，pulp-os 取保守） | `platformio.ini:47-48,138-142`、`bsp_onepage_c61/README.md:77,155`、`.cargo/config.toml:26-30`、`kernel/src/board_c61/memory.rs:129-133` |
| 原廠分割表（CrossPoint）：nvs 0x9000／otadata 0xe000／app0(ota_0) 0x10000 size 0x640000／app1 0x650000／spiffs 0xc90000／coredump 0xFF0000；bootloader 在 0x0、partition table 在 0x8000 | 【文件】 | `crosspoint-onepage/partitions.csv`、`sdkconfig.onepage:385,539` |
| pulp-os 沒有自己的 partition table 與 bootloader：2nd-stage bootloader 由 **espflash 內建**（ESP-IDF v6.1 建出），app 放 0x10000；每個 bin 都有 `esp_app_desc!()` 供 espflash 處理 | 【文件】 | espflash 4.6.0 `resources/bootloaders/esp32c61-bootloader.bin`、`src/bin/main_c61.rs:57`、`c61_boot.rs:82`、`build.rs:11` |
| 映像大小約 855 KB（`.rodata`＋`.text`＋`.data`＋`.rwtext` 加總；遠小於 app0 的 6.4 MB） | 【推論】 | 現有 ELF 的 section |
| secure boot、flash encryption 都沒啟用（只有 `SECURE_ROM_DL_MODE_ENABLED=y`，允許但未燒） | 【文件】 | `crosspoint-onepage/sdkconfig.onepage:439-444`。espflash 4.4.0 新增了 eFuse 寫入功能，**刷機流程不要呼叫它** |

### 2.2 espflash 版本：archive 的 ≥ 4.6.0 說法已過時

- 【文件】espflash CHANGELOG：**4.4.0（2026-04-16）起支援 ESP32-C61**（"Added ESP32-C61 chip support (#1009)"）；4.3.0 不認得（`baseline.md:171`）；crates.io 最新為 4.6.0（2026-09-10）。專案先前用 4.6.0 驗證過 `save-image`（`baseline.md` §9）。**最低 4.4.0，建議直接用 4.6.0。** `.cargo/config.toml:27-28` 與 `onepage-c61-port/bringup.md:12` 的「≥ 4.6.0」是過度保守的舊註解。
- 本機現況：**沒有** `espflash`、`esptool`、`cargo-espflash`（只有 `probe-rs`）。安裝由使用者／agent 在拿到板子時執行：`cargo install espflash --version 4.6.0 --locked`（MSRV 1.95）。
- espflash 預設使用 flasher stub；Puya 板有風險時要加 `--no-stub`，但 `cargo run-c61*` 的 runner 沒有這個旗標 → 改為手動呼叫 espflash（下面 2.5），只有確定需要才去改 runner。

### 2.3 進入下載模式

| 方式 | 步驟 | 標記 |
|---|---|---|
| 自動（優先） | 接 USB，直接執行 espflash；預設 `--before default-reset` 靠 USB-Serial-JTAG 的 DTR/RTS 邏輯重置進 ROM bootloader，不用按鍵 | 【文件】espflash `ResetBeforeOperation`；實機能否成功【待驗】 |
| 手動（自動失敗、或機器在 deep sleep、USB 埠消失時） | **人手**：接 USB → 按住 `NEXT`（GPIO9，側鍵長臂上）→ 按一下 `RESET` → 等 `/dev/cu.usbmodem*` 出現後放開 NEXT；此時 espflash 加 `--before no-reset` | 【推論】BSP README:55、SKU 照片絲印、esptool boot-mode 文件 |

**deep sleep 會讓 USB 埠從主機消失**（【推論】），所以：bring-up 期間先刷 boot 映像（預設不睡眠，`DEMO_IDLE_SLEEP_SECS = 0`，`c61_boot.rs:96`）；刷完整韌體後若進入 idle 睡眠，要先按 WAKE（GPIO2）喚醒，或走手動下載模式再刷。

### 2.4 備份原廠韌體（人手確認 USB 已連上；原廠韌體還在時才做）

espflash 會**改寫 0x0（bootloader）與 0x8000（partition table）**，CrossPoint 的 otadata／app1／spiffs 佈局會被丟掉，只能靠完整映像恢復。刷機前務必備份：

```sh
ls /dev/cu.usbmodem*                                                        # 【文件】crosspoint README:95
espflash board-info -c esp32c61 -p /dev/cu.usbmodemXXXX                     # 【待驗】確認晶片、flash 大小可連線
espflash read-flash 0x0 0x1000000 onepage-factory-16MB.bin -c esp32c61 -p /dev/cu.usbmodemXXXX   # 【待驗】完整 16 MB 備份；stub 讀取是否穩定不明，失敗時改 --no-stub 或只備份 0x0–0x650000
espflash read-flash 0x8000 0x1000 pt.bin -c esp32c61 -p /dev/cu.usbmodemXXXX && espflash partition-table --to-csv pt.bin   # 【待驗】驗證實際分割表
shasum -a 256 onepage-factory-16MB.bin
```

另請**人手複製整張 SD 卡內容**（CrossPoint 的設定與快取在 `.crosspoint/`，pulp-os 不會動它，但先備份）。備份檔放在 repo 之外（16 MB 的二進位檔不要進 git）。

### 2.5 刷 bring-up 映像（先 boot 映像，再完整韌體）

```sh
cargo build-c61 --locked        # 刷前重建：撰寫本文件時 target 內的 pulp-os-c61-boot 比 HEAD 舊（Oct 7 21:16），不可直接用
# 【文件】bringup.md:32；內部＝ espflash flash --monitor --chip esp32c61 --flash-mode dio --flash-freq 40mhz --flash-size 16mb
cargo run-c61-boot
# 若 stub 連線／寫入失敗（懷疑 Puya），手動呼叫並加 --no-stub（【待驗】；速度會慢很多）：
espflash flash --monitor --no-stub --chip esp32c61 --flash-mode dio --flash-freq 40mhz --flash-size 16mb \
  target/riscv32imac-unknown-none-elf/release/pulp-os-c61-boot
# 只想打包、不刷（【文件】baseline.md §9 以 4.6.0 驗證，未燒錄）：
espflash save-image --chip esp32c61 --flash-mode dio --flash-freq 40mhz --flash-size 16mb --merge \
  target/riscv32imac-unknown-none-elf/release/pulp-os-c61-boot /tmp/pulp-c61-boot-merged.bin
```

記錄：`espflash --version`、映像 `shasum -a 256`、espflash 輸出的 flash id／晶片資訊（確認 Winbond 或 Puya）。

完整韌體在 G1 全部有結論之後才刷：`cargo run-c61`（Wi-Fi 版 `cargo run-c61-wifi`）。

### 2.6 看序列 log

- **先接好 USB 再開機**：`esp-println` 的 `auto` 在 USB 已連過時走 USB-Serial-JTAG，否則走 UART0（【推論】`esp-println 0.18.0`）；沒接 USB 開機時 log 可能走 UART0 而看不到，且 UART0 預設腳就是 GPIO10／GPIO11（本板拿來當充電控制／USB detect）→ 這就是 A1b。
- `cargo run-c61-boot` 已含 `--monitor`；之後單獨監看：`espflash monitor -c esp32c61 -p /dev/cu.usbmodemXXXX`（【待驗】）。預期第一批 log：`boot: wake cause …`、`psram: … heap registered`、`pulp-os c61 boot: alive`。
- `ESP_LOG=info`（`.cargo/config.toml` `[env]`）；沒有時間戳、帶 ANSI 色碼。

### 2.7 回復原廠

```sh
# 【文件】crosspoint README:104-105、release.yml:62-66：官方 release 的 onepage-firmware-vX.bin 是 0x0 的合併映像
esptool --chip esp32c61 -p /dev/cu.usbmodemXXXX --no-stub -b 115200 write-flash \
  --flash-mode dio --flash-freq 80m --flash-size 16MB 0x0 onepage-firmware-vX.bin     # esptool 需自行安裝（pip install esptool）
# 【待驗】用自己的備份回復：
espflash write-bin 0x0 onepage-factory-16MB.bin -c esp32c61 -p /dev/cu.usbmodemXXXX
```

80 MHz 回復後若 boot loop，改 `--flash-freq 40m`（【推論】依 BSP:77,155）。成功判定以 esptool 的 "Hash of data verified" 為準，不看 exit code。

### 2.8 G0 通過條件與風險

- PASS：espflash 能連線並寫入 boot 映像；開機後序列口有 log；A1、A1b 有結論。
- 風險（皆【待驗】）：(1) USB-Serial-JTAG 自動 reset 能否成功；(2) Puya 板 stub 寫入 crash；(3) esp-hal `memory.x` 假設 iram_loader 起點為 `0x4083ea70`，espflash 內建的是 ESP-IDF 6.1 bootloader，版面若不同可能衝突（pulp-os 也依賴開機後回收 dram2 當 heap，`baseline.md` §30）；(4) GPIO27 在下載模式期間浮接，無已知危害，但畫面可能出現殘影；(5) USB polarity 與 GPIO27 的矛盾見 D6、A7f。

## 3. 關卡

| 關卡 | 內容 | 映像 | 進入條件 | 項目 |
|---|---|---|---|---|
| G0 | 刷機、能開機、有 log | boot | 第 1 節綠、硬體到手 | 第 2 節、A1、A1b |
| G1 | 硬體逐項（PSRAM、SD、EPD、按鍵、電池／USB、深睡） | `pulp-os-c61-boot` | G0 PASS | A2–A7 |
| G2 | 完整離線韌體（Home、TXT／EPUB、書籤、設定、睡眠、穩定度） | `pulp-os-c61` | **A1–A7 每項有結論**（PASS 或附原因的 FAIL／SKIPPED） | B1–B5 |
| G3 | 繁中 Iansui | `pulp-os-c61` + SD 字庫 | G2 的 B1、B2a、B2b、B3 PASS | K1–K9、H1 |
| G4 | Wi-Fi upload | `pulp-os-c61`（`build-c61-wifi`） | G2 的 B1、B3 PASS（boot／SD／EPD／BACK 與 ENTER 鍵無結論前，Wi-Fi 任何失敗都無法歸因） | W1–W8 |
| G5（選配） | X4 實機回歸 | X4 映像 | 手上有 X4 | X1–X3 |
| G6（選配） | C61 partial refresh（差異更新，`partial-refresh` feature） | `pulp-os-c61`（`build-c61-partial`） | G2 的 B1、B2a、B2b PASS（全刷路徑沒結論前，partial 的失敗無法歸因）；A4e 已拍下全刷的殘影／對比照片當對照 | P1–P10 |

## 4. 需要使用者決定的事

agent 不得自行定案；在記錄檔「結論」處留空或寫「待使用者」。

| # | 決定 | 預設（目前程式碼採用） | 何時需要 |
|---|---|---|---|
| D1 | 睡眠只靠 idle timeout（沒有手動睡眠鍵；Menu 動作無實體鍵） | 是（`sleep_timeout`，預設 10 分鐘） | B4 |
| D2 | Reader 內長按 ENTER 開 quick menu；Files 刪除在 C61 不可用 | 是 | B2h |
| D3 | restore 成功不刪 session、失敗才刪並正常開機 | 是 | B1、A7c |
| D4 | 熱路徑配置不加 uninit 版本（`alloc_external_uninit`） | 不加 | B2e：只有章節載入「明顯變慢」才重開 |
| D5 | 無 boot console；C61 預設一律 full refresh（`Partial` 升級為 full）；只有 `partial-refresh` 映像才做 partial（G6） | 是 | A4、B2f、G6 |
| D6 | **USB polarity**：BSP 實作與 BSP README 矛盾，原理圖推論 active-low，未實測 | `ActiveLow` | A6c：以實測電平定案 |
| D7 | 電流、BUSY 時間、手感沒有來源門檻 | 只記錄量測值，不判 pass／fail | A4c、A7e、B4b、W6 |
| D8 | 電流量測儀器、量程、量測點（W6：USB 供電同時給 log，量測會混入充電） | 未定 | A7e、W6 |
| D10 | 面板型號是否為 BSP 目標的 EPD0426A02（4.26"、800×480、SSD1677）；不是的話 A4 的結論不適用 | 未確認 | 接面板前 |
| D11 | 成品機何時刷：建議 SKU 通過 G2（最好 G3、G4）且「備份→刷回原廠」還原路徑已在 SKU 上驗過之後 | 最後 | G5 之後／日常使用前 |
| D12 | partial refresh 要不要成為預設（移除 feature、`Partial` 不再升級）：G6 全 PASS、殘影可接受、ghost_clear 預設值（10）合適之後才決定 | 否（feature 關） | G6 結束後 |
| D13 | partial 更新碼與溫度處理：`PARTIAL_UPDATE_SEQ = 0xDF`（廠商 moui 與 Arduino SDK 在這片面板上的值；不載入溫度，沿用上次全刷載入的） | 0xDF | P2、P3；候選見第 7 節 S9 |
| D9 | 實機驗收是否先 commit（Wi-Fi 的 T1–T6 曾全部未 commit；只記 HEAD 不足以重現） | 建議先 commit 再驗收 | 開始前 |

## 5. 驗收項目與追蹤表

狀態欄預設 `UNVERIFIED`。「失敗改哪裡」的代號見第 7 節。細節程序欄的路徑都在 `specs/changes/archive/` 下（簡寫，見第 9 節）。

### G1：Boot 映像（`cargo run-c61-boot`＝`espflash flash --monitor …`）

| ID | 項目（需求） | 做法 | 判定（來源） | 失敗改哪裡 | 狀態 |
|---|---|---|---|---|---|
| A1 | boot 與 log（R1） | 燒錄後看序列輸出 | 出現 `boot: wake cause …`（冷開機 cause 為空），之後週期性 `pulp-os c61 boot: alive`。記錄前 20 行 log | S1 | `UNVERIFIED` |
| A1b | GPIO10／GPIO11 與 log channel 無衝突（C61 UART0 預設 RX／TX 就是這兩腳，本板拿來當充電控制／USB detect） | 確認 `esp-println`（`auto`）實際走 USB-Serial-JTAG 還是 UART0 | log channel 不是 UART0，或 A6 運作時 log 不中斷、讀值有效。記錄通道 | S1 | `UNVERIFIED` |
| A2 | PSRAM 偵測／40 MHz／冒煙測試（R14） | 看開機 log | `psram: <N> B at 0x…, heap registered`，N 為 2 MiB 級（`PSRAM_HW_BYTES = 2097152`）。出現 `budget refused` 或 `running on internal memory` ＝降級，**不算 PASS** | S2 | `UNVERIFIED` |
| A2b | PSRAM 連續開關機穩定 | 冷開機 ≥ 10 次 | 10 次皆偵測成功、無 image-hash loop | S1、S2 | `UNVERIFIED` |
| A2c | DMA／ISR 用 internal buffer 正常（R15） | 結合 A3／A4 | SD、EPD 傳輸成功且無 `refused` log | S2 | `UNVERIFIED` |
| A3a | 冷開機 GPIO27 power-cycle 後 SD 掛載成功（R6） | 插卡冷開機 | `sd: card detect GPIO28 reads …`，無 `sd: init failed`／`volume mount failed` | S3 | `UNVERIFIED` |
| A3b | SD 初始化後 GPIO27 保持供電（R7） | 觀察 SD 讀寫期間（需示波器或讀寫不中斷） | 讀寫全程不掉電 | S3 | `UNVERIFIED` |
| A3c | EPD／SD 共用 SPI2 無互相干擾（R8） | 刷新畫面同時讀寫 SD | 兩者皆成功 | S3 | `UNVERIFIED` |
| A3d | 無卡冷開機：可恢復錯誤、不 panic（R9） | 不插卡開機 | 顯示可恢復的儲存錯誤，無 panic | — | `UNVERIFIED` |
| A3e | 執行中拔卡／再插卡 | 人手插拔 | `sd: card removed` → 再插卡後恢復 | S3 | `UNVERIFIED` |
| A3f | card detect 極性（GPIO28；BSP `board_c61.c:419` 自己也只是 "assume"） | 記錄插卡時讀到的電平 | 與預期一致；相反則改極性函式。**記錄實測電平** | S3 | `UNVERIFIED` |
| A4a | EPD init 與 full refresh 成功（R10） | 看 log 與畫面 | `epd: full refresh done (480x800 portrait test card)`。`epd: init failed` ＝ init 序列只對到 X4，要與 BSP 逐命令比對；`display reset refused` ＝ GPIO27 已為 SD 上電，只能用軟體 reset（設計如此） | S4 | `UNVERIFIED` |
| A4b | 畫面方向／480×800 直向正確 | 人手看（請拍照）。測試卡：左上 64×64 實心、右上與左下 24×24、右下無 | 四角圖樣位置正確；任一位置不對＝rotation／mirror 錯 | S4 | `UNVERIFIED` |
| A4c | full refresh 耗時（ms）（D7：只記錄） | log／碼表 | 記錄值（BUSY 上限 5000 ms，超過回 `BusyTimeout`） | S4 | `UNVERIFIED` |
| A4d | BUSY 異常時於上限內回 display failure（R11） | 拔掉／懸空 BUSY（GPIO29）腳（人手） | 上限內回錯、不卡死（log `epd: full refresh failed: …`） | S4 | `UNVERIFIED` |
| A4e | 殘影／對比（人手判讀） | 拍照 | 記錄 | — | `UNVERIFIED` |
| A5a | ADC ladder 四鍵電壓落在窗口（R12）：BACK 2400–2800、LEFT 1780–2140、RIGHT 1140–1500、ENTER 0–250 mV（BSP `board_keys.c`，是「那塊板」量測值） | 人手按鍵＋萬用表量 GPIO4 | 各鍵電壓落在窗口且互不重疊。**本專案沒有 mV 診斷輸出**，要靠萬用表或先補 instrumentation（第 6 節） | S5 | `UNVERIFIED` |
| A5b | GPIO 三鍵 WAKE(GPIO2)／PREV(GPIO6)／NEXT(GPIO9) 與優先序 WAKE>PREV>NEXT（R12） | 人手逐鍵、同時按 | 每次按鍵一行 `key: <event> -> <action>`；優先序符合。GPIO9 是 strapping pin：按住開機的影響也記錄 | S5 | `UNVERIFIED` |
| A5c | 短按／長按／repeat 手感、15 ms 去抖（D7） | 人手 | 記錄是否漏鍵／重複 | S5 | `UNVERIFIED` |
| A5d | startup grace 2.5 s（R13） | 上電後 2.5 s 內按 front ladder 鍵；再測 2.5 s 後仍按住的鍵 | 2.5 s 內無 key 事件；按住的鍵無假事件 | S5 | `UNVERIFIED` |
| A6a | 電池電壓讀值對照萬用表（R16）**〔無電池階段：只記錄 GPIO5 讀值與 log、確認不 panic 且不當 0 V，狀態維持 `UNVERIFIED`，電壓對照等電池／成品機〕** | 萬用表量電池端 | `battery: pin <mV> mV -> cell <mV> mV, <N>%`（開機一次，之後每 30 s；x2 分壓）與萬用表一致。取樣失敗為 `battery: sample failed …`，**絕不當 0 V** | S6 | `UNVERIFIED` |
| A6b | 取樣期間 GPIO10 低 ≥ 30 ms 並恢復（R16） | 示波器（人手） | 契約：GPIO10 拉低暫停充電 → 30 ms → 16 次 ADC → 還原 | S6 | `UNVERIFIED` |
| A6c | USB 插拔事件與 polarity（R17；D6）**〔無電池階段只能驗「插著」的電平；「拔出」電平與事件需電池或成品機，見 0.5〕** | 插著 USB 開機、不插開機各一次；執行中插拔 | 開機印 `usb: GPIO11 reads <level> -> <plugged?> (polarity …, UNVERIFIED on hardware)`。插入顯示為拔出＝相反，**只改** `USB_POLARITY`。記錄實測電平與來源 | S6 | `UNVERIFIED` |
| A6d | keys（GPIO4）與 battery（GPIO5）共用 ADC1 無干擾 | 按鍵同時等電池取樣 | 兩者讀值皆正常 | S5、S6 | `UNVERIFIED` |
| A7a | 睡眠序列 log 順序完整（R19） | 把 `src/bin/c61_boot.rs:95` 的 `DEMO_IDLE_SLEEP_SECS` 改為 N 秒，重建重燒 | 依序：`session: saved to slot …` → `sleep: GPIO2 wake armed …` → `sleep: epd parked …` → `sleep: sd flushed and closed` → `sleep: shared lines driven low`，然後斷電睡眠。`sleep: aborted, staying awake: …` ＝ wake 未能 arm／WAKE 鍵仍按著／rail 狀態不符 | S7 | `UNVERIFIED` |
| A7b | GPIO2 喚醒有效（arm 提前於 BSP 順序，**T11 必查**） | 人手按 GPIO2 | 能喚醒；不能時先查 arm 時序與 LP pad hold | S7 | `UNVERIFIED` |
| A7c | 睡前保存、喚醒後恢復位置（R18／R20；D3） | 喚醒後看 `boot: wake cause …`、`boot plan …`／`session[…]` | 恢復上次位置 | S7 | `UNVERIFIED` |
| A7d | session 檔損壞時正常開機（R20） | 在讀卡機上截斷或改一個位元組 `_PULP/SESSA.BIN`／`SESSB.BIN`（雙槽、CRC、序號），重開機 | log `session[…]: normal boot (…)`，無 panic | S7 | `UNVERIFIED` |
| A7e | 深睡電流（D7、D8）**〔不可在 USB 供電下量；見 0.5：電池座＋可調電源，或成品機＋電池〕** | 電流計（µA 級） | 記錄值（門檻待定） | S7 | `UNVERIFIED` |
| A7f | 睡眠中 GPIO27／GPIO10 pad 狀態；EPD／SPI 腳無反灌（**T11 必查**：BSP 與本移植都沒有 hold／isolate，GPIO27 若浮接，SD／MIC／EPD 可能被重新供電而吃電） | 示波器／萬用表 | 記錄 GPIO27 睡眠中電位 | S7 | `UNVERIFIED` |
| A7g | 睡眠中 EPD 影像保留、無殘影 | 人手看 | 影像保留 | S7 | `UNVERIFIED` |
| A7h | 喚醒後 SD 重新初始化成功 | log | 同 A3a 路徑成功 | S7 | `UNVERIFIED` |
| A7i | WAKE 鍵仍按著時進入睡眠（`AlreadyAsserted`） | 人手按住 GPIO2 再觸發睡眠 | 預期 `sleep: aborted, staying awake`；記錄實際行為 | S7 | `UNVERIFIED` |

**G1 通過條件**：A1–A7 每項在記錄中有「PASS（附證據）」或「FAIL（附現象與修正）」。不得留 `UNVERIFIED` 就進 G2。

### G2：完整離線韌體（`cargo run-c61`；準備：FAT 卡內含至少一本英文 `.txt` 與一本 `.epub`）

| ID | 項目（需求） | 做法 | 判定 | 狀態 |
|---|---|---|---|---|
| B1 | 冷開機進 Home；有效 session 恢復位置（R20；D3） | 讀書到某頁後睡眠再喚醒／重開 | Home 正常；restore 成功**不刪** session，失敗才刪並正常開機 | `UNVERIFIED` |
| B2a | 英文 TXT：分頁／翻頁／跳 10 頁／跳頭尾（R21） | 人手 | 與移植前 X4 行為一致（host 已證明等價，實機補 host 測不到的） | `UNVERIFIED` |
| B2b | 英文 EPUB：分頁／跳章／TOC（R21） | 人手 | 同上 | `UNVERIFIED` |
| B2c | 書籤新增、跨重開機保留（R21） | 人手 | 保留 | `UNVERIFIED` |
| B2d | 設定（字型、主題、`swap_buttons`）生效（R21） | 人手 | 生效（`swap_buttons` host 未涵蓋） | `UNVERIFIED` |
| B2e | 換頁／開書／換章耗時（PSRAM 熱路徑；D4） | 碼表／log | 只記錄；明顯變慢才考慮新增 `alloc_external_uninit` | `UNVERIFIED` |
| B2f | full refresh 阻塞期間（最壞約 5 s）短按遺失的實際影響（已知限制；D5） | 人手 | 記錄 | `UNVERIFIED` |
| B2g | 按鍵標籤（button feedback）bezel 位置對得上實體鍵（沿用 X4，**未驗證**） | 人手＋拍照 | 位置對得上 | `UNVERIFIED` |
| B2h | Reader 長按 ENTER 開 quick menu（D2） | 人手 | 開啟；記錄是否合用 | `UNVERIFIED` |
| B3 | 無卡開機顯示儲存錯誤、不 panic；插卡後行為（R9） | 不插卡開機再插卡 | 顯示錯誤、不 panic。已知限制：插卡後 Files 的 error 欄位**不會自動清除**，記錄操作上的影響 | `UNVERIFIED` |
| B4a | idle timeout 後睡眠，GPIO2 喚醒回原位置（R18／R19；D1） | 等 `sleep_timeout` | 先畫 `(sleep)` 畫面 → 儲存位置 → 睡眠；喚醒回原位置。序列中止時要多一次 full refresh | `UNVERIFIED` |
| B4b | 完整韌體睡眠電流（D7、D8）〔同 A7e，不可在 USB 供電下量〕 | 電流計 | 記錄值，與 A7e 對照 | `UNVERIFIED` |
| B5a | 電池週期更新；USB 插拔進 log〔無電池階段：只確認 30 s 取樣週期照跑、不 panic〕 | log | 30 s 週期更新；`USB_PLUGGED` 目前**無 UI**（只進 log） | `UNVERIFIED` |
| B5b | 連續閱讀／翻頁 ≥ 30 分鐘穩定；heap／stack 高水位 | 看 `stats:` 行 | 無 panic／重啟。靜態預算：statics 178,080 B、stack headroom 29,392 B（**後續 CJK／Wi-Fi 改動後數字已漂移，以當時 `task size`／`cargo test-harness` 輸出為準**）；高水位只能由 `stats:` 的 `hwm` 取得 | `UNVERIFIED` |

### G3：繁體中文 Iansui

`onepage-cjk-iansui` 沒有交付實機程序，只交付 SD 安裝指引（`install.md`）與「host 成功不證明 live heap 足夠、SD timing、display、scheduler」的限制。以下項目由該限制與規格推導，是本文件新增的實機項目。

準備：

```sh
cargo run -p pulp-fontconv --release -- bundle      # 產生 target/cjk-sd/_PULP/FONTS/（需要 repo 根目錄的 Iansui-Regular.ttf；它是未追蹤輸入，不隨 clone 出現）
# 把 target/cjk-sd/_PULP/FONTS/ 整個複製到 SD 卡根目錄的 _PULP/FONTS/（先換掉舊目錄；保留 PROV.TXT、OFL.TXT、COVERAGE.TXT）
```

9 個字級 pack：`F00016/19/23/27/28/32/35/38/46.PFN`，合計 14,792,301 B，每個 pack 含 12,665 個 Unicode scalar。測試書請含繁中標題、正文、書名與 TOC；`host-preview`／`iansui-acceptance` 的樣本是 `臺灣「繁體中文」，閱讀測試。山水之間，日月星辰。𪚥`。

| ID | 項目（需求） | 做法 | 判定 | 狀態 |
|---|---|---|---|---|
| K1 | 字庫安裝完整、位元組一致 | 卡上 `_PULP/FONTS/` 與 `target/cjk-sd` 比對 | 9 個 pack＋`PROV.TXT`／`OFL.TXT`／`COVERAGE.TXT` 齊全，`shasum` 一致 | `UNVERIFIED` |
| K2 | 5 個字級設定下，繁中標題／正文／書名／TOC 顯示（R14） | 設定切換 5 個 size，開含繁中的書 | 字形正確、無空心方框（U+2A6A5 以外） | `UNVERIFIED` |
| K3 | 缺字：U+2A6A5 `𪚥` 不在字庫內 | 開含該字的頁 | 顯示空心方框（R4）；其餘 25 個樣本字元有字形 | `UNVERIFIED` |
| K4 | 未安裝目前字級 pack：可開書，顯示該字級的空心方框 | 刪掉某字級 pack | 可開書、方框尺寸正確（R4 policy）；裝回後重開恢復 | `UNVERIFIED` |
| K5 | pack 損壞／版本不符：可恢復的 font preparation failure（R3／R9） | 在讀卡機上截斷或改 pack | 顯示可恢復錯誤、不 panic、不越界；換回 pack 重開後恢復 | `UNVERIFIED` |
| K6 | 真實 SD 讀取 I/O 錯誤：可恢復錯誤（R9） | 閱讀中拔卡（人手） | 顯示可恢復錯誤；插回後重開恢復 | `UNVERIFIED` |
| K7 | 字形 cache／heap 實際夠用（R8；host 只驗算術） | 看 `stats:`／`memory:`／`bigbuf:` log | C61 FontGlyphs 預算 224 KiB（PSRAM 類別 256 KiB）；internal-heap metadata 約 80 KiB 常駐／96 KiB 暫態（**預算算術，未量測**）。無 `bigbuf: … refused`、無 panic；降級 16 KiB 類別時，密集 CJK 頁回報可恢復的 OutOfMemory 屬預期。X4 BigBuf 無類別預算 | `UNVERIFIED` |
| K8 | 翻頁／準備頁面的實際時間（SD timing；R6／R7：繪製期間零 SD 讀取） | 碼表／log | 只記錄（D7）；host 的讀取計數是邏輯 VirtualStorage 呼叫，**不是**實體 SD 交易或延遲 | `UNVERIFIED` |
| K9 | 變更字級後分頁失效重建、書籤／位置一致（R11／R15） | 閱讀中切換字級；加書籤後重開機 | 重建分頁並定位到包含原 raw byte anchor 的新頁；書籤回到同一文字位置 | `UNVERIFIED` |
| K10 | 中文禁則（R10）實際版面 | 人手看窄行寬的書（`臺灣「繁體中文」，𠮷。`） | 行首不出現 `，。、？！）」』】`，行尾不出現 `（「『【` | `UNVERIFIED` |

### host 預覽 vs 實機對照

| ID | 項目 | 做法 | 判定 | 狀態 |
|---|---|---|---|---|
| H1 | 實機畫面與 host snapshot 視覺一致 | `target/accept-iansui/snapshots/`（需先跑 `iansui-acceptance`，見 `onepage-cjk-iansui/install.md`）與 `cargo host-preview` 輸出，對照實機拍照 | 排版（斷行、頁界）一致；差異記錄原因（殘影、字形二值化門檻、host 與 firmware 的 Rig 差異）。host 已知缺口：不驗 C61 PSRAM 預算／實際配置、RTC session restoration、swap_buttons、實體按鍵、SD timing、refresh／殘影、電源與喚醒（`onepage-host-validation/coverage.md`） | `UNVERIFIED` |

### G4：Wi-Fi upload

前置：`cargo run-c61-wifi`；Home 選單最後一項才有 `Upload`（離線建置沒有）。憑證寫在 SD 的 `_PULP/SETTINGS.TXT`（Settings 畫面沒有編輯 UI，只能在 PC 編輯）：

```
wifi_ssid=<SSID>
wifi_pass=<密碼 8–63 字元>
```

只讀檔案前 512 bytes；開機與插卡時載入（改檔後要重開機或重新插卡）；認證固定 WPA2-Personal（**開放網路／空密碼也被拒**）；SSID 上限 32 B、密碼 63 B（超長會被靜默截斷）。AP 需求：2.4 GHz、WPA2-PSK、DHCP 啟用、client isolation 關閉、PC 與裝置同一 L2 子網路；建議頻道 1–11（esp-radio 預設 country 為 `CN`）；W2／W5 需要能「關 AP」與「連得上 AP 但沒有 DHCP」的網路（repo 不提供，需自備）。**所有 `curl` 必須依序執行，不得並行**（HTTP 伺服器是單 socket）。

| ID | 項目（需求） | 做法（細節：`onepage-wifi-upload/hardware-acceptance.md` 同名章節） | 判定 | 狀態 |
|---|---|---|---|---|
| W1 | association（R4、R5、R14） | 有效憑證 Home → Upload，3 次；負向：(a) 無 `wifi_ssid` (b) 密碼 7 字元 (c) 密碼錯 (d) AP 關閉；每項後 BACK | 正向 20 s 內 `upload: connected to '<ssid>', waiting for DHCP`；(a)–(d) 各出對應錯誤畫面（`No WiFi credentials!`／`WiFi config error!`／`Connection failed!` 或 `Connection timed out!`），BACK 後回 Home 可操作；記錄 (c)(d) 落在 Failed 還是 Timeout（程式碼無法決定） | `UNVERIFIED` |
| W2 | DHCP 取得 IPv4（R4、R5、R14） | 接續 W1；負向：連得上但無 DHCP；DHCP 等待中 BACK | 15 s 內顯示 IPv4，與路由器 lease 一致，非 `0.0.0.0`／`169.254.x.x`，`ping` 通；無 DHCP 時 ≥ 15 s 顯示 `No IP address!`；DHCP 期間 BACK 可退出 | `UNVERIFIED` |
| W3 | HTTP 內容（R6、R7、R8、R14） | 頁面、`GET /files`、上傳 0 B／1 B／2047／2048／2049 B／100 KiB／1 MiB（`ACC1M.EPUB`→存成 `ACC1M.EPU`）／長檔名（→`ACCEPTAN.TXT`）、delete、404／431／500、中斷上傳、瀏覽器；上傳後退出、取卡、在 PC 以 `shasum -c` 驗證 | 全部 sha256 一致；應失敗的請求不得回 200；`../X` 類名稱不得刪任何檔；錯誤後伺服器仍可用；中斷的上傳**不得**出現 `upload: file saved as`；`/files` 只列根目錄 TXT／EPUB／EPU／MD、最多 64 筆。記錄 `NOSUCH.TXT` 刪除在 FAT 實機的結果（host 只驗過虛擬儲存體） | `UNVERIFIED` |
| W4 | mDNS `pulp.local`（R9、R14） | macOS：`dns-sd -G v4 pulp.local`；Linux：`avahi-resolve -4 -n pulp.local`；每次先清快取，10 次；負向 `other.local`、AAAA；選做 `tcpdump 'igmp or udp port 5353'`。**不要用 `dig`**（回應固定以 ID 0 送 224.0.0.251:5353） | 解析到與螢幕一致的 IP，10 次中 ≥ 9 次在 5 s 內成功（自訂門檻）；負向無答案；每次成功對應一行 `upload: mDNS answered pulp.local`。裝置端沒有「收到 query」log，分辨「query 沒到」與「回應送不出」要靠 PC 端封包 | `UNVERIFIED` |
| W5 | 重入與資源釋放（R10、R11、R14） | A：連續成功 N=10 次（自訂）；B：Associating／DHCP／serving／傳輸中各階段 BACK（各 3 次，之後立刻再進）；C：失敗後再進入（`AssociationFailed`／`DhcpTimeout`／`MissingCredentials`） | 每次可進 serving 與退出；無 panic／重啟（不得再出現開機行 `pulp-os c61: esp32c61 rv32imac, rtos + embassy up`）；無 `RadioUnavailable`／`upload: station interface already taken`；`stats:` 的 `used`（以第 1 次退出後為基準）第 2–10 次沒有連續 5 次以上嚴格遞增；`hwm` < 50K（≥ 48K 標警示）。第 1 次相對第 0 次的增量記為一次性 init 配置，不判 fail 但必須記錄 | `UNVERIFIED` |
| W6 | 功耗（R14；D7、D8）〔需自電池座／電池供電量測，USB 供電下不可做；可延到成品機〕 | S0 Home 基準；S1 associated 閒置；S2 serving 有負載；S3 退出後 Home；各 ≥ 60 s | 只記錄，不訂門檻。S3 明顯高於 S0 ＝ radio 未完全關閉的跡象，另開 issue。量測時改電池供電並斷開 USB（USB 供電會混入充電） | `UNVERIFIED` |
| W7 | HTTP 靜默對端逾時（R6／R10 邊界；G9：使用者決定不改程式） | serving 中 `nc -v $IP 80` 不送資料，另一終端每 ~2 s `curl -m 3 …/files` 至成功或 120 s；變體：送不完整 header 後靜默 | PASS＝已取得實測資料，且靜默連線存在時 BACK 仍能退出並釋放、之後可再進入。記錄第二個請求失敗型態與 `code=200` 首次出現時間（或「未回收」）。程式沒呼叫 `set_keep_alive`，依 smoltcp 文件靜默對端可能不會被 30 s 回收 | `UNVERIFIED` |
| W8 | radio 執行期 internal heap、stack 高水位、PSRAM 分工（R12 執行期部分；`[assumed]` 52 KiB） | 開機 `stats:` 的 `<total>`；W5 的 `peak`／`hwm`；開機 `log_report`；在 wifi 建置上開與離線相同的 EPUB／含圖書 | wifi 建置 `<total>` 約 114K 級（離線約 158K 級；若 wifi 顯示 158K ＝燒到離線映像）；W1 通過且 W5 無 OOM／panic；閱讀路徑無 `bigbuf: … refused`／panic，或有差異時記錄供使用者決定是否調整 52 KiB | `UNVERIFIED` |

### G5（選配）：X4 實機回歸

X4 為遷移 HAL 1.2 改了 SPI（`SpiDmaBus`→`SpiDma`）、deep sleep（`LowPower::sleep_deep` + GPIO3 wake）、esp-rtos 啟動與 Wi-Fi upload（esp-radio beta）。只證明過可連結且 wire trace／input／battery 與移植前等價。

| ID | 項目 | 指令 | 狀態 |
|---|---|---|---|
| X1 | X4 開機與閱讀（HAL 1.2：SPI／DMA、esp-rtos 啟動） | `cargo run-x4` | `UNVERIFIED` |
| X2 | X4 deep sleep／GPIO3 喚醒（兩個已知差異：pull 不再明確寫入、LP pad 不 isolate） | `cargo run-x4` | `UNVERIFIED` |
| X3 | X4 Wi-Fi upload，含 `full_refresh_async` 被 BACK 取消後的 panel 狀態 | `cargo run-x4-wifi` | `UNVERIFIED` |

### G6（選配）：C61 partial refresh（`cargo run-c61-partial`）

只驗 `partial-refresh` 映像。程式與 host 證據：`board-logic/src/ssd1677.rs` 的 `Epd::partial_refresh`（命令序列由 `partial_refresh_command_trace` 等 9 個 host 測試鎖定）、`kernel/src/kernel/scheduler_c61.rs` 的 `render_partial`。**這些只證明命令順序、區域對齊與錯誤路徑；波形、對比、殘影、耗時一項都沒有實機證據。**

行為摘要（判讀 log 用）：

- `Redraw::Partial(region)` 且 partial 次數 < Settings 的 `ghost_clear`（預設 10，範圍 5–100）→ 區域差異更新；否則 `display: promoted partial to full (ghosting clear)` 後全刷，次數歸零。`Redraw::Full` 一律全刷。
- 差異更新三步：新內容寫 BW RAM（RED 仍是畫面上的舊內容）→ `0x21 [00 00]`、`0x22 [DF]`、`0x20`、等 BUSY → 同一內容寫 RED＋BW（讓 RED 成為畫面上的內容）。開機第一幀與任何 `init`（含失敗後 reinit）之後的第一幀一定是全刷。
- 每次刷新一行：`display: <partial|full> refresh <w>x<h> at (<x>, <y>) in <ms> ms (partial count <n>)`；失敗：`display: partial refresh failed: <原因>`，放棄：`display: giving up on this frame, redraw on next input`。
- 與 X4 的差異（有意）：刷新期間阻塞（不收輸入、不跑背景工作）；沒有 rapid-navigation 的 `red_stale`／`inv_red` 捷徑（每次都做第 3 步）；區域邊緣位元組內「區域外」的像素照 app 畫的內容送出，不遮成白色（X4 遮白；參考實作 moui 與 Arduino SDK 都不遮）。

| ID | 項目 | 做法 | 判定 | 失敗改哪裡 | 狀態 |
|---|---|---|---|---|---|
| P0 | 預設映像不受影響；partial 映像確實不同 | 先燒 `pulp-os-c61` 翻頁 5 次；再用 `cargo run-c61-partial` 燒 partial 映像（同樣叫 `pulp-os-c61`，只差 feature），同操作。記錄兩個映像的 `shasum -a 256` | 預設映像 log 完全沒有 `display: partial`／`promoted partial`；每次翻頁都是全刷。partial 映像翻頁出現 `display: partial refresh …` | S9 | `UNVERIFIED` |
| P1 | 開機首幀全刷、之後第一次翻頁就是 partial | 冷開機進 Reader，翻 1 頁 | 開機畫面為全刷（閃）；翻頁 log 為 `partial refresh … (partial count 1)`，不是 `full` | S9 | `UNVERIFIED` |
| P2 | partial 耗時（D7：只記錄） | Reader 連續翻頁 20 次，抄 log 的 ms（含 phase 1＋BUSY＋phase 3＋app 繪製）；有示波器則量 GPIO29（BUSY）高電位寬度＝波形時間 | 記錄中位數與最大值。參考（不是門檻）：X4 README 寫 ~400 ms，Arduino SDK guide 寫 ~600 ms；全刷 ~1600 ms（A4c） | S9 | `UNVERIFIED` |
| P3 | 殘影／對比（人手判讀） | 連續 partial 翻頁，在第 1、5、10 次後各拍一張同倍率的照片；與 A4e 全刷照片對照 | 記錄：字緣是否變淡、前一頁字影是否可見、白底是否發灰。**沒有量化門檻**；明顯不可接受時先把 Settings 的 `ghost_clear` 降到 5 再判，仍不行才走 S9 | S9 | `UNVERIFIED` |
| P4 | ghost-clear 週期與設定一致 | `ghost_clear` 設 5、預設 10 各驗一次：連續 Reader 翻頁數 | 連續 N 次 partial 後，下一次出現 `promoted partial to full (ghosting clear)` 與 `full refresh`，`partial count` 回 0 | S9 | `UNVERIFIED` |
| P5 | 區域邊緣（不是 8 的倍數）不破壞鄰近像素 | 開關 quick menu（Reader 長按 ENTER）、按鍵 feedback（B2g）、位置 overlay、載入指示；每次拍照 | 區域外相鄰像素無缺損、無白邊／黑邊；關掉 overlay 後下方頁面完整還原（不殘留 overlay 邊框）。邊緣有白色缺口＝改回 X4 的遮罩做法（S9） | S9 | `UNVERIFIED` |
| P6 | 連續使用穩定 | Reader 前後翻頁共 ≥ 100 次、含跨章、含 Home↔Reader 切換 | 內容每頁正確（對照全刷映像同頁）、無累積殘影、無 panic／重啟；`stats:` 的 stack／heap 高水位與 B5b 同級 | S9 | `UNVERIFIED` |
| P7 | 刷新期間短按遺失（阻塞版；對照 B2f） | partial 映像連按 NEXT 5 次（約 200 ms 間隔），數實際翻幾頁；再用預設映像做同樣操作 | 只記錄兩者差異。已知限制：partial 仍阻塞，只是視窗比全刷短 | — | `UNVERIFIED` |
| P8 | 整頁切換仍為全刷並歸零計數 | Home→Reader、開書、換章（`Redraw::Full` 的路徑） | log 為 `full refresh`、`partial count 0` | S9 | `UNVERIFIED` |
| P9 | 睡眠喚醒後首幀全刷，之後 partial 正常 | idle 睡眠後喚醒（restore 成功），再翻頁 | 喚醒首幀為全刷；翻頁恢復 `partial refresh`；螢幕無殘留睡眠畫面痕跡 | S7、S9 | `UNVERIFIED` |
| P10 | BUSY 異常的恢復（延伸 A4d） | partial 映像下，在翻頁瞬間讓 GPIO29 懸空／拔掉（人手），再恢復 | 上限內（BUSY 5 s×最多 2 次）回錯、不卡死：`display: partial refresh failed: display busy timeout` → reinit → 重試為 `full refresh`；若仍失敗 `giving up on this frame`，下一次按鍵整頁重畫。無 panic | S9 | `UNVERIFIED` |

**G6 通過條件**：P0–P10 每項有「PASS（附證據）」或「FAIL（附現象與修正）」；D12 由使用者決定是否讓 partial 成為預設。

## 6. 需要先補 instrumentation 才能驗收的項目

目前韌體**沒有**下列輸出；在實機上直接驗會變成只能憑間接證據。先決定要不要加（另開小 change，不要夾在驗收中途改程式）：

| 缺口 | 影響項目 | 目前的替代方法 |
|---|---|---|
| ladder 各鍵的 mV 診斷輸出 | A5a | 萬用表量 GPIO4（BSP 的 `board_front_key_mv()` 沒有等價物） |
| mDNS「收到 query」log | W4 | PC 端 `tcpdump` |
| upload 期間的 heap／stack 時間曲線（upload 期間主迴圈暫停，沒有 `stats:`） | W5、W8 | 退出後那一行 `stats:` 的 `peak`／`hwm`（只含自開機起的高水位） |
| radio／esp-rtos 其他 task 的 stack 高水位、殘留 task 清單 | W5、W8 | 無 |
| Wi-Fi 期間 PSRAM／internal 分項（`log_report` 只在開機呼叫） | W8 | 無 |
| 連線 RSSI | W6、W1 | 無 |
| partial 刷新的分項時間（phase 1／BUSY／phase 3）與 SSD1677 實際使用的溫度 | P2、P3 | 只有每次刷新的總 ms；BUSY 寬度要用示波器量 GPIO29 |

## 7. 失敗時改哪裡

| 代號 | 症狀 | 檔案／常數 |
|---|---|---|
| S1 | 無法開機／image-hash loop／無 log | `.cargo/config.toml` runner 的 flash 參數（必須 `--flash-mode dio --flash-freq 40mhz --flash-size 16mb`；80 MHz 在此板 image-hash boot loop）；espflash 版本；序列口／log channel |
| S2 | PSRAM 不穩／降級 | `kernel/src/board_c61/memory.rs`（PSRAM 40 MHz 設定）；`board-logic/src/memory.rs` 預算常數；esp-hal 預設 `flash_tuning`／`ram_tuning`（`din_mode 3, din_num 1, extra_dummy 2`）在 40 MHz 下是否合適未驗 |
| S3 | card detect 相反／SD 不穩 | `board-logic/src/sd.rs`（GPIO28 電平→有卡的對應函式約 line 40、去抖；測試 `r9_cd_polarity_is_bsp_low_means_inserted` 要跟著改）。未驗風險：MISO pull-up 未套用、SPI 400 kHz→10 MHz 切換、GPIO27 power-cycle 後 20 ms 內能否 probe |
| S4 | 畫面方向錯／BUSY 逾時／全刷對比或殘影差 | `board-logic/src/ssd1677.rs`（`Rotation`；BSP `board_c61.c:50,211,216,222`；BUSY 上限 5000 ms 可配置）。init 序列最初只對到 X4；moui 驅動（`MoveCall/moui` 的 `src/drivers/moui_drv_ssd1677.c`，BSP 實際用的，用 `gh api repos/MoveCall/moui/contents/src/drivers/moui_drv_ssd1677.c` 讀）對 EPD0426A02（OTP 波形）的 init 與本專案 `configure()` 有兩處差異，**尚未驗證哪個對**：(1) `0x0C` booster soft-start：moui 送 `[AE C7 C3 80 C0]`，本專案送 `[AE C7 C3 C0 80]`（第 4、5 個位元組互換）；(2) moui 在 init 多送 `0x1A [5A]`（溫度暫存器，其 update 序列也不載入溫度），本專案只送 `0x18 [80]` 並在全刷用 `0xF7` 載入溫度。全刷對比／殘影不佳時先對照這兩點。GPIO8（DC）是 strapping pin、需外部上拉 |
| S5 | 按鍵窗口不符／手感 | `board-logic/src/keys.rs`（ladder mV 窗口、優先序、grace）。與 BSP 的有意差異：long-press 1000 ms（BSP 800）、無 120 ms 同鍵鎖 |
| S6 | 電池讀值偏／USB 插拔相反 | `board-logic/src/usb.rs:59` `USB_POLARITY`（目前 `ActiveLow`）；`board-logic/src/battery.rs`（分壓、取樣契約；充電暫停沉澱 BSP 30 ms、crosspoint 5 ms，哪個足夠未驗） |
| S7 | 睡眠無法喚醒／吃電 | `board-logic/src/sleep.rs`（序列順序）、`kernel/src/board_c61/sleep.rs`（wake 配置；LP pad hold；GPIO27 睡眠中 pad 狀態） |
| S9 | partial refresh（G6）：太淡／殘影重／太慢／區域錯位／邊緣破損 | `board-logic/src/ssd1677.rs`：`PARTIAL_UPDATE_SEQ`（目前 `0xDF`＝時脈＋類比開、載 LUT、mode 2、顯示、類比＋時脈關，不載入溫度）。**候選（皆未試）**：(a) `0xFF`＝加 LOAD_TEMP（用內部感測器；X4 的 `0xFC` 也載入溫度）；(b) 在 `start_partial_update` 的觸發前加 `0x1A [5A]`（moui 的 EPD0426A02 預設，Arduino SDK 的 HALF 也這樣做；偽稱高溫以換較短波形，代價是對比）。區域錯位看 `align_partial_region`／`transform_region`（共用 S4 的 `Rotation`）；邊緣破損看 `write_region`（有意不遮罩，改回 X4 做法＝套 `RenderState.left_mask`／`right_mask`）；週期不對看 `kernel/src/kernel/scheduler_c61.rs` 的 `render`（`partial_refreshes < ghost_clear_every`）。每次只改一個變數，重燒後重做 P2、P3 |
| S8 | Wi-Fi：radio 起不來／heap 不足 | `board-logic/src/memory.rs` 的 internal heap 切割（52 KiB main ＋ 64,000 B reclaimed，`[assumed]`）；`harness/tests/c61_memory_budget.rs` 的預算 |

## 8. 已知限制（不是失敗）

- full refresh 阻塞期間（最壞約 5 s）的短按會遺失。partial refresh（`partial-refresh` 映像）同樣阻塞（耗時未量，P2），只是視窗較短；X4 的非阻塞 BUSY 等待（`busy_wait_with_background`）與 rapid-navigation 的 `red_stale`／`inv_red` 捷徑沒有移植，列為 G6 之後的下一層。
- 插卡後 Files 的 `error` 欄位不會自動清除；`StorageStatus` 文字只進 log；`USB_PLUGGED` 無 UI。
- 實機上無法以正常流程同時具備 Wi-Fi 憑證與「SD 未掛載」狀態（拔卡會重載設定並清空憑證），所以 `GET /files` 在 SD 未掛載時回 500 的路徑只有 host 證據。
- 副檔名：`.epub` 上傳後存成 `.EPU`（8.3 清理）；非 TXT／EPUB／EPU／MD 的上傳會寫入但不在 `/files` 列表。
- 繼承而來、明確延後的 G5：真實 SD close error 被吞掉；host VirtualStorage 不驗 FAT metadata flush 失敗。
- `smol-epub` 內部暫存未遷移到 PSRAM（需改外部 crate）；`Cargo.lock` 不固定 smol-epub 的 Git revision。
- 序列 log 沒有時間戳、帶 ANSI 色碼（grep 用 `grep -a`）。
- Wi-Fi 掉線／DHCP lease 變動後 serving 階段不會偵測（螢幕與 mDNS 沿用 serving 開始時的 IP），spec 沒有要求；只記錄不判定。

## 9. 來源對照（細節仍在這些文件）

| 主題 | 文件 |
|---|---|
| C61 bring-up 程序、log 字串、記錄模板（A／B／C） | `specs/changes/archive/onepage-c61-port/bringup.md`、`bringup-record.md` |
| 五項待確認決定、各 task 的「尚未驗證項目」 | `onepage-c61-port/baseline.md` §6、§9、§12、§15、§18、§21、§24、§30、§33、§36、§39、§42、§44 |
| CJK SD 安裝、驗收邊界 | `specs/changes/archive/onepage-cjk-iansui/install.md`、`evidence.md` |
| host 驗證缺口 | `specs/changes/archive/onepage-host-validation/coverage.md` |
| Wi-Fi W1–W8 完整程序、log／畫面字串行號、curl 腳本、已知風險 | `specs/changes/archive/onepage-wifi-upload/hardware-acceptance.md`、`budget-report.md`（R12 UNVERIFIED）、`baseline.md`（UNVERIFIED） |
| Wi-Fi 版本集與支援證據 | `specs/references/onepage-wifi-support.md` |
| Partial refresh 的外部參考（本機沒有 checkout，用 `gh api` 讀） | `MoveCall/moui` 的 `src/drivers/moui_drv_ssd1677.c`（`update_partial`、`ssd1677_hw_flush`、`ssd1677_init_display`；`bsp_onepage_c61` 依賴它）；`MoveCall/onepage-reader-sdk-arduino` 的 `libs/display/EInkDisplay/src/EInkDisplay.cpp`（`refreshDisplay` 的 FAST 路徑、單緩衝模式）與 `doc/SSD1677_GUIDE.md`（Partial Refresh、Partial Update 兩節） |
| 軟體驗收一鍵入口 | `Taskfile.yml`（`task acceptance`）；本文件取代原腳本結尾印出的 UNVERIFIED 清單 |
