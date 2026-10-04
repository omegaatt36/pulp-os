#!/usr/bin/env bash
# Host tests of the HAL-free board logic (pulp-board-logic).
#
# esp-hal cannot build on the host, so only HAL-free crates are tested this way.
# The repo's default target is a bare-metal RISC-V one, so the host triple is passed
# explicitly (rustc's own, so this works on Linux and macOS alike). The root
# `build-std = [alloc, core]` is merged (not replaced) by --config, and a core-only
# build-std collides with the test harness's sysroot core (E0152), so std+test are
# added.
#
# Usage: scripts/test-board-logic.sh [extra cargo test args...]
set -euo pipefail
cd "$(dirname "$0")/.."
host="$(rustc -vV | sed -n 's/^host: //p')"
exec cargo test -p pulp-board-logic --target "$host" \
  --config 'unstable.build-std=["std","test"]' "$@"
