// One shared ADC1 for the front key ladder (GPIO4) and the battery (GPIO5).
//
// The BSP creates ADC1 once in `power_init` and lends the same oneshot handle to
// the keys (board_c61.c:257-260, :310-313: "so it doesn't create a second ADC1
// unit"). esp-hal needs the same shape: every pin is enabled in ONE `AdcConfig`
// before `Adc::new`, and the resulting `Adc` is shared. Both pins use 11 dB
// attenuation (BSP ADC_ATTEN_DB_12) and curve-fitting calibration, so
// `read_oneshot` returns calibrated mV for both.
//
// Channels: esp-hal's own table maps GPIO4 = ADC1_CH2 and GPIO5 = ADC1_CH3
// (esp-metadata-generated 0.5.3 `_generated_esp32c61.rs`), same as the BSP
// (board_c61.c:63, README.md:49-50). `init` checks both so a table change
// cannot silently read the wrong node. (The BSP comment at board_c61.c:315
// that says "GPIO5 = ADC1_CH2" is a typo for GPIO4.)
//
// Sharing: `SharedAdc` is a copyable handle to a critical-section mutex around
// the `Adc` and both pins. A conversion (start + bounded wait) runs
// inside ONE critical section, so a key poll and a battery measurement from
// different tasks can never interleave on the converter.
//
// Not verified on hardware: calibration data in this chip's eFuse, real
// conversion time, the high source impedance of the battery divider (about
// 2.55 MOhm, see pulp_board_logic::battery).

use core::cell::RefCell;

use critical_section::Mutex;
use esp_hal::{
    Blocking,
    analog::adc::{Adc, AdcCalCurve, AdcChannel, AdcConfig, AdcPin, Attenuation},
    peripherals::{ADC1, GPIO4, GPIO5},
};
use pulp_board_logic::battery::BATTERY_ADC_CHANNEL;
use pulp_board_logic::keys::FRONT_ADC_CHANNEL;
use static_cell::StaticCell;

/// Upper bound on busy-waiting for one ADC conversion (a conversion takes tens of
/// microseconds; this only prevents an endless wait if it never completes).
const ADC_SPIN_LIMIT: u32 = 20_000;

type FrontPin = AdcPin<GPIO4<'static>, ADC1<'static>, AdcCalCurve<ADC1<'static>>>;
type BatteryPin = AdcPin<GPIO5<'static>, ADC1<'static>, AdcCalCurve<ADC1<'static>>>;

#[derive(Copy, Clone, PartialEq, Eq)]
enum Channel {
    Front,
    Battery,
}

struct Converter {
    adc: Adc<'static, ADC1<'static>, Blocking>,
    front: FrontPin,
    battery: BatteryPin,
    pending: Option<Channel>,
}

impl Converter {
    fn finish(&mut self, channel: Channel) -> Option<u16> {
        self.pending = Some(channel);
        for _ in 0..ADC_SPIN_LIMIT {
            let result = match channel {
                Channel::Front => self.adc.read_oneshot(&mut self.front),
                Channel::Battery => self.adc.read_oneshot(&mut self.battery),
            };
            match result {
                Ok(mv) => {
                    self.pending = None;
                    return Some(mv);
                }
                Err(nb::Error::WouldBlock) => continue,
                // Retain the channel until a successful read confirms that the
                // HAL released it. A failed conversion is never reported as zero.
                Err(nb::Error::Other(())) => return None,
            }
        }
        None
    }

    fn read(&mut self, channel: Channel) -> Option<u16> {
        if let Some(pending) = self.pending {
            // The timed-out sample may predate the current measurement window.
            // Complete it to release the HAL channel, then take a fresh sample.
            self.finish(pending)?;
        }
        self.finish(channel)
    }
}

type AdcCell = Mutex<RefCell<Converter>>;
static ADC1_CELL: StaticCell<AdcCell> = StaticCell::new();

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AdcError {
    /// esp-hal maps a pin to a different ADC1 channel than the BSP.
    ChannelMismatch { gpio: u8, expected: u8, found: u8 },
    /// `init` was called twice (ADC1 and its pins are single-owner).
    AlreadyInitialised,
}

/// Handle to the one ADC1 instance. Cheap to copy; every copy reaches the same
/// converter.
#[derive(Copy, Clone)]
pub struct SharedAdc(&'static AdcCell);

impl SharedAdc {
    /// Read the front ladder, first completing any timed-out battery conversion.
    pub fn read_front_mv(&self) -> Option<u16> {
        critical_section::with(|cs| self.0.borrow_ref_mut(cs).read(Channel::Front))
    }

    /// Read the battery, first completing any timed-out ladder conversion.
    pub fn read_battery_mv(&self) -> Option<u16> {
        critical_section::with(|cs| self.0.borrow_ref_mut(cs).read(Channel::Battery))
    }
}

/// Shared handle; the converter keeps both enabled pins for timeout recovery.
pub struct AdcSet {
    pub adc: SharedAdc,
}

/// ADC1 channels esp-hal assigns to GPIO4 (front ladder) and GPIO5 (battery),
/// read from esp-hal's own table. BSP: 2 and 3.
pub fn channels(front: &GPIO4<'static>, battery: &GPIO5<'static>) -> (u8, u8) {
    (front.adc_channel(), battery.adc_channel())
}

/// Create the single ADC1 with both pins enabled in the same `AdcConfig`.
pub fn init(
    adc1: ADC1<'static>,
    front: GPIO4<'static>,
    battery: GPIO5<'static>,
) -> Result<AdcSet, AdcError> {
    let (front_ch, battery_ch) = channels(&front, &battery);
    if front_ch != FRONT_ADC_CHANNEL {
        return Err(AdcError::ChannelMismatch {
            gpio: 4,
            expected: FRONT_ADC_CHANNEL,
            found: front_ch,
        });
    }
    if battery_ch != BATTERY_ADC_CHANNEL {
        return Err(AdcError::ChannelMismatch {
            gpio: 5,
            expected: BATTERY_ADC_CHANNEL,
            found: battery_ch,
        });
    }
    let mut cfg = AdcConfig::new();
    let front = cfg.enable_pin_with_cal::<_, AdcCalCurve<ADC1>>(front, Attenuation::_11dB);
    let battery = cfg.enable_pin_with_cal::<_, AdcCalCurve<ADC1>>(battery, Attenuation::_11dB);
    let adc = Adc::new(adc1, cfg);
    let cell = ADC1_CELL
        .try_init(Mutex::new(RefCell::new(Converter {
            adc,
            front,
            battery,
            pending: None,
        })))
        .ok_or(AdcError::AlreadyInitialised)?;
    Ok(AdcSet {
        adc: SharedAdc(cell),
    })
}
