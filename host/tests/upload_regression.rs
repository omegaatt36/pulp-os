// Regression tests for the HTTP service of the upload app: listing
// failures, directories larger than the 64-entry list buffer, delete path
// safety, the two multipart rejection limits, and byte-exact large uploads.
//
// Expected values come only from these sources, named at each test:
//   contract   the upload service contract
//   client     assets/upload.html (the browser side of this API)
//   decision   the user decision of 2026-10-07: GET /files answers a non-2xx
//              status (500) when the SD card is not mounted or listing fails,
//              never a success page, never a file name, never a success event
//   limits     boundary <= 120 bytes, part headers <= 2048 bytes
// No expected value is copied from an implementation output.
//
// Every request runs on its own worker thread under a wall-clock guard, so a
// regression that never returns fails the test instead of hanging the suite.
// The fake socket keeps the connection open (read stays pending) once the
// request bytes are exhausted, like a browser waiting for the response.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration as StdDuration;

use embassy_futures::block_on;
use embedded_io_async::{ErrorKind as IoErrorKind, ErrorType, Read, Write};
use pulp_host::ErrorKind;
use pulp_host::apps::upload_http::{ServerEvent, serve_request};
use pulp_host::dir_entry::DirEntry;
use pulp_host::drivers::sdcard::SdStorage;
use pulp_host::storage::{StorageOp, VirtualStorage};

const GUARD: StdDuration = StdDuration::from_secs(120);
const WHOLE: usize = usize::MAX;
const MIB: usize = 1024 * 1024;
// the card sits deep inside a sandbox so "../" escapes stay observable
const SANDBOX_SD: &str = "d1/d2/d3/d4/d5/sd";
// the list buffer of the existing service holds 64 entries (upload_http.rs)
const LIST_BUF: usize = 64;

// ------------------------------------------------------------ fake socket

#[derive(Debug)]
struct SockError;

impl std::fmt::Display for SockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("fake socket failure")
    }
}

impl std::error::Error for SockError {}

impl embedded_io_async::Error for SockError {
    fn kind(&self) -> IoErrorKind {
        IoErrorKind::ConnectionReset
    }
}

struct FakeSocket {
    input: Vec<u8>,
    pos: usize,
    read_slice: usize,
    peer_eof: bool,
    out: Vec<u8>,
    flushes: usize,
}

impl ErrorType for FakeSocket {
    type Error = SockError;
}

impl Read for FakeSocket {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, SockError> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.pos >= self.input.len() {
            if self.peer_eof {
                return Ok(0);
            }
            return core::future::pending::<Result<usize, SockError>>().await;
        }
        let n = buf
            .len()
            .min(self.read_slice)
            .min(self.input.len() - self.pos);
        buf[..n].copy_from_slice(&self.input[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

impl Write for FakeSocket {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, SockError> {
        self.out.extend_from_slice(buf);
        Ok(buf.len())
    }

    async fn flush(&mut self) -> Result<(), SockError> {
        self.flushes += 1;
        Ok(())
    }
}

// ------------------------------------------------------------ scenario runner

#[derive(Clone)]
struct Scenario {
    // root files of the card
    files: Vec<(String, Vec<u8>)>,
    // host_dir only: extra files relative to the card directory (may live in a
    // subdirectory or outside the card through "../")
    extra: Vec<(String, Vec<u8>)>,
    input: Vec<u8>,
    slice: usize,
    peer_eof: bool,
    unmounted: bool,
    injections: Vec<(StorageOp, String, usize, ErrorKind)>,
    watch: Vec<String>,
    host_dir: bool,
}

impl Scenario {
    fn new(input: Vec<u8>) -> Self {
        Self {
            files: Vec::new(),
            extra: Vec::new(),
            input,
            slice: WHOLE,
            peer_eof: false,
            unmounted: false,
            injections: Vec::new(),
            watch: Vec::new(),
            host_dir: false,
        }
    }

    fn slice(mut self, n: usize) -> Self {
        self.slice = n;
        self
    }

    fn peer_eof(mut self) -> Self {
        self.peer_eof = true;
        self
    }

    fn seed(mut self, name: &str, data: &[u8]) -> Self {
        self.files.push((name.to_string(), data.to_vec()));
        self.watch(name)
    }

    fn extra(mut self, rel: &str, data: &[u8]) -> Self {
        self.extra.push((rel.to_string(), data.to_vec()));
        self
    }

    fn watch(mut self, name: &str) -> Self {
        if !self.watch.iter().any(|w| w == name) {
            self.watch.push(name.to_string());
        }
        self
    }

    fn unmounted(mut self) -> Self {
        self.unmounted = true;
        self
    }

    fn inject(mut self, op: StorageOp, name: &str, nth: usize, kind: ErrorKind) -> Self {
        self.injections.push((op, name.to_string(), nth, kind));
        self
    }

    // the card is a real directory inside a sandbox, so escapes are observable
    fn host_dir(mut self) -> Self {
        self.host_dir = true;
        self
    }
}

type Tree = Vec<(String, Vec<u8>)>;

struct Outcome {
    response: Vec<u8>,
    event: ServerEvent,
    flushes: usize,
    // watched root names, in watch order; None = file absent
    files: Vec<(String, Option<Vec<u8>>)>,
    pending_injections: usize,
    // host_dir only: every path under the sandbox ("dir/" for directories),
    // before the request and after it
    tree_before: Tree,
    tree_after: Tree,
}

impl Outcome {
    fn file(&self, name: &str) -> Option<&[u8]> {
        self.files
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("{name} was not watched"))
            .1
            .as_deref()
    }
}

fn guarded<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(GUARD) {
        Ok(v) => {
            let _ = worker.join();
            v
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("serve_request did not return within {GUARD:?}")
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => match worker.join() {
            Err(panic) => std::panic::resume_unwind(panic),
            Ok(()) => unreachable!("worker ended without a result"),
        },
    }
}

fn run(s: Scenario) -> Outcome {
    guarded(move || run_inner(s))
}

fn read_back(card: &VirtualStorage, name: &str) -> Option<Vec<u8>> {
    let size = card.file_size(name).ok()? as usize;
    let mut buf = vec![0u8; size];
    let mut got = 0;
    while got < size {
        let n = card
            .read_file_chunk(name, got as u32, &mut buf[got..])
            .ok()?;
        if n == 0 {
            break;
        }
        got += n;
    }
    assert_eq!(got, size, "{name}: card returned fewer bytes than its size");
    Some(buf)
}

fn sandbox_root() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "pulp-upload-regression-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ))
}

fn walk(base: &Path, dir: &Path, out: &mut Tree) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let rel = path
            .strip_prefix(base)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if path.is_dir() {
            out.push((format!("{rel}/"), Vec::new()));
            walk(base, &path, out);
        } else {
            out.push((rel, std::fs::read(&path).unwrap()));
        }
    }
}

