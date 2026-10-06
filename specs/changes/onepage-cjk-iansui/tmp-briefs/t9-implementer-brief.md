# Rebuild incompatible reader pagination

Read t9-contract.md first: binding requirements, raw-anchor semantics and
approved host controls. Four independent tests in host/tests/cjk_identity.rs
are read-only; frozen snapshot/manifest and actual3RED+1controlPASS are in
t9-tests-report.md and t9-tests-manifest.txt. Independent test review APPROVE.
Requirements currently have no provenance tags. Do not invent human provenance.

Own reader pagination/identity/invalidation and additive CjkState identity
exposure as necessary, no test/control edits. UI surface ownership belongs to
the completed previous task. Preserve its pre-render and source-free draw path.
Do not change font-independent EPUB chapter caches or English fixtures/pins.

Use body+heading font_id/pixel sizes and a compiled layout-version constant in
the layout key; selected size index alone is insufficient. Detect changed packs
on resume and before reuse for newly loaded pages; unavailable optional bank
has a deterministic absence identity. Genuine metadata/header faults propagate
via approved optional lookup rather than being treated as absence. Pure Latin
path must preserve pinned existing wrapping and avoid unnecessary pack work.

Capture active raw byte anchor before reset on changed size/font. Reset offsets,
continuation flags, completed-index flag and stale prefetch tags together; scan
new layout and select the page containing the anchor. Preserve chapter identity.
New page start can precede old anchor. Forward/back and bookmark/session restore
must consume each scalar once and use new prepared glyphs. Keep memory bounded,
no retained KernelHandle or source reads in strip draw.

No mutants, broad unchanged decoder repeats, commits, reference implementation,
spec IDs/provenance tags in source. If a test is wrong, stop and report; controller
must amend spec before independent author changes oracle. Not alone, preserve
all other edits. Scoped checks: cjk_identity, reader/heading navigation tests,
existing paging/bookmark paths and English tree golden when source changes it.
Final broad host/firmware gate belongs to final acceptance, not repeated here.

Report tmp-briefs/t9-impl-report.md: changes/key fields/anchor semantics,
commands+outputs, test hash/weakening gate and concerns. Do not claim compiled
version mutation runtime tested; source inspection is the specified evidence.
