# onepage-pre-hardware-hardening — Requirements

### R1: 文件中的 fontconv 指令可從乾淨 checkout 執行 `[derived]`
THE SYSTEM SHALL 在 `README.txt`、`onepage-cjk-iansui/install.md`、`hardware-acceptance.md` 中所有 `pulp-fontconv`（`bundle`、`pbm-to-png`）指令帶上 `--target host-tuple` 與 `--config 'unstable.build-std=["std","test"]'`。
IF 指令照文件原樣執行 THEN THE SYSTEM SHALL 成功（不是 243 個編譯錯誤）。

Scenario (R1): Given 乾淨 checkout When 照 `hardware-acceptance.md` G3 準備段執行 bundle 指令 Then 產生 `target/cjk-sd/_PULP/FONTS/`。

### R2: 字庫 cache 對 converter 變更失效 `[derived]`
WHEN `pulp-fontconv`／`pulp-fontpack` 的原始碼或其依賴版本改變 THE SYSTEM SHALL 產生不同的 cache key，不得重用舊產物（cache hit）。
WHEN manifest 與 rustc 版本不變且上述也不變 THE SYSTEM SHALL 仍 cache hit。

### R3: stack analyzer 缺資料不得假裝安全 `[derived]`
IF ELF 沒有 `.stack_sizes` THEN THE SYSTEM SHALL 以非零 exit 結束並回報 unknown，不得印出 worst stack、headroom 或餘裕百分比。

### R4: C61 log channel 固定為 USB-Serial-JTAG `[derived]`
WHERE board 為 `board-onepage-c61` THE SYSTEM SHALL 把 `esp-println` 設為 `jtag-serial`，不使用依 USB SOF 在執行期切換的 `auto`。
WHERE board 為 `board-x4` THE SYSTEM SHALL 維持原 `esp-println` 設定。

### R5: C61 booster soft-start 使用 BSP 值 `[derived]`
WHERE board 為 `board-onepage-c61` THE SYSTEM SHALL 在 init 送 `0x0C` 資料 `AE C7 C3 80 C0`。
WHERE board 為 `board-x4` THE SYSTEM SHALL 繼續送 `AE C7 C3 C0 80`，X4 golden wire trace 不變。
THE SYSTEM SHALL 在文件中把此項保持為 `UNVERIFIED`（A1）。

### R6: 完整韌體週期電池取樣有成功 log `[derived]`
WHEN 完整韌體的電池取樣成功（每 30 s 週期）THE SYSTEM SHALL 輸出一行含 cell mV 與百分比的 log。
IF 取樣失敗 THEN THE SYSTEM SHALL 維持現有 `battery: sample failed …`，且不得當作 0 mV。

### R7: boot 映像輸出 ladder mV `[assumed]`
WHEN boot 映像的前排 ladder 產生按鍵事件 THE SYSTEM SHALL 在 log 中附上該次取樣的 mV，使 A5a 不需外接儀器即可比對窗口。

### R8: 實機驗收關卡判定一致 `[derived]`
THE SYSTEM SHALL 在 `hardware-acceptance.md` 明訂 USB-only gate：G1→G2 的條件為每項皆為 `PASS`、`FAIL`，或附理由的 `DEFERRED`／`SKIPPED`；`UNVERIFIED` 不得帶入 G2。
THE SYSTEM SHALL 使 §0、§3、G1 通過條件、A6a／A6c 的備註使用同一組狀態詞，不互相矛盾。

### R9: A3d／A7c／A7d 與實際程式行為一致 `[derived]`
THE SYSTEM SHALL 把 A3d、A7c 各拆為 boot 映像（機制與 log）與完整韌體（B3 的 UI、B1 的閱讀位置）兩部分；boot 睡眠 demo 存的是 `SessionState::home()`，不得宣稱驗到閱讀位置。
THE SYSTEM SHALL 把 A7d 拆為「單槽損壞、另一槽有效 → 恢復該槽」與「雙槽皆損壞 → normal boot」。

### R10: A4d／P10 不得產生假 PASS `[derived]`
THE SYSTEM SHALL 把 A4d／P10 標示為延後，原因載明：BUSY 為 `Pull::None`、高電位為忙，懸空腳可能讀低而使刷新「成功」且無錯誤；改由另行設計的可控測試模式驗證。

### R11: 儀器對照與記錄模板 `[derived]`
THE SYSTEM SHALL 在 `hardware-acceptance.md` 加入儀器對照節，逐項列出萬用表、ESP32-S3 DevKitC-1／XIAO ESP32-S3 Plus、XIAO Debug Mate、XIAO Expansion Board 可驗與不可驗的項目，不可驗者預先登記為 `SKIPPED（原因）`。
THE SYSTEM SHALL 在該節載明：Debug Mate 數值只作相對比較、USB 供電下不是深睡電流、不得接到電池座、同時只能有一條供電路徑；S3 可作無 DHCP 的測試 AP（W2／W5C）。
THE SYSTEM SHALL 提供 `specs/hardware-records/` 模板，欄位含 `hardware-acceptance.md` §0 規則 2 要求的記錄頭部，且該目錄的 `*.md` 不得被 `.gitignore` 排除（resolved miss，見 proposal）。

### R12: 過時引用已更正 `[derived]`
THE SYSTEM SHALL 使 `Taskfile.yml` 結尾訊息指向 `specs/references/hardware-acceptance.md`。
THE SYSTEM SHALL 使 espflash 最低版本在 `bringup.md`、`hardware-acceptance.md` 一致為 4.4.0（建議 4.6.0）。
THE SYSTEM SHALL 使 B5a 的描述與 R6 的實作一致。
THE SYSTEM SHALL 使 A7a 睡眠序列列出的 log 字串與順序和程式碼一致（resolved miss，見 proposal）。
THE SYSTEM SHALL 在 `specs/changes/README.md` 說明 partial refresh（commit `4785a36`，無 change 資料夾）由 `hardware-acceptance.md` G6 涵蓋。

### R13: multipart 結尾驗證 `[assumed]`
WHEN upload 的 part 結尾 delimiter 後接 `--` THE SYSTEM SHALL 視為完成並儲存檔案。
IF delimiter 後不是 `--`（例如 `\r\n`，表示後面還有 part）或資料不足以判定 THEN THE SYSTEM SHALL 回錯誤，且不得記錄 `upload: file saved as`。

### R14: 不宣稱實機通過 `[human]`
THE SYSTEM SHALL 在本 change 所有產出中，把需要實機才能證明的項目維持 `UNVERIFIED`（或 `DEFERRED`／`SKIPPED` 並附原因）；編譯、link、host 測試通過不得記為實機通過。

### R15: 軟體基線不退步 `[derived]`
THE SYSTEM SHALL 使 `task acceptance` 在每個 task 完成後維持全綠；測試總數不得少於 1366 passed，ignored 不得多於 1（QEMU，Linux only）。
