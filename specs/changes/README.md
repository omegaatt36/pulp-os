# OnePage agent handoff

目標順序：OnePage C61 離線閱讀 → 繁體中文／Iansui → Wi-Fi 與其他功能。C61 port 已完成軟體候選版並封存；Wi-Fi 亦已完成軟體候選版並封存。沒有實機，hardware acceptance 均待驗。只使用目前 checkout，不引用其他分支或既有 spike。

| Change | Tasks | 接手條件 |
|---|---:|---|
| [onepage-c61-port](archive/onepage-c61-port/proposal.md) | 14＋3 項修正 | 已封存；軟體驗收通過，歷史證據缺失依使用者授權保留，硬體未驗 |
| [onepage-host-validation](archive/onepage-host-validation/proposal.md) | 7 | 已封存；host 測試／preview 通過（見 evidence.md），R1 linker 掃描器、R7 詮釋、provenance 待補，硬體未驗 |
| [onepage-cjk-iansui](archive/onepage-cjk-iansui/proposal.md) | 10 | 已封存；host 測試 719 通過，R1–R14／R16 untagged、R14 限縮、R8／R16 無完整 red-proof 經使用者接受，硬體未驗 |
| [onepage-wifi-upload](archive/onepage-wifi-upload/proposal.md) | 7 | 已封存；host880／board333、enabled／disabled link通過；3項red-proof例外與未標註provenance經使用者接受，實機未驗 |
| [onepage-pre-hardware-hardening](archive/onepage-pre-hardware-hardening/proposal.md) | 11 | 已封存；`task acceptance` 1394／0／1；R8–R12／R14／R15 無 red-proof 與 R7／R13 `[assumed]` 經使用者接受，硬體仍未驗 |

Partial refresh（commit `4785a36 feat: add partial refresh`，涉及 `board-logic/src/ssd1677.rs`、`kernel/src/kernel/scheduler_c61.rs`、`partial-refresh` cargo feature）**沒有 change 資料夾**；程式與 host 證據、實機項目 P0–P10 見 [../references/hardware-acceptance.md](../references/hardware-acceptance.md) G6。預設映像不含此功能，實機未驗。

**實機到手後的入口：[../references/hardware-acceptance.md](../references/hardware-acceptance.md)**（刷機、關卡順序、四個 change 全部未驗項目的總表）。

「獨立 session」指接手時無須前一段對話；各 task 仍有明列依賴。C61 實機 bring-up 與 host 測試可在軟體基線完成後分開進行。不同 session 不可同時修改同一 worktree 的共用檔案。

封存文件中引用的 `tmp-briefs/` 與 `review-evidence/`（subagent 交接 brief、報告、review diff、測試 log）已從工作樹移除，內容仍在 git 歷史中（例如 `git show 4785a36:<原路徑>`）。有效的驗收證據是各 change 的 `evidence.md` 列出的 Rust 測試。

## 每個新 session 的入口

1. 讀本文件、指定 change 的 `proposal.md`／`spec.md`／`tasks.md`，及它列出的 reference。
2. 只接手依賴已完成的 task；使用目前整合後的程式碼，不從舊分支拼接實作。
3. 回報改動、實際驗證指令／結果和未驗的實機項目；只有交付條件已成立才勾選 task。
4. `/spec-apply <change-name>` 執行該 change；也可指定只完成某一個已就緒的 T 編號。不要把「編譯／mock 通過」記成實機通過。

## 固定的研究起點

分析時 checkout：Pulp `445755fe`、BSP `57fbdb5`、OnePage hardware `9d7977d`、CrossPoint `23dec9d`、smol-epub `832609d`。來源路徑以 repo 根目錄為基準：`../bsp_onepage_c61`、`../onepage-reader`、`../crosspoint-onepage`、`../smol-epub`。這些是 reference，不要求重設 checkout。

- 板子：ESP32-C61HR2／16 MB flash／2 MB PSRAM；初始 flash40 MHz、PSRAM40 MHz。GPIO27 是 EPD reset **兼 SD/MIC power**，SD 初始化後不能再掉電。
- BSP 是 ESP-IDF C，使用它的 pin map／操作順序，不引入 IDF runtime。BSP USB detect 文件與實作極性矛盾，以 schematic／實測定案；CrossPoint SDK submodule 目前未初始化。
- Pulp 原 X4 release build 已通過隔離驗證。C61 的 HAL／RTOS／PSRAM 最小程式，以及 Wi-Fi／DHCP／TCP／UDP 最小程式可連結；完整 Pulp 尚未移植。
- [Wi-Fi 支援與版本證據](../references/onepage-wifi-support.md)：原 radio0.17／RTOS0.2 沒有 C61 feature；目前採用 radio1.0.0-beta.1／RTOS0.4.0，upload 已遷移並歸檔，仍無實機驗證。
- 本地 stable compiler1.99.0 與 RISC-V core1.98.1 不一致。分析用重建 core／alloc 繞過；T1 必須建立正式可重現工具鏈，不能依賴此 workaround 或 `/tmp` 產物。
- `smol-epub` 現有 test suite 是 0 unit tests／4 ignored doctests，需新增具體功能驗收。path dependency 的 Git revision 不由 Cargo.lock 固定。
- 本地 `Iansui-Regular.ttf` 是未追蹤輸入；後續 agent 以 font-path 使用，不假設它會隨 clone 出現。
