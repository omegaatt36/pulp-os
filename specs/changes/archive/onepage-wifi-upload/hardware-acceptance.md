# OnePage Wi-Fi upload 實機驗收程序與未驗記錄（T7；R13／R14）

**狀態：沒有任何一項在實機上執行過；第 3 節所有項目都是 `UNVERIFIED`。** 本文件只交付程序與追蹤表，不含任何實測結果。

- 「預期觀察」一律來自 requirement（`spec.md`）、本 repo 程式碼或固定版本的上游原始碼，每條附來源；**不是**實測。來源查不到的寫「未知／需確認」。
- 實測時複製本檔成 `hardware-record-<YYYY-MM-DD>-<板號>.md` 填寫（沿用 `specs/changes/archive/onepage-c61-port/bringup-record.md` 的作法），本檔保持全 `UNVERIFIED`。沒有證據不得把狀態改成 `PASS`。
- 數值門檻：repo 沒有電流預算，也沒有 heap／stack 的實測門檻。標示「自訂」的 N、秒數、K 數是本文件為了讓判定可執行而訂的，**不是 spec 數值**，驗收者可調整，但要在記錄中寫明用了什麼。
- 本文件撰寫時沒有重跑任何 build 或測試；引用的軟體證據取自 `progress.md`／`budget-report.md`。

## 0. R13 軟體證據（已交付，供對照；本文件未重跑）

| 證據 | 指令 | 最近一次記錄的結果（來源） |
|---|---|---|
| Wi-Fi enabled link（C61＋X4）、radio 版本集、radio opt-level 3、IPv4-only smoltcp、C61 memory 預算 | `scripts/check-wifi-build.sh` | exit 0（`progress.md` T6：`smoltcp lacks feature proto-ipv6` ok；statics 204,664 B、stack 51,960 B、餘裕 2,808 B） |
| Wi-Fi disabled link（離線版不連入 radio） | `scripts/check-offline-boundary.sh` | 0 FAIL（`progress.md` T6；離線 statics 180,940 B／stack 75,684 B 不變） |
| host service 測試 | `scripts/host-test.sh --locked`；單項如 `scripts/host-test.sh --test upload_http`（另有 `upload_connect`、`upload_mdns`、`upload_session`、`upload_regression`） | T6 後 `upload_regression` 15、`upload_connect` 9、`upload_session` 20、`upload_http` 24、`upload_mdns` 58 通過；host 全套 874（implementer 自報） |
| 一鍵軟體驗收（含上列 check-wifi-build stage） | `scripts/run-software-acceptance.sh` | 結尾列出 UNVERIFIED 清單（與第 3 節對照見 3.1） |

這些只證明「可編譯、可連結、host 上的服務邏輯正確」。association、DHCP、真實 HTTP client、mDNS、重入、電流都沒有實機證據（R14）。

## 1. 前置條件與設備

### 1.1 先決條件

1. C61 離線韌體的基礎 bring-up（boot、log、SD 掛載、EPD、按鍵）本身仍是 `UNVERIFIED`（`scripts/run-software-acceptance.sh` 結尾清單；程序見 `specs/changes/archive/onepage-c61-port/bringup.md`）。Wi-Fi 驗收疊在這些之上：**boot／log／SD／EPD／BACK 與 ENTER 鍵沒有結論前，Wi-Fi 的任何失敗都無法歸因。**
2. 被測韌體是 wifi 建置（Home 選單才有 `Upload` 項，`src/apps/home.rs:217`；離線建置沒有）。

### 1.2 建置與燒錄（以 repo 內實際 alias 為準）

```
cargo build-c61-wifi --locked
cargo run-c61-wifi
```

- `cargo run-c61-wifi` = `cargo run --release --target riscv32imac-unknown-none-elf --features board-onepage-c61,wifi --bin pulp-os-c61`（`.cargo/config.toml`）；runner 為 `espflash flash --monitor --chip esp32c61 --flash-mode dio --flash-freq 40mhz --flash-size 16mb`，需 espflash >= 4.4.0（建議 4.6.0；4.3.0 不認得 esp32c61）。
- 映像（預設 target 目錄）：`target/riscv32imac-unknown-none-elf/release/pulp-os-c61`。記錄 `shasum -a 256` 與 `espflash --version`。
- 離線對照（第 W8 節比較 heap 大小用）：`cargo run-c61`。
- 「韌體 commit」欄的限制：T1–T6 的改動目前**全部未 commit**（`git status` 起點含 `?? src/apps/upload/*.rs` 等未追蹤檔）。只記 `git rev-parse HEAD` 不足以重現被測韌體。每次驗收記錄：`git rev-parse HEAD`、`git status --porcelain` 完整輸出、映像 sha256。建議先由使用者決定是否 commit 再驗收。

### 1.3 憑證怎麼提供（來自程式碼）

- 位置：SD 卡上的 `_PULP/SETTINGS.TXT`（`kernel/src/drivers/dir_entry.rs:10` `PULP_DIR`；`kernel/src/kernel/config.rs:6` `SETTINGS_FILE`；`src/apps/settings.rs:116` 讀取）。**Settings 畫面沒有 wifi 欄位的編輯 UI**（`src/apps/settings.rs` 只載入／寫回），只能在 PC 上編輯該檔。
- 格式（`kernel/src/kernel/config.rs` `parse_settings_txt`／`apply_setting`）：

  ```
  wifi_ssid=<SSID>
  wifi_pass=<密碼>
  ```

  規則：以 `\n` 分行；每行前後的空白、tab、CR 會被修剪；`#` 開頭的行是註解；以第一個 `=` 切 key／value（密碼中可含 `=`，host 測試有 `p@ss=word` 案例）；value 前後空白被修剪，所以**開頭或結尾是空白的 SSID／密碼無法表達**。
- 只讀檔案前 512 bytes（`src/apps/settings.rs:110` `buf = [0u8; 512]`）：憑證行要放在檔案前 512 bytes 內。
- 長度上限在載入時就截斷：SSID 32 bytes、密碼 63 bytes（`config.rs` `WIFI_SSID_CAP`／`WIFI_PASS_CAP`、`set_ssid`／`set_pass`）。因此 `check_credentials` 的「SSID > 32」與「密碼 > 63」分支**無法由 SETTINGS.TXT 觸發**；超長密碼會被靜默截成 63 bytes，之後以錯誤密碼去連線。
- `check_credentials` 的規則（`src/apps/upload/connect.rs:59-67`）：
  - SSID 空 → `MissingCredentials`；
  - SSID > 32 bytes 或密碼長度不在 8..=63 → `InvalidCredentials`（所以**空密碼／開放網路也被拒**，因為 0 不在 8..=63）；
  - 認證方式固定 `Wpa2Personal`（`src/apps/upload/mod.rs:327-333`）。
- 憑證只在開機與插卡時載入（`AppManager::storage_changed` → `load_eager_settings`，`src/apps/manager.rs:207-218`）。改完檔案後要**重開機或重新插卡**；拔卡後設定會被重載成空（見 W3 對「SD 未掛載」的說明）。Settings 畫面存檔時會把目前 RAM 內的 wifi 憑證原樣寫回（`write_settings_txt`）。

### 1.4 AP／路由器條件

