use core::future::pending;
use embassy_futures::block_on;
use embassy_time::{Duration, with_timeout};
use embedded_io_async::{ErrorType, Read, Write};
use pulp_host::apps::upload_http::IdleStream;

struct Stalled;
impl ErrorType for Stalled {
    type Error = core::convert::Infallible;
}
impl Read for Stalled {
    async fn read(&mut self, _: &mut [u8]) -> Result<usize, Self::Error> {
        pending().await
    }
}
impl Write for Stalled {
    async fn write(&mut self, _: &[u8]) -> Result<usize, Self::Error> {
        pending().await
    }
    async fn flush(&mut self) -> Result<(), Self::Error> {
        pending().await
    }
}

#[test]
fn stalled_read_write_and_flush_expire() {
    block_on(async {
        let mut stream = Stalled;
        let mut bounded = IdleStream::new(&mut stream, Duration::from_millis(10));
        let read = with_timeout(Duration::from_millis(100), bounded.read(&mut [0; 8])).await;
        assert!(
            matches!(read, Ok(Err(_))),
            "read must expire before outer deadline"
        );
        let write = with_timeout(Duration::from_millis(100), bounded.write(b"response")).await;
        assert!(
            matches!(write, Ok(Err(_))),
            "write must expire before outer deadline"
        );
        let flush = with_timeout(Duration::from_millis(100), bounded.flush()).await;
        assert!(
            matches!(flush, Ok(Err(_))),
            "flush must expire before outer deadline"
        );
    });
}

struct PartialUpload {
    input: Vec<u8>,
    position: usize,
    trickle: bool,
}
impl ErrorType for PartialUpload {
    type Error = core::convert::Infallible;
}
impl Read for PartialUpload {
    async fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        if self.position == self.input.len() {
            if self.trickle {
                embassy_time::Timer::after(Duration::from_millis(2)).await;
                buffer[0] = b'x';
                return Ok(1);
            }
            return pending().await;
        }
        let n = buffer.len().min(self.input.len() - self.position);
        buffer[..n].copy_from_slice(&self.input[self.position..self.position + n]);
        self.position += n;
        Ok(n)
    }
}
impl Write for PartialUpload {
    async fn write(&mut self, buffer: &[u8]) -> Result<usize, Self::Error> {
        Ok(buffer.len())
    }
    async fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[test]
fn idle_or_total_deadline_removes_incomplete_upload_and_preserves_other_books() {
    use pulp_host::apps::upload_http::{HttpScratch, SessionToken, serve_request};
    use pulp_host::drivers::sdcard::SdStorage;
    use pulp_host::storage::VirtualStorage;
    for (total_first, trickle) in [(false, false), (true, false), (true, true)] {
        let sd = SdStorage::new(VirtualStorage::memory_with(&[("BOOK.TXT", b"original")]));
        let token = SessionToken::from_bytes([0; 16]);
        let mut input = format!("POST /upload HTTP/1.1\r\nX-Pulp-Token: {}\r\nContent-Type: multipart/form-data; boundary=TEST\r\nContent-Length: 20000\r\n\r\n--TEST\r\nContent-Disposition: form-data; name=\"file\"; filename=\"UP.TXT\"\r\n\r\n", token.as_str()).into_bytes();
        input.extend_from_slice(&[b'x'; 5000]);
        let mut socket = PartialUpload {
            input,
            position: 0,
            trickle,
        };
        let mut scratch = HttpScratch::EMPTY;
        let mut work = [0; 2048];
        let outcome = block_on(async {
            let mut stream = IdleStream::new(
                &mut socket,
                Duration::from_millis(if total_first && !trickle { 100 } else { 10 }),
            );
            with_timeout(
                Duration::from_millis(if trickle {
                    30
                } else if total_first {
                    10
                } else {
                    100
                }),
                serve_request(&mut stream, &sd, &mut scratch, &mut work, &token),
            )
            .await
        });
        assert_eq!(outcome.is_err(), total_first);
        assert_eq!(sd.card.writer_open_count(), 1);
        assert_eq!(sd.card.writer_close_count(), 1);
        assert!(sd.card.file_size("UP.TXT").is_err());
        assert_eq!(sd.card.file_size("BOOK.TXT").unwrap(), 8);
    }
}
