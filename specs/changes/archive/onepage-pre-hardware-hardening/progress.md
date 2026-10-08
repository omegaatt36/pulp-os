# onepage-pre-hardware-hardening — Progress ledger

每個 task 一個 fresh subagent（risk yellow，不拆 writer／implementer）；一次只有一個 task 修改工作樹。不 commit。

## T1 — 完成
- 改：README.txt:22、onepage-cjk-iansui/install.md:13,43、hardware-acceptance.md:244，皆補 `--target host-tuple --config 'unstable.build-std=["std","test"]'`。
- red-proof：原指令 exit 101，`E0463 can't find crate for 'std'`，243 個錯誤。
- 成功證據：文件版 bundle exit 0、`target/cjk-sd/_PULP/FONTS/` 重建（9 個 PFN＋PROV／OFL／COVERAGE／BUNDLE.JSON）；install.md 版 `pbm-to-png` exit 0。
- 未驗證：兩次 bundle 都是 cache hit，未驗冷快取（T2 之後 cache key 改變會自然覆蓋）。
- weakening gate：無測試檔改動。

## T2 — 完成
- 新增 `fontconv/build.rs`（sha256 雜湊 fontconv／fontpack 原始碼、兩 crate 與根 Cargo.toml、Cargo.lock、.cargo/config.toml，經 `PULP_FONTCONV_BUILD_ID` 嵌入）；`bundle.rs` 新增 `cache_identity`／`cache_key`，識別放 `build.id`；新增 `fontconv/tests/build_identity.rs`（10 測試）。
- 依賴識別用 Cargo.lock 整體雜湊（不漏傳遞依賴；代價是任何依賴升級會重建字庫一次）。BUILD_ID 只進 cache key 與 BUNDLE.JSON，不進字庫檔 bytes。
- red-proof：測試先寫，`cargo test-tools --test build_identity` 失敗（`couldn't find file fontconv/tests/../build.rs`，`BUILD_ID`／`cache_key` 不存在）。預期值全為 assert_ne!／assert_eq!，無硬編碼雜湊。
- 結果：`cargo test-tools` 350 passed／0 failed／0 ignored；bundle 第一次 built、第二次 cache hit；把 build.rs 納入輸入後 hit 變 built（實測原始碼改變 → key 改變）。
- weakening gate：既有測試檔 diff 為空。
- 順帶：README.txt cache key 敘述由「converter binary」改為「converter and fontpack source files (hashed at build time)」以與實作一致。

## T3 — 完成
- `harness/src/bin/stack_analyzer.rs`：缺 `.stack_sizes` → stderr `stack usage unknown: …`、exit 2、stdout 空；有 section 時行為不變。新增 `harness/tests/stack_analyzer.rs`（3 測試，手寫最小 ELF32 RISC-V fixture，無新依賴）。bin 名稱是 `stack-analyzer`。
- red-proof：無 section 的兩個測試先失敗（exit 0，印出 worst 0 B／headroom 51888 B／100.0% margin）；有 section 的測試通過，證明 fixture 有效。預期值取自 R3（非零 exit、stderr 含 unknown、stdout 不含 Worst／Headroom／margin／%）。
- 對真實 `target/accept/c61/.../pulp-os-c61` 手動驗證：exit 2、stdout 空。
- `cargo test-harness --locked` 全過（唯一 ignored 為既有 qemu）；fmt 通過；weakening gate：僅新增測試檔。
- 發現：目前 C61 映像**沒有** `.stack_sizes`，所以這支工具對現有映像只能回報 unknown。任何引用它的「stack 餘裕」數字都不能當作該工具的結果（本 change 不處理改用 `-Z emit-stack-sizes` 建置，列為後續）。

