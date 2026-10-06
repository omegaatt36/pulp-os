# Layout identity and raw-anchor preservation

Task: update incompatible pagination-cache invalidation and test forward/back,
bookmark and resume. Requirements in current spec have no provenance tags.
Binding text: WHEN font identity, pixel size or layout version changes, rebuild
incompatible pagination caches. WHEN returning to previous page or restoring a
bookmark, locate the same text position. UTF-8 raw offsets must stay on scalar
boundaries. Chinese line breaks conserve text and follow specified punctuation.

The cached state is reader page offsets/continuation flags/prefetch tags.
Font-independent EPUB chapter byte caches are not invalidated by font changes.
Layout identity includes active body/heading bank font_id+pixel sizes and a
version constant for the layout algorithm; it must not depend only on size index.
No persistent page-cache file currently exists. A compiled version constant is
sufficient to distinguish algorithm revisions; do not introduce storage solely
to test a version change. Existing Latin behavior must remain unchanged.

On live size change or resume with a replaced font pack, capture the current raw
anchor first. Rebuild from the beginning and select the page containing that
anchor. That page start may precede the original anchor under new wrapping;
assert anchor containment, scalar boundaries and no text loss, not page-number
equality. A pack replaced at the same pixel size must not leave old offsets.
Forward/back navigation must use rebuilt offsets and prepared new glyphs.

Independent tests should use hand-built PFNT metrics/bitmap bytes from frozen
cjk_support, change font_id and advances at the same px, and compute expectations
from literal fixture metrics. Reopen/bookmark controls already exist. Approved
thin host forwarding controls, only if needed: Rig::resume() invokes actual App
on_resume then normal background progression, Rig::quick_cycle(id,value) invokes
actual on_quick_cycle_update then normal progression. No alternative pagination.

Read-only source map: t9-cache-map.md. Test author must not read implementation
bodies or write reference algorithms. Write focused new host/tests/cjk_identity.rs,
necessary thin Rig forwarding only, report t9-tests-report.md with actual RED,
requirement-derived oracle, frozen hash/snapshot and weakening gate. No mutants,
no commits, no spec IDs in Rust. Not alone; preserve all other edits.
