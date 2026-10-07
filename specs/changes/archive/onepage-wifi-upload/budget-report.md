# onepage-wifi-upload — 預算報告（T6，R12／R13）

量測對象：T6 完成後的最終工作樹（未 commit），rustc `1.100.0-nightly (1303417c4 2026-09-21)`，release。所有數字是**靜態**量測（ELF、`-Zprint-type-sizes`），沒有任何實機數字。

## 重現

| 數字 | 指令 |
|---|---|
| 離線 C61 ELF | `scripts/check-offline-boundary.sh` 產出 `target/offline-boundary/c61/riscv32imac-unknown-none-elf/release/pulp-os-c61`；`C61_ELF=<elf> scripts/report-c61-memory.sh` |
| wifi C61 ELF | `scripts/check-wifi-build.sh`（內含 report；ELF 在 `target/wifi-build/offline-boundary/c61-wifi/...`，與 `target/offline-boundary/c61-wifi/...` 內容相同，差 12 B 為路徑字串）；`report-c61-memory.sh` 由 ELF 內 radio symbol 判定變體並印 `build variant: wifi` |
| async future 大小 | `CARGO_TARGET_DIR=<專用目錄> cargo rustc --release --locked --target riscv32imac-unknown-none-elf --features board-onepage-c61,wifi --lib -- -Zprint-type-sizes` |

## 記憶體：離線 vs wifi（C61）

| 項目 | 離線 | wifi | 差 |
|---|---:|---:|---:|
| statics（.trap .rwtext .data .bss .noinit，`check_image`） | 180,940 | 204,664 | +23,724 |
| .stack（= RAM − statics） | 75,684 | 51,960 | −23,724 |
| stack 餘裕（比 STACK_MIN_BYTES 49,152 多出） | 26,532 | 2,808 | −23,724 |
| statics 預算（STATIC_RAM_MAX_BYTES 207,472）剩餘 | 26,532 | 2,816 | |
| main heap static（`INTERNAL_HEAP_MAIN_BYTES`，在 .bss） | 98,304（96 KiB） | 53,248（52 KiB） | −45,056 |
| internal heap（main + reclaimed） | 162,304 | 117,248 | −45,056 |
| .dram2_uninit（reclaimed heap） | 64,000 | 64,000 | 0 |
| .rwtext | 11,032 | 13,368 | +2,336 |
| .rwtext.wifi（IRAM blob） | 0 | 30,704 | +30,704 |
| .data / .data.wifi | 30,904 / 0 | 37,564 / 364 | |
| .bss | 136,348 | 120,000 | −16,348 |
| `embassy_main` `POOL` | 12,352 | 19,704 | +7,352 |
| .text | 361,514 | 822,106 | |
| .rodata / .rodata.wifi | 449,904 / 0 | 499,248 / 54,080 | |

說明：
- wifi 的 statics 在 `report-c61-memory.sh` 的「internal RAM summary」列為 204,656，`check_image` 列為 204,664；差 8 B 是同一份輸出內兩處的對齊口徑差，兩者都小於預算 207,472。離線兩處皆為 180,940。
- stack 餘裕 2,808 B 與 T5 結束時相同；T6 的程式改動（`/files` 先列後寫、`/delete` 名稱驗證、DHCP 階段輪詢 IPv4、刪 `bring_up`）沒有改變 statics。
- 「stack 餘裕」是 `.stack` 區段大小減去 `STACK_MIN_BYTES`，不是實際使用量。

## async future 大小（wifi，`-Zprint-type-sizes`，bytes）

只報事實。T6 前（還原 T6 的 http.rs／mod.rs 改動後同法量測）與 T6 後：

