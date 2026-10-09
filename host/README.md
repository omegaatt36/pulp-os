# Host reader validation

Run these commands from the pulp-os repository root. The pinned toolchain is
`nightly-2026-09-22`; install its `rust-src` component for Cargo build-std.
The wrappers derive rustc's host triple and override the workspace's embedded
build-std configuration with `std,test`. No hardware or external renderer is
required. Cargo dependencies must already be cached for offline execution;
`--locked` does not itself disable network access.

The workspace uses the vendored `vendor/smol-epub` dependency. Preserve the
font inputs in `assets/fonts`; host/build.rs calls the production font generator.
The verified host is aarch64-apple-darwin, rustc 1.100.0-nightly (1303417c4).

```bash
rustup component add rust-src --toolchain nightly-2026-09-22
cargo test-host --locked
cargo test-reader-regression
task acceptance        # everything software-side: builds, host tests, boundary and budget checks
```

The boundary checks (`harness/tests/host_boundary.rs`) verify the host
dependency/build graph; the firmware images they inspect come from `task build`.
Firmware compilation and memory budgets do not validate physical hardware.

## PBM previews

```bash
cargo host-preview -- --help
cargo host-preview -- --output-dir /tmp/pulp-preview-a
cargo host-preview -- --output-dir /tmp/pulp-preview-b
diff -r /tmp/pulp-preview-a /tmp/pulp-preview-b && ls /tmp/pulp-preview-a/*.pbm | wc -l   # eleven identical PBMs
cargo host-preview -- --output-dir /tmp/pulp-selected --fixture PLAINLF.TXT --page 1 --font 0 --theme 0
```

Use fresh output directories when counting artifacts; the CLI overwrites its
own deterministic filenames but does not delete other files. Without
`--fixture`, it renders all eleven synthetic standard books (five TXT and six
EPUB2/3 compression variants). `--help` lists canonical, case-sensitive names.
Defaults are page 0, font 2, theme 1. Font indices are 0–4 and theme indices
0–3, validated against production exports. `--page` is a zero-based ordinal
from the beginning of each book; EPUB navigation crosses chapters through
production `Action::Next`. Out-of-range values fail instead of clamping.

Artifacts are named `NAME-pageN-fontN-themeN.pbm`: binary P4, 480×800,
MSB-first monochrome pixels, 1 = black, with a 48000-byte raster and no
clock or metadata. Production `Rig::draw` renders the page after bounded
background completion; the full-band and firmware-strip results must match
before a file is written. Errors exit nonzero with diagnostics. An error after
one book succeeds can leave its PBM in the output directory.

Exact repeatability is verified for the same pinned toolchain, dependencies,
inputs and host target. Cross-platform compressor/font rasterization identity
has not been established.

## Storage and coverage

The preview uses generated `standard_card()` memory storage. Tests also exercise
`VirtualStorage::host_dir(root)`, where an existing host directory acts as an SD
root and survives drop. Both backends expose read counters/logs and deterministic
short-read/error injection. Host filesystem permissions, case handling and I/O
errors are not complete FAT/SD emulation.

The separate `reader-regression/` workspace (`cargo test-reader-regression`)
remains intentionally available: it checks the real reader sources against the
golden trace pinned from the pre-port X4 commit; its duplicated host shims have
not been consolidated.
