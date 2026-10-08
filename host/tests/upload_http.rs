// HTTP service layer of the upload app: GET / and /files, POST /upload
// and /delete over a fake in-memory socket and the virtual SD card.
//
// Expected values follow the upload service contract and the browser client
// in assets/upload.html, named at each test.
// No expected value is copied from an implementation output.
//
// Every request runs on its own worker thread under a wall-clock guard, so a
// regression that never returns fails the test instead of hanging the suite.
// The fake socket keeps the connection open (read stays pending) once the
// request bytes are exhausted, like a browser waiting for the response; the
// peer-closed cases opt into `Ok(0)` or a read error explicitly.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration as StdDuration;

use embassy_futures::block_on;
use embedded_io_async::{ErrorKind as IoErrorKind, ErrorType, Read, Write};
use pulp_host::ErrorKind;
use pulp_host::apps::upload_http::{HttpScratch, ServerEvent, serve_request};
use pulp_host::dir_entry::DirEntry;
use pulp_host::drivers::sdcard::SdStorage;
use pulp_host::storage::{StorageOp, VirtualStorage};

const GUARD: StdDuration = StdDuration::from_secs(60);
const WHOLE: usize = usize::MAX;
const UPLOAD_PAGE: &[u8] = include_bytes!("../../assets/upload.html");
const UPLOAD_SLICES: [usize; 5] = [1, 7, 512, 1460, WHOLE];
// the contract's work-buffer-sensitive sizes plus a multi-buffer one
const KIB100: usize = 100 * 1024;
// boundary lengths named by the brief (RFC 2046 allows 1..=70)
const BOUNDARY_LENS: [usize; 3] = [1, 40, 70];
// decoy placed beside every sandboxed card; depth lets "../" escapes be seen
const SANDBOX_SD: &str = "d1/d2/d3/d4/d5/sd";

