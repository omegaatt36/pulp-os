#!/usr/bin/env bash
# Compile the real C61 adapters against a stateful host HAL seam.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TOOLCHAIN="$(sed -n 's/^channel *= *"\([^"]*\)"/\1/p' "$ROOT/rust-toolchain.toml")"
WORK="${C61_ADAPTER_WORKDIR:-$(mktemp -d)}"
mkdir -p "$WORK/src"
cat > "$WORK/Cargo.toml" <<TOML
[package]
name = "c61-adapter-regression"
version = "0.0.0"
edition = "2024"
[dependencies]
esp-hal = { path = "$ROOT/scripts/c61-adapter-regression/esp-hal-shim" }
pulp-board-logic = { path = "$ROOT/board-logic" }
critical-section = { version = "1.2", features = ["std"] }
static_cell = "2.1"
nb = "1"
embedded-hal = "1"
log = "0.4"
TOML
cat > "$WORK/src/lib.rs" <<RS
#![allow(dead_code)]
#[path = "$ROOT/kernel/src/board_c61/adc.rs"]
mod adc;
#[path = "$ROOT/kernel/src/board_c61/spi.rs"]
mod spi;
#[cfg(test)]
include!("$ROOT/scripts/c61-adapter-regression/tests.rs");
RS
HOST="$(rustc "+$TOOLCHAIN" -vV | sed -n 's/^host: //p')"
(cd "$WORK" && unset CARGO_BUILD_TARGET RUSTFLAGS && cargo "+$TOOLCHAIN" test --offline --target "$HOST" -- --test-threads=1)
