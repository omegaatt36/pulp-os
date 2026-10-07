# onepage-wifi-upload — T1 baseline (2026-10-07)

T1 原始狀態：C61 + wifi 無法連結（internal RAM 靜態區超出預算，main stack 小於 esp-hal 下限）；T1b 以 build-time heap 變體解決，見文末。下面「memory 起點」一節是縮 heap 前的數字。

## 固定的 radio／RTOS set（`cargo tree --locked`，`scripts/check-wifi-build.sh` 已斷言）

| crate | 版本 |
|---|---|
| esp-hal | 1.2.0 |
| esp-rtos | 0.4.0 |
| esp-alloc | 0.11.0 |
| esp-radio | 1.0.0-beta.1（feature `esp32c61`，無 `esp32c3`） |
| esp-bootloader-esp-idf | 0.6.0 |
| embassy-net | 0.8.0 |
| esp-radio-rtos-driver | 0.4.2 |
| esp-phy | 0.3.0 |
| esp-wifi-sys-esp32c61 | 0.3.0 |

rustc `1.100.0-nightly (1303417c4 2026-09-21)`（`rust-toolchain.toml`）。

## radio optimization

- 要求來源：`esp-radio-1.0.0-beta.1/src/lib.rs` 「Optimization level」：radio blobs 需要 opt-level 2 或 3。
- 作法：`Cargo.toml` 加 `[profile.dev.package.esp-radio]`／`[profile.release.package.esp-radio]`，`opt-level = 3`；全域維持 `s`；未提升 esp-phy／esp-wifi-sys（無 upstream 文件要求）。
- 取得方式：`check-wifi-build.sh` 先 `cargo clean -p esp-radio`，再 `cargo build -v`，由 rustc 命令列讀 `opt-level`。C61 與 X4 enabled 皆為 `opt-level=3`。

## memory 起點：C61 + wifi（`cargo build-c61 --features wifi`，release）

連結失敗：`rust-lld: error: Main stack is smaller than 8192 bytes.`（esp-hal `ld/sections/stack.x`，`ESP_HAL_CONFIG_ENSURE_MAIN_STACK_MINIMUM` 預設 8192）。

為了取得數字，另以 `cargo rustc ... -- -C link-arg=--noinhibit-exec` 產生一個不合法的 ELF（僅用於量測，已刪除，不屬於產物），再跑 `C61_ELF=<該 ELF> scripts/report-c61-memory.sh`：

| 項目 | 離線 C61 | C61 + wifi | 差 |
|---|---:|---:|---:|
| .trap | 2656 | 2656 | 0 |
| .rwtext | 11032 | 13368 | +2336 |
| .rwtext.wifi | 0 | 30704 | +30704 |
| .data | 30896 | 37332 | +6436 |
| .data.wifi | 0 | 364 | +364 |
| .bss | 136348 | 164848 | +28500 |
| statics 合計 | 180932 | 249272 | +68340 |
| .stack（= RAM − statics） | 75692 | 7344 | −68348 |
| .dram2_uninit（reclaimed heap） | 64000 | 64000 | 0 |
| .text | 361482 | 815980 | +454498 |
| .rodata | 449904 | 498080 + .rodata.wifi 54080 | +102256 |

`report-c61-memory.sh` 結論（enabled）：

- `FAIL image: StaticsOverBudget { used: 249280, max: 207472 }`（STATIC_RAM_MAX_BYTES = 256624 − 49152）；
- stack headroom over STACK_MIN_BYTES = −41808 B；statics budget left = −41800 B；
- 其餘規則（section placement、DMA buffer 在 internal RAM、PSRAM 單一 region 註冊、`.stack` 對齊 `_stack_start`）皆 ok；
- `RESULT: memory budget check FAILED`。

靜態區中已知來源：`main heap`（`INTERNAL_HEAP_MAIN_BYTES` = 98304，在 .bss）、`READER` 20656、`embassy_main POOL` 19496、`DIR_CACHE` 10764；radio 側新增具名符號合計約 30 KB（`g_cnxMgr` 5256、`esp_phy::PHY_STATE` 1920、`s_wifi_nvs` 1440 等）加上 `.rwtext.wifi` 30704 B 的 IRAM blob 程式碼。

## UNVERIFIED

- radio 執行期所需的 internal heap（esp-radio／esp-alloc 的 internal 配置量與高水位）：未量測，未有 upstream 數字。
- PSRAM 與 internal 的分工（radio buffer 能否放 PSRAM）：未驗。
- stack 高水位：只有靜態推算（RAM − statics），硬體上的實際使用量未量。
- `.rwtext.wifi`（IRAM blob）搬遷／縮減的可行性：未評估。
- 上述預算衝突的解法（縮 `INTERNAL_HEAP_MAIN_BYTES`、調整 `STACK_MIN_BYTES`／`STATIC_RAM_MAX_BYTES`、把 static buffer 移 PSRAM 等）由 orchestrator 決定；T1 未改任何預算常數。

