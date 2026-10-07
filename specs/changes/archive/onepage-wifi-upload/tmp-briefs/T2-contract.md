## T2 契約（orchestrator 定義；test author 針對它寫測試，implementer 依它實作）

新檔 `src/apps/upload/connect.rs`（`src/apps/upload.rs` 會被改成目錄 `src/apps/upload/mod.rs`）。**HAL-free**：不得 `use esp_*`、`embassy_net`、`crate::board`；只能用 `core`、`embassy_time`、`embassy_futures`。host 以 `#[path = "../../src/apps/upload/connect.rs"] pub mod upload_connect;` 引入（由 implementer 在 `host/src/apps.rs` 加這一行膠水；測試從 `pulp_host::apps::upload_connect` 取用）。

```rust
use embassy_time::Duration;

pub const ASSOCIATE_TIMEOUT: Duration = Duration::from_secs(20);
pub const DHCP_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits { pub associate: Duration, pub dhcp: Duration }
impl Limits { pub const DEFAULT: Limits = Limits { associate: ASSOCIATE_TIMEOUT, dhcp: DHCP_TIMEOUT }; }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectError {
    MissingCredentials,   // SSID 為空
    InvalidCredentials,   // SSID 超過 32 bytes，或 WPA2 密碼不在 8..=63 bytes
    AssociationFailed,    // associate future 回 Err
    AssociationTimeout,   // associate 超過 limits.associate
    DhcpTimeout,          // dhcp 超過 limits.dhcp
}
impl ConnectError {
    /// 錯誤畫面文字行；每個 variant 非空、第一行彼此相異。
    pub fn lines(self) -> &'static [&'static str];
}

/// 純函式；不接觸 radio。
pub fn check_credentials(ssid: &[u8], password: &[u8]) -> Result<(), ConnectError>;

/// 先 check_credentials；失敗時 **不得呼叫** associate／dhcp closure。
/// 再以 limits.associate 為上限 await associate()；Err(_) → AssociationFailed，逾時 → AssociationTimeout（此時不得呼叫 dhcp closure）。
/// 再以 limits.dhcp 為上限 await dhcp()；逾時 → DhcpTimeout；完成回 Ok(T)。
pub async fn bring_up<A, AF, E, D, DF, T>(
    ssid: &[u8],
    password: &[u8],
    associate: AF,
    dhcp: DF,
    limits: Limits,
) -> Result<T, ConnectError>
where
    AF: FnOnce() -> A, A: core::future::Future<Output = Result<(), E>>,
    DF: FnOnce() -> D, D: core::future::Future<Output = T>;
```

憑證規則（依據：`WifiConfig::has_credentials()` 只看 SSID 非空；esp-radio `Ssid` 上限 32 bytes；WPA2 passphrase 規格 8..=63；空密碼＝不支援開放網路，視為 InvalidCredentials，本專案 upload 只做 WPA2 station）：
- SSID 長度 0 → `MissingCredentials`（不論密碼）。
- SSID 長度 > 32 → `InvalidCredentials`。
- SSID 1..=32 且密碼長度 ∉ 8..=63（含空密碼）→ `InvalidCredentials`。
- 其餘 → `Ok(())`。
