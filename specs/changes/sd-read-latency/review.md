## Verdict: BLOCK

**1 MUST · 0 RECALL · 0 OPTIONAL**

This verdict applies to acceptance of the full change. The new T2a profiling
and T4b capacity changes passed independent spec and quality reviews without
new findings. The security sweep found no security MUSTs, and full software
acceptance passed.

### Blocking finding

#### MUST-1 — The literal session-wide repeat-read requirement remains unmet

[Requirement R3](spec.md) requires no pack reads for previously prepared
scalars in the same session. [CJK cache reuse:811](../../../src/fonts/cjk.rs#L811)
validates headers even for an unchanged page; changing pages discards bitmaps;
the bounded metric cache evicts entries and lifecycle transitions clear it.

**Fix:** Resolve whether R3 should describe bounded retained-cache reuse,
including whether header validation is allowed, then implement and test the
chosen contract. The user question is pending; the requirement remains intact.

**Verified:** Read `fill_roles`, `note` and `clear`; existing
[repeat/page-return tests:208](../../../host/tests/cjk_sd_reads.rs#L208) explicitly
permit header reads and require bitmap reads after A → B → A. The retained
upper-index cache does not retain those bitmap bytes.

### Remaining hardware acceptance

T3/T8 remain pending under the original requirements. After flashing, the
timestamped T8 run records ZH2 opening at all five book sizes in 6.080–6.602 s
to completed page refresh, with user confirmation of normal body glyphs.
The initial two Medium runs have visible loading per user observation.
CJK EPUB full opens complete refresh in 12.020/12.189 s; Contents and chapter
selection also complete without allocation failure or panic. These measured
cases pass their scoped checks, but do not establish the complete 700-scalar
boundary, exact 100 ms polling, missing-glyph checks, five-size EPUB heading/
Contents coverage or nonzero-anchor/restart acceptance. Detailed results:
[T8 board record](../../hardware-records/sd-read-latency-t8-2026-10-09.md).

The proposed narrower closeout in `closeout.md` is awaiting a scope decision.
The subsequent commit request does not relax requirements or authorize
archiving; the original R3 guarantee and unchecked tasks remain intact.

### Verification

`task acceptance` passes. Additional measurement-feature host tests and both
C61 checks pass. The 700-visible-glyph regressions fail the prior slot ceiling
and pass the new one; oversized bitmaps remain recoverable and unpublished.
The measurement + partial-refresh artifact has been flashed and boots; see
[build and board handoff](../../hardware-records/sd-read-latency-takeover-2026-10-09.md).
