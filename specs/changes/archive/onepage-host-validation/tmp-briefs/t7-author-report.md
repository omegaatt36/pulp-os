# Preview CLI test author report

Status: GREEN after implementation became available. Initial gate was authored and captured RED before preview entrypoint existed. No implementation edits or commits.

Command: `scripts/check-host-preview.sh`

Actual red output (exit 1):
```
FAIL preview entrypoint missing or not executable: scripts/host-preview.sh
```

Gate: `scripts/check-host-preview.sh`, 26 success assertions once implemented. Early entrypoint guard fails meaningfully before invoking a nonexistent command. Temporary artifacts/logs use mktemp and an EXIT cleanup trap. Python3 validates all PBM bytes, including exact P4 dimensions 480x800, complete 48000-byte raster and a nonblank frame; raster bytes are never stripped as whitespace.

Contract: `scripts/host-preview.sh --output-dir DIR [--fixture NAME] [--page N] [--font N] [--theme N]`; fixture names are canonical standard-card names. Defaults page=0, font=2, theme=1. Absent fixture yields eleven PBMs. Filenames may include fixture/options, but must be deterministic between directories. Selected fixture yields one PBM. Help must describe all five flags. Separate TXT comparisons require each page/font/theme option to change raster pixels independently from defaults; filenames and header comments do not satisfy this assertion. Valid nondefault selection page=1/font=0/theme=0 is exercised for TXT, EPUB2 and EPUB3. Missing arguments, unknown flag/name, negative and nonnumeric options, missing option values, out-of-range page and exported font/theme bounds must fail with diagnostics.

Expectation origins: eleven books and canonical names from existing standard fixture generator; dimensions/format, deterministic bytes, CLI options and page rejection from author brief; defaults confirmed by root and production DEFAULT_FONT_SIZE_IDX/DEFAULT_READING_THEME. Font rejection threshold derives FONT_SIZE_COUNT (whose count minus one is max_size_idx); theme threshold derives NUM_READING_THEMES. No copied implementation snapshots or hashes.

Coverage: R9 exercises exact rerun identity across two output directories, selection, defaults and validation errors. R2 black-box checks exercise produced PBMs from TXT/EPUB2/EPUB3, but cannot prove Rig::draw or Action::Next use: implementation review plus existing reader/render tests must provide that evidence. R1/R3 remain existing check-host-boundary.sh gates, intentionally not duplicated. R10 belongs the implementation coverage document, including all known gaps.

Weakening gate: no assertions removed or altered to accommodate implementation; zero implementation existed at initial red capture; three independent option-effect assertions were added at root request after implementation became available and before final baseline. Limits: no pixel oracle or cross-platform identity claim, no hardware validation, no independent chapter-navigation oracle. These retain review obligations rather than pretend binary-output comparisons prove internal algorithm reuse. Positive option maxima beyond selected 0/defaults should be covered by implementation/unit review.

Final actual verification: `bash -n scripts/check-host-preview.sh` exit 0; `scripts/check-host-preview.sh` exit 0, `PASS preview CLI (26 gates)`. Independent page/font/theme effects all passed. Initial missing-entrypoint red is historical evidence, not a claim about current state.
