// Upload writer session: one open file per upload, sector-aligned batches in
// bounded slices, close on every way out, and the work-buffer profiles, driven
// through the production `serve_request` over a fake socket and the virtual
// card (whose writer stand-in counts opens, closes and append sizes).
//
// Expected values follow the Task 4 contract (open once, append in <= 4 KiB
// slices of whole sectors with yields between them, close on success, error and
// dropped future, bytes stored exactly), never an implementation output. The
// future is polled by hand so yields and cancellation are observable; a socket
// that runs out of input stalls like a browser waiting for the response.

use std::cell::Cell;
use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use embedded_io_async::{ErrorKind as IoErrorKind, ErrorType, Read, Write};
use pulp_board_logic::upload::{FLUSH_SLICE_BYTES, MIN_WORK_BYTES, NetProfile};
use pulp_host::ErrorKind;
use pulp_host::apps::upload_http::{HttpScratch, ServerEvent, serve_request};
use pulp_host::drivers::sdcard::SdStorage;
use pulp_host::storage::{StorageOp, VirtualStorage};

const WHOLE: usize = usize::MAX;
const SLICES: [usize; 5] = [1, 7, 512, 1460, WHOLE];
const SECTOR: usize = 512;
const NAME: &str = "UP.BIN";

// ------------------------------------------------------------ fake socket

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum End {
    // input exhausted: the read never completes (connection stays open)
    Stall,
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

struct FakeSocket<'a> {
    input: Vec<u8>,
    pos: usize,
    read_slice: usize,
    // absolute input offsets no single read may cross
    splits: Vec<usize>,
    end: End,
    stalled: Rc<Cell<bool>>,
    out: Vec<u8>,
    // backpressure probe: the card and file whose size is the persisted payload
    card: &'a VirtualStorage,
    // most bytes taken from the socket and not yet on the card, seen at a read
    max_ahead: usize,
    // largest buffer a read was offered
    max_offer: usize,
}

impl ErrorType for FakeSocket<'_> {
    type Error = SockError;
}

impl Read for FakeSocket<'_> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, SockError> {
        if buf.is_empty() {
            return Ok(0);
        }
        let persisted = self.card.file_size(NAME).unwrap_or(0) as usize;
        self.max_ahead = self.max_ahead.max(self.pos.saturating_sub(persisted));
        self.max_offer = self.max_offer.max(buf.len());
        if self.pos >= self.input.len() {
            return match self.end {
                End::Zero => Ok(0),
                End::Fail => Err(SockError),
                End::Stall => {
                    self.stalled.set(true);
                    core::future::pending::<Result<usize, SockError>>().await
                }
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

impl Write for FakeSocket<'_> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, SockError> {
        self.out.extend_from_slice(buf);
        Ok(buf.len())
    }

    async fn flush(&mut self) -> Result<(), SockError> {
        Ok(())
    }
}

// ------------------------------------------------------------ runner

#[derive(Clone)]
struct Opts {
    work: usize,
    slice: usize,
    splits: Vec<usize>,
    end: End,
    // drop the request future after this many executor yields
    cut_after_yields: usize,
}

impl Opts {
    fn new() -> Self {
        Self {
            work: NetProfile::SMALL.work,
            slice: WHOLE,
            splits: Vec::new(),
            end: End::Stall,
            cut_after_yields: usize::MAX,
        }
    }

    fn work(mut self, n: usize) -> Self {
        self.work = n;
        self
    }

    fn slice(mut self, n: usize) -> Self {
        self.slice = n;
        self
    }

    fn split_at(mut self, at: usize) -> Self {
        self.splits.push(at);
        self
    }

    fn end(mut self, end: End) -> Self {
        self.end = end;
        self
    }

