# sd-read-latency — Takeover ledger

Baseline: `5574ec95`, branch `onepage`, 2026-10-09. The untracked EPUB in
`assets/` belongs to the user and is outside this change.

## Verified before edits

- T2/T4/T5/T7 focused baseline: `cargo test-host --locked --test cjk_sd_reads
  --test cjk_slicing` passed (20 tests).
- Current T4 already has ten-level reader / seven-level auxiliary index caches
  and tail-span reads. Current paging retains a paused text window. The three
  historical board logs do not measure these final optimizations.
- R3 remains open: the implementation retains only current-page bitmaps and
  bounded metrics. Returning to an older page reads bitmaps again; existing
  tests explicitly expect this. Header validation also reads the pack again.
- T3/T8 hardware work is pending: no `/dev/serial/by-id`, `ttyACM*`, or `ttyUSB*`
  device was visible inside the sandbox at takeover. Do not replace hardware evidence with host
  timing or mark the 700-scalar / 10-second target passed.

## Task handoffs

- T4 audit: independent read-only audit completed; handoff artifact
  `/tmp/sd-read-t4-audit.md`.
- T4 audit completed: final code already wires index caching; modeled cold
  700-scalar index reads are 931 calls / 801 blocks versus 8,888 / 7,090 for the
  plain path. This is host evidence, not SD timing. C61 visible slots currently
  hold 682 scalars; T4b will repair the 700-count metadata boundary.
- T2a completed; initial spec review found idle logging at the scheduler's
  10 ms tick and an unreliable outside-step interval after cancellation. Both
  were fixed, and independent spec and code-quality reviews now pass. Handoff artifacts:
  `/tmp/sd-read-t2a-result.md`, `/tmp/sd-read-t2a-spec-review.md`.
  Quality review: `/tmp/sd-read-t2a-quality-review.md` (no findings). Verification:
  24 focused feature tests / 22 default tests and both C61 checks passed.
- T4a completed: `spec.md` distinguishes historical captures from the final
  cached implementation. Historical `-3` stage gaps: TXT median 69.03 ms,
  unfinished EPUB median 151.23 ms; the latter reaches only 541/698 lookups.
- T4b completed: 700 metadata slots with target-specific allocation sizes;
  board aggregate and bitmap caps stay fixed. Implementation handoff:
  `/tmp/sd-read-t4b-result.md`.
- T4b independent spec and final quality reviews pass, with no new findings:
  `/tmp/sd-read-t4b-spec-review.md`, `/tmp/sd-read-final-quality-review.md`.
  The final security sweep reports no security MUST findings:
  `/tmp/sd-read-final-security-review.md`.

## Final verification and handoff

- `task acceptance`: exit 0. Five firmware variants, board/host/font/English
  regressions, ELF/dependency/memory checks and repository format checks pass.
  Log: `target/sd-read-takeover-acceptance.log`.
- Measurement-feature tests: `cargo test-host --locked --features sd-metrics
  --test reader_metrics --test cjk_capacity --test cjk_slicing --lib`: exit 0,
  18 tests. Log: `target/sd-read-takeover-metrics-tests.log`.
- C61 default ELF contains neither `reader-sd` nor `cjk-sd op=` markers;
  measurement ELF contains both. Changed/new file format checks and
  `git diff --check` pass.
- Measurement + partial-refresh ELF/app image prepared in
  `target/sd-read-latency-takeover/`; source/image hashes in `manifest.json`.
  Flashed on the user's subsequent request at 2026-10-09 19:56:08 Asia/Taipei;
  host access exposed the product board's `/dev/ttyACM0`. ota_0-only preflight
  and write succeeded, then FAT32/PSRAM/UI/kernel and Reader `Ready` were
  observed. Monitor stopped normally by SIGINT after verification.
- Durable acceptance review: `review.md`. New takeover code is reviewed;
  the full change remains open for literal R3 cache semantics and T3/T8 hardware
  timing, key responsiveness and visual checks. Do not archive yet.

## Subsequent board handoff

`specs/hardware-records/sd-read-latency-takeover-2026-10-09.log` preserves the
actual flash/boot and four successful opening episodes (restored CJK EPUB,
ZH1, ZH2, reopened CJK EPUB). Per-step sums are recorded in the adjacent `.md`;
they are not complete first-visible-page timings. T3 now has a completed EPUB
opening capture, while T3/T8 acceptance still needs the explicit visual,
input-response and target-boundary checks. The literal R3 cache blocker remains.
- T3/T8: retain pending status until board measurements and human observations
  exist.

## T8 verification started (2026-10-09 20:12 Asia/Taipei)

At the user's request, rechecked flashed artifact/source hashes and started
a timestamped serial capture of the same offline metrics/partial image.
Corrected espflash monitor startup resets the application without rewriting
flash. Restored EPUB chapter 3 reaches Ready and completes a page refresh;
host opening-to-refresh interval is 13.121 seconds, with 751 staged scalars.
This does not establish the <=700-scalar target or human visual acceptance.
First human round is complete: ZH2 opens twice at size index 2 / restored
offset 759, with user timings 5 s / 3 s, loading visible and glyphs normal.
Host opening-to-page-refresh intervals are 6.422 s / 6.731 s and are retained
separately from the user timings. Both are below 10 s for this 247-staged /
114-visible-scalar fixture; both perform 247 fresh metric lookups and bitmap
reads, so session-reuse acceptance is not claimed. Round 2 records ZH2 at all
five Book Font indices, user reports all normal, with open-to-refresh times
6.602/6.442/6.218/6.137/6.080 s and no OOM/panic (PSRAM peak 20,616 B).
Actual path is changing Settings and reopening, not the in-reader quick menu;
K9 raw-anchor/restart acceptance is not claimed. Explicit CJK EPUB open,
title/Contents and leave/reopen position checks are the next human round.
Session details and K2/K3/K7/K9 statuses are in
`specs/hardware-records/sd-read-latency-t8-2026-10-09.md`.
T8 remains unchecked and the literal R3 session-reuse blocker remains.

Round 3 records two explicit CJK EPUB opens (12.020/12.189 s to page refresh),
Contents (~1.132 s) and a selected chapter (~4.415 s), without OOM/panic.
User reports a generally 2–3 s experience; visual/nonzero-anchor/restart
acceptance is not inferred. User then suggested archiving; stopped only the
owned serial recorder and preserved the running firmware. Proposed narrower
closeout plus explicit deferred items are in `closeout.md`, awaiting the user's
scope decision. Original spec/tasks/review acceptance has not been relaxed.

## Guardrails

Preserve the PFN format, glyph output, page breaks, recoverable failures and
existing memory ceilings. Explain non-obvious constraints in comments; keep
requirement IDs and implementation/test mappings in change documents.
