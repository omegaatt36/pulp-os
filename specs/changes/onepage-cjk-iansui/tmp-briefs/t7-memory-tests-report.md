# T7 font-memory test author report

Added only `board-logic/tests/font_memory.rs` and the memory contract/report.
No production, existing tests, CJK provider implementation or commits changed.
The expected values are the controller-approved literals in
`t7-memory-contract.md`. Tests exercise the real public MemoryBudget API.

Three tests cover:

1. Ready mode places two font reservations in PSRAM, enforces their shared
   256 KiB class cap, preserves accounting after refusal, releases charges,
   and accepts a fresh full-class reservation. ExternalClass conversion,
   MemClass external classification and ALL membership are checked.
2. Degraded mode selects internal memory, enforces 16 KiB, leaves accounting
   unchanged on refusal, and recovers after release. The PSRAM class limit is 0.
3. The physical 2 MiB, 192 KiB reserve, 256 KiB font class and unchanged
   768/512/64/256 KiB existing PSRAM limits match literals. All classes can
   reserve their complete approved allowances simultaneously within the pool.

## Actual RED

Ran once before class implementation:

```text
scripts/test-board-logic.sh --test font_memory
exit status: 101
Compiling pulp-board-logic v0.1.0
error[E0432]: unresolved imports
  pulp_board_logic::memory::INTERNAL_FONT_GLYPHS_BYTES
  pulp_board_logic::memory::PSRAM_FONT_GLYPHS_BYTES
error[E0599]: no variant, associated function, or constant named
  FontGlyphs found for enum ExternalClass
error[E0599]: no variant, associated function, or constant named
  FontGlyphs found for enum MemClass
error: could not compile pulp-board-logic (test font_memory)
  due to 26 previous errors
```

Every diagnostic was an absent approved constant/variant. No unrelated compiler
failure appeared. Compile RED is accepted by the scoped contract; the three
runtime tests have not yet executed. Formatting ran before the freeze.

## Frozen SHA-256

```text
359d6bcc2ffb7d3582b5e329c4c1d29ff79e513603b0f1beb09c4f318aa4d180  board-logic/tests/font_memory.rs
19944c8940ed1032df3a9c62f0803d4b6ec8454a64cadae7dc2007317755b3de  t7-memory-contract.md
```

The implementer was notified only after this freeze. Replay the same focused
command after production class wiring; test-author work stops at RED.
Final code review, GREEN evidence and broader existing checks belong to the
controller/implementer. No mutations or reference implementation were used.

## Author extension: existing pool fixture premise

After production wiring, the controller authorized a setup-only correction in
the existing inline test
`memory::tests::r14_pool_exhaustion_is_reported_when_the_class_still_has_room`.
The name supplied initially was different; inspection identified this actual
test. An initial invocation with the supplied name and `--exact` selected zero
tests and provides no evidence. The corrected filter below selected one test.

Original setup RED:

```text
scripts/test-board-logic.sh --lib r14_pool_exhaustion_is_reported_when_the_class_still_has_room
exit status: 101
running 1 test
called Result::unwrap() on an Err value:
ClassLimit { class: ChapterText, region: Psram, limit: 786432,
             used: 0, requested: 835584 }
test result: FAILED. 0 passed; 1 failed; 320 filtered out
```

Reserve funding changes the 1 MiB usable pool to 832 KiB. The setup's 816 KiB
ChapterText request now exceeds its unchanged 768 KiB cap. The author replaced
that one setup reservation with 768 KiB ChapterText plus 48 KiB ZipToc, keeping
total setup usage at 816 KiB and ImageData unused. The obsolete comment's
576 KiB literal was removed. No production functions, assertion expressions,
skips, tolerances or alignment tests changed.

Only setup diff:

```diff
-        // a smaller chip: 1 MiB -> pool = 1 MiB - reserve = 576 KiB < sum of limits
+        // a smaller chip: 1 MiB -> pool = 1 MiB - reserve < sum of limits
-        b.reserve(MemClass::ChapterText, pool - 16 * KIB, 4)
+        b.reserve(MemClass::ChapterText, PSRAM_CHAPTER_TEXT_BYTES, 4)
+            .unwrap();
+        b.reserve(
+            MemClass::ZipToc,
+            pool - PSRAM_CHAPTER_TEXT_BYTES - 16 * KIB,
+            4,
+        )
             .unwrap();
```

Post-correction focused evidence:

```text
scripts/test-board-logic.sh --lib r14_pool_exhaustion_is_reported_when_the_class_still_has_room
exit status: 0
test result: ok. 1 passed; 0 failed; 320 filtered out

scripts/test-board-logic.sh --test font_memory
exit status: 0
test result: ok. 3 passed; 0 failed
```

Both commands ran once after correction. The existing test still requires exact
PoolExhausted, then acceptance of the final 16 KiB and refusal of the rounded
one-byte request, so it still exercises the pool boundary rather than class
exhaustion. New font tests retain their original frozen hash.

Post-correction SHA-256:

```text
69401dafcc4c25786a91ca2206fd891d070965f7b4cbf946b9e8e29cb5c17fb0  inline pool-exhaustion test block
eb0b5325c67941598827014832a3997cabd326d8cae8de430e05d07c2c8eb144  board-logic/src/memory.rs (whole-file observation)
359d6bcc2ffb7d3582b5e329c4c1d29ff79e513603b0f1beb09c4f318aa4d180  board-logic/tests/font_memory.rs
```

The inline block hash covers its `#[test]`, function, and trailing blank line
up to the arithmetic-section comment. Whole-file ownership remains with the
implementer except this specifically authorized setup edit.