## T4 — 完成
- 根 `Cargo.toml`：共用表的 `esp-println` 刪除並併入兩個 target 表（共用表保留預設 feature 會讓 C61 關不掉 `auto`）。C61：`default-features = false, features = ["esp32c61","jtag-serial","log-04","colors","critical-section"]`（`colors`／`critical-section` 為原預設，明確補回，ANSI 輸出不變）；X4：`["esp32c3","log-04"]` 保留預設。`Cargo.lock` 未變。新增 `harness/tests/esp_println_features.rs`（C61 只用 jtag-serial；X4 維持原設定）。
- red-proof：`c61_console_is_jtag_serial_only` 先失敗（列出 auto／default 等 feature）。X4 守護測試本來就過（性質為「維持不變」，無 red 可證）。
- X4 `cargo tree -e features -i esp-println` 改前後 diff 為空；C61 差異僅 auto／default 消失、jtag-serial 出現。五個映像（x4、x4-wifi、c61、c61-wifi、c61-partial）build 皆成功。
- ELF 證據：nm 因 inline 看不出，改用反組譯。舊 C61 `Printer::write_bytes` 0x14c B（讀 INT_RAW 的 SOF 位元＋ROM UART 分支）→ 新 0xb6 B（只剩 FIFO 狀態檢查，無 SOF 讀取、無 ROM UART 呼叫）。
- `cargo test-harness --locked` 全過（ignored 僅既有 qemu）；fmt 通過；weakening gate：既有測試檔無改動。
- 仍 UNVERIFIED：冷開機無 USB 時 log 的實際去向（A1b）與 jtag printer 在無主機下不卡死的實際表現。

## T5 — 完成
- `board-logic/src/ssd1677.rs`：`configure(b, booster: &[u8;5])` 改由呼叫端傳參；新增 `X4_BOOSTER = AE C7 C3 C0 80`、`C61_BOOSTER = AE C7 C3 80 C0`（附出處註解）。X4 走 `init_display`、C61 走 `Epd::init_inner`；全 repo 只有這裡送 `0x0C`，partial refresh 不送 booster。只改 0x0C 位元組順序，未動 0x1A 等其他命令。
- red-proof：`r5_c61_init_booster_is_otp_order` 失敗（left `[174,199,195,192,128]` vs right `[174,199,195,128,192]`），既有 `r10_epd_init_trace_…` 同樣失敗；新增的 X4 測試本來就過（性質為維持不變）。預期值取自 R5 文字。
- X4 golden：`git diff harness/tests/goldens` 空；`x4_driver_trace` 通過。`cargo test-board-logic` 332 passed（原 330＋2）；`cargo test-harness --locked` 全過；五個映像 build 成功；fmt 通過。
- weakening gate：唯一改動的既有斷言是 `r10_epd_init_trace_is_bounded_wait_soft_reset_wait_configure`（走 C61 的 `Epd::init`），預期由 `init_trace_expected()[2..]`（X4 值）改為 `c61_init_trace_expected()[2..]`（僅 booster 資料不同），仍整段序列逐項比對，強度未減。無新增 skip／ignore。
- 文件：hardware-acceptance.md S4 (1) 改為「C61 已改用 OTP 值，仍 UNVERIFIED，待實機」；(2) `0x1A [5A]` 不變。
- 仍 UNVERIFIED：BSP 的值是否適用於使用者面板（D10）及實機畫面（S4）。

