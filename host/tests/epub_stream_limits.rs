//! Streaming limits run through both production cache routes.
use std::future::Future;
use std::task::{Context, Poll, Waker};

use smol_epub::async_io::{AsyncReadAt, AsyncWriteChunk, stream_strip_entry_async};
use smol_epub::cache::{
    self, MAX_COMPRESSED_CHAPTER_SIZE, MAX_DECOMPRESSED_CHAPTER_SIZE, MAX_STRIPPED_CHAPTER_SIZE,
};
use smol_epub::zip::{METHOD_DEFLATE, METHOD_STORED, ZipEntry};

fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

fn fixture(input: &[u8], method: u16) -> (Vec<u8>, ZipEntry) {
    let compressed = if method == METHOD_STORED {
        input.to_vec()
    } else {
        miniz_oxide::deflate::compress_to_vec(input, 6)
    };
    let mut bytes = vec![0; 30];
    bytes[..4].copy_from_slice(&0x0403_4b50u32.to_le_bytes());
    bytes.extend_from_slice(&compressed);
    (
        bytes,
        ZipEntry {
            comp_size: compressed.len() as u32,
            uncomp_size: input.len() as u32,
            method,
            ..ZipEntry::EMPTY
        },
    )
}

struct Reader<'a> {
    bytes: &'a [u8],
    reads: usize,
    max_read: usize,
}
impl Reader<'_> {
    fn read(&mut self, offset: u32, out: &mut [u8]) -> Result<usize, &'static str> {
        self.reads += 1;
        let offset = offset as usize;
        if offset > self.bytes.len() {
            return Ok(0);
        }
        let n = out.len().min(self.bytes.len() - offset).min(self.max_read);
        out[..n].copy_from_slice(&self.bytes[offset..offset + n]);
        Ok(n)
    }
}
impl AsyncReadAt for Reader<'_> {
    async fn read_at(&mut self, offset: u32, out: &mut [u8]) -> Result<usize, &'static str> {
        self.read(offset, out)
    }
}
#[derive(Default)]
struct Writer {
    bytes: usize,
    calls: usize,
}
impl Writer {
    fn write(&mut self, data: &[u8]) -> Result<(), &'static str> {
        // Every accepted write, including the last chunk, must fit the limit.
        assert!(self.bytes + data.len() <= MAX_STRIPPED_CHAPTER_SIZE as usize);
        self.bytes += data.len();
        self.calls += 1;
        Ok(())
    }
}
impl AsyncWriteChunk for Writer {
    async fn write_chunk(&mut self, data: &[u8]) -> Result<(), &'static str> {
        self.write(data)
    }
}
fn run<'a>(
    bytes: &'a [u8],
    entry: &ZipEntry,
    asynchronous: bool,
    max_read: usize,
) -> (Result<u32, &'static str>, Reader<'a>, Writer) {
    let mut reader = Reader {
        bytes,
        reads: 0,
        max_read,
    };
    let mut writer = Writer::default();
    let result = if asynchronous {
        block_on(stream_strip_entry_async(
            entry,
            entry.local_offset,
            &mut reader,
            &mut writer,
        ))
    } else {
        cache::stream_strip_entry(
            entry,
            entry.local_offset,
            |offset, buf| reader.read(offset, buf),
            |data| writer.write(data),
        )
    };
    (result, reader, writer)
}

#[test]
fn declared_small_expansion_is_rejected_before_stripping_or_writing_excess() {
    let (bytes, mut entry) = fixture(&vec![b'x'; 512 * 1024], METHOD_DEFLATE);
    entry.uncomp_size = 1;
    for asynchronous in [false, true] {
        let (result, _, writer) = run(&bytes, &entry, asynchronous, usize::MAX);
        assert_eq!(result, Err("cache: deflate output exceeds declared size"));
        assert_eq!(
            writer.bytes, 0,
            "first 32 KiB output chunk exceeds declaration"
        );
        assert_eq!(writer.calls, 0);
    }
}

#[test]
fn final_short_decompression_and_stored_size_mismatch_are_errors() {
    for asynchronous in [false, true] {
        let (bytes, mut entry) = fixture(b"hello", METHOD_DEFLATE);
        entry.uncomp_size += 1;
        let (result, _, writer) = run(&bytes, &entry, asynchronous, usize::MAX);
        assert_eq!(result, Err("cache: decompressed size mismatch"));
        assert_eq!(
            writer.bytes, 0,
            "trailing text must not be flushed on mismatch"
        );
        for delta in [-1, 1] {
            let (bytes, mut entry) = fixture(b"hello", METHOD_STORED);
            entry.comp_size = (entry.comp_size as i32 + delta) as u32;
            let (result, reader, writer) = run(&bytes, &entry, asynchronous, usize::MAX);
            assert_eq!(result, Err("cache: stored size mismatch"));
            assert_eq!(reader.reads, 0);
            assert_eq!(writer.bytes, 0);
        }
    }
}