fn snapshot(sandbox: &Path) -> Tree {
    let mut tree = Vec::new();
    walk(sandbox, sandbox, &mut tree);
    tree.sort();
    tree
}

fn write_host_file(path: &Path, data: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, data).unwrap();
}

fn run_inner(s: Scenario) -> Outcome {
    let sandbox = sandbox_root();
    let card = if s.host_dir {
        let sd_dir = sandbox.join(SANDBOX_SD);
        std::fs::create_dir_all(&sd_dir).unwrap();
        for (name, data) in &s.files {
            write_host_file(&sd_dir.join(name), data);
        }
        for (rel, data) in &s.extra {
            write_host_file(&sd_dir.join(rel), data);
        }
        VirtualStorage::host_dir(&sd_dir)
    } else {
        assert!(s.extra.is_empty(), "test bug: extra files need host_dir");
        let seeds: Vec<(&str, &[u8])> = s
            .files
            .iter()
            .map(|(n, d)| (n.as_str(), d.as_slice()))
            .collect();
        VirtualStorage::memory_with(&seeds)
    };
    let mut sd = SdStorage::new(card);
    for (op, name, nth, kind) in &s.injections {
        sd.card.inject_error(*op, name, *nth, *kind);
    }
    if s.unmounted {
        sd.set_mounted(false);
    }
    let tree_before = if s.host_dir {
        snapshot(&sandbox)
    } else {
        Vec::new()
    };
    let mut sock = FakeSocket {
        input: s.input.clone(),
        pos: 0,
        read_slice: s.slice.max(1),
        peer_eof: s.peer_eof,
        out: Vec::new(),
        flushes: 0,
    };
    let event = block_on(serve_request(&mut sock, &sd));
    let files = s
        .watch
        .iter()
        .map(|n| (n.clone(), read_back(&sd.card, n)))
        .collect();
    let tree_after = if s.host_dir {
        let t = snapshot(&sandbox);
        let _ = std::fs::remove_dir_all(&sandbox);
        t
    } else {
        Vec::new()
    };
    Outcome {
        response: sock.out,
        event,
        flushes: sock.flushes,
        files,
        pending_injections: sd.card.pending_injections(),
        tree_before,
        tree_after,
    }
}

// ------------------------------------------------------------ requests

fn get(path: &str) -> Vec<u8> {
    format!(
        "GET {path} HTTP/1.1\r\nHost: 192.168.4.1\r\nUser-Agent: test\r\nAccept: */*\r\nConnection: keep-alive\r\n\r\n"
    )
    .into_bytes()
}

fn http_post(path: &str, content_type: Option<&str>, declared_len: usize, body: &[u8]) -> Vec<u8> {
    let mut req =
        format!("POST {path} HTTP/1.1\r\nHost: 192.168.4.1\r\nConnection: keep-alive\r\n");
    if let Some(ct) = content_type {
        req.push_str(&format!("Content-Type: {ct}\r\n"));
    }
    req.push_str(&format!("Content-Length: {declared_len}\r\n\r\n"));
    let mut out = req.into_bytes();
    out.extend_from_slice(body);
    out
}

// what upload.html sends: x.send(name) with Content-Type text/plain
fn delete_request(name: &[u8]) -> Vec<u8> {
    http_post("/delete", Some("text/plain"), name.len(), name)
}

// what a browser XHR sends for FormData with one file field named "file";
// `extra_header` is an additional part header line (without CRLF)
fn multipart_body(boundary: &str, filename: &str, extra_header: &str, content: &[u8]) -> Vec<u8> {
    let extra = if extra_header.is_empty() {
        String::new()
    } else {
        format!("{extra_header}\r\n")
    };
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n{extra}Content-Type: application/octet-stream\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(content);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

fn upload_request_with(
    boundary: &str,
    filename: &str,
    extra_header: &str,
    content: &[u8],
) -> Vec<u8> {
    let body = multipart_body(boundary, filename, extra_header, content);
    http_post(
        "/upload",
        Some(&format!("multipart/form-data; boundary={boundary}")),
        body.len(),
        &body,
    )
}

fn upload_request(boundary: &str, filename: &str, content: &[u8]) -> Vec<u8> {
    upload_request_with(boundary, filename, "", content)
}

// ------------------------------------------------------------ content generators

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

fn lcg_bytes(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as u8
        })
        .collect()
}

// every one of the 256 byte values, cycling
fn all_byte_values(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 256) as u8).collect()
}

fn make_boundary(len: usize) -> String {
    if len == 1 {
        return "b".to_string();
    }
    let alnum = "7MA4YWxkTrZu0gW9Qe2Vn5Lz";
    let dashes = len / 2;
    let mut b = "-".repeat(dashes);
    b.extend(alnum.chars().cycle().take(len - dashes));
    assert_eq!(b.len(), len);
    b
}

