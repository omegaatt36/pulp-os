#!/usr/bin/env bash
# One-shot software acceptance for the OnePage C61 firmware.
#
#   scripts/run-software-acceptance.sh
#
# Stages (each prints `ok`/`FAIL`; any FAIL gives a non-zero exit, but every stage runs):
#   1. host tests of the HAL-free board logic      scripts/test-board-logic.sh
#   2. host tests of the shared reader logic       scripts/host-test.sh
#   3. English TXT/EPUB regression                 check-reader-regression.sh both
#        pre-port HEAD and the work tree run the same tests and must produce the
#        same golden trace sha256
#   4. build matrix                                X4 offline, X4 + wifi, C61 (both images)
#   5. boundary / equivalence checks (board selection, host boundary,
#      offline radio, C61 raw-GPIO, X4 traces)
#   6. C61 ELF memory budget                       report-c61-memory.sh
#   7. cargo fmt --check
# Finally prints the hardware acceptance status: what has NOT been verified on hardware.
#
# The firmware builds share one target root with check-offline-boundary.sh and
# check-c61-no-c3-raw-gpio.sh, so those stages reuse the ELFs instead of
# rebuilding.
#
# Env: ACCEPT_TARGET_ROOT (default target/accept), READER_REG_WORKDIR (see
# check-reader-regression.sh). Needs network on the first run (cargo fetch), GNU
# coreutils/awk (as the other check scripts), and the nightly pinned in
# rust-toolchain.toml.
set -u
cd "$(dirname "$0")/.."
source scripts/lib/tools.sh || exit $?

root="${ACCEPT_TARGET_ROOT:-target/accept}"
x4_elf="$root/x4/riscv32imc-unknown-none-elf/release/pulp-os"
x4w_elf="$root/x4-wifi/riscv32imc-unknown-none-elf/release/pulp-os"
c61_dir="$root/c61"
c61_elf="$c61_dir/riscv32imac-unknown-none-elf/release/pulp-os-c61"
c61b_elf="$c61_dir/riscv32imac-unknown-none-elf/release/pulp-os-c61-boot"

results=()
fail=0
stage() { # <name> <command...>
  local name=$1; shift
  echo; echo "######## $name"
  if "$@"; then results+=("ok    $name"); else results+=("FAIL  $name"); fail=1; fi
}
size_row() { # <label> <elf>; fails on a missing ELF or unreadable `size` output
  local row
  [ -f "$2" ] || { echo "FAIL  missing ELF for $1: $2"; return 1; }
  row="$(size "$2" | awk 'NR==2 && $1 ~ /^[0-9]+$/ {printf "text %-8s data %-6s bss %-7s", $1, $2, $3}')"
  [ -n "$row" ] || { echo "FAIL  cannot read size of $2"; return 1; }
  printf '%-22s%s\n' "$1" "$row"
}
matrix_sizes() {
  local rc=0
  size_row 'X4 offline'      "$x4_elf"   || rc=1
  size_row 'X4 + wifi'       "$x4w_elf"  || rc=1
  size_row 'C61 pulp-os-c61' "$c61_elf"  || rc=1
  size_row 'C61 c61-boot'    "$c61b_elf" || rc=1
  return $rc
}

stage "host tests (pulp-board-logic)" scripts/test-board-logic.sh
stage "host tests (pulp-host, fontpack, fontconv)" scripts/host-test.sh --locked
stage "C61 shared ADC and SD probe adapters" bash scripts/check-c61-adapter-regression.sh
stage "reader regression, head vs tree" scripts/check-reader-regression.sh both

stage "build X4 offline"     env CARGO_TARGET_DIR="$root/x4"     cargo build-x4 --locked
stage "build X4 + wifi (control)" env CARGO_TARGET_DIR="$root/x4-wifi" cargo build-x4-wifi --locked
stage "build C61 full + boot" env CARGO_TARGET_DIR="$c61_dir"    cargo build-c61 --locked

stage "build matrix sizes" matrix_sizes

stage "board selection errors"        scripts/check-board-selection.sh
stage "host boundary probes"          env ACCEPT_TARGET_ROOT="$root" scripts/check-host-boundary.sh --skip-firmware
stage "offline radio boundary"        env OFFLINE_CHECK_TARGET_ROOT="$root" scripts/check-offline-boundary.sh
stage "C61 free of C3 raw GPIO"       env C61_TARGET_DIR="$c61_dir" X4_ELF="$x4_elf" scripts/check-c61-no-c3-raw-gpio.sh
stage "X4 input trace equivalence"    scripts/check-x4-input-trace.sh
stage "X4 driver trace equivalence"   scripts/check-x4-driver-trace.sh
stage "C61 ELF memory budget"         env C61_ELF="$c61_elf" scripts/report-c61-memory.sh
# -p, not --all: --all also checks path dependencies (../smol-epub is another repo)
stage "cargo fmt --check"             cargo fmt -p pulp-os -p pulp-kernel -p pulp-board-logic -- --check

echo
echo "################ software acceptance summary"
printf '%s\n' "${results[@]}"

cat <<'EOF'

################ hardware acceptance status, NOT run on a OnePage C61
UNVERIFIED  boot (flash40 / PSRAM40 image-hash, espflash >= 4.6.0 for esp32c61)
UNVERIFIED  PSRAM detection, 40 MHz, heap/stack high-water, cache/DMA coherence
UNVERIFIED  SD (card-detect polarity, SPI/DMA, write latency, power-loss behaviour)
UNVERIFIED  ADC ladder / GPIO keys (voltage windows, calibration, feel, 15 ms debounce)
UNVERIFIED  EPD (orientation, BUSY time, waveform, ghosting; init sequence only matched to X4)
UNVERIFIED  battery sampling and USB detect polarity (BSP code vs README conflict)
UNVERIFIED  deep sleep, GPIO2 wake (arm-before-poweroff differs from BSP), GPIO27/GPIO10 pad state
UNVERIFIED  sleep/active current, session restore across a real power cycle
UNVERIFIED  X4 on hardware (ported to HAL 1.2: SPI, sleep, startup)
Procedure and record template: see specs/changes/archive/onepage-c61-port/.
EOF

exit "$fail"
