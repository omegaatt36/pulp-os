# T7 independent code review

Verdict: changes required. **0 Critical, 3 Important, 1 minor follow-up.**
Important findings are verified correctness defects (MUST / HIGH). No security
or memory-corruption finding was identified. T7 is not accepted in this state.

Read-only review of T7 production additions and their call sites on 2026-10-06.
Loaded t7-contract.md, t7-memory-contract.md, t7-impl-report.md and
architecture-review.md; applied raiven-code-review. Existing T4 changes were
not attributed to T7. No tests, builds, mutations or production edits were run.
Only this report was written. Execution results below are attributed to their
authors/controller, not independently rerun by this reviewer.

## Important findings

### Important-1 — continued headings switch to body size on the next page

Files: src/apps/reader/paging.rs:503; src/fonts/cjk.rs:126.

Each copied page initializes heading/bold/italic as false, and metric staging
always scans from Regular. A heading opened on an earlier page therefore uses
body 16px rather than heading 23px on its continuation at size index 0. Backward
navigation and bookmark restore repeat that incorrect selection. This violates
R5 and the explicit heading-continuation contract addendum.

Fix: carry marker state at raw page boundaries through metric staging, wrapping
and visible preparation. Reconstruct the same state for indexing, backward loads
and restored-position scans; retain the raw-byte positions.

Verified: no paging state stores the inherited heading state. The independently
authored t7-heading-tests-report.md records actual behavioral RED: 0/1, with
page 2 Ready, raw heading-span offset and visible scalars passing before its
literal 23px bitmap assertion fails. Later navigation/restore checks have not
executed. This is a preexisting page-style reset newly material to T7's selected
CJK sizes, not a claim that T7 introduced the original reset.

### Important-2 — nested formatting inside CJK headings panics during draw

Files: src/fonts/cjk.rs:369; src/apps/reader/mod.rs:1677;
src/fonts/cjk.rs:426; src/apps/reader/paging.rs:510.

For stripped input `\x01H臺\x01B臺\x01b\x01h`, staging, wrapping and visibility
scanning keep heading precedence over bold and prepare only heading 23px `臺`.
The render loop unconditionally switches to Bold at `BOLD_ON` and to Regular
at `BOLD_OFF`, even while the heading is open. It then requests body 16px `臺`;
no body cache exists, so the new `expect("visible font glyph prepared")`
panics after successful preparation and Ready publication. Normal EPUB markup
such as a heading containing a strong/emphasis element can emit these markers.

Fix: share the same independent bold/italic/heading state and precedence between
staging, wrapping, visibility scanning and drawing. Do not hide the mismatch by
substituting an arbitrary glyph or silently swallowing a missing cache.

Verified: traced the exact marker sequence through all four code paths. Both
`臺` keys use heading 23px during staging; body visible count is zero;
`prepare_visible` leaves body None; draw switches to Bold and `get(16,'臺')`
returns None. Existing Latin rendering had a style disagreement; T7 adds the
runtime panic. No behavioral test was run by this reviewer.

### Important-3 — a group larger than PAGE_BUF truncates reachable text

Files: src/apps/reader/paging.rs:687; src/apps/reader/paging.rs:246;
src/apps/reader/paging.rs:316.

An inseparable group longer than the 8192-byte read window, for example `「`
followed by 3000 `」` scalars and then normal text, never creates a legal break:
every candidate following scalar is prohibited at line head. The new overhang
branch consumes the complete scalar-aligned window and emits one line. The
loader sees fewer than max_lines and marks the document fully indexed even
though next_offset is below file_size; cached chapter preindexing makes the
same decision. Page-forward then cannot reach the remaining input. Even text
within the emitted giant overhang is clipped outside the physical strip.

Fix: preserve forward progress when an inseparable group reaches the staging
window boundary. Do not treat a partially read window with spare line slots as
EOF; continue or explicitly handle the bounded-window limit while retaining all
scalars and the declared punctuation behavior.

Verified: PAGE_BUF is 8192. The sample has only two distinct fallback keys and
does not hit the metric/cache limits. `text_len` trims the last incomplete
scalar; the wrapper returns that complete prefix with line_count 1. TXT then
sets fully_indexed at lines 254-255; cached preindex breaks at 320-325. Both lack
an EOF condition on that branch. These branches predate T7, but the new unlimited
inseparable overhang supplies this new input path. No execution was run.

