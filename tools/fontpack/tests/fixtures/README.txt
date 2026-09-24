Iansui subset test fixtures

IansuiSubset.ttf
    A Modified Version (SIL OFL 1.1 terms) of Iansui Regular, subset so
    the host test suite runs without the full 9.4 MB font. It stays under
    the SIL Open Font License 1.1; OFL.txt in this directory is the
    upstream license file, byte for byte. The Iansui notice declares no
    Reserved Font Name, so the family name is kept.

    source      Iansui-Regular.ttf, 9,447,424 bytes,
                SHA-256 7f1aa62e9dcbf40d0ce41a5d3f1e5ea602e66c295778ac6fefb6b84d8ed08bd5
                (https://github.com/ButTaiwan/iansui)
    result      71,444 bytes, 204 code points,
                SHA-256 08fb8d589fb6a2ca7ed4fce9c35036b05d9f8269ef53252c5b71598ceba77a2b
    contents    printable ASCII U+0020..U+007E, U+25A1 (the default
                fallback) and every character of
                render/tests/fixtures/iansui-sample.txt. Iansui has no
                Hangul, so the sample's 한 is absent and exercises the
                fallback.

    generated once, from the repository root, with fontTools 4.66.0
    (the test suite itself needs no Python):

        uvx --from fonttools==4.66.0 pyftsubset Iansui-Regular.ttf \
            --unicodes=U+0020-007E,U+25A1 \
            --text-file=render/tests/fixtures/iansui-sample.txt \
            --no-hinting --layout-features='' --name-IDs='*' \
            --output-file=tools/fontpack/tests/fixtures/IansuiSubset.ttf

    the output is deterministic (two runs gave the same SHA-256).

OFL.txt
    Iansui's license file, unchanged (4,479 bytes).

render/tests/fixtures/IANSUI24.PFP
    The subset converted at 24 px, loaded by the render acceptance test
    (render/tests/iansui_acceptance.rs). It is generated output: the
    converter self-verifies it with the device loader and a per-glyph
    round trip, and tests/convert.rs
    (tracked_render_fixture_pack_is_reproducible) fails if regenerating it
    from IansuiSubset.ttf and OFL.txt does not give the same bytes.
    Regenerate after changing the subset or the converter with:

        cargo run -p pulp-fontpack --release -- \
            --ttf tools/fontpack/tests/fixtures/IansuiSubset.ttf \
            --license tools/fontpack/tests/fixtures/OFL.txt \
            --size 24 --family IANSUI --out out/fixture/
        cp out/fixture/IANSUI24.PFP render/tests/fixtures/

    Editing iansui-sample.txt requires regenerating both files.

redistribution
    Iansui is SIL OFL 1.1. Three of its conditions bear on what this
    repository may do with the artifacts, and all three are satisfied:

      clause 1  neither the font nor a pack may be SOLD BY ITSELF. The
                firmware is MIT and free; nothing here is sold.
      clause 2  a copy may be redistributed with software provided that
                each copy contains the copyright notice and the license.
                The .PFP format satisfies this inside the file itself: the
                header carries license_off/license_len and the converter
                embeds the OFL text as its final section, so a pack lifted
                off an SD card is self-licensing. The raw TTF fixtures are
                accompanied by OFL.txt in the same directory.
      clause 3  no Modified Version may use a Reserved Font Name. Iansui's
                notice declares none, so IansuiSubset.ttf keeps the family
                name and a .PFP may be named after it.

    Consequence for this repo: the firmware stays MIT and the font
    artifacts stay OFL. "Does not apply to any document created using the
    fonts" is what lets an MIT reader ship with an OFL font; keep the two
    licenses clearly separated in any release.

    The full-size upstream original is NOT tracked here. It is 9.4 MB and
    every test that needs font data uses the 71 KB subset. To rebuild from
    source, put Iansui-Regular.ttf and its OFL.txt in the repository root
    (both gitignored) and follow the commands above.
