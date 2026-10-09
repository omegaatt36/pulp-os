# Proposed closeout scope — awaiting user decision

The user considers the current improvement sufficient for archiving. This
document makes the proposed scope explicit; it does not change acceptance,
mark pending tasks complete or authorize archiving by itself.

## Verified result to close

- Keep the held font handles, upper-index caches, batched index tail reads,
  retained paused text windows, sliced preparation and visible loading.
- Retain default-off read/phase profiling and the 700 metadata-slot fix within
  the existing firmware/bitmap budgets; full software acceptance passes.
- ZH2 body renders normally at all five book sizes per user observation.
  Host opening-to-visible-refresh times are 6.080–6.602 s for those runs;
  initial user stopwatch observations are 5 s / 3 s with loading visible.
- CJK EPUB opens, Contents and chapter navigation complete without error.
  Full EPUB open-to-refresh is 12.020 / 12.189 s, Contents ~1.132 s and the
  recorded chapter selection ~4.415 s. The user reports a generally 2–3 s
  experience; it is not substituted for complete opening intervals.
- No allocation failure or panic occurs in the measured rounds. Captured
  PSRAM peak is 20,616 B and internal heap lifetime peak 42 KiB.

## Changes required to acceptance if this closeout is selected

R2 would claim visible loading and cooperative preparation supported by
the current host/board evidence; the precise loading-before-work order,
100 ms polling bound and global heartbeat bound would remain follow-up work.

R3 latency would be accepted for the measured ZH2 five-size cases; the general
700-scalar hardware target would remain a follow-up target. EPUB full-opening
latency would be recorded as measured, without a 10-second opening claim.

R3 reuse would describe the current bounded caches: matching retained metrics
and an unchanged retained visible page can be reused; header validation is
allowed. Page changes, eviction, font/bank identity changes and lifecycle
clears may cause reads again. The original unlimited session-wide no-rereads
promise would move to follow-up evaluation rather than be reported as met.

T3/T8 would be rewritten to the explicitly accepted measured scope, preserving
the original tasks and unverified checks in follow-up documentation. Passing
host glyph/metric/page-break and recoverable-error tests remain required.

## Follow-up items retained with evidence

1. Exact 100 ms input polling/loading-order/heartbeat hardware traces; cold
   pack open and blocking display work can exceed a nominal glyph slice.
2. 700-scalar hardware timing/capacity at the applicable bitmap sizes.
3. Repeat-cache contract and whether to add longer-lived bitmap retention
   within bounded memory, including header validation and lifecycle resets.
4. K3 missing scalar U+2A6A5, five-size EPUB heading/Contents coverage, and K9
   nonzero raw-anchor containment plus restart persistence.
5. EPUB open overhead: recorded full opens are ~12 s despite much shorter
   glyph work; compare metadata/TOC/text and display phases before optimizing.

If the user chooses to preserve the original requirements, these remain
completion work for this change, and the folder stays active. If the user
explicitly accepts this narrower closeout, first update spec/tasks/review and
preserve the deferred work, then verify the revised scope before archiving.