## Minor follow-up

- src/fonts/cjk.rs:204: every preparation discards both caches and reallocates
  character scratch, slot tables and bitmap backing. Ownership and refusal are
  safe, but page turns retain the allocation/fragmentation cost identified by
  architecture-review.md. Reuse bounded backing in a later scoped change; this
  does not gate T7.

## Compliance and memory assessment

- Bitmap backing is now BigBuf/FontGlyphs. Ready C61 placement reaches the private
  PSRAM heap through budgeted allocation; degraded placement has a shared 16 KiB
  bitmap allowance. No ordinary typed allocation registers PSRAM globally.
- The 256 KiB shared PSRAM font class is funded from reserve 448→192 KiB. Existing
  limits are unchanged; all class limits plus reserve equal 2 MiB.
- Persistent metric capacity is checked against 16 KiB; growth accounts for old
  and new buffers under 32 KiB. Each typed slot buffer has a 16 KiB cap and scratch
  a 4 KiB cap. Both 64 KiB bitmap roles, cache/state bookkeeping and transient metric
  growth participate in the compile-time 200 KiB ceiling. Internal peak is roughly
  68 KiB plus bookkeeping, rather than the earlier internal-only 160 KiB bitmap
  problem. Physical heap sufficiency is not established by host tests.
- Slot helpers inspect Vec capacity before boxing. X4 FontGlyphs zeroed buffers
  expose the Vec capacity to PageCache. C61 uses allocation-granule charges in
  the board budget. Clear-before-replace avoids old/new bitmap doubling.
- Storage access is confined to short-lived Source/PackReader preparation.
  Owned state retains no storage handle. Get/draw use immutable RAM views.
- Reader routes preparation failures into Error before Ready; partial role
  preparation cannot render through the Reader error gate. Reopen/exit clear
  owners. Public consumers must likewise draw only after complete success.
- Supported static Latin glyph/style selection remains authoritative. English
  without fallback opens no pack. The existing no-pack unsupported-CJK UTF-8
  test failures contradict T7's frozen missing-required-pack expectation; they
  remain a pending policy clarification, not an additional verified defect.
- T8 has the owned state, prepared view, foreground draw and widened advance
  seams. Its callers must stage matching sizes/styles and visible requests.
  `stage_metrics`/`prepare_text` currently assume Regular initial style; inherited
  heading callers need the correction in Important-1. No T8 surface work is
  required by this review.

## Evidence and attribution

The provided execution evidence records focused CJK 8/8, memory 3/3 and pertinent
existing paging/bookmark/settings/render 97/97. These passes do not exercise the
three defects above. Heading continuation has independently authored 0/1 RED.
No execution claim is made for nested headings or the long punctuation group.

This reviewer ran only source/diff reads and integrity commands. The original
T7 manifest verifies all five files OK. font_memory.rs retains SHA256
`359d6bcc2ffb7d3582b5e329c4c1d29ff79e513603b0f1beb09c4f318aa4d180`;
cjk_heading_pages.rs retains
`d3add6b8c7d34088a5ebde8ad9fd1aed119c992f1092513d129ddee9bf68c260`.
The author-only legacy pool-test change adjusts setup and preserves all original
assertions; it is a valid changed-policy premise correction.

## Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 0 | pass |
| HIGH (Important) | 3 | warn |
| MEDIUM | 1 | info |
| LOW | 0 | pass |

Verdict: WARNING — resolve the three Important correctness issues before T7
acceptance. Architectural direction and memory-policy integration are sound;
style consistency and bounded-window forward progress remain incomplete.

## Bounded source re-review after fixes and policy amendment

2026-10-06. This section supersedes the historical verdict above. No tests,
builds or mutations were rerun. **Current verdict: changes required — 0 Critical,
1 Important, 1 non-blocking allocation-churn follow-up.**

### Original findings closed

