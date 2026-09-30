// board support for the MoveCall OnePage (ESP32-C61, SSD1677 800x480, SD over SPI2)
// DMA-backed SPI (AHB GDMA CH0); CriticalSectionDevice arbitrates the bus

pub mod action;
pub mod battery;
pub mod button;
pub mod layout;

pub use crate::drivers::sdcard::{SdStorage, SyncSdCard};
pub use crate::drivers::ssd1677::{DisplayDriver, SPI_FREQ_MHZ};
pub use button::{Button, KEY_LADDER_BANDS, decode_ladder};
pub use pulp_render::panel::{HEIGHT, WIDTH};

// logical screen size (portrait mode via 270-degree rotation of 800x480 panel)
pub const SCREEN_W: u16 = HEIGHT; // 480
pub const SCREEN_H: u16 = WIDTH; // 800

use core::cell::RefCell;

use critical_section::Mutex;
use embedded_hal_bus::spi::CriticalSectionDevice;
use esp_hal::{
    Blocking,
    analog::adc::{Adc, AdcCalCurve, AdcConfig, AdcPin, Attenuation},
    delay::Delay,
    dma::{DmaRxBuf, DmaTxBuf},
    gpio::{Event, Input, InputConfig, Io, Level, Output, OutputConfig, Pull, WakeupConfig},
    peripherals::{ADC1, GPIO4, GPIO5, Peripherals},
    spi,
    time::Rate,
};
use log::info;
use static_cell::StaticCell;

pub type SpiBus = spi::master::SpiDma<'static, Blocking>;
pub type SharedSpiDevice = CriticalSectionDevice<'static, SpiBus, Output<'static>, Delay>;
pub type SdSpiDevice = CriticalSectionDevice<'static, SpiBus, Output<'static>, Delay>;
pub type Epd = DisplayDriver<SharedSpiDevice, Output<'static>, Output<'static>, Input<'static>>;

static SPI_BUS: StaticCell<Mutex<RefCell<SpiBus>>> = StaticCell::new();

// cached ref to the SPI bus mutex, set once in Board::init
// pub(crate) so the scheduler can access the bus in sd_card_sleep
// before deep sleep
pub(crate) static SPI_BUS_REF: Mutex<core::cell::Cell<Option<&'static Mutex<RefCell<SpiBus>>>>> =
    Mutex::new(core::cell::Cell::new(None));

// sd cs clone; only used in enter_sleep to send cmd0
// safety: same clone_unchecked pattern as the pins in init_input;
// only accessed after all normal sd i/o has stopped and before mcu halts
pub(crate) static SD_CS_SLEEP: Mutex<RefCell<Option<Output<'static>>>> =
    Mutex::new(RefCell::new(None));

// GPIO27 (EPD RST + SD/MIC power gate) clone; only used to hold the pad
// level across deep sleep
pub(crate) static EPD_RST_REF: Mutex<RefCell<Option<Output<'static>>>> =
    Mutex::new(RefCell::new(None));

/// Discrete active-low keys, kept behind one critical section so the GPIO
/// interrupt handler can clear them.
struct DiscreteKeys {
    /// GPIO6 = PREV / UP
    prev: Input<'static>,
    /// GPIO9 = NEXT / DOWN
    next: Input<'static>,
    /// GPIO2 = WAKE / power. This is also the deep-sleep wake source, so it
    /// is the only pin configured with the low-power path.
    wake: Input<'static>,
}

static DISCRETE: Mutex<RefCell<Option<DiscreteKeys>>> = Mutex::new(RefCell::new(None));

/// GPIO28 card-detect switch. The pin is kept, not just sampled, so the
/// state can be re-read live: a card can be inserted or pulled at any time,
/// and a boot-time snapshot would leave a later insertion looking like a
/// permanently empty slot.
static SD_CARD_DETECT: Mutex<RefCell<Option<Input<'static>>>> = Mutex::new(RefCell::new(None));

#[esp_hal::handler]
fn gpio_handler() {
    critical_section::with(|cs| {
        if let Some(keys) = DISCRETE.borrow_ref_mut(cs).as_mut() {
            for pin in [&mut keys.prev, &mut keys.next, &mut keys.wake] {
                if pin.is_interrupt_set() {
                    pin.clear_interrupt();
                }
            }
        }
    });
}

