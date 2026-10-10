# embedded-sdmmc (vendored)

Upstream: https://github.com/hansmrtn/embedded-sdmmc-rs, branch `async`.
Base revision: `0bf12548d1144b0e2b06b290acde3e4bb46cd91b`.
License: MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`).

Tests, examples, the changelog and `[dev-dependencies]` were dropped. The
copy is a path dependency excluded from the workspace (root `Cargo.toml`).

## Local patch

Both FAT32 directory walkers (`fat/volume.rs` and `fat/async_volume.rs`,
`iterate_dir`) only left the inner per-block entry loop on the
end-of-directory marker, or when the callback returned `Break`. The walk then
went on to read every remaining block of the cluster (64 blocks, ~32 ms on an
SPI SD card) for each directory or file open. The loop is now labelled
`'outer` and both exits leave the whole walk. The same two-hunk change is in
timcki/plump `vendor/embedded-sdmmc-rs`.
