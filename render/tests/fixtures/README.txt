render/tests/fixtures

IANSUI16.PFP  IANSUI19.PFP  IANSUI23.PFP  IANSUI28.PFP  IANSUI35.PFP
    One pack per book-font size tier, at the pixel sizes build.rs
    rasterises for the body face (16/19/23/28/35 px). The reader picks a
    pack purely by file name -- src/apps/reader/font.rs maps the size tier
    to IANSUI<SS>.PFP and opens exactly that -- so these five names are the
    whole interface between a font on the card and a size in the reader.
    All five are generated from the same 71,444-byte subset below, so a
    tier test costs about 13..28 KB rather than a megabyte.

    Generated with the converter, from the repository root:

        for PX in 16 19 23 28 35; do
          cargo run -p pulp-fontpack --release -- \
            --ttf tools/fontpack/tests/fixtures/IansuiSubset.ttf \
            --license tools/fontpack/tests/fixtures/OFL.txt \
            --size $PX --family IANSUI --out render/tests/fixtures/
        done

    The converter also writes a .TXT build manifest and an IANSUOFL.TXT
    licence sidecar next to the pack. Neither is kept: the manifest is
    host-side bookkeeping the device never opens, and the licence already
    ships inside every .PFP (see docs/font-pack.txt section 2) and as
    tools/fontpack/tests/fixtures/OFL.txt.

IANSUI24.PFP
    Not a tier. 24 px is the size the reviewed golden images were rendered
    at, so this file is the oracle for render/tests/iansui_golden.rs and
    render/tests/iansui_acceptance.rs. The device never asks for it: no
    tier maps to 24 px, which is why the reader looks for IANSUI23.PFP at
    the Medium tier. Regenerate with:

        cargo run -p pulp-fontpack --release -- \
            --ttf tools/fontpack/tests/fixtures/IansuiSubset.ttf \
            --license tools/fontpack/tests/fixtures/OFL.txt \
            --size 24 --family IANSUI --out render/tests/fixtures/

iansui-sample.txt
    The one-paragraph CJK sample every pack test lays out. It exercises CJK
    punctuation pairs that must not be split, mixed Latin, kana, and 한 --
    which Iansui has no glyph for, so that one falls back to the pack's
    U+25A1. Editing it requires regenerating all six packs above.

running these on a device
    The committed packs are test fixtures, not what a device reads. A real
    card wants full-coverage packs built from the 9.4 MB original, and they
    are never tracked -- see tools/fontpack/tests/fixtures/README.txt for
    the licence terms. With Iansui-Regular.ttf and OFL.txt in the repository
    root:

        for PX in 16 19 23 28 35; do
          cargo run -p pulp-fontpack --release -- \
            --ttf Iansui-Regular.ttf --license OFL.txt \
            --size $PX --family IANSUI --out /media/$USER/SD/
        done

    Copy the .PFP files to the card root. A book then renders in CJK at any
    tier; a tier whose pack is missing falls back to the built-in fonts, so
    one size can be tested at a time.
