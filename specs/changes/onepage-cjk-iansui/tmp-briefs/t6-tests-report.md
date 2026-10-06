# T6 test-author report

Status: tests and interface complete; production implementation intentionally
untouched. Eight focused tests, no mutation campaign or reference implementation.
Read only public crate API, the prior interface contract, and existing tests/support;
did not inspect `reader.rs` or `missing.rs` implementation files.

## Red proof

Command run before the cache API existed:

```sh
cargo test -p pulp-fontpack --features builder --target aarch64-apple-darwin \
  --config 'unstable.build-std=["std","test"]' --test page_cache
```

Exit code: **101**. Actual output:

```text
   Compiling pulp-fontpack v0.1.0 (/Users/raiven_kao/dev/pulp-os/fontpack)
error[E0432]: unresolved imports `pulp_fontpack::CacheStorageError`, `pulp_fontpack::PageCache`, `pulp_fontpack::PageGlyphSlot`, `pulp_fontpack::PreparationError`
 --> fontpack/tests/page_cache.rs:9:5
  |
9 |     CacheStorageError, FontError, Metrics, PackError, PageCache, PageGlyphSlot, PreparationError,
  |     ^^^^^^^^^^^^^^^^^                                 ^^^^^^^^^  ^^^^^^^^^^^^^  ^^^^^^^^^^^^^^^^ no `PreparationError` in the root
  |     |                                                 |          |
  |     |                                                 |          no `PageGlyphSlot` in the root
  |     |                                                 no `PageCache` in the root
  |     no `CacheStorageError` in the root

For more information about this error, try `rustc --explain E0432`.
error: could not compile `pulp-fontpack` (test "page_cache") due to 1 previous error
```

This is the authorized missing-API red, not a behavioral green claim. All eight
tests are blocked by that missing public interface; rerun the same command after
implementation to exercise their assertions.

## Traceability and independent expectations

| Requirement | Behavior tests | Expected-value source |
| --- | --- | --- |
| R6 | `preparation_loads_distinct_page_glyphs_and_retrieval_needs_no_source`; `missing_glyph_is_prepared_as_the_size_specific_hollow_square`; `replacement_page_records_new_font_identity_and_empty_page_needs_no_io` | Requirement and controller-approved contract; golden hand-written metrics, bytes and offsets; 12×12 fallback border literal derived from the missing-glyph contract; second font ID 99 and size/advance 23 handwritten in the fixture. |
| R7 | `preparation_loads_distinct_page_glyphs_and_retrieval_needs_no_source` | Zero extra reads during four strip-like retrieval passes is the requirement. The source is disabled and reader dropped before retrieval. Duplicate-vs-unique read-log equality expresses required deduplication without assuming a binary-search midpoint implementation. Actual renderer integration is not tested or claimed. |
| R8 | `storage_budget_includes_reserved_metadata_bitmap_and_cache_object`; capacity/recovery tests check stable reserved footprint | Contract's full storage sum: target ABI `size_of` for cache object and all slots plus caller bitmap capacity. Exact budget accepted, one byte below rejected. Heap absence still requires production source review/no-default-feature validation; these tests do not install an unsafe allocation hook. |
| R9 | `metadata_capacity_failure_hides_old_and_partial_pages_and_allows_retry`; `bitmap_capacity_failure_does_not_read_overflowing_glyph_and_allows_retry`; `lookup_and_bitmap_io_failures_hide_pages_and_are_recoverable`; `corrupted_record_is_a_preparation_failure_and_a_replacement_reader_recovers` | Slot capacity 1 versus two distinct characters; bitmap capacity 5 versus literal lengths 2+4; injected source failures at literal index/bitmap offsets 88/135; independently corrupted 8×2 metrics for declared four bytes; no visible glyphs or identity on failure; successful same-object retry. |

No expected metrics/bitmap/error was derived by running production implementation.
Relative read-log comparisons test the requirement's deduplication relation rather
than adopting observed counts as an oracle.

## Interface decisions

Controller approved `info() -> Option<FontInfo>` and full invalidation on failure.
Controller then selected generic owned-or-borrowed storage handles to allow the
firmware to own boxed slices without self-reference or leaked app buffers. Tests
use borrowed slices and compute that concrete cache type's reserved footprint.
The full API and storage obligations are in `t6-contract.md`.

## Files and weakening gate

New files only:

- `fontpack/tests/page_cache.rs`
- `fontpack/tests/page_cache_support/mod.rs`
- `specs/changes/onepage-cjk-iansui/tmp-briefs/t6-contract.md`
- `specs/changes/onepage-cjk-iansui/tmp-briefs/t6-tests-report.md`

Existing test files were not edited. No removed or loosened assertions, skips,
widened tolerances, narrowed pre-existing tables, or replacements of existing
tested paths. The controllable source supplies real hand-written pack bytes to
the real public reader; it only counts reads and injects transport failures.
New tests were formatted with `rustfmt --edition 2024`. No spec IDs were added to
Rust code. No commits were made.

## Independent post-implementation weakening check

The original test author reread both current test files and compared their full
contents against the recorded creation patch, the generic-storage adjustment,
the pre-red read-count correction, and the recorded `rustfmt` step. The eight
test functions, all assertions, fixture literals, source controls, and helper
behavior match the authored files. No assertion removal or weakening, skip,
tolerance change, narrowed cases, or replaced call path was found. Neither test
file was rewritten during this check; no test suite was run.

Limitation: no file snapshot or SHA-256 digest was captured before implementation.
This is an independent comparison against the authoring record, not a
cryptographic pre/post snapshot comparison. The following current digests provide
a durable baseline for later checks, but cannot independently prove historical
byte identity before they were captured.

SHA-256 manifest, captured with `shasum -a 256`:

```text
d0898b379374e4bd77f4f9fad80fb4265f769d2cb4d8b9d96a7107d2ad505146  fontpack/tests/page_cache.rs
9d96faae7c59d666723488b87975d5cc6f91a1b7fc72d11c31ca792ef00e8712  fontpack/tests/page_cache_support/mod.rs
```
