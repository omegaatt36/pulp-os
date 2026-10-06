# T7 structural review (implementation in progress)

Read-only source inspection, 2026-10-06. This is architectural feedback to the
controller while the T7 worker is active, not final implementation approval.
No source/tests changed; no compilation or suites run.

## Architectural impact: Medium

The static Latin descriptor remains Copy. An app-owned CJK provider stages
metrics before the shared wrapping function and owns the visible bitmap caches.
The direction of dependencies is appropriate: the application integrates the
allocation-free fontpack core with a short-lived kernel storage adapter.

## Pattern compliance

- Confirmed: `wrap_lines_counted(k, ...)` stages the copied buffer's metrics
  before wrapping. Indexing, restore scans and visible loads use this same seam.
- Confirmed: bitmap requests come from final visible spans. Index-only loads
  pass `visible=false`; the final last-page/restore result gets preparation.
- Confirmed: `Source` borrows the handle only during preparation. Neither
  `CjkState` nor its caches retain a PackReader or a kernel handle.
- Confirmed: immutable draw resolves `CjkState::get` and borrowed cached bytes;
  this path has no storage capability.
- Confirmed: `on_enter` and `on_exit` release app-owned state; suspended reader
  background work does not touch CJK caches.
- Confirmed at application boundary: preparation errors go to Reader's Error
  state before publishing Ready. Reopen clears the state and retries.
- Concern: total memory placement is disconnected from the C61 memory policy.
- Concern: visible cache storage is allocated again on every preparation.

## Concrete architectural concerns

### RECALL-1: global-heap cache limits exceed the C61 internal heap plan

`src/fonts/cjk.rs:15-19,25-33,59-74,102-125` reserves through ordinary Vec/Box
allocation: up to 96 KiB of metrics, two 64 KiB caches and 32 KiB of preparation
scratch. `CJK_STORAGE_BUDGET` is an advertised 256 KiB ceiling, not an enforced
aggregate allocation check.

The C61 global heap is explicitly INTERNAL only
(`kernel/src/board_c61/memory.rs`). Its whole plan is 98,304 + 64,000 bytes
(`board-logic/src/memory.rs:127-129`). PSRAM remains available only through the
budgeted allocator. Ordinary font Vec/Box allocations neither reach PSRAM nor
charge `MemoryBudget`, so the provider competes with runtime/application heap
use and cannot realize its advertised upper allowance even in PSRAM-ready mode.
Fallible allocations preserve recovery, so this observation alone is not a
verified runtime/spec blocker. It is a real placement/design concern, not
unfinished call-site wiring.

Direction: select deliberately smaller internal limits with room for existing
heap users, or provide a budgeted font-storage class/handle with suitable typed
metadata and byte storage. Do not register PSRAM on the global allocator.
The current byte-only BigBuf is a useful placement precedent, not a direct
replacement for typed PageGlyphSlot storage. If introducing a new class, rebalance
the already asserted PSRAM pool totals instead of adding another independent
allowance. Count provider bookkeeping, full reserved capacities and preparation
scratch in the integration's aggregate ceiling.

### RECALL-2: preparation discards reusable visible-page storage

`src/fonts/cjk.rs:102-125` drops both owned caches and allocates new character,
slot and bitmap buffers for every page. The ownership is safe, but this loses
T6's reusable storage benefit and increases allocator fragmentation pressure
on the small internal heap. The metrics Vec already demonstrates capacity reuse.

Direction: keep bounded cache backing storage across page preparations, replace
only when a larger approved capacity is needed, and reuse bounded character
scratch. Invalidate prepared visibility before reuse; keep Reader's error gate
until every required size succeeds. Release retained storage on app exit.

## Path to T8 (not additional T7 scope)

`prepare_text` replaces the whole metric/cache state. Sequential calls for body,
title and UI labels would evict earlier surfaces. T8 should gather the visible
surface requests for all required sizes before preparing, or add a bounded
accumulation phase. The current two-role shape is adequate for T7; chrome/UI
need their own actual pixel sizes later.

`PreparedFonts` is currently an alias of `LayoutFonts`, and view creation does
not encode successful bitmap preparation. Reader's Ready/Error state gate
provides the current publication guarantee. For the pre-render/App seam, retain
that guarantee explicitly: create the draw view only after successful preparation
for the whole visible state, and render an existing loading/error state on
failure. A view over a failed or differently prepared state reaches assertions.

The integration map's App/AppLayer preparation hook remains the appropriate
T8 extension point. It must run after visible event changes and before each
render entry, including boot and nested/deferred redraws, and cover manager-owned
overlays. Adding UI consumers directly to Reader's background would miss those
boundaries. No scheduler changes are requested for T7.

## Long-term implications

The staging/draw split allows more surfaces without storage reads during strips.
Keeping storage placement and aggregate limits explicit prevents additional UI
surfaces from silently consuming internal RAM or multiplying independent cache
allowances. Reusable backing storage also makes allocation behavior more stable
across many page turns. Final implementation review and actual target checks
remain separate work after the active worker completes.

## Follow-up: proposed allocation correction

The worker proposed `PageCache<Box<[PageGlyphSlot]>, BigBuf>` with an explicit
`FontGlyphs` allocation class. Bitmap storage uses the C61 budgeted PSRAM path;
typed metadata stays internal. The class receives 256 KiB from the existing
448 KiB PSRAM reserve, leaving 192 KiB reserve; degraded bitmap allowance is
16 KiB shared across the class. The pool total remains conserved.

The proposed per-app maximum is 200 KiB: 32 KiB staged unique metrics,
two 16 KiB metadata buffers, 4 KiB scratch, and two 64 KiB bitmap buffers sum
to 196 KiB before bookkeeping. This is a sound correction to RECALL-1's
placement issue. Allocation refusal remains recoverable; no hardware/live-heap
sufficiency claim follows from these configured ceilings.

Final review should confirm actual reserved capacities and owner bookkeeping
are included, BigBuf exposes its entire stable reserved slice to PageCache,
clear-before-replace avoids transient doubling, and a future independent
chrome state shares the app's combined ceiling. The worker's chosen
clear-before-replacement policy retains per-page allocation churn; RECALL-2
remains non-blocking and does not justify widening T7's scope.
