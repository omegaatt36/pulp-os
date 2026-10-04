# onepage-wifi-upload — Tasks

延後 change；前置 C61 port 軟體驗收。依據 reference 重建 probe，勿依賴 `/tmp` 或既有分支。沒有機器可完成軟體候選版，實測證據另作 gate。

- [ ] T1: 固定 C61 radio set／optimization，建立 enabled／disabled build／link smoke，記錄版本與 memory 起點 — satisfies R1, R2, R3, R12；無本 change 依賴。
- [ ] T2: 遷移 station controller／credentials／interface API，建立有界 association／DHCP 與錯誤返回 — satisfies R4, R5；依賴 T1。
- [ ] T3: 整合 network runner／TCP／UDP 與既有 HTTP upload／list／delete，驗收檔案內容及 SD failure — satisfies R6, R7, R8；依賴 T2。
- [ ] T4: 恢復 mDNS 發現與 IP 顯示，驗收 UDP response — satisfies R9, R10, R11；依賴 T3。
- [ ] T5: 修正退出／失敗／再次進入的 runner、socket、interface／radio ownership，驗收 reader 返回 — satisfies R9, R10, R11；依賴 T2, T3, T4。
- [ ] T6: 完成 host service／timeout／storage-error 回歸及 firmware link／heap／stack 預算報告 — satisfies R1–R14；依賴 T5。
- [ ] T7: 交付 OnePage Wi-Fi 實機驗收流程與未驗記錄，覆蓋 association／DHCP／HTTP內容／mDNS／重入／功耗 — satisfies R13, R14；依賴 T6；此 task 交付流程，不假冒實測通過。
