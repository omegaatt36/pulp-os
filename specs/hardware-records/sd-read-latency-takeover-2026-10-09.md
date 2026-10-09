# sd-read-latency takeover — software evidence and board flash

The prepared artifact was flashed at **2026-10-09 19:56:08 Asia/Taipei** after
the user requested flashing. USB serial `88:56:A6:CF:DA:90` matches the product
board. The device node was hidden inside the sandbox; host access exposed
`/dev/ttyACM0`. Only ota_0 was written after the runner's factory-layout
preflight; the bootloader and partition table were retained.

## Prepared artifact

| Field | Value |
| --- | --- |
| Source baseline | `5574ec95` plus the uncommitted takeover changes |
| Features | `board-onepage-c61,sd-metrics,partial-refresh`; offline |
| Build | `cargo build-c61-sd-metrics --locked --features partial-refresh --bin pulp-os-c61` |
| ELF | `target/sd-read-latency-takeover/pulp-os-c61` |
| ELF SHA-256 | `189d5d23d19d1f10f3cb5ec9045f28178227f38dc7a1b07e0acbada1a06f2748` |
| App image | `target/sd-read-latency-takeover/app.bin` |
| App SHA-256 | `4492d788d6a86430d0e7da9affc09732ad59621e2a86f34862c9d825b64b3536` |
| Image settings | ESP32-C61, DIO, flash 40 MHz / 16 MB; app-only image |
| Manifest | `target/sd-read-latency-takeover/manifest.json`, including modified source hashes |

The existing runner preserves the board bootloader and partition table and
writes ota_0 at `0x10000` after its read-only preflight. When a board is
available, the prepared ELF can be used with:

```sh
env -u PULP_C61_BOOTLOADER ESPFLASH_PORT=/dev/ttyACM0 \
  scripts/run-c61.sh target/sd-read-latency-takeover/pulp-os-c61
```

The command was executed with `/dev/ttyACM0`. The monitor was intentionally
stopped with SIGINT after successful flash and boot verification (runner exit
130); that exit does not indicate a write failure. The board remains running.

## Flash and boot evidence

[Serial capture](sd-read-latency-takeover-2026-10-09.log), 670 lines, ANSI
removed. The log reports successful write, ESP32-C61 rev v1.1, FAT32 mount,
SD clock raised to 10 MHz, PSRAM ready, `ui ready` and `kernel ready`. The
restored session opens `ASSEMB~1.EPU` chapter 3 and reaches Reader `Ready`
followed by a page refresh. Subsequent operations return to Files, open
`ZH1.TXT`, `ZH2.TXT`, and reopen `ASSEMB~1.EPU`; all four opening episodes reach
`Ready`. No panic appears in the capture. Visual glyph correctness and the
100 ms input bound are not established by these log events.

The following sums cover completed reader steps through the first `Ready`
after each opening. They exclude work before the first measured step,
completion-log emission and the final page refresh, so they are **not
stopwatch measurements of first-visible-page latency**. Known outside-step
intervals include display work/wait and cannot all be attributed to SD.

| Opening episode | Reader step wall sum | Known outside-step sum | Layout/metrics sum | Visible bitmap sum | Maximum staged scalars |
| --- | ---: | ---: | ---: | ---: | ---: |
| Restored `ASSEMB~1.EPU` | 4.221 s | 4.154 s | 2.193 s | 0.140 s | 751 |
| `ZH1.TXT` (43 bytes) | 0.540 s | 3.897 s | 0.306 s | 0.027 s | 14 |
| `ZH2.TXT` (1189 bytes) | 1.358 s | 4.686 s | 0.711 s | 0.150 s | 247 |
| Reopened `ASSEMB~1.EPU` | 3.138 s | 7.395 s | 1.513 s | 0.181 s | 431 |

The initial EPUB opening has 1,590 staging API reads / 754 lookups (including
its title), and ZH2 has 708 / 247. Median measured intervals outside reader
steps are 0.131 ms and 0.166 ms respectively, with occasional much larger
display intervals. These fields differ from historical font-call `gap_us`;
do not compare their numbers as identical intervals. R3's <=700-scalar target,
all font-size checks and repeat-cache semantics remain open.

## Software verification

- `task acceptance`: exit 0. Log: `target/sd-read-takeover-acceptance.log`.
- `cargo test-host --locked --features sd-metrics --test reader_metrics --test
  cjk_capacity --test cjk_slicing --lib`: exit 0, 18 tests. Log:
  `target/sd-read-takeover-metrics-tests.log`.
- Both C61 checks pass; 32-bit 24-byte slots and unchanged aggregate/PSRAM
  bounds are compile-checked. Profile strings are absent from the default ELF
  and present in the measurement ELF.
- Final independent quality/security reviews have no new findings. Literal
  session-wide R3 retention remains an acceptance blocker; see
  [change review](../changes/sd-read-latency/review.md).

## Pending board measurements

Record the card/board, runtime SD clock (target 10 MHz), installed bank IDs,
fixture contents/hashes and this artifact's hashes. Time from initiating open
to the first completed visible page as well as font staging/preparation;
include scheduler gaps and EPD time rather than summing font reads alone.

1. Cold `ZH1.TXT`, `ZH2.TXT`, a <=700-distinct-scalar preparation and a completed
   CJK EPUB chapter open; collect `cjk-sd` and `reader-sd` records.
2. Files entry/return with CJK book titles; record actual visible-list latency.
3. Key response during preparation and heartbeat cadence; the cold bank open
   and EPD loading refresh can block beyond the nominal glyph slice.
4. K2/K3/K7/K9: all font sizes, missing glyphs, heap/PSRAM use, and page/anchor
   consistency after font changes/reopen; retain human visual observations.
5. Repeat preparation/page return: record headers, index and bitmap reads
   separately under the cache contract ultimately agreed for R3.

New `gap_after_log_us` excludes prior font-log emission. `reader-sd` separates
text, layout, visible bitmaps, cooperative pause and instrumented logging from
outside-step work. First/cancelled outside intervals are unknown; idle ticks
emit no reader profiles. Historical `gap_us` has different semantics.
