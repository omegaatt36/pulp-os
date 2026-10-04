# onepage-cjk-iansui — Tasks

先讀 proposal 及其前置 changes。字庫不是完整 Unicode 保證；樣本自行生成。smol 在 writable roots 外，T5 可在 repo 內管理 fork／patch 並固定 revision，不能只改 sibling 然後宣稱可重現。

- [ ] T1: 定義／驗收 SD pack 格式及 Unicode／offset／version／長度契約，覆蓋越64KiB／四位元組字元／壞檔 — satisfies R2, R3；無本 change 依賴。
- [ ] T2: 建立可配置現有 body／heading 字級的 Iansui host converter，輸出 coverage 與 license／provenance — satisfies R1, R4, R5, R16；依賴 T1。
- [ ] T3: 建立共用 pack reader／metrics lookup／missing-glyph 行為，驗收 converter roundtrip — satisfies R2, R3, R4, R5；依賴 T2。
- [ ] T4: 修正 Pulp UTF-8 decode／monospace／read-tail／title truncation，建立逐邊界回歸 — satisfies R12, R13；無本 change 依賴。
- [ ] T5: 修正並固定 smol 的 TOC truncation／非法 numeric entity 行為，驗收有效中文 streaming，記錄256-entry ZIP 限制 — satisfies R12, R13；無本 change 依賴。
- [ ] T6: 建立有界 page glyph preparation／cache 與錯誤路徑，證明 draw 零字庫 I/O — satisfies R6, R7, R8, R9；依賴 T3。
- [ ] T7: 接入正式 reader 的 metrics／glyph provider 和目前字級，實作橫排繁中禁則／中英換行 — satisfies R4, R5, R10, R11；依賴 T3, T4, T5, T6。
- [ ] T8: 接入 heading／TOC／書名／UI 的 CJK regular fallback，保持既有 Latin styles — satisfies R14；依賴 T6, T7。
- [ ] T9: 更新分頁 cache identity 與字體變更時的失效，驗收前後頁／書籤／resume — satisfies R10, R11, R15；依賴 T7。
- [ ] T10: 完成中文 host／strip snapshots、缺字／壞字庫／cache不足／SD失敗 matrix，重驗 firmware link，交付 SD 安裝指令 — satisfies R1–R16；依賴 T8, T9。
