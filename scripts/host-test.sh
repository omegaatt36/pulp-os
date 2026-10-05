#!/usr/bin/env bash
# Host validation tests (pulp-host): paging / rendering / storage logic that
# runs the firmware's own source files on the host, with no esp-* crate and no
# embedded linker script.
#
# Same shape as test-board-logic.sh: the repo's default target is bare-metal
# RISC-V, so rustc's host triple is passed explicitly, and build-std is
# overridden to std+test (a core-only build-std collides with the test
# harness's sysroot core, E0152). Honors $CARGO_TARGET_DIR.
#
# Usage: scripts/host-test.sh [extra cargo test args...]
set -euo pipefail
cd "$(dirname "$0")/.."
host="$(rustc -vV | sed -n 's/^host: //p')"
exec cargo test -p pulp-host --target "$host" \
  --config 'unstable.build-std=["std","test"]' "$@"