## T6 — 完成
- R7：`board-logic/src/keys.rs` 新增 `KeyInput::last_front_mv()`（`KeyScanner` 每次 `sample()` 記下該次 ADC 平均 mV；寬限期／側鍵按下／ADC 失敗回 `None`；排隊的第二個事件沿用同一次取樣）；`src/bin/c61_boot.rs` 的 `key:` 行改為 `key: .. -> .. (ladder_mv=Some(2592))`，只在事件時印。完整韌體不加（假設 A2）。+5 個 `r7_*` 測試。
- R6：`scheduler_c61.rs` `poll_battery` 成功時印 `battery: cell {} mV, {}% (periodic)`。**與 brief 的「同一格式」有出入**：加了 ` (periodic)` 後綴，因為 `log!` 會把格式字串拆成片段，沒有後綴就無法用 ELF 字串區分週期那行是否連進映像；前綴相同，以前綴比對的 parser 兩處都吃得到。新增 `harness/tests/elf_c61_battery_log.rs`（2 測試）。
- 證據等級：R6 是 **link-level**（`% (periodic)` 字串存在於 c61、c61-wifi 的 .rodata，boot 映像沒有）；週期每 30 s 真的出現須實機。
- red-proof：R7 用回傳 None 的 stub，4 個測試失敗（如 `left: Some((Press(Back), None))`）；R6 ELF 測試失敗（`no '% (periodic)' literal in the image`）。預期值由需求推導：事件附帶的 mV＝造成該事件那次輪詢的值（Press：ENTER 窗口內抖動序列第四筆 31 mV；Release：靜止值 3099 mV）。過程中一個自寫測試先寫錯（先把 ladder 設 0 才過寬限期），已修測試、實作未動。
- 結果：`cargo test-board-logic` 337 passed（原 332）；五映像 build 成功；`cargo test-harness --locked` 全過（ignored 僅 qemu）；fmt 通過。
- weakening gate：無移除／放寬／skip；僅 `sample()` 最後一行等價改寫。
- 因此過時、交 T8／T10 處理的文件敘述：hardware-acceptance.md A5a（「沒有 mV 診斷輸出」）、A5b（`key:` 行新格式）、A6a（完整韌體也有週期 log）、B5a、§6 的「ladder mV」列。

## T7 — 完成
- `src/apps/upload/http.rs` `handle_upload`：找到 end_marker 後先 flush `work[..pos]` 並 `copy_within` 把 marker 搬到緩衝開頭（緩衝再滿也有位置容納其後 2 bytes，結果不依賴 read 切法）；`filled >= em_len+2` 時判定：`--` → `upload: complete`＋Ok，否則 `Err("malformed multipart end")`；不足 2 bytes 繼續 read，EOF → `Err("upload incomplete")`，且 marker bytes 不寫入檔案。`upload: file saved as` 只在 `src/apps/upload/mod.rs:261` 的 `ServerEvent::Uploaded` 才記；錯誤路徑走 `send_error_response`（4xx）＋`UploadFailed`。
- 新增 4 個測試於 `host/tests/upload_http.rs`（＋FakeSocket `splits` 欄位與 `Scenario::split_at`）：(a) delimiter 後 `\r\n`／其他 2 bytes／第二個 part → 非 200 且無 Uploaded event；(b) 0 或 1 byte 後 EOF／讀取失敗 → 同上；(c) 最終 `--`（含裸 `--` 不多讀、epilogue、逐 byte、緩衝邊界大小）→ 成功；(d) `--` 前／兩個 `-` 之間／`--` 後／CRLF 中間切 read → 成功。
- red-proof：(a)(b) 實作前失敗（`failure response contains "200 OK"`）；(c)(d) 本來就過（迴歸保護，無 red 可證）。預期值取自 R13 文字。
- 既有 fixture：`upload_http.rs:379`、`upload_regression.rs:393` 的 `multipart_body` 皆已寫 `--\r\n`，無缺漏，R13 假設無衝突。
- 結果：host 559 → 563 passed／0 failed；board-logic、reader-regression、harness 全過，五映像 build 成功；fmt 通過。weakening gate：測試 diff 僅新增（+159／−0）。
- **已知行為（不在 R13 範圍，未處理）**：R13 錯誤路徑（malformed、EOF 不足）及既有 "upload incomplete" 路徑，SD 上都會留下已寫入的檔案（前者為完整內容但被判失敗，後者為被截斷）；`write_file(空檔)` 與 marker 前內容已先寫入。影響只在 SD 內容，不影響 `saved` log。建議另案決定是否清除殘檔。
- 過程提醒：subagent 為跑基準用過 `git stash`／`pop`；已核對 stash 為空、T1–T6 的所有改動標記仍在。`tasks.md` 因此由 `AM` 變成 `A `（內容完好）。

