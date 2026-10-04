# Portable tool selection for the check scripts (Linux and macOS). Source it; do not run it.
#
# The ELF checks parse tool output, and GNU binutils / BSD tools format it differently
# (and Apple's objdump cannot read RISC-V at all). So instead of whatever is on PATH,
# the pinned toolchain's own tools are used on every platform:
#   nm, size, objdump, readelf  ->  llvm-nm / llvm-size / llvm-objdump / llvm-readobj
#                                   (rustup component `llvm-tools`, listed in rust-toolchain.toml)
#   awk                         ->  first of gawk, awk that has strtonum
#   sha256sum                   ->  shasum -a 256 when sha256sum is absent
#   host_triple                 ->  rustc's host, for host-side cargo runs
# The first four are shell functions, so they shadow the system commands inside the
# sourcing script and its $(...) subshells (not in `bash -c` children).

_tools_fail() { echo "FAIL  $*" >&2; return 2 2>/dev/null || exit 2; }

host_triple() { rustc -vV | sed -n 's/^host: //p'; }

_llvm_bin="$(rustc --print sysroot)/lib/rustlib/$(host_triple)/bin"
for _t in llvm-nm llvm-size llvm-objdump llvm-readobj; do
  [ -x "$_llvm_bin/$_t" ] || _tools_fail "missing $_t in $_llvm_bin (rustup component add llvm-tools)" || return 2 2>/dev/null || exit 2
done
nm()      { "$_llvm_bin/llvm-nm" "$@"; }
size()    { "$_llvm_bin/llvm-size" "$@"; }
objdump() { "$_llvm_bin/llvm-objdump" "$@"; }
readelf() { "$_llvm_bin/llvm-readobj" --elf-output-style=GNU "$@"; }

_awk=""
for _c in gawk awk; do
  if command -v "$_c" >/dev/null 2>&1 && "$_c" 'BEGIN { exit (strtonum("0x10") != 16) }' 2>/dev/null; then
    _awk="$(command -v "$_c")"; break
  fi
done
[ -n "$_awk" ] || { _tools_fail "no awk with strtonum (install GNU awk: brew install gawk / apt install gawk)"; return 2 2>/dev/null || exit 2; }
awk() { "$_awk" "$@"; }

command -v sha256sum >/dev/null 2>&1 || sha256sum() { shasum -a 256 "$@"; }

file_size() { wc -c < "$1" | tr -d '[:space:]'; }
