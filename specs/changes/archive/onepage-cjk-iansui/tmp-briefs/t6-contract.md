# T6 contract: bounded visible-page preparation

This is a core API in `pulp-fontpack`, available without `builder`, allocation,
dependencies, or unsafe code. Reader/draw integration belongs to the next task.
The existing random-access and missing-glyph interfaces are defined in
`t3-contract.md`; their implementations were not inspected to author these tests.

Requirements, untagged in the specification:

- R6: WHEN 準備可見頁面 THE SYSTEM SHALL 先載入該頁所需的 metrics 與字形。
- R7: WHILE 繪製任何 strip THE SYSTEM SHALL 不執行 SD 字庫讀取。
- R8: WHILE 字形 cache 存在 THE SYSTEM SHALL 不超過設定的記憶體預算。
- R9: IF cache 無法容納必要頁面資料或 SD 讀取失敗 THEN THE SYSTEM SHALL 回報可恢復的 font preparation failure。

## Public API

```rust
#[derive(Clone, Copy, Default)]
pub struct PageGlyphSlot { /* private cache metadata */ }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStorageError {
    SizeOverflow,
    BudgetExceeded { needed: usize, budget: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparationError<E> {
    MetadataFull { needed: usize, capacity: usize },
    BitmapFull { needed: usize, capacity: usize },
    Font(FontError<E>),
}

pub struct PageCache<S, B> { /* owns storage handles, never stores a reader */ }
impl<S, B> PageCache<S, B>
where
    S: AsRef<[PageGlyphSlot]> + AsMut<[PageGlyphSlot]>,
    B: AsRef<[u8]> + AsMut<[u8]>,
{
    pub fn new(
        slots: S,
        bitmap: B,
        budget: usize,
    ) -> Result<Self, CacheStorageError>;
    pub fn storage_bytes(&self) -> usize;
    pub fn len(&self) -> usize;
    pub fn info(&self) -> Option<FontInfo>;
    pub fn prepare<R: ReadAt>(
        &mut self,
        reader: &mut PackReader<R>,
        codepoints: &[char],
    ) -> Result<(), PreparationError<R::Error>>;
    pub fn get(&self, codepoint: char) -> Option<Glyph<'_>>;
}
```

## Semantics

- Storage handles may be borrowed slices in tests or owned boxed slices in firmware.
  The cache crate introduces no allocation dependency; the firmware supplies storage.
  This avoids self-referential reader ownership or leaked per-app buffers. Storage
  lengths and backing memory must remain stable during the cache lifetime, and
  `AsRef` must expose the full reserved backing capacity. Handles with hidden
  allocations or unexposed capacity do not satisfy this storage contract; ordinary
  slices/boxed slices do. In particular, a shortened `Vec` must not be supplied.

- Reserved storage is exactly `size_of::<PageCache<S, B>>() + size_of_val(slots.as_ref()) +
  bitmap.as_ref().len()`. This counts **all** bitmap capacity, metadata capacity, metrics,
  padding and the cache object's own bookkeeping. `new` rejects a budget smaller
  than that sum, accepts equality, and reports the exact sum as `needed`.
  Checked summation failure is `SizeOverflow`. `storage_bytes()` reports that
  same constant reserved footprint throughout the cache's lifetime. No hidden
  heap or additional persistent page-sized storage is permitted. The reader,
  caller's input codepoint slice, and temporary preparation stack are outside
  this cache budget; the integration must budget those separately.
- Before preparation the cache is empty: `len() == 0`, `info() == None`, and
  all `get` calls return `None`.
- Every preparation replaces the previous page, even when using another reader.
  Successful preparation stores `reader.info()` (all fields, including font ID
  and pixel size), one slot per **distinct requested codepoint**, each requested
  glyph's metrics and complete bitmap. Requests are processed in first-occurrence
  order. Repeated codepoints consume no extra slot/bitmap and cause no repeated
  lookup or bitmap read. Linear deduplication is sufficient for this small core.
- Present glyphs use the reader API. Zero-sized glyphs are valid and need no
  bitmap bytes. Absent glyphs use the existing pure missing-glyph renderer and
  metrics for this reader's font information. Distinct missing codepoints each
  occupy one slot and one synthetic bitmap; sharing fallback bytes is outside
  this minimal contract.
- `get` returns only a glyph prepared for the current page; an unrequested
  codepoint is `None`. It accepts no source/reader, does no I/O, and returns slices
  borrowed from cache storage. It still works after the preparing reader is
  dropped. A consumer can copy the `Glyph` view and draw as many strips as needed.
  Actual strip-renderer enforcement is deliberately deferred to integration.
- Running out of slots returns `MetadataFull`: `needed` is the distinct count
  including the first new codepoint that cannot fit; `capacity` is slot length.
  This check precedes lookup of that new codepoint. Running out of bitmap bytes
  returns `BitmapFull`: `needed` is cumulative bitmap size including the first
  glyph that cannot fit; `capacity` is bitmap buffer length. It happens before
  reading/rendering that glyph's bitmap. Addition is checked; unrepresentable
  bitmap size maps to `BitmapFull { needed: usize::MAX, capacity }`.
- Any reader error is preserved as `PreparationError::Font(error)`, with no retry
  inside preparation. Corruption is also a preparation failure, never a fallback.
- On **any** preparation error, old and partial pages are invisible:
  `len() == 0`, `info() == None`, and every `get` is `None`. Storage contents need
  not be wiped. A subsequent `prepare` on the same object may succeed, including
  after replacing the reader or recovering an injected SD failure.
- Preparing an empty page succeeds, has zero glyphs, records this reader's font
  information, and performs no I/O.

## Tests and oracle

`fontpack/tests/page_cache.rs` uses the existing hand-written golden pack and a
new tests-only controllable source in `page_cache_support/mod.rs`. Metrics,
bitmap bytes, source offsets, and the 12×12 hollow missing-glyph square come
from literals and the previous format contract, never from production output.
Metadata-size expectations use `size_of` because target ABI padding is not a
font behavior. The parent controller accepted this API before implementation.
