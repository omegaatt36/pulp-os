// storage regression -- virtual storage backends and storage failure injection.
//
// Run: cargo test-host --test storage
//
// ============================================================================
// CONTRACT (implementer must provide exactly this; the tests are the spec)
// ============================================================================
//
// host/src/lib.rs must expose two PUBLIC modules (`pub mod`):
//
//   pulp_host::error    -- the firmware's own kernel/src/error.rs included with
//                          #[path] (Error, ErrorKind; no copy). Tests only use
//                          `Error::kind()` and the `ErrorKind` variants.
//   pulp_host::storage  -- the virtual storage described below.
//
// pulp_host::storage items
// ------------------------
//   pub struct VirtualStorage;        // one backend + read log + injections
//   pub struct DirEntry;              // host-side twin of kernel storage::DirEntry:
//                                     //   pub is_dir: bool, pub size: u32,
//                                     //   pub const EMPTY: Self,
//                                     //   pub fn name_str(&self) -> &str
//   pub type Result<T> = core::result::Result<T, pulp_host::error::Error>;
//
//   #[derive(Debug, Clone, PartialEq, Eq)]
//   pub struct ReadRecord {
//       pub path: String,        // normalized path, see "Paths"
//       pub offset: u32,         // offset requested (0 for *_start reads)
//       pub requested: usize,    // buf.len() the caller passed
//       pub returned: usize,     // bytes handed back; 0 when the read failed
//       pub outcome: ReadOutcome,
//   }
//
//   #[derive(Debug, Clone, Copy, PartialEq, Eq)]
//   pub enum ReadOutcome {
//       Ok,                          // normal completion (EOF-short is still Ok)
//       ShortInjected,               // a short-read injection fired
//       ErrorInjected(ErrorKind),    // an error injection fired
//       Failed(ErrorKind),           // natural failure, e.g. OpenFile
//   }
//
//   #[derive(Debug, Clone, Copy, PartialEq, Eq)]
//   pub enum StorageOp { Read, Write, Append, Delete, FileSize, List }
//
// Constructors
// ------------
//   VirtualStorage::memory() -> VirtualStorage
//       empty in-memory card.
//   VirtualStorage::memory_with(files: &[(&str, &[u8])]) -> VirtualStorage
//       in-memory card seeded from (path, bytes); directories are implied by the
//       path prefix.
//   VirtualStorage::host_dir(root: &std::path::Path) -> VirtualStorage
//       the existing host directory `root` is the SD root. Files are real host
//       files: writes through the backend are visible with std::fs and files
//       created with std::fs are visible through the backend. The backend does
//       not delete `root` (the caller owns it).
//
// Paths (used by ReadRecord::path and by every inject_* `path` argument)
// -----
//   root file            "NAME"                 read_file_chunk("NAME", ..)
//   file in root subdir  "DIR/NAME"             *_in_dir("DIR", "NAME", ..)
//   file in _PULP        "_PULP/NAME"           *_in_pulp("NAME", ..)
//   file in _PULP subdir "_PULP/DIR/NAME"       *_in_pulp_subdir("DIR", "NAME", ..)
//   List operation       ""                     list_root_files
//   Case is preserved verbatim; tests use upper-case 8.3 names consistently.
//
// Storage methods. All take `&self` (interior mutability), because the firmware
// functions they replace take `&SdStorage`. Names/arguments mirror the
// kernel/src/drivers/storage.rs functions with the `sd` argument removed; return
// types are `pulp_host::storage::Result<..>` with the same Ok payloads:
//
//   file_size(&self, name: &str) -> Result<u32>
//   read_file_chunk(&self, name: &str, offset: u32, buf: &mut [u8]) -> Result<usize>
//   read_file_start(&self, name: &str, buf: &mut [u8]) -> Result<(u32, usize)>
//   write_file(&self, name: &str, data: &[u8]) -> Result<()>
//   append_root_file(&self, name: &str, data: &[u8]) -> Result<()>
//   delete_file(&self, name: &str) -> Result<()>
//   list_root_files(&self, buf: &mut [DirEntry]) -> Result<usize>
//   ensure_dir(&self, name: &str) -> Result<()>
//   write_file_in_dir(&self, dir: &str, name: &str, data: &[u8]) -> Result<()>
//   append_file_in_dir(&self, dir: &str, name: &str, data: &[u8]) -> Result<()>
//   read_file_chunk_in_dir(&self, dir: &str, name: &str, offset: u32, buf: &mut [u8]) -> Result<usize>
//   read_file_start_in_dir(&self, dir: &str, name: &str, buf: &mut [u8]) -> Result<(u32, usize)>
//   ensure_pulp_dir(&self) -> Result<()>                    // sync twin of ensure_pulp_dir_async
//   ensure_pulp_subdir(&self, name: &str) -> Result<()>
//   write_in_pulp_subdir(&self, dir: &str, name: &str, data: &[u8]) -> Result<()>
//   append_in_pulp_subdir(&self, dir: &str, name: &str, data: &[u8]) -> Result<()>
//   read_chunk_in_pulp_subdir(&self, dir: &str, name: &str, offset: u32, buf: &mut [u8]) -> Result<usize>
//   file_size_in_pulp_subdir(&self, dir: &str, name: &str) -> Result<u32>
//   delete_in_pulp_subdir(&self, dir: &str, name: &str) -> Result<()>
//   read_chunk_in_pulp(&self, name: &str, offset: u32, buf: &mut [u8]) -> Result<usize>
//   write_in_pulp(&self, name: &str, data: &[u8]) -> Result<()>
//   append_in_pulp(&self, name: &str, data: &[u8]) -> Result<()>
//   file_size_in_pulp(&self, name: &str) -> Result<u32>
//   delete_in_pulp(&self, name: &str) -> Result<()>
//   write_at_in_pulp(&self, name: &str, offset: u32, data: &[u8]) -> Result<()>
//
// Semantics (identical on every backend)
//   * read chunk: copies min(buf.len(), len - offset) bytes starting at `offset`
//     into the front of `buf`, returns that count. offset == len -> Ok(0).
//     offset > len -> Err(ErrorKind::SeekFailed) (firmware: the underlying seek
//     fails), `buf` untouched; that read is still counted and logged as
//     Failed(SeekFailed). Bytes of `buf` beyond the returned count are not touched.
//   * read_*_start: reads from offset 0; returns (true file size, bytes read).
//   * write_*: create or truncate-and-replace.
//   * append_*: append; creates the file when it does not exist.
//   * write_at_in_pulp: overwrites bytes in place at `offset` (tests only use a
//     range that lies inside the existing file).
//   * delete_*: removes the file; later size/read -> Err(OpenFile).
//   * Missing file (firmware kinds): every read variant (chunk and start) ->
//     Err(ErrorKind::OpenFile); every file_size variant -> Err(OpenFile);
//     every delete variant -> Err(ErrorKind::DeleteFailed). Never a panic,
//     never an empty Ok. A failed read leaves `buf` untouched.
//   * ensure_*: create the directory; calling again is Ok (idempotent).
//     ensure_pulp_dir must precede ensure_pulp_subdir on an empty card.
//   * list_root_files: fills buf with the entries directly under the root and
//     returns n = min(entries, buf.len()); order unspecified. Same filter as the
//     firmware: directories (incl. `_PULP`) and volume entries are NOT listed;
//     names starting with '.' or '_' are skipped; only files whose extension is
//     TXT, EPUB, EPU or MD are listed (upper-case names in the tests). Every
//     listed entry has is_dir = false and carries its byte size. Contents of
//     subdirectories are never listed.
//
// Read counter (virtual storage)
//   Counted as a "read" = every call of read_file_chunk, read_file_start,
//   read_file_chunk_in_dir, read_file_start_in_dir, read_chunk_in_pulp,
//   read_chunk_in_pulp_subdir -- including calls that fail (natural or
//   injected). NOT counted: file_size*, list_root_files, write*, append*,
//   delete*, ensure*.
//   read_count(&self) -> usize            // number of reads since creation/reset
//   read_log(&self) -> Vec<ReadRecord>    // one record per read, call order;
//                                         // read_count() == read_log().len()
//   reset_reads(&self)                    // clears count + log ONLY; pending
//                                         // injections are kept
//
// Error / short-read injection (storage failure injection). One-shot.
//   inject_short_read(&self, path: &str, nth: usize, returned: usize)
//   inject_error(&self, op: StorageOp, path: &str, nth: usize, kind: ErrorKind)
//   pending_injections(&self) -> usize    // registered and not yet fired
//   clear_injections(&self)               // drops all pending injections
//
//   * `nth` is 1-based and counts the matching calls (same op class + same
//     normalized path) made AFTER registration; nth = 1 hits the very next one.
//     Once fired, the injection is consumed (pending_injections drops by 1) and
//     later calls behave normally.
//   * Op classes: Read = the six read functions above; Write = write_file,
//     write_file_in_dir, write_in_pulp, write_in_pulp_subdir, write_at_in_pulp;
//     Append = append_root_file, append_file_in_dir, append_in_pulp,
//     append_in_pulp_subdir; Delete = delete_file, delete_in_pulp,
//     delete_in_pulp_subdir; FileSize = file_size, file_size_in_pulp,
//     file_size_in_pulp_subdir; List = list_root_files (path "").
//     Calls of other classes never advance `nth` of an injection.
//   * short read: the fired call returns Ok(returned) where `returned` is
//     strictly smaller than what a normal read would have returned; only
//     buf[..returned] is written (the same leading bytes a normal read would
//     give); for *_start reads the size half of the tuple is still the true
//     file size. The log record has returned == `returned` and outcome
//     ShortInjected.
//   * inject_error(Read, ..): the fired read returns Err(kind), `buf` is
//     untouched, the log record has returned == 0 and outcome
//     ErrorInjected(kind).
//   * inject_error(Write/Append/Delete/FileSize/List, ..): the fired call
//     returns Err(kind) and has NO effect on the storage (no create, no data
//     change, no delete). Not recorded in the read log.
//   * Injections on different paths / op classes are independent.
// ============================================================================

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use pulp_host::error::ErrorKind;
use pulp_host::storage::{DirEntry, ReadOutcome, ReadRecord, StorageOp, VirtualStorage};

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

