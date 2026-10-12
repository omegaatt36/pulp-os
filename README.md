# pulp-os

A bare-metal, async e-reader operating system for the OnePage C61, a RISC-V e-paper device.

Read EPUB and TXT books with crisp typography and zero fluff. Written in pure Rust (`no_std`), featuring no-framebuffer strip rendering, monomorphized static dispatch, class-budgeted memory hierarchy, and an async runtime powered by Embassy on `esp-rtos`.

---

## Supported Hardware

| | OnePage C61 |
|---|---|
| **MCU** | ESP32-C61 (RV32IMAC @ 160 MHz) |
| **Internal RAM** | ~314 KB HP SRAM (~158 KB heap) |
| **PSRAM** | 2–8 MiB SPI PSRAM @ 40 MHz |
| **Flash** | 16 MB SPI Flash (DIO @ 40 MHz) |
| **Display** | 800×480 SSD1677 Mono EPD |
| **Storage** | MicroSD via SPI (FAT32, PSRAM cached) |
| **Input** | Front ADC ladder + 3 side keys |
| **Power / Battery** | Li-ion ADC (GPIO5) + USB detect (GPIO11) |

The XTEink X4 (ESP32-C3) port was removed; the last commit that builds it is tagged `last-x4`.

---

## Quick Start

### Prerequisites

Ensure you have Rust nightly pinned via [rust-toolchain.toml](file:///home/raiven/dev/pulp-os/rust-toolchain.toml) and [`espflash`](https://github.com/esp-rs/espflash) >= 4.4.0 installed:

```sh
cargo install espflash --version ">=4.4.0"
```

### Build & Run

Core targets are configured in `.cargo/config.toml` with `partial-refresh` and `wifi` enabled by default:

```sh
# ESP32-C61 (defaults to partial-refresh + wifi)
cargo build-c61               # Build release firmware
cargo run-c61                 # Flash and monitor via scripts/run-c61.sh
cargo run-c61-boot            # Hardware bring-up diagnostic image

# Offline or custom builds (pass explicit features)
cargo build --release --target riscv32imac-unknown-none-elf
```

### Tests & Verification

```sh
cargo test-all                # Run all host workspace tests
cargo test-board              # Fast test: HAL-free board logic
cargo test-host               # Fast test: Reader layout, CJK & storage
task acceptance               # Full CI pipeline (multi-image build, verify, fmt)
```

---

## Controls

| Action | Key |
|---|---|
| **Next / Prev Page** | Side Next / Side Prev Keys |
| **Open Book / Select** | Front Enter |
| **Quick Menu** | Enter (Long-Press in Reader) |
| **File Browser / Home** | Front Back |
| **Power / Sleep** | Key Wake / Power |

---

## Architecture Highlights

- **Kernel / App Split**: Clean abstraction separation. The kernel provides drivers, async scheduling, storage, bookmarks, and font rasterization. Apps never touch raw hardware.
- **Strip Rendering**: Streams 12 × 40-row display strips (4 KB each) directly over SPI DMA instead of allocating a 48 KB full framebuffer.
- **Zero Dynamic Dispatch**: All app dispatch and lifecycle transitions are monomorphized via macros (`with_app!`), eliminating vtables and dynamic allocation overhead.
- **Class-Budgeted Memory**: Explicit pool budgeting for PSRAM and internal SRAM to guarantee that ISRs, DMA descriptors, and task stacks never touch external memory.
- **Shared SPI Bus Arbitration**: Display and SD card share SPI2 without bus collisions using scoped critical sections.

---

## Documentation

- [OnePage ESP32-C61 Memory Hierarchy](docs/c61-memory-hierarchy.md) – Detailed architectural breakdown of SRAM, PSRAM, Flash, and SD Card storage tiers.

---

## Acknowledgements

Forked from [hansmrtn/pulp-os](https://github.com/hansmrtn/pulp-os) by Hans Martin, replacing its XTEink X4 (ESP32-C3) board support with OnePage (ESP32-C61) support, and adding CJK typography rendering and memory hierarchy optimizations.

---

## License

[MIT](LICENSE)
