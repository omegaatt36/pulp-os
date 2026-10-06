# T6 implementation report

Status: bounded page cache implementation complete; focused validation passed.
Controller owns final review, broader host validation, and renderer integration.

## Red proof and oracle

The independent test author recorded the pre-implementation missing-public-API
red in `t6-tests-report.md`: the focused command exited 101 with unresolved
imports for `CacheStorageError`, `PageCache`, `PageGlyphSlot`, and
`PreparationError`. This is the controller-authorized missing-API red.
The test author's expected metrics, bitmaps, fallback square, offsets, and
capacity failures came from the approved requirements/interface and handwritten
pack fixtures, not production output. I read that report, the exact contract,
the eight tests, and their controllable source before implementing the cache.

The first implementation compile failed with E0382 because reusing a generic
`PreparationError<E>` value incorrectly assumed `E: Copy`. Changing the local
bitmap capacity error to a constructing closure resolved that compile error;
no assertion or interface was changed.

## Implementation

- `fontpack/src/page_cache.rs`: generic storage handles for metadata and bitmap;
  complete reserved-capacity and cache-object budget accounting with checked
  summation; immutable zero-I/O glyph views; first-occurrence deduplication;
  present and synthetic missing-glyph preparation; exact capacity errors;
  unchanged font errors and no internal retry.
- `fontpack/src/lib.rs`: module declaration and public cache API exports.
- This report is the only additional implementation-stage file.

Preparation clears visible count and font identity before processing. The
in-progress count and bitmap extent stay on the stack, and visibility is
published only after all codepoints succeed. Thus an early capacity, I/O, or
corruption return leaves neither old nor partial glyphs visible. Empty pages
publish the new reader identity without reads. Buffers are reused on retry.

There is no reader/source member, page-sized temporary buffer, allocation,
unsafe block, or new dependency in the cache. The crate remains `no_std` and
forbids unsafe. Caller storage must expose full reserved backing capacity and
remain stable, as documented in the approved contract and public type docs.
Actual strip-renderer enforcement is deferred to integration; the cache API's
immutable retrieval cannot perform source I/O.

## Validation

Formatting:

```sh
rustfmt --edition 2024 fontpack/src/page_cache.rs fontpack/src/lib.rs
```

Exit 0. After the local compile correction, the new file was formatted again.

Focused tests:

```sh
cargo test -p pulp-fontpack --features builder --target aarch64-apple-darwin \
  --config 'unstable.build-std=["std","test"]' --test page_cache
```

Exit 0; actual output:

```text
   Compiling pulp-fontpack v0.1.0 (/Users/raiven_kao/dev/pulp-os/fontpack)
    Finished `test` profile [optimized + debuginfo] target(s) in 1.00s
     Running tests/page_cache.rs (target/aarch64-apple-darwin/debug/build/pulp-fontpack/d093fa7abcfc7923/out/page_cache-d093fa7abcfc7923)

running 8 tests
test bitmap_capacity_failure_does_not_read_overflowing_glyph_and_allows_retry ... ok
test storage_budget_includes_reserved_metadata_bitmap_and_cache_object ... ok
test missing_glyph_is_prepared_as_the_size_specific_hollow_square ... ok
test metadata_capacity_failure_hides_old_and_partial_pages_and_allows_retry ... ok
test replacement_page_records_new_font_identity_and_empty_page_needs_no_io ... ok
test preparation_loads_distinct_page_glyphs_and_retrieval_needs_no_source ... ok
test lookup_and_bitmap_io_failures_hide_pages_and_are_recoverable ... ok
test corrupted_record_is_a_preparation_failure_and_a_replacement_reader_recovers ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

Firmware no-std/no-default-features check:

```sh
cargo check -p pulp-fontpack --no-default-features --target riscv32imac-unknown-none-elf
```

Exit 0; actual output:

```text
    Checking pulp-fontpack v0.1.0 (/Users/raiven_kao/dev/pulp-os/fontpack)
    Finished `dev` profile [optimized + debuginfo] target(s) in 0.25s
```

The full host suite was not repeated; the controller owns that final gate.

## Weakening gate

Test files were read-only throughout this implementation. No old or new
assertions were removed, loosened, skipped, replaced, or adjusted. No tolerances
were widened and no expected-value tables narrowed. No mutation campaign or
reference implementation was created. No spec IDs were added to Rust code and
no commits were made.