const BOOK: &[u8] = b"0123456789abcdefghij"; // 20 bytes
const NOTE: &[u8] = b"hello"; // 5 bytes
const IN_DIR: &[u8] = b"in-dir-file"; // 11 bytes
const BKMK: &[u8] = b"bookmark-bytes"; // 14 bytes
const P0: &[u8] = b"subdir-bytes"; // 12 bytes

const SEED: &[(&str, &[u8])] = &[
    ("BOOK.TXT", BOOK),
    ("NOTE.TXT", NOTE),
    ("BOOKS/A.TXT", IN_DIR),
    ("_PULP/BKMK.BIN", BKMK),
    ("_PULP/CACHE/P0.BIN", P0),
];

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("pulp-host-storage-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempRoot(p)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Fx {
    label: &'static str,
    sd: VirtualStorage,
    root: Option<TempRoot>,
}

impl Fx {
    fn host_root(&self) -> &Path {
        &self.root.as_ref().expect("host-file backend only").0
    }
}

fn memory_fx(seeded: bool) -> Fx {
    let sd = if seeded {
        VirtualStorage::memory_with(SEED)
    } else {
        VirtualStorage::memory()
    };
    Fx { label: "memory", sd, root: None }
}

fn host_fx(seeded: bool) -> Fx {
    let root = TempRoot::new();
    if seeded {
        for (p, data) in SEED {
            let full = root.0.join(p);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, data).unwrap();
        }
    }
    let sd = VirtualStorage::host_dir(&root.0);
    Fx { label: "host-file", sd, root: Some(root) }
}

// run `f` once per backend, each on a freshly built, identically seeded card
fn each_backend(seeded: bool, f: impl Fn(&Fx)) {
    for fx in [memory_fx(seeded), host_fx(seeded)] {
        eprintln!("backend: {}", fx.label);
        f(&fx);
    }
}

fn rec(path: &str, offset: u32, requested: usize, returned: usize) -> ReadRecord {
    ReadRecord {
        path: path.to_string(),
        offset,
        requested,
        returned,
        outcome: ReadOutcome::Ok,
    }
}

fn read_all(sd: &VirtualStorage, name: &str) -> Vec<u8> {
    let mut buf = vec![0u8; 256];
    let n = sd.read_file_chunk(name, 0, &mut buf).unwrap();
    buf.truncate(n);
    buf
}

fn read_all_pulp(sd: &VirtualStorage, name: &str) -> Vec<u8> {
    let mut buf = vec![0u8; 256];
    let n = sd.read_chunk_in_pulp(name, 0, &mut buf).unwrap();
    buf.truncate(n);
    buf
}

// ---------------------------------------------------------------------------
// virtual storage: backend behaviour (memory and host-file must agree)
// ---------------------------------------------------------------------------

#[test]
fn seeded_files_read_back_through_every_read_variant() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        let mut b = [0u8; 4];
        assert_eq!(sd.read_file_chunk("BOOK.TXT", 5, &mut b).unwrap(), 4);
        assert_eq!(&b, b"5678");

        let mut b = [0u8; 8];
        assert_eq!(sd.read_file_start("BOOK.TXT", &mut b).unwrap(), (20, 8));
        assert_eq!(&b, b"01234567");

        let mut b = [0u8; 100];
        assert_eq!(sd.read_file_chunk_in_dir("BOOKS", "A.TXT", 3, &mut b).unwrap(), 8);
        assert_eq!(&b[..8], b"dir-file");

        let mut b = [0u8; 4];
        assert_eq!(sd.read_file_start_in_dir("BOOKS", "A.TXT", &mut b).unwrap(), (11, 4));
        assert_eq!(&b, b"in-d");

        let mut b = [0u8; 100];
        assert_eq!(sd.read_chunk_in_pulp("BKMK.BIN", 0, &mut b).unwrap(), 14);
        assert_eq!(&b[..14], b"bookmark-bytes");

        let mut b = [0u8; 100];
        assert_eq!(sd.read_chunk_in_pulp_subdir("CACHE", "P0.BIN", 7, &mut b).unwrap(), 5);
        assert_eq!(&b[..5], b"bytes");

        assert_eq!(sd.file_size("BOOK.TXT").unwrap(), 20);
        assert_eq!(sd.file_size("NOTE.TXT").unwrap(), 5);
        assert_eq!(sd.file_size_in_pulp("BKMK.BIN").unwrap(), 14);
        assert_eq!(sd.file_size_in_pulp_subdir("CACHE", "P0.BIN").unwrap(), 12);
    });
}

