# T1 test author brief — onepage-wifi-upload

你是 **test author**。只寫測試／檢查腳本，**不得讀取或修改實作檔**（`src/**`、`kernel/src/**`、`board-logic/src/**`、`Cargo.toml`、`.cargo/config.toml`、`build.rs`）。可以讀：`scripts/**`（現有檢查腳本的慣例）、`specs/changes/onepage-wifi-upload/{spec.md,proposal.md}`、`specs/references/onepage-wifi-support.md`、`rust-toolchain.toml`。repo：`/Users/raiven_kao/dev/pulp-os`，branch `onepage`，不要 commit。

## Task

T1：固定 C61 radio set／optimization，建立 enabled／disabled build／link smoke，記錄版本與 memory 起點。

## 本 task 滿足的 requirements（未標註 provenance；若你認為模糊就停下回報，不要自行解讀）

- R1: WHERE Wi-Fi enabled THE SYSTEM SHALL 使用有 C61 支援且相容的固定 radio／RTOS dependencies 連結 release ELF。
- R2: WHERE Wi-Fi enabled THE SYSTEM SHALL 以 radio 要求的 optimization level 2 或 3 建置 radio package。
- R3: WHERE Wi-Fi disabled THE SYSTEM SHALL 保持不連入 radio driver 的離線版本。
- R12（本 task 只負責「起點」那一半）: WHILE Wi-Fi enabled THE SYSTEM SHALL 保持有證據的 internal heap／stack 與 PSRAM 預算。

## 你要交付

一支新腳本 `scripts/check-wifi-build.sh`（bash，風格比照 `scripts/check-offline-boundary.sh`：`source scripts/lib/tools.sh`、`ok`/`FAIL` 輸出、任何 FAIL 非零結束、獨立 `CARGO_TARGET_DIR` 在 `target/` 下、可用環境變數覆寫 target root），驗證下列**可觀察行為**（期望值只能從 requirement 與 reference 文件推導，不得執行實作後抄輸出）：

1. **R1 enabled link**：C61 目標 `riscv32imac-unknown-none-elf`，features `board-onepage-c61,wifi`，`--release --locked`，`pulp-os-c61` 這個 bin 連結成功，ELF 存在。ELF 內有 radio driver 的非 absolute symbol（與 `check-offline-boundary.sh` 的 `SYM_RE`、absolute symbol 排除規則一致），且可見 `esp_wifi_start`／`esp_wifi_init_internal`／`esp_wifi_connect_internal` 三者（reference 文件說 probe ELF 有這些）。
2. **R1 固定版本**：用 `cargo tree --locked`（含 `-e features` 視需要）驗證該 enabled 建置解析出的版本就是 reference 文件列出的固定組：esp-hal 1.2.0、esp-rtos 0.4.0、esp-alloc 0.11.0、esp-radio 1.0.0-beta.1、esp-bootloader-esp-idf 0.6.0、embassy-net 0.8.x；並且 esp-radio 在 C61 enabled 圖中帶 `esp32c61` feature、**不帶** `esp32c3`；esp-radio 版本來自 `Cargo.lock`（`--locked` 即可）。X4 + wifi 同樣不得使用過舊的 esp-radio 0.17／esp-rtos 0.2。
3. **R2 optimization**：以 `cargo build -v`（或 `--message-format=json` 搭配 `-v` 的 rustc 指令列）擷取 `esp_radio` crate 實際編譯時的 `opt-level`，必須是 2 或 3；C61 與 X4（`riscv32imc-unknown-none-elf`，features `board-x4,wifi`）兩個 enabled 組態都要驗。注意：增量／快取命中時 `-v` 可能不重印 rustc 指令——請讓腳本在專用 target dir 內以可重現方式取得指令（例如對該 package 觸發 fresh build，或解析 `target/.../*.d`／`.fingerprint`，擇一並在腳本註解說明理由）。禁止只靠 `grep Cargo.toml`。
4. **R3 disabled**：C61 與 X4 的預設（無 `wifi`）建置，`cargo tree -e normal -i` 找不到 `esp-radio`、`embassy-net`、`smoltcp`、`esp-wifi-sys`，且 ELF 無非 absolute 的 radio／net symbol（`esp_radio|esp_wifi|embassy_net|smoltcp`）、無 `pulp.local`／`Upload` 字串。可以呼叫或重用現有 `scripts/check-offline-boundary.sh`，但如果它的內容與本需求矛盾（它目前假設 C61 永遠離線、`src/lib.rs` 有 compile_error 拒絕 C61+wifi），你**可以修改該腳本**讓它與 requirement 一致：C61 離線仍須 absent，並新增 C61+wifi 作為 positive control。腳本的 probe 本身必須有正控制組（enabled ELF 一定要探得到 symbol，否則視為 probe 壞掉）。
5. **R12 起點**：以 `scripts/report-c61-memory.sh` 的 `C61_ELF=<enabled ELF>` 模式對 enabled ELF 產出報告；腳本把 `size`（text／data／bss）與報告結果寫入一個機器可讀的檔案（例如 `$root/wifi-memory-baseline.txt`），並在 stdout 印出。**此項只驗證「報告能產生、內容含 text/data/bss 與 report 腳本的結論」，不要硬寫 heap／stack 的數字門檻**（實測前沒有證據支持任何門檻）。若 `report-c61-memory.sh` 對 enabled ELF 的預算檢查失敗，腳本要把這個失敗**如實呈現並使整體失敗**，不要吞掉；implementer 會處理（調整預算或回報）。

## 紀律

- **red-proof**：寫完後在目前（實作前）的 tree 上執行 `scripts/check-wifi-build.sh`，把失敗輸出完整貼進你的報告。預期它現在失敗，因為 `src/lib.rs` 的 `compile_error!` 擋住 C61+wifi，且沒有 radio 的 optimization 設定。若它現在就通過某些項目，說明為什麼（那項可能沒在驗任何東西）。
- 每個 expected value 說明來源：「requirement 文字」或「reference 文件」；任何一個來自執行實作的輸出就是缺陷，改寫或回報 requirement 不明。
- 不要修改 `src/**`、`kernel/**`、`Cargo.toml`、`.cargo/**`。不要新增 `#[ignore]`／skip 開關讓腳本在缺工具時靜默通過——缺工具要 FAIL。
- 長時間建置：先 `cargo build-c61`／`cargo build-x4` 預熱 cache 通常很快（已有 `target/`），但 enabled build 會編譯 radio，可能數分鐘；用較長 timeout。
- 報告格式：(1) 新增／修改的檔案清單，(2) 每個檢查項對應的 requirement 與 expected value 來源，(3) 實作前的完整失敗輸出，(4) 任何你認為 requirement 含糊之處。
