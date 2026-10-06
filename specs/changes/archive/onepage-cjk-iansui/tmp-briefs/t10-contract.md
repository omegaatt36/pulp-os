# Final acceptance and install artifacts

Task: finish Chinese host/strip snapshots, coverage/missing glyph, failure and
cache/SD read matrix; reverify firmware link; deliver SD install instructions.
Requirements in spec.md currently have no provenance tags. They require
reproducible Iansui host packs with provenance/OFL, Unicode/offset-safe readers,
bounded page preparation, source-free strip draw, recoverable failures,
kinsoku/text-position/scalar preservation, broad visible-surface fallback and
incompatible-layout invalidation. Exact task/requirement traceability belongs in
evidence.md, not source comments/test names.

Use existing independent tests and their actual RED→GREEN reports for covered
matrix cases. Do not invent a fresh exhaustive suite, repeat unchanged decoder
campaigns, or run mutants. Add a focused acceptance check only for a concrete
remaining gap. Existing golden fixture tests are independent oracles; visual
snapshots are exported observations and must not redefine expected correctness.

Generate all nine current supported pixel packs from actual local
Iansui-Regular.ttf and OFL.txt using the existing converter. Record source hash,
converter convention, pack bytes, coverage and known absent scalar 𪚥. Distinguish
coverage of the chosen fixture from all Unicode support. Keep large generated
packs in target acceptance artifacts, not embedded firmware or test fixtures.
Do not write any removable SD device; deliver copy instructions for _PULP/FONTS.

Export actual real ReaderApp full and stitched Chinese snapshots (PBM and a
viewable PNG), including heading/body/TOC/title where supported, from generated
packs. Record preparation read count/bytes separately from draw counts; any
full/stitched draw must have zero font reads. State whether counts are logical
adapter reads or physical SD transactions; no hardware latency claim without
hardware evidence. Inspect visibly for truncation/blank pages/size mismatch.

Failure matrix must reference exercised evidence for missing glyph, optional
absent pack, corrupt/version mismatch, bitmap/metadata capacity, SD bitmap and
metadata failures, retry. Include combined UI/body cache bounds and current
hardware limitation; do not claim PSRAM live heap sufficiency from host alone.

After T8/T9 acceptance, run one final full gate:
IANSUI_REQUIRED=1 CARGO_NET_OFFLINE=true scripts/check-host-boundary.sh
(it includes host suite and both firmware link builds), board logic suite,
fontpack no_std check and offline reader regression both. Avoid separately
repeating the same full host suite when boundary already ran it.
Resolve concrete failures without touching independent tests unless controller
amends the requirement and an independent author updates the oracle.

Deliver install/validation documentation, final evidence.md and
tmp-briefs/t10-impl-report.md with commands/output/artifact paths/hash counts,
failure matrix, risks, weakening gate. Tests/historical snapshots read-only.
No commits, no spec IDs in code. Not alone; preserve all prior agents' edits.