#[test]
fn read_at_and_past_eof() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        // tail shorter than the request: naturally short, still Ok
        let mut b = [0xEEu8; 10];
        assert_eq!(sd.read_file_chunk("BOOK.TXT", 15, &mut b).unwrap(), 5);
        assert_eq!(&b[..5], b"fghij");
        assert_eq!(&b[5..], &[0xEE; 5], "bytes past the returned count are untouched");

        // exactly at EOF (offset == len): Ok(0), not an error
        let mut b = [0xEEu8; 10];
        assert_eq!(sd.read_file_chunk("BOOK.TXT", 20, &mut b).unwrap(), 0);
        assert_eq!(b, [0xEE; 10]);

        // beyond EOF (offset > len): the firmware's seek fails -> SeekFailed,
        // buf untouched, and the failed read is still counted and logged
        sd.reset_reads();
        assert_eq!(sd.read_file_chunk("BOOK.TXT", 21, &mut b).unwrap_err().kind(), ErrorKind::SeekFailed);
        assert_eq!(sd.read_file_chunk("BOOK.TXT", 4096, &mut b).unwrap_err().kind(), ErrorKind::SeekFailed);
        assert_eq!(b, [0xEE; 10]);
        assert_eq!(sd.read_count(), 2);
        assert_eq!(
            sd.read_log(),
            vec![
                ReadRecord {
                    path: "BOOK.TXT".to_string(),
                    offset: 21,
                    requested: 10,
                    returned: 0,
                    outcome: ReadOutcome::Failed(ErrorKind::SeekFailed),
                },
                ReadRecord {
                    path: "BOOK.TXT".to_string(),
                    offset: 4096,
                    requested: 10,
                    returned: 0,
                    outcome: ReadOutcome::Failed(ErrorKind::SeekFailed),
                },
            ]
        );

        // request larger than the file from offset 0
        let mut b = [0u8; 64];
        assert_eq!(sd.read_file_start("NOTE.TXT", &mut b).unwrap(), (5, 5));
        assert_eq!(&b[..5], b"hello");
    });
}

#[test]
fn missing_file_fails_with_firmware_kinds_and_buffer_untouched() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        let mut b = [0xEEu8; 8];
        assert_eq!(sd.read_file_chunk("NOPE.TXT", 0, &mut b).unwrap_err().kind(), ErrorKind::OpenFile);
        assert_eq!(sd.read_file_start("NOPE.TXT", &mut b).unwrap_err().kind(), ErrorKind::OpenFile);
        assert_eq!(
            sd.read_file_chunk_in_dir("BOOKS", "NOPE.TXT", 0, &mut b).unwrap_err().kind(),
            ErrorKind::OpenFile
        );
        assert_eq!(
            sd.read_file_start_in_dir("BOOKS", "NOPE.TXT", &mut b).unwrap_err().kind(),
            ErrorKind::OpenFile
        );
        assert_eq!(sd.read_chunk_in_pulp("NOPE.BIN", 0, &mut b).unwrap_err().kind(), ErrorKind::OpenFile);
        assert_eq!(
            sd.read_chunk_in_pulp_subdir("CACHE", "NOPE.BIN", 0, &mut b).unwrap_err().kind(),
            ErrorKind::OpenFile
        );
        assert_eq!(b, [0xEE; 8], "failed reads must not touch the buffer");

        assert_eq!(sd.file_size("NOPE.TXT").unwrap_err().kind(), ErrorKind::OpenFile);
        assert_eq!(sd.file_size_in_pulp("NOPE.BIN").unwrap_err().kind(), ErrorKind::OpenFile);
        assert_eq!(
            sd.file_size_in_pulp_subdir("CACHE", "NOPE.BIN").unwrap_err().kind(),
            ErrorKind::OpenFile
        );
        assert_eq!(sd.delete_file("NOPE.TXT").unwrap_err().kind(), ErrorKind::DeleteFailed);
        assert_eq!(sd.delete_in_pulp("NOPE.BIN").unwrap_err().kind(), ErrorKind::DeleteFailed);
        assert_eq!(
            sd.delete_in_pulp_subdir("CACHE", "NOPE.BIN").unwrap_err().kind(),
            ErrorKind::DeleteFailed
        );
    });
}

#[test]
fn root_write_append_delete_roundtrip() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.write_file("NEW.TXT", b"abc").unwrap();
        assert_eq!(sd.file_size("NEW.TXT").unwrap(), 3);
        assert_eq!(read_all(sd, "NEW.TXT"), b"abc");

        sd.append_root_file("NEW.TXT", b"def").unwrap();
        assert_eq!(sd.file_size("NEW.TXT").unwrap(), 6);
        assert_eq!(read_all(sd, "NEW.TXT"), b"abcdef");

        // write replaces (truncates), it does not append
        sd.write_file("NEW.TXT", b"z").unwrap();
        assert_eq!(sd.file_size("NEW.TXT").unwrap(), 1);
        assert_eq!(read_all(sd, "NEW.TXT"), b"z");

        sd.delete_file("NEW.TXT").unwrap();
        assert_eq!(sd.file_size("NEW.TXT").unwrap_err().kind(), ErrorKind::OpenFile);
        let mut b = [0u8; 4];
        assert_eq!(sd.read_file_chunk("NEW.TXT", 0, &mut b).unwrap_err().kind(), ErrorKind::OpenFile);
        // other files untouched
        assert_eq!(read_all(sd, "NOTE.TXT"), NOTE);
    });
}

#[test]
fn append_creates_a_missing_file() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.append_root_file("APP.TXT", b"first").unwrap();
        assert_eq!(read_all(sd, "APP.TXT"), b"first");
        sd.append_in_pulp("APP.BIN", b"p1").unwrap();
        assert_eq!(read_all_pulp(sd, "APP.BIN"), b"p1");
    });
}

#[test]
fn pulp_write_append_write_at_delete_roundtrip() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.write_in_pulp("X.BIN", b"abcdef").unwrap();
        assert_eq!(sd.file_size_in_pulp("X.BIN").unwrap(), 6);
        sd.append_in_pulp("X.BIN", b"gh").unwrap();
        assert_eq!(read_all_pulp(sd, "X.BIN"), b"abcdefgh");
        assert_eq!(sd.file_size_in_pulp("X.BIN").unwrap(), 8);

        sd.write_at_in_pulp("X.BIN", 1, b"ZZ").unwrap();
        assert_eq!(read_all_pulp(sd, "X.BIN"), b"aZZdefgh");
        assert_eq!(sd.file_size_in_pulp("X.BIN").unwrap(), 8, "in-range write_at keeps the size");

        // an existing seeded _PULP file is replaced, not merged
        sd.write_in_pulp("BKMK.BIN", b"new").unwrap();
        assert_eq!(read_all_pulp(sd, "BKMK.BIN"), b"new");

        sd.delete_in_pulp("X.BIN").unwrap();
        assert_eq!(sd.file_size_in_pulp("X.BIN").unwrap_err().kind(), ErrorKind::OpenFile);
        // a root file with the same name is a different file
        assert_eq!(sd.file_size("X.BIN").unwrap_err().kind(), ErrorKind::OpenFile);
    });
}

