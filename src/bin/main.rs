// hardware init, construct Kernel + AppManager, boot, run

#![no_std]
#![no_main]

extern crate alloc;

use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::ram;
use esp_hal::timer::timg::TimerGroup;
use log::info;

use pulp_os::apps::Launcher;
use pulp_os::apps::files::FilesApp;
use pulp_os::apps::home::HomeApp;
use pulp_os::apps::manager::AppManager;
use pulp_os::apps::reader::ReaderApp;
use pulp_os::apps::settings::SettingsApp;
use pulp_os::apps::widgets::{ButtonFeedback, QuickMenu};
use pulp_os::board::action::ButtonMapper;
use pulp_os::board::{Board, speed_up_spi};
use pulp_os::drivers::battery;
use pulp_os::drivers::input::InputDriver;
use pulp_os::drivers::sdcard::SdStorage;
use pulp_os::drivers::storage;
use pulp_os::kernel::BookmarkCache;
use pulp_os::kernel::BootConsole;
use pulp_os::kernel::Kernel;
use pulp_os::kernel::dir_cache::DirCache;
use pulp_os::kernel::tasks;
use pulp_os::kernel::work_queue;
use pulp_os::ui::paint_stack;
use pulp_render::strip::StripBuffer;
use static_cell::{ConstStaticCell, StaticCell};

esp_bootloader_esp_idf::esp_app_desc!();

// heavy statics: kept out of the async future to keep it ~200 B

static STRIP: ConstStaticCell<StripBuffer> = ConstStaticCell::new(StripBuffer::new());
static READER: ConstStaticCell<ReaderApp> = ConstStaticCell::new(ReaderApp::new());
static LAUNCHER: ConstStaticCell<Launcher> = ConstStaticCell::new(Launcher::new());
static QUICK_MENU: ConstStaticCell<QuickMenu> = ConstStaticCell::new(QuickMenu::new());
static BUMPS: ConstStaticCell<ButtonFeedback> = ConstStaticCell::new(ButtonFeedback::new());
static DIR_CACHE: ConstStaticCell<DirCache> = ConstStaticCell::new(DirCache::new());
static BM_CACHE: ConstStaticCell<BookmarkCache> = ConstStaticCell::new(BookmarkCache::new());
// BootConsole is heap-allocated during boot and dropped after display,
// reclaiming ~3 KB that would otherwise sit unused in .bss forever.

static HOME: StaticCell<HomeApp> = StaticCell::new();
static FILES: StaticCell<FilesApp> = StaticCell::new();
static SETTINGS: StaticCell<SettingsApp> = StaticCell::new();

#[esp_rtos::main]
async fn main(spawner: embassy_executor::Spawner) -> ! {
    esp_println::logger::init_logger_from_env();
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    paint_stack();
    // C61 usable DRAM is RAM 0x3EA70 (256,624) + dram2_seg 0x10000 (65,536)
    // = 322,160 bytes. That is ~18 KB less than the C3 had, so the heaps are
    // sized down here rather than moving the stack to PSRAM (the C61's stock
    // linker script has no extmem region, so PSRAM statics cannot be placed).
    // The reclaimed bootloader heap is the first thing to give up: it is the
    // cheapest 32 KB on the board.
    esp_alloc::heap_allocator!(size: 48 * 1024);
    esp_alloc::heap_allocator!(#[ram(reclaimed)] size: 32 * 1024);

    let mut console = alloc::boxed::Box::new(BootConsole::new());
    console.push("pulp-os 0.1.0");
    console.push("esp32c61 rv32imac");
    console.push("heap: 80K (48K + 32K reclaimed)");

    info!("booting...");

    // Safety: TIMG0 and FROM_CPU_INTR0 are cloned here and consumed by
    // esp_rtos::start. They are never used again after this point.
    // Board::init (which takes ownership of `peripherals`) does not
    // touch TIMG0 or FROM_CPU_INTR0, see the pin ownership table in
    // board/mod.rs for the full split.
    let timg0 = TimerGroup::new(unsafe { peripherals.TIMG0.clone_unchecked() });
    esp_rtos::start(timg0.timer0, unsafe {
        peripherals.FROM_CPU_INTR0.clone_unchecked()
    });

    // Peripherals move into Board::init, which splits them across
    // init_input (ADC pins, discrete keys, IO_MUX) and init_spi_peripherals
    // (SPI2, DMA, display + SD GPIOs). each peripheral is used in
    // exactly one place, see the ownership table in board/mod.rs.
    let board = Board::init(peripherals);
    console.push("spi: dma ch0, 4096B tx+rx");

    let mut epd = board.display.epd;
    let mut delay = Delay::new();
    epd.init(&mut delay);
    console.push("epd: ssd1677 800x480 init");

    speed_up_spi();
    console.push("spi: 400kHz -> 20MHz");

    let sd = match board.storage.sd_card {
        Some(card) => {
            console.push("sd: card detected");
            SdStorage::mount(card).await
        }
        None => {
            // say which case this is. "not found" is ambiguous between an
            // empty slot and a card that would not answer on the bus.
            if pulp_os::board::sd_card_inserted() {
                console.push("sd: card present, no response");
            } else {
                console.push("sd: no card in slot");
            }
            SdStorage::empty()
        }
    };

    let sd_ok = sd.probe_ok();
    if sd_ok {
        console.push("sd: fat32 mounted");
        if let Err(e) = storage::ensure_pulp_dir_async(&sd).await {
            console.push("sd: pulp dir failed");
            log::warn!("ensure_pulp_dir: {:?}", e);
        }
    }

    let mut input = InputDriver::new(board.input);
    let battery_mv = battery::adc_to_battery_mv(input.read_battery_mv());

    let mut kernel = Kernel::new(
        sd,
        epd,
        STRIP.take(),
        DIR_CACHE.take(),
        BM_CACHE.take(),
        delay,
        sd_ok,
        battery_mv,
    );

    let mut app_mgr = AppManager::new(
        LAUNCHER.take(),
        HOME.init(HomeApp::new()),
        FILES.init(FilesApp::new()),
        READER.take(),
        SETTINGS.init(SettingsApp::new()),
        QUICK_MENU.take(),
        BUMPS.take(),
        ButtonMapper::new(),
    );

    console.push("kernel: constructed");
    kernel.show_boot_console(&console).await;
    drop(console); // reclaim ~3 KB of heap

    kernel.boot(&mut app_mgr).await;

    // register the image decoder so the kernel's worker task can
    // decode JPEG/PNG without depending on smol-epub directly
    work_queue::register_image_decoder(|data, is_jpeg, max_w, max_h| {
        let raw = if is_jpeg {
            smol_epub::jpeg::decode_jpeg_fit(data, max_w, max_h)
        } else {
            smol_epub::png::decode_png_fit(data, max_w, max_h)
        };
        raw.map(|img| work_queue::DecodedImage {
            width: img.width,
            height: img.height,
            data: img.data,
            stride: img.stride,
        })
    });

    // embassy-executor 0.10: `#[task]` now yields `Result<SpawnToken, SpawnError>`
    // and `spawn` is infallible, so the token is unwrapped here and `spawn`
    // takes it bare.
    spawner.spawn(tasks::input_task(input).expect("spawn input_task"));
    spawner.spawn(tasks::housekeeping_task().expect("spawn housekeeping_task"));
    spawner.spawn(tasks::idle_timeout_task().expect("spawn idle_timeout_task"));
    spawner.spawn(work_queue::worker_task().expect("spawn worker_task"));
    info!("kernel ready.");

    kernel.run(&mut app_mgr).await
}