// a multipart body is only well formed when the content holds no CRLF--boundary
fn assert_well_formed(boundary: &str, content: &[u8]) {
    let delimiter = format!("\r\n--{boundary}");
    let mut probe = b"\r\n".to_vec();
    probe.extend_from_slice(content);
    probe.extend_from_slice(delimiter.as_bytes());
    assert_eq!(
        find(&probe, delimiter.as_bytes()),
        Some(content.len() + 2),
        "test bug: content contains the delimiter of boundary {boundary:?}"
    );
}

// ------------------------------------------------------------ response helpers

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(120)]).into_owned()
}

fn status_line(raw: &[u8]) -> String {
    let end = raw
        .iter()
        .position(|&b| b == b'\r' || b == b'\n')
        .unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

fn status_code(raw: &[u8]) -> Option<u16> {
    let line = status_line(raw);
    if !line.starts_with("HTTP/1.") {
        return None;
    }
    line.split(' ').nth(1)?.parse().ok()
}

fn split_response(raw: &[u8]) -> (&[u8], &[u8]) {
    match find(raw, b"\r\n\r\n") {
        Some(i) => (&raw[..i], &raw[i + 4..]),
        None => (raw, &[]),
    }
}

fn header_value(raw: &[u8], name: &str) -> Option<String> {
    let (head, _) = split_response(raw);
    let head = String::from_utf8_lossy(head).into_owned();
    head.split("\r\n").skip(1).find_map(|line| {
        let (k, v) = line.split_once(':')?;
        k.trim()
            .eq_ignore_ascii_case(name)
            .then(|| v.trim().to_string())
    })
}

fn assert_bytes_eq(actual: &[u8], expected: &[u8], ctx: &str) {
    if actual == expected {
        return;
    }
    let at = actual
        .iter()
        .zip(expected)
        .position(|(a, b)| a != b)
        .unwrap_or(actual.len().min(expected.len()));
    panic!(
        "{ctx}: bytes differ (actual {} bytes, expected {} bytes, first difference at {at})",
        actual.len(),
        expected.len()
    );
}

// a failure is a non-2xx answer, never "200 OK"; the peer is waiting, so
// a status line is required and the response must be flushed
fn failure_problem(out: &Outcome) -> Option<String> {
    let r = &out.response;
    if find(r, b"200 OK").is_some() {
        return Some(format!(
            "failure response contains \"200 OK\": {:?}",
            lossy(r)
        ));
    }
    if split_response(r).1 == b"OK" {
        return Some("failure body is \"OK\"".to_string());
    }
    match status_code(r) {
        None => return Some(format!("no HTTP status line in {:?}", lossy(r))),
        Some(code) if code < 400 => return Some(format!("failure answered with status {code}")),
        Some(_) => {}
    }
    if out.flushes < 1 {
        return Some("response was not flushed".to_string());
    }
    None
}

fn assert_failure(out: &Outcome, ctx: &str) {
    if let Some(p) = failure_problem(out) {
        panic!("{ctx}: {p}");
    }
}

fn assert_200(out: &Outcome, content_type: Option<&str>, ctx: &str) -> Vec<u8> {
    assert_eq!(
        status_line(&out.response),
        "HTTP/1.0 200 OK",
        "{ctx}: status line"
    );
    if let Some(ct) = content_type {
        let v = header_value(&out.response, "Content-Type")
            .unwrap_or_else(|| panic!("{ctx}: no Content-Type header"));
        assert!(
            v.to_ascii_lowercase().starts_with(ct),
            "{ctx}: Content-Type {v:?}, expected {ct}"
        );
    }
    let (_, body) = split_response(&out.response);
    if let Some(cl) = header_value(&out.response, "Content-Length") {
        assert_eq!(
            cl.parse::<usize>().ok(),
            Some(body.len()),
            "{ctx}: Content-Length does not match the body"
        );
    }
    assert!(out.flushes >= 1, "{ctx}: response was not flushed");
    body.to_vec()
}

fn name_of(name: &[u8; 13], len: u8) -> &[u8] {
    assert!(
        len as usize <= 13,
        "name_len {len} exceeds the 13-byte slot"
    );
    &name[..len as usize]
}

fn assert_uploaded(out: &Outcome, name: &str, ctx: &str) {
    match out.event {
        ServerEvent::Uploaded {
            name: n,
            name_len: l,
        } => {
            assert_eq!(name_of(&n, l), name.as_bytes(), "{ctx}: uploaded name")
        }
        other => panic!("{ctx}: expected Uploaded({name}), got {other:?}"),
    }
}

// ------------------------------------------------------------ JSON (strict, for the listing)

struct Json<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Json<'a> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\r' | b'\n') {
            self.i += 1;
        }
    }

    fn eat(&mut self, b: u8) -> Result<(), String> {
        self.ws();
        if self.s.get(self.i) == Some(&b) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected {:?} at {}", b as char, self.i))
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out = Vec::new();
        loop {
            let b = *self.s.get(self.i).ok_or("unterminated string")?;
            self.i += 1;
            match b {
                b'"' => break,
                0..=0x1f => return Err(format!("raw control byte {b:#x} in string")),
                b'\\' => {
                    let e = *self.s.get(self.i).ok_or("dangling escape")?;
                    self.i += 1;
                    match e {
                        b'"' | b'\\' | b'/' => out.push(e),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let hex = self.s.get(self.i..self.i + 4).ok_or("short \\u escape")?;
                            let hex = std::str::from_utf8(hex).map_err(|e| e.to_string())?;
                            let cp = u32::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
                            self.i += 4;
                            let c = char::from_u32(cp).ok_or("lone surrogate")?;
                            let mut tmp = [0u8; 4];
                            out.extend_from_slice(c.encode_utf8(&mut tmp).as_bytes());
                        }
                        _ => return Err(format!("bad escape \\{}", e as char)),
                    }
                }
                _ => out.push(b),
            }
        }
        String::from_utf8(out).map_err(|e| e.to_string())
    }

    fn number(&mut self) -> Result<u64, String> {
        self.ws();
        let start = self.i;
        while self.s.get(self.i).is_some_and(|b| b.is_ascii_digit()) {
            self.i += 1;
        }
        let digits = &self.s[start..self.i];
        if digits.is_empty() || (digits.len() > 1 && digits[0] == b'0') {
            return Err(format!("bad number at {start}"));
        }
        std::str::from_utf8(digits)
            .unwrap()
            .parse()
            .map_err(|e: std::num::ParseIntError| e.to_string())
    }

    fn entry(&mut self) -> Result<(String, u64), String> {
        self.eat(b'{')?;
        let (mut name, mut size) = (None, None);
        loop {
            let key = self.string()?;
            self.eat(b':')?;
            match key.as_str() {
                "name" if name.is_none() => name = Some(self.string()?),
                "size" if size.is_none() => size = Some(self.number()?),
                _ => return Err(format!("unexpected or repeated key {key:?}")),
            }
            self.ws();
            if self.s.get(self.i) == Some(&b',') {
                self.i += 1;
            } else {
                break;
            }
        }
        self.eat(b'}')?;
        Ok((
            name.ok_or("entry without name")?,
            size.ok_or("entry without size")?,
        ))
    }
}

