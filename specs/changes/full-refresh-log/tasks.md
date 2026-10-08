# full-refresh-log — Tasks

- [ ] T1: Add the completion log to the full-refresh path (duration, count 0), only when the frame was shown — satisfies R1, R2
- [ ] T2: Confirm by reading and by build that chapter change and `nav.resume` still take the partial path and no existing log line changed (build `run-c61`, `run-c61-partial`; run `task acceptance`) — satisfies R3
- [ ] T3: Update P8 and P9 in `specs/references/hardware-acceptance.md` per R4 — satisfies R4
- [ ] T4: On the product board with the partial image: Home → Files/Reader/Settings, open a book, change chapter, return to Home, and wake from sleep; save the log next to the hardware record; set P8 and P9 in `2026-10-08-product.md` from that log (P8 is currently `FAIL` because of the old criterion) — satisfies R1, R2, R3, R4
