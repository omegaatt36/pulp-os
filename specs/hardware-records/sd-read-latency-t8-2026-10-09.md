# sd-read-latency — T8 board verification, 2026-10-09

Verification started at 20:12:09 Asia/Taipei at the user's request. This is an
in-progress record; T8 remains unchecked until the required observations exist.

## Session identity

| Field | Evidence |
| --- | --- |
| Board | Product C61, USB serial/MAC `88:56:A6:CF:DA:90`, `/dev/ttyACM0` |
| Panel | Same product panel as previous record; physical model remains unconfirmed |
| Source HEAD | `5574ec95cb6ea87645f3a46ce6cb6fa7080cfb81` plus takeover working-tree changes |
| ELF SHA-256 | `189d5d23d19d1f10f3cb5ec9045f28178227f38dc7a1b07e0acbada1a06f2748` |
| App SHA-256 | `4492d788d6a86430d0e7da9affc09732ad59621e2a86f34862c9d825b64b3536` |
| Features | `board-onepage-c61,sd-metrics,partial-refresh`, offline |
| Software identity | All manifest source/artifact hashes rechecked and match |
| Tool/log channel | espflash 4.6.0 / USB-Serial-JTAG |
| SD | Boot reports 63,864,569,856 bytes, FAT32, runtime 10 MHz; previous card label `PULPSTICKY`, brand not recorded |
| AP | Not applicable to offline verification |
| Timing | Host UTC + monotonic timestamps when complete serial lines arrive; ZH2 human stopwatch observations recorded separately below |

Start-of-session Git status:

```text
 M host/Cargo.toml
 M kernel/src/kernel/scheduler.rs
 M specs/changes/sd-read-latency/spec.md
 M specs/changes/sd-read-latency/tasks.md
 M specs/hardware-records/2026-10-08-product.md
 M src/apps/reader/mod.rs
 M src/apps/reader/paging.rs
 M src/fonts/cjk.rs
?? "assets/\351\233\206\345\220\210\351\253\224.epub"
?? host/tests/cjk_capacity.rs
?? host/tests/reader_metrics.rs
?? specs/changes/sd-read-latency/progress.md
?? specs/changes/sd-read-latency/review.md
?? specs/hardware-records/sd-read-latency-t8-20261009T121115Z.raw.log
?? specs/hardware-records/sd-read-latency-t8-20261009T121115Z.timed.log
?? specs/hardware-records/sd-read-latency-t8-20261009T121209Z.raw.log
?? specs/hardware-records/sd-read-latency-t8-20261009T121209Z.timed.log
?? specs/hardware-records/sd-read-latency-takeover-2026-10-09.log
?? specs/hardware-records/sd-read-latency-takeover-2026-10-09.md
?? src/apps/reader/profile.rs
```

## Capture procedure

Live raw capture: [sd-read-latency-t8-20261009T121209Z.raw.log](sd-read-latency-t8-20261009T121209Z.raw.log); timestamped capture:
[sd-read-latency-t8-20261009T121209Z.timed.log](sd-read-latency-t8-20261009T121209Z.timed.log). Original source, installed packs and SD fixture contents
were not modified for this run. Existing fixture/pack provenance is in the
[product record](2026-10-08-product.md); contents have not been rehashed from
the board in this session.

For committing, the raw log's CRLF line endings were normalized to LF; ANSI
and message contents remain intact. Original byte-for-byte capture is retained
at `target/sd-read-latency-takeover/original-captures/` under the same filename.
The timestamped log and its referenced line numbers were unchanged.

The first attempted monitor at 20:11:15 used `--no-reset`. espflash still
connected using the flash stub, and skipping the subsequent app reset left
no running application output. That attempt was stopped and is not test
evidence. The corrected monitor omits that flag, deliberately resets the app
and captures the cold-cache restored session. No flash write occurred.

## Initial restored EPUB observation

`ASSEMB~1.EPU`, chapter 3, offset 0, size index 2: `reader: opening` arrives at
20:12:10.836, first `Ready` at 20:12:23.136 and the following page refresh
completion at 20:12:23.957. The host opening-to-page-refresh interval is
**13.121 seconds**. The refresh covers 480×794 pixels, 818 ms. The stage
window contains 751 scalars; this run alone cannot establish the <=700
visible-scalar preparation target or the physical button-to-visible-page
interval. Human glyph/loading observations remain pending.

## Acceptance status