// body must be exactly one JSON array of {"name":string,"size":number}
fn parse_listing(body: &[u8]) -> Result<Vec<(String, u64)>, String> {
    let mut p = Json { s: body, i: 0 };
    p.eat(b'[')?;
    let mut items = Vec::new();
    p.ws();
    if p.s.get(p.i) == Some(&b']') {
        p.i += 1;
    } else {
        loop {
            items.push(p.entry()?);
            p.ws();
            match p.s.get(p.i) {
                Some(b',') => p.i += 1,
                Some(b']') => {
                    p.i += 1;
                    break;
                }
                _ => return Err(format!("expected , or ] at {}", p.i)),
            }
        }
    }
    p.ws();
    if p.i != body.len() {
        return Err(format!("trailing bytes after the array at {}", p.i));
    }
    Ok(items)
}

// ============================================================ decision: GET /files failures

const LIST_FILES: [(&str, &[u8]); 3] = [
    ("BOOK.TXT", b"book bytes"),
    ("NOTES.MD", b"notes"),
    ("SECRET.TXT", b"s"),
];

fn with_list_files(mut s: Scenario) -> Scenario {
    for (n, d) in LIST_FILES {
        s = s.seed(n, d);
    }
    s
}

// decision: a failed listing is a non-2xx answer without any file name, and no
// upload / delete success event. `nothing` includes Nothing as the only event.
fn assert_list_refused(out: &Outcome, ctx: &str) {
    assert_failure(out, ctx);
    for (name, _) in LIST_FILES {
        assert!(
            find(&out.response, name.as_bytes()).is_none(),
            "{ctx}: response leaks file name {name}: {:?}",
            lossy(&out.response)
        );
    }
    assert_eq!(out.event, ServerEvent::Nothing, "{ctx}: event");
    for (name, data) in LIST_FILES {
        assert_eq!(
            out.file(name),
            Some(data),
            "{ctx}: card file {name} changed"
        );
    }
}

// decision: injected listing failure -> non-2xx, no names, event Nothing.
// The injection must have fired, so the failure is the injected one.
#[test]
fn get_files_with_a_failing_listing_is_refused() {
    for kind in [ErrorKind::OpenDir, ErrorKind::ReadFailed] {
        for slice in [1, WHOLE] {
            for path in ["/files", "/files?x=1"] {
                let ctx = format!("list injection {kind:?} slice {slice} GET {path}");
                let out = run(with_list_files(Scenario::new(get(path)))
                    .slice(slice)
                    .inject(StorageOp::List, "", 1, kind));
                assert_eq!(out.pending_injections, 0, "{ctx}: injection never fired");
                assert_list_refused(&out, &ctx);
            }
        }
    }
}

// decision: SD not mounted -> same refusal; seeded files must not leak
#[test]
fn get_files_without_a_mounted_card_is_refused() {
    for slice in [1, WHOLE] {
        for path in ["/files", "/files?x=1"] {
            let ctx = format!("unmounted slice {slice} GET {path}");
            let out = run(with_list_files(Scenario::new(get(path)))
                .slice(slice)
                .unmounted());
            assert_list_refused(&out, &ctx);
        }
    }
}

// guard against over-correction: a healthy card still lists with 200 and JSON
// (client: upload.html reads n.name / n.size of a JSON array on status 200).
// Same scenario shape as the failing cases, minus the failure.
#[test]
fn get_files_on_a_healthy_card_still_answers_200_json() {
    for slice in [1, WHOLE] {
        let ctx = format!("healthy slice {slice}");
        let out = run(with_list_files(Scenario::new(get("/files"))).slice(slice));
        let body = assert_200(&out, Some("application/json"), &ctx);
        assert_eq!(out.event, ServerEvent::Nothing, "{ctx}");
        let mut listed = parse_listing(&body).unwrap_or_else(|e| panic!("{ctx}: {e}"));
        listed.sort();
        let mut expected: Vec<(String, u64)> = LIST_FILES
            .iter()
            .map(|(n, d)| (n.to_string(), d.len() as u64))
            .collect();
        expected.sort();
        assert_eq!(listed, expected, "{ctx}: listing");
    }
}

// an injection that does not match the listing must not disturb it (the List
// op class is the one that fires): a failing write injection on a file name is
// irrelevant to GET /files, which stays a success
#[test]
fn unrelated_storage_injection_does_not_refuse_listing() {
    let out = run(with_list_files(Scenario::new(get("/files"))).inject(
        StorageOp::Write,
        "BOOK.TXT",
        1,
        ErrorKind::WriteFailed,
    ));
    assert_eq!(
        out.pending_injections, 1,
        "a write injection fired during a listing"
    );
    let body = assert_200(&out, Some("application/json"), "unrelated injection");
    assert_eq!(parse_listing(&body).expect("valid listing").len(), 3);
}

