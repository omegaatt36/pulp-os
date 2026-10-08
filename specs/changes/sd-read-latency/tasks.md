# sd-read-latency — Tasks

- [x] T1: Opened English-only files on the product board with a timestamped log (`~/onepage-backup-product/timed-eng.log`) and user observation: `ENG.TXT` opens instantly, `ENG.EPUB` first open costs 4 s to OPF and 12 s to build the cache (one time), re-open is almost instant. English path is fine; the bottleneck is CJK glyph preparation, including the Files list and returning to Files (CJK title) — satisfies R6. Page-turn timing untested.
- [ ] T2: Add a feature-gated measurement of font-pack reads (count, bytes, elapsed) and distinct scalars per preparation, logged once per preparation — satisfies R1
- [ ] T3: Run T2 on the board with `ZH1.TXT`, `ZH2.TXT`, one CJK EPUB chapter, and entering/returning to the Files list with the CJK-titled books; save the logs next to the hardware record and write the per-read and per-glyph numbers into `spec.md` as the R3 target — satisfies R1, R3
- [ ] T4: Remove the dominant cost the measurement shows (candidates: keep the pack file open across reads, cache the pack index, batch adjacent reads) without changing outputs — satisfies R3, R4
- [ ] T5: Add the loading indication and heartbeat/key polling during long preparation — satisfies R2
- [ ] T6: Make EPD refresh and font preparation not interleave on SPI2, and handle card removal during preparation — satisfies R5
- [ ] T7: Host tests that the prepared glyph data and page breaks are identical to the old path; absent/invalid pack cases still behave as before — satisfies R4
- [ ] T8: Re-run K2, K3, K7, K9 and the CJK EPUB open on the board; fill the hardware record with before/after numbers — satisfies R2, R3, R4