- Important-1: PageState owns bounded style_flags/indents tables, each 512 bytes.
  paging.rs:91 records consumed marker state at the next raw offset;
  stage_metrics_with_flags, wrapper initial state and visible span preparation
  receive those flags. Cached preindex selects its current page and resets back
  to page 0; normal loads, backward loads and restored-offset scans use the same
  tables. This closes the heading-role reset without changing raw offsets.
- Important-2: fonts/mod.rs:91 introduces the shared StyleState with independent
  marker bits and heading precedence. Metric scan, wrap, continuation recording
  and Reader draw now use it. mark_visible_with_flags validates missing keys
  through Result; draw no longer calls expect. The shared state semantics fix
  the original mismatch, rather than relying on the emergency draw box alone.
- Important-3: paging.rs:846 returns recoverable BufferTooSmall when a whole
  non-EOF window is an overhanging group without a safe boundary; earlier
  completed spans can make progress before retrying the group. TXT page creation
  at 291 and cached preindex at 368 now use next_offset and consumed progress,
  rather than treating spare line slots as EOF. This conforms to the explicit
  amended bounded-resource policy.

The controller attributes latest-source GREEN to the worker: amended reader 9/9,
heading 1/1, nested 1/1 and long-group 1/1. Their independent RED reports record the
heading bitmap failure, nested-render panic and 8220-byte false-Ready failure.
These execution results were not independently rerun here. Post-policy legacy
UTF-8 and golden-tree execution was still pending at this handoff.

### Important-4 — real SD metadata failures are misclassified as absent packs

File: src/fonts/cjk.rs:110.

The new absent-pack path treats OpenFile/OpenDir as proof that the optional bank
is not installed, then creates an empty pack and permits Ready with boxes.
Firmware storage.rs:44-51 maps **every** find_directory_entry failure to
OpenFile, while storage.rs:178-183 maps **every** parent/subdirectory open error
to OpenDir. This includes SD I/O failures while locating an installed pack.
The adapter has discarded the information that the new policy needs. Its
ReadFailed-only host fixture exercises bitmap/header reads, not this lookup
failure. The current implementation therefore violates the amended promise
that installed-pack storage failures remain preparation errors.

Fix: add or use a narrowly scoped font lookup that distinguishes definitive
not-found from actual directory/metadata I/O failure. Synthesize the empty bank
only for verified absence; propagate other failures. Preserve unrelated storage
callers' existing behavior.

Verified: traced KernelHandle::file_size_app_subdir at handle.rs:126 directly
to file_size_in_pulp_subdir at storage.rs:501 and its unconditional error maps.
The new cjk.rs match accepts both collapsed categories. No later error can
restore the discarded cause because this branch never reads the installed
source. This is a source-verified policy/error-handling defect, not speculation
about available hardware or a request for a broader storage refactor.

### Policy, weakening gate and memory

The no-pack compatibility choice is explicitly recorded as the controller's
default after an unanswered optional user choice, not as user confirmation.
spec/proposal and contract addenda transparently supersede the historical
absent→Error clause. Diff against the preserved original T7 snapshot confirms
only absent was removed from the failure table; corrupt/read assertions remain
identical. The added ninth test requires Ready, literal size-specific boxes,
text conservation, no draw reads and real glyphs after install/reopen. This is
a declared policy amendment, not silent weakening. The original snapshot and
manifest remain intact.

The amended no-pack manifest, heading/nested/long-group manifests all verify OK.
Reader/support and both host harness hashes match their historical values except
the explicitly amended reader test. Memory test hash remains 359d6bcc…4d180.

Font bitmap placement/class allowances and capacity accounting remain as
previously inspected: 256 KiB shared PSRAM, 16 KiB degraded, 192 KiB reserve, typed
metadata/scratch internal. The new two 512-byte style tables add exactly 1 KiB
internal Reader state; no per-page allocation is added. Even adding this to the
previous conservative 196 KiB payload ceiling leaves room below 200 KiB for the
small owner bookkeeping. No hardware heap-sufficiency claim follows. Source
diff-check passes; original pool-test setup correction still preserves all
assertions. T8's preparation/view seams remain usable, including foreground
draw; storage-free draw and recoverable Reader publication gates are retained.

Quality verdict: the three original correctness fixes are sound and narrowly
integrated. The new absence classifier must preserve the actual storage cause
before T7 can be accepted. Allocation churn remains a later non-blocking item.