    fn cut_after_yields(mut self, n: usize) -> Self {
        self.cut_after_yields = n;
        self
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Finish {
    Done,
    // the socket had nothing more to give and the future was dropped
    Stalled,
    // dropped at an executor yield
    Cut,
}

struct Run {
    event: Option<ServerEvent>,
    finish: Finish,
    response: Vec<u8>,
    // Pending polls caused by executor yields (not by a stalled socket)
    yields: usize,
    max_ahead: usize,
    max_offer: usize,
}

fn run(sd: &SdStorage, input: Vec<u8>, o: &Opts) -> Run {
    let stalled = Rc::new(Cell::new(false));
    let mut sock = FakeSocket {
        input,
        pos: 0,
        read_slice: o.slice.max(1),
        splits: o.splits.clone(),
        end: o.end,
        stalled: stalled.clone(),
        out: Vec::new(),
        card: &sd.card,
        max_ahead: 0,
        max_offer: 0,
    };
    let mut scratch = Box::new(HttpScratch::EMPTY);
    let mut work = vec![0u8; o.work];
    let mut yields = 0usize;
    let (event, finish) = {
        let fut = serve_request(&mut sock, sd, &mut scratch, &mut work);
        let mut fut = pin!(fut);
        let mut cx = Context::from_waker(Waker::noop());
        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(ev) => break (Some(ev), Finish::Done),
                Poll::Pending if stalled.get() => break (None, Finish::Stalled),
                Poll::Pending => {
                    yields += 1;
                    if yields > o.cut_after_yields {
                        yields -= 1;
                        break (None, Finish::Cut);
                    }
                }
            }
        }
        // the future is dropped here, with its writer
    };
    Run {
        event,
        finish,
        response: sock.out,
        yields,
        max_ahead: sock.max_ahead,
        max_offer: sock.max_offer,
    }
}

fn card() -> SdStorage {
    SdStorage::new(VirtualStorage::memory())
}

fn stored(sd: &SdStorage) -> Option<Vec<u8>> {
    let size = sd.card.file_size(NAME).ok()? as usize;
    let mut buf = vec![0u8; size];
    let mut got = 0;
    while got < size {
        let n = sd
            .card
            .read_file_chunk(NAME, got as u32, &mut buf[got..])
            .ok()?;
        assert!(n > 0, "card returned fewer bytes than its size");
        got += n;
    }
    Some(buf)
}

// ------------------------------------------------------------ requests