/// True while a card is seated in the microSD socket (GPIO28 low).
///
/// Read live, so this tracks insertion and removal. It is only used to skip
/// the pointless bus probe at boot; it does not by itself remount a card that
/// appears later.
pub fn sd_card_inserted() -> bool {
    critical_section::with(|cs| {
        SD_CARD_DETECT
            .borrow_ref_mut(cs)
            .as_ref()
            .map(|pin| pin.is_low())
            .unwrap_or(true)
    })
}

/// True while the wake/power key (GPIO2) is held low. The MCU stays in deep
/// sleep on this pin, so this is also how a held key survives a wake.
pub fn wake_key_is_low() -> bool {
    with_discrete(|k| k.wake.is_low())
}

/// True while the PREV/UP key (GPIO6) is held low.
pub fn prev_key_is_low() -> bool {
    with_discrete(|k| k.prev.is_low())
}

/// True while the NEXT/DOWN key (GPIO9) is held low.
pub fn next_key_is_low() -> bool {
    with_discrete(|k| k.next.is_low())
}

fn with_discrete(f: impl FnOnce(&mut DiscreteKeys) -> bool) -> bool {
    critical_section::with(|cs| DISCRETE.borrow_ref_mut(cs).as_mut().map(f).unwrap_or(false))
}

pub struct InputHw {
    pub adc: Adc<'static, ADC1<'static>, Blocking>,
    /// GPIO4 = ADC1_CH2, the 4-way front-key ladder
    pub ladder: AdcPin<GPIO4<'static>, ADC1<'static>, AdcCalCurve<ADC1<'static>>>,
    /// GPIO5 = ADC1_CH3, battery sense through a 1:1 divider
    pub battery: AdcPin<GPIO5<'static>, ADC1<'static>, AdcCalCurve<ADC1<'static>>>,
}

pub struct DisplayHw {
    pub epd: Epd,
}

pub struct StorageHw {
    // sd card, initialised at 400 kHz before EPD touches the bus
    pub sd_card: Option<SyncSdCard>,
}

pub struct Board {
    pub input: InputHw,
    pub display: DisplayHw,
    pub storage: StorageHw,
}

impl Board {
    pub fn init(p: Peripherals) -> Self {
        let input = Self::init_input(&p);
        let (display, storage) = Self::init_spi_peripherals(p);
        Board {
            input,
            display,
            storage,
        }
    }

    // gpio / peripheral ownership:
    //
    // init_input (clone_unchecked)      init_spi_peripherals (move/clone)
    // ---                               ---
    // GPIO2   KEY_WAKE / power          GPIO8   EPD DC
    // GPIO4   front-key ladder ADC      GPIO10  charge enable
    // GPIO5   battery sense ADC         GPIO22  SPI SCLK
    // GPIO6   KEY_PREV                  GPIO23  SPI MOSI
    // GPIO9   KEY_NEXT                  GPIO24  SPI MISO (SD only, must stay wired)
    // GPIO11  USB detect                GPIO25  EPD CS
    // GPIO28  SD card detect            GPIO26  SD CS
    // ADC1                                GPIO27  EPD RST + SD/MIC power gate
    // IO_MUX                              GPIO29  EPD BUSY
    //                                    SPI2, DMA_CH0
    //
    // GPIO27 is NOT a plain reset pin: OnePage wires EPD RST and the SD/MIC
    // power gate to the same net, so driving it high powers the panel and the
    // SD rail. The one reset pulse therefore has to happen before the first SD
    // mount; after that the controller is reset with its soft reset (0x12).
    //
    // Two pads are shared deliberately, each for a pre-sleep use that can only
    // run once all other I/O has stopped:
    //   GPIO26  SD device CS, and SD_CS_SLEEP for the CMD0 before sleep
    //   GPIO27  EPD reset, and EPD_RST_REF for the pad hold before sleep
    // Both halves are built from the same pin singleton, and they are used in
    // sequence, never concurrently.

