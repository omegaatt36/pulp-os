pulp-os -- e-reader firmware for the MoveCall OnePage (ESP32-C61)

bare-metal e-reader operating system for the MoveCall OnePage board
(ESP32-C61 + SSD1677 e-paper). written in Rust. no std, no
framebuffer, no dyn dispatch. async runtime via Embassy on
esp-rtos.

previously targeted the XTEink X4 (ESP32-C3); see "porting to the
OnePage C61" below for what the move changed.

hardware
    mcu         ESP32-C61, single-core RISC-V RV32IMAC
    ram         322 KB usable DRAM; ~80 KB heap (48 KB main + 32 KB reclaimed)
    psram       2 MB present, NOT used yet (see porting notes)
    display     800x480 SSD1677 mono e-paper, DMA-backed SPI, portrait
    storage     microSD over shared SPI bus (400 kHz probe, 20 MHz run)
    input       1 ADC ladder (GPIO4) + 3 discrete keys (GPIO2/6/9)
    battery     li-ion via ADC, 5.1M/5.1M divider on GPIO5

    pin map:
      GPIO2   KEY_WAKE / power      GPIO25  EPD CS
      GPIO4   front-key ladder ADC  GPIO26  SD CS
      GPIO5   battery ADC           GPIO27  EPD RST + SD/MIC power gate
      GPIO6   KEY_PREV              GPIO28  SD card detect
      GPIO8   EPD DC                GPIO29  EPD BUSY (HIGH = busy)
      GPIO9   KEY_NEXT              GPIO10  charge enable
      GPIO11  USB detect            GPIO22  SPI SCLK
      GPIO0/1 32.768 kHz XTAL (unused; ESP_HAL_CONFIG_USE_XTAL32K off)
      GPIO12/13 USB DM/DP (unused)

    EPD and SD share SPI2, arbitrated by CriticalSectionDevice.
    MISO (GPIO24) is wired for SD only, but it must stay connected:
    detaching it breaks every SD read.

    the front-key ladder is 4 keys on one node (BACK 2400-2800 mV,
    LEFT 1780-2140, RIGHT 1140-1500, ENTER 0-250; idle ~3100 mV). the
    node reads ~0 mV while the ADC settles at boot, which is
    indistinguishable from ENTER, so the ladder is ignored for the
    first 2.5 s.

