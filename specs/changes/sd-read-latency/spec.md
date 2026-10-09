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

## Measured (C61, SD 10 MHz, `sd-metrics` build; logs in `specs/hardware-records/sd-read-latency-*.log`)

Per index probe: ~1.1 ms average (max ~2 ms, one SD block); ~14 probes per looked-up scalar (~15 ms). Elapsed time was not stopwatch-timed: figures below are derived from the log counters and are estimates.

| Build | Cost per slice | Glyphs per slice | ZH2.TXT (247 distinct) |
|---|---|---|---|
| Original (T1 baseline, K2) | ~5 s per scalar, no slicing | n/a | >28 min unfinished |
| Single open per phase (`sd-read-latency-1`) | open ~190 ms (fixed) + ~15 ms reads, 3 opens per glyph in bitmap phase | 1 | ~40 s |
| Held pack handle (`-2`) | open ~0.1 ms | 1 (slice clock started before the page read) | ~20 s |
| Slice clock starts at first glyph (`-3`) | gap between slices ~69 ms, slice ~60-75 ms | 4-5 | ~9 s |

R3 target kept at 10 s for a 700-scalar page, which is not met yet: at ~30 ms per scalar a 698-scalar CJK EPUB chapter is ~20 s. Remaining cost per scalar is ~14 probes (~90% of in-slice time) plus the ~69 ms gap per slice.
