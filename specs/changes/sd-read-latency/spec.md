# sd-read-latency — Requirements

### R1: 量測先於修改
WHEN a CJK page or label is prepared on a C61 build with measurement enabled THE SYSTEM SHALL log, per preparation, the number of SD reads, their total and per-read time, and the number of distinct scalars prepared.
IF the measurement build is not enabled THEN THE SYSTEM SHALL NOT add logging cost to the default image.

### R2: 開啟含 CJK 的文字不得讓使用者以為死機
WHEN a book with CJK text is opened THE SYSTEM SHALL show a visible progress or loading indication before the first CJK preparation that may exceed 2 seconds.
WHILE font preparation is running THE SYSTEM SHALL keep polling keys at least every 100 ms and log a heartbeat at least every 5 seconds.

### R3: 準備時間有上限
WHEN a page of at most 700 distinct CJK scalars is prepared with the pack installed THE SYSTEM SHALL finish within a target set from the R1 measurement (initial target: 10 seconds on the C61 board, SD at 10 MHz).
WHEN the same scalars are requested again in the same session THE SYSTEM SHALL NOT read the pack again for scalars already prepared.

### R4: 行為不變
THE SYSTEM SHALL produce the same glyph bitmaps, metrics and page breaks as before for the same inputs.
IF the pack is absent, truncated or invalid THEN THE SYSTEM SHALL keep the existing recoverable-error and hollow-box behavior.

### R5: SD 與 EPD 共用匯流排安全
WHILE font preparation holds the SD handle THE SYSTEM SHALL NOT start an EPD refresh that interleaves with an SD transaction on SPI2.
IF the card is removed during preparation THEN THE SYSTEM SHALL return a recoverable error and not panic.

### R6: Files 與返回 Files 不得因 CJK 書名而卡住
WHEN Files is entered or re-entered with CJK-titled books on the card THE SYSTEM SHALL show the list within a target set from the R1 measurement (baseline: about 20 s on entry and several seconds on return from the reader, with one 3-glyph title shown for three books).
WHEN an English-only TXT or EPUB is opened THE SYSTEM SHALL NOT incur CJK pack reads (baseline: ENG.TXT and a re-opened ENG.EPUB open almost instantly).

Scenario (R3): Given `ZH2.TXT` (400 chars, 252 distinct) and the installed pack set, When it is opened from Files, Then the first page is shown within the R3 target and the log reports the R1 counters.

## Historical measurements (C61, SD 10 MHz, `sd-metrics` build)

Logs: `specs/hardware-records/sd-read-latency-*.log`. These captures precede the
final ten-level index cache and retained paused text-window implementation in
`87a01f96`; they are not latency measurements of the current code. Read counts
here are font `ReadAt` calls, not independently counted physical SD sectors.

Per index probe: ~1.1 ms average (max ~2 ms, one SD block); ~14 probes per looked-up scalar (~15 ms). Elapsed time was not stopwatch-timed: figures below are derived from the log counters and are estimates.

| Build | Cost per slice | Glyphs per slice | ZH2.TXT (247 distinct) |
|---|---|---|---|
| Original (T1 baseline, K2) | ~5 s per scalar, no slicing | n/a | >28 min unfinished |
| Single open per phase (`sd-read-latency-1`) | open ~190 ms (fixed) + ~15 ms reads, 3 opens per glyph in bitmap phase | 1 | staging alone sums to 50.43 s; total first-page time not established |
| Held pack handle (`-2`) | open ~0.1 ms | 1 (slice clock started before the page read) | ~20 s |
| Slice clock starts at first glyph (`-3`) | gap between slices ~69 ms, slice ~60-75 ms | 4-5 | ~9 s |

The 10-second R3 target remains unverified for the final implementation. The
historical uncached reader could not meet it; extrapolating its per-scalar cost
to the current cached reader is invalid.

Re-analysis of the `-3` capture: the 247-scalar TXT staging finishes 247 lookups
in 52 slices, with 3.467 s summed stage wall time and a 69.03 ms median
inter-call gap. The EPUB staging only finishes 541 of 698 lookups in 112 slices,
with 7.520 s stage wall time and a 151.23 ms median gap. Neither stage sum is a
first-page latency: bitmap preparation, work between stages and EPD refresh
must also be included. Historical `gap_us` includes prior font-log emission;
it is not a measurement of scheduler sleep alone.

