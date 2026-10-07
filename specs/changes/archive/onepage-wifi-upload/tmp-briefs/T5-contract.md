## T5 契約（orchestrator 定義）

新檔 `src/apps/upload/session.rs`（**HAL-free**：只用 `core`、`embassy_time`、`embassy_futures`，以及同目錄 `connect.rs` 的 `Limits`／`ConnectError`／`check_credentials`）。host 以 `#[path = "../../src/apps/upload/session.rs"] pub mod upload_session;` 引入；因為它 `use super::connect::…` 或 `crate::apps::upload_connect::…` 之類的路徑在 host 與 firmware 的模組位置不同，implementer 必須讓兩邊都能編譯（例如在 session.rs 內用 `use super::connect`，並讓 host 把 `upload_connect` 與 `upload_session` 放在能被 `super::connect` 解析的位置，或以 `#[path]` 嵌套模組；選最簡單且不複製邏輯的方式）。測試從 `pulp_host::apps::upload_session`（及 `pulp_host::apps::upload_connect`）取用。

`connect.rs` 的修改：`ConnectError` 新增 variant `RadioUnavailable`（radio／controller 初始化失敗，或 station interface 已被占用），`lines()` 第一行 `"WiFi init failed!"`，與其他 variant 第一行相異、各行非空。既有 `bring_up`、`check_credentials`、`Limits` 的行為與簽名**不得改變**（`host/tests/upload_connect.rs` 唯讀、必須仍全綠）。

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase { Associating, Dhcp, Serving }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionEnd {
    Exited(Phase),          // back 先於其他事完成；Phase 是當時所在階段
    Failed(ConnectError),
}

/// 一次完整的 upload 連線 session。
/// 順序：check_credentials（失敗 → Failed，acquire 不得被呼叫）→ acquire()（Err(e) → Failed(e)，其後的 closure 都不得被呼叫）
/// → 以 limits.associate 為上限 associate(&mut c)（Err → AssociationFailed；逾時 → AssociationTimeout）
/// → 以 limits.dhcp 為上限 dhcp(&mut c)（逾時 → DhcpTimeout）→ serve(&mut c, t)（永不返回，直到被取消）。
/// `back` 與上述所有階段（含 acquire 之後的每一個 await 點）競爭；back 完成 → 取消當時的階段 future，回 Exited(當時階段)。
/// back 在 acquire 之前或同時已完成 → 回 Exited(Phase::Associating)（不論 acquire 是否已被呼叫，C 若已建立必須被 drop）。
///
/// **所有權保證（R10、R11）**：不論以何種結果返回（Exited／Failed／任一階段被取消），`c`（持有 radio controller 與 network interface 的 context）
/// 與所有階段 future 都在 `run` 返回**之前**已被 drop（先 drop 階段 future，再 drop c）。
pub async fn run<C, E, T>(
    ssid: &[u8],
    password: &[u8],
    limits: Limits,
    acquire: impl FnOnce() -> Result<C, ConnectError>,
    associate: impl AsyncFnOnce(&mut C) -> Result<(), E>,
    dhcp: impl AsyncFnOnce(&mut C) -> T,
    serve: impl AsyncFnOnce(&mut C, T) -> core::convert::Infallible,
    back: impl core::future::Future<Output = ()>,
) -> SessionEnd;
```

firmware（`mod.rs`）端要求（host 不測，compile 驗收＋source 證據）：
- `C` = 持有 `WifiController`、embassy-net `Stack`／`Runner`／`StackResources` 的 struct（欄位順序決定 drop 順序：network 先於 controller）；`acquire` 用 `Interface::try_station()`（None → `RadioUnavailable`，**不得 panic**）、`WifiController::new` 失敗 → `RadioUnavailable`。
- `run_upload_mode` 呼叫 `session::run(...)`；結果 `Failed(e)` 時：**session 已返回（資源已釋放）之後**才顯示 `e.lines()` 錯誤畫面並等 BACK；`Exited(_)` 直接返回。`run_upload_mode` 返回即代表 radio／interface 已釋放，scheduler 的既有路徑（`handle_special_mode`：Pop＋full redraw）接手回到閱讀介面。
- 不得新增大型 `static`（wifi 變體 statics 餘裕約 3.2 KB）。
