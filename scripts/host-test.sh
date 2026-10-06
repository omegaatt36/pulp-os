#!/usr/bin/env bash
# Host validation tests: pulp-host (paging / rendering / storage logic that runs
# the firmware's own source files on the host, with no esp-* crate and no
# embedded linker script), pulp-fontpack (SD font pack format: reader and
# writer, tested with its `builder` feature on), pulp-fontconv (host TTF ->
# font pack converter) and board-harness (the real C61 shared-ADC / SD-probe
# adapters and the real X4 EPD-driver / input sources host-compiled against
# an esp-hal seam, locking the golden wire traces).
#
# Same shape as test-board-logic.sh: the repo's default target is bare-metal
# RISC-V, so rustc's host triple is passed explicitly, and build-std is
# overridden to std+test (a core-only build-std collides with the test
# harness's sysroot core, E0152). Honors $CARGO_TARGET_DIR.
#
# All suites always run; the script fails if any fails.
#
# Real-font tests of pulp-fontconv read $IANSUI_TTF, defaulting to
# Iansui-Regular.ttf in the repo root (not in git). Without the file they print
# `SKIPPED(no-iansui-font): <test>` lines; set IANSUI_REQUIRED=1 to make that a
# failure instead.
#
# Usage: scripts/host-test.sh [extra cargo test args...]
set -euo pipefail
cd "$(dirname "$0")/.."
host="$(rustc -vV | sed -n 's/^host: //p')"
build_std='unstable.build-std=["std","test"]'
if [ -z "${IANSUI_TTF:-}" ] && [ -f "$PWD/Iansui-Regular.ttf" ]; then
  export IANSUI_TTF="$PWD/Iansui-Regular.ttf"
fi
status=0
cargo test -p pulp-host --target "$host" \
  --config "$build_std" "$@" || status=$?
cargo test -p pulp-fontpack --features builder --target "$host" \
  --config "$build_std" "$@" || status=$?
cargo test -p pulp-fontconv --target "$host" \
  --config "$build_std" "$@" || status=$?
cargo test -p board-harness --target "$host" \
  --config "$build_std" "$@" || status=$?
exit "$status"
