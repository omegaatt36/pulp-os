# T9 independent test and forwarding review

Verdict: APPROVE for the test/control gate. No findings.

Scope: `host/tests/cjk_identity.rs`, frozen `cjk_support` as fixture context,
and only the added `Rig::resume` / `Rig::quick_cycle` forwarding block in
`host/src/reader.rs`. Reviewed against `t9-contract.md`, `t9-cache-map.md`,
`t9-tests-report.md` and `t9-tests-manifest.txt`. Applied the bounded review-worker
checklist from `raiven-code-review`; no additional agents or implementation
review were performed. No source edits, RED rerun, broad suites or mutants.

## Verified test contract

- The PFNT fixture is built from literal bytes: 44-byte header, 22-byte records,
  advance at record offset 12, and three bitmap bytes per glyph. Replacement
  changes the literal font ID, advance and bitmap while preserving pixel size.
  Original advances 17/24 at widths 68/96 allow four scalars; replacement
  advances 34/48 allow two. Size index 1 uses the frozen 19px/advance20 fixture,
  allowing three scalars at width 68. No production wrapping result supplies
  these expectations.
- Each transition starts after three real Next actions at a nonzero raw anchor.
  `assert_anchor` requires `page_start <= old_anchor < page_start + visible_bytes`,
  scalar boundaries and exact agreement with the literal source slice. It does
  not require the old page number or old page start after repagination.
  The heading test preserves its two-byte opening marker in the raw source;
  its captured anchor and checked page lie inside the heading text.
- New literal bitmaps are checked through the frozen framebuffer observation
  helper. Each transition checks a real Prev/Next round trip from the rebuilt
  page. Body resume, live cycle and fresh-Rig bookmark restore additionally walk
  the complete TXT and require exact source conservation and scalar boundaries
  for all observed page offsets. The heading case checks its selected page and
  adjacent round trip rather than a full marked-source walk.
- `host/src/apps.rs` includes the actual production ReaderApp source in place.
  The approved controls call actual App callbacks, reapply the existing geometry
  override and use the normal settle/background path. Neither control contains
  pagination, cache invalidation, font preparation or anchor logic.
- Four ordinary tests remain enabled. No spec IDs, ignores, forced panic,
  alternate pagination, reference algorithm or relaxed assertions were found.

## Frozen evidence

Checked the actual original and frozen RED logs; they match SHA256
`09f80b653537a40e8a2da4fa5997075d134583174403df67aab7bf5148de0a14`.
The log records successful compilation, four executed tests, three behavioral
failures (stale four-scalar body/heading lines and live-cycle source omission)
and one passing bookmark regression control. This review inspected that saved
evidence; it did not independently rerun the command or reconstruct its source
baseline.

`shasum -a 256 -c SHA256SUMS` in `/private/tmp/cjk-snap/T9-tests` returned OK for
all four manifest entries. Live tests and fixture support byte-match their
frozen copies. The extracted live forwarding block also byte-matches the frozen
block, with SHA256
`a622703d16c92a677f286f73557c0a5931f0c7e53237f1abd2b4c872de6b747f`.
Test SHA256 is
`42376f2733b6e7051735a008214eba869495428d13b262bb8d64b7b2f6eef1a3`;
fixture SHA256 is
`c54f7693bfaec43d1c621ac935f79993b43b91c00e6c1b7ed034bb33a81d7393`.

## Gate and limits

Implementation must pass the frozen tests unchanged. Any justified oracle or
fixture correction requires independent test-author review, recaptured RED and
an updated manifest before implementation continues. No weakening was observed
at this review point.

This approval covers independent test validity and thin controls only. The
compiled layout-version key, full identity contents, font-independent EPUB
cache behavior and unchanged Latin behavior require the later implementation
review. The T9 literal text has no punctuation, so its kinsoku observation does
not add punctuation-case coverage.

## Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 0 | pass |
| HIGH | 0 | pass |
| MEDIUM | 0 | pass |
| LOW | 0 | pass |

Verdict: APPROVE — no issues found within the bounded test/control scope.
