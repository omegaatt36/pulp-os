# Iansui SD installation and validation

Generated acceptance set: `target/accept-iansui/packs/`. Copy all nine
`F00016.PFN`, `F00019.PFN`, `F00023.PFN`, `F00027.PFN`, `F00028.PFN`,
`F00032.PFN`, `F00035.PFN`, `F00038.PFN`, `F00046.PFN`, plus `PROV.TXT`,
`OFL.TXT` and `COVERAGE.TXT`, into `_PULP/FONTS/` at the SD card root.
The nine pack files total 14,792,301 bytes. Keep the provenance and license.
No removable device was written during acceptance.

Recreate in an empty output directory from the local original source files:

```sh
CARGO_NET_OFFLINE=true cargo run -p pulp-fontconv --target aarch64-apple-darwin \
  --config 'unstable.build-std=["std","panic_unwind"]' -- \
  --font Iansui-Regular.ttf --license OFL.txt \
  --upstream-url https://github.com/ButTaiwan/iansui --out target/iansui-install
```

Use your host triple instead of `aarch64-apple-darwin` on another system.
Check source/pack hashes against `target/accept-iansui/packs/PROV.TXT` and
`target/accept-iansui/pack-manifest.json`. Converter convention version is 1.
The converter uses fontdue rasterization and the documented 1-bit threshold;
its existing reproducibility tests check identical outputs.

Recreate ReaderApp observations from the generated install set:

```sh
CARGO_NET_OFFLINE=true cargo run -p pulp-host --bin iansui-acceptance \
  --target aarch64-apple-darwin \
  --config 'unstable.build-std=["std","panic_unwind"]' -- \
  target/accept-iansui/snapshots target/iansui-install
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
