# 實機驗收記錄（模板）

複製為 `specs/hardware-records/<YYYY-MM-DD>-<板號>.md` 後填寫；本檔不填。狀態詞定義見 `specs/references/hardware-acceptance.md` §0 規則 1（`UNVERIFIED`／`PASS`／`FAIL`／`SKIPPED`／`DEFERRED`）；沒有證據不得寫 `PASS`，編譯、link、host 測試通過不算實機通過。
初始狀態取自該文件第 5 節的〔預設延後〕／〔預設跳過〕標記（原因文字照抄），其餘為 `UNVERIFIED`。`UNVERIFIED` 不得帶入 G2（USB-only gate 見該文件第 3 節與 G1 通過條件）。儀器對照見該文件 5.5。

## 頭部（開始前必填）

| 欄位 | 內容 |
|---|---|
| 日期 | |
| 板號 | |
| 面板型號（D10） | |
| `git rev-parse HEAD` | |
| `git status --porcelain`（全文） | |
| 各映像 `shasum -a 256` | |
| `espflash --version` | |
| SD 卡廠牌／容量／格式 | |
| log 通道（A1／A1b） | |
| AP 型號 | |
| 儀器清單 | |

## 狀態表

| ID | 狀態 | 證據（log 摘錄／檔名／照片） | 備註 |
|---|---|---|---|
| A1 | `UNVERIFIED` | | |
| A1b | `UNVERIFIED` | | |
| A2 | `UNVERIFIED` | | |
| A2b | `UNVERIFIED` | | |
| A2c | `UNVERIFIED` | | |
| A3a | `UNVERIFIED` | | |
| A3b | `UNVERIFIED` | | |
| A3c | `UNVERIFIED` | | |
| A3d | `UNVERIFIED` | | |
| A3e | `UNVERIFIED` | | |
| A3f | `UNVERIFIED` | | |
| A4a | `UNVERIFIED` | | |
| A4b | `UNVERIFIED` | | |
| A4c | `UNVERIFIED` | | |
| A4d | `DEFERRED（BUSY 為 Pull::None（kernel/src/board_c61/epd.rs:85）、高電位為忙（is_busy() 為 is_high()，同檔 :70）；拔掉／懸空 BUSY（GPIO29）腳電平不確定，讀低時驅動視為不忙而立即返回，刷新「成功」且無錯誤，驗不到逾時路徑，會得到假 PASS。**不建議**以拔腳／懸空驗收。補驗條件：改由另行設計的可控測試模式（例如在測試映像強制 BUSY 逾時）驗證；本 change 不實作）` | | |
| A4e | `UNVERIFIED` | | |
| A5a | `UNVERIFIED` | | |
| A5b | `UNVERIFIED` | | |
| A5c | `UNVERIFIED` | | |
| A5d | `UNVERIFIED` | | |
| A6a | `DEFERRED（需電池／成品機，記錄檔狀態為 DEFERRED；無電池階段仍要記錄 GPIO5 讀值與 log、確認不 panic 且不當 0 V，電壓對照等電池／成品機）` | | |
| A6b | `SKIPPED（無示波器；用 S3 時間戳記錄器且接得到 GPIO10 時才可驗，見 5.5）` | | |
| A6c | `DEFERRED（需電池／成品機，記錄檔狀態為 DEFERRED；無電池階段只能驗「插著」的電平，仍要記錄 GPIO11 的讀值；「拔出」電平與事件需電池或成品機，見 0.5）` | | |
| A6d | `UNVERIFIED` | | |
| A7a | `UNVERIFIED` | | |
| A7b | `UNVERIFIED` | | |
| A7c-1 | `UNVERIFIED` | | |
| A7c-2 | `DEFERRED（需完整韌體（G2），在 G1 之前無法驗；補驗條件：B1、B4a 有結論）` | | |
| A7d-1 | `UNVERIFIED` | | |
| A7d-2 | `UNVERIFIED` | | |
| A7e | `DEFERRED（無電池、可調電源與 µA 級電流計；Debug Mate 量的是 5V 路徑，不是深睡電流，見 5.5。補驗條件：取得電池＋可調電源＋電流計，或成品機＋電池）` | | |
| A7f | `UNVERIFIED` | | |
| A7g | `UNVERIFIED` | | |
| A7h | `UNVERIFIED` | | |
| A7i | `UNVERIFIED` | | |
| B1 | `UNVERIFIED` | | |
| B2a | `UNVERIFIED` | | |
| B2b | `UNVERIFIED` | | |
| B2c | `UNVERIFIED` | | |
| B2d | `UNVERIFIED` | | |
| B2e | `UNVERIFIED` | | |
| B2f | `UNVERIFIED` | | |
| B2g | `UNVERIFIED` | | C61 前緣實體鍵由左到右為 Back／Left／Right／Enter；預設導覽列應為 `Back`、`<<`、`>>`、`Ok`。記錄目視與操作確認、照片或使用者回報，並註明按鍵交換設定是否另驗 |
| B2h | `UNVERIFIED` | | |
| B3 | `UNVERIFIED` | | |
| B4a | `UNVERIFIED` | | |
| B4b | `DEFERRED（同 A7e）` | | |
| B5a | `DEFERRED（需電池／成品機，記錄檔 DEFERRED；無電池階段只確認 30 s 取樣週期照跑、不 panic）` | | |
| B5b | `UNVERIFIED` | | |
| K1 | `UNVERIFIED` | | |
| K2 | `UNVERIFIED` | | |
| K3 | `UNVERIFIED` | | |
| K4 | `UNVERIFIED` | | |
| K5 | `UNVERIFIED` | | |
| K6 | `UNVERIFIED` | | |
| K7 | `UNVERIFIED` | | |
| K8 | `UNVERIFIED` | | |
| K9 | `UNVERIFIED` | | |
| K10 | `UNVERIFIED` | | |
| H1 | `UNVERIFIED` | | |
| W1 | `UNVERIFIED` | | |
| W2 | `UNVERIFIED` | | |
| W3 | `UNVERIFIED` | | |
| W4 | `UNVERIFIED` | | |
| W5 | `UNVERIFIED` | | |
| W6 | `DEFERRED（無電池；Debug Mate 的 5V 入口未確認。補驗條件：確認 5V 入口可接後，以 5.5 的相對比較驗 S0–S3；絕對電池功耗等電池／成品機）` | | |
| W7 | `UNVERIFIED` | | |
| W8 | `UNVERIFIED` | | |
| X1 | `UNVERIFIED` | | |
| X2 | `UNVERIFIED` | | |
| X3 | `UNVERIFIED` | | |
| P0 | `UNVERIFIED` | | |
| P1 | `UNVERIFIED` | | |
| P2 | `UNVERIFIED` | | |
| P3 | `UNVERIFIED` | | |
| P4 | `UNVERIFIED` | | |
| P5 | `UNVERIFIED` | | |
| P6 | `UNVERIFIED` | | |
| P7 | `UNVERIFIED` | | |
| P8 | `UNVERIFIED` | | |
| P9 | `UNVERIFIED` | | |
| P10 | `DEFERRED（原因同 A4d，BUSY 為 Pull::None、高電位為忙，懸空腳可能讀低而使刷新「成功」且無錯誤，會得到假 PASS，**不建議**拔腳／懸空。補驗條件：另行設計的可控測試模式（例如在測試映像強制 BUSY 逾時）；本 change 不實作）` | | |