// ============================================================ more than 64 entries

// client: GET /files answers a JSON array of {name,size}. The list
// buffer of the service is 64 entries; whether entries 65.. are cut is not
// decided by any requirement, so only validity, no panic, 200 and the first
// 64 entries (order of list_root_files, per the contract) are asserted.
#[test]
fn get_files_with_seventy_files_is_a_valid_listing() {
    let names: Vec<String> = (0..70).map(|i| format!("F{i:02}.TXT")).collect();
    let datas: Vec<Vec<u8>> = (0..70).map(|i| lcg_bytes(i + 1, i as u64 + 500)).collect();
    let seeds: Vec<(&str, &[u8])> = names
        .iter()
        .zip(&datas)
        .map(|(n, d)| (n.as_str(), d.as_slice()))
        .collect();

    // oracle: the card's own listing order, truncated at the 64-entry buffer
    let card = VirtualStorage::memory_with(&seeds);
    let mut buf = [DirEntry::EMPTY; LIST_BUF];
    let n = card.list_root_files(&mut buf).unwrap();
    assert!(n <= LIST_BUF);
    let oracle: Vec<(String, u64)> = buf[..n]
        .iter()
        .map(|e| {
            (
                std::str::from_utf8(&e.name[..e.name_len as usize])
                    .unwrap()
                    .to_string(),
                e.size as u64,
            )
        })
        .collect();
    assert_eq!(
        oracle.len(),
        LIST_BUF,
        "test bug: expected a full 64-entry buffer"
    );

    for slice in [1460, WHOLE] {
        let ctx = format!("70 files slice {slice}");
        let mut s = Scenario::new(get("/files")).slice(slice);
        for (n, d) in &seeds {
            s = s.seed(n, d);
        }
        let out = run(s);
        let body = assert_200(&out, Some("application/json"), &ctx);
        assert_eq!(out.event, ServerEvent::Nothing, "{ctx}: event");
        let listed = parse_listing(&body).unwrap_or_else(|e| panic!("{ctx}: invalid JSON: {e}"));
        assert!(
            listed.len() >= LIST_BUF,
            "{ctx}: only {} entries",
            listed.len()
        );
        assert!(
            listed.len() <= 70,
            "{ctx}: {} entries from 70 files",
            listed.len()
        );
        assert_eq!(&listed[..LIST_BUF], &oracle[..], "{ctx}: first 64 entries");
        // every entry is a real file with its real size, none twice
        for (name, size) in &listed {
            let i = names
                .iter()
                .position(|n| n == name)
                .unwrap_or_else(|| panic!("{ctx}: listed {name:?}, which is not on the card"));
            assert_eq!(*size, datas[i].len() as u64, "{ctx}: size of {name}");
        }
        let mut sorted: Vec<&String> = listed.iter().map(|(n, _)| n).collect();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), listed.len(), "{ctx}: duplicate entries");
    }
}

// ============================================================ delete path safety

// Names whose delete must fail. Three groups, so a failure says which kind of
// name got through. Every file in the sandbox, inside or outside the card,
// must survive each of them.

// brief: targets outside the SD root ("..", "../X") and spellings that come
// back into it through ".."
fn escaping_delete_names() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("dot dot", b"..".to_vec()),
        ("parent escape", b"../DECOY.TXT".to_vec()),
        ("two level escape", b"../../DECOY2.TXT".to_vec()),
        ("escape back into the card", b"../sd/KEEP.TXT".to_vec()),
        ("dot dot inside", b"A/../KEEP.TXT".to_vec()),
        ("backslash escape", b"..\\DECOY.TXT".to_vec()),
    ]
}

// brief: any name containing a path separator is not a plain root file name,
// even when it points at an existing root file
fn separator_delete_names() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("slash prefix of a root file", b"/KEEP.TXT".to_vec()),
        ("slash suffix of a root file", b"KEEP.TXT/".to_vec()),
        ("dot slash root file", b"./KEEP.TXT".to_vec()),
        ("subdirectory file", b"SUB/INNER.TXT".to_vec()),
        ("backslash prefix of a root file", b"\\KEEP.TXT".to_vec()),
        ("backslash subdirectory", b"SUB\\INNER.TXT".to_vec()),
    ]
}

// brief: empty, "." and over-long names (a 13 byte name that exists on the
// card included, as in upload_http.rs)
fn malformed_delete_names() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("single dot", b".".to_vec()),
        ("empty", b"".to_vec()),
        ("13 bytes", b"ABCDEFGH.TXTX".to_vec()),
        ("14 bytes", b"ABCDEFGH.TXTXY".to_vec()),
        ("100 bytes", vec![b'A'; 100]),
        ("1000 bytes", vec![b'B'; 1000]),
    ]
}

fn delete_sandbox(req: Vec<u8>) -> Scenario {
    Scenario::new(req)
        .host_dir()
        .seed("KEEP.TXT", b"keep me")
        .seed("OTHER.TXT", b"other")
        .seed("ABCDEFGH.TXTX", b"thirteen")
        .extra("SUB/INNER.TXT", b"inner")
        .extra("../DECOY.TXT", b"decoy one")
        .extra("../../DECOY2.TXT", b"decoy two")
}

