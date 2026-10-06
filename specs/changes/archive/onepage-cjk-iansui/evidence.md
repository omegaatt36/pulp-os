# Acceptance evidence

驗證環境：branch `onepage` @f1c6539、toolchain 見 `rust-toolchain`、aarch64-apple-darwin。Replay 指令皆從 repo 根目錄執行。`/spec-archive` 重跑基準：`CARGO_NET_OFFLINE=true scripts/host-test.sh` exit 0、**719 passed／0 failed／0 ignored／0 SKIPPED**（真字型在場）；`(cd target/accept-iansui && shasum -a 256 -c SHA256SUMS)` 全 OK。

`H` = `CARGO_NET_OFFLINE=true scripts/host-test.sh`（可加 `--test <name>` 縮小）；`F` = `-p pulp-fontpack --features builder`；`C` = `-p pulp-fontconv`。Provenance 欄：`spec.md` 只有 R15 policy 標 `[human]`，其餘 requirement 從未標 tag，故為 **untagged**（見 Open items 1）。

| R-ID | Provenance | Test layer | Test path / name | Replay command | Red-proof |
|---|---|---|---|---|---|
| R1 host 轉換 | untagged | integration（真字型＋獨立 fontdue oracle） | `fontconv/tests/{provenance,reproducible,coverage_report,real_font,sizes,font_id,pack_content,cli_args,errors}.rs` | `IANSUI_REQUIRED=1 scripts/host-test.sh`（C 段） | T2：author 先在 workspace 缺 crate 時編譯失敗（progress T2 列）；`target/accept-iansui/packs/{PROV,OFL,COVERAGE}.TXT` 為產物 |
| R2 Unicode／大 offset | untagged | unit／golden／property | `fontpack/tests/{golden,lookup,corrupt,fuzz,header_record}.rs`；`fontconv/tests/roundtrip.rs` | `cargo test F`／`H` | T1：src 不存在時 4 個測試檔編譯失敗；mutation「`Pack::glyph` offset `& 0xFFFF`」→ 4 測試紅（progress T1 列） |
| R3 字庫 failure | untagged | unit／fault injection | `fontpack/tests/{corrupt,reader_faults}.rs`；`host/tests/cjk_reader.rs`（required_pack_failures） | `cargo test F`／`H --test cjk_reader` | T3：`PackReader`／`FontError` 等未解析 → 3 個 test target 編譯失敗（progress T3 段） |
| R4 缺字 fallback | untagged（policy 為 **暫定**，見 Open items 2） | unit＋integration | `fontpack/tests/missing_glyph.rs`；`host/tests/cjk_reader.rs`（absent_scalar、uninstalled_pack）；`host/tests/cjk_metadata_failure.rs` | `H --test cjk_reader --test cjk_metadata_failure` | T3 同上；no-pack policy：`tmp-briefs/no-pack-policy-tests-report.md`；metadata：`t7-metadata-tests-report.md` actual RED（Ready≠Error） |
| R5 字級一致 | untagged | integration（5 body＋5 heading 字級逐字級 literal） | `host/tests/cjk_reader.rs`；`fontconv/tests/sizes.rs` | `H --test cjk_reader` | T7：`tmp-briefs/t7-tests-report.md`（authored 先於 implementer，未修 production 紅燈） |
| R6 page preparation | untagged | unit＋integration | `fontpack/tests/page_cache.rs`（focused 8）；`host/tests/{cjk_reader,cjk_surfaces,cjk_surface_fixes}.rs` | `cargo test F --test page_cache`／`H --test cjk_surfaces` | T6：cache API 4 個 export 缺 → E0432、exit 101（progress T6）；T8 fixes：`t8-fixes-tests-report.md` RED |
| R7 draw 零 SD read | untagged | integration（read 計數＋full==stitched） | `host/tests/{cjk_reader,cjk_surfaces,cjk_surface_fixes,cjk_lifecycle}.rs`；`target/accept-iansui/snapshots/`（20 PBM＋20 PNG） | `H`；匯出見下方 Snapshot reproduction | lifecycle：`cjk_lifecycle::suspended_toc_resume_back_restores_original_body_glyphs_and_pixels` actual RED "literal 8x3 glyph missing at x=8 in line 2"；T8：`t8-tests-report.md` |
| R8 cache 預算 | untagged | 編譯期 assert＋算術 | `fontpack/tests/page_cache.rs`（capacity）；`board-logic/tests/font_memory.rs`（3） | `scripts/test-board-logic.sh`；`cargo test F --test page_cache` | **無獨立 red-proof 記錄**：board `font_memory` 與 compile-time assert 為算術；page_cache capacity 隨 T6 一併 RED。**未量測**：內部 heap metadata ~80–96 KiB、X4 BigBuf 未預算（見 Open items 4） |
| R9 recoverable failure | untagged | unit＋integration | `fontpack/tests/page_cache.rs`（metadata_capacity_failure、bitmap_capacity_failure、lookup_and_bitmap_io_failures）；`host/tests/{cjk_reader,cjk_surface_fixes}.rs` | `H --test cjk_surface_fixes --test cjk_reader` | `t6-impl-report.md` actual GREEN；`t8-fixes-tests-report.md`（T-A 標題載入失敗：loading 未清、未 redraw）RED |
| R10 禁則 | untagged（boundary policy 為 [human] 選項 1，spec 內文未標） | integration（literal 窄行 fixture＋內容守恆） | `host/tests/{cjk_reader,cjk_heading_pages,cjk_nested_styles,cjk_long_group,cjk_heading_latin_tail}.rs` | `H --test cjk_heading_latin_tail --test cjk_long_group --test cjk_nested_styles --test cjk_heading_pages` | `cjk_heading_latin_tail` 3 RED（heading capacity 2 vs 4）；closing-marker line 2 RED（`tmp-briefs/final-heading-tests-report.md`、`final-fix2-report.md`）；long group／nested：`t7-*-tests-report.md` |
| R11 文字位置 | untagged | integration | `host/tests/{cjk_identity,cjk_chapter_identity,reader_bookmarks}.rs` | `H --test cjk_identity --test cjk_chapter_identity` | T9：`cjk_identity` actual RED exit 101（3 行為失敗＋1 bookmark control PASS）；chapter：RED chapter(1,6) vs expected(1,0)（`t9-chapter-tests-report.md`） |
| R12 UTF-8 邊界 | untagged | boundary／oracle（`from_utf8_lossy`） | `host/tests/utf8_{boundary,title,measure,residue,epub}.rs`、`smol_{stream,toc,gaps}.rs`；`host/tests/cjk_surfaces.rs`（BitmapDynLabel） | `H --test utf8_boundary --test smol_toc` | T4：未修 production 上 26 紅；T5：46 測試 19 紅（progress T4／T5 列） |
| R13 malformed policy | untagged（maximal-subpart policy 為使用者於 T4 決定，spec 內文未標） | oracle＋偽隨機全比對 | `host/tests/{utf8_decoder,utf8_malformed,smol_entity,smol_malformed}.rs` | `H --test utf8_decoder --test smol_entity` | 同 R12（T4 26 紅、T5 19 紅） |
| R14 適用範圍 | untagged | integration（reader 標題／TOC／widgets）＋**source inspection** | `host/tests/{cjk_surfaces,cjk_surface_fixes,cjk_lifecycle}.rs`；Files／Home／Settings／manager／schedulers **僅 source inspection＋`cargo check-x4`／`check-c61`** | `H --test cjk_surfaces`；`cargo check-x4 && cargo check-c61` | T8：`t8-tests-report.md`；lifecycle RED 同 R7。**Files／Home／Settings 無測試、無 red-proof**（Open items 3） |
| R15 分頁失效 | **[human]** | integration（anchor containment＋全文守恆）＋golden | `host/tests/{cjk_identity,cjk_unused_bank}.rs`；`scripts/reader-regression/os/tests/pagination.rs`（head／tree 兩版）；`scripts/check-reader-regression.sh`（head83／tree90、golden 3105 行 sha `8cb6e31a…`） | `CARGO_NET_OFFLINE=true scripts/check-reader-regression.sh both` | `cjk_identity` actual RED（見 R11）；tree 新 oracle 於現行 production 實際 RED（`english-anchor-tests-report.md`）；`cjk_unused_bank` RED（`unused-bank-tests-report.md`） |
| R16 驗收證據 | untagged | artifact＋script | `target/accept-iansui/{packs/COVERAGE.TXT,fixture-coverage.json,snapshots/}`；`install.md` | `cargo run -p pulp-host --bin iansui-acceptance …`（見 Snapshot reproduction）＋`(cd target/accept-iansui && shasum -a 256 -c SHA256SUMS)` | **N/A（性質上不適用）**：artifact 非行為；使用者需明確接受（Open items 5） |

