#!/usr/bin/env bash
# One-shot software acceptance for the OnePage C61 port (T13; R21, R22, R23).
#
#   scripts/run-software-acceptance.sh [--with-mutants] [--skip-builds]
#
# Stages (each prints `ok`/`FAIL`; any FAIL gives a non-zero exit, but every stage runs):
#   1. host tests of the HAL-free logic            scripts/test-board-logic.sh
#   2. English TXT/EPUB regression (R21)           check-reader-regression.sh both
#        pre-port HEAD and the work tree run the same tests and must produce the
#        same golden trace sha256
#   3. build matrix (R1, R3, R4)                   X4 offline, X4 + wifi, C61 (both images)
#   4. boundary / equivalence checks (R2, R4, R5, R8)
#   5. C61 ELF memory budget (R14, R15, R22)       report-c61-memory.sh
#   6. cargo fmt --check
#   --with-mutants  also run check-reader-mutants.sh (slow: one full suite per mutant)
#   --skip-builds   skip stage 3 and reuse the ELFs under target/accept-* (they must exist)
# Finally prints the R23 table: what has NOT been verified on hardware.
#
# Env: ACCEPT_TARGET_ROOT (default target/accept), READER_REG_WORKDIR (see
# check-reader-regression.sh). Needs network on the first run (cargo fetch), GNU
# coreutils/awk (as the other check scripts), and the nightly pinned in
# rust-toolchain.toml.
set -u
cd "$(dirname "$0")/.."
source scripts/lib/tools.sh || exit $?

WITH_MUTANTS=0
SKIP_BUILDS=0
for a in "$@"; do
  case "$a" in
    --with-mutants) WITH_MUTANTS=1 ;;
    --skip-builds) SKIP_BUILDS=1 ;;
    *) echo "unknown arg $a" >&2; exit 2 ;;
  esac
done

root="${ACCEPT_TARGET_ROOT:-target/accept}"
x4_elf="$root-x4/riscv32imc-unknown-none-elf/release/pulp-os"
x4w_elf="$root-x4w/riscv32imc-unknown-none-elf/release/pulp-os"
c61_dir="$root-c61"
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
stage "C61 shared ADC and SD probe adapters" bash scripts/check-c61-adapter-regression.sh
stage "reader regression, head vs tree (R21)" scripts/check-reader-regression.sh both
[ "$WITH_MUTANTS" = 1 ] && stage "reader mutants" scripts/check-reader-mutants.sh

if [ "$SKIP_BUILDS" = 0 ]; then
  stage "build X4 offline (R3, R4)"     env CARGO_TARGET_DIR="$root-x4"  cargo build-x4 --locked
  stage "build X4 + wifi (control)"     env CARGO_TARGET_DIR="$root-x4w" cargo build-x4-wifi --locked
  stage "build C61 full + boot (R1)"    env CARGO_TARGET_DIR="$c61_dir"  cargo build-c61 --locked
fi

stage "build matrix sizes" matrix_sizes

stage "board selection errors (R2)"   scripts/check-board-selection.sh
stage "offline radio boundary (R4, R5)" env OFFLINE_CHECK_TARGET_ROOT="$root-offline" scripts/check-offline-boundary.sh
stage "C61 free of C3 raw GPIO (R8)"  env C61_TARGET_DIR="$c61_dir" X4_ELF="$x4_elf" scripts/check-c61-no-c3-raw-gpio.sh
stage "X4 input trace equivalence"    scripts/check-x4-input-trace.sh
stage "X4 driver trace equivalence"   scripts/check-x4-driver-trace.sh
stage "X4 battery equivalence"        scripts/check-x4-battery-equiv.sh
stage "C61 ELF memory budget (R14, R15, R22)" env C61_ELF="$c61_elf" scripts/report-c61-memory.sh
# -p, not --all: --all also checks path dependencies (../smol-epub is another repo)
stage "cargo fmt --check"             cargo fmt -p pulp-os -p pulp-kernel -p pulp-board-logic -- --check

echo
echo "################ software acceptance summary"
printf '%s\n' "${results[@]}"

cat <<'EOF'

################ R23: hardware acceptance status, NOT run on a OnePage C61
UNVERIFIED  boot (flash40 / PSRAM40 image-hash, espflash >= 4.6.0 for esp32c61)
UNVERIFIED  PSRAM detection, 40 MHz, heap/stack high-water, cache/DMA coherence
UNVERIFIED  SD (card-detect polarity, SPI/DMA, write latency, power-loss behaviour)
UNVERIFIED  ADC ladder / GPIO keys (voltage windows, calibration, feel, 15 ms debounce)
UNVERIFIED  EPD (orientation, BUSY time, waveform, ghosting; init sequence only matched to X4)
UNVERIFIED  battery sampling and USB detect polarity (BSP code vs README conflict)
UNVERIFIED  deep sleep, GPIO2 wake (arm-before-poweroff differs from BSP), GPIO27/GPIO10 pad state
UNVERIFIED  sleep/active current, session restore across a real power cycle
UNVERIFIED  X4 on hardware (ported to HAL 1.2: SPI, sleep, startup)
Procedure and record template: see specs/changes/archive/onepage-c61-port/ (T14).
EOF

exit "$fail"
