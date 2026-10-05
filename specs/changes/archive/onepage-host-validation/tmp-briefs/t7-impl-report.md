# T7 implementer report

Implemented production-reader preview binary, executable wrapper, host README
and R1–R10 coverage report. Owned files only; no commits or cargo fmt. Tests
remain read-only. Every PBM goes through Rig::draw, bounded real background
work, real Action::Next for cross-chapter ordinals, and full/stitched equality.
Defaults/bounds use production exports. Arguments, unavailable pages, reader
state/cache timeout and I/O errors produce diagnostics and nonzero status.

Verification:
- `scripts/check-host-preview.sh` exit0, 26 final checks, `/tmp/t7-preview-final.log`
  (including independently authored page/font/theme pixel effects).
- `scripts/host-test.sh` exit0, all236 pass/no ignored, `/tmp/t7-host-tests.log`.
- Root reports final boundary exit0 (graph81, host236, both firmware builds),
  reader regression head83/tree90 and pinned golden equal, memory checks green.
- Final focused gate reran after author added all three option-effect assertions;
  binary/wrapper unchanged. No full-suite rerun needed absent code change.

Red-proof existed before entrypoint: test author recorded exit1,
`FAIL preview entrypoint missing or not executable: scripts/host-preview.sh`.
Expectation origins are brief/production fixture names, exported defaults and
bounds; no implementation pixel hashes. Weakening gate: no tests/assertions
edited, removed or altered; no production changes. Known gaps are explicitly
listed in coverage.md, including partial output on later-book failure and
unproved cross-platform identity. CLI source review is still required to prove
internal algorithm reuse/cross-chapter navigation; black-box bytes alone cannot.
