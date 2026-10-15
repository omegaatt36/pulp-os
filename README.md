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

Ensure you have Rust nightly pinned via [rust-toolchain.toml](rust-toolchain.toml) and [`espflash`](https://github.com/esp-rs/espflash) >= 4.4.0 installed:

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

# Vertical writing on the device
cargo device-fixtures                      # VERTRUBY.EPU into target/device-fixtures
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

## Reading Progress

Reading positions are kept for the 16 most recently saved books in `_PULP/BKMK.BIN`.
Saving a book again moves it to the front of recent reading lists. When all slots are occupied, the oldest position is replaced.
The ordering survives a reboot and generation-counter exhaustion without changing the bookmark file format.

File writes, appends, and positioned writes report success only after the file closes successfully, including FAT metadata flush.
If writing or seeking fails, that original error is preserved even if closing also fails.

---

## Vertical Writing

An EPUB whose spine declares `page-progression-direction="rtl"` and whose `dc:language` is Chinese, Japanese or Korean is set in vertical columns: top to bottom, columns right to left. Everything else stays horizontal.

- **CJK ideographs, kana, fullwidth forms:** upright, one em per step down the column.
- **Brackets, dashes, ellipsis, stops:** the pack's vertical presentation form (`U+FE10..FE48`) when it has one. Otherwise brackets and dashes are the horizontal glyph turned a quarter clockwise; `，、。：；` stay upright.
- **Latin text and digits:** turned a quarter clockwise, on their own advance.
- **Images:** an inline image takes whole columns, up to 60% of the text width and as tall as the text area. It never straddles a page edge and a page holds one: an image that does not fit, or follows another, starts the next page.
- **Not yet:** ruby, tate-chu-yoko, emphasis marks.

Render pages from the real reader without hardware:

```sh
cargo run -p pulp-host --bin vertical-preview --target host-tuple \
  --config 'unstable.build-std=["std","test"]' -- OUT_DIR PACK_DIR [FONT_IDX [PAGES]]
```

`PACK_DIR` holds the `F000nn.PFN` packs from `pulp-fontconv`; `pulp-fontconv pbm-to-png OUT_DIR` converts the PBMs to PNG.

---

## EPUB Streaming Limits

Text chapters use bounded streaming rather than the 192 KiB in-memory extraction limit:

| Resource | Limit |
|---|---|
| Compressed chapter | 16 MiB |
| Decompressed chapter | 8 MiB |
| Stripped text per chapter | 8 MiB |
| Chapters in the text cache | 256 |
| Book text-cache file, including header and chapter table | 64 MiB |
| Streaming time per chapter | 120 seconds |

Stored entries must declare matching compressed and decompressed sizes.
DEFLATE streams must produce exactly their declared decompressed size; excess output is rejected before it reaches the text stripper.
An interrupted or failed chapter is never marked cached, and incomplete caches are regenerated when the book is reopened.

---

## Wi-Fi Upload Authorization

Each upload session creates a new 128-bit token after Wi-Fi starts.
The device shows the token in two lines and includes it in the QR URL fragment.

1. Enter Upload mode on the device.
2. Scan the QR code, or open `http://pulp.local/` and enter both token lines in the page.
3. If the page rejects the token, enter the current token from the device.

The browser sends `X-Pulp-Token` with `/files`, `/upload`, `/delete`, and `/time` requests.
These endpoints reject missing, incorrect, or duplicate token headers with HTTP 403.
The token expires when Upload mode ends. HTTP does not encrypt the token or file contents.

Uploads never replace an existing FAT short name. A name collision returns HTTP 409 and preserves the existing file.
To replace a file, delete it explicitly first. Different original names can map to the same 8.3 name.

A completed upload flushes and closes the new file before it reports success.
An interrupted upload closes and deletes the incomplete file.
An SD failure can prevent cleanup. A power loss can also leave an incomplete new file on the card.
An existing file remains protected by the create-only upload policy.

The server limits each read, write, and flush wait to 30 seconds.
The total request limit is 600 seconds after the server accepts the connection.

The browser serializes time calibration, listing, uploads, and deletion through one request queue.
It waits 100 ms between requests so the single-connection server can finish closing the previous connection.
It refreshes the file list after a batch finishes. List and time requests wait up to 5 seconds, deletion up to 35 seconds, and an upload up to 610 seconds.
Only failed file-list reads are retried automatically. Reconnecting cancels work from the previous browser session, and late callbacks from that session are ignored.

---

## Local Date and FAT Timestamps

The device has no battery-backed calendar clock. The firmware records the source of its local date:

- **Unknown:** No usable date exists. The date API returns `None`, and FAT writes use the 1980-01-01 epoch.
- **FatMtime:** The latest valid modification time in the SD root directory supplies an estimate. Older timestamps cannot move this estimate backward.
- **Browser:** The Wi-Fi upload page sets the local date from the browser clock before file uploads start.

The firmware advances the date with its monotonic uptime clock. New FAT files and file modifications use this date, with two-second FAT precision.
FAT timestamps contain no timezone. Browser calibration uses local time, and its accuracy depends on the computer clock.

Open the Wi-Fi upload page to set the date. The page reports whether calibration succeeds.
The calibration endpoint requires the session token.
It accepts `POST /time` with a 19-byte body in `YYYY-MM-DDTHH:MM:SS` format and `Content-Length: 19`.
Invalid dates return HTTP 400 and preserve the current date.

After a reset or deep sleep, browser calibration is lost. The next SD mount supplies an estimate again.
SD metadata cannot establish today's date or account for elapsed time while the device is off.
Apps can read the date and its source through `KernelHandle::current_datetime()`.

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