// a delete request that names anything but a plain root file fails (>= 400,
// DeleteFailed) and removes nothing: the whole sandbox tree is unchanged.
// Every offending name is collected instead of stopping at the first.
fn assert_delete_names_refused(names: Vec<(&'static str, Vec<u8>)>) {
    let mut problems: Vec<String> = Vec::new();
    for (label, name) in names {
        for slice in [1, WHOLE] {
            let ctx = format!("delete {label} ({:?}) slice {slice}", lossy(&name));
            let out = run(delete_sandbox(delete_request(&name)).slice(slice));
            assert!(!out.tree_before.is_empty(), "{ctx}: empty sandbox");
            if let Some(p) = failure_problem(&out) {
                problems.push(format!("{ctx}: {p}"));
            }
            if out.event != ServerEvent::DeleteFailed {
                problems.push(format!("{ctx}: event {:?}", out.event));
            }
            if out.tree_after != out.tree_before {
                let gone: Vec<&str> = out
                    .tree_before
                    .iter()
                    .filter(|e| !out.tree_after.contains(e))
                    .map(|(p, _)| p.as_str())
                    .collect();
                let new: Vec<&str> = out
                    .tree_after
                    .iter()
                    .filter(|e| !out.tree_before.contains(e))
                    .map(|(p, _)| p.as_str())
                    .collect();
                problems.push(format!(
                    "{ctx}: sandbox changed, removed {gone:?}, added {new:?}"
                ));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "{} problems:\n{}",
        problems.len(),
        problems.join("\n")
    );
}

// brief: a delete can never reach a file outside the SD root
#[test]
fn delete_never_reaches_outside_the_card_root() {
    assert_delete_names_refused(escaping_delete_names());
}

// brief: names with a path separator are refused, not normalized
#[test]
fn delete_refuses_names_with_path_separators() {
    assert_delete_names_refused(separator_delete_names());
}

// brief: empty, "." and over-long names are refused
#[test]
fn delete_refuses_empty_dot_and_over_long_names() {
    assert_delete_names_refused(malformed_delete_names());
}

// the same sandbox must allow a plain delete, so "nothing was deleted" above
// is the service refusing the name, not a card that cannot delete
#[test]
fn delete_sandbox_control_removes_a_plain_root_file() {
    for slice in [1, WHOLE] {
        let ctx = format!("control slice {slice}");
        let out = run(delete_sandbox(delete_request(b"KEEP.TXT")).slice(slice));
        let body = assert_200(&out, None, &ctx);
        assert_eq!(body, b"OK", "{ctx}: body");
        match out.event {
            ServerEvent::Deleted { name, name_len } => {
                assert_eq!(name_of(&name, name_len), b"KEEP.TXT", "{ctx}")
            }
            other => panic!("{ctx}: expected Deleted, got {other:?}"),
        }
        let gone = format!("{SANDBOX_SD}/KEEP.TXT");
        assert!(
            out.tree_before.iter().any(|(p, _)| *p == gone),
            "{ctx}: test bug"
        );
        let expected: Tree = out
            .tree_before
            .iter()
            .filter(|(p, _)| *p != gone)
            .cloned()
            .collect();
        assert_eq!(
            out.tree_after, expected,
            "{ctx}: only KEEP.TXT may disappear"
        );
    }
}

// ============================================================ upload rejection limits

fn assert_upload_refused(out: &Outcome, ctx: &str) {
    assert_failure(out, ctx);
    assert_eq!(out.event, ServerEvent::UploadFailed, "{ctx}: event");
    // nothing was written: no new file, no changed file, anywhere
    assert!(!out.tree_before.is_empty(), "{ctx}: empty sandbox");
    assert_eq!(out.tree_after, out.tree_before, "{ctx}: sandbox changed");
}

fn upload_sandbox(req: Vec<u8>) -> Scenario {
    Scenario::new(req)
        .host_dir()
        .seed("OTHER.TXT", b"other")
        .seed("UP.TXT", b"previous content")
        .watch("UP.TXT")
}

// brief: a multipart boundary over 120 bytes is rejected (>= 400, UploadFailed)
// and writes nothing, whatever the read slicing. Control: the same request
// shape with a legal RFC 2046 boundary (70 bytes) is accepted.
#[test]
fn upload_with_a_boundary_over_120_bytes_is_refused() {
    let content = lcg_bytes(300, 601);
    for blen in [121usize, 122, 200, 500] {
        let boundary = make_boundary(blen);
        assert_well_formed(&boundary, &content);
        for slice in [1, 1460, WHOLE] {
            let ctx = format!("boundary {blen} bytes slice {slice}");
            let out =
                run(upload_sandbox(upload_request(&boundary, "UP.TXT", &content)).slice(slice));
            assert_upload_refused(&out, &ctx);
            assert_eq!(
                out.file("UP.TXT"),
                Some(&b"previous content"[..]),
                "{ctx}: UP.TXT"
            );
        }
    }
}

#[test]
fn upload_with_a_legal_boundary_control_is_accepted() {
    let content = lcg_bytes(300, 601);
    let boundary = make_boundary(70);
    for slice in [1, 1460, WHOLE] {
        let ctx = format!("control boundary 70 slice {slice}");
        let out = run(upload_sandbox(upload_request(&boundary, "UP.TXT", &content)).slice(slice));
        assert_eq!(assert_200(&out, None, &ctx), b"OK", "{ctx}");
        assert_uploaded(&out, "UP.TXT", &ctx);
        assert_bytes_eq(out.file("UP.TXT").expect("UP.TXT missing"), &content, &ctx);
    }
}

// the extra part header that makes the part headers exceed `total` bytes in
// all (counted from after the boundary line to the blank line)
fn padded_part_header(total_over: usize) -> String {
    format!("X-Pad: {}", "a".repeat(total_over))
}

// brief: part headers over 2048 bytes are rejected (>= 400, UploadFailed, no
// write). Sizes are clearly over the limit: the padding line alone exceeds it.
#[test]
fn upload_with_part_headers_over_2048_bytes_is_refused() {
    let content = lcg_bytes(300, 602);
    let boundary = make_boundary(40);
    for pad in [2100usize, 3000, 10_000] {
        let extra = padded_part_header(pad);
        for slice in [1, 1460, WHOLE] {
            let ctx = format!("part headers padding {pad} slice {slice}");
            let req = upload_request_with(&boundary, "UP.TXT", &extra, &content);
            let out = run(upload_sandbox(req).slice(slice));
            assert_upload_refused(&out, &ctx);
            assert_eq!(
                out.file("UP.TXT"),
                Some(&b"previous content"[..]),
                "{ctx}: UP.TXT"
            );
        }
    }
}

// control: a modest extra part header (well under 2048) is accepted, so the
// refusal above is about the size, not about the extra header's presence
#[test]
fn upload_with_a_small_extra_part_header_control_is_accepted() {
    let content = lcg_bytes(300, 602);
    let boundary = make_boundary(40);
    let extra = padded_part_header(200);
    for slice in [1, 1460, WHOLE] {
        let ctx = format!("control part headers slice {slice}");
        let out = run(
            upload_sandbox(upload_request_with(&boundary, "UP.TXT", &extra, &content)).slice(slice),
        );
        assert_eq!(assert_200(&out, None, &ctx), b"OK", "{ctx}");
        assert_uploaded(&out, "UP.TXT", &ctx);
        assert_bytes_eq(out.file("UP.TXT").expect("UP.TXT missing"), &content, &ctx);
    }
}

// ============================================================ large files

// stored bytes == input bytes for 1 MiB and 3 MiB covering all 256 byte
// values, for several read slicings. The card is read back and compared.
#[test]
fn large_uploads_are_stored_byte_for_byte() {
    let boundary = make_boundary(40);
    for (size, slices) in [
        (MIB, vec![1usize, 1460, WHOLE]),
        (3 * MIB, vec![1460, WHOLE]),
    ] {
        let content = all_byte_values(size);
        assert_well_formed(&boundary, &content);
        for slice in slices {
            let ctx = format!("{size} bytes slice {slice}");
            let out = run(
                Scenario::new(upload_request(&boundary, "BIG.BIN", &content))
                    .slice(slice)
                    .watch("BIG.BIN"),
            );
            assert_eq!(assert_200(&out, None, &ctx), b"OK", "{ctx}");
            assert_uploaded(&out, "BIG.BIN", &ctx);
            let stored = out
                .file("BIG.BIN")
                .unwrap_or_else(|| panic!("{ctx}: file missing"));
            assert_bytes_eq(stored, &content, &format!("{ctx}: stored file"));
        }
    }
}

// same with pseudo-random content (a different byte order than the cycle),
// at the 3 MiB size and an unaligned odd slice
#[test]
fn large_random_upload_is_stored_byte_for_byte() {
    let boundary = make_boundary(70);
    let content = lcg_bytes(3 * MIB + 17, 603);
    assert_well_formed(&boundary, &content);
    for slice in [4093usize, WHOLE] {
        let ctx = format!("random {} bytes slice {slice}", content.len());
        let out = run(
            Scenario::new(upload_request(&boundary, "RAND.BIN", &content))
                .slice(slice)
                .watch("RAND.BIN"),
        );
        assert_eq!(assert_200(&out, None, &ctx), b"OK", "{ctx}");
        assert_uploaded(&out, "RAND.BIN", &ctx);
        let stored = out
            .file("RAND.BIN")
            .unwrap_or_else(|| panic!("{ctx}: file missing"));
        assert_bytes_eq(stored, &content, &format!("{ctx}: stored file"));
    }
}

// The delete contract accepts one plain 8.3 root filename and requires the
// complete Content-Length body before touching storage. Truncating a longer
// body to a valid prefix must never turn an invalid request into a delete.
#[test]
fn delete_rejects_overlong_bodies_without_touching_storage() {
    let mut problems = Vec::new();
    for (label, declared, body, eof) in [
        (
            "suffix beyond filename buffer",
            14,
            &b"BOOK.TXT     X"[..],
            false,
        ),
        (
            "long suffix",
            40,
            &b"BOOK.TXT     XXXXXXXXXXXXXXXXXXXXXXXXXXXX"[..],
            false,
        ),
        (
            "early peer EOF beyond filename buffer",
            14,
            &b"BOOK.TXT     "[..],
            true,
        ),
    ] {
        for slice in [1, 7, WHOLE] {
            for inject in [false, true] {
                let ctx = format!("{label}, slice {slice}, delete injection {inject}");
                let mut scenario =
                    Scenario::new(http_post("/delete", Some("text/plain"), declared, body))
                        .seed("BOOK.TXT", b"original book bytes")
                        .slice(slice);
                if eof {
                    scenario = scenario.peer_eof();
                }
                if inject {
                    scenario =
                        scenario.inject(StorageOp::Delete, "BOOK.TXT", 1, ErrorKind::DeleteFailed);
                }
                let out = run(scenario);
                if let Some(problem) = failure_problem(&out) {
                    problems.push(format!("{ctx}: {problem}"));
                }
                if out.event != ServerEvent::DeleteFailed {
                    problems.push(format!("{ctx}: event {:?}", out.event));
                }
                if out.file("BOOK.TXT") != Some(&b"original book bytes"[..]) {
                    problems.push(format!("{ctx}: original file changed or deleted"));
                }
                if inject && out.pending_injections != 1 {
                    problems.push(format!("{ctx}: storage delete was called"));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

// Failure from the card must remain a failure at the HTTP and event boundary.
#[test]
fn delete_storage_failures_never_report_success() {
    for slice in [1, WHOLE] {
        for unmounted in [false, true] {
            let ctx = format!("delete card failure, slice {slice}, unmounted {unmounted}");
            let mut scenario = Scenario::new(delete_request(b"BOOK.TXT"))
                .seed("BOOK.TXT", b"original book bytes")
                .slice(slice);
            if unmounted {
                scenario = scenario.unmounted();
            } else {
                scenario =
                    scenario.inject(StorageOp::Delete, "BOOK.TXT", 1, ErrorKind::DeleteFailed);
            }
            let out = run(scenario);
            assert_failure(&out, &ctx);
            assert_eq!(out.event, ServerEvent::DeleteFailed, "{ctx}: event");
            assert_eq!(
                out.file("BOOK.TXT"),
                Some(&b"original book bytes"[..]),
                "{ctx}: file"
            );
            if !unmounted {
                assert_eq!(out.pending_injections, 0, "{ctx}: injection did not fire");
            }
        }
    }
}

#[test]
fn delete_rejects_truncated_filename_body() {
    for slice in [1, WHOLE] {
        let ctx = format!("truncated filename, slice {slice}");
        let out = run(
            Scenario::new(http_post("/delete", Some("text/plain"), 8, b"BOOK.TX"))
                .peer_eof()
                .slice(slice)
                .seed("BOOK.TX", b"original bytes")
                .inject(StorageOp::Delete, "BOOK.TX", 1, ErrorKind::DeleteFailed),
        );
        assert_failure(&out, &ctx);
        assert_eq!(out.event, ServerEvent::DeleteFailed, "{ctx}: event");
        assert_eq!(
            out.file("BOOK.TX"),
            Some(&b"original bytes"[..]),
            "{ctx}: file"
        );
        assert_eq!(
            out.pending_injections, 1,
            "{ctx}: storage delete was called"
        );
    }
}

// Independent oracle: pinned embedded-sdmmc ShortFileName::create_from_str
// permits these ASCII characters in short filenames. A filename
// returned by the listing must remain usable by the browser's delete request.
#[test]
fn listed_fat_short_names_with_punctuation_can_be_deleted() {
    let mut problems = Vec::new();
    let mut names: Vec<String> = ['(', ')', '\'', '%', '@', '^', '`', '{', '}']
        .into_iter()
        .map(|punctuation| format!("A{punctuation}.TXT"))
        .collect();
    names.push("A(B).TXT".to_string());
    for name in names {
        for slice in [1, WHOLE] {
            let ctx = format!("FAT name {name:?}, slice {slice}");
            let listing = run(Scenario::new(get("/files"))
                .seed(&name, b"book bytes")
                .seed("OTHER.TXT", b"other")
                .slice(slice));
            let body = assert_200(&listing, Some("application/json"), &ctx);
            let listed = parse_listing(&body).expect("valid listing");
            assert!(
                listed.contains(&(name.clone(), 10)),
                "{ctx}: name absent from listing: {listed:?}"
            );
            let out = run(Scenario::new(delete_request(name.as_bytes()))
                .seed(&name, b"book bytes")
                .seed("OTHER.TXT", b"other")
                .slice(slice));
            if status_code(&out.response) != Some(200) || split_response(&out.response).1 != b"OK" {
                problems.push(format!(
                    "{ctx}: expected 200 OK, got {:?}",
                    lossy(&out.response)
                ));
            }
            match out.event {
                ServerEvent::Deleted {
                    name: deleted,
                    name_len,
                } if name_of(&deleted, name_len) == name.as_bytes() => {}
                other => problems.push(format!("{ctx}: event {other:?}")),
            }
            if out.file(&name).is_some() {
                problems.push(format!("{ctx}: target still exists"));
            }
            if out.file("OTHER.TXT") != Some(&b"other"[..]) {
                problems.push(format!("{ctx}: unrelated file changed"));
            }
            if out.flushes == 0 {
                problems.push(format!("{ctx}: response not flushed"));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn delete_rejects_invalid_fat_short_names_without_normalizing() {
    let names = [
        " KEEP.TXT",
        "KEEP.TXT ",
        "\tKEEP.TXT",
        "KEEP.TXT\t",
        "\nKEEP.TXT",
        "KEEP.TXT\n",
        "KEEP+.TXT",
        "KEEP,.TXT",
        "KEEP:.TXT",
        "KEEP;.TXT",
        "KEEP=.TXT",
        "KEEP?.TXT",
        "KEEP*.TXT",
        "KEEP[.TXT",
        "KEEP].TXT",
        "KEEP|.TXT",
        "KEEP<.TXT",
        "KEEP>.TXT",
        "KEEP\".TXT",
        "KE EP.TXT",
        "KEEP. TX",
        "123456789.TX",
        "KEEP.TXTX",
        "KEEP.T.X",
        "./KEEP.TXT",
        "A/../KEEP.TXT",
        "SUB/KEEP.TXT",
        "SUB\\KEEP.TXT",
        "/KEEP.TXT",
        "../KEEP.TXT",
    ];
    assert_delete_names_refused(
        names
            .into_iter()
            .map(|name| (name, name.as_bytes().to_vec()))
            .collect(),
    );
}

// The upload compatibility contract keeps the original character filtering:
// parentheses are omitted, so expanding delete acceptance must not expand it.
#[test]
fn upload_keeps_existing_parenthesis_sanitization() {
    for slice in [1, WHOLE] {
        let ctx = format!("upload punctuation compatibility, slice {slice}");
        let out = run(
            Scenario::new(upload_request("Boundary123", "A(B).TXT", b"new content"))
                .seed("OTHER.TXT", b"other")
                .watch("AB.TXT")
                .watch("A(B).TXT")
                .slice(slice),
        );
        assert_eq!(assert_200(&out, None, &ctx), b"OK", "{ctx}: body");
        assert_uploaded(&out, "AB.TXT", &ctx);
        assert_eq!(
            out.file("AB.TXT"),
            Some(&b"new content"[..]),
            "{ctx}: stored bytes"
        );
        assert_eq!(
            out.file("A(B).TXT"),
            None,
            "{ctx}: original upload spelling"
        );
        assert_eq!(
            out.file("OTHER.TXT"),
            Some(&b"other"[..]),
            "{ctx}: unrelated file"
        );
    }
}