Firmware gate（R3／R8／R14 共用）：`IANSUI_REQUIRED=1 CARGO_NET_OFFLINE=true scripts/check-host-boundary.sh`（X4＋C61 firmware link；歷史 log `target/final-boundary2.log` exit 0）。**本次 archive session 未重跑此 gate**，只重跑 host 測試與 artifact hash。

Red-proof and weakening reports live in tmp-briefs and progress.md. Historical
mutants remain evidence only; 70 cancelled mutants were not executed or counted
as pass. No further mutant campaign is an acceptance condition.

## Real-font observations and provenance

Artifacts are under `target/accept-iansui/`: `packs/PROV.TXT`, `packs/OFL.TXT`,
`packs/COVERAGE.TXT`, `pack-manifest.json`, `fixture-coverage.json`,
`converter.log`, `export.log`, and `snapshots/`. Install/reproduction: `install.md`.
Font SHA256 `7f1aa62e9dcbf40d0ce41a5d3f1e5ea602e66c295778ac6fefb6b84d8ed08bd5`;
OFL SHA256 `5d6d9d02d598daa2f178d0b577c55594892c5cf9c38566e2343cea049a371ac3`.
Convention version 1, pack format 1. Nine packs total 14,792,301 bytes;
each contains 12,665 scalars. Fixture 25/26 covered; only absent scalar is
𪚥 U+2A6A5. Coverage does not mean all Unicode is supported.

