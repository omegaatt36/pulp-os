# Final acceptance implementation report

Initial artifacts and boundary completed; the later all-text requirement
conflict is resolved and full regression now passes. Current boundary709/both links and regenerated exporter PASS; see
t10-final-supplement.md for durable current logs and manifest.

## Ownership and baseline

Initial diff `/private/tmp/t10-start.diff`; initial evidence
`/private/tmp/t10-evidence-start.md`. Added exporter
`host/src/bin/iansui-acceptance.rs`, stdlib PNG conversion
`scripts/pbm-to-png.py`, install.md and target artifacts. Controller amended
ownership to reader header drawing after isolated causality proof; snapshot
`/private/tmp/t10-reader-start.rs`, isolated source `/private/tmp/t10-header-source`.
Task-only review diff `tmp-briefs/t10-review.diff`.

## Golden blocker investigation and authorized correction

Original five changed framebuffer strip hashes predated T9. Restoring only old
header drawing in isolated source produced exactly the original 3105-line pin
`8cb6e31a70a32cdc05d30fab45ee0c7983da8cde6a55ba8bb45a0540f8d3f454`.
Actual command: `CARGO_NET_OFFLINE=true READER_REG_SRC=/private/tmp/t10-header-source
READER_REG_WORKDIR=/private/tmp/t10-header-regression bash
scripts/check-reader-regression.sh tree --golden-only`; exit0,
`target/accept-iansui/header-causality.log`.

Cause: historical header `chrome_font=None` used FONT_9X18; T8 prepared header
unconditionally selected generated chrome bitmap. Live minimal authorized
branch uses original draw_chrome_text for ASCII title with absent chrome font;
non-ASCII title retains prepared fallback. No TOC spacing change was implicated.
Existing immutable golden provides actual RED and exact GREEN; no new oracle.
`cjk_surfaces` 7/7 PASS (`header-cjk-green.log`), tree original golden PASS
(`header-golden-green.log`). All source tests and pin remain unchanged.

## Artifacts and commands

`CARGO_NET_OFFLINE=true cargo run -p pulp-fontconv --target aarch64-apple-darwin
--config 'unstable.build-std=["std","panic_unwind"]' -- --font Iansui-Regular.ttf
--license OFL.txt --upstream-url https://github.com/ButTaiwan/iansui
--out target/accept-iansui/packs`: exit0. `converter.log`.
Nine packs: 16,19,23,27,28,32,35,38,46px; total14,792,301 bytes;
12,665 scalars each. Individual bytes/hashes in pack-manifest.json and PROV.TXT.
Font SHA256 `7f1aa62e9dcbf40d0ce41a5d3f1e5ea602e66c295778ac6fefb6b84d8ed08bd5`;
OFL SHA256 `5d6d9d02d598daa2f178d0b577c55594892c5cf9c38566e2343cea049a371ac3`.
Convention1, format1. Fixture coverage25/26, known missing𪚥 U+2A6A5 in all nine.
No all-Unicode coverage claim.

`CARGO_NET_OFFLINE=true cargo run -p pulp-host --bin iansui-acceptance
--target aarch64-apple-darwin --config 'unstable.build-std=["std","panic_unwind"]'
-- target/accept-iansui/snapshots target/accept-iansui/packs`: exit0;
`export.log`. `python3 scripts/pbm-to-png.py target/accept-iansui/snapshots`.
Twenty PBM plus twenty PNG, 480×800; actual ReaderApp body/heading/title/TOC at
five sizes. Full/stitched equal, zero draw reads for every render. All full PNGs
visually inspected, legible, no blank/clipping. Largest body observation has two
pages. Duplicate heading is deliberate chapter title plus explicit Heading block.
Preparation logical font ops/bytes separately recorded in evidence.md and log;
not physical SD calls, no hardware latency claim. Install copy instructions in
install.md; no removable SD written.

## Initial gates and resolved conflict

One final required/offline boundary: exit0,706 tests; both offline firmware
links PASS. Board logic exit0,321+3 tests. Fontpack no_std exit0.
Logs `target/accept-iansui/final-{boundary,board,no-std}.log`; underlying link
logs `target/accept-host-{x4,c61}.log`. No duplicate broad host suite.

Offline regression both: exit1. Head83 PASS and original pin PASS. Tree fails
immutable `txt_font_change_while_reading_keeps_the_page_table_baseline_quirk`:
page1 vs expected3 at pagination.rs215. Its comment requires historical English
page-table offsets after font change; current raw-anchor rebuild changes them.
At the initial failure, controller notified and no pagination production or
oracle edited by this acceptance agent. Subsequent user-approved independent
oracle amendment and reviewed repairs are documented in the supplement.
`final-regression.log`, detailed `$TMPDIR/pulp-reader-regression/tree/test.log`.

## Failure matrix, memory and weakening

Exact requirement map and exercised failure matrix in evidence.md. Existing
actual independent RED/GREEN reports reused; no fresh decoder campaigns,
mutants, reference oracle or expected snapshots. Acceptance-agent tests read-only; later independent approved English oracle
amendment is disclosed in the supplement. Separate
controller-approved historical comment-ID hygiene disclosed in hygiene-report.md.
No commits or spec IDs added to code. git diff --check PASS.

C61 combined bitmap224KiB <=256KiB, CJK total compile-time <=336KiB;
metadata~80KiB persistent/~96KiB transient unmeasured. Degraded16KiB dense-page
failure recoverable; X4 BigBuf unbudgeted. Physical SD timing, display/schedulers,
Files/Home/Settings runtime and live heap remain hardware limitations. Host does
not establish PSRAM live heap sufficiency. Independent whole-change review pending.

User chose all-text rebuild. Full regression head83/tree90 and original golden
now PASS; current boundary709/both links and exporter PASS. Final regenerated
manifest and durable current logs recorded in supplement; broad review pending.
