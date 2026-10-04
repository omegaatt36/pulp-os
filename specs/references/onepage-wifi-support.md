# C61 Wi-Fi support check

2026-10-04，針對目前 Pulp 與 crates.io cache 原始碼；沒有裝置，未掃描／連線任何 AP。本文件保存研究結論，後續實作不得依賴暫存 probe。

## 版本證據

| Crate | C61 feature | HAL 相容版本 | 判斷 |
|---|---|---|---|
| Pulp 現有 esp-radio0.17.0 | 無 | 1.0 | 不能直接改 chip feature |
| Pulp 現有 esp-rtos0.2.0 | 無 | 1.0 | 需要升級 |
| esp-radio0.18.0 | 有 | ~1.1.0-rc.0 | 有 C61 路徑，但不能直接搭 HAL1.2 |
| esp-radio1.0.0-beta.0 | 有 | ~1.1.0 | 仍非 HAL1.2 配套 |
| esp-radio1.0.0-beta.1 | 有 | ~1.2.0 | 本次 compile／link 候選 |
| esp-rtos0.4.0 | 有 | ~1.2.0-rc.0 | 本次 compile／link 候選 |

radio beta.1 的 C61 feature 串接 HAL、RTOS、esp-phy、esp-wifi-sys-esp32c61 與 scheduler driver，不只有 feature 名稱。其 `Interface` 實作 embassy-net-driver0.2，可接目前 embassy-net0.8。

## 實際連結證據

獨立最小程式 release build／ELF link 成功，包含可達的 PSRAM 初始化、RTOS 啟動、WPA2 station controller、`connect_async`、embassy-net DHCP runner、TCP accept80、UDP bind／receive5353 呼叫。這驗證了 API／driver ABI 的編譯與連結，沒有驗證執行結果，也不是完整 HTTP 或 mDNS 測試。

ELF symbol table 可見 `esp_wifi_start`／`esp_wifi_init_internal`／`esp_wifi_connect_internal`；probe 不是只匯入 crate 的空程式。`size` 結果 text647,064／data6,992／bss94,616 bytes，不能據此當作完整 Pulp 的 RAM／radio runtime 預算。

| 直接依賴 | 固定版本／features |
|---|---|
| esp-hal | 1.2.0；esp32c61、unstable |
| esp-rtos | 0.4.0；esp32c61、embassy、esp-radio、esp-alloc |
| esp-alloc | 0.11.0；esp32c61 |
| esp-radio | 1.0.0-beta.1；default-features=false；esp32c61、wifi、esp-alloc |
| esp-bootloader-esp-idf | 0.6.0；esp32c61 |
| embassy-executor | 0.10.0 |
| embassy-net | 0.8.0；dhcpv4、medium-ethernet、tcp、udp |
| embassy-futures | 0.1（解析為0.1.2） |

傳遞依賴包含 esp-radio-rtos-driver0.4.2、esp-phy0.3.0、esp-wifi-sys-esp32c61 0.3.0。目標是 `riscv32imac-unknown-none-elf`；MSRV 至少1.95.0。應將這組版本及 lockfile 視為實作起點，升級要重驗，不能認定 beta API 已穩定。

probe 位於 `/tmp/pulp-c61-wifi-probe`，建置 log `/tmp/pulp-c61-wifi-probe.log`，target `/tmp/pulp-c61-compile-target`。因本地 compiler／target core 不一致，使用已安裝 toolchain 的實際 cargo/rustc 路徑與 `RUSTC_BOOTSTRAP=1`、`-Z build-std=core,alloc`，offline 建置。這是隔離研究方式，不是正式工具鏈方案。

## 原 upload 需要遷移的部分

- `src/apps/upload.rs` 使用 `esp_radio::init`、`wifi::new`、`ClientConfig`／`ModeConfig`、`start_async`；beta.1 改為 controller／station interface 與新配置契約。
- station config 位於 `wifi::sta::StationConfig`；認證透過 `AuthenticationMethodConfig`，SSID／password 的有界型別會拒絕超長值。
- 上游要求 radio optimization level2或3；Pulp 全域 dev／release 是 `s`。Wi-Fi 實作要配置 radio 專用最佳化，不能把 compile pass 視為 runtime 可用。
- C61 internal SRAM 比 C3 少；radio／scheduler 的 internal allocations、stack 及 PSRAM 的分工需要量測。
- 必須重驗 radio／interface／runner 在退出、失敗和再次進入時的生命週期；不能沿用一次初始化就永不釋放的假設。

## 決策

目前沒有「C61 完全不支援 Wi-Fi」的證據。第一階段仍先交付 Wi-Fi disabled 的離線閱讀版本；Wi-Fi 上傳放在獨立 change，避免 beta API／runtime 驗證拖住 port 和 CJK。實機驗收包括 AP association、DHCP、HTTP 檔案內容、mDNS、重連／退出、heap／電流；link pass 不能證明這些。

來源：[radio beta.1 manifest](https://github.com/esp-rs/esp-hal/blob/esp-radio-v1.0.0-beta.1/esp-radio/Cargo.toml)、[radio source／optimization requirement](https://github.com/esp-rs/esp-hal/blob/esp-radio-v1.0.0-beta.1/esp-radio/src/lib.rs)、[beta.1 migration](https://github.com/esp-rs/esp-hal/releases/tag/esp-radio-v1.0.0-beta.1)。本次具體 version／feature／API 判斷亦逐項對照已下載 crate 原始碼。