    // Safety for all clone_unchecked calls below:
    //
    // init_input borrows Peripherals immutably and clones the pins it
    // needs.  init_spi_peripherals later takes ownership of the full
    // Peripherals struct but only touches a disjoint set of GPIOs
    // (GPIO8, GPIO10, GPIO11, GPIO22-27, GPIO29, SPI2, DMA_CH0).  See the
    // ownership table above for the complete split.  Each peripheral listed
    // here is used exclusively by InputHw and never touched again.
    fn init_input(p: &Peripherals) -> InputHw {
        let mut adc_cfg = AdcConfig::new();

        // Safety: GPIO4 is used only here (front-key ladder ADC).
        let ladder = adc_cfg.enable_pin_with_cal::<_, AdcCalCurve<ADC1>>(
            unsafe { p.GPIO4.clone_unchecked() },
            Attenuation::_11dB,
        );

        // Safety: GPIO5 is used only here (battery voltage ADC).
        let battery = adc_cfg.enable_pin_with_cal::<_, AdcCalCurve<ADC1>>(
            unsafe { p.GPIO5.clone_unchecked() },
            Attenuation::_11dB,
        );

        // Safety: ADC1 is used only here; init_spi_peripherals does not use ADC.
        let adc = Adc::new(unsafe { p.ADC1.clone_unchecked() }, adc_cfg);

        // Safety: IO_MUX is used only here for the GPIO interrupt handler.
        let mut io = Io::new(unsafe { p.IO_MUX.clone_unchecked() });
        io.set_interrupt_handler(gpio_handler);

        // Safety: each of these three is used only here (discrete key input).
        let mut prev = Input::new(
            unsafe { p.GPIO6.clone_unchecked() },
            InputConfig::default().with_pull(Pull::Up),
        );
        let mut next = Input::new(
            unsafe { p.GPIO9.clone_unchecked() },
            InputConfig::default().with_pull(Pull::Up),
        );
        let mut wake = Input::new(
            unsafe { p.GPIO2.clone_unchecked() },
            InputConfig::default().with_pull(Pull::Up),
        );

        // The discrete keys only need to be *read*; polling is enough and
        // avoids three interrupt sources for a device that is polled anyway.
        prev.listen(Event::FallingEdge);
        next.listen(Event::FallingEdge);
        wake.listen(Event::FallingEdge);

        // GPIO2 is an LP pad (LP_GPIO2) and is the deep-sleep wake source.
        // Without this the pad is read through the high-performance GPIO
        // peripheral, which deep sleep powers down.
        wake.apply_wakeup_config(&WakeupConfig::default().with_low_power_path(true))
            .expect("GPIO2 must have a low-power path on ESP32-C61");

        critical_section::with(|cs| {
            DISCRETE
                .borrow_ref_mut(cs)
                .replace(DiscreteKeys { prev, next, wake });
        });

        // Safety: GPIO28 is used only here (SD card-detect switch).
        let cd = Input::new(
            unsafe { p.GPIO28.clone_unchecked() },
            InputConfig::default().with_pull(Pull::Up),
        );
        // the switch shorts the pad to ground when a card is seated
        let inserted = cd.is_low();
        info!(
            "sd card detect: GPIO28 {} at boot",
            if inserted {
                "low (card present)"
            } else {
                "high (slot empty)"
            }
        );
        critical_section::with(|cs| SD_CARD_DETECT.borrow_ref_mut(cs).replace(cd));

        info!("keys: ladder GPIO4 (ADC1_CH2), prev GPIO6, next GPIO9, wake GPIO2");
        info!("wake button: GPIO2 LowLevel + low-power path armed");

        InputHw {
            adc,
            ladder,
            battery,
        }
    }

