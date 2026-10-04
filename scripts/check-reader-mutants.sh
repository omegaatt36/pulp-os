#!/usr/bin/env bash
# Mutation check for the reader regression suite (T13).
#
# Copies the sources the harness compiles into a scratch tree, breaks one thing at a
# time (a margin, a wrap constant, an off-by-one in paging, a bookmark limit, a
# settings key, ...), runs scripts/check-reader-regression.sh against that tree
# and requires it to FAIL. The unmutated scratch tree is run first and must pass,
# so a "killed" result cannot come from a broken harness.
#
# Mutants that were tried and are deliberately NOT in the list (equivalent: no
# observable difference through the reader's output, so no test can kill them):
#   LINES_PER_PAGE 37 -> 36   a cap only; max_lines is at most 25 for every shipped
#                             font x theme (772 / 31), so the cap never binds
#   CHAPTER_CACHE_MAX `>` -> `>=` the RAM chapter cache is a pure optimisation:
#                             pages are identical whether or not it is used
#
# Usage: scripts/check-reader-mutants.sh [--list] [mutant-id ...]
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRATCH="${READER_MUT_DIR:-${TMPDIR:-/tmp}/pulp-reader-mutants}"
SRC="$SCRATCH/src"

# id ~ file ~ old text ~ new text ~ what it breaks ~ occurrence (optional, default 1)
MUTANTS=$(cat <<'M'
R1~kernel/src/kernel/config.rs~margin_h: 16,~margin_h: 17,~Default theme margin 16 -> 17
R2~src/apps/reader/mod.rs~((native_h as u32 * spacing_pct as u32) / 100).max(1) as u16;~((native_h as u32 * spacing_pct as u32) / 101).max(1) as u16;~line spacing percentage divisor 100 -> 101
R3~src/apps/reader/paging.rs~if c <= b' ' || c > 0x7E {~if c < b' ' || c > 0x7E {~word-run scan treats space as part of a word
R4~src/apps/reader/paging.rs~if self.pg.page + 1 < self.pg.total_pages {~if self.pg.page + 1 <= self.pg.total_pages {~page_forward boundary off by one (2nd occurrence)~2
R5~src/apps/reader/paging.rs~let target = (self.pg.page + 10).min(last);~let target = (self.pg.page + 9).min(last);~TXT jump +10 -> +9
R6~src/apps/reader/paging.rs~if self.is_epub && self.epub.chapter > 0 {~if self.is_epub && self.epub.chapter > 1 {~page_backward chapter boundary off by one
R7~src/apps/reader/mod.rs~if self.pg.offsets[self.pg.page + 1] > target_off {~if self.pg.offsets[self.pg.page + 1] >= target_off {~offset -> page mapping off by one
R8~src/apps/reader/mod.rs~ActionEvent::LongPress(Action::Back) => Transition::Home,~ActionEvent::LongPress(Action::Back) => Transition::Pop,~long-press Back no longer goes Home
R9~kernel/src/kernel/bookmarks.rs~pub const SLOTS: usize = 16;~pub const SLOTS: usize = 15;~bookmark capacity 16 -> 15
R10~kernel/src/kernel/bookmarks.rs~if slot.generation < lru_gen {~if slot.generation > lru_gen {~LRU evicts the newest instead of the oldest
R11~kernel/src/kernel/bookmarks.rs~write_u16_le(&mut rec, 10, if self.valid { 1 } else { 0 });~write_u16_le(&mut rec, 10, if self.valid { 2 } else { 0 });~persisted valid flag changes
R12~kernel/src/kernel/config.rs~b"swap_buttons" => {~b"swap_button" => {~settings key renamed on read
R13~kernel/src/kernel/config.rs~b"wifi_pass" => w.set_pass(val),~b"wifi_pass" => {}~wifi_pass no longer preserved
R14~kernel/src/kernel/config.rs~wr.kv_num(b"book_font", s.book_font_size_idx as u16);~wr.kv_num(b"book_fnt", s.book_font_size_idx as u16);~settings key renamed on write
R15~kernel/src/kernel/config.rs~.clamp(MIN_GHOST_CLEAR, MAX_GHOST_CLEAR);~.clamp(MIN_GHOST_CLEAR, MAX_GHOST_CLEAR - 1);~ghost clear upper clamp 100 -> 99
R16~kernel/src/util/utf8.rs~} else if b0 < 0xE0 {~} else if b0 < 0xDF {~2-byte UTF-8 decode boundary
M
)

if [ "${1:-}" = "--list" ]; then echo "$MUTANTS" | awk -F'~' '{print $1": "$5}'; exit 0; fi

mkdir -p "$SCRATCH"
rm -rf "$SRC"; mkdir -p "$SRC"
cp -r "$ROOT/src" "$ROOT/kernel" "$ROOT/assets" "$ROOT/build.rs" "$SRC/"
rm -rf "$SRC/kernel/target"
export READER_REG_SRC="$SRC"
export READER_REG_WORKDIR="$SCRATCH/work"

echo "== baseline copy of the work tree (must pass)"
if ! bash "$ROOT/scripts/check-reader-regression.sh" tree > "$SCRATCH/baseline.log" 2>&1; then
    tail -20 "$SCRATCH/baseline.log"; echo "FAIL: unmutated scratch tree does not pass"; exit 1
fi
echo "ok   unmutated copy passes"

want=("$@")
killed=0; survived=0; total=0
while IFS='~' read -r id file old new desc occ; do
    [ -z "$id" ] && continue
    if [ ${#want[@]} -gt 0 ] && ! printf '%s\n' "${want[@]}" | grep -qx "$id"; then continue; fi
    total=$((total+1))
    target="$SRC/$file"
    cp "$target" "$target.orig"
    python3 - "$target" "$old" "$new" "${occ:-1}" <<'PY' || { echo "ERR  $id: pattern not found in $file"; mv "$target.orig" "$target"; exit 1; }
import sys
p, old, new, occ = sys.argv[1:5]
s = open(p).read()
idx = -1
for _ in range(int(occ)):
    idx = s.find(old, idx + 1)
    if idx < 0:
        sys.exit(1)
open(p, 'w').write(s[:idx] + new + s[idx + len(old):])
PY
    log="$SCRATCH/mutant-$id.log"
    if bash "$ROOT/scripts/check-reader-regression.sh" tree > "$log" 2>&1; then
        echo "SURVIVED $id  ($desc)"
        survived=$((survived+1))
    else
        failing=$(grep -E "^test .* FAILED" "$READER_REG_WORKDIR/tree/test.log" 2>/dev/null | sed -E 's/^test (.*) \.\.\. FAILED/\1/' | paste -sd, -)
        if grep -q "^FAIL golden trace" "$log" && [ -z "$failing" ]; then failing="(golden trace only)"; fi
        if grep -q "^FAIL golden trace" "$log" && [ -n "$failing" ]; then failing="$failing + golden trace"; fi
        if [ -z "$failing" ]; then failing="(harness build/run error; see $log)"; fi
        echo "KILLED   $id  ($desc)"
        echo "           by: $failing"
        killed=$((killed+1))
    fi
    mv "$target.orig" "$target"
done <<< "$MUTANTS"

echo "mutants: $total run, $killed killed, $survived survived"
[ "$survived" -eq 0 ]