- 2.4 GHz、WPA2-PSK。程式碼只設定 `Wpa2Personal`；其他認證方式（開放、WEP、WPA3-only）不在本驗收範圍。5 GHz 能力：esp-radio 預設 band mode 取決於 `wifi_has_5g` cfg（`esp-radio-1.0.0-beta.1/src/wifi/mod.rs:325-345`），C61 是否有 5 GHz：**未確認**，請用 2.4 GHz SSID。
- DHCP 伺服器啟用，會發 IPv4（本韌體 smoltcp 無 `proto-ipv6`，`scripts/check-wifi-build.sh` 守門）。
- AP 的 client isolation 關閉；PC 與裝置在同一個 L2 子網路（HTTP 用 IP 連線，mDNS 是 link-local multicast）。
- 記錄 AP 的 DTIM、IGMP snooping／multicast 設定、頻道。esp-radio 預設 country 為 `CN`（`esp-radio-1.0.0-beta.1/src/wifi/mod.rs:2568`，repo 未覆寫）；頻道 12–13 的行為未驗，建議用 1–11。
- 為了 W2 的 DHCP 逾時與 W5 的失敗路徑，需要能（a）關掉 AP 或把它移出範圍，（b）建立「可連上 AP 但沒有 DHCP 回應」的網路（例如另一台可關 DHCP 的路由器或手機熱點；**repo 沒有提供此設備，需自備**）。

### 1.5 量測與 PC 端工具

| 項目 | 內容 |
|---|---|
| log | `ESP_LOG=info`（`.cargo/config.toml` `[env]`），`esp-println` features 為 `auto`、`log-04`、`colors`（`cargo tree --locked -e features -i esp-println` 查得）。**沒有 `timestamp` feature → log 行沒有時間戳**；有 `colors` → 行首尾可能帶 ANSI 色碼，grep 時用 `grep -a` 並容許色碼。需要時間的步驟用碼表／錄影，或自備能替終端輸出加時間戳的方式（espflash monitor 在管線下是否可用：未驗）。log channel（USB-Serial-JTAG 或 UART）：未驗，見 archive bringup A1b |
| 電流 | **repo 沒有指定電流量測儀器、量程、取樣率或量測點。** archive `bringup.md` 只列「電流計（deep sleep µA 等級）、可調／可量電壓的電源或電池」。Wi-Fi 的 mA 級與突發電流需要什麼儀器：未知，需使用者決定。供電方式與 USB log 的衝突見 W6 |
| PC：HTTP | `curl`、`shasum`（或 `sha256sum`）、`cmp`、`nc`；macOS／Linux 皆有。可選 `jq` |
| PC：mDNS（macOS） | `dns-sd`、`ping`、`dscacheutil` |
| PC：mDNS（Linux） | `avahi-resolve`（`avahi-utils`）、`getent`（需 nss-mdns） |
| PC：封包 | `tcpdump` 或 Wireshark（repo 未提供，需自備；用來看 IGMP 與 UDP 5353） |
| SD | 一張 FAT 卡、能在 PC 掛載的讀卡機（W3 用 sha256 驗證寫入內容） |
| 瀏覽器 | 任一桌面／手機瀏覽器（W3f 檢查 `assets/upload.html`） |

## 2. 驗收項目

每節固定欄位：目的、步驟、預期觀察、判定準則、要記錄的數據。log 與畫面字串的來源行號集中在第 5 節。

通用事項：
- 進入 upload：Home 選單最後一項 `Upload` → Select。BACK（短按或長按都算）結束（`src/apps/upload/mod.rs:365-368`）。
- HTTP 伺服器是**單 socket、逐一處理**（`serve_http` 迴圈，`mod.rs:215-240`）：所有 `curl` 一律**依序**執行，不要並行；瀏覽器開多條連線也可能互相卡住（`assets/upload.html` 末尾註解）。
- 把完整的監看 log 存檔，每個項目記錄對應的 log 抄錄。

### W1 Association

**目的**：R4（用有效 station credentials 進入連線）、R5（credentials 缺失／不合法、association 失敗要顯示可退出的錯誤）、R14。

**步驟**
1. 放好有效憑證（1.3），冷開機，Home → Upload。
2. 觀察螢幕與 log，用碼表記錄「按下 Select」到 log 出現 `upload: connected to '<ssid>'` 的秒數（重複 3 次）。
3. 負向（每項之後按 BACK，確認回到可操作的 Home）：
   - (a) SETTINGS.TXT 沒有 `wifi_ssid`（或值為空）。
   - (b) 密碼只有 7 個字元。
   - (c) 密碼錯誤（長度合法）。
   - (d) AP 關閉或移出範圍。
   - (e)（選做）把密碼寫成 70 個字元：預期被截成 63 bytes 後以錯誤密碼連線（見 1.3），歸入 (c) 的結果類別。
4. 負向期間按 BACK 的路徑（Associating 中途）併入 W5。

**預期觀察（來源）**
- 正向螢幕：先顯示標題 `Upload`、內文 `Connecting to '<ssid>'...`（無 footer；`mod.rs:93`、`render_screen` 以 full refresh）。連上後進入 DHCP（見 W2）。
- 正向 log 順序：`upload: wifi initialised, connecting to '<ssid>'` → `upload: connected to '<ssid>', waiting for DHCP`（`mod.rs:140`、`:148`）。
- 期限：association 階段 20 s（`connect.rs:9` `ASSOCIATE_TIMEOUT`），實際以 `at_least` 加 1 tick（`connect.rs:72-80`），所以**耗時下限也不會早於 20 s**。期限只涵蓋 `connect_async`；`acquire`（含同步的 `WifiController::new`）在計時前執行且沒有期限。radio init 的耗時：未知，需記錄。
- 錯誤畫面（每個錯誤都是標題 `Upload` + 下列內文 + footer `Press BACK to exit`；`connect.rs:38-56`、`mod.rs:345-358`）：

  | 情境 | 螢幕內文 | `ConnectError`（log 的 Debug 名稱） |
  |---|---|---|
  | (a) | `No WiFi credentials!` / `Set wifi_ssid in` / `_PULP/SETTINGS.TXT` | `MissingCredentials` |
  | (b) | `WiFi config error!` / `SSID: 1-32 bytes` / `Password: 8-63 bytes` | `InvalidCredentials` |
  | (c)、(d) 驅動回報斷線 | `Connection failed!` / `Check SSID and password` | `AssociationFailed` |
  | (c)、(d) 20 s 內沒有結果 | `Connection timed out!` / `Router not responding` | `AssociationTimeout` |

  (c)／(d) 會落在 `AssociationFailed` 還是 `AssociationTimeout`：取決於驅動是否在 20 s 內送出 `StationDisconnected` 事件（`esp-radio` `connect_async`，`mod.rs:3337-3357`），**程式碼無法決定，需實測並記錄是哪一個**。`ConnectionError` 的 Debug 內容（`upload: connect failed: {:?}`，`mod.rs:145`）：未知，需記錄。
- 失敗時 log：`upload: failed: <Debug 名稱>, WiFi released`（`mod.rs:209`）。(a)(b) 在 `acquire` 之前就返回，不會出現 `upload: wifi initialised`。

**判定準則**
- PASS：正向在 20 s 內出現 `connected` log；(a)–(d) 各自出現上表對應的錯誤畫面，且按 BACK 後回到 Home 並可操作（按任一導覽鍵選單會動）。(c)(d) 的 timeout 類結果，錯誤畫面出現時間應 >= 20 s（碼表誤差自行註明）。
- FAIL：正向連不上、任何負向沒有出現錯誤畫面或無法 BACK 退出、裝置 panic／重啟、錯誤畫面的文字與上表不符。

**記錄欄位**：AP 型號與韌體版本、頻道、安全模式、SSID／密碼長度；3 次的連線秒數；每個負向情境實際出現的畫面文字與 log 抄錄；(c)(d) 落在 Failed 還是 Timeout；`connect failed:` 的 Debug 內容；`WifiController::new` 到 `connecting` 的耗時（若能量到）。

### W2 DHCP（取得 IPv4）

**目的**：R4（設定期限內取得可用的 IPv4）、R5（DHCP 逾時要顯示可退出的錯誤）、R14。

**步驟**
1. 接續 W1 正向：觀察從 `connected` 到 `serving` 的秒數與螢幕。
2. 取得螢幕括號中的 IP，與路由器的 DHCP lease／client 清單比對；PC 上 `ping -c 3 <IP>`。
3. 負向：用「可連上 AP 但沒有 DHCP 回應」的網路，觀察逾時與錯誤畫面。
4. 在 DHCP 等待期間按 BACK（可用步驟 3 的網路穩定重現）。