// ------------------------------------------------------------ fake socket

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum End {
    // connection stays open, read never completes (a browser awaiting a response)
    Pending,
    // peer closed: read returns Ok(0)
    Zero,
    // connection broke: read returns an error
    Fail,
}

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
    // absolute input offsets no single read may cross
    splits: Vec<usize>,
    end: End,
    write_limit: usize,
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
            return match self.end {
                End::Zero => Ok(0),
                End::Fail => Err(SockError),
                End::Pending => core::future::pending::<Result<usize, SockError>>().await,
            };
        }
        let to_split = self
            .splits
            .iter()
            .find(|&&s| s > self.pos)
            .map_or(usize::MAX, |s| s - self.pos);
        let n = buf
            .len()
            .min(self.read_slice)
            .min(to_split)
            .min(self.input.len() - self.pos);
        buf[..n].copy_from_slice(&self.input[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

impl Write for FakeSocket {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, SockError> {
        if buf.is_empty() {
            return Ok(0);
        }
        let n = buf.len().min(self.write_limit);
        self.out.extend_from_slice(&buf[..n]);
        Ok(n)
    }

    async fn flush(&mut self) -> Result<(), SockError> {
        self.flushes += 1;
        Ok(())
    }
}

// ------------------------------------------------------------ scenario runner

#[derive(Clone)]
struct Scenario {
    files: Vec<(String, Vec<u8>)>,
    input: Vec<u8>,
    slice: usize,
    splits: Vec<usize>,
    end: End,
    write_limit: usize,
    unmounted: bool,
    injections: Vec<(StorageOp, String, usize, ErrorKind)>,
    watch: Vec<String>,
    host_dir: bool,
}

impl Scenario {
    fn new(input: Vec<u8>) -> Self {
        Self {
            files: Vec::new(),
            input,
            slice: WHOLE,
            splits: Vec::new(),
            end: End::Pending,
            write_limit: usize::MAX,
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

    // a read never returns bytes on both sides of input offset `at`
    fn split_at(mut self, at: usize) -> Self {
        self.splits.push(at);
        self
    }

    fn end(mut self, end: End) -> Self {
        self.end = end;
        self
    }

    fn write_limit(mut self, n: usize) -> Self {
        self.write_limit = n;
        self
    }

    fn seed(mut self, name: &str, data: &[u8]) -> Self {
        self.files.push((name.to_string(), data.to_vec()));
        self.watch(name)
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

    // card is a real directory (inside a sandbox) so escapes are observable
    fn host_dir(mut self) -> Self {
        self.host_dir = true;
        self
    }
}

struct Outcome {
    response: Vec<u8>,
    event: ServerEvent,
    flushes: usize,
    // watched root names, in watch order; None = file absent
    files: Vec<(String, Option<Vec<u8>>)>,
    pending_injections: usize,
    // host_dir only: every path under the sandbox ("dir/" for directories)
    tree: Vec<(String, Vec<u8>)>,
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
        "pulp-upload-http-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ))
}

fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
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

fn run_inner(s: Scenario) -> Outcome {
    let sandbox = sandbox_root();
    let card = if s.host_dir {
        let sd_dir = sandbox.join(SANDBOX_SD);
        std::fs::create_dir_all(&sd_dir).unwrap();
        for (name, data) in &s.files {
            std::fs::write(sd_dir.join(name), data).unwrap();
        }
        VirtualStorage::host_dir(&sd_dir)
    } else {
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
    let mut sock = FakeSocket {
        input: s.input.clone(),
        pos: 0,
        read_slice: s.slice.max(1),
        splits: s.splits.clone(),
        end: s.end,
        write_limit: s.write_limit.max(1),
        out: Vec::new(),
        flushes: 0,
    };
    let mut scratch = Box::new(HttpScratch::EMPTY);
    let event = block_on(serve_request(&mut sock, &sd, &mut scratch));
    let files = s
        .watch
        .iter()
        .map(|n| (n.clone(), read_back(&sd.card, n)))
        .collect();
    let mut tree = Vec::new();
    if s.host_dir {
        walk(&sandbox, &sandbox, &mut tree);
        tree.sort();
        let _ = std::fs::remove_dir_all(&sandbox);
    }
    Outcome {
        response: sock.out,
        event,
        flushes: sock.flushes,
        files,
        pending_injections: sd.card.pending_injections(),
        tree,
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

// what a browser XHR sends for FormData with one file field named "file"
fn part_header(filename: Option<&str>) -> String {
    match filename {
        Some(f) => format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{f}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        ),
        None => "Content-Disposition: form-data; name=\"file\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            .to_string(),
    }
}

fn multipart_body(boundary: &str, filename: Option<&str>, content: &[u8]) -> Vec<u8> {
    let mut body = format!("--{boundary}\r\n{}", part_header(filename)).into_bytes();
    body.extend_from_slice(content);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

fn upload_request(boundary: &str, filename: &str, content: &[u8]) -> Vec<u8> {
    let body = multipart_body(boundary, Some(filename), content);
    http_post(
        "/upload",
        Some(&format!("multipart/form-data; boundary={boundary}")),
        body.len(),
        &body,
    )
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

// a multipart body is only well formed when the content holds no CRLF--boundary;
// the CRLF ending the part headers counts as the byte before the content
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

// fragments that look like (parts of) the delimiter, none of them a full one
fn lookalike_pieces(b: &str) -> Vec<Vec<u8>> {
    let prefix = &b[..b.len() - 1];
    let mut pieces: Vec<Vec<u8>> = vec![
        b"\r\n--".to_vec(),
        format!("\r\n--{prefix}").into_bytes(),
        format!("x--{b}").into_bytes(),
        format!("\r--{b}").into_bytes(),
        format!("\n--{b}").into_bytes(),
        format!("x--{b}--").into_bytes(),
        b"--".to_vec(),
        b"--\r\n".to_vec(),
        b"\r\n--\r\n".to_vec(),
        b"\r\n".to_vec(),
        b"\r\n\r\n".to_vec(),
    ];
    // a prefix of the delimiter cut at every length, glued to a following byte
    // that is not the next delimiter byte
    for k in 0..b.len() {
        let mut p = format!("\r\n--{}", &b[..k]).into_bytes();
        p.push(b'|');
        pieces.push(p);
    }
    pieces
}

fn lookalike_content(b: &str) -> Vec<Vec<u8>> {
    let pieces = lookalike_pieces(b);
    let prefix = &b[..b.len() - 1];
    let tail = format!("\r\n--{prefix}").into_bytes();
    let mut all = Vec::new();
    // each fragment alone
    for p in &pieces {
        all.push(p.clone());
    }
    // everything glued with a separator that cannot complete a delimiter
    let mut glued = Vec::new();
    for p in &pieces {
        glued.extend_from_slice(p);
        glued.push(b'|');
    }
    all.push(glued.clone());
    // ending in a delimiter prefix, or in a lone CRLF-dash-dash
    let mut ends_prefix = b"text".to_vec();
    ends_prefix.extend_from_slice(&tail);
    all.push(ends_prefix);
    all.push(b"text\r\n--".to_vec());
    // starting with a delimiter prefix
    let mut starts = tail.clone();
    starts.extend_from_slice(b"|rest");
    all.push(starts);
    // the glued pattern repeated across several work buffers
    let mut long = Vec::new();
    while long.len() < KIB100 {
        long.extend_from_slice(&glued);
    }
    all.push(long);
    all
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

// contract: 200 with the given Content-Type and body, response flushed
fn assert_200(out: &Outcome, content_type: Option<&str>, body: &[u8], ctx: &str) {
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
    let (_, actual_body) = split_response(&out.response);
    if let Some(cl) = header_value(&out.response, "Content-Length") {
        assert_eq!(
            cl.parse::<usize>().ok(),
            Some(actual_body.len()),
            "{ctx}: Content-Length does not match the body"
        );
    }
    assert_bytes_eq(actual_body, body, &format!("{ctx}: body"));
    assert!(out.flushes >= 1, "{ctx}: response was not flushed");
}

// contract: a failure is a non-200 error status, never 200 or "OK".
// `answered`: the peer is still there, so a status line is required.
fn assert_failure(out: &Outcome, answered: bool, ctx: &str) {
    let r = &out.response;
    assert!(
        find(r, b"200 OK").is_none(),
        "{ctx}: failure response contains \"200 OK\": {:?}",
        lossy(r)
    );
    assert!(
        find(r, b"HTTP/1.0 200").is_none(),
        "{ctx}: failure response has a 200 status: {:?}",
        lossy(r)
    );
    assert_ne!(split_response(r).1, b"OK", "{ctx}: failure body is \"OK\"");
    if answered || !r.is_empty() {
        let code = status_code(r)
            .unwrap_or_else(|| panic!("{ctx}: no HTTP status line in {:?}", lossy(r)));
        assert!(code >= 400, "{ctx}: failure answered with status {code}");
        assert!(out.flushes >= 1, "{ctx}: response was not flushed");
    }
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
        } => assert_eq!(name_of(&n, l), name.as_bytes(), "{ctx}: uploaded name"),
        other => panic!("{ctx}: expected Uploaded({name}), got {other:?}"),
    }
}

fn assert_deleted(out: &Outcome, name: &str, ctx: &str) {
    match out.event {
        ServerEvent::Deleted {
            name: n,
            name_len: l,
        } => assert_eq!(name_of(&n, l), name.as_bytes(), "{ctx}: deleted name"),
        other => panic!("{ctx}: expected Deleted({name}), got {other:?}"),
    }
}

fn assert_unchanged(out: &Outcome, seeds: &[(&str, &[u8])], ctx: &str) {
    for (name, data) in seeds {
        assert_eq!(
            out.file(name),
            Some(*data),
            "{ctx}: existing file {name} changed"
        );
    }
}

// the card-side expectation for GET /files: contract says the array follows
// list_root_files order and lists name / size of each entry
fn expected_listing(files: &[(&str, &[u8])]) -> String {
    let card = VirtualStorage::memory_with(files);
    let mut buf = [DirEntry::EMPTY; 64];
    let n = card.list_root_files(&mut buf).unwrap();
    let items: Vec<String> = buf[..n]
        .iter()
        .map(|e| {
            format!(
                "{{\"name\":\"{}\",\"size\":{}}}",
                std::str::from_utf8(&e.name[..e.name_len as usize]).unwrap(),
                e.size
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

fn is_legal_8_3(name: &[u8]) -> bool {
    let Ok(s) = std::str::from_utf8(name) else {
        return false;
    };
    let (stem, ext) = match s.split_once('.') {
        Some((stem, ext)) => (stem, ext),
        None => (s, ""),
    };
    let ok = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '~');
    !stem.is_empty()
        && stem.len() <= 8
        && ext.len() <= 3
        && stem.chars().all(ok)
        && ext.chars().all(ok)
}

// ============================================================
// contract: GET / -> 200, text/html, body == assets/upload.html, event Nothing
#[test]
fn get_root_serves_the_upload_page() {
    let out = run(Scenario::new(get("/")));
    assert_200(&out, Some("text/html"), UPLOAD_PAGE, "GET /");
    assert_eq!(out.event, ServerEvent::Nothing);
}

// contract: GET /files, [] when empty, application/json, event Nothing
#[test]
fn get_files_on_an_empty_card_is_an_empty_array() {
    let out = run(Scenario::new(get("/files")));
    assert_200(&out, Some("application/json"), b"[]", "GET /files (empty)");
    assert_eq!(out.event, ServerEvent::Nothing);
}

// contract: array of exactly the root files' name / size in list_root_files
// order; the query string is ignored. Decoys that list_root_files does not
// report (unsupported extension, hidden, inside a directory) must not appear.
#[test]
fn get_files_lists_exactly_the_root_files() {
    let a = lcg_bytes(1, 1);
    let book = lcg_bytes(100, 2);
    let big = lcg_bytes(70_000, 3);
    let decoys: [(&str, Vec<u8>); 3] = [
        ("DATA.BIN", lcg_bytes(9, 4)),
        (".HIDDEN.TXT", lcg_bytes(9, 5)),
        ("_PULP/CACHE/P0.BIN", lcg_bytes(9, 6)),
    ];
    let seeds: Vec<(&str, &[u8])> = vec![
        ("ZED.MD", big.as_slice()),
        ("A.TXT", a.as_slice()),
        ("BOOK.TXT", book.as_slice()),
        (decoys[0].0, decoys[0].1.as_slice()),
        (decoys[1].0, decoys[1].1.as_slice()),
        (decoys[2].0, decoys[2].1.as_slice()),
    ];
    let expected = expected_listing(&seeds);
    assert_eq!(
        expected.matches("\"name\"").count(),
        3,
        "test bug: virtual card should list exactly the 3 supported files"
    );
    for path in ["/files", "/files?x=1"] {
        let mut s = Scenario::new(get(path));
        for (name, data) in &seeds {
            s = s.seed(name, data);
        }
        let out = run(s);
        assert_200(
            &out,
            Some("application/json"),
            expected.as_bytes(),
            &format!("GET {path}"),
        );
        assert_eq!(out.event, ServerEvent::Nothing, "GET {path}");
    }
}

// contract: size is the decimal byte count, so 0 is "0"
#[test]
fn get_files_reports_a_zero_byte_file() {
    let out = run(Scenario::new(get("/files")).seed("E.TXT", b""));
    assert_200(
        &out,
        Some("application/json"),
        br#"[{"name":"E.TXT","size":0}]"#,
        "GET /files (zero size)",
    );
}

// contract: POST /delete, Content-Length n, body = plain file name (the form
// upload.html sends) -> 200 OK, Deleted{name}, the file is gone, others stay.
// Note: the brief's " BOOK.TXT\n" trim case is not tested: the client never
// sends whitespace, so trimming is an implementation detail, not a requirement.
#[test]
fn delete_removes_the_named_file() {
    let book = lcg_bytes(100, 7);
    let keep = lcg_bytes(5, 8);
    for slice in [1, 3, 1460, WHOLE] {
        let ctx = format!("delete BOOK.TXT slice {slice}");
        let out = run(Scenario::new(delete_request(b"BOOK.TXT"))
            .slice(slice)
            .seed("BOOK.TXT", &book)
            .seed("KEEP.TXT", &keep));
        assert_200(&out, None, b"OK", &ctx);
        assert_deleted(&out, "BOOK.TXT", &ctx);
        assert_eq!(out.file("BOOK.TXT"), None, "{ctx}: file still on card");
        assert_unchanged(&out, &[("KEEP.TXT", &keep)], &ctx);
    }
}

// contract: names over 12 bytes are rejected, so exactly 12 must work
#[test]
fn delete_accepts_a_twelve_byte_name() {
    let data = lcg_bytes(33, 9);
    let out = run(Scenario::new(delete_request(b"ABCDEFGH.TXT")).seed("ABCDEFGH.TXT", &data));
    assert_200(&out, None, b"OK", "delete 12-byte name");
    assert_deleted(&out, "ABCDEFGH.TXT", "delete 12-byte name");
    assert_eq!(out.file("ABCDEFGH.TXT"), None);
}

// contract: other method / path -> HTTP/1.0 404 Not Found, event Nothing
#[test]
fn other_methods_and_paths_are_404() {
    let requests: Vec<(&str, Vec<u8>)> = vec![
        ("GET /nope", get("/nope")),
        ("GET /index.html", get("/index.html")),
        ("GET /files/x", get("/files/x")),
        ("GET /upload", get("/upload")),
        ("GET /delete", get("/delete")),
        ("POST /", http_post("/", None, 0, b"")),
        ("POST /files", http_post("/files", None, 0, b"")),
        (
            "PUT /upload",
            b"PUT /upload HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n".to_vec(),
        ),
        (
            "DELETE /files",
            b"DELETE /files HTTP/1.1\r\nHost: x\r\n\r\n".to_vec(),
        ),
    ];
    for (label, req) in requests {
        let out = run(Scenario::new(req).seed("BOOK.TXT", b"keep"));
        assert_eq!(
            status_line(&out.response),
            "HTTP/1.0 404 Not Found",
            "{label}"
        );
        assert_eq!(out.event, ServerEvent::Nothing, "{label}");
        assert!(out.flushes >= 1, "{label}: not flushed");
        assert_unchanged(&out, &[("BOOK.TXT", b"keep")], label);
    }
}

// contract: header over 1024 bytes with no CRLFCRLF -> HTTP/1.0 431, Nothing
#[test]
fn oversized_header_is_431() {
    let mut req = b"GET / HTTP/1.1\r\nHost: x\r\nX-Pad: ".to_vec();
    req.extend(std::iter::repeat_n(b'a', 3000));
    for slice in [1, 1460, WHOLE] {
        let out = run(Scenario::new(req.clone()).slice(slice));
        let line = status_line(&out.response);
        assert!(
            line.starts_with("HTTP/1.0 431"),
            "slice {slice}: status line {line:?}"
        );
        assert_eq!(out.event, ServerEvent::Nothing, "slice {slice}");
        assert!(out.flushes >= 1, "slice {slice}: not flushed");
    }
}

// contract: only "over 1024 without terminator" is rejected, so a complete
// header of a few hundred bytes must still be served
#[test]
fn a_long_but_complete_header_is_served() {
    let mut req = b"GET /files HTTP/1.1\r\nHost: x\r\nCookie: ".to_vec();
    req.extend(std::iter::repeat_n(b'c', 800));
    req.extend_from_slice(b"\r\n\r\n");
    assert!(req.len() < 1024, "test bug: header must stay under 1024");
    let out = run(Scenario::new(req));
    assert_200(&out, Some("application/json"), b"[]", "long header");
}

// contract: peer closes before the header is complete -> no panic, Nothing
#[test]
fn peer_closing_before_the_header_ends_is_nothing() {
    let partial: [&[u8]; 5] = [
        b"",
        b"GET",
        b"GET /fil",
        b"GET /files HTTP/1.1\r\nHost: x\r\n",
        b"GET /files HTTP/1.1\r\nHost: x\r\n\r",
    ];
    for input in partial {
        for slice in [1, WHOLE] {
            let out = run(Scenario::new(input.to_vec()).slice(slice).end(End::Zero));
            assert_eq!(
                out.event,
                ServerEvent::Nothing,
                "closed after {:?} slice {slice}",
                String::from_utf8_lossy(input)
            );
        }
    }
}

// contract: results do not depend on how the input is cut into reads
#[test]
fn results_do_not_depend_on_read_slicing() {
    let book = lcg_bytes(120, 10);
    let big = lcg_bytes(3000, 11);
    let small_upload = lcg_bytes(3000, 12);
    let mut oversized = b"GET / HTTP/1.1\r\nX-Pad: ".to_vec();
    oversized.extend(std::iter::repeat_n(b'z', 2000));
    let cases: Vec<(&str, Scenario)> = vec![
        ("GET /", Scenario::new(get("/"))),
        (
            "GET /files",
            Scenario::new(get("/files"))
                .seed("BOOK.TXT", &book)
                .seed("BIG.MD", &big),
        ),
        (
            "GET /files?x=1",
            Scenario::new(get("/files?x=1")).seed("BOOK.TXT", &book),
        ),
        ("GET /nope", Scenario::new(get("/nope"))),
        ("431", Scenario::new(oversized)),
        (
            "delete",
            Scenario::new(delete_request(b"BOOK.TXT"))
                .seed("BOOK.TXT", &book)
                .seed("KEEP.TXT", b"k"),
        ),
        (
            "delete missing",
            Scenario::new(delete_request(b"NOPE.TXT")).seed("KEEP.TXT", b"k"),
        ),
        (
            "upload",
            Scenario::new(upload_request(
                &make_boundary(40),
                "BOOK.TXT",
                &small_upload,
            ))
            .seed("BOOK.TXT", &book),
        ),
    ];
    for (label, base) in cases {
        let reference = run(base.clone().slice(WHOLE));
        for slice in [1, 3, 64, 1460] {
            let out = run(base.clone().slice(slice));
            let ctx = format!("{label}: slice {slice} vs whole");
            assert_bytes_eq(&out.response, &reference.response, &ctx);
            assert_eq!(out.event, reference.event, "{ctx}: event");
            assert_eq!(out.files, reference.files, "{ctx}: card state");
        }
    }
}

// contract: "寫出完整回應後 flush": the whole response arrives even when the
// socket accepts only a few bytes per write
#[test]
fn partial_socket_writes_still_deliver_the_whole_response() {
    let cases: Vec<(&str, Scenario)> = vec![
        ("GET /", Scenario::new(get("/"))),
        (
            "GET /files",
            Scenario::new(get("/files")).seed("BOOK.TXT", b"abc"),
        ),
        ("GET /nope", Scenario::new(get("/nope"))),
        (
            "upload",
            Scenario::new(upload_request(&make_boundary(40), "UP.TXT", b"hello")),
        ),
    ];
    for (label, base) in cases {
        let reference = run(base.clone());
        for limit in [1, 7, 1460] {
            let out = run(base.clone().write_limit(limit));
            assert_bytes_eq(
                &out.response,
                &reference.response,
                &format!("{label}: write limit {limit}"),
            );
            assert!(out.flushes >= 1, "{label}: not flushed");
        }
    }
}

// ============================================================
fn check_upload(ctx: &str, boundary: &str, filename: &str, content: &[u8], slice: usize) {
    assert_well_formed(boundary, content);
    let out = run(Scenario::new(upload_request(boundary, filename, content))
        .slice(slice)
        .watch(filename));
    assert_200(&out, None, b"OK", ctx);
    assert_uploaded(&out, filename, ctx);
    match out.file(filename) {
        Some(stored) => assert_bytes_eq(stored, content, &format!("{ctx}: stored file")),
        None => panic!("{ctx}: file {filename} missing from the card"),
    }
}

// stored bytes == input bytes, around the 2048-byte work buffer, for
// every boundary length and read slicing
#[test]
fn upload_preserves_bytes_around_buffer_sizes() {
    let sizes = [0, 1, 2047, 2048, 2049, KIB100];
    for size in sizes {
        for kind in ["random", "all-values"] {
            let content = match kind {
                "random" => lcg_bytes(size, size as u64 + 100),
                _ => all_byte_values(size),
            };
            for blen in BOUNDARY_LENS {
                let boundary = make_boundary(blen);
                for slice in UPLOAD_SLICES {
                    let ctx = format!("{kind} size {size} boundary {blen} slice {slice}");
                    check_upload(&ctx, &boundary, "UP.TXT", &content, slice);
                }
            }
        }
    }
}

// every size in small windows around the buffer multiples, which moves
// the closing delimiter across the work buffer edge
#[test]
fn upload_size_sweep_near_buffer_edges() {
    let sizes: Vec<usize> = (0..=40).chain(2030..=2070).chain(4090..=4100).collect();
    for size in sizes {
        let content = lcg_bytes(size, size as u64 + 7);
        for blen in BOUNDARY_LENS {
            let boundary = make_boundary(blen);
            for slice in [1, 7, 1460, WHOLE] {
                let ctx = format!("size {size} boundary {blen} slice {slice}");
                check_upload(&ctx, &boundary, "UP.TXT", &content, slice);
            }
        }
    }
}

// content that looks like the delimiter (but never is one) is data
#[test]
fn upload_preserves_boundary_lookalike_content() {
    for blen in BOUNDARY_LENS {
        let boundary = make_boundary(blen);
        for (i, content) in lookalike_content(&boundary).iter().enumerate() {
            let slices: &[usize] = if content.len() > 4096 {
                &[7, 512, 1460, WHOLE]
            } else {
                &UPLOAD_SLICES
            };
            for &slice in slices {
                let ctx = format!(
                    "lookalike #{i} ({} bytes) boundary {blen} slice {slice}",
                    content.len()
                );
                check_upload(&ctx, &boundary, "UP.TXT", content, slice);
            }
        }
    }
}

// content ending with (or made of) CRLF keeps every CRLF; only the one
// CRLF that belongs to the delimiter is not data
#[test]
fn upload_preserves_crlf_content() {
    let mut long_text = Vec::new();
    for i in 0..300 {
        long_text.extend_from_slice(format!("line {i}\r\n").as_bytes());
    }
    let contents: Vec<Vec<u8>> = vec![
        b"line\r\n".to_vec(),
        b"\r\n".to_vec(),
        b"\r\n\r\n".to_vec(),
        b"a\r\nb\r\n".to_vec(),
        b"\r".to_vec(),
        b"\n".to_vec(),
        b"abc\r".to_vec(),
        b"\r\n\r\n\r\n".to_vec(),
        long_text,
    ];
    for blen in BOUNDARY_LENS {
        let boundary = make_boundary(blen);
        for content in &contents {
            for slice in UPLOAD_SLICES {
                let ctx = format!(
                    "crlf content {:?}.. boundary {blen} slice {slice}",
                    lossy(content).chars().take(12).collect::<String>()
                );
                check_upload(&ctx, &boundary, "UP.TXT", content, slice);
            }
        }
    }
}

// an existing file is replaced by the new content, never appended to
#[test]
fn upload_overwrites_an_existing_file() {
    let boundary = make_boundary(40);
    let cases: Vec<(&str, Vec<u8>, Vec<u8>)> = vec![
        ("larger to smaller", vec![b'A'; 5000], vec![b'B'; 100]),
        ("smaller to larger", vec![b'A'; 10], lcg_bytes(5000, 21)),
        ("same size", vec![b'A'; 2048], lcg_bytes(2048, 22)),
        ("to empty", vec![b'A'; 3000], Vec::new()),
        ("multi buffer", lcg_bytes(70_000, 23), lcg_bytes(30_000, 24)),
    ];
    for (label, old, new) in cases {
        for slice in [7, 1460, WHOLE] {
            let ctx = format!("overwrite {label} slice {slice}");
            let out = run(Scenario::new(upload_request(&boundary, "BOOK.TXT", &new))
                .slice(slice)
                .seed("BOOK.TXT", &old)
                .seed("OTHER.TXT", b"untouched"));
            assert_200(&out, None, b"OK", &ctx);
            assert_uploaded(&out, "BOOK.TXT", &ctx);
            assert_bytes_eq(out.file("BOOK.TXT").expect("file missing"), &new, &ctx);
            assert_unchanged(&out, &[("OTHER.TXT", b"untouched")], &ctx);
        }
    }
}

// contract: a file name that is already a legal 8.3 name is written as is
#[test]
fn upload_keeps_a_legal_8_3_name() {
    let content = lcg_bytes(300, 31);
    for name in [
        "BOOK.TXT",
        "A.TXT",
        "NOTES.MD",
        "ABCDEFGH.TXT",
        "BOOK.EPU",
        "NOEXT",
    ] {
        let out =
            run(Scenario::new(upload_request(&make_boundary(40), name, &content)).watch(name));
        assert_200(&out, None, b"OK", name);
        assert_uploaded(&out, name, name);
        assert_bytes_eq(out.file(name).expect("file missing"), &content, name);
    }
}

// contract: names are sanitized to 8.3. Hostile or odd names must either be
// sanitized into one plain legal name in the card root or fail; both are
// fine, but nothing may land outside the card root and nothing may panic.
#[test]
fn upload_never_writes_outside_the_card_root() {
    let names = [
        "../EVIL.TXT",
        "..\\EVIL.TXT",
        "/EVIL.TXT",
        "SUB/EVIL.TXT",
        "SUB\\EVIL.TXT",
        "a/../../EVIL.TXT",
        "../../../../EVIL.TXT",
        "C:\\Users\\me\\EVIL.TXT",
        "..",
        ".",
        "/",
        "../",
        "EVIL.TXT/",
        "a very long file name.txt",
        "UPPER lower.MD",
        "\u{65e5}\u{672c}\u{8a9e}.txt",
    ];
    let content = lcg_bytes(300, 41);
    let root_prefix = format!("{SANDBOX_SD}/");
    let allowed_dirs: Vec<String> = {
        let mut dirs = Vec::new();
        let mut acc = String::new();
        for part in SANDBOX_SD.split('/') {
            acc.push_str(part);
            acc.push('/');
            dirs.push(acc.clone());
        }
        dirs
    };
    for filename in names {
        let ctx = format!("filename {filename:?}");
        let out =
            run(Scenario::new(upload_request(&make_boundary(40), filename, &content)).host_dir());
        // nothing outside the root: no extra directory, no file below a subdirectory
        let mut root_files: Vec<(&str, &Vec<u8>)> = Vec::new();
        for (path, data) in &out.tree {
            if path.ends_with('/') {
                assert!(
                    allowed_dirs.contains(path),
                    "{ctx}: unexpected directory {path:?} in {:?}",
                    out.tree.iter().map(|(p, _)| p).collect::<Vec<_>>()
                );
            } else {
                let rest = path.strip_prefix(root_prefix.as_str()).unwrap_or_else(|| {
                    panic!("{ctx}: file written outside the card root: {path:?}")
                });
                assert!(
                    !rest.contains('/'),
                    "{ctx}: file below a subdirectory: {path:?}"
                );
                root_files.push((rest, data));
            }
        }
        if status_code(&out.response) == Some(200) {
            let ServerEvent::Uploaded { name, name_len } = out.event else {
                panic!("{ctx}: 200 without Uploaded: {:?}", out.event);
            };
            let written = name_of(&name, name_len);
            assert!(
                is_legal_8_3(written),
                "{ctx}: reported name {:?} is not a legal 8.3 name",
                String::from_utf8_lossy(written)
            );
            assert_eq!(
                root_files.len(),
                1,
                "{ctx}: expected exactly the uploaded file"
            );
            assert_eq!(
                root_files[0].0.as_bytes(),
                written,
                "{ctx}: file name on card"
            );
            assert_bytes_eq(root_files[0].1, &content, &ctx);
        } else {
            assert_failure(&out, true, &ctx);
            assert_eq!(out.event, ServerEvent::UploadFailed, "{ctx}");
        }
    }
}

// ============================================================
// a storage write failure (Write on the first chunk, Append on a later
// one) is reported as a failure, never as a success. The injection must have
// fired, so the failure is the injected one.
#[test]
fn storage_write_failure_is_reported_not_uploaded() {
    let boundary = make_boundary(40);
    // (label, op, nth, content size)
    let cases: [(&str, StorageOp, usize, usize); 8] = [
        ("write 1st, tiny", StorageOp::Write, 1, 100),
        ("write 1st, 5000", StorageOp::Write, 1, 5000),
        ("write 1st, 100KiB", StorageOp::Write, 1, KIB100),
        ("append 1st, 100KiB", StorageOp::Append, 1, KIB100),
        ("append 2nd, 100KiB", StorageOp::Append, 2, KIB100),
        ("append 5th, 100KiB", StorageOp::Append, 5, KIB100),
        ("append 1st, 5000", StorageOp::Append, 1, 5000),
        ("append 2nd, 5000", StorageOp::Append, 2, 5000),
    ];
    for (label, op, nth, size) in cases {
        let content = lcg_bytes(size, 51);
        for slice in [7, 1460, WHOLE] {
            for kind in [ErrorKind::WriteFailed, ErrorKind::OpenFile] {
                let ctx = format!("{label} slice {slice} {kind:?}");
                let out = run(Scenario::new(upload_request(&boundary, "UP.TXT", &content))
                    .slice(slice)
                    .seed("OTHER.TXT", b"untouched")
                    .inject(op, "UP.TXT", nth, kind));
                assert_eq!(out.pending_injections, 0, "{ctx}: injection never fired");
                assert_failure(&out, true, &ctx);
                assert_eq!(out.event, ServerEvent::UploadFailed, "{ctx}");
                assert_unchanged(&out, &[("OTHER.TXT", b"untouched")], &ctx);
            }
        }
    }
}

// with no SD card, upload fails, listing does not panic or leak files,
// delete fails
#[test]
fn missing_sd_card_fails_cleanly() {
    let seeded = lcg_bytes(40, 61);

    let up = run(
        Scenario::new(upload_request(&make_boundary(40), "UP.TXT", b"hello"))
            .seed("BOOK.TXT", &seeded)
            .watch("UP.TXT")
            .unmounted(),
    );
    assert_failure(&up, true, "upload without SD");
    assert_eq!(up.event, ServerEvent::UploadFailed);
    assert_eq!(up.file("UP.TXT"), None, "upload without SD created a file");

    let list = run(Scenario::new(get("/files"))
        .seed("BOOK.TXT", &seeded)
        .unmounted());
    assert_eq!(list.event, ServerEvent::Nothing, "list without SD");
    assert!(
        find(&list.response, b"BOOK.TXT").is_none(),
        "list without SD reports a file: {:?}",
        lossy(&list.response)
    );

    let del = run(Scenario::new(delete_request(b"BOOK.TXT"))
        .seed("BOOK.TXT", &seeded)
        .unmounted());
    assert_failure(&del, true, "delete without SD");
    assert_eq!(del.event, ServerEvent::DeleteFailed);
    assert_unchanged(&del, &[("BOOK.TXT", &seeded)], "delete without SD");
}

// contract: the connection ends before the multipart body does ->
// UploadFailed, non-200. After a complete HTTP header, a read of 0 or a read
// error is an interrupted upload. (If the peer is gone the implementation may
// write nothing; if it writes, it must not be a success.)
#[test]
fn interrupted_upload_is_a_failure() {
    let boundary = make_boundary(40);
    let content = lcg_bytes(5000, 71);
    let body = multipart_body(&boundary, Some("UP.TXT"), &content);
    let prefix = format!("--{boundary}\r\n{}", part_header(Some("UP.TXT"))).len();
    let closing = format!("\r\n--{boundary}--\r\n").len();
    let cuts: [(&str, usize); 6] = [
        ("no body at all", 0),
        ("inside the first boundary line", 10),
        ("inside the part headers", prefix - 5),
        ("middle of the content", prefix + 2500),
        ("content complete, no closing delimiter", prefix + 5000),
        ("inside the closing delimiter", body.len() - closing + 12),
    ];
    let ct = format!("multipart/form-data; boundary={boundary}");
    for (label, cut) in cuts {
        let req = http_post("/upload", Some(&ct), body.len(), &body[..cut]);
        for slice in [1, 1460, WHOLE] {
            for end in [End::Zero, End::Fail] {
                let ctx = format!("{label} slice {slice} {end:?}");
                let out = run(Scenario::new(req.clone()).slice(slice).end(end));
                assert_failure(&out, false, &ctx);
                assert_eq!(out.event, ServerEvent::UploadFailed, "{ctx}");
            }
        }
    }
}

// ------------------------------------------------------------ final delimiter

// R13: a part is complete only when its closing delimiter is followed by "--".
// The body is the usual one-part form up to the delimiter, then `tail` as is.
fn upload_request_with_tail(
    boundary: &str,
    filename: &str,
    content: &[u8],
    tail: &[u8],
) -> Vec<u8> {
    let mut body = format!("--{boundary}\r\n{}", part_header(Some(filename))).into_bytes();
    body.extend_from_slice(content);
    body.extend_from_slice(format!("\r\n--{boundary}").as_bytes());
    body.extend_from_slice(tail);
    http_post(
        "/upload",
        Some(&format!("multipart/form-data; boundary={boundary}")),
        body.len(),
        &body,
    )
}

const TAIL_SIZES: [usize; 6] = [0, 100, 2040, 2048, 2100, 5000];

// R13: delimiter followed by anything but "--" (CRLF = another part follows)
// is an error and never a stored upload
#[test]
fn upload_delimiter_not_followed_by_dashes_is_a_failure() {
    let boundary = make_boundary(40);
    let second_part = format!(
        "\r\n--{boundary}\r\n{}other\r\n--{boundary}--\r\n",
        part_header(Some("TWO.TXT"))
    );
    let tails: [(&str, Vec<u8>); 6] = [
        ("CRLF only", b"\r\n".to_vec()),
        ("CRLF and text", b"\r\nxx".to_vec()),
        ("second part follows", second_part.into_bytes()),
        ("two non-dash bytes", b"xx".to_vec()),
        ("dash then other", b"-x".to_vec()),
        ("other then dash", b"x-".to_vec()),
    ];
    for size in TAIL_SIZES {
        let content = lcg_bytes(size, size as u64 + 900);
        assert_well_formed(&boundary, &content);
        for (label, tail) in &tails {
            let req = upload_request_with_tail(&boundary, "UP.TXT", &content, tail);
            for slice in [1, 7, 1460, WHOLE] {
                let ctx = format!("{label} size {size} slice {slice}");
                let out = run(Scenario::new(req.clone()).slice(slice));
                assert_failure(&out, true, &ctx);
                assert_eq!(out.event, ServerEvent::UploadFailed, "{ctx}");
            }
        }
    }
}

// R13: the data after the delimiter ends before it can be judged
// (0 or 1 byte, then the peer closes or the connection breaks) -> error
#[test]
fn upload_delimiter_with_too_little_after_it_is_a_failure() {
    let boundary = make_boundary(40);
    for size in TAIL_SIZES {
        let content = lcg_bytes(size, size as u64 + 910);
        assert_well_formed(&boundary, &content);
        for (label, tail) in [("no byte after", &b""[..]), ("one dash after", &b"-"[..])] {
            let req = upload_request_with_tail(&boundary, "UP.TXT", &content, tail);
            for slice in [1, 7, WHOLE] {
                for end in [End::Zero, End::Fail] {
                    let ctx = format!("{label} size {size} slice {slice} {end:?}");
                    let out = run(Scenario::new(req.clone()).slice(slice).end(end));
                    assert_failure(&out, false, &ctx);
                    assert_eq!(out.event, ServerEvent::UploadFailed, "{ctx}");
                }
            }
        }
    }
}

// R13: delimiter + "--" completes the upload, with or without the trailing
// CRLF (the connection stays open, so no read past the "--" may be needed) and
// with an epilogue
#[test]
fn upload_final_delimiter_completes_the_upload() {
    let boundary = make_boundary(40);
    let tails: [(&str, &[u8]); 3] = [
        ("bare dashes", b"--"),
        ("dashes CRLF", b"--\r\n"),
        ("dashes CRLF epilogue", b"--\r\nepilogue\r\n"),
    ];
    for size in TAIL_SIZES {
        let content = lcg_bytes(size, size as u64 + 920);
        assert_well_formed(&boundary, &content);
        for (label, tail) in tails {
            let req = upload_request_with_tail(&boundary, "UP.TXT", &content, tail);
            for slice in [1, 7, 1460, WHOLE] {
                let ctx = format!("{label} size {size} slice {slice}");
                let out = run(Scenario::new(req.clone()).slice(slice).watch("UP.TXT"));
                assert_200(&out, None, b"OK", &ctx);
                assert_uploaded(&out, "UP.TXT", &ctx);
                match out.file("UP.TXT") {
                    Some(stored) => assert_bytes_eq(stored, &content, &format!("{ctx}: file")),
                    None => panic!("{ctx}: UP.TXT missing from the card"),
                }
            }
        }
    }
}

// R13: the final "--" may arrive in two reads, cut at every point from just
// before the delimiter's dashes to inside the trailing CRLF
#[test]
fn upload_final_dashes_split_across_reads_still_complete() {
    let boundary = make_boundary(40);
    for size in [100, 2040, 2048, 5000] {
        let content = lcg_bytes(size, size as u64 + 930);
        assert_well_formed(&boundary, &content);
        let req = upload_request_with_tail(&boundary, "UP.TXT", &content, b"--\r\n");
        // offset of the first of the two closing dashes
        let dashes = req.len() - 4;
        for (label, cut) in [
            ("before the dashes", dashes),
            ("between the dashes", dashes + 1),
            ("after the dashes", dashes + 2),
            ("inside the CRLF", dashes + 3),
        ] {
            for slice in [1460, WHOLE] {
                let ctx = format!("cut {label} size {size} slice {slice}");
                let out = run(Scenario::new(req.clone())
                    .slice(slice)
                    .split_at(cut)
                    .watch("UP.TXT"));
                assert_200(&out, None, b"OK", &ctx);
                assert_uploaded(&out, "UP.TXT", &ctx);
                match out.file("UP.TXT") {
                    Some(stored) => assert_bytes_eq(stored, &content, &format!("{ctx}: file")),
                    None => panic!("{ctx}: UP.TXT missing from the card"),
                }
            }
        }
    }
}

// contract: request defects are failures: missing boundary, missing
// filename, empty filename
#[test]
fn malformed_upload_requests_fail() {
    let content = lcg_bytes(300, 81);
    let boundary = make_boundary(40);
    let good_body = multipart_body(&boundary, Some("UP.TXT"), &content);

    let mut cases: Vec<(String, Vec<u8>)> = Vec::new();
    for ct in [None, Some("multipart/form-data"), Some("text/plain")] {
        cases.push((
            format!("content type {ct:?}"),
            http_post("/upload", ct, good_body.len(), &good_body),
        ));
    }
    let ct = format!("multipart/form-data; boundary={boundary}");
    let no_filename = multipart_body(&boundary, None, &content);
    cases.push((
        "no filename".into(),
        http_post("/upload", Some(&ct), no_filename.len(), &no_filename),
    ));
    let empty_filename = multipart_body(&boundary, Some(""), &content);
    cases.push((
        "empty filename".into(),
        http_post("/upload", Some(&ct), empty_filename.len(), &empty_filename),
    ));
    for (label, req) in cases {
        for slice in [1, 1460, WHOLE] {
            let ctx = format!("{label} slice {slice}");
            let out = run(Scenario::new(req.clone())
                .slice(slice)
                .seed("OTHER.TXT", b"untouched"));
            assert_failure(&out, true, &ctx);
            assert_eq!(out.event, ServerEvent::UploadFailed, "{ctx}");
            assert_unchanged(&out, &[("OTHER.TXT", b"untouched")], &ctx);
        }
    }
}

// contract: delete failures are DeleteFailed with a non-200 status and
// leave every existing file as it was
#[test]
fn delete_failures_are_reported_and_change_nothing() {
    let book = lcg_bytes(100, 91);
    let keep = lcg_bytes(5, 92);
    let thirteen = lcg_bytes(13, 93);
    let seeds: [(&str, &[u8]); 3] = [
        ("BOOK.TXT", book.as_slice()),
        ("KEEP.TXT", keep.as_slice()),
        ("ABCDEFGH.TXTX", thirteen.as_slice()),
    ];
    let seeded = |req: Vec<u8>| {
        let mut s = Scenario::new(req);
        for (n, d) in seeds {
            s = s.seed(n, d);
        }
        s
    };

    let mut cases: Vec<(&str, Scenario)> = vec![
        (
            "storage delete error",
            seeded(delete_request(b"BOOK.TXT")).inject(
                StorageOp::Delete,
                "BOOK.TXT",
                1,
                ErrorKind::DeleteFailed,
            ),
        ),
        ("missing file", seeded(delete_request(b"NOPE.TXT"))),
        ("empty name", seeded(delete_request(b""))),
        (
            "13-byte name (an existing file)",
            seeded(delete_request(b"ABCDEFGH.TXTX")),
        ),
        (
            "non UTF-8 name",
            seeded(delete_request(&[0xFF, 0xFE, b'.', b'T', b'X', b'T'])),
        ),
        (
            "body shorter than Content-Length",
            seeded(http_post("/delete", Some("text/plain"), 8, b"BOOK")),
        ),
    ];
    let mut with_prefix_file = seeded(http_post("/delete", Some("text/plain"), 8, b"BOO"));
    with_prefix_file = with_prefix_file.seed("BOO", b"x");
    cases.push(("body cut after a prefix that is a file", with_prefix_file));

    for (label, base) in cases {
        for slice in [1, 1460, WHOLE] {
            // the last case ends the connection early
            let early_end = label.starts_with("body ");
            let s = base.clone().slice(slice);
            let s = if early_end { s.end(End::Zero) } else { s };
            let out = run(s);
            let ctx = format!("{label} slice {slice}");
            if label == "storage delete error" {
                assert_eq!(out.pending_injections, 0, "{ctx}: injection never fired");
            }
            assert_failure(&out, !early_end, &ctx);
            assert!(
                !matches!(out.event, ServerEvent::Deleted { .. }),
                "{ctx}: reported Deleted"
            );
            if !early_end {
                assert_eq!(out.event, ServerEvent::DeleteFailed, "{ctx}");
            }
            assert_unchanged(&out, &seeds, &ctx);
            if label.contains("prefix") {
                assert_eq!(out.file("BOO"), Some(&b"x"[..]), "{ctx}: BOO changed");
            }
        }
    }
}
