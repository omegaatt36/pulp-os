# pulp-os

A bare-metal, async e-reader operating system for RISC-V e-paper devices.

Read EPUB and TXT books with crisp typography and zero fluff. Written in pure Rust (`no_std`), featuring no-framebuffer strip rendering, monomorphized static dispatch, class-budgeted memory hierarchy, and an async runtime powered by Embassy on `esp-rtos`.

---

## Supported Hardware

| Hardware | XTEink X4 | OnePage C61 |
|---|---|---|
| **MCU** | ESP32-C3 (RV32IMC @ 160 MHz) | ESP32-C61 (RV32IMAC @ 160 MHz) |
| **Internal RAM** | 400 KB DRAM (~172 KB heap) | ~314 KB HP SRAM (~158 KB heap) |
| **PSRAM** | None | 2–8 MiB SPI PSRAM @ 40 MHz |
| **Flash** | 4 MB SPI Flash | 16 MB SPI Flash (DIO @ 40 MHz) |
| **Display** | 800×480 SSD1677 Mono EPD | 800×480 SSD1677 Mono EPD |
| **Storage** | MicroSD via SPI (FAT32) | MicroSD via SPI (FAT32, PSRAM cached) |
| **Input** | 2 ADC ladders + Power IRQ | Front ADC ladder + 3 side keys |
| **Power / Battery** | Li-ion ADC (GPIO0) | Li-ion ADC (GPIO5) + USB detect (GPIO11) |

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
# OnePage C61 (ESP32-C61, defaults to partial-refresh + wifi)
cargo build-c61               # Build release firmware
cargo run-c61                 # Flash and monitor via scripts/run-c61.sh
cargo run-c61-boot            # Hardware bring-up diagnostic image

# XTEink X4 (ESP32-C3, defaults to wifi)
cargo build-x4                # Build release firmware
cargo run-x4                  # Flash and monitor via espflash

# Offline or custom builds (pass explicit features)
cargo build --release --target riscv32imac-unknown-none-elf --features board-onepage-c61
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

| Action | XTEink X4 | OnePage C61 |
|---|---|---|
| **Next / Prev Page** | Right / Left Buttons | Side Next / Side Prev Keys |
| **Open Book / Select** | Center / Confirm | Front Confirm / Enter |
| **Quick Menu** | Power Short-Press | Enter (Long-Press in Reader) |
| **File Browser / Home** | Back / Menu | Front Back |
| **Power / Sleep** | Power Button | Key Wake / Power |

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

Forked from [hansmrtn/pulp-os](https://github.com/hansmrtn/pulp-os) by Hans Martin, extending it with OnePage (ESP32-C61) board support, CJK typography rendering, and memory hierarchy optimizations.

---

## License

[MIT](LICENSE)
