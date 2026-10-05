# Test author report

Ownership: only new host/tests/reader_failures.rs, host/tests/reader_screens.rs and this report. No implementation edits, cargo fmt, commits, or requirement/provenance IDs in test code.

## Required adapter contract

```
impl Rig {
    pub fn set_sd_ok(&mut self, mounted: bool);
    pub fn bookmarks_dirty(&self) -> bool;
}
impl SettingsRig {
    pub fn storage(&self) -> &VirtualStorage;
    pub fn is_dirty(&self) -> bool;
}
```

`set_sd_ok(false)` changes Kernel.sd_ok (via the real KernelHandle::sd_ok observation) AND the real host storage boundary mounted-card state. Storage operations must reach firmware-equivalent missing-card borrowing semantics (kernel/src/drivers/storage.rs:198 returns NoCard when borrowing absent mounted storage). Do not construct a reader Error or inject NoCard artificially. `true` restores the SAME storage/card and permits reopen. `bookmarks_dirty` reads production BookmarkCache::is_dirty without mutation. SettingsRig::is_dirty reads SettingsApp dirty state without mutation. SettingsRig::storage exposes the existing card to one-shot injections; it must not copy card state. These are forwarding/probe contracts, no new application logic.

## Evidence and origins

- Command: `scripts/host-test.sh --test reader_failures --test reader_screens`. Actual cargo compile failed with nine E0599 missing-method errors: set_sd_ok (2), SettingsRig::storage (2), SettingsRig::is_dirty (2), Rig::bookmarks_dirty (3). Full capture `/tmp/t6c-red.log`. This is API red, NOT production behavioral failure evidence.
- Command: `scripts/host-test.sh --test reader_screens`. Final actual result: 1 passed, 0 failed, seven generated books traversed (PLAINLF plus six EPUB2/3 compression variants), including decoded image pages for every EPUB. `/tmp/t6c-screens.log`. Screen tests are green regression invariants; no invented red evidence.
- A first screen attempt failed before reader execution because an author precondition selected TXT with more than 100 lines, absent from standard fixtures. Corrected to the existing declared PLAINLF.TXT fixture after reading standard fixture declarations. No product expectation weakened.

R2: reader tests exercise Rig production ReaderApp open/press/draw; no test-side pagination/rendering. Screen invariants green; failure behavior awaits adapter API.
R4: read_log and one-shot consumed-injection assertions specify storage observability. API compile red blocks runtime failure checks; memory backend covered here, host-file backend belongs to existing storage suite.
R5: open and paging read failures expect Error/ReadFailed then intact reopen; settings failed write preserves previous bytes/dirty and next tick persists; bookmark failed flush preserves bytes/dirty and next flush persists, reboot verifies offset 456; short injection records exact returned=17 and requested>17, accepts settled Ready or Error with an observable error kind. These expectations are explicit task brief acceptance, not derived from running production. Missing-card expectation additionally uses firmware storage boundary contract. No runtime behavioral red yet.
R6: screen PBM magic/header P4 480x800, payload exactly 480*800/8, ink and white both present. Requirement-origin invariants; green.
R7: full PBM equals stitched PBM including observed decoded image page in each standard EPUB. Requirement-origin differential expectation; green.
R8: screen next differs, prev returns exact first screen, repeat draw and reopen match first screen; EPUB2/3 decoded image is found with bounded 3000-step walk. Green. Settings step expectation originates existing reader_settings oracle (default 10, increments 5); two edits ->20. Bookmark slot byte offset origin is explicit test input, never implementation output.

## Gaps and gate

This task does not duplicate existing TOC/settings/bookmark success oracle suites; those remain required companion evidence. Screen coverage is one TXT, all six EPUB variants, default font/theme, first/second pages and one supported image page per EPUB, not every page/configuration. PBMs are validated in memory, not written to the repo. Short-read Ready is permitted intentionally by brief; no universal short-read-is-error assumption. Missing-card does not assert a backend injected record since error belongs to borrowing boundary. Paging failure assumes a next-page book read, and consumed injection must prove it actually occurred. If production differs, stop and report: do not relax Error/ReadFailed or retry/dirty/byte invariants without explicit requirement clarification. Contract implementer must run both focused executables after forwarding API exists; cannot claim failure behavior verified from compile red.

Controller correction: adapter mount flag lives in Kernel.sd_ok, not AppContext; verified host/src/kernel.rs and kernel/src/kernel/handle.rs. No test or behavioral expectation changed.