Actual ReaderApp observations include body, chapter title + explicit Heading,
Chinese chrome title and selected Chinese TOC at all five sizes. Each full PBM
equals stitched PBM; all ten pairs are 480×800, with PNG copies inspected.
No blank page or clipping observed; largest body fixture spans two pages.
Snapshots are observations, not newly defined correctness oracles.

| Size index | Reader prepare font ops / bytes | TOC prepare font ops / bytes | Full / stitched draw reads |
| --- | --- | --- | --- |
| 0 | 1216 / 27335 | 229 / 5139 | 0 / 0 |
| 1 | 1216 / 27767 | 229 / 5290 | 0 / 0 |
| 2 | 1216 / 28119 | 229 / 5415 | 0 / 0 |
| 3 | 1216 / 28887 | 229 / 5681 | 0 / 0 |
| 4 | 1510 / 36641 | 229 / 6059 | 0 / 0 |

Reader preparation includes opening/layout/visible preparation, not only bitmap
fetches. Requested bytes equal returned bytes in these runs. Counts are logical
VirtualStorage adapter calls, not physical SD transactions; there is no hardware
latency measurement. Body+UI bitmap budget is 224 KiB within C61 256 KiB class.
Combined CJK compile-time total <=336 KiB; internal metadata ~80 KiB persistent,
~96 KiB transient remains unmeasured. Degraded C61 16 KiB and X4 unbudgeted BigBuf
limitations remain. Host tests do not prove device live heap sufficiency.

## Exercised failure matrix

| Case | Independent exercised evidence | Outcome |
| --- | --- | --- |
| Missing scalar | fontpack missing/page_cache hollow-square tests; cjk_reader absent_scalar; real 𪚥 snapshots | Explicit size-specific box |
| Optional absent pack | cjk_reader uninstalled_pack; no-pack-policy-tests-report.md | Ready with boxes; install/reopen recovers |
| Corrupt/version mismatch | fontpack corrupt/reader_faults; cjk_reader required_pack_failures | Recoverable error, no invisible Ready text |
| Metadata capacity | page_cache metadata_capacity_failure; t6-impl-report.md actual GREEN | Old/partial pages hidden; retry recovers |
| Bitmap capacity | page_cache bitmap_capacity_failure; board font_memory | Bounded allocation/read; retry recovers |
| SD bitmap/lookup failure | page_cache lookup_and_bitmap_io_failures; cjk_reader required_pack_failures | Recoverable error; prepared draw source-free |
| SD metadata failure | cjk_metadata_failure; t7-metadata-tests-report.md actual RED, t7-impl-report.md GREEN | Genuine errors propagate, absence distinguished |
| Title load error / retry | cjk_surface_fixes; t8-fixes-tests-report.md RED, t8-fixes-impl-report.md GREEN | Loading clears, redraw requested; reopen recovers |
| Incompatible body/heading layout | cjk_identity; t9-tests-report.md actual RED, t9-impl-report.md GREEN | Rebuild from raw anchor, preserve chapter |