**預期觀察（來源）**
- DHCP 階段期限 15 s（`connect.rs:10` `DHCP_TIMEOUT`，同樣加 1 tick）；從 association 完成後才開始計時。association＋DHCP 最長約 35 s（另加 radio init 與畫面刷新時間，未量）。
- 只有取得 IPv4 才結束此階段：每 100 ms 輪詢 `stack.config_v4()`，為 `None` 時繼續等（`mod.rs:56` `DHCP_POLL_MS`、`:153-166`），不會以 `0.0.0.0` 進入 serving。
- 成功：log `upload: serving at http://pulp.local/  (a.b.c.d)`（URL 與 IP 之間兩個空格，`mod.rs:171`）；螢幕兩行 `http://pulp.local/`、`(a.b.c.d)`，footer `Press BACK to exit`（`mod.rs:179-180`）。IP label 無前導零（`mdns.rs` `ip_label`）。
- 逾時：螢幕 `No IP address!` / `DHCP timed out`；log `upload: failed: DhcpTimeout, WiFi released`。
- DHCP 階段 BACK：log `upload: user exited (Dhcp), WiFi released`，回 Home。

**判定準則**
- PASS：正向在 15 s 內顯示 IPv4，位址與路由器 lease 一致、不是 `0.0.0.0` 或 `169.254.x.x`、`ping` 通；負向在約 15 s（>= 15 s）顯示 `No IP address!` 並可 BACK；DHCP 期間 BACK 能退出。
- FAIL：顯示 `0.0.0.0`／與 lease 不符的位址；逾時無錯誤畫面；BACK 無效。

**記錄欄位**：`connected`→`serving` 秒數（3 次）；顯示的 IP、路由器 lease 的 IP／MAC／到期時間；`ping` 結果；DHCP 逾時實測秒數；子網路設定；路由器的 DHCP 回應是否很慢（`config_v4()` 輪詢對真實 router 的行為未驗，見 4）。

### W3 HTTP 內容（頁面、list、upload、delete、錯誤路徑）

**目的**：R6（既有 upload／list／delete 行為）、R7（SD 上的檔案與輸入位元組一致）、R8（失敗要回報，不顯示上傳成功）、R14。

**準備**（PC）

```
export IP=<螢幕括號內的位址>          # 例如 192.168.1.50
mkdir -p /tmp/pulp-acc && cd /tmp/pulp-acc
: > ACC0B.TXT
printf 'x' > ACC1B.TXT
head -c 2047    /dev/urandom > ACC2047.TXT
head -c 2048    /dev/urandom > ACC2048.TXT
head -c 2049    /dev/urandom > ACC2049.TXT
head -c 102400  /dev/urandom > ACC100K.TXT
head -c 1048576 /dev/urandom > ACC1M.EPUB
cp ACC100K.TXT acceptance-long-name.txt
printf 'delete me' > DEL1.TXT
printf 'keep me'   > KEEP.TXT
```

檔名選擇的依據：上傳檔名會被 `sanitize_83` 轉成 8.3 大寫（`src/apps/upload/http.rs:390-439`）：基底最多 8 個合法字元（英數與 `_-~!#$&`）、副檔名最多 3 個字元。所以 `ACC1M.EPUB` 存成 `ACC1M.EPU`、`acceptance-long-name.txt` 存成 `ACCEPTAN.TXT`（兩者都會多一行 `warn`，`http.rs:258`）。`ACC2047/2048/2049` 對準 upload 的工作緩衝區 2048 bytes（`http.rs:25`）。

**3a 頁面**

```
curl -sS -D - -o page.html http://$IP/ | head -5
shasum -a 256 page.html /path/to/repo/assets/upload.html
```
預期：`HTTP/1.0 200 OK`、`Content-Type: text/html; charset=utf-8`、`Connection: close`（`http.rs:11-12`）；`page.html` 與韌體編入的 `assets/upload.html` 位元組一致（`http.rs:22` `include_bytes!`；用建置該韌體的同一份工作樹比對）。

**3b list**

```
curl -sS -D - http://$IP/files            # 加 | jq . 看格式
```
預期：`HTTP/1.0 200 OK`、`Content-Type: application/json`、`Access-Control-Allow-Origin: *`（`http.rs:13-14`）；body 為 `[{"name":"X.TXT","size":N},...]`。**只列根目錄、副檔名為 TXT／EPUB／EPU／MD、且名稱不以 `.` 或 `_` 開頭的檔案，最多 64 筆**（`kernel/src/drivers/dir_entry.rs:94-102` `is_listed_name`；`http.rs:29` `DIR_LIST_MAX`）。因此上傳到其他副檔名的檔案雖然會寫入，卻不會出現在 list。與 PC 掛載卡片後的 `ls -l` 逐筆比對 name 與 size。

**3c upload 與位元組一致**

```
for f in ACC0B.TXT ACC1B.TXT ACC2047.TXT ACC2048.TXT ACC2049.TXT ACC100K.TXT ACC1M.EPUB acceptance-long-name.txt DEL1.TXT KEEP.TXT; do
  printf '%s -> ' "$f"
  curl -sS -H 'Expect:' -F "file=@$f" -w ' [%{http_code}] %{time_total}s\n' http://$IP/upload
done
curl -sS http://$IP/files
```
預期：每次 body 為 `OK`、HTTP 200（`http.rs:143-145`）；裝置 log 每個檔案有 `upload: receiving file '<NAME>'`（`http.rs:289`）→ `upload: complete, <N> bytes written`（`http.rs:304`，N 等於檔案大小，0 B 檔為 0）→ `upload: file saved as '<NAME>'`（`mod.rs:225`）；`GET /files` 列出 `ACC0B.TXT`(0)、`ACC1B.TXT`(1)、`ACC2047.TXT`、`ACC2048.TXT`、`ACC2049.TXT`、`ACC100K.TXT`(102400)、`ACC1M.EPU`(1048576)、`ACCEPTAN.TXT`(102400)、`DEL1.TXT`、`KEEP.TXT`，大小與來源一致。同名再上傳會覆寫（上傳開頭以 `write_file(name, &[])` 截斷，`http.rs:291`）。

內容驗證（HTTP 沒有下載端點，只能讀卡）：**先在裝置上按 BACK 退出 upload，等 Home 出現，再取出卡片**（卡片移除後 Home 會被重設且憑證會被清空，這是 `storage_changed` 的行為），在 PC 掛載後：

```
cd /tmp/pulp-acc
{ shasum -a 256 ACC0B.TXT ACC1B.TXT ACC2047.TXT ACC2048.TXT ACC2049.TXT ACC100K.TXT KEEP.TXT
  shasum -a 256 ACC1M.EPUB | sed 's/ACC1M.EPUB/ACC1M.EPU/'
  shasum -a 256 acceptance-long-name.txt | sed 's/acceptance-long-name.txt/ACCEPTAN.TXT/'; } > expected.sha256
cd <SD 掛載點> && shasum -a 256 -c /tmp/pulp-acc/expected.sha256
```
（`DEL1.TXT` 會在 3d 刪除，不在清單內；`ls` 確認它不存在。）

**3d delete**

```
curl -sS -w ' [%{http_code}]\n' -H 'Content-Type: text/plain' --data-binary 'DEL1.TXT' http://$IP/delete
curl -sS http://$IP/files
```
預期：`OK [200]`、log `upload: deleted 'DEL1.TXT'`（`mod.rs:232`），list 不再含 `DEL1.TXT`。name 以 body 傳入，長度取 `min(Content-Length, 13)`（`http.rs:158`）。

**3e 錯誤路徑**

