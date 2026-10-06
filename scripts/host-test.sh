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
# With no package or named target, all suites run. Named targets default to
# pulp-host; use -p/--package to select another host suite.
#
# Real-font tests of pulp-fontconv read $IANSUI_TTF, defaulting to
# Iansui-Regular.ttf in the repo root. Without the file they print
# `SKIPPED(no-iansui-font): <test>` lines; set IANSUI_REQUIRED=1 to make that a
# failure instead.
#
# Usage: scripts/host-test.sh [-p PACKAGE] [extra cargo test args...]
set -euo pipefail
cd "$(dirname "$0")/.."
host="$(rustc -vV | sed -n 's/^host: //p')"
build_std='unstable.build-std=["std","test"]'
if [ -z "${IANSUI_TTF:-}" ] && [ -f "$PWD/Iansui-Regular.ttf" ]; then
  export IANSUI_TTF="$PWD/Iansui-Regular.ttf"
fi
packages=()
args=()
focused=0
while [ $# -gt 0 ]; do
  case "$1" in
    -p|--package)
      [ $# -ge 2 ] || { echo "missing package after $1" >&2; exit 2; }
      packages+=("$2"); shift 2 ;;
    --package=*) packages+=("${1#*=}"); shift ;;
    -p=*) packages+=("${1#*=}"); shift ;;
    -p?*) packages+=("${1#-p}"); shift ;;
    --test|--test=*|--bin|--bin=*|--example|--example=*|--bench|--bench=*|--lib)
      focused=1; args+=("$1"); shift ;;
    --) args+=("$@"); break ;;
    *) args+=("$1"); shift ;;
  esac
done
if [ ${#packages[@]} -eq 0 ]; then
  if [ "$focused" -eq 1 ]; then
    packages=(pulp-host)
  else
    packages=(pulp-host pulp-fontpack pulp-fontconv board-harness)
  fi
fi
for package in "${packages[@]}"; do
  case "$package" in
    pulp-host|pulp-fontpack|pulp-fontconv|board-harness) ;;
    *) echo "not a host test package: $package" >&2; exit 2 ;;
  esac
done
status=0
for package in "${packages[@]}"; do
  features=()
  [ "$package" != pulp-fontpack ] || features=(--features builder)
  cargo test -p "$package" "${features[@]}" --target "$host" \
    --config "$build_std" "${args[@]}" || status=$?
done
exit "$status"
