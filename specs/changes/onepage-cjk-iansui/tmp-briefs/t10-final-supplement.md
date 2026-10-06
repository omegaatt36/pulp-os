# Final acceptance supplement

## Resolved all-text policy and approved oracle amendment

The user chose all-text rebuilding. R15 records [human] provenance; both resume
and live font-size callbacks rebuild incompatible pagination and select the page
containing the saved raw byte anchor. Page numbers may change. The historical
English stale-offset behavior no longer governs current-tree acceptance.

An independent author retained the entire old bug-preservation function for
historical head under cfg(not(feature="tree")). For tree, the obsolete page=3
and unchanged-offset expectations were replaced with requirement-derived raw
anchor containment, configured geometry, scalar/page/line boundaries, width and
line budgets, repeatable adjacent/reverse navigation and complete ordered source
conservation for both callbacks. No old head assertion was deleted. This is an
explicitly user-approved oracle amendment, not an unchanged-test claim; the
original golden fixture and pinned SHA256 remain untouched.

Frozen test SHA256: 454d1a92c96c0749d6ec82bcb70493a898d925e22f5f41424f3d1163168ce06e.
Current-production pre-repair RED failed adjacent navigation/layout consistency;
the repair refreshed the stale wrapped English page and passed the frozen test.
Independent review APPROVE: english-anchor-tests-review.md. Reports:
english-anchor-tests-report.md and english-anchor-fix-report.md.

Historical RED provenance was corrected: the isolated source used original
reader/paging with current-font compatibility helpers, not exact-original
production. An attempted original cjk snapshot copy targeted the wrong path;
actual src/fonts/cjk.rs SHA256 was
42f2be4e24026be1de6071ededc6462bc6c770a865e9be646ed4a4fdc8db20dc.
The unused original snapshot was disclosed separately. Independent evidence
re-review resolved that finding; actual current-production pre-repair RED is the
required red proof. No exact historical replay claim is made.

## Unused bank availability regression

Independent unused-bank tests captured three actual RED failures: corrupt unused
body, corrupt unused heading and reads of valid unused banks. The reviewed repair
records all-text pixel/layout identity independently and tracks validated pack
identities only after fallback metrics depend on the bank. Latin windows no
longer validate unused CJK packs; participating identities persist and continue
to detect same-size replacement. Frozen unused-bank SHA256:
72d094e6b9b2acb6b04d492af20d7236440206aee4321a9bbc2185cb3bc64b25.
Reports/reviews: unused-bank-tests-report.md, unused-bank-tests-review.md,
unused-bank-fix-report.md, unused-bank-fix-review.md (APPROVE).

Current full offline reader regression exit0: historical head83/current tree90
PASS; both golden traces3105lines with unchanged original SHA256
8cb6e31a70a32cdc05d30fab45ee0c7983da8cde6a55ba8bb45a0540f8d3f454.
Log /private/tmp/unused-bank-fix-both.log. Covering host75 PASS:
/private/tmp/unused-bank-fix-host.log. Focused English acceptance GREEN:
/private/tmp/unused-bank-fix-focused.log. No acceptance-agent cargo reruns.

## Current final boundary and regenerated artifacts

Controller current gate exit0:709 host tests, no forbidden host dependencies or
linker scripts, both X4/C61 firmware links PASS. Log:
target/accept-iansui/final-boundary-current.log. Board324 and no_std PASS from
recorded final-board.log/final-no-std.log remain valid.

Current exporter exit0 in export-current.log regenerated20PBM+20PNG at480×800,
full=stitched and every draw0reads. All original snapshot pixels/hashes and
logical preparation counts unchanged. Reader font ops/bytes by size:
1216/27335,1216/27767,1216/28119,1216/28887,1510/36641.
TOC:229ops each,5139/5290/5415/5681/6059bytes. No physical SD claims.
Durable current regression and covering evidence copied into acceptance artifacts:
final-regression-current.log (head83/tree90 and original pin PASS),
unused-bank-host-green.log (75 PASS).

SHA256SUMS regenerated to include all final logs/artifacts. Verification must
run from the artifact directory: `(cd target/accept-iansui && shasum -a 256 -c
SHA256SUMS)`. A first root-directory check failed solely because manifest paths
are relative; corrected check passed. Earlier64file manifest also checked OK.
Earlier failed/cancelled gate logs remain historical, not counted as passes.
All execution gates pass; final broad independent review remains controller-owned.

## Weakening accounting and limitations

Approved English oracle amendment and comment-only source-ID hygiene are
explicitly disclosed. All other frozen independent tests and original golden pin
remain unchanged. No fresh mutants, decoder campaign, generated correctness
oracle, commits or production changes from this docs-only supplement.

Generated observations remain separate from correctness oracles. Font coverage
is12,665scalars per pack; chosen fixture25/26, known missing𪚥 U+2A6A5.
Logical adapter read counts are not physical SD transactions or device timing.
C61 bitmap224KiB within256KiB class and compile-time CJK total<=336KiB are budget
arithmetic; metadata~80KiB persistent/~96KiB transient remains unmeasured.
Degraded16KiB dense-page capacity and X4 unbudgeted BigBuf remain limitations.
No live device heap/PSRAM, SD latency, scheduler or physical display claim.

Final manifest covers 69 files; SHA256 24cc93b83bdc98a9fa78023dee170c5fceb1d1d82b67d76a685999f63f4eeda4.