#[test]
fn in_dir_and_pulp_subdir_write_append_delete_roundtrip() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.write_file_in_dir("BOOKS", "B.TXT", b"book-b").unwrap();
        sd.append_file_in_dir("BOOKS", "B.TXT", b"+more").unwrap();
        let mut b = [0u8; 64];
        let n = sd.read_file_chunk_in_dir("BOOKS", "B.TXT", 0, &mut b).unwrap();
        assert_eq!(&b[..n], b"book-b+more");
        // not visible in the root under the same bare name
        assert_eq!(sd.file_size("B.TXT").unwrap_err().kind(), ErrorKind::OpenFile);

        sd.write_in_pulp_subdir("CACHE", "P1.BIN", b"page1").unwrap();
        sd.append_in_pulp_subdir("CACHE", "P1.BIN", b"-tail").unwrap();
        assert_eq!(sd.file_size_in_pulp_subdir("CACHE", "P1.BIN").unwrap(), 10);
        let mut b = [0u8; 64];
        let n = sd.read_chunk_in_pulp_subdir("CACHE", "P1.BIN", 0, &mut b).unwrap();
        assert_eq!(&b[..n], b"page1-tail");
        // seeded sibling untouched
        assert_eq!(sd.file_size_in_pulp_subdir("CACHE", "P0.BIN").unwrap(), 12);

        sd.delete_in_pulp_subdir("CACHE", "P1.BIN").unwrap();
        assert_eq!(
            sd.file_size_in_pulp_subdir("CACHE", "P1.BIN").unwrap_err().kind(),
            ErrorKind::OpenFile
        );
        assert_eq!(sd.file_size_in_pulp_subdir("CACHE", "P0.BIN").unwrap(), 12);
    });
}

#[test]
fn ensure_dirs_are_idempotent_and_usable_on_an_empty_card() {
    each_backend(false, |fx| {
        let sd = &fx.sd;
        sd.ensure_dir("NEWDIR").unwrap();
        sd.ensure_dir("NEWDIR").unwrap();
        sd.write_file_in_dir("NEWDIR", "F.TXT", b"f").unwrap();
        let mut b = [0u8; 4];
        assert_eq!(sd.read_file_chunk_in_dir("NEWDIR", "F.TXT", 0, &mut b).unwrap(), 1);
        assert_eq!(b[0], b'f');

        sd.ensure_pulp_dir().unwrap();
        sd.ensure_pulp_dir().unwrap();
        sd.ensure_pulp_subdir("CACHE").unwrap();
        sd.ensure_pulp_subdir("CACHE").unwrap();
        sd.write_in_pulp_subdir("CACHE", "P.BIN", b"pp").unwrap();
        assert_eq!(sd.file_size_in_pulp_subdir("CACHE", "P.BIN").unwrap(), 2);
        sd.write_in_pulp("TOP.BIN", b"t").unwrap();
        assert_eq!(sd.file_size_in_pulp("TOP.BIN").unwrap(), 1);

        if fx.root.is_some() {
            let r = fx.host_root();
            assert!(r.join("NEWDIR").is_dir());
            assert!(r.join("_PULP").is_dir());
            assert!(r.join("_PULP").join("CACHE").is_dir());
        }
    });
}

fn list(sd: &VirtualStorage, cap: usize) -> (usize, Vec<DirEntry>) {
    let mut buf = vec![DirEntry::EMPTY; cap];
    let n = sd.list_root_files(&mut buf).unwrap();
    buf.truncate(n);
    (n, buf)
}

fn files_of(entries: &[DirEntry]) -> Vec<(String, u32)> {
    let mut v: Vec<(String, u32)> = entries
        .iter()
        .filter(|e| !e.is_dir)
        .map(|e| (e.name_str().to_string(), e.size))
        .collect();
    v.sort();
    v
}

#[test]
fn list_root_files_reports_root_files_with_sizes() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        let (n, entries) = list(sd, 16);
        assert_eq!(n, entries.len());
        assert_eq!(
            files_of(&entries),
            vec![("BOOK.TXT".to_string(), 20), ("NOTE.TXT".to_string(), 5)],
            "files in subdirectories must not be listed"
        );
        // directories (BOOKS, _PULP) are never listed: exactly the 2 files
        assert_eq!(n, 2, "directories must not be listed");
        assert!(entries.iter().all(|e| !e.is_dir), "is_dir is always false");

        // the list follows writes and deletes
        sd.write_file("NEW.TXT", b"abc").unwrap();
        let (_, entries) = list(sd, 16);
        assert_eq!(
            files_of(&entries),
            vec![
                ("BOOK.TXT".to_string(), 20),
                ("NEW.TXT".to_string(), 3),
                ("NOTE.TXT".to_string(), 5),
            ]
        );
        sd.delete_file("BOOK.TXT").unwrap();
        let (_, entries) = list(sd, 16);
        assert_eq!(
            files_of(&entries),
            vec![("NEW.TXT".to_string(), 3), ("NOTE.TXT".to_string(), 5)]
        );
    });
}

#[test]
fn list_root_files_applies_the_firmware_filters() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        // filtered out: wrong extension, leading '_', leading '.', plus an extra
        // (empty) directory next to the seeded BOOKS and _PULP
        sd.write_file("SKIP.BIN", b"skip").unwrap();
        sd.write_file("_HIDDEN.TXT", b"hid").unwrap();
        sd.write_file(".DOT.TXT", b"dot").unwrap();
        sd.ensure_dir("EMPTYD").unwrap();
        // kept: every listed extension
        sd.write_file("NOTES.MD", b"md").unwrap();
        sd.write_file("BOOK2.EPUB", b"epub").unwrap();
        sd.write_file("BOOK3.EPU", b"epu!!").unwrap();

        let (n, entries) = list(sd, 32);
        assert!(entries.iter().all(|e| !e.is_dir), "is_dir is always false");
        let names: Vec<&str> = entries.iter().map(|e| e.name_str()).collect();
        for hidden in ["SKIP.BIN", "_HIDDEN.TXT", ".DOT.TXT", "EMPTYD", "BOOKS", "_PULP"] {
            assert!(!names.contains(&hidden), "{hidden} must be filtered out, got {names:?}");
        }
        assert_eq!(
            files_of(&entries),
            vec![
                ("BOOK.TXT".to_string(), 20),
                ("BOOK2.EPUB".to_string(), 4),
                ("BOOK3.EPU".to_string(), 5),
                ("NOTE.TXT".to_string(), 5),
                ("NOTES.MD".to_string(), 2),
            ]
        );
        assert_eq!(n, 5);

        // truncation applies after filtering
        let (n, entries) = list(sd, 3);
        assert_eq!((n, entries.len()), (3, 3));
        assert!(entries.iter().all(|e| !e.is_dir));
    });
}

