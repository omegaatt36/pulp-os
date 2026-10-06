# T7 memory integration contract

Controller-approved public policy addition:

- `MemClass::FontGlyphs` and `ExternalClass::FontGlyphs` support explicit
  budgeted font-bitmap storage; the former participates in `MemClass::ALL`.
- Ready PSRAM placement has a shared 256 KiB class allowance, exposed by
  `PSRAM_FONT_GLYPHS_BYTES`.
- Degraded placement is internal, with a shared 16 KiB allowance exposed by
  `INTERNAL_FONT_GLYPHS_BYTES`. An over-budget request is recoverable, and a
  released reservation restores capacity.
- The 256 KiB PSRAM allowance is funded by reducing the existing reserve from
  448 KiB to 192 KiB. Existing chapter/image/page-table/ZIP limits remain
  768/512/64/256 KiB. All class allowances plus reserve fit the physical 2 MiB.

`board-logic/tests/font_memory.rs` contains three independent public-API tests:
ready placement/shared class cap/release; degraded cap/refusal/recovery; and
literal funding values plus simultaneous full-class reservations. Expected
values come from this approved contract, not new production output. No allocator,
HAL, provider implementation, mocks, mutants or reference algorithm are added.

Replay: `scripts/test-board-logic.sh --test font_memory`.

Missing FontGlyphs variants/constants are an accepted initial compile RED for
this narrowly approved API addition. The author owns only this new test and
the contract/report; production class wiring belongs to the implementer.

## Approved existing-test premise correction

Funding FontGlyphs from reserve increases a 1 MiB chip's usable pool from
576 KiB to 832 KiB. The existing test
`r14_pool_exhaustion_is_reported_when_the_class_still_has_room` previously
filled all but 16 KiB using ChapterText alone. Its new 816 KiB setup request
exceeds ChapterText's unchanged 768 KiB cap and cannot reach the intended
pool-exhaustion assertion.

The author is now authorized to change only that setup to 768 KiB ChapterText
plus 48 KiB ZipToc and correct its obsolete 576 KiB comment. ImageData retains
its whole allowance for the tested request. The expected `PoolExhausted`
error, exact used/requested values, successful final 16 KiB reservation and
rejected granule-rounded one-byte request remain unchanged. This is a policy
premise correction, not weaker expectations or an implementation change.