#[test]
fn declared_limits_are_checked_before_io_or_inflate_allocations() {
    for asynchronous in [false, true] {
        for (comp_size, uncomp_size, error) in [
            (
                MAX_COMPRESSED_CHAPTER_SIZE + 1,
                1,
                "cache: compressed chapter too large",
            ),
            (
                1,
                MAX_DECOMPRESSED_CHAPTER_SIZE + 1,
                "cache: decompressed chapter too large",
            ),
        ] {
            let entry = ZipEntry {
                comp_size,
                uncomp_size,
                method: METHOD_DEFLATE,
                ..ZipEntry::EMPTY
            };
            let (result, reader, writer) = run(&[], &entry, asynchronous, usize::MAX);
            assert_eq!(result, Err(error));
            assert_eq!(reader.reads, 0);
            assert_eq!(writer.bytes, 0);
        }
    }
    let mut reader = Reader {
        bytes: &[],
        reads: 0,
        max_read: usize::MAX,
    };
    let mut writer = Writer::default();
    let entry = ZipEntry {
        comp_size: 1,
        uncomp_size: MAX_DECOMPRESSED_CHAPTER_SIZE + 1,
        method: METHOD_DEFLATE,
        ..ZipEntry::EMPTY
    };
    let result = block_on(
        smol_epub::async_io::stream_strip_entry_async_with_scratch::<
            _,
            _,
            smol_epub::scratch::HeapScratch,
            _,
        >(&entry, 0, &mut reader, &mut writer, |_| {
            panic!("oversize declaration must precede scratch allocation")
        }),
    );
    assert_eq!(result, Err("cache: decompressed chapter too large"));
}

#[test]
fn exact_chapter_boundaries_and_streams_larger_than_in_memory_limit_succeed() {
    let mut input = vec![b'x'; MAX_DECOMPRESSED_CHAPTER_SIZE as usize];
    // A tag consumes three input bytes; finish adds one newline.
    input[..3].copy_from_slice(b"<a>");
    for method in [METHOD_STORED, METHOD_DEFLATE] {
        let (bytes, entry) = fixture(&input, method);
        for asynchronous in [false, true] {
            let (result, _, writer) = run(&bytes, &entry, asynchronous, usize::MAX);
            assert_eq!(result, Ok(MAX_DECOMPRESSED_CHAPTER_SIZE - 2));
            assert_eq!(writer.bytes, input.len() - 2);
        }
    }
    let (bytes, entry) = fixture(
        &vec![b'x'; MAX_STRIPPED_CHAPTER_SIZE as usize - 1],
        METHOD_STORED,
    );
    for asynchronous in [false, true] {
        assert_eq!(
            run(&bytes, &entry, asynchronous, usize::MAX).0,
            Ok(MAX_STRIPPED_CHAPTER_SIZE)
        );
    }
    // Check the declared compressed-range boundary with trailing bytes after
    // a short raw DEFLATE stream; the range still must exist in the source.
    let (mut bytes, mut entry) = fixture(b"x", METHOD_DEFLATE);
    entry.comp_size = MAX_COMPRESSED_CHAPTER_SIZE;
    bytes.resize(30 + entry.comp_size as usize, 0);
    for asynchronous in [false, true] {
        assert_eq!(run(&bytes, &entry, asynchronous, usize::MAX).0, Ok(2));
    }
}

#[test]
fn malformed_utf8_expansion_cannot_write_past_stripped_budget() {
    // Each invalid byte emits three bytes of U+FFFD, so valid decompression
    // alone is insufficient to bound the stripped text.
    let (bytes, entry) = fixture(&vec![0xff; 3 * 1024 * 1024], METHOD_DEFLATE);
    for asynchronous in [false, true] {
        let (result, _, writer) = run(&bytes, &entry, asynchronous, usize::MAX);
        assert_eq!(result, Err("cache: stripped chapter too large"));
        assert!(writer.bytes <= MAX_STRIPPED_CHAPTER_SIZE as usize);
        assert!(writer.bytes > MAX_STRIPPED_CHAPTER_SIZE as usize - 4096);
    }
}

#[test]
fn local_offset_overflow_and_physical_compressed_eof_are_rejected() {
    let (bytes, mut entry) = fixture(b"hello", METHOD_DEFLATE);
    entry.comp_size += 1;
    for asynchronous in [false, true] {
        let (result, _, writer) = run(&bytes, &entry, asynchronous, usize::MAX);
        assert_eq!(result, Err("cache: compressed range out of bounds"));
        assert_eq!(writer.bytes, 0);
        let entry = ZipEntry {
            local_offset: u32::MAX - 10,
            ..entry
        };
        let (result, reader, _) = run(&bytes, &entry, asynchronous, usize::MAX);
        assert_eq!(result, Err("cache: offset overflow"));
        assert_eq!(reader.reads, 0);
    }
}

#[test]
fn short_reads_include_local_header_and_remain_equivalent() {
    let (bytes, entry) = fixture("<p>中文😀hello</p>".as_bytes(), METHOD_DEFLATE);
    let sync = run(&bytes, &entry, false, 1);
    let asynchronous = run(&bytes, &entry, true, 1);
    assert!(sync.0.is_ok());
    assert_eq!(sync.0, asynchronous.0);
    assert_eq!(sync.2.bytes, asynchronous.2.bytes);
}

#[test]
fn ready_stored_io_yields_and_dropping_the_future_stops_partial_output() {
    let (bytes, entry) = fixture(&vec![b'x'; 512 * 1024], METHOD_STORED);
    let mut reader = Reader {
        bytes: &bytes,
        reads: 0,
        max_read: usize::MAX,
    };
    let mut writer = Writer::default();
    {
        let mut stream = std::pin::pin!(stream_strip_entry_async(
            &entry,
            0,
            &mut reader,
            &mut writer
        ));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(
            stream.as_mut().poll(&mut cx).is_pending(),
            "ready I/O must still let input cancellation run"
        );
    }
    assert_eq!(
        reader.reads, 3,
        "header, one-byte range probe, first stored chunk"
    );
    assert!(writer.bytes > 0 && writer.bytes < entry.uncomp_size as usize);
    assert_eq!(writer.calls, 1);
}