#[test]
fn list_root_files_truncates_to_buffer_and_handles_empty_card() {
    each_backend(true, |fx| {
        let (n, entries) = list(&fx.sd, 1);
        assert_eq!((n, entries.len()), (1, 1), "n = min(entries, buf.len())");
    });
    each_backend(false, |fx| {
        let (n, entries) = list(&fx.sd, 8);
        assert_eq!((n, entries.len()), (0, 0));
    });
}

#[test]
fn host_file_backend_is_backed_by_real_host_files() {
    let fx = host_fx(true);
    let sd = &fx.sd;
    let root = fx.host_root();

    // backend writes land on the host file system
    sd.write_file("HOSTW.TXT", b"via backend").unwrap();
    assert_eq!(std::fs::read(root.join("HOSTW.TXT")).unwrap(), b"via backend");
    sd.append_root_file("HOSTW.TXT", b"!").unwrap();
    assert_eq!(std::fs::read(root.join("HOSTW.TXT")).unwrap(), b"via backend!");
    sd.write_in_pulp("W.BIN", b"pulp").unwrap();
    assert_eq!(std::fs::read(root.join("_PULP").join("W.BIN")).unwrap(), b"pulp");
    sd.write_in_pulp_subdir("CACHE", "W2.BIN", b"sub").unwrap();
    assert_eq!(
        std::fs::read(root.join("_PULP").join("CACHE").join("W2.BIN")).unwrap(),
        b"sub"
    );
    sd.write_file_in_dir("BOOKS", "W3.TXT", b"dir").unwrap();
    assert_eq!(std::fs::read(root.join("BOOKS").join("W3.TXT")).unwrap(), b"dir");

    // backend delete removes the host file
    sd.delete_file("HOSTW.TXT").unwrap();
    assert!(!root.join("HOSTW.TXT").exists());

    // files created behind the backend's back are visible to it
    std::fs::write(root.join("EXT.TXT"), b"external").unwrap();
    assert_eq!(sd.file_size("EXT.TXT").unwrap(), 8);
    assert_eq!(read_all(sd, "EXT.TXT"), b"external");
    let (_, entries) = list(sd, 16);
    assert!(files_of(&entries).contains(&("EXT.TXT".to_string(), 8)));
}

// ---------------------------------------------------------------------------
// virtual storage: read counter
// ---------------------------------------------------------------------------

#[test]
fn counter_starts_at_zero_with_empty_log() {
    each_backend(true, |fx| {
        assert_eq!(fx.sd.read_count(), 0);
        assert!(fx.sd.read_log().is_empty());
        assert_eq!(fx.sd.pending_injections(), 0);
    });
}

#[test]
fn counter_records_every_read_variant_exactly() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        let mut b = [0u8; 8];
        assert_eq!(sd.read_file_chunk("BOOK.TXT", 4, &mut b).unwrap(), 8);
        assert_eq!(sd.read_file_start("BOOK.TXT", &mut b[..6]).unwrap(), (20, 6));
        assert_eq!(sd.read_file_chunk_in_dir("BOOKS", "A.TXT", 9, &mut b).unwrap(), 2);
        assert_eq!(sd.read_file_start_in_dir("BOOKS", "A.TXT", &mut b[..3]).unwrap(), (11, 3));
        assert_eq!(sd.read_chunk_in_pulp("BKMK.BIN", 0, &mut b).unwrap(), 8);
        assert_eq!(sd.read_chunk_in_pulp_subdir("CACHE", "P0.BIN", 7, &mut b).unwrap(), 5);

        assert_eq!(sd.read_count(), 6);
        assert_eq!(
            sd.read_log(),
            vec![
                rec("BOOK.TXT", 4, 8, 8),
                rec("BOOK.TXT", 0, 6, 6),
                rec("BOOKS/A.TXT", 9, 8, 2),
                rec("BOOKS/A.TXT", 0, 3, 3),
                rec("_PULP/BKMK.BIN", 0, 8, 8),
                rec("_PULP/CACHE/P0.BIN", 7, 8, 5),
            ]
        );
    });
}

#[test]
fn n_consecutive_reads_give_exactly_n_matching_records() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        const N: usize = 25;
        let mut expected = Vec::new();
        for i in 0..N {
            let offset = ((i % 5) * 4) as u32; // 0,4,8,12,16
            let len = 1 + (i % 3); // 1,2,3 -> always inside the 20-byte file
            let mut b = vec![0u8; len];
            assert_eq!(sd.read_file_chunk("BOOK.TXT", offset, &mut b).unwrap(), len);
            assert_eq!(&b[..], &BOOK[offset as usize..offset as usize + len]);
            expected.push(rec("BOOK.TXT", offset, len, len));
            assert_eq!(sd.read_count(), i + 1, "count after call {}", i + 1);
        }
        assert_eq!(sd.read_count(), N);
        let log = sd.read_log();
        assert_eq!(log.len(), N);
        assert_eq!(log, expected);
    });
}

#[test]
fn writes_appends_deletes_sizes_lists_and_ensures_are_not_reads() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.write_file("W.TXT", b"w").unwrap();
        sd.append_root_file("W.TXT", b"a").unwrap();
        sd.write_file_in_dir("BOOKS", "W.TXT", b"w").unwrap();
        sd.append_file_in_dir("BOOKS", "W.TXT", b"a").unwrap();
        sd.write_in_pulp("W.BIN", b"w").unwrap();
        sd.append_in_pulp("W.BIN", b"a").unwrap();
        sd.write_at_in_pulp("W.BIN", 0, b"z").unwrap();
        sd.write_in_pulp_subdir("CACHE", "W.BIN", b"w").unwrap();
        sd.append_in_pulp_subdir("CACHE", "W.BIN", b"a").unwrap();
        sd.ensure_dir("BOOKS").unwrap();
        sd.ensure_pulp_dir().unwrap();
        sd.ensure_pulp_subdir("CACHE").unwrap();
        sd.file_size("W.TXT").unwrap();
        sd.file_size_in_pulp("W.BIN").unwrap();
        sd.file_size_in_pulp_subdir("CACHE", "W.BIN").unwrap();
        list(sd, 16);
        sd.delete_file("W.TXT").unwrap();
        sd.delete_in_pulp("W.BIN").unwrap();
        sd.delete_in_pulp_subdir("CACHE", "W.BIN").unwrap();
        assert_eq!(sd.read_count(), 0);
        assert!(sd.read_log().is_empty());

        let mut b = [0u8; 2];
        sd.read_file_chunk("NOTE.TXT", 0, &mut b).unwrap();
        assert_eq!(sd.read_count(), 1, "only the actual read is counted");
    });
}

#[test]
fn failed_reads_are_recorded_with_the_failure_kind() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        let mut b = [0u8; 8];
        sd.read_file_chunk("NOPE.TXT", 3, &mut b).unwrap_err();
        assert_eq!(sd.read_count(), 1);
        assert_eq!(
            sd.read_log(),
            vec![ReadRecord {
                path: "NOPE.TXT".to_string(),
                offset: 3,
                requested: 8,
                returned: 0,
                outcome: ReadOutcome::Failed(ErrorKind::OpenFile),
            }]
        );
    });
}

