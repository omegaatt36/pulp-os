# T1b test author brief — onepage-wifi-upload（R12 預算變體）

你是 **test author**。只寫測試，**不得讀取或修改實作檔**（`board-logic/src/**`、`kernel/**`、`src/**`、`Cargo.toml`）。可以讀：`board-logic/tests/font_memory.rs`（現有 integration test 的寫法／import 慣例）、`board-logic/Cargo.toml`、`scripts/test-board-logic.sh`、`specs/changes/onepage-wifi-upload/{spec.md,proposal.md,baseline.md}`。repo：`/Users/raiven_kao/dev/pulp-os`，不要 commit。

## Requirement（未標註 provenance；模糊就停下回報）

- R12: WHILE Wi-Fi enabled THE SYSTEM SHALL 保持有證據的 internal heap／stack 與 PSRAM 預算。

## 已定的決定（proposal.md Assumptions，[human]）

wifi 建置（build-time，非 runtime）把 C61 `INTERNAL_HEAP_MAIN_BYTES` 由 96 KiB 縮為 **52 KiB**；離線建置維持 96 KiB。`STACK_MIN_BYTES`（48 KiB）、`STATIC_RAM_MAX_BYTES`（= `C61_RAM_LEN - STACK_MIN_BYTES`）、`INTERNAL_HEAP_RECLAIMED_BYTES`（64,000）都**不變、不放寬**。

## 實測證據（T1 implementer 量測，2026-10-07，`baseline.md`）

- 離線 C61 firmware 的 statics 合計：180,932 B。
- C61+wifi 的 statics 合計：249,272 B（比離線多 **68,340 B**，含 main heap 本身的 96 KiB 不變）。
- 預算：`STATIC_RAM_MAX_BYTES` = 207,472 B。

## 契約（你只能針對這個 API 寫測試；實作者會依此新增）

在 crate `pulp_board_logic`，模組 `memory`：

- `pub const INTERNAL_HEAP_MAIN_BYTES_WIFI: usize`
- `pub const fn internal_heap_main_bytes(wifi: bool) -> usize`：`false` → 既有 `INTERNAL_HEAP_MAIN_BYTES`；`true` → `INTERNAL_HEAP_MAIN_BYTES_WIFI`。
- `pub const fn internal_heap_bytes(wifi: bool) -> usize`：該變體的 main + `INTERNAL_HEAP_RECLAIMED_BYTES`。

既有的 `INTERNAL_HEAP_MAIN_BYTES`、`INTERNAL_HEAP_RECLAIMED_BYTES`、`INTERNAL_HEAP_BYTES`、`STACK_MIN_BYTES`、`STATIC_RAM_MAX_BYTES`、`C61_RAM_LEN`、`ALLOC_GRANULE`、`KIB` 等照舊（用 `grep -n "pub const" board-logic/src/memory.rs` 只確認名稱是否 pub 可見可以；不要讀其他實作內容。若某名稱沒有 `pub`，在報告中指出並用字面值代替）。

## 你要交付

新檔 `board-logic/tests/wifi_budget.rs`（integration test，不放進 `board-logic/src`）。測試項（expected value 只能來自上面的決定／實測／requirement）：

1. 離線不動：`internal_heap_main_bytes(false) == 96 * 1024`；`STACK_MIN_BYTES == 48 * 1024`；`INTERNAL_HEAP_RECLAIMED_BYTES == 64_000`。
2. wifi 變體：`INTERNAL_HEAP_MAIN_BYTES_WIFI == 52 * 1024`；`internal_heap_main_bytes(true) == INTERNAL_HEAP_MAIN_BYTES_WIFI`；`internal_heap_bytes(w) == internal_heap_main_bytes(w) + INTERNAL_HEAP_RECLAIMED_BYTES`（兩個 `w` 值）。
3. 預算算式（以實測推導）：`180_932 + 68_340 - (96 KiB - INTERNAL_HEAP_MAIN_BYTES_WIFI) <= STATIC_RAM_MAX_BYTES`；並以此算出 stack 餘裕（`C61_RAM_LEN - statics`）≥ `STACK_MIN_BYTES`。
4. 對照組（防止測試空轉）：不縮 heap 的 wifi（即 `180_932 + 68_340`）**會**超過 `STATIC_RAM_MAX_BYTES`——這證明縮減是必要而非多餘。
5. 單調性：`INTERNAL_HEAP_MAIN_BYTES_WIFI < internal_heap_main_bytes(false)`，且 wifi main heap 為 `ALLOC_GRANULE` 的整數倍。

## 紀律

- **red-proof**：寫完後在實作前執行（`scripts/test-board-logic.sh`，或 `cargo test -p pulp-board-logic --test wifi_budget` 搭配 host triple，擇腳本實際使用的方式），把失敗輸出完整貼進報告。預期是編譯失敗（API 不存在）。若某個測試在實作後也會因算式本身錯誤而永遠失敗／永遠通過，指出來。
- 每個 expected value 說明來源：「requirement」「proposal 的 [human] 決定」「T1 實測」。任何值來自執行實作的輸出就是缺陷。
- 不得 `#[ignore]`／skip。
- 報告（正體中文）：(1) 檔案，(2) 每項測試與 expected value 來源，(3) 實作前的完整失敗輸出，(4) 含糊之處。
