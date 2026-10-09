// Consolidated Wi-Fi upload test suite
// Covers station connection, HTTP endpoints, mDNS, regressions, session lifecycle, and file writers.

#[path = "upload/connect.rs"]
mod connect;
#[path = "upload/http.rs"]
mod http;
#[path = "upload/mdns.rs"]
mod mdns;
#[path = "upload/regression.rs"]
mod regression;
#[path = "upload/session.rs"]
mod session;
#[path = "upload/writer.rs"]
mod writer;