```
curl -sS -i http://$IP/nosuch                                                     # 404
curl -sS -i -H "X-Pad: $(head -c 1100 /dev/zero | tr '\0' a)" http://$IP/         # 431
for n in '../KEEP.TXT' '..' '/KEEP.TXT' './KEEP.TXT' 'KEEP.TXT/' '\KEEP.TXT' 'ABCDEFGHIJKLMN.TXT' 'NOSUCH.TXT'; do
  printf '%s -> ' "$n"; curl -sS --data-binary "$n" -w ' [%{http_code}]\n' http://$IP/delete
done
curl -sS http://$IP/files                                                          # KEEP.TXT 仍在
```
預期：
- 未知路徑：`HTTP/1.0 404 Not Found`、body `Not Found`（`http.rs:19`、`:212`）。
- header 超過 1024 bytes：`HTTP/1.0 431 Headers Too Large`（`http.rs:20`、`:58-62`）。
- `/delete` 的前七個名稱：`Invalid filename [500]`，且 `KEEP.TXT` 不受影響。名稱必須等於 `sanitize_83` 清理後的結果（大小寫不分）才會被接受（`http.rs:184-187`、`:441-448` `is_plain_83`）；含 `/`、`\`、`..`、`.`、空字串、超過 8.3 長度都被拒絕，且不呼叫 storage。裝置 log：`upload: file delete failed`（`mod.rs:235`），沒有 `upload: deleted`。
- `NOSUCH.TXT`：預期 `delete failed [500]`、log `upload: delete failed for 'NOSUCH.TXT': ...`（`http.rs:205-206`）。**storage 對不存在檔案是否回錯，只在 host 虛擬儲存體上驗過；FAT 實機行為需確認並記錄。**
- 每一個錯誤之後，下一個 `curl http://$IP/files` 仍回 200（伺服器沒有卡住）。
- `GET /files` 在 SD 未掛載或列表失敗時回 `500`、body `list failed`（`http.rs:96-103`；host 證據：`host/tests/upload_regression.rs` 的 `get_files_without_a_mounted_card_is_refused`、`get_files_with_a_failing_listing_is_refused`）。**實機可達性限制**：SD「未掛載」（`SdStorage::empty()`）在實機上只會由 `poll_card` 偵測到拔卡產生（`kernel/src/kernel/scheduler_c61.rs:255-272`），而拔卡會同時重載設定並清空憑證（`storage_changed`），之後進 Upload 只會得到 `No WiFi credentials!`。所以**以正常流程無法在實機上同時具備憑證與「未掛載」狀態；此路徑的證據只有 host 測試。** 實機替代（選做，只觀察不判定）：serving 中拔卡（upload 期間不執行 `poll_card`，`SdStorage` 仍是已掛載狀態）再 `curl -i http://$IP/files`，記錄是回 500、卡住或其他；是否回 500 取決於底層讀取錯誤的呈現方式，未知。避免在寫入進行中拔卡。

**R8：中斷的上傳不得顯示成功**

```
curl -sS -H 'Expect:' --limit-rate 20k --max-time 5 -F "file=@ACC1M.EPUB;filename=INTR.TXT" http://$IP/upload; echo "curl exit=$?"
curl -sS -o /dev/null -w '%{http_code}\n' http://$IP/files
```
預期：curl 約 5 s 後 exit 28；裝置 log 有 `upload: receiving file 'INTR.TXT'`，之後有 `upload: handle_upload error: ...`（`upload incomplete` 或 `read error during upload`，`http.rs:149`、`:320-325`）與 `upload: file upload failed`（`mod.rs:228`），**沒有** `upload: file saved as 'INTR.TXT'`；之後 `/files` 仍回 200。殘留的半個 `INTR.TXT`：spec 未規定其語意（`progress.md` T6 G4），只記錄是否存在與大小，不作 pass／fail。

**3f 瀏覽器**：用瀏覽器開 `http://<IP>/`（W4 通過後也開 `http://pulp.local/`）。預期（來自 `assets/upload.html`）：標題 `pulp manager`；檔案表載入（失敗會重試 3 次，錯誤分別顯示 `Could not list files`／`Bad response`／`Connection error`）；拖放或選檔上傳顯示 `<name> uploaded`；× 鈕確認後顯示 `<name> deleted`。記錄瀏覽器與版本，以及上傳多檔排隊時是否正常。

**判定準則**
- PASS：3a 的 sha256 一致；3b 的欄位與 list 內容符合；3c 全部 200、log 序列正確、`shasum -c` 全部 OK、size 與來源一致；3d 刪除成功；3e 全部狀態碼／body／log 如預期且伺服器持續可用；R8 的中斷上傳沒有成功 log；3f 在至少一種瀏覽器上完整運作。
- FAIL：任何 sha256 不一致、任何應失敗的請求回 200、`../X` 類名稱導致任何檔案被刪、錯誤後伺服器不再回應、中斷的上傳出現 `file saved`。

**記錄欄位**：每個檔案的 http_code、`time_total`、來源與 SD 上的 sha256、size；`/files` 原文；每個錯誤請求的原始回應與 log；SD 卡廠牌／容量／格式；上傳 1 MiB 的吞吐（`time_total`）；`NOSUCH.TXT` 的實際結果；選做的拔卡觀察；瀏覽器版本。

### W4 mDNS（`pulp.local`）

**目的**：R9（既有 `pulp.local` mDNS 回應）、R14。

**步驟**（裝置處於 serving；PC 與裝置同一子網路）
1. 解析（任選 PC 作業系統對應者；每次先清快取）：
   - macOS：`sudo dscacheutil -flushcache; sudo killall -HUP mDNSResponder`；`dns-sd -G v4 pulp.local`（看到 `Add` 與 IP 後 Ctrl-C）；`dns-sd -q pulp.local A`；`ping -c 3 pulp.local`。
   - Linux：`avahi-resolve -4 -n pulp.local`；`getent ahostsv4 pulp.local`。
   - **不要用 `dig` 當 pass／fail 依據**：回應固定以 ID 0 送到 `224.0.0.251:5353`（`mdns.rs:33`、`mod.rs:285-288`），不會回到 `dig` 的臨時來源埠，也不會帶它的 query ID。
2. 重複 10 次（自訂 N=10），每次清快取後解析，記錄是否成功、耗時、解析到的 IP。
3. 負向：`dns-sd -G v4 other.local`、`dns-sd -G v6 pulp.local`。
4. 封包（選做）：`sudo tcpdump -n -i <wifi 介面> -vv 'igmp or udp port 5353'`，同時執行步驟 1。

**預期觀察（來源）**
- 回應內容：DNS ID 0、旗標 `0x8400`（response、authoritative）、1 個 answer：`pulp.local` A IN，cache-flush 位元、TTL 120、RDATA 為裝置 IP，DNS payload 共 38 bytes（`mdns.rs:30-37`）；目的 `224.0.0.251:5353`（`mdns.rs:7-8`、`mod.rs:285-288`）；來源埠 5353（socket 綁在 5353，`mod.rs:314`）。
- 每回覆一次 query，裝置 log 一行 `upload: mDNS answered pulp.local`（`mod.rs:292`）；送出失敗為 `upload: mDNS send failed: ...`（`:289`）。
- 只回 `pulp.local` 的 A（或 ANY）查詢；其他名稱、AAAA 不回（`mdns.rs:93-108`），所以負向兩條不該得到答案、裝置也不該有 `answered` log。
- 沒有主動宣告：韌體只在收到查詢時回應（`mdns.rs:157-166`）。
- 加入群組：serving 開始時 `join_multicast_group(224.0.0.251)`；失敗會有 `upload: mDNS group join failed: ...`（`mod.rs:316-317`）；成功後 smoltcp 在下次輪詢送 IGMP membership report（`smoltcp-0.12.0/src/iface/interface/multicast.rs:105-128`、`:185-200`；版本與是否被 AP 轉發以封包為準）。
- 查詢大小：超過 512 bytes 的查詢被丟棄（`mdns.rs:13-16`，刻意取捨）。
- **裝置端沒有「收到 query」的 log**；只有成功送出回應時才有 `answered`。若 `answered` 沒出現，無法分辨「query 沒到」與「回應送不出」：**目前韌體沒有此輸出，需先加 instrumentation**；實機上改用 PC 端 `tcpdump` 區分。
- Wi-Fi power-save：esp-radio 在 `WifiController::new` 內固定套用 `PowerSaveMode::default()`（= `None`，`esp-radio-1.0.0-beta.1/src/wifi/mod.rs:2769`、`:2271-2282`），本 repo 沒有覆寫。這是設定值，multicast 實際是否送達裝置：未驗。