### Current Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 0 | pass |
| HIGH (Important) | 1 | warn |
| MEDIUM | 1 | info |
| LOW | 0 | pass |

Verdict: WARNING — resolve Important-4 before T7 acceptance; the original three
Important findings are closed by source inspection.

## Final metadata-fix and oracle re-review

2026-10-06. This section supersedes earlier verdicts. **Scoped source review:
APPROVE — 0 Critical, 0 Important, 1 non-blocking follow-up.** No tests/builds
were rerun; execution evidence below is attributed to the controller/worker.

### Important-4 closed

kernel/drivers/storage.rs:503 adds a narrow optional lookup returning
Result<Option<u32>>. It returns None only for the actual embedded_sdmmc NotFound
variant from parent directory, font directory or directory-entry lookup. Other
directory failures return OpenDir; other entry failures return OpenFile. The
existing general file-size helper and its callers are unchanged.

Cleanup is complete: failure to open the first directory owns no handle;
failure to open the second closes its parent; every final lookup outcome closes
both handles before returning. The storage borrow remains scoped. The new
KernelHandle forwarder at 126 preserves this result without reinterpretation.

cjk.rs:107 creates synthetic Empty only for Ok(None). Ok(Some(len)) opens and
validates the installed source; Err propagates with font context. Corrupt packs,
metadata errors and later source reads therefore remain errors. There is no
remaining OpenFile/OpenDir-to-absence conversion at the font boundary.

The thin host driver forwards the same optional result. VirtualStorage retains
StorageOp::FileSize dispatch: explicit injections return Err before natural
absence checking, while the in-memory file store returns None for absence. This
preserves the independently authored metadata test's installed-pack OpenFile
failure instead of accepting it as missing.

### UTF-8 oracle weakening gate passed

Direct byte diff against saved original helper SHA256
`1c7991bb886fab1de05c1d1dbe533ad9865bb71443e20e17e61964785c7efcf4`
confirms only line_ink's advance oracle and explanatory comment changed.
Unsupported valid scalars on these no-pack proportional fixtures use literal
body advances [16,19,23,28,35]. Native glyphs and U+FFFD retain existing FontSet
advances; the new expected values are not derived from production CJK output.

Both ink checks, row scans, +2 tolerance, messages, all cases/assertions/skips
and every other helper remain unchanged. Relevant ink-check clients use normal
proportional open_book; forced-monospace cases do not call this oracle. This is
the explicit approved no-pack fallback policy reflected in the old draw oracle,
not a weakened assertion or a changed malformed-decoding expectation.

The new metadata test independently requires injection consumption, Error with
a cause, error-render full/strip parity and zero source reads during draw. The
four-file t7-metadata-tests manifest verifies OK after the source fix. Original
snapshots remain intact.

### Evidence and remaining scope

Independent author evidence records metadata 0/1 RED with consumed injection and
actual Ready versus Error. The controller reports fresh worker GREEN on the
landed source: metadata 1/1, amended reader 9/9, heading 1/1, nested 1/1 and
long-group 1/1, 13 focused cases total. The author separately reports 5 affected
legacy UTF-8 draw cases passing after the oracle amendment. Those execution
results are attributed, not independently rerun by this reviewer. The final
controller handoff additionally reports utf8_boundary 9/9 and
utf8_malformed 12/12 GREEN, and 3105 exact English/tree golden observations at
the pinned 8cb6… reference with exit 0; no further production edits followed.
git diff --check passes. No hardware success is claimed here.

The original three findings remain closed; memory limits, typed internal
metadata, PSRAM bitmap placement, the 1 KiB style-table addition and zero-source
draw direction remain sound by source review. Per-page allocation churn is the
only retained non-blocking follow-up. T9 cache identity is separately mapped in
t9-cache-map.md and does not widen this T7 review.

### Final Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 0 | pass |
| HIGH (Important) | 0 | pass |
| MEDIUM | 1 | info |
| LOW | 0 | pass |

Verdict: APPROVE. All four Important findings are resolved. The supplied final
focused, compatibility and pinned-golden evidence supports T7 acceptance;
hardware verification remains outside this review.