| Check | Status | Evidence still needed |
| --- | --- | --- |
| K2 (ZH2 body, five sizes) | PASS | User reports all normal; indices 0–4 each reach Ready and complete a page refresh |
| K2 (full scope) | UNVERIFIED | CJK heading/book name/TOC coverage still needed |
| K3 | UNVERIFIED | U+2A6A5 `𪚥` hollow box and normal covered sample glyphs |
| K7 (ZH2, five sizes) | PASS | No allocation failures/panic in five completed opens; max PSRAM use 20,616 B |
| K7 (dense/boundary coverage) | UNVERIFIED | 700-scalar/dense EPUB memory evidence still needed |
| K9 | UNVERIFIED | Font-size change, position persistence, restart and same text anchor; log plus human observation |
| CJK EPUB opening | UNVERIFIED | Restored chapter reaches Ready and refresh; explicit Files open and human observation pending |
| R2 loading/input/heartbeat | UNVERIFIED | ZH2 loading visible per user; initial ordering and actual <=100 ms polling/<=5 s heartbeat evidence still needed |
| R3 latency (ZH2 case) | PASS | Initial two size-2 opens and subsequent five-size opens all below 10 s; 247 staged scalars |
| R3 latency (full scope) | UNVERIFIED | 700-scalar boundary and CJK EPUB case still needed |
| R3 session reuse | FAIL | Previously documented bitmap rereads/metric eviction/header reads conflict with unchanged literal requirement |

First human round completed: Files → `ZH2.TXT`, twice. User reports first
open **5 seconds**, second **3 seconds**, loading indication visible and
normal glyphs. Controller retains serial monitoring between rounds.

## Round 1 — ZH2 open/reopen (20:15 Asia/Taipei)

Both runs use book font index 2 (Medium), restore chapter 0 / raw offset 759,
and open the same 1,189-byte TXT. This is a restored-page opening, not a
zero-offset first-page or second preparation inside one retained Reader
session. Leaving Reader clears its metric/page caches.

| Measurement | First open | Second open |
| --- | ---: | ---: |
| User stopwatch | ~5 s | ~3 s |
| Host opening log → Ready | 5.603 s | 5.913 s |
| Host opening log → page refresh completion | 6.422 s | 6.731 s |
| Reader background work sum | 1.552 s | 1.356 s |
| Known intervals between steps, excluding entry interval | 4.041 s | 4.547 s |
| Staging API reads / lookups | 709 / 247 | 708 / 247 |
| Visible preparation API reads / bytes | 118 / 7,137 | 118 / 7,137 |
| Separate bank/header API reads / bytes | 1 / 44 | 1 / 44 |
| Staged / visible prepared scalars | 247 / 114 | 247 / 114 |

Capture line ranges: first opening 303, Ready 367, completed page refresh
370; second opening 387, Ready 448, refresh 451. Both final page refreshes
take 816 ms. The first large interval between reader steps is 3.897 s /
3.889 s respectively; these outside-step intervals include work such as
display refresh, not just scheduler sleep or SD reads. The user timings and
host intervals disagree, especially on the second run, and are retained as
distinct observations rather than substituting one for the other. Neither
host line-arrival timestamp measures the physical button edge.

Loading indication and normal body glyphs are confirmed by the user. Both
measured runs satisfy the 10-second bound for this <=700-scalar fixture.
This does not establish 700-scalar capacity on hardware, all-font-size visual
correctness, precise key polling, or the strict session-reuse guarantee:
both openings perform 247 fresh lookups and reread the visible bitmaps.

Independent round-1 extraction agrees with the intervals/read counts above.
Observed font heartbeat arrival gaps are 4.168 s and 4.676 s within the two
runs, both below 5 s; this is narrow host-receipt evidence, not a global
heartbeat or input-polling guarantee. Heap current usage is 10→13 KiB,
lifetime peak 42 KiB; stack free/high-water 67/17 KiB. Each run finishes with
15,156 PSRAM bytes used and 15,168 bytes reserved. No allocation failure,
reader error or panic is recorded in either opening interval.

The recorded return from the first TXT run to Files completes its list
refresh in 0.844 s (lines 371–379), compared with the historical ~20 s entry /
several-second return baseline. This is a host transition-to-refresh interval;
no separate user stopwatch or visual Files-list observation was requested.

## Round 2 — ZH2 five font sizes (20:19–20:21 Asia/Taipei)

User reports **all normal** after the five-size glyph/position check. The
actual operation in the capture is Reader → Home → Settings → Home → Reader,
changing Book Font and reopening ZH2 each time, rather than the initially
suggested in-reader quick-menu path. Each of indices 0–4 is recorded with
Ready and a completed page refresh; no source or pack changes occurred.