## English golden causality and weakening gate

Five render-strip differences predated T9 and reproduced its task-start trace.
Header-only isolated restoration reproduced the unchanged original golden pin,
3105 lines SHA256 `8cb6e31a70a32cdc05d30fab45ee0c7983da8cde6a55ba8bb45a0540f8d3f454`.
Root cause was changing absent chrome font from FONT_9X18 to generated bitmap
chrome. The controller authorized preserving legacy ASCII/None header drawing;
prepared CJK title drawing remains. `header-causality.log`,
`header-golden-green.log`, `header-cjk-green.log` record actual results.
No TOC spacing attribution or expected golden hash update occurred.

Tests/oracles were read-only for the acceptance exporter and production repairs.
The independent user-approved English oracle amendment is disclosed below. Separately
approved comment-only hygiene is disclosed in `tmp-briefs/hygiene-report.md`;
comment-stripped executable equivalence was checked. Existing actual RED/GREEN
reports supply correctness evidence; no fresh mutants or snapshot oracle was
created. No commits or spec IDs were added to code.

## Initial gate history and current follow-up

Initial `IANSUI_REQUIRED=1 CARGO_NET_OFFLINE=true scripts/check-host-boundary.sh`: exit 0,
706 host tests, no forbidden host dependencies/linker scripts, both X4 and C61
firmware links PASS. Logs: `target/accept-iansui/final-boundary.log`,
`target/accept-host.log`, `target/accept-host-x4.log`, `target/accept-host-c61.log`.
`CARGO_NET_OFFLINE=true scripts/test-board-logic.sh`: exit 0, 321+3 tests;
`target/accept-iansui/final-board.log`.
`CARGO_NET_OFFLINE=true cargo check -p pulp-fontpack --no-default-features
--target riscv32imac-unknown-none-elf`: exit 0; `final-no-std.log`.

Initial `scripts/check-reader-regression.sh both` failed the historical English
stale-offset oracle (`final-regression.log`). The user selected all-text rebuild
and R15 now records this [human] policy. Independent author retained the complete
historical function for head under cfg(not(tree)), and replaced tree stale-page
and stale-offset expectations with raw-anchor containment, scalar boundaries,
repeatable navigation and complete source conservation for both callbacks.
This is an explicitly approved oracle amendment; the original golden pin remains
unchanged. Frozen tree test actually failed current production before repair,
then passed after repair. A separate unused-bank3 RED caught eager validation of
unused CJK banks; its reviewed repair tracks only participating bank identities.

Current full regression: exit0, head83/tree90 PASS, both 3105-line original golden
hashes unchanged. Log `/private/tmp/unused-bank-fix-both.log`; 75 covering host
checks PASS in `/private/tmp/unused-bank-fix-host.log`. Independent tests, repair
and review reports: `tmp-briefs/english-anchor-{tests,fix}-report.md`,
`english-anchor-tests-review.md`, `unused-bank-{tests,fix}-report.md` and
`unused-bank-fix-review.md`. Corrected historical provenance labels the isolated
RED original-reader/current-font compatibility, not exact-original production;
actual isolated cjk.rs hash was 42f2be4e24026be1de6071ededc6462bc6c770a865e9be646ed4a4fdc8db20dc.
The actual current-production pre-repair RED supplies the required red proof.

Current controller gate: exit0,709 host tests and both X4/C61 firmware links
PASS (`target/accept-iansui/final-boundary-current.log`). Current exporter exit0:
`export-current.log`; regenerated20PBM+20PNG retain every prior pixel/hash and
logical preparation count listed above; every full/stitched draw reads0.
Durable current full regression log: `final-regression-current.log`;
covering75 log: `unused-bank-host-green.log`. Board324 and no_std PASS remain
recorded above. Historical failed/cancelled logs are retained as history, not
counted as current passes. All execution gates pass; final broad independent
review remains controller-owned.

Snapshot reproduction:

```sh
CARGO_NET_OFFLINE=true cargo run -p pulp-host --bin iansui-acceptance \
  --target aarch64-apple-darwin \
  --config 'unstable.build-std=["std","panic_unwind"]' -- \
  target/accept-iansui/snapshots target/accept-iansui/packs
python3 scripts/pbm-to-png.py target/accept-iansui/snapshots
```