porting to the OnePage C61 (in progress)
    target: the MoveCall OnePage e-reader, ESP32-C61HR2. Same panel
    (SSD1677 800x480), so the display driver itself barely changed --
    only the pins around it, and the BUSY polarity. The C61 has 2 MB
    PSRAM and 30 GPIOs; the X4's C3 has none and 22.

    pin map:
      GPIO22  SPI SCK (shared)     GPIO4   front-key ladder (ADC1_CH2)
      GPIO23  SPI MOSI (shared)    GPIO5   battery sense (ADC1_CH3, x2)
      GPIO24  SPI MISO (SD only)   GPIO10  charge enable (low = pause)
      GPIO25  EPD CS               GPIO11  USB detect (low = present)
      GPIO8   EPD DC               GPIO2   KEY_WAKE (deep-sleep wake)
      GPIO27  EPD RST + SD/MIC     GPIO6   KEY_PREV
          power gate               GPIO9   KEY_NEXT
      GPIO29  EPD BUSY (high)      GPIO28  SD card detect (low = inserted)

    GPIO27 is the sharp edge: it is the EPD reset line AND the power gate
    for the SD and mic. Its hardware reset pulse must happen once, early,
    before the card is first mounted -- pulsing it later browns out an
    already-mounted card and latches it into a state that does not
    recover. After boot the driver uses the controller's soft reset
    (0x12) instead.

    wake pin: RESOLVED -- GPIO2 CAN wake the chip from deep sleep.
    This was the open question, and it is answered. esp-hal 1.2.2
    GPIO2. esp-hal 1.2.2
    defines the C61's low-power pads in
    esp-metadata-generated-0.5.3/src/_generated_esp32c61.rs, in
    `for_each_lp_function!`: the LP pads are exactly GPIO0..GPIO6,
    with LP_GPIO2 present. esp-hal's
    src/gpio/lp_io/low_level/v4.rs then implements `LpPin` for
    GPIO2 with lp_number() == 2. IDF's own soc_caps.h for the C61
    agrees (components/soc/esp32c61/include/soc/soc_caps.h):

        // GPIO0~6 on ESP32C61 can support chip HP peripheral
        // powerdown-ed sleep wakeup
        #define SOC_GPIO_SUPPORT_HP_PERIPH_PD_SLEEP_WAKEUP   (1)
        #define SOC_GPIO_HP_PERIPH_PD_SLEEP_WAKEABLE_MASK \
            (0ULL | BIT0 | BIT1 | BIT2 | BIT3 | BIT4 | BIT5 | BIT6)
        #define SOC_RTCIO_PIN_COUNT                 7

    The same header also settles the pad count, which is worth writing
    down because a docs page and soc_caps.h disagree: soc_caps.h gives
    SOC_GPIO_PIN_COUNT 30 and SOC_GPIO_IN_RANGE_MAX 29, so all 30 pads
    are general-purpose digital I/O and the pin map below is sound.

    So the "LIGHT SLEEP ONLY" note in the changelog refers to the
    ordinary GPIO interrupt path only; the LP path is what deep sleep
    uses, and the C61 has one. Deep sleep is therefore viable, and
    the firmware arms GPIO2 with
    `apply_wakeup_config(&WakeupConfig::default().with_low_power_path(true))`
    before `LowPower::sleep_deep`. No board change is needed, and
    nothing has to be reordered before hardware is ordered.

    Still open: the deep-sleep current figure. The X4's ~3 uA below
    came from physically cutting power; the C61 stays in deep sleep
    instead, and that path has not been measured. Treat the C61
    figure as UNMEASURED until it is measured on the board.

    session persistence across sleep DOES NOT WORK on the C61, and
    the firmware no longer pretends otherwise. The C3 had 8 KB of
    RTC FAST memory, and the old code put the session struct in
    `.rtc_fast.persistent`. The C61's esp-hal linker script
    (ld/esp32c61/memory.x) defines only RAM, ROM and dram2_seg --
    there is no RTC region at all, unlike the C3 and C6. A static
    with that link_section is therefore placed by the linker in
    ordinary DRAM, where a deep-sleep wake (a full reset, powering
    down every memory group) does not preserve it. The attribute has
    been removed and restore comes from the SD card instead. See the
    header of kernel/src/kernel/rtc_session.rs.

    version floor: esp-hal 1.1.0 does list the C61, but only half of it.
    1.1.0 brought up GPIO, SPI, DMA, PSRAM, UART, I2C, RNG and SHA/ECC;
    ADC (#6130) and sleep (#5800) only arrived in 1.2.0. This firmware
    reads both its front-key ladder and its battery voltage through ADC1,
    so 1.1.0 leaves it with no input device at all. 1.2.2 is the floor.

    esp-rom-sys must be 0.1.5, not the 0.1.4 that resolves by
    default. 0.1.4 does not declare `esp_rom_spiflash_write_encrypted`
    in its Rust bindings, which esp-storage 0.10.0 calls unconditionally,
    so the C61 build fails to compile. 0.1.5 declares it (the symbol is
    at 0x40000124 in the C61 ROM). That in turn sets the MSRV at 1.95,
    which is why the workspace rust-version moved from 1.88.

    PSRAM is present but unused. The C61's stock esp-hal linker script
    has no `extmem` region, so PSRAM statics cannot be placed
    statically at all; wiring that up is a later phase, not part of
    this port.

    Wi-Fi was deliberately removed from this project. It was the
    thing blocking the port: the C61 is a different radio subsystem
    and nothing in an offline e-reader needs it. So `esp-radio` and
    `embassy-net` are NOT dependencies and must not be added back
    without a reason. There is no network stack in this firmware.

building
    requires stable Rust >= 1.95 and the riscv32imac-unknown-none-elf
    target. rust-toolchain.toml handles both automatically. (the
    firmware build itself needs nightly, because `cargo fw` passes
    -Zbuild-std; run cargo as RUSTUP_TOOLCHAIN=nightly.)

        cargo build --release
        espflash flash --monitor --chip esp32c61 target/...

    NOTE: the espflash in the dev environment is 4.3.0 and does not
    know the C61 (needs >= 4.5). Building is unaffected; only
    flashing is.

    or:

        cargo run --release

    host tests (no hardware, no Python, no untracked files):

        cargo host-test

    an alias for `cargo test -p pulp-render -p pulp-fontpack
    --target host-tuple`, defined in .cargo/config.toml. it runs the
    shared text/render core in render/ (the code the firmware uses)
    and the font pack converter in tools/fontpack/. it does not compile
    the reader app or board code; check those with `cargo build --release`.
    the host suite reports pass
    or fail for:
      font lookup       pack loading and every §6 load error, glyph
                        lookup, the visible fallback for absent glyphs
      pagination        line wrapping with the CJK line-break rules,
                        page splits with nothing lost or repeated
      rendered pixels   page glyphs drawn through the strip renderer
                        with no font reads during the draw
      converter         pack writer, rasteriser, self-verification,
                        CLI, the tracked fixture staying reproducible
    render/tests/iansui_acceptance.rs runs all three core stages end
    to end on a real CJK face: a tracked, OFL-licensed Iansui subset
    (tools/fontpack/tests/fixtures/README.txt says how it was made).
    render/tests/iansui_golden.rs compares two pages of it, drawn
    through the strip renderer, byte for byte with the reviewed
    golden frames in render/tests/golden/ (PBM), and checks that
    partial-window passes match the full-frame pass inside each
    window. every run writes upright PNG copies of the goldens to
    target/<host triple>/tmp/golden-review/; on a mismatch it also
    writes the actual render and a diff there and names the files.
    goldens change only on request, after reviewing the new render:

        PULP_UPDATE_GOLDEN=1 cargo host-test

    git dependencies (fetched by cargo):
      embedded-sdmmc    async FAT filesystem over SD/SPI (fork)
      smol-epub         no_std epub/zip/html/image processing

CJK font pack (Iansui)
    pulp-fontpack converts a TTF into a 1-bit font pack (.PFP, format
    in docs/font-pack.txt). it packs every code point of the font,
    prints Unicode coverage by Han / kana / Hangul block, and loads
    the written pack with the device loader (pulp-render) and checks
    every glyph round-trips before anything is written. it stages the
    pack, license, and manifest together; reported conversion or
    publication errors leave previous outputs unchanged. a forced kill
    during the three-file publication can leave a partial output set;
    rerun the converter before copying files to SD.
    on Unix hosts a conversion locks the output directory while it
    publishes the three files. the OS releases the lock when the
    process exits, including after a forced kill.

    with Iansui-Regular.ttf and its OFL.txt in the repository root
    (neither is tracked; get them from
    https://github.com/ButTaiwan/iansui):

        cargo run -p pulp-fontpack --target host-tuple --release -- \
            --ttf Iansui-Regular.ttf --license OFL.txt \
            --size 24 --family IANSUI --out out/FONTS/ \
            --expect-sha256 7f1aa62e9dcbf40d0ce41a5d3f1e5ea602e66c295778ac6fefb6b84d8ed08bd5

    --expect-sha256 is the optional full check: it fails before
    writing anything if the TTF is not the supplied Iansui build.
    output in out/FONTS/ (gitignored): IANSUI24.PFP (about 1 MB,
    12,666 glyphs), IANSUOFL.TXT (the license, byte-identical) and
    IANSUI24.TXT (build manifest: source and pack SHA-256, coverage,
    self-verify result). --size takes 8..96; each size is its own
    file.

    SD card: copy IANSUI24.PFP and IANSUOFL.TXT to /FONTS/ at the
    card root, i.e. /FONTS/IANSUI24.PFP and /FONTS/IANSUOFL.TXT
    (docs/font-pack.txt §7). the firmware does not open packs yet;
    the loader it will use is render/src/font_pack.rs.

features
    txt reader      lazy page-indexed, read-ahead prefetch,
                    proportional font wrapping
                    up to 512 pages per index; overflow reports an error
    epub reader     ZIP/OPF/HTML-strip pipeline, chapter cache on SD,
                    proportional fonts with bold/italic/heading styles,
                    inline PNG/JPEG (1-bit Floyd-Steinberg dithered),
                    TOC browser (NCX or inline), chapter navigation
    file browser    paginated SD listing, background EPUB title
                    scanner (resolves titles from OPF metadata)
    bookmarks       16-slot LRU in RAM, flushed to SD every 30 s;
                    home screen bookmarks browser sorted by recency
    fonts           regular/bold/italic TTFs rasterised at build time
                    via fontdue; five sizes, book and UI independently
                    configurable
    display         partial DU refresh (~400 ms page turn), periodic
                    full GC refresh (configurable interval)
    quick menu      per-app actions + screen refresh + go home,
                    triggered by the WAKE/power key
    settings        sleep timeout, ghost clear interval,
                    book font size, UI font size
    sleep           idle timeout + power long-press; EPD deep sleep
                    + ESP32-C61 deep sleep, woken by a level-low on
                    GPIO2 via the LP path. The C3-era current figures
                    do not carry over: the X4 cut power outright, this
                    board does not. UNMEASURED on the C61.

controls
    Prev / Next         scroll or turn page
    PrevJump / NextJump page skip (files: full page; reader: chapter)
    Select              open item
    Back                go back; long-press goes home
    Power (short)       open quick-action menu
    Power (long)        deep sleep

runtime
    embassy async executor on esp-rtos. five concurrent tasks:

    main            event loop: input dispatch, app work, rendering
    input_task      10 ms ADC poll, debounce, battery read (30 s)
    housekeeping    status bar (5 s), SD check (30 s), bookmark flush (30 s)
    idle_timeout    configurable idle timer, signals deep sleep
    worker_task     background CPU-heavy work (HTML strip, image decode)

    CPU sleeps (WFI) whenever all tasks are waiting.

directory layout
    kernel/                 pulp-kernel workspace crate (zero app imports)
      src/
        lib.rs              crate root, re-exports
        kernel/
          mod.rs            Kernel struct, resource ownership
          app.rs            App trait, AppLayer trait, AppIdType,
                            Transition, Redraw, AppContext, Launcher,
                            QuickAction protocol types
          console.rs        boot console (FONT_6X13, no fontdue)
          scheduler.rs      main loop, render pipeline, sleep
          handle.rs         KernelHandle (app I/O API)
          tasks.rs          spawned embassy tasks
          work_queue.rs     background work with generation cancellation
          bookmarks.rs      LRU bookmark cache
          config.rs         settings parser/writer
          dir_cache.rs      sorted directory cache with title resolution
          wake.rs           uptime helper (embassy monotonic clock)
        board/              board support (pin map, SPI wiring, button layout)
          mod.rs            Board::init, peripheral splitting
          action.rs         ActionEvent (semantic button actions)
          battery.rs        voltage-to-percentage mapping
          button.rs         physical button enum, ButtonMapper
          layout.rs         button-to-action table
          raw_gpio.rs       register-level GPIO for SD CS
        drivers/            hardware drivers
          mod.rs            driver re-exports
          ssd1677.rs        EPD display driver, 3-phase partial refresh
          strip.rs          4 KB strip buffer, rotation, glyph blitting
          sdcard.rs         SD card init and SPI wiring
          storage.rs        FAT filesystem ops, poll_once, with_fs! macros
          input.rs          ADC button polling, debounce, repeat
          battery.rs        ADC battery voltage sampling
        ui/                 font-independent primitives
          mod.rs            Region, Alignment, stack measurement
          stack_fmt.rs      no_alloc formatting (StackFmt)
          statusbar.rs      status bar rendering
          widget.rs         widget trait and helpers

    src/                    distro / app layer
      bin/main.rs           entry point, hardware init, boot
      lib.rs                crate root
      ui/
        mod.rs              app-side UI helpers
      fonts/
        mod.rs              font size tiers, FontSet lookups
        bitmap.rs           build-time bitmap font data
      apps/
        mod.rs              AppId enum, type aliases binding kernel generics
        manager.rs          AppLayer impl, with_app! dispatch, lifecycle
        home.rs             launcher menu + bookmarks browser
        files.rs            SD file browser + background title scanner
        settings.rs         settings UI
        reader/
          mod.rs            state machine, lifecycle, draw, quick actions
          paging.rs         text wrapping, page navigation, load/prefetch
          epub_pipeline.rs  ZIP/OPF parsing, chapter caching, background strip
          images.rs         image detection, decode dispatch, dithering
        widgets/
          mod.rs            widget re-exports
          bitmap_label.rs   proportional text label (uses fonts/)
          quick_menu.rs     power-button overlay menu
          button_feedback.rs  button press visual feedback

    build.rs                fontdue TTF rasterisation at compile time
    assets/fonts/           TTF files (regular, bold, italic)

design notes
    kernel / app split. the kernel crate (kernel/) has zero imports
    from apps/ or fonts/. the scheduler is generic over AppLayer;
    it never names a concrete app. AppId is defined by the distro,
    not the kernel -- the kernel only knows AppIdType::HOME.

    no dyn dispatch. with_app!() macro matches AppId, expands to
    concrete calls per app struct. all monomorphised; no vtable,
    no Box.

    strip rendering. 12 x 40-row strips (4 KB each) instead of a
    48 KB framebuffer. draw callback fires per strip during SPI
    transfer. blit_1bpp_270 fast path walks physical memory linearly
    for the portrait rotation. windowed mode for partial refresh.

    3-phase partial refresh. write BW RAM, kick DU waveform, collect
    input during ~400 ms refresh, then sync RED RAM. phase3 skipped
    during rapid navigation (RED marked stale; next partial uses
    inv_red recovery). full GC promoted after configurable number
    of partials to clear ghosting.

    SPI bus sharing. EPD and SD share one SPI2 bus. all SD I/O
    completes before any EPD render pass. busy_wait_with_input()
    collects only input events, no background work. violating the
    ordering panics (RefCell double-borrow), never corrupts.

    poll_once. embedded-sdmmc's async API wraps blocking SPI+DMA
    that never pends. poll_once drives every future to completion
    in a single poll, avoiding task spawn overhead.

    KernelHandle. apps never touch hardware. KernelHandle borrows
    the Kernel for one lifecycle method and exposes file I/O, dir
    cache, bookmarks. every async method does sync work then
    yield_now() for executor fairness.

    smol-epub sync bridge. smol-epub I/O uses closures, not async.
    with_sync_reader() provides a scoped closure that completes
    all storage access before returning -- no borrows across await.

    heavy statics. large structs (ReaderApp ~28 KB, DirCache ~10 KB,
    StripBuffer ~4 KB) live in ConstStaticCell / StaticCell so the
    async future stays ~200 B.

    nav stack. Launcher<Id> holds a 4-deep stack. transitions
    (Push/Pop/Replace/Home) drive on_suspend / on_enter / on_resume
    lifecycle. Push degrades to Replace when stack is full.

    dirty-region tracking. apps call ctx.mark_dirty(region); regions
    are unioned per frame. partial DU or full GC issued accordingly.

    work queue. dedicated embassy task for CPU-heavy work (HTML strip,
    image decode). generation-based cancellation: bump a counter and
    drain channels; worker checks generation before and after
    processing. channel capacity 1 for back-pressure.

    input. ADC ladders at 100 Hz, 4-sample oversampling, 15 ms
    debounce, 1 s long-press, 150 ms repeat. ButtonMapper translates
    physical buttons to semantic actions. apps never see hardware.

    fonts. build.rs rasterises TTFs via fontdue into 1-bit bitmaps
    at five sizes (xsmall through xlarge), three styles (regular,
    bold, italic). ASCII direct-indexed, extended unicode binary-
    searched. book and UI sizes independently hot-swappable.

    boot console. kernel renders text during hardware init using
    built-in FONT_6X13 mono font. works with zero fontdue, zero
    TTFs. if the SD card is missing, user still sees boot progress.

    bookmarks. 16-slot LRU, RAM-resident, binary format on SD.
    flushed every 30 s if dirty, plus on sleep. lookup by fnv1a
    hash + case-insensitive name comparison.

    settings. key=value text in _PULP/SETTINGS.TXT. parsed at boot,
    saved on change. font size changes propagate to all apps.

    memory budget. ~172 KB heap for epub text and image decode
    (alloc::vec). everything else is static or stack. ~56 KB stack,
    painted 0xDEAD_BEEF at boot, high-water mark logged every 5 s.

    forkable kernel. designed to be extracted as a standalone crate.
    a fork defines its own AppId, implements AppLayer, brings its
    own fonts and apps, writes a main.rs. the kernel provides
    drivers, scheduling, storage, bookmarks, config, and a working
    EPD with mono boot console.

license
    MIT
