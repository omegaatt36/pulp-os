#!/usr/bin/env bash
# onepage-host-validation / boundary acceptance acceptance: host build boundary (host boundary) and
# firmware build regression (firmware builds).
#
# Usage: scripts/check-host-boundary.sh [--skip-firmware]
#   --skip-firmware   run only the host boundary host checks (fast iteration); the default
#                     also runs the firmware builds firmware builds.
#
# ---------------------------------------------------------------------------
# CONTRACT the implementation must provide (this script only verifies it)
#
#   1. A workspace member crate with package name exactly:   pulp-host
#      This is the single host entry crate. paging regression-render regression (paging / rendering /
#      storage host tests) hang off the same crate and the same command.
#      It must contain at least one #[test] (boundary acceptance needs a smoke test so the
#      host command demonstrably executes something; an empty test run is
#      rejected below).
#
#   2. The host acceptance command, verbatim:
#
#          scripts/host-test.sh [extra cargo test args...]
#
#      which must be equivalent to
#
#          cargo test -p pulp-host --target "$(rustc -vV | sed -n 's/^host: //p')" \
#              --config 'unstable.build-std=["std","test"]' "$@"
#
#      (same shape as scripts/test-board-logic.sh). Properties required:
#        a. Targets rustc's HOST triple. It never builds for, or passes,
#           any riscv32* target, and does not use the board-* features.
#        b. Honors $CARGO_TARGET_DIR and forwards extra args ("$@") to
#           `cargo test` (this script passes `-vv` and `--locked`).
#        c. Exits 0 and runs >= 1 test.
#        d. host boundary: the pulp-host dependency graph (normal+build+dev edges, host
#           target) contains no Espressif crate (no package named esp-* or
#           esp_*, e.g. esp-hal, esp-rtos, esp-backtrace, esp-println,
#           esp-bootloader-esp-idf, esp-alloc, esp-radio), and the build
#           does not apply any embedded linker script (no `-T<x>.x` linker
#           argument such as -Tlinkall.x, no `linkall.x`).
#           In practice this means the kernel/build.rs/.cargo logic that
#           emits `cargo:rustc-link-arg=-Tlinkall.x` etc. and the esp-*
#           dependencies must be conditional on the firmware target (or
#           otherwise out of pulp-host's graph), and font generation in
#           build.rs must work when the host crate is built.
#
#   No new cargo alias is required: aliases cannot compute the host triple,
#   hence the wrapper script (precedent: scripts/test-board-logic.sh).
#
# Expected values are derived from the requirement text:
#   host boundary "不載入 Espressif 硬體 runtime 或 embedded linker scripts"
#      -> forbidden: any package named esp-*/esp_* in the host graph; any
#         -T*.x linker-script argument in the host build's verbose output.
#   firmware builds "OnePage 與 X4 離線 firmware 可編譯"
#      -> `cargo build-x4` and `cargo build-c61` (.cargo/config.toml
#         aliases) exit 0 with --locked.
#   shared production logic (shared production logic) is NOT covered here; it is verified at
#      paging regression/render regression where actual paging/rendering code is wired to the host crate.
# ---------------------------------------------------------------------------
set -u
cd "$(dirname "$0")/.."
source scripts/lib/tools.sh || exit $?

skip_firmware=0
for a in "$@"; do
  case "$a" in
    --skip-firmware) skip_firmware=1 ;;
    *) echo "usage: $0 [--skip-firmware]" >&2; exit 2 ;;
  esac
done

fail=0
ok()  { echo "ok    $*"; }
bad() { echo "FAIL  $*"; fail=1; }

host="$(host_triple)"
HOST_CMD=scripts/host-test.sh
HOST_PKG=pulp-host
# Espressif crates by name prefix (host boundary "Espressif 硬體 runtime")
ESP_RE='^esp[-_]'
# Embedded linker-script arguments (host boundary): any `-T<script>.x`, or linkall.x itself
LD_RE='(^|[[:space:]=])-T[^[:space:]]*\.x|linkall\.x'

out_dir=target/accept-host
mkdir -p target

# ---------------------------------------------------------------------------
# Probe self-checks (positive controls): the detectors below must fire on
# known-bad input, otherwise a pass would mean nothing.
# ---------------------------------------------------------------------------
probe_ok=1
if ! grep -Eq "$LD_RE" <<<'-C link-arg=-Tlinkall.x'; then
  bad "probe broken: linker regex does not match -Tlinkall.x"; probe_ok=0
fi
if grep -Eq "$LD_RE" <<<'-C link-arg=-fuse-ld=lld --target aarch64-apple-darwin'; then
  bad "probe broken: linker regex matches an ordinary host link arg"; probe_ok=0
