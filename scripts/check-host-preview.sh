#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cli=scripts/host-preview.sh
if [[ ! -x "$cli" ]]; then
    echo "FAIL preview entrypoint missing or not executable: $cli" >&2
    exit 1
fi
work=$(mktemp -d "${TMPDIR:-/tmp}/pulp-preview-check.XXXXXX")
trap 'rm -rf "$work"' EXIT
count=0
pass() { count=$((count + 1)); echo "ok $*"; }
fail() { echo "FAIL $*" >&2; exit 1; }
run() {
    "$cli" "$@" >"$work/command.log" 2>&1 || {
        cat "$work/command.log" >&2
        fail "preview command: $*"
    }
}
validate() {
    python3 - "$1" "$2" <<'PY'
import pathlib, re, sys
root = pathlib.Path(sys.argv[1])
files = sorted(root.glob('*.pbm'))
assert len(files) == int(sys.argv[2]), (root, 'artifact count', len(files))
for path in files:
    data = path.read_bytes()
    # Read the three header tokens without treating binary raster as whitespace.
    pos = 0
    tokens = []
    while len(tokens) < 3:
        while pos < len(data) and data[pos] in b' \t\r\n':
            pos += 1
        if pos < len(data) and data[pos] == 35:
            pos = data.index(b'\n', pos) + 1
            continue
        start = pos
        while pos < len(data) and data[pos] not in b' \t\r\n':
            pos += 1
        tokens.append(data[start:pos])
    assert tokens == [b'P4', b'480', b'800'], (path, tokens)
    assert pos < len(data) and data[pos] in b' \t\r\n', (path, 'header terminator')
    pos += 2 if data[pos:pos + 2] == b'\r\n' else 1
    raster = data[pos:]
    assert len(raster) == 480 * 800 // 8, (path, 'complete raster', len(raster))
    assert any(raster) and any(byte != 255 for byte in raster), (path, 'blank frame')
PY
}
compare() {
    python3 - "$1" "$2" <<'PY'
import pathlib, sys
roots = [pathlib.Path(p) for p in sys.argv[1:]]
sets = [{p.name: p.read_bytes() for p in root.glob('*.pbm')} for root in roots]
assert sets[0] and sets[0] == sets[1], 'PBM names or bytes differ'
PY
}
reject() {
    if "$cli" "$@" >"$work/error.log" 2>&1; then
        fail "invalid arguments accepted: $*"
    fi
    [[ -s "$work/error.log" ]] || fail "invalid arguments lack diagnostic: $*"
    pass "reject $*"
}
run --help
for option in output-dir fixture page font theme; do
    grep -q -- "--$option" "$work/command.log" || fail "help lacks --$option"
done
pass "CLI help"
run --output-dir "$work/all-a"
validate "$work/all-a" 11
pass "eleven complete first-page PBMs"
run --output-dir "$work/all-b"
validate "$work/all-b" 11
compare "$work/all-a" "$work/all-b"
pass "directory-independent deterministic PBMs"
run --output-dir "$work/default" --fixture PLAINLF.TXT
run --output-dir "$work/explicit" --fixture PLAINLF.TXT --page 0 --font 2 --theme 1
validate "$work/default" 1
validate "$work/explicit" 1
compare "$work/default" "$work/explicit"
pass "documented defaults page 0 font 2 theme 1"
for option in page font theme; do
    value=0
    [[ "$option" != page ]] || value=1
    run --output-dir "$work/effect-$option" --fixture PLAINLF.TXT "--$option" "$value"
    validate "$work/effect-$option" 1
    python3 - "$work/default" "$work/effect-$option" <<'PY'
import pathlib, sys
frames = [next(pathlib.Path(p).glob('*.pbm')).read_bytes() for p in sys.argv[1:]]
# Compare rasters alone so changing filenames/header comments cannot pass.
assert frames[0][-48000:] != frames[1][-48000:], 'option ignored: identical pixels'
PY
    pass "independent $option pixel effect"
done
for fixture in PLAINLF.TXT E2STORED.EPU E3STORED.EPU; do
    run --output-dir "$work/$fixture-a" --fixture "$fixture" --page 1 --font 0 --theme 0
    run --output-dir "$work/$fixture-b" --fixture "$fixture" --page 1 --font 0 --theme 0
    validate "$work/$fixture-a" 1
    validate "$work/$fixture-b" 1
    compare "$work/$fixture-a" "$work/$fixture-b"
    pass "selected nondefault page/config $fixture"
done
reject
reject --unknown
reject --output-dir
reject --output-dir "$work/bad" --fixture UNKNOWN.TXT
for option in page font theme; do
    reject --output-dir "$work/bad" --fixture PLAINLF.TXT "--$option" -1
    reject --output-dir "$work/bad" --fixture PLAINLF.TXT "--$option" abc
    reject --output-dir "$work/bad" --fixture PLAINLF.TXT "--$option"
done
reject --output-dir "$work/bad" --fixture TINY.TXT --page 999999
font_count=$(sed -n 's/^pub const FONT_SIZE_COUNT: usize = \([0-9]*\);/\1/p' src/fonts/mod.rs)
theme_count=$(sed -n 's/^pub const NUM_READING_THEMES: u8 = \([0-9]*\);/\1/p' kernel/src/kernel/config.rs)
[[ -n "$font_count" && -n "$theme_count" ]] || fail "exported option bounds unavailable"
reject --output-dir "$work/bad" --fixture PLAINLF.TXT --font "$font_count"
reject --output-dir "$work/bad" --fixture PLAINLF.TXT --theme "$theme_count"
echo "PASS preview CLI ($count gates)"