**判定準則**
- PASS：真實 client 解析 `pulp.local` 得到與螢幕一致的 IP（無任何一次回錯誤位址）；10 次中至少 9 次在 5 s 內成功（自訂門檻，失敗的那次需有 tcpdump 或 log 解釋）；負向沒有答案；每次成功對應一行 `answered` log。
- FAIL：解析不到、得到錯誤 IP、負向得到答案、`join failed`／`bind failed`。
- 失敗時的分流：HTTP 用 IP 正常但 mDNS 失敗 → 查 AP 的 IGMP snooping／multicast 設定與 tcpdump（query 是否到達、IGMP report 是否送出）；`pulp.local` 解析到但 HTTP 失敗 → 單 socket 被占用（見 W7）。

**記錄欄位**：PC 作業系統與版本；每次解析的結果／耗時；`tcpdump` 抄錄（是否看到 IGMP report、query、response，response 的欄位）；AP 的 IGMP snooping 與 DTIM 設定；負向結果；`answered` log 次數；首次解析的延遲（macOS 會同時詢問 AAAA，本韌體不回 AAAA，對延遲的影響：未知）。

### W5 重入與資源釋放

**目的**：R10（退出或連線失敗要釋放 network／radio 資源並回到可操作的閱讀介面）、R11（再次啟動 upload 不因殘留 ownership 失敗）、R14。

**自訂參數**：N = 10 次連續成功 session；每條失敗路徑各 3 次。理由：host 測試已用 100 次驗證 `session::run` 的 drop 順序（`host/tests/upload_session.rs`），實機要看的是真實 `Interface`／`WifiController`／radio 的釋放與 heap 趨勢；10 次足以看出每圈固定洩漏的單調成長，且能在一個驗收時段完成。

**heap／stack 讀數（韌體已有的輸出）**：主迴圈每 5 s 一行 `stats: heap <used>/<total>K peak <peak>K | stack free <F>K hwm <H>K | bat ... | SD:ok`（`kernel/src/kernel/scheduler.rs:288-289`、`:600-620`，間隔 `timing.rs:35` `STATUS_INTERVAL_SECS = 5`；C61 另在開機與每次重繪時印，`scheduler_c61.rs:156`、`:230`）。`used` 與 `peak` 是 `esp_alloc::HEAP` 的 `current_usage` 與 `max_usage`（esp-alloc 的 `internal-heap-stats` 已啟用，根 `Cargo.toml:23`；`max_usage` 是上游標示的「估計值」）；單位被整除成 K（精度 1 KiB）；`hwm` 是主 stack 的高水位（開機時 `paint_stack`，`src/bin/main_c61.rs:79`；esp-rtos 的 main task 使用 `_stack_start_cpu0`／`_stack_end_cpu0`，`esp-rtos-0.4.0/src/lib.rs:414-422`）。**upload 期間主迴圈暫停，沒有 `stats:` 輸出**；退出回到 Home 後的 5 s 內會出現。`peak` 與 `hwm` 是自開機起的高水位，所以退出後的那一行能反映 upload 期間的峰值，但不能反映期間的時間曲線。

**步驟**
- **A. 連續成功 N 次**：Home → Upload → 等到 serving 畫面 → `curl -sS -o /dev/null -w '%{http_code}\n' http://$IP/files`（應 200；第 1、5、10 次再做一次 W4 解析）→ BACK → 等 Home 出現並等到一行 `stats:` → 記錄。第 0 次記錄：進入 Upload 前最後一行 `stats:`。
- **B. 各階段 BACK**（每項 3 次，之後立刻再進一次 Upload 到 serving 並做 `curl`）：
  - B1 Associating 中（AP 關閉，在 20 s 內按 BACK）；
  - B2 DHCP 等待中（無 DHCP 網路，15 s 內按 BACK）；
  - B3 serving 中；
  - B4 serving 且有進行中的傳輸：`curl -H 'Expect:' --limit-rate 20k -F "file=@ACC1M.EPUB;filename=BACK1.TXT" http://$IP/upload` 傳到一半按 BACK。
- **C. 失敗後再進入**（每項 3 次，之後以正確網路再進一次到 serving）：C1 `AssociationFailed` 或 `AssociationTimeout`（錯誤密碼／AP 關閉）；C2 `DhcpTimeout`；C3 `MissingCredentials`（不經過 radio，確認錯誤畫面後 BACK 的路徑）。每次先在錯誤畫面按 BACK 回 Home。

**預期觀察（來源）**
- 退出 log：`upload: user exited (<Associating|Dhcp|Serving>), WiFi released`（B 類）或 `upload: failed: <Debug 名稱>, WiFi released`（C 類）（`mod.rs:207-210`）。畫面回 Home 並完整重繪（`handle_special_mode` 做 `Pop` + `request_full_redraw`，`scheduler.rs:219-225`）。
- 釋放順序（程式碼與 host 測試保證）：stage future → `Net { runner(含 Interface) → stack → controller }` → `run` 返回，之後才顯示錯誤畫面（`session.rs:1-9`、`mod.rs:60-71`、`baseline.md` T5 所有權審查）。
- 再進入不得出現 `upload: station interface already taken`（`mod.rs:116`）或 `upload: wifi init failed`（`:129`）；兩者都會顯示 `WiFi init failed!` / `Radio not available`（`ConnectError::RadioUnavailable`，`connect.rs:54`）。
- 回到 Home 後不應有重啟：log 中不應再次出現開機行 `pulp-os c61: esp32c61 rv32imac, rtos + embassy up, wake ...`（`src/bin/main_c61.rs:103-106`）。
- radio 配置是否由 deinit 還清：source 只到 `esp_wifi_deinit_internal`，blob 內部不可見（`baseline.md` T5 UNVERIFIED），**只能以 `stats:` 的 `used` 間接觀察**。

**判定準則（自訂）**
- PASS：A 的 10 次都到達 serving 且 `curl /files` 回 200；B、C 每次都能 BACK 回 Home 且 Home 可操作（導覽鍵選單會動、可進 Files 再返回），之後的再進入都成功；整個過程沒有 panic／重啟、沒有 `RadioUnavailable` 的 log 或畫面；`stats:` 的 `used`（以第 1 次退出後為基準）在第 2–10 次沒有連續 5 次以上嚴格遞增（有則視為疑似洩漏，需另行調查）；第 1 次相對第 0 次的增量記為「一次性 init 配置」，**不判 fail 但必須記錄**；`hwm` 的最大值 < 50K（stack 區 51,960 B，`budget-report.md`）；`hwm` >= 48K 要在備註標示為警示。
- FAIL：任何一次無法進入 serving 或無法退出、出現 `RadioUnavailable`、panic／重啟、`used` 持續成長、`hwm` >= 50K。
- 精度限制：`used` 以 K 為單位，每圈 < 1 KiB 的洩漏在 10 次內可能看不出，只能說「在此精度下未見成長」。

**記錄欄位**：每個 cycle 一列：

| cycle／路徑 | 到達的畫面 | exit log | `stats:` used／total／peak | hwm | free | 備註（重啟？Home 可操作？curl 結果） |
|---|---|---|---|---|---|---|

