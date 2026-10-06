# All-text layout rebuild acceptance amendment

Task: independently replace the tree-only English stale-offset bug-preservation
oracle with a raw-anchor/navigation/text-conservation acceptance test. Preserve
the historical head regression's old behavior assertion under not(feature=tree).
Ownership: scripts/reader-regression/os/tests/pagination.rs only. Production and
all other tests/oracles read-only. You are not alone; preserve other edits.

Binding requirement [human], user option1 2026-10-06: WHEN font identity, pixel
size or layout version changes THE SYSTEM SHALL rebuild incompatible pagination
for all text, including pure English. WHEN a live font-size change or resume
occurs THE SYSTEM SHALL select the new page containing the previous raw byte
anchor. New page numbers may differ. Preserve scalar boundaries, forward/back
consistency and complete source text exactly once. Do not preserve stale offsets.

Acceptance: navigate to a nonzero raw anchor in an English TXT, change size via
both real resume and quick-cycle callbacks. Assert new page contains anchor,
starts on scalar boundary, new configured geometry applies, back/forward agrees,
and whole forward page walk conserves all source content using source spans or
other requirement-derived observations. No replacement page number oracle.
Expected values come from literal source and required invariants, not running
new implementation. Do not read production implementation bodies or write a
reference pagination algorithm.

Capture requirement-derived test failing against isolated original task-start
production (pre identity implementation) or pre-port head using the same test
without bypassing assertions; live production is not to be restored. Existing
/private/tmp/t9-production-start source snapshots are evidence candidates.
Record actual RED command/output, GREEN against current production, start/end
diff/hashes, exact removed bug assertions and added stronger contract assertions.
No mutants/campaigns/skips/tolerances narrowed cases. No commits. Source spec IDs
must not appear in names/comments/errors. Historical existing comments need not
be rewritten except in edited blocks. Report english-anchor-tests-report.md.