fi
# Real graph control: the X4 firmware graph must contain esp-hal as seen by
# the same tree/parse pipeline used for the host crate.
esp_pkgs() { # stdin: `cargo tree --prefix none --format '{p}'`; prints esp-* names
  awk '{ print $1 }' | grep -E "$ESP_RE" | sort -u
}
ctl=$(cargo tree --locked -e normal,build --prefix none --format '{p}' \
        --target riscv32imc-unknown-none-elf --features board-x4 2>/dev/null | esp_pkgs)
if grep -qx 'esp-hal' <<<"$ctl"; then
  ok "probe control: X4 firmware graph shows esp-hal through the same pipeline"
else
  bad "probe broken: X4 firmware graph did not show esp-hal"; probe_ok=0
fi

# ---------------------------------------------------------------------------
# host boundary (a): host crate exists, and its host-target graph has no esp-* crate
# ---------------------------------------------------------------------------
echo "== host: host dependency graph ($HOST_PKG, target $host)"
tree_err=$(mktemp)
if tree=$(cargo tree --locked -p "$HOST_PKG" -e normal,build,dev --target "$host" \
            --prefix none --format '{p}' 2>"$tree_err"); then
  if [ "$(head -1 <<<"$tree" | awk '{print $1}')" != "$HOST_PKG" ]; then
    bad "cargo tree for $HOST_PKG returned an unexpected graph root"
  else
    found=$(esp_pkgs <<<"$tree")
    if [ -n "$found" ]; then
      bad "host graph of $HOST_PKG contains Espressif crates: $(tr '\n' ' ' <<<"$found")"
    else
      ok "host graph of $HOST_PKG contains no esp-* crate ($(wc -l <<<"$tree" | tr -d ' ') packages)"
    fi
  fi
else
  bad "host package $HOST_PKG not resolvable: $(head -3 "$tree_err" | tr '\n' ' ')"
fi
rm -f "$tree_err"

# ---------------------------------------------------------------------------
# host boundary (b)+(c): run the real host command, verbose, from a clean target dir so
# every rustc / build-script invocation is printed.
# ---------------------------------------------------------------------------
echo "== host: host command ($HOST_CMD)"
log="$out_dir.log"
if [ ! -x "$HOST_CMD" ]; then
  bad "host acceptance command missing or not executable: $HOST_CMD"
else
  rm -rf "$out_dir"
  if ! CARGO_TARGET_DIR="$out_dir" "$HOST_CMD" --locked -vv >"$log" 2>&1; then
    bad "host command failed (exit non-zero): $HOST_CMD --locked -vv (log: $log)"
    tail -15 "$log"
  else
    ok "host command exited 0"
    # (c) it really ran tests: sum of `N passed` over all test binaries >= 1
    passed=$(sed -n 's/^test result: ok\. \([0-9][0-9]*\) passed.*/\1/p' "$log" \
             | awk '{ s += $1 } END { print s + 0 }')
    if [ "$passed" -ge 1 ]; then
      ok "host command executed $passed test(s)"
    else
      bad "host command ran no tests (empty set is not acceptable)"
    fi
    # (b) no embedded linker script, no esp-* crate compiled, no riscv target
    if hits=$(grep -En "$LD_RE" "$log"); then
      bad "host build applies an embedded linker script:"; head -5 <<<"$hits"
    else
      ok "no embedded linker script (-T*.x / linkall.x) in host build output"
    fi
    if hits=$(grep -E '^[[:space:]]*(Compiling|Fresh)[[:space:]]+esp[-_]' "$log"); then
      bad "host build compiled Espressif crates:"; head -5 <<<"$hits"
    else
      ok "no esp-* crate compiled in host build"
    fi
    if hits=$(grep -E -- '--target[ =]riscv32|riscv32[a-z]*-unknown-none' "$log"); then
      bad "host build touched a riscv32 firmware target:"; head -3 <<<"$hits"
    else
      ok "host build did not use a riscv32 target"
    fi
  fi
fi

# ---------------------------------------------------------------------------
# firmware builds: firmware regression gate (X4 and OnePage C61, offline, --locked)
# ---------------------------------------------------------------------------
if [ "$skip_firmware" -eq 1 ]; then
  echo "== firmware: skipped (--skip-firmware)"
else
  for board in x4 c61; do
    echo "== firmware: cargo build-$board"
    dir="target/accept-host-$board"
    if CARGO_TARGET_DIR="$dir" cargo "build-$board" --locked >"$dir.log" 2>&1; then
      ok "cargo build-$board exited 0"
    else
      bad "cargo build-$board failed (log: $dir.log)"; tail -15 "$dir.log"
    fi
  done
fi

[ "$probe_ok" -eq 1 ] || fail=1
if [ "$fail" -eq 0 ]; then echo "PASS  host boundary"; else echo "FAIL  host boundary"; fi
exit $fail