另記：重新 init 的耗時（第 2 次以後進入到 `wifi initialised` 的秒數，與第 1 次比）；esp-rtos 為 radio 建立的任務是否殘留：**目前韌體沒有此輸出，需先加 instrumentation**。

### W6 功耗（電流）

**目的**：R14（電流是實機項目，未測前標未驗）。repo 沒有電流預算，**不訂合格門檻**；本節的目標是產生可比較的量測值。

**狀態定義**（同一電池／電源、同一量測點，每個狀態穩定後記錄至少 60 s；自訂）
- S0：wifi 建置、Home 閒置（進入 Upload 前的基準）。
- S1：associated 閒置：serving 畫面，沒有任何 client 流量。
- S2：serving 且有負載：PC 端連續請求，例如 `while :; do curl -sS -o /dev/null http://$IP/files; done`，或重複上傳 `ACC1M.EPUB`。
- S3：BACK 退出、回到 Home 後閒置至少 60 s。
- 可選 S1a：`Connecting` 期間（association／DHCP 的突發電流）。

**預期觀察（來源，非實測）**
- Wi-Fi power-save 模式為 `None`（`esp-radio-1.0.0-beta.1/src/wifi/mod.rs:2769`，repo 沒有呼叫 `set_power_saving`）：S1 不應有 modem sleep 帶來的降幅。
- 退出時 `WifiController` drop → `wifi_deinit`，最後一個 radio guard drop 時關 modem power domain 與 clocks（`baseline.md` T5 所有權審查所列 `src/wifi/mod.rs:1221-1240`、`src/lib.rs:397-423`，皆為 source 證據）：S3 預期回到接近 S0。「接近」的容差：未知，記錄 S3−S0 差值，由使用者判定；明顯高於 S0 是 radio 未完全關閉的跡象（R10）。
- serving 期間 CPU 是否進入任何 idle／sleep 狀態：未知。

**需要使用者決定／未知**
- 儀器、量程、取樣率、量測點（電池端串接、或 USB 5 V）：repo 未載明，需依原理圖與手上設備決定。
- log 與供電的衝突：以 USB 連線取得 log 時，USB 同時供電，且板上有 USB 偵測與充電控制路徑（`board-logic` 的 `usb`／`battery`），電流量測會混入充電。建議（非 repo 事實）：先以 USB log 確認狀態，量測時改電池供電並斷開 USB，以螢幕狀態（serving 畫面顯示 IP、退出後 Home）作為狀態標記；是否可行取決於 log channel 與供電設計，未驗。
- idle 計時器：upload 期間 idle-sleep 計時器是否持續運作、長時間 serving 後退出是否立即進入睡眠：未查證。預設 `sleep_timeout` 為 10 分鐘（`config.rs:9` `DEFAULT_SLEEP_TIMEOUT`）；每次量測記錄時長，若超過 `sleep_timeout` 要在備註註明。

**判定準則**：S0／S1／S2／S3 都有量測值與完整條件即可把此項標為「已記錄」；是否合格由使用者依電池容量需求決定（repo 無依據）。S3 明顯高於 S0 另開 issue。

**記錄欄位**：儀器型號與量程、取樣率、量測點、供電方式與電壓、USB 是否連接、每個狀態的 min／avg／max／峰值與持續時間、AP 與距離（RSSI：目前韌體沒有此輸出）、`sleep_timeout` 設定、EPD 狀態（顯示中的畫面）。

### W7 HTTP 靜默對端逾時

**目的**：R6／R10 的邊界；`progress.md` T6 的 G9：使用者決定**不改**程式，標 UNVERIFIED 交本任務實機確認 smoltcp 對靜默對端的行為。

**步驟**（serving 中）
```
# 終端 A：建立連線但不送任何資料
nc -v $IP 80
# 終端 B：每 ~2 s 探測並記時，直到成功或 120 s
start=$(date +%s); while :; do
  code=$(curl -s -m 3 -o /dev/null -w '%{http_code}' http://$IP/files); now=$(date +%s)
  echo "t+$((now-start))s code=$code"
  [ "$code" = 200 ] && break; [ $((now-start)) -ge 120 ] && break; sleep 2
done
```
變體：先送不完整的 header 再靜默：`(printf 'GET /files HTTP/1.1\r\nHost: x\r\n'; sleep 300) | nc $IP 80`。最後在終端 A 仍連線時按裝置 BACK，確認能退出並可再進入。

**預期觀察（來源）**
- 伺服器是單 socket、逐一處理；已 accept 的連線在 `socket.read` 等 header（`http.rs:51`），其間沒有其他請求能被服務。第二個請求會被拒絕或逾時（哪一種：未知，記錄）。
- socket 設有 30 s timeout（`mod.rs:54`、`:249`），**但它是否會回收靜默對端：文件互相矛盾**——embassy-net 的說明是「超過時間沒收到資料就關閉」（`embassy-net-0.8.0/src/tcp.rs:346-351`），smoltcp 的說明只在「剛 connect 沒回應」、「傳送緩衝區有資料而對端沉默」、「啟用 keep-alive 且對端沉默」三種情況才中止（`smoltcp-0.12.0/src/socket/tcp.rs:703-715`）；本 repo 沒有呼叫 `set_keep_alive`（grep 無結果）。依 smoltcp 文件，靜默的 established 對端**可能不會被 30 s 回收**，單 socket 會一直被占用到對端關閉或使用者按 BACK。這是讀碼推論，實測決定。
- BACK 不受影響：`back` 與所有階段競爭（`session.rs:71-73`），按 BACK 應直接結束 session。

**判定準則**：不訂「必須在 N 秒內回收」的門檻（使用者決定不改）。PASS 的意思是「已取得實測資料且 BACK 在靜默連線存在時仍能退出並釋放（R10）、之後可再進入」；若 BACK 無法退出或再進入失敗 → FAIL。

**記錄欄位**：第二個請求的失敗型態（refused／timeout）；`code=200` 首次出現的 t（若 120 s 內都沒有，記為「未回收」）；`nc` 變體的結果；BACK 與再進入結果；`nc`／`curl` 版本。

### W8 radio 執行期 heap、stack 高水位與 PSRAM 分工（R12 的實機部分）

**目的**：把 `budget-report.md`「R12 UNVERIFIED」的實機項目變成可記錄的數據。R12 本身由 T6 的靜態預算證據滿足；本項不改變靜態結論，只補執行期數據。

**步驟與預期（來源）**
1. 確認被測映像是 wifi 變體：開機後第一行或第一批 `stats:` 的 `<total>` 應約為 114K 級（internal heap = 52 KiB main + 64,000 B reclaimed = 117,248 B，`budget-report.md`；÷1024 ≈ 114.5，allocator 內部開銷可能使實際值略小）；離線建置為 162,304 B ≈ 158K 級。**若 wifi 建置顯示 158K 級，表示燒到了離線映像。**
2. 取 W5 的 `stats:` 資料：第 0 次、第 1 次之後的 `peak`（radio 峰值上界 = 第 1 次退出後的 `peak` − 第 0 次的 `peak`，只是粗估）與各次 `hwm`／`free`。
3. PSRAM：開機時 `memory::log_report()` 印 region 與 PSRAM heap 的使用量（`kernel/src/board_c61/memory.rs:353-369`、`src/bin/main_c61.rs:91`）。全域 heap 只有 internal region（`memory.rs:8-10`），radio 經全域 allocator 的配置只能在 internal；radio 是否另外要求 PSRAM：未知。**Wi-Fi 期間與之後沒有 PSRAM／internal 的分項輸出**（`log_report` 只在開機呼叫、`log_classes` 只在 bigbuf 配置被拒時呼叫，`kernel/src/kernel/bigbuf.rs:105`）：目前韌體沒有此輸出，需先加 instrumentation。
4. 閱讀路徑受 52 KiB 縮減的影響：在 wifi 建置上開啟與離線建置相同的 EPUB／含圖片的書（若 baseline 有指定檔案則用同一份），觀察 `stats:` 的 `peak`、是否出現 `bigbuf: ... refused`（`bigbuf.rs:105`）或 panic。與離線建置對照。

