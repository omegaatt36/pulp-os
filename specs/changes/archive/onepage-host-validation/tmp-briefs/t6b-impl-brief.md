# T6b implementer brief — build the settings/bookmarks contract so the ported tests pass

## Role (red tier)

You are the **implementer**. The test files are authority; treat them and all
existing test files as **read-only**. Your acceptance is:

```
scripts/host-test.sh --test reader_settings --test reader_bookmarks   # 32 passes
```

then the full gate:

```
scripts/host-test.sh                       # everything green (195 baseline + 32 new)
scripts/check-host-boundary.sh             # full version incl. X4 + C61 --locked builds
scripts/check-reader-regression.sh both    # head 83 / tree 90, golden sha = pin
```

If any test asserts something the production code makes impossible, **STOP and
report** — a test that needs changing means ambiguity; the main session decides.
Do not edit tests, do not delete tests. The test tree must end up byte-identical
to what the author left.

## Task

Implement exactly the contract defined in
`specs/changes/onepage-host-validation/tmp-briefs/t6b-author-report.md`
section 1 (read it first). Summary:

1. `host/src/apps.rs`: add the real production settings app via
   `#[path = "../../src/apps/settings.rs"]`, update the module comment that
   currently calls the other apps firmware-only (settings is pure UI + config).
   Resolve drift between the host ui/fonts stand-ins and `src/ui/mod.rs` if the
   first compile surfaces it (the author found none by inspection).
2. `host/src/kernel.rs`: `Kernel::bookmarks_flush()` — delegate
   `bm_cache.flush(&self.sd)`; this mirrors the production scheduler
   housekeeping at `kernel/src/kernel/scheduler.rs:282-283` — read that site
   and match its semantics (only flush when dirty/needed, whatever production
   actually does there), do not invent new semantics.
3. `host/src/reader.rs`: `Rig` additions (`save_position`, `bookmark_save`,
   `bookmark_remove`, `bookmark_find`, `bookmark_list_into`, `bookmarks_flush`,
   `into_storage`, `theme_idx`) and the new `SettingsRig` — every method
   forwards to the real production objects (real `SettingsApp`, real
   `BookmarkCache` on the kernel, real `KernelHandle`), like the existing
   `Rig::chapter()` / `Rig::quick_trigger()` forwarding style. No logic of your
   own beyond forwarding; no re-implementation of anything production does.
   - `SettingsRig::new(storage)`: real `SettingsApp::new()` over a fresh
     kernel/AppContext, no bookmark load.
   - `SettingsRig::boot()`: the load half of production
     `AppManager::load_eager_settings` (real `SettingsApp::load_eager(&mut
     KernelHandle)` — read the production call site and mirror it).
   - `SettingsRig::enter()` / `press(a)` / `tick()`: real `on_enter(ctx,
     handle)` / `on_event(Press(a), ctx)` / one background scheduler tick via
     the host executor (`block_on(background)`), matching how the reader rig
     runs its tick.
   - `SettingsRig::file() -> Option<Vec<u8>>`: `_PULP/SETTINGS.TXT` bytes.
   - `SettingsRig::into_storage(self)`, `Rig::into_storage(self)`: the
     power-off/reboot seam — same `VirtualStorage` object back out so a new
     rig can re-mount it.
4. Signatures are in the author report §1 (exact); the tests tell you the
   observable semantics. Production source (src/**, kernel/**) must NOT be
   modified by this batch — if the contract cannot be satisfied without
   touching it, STOP and report.

## Environment facts

- Crate `pulp-host`; run tests through `scripts/host-test.sh` (the repo's
  default is bare-metal RISC-V; plain cargo picks the wrong sysroot).
- `VirtualStorage` is case-sensitive (old harness FakeFs was not). Existing Us:
  `host/tests/reader_epub.rs` (style reference), probe forwarding
  (`host/src/apps/probe.rs`), image-worker registration pattern
  (`host/src/reader.rs` IMAGE_WORKER).
- Formatting: `rustfmt --edition 2024 <single-file>` on files you touched only.
  NEVER `cargo fmt` (it would rewrite the sibling repo smol-epub).
- No git commits. Production firmware sources must not change in this batch.

## Mutation testing (mandatory, then report)

After greens, attack your own implementation with ~10-15 focused mutations
(one at a time, revert after each, `diff` to confirm each mutation actually
applied — sed mistakes happen). Aim at the seams the tests claim to pin:

- bookmark generation bump / eviction picks newest instead of lowest / record
  layout field order / case-folding in find / flush skips clean check /
  reboot loses data (into_storage or the next boot loading the wrong file) /
  list order reversed / short-buffer semantics
- settings: 512-byte load cap / sanitize clamp table / edit step direction /
  save-not-on-tick (immediate write) / wifi credentials dropped on save /
  propagate to reader absent / theme index not reaching the app

Report per mutation: what, which tests went red, killed/survived/equivalent.
Survivals are findings, not failures — report them for the ledger.

## Report & weakening gate

Write your report to
`specs/changes/onepage-host-validation/tmp-briefs/t6b-impl-report.md`:
files touched, per-gate results with numbers, mutation table, survived
mutants, anything you want the main session to re-verify.

The main session checks weakening by diffing `host/` and `scripts/` against the
pre-batch snapshot `/tmp/hv-snap/T6b` — expect exactly: `host/src/**` changes,
the two new test files, and nothing else. Reaffirm in your report that the two
test files match the author's state byte-for-byte (you never edited them).

Reply with: status DONE / DONE_WITH_CONCERNS / BLOCKED, gates run and results,
mutation summary (killed/survived), concerns.
