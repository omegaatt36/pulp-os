#!/usr/bin/env bash
# Cargo runner for the OnePage C61 (`cargo run-c61*`): called as `run-c61.sh <ELF> [args]`.
#
# Writes ONLY the app into ota_0 (0x10000) and keeps the board's factory
# bootloader + partition table. `espflash flash` would also rewrite 0x0 and 0x8000
# with its bundled ESP-IDF v6.1 bootloader, with which the app panics on every boot
# (Store/AMO access fault in esp_hal::interrupt::bind_handler; mechanism suspected
# to be the PMP setup of that bootloader, not proven).
#
# Flash 40 MHz DIO / 16 MB: 80 MHz hash-fails on this board;
# these only fill the app image header, the flash speed is set by the bootloader.
# The serial port comes from ESPFLASH_PORT (espflash reads it natively).
#
# Escape hatch: PULP_C61_BOOTLOADER=<bootloader.bin> does a full `espflash flash`
# with that bootloader instead (also rewrites the partition table). A working one
# can be cut from a factory backup: head -c $((0x8000)) backup.bin > bootloader.bin
set -euo pipefail

elf=${1:?usage: run-c61.sh <ELF>}
img_args=(--chip esp32c61 --flash-mode dio --flash-freq 40mhz --flash-size 16mb)

if [[ -n ${PULP_C61_BOOTLOADER:-} ]]; then
  [[ -f $PULP_C61_BOOTLOADER ]] || { echo "PULP_C61_BOOTLOADER: no such file: $PULP_C61_BOOTLOADER" >&2; exit 1; }
  exec espflash flash --monitor "${img_args[@]}" --bootloader "$PULP_C61_BOOTLOADER" "$elf"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

hex() { od -An -v -tx1 -j "$1" -N "$2" "$tmp/head.bin" | tr -d ' \n'; }

# Pre-flight (read-only): first 0x9000 bytes = bootloader + partition table.
# Expect a valid bootloader image (magic 0xE9) and a partition table (magic 0xAA50,
# 32-byte entries) with an ota_0 app entry (type 00, subtype 10) at 0x10000. The
# espflash default table has a *factory* (subtype 00) app there, so it is refused.
espflash read-flash 0x0 0x9000 "$tmp/head.bin" --chip esp32c61 >/dev/null
found="bootloader byte 0x0 = 0x$(hex 0 1) (want e9)"
ok=
if [[ $(hex 0 1) == e9 ]]; then
  for ((off = 0x8000; off < 0x9000; off += 32)); do
    entry=$(hex "$off" 8)
    [[ $entry == aa50* ]] || break
    [[ $entry == aa50001000000100 ]] && { ok=1; break; }
  done
  found+=", partition table 0x8000 = 0x$(hex 0x8000 2) (want aa50)"
fi
if [[ -z $ok ]]; then
  cat >&2 <<MSG
run-c61: refusing to flash, the board does not have the factory layout.
  found: $found; no ota_0 app entry at 0x10000
  App-only flashing keeps the bootloader and partition table already on the board,
  and needs the factory ones (nvs 0x9000, otadata 0xe000, ota_0 0x10000): the
  bootloader bundled with espflash makes the app panic on boot.
  Fix: restore a factory backup, or
  set PULP_C61_BOOTLOADER=<bootloader.bin> for a full flash with that bootloader.
MSG
  exit 1
fi

espflash save-image "${img_args[@]}" "$elf" "$tmp/app.bin"
espflash write-bin 0x10000 "$tmp/app.bin" --chip esp32c61
# monitor needs a terminal for its key handling; without one (agents, CI) log only
mon_args=()
[[ -t 0 ]] || mon_args=(--non-interactive)
espflash monitor --chip esp32c61 ${mon_args[@]+"${mon_args[@]}"}