## Takeover findings and host evidence (2026-10-09)

The actual firmware already calls `find_cached`, retaining ten search levels
for reader banks (4092 bytes per bank) and seven for auxiliary labels. Below
the cached levels it reads the remaining index interval in one bounded span.
It also retains a paused TXT/cache-file window instead of fetching it on every
slice. Keep these optimizations and measure their final combination on the
board before selecting another bottleneck.

`cargo test-tools --locked --test index_cache -- --nocapture` passes 17 tests.
For a cold 12,665-record synthetic pack, the existing 700-scalar test reports
931 cached API reads / 801 modeled 512-byte block fetches, versus 8,888 / 7,090
for plain search. The block model has one cached block and measures index
requests only. This is an approximately 89% reduction in modeled index block
fetches, not evidence that first-page rendering takes less than 10 seconds.

R3 repeat-read wording remains unresolved: exact-page reuse still validates
pack headers; changed visible sets reread bitmaps; bounded metrics can be
evicted, and suspend/exit clears caches. These are known differences from the
literal session-wide guarantee, not a passed acceptance criterion. A pending
user question asks whether the guarantee should apply only to retained glyphs.
The requirement above has not been weakened while that question is pending.

T4b fixes the metadata ceiling of 682 visible slots on C61: 700 slots now cost
16,800 bytes per role, adding 832 bytes across the two reader roles within the
unchanged 336 KiB firmware bound. The host models the same count with larger
pointer-sized metadata. The 700-scalar boundary must also fit the existing
per-role 64 KiB bitmap ceiling; oversized pages still fail recoverably. A
698-scalar staged window in the EPUB log is not proof of 698 visible bitmaps
fitting a page.

The new `host/tests/cjk_capacity.rs` production-path integration uses 700
scalars spread through a 12,665-record pack: cached staging issues 1,638 index
reads / 183,040 bytes versus 8,906 / 195,932 for plain search. The same counts
and glyph data hold after 233 pauses in each phase, reopening readers on every
slice. This is an 81.6% reduction in API calls for that fixture, not physical
SD sector counts. All 700 16×16 glyphs fit (22,400 bitmap bytes); the 32×32 case
(89,600 bytes) returns recoverable `BufferTooSmall`, publishes no old/partial
page and succeeds on a subsequent smaller preparation.

For the next measurement build, T2a adds `reader-sd` phase and resume records.
`text_us`, `layout_us`, `visible_us`, `pause_us`, instrumented `logging_us` and
`other_us` partition the completed background step. `outside_step_us` covers
work/wait/render between completed steps; first or cancelled boundaries are
marked `outside_step_known=false`. Idle reader ticks are silent. New font logs
use `gap_after_log_us`, excluding the preceding font-log emission; do not mix
that field with historical `gap_us`. Initial EPUB chapter extraction, other
app logs and EPD time still require their surrounding records to attribute
them. All profiling remains feature-gated; the default firmware has no added
clock reads or profile logs.

Full `task acceptance` passes after the takeover changes. Additional
measurement-feature host tests pass, and a C61 `sd-metrics,partial-refresh`
image was subsequently flashed at 2026-10-09 19:56:08 Asia/Taipei, with normal
boot and successful CJK EPUB/ZH1/ZH2 opening records. These captures do not
complete the latency, key or visual acceptance criteria. Build hashes, phase
sums and the remaining board checklist
are in `specs/hardware-records/sd-read-latency-takeover-2026-10-09.md`.

The subsequent timestamped T8 run records ZH2 at all five book font indices:
host opening-to-page-refresh times are 6.602, 6.442, 6.218, 6.137 and 6.080 s,
with 247 staged scalars and 247/237/174/133/89 visible prepared scalars. The
user confirms normal body glyphs at all sizes. Two earlier Medium opens were
timed by the user at approximately 5 s / 3 s and by host log arrival at
6.422 s / 6.731 s; these observations are retained separately. Visible loading
is confirmed, but exact loading order/input polling and whole-session reuse
are not passed. The 700-scalar hardware boundary and CJK EPUB visual/position
checks remain open. Full scope/evidence is recorded in
`specs/hardware-records/sd-read-latency-t8-2026-10-09.md`.
