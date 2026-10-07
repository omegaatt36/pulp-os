# C61 Wi-Fi support check

2026-10-04，針對目前 Pulp 與 crates.io cache 原始碼；沒有裝置，未掃描／連線任何 AP。本文件保存研究結論，後續實作不得依賴暫存 probe。

2026-10-08 更新：Wi-Fi 上傳已實作並歸檔（`specs/changes/archive/onepage-wifi-upload/`），下列「版本證據」「實際連結證據」仍是該實作的版本依據；「原 upload 需要遷移的部分」與「決策」已改寫為現況。仍然沒有任何實機或執行期證據。

## 版本證據

| Crate | C61 feature | HAL 相容版本 | 判斷 |
|---|---|---|---|
| esp-radio0.17.0（原 Pulp 使用，已淘汰） | 無 | 1.0 | 不能直接改 chip feature |
| esp-rtos0.2.0（原 Pulp 使用，已淘汰） | 無 | 1.0 | 需要升級 |
| esp-radio0.18.0 | 有 | ~1.1.0-rc.0 | 有 C61 路徑，但不能直接搭 HAL1.2 |
| esp-radio1.0.0-beta.0 | 有 | ~1.1.0 | 仍非 HAL1.2 配套 |
| esp-radio1.0.0-beta.1 | 有 | ~1.2.0 | 目前採用 |
| esp-rtos0.4.0 | 有 | ~1.2.0-rc.0 | 目前採用 |

2026-10-08 複查 crates.io：esp-radio 最新仍是 1.0.0-beta.1（最新 stable 為 0.18.0）；esp-rtos 0.4.0、esp-alloc 0.11.0、esp-phy 0.3.0 也仍是最新；esp-hal 已有 1.2.2，專案固定 `=1.2.0`，beta.1 要求 `~1.2.0`，升級屬 patch 範圍但要重驗。

radio beta.1 的 C61 feature 串接 HAL、RTOS、esp-phy、esp-wifi-sys-esp32c61 與 scheduler driver，不只有 feature 名稱。其 `Interface` 實作 embassy-net-driver0.2，可接目前 embassy-net0.8。

## 實際連結證據

獨立最小程式 release build／ELF link 成功，包含可達的 PSRAM 初始化、RTOS 啟動、WPA2 station controller、`connect_async`、embassy-net DHCP runner、TCP accept80、UDP bind／receive5353 呼叫。這驗證了 API／driver ABI 的編譯與連結，沒有驗證執行結果，也不是完整 HTTP 或 mDNS 測試。

ELF symbol table 可見 `esp_wifi_start`／`esp_wifi_init_internal`／`esp_wifi_connect_internal`（`scripts/check-wifi-build.sh` 以這三個符號檢查完整韌體的 Wi-Fi ELF）；probe 不是只匯入 crate 的空程式。`size` 結果 text647,064／data6,992／bss94,616 bytes，不能據此當作完整 Pulp 的 RAM／radio runtime 預算。

| 直接依賴 | 固定版本／features |
|---|---|
| esp-hal | 1.2.0；esp32c61、unstable |
| esp-rtos | 0.4.0；esp32c61、embassy、log-04、esp-radio、esp-alloc（後兩者由 `wifi` feature 加入） |
| esp-alloc | 0.11.0；esp32c61 |
| esp-radio | 1.0.0-beta.1；default-features=false；esp32c61、wifi、log-04、esp-alloc |
| esp-bootloader-esp-idf | 0.6.0；esp32c61 |
| embassy-executor | 0.10.0 |
| embassy-net | 0.8.0；dhcpv4、medium-ethernet、multicast（mDNS 加入群組）、tcp、udp |
| embassy-futures | 0.1（解析為0.1.2） |

傳遞依賴包含 esp-radio-rtos-driver0.4.2、esp-phy0.3.0、esp-wifi-sys-esp32c61 0.3.0。目標是 `riscv32imac-unknown-none-elf`；MSRV 至少1.95.0。應將這組版本及 lockfile 視為實作起點，升級要重驗，不能認定 beta API 已穩定。