| future | T6 前 | T6 後 |
|---|---:|---:|
| `run_upload_mode` | 16,192 | 16,192 |
| `session::run` | 14,216 | 14,216 |
| serve closure（`mod.rs` 第 167 行起的 async closure） | 13,944 | 13,944 |
| `Select3<Runner::run, serve_http, serve_mdns>` | 10,288 | 10,288 |
| `serve_http` | 8,968 | 8,968 |
| `serve_one_request` | 8,912 | 8,912 |
| `http::serve_request<TcpSocket>` | 8,856 | 8,856 |
| `http::handle_upload<TcpSocket>` | 2,276 | 2,276 |
| `serve_mdns` | 1,304 | 1,304 |
| dhcp closure | 48 | 56 |

- 唯一變大的是 dhcp closure（+8 B，內含輪詢 IPv4 的迴圈）；它的外層 future（`session::run`、`run_upload_mode`）大小不變。
- `serve_request` 的 `entries: [DirEntry; 64]` 為 5,376 B（兩次量測相同），佔 `serve_request` 的 60.7%（5,376 / 8,856）、`run_upload_mode` 的 33.2%（5,376 / 16,192）。在 `serve_request` 的 layout 中，`.hdr`（1,024 B）之後有一段 5,464 B 的重疊區，`entries` 位在這一段，`handle_upload` 的 future（2,276 B）排在其後（Suspend13 variant 總計 8,855 B）。
- G3（`/files` 先列後寫）：`entries` 本來就跨 await（列表中途寫 JSON），這次只是把列表移到寫標頭之前，沒有新增跨 await 的大型局部變數；`serve_request` 維持 8,856 B。錯誤路徑（500）是新增的 `send_error_response` await，其 variant（Suspend6）6,675 B，小於最大 variant（Suspend13）8,855 B，所以不影響總大小。
- 沒有量「去掉 `entries` 後 `serve_request` 會變多小」，不推測。

## R12 UNVERIFIED

以下全部沒有實機或執行期證據：

- radio 執行期 internal heap 需求與高水位：52 KiB main + 64,000 B reclaimed = 117,248 B 是由「statics 要省 ≥ 41.8 KB」推得的切割，不是由 radio 實際配置量測得；可能不足。
- stack 高水位：上表的 51,960 B／2,808 B 餘裕是靜態推算；實際最深呼叫鏈未量。上表 future 的外層（`run_upload_mode`）由 `embassy_main` 的 task `POOL`（19,704 B，static）承載，這是由大小對得上（16,192 B 在 19,704 B 內）作的推論，未逐一驗證每個 future 的放置。
- PSRAM 分工：radio buffer 能否放 PSRAM、`embassy_net` 的 socket buffer（`rx_buf`／`tx_buf` 是 serve closure 跨 await 的區域變數，共 3,584 B，位於該 future 內）位置，未驗；`report-c61-memory.sh` 斷言的 PSRAM 註冊規則（單一 External region、私有 `PSRAM_HEAP`）在 wifi ELF 上為 ok，但這只是註冊規則，不是 radio 的分配行為。
- 重入高水位：反覆進出 upload（BACK、失敗、逾時各路徑）後的 heap／stack 高水位與 radio deinit 後的 allocation 歸還（T5 的 source 證據只到 `esp_wifi_deinit_internal`）。
- 52 KiB main heap 對閱讀路徑的影響：wifi 建置的離線功能（EPUB／圖片解碼的 internal 配置）在 internal heap 縮小後是否仍夠用，未跑任何閱讀路徑的 wifi 建置實測；`board-logic/tests/wifi_budget.rs` 與 `report-c61-memory.sh` 只驗常數與映像配置，不是閱讀路徑的執行期證據。
- R4 的 IPv4：`smoltcp` 沒有 `proto-ipv6` 由 `check-wifi-build.sh` 守門，dhcp 階段輪詢直到 `config_v4()` 為 `Some`，這是 source／編譯證據；對真實路由器的 association、DHCP 與取得位址未驗。
- HTTP 單請求逾時：socket 層 30 s（`HTTP_TIMEOUT_SECS`）沒有改，對「靜默對端」的實際行為未驗（T7）。
