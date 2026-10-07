## T4 契約（orchestrator 定義）

新檔 `src/apps/upload/mdns.rs`（從 `mod.rs` 抽出並重寫 mDNS；**HAL-free**：只用 `core`、`embassy_time` 不需要；不得 `use esp_*`／`embassy_net`／`crate::board`）。host 以 `#[path = "../../src/apps/upload/mdns.rs"] pub mod upload_mdns;` 引入（implementer 在 `host/src/apps.rs` 加膠水）。測試從 `pulp_host::apps::upload_mdns` 取用。

```rust
pub const PORT: u16 = 5353;
pub const GROUP: [u8; 4] = [224, 0, 0, 251];
pub const RESPONSE_LEN: usize = 38;
pub const IP_LABEL_MAX: usize = 17;      // "(255.255.255.255)"

/// 若 `pkt` 是對 `pulp.local` 的 A（type 1）或 ANY（255）、class IN 的 mDNS 查詢，
/// 把回應寫入 `out` 並回 true；否則回 false（out 內容未定義）。永不 panic、不 hang（任意位元組輸入）。
pub fn handle_packet(pkt: &[u8], ip: [u8; 4], out: &mut [u8; RESPONSE_LEN]) -> bool;

/// 目前顯示用的 IP 標籤，格式 "(a.b.c.d)"，十進位、無前導零。
pub fn ip_label(ip: [u8; 4], buf: &mut [u8; IP_LABEL_MAX]) -> &str;

/// 對 embassy-net `UdpSocket` 的最小接縫（firmware 實作；host 測試以 fake 實作）。
pub trait Datagrams {
    type Error;
    /// 收下一個封包到 buf，回長度。
    async fn recv(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error>;
    /// 把 data 送到 mDNS multicast group（GROUP:PORT）。
    async fn send(&mut self, data: &[u8]) -> Result<(), Self::Error>;
}

/// 持續服務：反覆 recv；符合的查詢就 send 一個回應；不符合的封包被忽略而**不返回**；
/// send 失敗不終止（繼續下一個封包）；recv 失敗才返回 Err。除了 recv 錯誤外永不返回。
pub async fn serve<D: Datagrams>(socket: &mut D, ip: [u8; 4]) -> Result<core::convert::Infallible, D::Error>;
```

查詢判定規則（oracle：RFC 1035 §4.1／§4.1.4、RFC 6762 §5、§6、§10；不是現行碼）：
- header 12 bytes；QR bit（flags 0x8000）為 1（是回應）→ 不回；opcode（flags 0x7800）≠ 0 → 不回；QDCOUNT = 0 → 不回。
- 逐一解析 question section（QDCOUNT 個）；**任何一個**問題符合即回應（不只第一個）。name 比對為 `pulp.local`，label 不分大小寫；name 可含壓縮指標（0xC0xx；跟隨，但指標必須只指向較前面的位置，迴圈或前向指標 → 視為格式錯誤 → 不回）。
- qtype 1 或 255，qclass 去掉最高位（QU bit 0x8000）後 == 1 才算符合。
- 查詢的 ANCOUNT／NSCOUNT／ARCOUNT 可以非零（known-answer、EDNS OPT 常見）；解析 question 之後不必驗證後續記錄、截斷的後續記錄也不影響符合與否（只要 question section 完整）。
- 任何截斷／越界 → 不回。

回應格式（RFC 6762 §6、§10；38 bytes）：ID=0；flags=0x8400（QR=1、AA=1、opcode 0、rcode 0）；QDCOUNT=0；ANCOUNT=1；NSCOUNT=0；ARCOUNT=0；answer：NAME=`\x04pulp\x05local\x00`（不壓縮）、TYPE=1（A）、CLASS=0x8001（IN＋cache-flush）、TTL=120、RDLENGTH=4、RDATA=ip。

`serve` 行為由 fake `Datagrams` 驗證；firmware 端（mod.rs）：UDP socket 在整個 upload 連線期間 **bind 一次**並以 `Stack::join_multicast_group(GROUP)` 加入群組（embassy-net 需開 `multicast` feature；smoltcp 的 `has_multicast_group` 對未加入的 224.0.0.251 回 false，現行碼因此收不到查詢），同一個 socket 做 recv／send。