#[test]
fn reset_clears_count_and_log_but_keeps_pending_injections() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        let mut b = [0u8; 4];
        sd.read_file_chunk("BOOK.TXT", 0, &mut b).unwrap();
        sd.read_file_chunk("BOOK.TXT", 4, &mut b).unwrap();
        assert_eq!(sd.read_count(), 2);
        sd.inject_short_read("NOTE.TXT", 1, 2);

        sd.reset_reads();
        assert_eq!(sd.read_count(), 0);
        assert!(sd.read_log().is_empty());
        assert_eq!(sd.pending_injections(), 1, "reset_reads must not drop injections");

        sd.read_file_chunk("BOOK.TXT", 8, &mut b).unwrap();
        assert_eq!(sd.read_count(), 1);
        assert_eq!(sd.read_log(), vec![rec("BOOK.TXT", 8, 4, 4)]);
    });
}

// ---------------------------------------------------------------------------
// storage failure injection: short read injection
// ---------------------------------------------------------------------------

#[test]
fn short_read_returns_the_injected_length_and_is_marked_in_the_log() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.inject_short_read("BOOK.TXT", 1, 3);
        assert_eq!(sd.pending_injections(), 1);

        let mut b = [0xEEu8; 10];
        let n = sd.read_file_chunk("BOOK.TXT", 0, &mut b).unwrap();
        assert_eq!(n, 3, "caller sees the injected short length, as Ok");
        assert_eq!(&b[..3], b"012");
        assert_eq!(&b[3..], &[0xEE; 7], "nothing written past the short length");

        assert_eq!(
            sd.read_log(),
            vec![ReadRecord {
                path: "BOOK.TXT".to_string(),
                offset: 0,
                requested: 10,
                returned: 3,
                outcome: ReadOutcome::ShortInjected,
            }]
        );
        assert_eq!(sd.pending_injections(), 0, "one-shot: consumed after firing");

        // the very next identical read is normal and full
        let mut b = [0xEEu8; 10];
        assert_eq!(sd.read_file_chunk("BOOK.TXT", 0, &mut b).unwrap(), 10);
        assert_eq!(&b, b"0123456789");
        assert_eq!(sd.read_log()[1], rec("BOOK.TXT", 0, 10, 10));
        assert_eq!(sd.read_count(), 2);
    });
}

#[test]
fn short_read_hits_only_the_nth_read_of_that_file() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.inject_short_read("BOOK.TXT", 3, 1);
        let mut b = [0u8; 4];
        // interleave reads of another file: they must not advance BOOK.TXT's nth
        assert_eq!(sd.read_file_chunk("BOOK.TXT", 0, &mut b).unwrap(), 4); // 1st
        assert_eq!(sd.read_file_chunk("NOTE.TXT", 0, &mut b).unwrap(), 4);
        assert_eq!(sd.read_file_chunk("BOOK.TXT", 4, &mut b).unwrap(), 4); // 2nd
        assert_eq!(sd.read_file_chunk("NOTE.TXT", 1, &mut b).unwrap(), 4);
        assert_eq!(sd.read_file_chunk("BOOK.TXT", 8, &mut b).unwrap(), 1); // 3rd -> short
        assert_eq!(sd.read_file_chunk("BOOK.TXT", 12, &mut b).unwrap(), 4); // 4th normal again
        assert_eq!(
            sd.read_log(),
            vec![
                rec("BOOK.TXT", 0, 4, 4),
                rec("NOTE.TXT", 0, 4, 4),
                rec("BOOK.TXT", 4, 4, 4),
                rec("NOTE.TXT", 1, 4, 4),
                ReadRecord {
                    path: "BOOK.TXT".to_string(),
                    offset: 8,
                    requested: 4,
                    returned: 1,
                    outcome: ReadOutcome::ShortInjected,
                },
                rec("BOOK.TXT", 12, 4, 4),
            ]
        );
        assert_eq!(b, *b"cdef", "last read delivered the normal bytes");
        assert_eq!(sd.pending_injections(), 0);
    });
}

#[test]
fn nth_counts_from_registration_not_from_creation() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        let mut b = [0u8; 4];
        sd.read_file_chunk("BOOK.TXT", 0, &mut b).unwrap();
        sd.read_file_chunk("BOOK.TXT", 4, &mut b).unwrap();
        sd.inject_short_read("BOOK.TXT", 1, 2); // the very next read
        assert_eq!(sd.read_file_chunk("BOOK.TXT", 8, &mut b).unwrap(), 2);
        let log = sd.read_log();
        assert_eq!(log.len(), 3);
        assert_eq!(log[0].outcome, ReadOutcome::Ok);
        assert_eq!(log[1].outcome, ReadOutcome::Ok);
        assert_eq!(log[2].outcome, ReadOutcome::ShortInjected);
        assert_eq!(log[2].returned, 2);
    });
}

#[test]
fn an_injection_that_has_not_reached_its_nth_stays_pending() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.inject_short_read("BOOK.TXT", 4, 1);
        let mut b = [0u8; 4];
        for i in 0..3u32 {
            assert_eq!(sd.read_file_chunk("BOOK.TXT", i * 4, &mut b).unwrap(), 4);
        }
        assert_eq!(sd.pending_injections(), 1);
        assert_eq!(sd.read_file_chunk("BOOK.TXT", 12, &mut b).unwrap(), 1);
        assert_eq!(sd.pending_injections(), 0);
    });
}

#[test]
fn multiple_short_reads_on_one_file_each_fire_once() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.inject_short_read("BOOK.TXT", 1, 1);
        sd.inject_short_read("BOOK.TXT", 2, 2);
        assert_eq!(sd.pending_injections(), 2);
        let mut b = [0u8; 6];
        assert_eq!(sd.read_file_chunk("BOOK.TXT", 0, &mut b).unwrap(), 1);
        assert_eq!(sd.read_file_chunk("BOOK.TXT", 0, &mut b).unwrap(), 2);
        assert_eq!(sd.read_file_chunk("BOOK.TXT", 0, &mut b).unwrap(), 6);
        assert_eq!(sd.pending_injections(), 0);
        let outcomes: Vec<ReadOutcome> = sd.read_log().iter().map(|r| r.outcome).collect();
        assert_eq!(
            outcomes,
            vec![ReadOutcome::ShortInjected, ReadOutcome::ShortInjected, ReadOutcome::Ok]
        );
    });
}

#[test]
fn short_read_on_start_reads_keeps_the_true_file_size() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.inject_short_read("BOOK.TXT", 1, 4);
        let mut b = [0xEEu8; 8];
        assert_eq!(sd.read_file_start("BOOK.TXT", &mut b).unwrap(), (20, 4));
        assert_eq!(&b[..4], b"0123");
        assert_eq!(&b[4..], &[0xEE; 4]);
        assert_eq!(
            sd.read_log(),
            vec![ReadRecord {
                path: "BOOK.TXT".to_string(),
                offset: 0,
                requested: 8,
                returned: 4,
                outcome: ReadOutcome::ShortInjected,
            }]
        );
    });
}

