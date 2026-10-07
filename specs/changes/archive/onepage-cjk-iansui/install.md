# Iansui SD installation and validation

Generated acceptance set: `target/accept-iansui/packs/`. Copy all nine
`F00016.PFN`, `F00019.PFN`, `F00023.PFN`, `F00027.PFN`, `F00028.PFN`,
`F00032.PFN`, `F00035.PFN`, `F00038.PFN`, `F00046.PFN`, plus `PROV.TXT`,
`OFL.TXT` and `COVERAGE.TXT`, into `_PULP/FONTS/` at the SD card root.
The nine pack files total 14,792,301 bytes. Keep the provenance and license.
No removable device was written during acceptance.

Current reproducible installation workflow:

```sh
python3 scripts/build-cjk-fonts.py
```

Copy `target/cjk-sd/_PULP/FONTS/` to `_PULP/FONTS/` on the SD card.
Replace the old directory before installation. Keep all reports and license files.
`fonts/cjk.json` pins the original font and license by SHA256.
Its source version is a hash identifier, not an upstream release tag.
The duplicate `iansui.zip` was removed after exact comparison with both originals.

The builder reuses packs only after it checks every cached output hash.
Manifest, converter, toolchain or dependency changes produce a new cache key.
`BUNDLE.JSON` records these inputs and the hashes of all converter outputs.
The generated bundle and cache remain under `target/` and outside Git.

For another TTF/OTF, use `--manifest <file>` to select one font per bundle.
Paths are relative to the manifest. Record the actual license name and text.
The current firmware needs all nine sizes in the default manifest.
No runtime font selector is required. Validate coverage and layout for each font.
Non-OFL licenses use `LICENSE.TXT`; the legacy converter default remains OFL.

Converter convention version is 1. The converter uses fontdue rasterization
and the documented 1-bit threshold. Existing tests check identical outputs.

Recreate ReaderApp observations from the generated install set:

```sh
CARGO_NET_OFFLINE=true cargo run -p pulp-host --bin iansui-acceptance \
  --target aarch64-apple-darwin \
  --config 'unstable.build-std=["std","panic_unwind"]' -- \
  target/accept-iansui/snapshots target/cjk-sd/_PULP/FONTS
python3 scripts/pbm-to-png.py target/accept-iansui/snapshots
```

The exporter prints separate logical preparation operation/byte counts and
full/stitched draw counts. It requires equal pixels and zero draw reads.

Safely eject the card, insert it into the device and reopen the book. Test all
five size settings with Chinese headings, body, title and TOC labels. A missing
optional pack produces size-specific hollow boxes. Install/reopen restores
supported glyphs. Corrupt packs and actual I/O errors must show a recoverable
error; replace the pack or restore storage and reopen. Do not edit font IDs:
regenerated changed banks need changed identity for pagination invalidation.

Each pack contains 12,665 Unicode scalars. The observation fixture uses 26
unique non-whitespace scalars; 25 are covered. `𪚥` (U+2A6A5) is absent and
renders a hollow box. This font does not cover all Unicode. Host snapshots
under `target/accept-iansui/snapshots/` show the expected visible observation.
The two heading occurrences come from the chapter title and explicit Heading
block in the fixture. Full and stitched files are 480×800 and identical.

Host read counts are logical VirtualStorage adapter calls and requested/returned
bytes. They are not physical SD transactions or device latency measurements.
Hardware validation remains necessary for SD timing, live PSRAM/internal heap
usage, display update and scheduler behavior. C61 bitmap budget is 224 KiB within
its 256 KiB class; metadata is about 80 KiB persistent / 96 KiB transient and
unmeasured. The degraded 16 KiB class can reject dense CJK pages recoverably.
X4 BigBuf has no class budget. Host success does not prove live heap sufficiency.

Validate the artifact manifest from its directory (paths are relative):

```sh
(cd target/accept-iansui && shasum -a 256 -c SHA256SUMS)
```
