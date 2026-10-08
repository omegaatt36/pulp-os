pulp-os -- e-reader firmware for the XTEink X4

bare-metal e-reader operating system for the XTEink X4 board
(ESP32-C3 + SSD1677 e-paper). written in Rust. no std, no
framebuffer, no dyn dispatch. async runtime via Embassy on
esp-rtos.

C61 memory diagnostics
    Large decoder scratch buffers use explicit memory class budgets.
    Font metadata and runtime state remain in internal RAM.
    Heap and stack logs include PSRAM allocation current, free and peak bytes.
    Reservation logs include per-class high-water values.
    Reservation peaks can include requests that the allocator later rejects.
    They are distinct from physical allocation peaks.
    esp-alloc 0.11 does not expose the largest free block through its public API.
    The firmware does not allocate probe blocks to estimate fragmentation.
    Hardware peak usage and latency remain unverified.

CJK font installation
    Build one selected font with the pinned source in fonts/cjk.json:

        cargo run -p pulp-fontconv --release -- bundle

    Copy target/cjk-sd/_PULP/FONTS to _PULP/FONTS on the SD card.
    Replace the previous FONTS directory to remove packs from the old font.
    Keep PROV.TXT, COVERAGE.TXT, BUNDLE.JSON and the license file.
    The firmware loads these SD packs. It has no font selection menu.

    The manifest selects one TTF/OTF, its license, upstream URL, source
    version and pixel sizes. Paths are relative to the manifest directory.
    Every source file has a pinned SHA256. No network download is required.
    The Iansui version is a source hash identifier, not an upstream release tag.

    To use another font, provide a manifest with that font and its actual
    license. Use --manifest <file> and --out <bundle-directory>.
    Keep all nine sizes for the current firmware: 16,19,23,27,28,32,35,38,46.
    Optional require_chars has path and sha256 fields for a UTF-8 coverage
    fixture. Missing required characters fail the bundle build.
    Glyph coverage, baseline, dense pages and heading metrics need validation
    for each replacement font. CJK bold and italic use the regular pack.

    The converter writes OFL.TXT for SIL OFL 1.1 and LICENSE.TXT otherwise.
    Direct converter calls retain the OFL default for compatibility.
    For other licenses, pass --license-name <actual-license-name>.

    The builder checks Cargo dependencies before it reuses cached packs.
    The cache key includes the manifest, converter binary, source files,
    Cargo.lock, Cargo configuration and rustc version.
    Each cache hit checks every output hash. Corrupt cache files cause a
    rebuild. The builder replaces only directories with its BUNDLE.JSON.
    Use --cache <directory> to choose another cache location.

    Commit source fonts, licenses and manifests. Keep generated packs and
    caches under target/. Publish the selected bundle as a release artifact.
    The duplicate archived Iansui ZIP was removed after byte comparison.

    The bundle tests run with the font tools: cargo test-tools

hardware
    mcu         ESP32-C3, single-core RISC-V RV32IMC, 160 MHz
    ram         400 KB DRAM; ~172 KB heap (108 KB main + 64 KB reclaimed)
    display     800x480 SSD1677 mono e-paper, DMA-backed SPI, portrait
    storage     microSD over shared SPI bus (400 kHz probe, 20 MHz run)
    input       2 ADC ladders (GPIO1, GPIO2) + power button (GPIO3 IRQ)
    battery     li-ion via ADC, 100K/100K divider on GPIO0

    pin map:
      GPIO0   battery ADC          GPIO6   EPD BUSY
      GPIO1   button row 1 ADC     GPIO7   SPI MISO
      GPIO2   button row 2 ADC     GPIO8   SPI SCK
      GPIO3   power button         GPIO10  SPI MOSI
      GPIO4   EPD DC               GPIO12  SD CS (raw register GPIO)
      GPIO5   EPD RST              GPIO21  EPD CS

    EPD and SD share SPI2, arbitrated by CriticalSectionDevice.