Artifact hashes: `target/accept-iansui/SHA256SUMS` refreshed with current logs
and verified from its containing directory. The initial root-directory check
failed because manifest paths are relative; corrected directory check passes.
Use `(cd target/accept-iansui && shasum -a 256 -c SHA256SUMS)`.
Final execution gates and generated observations pass; broad final review pending.

## Whole-change review follow-up (2026-10-06)

Two defects found by whole-change review, each with an independent RED test, a production-only fix, and re-verification. Reports: `tmp-briefs/final-heading-tests-report.md`, `final-fix-report.md`, `final-fix2-report.md`, `final-fix-review.md` (APPROVE). Frozen hashes: `final-lifecycle-tests.sha256`, `final-heading-tests.sha256`.

| Defect | RED test (actual failure) | Fix | GREEN |
| --- | --- | --- | --- |
| TOC suspend→resume→Back leaves body CJK glyphs as boxes (R14/R7) | `cjk_lifecycle::suspended_toc_resume_back_restores_original_body_glyphs_and_pixels`: "literal 8x3 glyph missing at x=8 in line 2" | `on_suspend` sets `render_fonts_released` for `Ready` and `ShowToc` | 2/2; draw reads 0, full == stitched |
| Open heading loses style on Latin-only continuation pages (R10 boundary) | `cjk_heading_latin_tail`: heading capacity 2 vs 4 chars on page 1 (3 RED) | per-page `CARRIED_FALLBACK` bit in `style_flags` | 5/5 |
| Closing-marker line of a carried heading drawn body-style (found by review of the fix) | pixel comparison vs reference rigs: "single-letter run: page 8 line 20 \"A\" … not drawn in heading style"; "suffix \"\": page 6 line 14 \"BBB \" …" | `wrap_proportional`: one `line_start_flags` predicate, `space_flags` captured at last_space | `cjk_heading_latin_tail` 8/8 |

Final gates after round 2 (controller re-run): `IANSUI_REQUIRED=1 scripts/check-host-boundary.sh` exit 0, 719 host tests, X4 and C61 firmware links PASS (`target/final-boundary2.log`); `scripts/check-reader-regression.sh both` exit 0, head 83 / tree 90, both goldens 3105 lines sha `8cb6e31a…` unchanged (`target/final-regression2.log`); `scripts/test-board-logic.sh` exit 0. All frozen test hashes verified unchanged.

Weakening gate: one assertion inverted by user-approved spec decision. `oracle_latin_heading_is_consistent_across_pages` (pure-Latin heading keeps heading style across pages) became `pure_latin_heading_resets_style_at_page_start`, because it contradicts the original English golden pin; spec R10 boundary policy was scoped to chapters that have staged fallback glyphs. No skip, tolerance, case-table or mock change otherwise.

Known residuals (not claims): the quote-indent of the closing-marker line in a carried `Q` block is stamped at emit time, not line start (reviewer-executed probe; indent is outside the scoped R10 style policy; same behaviour already exists in fallback windows and the pinned English path); carry latches on any fallback-staged scalar (Cyrillic, symbols), at window granularity, declared in spec R10; pure-Latin pages before the chapter's first fallback window keep English per-page reset; bit 7 can appear in a first line's `LineSpan.flags` (draw masks it).

## Open items 處置（使用者決定，2026-10-06）

1. Provenance：**不補標**，以 untagged 歸檔（R15 保留 `[human]`）。
2. R4：**維持合成方框 policy**（未安裝 pack 仍開書；已安裝但損壞才報 font failure）。由使用者確認，spec 本文不改。
3. R14：**接受限縮**——runtime 證據只涵蓋 reader 正文／heading／書名／TOC 與 widgets；Files／Home／Settings／manager／schedulers 僅 source inspection＋`cargo check-x4`／`check-c61`。
4. R8：**不處理**。無獨立 red-proof；internal-heap metadata 未量測、X4 BigBuf 未預算、無實機量測，維持現狀。
5. R16：red-proof 性質上不適用，**接受**。

仍有效的備註：本次 archive session 未重跑 firmware link gate（引用 `target/final-*2.log`）；T1–T5 的 weakening 快照 `/tmp/cjk-snap` 可能已不存在；`iansui.zip` 已追蹤並隨資料夾歸檔。

N verified / M open：16 條 R 皆列測試路徑與 replay；R8、R14（部分）、R16 無完整 red-proof，皆經使用者明確接受歸檔；0 條未決。
