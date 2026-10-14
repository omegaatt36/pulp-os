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

fn session_token() -> pulp_host::apps::upload_http::SessionToken {
    pulp_host::apps::upload_http::SessionToken::from_bytes([0x5a; 16])
}

// Existing HTTP behavior tests run as an authorized browser. Preserve their
// explicitly selected read boundaries when inserting the capability header.
fn authorize_request(mut input: Vec<u8>, splits: &mut [usize]) -> Vec<u8> {
    let Some(at) = input.windows(2).position(|w| w == b"\r\n").map(|at| at + 2) else {
        return input;
    };
    let header = format!("X-Pulp-Token: {}\r\n", session_token().as_str());
    for split in splits {
        if *split >= at {
            *split += header.len();
        }
    }
    input.splice(at..at, header.bytes());
    input
}