building
    requires the nightly pinned in rust-toolchain.toml (build-std needs
    nightly; rust-src and the riscv32imc/imac targets are installed
    automatically by rustup). see specs/changes/archive/onepage-c61-port/baseline.md.

    exactly one board must be selected (a bare `cargo build` fails with
    a "no board selected" error on purpose):

        cargo build-x4 --locked      xteink x4, esp32c3, riscv32imc
        cargo run-x4                 build + flash + monitor (espflash)
        cargo build-x4-wifi --locked x4 with wifi upload (--features wifi)
        cargo build-c61-wifi --locked c61 with wifi upload (--features wifi)

        cargo build-c61 --locked     onepage c61, esp32c61, riscv32imac;
                                     builds both images: pulp-os-c61 (full
                                     offline firmware, src/bin/main_c61.rs)
                                     and pulp-os-c61-boot (bring-up image)
        cargo run-c61                flash + monitor the full firmware;
                                     needs espflash >= 4.4.0 (4.3.0 does
                                     not know esp32c61); flash 40 MHz dio
        cargo run-c61-boot           same for the bring-up image
        cargo test-board-logic       host tests of the HAL-free board logic
                                     (board-logic/, host target)
        cargo test-harness           harness/tests (board-harness): the real
                                     c61 adapters + x4 epd-driver / input
                                     sources host-compiled against an esp-hal
                                     shim (wire traces locked by committed
                                     goldens), plus the ELF / dependency
                                     checks below; needs `task build`
        cargo test-host              pulp-host (paging, rendering, storage)
        cargo test-reader-regression English TXT/EPUB regression with the
                                     pre-port golden trace (reader-regression/)
        task acceptance              everything software-side (Taskfile.yml)

    the aliases live in .cargo/config.toml and pass --target and
    --features board-x4 / board-onepage-c61 explicitly. the chip crates
    follow the target; the board feature must agree with it. checked by
    harness/tests/board_selection.rs.

    wifi upload is optional and off by default for every board: the
    default (offline) firmware does not link esp-radio / embassy-net, has
    no Upload menu entry, and cannot enter upload mode. `--features wifi`
    (or the *-wifi aliases) adds it. esp-radio is built at opt-level 3
    (package override in Cargo.toml), the rest stays at 's'. settings.txt
    still round-trips the
    wifi_ssid / wifi_pass keys so credentials are not lost. the c61 wifi
    build shrinks the main internal heap from 96 KiB to 52 KiB
    (INTERNAL_HEAP_MAIN_BYTES_WIFI) to fit the radio's statics; the
    offline build keeps 96 KiB. checked by harness/tests:

        offline_boundary.rs    offline: no radio crates / symbols
        wifi_build_check.rs    wifi: radio set, opt-level, IPv4-only smoltcp
        c61_memory_budget.rs   c61 image memory budget (offline and wifi)

    c61 wifi hardware acceptance (association, DHCP, HTTP, mDNS, re-entry,
    current; none run on hardware, all tracked as UNVERIFIED):
    specs/changes/archive/onepage-wifi-upload/hardware-acceptance.md

    status: the c61 build is the full offline firmware
    (src/bin/main_c61.rs: reader, files, settings, bookmarks, home,
    session restore, idle-timeout deep sleep, PSRAM-budgeted buffers)
    plus the minimal bring-up image (src/bin/c61_boot.rs). both link;
    neither has been run on hardware. every c61 refresh is a full
    refresh (no partial refresh yet); see
    specs/changes/archive/onepage-c61-port/baseline.md (T12) for the key
    mapping (no Menu key: long-press ENTER in the reader opens the quick
    menu) and the decisions awaiting confirmation.

    local path dependencies (sibling dirs):
      embedded-sdmmc    async FAT filesystem over SD/SPI (local fork)
      smol-epub         no_std epub/zip/html/image processing

features
    txt reader      lazy page-indexed, read-ahead prefetch,
                    proportional font wrapping
    epub reader     ZIP/OPF/HTML-strip pipeline, chapter cache on SD,
                    proportional fonts with bold/italic/heading styles,
                    inline PNG/JPEG (1-bit Floyd-Steinberg dithered),
                    TOC browser (NCX or inline), chapter navigation
    file browser    paginated SD listing, background EPUB title
                    scanner (resolves titles from OPF metadata)
    bookmarks       16-slot LRU in RAM, flushed to SD every 30 s;
                    home screen bookmarks browser sorted by recency
    wifi upload     optional (--features wifi, off by default);
                    HTTP file upload + mDNS (pulp.local);
                    drag-and-drop web UI with delete support
    fonts           regular/bold/italic TTFs rasterised at build time
                    via fontdue; five sizes, book and UI independently
                    configurable
    display         partial DU refresh (~400 ms page turn), periodic
                    full GC refresh (configurable interval)
    quick menu      per-app actions + screen refresh + go home,
                    triggered by power button
    settings        sleep timeout, ghost clear interval,
                    book font size, UI font size, wifi credentials
    sleep           idle timeout + power long-press; EPD deep sleep
                    (~3 uA) + ESP32-C3 deep sleep (~5 uA); GPIO3 wake

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
        upload/             wifi upload server (wifi feature only)
          mod.rs            radio / network wiring, TCP accept, screens
          session.rs        one session: credentials, association, DHCP, serve
          connect.rs        credential check, stage limits, error text
          http.rs           GET / and /files, POST /upload and /delete
          mdns.rs           pulp.local responder
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
    assets/upload.html      web UI for wifi upload mode

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

    wifi upload. bypasses normal dispatch. HTTP server on port 80,
    mDNS on 5353 (pulp.local). multipart upload with 8.3 filename
    sanitisation. radio torn down before returning to app loop.

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
