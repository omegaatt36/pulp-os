use embassy_futures::block_on;
use embedded_io_async::{ErrorType, Read, Write};
use pulp_host::apps::upload_http::{HttpScratch, SessionToken, serve_request};
use pulp_host::drivers::sdcard::SdStorage;
use pulp_host::storage::VirtualStorage;

struct Socket {
    input: Vec<u8>,
    position: usize,
    output: Vec<u8>,
}
impl ErrorType for Socket {
    type Error = core::convert::Infallible;
}
impl Read for Socket {
    async fn read(&mut self, bytes: &mut [u8]) -> Result<usize, Self::Error> {
        let n = bytes.len().min(self.input.len() - self.position);
        bytes[..n].copy_from_slice(&self.input[self.position..self.position + n]);
        self.position += n;
        Ok(n)
    }
}
impl Write for Socket {
    async fn write(&mut self, bytes: &[u8]) -> Result<usize, Self::Error> {
        self.output.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    async fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
fn request(sd: &SdStorage, method: &str, path: &str, header: &str, body: &str) -> Vec<u8> {
    let mut socket = Socket {
        input: format!("{method} {path} HTTP/1.1\r\nHost: pulp.local\r\n{header}Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes(),
        position: 0,
        output: Vec::new(),
    };
    let mut scratch = HttpScratch::EMPTY;
    block_on(serve_request(
        &mut socket,
        sd,
        &mut scratch,
        &mut [0; 4096],
        &SessionToken::from_bytes([0; 16]),
    ));
    socket.output
}
#[test]
fn missing_authorization_rejects_all_device_data_endpoints() {
    let sd = SdStorage::new(VirtualStorage::memory_with(&[("BOOK.TXT", b"original")]));
    for (method, path, body) in [
        ("GET", "/files", ""),
        ("POST", "/delete", "BOOK.TXT"),
        ("POST", "/time", "2026-10-14T12:00:00"),
        ("POST", "/upload", ""),
    ] {
        let response = request(&sd, method, path, "", body);
        assert!(
            response.starts_with(b"HTTP/1.0 403 Forbidden\r\n"),
            "{path}: {}",
            String::from_utf8_lossy(&response)
        );
    }
    assert_eq!(sd.card.file_size("BOOK.TXT").unwrap(), 8);
    assert_eq!(sd.local_time(), None);
}
#[test]
fn wrong_or_ambiguous_authorization_cannot_delete() {
    for header in [
        "X-Pulp-Token: wrong\r\n",
        "X-Pulp-Token: 00000000000000000000000000000000\r\nX-Pulp-Token: wrong\r\n",
    ] {
        let sd = SdStorage::new(VirtualStorage::memory_with(&[("BOOK.TXT", b"original")]));
        let response = request(&sd, "POST", "/delete", header, "BOOK.TXT");
        assert!(response.starts_with(b"HTTP/1.0 403 Forbidden\r\n"));
        assert_eq!(sd.card.file_size("BOOK.TXT").unwrap(), 8);
    }
}
#[test]
fn landing_page_is_public_and_does_not_allow_cross_origin_listing() {
    let sd = SdStorage::new(VirtualStorage::memory());
    assert!(request(&sd, "GET", "/", "", "").starts_with(b"HTTP/1.0 200 OK\r\n"));
    let response = request(&sd, "GET", "/files", "", "");
    assert!(!String::from_utf8_lossy(&response).contains("Access-Control-Allow-Origin: *"));
}

#[test]
fn current_token_accepts_case_insensitive_header_and_optional_whitespace() {
    let sd = SdStorage::new(VirtualStorage::memory_with(&[("BOOK.TXT", b"original")]));
    let response = request(
        &sd,
        "GET",
        "/files",
        "x-pulp-token:\t00000000000000000000000000000000 \r\n",
        "",
    );
    assert!(response.starts_with(b"HTTP/1.0 200 OK\r\n"));
    assert!(String::from_utf8_lossy(&response).contains("BOOK.TXT"));
    let response = request(
        &sd,
        "POST",
        "/delete",
        "X-Pulp-Token: 00000000000000000000000000000000\r\n",
        "BOOK.TXT",
    );
    assert!(response.starts_with(b"HTTP/1.0 200 OK\r\n"));
    assert!(sd.card.file_size("BOOK.TXT").is_err());
}

#[test]
fn previous_session_token_and_token_in_query_are_rejected() {
    let sd = SdStorage::new(VirtualStorage::memory_with(&[("BOOK.TXT", b"original")]));
    let previous = pulp_host::apps::upload_http::SessionToken::from_bytes([1; 16]);
    let response = request(
        &sd,
        "POST",
        "/delete",
        &format!("X-Pulp-Token: {}\r\n", previous.as_str()),
        "BOOK.TXT",
    );
    assert!(response.starts_with(b"HTTP/1.0 403 Forbidden\r\n"));
    let response = request(
        &sd,
        "GET",
        "/files?token=00000000000000000000000000000000",
        "",
        "",
    );
    assert!(response.starts_with(b"HTTP/1.0 403 Forbidden\r\n"));
    assert_eq!(sd.card.file_size("BOOK.TXT").unwrap(), 8);
}

#[test]
fn longest_ipv4_session_url_fits_the_device_qr_encoder() {
    let token = SessionToken::from_bytes([0xff; 16]);
    let url = format!("http://255.255.255.255/#{}", token.as_str());
    assert_eq!(url.len(), 56);
    let qr = pulp_host::ui::QrSymbol::encode(&url).expect("authorized URL must fit");
    assert_eq!(qr.side(), 33);
}
