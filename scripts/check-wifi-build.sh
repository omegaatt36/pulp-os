#!/usr/bin/env bash
# Wi-Fi build smoke: fixed radio set, radio optimization,
# enabled / disabled link, and the memory report for the enabled C61 image.
#
#   Wi-Fi enabled: C61 (`board-onepage-c61,wifi`) links a release ELF with the pinned,
#        C61-capable radio / RTOS set (versions from specs/references/onepage-wifi-support.md),
#        and the ELF really contains the radio driver (esp_wifi_start / _init_internal /
#        _connect_internal). X4 + wifi must not fall back to esp-radio 0.17 / esp-rtos 0.2.
#   the esp_radio crate is compiled with opt-level 2 or 3 (read from the real rustc
#        command line, on both enabled configurations).
#        smoltcp has no proto-ipv6 (IPv4-only stack).
#   Wi-Fi disabled: delegated to scripts/check-offline-boundary.sh (dependency graph
#        and ELF probes for X4 and C61, plus enabled positive controls).
#   starting point: size + scripts/report-c61-memory.sh on the enabled C61 ELF, written
#        to $root/wifi-memory-baseline.txt. No heap / stack numbers are asserted here; the
#        report script's own verdict must be "ok", otherwise this script fails.
#
# opt-level capture: `cargo build -v` prints the rustc command only when a unit is actually
# compiled, and a cache hit prints nothing. So before each enabled build the esp-radio
# package is cleaned (`cargo clean -p esp-radio`) inside the dedicated target dir; the
# following `-v` build must recompile it and print its command. This is cheaper than a
# full fresh build (core/alloc stay cached) and does not depend on parsing fingerprint files.
#
# Targets build into separate CARGO_TARGET_DIRs under target/ (override the root with
# WIFI_CHECK_TARGET_ROOT). The two enabled builds share the dirs of
# check-offline-boundary.sh (<root>/offline-boundary/{x4-wifi,c61-wifi}) so that script
# finds them already built.
set -u
cd "$(dirname "$0")/.."
source scripts/lib/tools.sh || exit $?
root="${WIFI_CHECK_TARGET_ROOT:-target/wifi-build}"
offroot="$root/offline-boundary"
fail=0
ok()   { echo "ok    $*"; }
bad()  { echo "FAIL  $*"; fail=1; }
hr()   { echo; echo "== $* =="; }
command -v cargo >/dev/null 2>&1 || { echo "FAIL  missing tool: cargo"; exit 2; }

X4T=riscv32imc-unknown-none-elf
C61T=riscv32imac-unknown-none-elf
C61F=board-onepage-c61,wifi
X4F=board-x4,wifi
# Same pattern and absolute-symbol rule as check-offline-boundary.sh.
SYM_RE='esp_radio|esp_wifi|embassy_net|smoltcp'
# Present in the reference probe ELF (specs/references/onepage-wifi-support.md).
REQUIRED_SYMS=(esp_wifi_start esp_wifi_init_internal esp_wifi_connect_internal)

mkdir -p "$offroot"

# "name|version|features" for every package in the normal-dependency graph of a config
tree_pkgs() { # <target> <features>
  cargo tree --locked -e normal --target "$1" --features "$2" --prefix none --format '{p}|{f}' 2>&1 |
    awk -F'|' 'NF >= 2 { split($1, a, " "); v = a[2]; sub(/^v/, "", v); f = $2; sub(/ \(\*\)$/, "", f); print a[1] "|" v "|" f }'
}

# every resolved version of <name> must match the anchored regex <want>
expect_version() { # <desc> <pkgs> <name> <want regex>
  local desc=$1 pkgs=$2 name=$3 want=$4 vers v
  vers=$(awk -F'|' -v n="$name" '$1 == n { print $2 }' <<<"$pkgs" | sort -u)
  if [ -z "$vers" ]; then bad "$desc: $name not in dependency graph"; return; fi
  for v in $vers; do
    if grep -qE "^($want)\$" <<<"$v"; then ok "$desc: $name $v"; else bad "$desc: $name $v (want $want)"; fi
  done
}

# no resolved version of <name> may match the anchored regex <stale>
reject_version() { # <desc> <pkgs> <name> <stale regex>
  local desc=$1 pkgs=$2 name=$3 stale=$4
  if awk -F'|' -v n="$name" '$1 == n { print $2 }' <<<"$pkgs" | grep -qE "^($stale)\$"; then
    bad "$desc: $name uses a stale version (matches $stale)"
  else
    ok "$desc: $name is not a stale version"
  fi
}

