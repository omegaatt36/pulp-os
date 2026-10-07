## T3 契約（orchestrator 定義）

新檔 `src/apps/upload/http.rs`（自現有 `src/apps/upload/mod.rs` 抽出 HTTP 邏輯）。**不得**依賴 `esp_*`、`embassy_net`、`crate::board`、`embassy_time`（socket timeout 留在 mod.rs 的 socket 上）；可用 `core`、`alloc`（若現有程式已用）、`embedded_io_async`、`crate::drivers::{storage, sdcard::SdStorage, dir_entry::DirEntry}`、`crate::drivers` 的既有 `log`。host 以 `#[path = "../../src/apps/upload/http.rs"] pub mod upload_http;` 引入（implementer 在 `host/src/apps.rs` 加膠水；host 的 `drivers::storage`／`drivers::sdcard` 已是與 production 同簽名的 stand-in，implementer 視需要補上缺的 stand-in 函式，例如 `write_file`、`append_root_file`，補法必須與 production 的簽名一致並轉呼叫 `VirtualStorage`）。測試從 `pulp_host::apps::upload_http` 取用。

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerEvent {
    Nothing,
    Uploaded { name: [u8; 13], name_len: u8 },
    UploadFailed,
    Deleted { name: [u8; 13], name_len: u8 },
    DeleteFailed,
}

/// 讀一個已 accept 的連線上的**一個** HTTP 請求，處理並寫出完整回應後 flush；
/// **不關閉** socket（由呼叫端關閉）。
pub async fn serve_request<S>(socket: &mut S, sd: &SdStorage) -> ServerEvent
where S: embedded_io_async::Read + embedded_io_async::Write;
```
（`UPLOAD_PAGE` 為 `include_bytes!("../../../assets/upload.html")`。）

既有行為（R6「既有」的定義；瀏覽器端 `assets/upload.html` 就是這個 API 的 client，是獨立 oracle）：
- `GET /` → `HTTP/1.0 200 OK`，`Content-Type: text/html`，body 為 `assets/upload.html` 的逐位元組內容；event `Nothing`。
- `GET /files`（query string 被忽略）→ 200，`Content-Type: application/json`，body 是根目錄檔案的 JSON 陣列 `[{"name":"BOOK.TXT","size":123},...]`（空則 `[]`；`name` 為 8.3 名、`size` 為十進位位元組數；順序依 `list_root_files`）；event `Nothing`。
- `POST /upload`，`multipart/form-data; boundary=...`，欄位含 `filename="..."`：把 part 內容寫成根目錄檔案，檔名經 8.3 sanitize；成功 → 200，body `OK`，event `Uploaded{name,len}`（name 為實際寫入的 8.3 名）。任何失敗（缺 boundary、缺 filename、無效檔名、header 過大、連線中斷、儲存寫入失敗、SD 不存在）→ **非 200** 的狀態行（現行為 500）、event `UploadFailed`；**絕不**在失敗時送 200／`OK`，也**絕不**回報 `Uploaded`。
- `POST /delete`，`Content-Length: n`，body 為純檔名（client 以 `text/plain` 送 `name`）→ 刪除該根目錄檔案；成功 → 200 `OK`，event `Deleted`；檔名空／超過 12 bytes／非 UTF-8、或檔案不存在／儲存失敗 → 非 200、event `DeleteFailed`。
- 其他 method／path → `HTTP/1.0 404 Not Found`，event `Nothing`。
- request header 超過 1024 bytes 仍未見 `\r\n\r\n` → `HTTP/1.0 431 ...`，event `Nothing`。對端在 header 完成前關閉（read 回 0）→ 不 panic、event `Nothing`。
- 輸入以任意切片大小送達（含 1 byte／次）時結果必須相同；multipart 結尾 boundary 跨越 read 邊界亦然。