| Book Font | Open → Ready | Open → page refresh | Staged / visible scalars | Font API reads | Capture opening/Ready/refresh lines |
| --- | ---: | ---: | ---: | ---: | --- |
| XSmall (0) | 5.696 s | 6.602 s | 247 / 247 | 962 | 611 / 676 / 679 |
| Small (1) | 5.535 s | 6.442 s | 247 / 237 | 953 | 737 / 806 / 809 |
| Medium (2) | 5.357 s | 6.218 s | 247 / 174 | 889 | 866 / 931 / 934 |
| Large (3) | 5.301 s | 6.137 s | 247 / 133 | 847 | 992 / 1052 / 1055 |
| XLarge (4) | 5.258 s | 6.080 s | 247 / 89 | 803 | 1120 / 1180 / 1183 |

All times are host line-arrival intervals, without separate human timing for
this round. Read counts sum stage/prepare/bank records inside each open;
they are logical font API calls, not physical SD sectors. All are below 10 s
for this fixture. Maximum PSRAM used/peak is 20,616 B, reserved peak 20,624 B;
no allocation refusal, reader error or panic occurs in the round.

XSmall restores offset 759, then saves the containing page's start at offset
0. Subsequent sizes restore/save offset 0. This is consistent with reflow to
a larger containing page, but the capture does not independently establish
raw-anchor containment. No in-reader font-change or restart round is present.
The user's overall normal report is retained without inventing a specific
anchor phrase or treating this as a full K9 pass.

Next requested human round: return Book Font to Medium, explicitly open the
CJK EPUB from Files, time it and check book name/body/Contents, then go to the
next page, note the first line and verify chapter/text after leave/reopen.

## Round 3 — explicit EPUB opens and Contents (20:25–20:26)

The user reports that operations are generally **2–3 seconds**. Keep this
general experience separate from full-opening log intervals: the two Files
opens of `ASSEMB~1.EPU` (Medium, chapter 3 / offset 0) take 11.200 / 11.368 s
to Ready, then **12.020 / 12.189 s** to completed page refresh. Opening/
Ready/refresh capture lines are 1354/1491/1494 and 1546/1683/1686. Each open
has 751 staged entries and 96 visible entries, with 1,697 font API reads /
202,516 bytes. Neither proves the 700-entry boundary or strict reuse.

Contents opens at lines 1518 and 1762, with completed refreshes at 1523 and
1767: both approximately 1.132 s. Selecting `　還可以` at line 1770 jumps to
spine 4 and reaches Ready at 1853, page refresh at 1856: 3.583 s / 4.415 s
from the selection log. Title/body/Contents are operational; explicit visual
and position confirmation was requested but not received before the user
suggested closing the change. Both recorded leave/reopen bookmarks are
chapter 3 / offset 0; nonzero-anchor and restart persistence remain untested.
No allocation failure, reader error or panic appears in these three scopes.

The user subsequently suggested archiving. Serial recording was stopped with
SIGINT after preserving these results; the board continues running the same
firmware. Further human checks are not being requested while the intended
closeout scope is clarified. T8 remains unchecked under the original scope.

## Evidence limitations checked independently

The scoped T8 audit independently confirms the initial 12.300 s opening-to-
Ready and 13.121 s opening-to-refresh intervals. Normal input is sampled by
a separate Embassy task on the same executor, with nominal 10/50 ms timing;
the image emits no per-poll trace. Background-step wall time alone is not a
direct polling measurement. Synchronous cold pack opens can exceed the 60 ms
slice, so neither nominal slicing nor human button response proves the
100 ms bound. Supporting audit: `/tmp/sd-read-t8-evidence.md`.

The old capture only tested book size index 2 and contains no font-change/
restart round. For K9, record a unique phrase, change book font, check that
phrase remains on the new containing page, then leave/reopen and restart
after `bookmarks: flushed`. Matching saved/restored offsets establishes
persistence; human observation is still needed for page containment.

Local backup fixtures were inspected read-only; their identities are below.
These are **backup identities**, not a fresh hash of the live card. CJK counts
include CJK/fullwidth punctuation and exclude newline/Latin punctuation.
None contains the K3 test scalar U+2A6A5; a separate missing-glyph fixture is
needed for K3.

| Backup fixture (`~/onepage-backup-product/sd-test/`) | Bytes | Distinct CJK/marks | SHA-256 |
| --- | ---: | ---: | --- |
| `ZH1.TXT` | 43 | 14 | `61a5297459efc01ffa47c758be17e4d2c070cc41fa709eea755179ba527ae075` |
| `ZH2.TXT` | 1189 | 247 | `3275222c2f14b076f611e6b69548191aa0da3312694988a3050e9788d5ba6990` |
| `ZHTEST.TXT` | 27717 | 1443 | `9a2374ac6a7a80ff053d92014b8fcac28696a22315a8bf3fbaecc33a7068f1be` |