# feature <feat> of <name> enabled (want=yes) or not (want=no)
expect_feature() { # <desc> <pkgs> <name> <feat> <yes|no>
  local desc=$1 pkgs=$2 name=$3 feat=$4 want=$5 feats
  feats=$(awk -F'|' -v n="$name" '$1 == n { print $3 }' <<<"$pkgs")
  if [ -z "$feats" ]; then bad "$desc: $name not in dependency graph"; return; fi
  if tr ',' '\n' <<<"$feats" | grep -qx "$feat"; then
    [ "$want" = yes ] && ok "$desc: $name has feature $feat" || bad "$desc: $name must not have feature $feat"
  else
    [ "$want" = no ] && ok "$desc: $name lacks feature $feat" || bad "$desc: $name lacks feature $feat"
  fi
}

# Clean esp-radio in <dir>, then build with -v so its rustc command is printed.
# Log: <dir>.verbose.log. Returns non-zero (after reporting) if the build fails.
enabled_build() { # <desc> <dir> <target> <alias> <features-arg...>
  local desc=$1 dir=$2 target=$3; shift 3
  if ! CARGO_TARGET_DIR="$dir" cargo clean -p esp-radio --release --target "$target" --locked >"$dir.clean.log" 2>&1; then
    bad "$desc: cargo clean -p esp-radio failed (log: $dir.clean.log)"; tail -5 "$dir.clean.log" | cut -c1-300; return 1
  fi
  if ! CARGO_TARGET_DIR="$dir" cargo "$@" --locked -v >"$dir.verbose.log" 2>&1; then
    bad "$desc: build failed: cargo $* (log: $dir.verbose.log)"; tail -8 "$dir.verbose.log" | cut -c1-300; return 1
  fi
  ok "$desc: build/link succeeded"
}

# opt-level of the esp_radio rustc invocation(s) captured in a -v log must be 2 or 3
radio_opt_level() { # <desc> <verbose log>
  local desc=$1 log=$2 cmds line lvl n=0
  cmds=$(grep -E 'Running `.* --crate-name esp_radio ' "$log" 2>/dev/null | grep -e '--crate-type lib')
  if [ -z "$cmds" ]; then
    bad "$desc: no rustc command for crate esp_radio in $log (not compiled, or build stopped before it)"
    return
  fi
  while IFS= read -r line; do
    n=$((n + 1))
    lvl=$(grep -oE 'opt-level=[^ `]+' <<<"$line" | tail -1 | cut -d= -f2)
    case "$lvl" in
      2|3) ok "$desc: esp_radio compiled with opt-level=$lvl" ;;
      *)   bad "$desc: esp_radio compiled with opt-level=${lvl:-<none, rustc default 0>} (want 2 or 3)" ;;
    esac
  done <<<"$cmds"
}

# ===== C61 + wifi: ===================================================================
hr "C61 + wifi: dependency set"
c61pkgs=$(tree_pkgs $C61T $C61F)
if [ -z "$c61pkgs" ]; then
  bad "c61 wifi: cargo tree produced no packages"
else
  expect_version "c61 wifi" "$c61pkgs" esp-hal '1\.2\.0'
  expect_version "c61 wifi" "$c61pkgs" esp-rtos '0\.4\.0'
  expect_version "c61 wifi" "$c61pkgs" esp-alloc '0\.11\.0'
  expect_version "c61 wifi" "$c61pkgs" esp-radio '1\.0\.0-beta\.1'
  expect_version "c61 wifi" "$c61pkgs" esp-bootloader-esp-idf '0\.6\.0'
  expect_version "c61 wifi" "$c61pkgs" embassy-net '0\.8\.[0-9]+'
  expect_version "c61 wifi" "$c61pkgs" esp-radio-rtos-driver '0\.4\.2'
  expect_version "c61 wifi" "$c61pkgs" esp-phy '0\.3\.0'
  expect_version "c61 wifi" "$c61pkgs" esp-wifi-sys-esp32c61 '0\.3\.0'
  expect_feature "c61 wifi" "$c61pkgs" esp-radio esp32c61 yes
  expect_feature "c61 wifi" "$c61pkgs" esp-radio esp32c3 no
  # The service requires IPv4: the serving address is read with config_v4(), so an IPv6-capable stack
  # would let wait_config_up() return without an IPv4 address.
  expect_feature "c61 wifi" "$c61pkgs" smoltcp proto-ipv6 no
fi

