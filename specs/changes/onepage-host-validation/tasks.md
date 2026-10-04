# onepage-host-validation — Tasks

前置：C61 port 軟體候選版完成。接手單項前檢查依賴；保持共用演算法與 firmware 建置，不以另寫一份 pager 取代正式路徑。

- [ ] T1: 建立必要的 host build boundary 與條件化 font-generation／linker 行為，驗收兩個 firmware builds — satisfies R1, R2, R3；無本 change 依賴。
- [ ] T2: 將 UTF-8／paging 的必要正式邏輯接至 host 入口，保留既有英文行為 — satisfies R2, R3, R8；依賴 T1。
- [ ] T3: 建立 virtual storage 的 host-file／memory、read counter 與錯誤注入 — satisfies R4, R5；依賴 T1。
- [ ] T4: 將正式 strip／glyph rendering 接至 software framebuffer，輸出 portrait artifacts 與 strip equality 驗收 — satisfies R2, R3, R6, R7；依賴 T1。
- [ ] T5: 產生可分發的英文 TXT／EPUB2／3 fixtures，包含 STORED／DEFLATE、TOC、metadata／圖片 — satisfies R8；依賴 T3。
- [ ] T6: 建立 production-path 導航／設定／書籤／畫面回歸，涵蓋缺卡與 storage failure — satisfies R2, R3, R4, R5, R6, R7, R8；依賴 T2, T3, T4, T5。
- [ ] T7: 交付 host test／preview 的可重現指令與覆蓋報告，重驗 firmware builds — satisfies R1, R2, R3, R9, R10；依賴 T6。
