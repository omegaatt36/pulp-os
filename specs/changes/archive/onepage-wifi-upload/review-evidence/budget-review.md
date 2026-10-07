# Budget/build review

No verified MUST findings in this slice. Read-only inspection; no builds or tests rerun.

[RECALL] New Wi-Fi reservation guard lacks behavioral coverage
File: board-logic/tests/wifi_budget.rs:35; board-logic/src/memory.rs:614
The seven new tests cover helper constants and the arithmetic of a historical ELF measurement. None constructs MemoryBudget::for_build(true), tests pool_limit_for(true), or proves reserve rejects aggregate internal use above 117,248 B. All existing in-file budget tests still construct the offline MemoryBudget::new(). A regression that discards the new wifi field in MemoryBudget::pool_limit would pass this suite and the ELF report, despite permitting 162,304 B reservations against a 117,248 B heap.
Fix: Add a behavioral Wi-Fi reservation boundary test (preferably across PSRAM statuses), with an offline positive control for the additional 45,056 B. This is a coverage gap, not a current implementation bug: the current pool_limit correctly forwards self.wifi.

Verified propagation:
- Root wifi forwards pulp-kernel/wifi; kernel memory.rs uses the same cfg!(feature = "wifi") for both public main heap constant and BUDGET::for_build. main_c61.rs registers that constant in heap_allocator. Main/reclaimed plans are 53,248/64,000 B versus offline 98,304/64,000 B.
- ELF report selects variant from non-absolute driver symbols and verifies matching main heap size; static/stack and PSRAM registration rules remain intact. STACK_MIN_BYTES=49,152 and STATIC_RAM_MAX_BYTES=207,472 have not been relaxed.
- check-wifi-build validates fixed C61 versions, both radio optimization invocations, C61 concrete driver symbols, IPv4-only smoltcp, enabled/offline controls, and real ELF memory report. Failed builds set persistent fail=1 even when an older ELF exists.
- Offline graph/ELF probes cover both chips. C61 enabled positive control checks symbols rather than upload strings; X4 provides the upload-string positive control.

Evidence limits / process omissions:
- Historical wifi_budget arithmetic predicts 204,216 B, whereas final recorded ELF statics are 204,664 B. The automated ELF report covers this 448 B drift; do not treat that host arithmetic test as final image evidence.
- T1b/T2/T3/T4/T5 first red proofs largely fail at absent imports. These prove an interface was absent, not that individual assertions fail against faulty behavior. The ledger explicitly discloses this; subsequent mutation tests supply independent coverage for HTTP/mDNS/session. T1b has no behavioral mutation coverage for the new MemoryBudget guard above.
- Weakening gate T6 explicitly records nine removed bring_up tests and a timing threshold relaxed 500 ms -> 1.5 s; no concealed weakening identified from available files. /tmp snapshot comparison and historical command outputs were not independently reproduced in this review.
- budget-report is explicit that radio heap demand, actual stack usage, PSRAM allocation behavior, repeated entry, and reading with smaller heap are UNVERIFIED. Hardware-acceptance W8 says R12 is satisfied by static evidence while spec R12 asks for evidenced internal heap/stack/PSRAM budgets: treat completion as software/static evidence only, not runtime adequacy. This limitation is clearly disclosed rather than hidden.