probe 位於 `/tmp/pulp-c61-wifi-probe`，建置 log `/tmp/pulp-c61-wifi-probe.log`，target `/tmp/pulp-c61-compile-target`。因本地 compiler／target core 不一致，使用已安裝 toolchain 的實際 cargo/rustc 路徑與 `RUSTC_BOOTSTRAP=1`、`-Z build-std=core,alloc`，offline 建置。這是隔離研究方式，不是正式工具鏈方案。

## 原 upload 的遷移（已完成）

舊 `src/apps/upload.rs` 已拆成 `src/apps/upload/{mod,session,connect,http,mdns}.rs`。當時列的問題現況：

- API 遷移：已改用 `WifiController::new`／`Interface::try_station`／`StationConfig`（`wifi::sta`）／`AuthenticationMethodConfig`，SSID／password 以有界型別驗證。
- radio optimization：上游要求 level 2 或 3；`Cargo.toml` 的 `[profile.*.package.esp-radio]` 設 `opt-level = 3`，全域維持 `s`，由 `scripts/check-wifi-build.sh` 從 rustc 命令列驗證。
- 生命週期：一次 session 的 radio／interface／runner 由 `session::run` 以 RAII 擁有，退出、失敗、再入的 source 證據見歸檔 change 的 `baseline.md`（T5）。
- C61 internal SRAM：Wi-Fi 建置的 statics 增加約 68 KB，main heap 因此從 96 KiB 縮到 52 KiB；數字與重現指令見歸檔 change 的 `budget-report.md`。Upload 的 TCP／HTTP／目錄列表緩衝放在 PSRAM（`NetScratch` class），radio 本身不能（見下節）。
- 仍未驗證（實機）：radio 執行期 heap 高水位、stack 高水位、deinit 後 allocation 是否歸還、重複進出後的高水位。

## radio 的記憶體行為（原始碼，beta.1）

- radio 的核心配置走 `malloc_internal`／`InternalMemory`（`esp-radio/src/compat/malloc.rs`、`wifi/os_adapter/mod.rs`），只能放 internal RAM；Pulp 的 PSRAM 在私有 heap，radio 永遠拿不到，也不應該拿到。
- radio 的配置直接走 esp-alloc，不經過 Pulp 的 `MemoryBudget`，所以預算看不到 radio 佔用的 internal heap。
- `ControllerConfig::default()`：靜態 RX buffer 10、動態 RX 32、動態 TX 32、AMPDU RX／TX 開、`rx_ba_win` 6；每個 buffer 約 1.6 KB（上游註解）。Pulp 目前沒有調整。這些 setter 標 `unstable`，要啟用 esp-radio 的 `unstable` feature（esp-hal 的 `unstable` 已啟用）；是否調整，等實機量到 heap／stack 吃緊再決定。
- esp-phy 的 env 選項：`phy_full_calibration` 預設 true（每次 init 完整校準）、`phy_enable_usb` 預設 true（Wi-Fi 期間保留 USB，上游建議關閉以提升效能，但關閉會失去 USB log）。校準資料可用 `backup_phy_calibration_data`／`set_phy_calibration_data` 自行保存。
- station 的省電預設為 `PowerSaveMode::None`。

## 決策

目前沒有「C61 完全不支援 Wi-Fi」的證據。離線閱讀版本與 Wi-Fi 上傳都已實作，Wi-Fi 以 `wifi` feature 隔離（預設關閉，不連結 radio）；兩者都還沒有實機驗證。實機驗收包括 AP association、DHCP、HTTP 檔案內容、mDNS、重連／退出、heap／stack 高水位、電流；link pass 與靜態預算不能證明這些。

來源：[radio beta.1 manifest](https://github.com/esp-rs/esp-hal/blob/esp-radio-v1.0.0-beta.1/esp-radio/Cargo.toml)、[radio source／optimization requirement](https://github.com/esp-rs/esp-hal/blob/esp-radio-v1.0.0-beta.1/esp-radio/src/lib.rs)、[beta.1 migration](https://github.com/esp-rs/esp-hal/releases/tag/esp-radio-v1.0.0-beta.1)。本次具體 version／feature／API 判斷亦逐項對照已下載 crate 原始碼。