## T1b：wifi 建置縮 main heap（2026-10-07，[human] 決定，見 proposal.md Assumptions）

做法：wifi 建置 `INTERNAL_HEAP_MAIN_BYTES` 96 KiB → 52 KiB（`INTERNAL_HEAP_MAIN_BYTES_WIFI`）；離線建置維持 96 KiB。`STACK_MIN_BYTES`、`STATIC_RAM_MAX_BYTES`、`INTERNAL_HEAP_RECLAIMED_BYTES` 不變。變體由 `internal_heap_main_bytes(wifi)`／`internal_heap_bytes(wifi)` 提供；`pulp-kernel` 新增 `wifi` feature（根 crate `wifi` 轉送），`MemoryBudget::for_build(wifi)` 的 internal pool 跟著變體走。

預算數字（statics、stack 餘裕、heap、future 大小等）不在此重複，一律以 `budget-report.md` 為準（T6 以最終工作樹重新量測）。T1b 當時的結論：`check-wifi-build.sh` 全綠，`report-c61-memory.sh` 對 enabled ELF 為 `RESULT: all memory budget checks ok`；縮 heap 後 internal pool 為 52 KiB + 64000 B。

UNVERIFIED（沿用）：
- radio 執行期 internal heap 需求：52 KiB 是由「statics 需再省 ≥ 41.8 KB」推得的切割，不是由 radio 實際需求量測；可能不足。
- PSRAM 分工（radio buffer 能否放 PSRAM）。
- stack 高水位（`.stack` 區段大小是靜態推算的可用量，不是使用量；硬體上未量，數字見 `budget-report.md`）。

## T5 所有權審查（2026-10-07）

範圍：upload 退出／失敗／再進入時，radio controller、station interface、network stack 的所有權。引用皆為 `esp-radio-1.0.0-beta.1`（`~/.cargo/registry/src/*/esp-radio-1.0.0-beta.1/`）與 `embassy-net-0.8.0`。

### 程式端結構（`src/apps/upload/session.rs`、`mod.rs`）

- `session::run` 持有 `C`（firmware 為 `Net { runner, stack, controller }`），階段 future 借用 `&mut C`；任何結果（back、階段錯誤、逾時）都先 drop 階段 future，再 drop `C`，之後 `run` 才返回（host 測試 `host/tests/upload_session.rs` 以 token／事件順序驗證）。
- `Net` 欄位順序＝drop 順序：`runner`（內含 `Interface`）→ `stack` → `controller`。`StackResources` 在 `run_upload_mode` 的 frame，只存 socket 空間，比 `session::run` 晚 drop（`Stack` 借用它，不能同住一個 struct）。
- `acquire`：`Interface::try_station()` → `None` 回 `RadioUnavailable`（不 panic）；之後才 `WIFI::steal()` 與 `WifiController::new`（失敗回 `RadioUnavailable`，此時 `interface` 隨 scope 結束 drop，singleton 歸還）。
- 錯誤畫面在 `session::run` 返回後才顯示。

### Source 證據

| 事項 | 位置 | 內容 |
|---|---|---|
| station singleton | `src/wifi/mod.rs:1521-1534` | `SINGLETONS: AtomicU8`；`try_acquire(bit)` = `fetch_or(bit) & bit == 0`，`release(bit)` = `fetch_and(!bit)` |
| `try_station` | `src/wifi/mod.rs:1568-1576` | `try_acquire(STA_BIT)` 成功才回 `Some(Interface)`，否則 `None`（`station()` 在 1561 是 `try_station().expect(..)`，會 panic，故不用） |
| `Drop for Interface` | `src/wifi/mod.rs:1633-1641` | `release(STA_BIT/AP_BIT)`：interface 一 drop singleton 立即歸還 |
| runner 持有 interface | `embassy-net-0.8.0/src/lib.rs:255-258`、`298-303` | `Runner<'d, D> { driver: D, stack }`；`embassy_net::new(driver: D, ..)` 以值取得 driver，`Stack<'d>` 只是 `&'d RefCell<Inner>`（265-267）。所以 `Runner` drop ⇒ `Interface` drop ⇒ singleton 歸還；`Stack` 不持有 interface |
| controller 的 RAII | `src/wifi/mod.rs:2635-2638`、`2594-2625` | `WifiController { _guard: WifiRefGuard }`；`Drop for WifiRefGuard` → `WIFI_REFCOUNT.decrement(..)`，最後一個 guard 時把 station／AP state 設為 `Uninitialized` 並呼叫 `wifi_deinit()`（失敗只 `warn!`，不 panic） |
| `wifi_deinit` | `src/wifi/mod.rs:1221-1240` | `esp_wifi_stop`、清空 STA／AP RX queue、`esp_wifi_deinit_internal`、`esp_supplicant_deinit`；註解明說 interface 比 controller 晚 drop 也被容許（RX queue 先排空以免 dangling `eb`） |
| 底層 radio refcount | `src/lib.rs:397-423`、`348-` | `RadioRefGuard`（`RADIO_REFCOUNT`）最後一個 drop 時 `deinit()`：停 wifi ISR、關 modem power domain 與 clocks |
| `new` 的失敗路徑 | `src/wifi/mod.rs:2673-2767` | `RadioRefGuard` 先建立；`try_increment` 失敗時 refcount 還原為 0，`?` 提早返回使 `radio_guard` drop；`new` 後段（`set_country_info` 等）失敗時 `controller` 本身被 drop，走 `WifiRefGuard::drop` |
| 重入 | `src/refcount.rs` | refcount 0→2（首次 init）、2→0（最後一個 deinit），之後可再次 `new`（`new` 內 `first` 為 true 才重新套用 init-only 設定） |