hr "X4 + wifi: no stale radio / RTOS"
x4pkgs=$(tree_pkgs $X4T $X4F)
if [ -z "$x4pkgs" ]; then
  bad "x4 wifi: cargo tree produced no packages"
else
  # stale sets named in the brief: esp-radio 0.17.x and esp-rtos 0.2.x
  expect_version "x4 wifi" "$x4pkgs" esp-radio '.*'
  expect_version "x4 wifi" "$x4pkgs" esp-rtos '.*'
  reject_version "x4 wifi" "$x4pkgs" esp-radio '0\.17\..*'
  reject_version "x4 wifi" "$x4pkgs" esp-rtos '0\.2\..*'
fi

hr "C61 + wifi: link and ELF"
c61dir="$offroot/c61-wifi"
c61elf="$c61dir/$C61T/release/pulp-os-c61"
if enabled_build "c61 wifi" "$c61dir" $C61T build-c61 --features wifi; then
  [ -f "$c61elf" ] && ok "c61 wifi: ELF exists ($c61elf)" || bad "c61 wifi: ELF missing ($c61elf)"
fi
if [ -f "$c61elf" ]; then
  syms=$(mktemp); nm -C "$c61elf" 2>/dev/null | awk '$2 != "A" && $2 != "a"' >"$syms"
  n=$(grep -ciE "$SYM_RE" "$syms")
  [ "$n" -gt 0 ] && ok "c61 wifi: radio/net symbols found ($n)" || bad "c61 wifi: probe finds no radio/net symbols"
  for s in "${REQUIRED_SYMS[@]}"; do
    if awk -v s="$s" '$NF == s { f = 1 } END { exit !f }' "$syms"; then
      ok "c61 wifi: symbol $s"
    else
      bad "c61 wifi: symbol $s missing (non-absolute)"
    fi
  done
  rm -f "$syms"
fi

hr "C61 + wifi: radio optimization"
radio_opt_level "c61 wifi" "$c61dir.verbose.log"

# ===== X4 + wifi: =========================================================================
hr "X4 + wifi: build and radio optimization"
x4dir="$offroot/x4-wifi"
enabled_build "x4 wifi" "$x4dir" $X4T build-x4 --features wifi
radio_opt_level "x4 wifi" "$x4dir.verbose.log"

# ===== disabled builds: ===================================================================
hr "Wi-Fi disabled : scripts/check-offline-boundary.sh"
if OFFLINE_CHECK_TARGET_ROOT="$offroot" scripts/check-offline-boundary.sh; then
  ok "offline boundary (X4 / C61 disabled absent; X4 / C61 enabled positive controls)"
else
  bad "offline boundary check failed"
fi

# ===== memory starting point: ============================================================
hr "C61 + wifi: memory baseline (starting point)"
base="$root/wifi-memory-baseline.txt"
if [ ! -f "$c61elf" ]; then
  bad "memory baseline: enabled C61 ELF missing ($c61elf)"
else
  rep=$(mktemp)
  C61_ELF="$c61elf" scripts/report-c61-memory.sh >"$rep" 2>&1; rc=$?
  sz=$(size "$c61elf")
  {
    echo "elf: $c61elf"
    echo "features: $C61F"
    echo "rustc: $(rustc -V)"
    echo "esp-radio: $(awk -F'|' '$1 == "esp-radio" { print $2; exit }' <<<"${c61pkgs:-}")"
    echo
    echo "-- size"
    echo "$sz"
    echo
    echo "-- scripts/report-c61-memory.sh (exit $rc)"
    cat "$rep"
  } >"$base"
  rm -f "$rep"
  cat "$base"
  echo
  if awk 'NR == 2 && NF >= 4 && $1 ~ /^[0-9]+$/ && $2 ~ /^[0-9]+$/ && $3 ~ /^[0-9]+$/ && $1 > 0 { f = 1 } END { exit !f }' <<<"$sz"; then
    ok "baseline has size text/data/bss ($base)"
  else
    bad "baseline: size output has no numeric text/data/bss"
  fi
  grep -q '^RESULT:' "$base" && ok "baseline carries the report verdict" || bad "baseline: report printed no RESULT line"
  if [ "$rc" -eq 0 ]; then ok "report-c61-memory.sh on the enabled ELF: all budget checks ok"
  else bad "report-c61-memory.sh on the enabled ELF failed (exit $rc); see $base"; fi
fi

echo
[ "$fail" -eq 0 ] && echo "RESULT: wifi build checks ok" || echo "RESULT: wifi build checks FAILED"
exit "$fail"
