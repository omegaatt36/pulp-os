# T6b test-author brief — settings & bookmarks regression on the host

## Role (red tier)

You are the **test author**. You write tests only. You MUST NOT write or edit
any implementation file (`host/src/**`, `src/**`, `kernel/**`, `scripts/**`,
other `host/tests/*`). You only create two new files:

- `host/tests/reader_settings.rs`
- `host/tests/reader_bookmarks.rs`

You MAY read:

- the **oracle scenarios**: `scripts/reader-regression/os/tests/settings.rs` and
  `scripts/reader-regression/os/tests/bookmarks.rs`, plus the old harness
  (`scripts/reader-regression/os/src/{rig,lib,fixtures}.rs`) — all of it
- **signatures only** of host production surface: crate `pulp-host`
  (`host/src/**`), public `pulp_host` API, `Rig` methods, probe modules, and the
  declarations in `src/apps/settings.rs`
- the fixture generators' public API (`pulp_host::fixtures`)

You MUST NOT read production/logic bodies of `host/src/reader.rs`, the pulled-in
`src/apps/**` reader bodies, or `kernel/src/**` inside-out to derive expected
values. Expected values must come from **the requirements and the old oracle
tests**, never from running or reading the implementation. If a numeric
expectation is not in the oracle, prefer invariants (roundtrip, order, equality
after reboot) over pinning magic numbers.

## Task

Port the settings and bookmarks regression to the host Rig that drives the real
production `ReaderApp` (see T6a files for style: `host/tests/reader_epub.rs`).
The Rig does **not yet** have settings/bookmark APIs — part of your job is to
define the contract the implementer will build (see Contract below).

### Requirements (untagged — treat all lines as unverified; push back if code or oracle contradicts)

- **R2 (production-path)**: the host runs the firmware's real algorithms and
  real app code (`SettingsApp`, reader, `BookmarkCache` via kernel), not a
  reimplementation. Your tests exercise `pulp_host`'s production-driven Rig.
- **R4 (virtual storage)**: all persistence flows through `VirtualStorage`
  (`Rig::new(storage)` / `pulp_host::fixtures::standard_card()`): settings file,
  bookmark storage — the same bytes a real SD card would hold.
- **R8 (settings/bookmarks portion)**: with the generated fixtures (TXT and EPUB
  format copies), passing the settings regression, and the bookmark regression:
  add/jump/delete/reorder bookmarks, bookmark persistence across reboot,
  settings read/write roundtrip, and any menu/navigation scenario the old
  oracle covers that is still hardware-independent on the host.

### Out of scope (exclude; record each exclusion in your report under "gaps")

- SD session restore (old tests gated `cfg(feature = "tree")` depending on C61
  session hardware logic) — record as a coverage gap
- storage failure injection — later batch (T6c) owns error paths
- Wi-Fi hardware/network behavior; only the WifiConfig bytes inside the settings
  file are fair game if the old oracle exercises them without hardware
- cover rendering, image chapters, EPUB navigation internals — already covered
  by `host/tests/reader_epub.rs`

## Contract you must define (convention from the previous batch)

Your tests may only use the **public `pulp_host` API**. Whatever new
observability or input you need, name it as a contract item in your report,
e.g.:

- `Rig` additions (forwarding methods taking the real app/kernel call, exactly
  like `Rig::chapter()` etc. in `host/src/reader.rs`)
- settings entry: production navigation into `SettingsApp` (the firmware path is
  `AppManager`; the host currently drives `ReaderApp` directly). Decide with
  your scenario needs: either a `Rig::enter_settings()`-style method that runs
  the real production navigation/entry code, or an explicitly-scoped direct
  drive of the real `SettingsApp`. Say which and why (fewest new seams).
- bookmark observability: the real `BookmarkCache` already lives in the
  host kernel build (`kernel/src/kernel/bookmarks.rs`, loaded by `Rig::new`
  via the production boot path). Expose read-only views as needed.