**判定準則（自訂）**：radio 啟動成功（W1 通過）且 W5 無 `RadioUnavailable`、無 OOM／panic；閱讀路徑在 wifi 建置上無 `bigbuf refused` 與 panic，或有差異時已記錄供使用者決定是否調整 52 KiB（它是 `[assumed]` 值，見 `proposal.md`）。heap 的數值無 spec 門檻，只記錄。

**記錄欄位**：wifi／離線的 `<total>`；第 0 次與第 1–10 次的 `used`／`peak`／`hwm`／`free`；開機 `log_report` 抄錄；閱讀路徑的檔案、`stats:`、警告。

## 3. UNVERIFIED 追蹤表

狀態只能是 `UNVERIFIED`（預設）；實測時在記錄副本中改為 `PASS`／`FAIL`／`SKIPPED` 並附證據。本表保持全 `UNVERIFIED`。

| 項目 | 對應 R | 狀態 | 驗證者／日期／韌體 commit | 結果 | 備註 |
|---|---|---|---|---|---|
| W1 association（含 credentials／association 失敗畫面與 BACK 退出） | R4、R5、R14 | `UNVERIFIED` | — | — | script 行：association and DHCP IPv4 |
| W2 DHCP 取得 IPv4（期限 15 s、逾時畫面、DHCP 期間 BACK） | R4、R5、R14 | `UNVERIFIED` | — | — | 同上；IPv4 輪詢對真實 router 未驗 |
| W3 HTTP 內容（頁面、list、upload 位元組一致、delete、404／431／500 路徑、R8 中斷上傳、瀏覽器） | R6、R7、R8、R14 | `UNVERIFIED` | — | — | script 行：HTTP page / file list / upload / delete content；SD 未掛載 500 在實機不可由正常流程達成（見 W3），僅 host 證據 |
| W4 mDNS `pulp.local` 實收（含 IGMP／multicast、power-save 觀察） | R9、R14 | `UNVERIFIED` | — | — | script 行：mDNS pulp.local answered to real queries |
| W5 重入與釋放（N=10、各階段 BACK、失敗後再進入、radio heap 回收） | R10、R11、R14 | `UNVERIFIED` | — | — | script 行：re-entry after BACK / failure / timeout |
| W6 功耗（Home 基準、associated 閒置、serving、退出後） | R14 | `UNVERIFIED` | — | — | script 行：current draw；儀器／量測點未定 |
| W7 HTTP 靜默對端（30 s socket timeout） | R6、R10（邊界；G9） | `UNVERIFIED` | — | — | script 行：HTTP request to a silent peer |
| W8 radio 執行期 internal heap、stack 高水位、PSRAM 分工 | R12（執行期部分）、R14 | `UNVERIFIED` | — | — | script 行：radio runtime internal heap, stack high-water, PSRAM split |

### 3.1 與 `scripts/run-software-acceptance.sh` 的對照

腳本結尾列了 7 行 Wi-Fi `UNVERIFIED`，與本表一一對應：association+DHCP → W1、W2；HTTP → W3；mDNS → W4；re-entry → W5；radio heap／stack／PSRAM → W8；current draw → W6；silent peer → W7。本表的 8 列對腳本的 7 行（W1、W2 共用一行）。T7 時已把腳本的電流那一行措辭改為涵蓋「associated 閒置、serving、退出後」，並補上指向本文件的一行；沒有動任何檢查或 stage。

## 4. 已知風險與觀察點

來源：`progress.md`、`budget-report.md`、`baseline.md`，另標「本文件讀碼補充」者為撰寫本文件時讀碼所見。

| 風險 | 事實與來源 | 如何在實機觀察 |
|---|---|---|
| stack 餘裕僅 2,808 B | wifi 變體 `.stack` 51,960 B，比 STACK_MIN_BYTES 49,152 多 2,808 B（`budget-report.md`）。這是靜態推算，不是使用量；async future 由 `embassy_main` task 的 static `POOL`（19,704 B）承載，不計入 stack（`budget-report.md` 的推論，未逐一驗證） | 主 task stack 的高水位已有輸出：`stats:` 的 `hwm`（見 W5 的讀數說明）；退出後那一行含 upload 期間的峰值。radio／RTOS 其他 task 的 stack：**目前韌體沒有此輸出，需先加 instrumentation**。upload 期間沒有 `stats:`（主迴圈暫停） |
| 52 KiB internal heap 為 `[assumed]` | `proposal.md` Assumptions：由「statics 需再省 ≥ 41.8 KB」推得，不是 radio 實際需求量測；可能不足。對閱讀路徑的影響也未實測（`budget-report.md`） | `stats:` 的 `<total>`、`peak`（W8）；`upload: wifi init failed: ...`（`mod.rs:129`）與 `WiFi init failed!` 畫面是 radio 起不來的徵兆；閱讀路徑看 `bigbuf: ... refused`／panic。upload 期間 heap 時間曲線：**目前韌體沒有此輸出，需先加 instrumentation** |
| HTTP 靜默對端逾時未驗 | G9：30 s socket timeout 沒改；兩份上游文件對 timeout 語意不一致（見 W7）；本 repo 未設 keep-alive | W7 |
| multicast 實收未驗 | 實機是否收到 multicast（IGMP join 被 AP 接受、驅動放行 `01:00:5e:00:00:fb`、smoltcp multicast 路徑）、power-save／DTIM 的影響、自送回應是否回環（`progress.md` T4） | W4；PC 端 `tcpdump`。裝置端沒有「收到 query」log：**目前韌體沒有此輸出，需先加 instrumentation** |
| deinit 是否還清 radio allocation 未驗 | source 只到 `esp_wifi_deinit_internal`（`baseline.md` T5）；`wifi_init` 中途失敗沒有 rollback（`src/wifi/mod.rs:1163-1190`）；重新 init 耗時、esp-rtos 任務殘留皆未驗 | W5 的 `used` 趨勢是間接證據；radio 內部配置與 RTOS 任務清單：**目前韌體沒有此輸出，需先加 instrumentation** |
| DHCP IPv4 輪詢對真實 router 未驗 | dhcp 階段迴圈 `wait_config_up` + 每 100 ms 檢 `config_v4()`（`mod.rs:153-166`）；host 測不到（`progress.md` T6） | W2：螢幕顯示的 IP 與路由器 lease 比對；`connected`→`serving` 秒數。輪詢本身沒有 log |
| `WifiController::new` 失敗現歸 `RadioUnavailable` | `progress.md` T2 原記為 `AssociationFailed`（`Connection failed!`），T5 起改為 `RadioUnavailable`（`WiFi init failed!` / `Radio not available`）；`station interface already taken` 也歸同一錯誤（`mod.rs:115-131`） | 發生時以 log 分辨：`upload: station interface already taken`（`:116`）vs `upload: wifi init failed: <err>`（`:129`）。實機上沒有刻意誘發的辦法，只能在 W5 被動觀察是否出現 |
| link 掉線／DHCP lease 變動後的行為（本文件讀碼補充） | serving 階段只有 `select3(runner.run(), serve_http, serve_mdns)`（`mod.rs:192-197`），沒有任何程式碼觀察 controller 的斷線事件或位址變化；螢幕與 mDNS 回應使用 serving 開始時的 IP（`mod.rs:167-196`）。spec 沒有要求 | serving 中把 AP 斷電再恢復，觀察螢幕是否仍顯示舊 IP、HTTP／mDNS 是否還回應、BACK 是否仍能退出。不判 pass／fail，只記錄 |
| 副檔名與列表（本文件讀碼補充） | `.epub` 上傳後存成 `.EPU`（`sanitize_83` 副檔名截 3 字元，`http.rs:423-431`）；list 只含 TXT／EPUB／EPU／MD（`dir_entry.rs:94-102`）。瀏覽器 UI 宣稱 `any file` | W3c 已涵蓋；記錄其他副檔名（如 `.pdf`）上傳成功但不在 list 的現象即可 |