#[test]
fn short_read_paths_cover_dir_pulp_and_pulp_subdir_reads() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.inject_short_read("BOOKS/A.TXT", 1, 2);
        sd.inject_short_read("_PULP/BKMK.BIN", 1, 3);
        sd.inject_short_read("_PULP/CACHE/P0.BIN", 1, 4);
        let mut b = [0u8; 8];
        assert_eq!(sd.read_file_chunk_in_dir("BOOKS", "A.TXT", 0, &mut b).unwrap(), 2);
        assert_eq!(sd.read_chunk_in_pulp("BKMK.BIN", 0, &mut b).unwrap(), 3);
        assert_eq!(sd.read_chunk_in_pulp_subdir("CACHE", "P0.BIN", 0, &mut b).unwrap(), 4);
        assert_eq!(sd.pending_injections(), 0);
        let log = sd.read_log();
        assert_eq!(
            log.iter().map(|r| (r.path.as_str(), r.returned, r.outcome)).collect::<Vec<_>>(),
            vec![
                ("BOOKS/A.TXT", 2, ReadOutcome::ShortInjected),
                ("_PULP/BKMK.BIN", 3, ReadOutcome::ShortInjected),
                ("_PULP/CACHE/P0.BIN", 4, ReadOutcome::ShortInjected),
            ]
        );
    });
}

// ---------------------------------------------------------------------------
// storage failure injection: storage error injection
// ---------------------------------------------------------------------------

#[test]
fn injected_read_error_returns_the_injected_kind_only_for_that_read() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.inject_error(StorageOp::Read, "BOOK.TXT", 2, ErrorKind::ReadFailed);
        assert_eq!(sd.pending_injections(), 1);

        let mut b = [0xEEu8; 4];
        assert_eq!(sd.read_file_chunk("BOOK.TXT", 0, &mut b).unwrap(), 4); // 1st ok
        assert_eq!(&b, b"0123");

        let mut b = [0xEEu8; 4];
        let err = sd.read_file_chunk("BOOK.TXT", 4, &mut b).unwrap_err(); // 2nd injected
        assert_eq!(err.kind(), ErrorKind::ReadFailed);
        assert_eq!(b, [0xEE; 4], "failed read must not touch the buffer");
        assert_eq!(sd.pending_injections(), 0);

        let mut b = [0u8; 4];
        assert_eq!(sd.read_file_chunk("BOOK.TXT", 4, &mut b).unwrap(), 4); // 3rd ok again
        assert_eq!(&b, b"4567");

        assert_eq!(
            sd.read_log(),
            vec![
                rec("BOOK.TXT", 0, 4, 4),
                ReadRecord {
                    path: "BOOK.TXT".to_string(),
                    offset: 4,
                    requested: 4,
                    returned: 0,
                    outcome: ReadOutcome::ErrorInjected(ErrorKind::ReadFailed),
                },
                rec("BOOK.TXT", 4, 4, 4),
            ]
        );
    });
}

#[test]
fn injected_read_error_preserves_each_requested_kind() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        let kinds = [
            ErrorKind::ReadFailed,
            ErrorKind::SeekFailed,
            ErrorKind::OpenFile,
            ErrorKind::NoCard,
            ErrorKind::Other,
        ];
        sd.reset_reads();
        for kind in kinds {
            sd.inject_error(StorageOp::Read, "NOTE.TXT", 1, kind);
            let mut b = [0u8; 4];
            assert_eq!(sd.read_file_chunk("NOTE.TXT", 0, &mut b).unwrap_err().kind(), kind);
            let last = sd.read_log().pop().unwrap();
            assert_eq!(last.outcome, ReadOutcome::ErrorInjected(kind));
            assert_eq!(last.returned, 0);
        }
        assert_eq!(sd.read_count(), kinds.len());
    });
}

#[test]
fn injected_error_applies_to_the_other_read_variants_by_path() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.inject_error(StorageOp::Read, "BOOK.TXT", 1, ErrorKind::OpenFile);
        sd.inject_error(StorageOp::Read, "BOOKS/A.TXT", 1, ErrorKind::SeekFailed);
        sd.inject_error(StorageOp::Read, "_PULP/BKMK.BIN", 1, ErrorKind::ReadFailed);
        sd.inject_error(StorageOp::Read, "_PULP/CACHE/P0.BIN", 1, ErrorKind::OpenDir);
        let mut b = [0u8; 4];
        assert_eq!(sd.read_file_start("BOOK.TXT", &mut b).unwrap_err().kind(), ErrorKind::OpenFile);
        assert_eq!(
            sd.read_file_start_in_dir("BOOKS", "A.TXT", &mut b).unwrap_err().kind(),
            ErrorKind::SeekFailed
        );
        assert_eq!(
            sd.read_chunk_in_pulp("BKMK.BIN", 0, &mut b).unwrap_err().kind(),
            ErrorKind::ReadFailed
        );
        assert_eq!(
            sd.read_chunk_in_pulp_subdir("CACHE", "P0.BIN", 0, &mut b).unwrap_err().kind(),
            ErrorKind::OpenDir
        );
        assert_eq!(sd.pending_injections(), 0);
        assert_eq!(sd.read_count(), 4);
    });
}

#[test]
fn injection_on_one_file_leaves_other_files_untouched() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.inject_error(StorageOp::Read, "BOOK.TXT", 1, ErrorKind::ReadFailed);
        sd.inject_short_read("BOOKS/A.TXT", 1, 1);
        let mut b = [0u8; 5];
        // other files read normally and in full, injections stay pending
        assert_eq!(sd.read_file_chunk("NOTE.TXT", 0, &mut b).unwrap(), 5);
        assert_eq!(&b, b"hello");
        assert_eq!(sd.read_chunk_in_pulp("BKMK.BIN", 0, &mut b).unwrap(), 5);
        assert_eq!(&b, b"bookm");
        assert_eq!(sd.pending_injections(), 2);
        assert!(sd.read_log().iter().all(|r| r.outcome == ReadOutcome::Ok));
        // the same bare name under another directory is a different file
        assert_eq!(sd.read_file_chunk_in_dir("BOOKS", "A.TXT", 0, &mut b).unwrap(), 1);
    });
}

#[test]
fn clear_injections_drops_pending_ones() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.inject_short_read("BOOK.TXT", 1, 1);
        sd.inject_error(StorageOp::Read, "NOTE.TXT", 1, ErrorKind::ReadFailed);
        sd.inject_error(StorageOp::Write, "NOTE.TXT", 1, ErrorKind::WriteFailed);
        assert_eq!(sd.pending_injections(), 3);
        sd.clear_injections();
        assert_eq!(sd.pending_injections(), 0);

        let mut b = [0u8; 5];
        assert_eq!(sd.read_file_chunk("BOOK.TXT", 0, &mut b).unwrap(), 5);
        assert_eq!(sd.read_file_chunk("NOTE.TXT", 0, &mut b).unwrap(), 5);
        sd.write_file("NOTE.TXT", b"ok").unwrap();
        assert!(sd.read_log().iter().all(|r| r.outcome == ReadOutcome::Ok));
    });
}

