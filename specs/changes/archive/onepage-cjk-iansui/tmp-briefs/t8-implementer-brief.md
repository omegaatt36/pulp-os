# Visible-surface prepared fallback integration

Read t8-contract.md first; it contains exact approved APIs, seven frozen tests,
and required production-path inspection. Requirements in spec.md have no
provenance tags; do not invent human confirmation. Task: integrate the shared
prepared regular CJK fallback for body/heading/TOC/book title/UI labels while
preserving static Latin styles. Body/heading core was implemented already.

Binding requirements: visible-page metrics/glyphs are loaded before draw;
any strip draw performs zero SD font reads; font caches stay in their configured
memory budget; capacity/SD errors are recoverable; each surface uses its actual
pixel size. Unsupported scalars in an absent optional pack use the existing
size-specific synthetic missing glyph. Installed corrupted/read-failing packs
must report failure. Only definitive storage absence enables fallback.
UTF-8 label truncation preserves the longest complete scalar prefix.

Own production App/AppLayer hook, manager dispatch, reader title/TOC integration,
widgets, Files/Home/Settings and overlay preparation, both render schedulers,
and thin host Rig hook. Tests/support/geometry seams remain read-only.
No host substitutes for omitted production UI. Do not mutate prior tests.

Use existing CjkState and immutable PreparedFonts. It retains no KernelHandle
between callbacks. Sequential prepare_text calls replace prepared state: do not
evict body glyphs by preparing title afterward. Separate bounded visible-surface
state or aggregate preparation is permitted, with a combined memory analysis.
FontGlyphs BigBuf storage follows board budget: PSRAM256KiB/degraded16KiB.
Generic typed Vec/Box uses C61 internal heap, so no unbounded strings/metadata.
Release unused surface caches on lifecycle transitions; suspended work cannot
evict active prepared render. Pure Latin redraw should avoid font storage work.

Hook signatures: App takes mutable AppContext+KernelHandle; AppLayer takes
KernelHandle only because manager owns context. Every render path must prepare
after last event/background mutation, including boot/deferred/partial draws.

Start from frozen independent RED evidence; implement without changing tests.
If an oracle is wrong, report and stop that correction; controller must amend
requirements before independent author changes it. No mutants, no reference
implementation, no commits, no spec IDs/provenance tags in source.

Report t8-impl-report.md: owned changes, commands/output, runtime tests, production
inspection paths, combined memory limits, test hashes/weakening gate, concerns.
Run cjk_surfaces and affected focused tests; do not repeat unchanged broad gates.
You are not alone; preserve all other agents' edits.
