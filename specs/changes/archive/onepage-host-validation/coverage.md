# Host validation coverage

Evidence is for this uncommitted change on the pinned toolchain described in
[host/README.md](../../../../host/README.md). Requirement IDs belong in this report,
not runtime code. Test-author red proofs, expectation origins and weakening
reports remain in `tmp-briefs/` for a future `/spec-archive` audit.

| Requirement | Evidence / replay command | Limit |
|---|---|---|
| R1 host boundary | `scripts/check-host-boundary.sh --skip-firmware`; dependency graph, verbose host linker/build checks | Final root boundary passed: 81-package graph, host236 and both firmwares |
| R2 production algorithms | `scripts/host-test.sh --locked`; path-included ReaderApp, paging, glyph rendering, StripBuffer and smol-epub; preview uses Rig::draw and Action::Next | Hardware edges are shims |
| R3 firmware builds | Root X4/C61 `--locked` builds exit 0, `/tmp/hv-final-{x4,c61}.log`; `scripts/check-host-boundary.sh` repeats both | Compilation does not validate hardware |
| R4 storage | `scripts/host-test.sh --test storage`: memory/host-file semantics, counters and read logs | Host filesystem differs from FAT |
| R5 injection | storage + reader_failures tests; required-page Error/ReadFailed/reopen and best-effort prefetch recovery; bookmark/settings dirty retry | Injection limitations below |
| R6 portrait artifact | render + reader_screens tests; `scripts/check-host-preview.sh`: P4 480×800 and complete nonblank raster | Portrait configuration only |
| R7 strip equality | render tests and CLI compare full-band with stitched firmware strips before writing | Full-frame interpretation below |
| R8 fixtures/regressions | fixtures, paging, utf8, reader_epub, reader_settings, reader_bookmarks, reader_failures, reader_screens tests; root reader regression head83/tree90, golden3105-line SHA matches pin | Codec/hardware/image gaps below |
| R9 reproducibility | `scripts/check-host-preview.sh`: eleven artifacts, exact second-directory match, selection/options/defaults/errors; README gives copy-paste commands | Same pinned inputs/target; no cross-platform identity proof |
| R10 uncovered behavior | This report and progress ledger record scope/observations | No hardware acceptance conclusion |

Root evidence before T7: all 236 host tests passed; both firmware builds passed;
reader regression head83/tree90 passed with golden SHA256
`8cb6e31a70a32cdc05d30fab45ee0c7983da8cde6a55ba8bb45a0540f8d3f454`;
C61 memory-budget checks passed (`/tmp/hv-final-c61-memory.log`). Final root boundary verification passed (host236, graph81, both firmware
builds); C61 memory checks also passed. T7 final focused CLI gate passed 26 assertions, including page/font/theme
option effects (`/tmp/t7-preview-final.log`); implementation full
host rerun passed all 236 tests (no skips/ignored), recorded in `tmp-briefs/t7-impl-report.md`.

## Known gaps

- Host defaults to board-x4 and heap BigBuf. It does not validate C61 PSRAM
  budget/runtime allocation, RTC session restoration, swap_buttons, physical
  buttons, SD timing, display refresh/ghosting, power or wake behavior. Memory
  budget checks are static evidence only. Partial-refresh/screen configurations
  beyond the tested portrait paths are not covered.
- Memory and host-file storage share deterministic semantics, not complete FAT
  emulation. Legacy FakeFs compares names without case sensitivity while virtual
  memory storage is case-sensitive; host-directory behavior depends on its
  filesystem. Permissions and natural filesystem errors are not an exhaustive
  error matrix. Write/append map host errors to OpenFile while firmware may use
  WriteFailed. Short injection length at least the normal length acts as success;
  simultaneous due injections trigger one; natural failure takes precedence over
  short injection. Missing-file creation and offset overflow in write_at_in_pulp
  follow production but lack tests. The historic PULP_DIR duplication gap is no
  longer applicable: host storage imports the production dir_entry constant.
- The full-frame reference means production StripBuffer rendered in different
  full-width bands, not an independent renderer or firmware full-page buffer.
  Equality establishes independence from tile shape. Its 64-row band size is a
  host scheduling choice. Pixel copying depends on documented strip byte layout;
  no public StripCore pixel accessor exists. A Deg270 mutation was detected by
  bounds panic, a weaker oracle than direct pixel mismatch. Interpretation of R7
  remains a recorded review question.
- OPF creator/title and NCX/nav TOC title entities remain undecoded by smol-epub
  (characterization test pins this behavior). Cover declaration is tested but no
  production cover-viewer API exists. TOC fragments and non-spine entries are not
  covered; parse_opf's trailing slash convention is exercised by real Rig opening.
- JPEG fixtures cover baseline single-component DC-only 8×8 flat blocks: no AC
  detail/IDCT stress, color, progressive/restart/EXIF or non-8-multiple dimensions.
  PNG covers filter 0, one IDAT and gray1/gray8/palette8; no general codec matrix.
  Thumbnail scaling covers 2× only. ZIP omits ZIP64, data descriptors, extras,
  non-ASCII filenames, archive comments and large archives. Text formatting after
  markup stripping includes characterization measurements rather than an
  independent typography oracle.
- Image behavior still has two observed production limitations: images near the
  page bottom can be clipped rather than moved/shrunk; reopening an image-bearing
  first page with preceding text can leave page_image None until navigation away
  and back. Cache progress/has_bg_work has no independent observation point.
  Preview drains bounded work but does not claim to repair those limitations.
- Legacy reader-regression Rig/probe/Kernel/storage forwarding remains duplicated;
  retaining HEAD comparison and run-software-acceptance usage is deliberate.
  Four existing unrelated import-format changes in sibling smol-epub
  src/{cache,jpeg,png,zip}.rs remain untouched.
- CLI black-box checks do not independently prove internal algorithm reuse or
  cross-chapter ordinal navigation. Source review and reader_epub tests supply
  that evidence; there is no implementation-derived pixel hash oracle.