## 5. log 與畫面字串對照（從程式碼取得）

通用：`log::info!` 等級；ESP_LOG=info；無時間戳；色碼見 1.5。radio 本身也會輸出（esp-radio 啟用 `log-04`，根 `Cargo.toml` 的 `esp-radio` features），這些字串不在本 repo，不列。`{:?}` 的內容（驅動錯誤、`ConnectionError`）：未知，需記錄。

### 5.1 upload 流程 log

| 檔案:行 | 字串 | 何時出現 |
|---|---|---|
| `src/apps/upload/mod.rs:116` | `upload: station interface already taken` | `Interface::try_station()` 為 `None`（前一個 owner 未釋放）→ `RadioUnavailable` |
| `mod.rs:129` | `upload: wifi init failed: {:?}` | `WifiController::new` 失敗 → `RadioUnavailable` |
| `mod.rs:140` | `upload: wifi initialised, connecting to '{ssid}'` | associate 階段開始 |
| `mod.rs:145` | `upload: connect failed: {:?}` | `connect_async` 回錯 → `AssociationFailed` |
| `mod.rs:148` | `upload: connected to '{ssid}', waiting for DHCP` | association 完成，DHCP 階段開始 |
| `mod.rs:171` | `upload: serving at http://pulp.local/  {(a.b.c.d)}` | 取得 IPv4，進入 serving |
| `mod.rs:207` | `upload: user exited ({Associating｜Dhcp｜Serving}), WiFi released` | BACK 結束 session |
| `mod.rs:209` | `upload: failed: {MissingCredentials｜InvalidCredentials｜AssociationFailed｜AssociationTimeout｜DhcpTimeout｜RadioUnavailable}, WiFi released` | 連線階段失敗 |
| `mod.rs:225` | `upload: file saved as '{name}'` | 上傳成功 |
| `mod.rs:228` | `upload: file upload failed` | 上傳失敗 |
| `mod.rs:232` | `upload: deleted '{name}'` | 刪除成功 |
| `mod.rs:235` | `upload: file delete failed` | 刪除失敗（含名稱被拒） |
| `mod.rs:289` | `upload: mDNS send failed: {:?}` | mDNS 回應送出失敗 |
| `mod.rs:292` | `upload: mDNS answered pulp.local` | mDNS 回應送出成功 |
| `mod.rs:315` | `upload: mDNS bind failed: {:?}` | UDP 5353 bind 失敗 |
| `mod.rs:317` | `upload: mDNS group join failed: {:?}` | 加入 224.0.0.251 失敗 |
| `mod.rs:320` | `upload: mDNS stopped: {:?}` | mDNS 接收錯誤，mDNS 停止（HTTP 繼續） |
| `src/apps/upload/http.rs:99` | `upload: listing failed: {}` | `GET /files` 列表失敗（回 500） |
| `http.rs:149` | `upload: handle_upload error: {}` | 上傳處理錯誤（訊息見 5.3） |
| `http.rs:205` | `upload: delete failed for '{name}': {}` | storage 刪除失敗 |
| `http.rs:258` | `upload: sanitised '{raw}' -> '{clean}' (may overwrite existing file)`（warn） | 檔名被 8.3 清理改動 |
| `http.rs:289` | `upload: receiving file '{name}'` | 開始接收檔案 |
| `http.rs:304` | `upload: complete, {N} bytes written` | 檔案寫完 |

### 5.2 螢幕文字

| 檔案:行 | 文字 | 何時 |
|---|---|---|
| `mod.rs:404` | 標題 `Upload` | 所有 upload 畫面 |
| `mod.rs:93` | `Connecting to '{ssid}'...` | 連線中（憑證通過 `check_credentials` 才顯示，`mod.rs:90`） |
| `mod.rs:179-180` | `http://pulp.local/`、`(a.b.c.d)`、footer `Press BACK to exit` | serving |
| `mod.rs:352` | footer `Press BACK to exit` | 錯誤畫面 |
| `connect.rs:42-44` | `No WiFi credentials!`／`Set wifi_ssid in`／`_PULP/SETTINGS.TXT` | `MissingCredentials` |
| `connect.rs:47-49` | `WiFi config error!`／`SSID: 1-32 bytes`／`Password: 8-63 bytes` | `InvalidCredentials` |
| `connect.rs:51` | `Connection failed!`／`Check SSID and password` | `AssociationFailed` |
| `connect.rs:52` | `Connection timed out!`／`Router not responding` | `AssociationTimeout` |
| `connect.rs:53` | `No IP address!`／`DHCP timed out` | `DhcpTimeout` |
| `connect.rs:54` | `WiFi init failed!`／`Radio not available` | `RadioUnavailable` |
| `src/apps/home.rs:217` | `Upload` | Home 選單項（僅 wifi 建置） |

### 5.3 HTTP 回應與錯誤訊息

| 檔案:行 | 內容 |
|---|---|
| `http.rs:11-20` | 狀態列與 header：200 HTML／JSON／text、`500 Internal Server Error`、`404 Not Found`（body `Not Found`）、`431 Headers Too Large`；皆為 `HTTP/1.0` |
| `http.rs:100` | 500 body `list failed` |
| `http.rs:137` | 500 body `Missing multipart boundary` |
| `http.rs:172`、`:179`、`:185` | 500 body `Truncated body`、`Invalid filename`、`Invalid filename` |
| `http.rs:206` | 500 body `delete failed` |
| `http.rs:227`、`:248`、`:251`、`:273`、`:279-281`、`:287`、`:291`、`:301`、`:310`、`:320`、`:325` | 上傳錯誤訊息：`boundary too long`、`no filename in upload`、`invalid filename`、`part headers too large`、`read error`／`connection closed during headers`、`filename encoding error`、`write failed`、`read error during upload`、`upload incomplete` |

### 5.4 系統 log（判讀上述驗收用）

| 檔案:行 | 字串 | 用途 |
|---|---|---|
| `kernel/src/kernel/scheduler.rs:608-620` | `stats: heap {used}/{total}K peak {peak}K | stack free {F}K hwm {H}K | bat {pct}% {V} | up {h}:{mm} | SD:{ok｜--}`（呼叫：`scheduler.rs:289` 每 5 s、`scheduler_c61.rs:156` 開機、`:230` 每次重繪） | heap／stack 讀數（W5、W8） |
| `kernel/src/board_c61/memory.rs:353-369` | `memory {Region}: pool {used} / {limit} B`、`heap internal: used {} B, free {} B; psram heap: used {} B, free {} B` 等 | 開機時的 heap／PSRAM 基準 |
| `kernel/src/kernel/bigbuf.rs:105` | `bigbuf: {:?} {} B refused: {:?}`（warn，之後印各 class 用量） | 閱讀路徑配置被拒 |
| `src/bin/main_c61.rs:103-106` | `pulp-os c61: esp32c61 rv32imac, rtos + embassy up, wake {:?}` | 開機行；重現代表發生過重啟 |
| `kernel/src/kernel/scheduler_c61.rs:183` | `ui ready.` | 開機完成 |
| `src/apps/settings.rs:120`、`:123` | `settings: loaded from SETTINGS.TXT`／`settings: no file found, using defaults` | 憑證檔是否被讀到 |
| `kernel/src/kernel/scheduler_c61.rs:268`、`:290` | `sd: card removed ({})`／`sd: card inserted ({})` | 拔插卡 |
