# onepage-pre-hardware-hardening

## Why

實機到貨前，對目前 checkout（`3fcbbea`）的獨立審查找到：幾個「驗收機制會產生假訊號」的主機端問題（stack analyzer 缺資料仍印 100% 餘裕、字庫 cache 對程式碼變更失效不完整、文件指令照抄會編譯失敗），一處 C61 EPD init 位元組很可能照搬 X4 而非 BSP 對本面板的值，以及 `hardware-acceptance.md` 幾個會造成誤判或自相矛盾的判定。這些都能在沒有板子時修掉；修完後實機驗收才不會被工具或文件本身誤導。

## Scope

- In：A1–A6 主機端程式／工具／指令修正；C1 upload multipart 結尾驗證；B1–B6 `hardware-acceptance.md` 與周邊文件的判定、儀器對照、模板與過時引用修正。
- Out：upload 認證／session token（新功能，另開 change）；async BUSY（待手感驗收後再議）；任何實機驗證宣稱；C61 以外 X4 行為變更。
- 前置：四個 archived change 的軟體基線（`task acceptance` 全綠，1366 passed／0 failed／1 ignored）。只使用目前 checkout。

## Impact

`README.txt`、`specs/changes/archive/onepage-cjk-iansui/install.md`、`specs/references/hardware-acceptance.md`、`fontconv/src/bundle.rs`＋測試、`harness/src/bin/stack_analyzer.rs`＋測試、`Cargo.toml`（C61 `esp-println` feature）、`board-logic/src/ssd1677.rs`＋golden、`kernel/src/kernel/scheduler_c61.rs`、`kernel/src/board_c61/keys.rs`／`src/bin/c61_boot.rs`（ladder mV log）、`src/apps/upload/http.rs`＋host 測試、`Taskfile.yml`、`specs/changes/README.md`、`onepage-c61-port/bringup.md`、新增 `specs/hardware-records/`。

## Risk

**yellow** — 驗證獨立性低：R5（booster 位元組）與 R4（log channel）改變硬體行為，主機無法驗證；R13 改變 HTTP 結尾判定語意。critical path 上僅 R14 是 `[human]`，其餘為 `[derived]`／`[assumed]`；R5 的外部依據是 BSP 原始碼，不是使用者陳述，也不是實機。不涉及 auth／crypto／migration，故未列 red。

## Assumptions

- A1：**R5 的 BSP 值適用於使用者手上的面板。** BSP 以 `use_otp_voltages=true` 驅動 EPD0426A02（`bsp_onepage_c61/board_c61.c:213`；moui 對應送 `AE C7 C3 80 C0`），但使用者面板是否為該型未確認（D10）。改後仍是 UNVERIFIED，失敗時回到 `hardware-acceptance.md` S4。
- A2：**R7 的 ladder mV log 只要求 boot 映像**（A5a 在 G1 用 boot 映像）；格式為隨 `key:` 事件附帶 mV，細節由實作者定，不新增 task 之外的 UI。完整韌體是否也要輸出不在本 change。
- A3：**R13 一律要求結尾 `--`。** 非最終 delimiter 後（`\r\n`）視為格式錯誤、回錯誤且不得記錄 `upload: file saved as`；curl 與瀏覽器單檔上傳皆送最終 `--`，依 R13 驗收；若現有 host 測試的 fixture 缺 `--`，視為 fixture 錯誤而非放寬 R13。
- A4：**R9／R10 的拆分只改文件判定。** 被延後的項目（A4d／P10）保留編號並標示延後原因，不刪除。（原文寫「不增減驗收項目的總數」不精確：R9 把 A7c、A7d 各拆為兩項，狀態列由 80 增為 82；見 resolved miss。）
- A5：**儀器對照（R11）以公開規格頁為據**（Seeed XIAO Debug Mate power meter：1 µA–1 A、10 µA 以上 ±1%；取樣率與 burden voltage 官方頁未載），未實測。

### Resolved misses（實作中證明 spec 有盲點，2026-10-08）

- M1：**A4 的「總數不變」不成立**——R9 的拆分使 `hardware-acceptance.md` 狀態列 80 → 82（A7c-1／A7c-2、A7d-1／A7d-2）。不影響任何需求。
- M2：**R11 沒預見 `.gitignore` 的 `*.md` 會排除 `specs/hardware-records/`**，模板形同未交付。已加 `!specs/hardware-records/**/*.md` 並補進 R11。
- M3：**R12 沒涵蓋 A7a**。核對程式碼發現 A7a 的睡眠序列首行 `session: saved to slot …` 不屬睡眠路徑（只在開機無有效 session 時出現），`sleep: session saved before power-off` 排在 `shared lines driven low` 之後。已修正並補進 R12。
- M4：**R6 的 log 與開機那行並非逐字同格式**：週期行多 ` (periodic)` 後綴，因 `log!` 會拆分格式字串，無後綴就無法用 ELF 字串證明週期行有連進映像。R6 文字（含 mV 與百分比）仍成立。
- M5：**R3 的修正使 stack analyzer 對現有映像一律回報 unknown**（C61 映像沒有 `.stack_sizes`）。這是正確行為，但表示目前沒有任何由該工具得出的 stack 餘裕數字；改用 `-Z emit-stack-sizes` 建置另案。
- M6：**subagent 在 T7 擅自使用 `git stash`**（brief 未禁止）。事後核對無損；T8 之後的 brief 已明確禁止。

- [human, 2026-10-08] 歸檔時接受：R8–R12、R14、R15 無自動化 red-proof 的例外；R7、R13 仍為 `[assumed]` 並保留標記。