## T8 — 完成（純文件）
- 只改 `specs/references/hardware-acceptance.md`：§0 規則 1／2／3、§0.5 無電池列與建議順序、§3 G2 列、§4 D3、G1 表 A3d／A4d／A6a／A6b／A6c／A7c／A7d、G1 通過條件、B5a、G6 P10 與通過條件、§6 新增「BUSY 逾時的可控注入」列。
- 狀態詞：`UNVERIFIED`／`PASS`／`FAIL`／`SKIPPED`（附原因，不影響後續關卡）／`DEFERRED`（附原因與補驗條件）。本檔狀態欄永遠 `UNVERIFIED`；已知必然延後者在「做法／判定」欄標〔預設延後：…〕／〔預設跳過：…〕，記錄檔據此初始化。USB-only gate：G1→G2 要求 A1–A7（含新 ID）每項 PASS／FAIL／附理由 SKIPPED／附理由與補驗條件 DEFERRED，不得有 UNVERIFIED；FAIL 須附處置。
- 拆分：A3d（boot 只驗 `storage error: …` log，UI 歸 B3）；A7c-1（boot：`sleep: session saved before power-off`、`boot: wake cause`、`boot plan: RESTORE position`、`session[boot]: restore slot …`，存的是 `SessionState::home()`）／A7c-2（閱讀位置歸 B1、B4a，預設延後）；A7d-1（兩槽先皆有效，各毀較新／較舊槽一次 → `session[boot]: restore slot <未損壞槽>`）／A7d-2（兩槽皆毀 → `boot plan: normal boot`、`session[boot]: normal boot (Corrupt(…))`）。log 字串皆經 grep 對照 `src/bin/c61_boot.rs`、`kernel/src/board_c61/sleep.rs`。
- A4d／P10 標〔預設延後〕：`epd.rs:85` `Pull::None`、`epd.rs:70` `is_high()` 為忙，懸空讀低會被當不忙而「成功」，產生假 PASS；拔腳／懸空改為「不建議」，補驗條件為另行設計的可控測試模式（本 change 不實作）；G6 通過條件允許 P10 為 DEFERRED。
- 主持人裁決：①A7a 首行不準（現寫 `session: saved to slot …`，該字串只出現在開機無有效 session 時，睡眠路徑實際為 `sleep: session saved before power-off`）→ 併入 T10；②subagent 為與 §0.5 一致，給 A6b 加了〔預設延後：需電池〕，但 A6b 的限制是時間量測儀器而非電池 → 交 T9 改為儀器依據；③A7c-2 標預設延後接受（B1／B4a 屬 G2，否則與 gate 循環）。
- 驗證：狀態詞 grep 無「維持 UNVERIFIED 又不得帶入 G2」矛盾；狀態欄全為 `UNVERIFIED`（R14）；T1、T5 的修改仍在。

## T9 — 完成（純文件）
- `hardware-acceptance.md` 新增 **5.5 儀器對照**（第 5 節後、第 6 節前）：器材與測試點說明、逐項表（項目／儀器／前提／不可驗處置）、Debug Mate 與 S3 注意事項。覆蓋 A3b、A4c／B2e／K8、A4d／P10、A5a、A6a／A6b／A6c、A7e／B4b、A7f、P2、W1(d)／W2／W5C、W6（A7f、B2e／K8 為 subagent 額外發現）。測試點經重新 grep BOM：只有 TP1、TP3（VBACKUP），GPIO4／5／10／11／27／29 皆無。
- 預設標記：A6b `SKIPPED`（無示波器；S3 時間戳記錄器且接得到 GPIO10 才可驗）；A7e、B4b、W6 `DEFERRED`（無電池／可調電源／電流計；W6 另受 5V 入口未確認限制）；A6a、A6c、B5a、A4d、P10、A7c-2 沿用 `DEFERRED`。條件式：A5a 接不到 GPIO4 → 記錄檔 `SKIPPED`；A7f 接不到 GPIO27 → `DEFERRED`（不許 SKIPPED，因 T11 必查）；無 S3 測試 AP 時 W1(d)／W2／W5C → `DEFERRED`。
- A6b 標記更正為儀器依據；§0.5「無電池」列與建議順序改為 A6b 不依賴電池。§0 規則 2 改為複製 `TEMPLATE.md`；順手把規則 4、第 5 節開頭、A3b／P2 做法、§6 BUSY 示波器敘述指向 5.5。
- 新增 `specs/hardware-records/TEMPLATE.md`：頭部欄位含規則 2 全部項目＋日期／板號／面板型號（D10）／儀器清單；狀態表 82 個 ID（腳本自第 5 節抽出，`diff` 為空），初始 72 個 `UNVERIFIED`、9 個 `DEFERRED`、1 個 `SKIPPED`，無任何 `PASS`。hardware-acceptance.md 的 82 個狀態欄仍全為 `UNVERIFIED`（R14）。
- 主持人處理：`.gitignore` 的 `*.md` 會讓 `specs/hardware-records/` 不進版控（模板形同未交付）→ 已加 `!specs/hardware-records/**/*.md`，`git check-ignore` 確認不再被忽略。
- 仍未決（使用者）：D8（電流量測儀器、量程、量測點）；OnePage 是否有 USB-C 以外的 5V 入口（Debug Mate 能否用於 W6 取決於此）。