結論（由 source 得到，非實機）：只要 `Net` 被 drop，singleton 與 Wi-Fi／radio refcount 都由 RAII 歸還；沒有 `mem::forget`、`'static` 洩漏或 `Box::leak` 的路徑。`try_station()` 取代 `station()` 後，殘留 owner 只會得到 `RadioUnavailable` 錯誤畫面，不 panic。

### `WIFI::steal()` 審查

- 舊作法：`manager.rs` 在 `run_special_mode` 內 `steal()` 後以值傳入 `run_upload_mode`。`WIFI<'d>` 只是零大小 token，`WifiController::new(device, ..)` 取用後不保存（`wifi_init(_wifi)`，只有 esp32 用它）；風險只在「兩個 controller 同時存在」（`new` 第二次會只 increment refcount 並忽略 init-only 設定，`mod.rs:2740`）。
- 新作法：`steal` 搬進 `acquire`，且放在 `Interface::try_station()` 成功之後。只有持有 station singleton 的 session 才會走到 `steal` 與 `WifiController::new`，所以 session 之間互斥由 source 保證，`unsafe` 的前提（沒有第二個使用者）就地成立並寫在 SAFETY 註解。`manager.rs` 不再 `steal`，`run_upload_mode` 少一個參數。
- 殘餘：`WIFI` 沒有其他 driver 使用（`grep` 全 repo 僅此一處），這是靜態事實，不是型別保證。

### 其他審查結果

- scheduler：`handle_special_mode`（`kernel/src/kernel/scheduler.rs`）在 `run_special_mode` 返回後 `Pop` + `request_full_redraw`；C61 的 `run` 迴圈下一輪由 `render` → `render_full` → `refresh_with_recovery`（`scheduler_c61.rs`）以 `hw.display` 的 `DisplayHealth` 做 full refresh，失敗會 re-init 後重試。upload 畫面經 `board_c61::api::full_refresh_screen` 直接畫（失敗只 log，不動 `hw.display`），因此返回後的第一個 render 才是帶 recovery 的路徑；沒有發現缺口，未改動。upload 期間沒有 `poll_card`／idle sleep（兩者都在 `run` 迴圈內），SD 與 EPD 沒有並行存取。
- `upload_available`：Upload 只在 special-mode 呼叫期間為 active，之後立即 Pop；`sleep_with_session` 只在 `run` 迴圈的 `needs_special_mode()` 檢查之後被呼叫，所以 `collect_session` 看不到 Upload；`apply_session` 的 id mapper 沒有 Upload（id 4 → Home）。推論成立，值維持 `false`，註解改為此理由。

### UNVERIFIED（交給 T7 實機）

- `wifi_deinit`／`deinit()` 是否把 radio 內部 allocation 全數還給 heap（source 只到 `esp_wifi_deinit_internal`，blob 內部不可見）。
- 重複 N 次（含失敗、BACK、逾時各路徑）後的 heap 高水位。
- Wi-Fi 重新 init 的耗時（影響第二次進入的等待與 `ASSOCIATE_TIMEOUT` 是否足夠）。
- esp-rtos 為 radio 建立的任務是否在 deinit 後殘留。
- `wifi_init` 中途失敗（如 `esp_supplicant_init` 失敗）時，`esp_wifi_init_internal` 已完成的部分是否被還原：`wifi_init`（`src/wifi/mod.rs:1163-1190`）沒有 rollback，`try_increment` 只還原 refcount；upstream 行為，未驗證其後果。
- X4 的 `full_refresh_async` 在 `serve` 開頭 render 時被 BACK 取消（`session::run` 以 back 與所有階段競爭）的 panel 狀態：C61 的 refresh 是同步、沒有 await 點，不受影響；X4 未驗，後續由 scheduler 的 full redraw 接手。
- 本 task 的 host 測試以 fake `C` 證明 `session::run` 的所有權順序；真實 `Interface`／`WifiController` 的 drop 只有上表的 source 證據，沒有實機證據。
