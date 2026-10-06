# Pagination identity implementation

Production-start snapshots: `/private/tmp/t9-production-start/{mod.rs,paging.rs,cjk.rs}`; initial production diff `/private/tmp/t9-production-start.diff`. Task-only unified diff: `tmp-briefs/t9-review.diff`. No commits, test edits, mutants, or broad gates.

## Changes and root cause

Resume previously compared only selected size index. Same-size replacement therefore retained incompatible offsets. Live quick size changes left TXT offsets and fully-indexed state intact, producing missing text during the frozen navigation walk.

Reader now retains a layout key with validated body/heading font IDs, body/heading pixel sizes, an installed/absent bank discriminant and compiled `LAYOUT_VERSION`. The key is created lazily after encountering fallback scalars; pure Latin pages do not perform identity pack reads. The compiled constant is included by source inspection, not claimed runtime mutation evidence.

Key checks occur on resume and before background index/page reuse. Optional metadata lookup and validated PFNT header errors propagate. Missing optional banks have deterministic synthetic identity and cannot alias installed font ID zero. No handles or source objects are retained.

Live size change or changed key captures `byte_offset()` before reset. Existing `reset_paging()` resets the effective offset/continuation table, page count, completed-index state and prefetch tags. The saved raw anchor then drives the existing scan-from-zero page-containment restore path. Chapter identity remains unchanged; newly displayed page start may precede the anchor. Font-independent EPUB byte caches are untouched by invalidation. Prepared glyphs are refreshed using the existing pre-render path; strip draw remains source-free.

Thin English regression harness fix: reexport the actual `FONT_GLYPHS_PSRAM_BYTES` constant with existing BigBuf symbols. Its missing export initially blocked compilation. Controller explicitly included this fix in ownership; no independent test files or pinned expectations changed.

## Verification

Existing actual RED from independent author: three behavioral failures plus passing bookmark regression control, `/private/tmp/t9-identity-red.log` (frozen report).

`cargo test -p pulp-host --target aarch64-apple-darwin --config 'unstable.build-std=["std","test"]' --test cjk_identity -- --test-threads=1`: exit 0, 4 passed, log `/private/tmp/t9-identity-green.log`.

Same cargo command selecting `--test cjk_reader --test cjk_heading_pages --test paging --test reader_bookmarks --test reader_settings --test reader_epub`: exit 0; targets pass 1, 9, 24, 15, 31 and 19 tests respectively (99 total). Log `/private/tmp/t9-scoped-green.log`.

`bash scripts/check-reader-regression.sh tree --golden-only`: initially exit 1, missing harness constant export. After thin export: builds/runs 3105 lines but exit 1 against pin. Current hash `d5830acbcbd368c8cbbcb739ebe970b3a3867fef03034ea18955f72679e02d21`; pinned `8cb6e31a70a32cdc05d30fab45ee0c7983da8cde6a55ba8bb45a0540f8d3f454`. Logs `/private/tmp/t9-english-golden.log`, `/private/tmp/t9-english-golden-rerun.log`. Diff against accessible pre-port head trace consists only of five framebuffer strip hashes: TXT pages 0/3/60 and EPUB c0p0/c2p3; all pagination/navigation/settings/bookmark transcript lines match.

`git diff --check`: exit 0.

## Frozen-test weakening gate

SHA256 rechecked unchanged: identity test `42376f2733b6e7051735a008214eba869495428d13b262bb8d64b7b2f6eef1a3`; support `c54f7693bfaec43d1c621ac935f79993b43b91c00e6c1b7ed034bb33a81d7393`. Tests, controls, fixture metrics, expected counts/bitmaps, raw-anchor assertions, scalar boundaries and conservation/navigation assertions were not edited or relaxed.

## Concerns

English golden pin mismatch requires disposition; independent task-start comparison pending below. No claim of global golden PASS or final broad firmware acceptance. Identity assumes font pack producers change font_id when bank contents/metrics change, as required by the contract.

Task-start causality check: built isolated `/private/tmp/t9-start-source` overlay, replacing only the three T9-owned production files with captured start snapshots. No live production restoration. Initial new harness fetch failed DNS for crates.io; copied the existing resolved harness lockfile and retried with `CARGO_NET_OFFLINE=true READER_REG_SRC=/private/tmp/t9-start-source READER_REG_WORKDIR=/private/tmp/t9-start-regression bash scripts/check-reader-regression.sh tree --golden-only`. Result: 3105-line hash exactly `d5830acbcbd368c8cbbcb739ebe970b3a3867fef03034ea18955f72679e02d21`, same pin mismatch. `cmp` of task-start and final golden transcripts exits 0. Therefore all five changed framebuffer hashes predate T9; this task preserves the entire accessible task-start English transcript. Logs `/private/tmp/t9-start-golden.log` and `/private/tmp/t9-start-golden-offline.log`. Pin untouched; final acceptance must resolve earlier rendering changes.