fn boundary(len: usize) -> String {
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

// CRLF-heavy bytes that never hold CRLF--b (b starts with a letter or dash
// run, the content only ever follows CRLF with '-' once and then a space)
fn crlf_bytes(len: usize) -> Vec<u8> {
    let unit = b"\r\n- a\r\n\r\n--\r\n";
    unit.iter().copied().cycle().take(len).collect()
}

struct Request {
    bytes: Vec<u8>,
    // offset of the first multipart body byte
    body_at: usize,
    // offset of the closing delimiter's CRLF in `bytes`
    marker_at: usize,
    // offset of the first content byte
    content_at: usize,
}

fn upload_request(b: &str, content: &[u8]) -> Request {
    let part = format!(
        "--{b}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{NAME}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    );
    let mut body = part.into_bytes();
    let content_in_body = body.len();
    body.extend_from_slice(content);
    let marker_in_body = body.len();
    body.extend_from_slice(format!("\r\n--{b}--\r\n").as_bytes());
    let head = format!(
        "POST /upload HTTP/1.1\r\nHost: 192.168.4.1\r\nContent-Type: multipart/form-data; boundary={b}\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    let mut bytes = head.clone().into_bytes();
    bytes.extend_from_slice(&body);
    Request {
        bytes,
        body_at: head.len(),
        marker_at: head.len() + marker_in_body,
        content_at: head.len() + content_in_body,
    }
}

fn status(response: &[u8]) -> &[u8] {
    response.split(|&b| b == b'\r').next().unwrap_or_default()
}

fn assert_ok_response(r: &Run, ctx: &str) {
    assert_eq!(r.finish, Finish::Done, "{ctx}");
    assert_eq!(
        r.event,
        Some(ServerEvent::Uploaded {
            name: *b"UP.BIN\0\0\0\0\0\0\0",
            name_len: 6
        }),
        "{ctx}"
    );
    assert_eq!(status(&r.response), b"HTTP/1.0 200 OK", "{ctx}");
    assert!(r.response.ends_with(b"\r\n\r\nOK"), "{ctx}");
}

fn assert_failure_response(r: &Run, message: &str, ctx: &str) {
    assert_eq!(r.finish, Finish::Done, "{ctx}");
    assert_eq!(r.event, Some(ServerEvent::UploadFailed), "{ctx}");
    assert_eq!(
        status(&r.response),
        b"HTTP/1.0 500 Internal Server Error",
        "{ctx}"
    );
    assert!(
        r.response.ends_with(message.as_bytes()),
        "{ctx}: {:?}",
        String::from_utf8_lossy(&r.response)
    );
}

// one open, one close, nothing left open
fn assert_balanced(sd: &SdStorage, opens: usize, ctx: &str) {
    assert_eq!(sd.card.writer_open_count(), opens, "{ctx}: opens");
    assert_eq!(sd.card.writer_close_count(), opens, "{ctx}: closes");
    assert_eq!(sd.card.live_writers(), 0, "{ctx}: open writers");
}

// every append is a bounded slice; all of them but the last are whole sectors
fn assert_slices(sd: &SdStorage, total: usize, ctx: &str) {
    let sizes = sd.card.writer_append_sizes();
    assert_eq!(sizes.iter().sum::<usize>(), total, "{ctx}: appended bytes");
    for (i, &n) in sizes.iter().enumerate() {
        assert!(n > 0, "{ctx}: empty append #{i}");
        assert!(n <= FLUSH_SLICE_BYTES, "{ctx}: append #{i} is {n} bytes");
        if i + 1 < sizes.len() {
            assert_eq!(n % SECTOR, 0, "{ctx}: unaligned append #{i} of {n} bytes");
        }
    }
}

// ------------------------------------------------------------ exact output

// sizes around sectors, slices and the work-buffer edges of both profiles
fn sizes_for(work: usize, blen: usize) -> Vec<usize> {
    let hold = 4 + blen;
    let mut v = vec![
        0, 1, 2, 511, 512, 513, 1023, 1024, 1025, 4095, 4096, 4097, 8191, 8192, 8193,
    ];
    for edge in [
        work - hold,
        work,
        work + hold,
        2 * work - hold,
        2 * work + 7,
    ] {
        v.extend([edge - 1, edge, edge + 1]);
    }
    v.sort_unstable();
    v.dedup();
    v
}

#[test]
fn bytes_are_stored_exactly_for_every_size_slice_and_profile() {
    for profile in [NetProfile::SMALL, NetProfile::LARGE] {
        for blen in [1, 40, 70, 120] {
            let b = boundary(blen);
            for size in sizes_for(profile.work, blen) {
                let content = lcg_bytes(size, size as u64 + 3);
                let req = upload_request(&b, &content);
                for slice in SLICES {
                    let ctx = format!(
                        "work {} boundary {blen} size {size} slice {slice}",
                        profile.work
                    );
                    let sd = card();
                    let r = run(
                        &sd,
                        req.bytes.clone(),
                        &Opts::new().work(profile.work).slice(slice),
                    );
                    assert_ok_response(&r, &ctx);
                    assert_eq!(stored(&sd).as_deref(), Some(&content[..]), "{ctx}");
                    assert_balanced(&sd, 1, &ctx);
                    assert_slices(&sd, size, &ctx);
                }
            }
        }
    }
}

#[test]
fn crlf_and_dash_content_survives_every_split_around_the_closing_delimiter() {
    for profile in [NetProfile::SMALL, NetProfile::LARGE] {
        for blen in [1, 40, 70] {
            let b = boundary(blen);
            for size in [0, 1, 511, 512, 513, 5000] {
                let content = crlf_bytes(size);
                let req = upload_request(&b, &content);
                // one split at every offset from before the delimiter to the end
                for at in req.marker_at.saturating_sub(3)..=req.bytes.len() {
                    let ctx = format!(
                        "work {} boundary {blen} size {size} split {at}",
                        profile.work
                    );
                    let sd = card();
                    let r = run(
                        &sd,
                        req.bytes.clone(),
                        &Opts::new().work(profile.work).split_at(at),
                    );
                    assert_ok_response(&r, &ctx);
                    assert_eq!(stored(&sd).as_deref(), Some(&content[..]), "{ctx}");
                    assert_balanced(&sd, 1, &ctx);
                }
            }
        }
    }
}

#[test]
fn splits_inside_the_part_headers_and_at_the_first_content_byte_are_exact() {
    let b = boundary(40);
    let content = lcg_bytes(3000, 9);
    let req = upload_request(&b, &content);
    for profile in [NetProfile::SMALL, NetProfile::LARGE] {
        for at in 1..req.content_at + 3 {
            let ctx = format!("work {} split {at}", profile.work);
            let sd = card();
            let r = run(
                &sd,
                req.bytes.clone(),
                &Opts::new().work(profile.work).split_at(at),
            );
            assert_ok_response(&r, &ctx);
            assert_eq!(stored(&sd).as_deref(), Some(&content[..]), "{ctx}");
            assert_balanced(&sd, 1, &ctx);
        }
    }
}

#[test]
fn a_megabyte_upload_stays_in_bounded_aligned_slices_without_whole_file_buffering() {
    let b = boundary(40);
    let content = lcg_bytes(1024 * 1024 + 123, 21);
    let req = upload_request(&b, &content);
    for profile in [NetProfile::SMALL, NetProfile::LARGE] {
        let ctx = format!("work {}", profile.work);
        let sd = card();
        let r = run(
            &sd,
            req.bytes.clone(),
            &Opts::new().work(profile.work).slice(1460),
        );
        assert_ok_response(&r, &ctx);
        assert_eq!(stored(&sd).as_deref(), Some(&content[..]), "{ctx}");
        assert_balanced(&sd, 1, &ctx);
        assert_slices(&sd, content.len(), &ctx);
        // backpressure: the socket is never read further ahead of the card
        // than the window plus the header bytes taken with the first read
        assert!(
            r.max_ahead <= profile.work + 1024,
            "{ctx}: read {} bytes ahead of storage",
            r.max_ahead
        );
        assert!(
            r.max_offer <= profile.work,
            "{ctx}: offered {}",
            r.max_offer
        );
    }
}

#[test]
fn the_large_work_buffer_batches_into_full_slices() {
    let b = boundary(40);
    let content = lcg_bytes(256 * 1024, 5);
    let req = upload_request(&b, &content);
    let appends = |work: usize| {
        let sd = card();
        let r = run(&sd, req.bytes.clone(), &Opts::new().work(work));
        assert_ok_response(&r, "batching");
        sd.card.writer_append_sizes()
    };
    let small = appends(NetProfile::SMALL.work);
    let large = appends(NetProfile::LARGE.work);
    // the small window (1.9 KiB payload per flush) never reaches a full slice;
    // the large one writes mostly 4 KiB slices
    assert!(small.iter().all(|&n| n < FLUSH_SLICE_BYTES), "{small:?}");
    let full = large.iter().filter(|&&n| n == FLUSH_SLICE_BYTES).count();
    assert!(
        full * 5 >= large.len() * 3,
        "{full} of {} appends",
        large.len()
    );
    assert!(
        large.len() * 2 < small.len(),
        "{} vs {}",
        large.len(),
        small.len()
    );
}

// ------------------------------------------------------------ yields

#[test]
fn flushes_yield_between_slices_and_only_there() {
    let b = boundary(40);
    // input with no stall: the window holds the whole request, so the one
    // flush is the delimiter-found one and splits into ceil(size/4096) slices
    for (size, slices) in [(4096, 1), (4097, 2), (8192, 2), (12288, 3), (12289, 4)] {
        let content = lcg_bytes(size, 31);
        let req = upload_request(&b, &content);
        let sd = card();
        let r = run(&sd, req.bytes, &Opts::new().work(NetProfile::LARGE.work));
        let ctx = format!("size {size}");
        assert_ok_response(&r, &ctx);
        assert_eq!(sd.card.writer_append_sizes().len(), slices, "{ctx}");
        assert_eq!(r.yields, slices - 1, "{ctx}: yields");
        assert_eq!(stored(&sd).as_deref(), Some(&content[..]), "{ctx}");
    }
}

#[test]
fn a_future_dropped_at_any_yield_closes_the_file_once() {
    let b = boundary(40);
    let content = lcg_bytes(12289, 41);
    let req = upload_request(&b, &content);
    // 3 yields in a full run (4 slices)
    for cut in 0..=4 {
        let ctx = format!("cut after {cut} yields");
        let sd = card();
        let r = run(
            &sd,
            req.bytes.clone(),
            &Opts::new()
                .work(NetProfile::LARGE.work)
                .cut_after_yields(cut),
        );
        if cut < 3 {
            assert_eq!(r.finish, Finish::Cut, "{ctx}");
            assert_eq!(r.event, None, "{ctx}");
            assert!(r.response.is_empty(), "{ctx}: nothing answered");
            // the slices before the cut are on the card, nothing after
            let got = stored(&sd).unwrap();
            assert_eq!(got.len(), 4096 * (cut + 1), "{ctx}");
            assert_eq!(got, &content[..got.len()], "{ctx}");
        } else {
            assert_ok_response(&r, &ctx);
        }
        assert_balanced(&sd, 1, &ctx);
    }
}

// ------------------------------------------------------------ cancellation

#[test]
fn a_stalled_upload_dropped_anywhere_closes_what_it_opened() {
    let b = boundary(40);
    let content = lcg_bytes(5000, 51);
    let req = upload_request(&b, &content);
    let part_end = req.content_at;
    let em_len = 4 + 40;
    // (label, bytes the peer sent before going quiet, writer opened by then,
    // the whole content is on the card: a complete delimiter flushes the tail)
    let cuts = [
        ("no body", req.body_at, false, false),
        ("inside the part headers", part_end - 30, false, false),
        ("part headers complete", part_end, true, false),
        ("100 content bytes", part_end + 100, true, false),
        ("2500 content bytes", part_end + 2500, true, false),
        ("all content", part_end + 5000, true, false),
        (
            "inside the closing delimiter",
            req.marker_at + 10,
            true,
            false,
        ),
        (
            "delimiter without dashes",
            req.marker_at + em_len,
            true,
            true,
        ),
        (
            "delimiter and one dash",
            req.marker_at + em_len + 1,
            true,
            true,
        ),
    ];
    for profile in [NetProfile::SMALL, NetProfile::LARGE] {
        for (label, sent, opened, all) in cuts {
            for slice in [7, WHOLE] {
                let ctx = format!("{label} work {} slice {slice}", profile.work);
                let sd = card();
                let r = run(
                    &sd,
                    req.bytes[..sent].to_vec(),
                    &Opts::new().work(profile.work).slice(slice),
                );
                assert_eq!(r.finish, Finish::Stalled, "{ctx}");
                assert_eq!(r.event, None, "{ctx}");
                assert_balanced(&sd, usize::from(opened), &ctx);
                if !opened {
                    assert_eq!(stored(&sd), None, "{ctx}: file created too early");
                    continue;
                }
                let got = stored(&sd).expect("file created on open");
                if all {
                    assert_eq!(got, content, "{ctx}");
                } else {
                    // only whole sectors were written, all of them payload
                    assert_eq!(got.len() % SECTOR, 0, "{ctx}");
                    assert_eq!(got, &content[..got.len()], "{ctx}");
                }
            }
        }
    }
}

// ------------------------------------------------------------ failures

#[test]
fn peer_close_or_socket_error_mid_upload_fails_and_closes() {
    let b = boundary(40);
    let content = lcg_bytes(5000, 61);
    let req = upload_request(&b, &content);
    let sent = req.content_at + 3000;
    for end in [End::Zero, End::Fail] {
        for profile in [NetProfile::SMALL, NetProfile::LARGE] {
            let ctx = format!("{end:?} work {}", profile.work);
            let sd = card();
            let r = run(
                &sd,
                req.bytes[..sent].to_vec(),
                &Opts::new().work(profile.work).end(end),
            );
            let message = match end {
                End::Zero => "upload incomplete",
                _ => "read error during upload",
            };
            assert_failure_response(&r, message, &ctx);
            assert_balanced(&sd, 1, &ctx);
            let got = stored(&sd).unwrap();
            if end == End::Zero {
                // a closed peer: what the window still held is written out
                assert_eq!(got, &content[..3000], "{ctx}");
            } else {
                assert!(
                    got.len() <= 3000 && got.len() % SECTOR == 0,
                    "{ctx}: {}",
                    got.len()
                );
                assert_eq!(got, &content[..got.len()], "{ctx}");
            }
        }
    }
}

#[test]
fn short_storage_write_fails_the_upload_and_closes() {
    let b = boundary(40);
    let content = lcg_bytes(20000, 71);
    let req = upload_request(&b, &content);
    for profile in [NetProfile::SMALL, NetProfile::LARGE] {
        for (nth, written) in [(1, 0), (1, 100), (2, 511), (3, 1), (5, 4000)] {
            let ctx = format!("work {} append #{nth} stores {written}", profile.work);
            let sd = card();
            sd.card.inject_short_write(NAME, nth, written);
            let r = run(
                &sd,
                req.bytes.clone(),
                &Opts::new().work(profile.work).slice(1460),
            );
            assert_failure_response(&r, "write failed", &ctx);
            assert_eq!(sd.card.pending_injections(), 0, "{ctx}: never fired");
            assert_balanced(&sd, 1, &ctx);
            // the appends before the failing one, plus the bytes it managed
            let sizes = sd.card.writer_append_sizes();
            assert_eq!(sizes.len(), nth, "{ctx}: no append after the failure");
            let want: usize = sizes[..nth - 1].iter().sum::<usize>() + written.min(sizes[nth - 1]);
            let got = stored(&sd).unwrap();
            assert_eq!(got.len(), want, "{ctx}");
            assert_eq!(got, &content[..want], "{ctx}");
        }
    }
}

#[test]
fn storage_errors_on_open_append_or_close_are_reported_and_leave_no_handle() {
    let b = boundary(40);
    let content = lcg_bytes(9000, 81);
    let req = upload_request(&b, &content);
    for kind in [
        ErrorKind::WriteFailed,
        ErrorKind::OpenFile,
        ErrorKind::NoCard,
    ] {
        for (label, op, nth, opens) in [
            ("open", StorageOp::Write, 1, 0),
            ("first append", StorageOp::Append, 1, 1),
            ("later append", StorageOp::Append, 3, 1),
            ("close", StorageOp::Close, 1, 1),
        ] {
            let ctx = format!("{label} {kind:?}");
            let sd = card();
            sd.card.inject_error(op, NAME, nth, kind);
            let r = run(&sd, req.bytes.clone(), &Opts::new());
            assert_failure_response(&r, "write failed", &ctx);
            assert_eq!(sd.card.pending_injections(), 0, "{ctx}: never fired");
            // the handle is released even when the close itself failed
            assert_balanced(&sd, opens, &ctx);
        }
    }
}

#[test]
fn a_file_cannot_grow_past_the_card_size_bound() {
    let b = boundary(40);
    let content = lcg_bytes(5000, 91);
    let req = upload_request(&b, &content);
    for profile in [NetProfile::SMALL, NetProfile::LARGE] {
        // exactly at the bound is fine
        let ctx = format!("at the bound, work {}", profile.work);
        let sd = card();
        sd.card.set_max_file_len(5000);
        let r = run(&sd, req.bytes.clone(), &Opts::new().work(profile.work));
        assert_ok_response(&r, &ctx);
        assert_eq!(stored(&sd).as_deref(), Some(&content[..]), "{ctx}");
        assert_balanced(&sd, 1, &ctx);

        // one byte over: refused before that append is written, never truncated
        for limit in [0u32, 1, 511, 4999] {
            let ctx = format!("limit {limit}, work {}", profile.work);
            let sd = card();
            sd.card.set_max_file_len(limit);
            let r = run(&sd, req.bytes.clone(), &Opts::new().work(profile.work));
            assert_failure_response(&r, "write failed", &ctx);
            assert_balanced(&sd, 1, &ctx);
            let got = stored(&sd).unwrap();
            assert!(got.len() <= limit as usize, "{ctx}: {} bytes", got.len());
            assert_eq!(got, &content[..got.len()], "{ctx}");
        }
    }
}

#[test]
fn a_missing_card_fails_before_anything_is_opened() {
    let b = boundary(40);
    let req = upload_request(&b, b"hello");
    let mut sd = card();
    sd.set_mounted(false);
    let r = run(&sd, req.bytes, &Opts::new());
    assert_failure_response(&r, "write failed", "no card");
    assert_balanced(&sd, 0, "no card");
}

// ------------------------------------------------------------ profiles

#[test]
fn a_work_buffer_below_the_minimum_refuses_the_upload_without_opening() {
    let b = boundary(40);
    let req = upload_request(&b, b"hello");
    let sd = card();
    let r = run(
        &sd,
        req.bytes.clone(),
        &Opts::new().work(MIN_WORK_BYTES - 1),
    );
    assert_failure_response(&r, "work buffer too small", "below minimum");
    assert_balanced(&sd, 0, "below minimum");

    // the minimum itself works, with the longest boundary the server accepts
    let b = boundary(120);
    let content = lcg_bytes(5000, 101);
    let req = upload_request(&b, &content);
    for slice in [1, 1460, WHOLE] {
        let ctx = format!("minimum work, slice {slice}");
        let sd = card();
        let r = run(
            &sd,
            req.bytes.clone(),
            &Opts::new().work(MIN_WORK_BYTES).slice(slice),
        );
        assert_ok_response(&r, &ctx);
        assert_eq!(stored(&sd).as_deref(), Some(&content[..]), "{ctx}");
        assert_balanced(&sd, 1, &ctx);
    }
}

#[test]
fn both_profiles_store_the_same_bytes() {
    let b = boundary(70);
    let content = lcg_bytes(100 * 1024 + 17, 111);
    let req = upload_request(&b, &content);
    let mut results = Vec::new();
    for profile in [NetProfile::SMALL, NetProfile::LARGE] {
        let sd = card();
        let r = run(
            &sd,
            req.bytes.clone(),
            &Opts::new().work(profile.work).slice(1460),
        );
        assert_ok_response(&r, "profile");
        results.push((stored(&sd).unwrap(), r.response));
    }
    assert_eq!(results[0], results[1]);
    assert_eq!(results[0].0, content);
}

#[test]
fn consecutive_uploads_each_open_and_close_one_writer() {
    let sd = card();
    let b = boundary(40);
    for (i, size) in [100usize, 5000, 0].into_iter().enumerate() {
        let content = lcg_bytes(size, 121 + i as u64);
        let req = upload_request(&b, &content);
        let r = run(&sd, req.bytes, &Opts::new().work(NetProfile::LARGE.work));
        assert_ok_response(&r, "sequence");
        assert_eq!(stored(&sd).as_deref(), Some(&content[..]), "upload {i}");
        assert_balanced(&sd, i + 1, "sequence");
    }
}
