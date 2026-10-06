# T10 independent review

Source/artifact verdict: APPROVE. No concrete code findings in the scoped
exporter, PBM-to-PNG converter or ASCII/None header compatibility branch.
Task-compliance verdict: CONDITIONAL; final acceptance remains pending the
controller/user disposition of the immutable English pagination oracle versus
the unqualified incompatible-layout invalidation requirement.

Reviewed t10-contract.md, t10-review.diff, live source/call sites, install.md,
evidence.md and the assembled t10-impl-report.md. Used raiven-code-review worker
checklist. No tests rerun, source changes, oracle edits or commits.

## Verified observations

- Exporter calls actual Rig/ReaderApp preparation and draw. It resets logical
  storage accounting between preparation, full and stitched draws; it asserts
  Ready/TOC phase, exact PBM equality and zero total draw reads before writing.
- ASCII title with absent chrome font uses historical FONT_9X18 path only;
  non-ASCII titles retain BitmapLabel prepared title view. Existing CJK title
  preparation and error handling are intact.
- All nine actual pack files match manifest byte lengths and SHA256; total
  14,792,301 bytes. PROV.TXT records source/converter/format identities and every
  pack hash. Actual OFL hash matches recorded source license hash; coverage
  declares 12,665 included scalars, not universal Unicode support.
- Ten actual full/stitched PBM pairs are identical, each 480x800/48,011 bytes.
  All twenty PNGs were decoded and their pixel bytes compared with their PBMs;
  all agree. Viewed reader size0, reader size4 and TOC size4: populated CJK
  title/heading/body/selected TOC, hollow missing-scalar boxes, no obvious crop
  or blank output in these representative extremes. Other sizes are backed by
  implementer's disclosed visual inspection and equality checks.
- Export log has separate preparation font operation/byte totals and zero
  full/stitched draw reads for all five sizes. Documentation accurately calls
  these logical VirtualStorage reads, without physical SD/latency claims.
- Actual tree golden artifact hashes to original immutable 3105-line pin
  8cb6e31a70a32cdc05d30fab45ee0c7983da8cde6a55ba8bb45a0540f8d3f454;
  header-golden-green.log corroborates it. Independent pagination/golden source
  diffs are empty.
- Existing failure matrix references missing/absent/corrupt/version, metadata
  and bitmap bounds, lookup/bitmap/metadata I/O and retry evidence. Documentation
  distinguishes exercised tests from source-inspected apps and unmeasured
  hardware. Combined bitmap budget, transient metadata, degraded 16KiB failure
  and unbudgeted X4 limits are disclosed.
- Installation gives all nine filenames, provenance/license retention, exact
  converter command and source hashes. Assembled implementation report adds
  exact exporter and PNG conversion commands. No SD-device write is claimed.

## Pending gate (not a newly discovered defect)

Read actual final logs: boundary PASS (706 host tests and both firmware links),
board PASS (321+3), no_std check PASS. Full regression both exits 1: head passes
83 tests and exact pin; tree fails
txt_font_change_while_reading_keeps_the_page_table_baseline_quirk. Tree golden
passes separately. The assembled evidence/report now disclose this correctly.
Do not claim T10 or whole-change acceptance PASS until the controller resolves
which requirement governs and completes the resulting gate. This review does
not authorize weakening the oracle or changing pagination.

## Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 0 | pass |
| HIGH | 0 | pass |
| MEDIUM | 0 | pass |
| LOW | 0 | pass |

Verdict: APPROVE scoped source/artifacts; task acceptance CONDITIONAL on pending
requirement disposition and gate. No new blocking source findings.