## T10 — 完成（純文件／設定文字）
- `Taskfile.yml:76` 指向 `specs/references/hardware-acceptance.md`；全 repo 已無舊路徑。
- espflash：`bringup.md:12`、`bringup-record.md:14`、`onepage-wifi-upload/hardware-acceptance.md:35`（後兩者為主持人補改）一致為「最低 4.4.0（建議 4.6.0；4.3.0 不認得 esp32c61）」；hardware-acceptance.md §1、§2.2 刪除「archive 與 config.toml 寫 ≥4.6.0 已過時」（`.cargo/config.toml` 本來就是 4.4.0，未改）。其他 archive 的 progress／baseline 屬歷史敘述，保留。
- B5a／A6a：完整韌體每 30 s `battery: cell <mV> mV, <N>% (periodic)`（失敗為 `battery: sample failed: …`；開機第一次 `battery: cell <mV> mV, <N>%`）；註明週期 log 在主機端只有 link-level 證據。A5a／A5b：boot 映像 `key:` 行附 `(ladder_mv=<Some(mV)|None>)`，萬用表降為選配；§6 ladder 列標「已補」（mV 準確度仍需實機）。
- **A7a 更正（核對程式碼後）**：`session: saved to slot …` 不在睡眠序列內（只在開機無有效 session，`c61_boot.rs:406`）；`sleep: session saved before power-off` 排在 `shared lines driven low` **之後**而非第一行；睡眠序列第 1 步存 session 本身不印 log。更正為 `GPIO2 wake armed` → `epd parked` → `sd flushed and closed` → `shared lines driven low` → `session saved before power-off` → `entering deep sleep, wake = GPIO2 …`；`DEMO_IDLE_SLEEP_SECS` 行號 `:95` → `:96`。A7c-1 引用的 `:387` → `:388`（主持人補）。
- `specs/changes/README.md`：表內新增本 change 一列（進行中）；表下說明 partial refresh（`4785a36`）無 change 資料夾，證據與 P0–P10 見 hardware-acceptance.md G6，預設映像不含。
- 驗證：`task --list` 正常；Taskfile 舊路徑 grep 無結果；hardware-acceptance.md 狀態欄 PASS 數 0；未動任何 .rs／Cargo.toml。

## T11 — 完成
- `task acceptance`：exit 0、**1394 passed／0 failed／1 ignored**（基準 1366／0／1；+28＝T2 10、T3 3、T4 2、T5 2、T6 7、T7 4）、wall 2:38；唯一 ignored 為 `qemu_riscv32_executes_target_binary`（需 Linux qemu-user）。五映像 build、fmt、harness 皆過；`git diff harness/tests/goldens` 為空。
- 寫入 `evidence.md`（R1–R15 逐項：路徑、重跑指令、red-proof、預期值來源、證據等級；weakening gate 總結；仍 UNVERIFIED 清單；已知限制）。
- 主持人補：把 M1–M6（resolved misses）寫回 proposal.md，R11／R12 補入 spec.md，README 該列狀態改為「實作完成，待 /spec-archive」。