- "reboot": construct a **new `Rig` on the SAME `VirtualStorage`** (same object
  re-mounted) — mirroring firmware reboot of one SD card. Bookmarks/settings
  must survive it and re-entering the same book must restore page/scroll
  behavior only as far as the old oracle asserts.

Name things by what they do (`Rig::bookmark_titles()`), not by test intents.
Write the exact signatures in the report so the implementer cannot guess.

## Environment facts

- Crate/package: `pulp-host`. Run your two suites:
  `cargo test -p pulp-host --test reader_settings --test reader_bookmarks`
- Full baseline before you start: `scripts/host-test.sh` → **195 entries pass**
  (lib 1, utf8 18, paging 24, storage 36, fixtures 46, render 39, reader_epub
  31). Do not "fix" anything there.
- `pulp_host::fixtures::{TxtSpec, build_txt, EpubSpec, build_epub, standard(),
  standard_card(), SpecError…}` — `standard()` = fixed 11 files (5 TXT + 6 EPUB:
  EPUB2/3 × STORED/DEFLATE/Mixed). Fixture titles avoid XML/HTML entities by
  policy.
- `VirtualStorage` is **case-sensitive** (the old harness `FakeFs` was
  case-insensitive FAT emulation). Watch fixture filename case.
- The Rig already has: `new(storage)`, `storage()`, `configure(font, theme)`,
  `open(name)`, `press(Action)`, `phase()`, `error_kind()`, `page()`,
  `total_pages()`, `chapter()`, `spine_len()`, `lines()`, `line_infos()`,
  `draw(strip)`, `quick_action_ids()`, `quick_trigger(id)`, `exit()`, `idle(t)`
  and more (read the signature list in `host/src/reader.rs`).
- Formatting: run `rustfmt --edition 2024 <single file>` per NEW file only.
  NEVER `cargo fmt` (it touches the smol-epub sibling repo).

## Red-proof (mandatory)

The new contract APIs do not exist. Prove red **before implementation**:

1. Write both test files.
2. Run the suites. Capture the failure: missing-API items will fail as compile
   errors — record which missing item each test group needs (excerpt of the
   compiler output is enough).
3. Separately, any test NOT needing a new API must either already pass or fail
   for a behavior reason — record which and why (a failure here is a finding:
   it means current production behavior contradicts the oracle; **stop and
   report it**, do not reframe the test).
4. For each requirement above, state in your report: the failing output
   captured before implementation, and whether each expected value was derived
   from requirement/oracle text or from running the implementation. The latter
   is a defect — rewrite against the requirement, or report the requirement as
   underspecified.
5. Predicted-green-after-implementation is fine for ported scenarios (they were
   green on the old harness); label them as such so they are not mistaken for
   weakened red-proof.

## Hygiene and file rules

- Never write spec IDs (`R2`, `T6b`, `Spec R…`) in file contents, test names,
  comments, or error messages. Name tests for behavior
  (`reopened_rig_sees_added_bookmark`).
- Only create the two new test files. Do not edit any existing file — if
  something existing blocks you (fixture API mismatch), report it instead.
- Use the checks already done elsewhere (e.g., do not rewrite assertions for
  easy green). Behavior names, minimal imports, no `unsafe`.

## Report

Write `specs/changes/onepage-host-validation/tmp-briefs/t6b-author-report.md`
(project-relative) with:

1. contract definitions (exact signatures) grouped by file they belong in
2. test list with per-test: what behavior it pins, which requirement it serves,
   predicted green/red after implementation, oracle source line references
3. red-proof evidence (compiler excerpts)
4. gaps (out-of-scope notes for the coverage report)
5. findings (oracle-vs-requirements mismatches, underspecified points)

Reply to me with only: file paths, test counts, contract API list (one line per
item), and status (DONE / DONE_WITH_CONCERNS / BLOCKED).