    // 400 kHz for SD probe, then 20 MHz; DMA-backed
    //
    // GPIO26 (SD CS) and GPIO27 (EPD RST / power gate) each need two owners:
    // the one the driver holds, and one parked in a static for the pre-sleep
    // CMD0 and pad-hold. Both are built here from `&Peripherals`, and the
    // drivers then take the owned `Peripherals`. Safety: a pad has exactly one
    // configuration, and the second handle is only ever touched after the
    // first owner's last operation, which is why both are documented in the
    // ownership table above.
    fn init_spi_peripherals(p: Peripherals) -> (DisplayHw, StorageHw) {
        // Safety: GPIO26 is shared between `sd_cs` (the SD device) and
        // `sd_cs_sleep` (a pre-sleep CMD0), which are used in sequence and
        // never concurrently.
        let sd_cs_sleep = Output::new(
            unsafe { p.GPIO26.clone_unchecked() },
            Level::High,
            OutputConfig::default(),
        );
        critical_section::with(|cs| {
            SD_CS_SLEEP.borrow_ref_mut(cs).replace(sd_cs_sleep);
        });

        // Safety: same split for GPIO27, between the EPD driver's reset and
        // the pad hold taken just before deep sleep.
        let rst_sleep = Output::new(
            unsafe { p.GPIO27.clone_unchecked() },
            Level::High,
            OutputConfig::default(),
        );
        critical_section::with(|cs| {
            EPD_RST_REF.borrow_ref_mut(cs).replace(rst_sleep);
        });

        let epd_cs = Output::new(p.GPIO25, Level::High, OutputConfig::default());
        let dc = Output::new(p.GPIO8, Level::High, OutputConfig::default());
        // GPIO27 gates the EPD and the SD/MIC rail. Start high so the panel
        // and the card are powered before the reset pulse below.
        let rst = Output::new(p.GPIO27, Level::High, OutputConfig::default());
        // OnePage BUSY is high while the controller is busy (the X4 was the
        // other way round), so the driver reads this pin as active-low.
        let busy = Input::new(p.GPIO29, InputConfig::default().with_pull(Pull::None));

        // GPIO26 has a proper peripheral type on the C61, so no raw registers.
        let sd_cs = Output::new(p.GPIO26, Level::High, OutputConfig::default());

        // charge enable: low pauses charging, so drive it high (charging on)
        let _charge_enable = Output::new(p.GPIO10, Level::High, OutputConfig::default());

        // USB detect: low means USB is present. Only read for now; the
        // housekeeping task decides what to do with it.
        let _usb_detect = Input::new(p.GPIO11, InputConfig::default().with_pull(Pull::Up));

        let slow_cfg = spi::master::Config::default().with_frequency(Rate::from_khz(400));

        let mut spi_raw = spi::master::Spi::new(p.SPI2, slow_cfg)
            .unwrap()
            .with_sck(p.GPIO22)
            .with_mosi(p.GPIO23)
            .with_miso(p.GPIO24);

        // 80 clocks with CS high before DMA conversion (SD spec init)
        let _ = spi_raw.write(&[0xFF; 10]);

        // 4096B each direction: strip max ~4000B, SD sectors 512B.
        // `DmaRxBuf::new` takes owned `DmaAlignedMut`s, and the public
        // `dma_buffers!` macro throws that ownership away with `into_inner`,
        // so call the underlying impl macro. The three-argument form is
        // needed because the default chunk size the single-argument form
        // picks is not accepted by the descriptor-count helper on this chip;
        // 4092 is esp-hal's own `dma::CHUNK_SIZE`.
        let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) =
            esp_hal::dma_buffers_impl!(4096, 4096, esp_hal::dma::CHUNK_SIZE);
        let dma_rx_buf = DmaRxBuf::new(rx_descriptors, rx_buffer).unwrap();
        let dma_tx_buf = DmaTxBuf::new(tx_descriptors, tx_buffer).unwrap();

        let spi_dma_bus = spi_raw
            .with_dma(p.DMA_CH0)
            .with_buffers(dma_rx_buf, dma_tx_buf);

        let spi_ref: &'static Mutex<RefCell<SpiBus>> =
            SPI_BUS.init(Mutex::new(RefCell::new(spi_dma_bus)));
        info!("SPI bus: DMA enabled (CH0, 4096B TX+RX)");

        critical_section::with(|cs| SPI_BUS_REF.borrow(cs).set(Some(spi_ref)));

        let sd_spi = CriticalSectionDevice::new(spi_ref, sd_cs, Delay::new()).unwrap();

        // init SD card now, at 400 kHz on a pristine bus, before EPD
        // traffic -- SD spec requires CMD0 on a clean bus
        let sd_card = SdStorage::init_card(sd_spi);

        let epd_spi = CriticalSectionDevice::new(spi_ref, epd_cs, Delay::new()).unwrap();
        let epd = DisplayDriver::new(epd_spi, dc, rst, busy);

        (DisplayHw { epd }, StorageHw { sd_card })
    }
}

// switch SPI bus from 400 kHz to operational frequency (20 MHz)
// call after Board::init and before first EPD render
pub fn speed_up_spi() {
    let fast_cfg = spi::master::Config::default().with_frequency(Rate::from_mhz(SPI_FREQ_MHZ));
    critical_section::with(|cs| {
        if let Some(bus) = SPI_BUS_REF.borrow(cs).get() {
            bus.borrow(cs).borrow_mut().apply_config(&fast_cfg).unwrap();
            info!("SPI bus: 400kHz -> {}MHz", SPI_FREQ_MHZ);
        }
    });
}
