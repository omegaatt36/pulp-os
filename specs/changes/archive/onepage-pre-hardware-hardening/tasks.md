# onepage-pre-hardware-hardening — Tasks

依序執行，每個 task 一個 fresh subagent；一次只有一個 task 修改工作樹。每個 task 結束須跑與其範圍相符的測試，T11 跑完整 `task acceptance`。測試預期值取自 spec 文字，不由實作輸出複製。不要勾選實機項目。

- [x] T1: 在 `README.txt`、`onepage-cjk-iansui/install.md`（bundle 與 `pbm-to-png`）、`hardware-acceptance.md` G3 準備段補 host target／build-std 參數，並實際執行文件版指令確認成功 — satisfies R1；無依賴。
- [x] T2: 讓 fontconv cache key 納入 converter／fontpack 原始碼與依賴識別；補「改變後不得 cache hit」與「不變仍 cache hit」測試 — satisfies R2；無依賴。
- [x] T3: stack analyzer 缺 `.stack_sizes` 時非零退出並回報 unknown，不印 headroom；補測試（有／無該 section 的 ELF fixture） — satisfies R3；無依賴。
- [x] T4: C61 的 `esp-println` 改 `jtag-serial`（`default-features = false`），X4 不動；用 harness 依賴圖／feature 檢查鎖定，並確認 C61、C61-wifi、C61-partial、X4 皆可 build — satisfies R4；無依賴。
- [x] T5: C61 booster 位元組改 `AE C7 C3 80 C0`、X4 保持原值；更新／新增 host 測試與 golden，確認 X4 golden trace 不變；文件保持 UNVERIFIED — satisfies R5, R14；無依賴；**不得**同時改其他 init 命令（`0x1A` 等另案）。
- [x] T6: 完整韌體 `poll_battery` 成功時輸出週期 log；boot 映像的 ladder 事件附 mV；補 host／harness 可驗的部分，其餘列為實機項 — satisfies R6, R7；無依賴（置於 T5 之後，避免同時改 kernel）。
- [x] T7: upload multipart 結尾驗證（`--` 才算完成；非最終 delimiter 或資料不足回錯誤、不記 saved）；先補負向 host 測試再修正；檢查既有 fixture 是否缺 `--` — satisfies R13；無依賴。
- [x] T8: `hardware-acceptance.md` 判定修正：USB-only gate 與統一狀態詞、A3d／A7c／A7d 拆分、A4d／P10 延後與原因 — satisfies R8, R9, R10, R14；依賴 T5, T6（引用其行為）。
- [x] T9: `hardware-acceptance.md` 新增儀器對照節（並把 T8 誤標在 A6b 的〔預設延後：需電池〕改為以儀器為依據的標記：A6b 的限制是缺時間量測儀器，不是電池）；建立 `specs/hardware-records/` 模板 — satisfies R11, R14；依賴 T8（同一檔案，序列修改）。
- [x] T10: 修正過時引用（Taskfile 訊息、espflash 版本、B5a 描述、A7a 睡眠序列首行（現寫 `session: saved to slot …`，睡眠路徑實際是 `sleep: session saved before power-off`）、A5a／A5b／A6a 與 §6「ladder mV 診斷」列（T6 之後已有 mV 輸出與 `key:` 行新格式、完整韌體週期 battery log）、`specs/changes/README.md` partial refresh 說明） — satisfies R12；依賴 T6, T8, T9。
- [x] T11: 跑完整 `task acceptance`，確認 R15；寫 `evidence.md`（每個 R 對應的測試路徑、重跑指令、red-proof 摘要、無法在主機驗證而仍 UNVERIFIED 的項目） — satisfies R1–R15；依賴 T1–T10。