#[test]
fn injected_write_error_returns_kind_and_leaves_storage_unchanged() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.inject_error(StorageOp::Write, "NEW.TXT", 1, ErrorKind::WriteFailed);
        let err = sd.write_file("NEW.TXT", b"abc").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::WriteFailed);
        assert_eq!(sd.file_size("NEW.TXT").unwrap_err().kind(), ErrorKind::OpenFile, "no file created");
        if fx.root.is_some() {
            assert!(!fx.host_root().join("NEW.TXT").exists());
        }
        // one-shot: the retry succeeds
        sd.write_file("NEW.TXT", b"abc").unwrap();
        assert_eq!(read_all(sd, "NEW.TXT"), b"abc");
        assert_eq!(sd.pending_injections(), 0);

        // an injected failure over an existing file keeps the old content
        sd.inject_error(StorageOp::Write, "NOTE.TXT", 1, ErrorKind::DirFull);
        assert_eq!(sd.write_file("NOTE.TXT", b"zz").unwrap_err().kind(), ErrorKind::DirFull);
        assert_eq!(read_all(sd, "NOTE.TXT"), NOTE);
        // injected write failures are not reads
        assert_eq!(sd.read_log().iter().filter(|r| r.path == "NEW.TXT").count(), 1);
    });
}

#[test]
fn injected_write_error_covers_pulp_write_and_write_at() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.inject_error(StorageOp::Write, "_PULP/BKMK.BIN", 1, ErrorKind::WriteFailed);
        assert_eq!(
            sd.write_at_in_pulp("BKMK.BIN", 0, b"XX").unwrap_err().kind(),
            ErrorKind::WriteFailed
        );
        assert_eq!(read_all_pulp(sd, "BKMK.BIN"), BKMK, "failed write_at must not modify the file");

        sd.inject_error(StorageOp::Write, "_PULP/BKMK.BIN", 1, ErrorKind::SeekFailed);
        assert_eq!(sd.write_in_pulp("BKMK.BIN", b"new").unwrap_err().kind(), ErrorKind::SeekFailed);
        assert_eq!(read_all_pulp(sd, "BKMK.BIN"), BKMK);

        // both consumed: writes work again
        sd.write_at_in_pulp("BKMK.BIN", 0, b"XX").unwrap();
        assert_eq!(&read_all_pulp(sd, "BKMK.BIN")[..4], b"XXok");
    });
}

#[test]
fn injected_append_error_leaves_the_file_unchanged() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.inject_error(StorageOp::Append, "NOTE.TXT", 1, ErrorKind::WriteFailed);
        assert_eq!(sd.append_root_file("NOTE.TXT", b"!!").unwrap_err().kind(), ErrorKind::WriteFailed);
        assert_eq!(read_all(sd, "NOTE.TXT"), NOTE);
        sd.append_root_file("NOTE.TXT", b"!!").unwrap();
        assert_eq!(read_all(sd, "NOTE.TXT"), b"hello!!");
    });
}

#[test]
fn injected_delete_error_keeps_the_file() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.inject_error(StorageOp::Delete, "NOTE.TXT", 1, ErrorKind::DeleteFailed);
        assert_eq!(sd.delete_file("NOTE.TXT").unwrap_err().kind(), ErrorKind::DeleteFailed);
        assert_eq!(sd.file_size("NOTE.TXT").unwrap(), 5, "file survives a failed delete");
        sd.delete_file("NOTE.TXT").unwrap();
        assert_eq!(sd.file_size("NOTE.TXT").unwrap_err().kind(), ErrorKind::OpenFile);
    });
}

#[test]
fn injected_file_size_and_list_errors() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.inject_error(StorageOp::FileSize, "NOTE.TXT", 1, ErrorKind::OpenFile);
        assert_eq!(sd.file_size("NOTE.TXT").unwrap_err().kind(), ErrorKind::OpenFile);
        assert_eq!(sd.file_size("NOTE.TXT").unwrap(), 5);

        sd.inject_error(StorageOp::List, "", 1, ErrorKind::OpenDir);
        let mut buf = [DirEntry::EMPTY; 16];
        assert_eq!(sd.list_root_files(&mut buf).unwrap_err().kind(), ErrorKind::OpenDir);
        let n = sd.list_root_files(&mut buf).unwrap();
        assert_eq!(files_of(&buf[..n]).len(), 2);
        assert_eq!(sd.read_count(), 0, "size/list failures are not reads");
    });
}

#[test]
fn injections_of_one_op_class_do_not_fire_for_other_classes_on_the_same_path() {
    each_backend(true, |fx| {
        let sd = &fx.sd;
        sd.inject_error(StorageOp::Write, "NOTE.TXT", 1, ErrorKind::WriteFailed);
        sd.inject_error(StorageOp::Read, "NOTE.TXT", 2, ErrorKind::ReadFailed);

        // append / size / read(1st) are not writes and must not trip the Write injection;
        // none of them may advance the Read injection's nth except the reads themselves
        sd.append_root_file("NOTE.TXT", b"!").unwrap();
        assert_eq!(sd.file_size("NOTE.TXT").unwrap(), 6);
        let mut b = [0u8; 8];
        assert_eq!(sd.read_file_chunk("NOTE.TXT", 0, &mut b).unwrap(), 6); // read #1 ok
        assert_eq!(sd.pending_injections(), 2);

        assert_eq!(sd.write_file("NOTE.TXT", b"x").unwrap_err().kind(), ErrorKind::WriteFailed);
        assert_eq!(sd.pending_injections(), 1);
        assert_eq!(
            sd.read_file_chunk("NOTE.TXT", 0, &mut b).unwrap_err().kind(),
            ErrorKind::ReadFailed
        ); // read #2 injected
        assert_eq!(sd.pending_injections(), 0);
        assert_eq!(sd.read_count(), 2);
    });
}


#[test]
fn purge_pulp_subdir_removes_the_files_then_the_directory() {
    let sd = VirtualStorage::memory();
    sd.ensure_pulp_dir().unwrap();
    sd.ensure_pulp_subdir("_ABC1234").unwrap();
    sd.write_in_pulp_subdir("_ABC1234", "PG000.IDX", &[1, 2, 3]).unwrap();
    sd.write_in_pulp_subdir("_ABC1234", "IMG0.BIN", &[4; 10]).unwrap();
    sd.ensure_pulp_subdir("_KEEP000").unwrap();
    sd.write_in_pulp_subdir("_KEEP000", "PG000.IDX", &[9]).unwrap();

    assert_eq!(sd.purge_pulp_subdir("_ABC1234").unwrap(), 2);
    assert!(sd.file_size_in_pulp_subdir("_ABC1234", "PG000.IDX").is_err());
    assert!(sd.file_size_in_pulp_subdir("_ABC1234", "IMG0.BIN").is_err());
    // another book's directory is untouched
    assert_eq!(sd.file_size_in_pulp_subdir("_KEEP000", "PG000.IDX").unwrap(), 1);
}

#[test]
fn purge_pulp_subdir_of_a_missing_directory_is_ok_and_idempotent() {
    let sd = VirtualStorage::memory();
    assert_eq!(sd.purge_pulp_subdir("_NOPE000").unwrap(), 0);
    sd.ensure_pulp_dir().unwrap();
    sd.ensure_pulp_subdir("_ABC1234").unwrap();
    assert_eq!(sd.purge_pulp_subdir("_ABC1234").unwrap(), 0);
    assert_eq!(sd.purge_pulp_subdir("_ABC1234").unwrap(), 0);
}
