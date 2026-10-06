# T6 review report

Compliance verdict: PASS for the approved T6 core contract (R6–R9).
Code quality verdict: APPROVE. Critical: 0; Important: 0; Minor: 0.
No actionable findings.

## Scope and evidence

Read the exact untagged R6–R9 requirements, `t6-contract.md`, independent
`t6-tests-report.md`, completed `t6-impl-report.md`, the complete cache source,
crate exports, page-cache tests/support, and reader/missing-glyph dependencies.
Inspected staged/working-tree changes and API references. The fontpack tree is
currently untracked, so Git provides no historical T6 test baseline; the
weakening assessment also relies on the independent author/implementer reports.
Existing `format.rs` and `names.rs` are byte-identical to the controller's
T3-start backups; `reader.rs` is byte-identical to `reader.rs.good`. The supplied
`t3mut/backup_missing.rs` path does not exist, so historical equality for
`missing.rs` could not be checked. Its current complete implementation was read.
No source/test edits, tests reruns, or mutants were performed in this review.
The implementer records 8 focused tests passed and the firmware no-default-
features check exited 0; these are attributed results, not reviewer reruns.

## Contract checks

- R6: First-occurrence deduplication prepares every distinct requested glyph's
  metrics and full bitmap. Missing glyphs use the existing pure size-specific
  fallback; corrupt/failed lookups propagate instead of falling back. Blank
  glyphs consume a slot and zero bitmap bytes.
- R7: `get` only searches stored slots and borrows bitmap storage. The cache
  contains no reader/source and retrieval remains valid after reader disposal.
  Tests disable/drop the reader and perform repeated strip-like retrievals.
  Actual renderer enforcement remains deliberately deferred to T7/T8; this
  review does not claim production rendering satisfies R7 yet.
- R8: Constructor checks the exact sum of cache-object size, full slot-array
  size (including metrics/padding), and full bitmap capacity. Checked addition
  reports overflow; equality succeeds. The stored footprint stays constant.
  There is no allocation, dependency, unsafe code, or persistent temporary page
  buffer. This guarantee assumes the approved storage-handle contract: full
  backing capacity exposed and stable lengths/memory; shortened Vec/custom
  hidden-allocation handles are outside that contract. Integration must separately
  budget the reader, input list, and preparation stack.
- R9: Visible length and identity are cleared before work and published only
  after complete success. All early capacity/I/O/corruption exits therefore
  hide old and partial pages and permit same-object retry. Metadata bounds are
  checked before lookup, bitmap extent is checked before bitmap I/O/rendering,
  and errors preserve exact cumulative capacity needs or the original font error.
  Buffer access uses checked slice retrieval. Count increment is bounded by a
  valid slot-slice length; cumulative bitmap addition is checked. No panic path
  from cache indexing/arithmetic was found under the storage contract.
- Successful replacement records all `FontInfo` fields, including font identity
  and pixel size; an empty page records the new identity without I/O.

## Hygiene and weakening gate

No spec/task IDs occur in the reviewed Rust source/tests. Requirement traceability
is confined to the contract/reports. Tests use literal golden data and independent
capacity/error expectations; relative read-log comparisons verify deduplication
without pinning binary-search internals. No weakened assertions, skips, widened
tolerances, narrowed tables, substitute implementation, or mutation campaign was
identified. The implementation report states test files remained read-only.

## Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 0 | pass |
| HIGH | 0 | pass |
| MEDIUM | 0 | pass |
| LOW | 0 | pass |

Verdict: APPROVE — T6 core contract satisfied; production renderer and whole-app
memory enforcement remain integration gates.
