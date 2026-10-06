//! Host stand-in for `esp-hal` 1.2 behind `board-harness`: the shared seam the
//! real kernel/board sources are host-compiled against.
//!
//! Superset of the per-suite seams the old script temp crates had:
//!   * c61 adapters (board_c61/{adc,spi}.rs): ADC1 oneshot semantics,
//!     SPI/GPIO recorder, failure injection (`adc_ready`/`fail_write`/
//!     `take_trace`),
//!   * x4 driver trace (drivers/{ssd1677,strip}.rs): `Delay` recording
//!     `DELAY <ms>` lines (`log`/`take_log`),
//!   * x4 input trace (drivers/input.rs): controllable microsecond clock
//!     (`set_now_us`).
//!
//! A suite uses only part of this surface, so unused parts stay compiled and
//! `dead_code` is allowed. All mutable state sits behind one std Mutex, and
//! tests sharing the recorder hold `session` for their whole body, so the
//! default parallel libtest harness (one thread per test) is safe at any
//! thread count.

#![allow(dead_code)]

use std::sync::Mutex;

/// esp-hal blocking-mode marker (imported by the included sources).
#[derive(Clone, Copy)]
pub struct Blocking;

#[derive(Default)]
struct State {
    /// c61 adapters: adc:/clock:/write:/high:/low:/flush lines
    trace: Vec<String>,
    /// x4 driver stim: CMD/DATA/DELAY/RST lines
    log: Vec<String>,
    /// fake ADC: `true` lets `read_oneshot` complete the conversion
    adc_ready: bool,
    /// fake SPI: `true` makes every write fail (c61 adapter error path)
    fail_write: bool,
    /// simulated clock, microseconds (read by `time::Instant::now`)
    now_us: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    trace: Vec::new(),
    log: Vec::new(),
    adc_ready: false,
    fail_write: false,
    now_us: 0,
});

/// Whole-test lock so parallel test threads cannot interleave their traces.
static SESSION: Mutex<()> = Mutex::new(());

/// Run `f` while holding the recorder lock for the whole test body: tests
/// touching the shared trace/ADC/SPI state wrap their body in this.
pub fn session<R>(f: impl FnOnce() -> R) -> R {
    let _guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    f()
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut s)
}

/// Feed the fake ADC: `false` keeps `read_oneshot` at `WouldBlock` (stalled
/// conversion), `true` lets it complete.
pub fn adc_ready(ready: bool) {
    with(|s| s.adc_ready = ready);
}

/// Drain the c61 adapter trace (ADC/SPI/GPIO order).
pub fn take_trace() -> Vec<String> {
    with(|s| std::mem::take(&mut s.trace))
}

/// Inject SPI write failures for the c61 adapter error path.
pub fn fail_write(fail: bool) {
    with(|s| s.fail_write = fail);
}

fn trace(s: String) {
    with(|st| st.trace.push(s));
}

/// Record one x4 stim trace line.
pub fn log(s: String) {
    with(|st| st.log.push(s));
}

/// Drain the x4 stim trace.
pub fn take_log() -> Vec<String> {
    with(|s| std::mem::take(&mut s.log))
}

/// Set the simulated clock, in microseconds (x4 input stim).
pub fn set_now_us(us: u64) {
    with(|s| s.now_us = us);
}

pub mod peripherals {
    #![allow(non_camel_case_types)]

    use std::marker::PhantomData;

    macro_rules! peripheral {
        ($($p:ident),*) => { $(pub struct $p<'a>(pub PhantomData<&'a ()>);)* };
    }

    // The C61 adapters' single-owner peripherals (board_c61: ADC1 for the
    // front ladder + battery, SPI2/DMA_CH0 for the shared bus).
    peripheral!(
        ADC1, GPIO4, GPIO5, GPIO22, GPIO23, GPIO24, GPIO25, GPIO26, SPI2, DMA_CH0
    );
}

pub mod analog {
    pub mod adc {
        use super::super::{Blocking, peripherals, trace, with};
        use std::marker::PhantomData;

        pub trait AdcChannel {
            fn adc_channel(&self) -> u8;
        }
        impl AdcChannel for peripherals::GPIO4<'_> {
            fn adc_channel(&self) -> u8 {
                2
            }
        }
        impl AdcChannel for peripherals::GPIO5<'_> {
            fn adc_channel(&self) -> u8 {
                3
            }
        }
        pub trait AdcCalScheme<A> {}
        pub struct AdcCalCurve<A>(PhantomData<A>);
        impl<A> AdcCalScheme<A> for AdcCalCurve<A> {}
        pub enum Attenuation {
            _11dB,
        }
        pub struct AdcPin<P, A, C> {
            pin: P,
            _marker: PhantomData<(A, C)>,
        }
        pub struct AdcConfig<A>(PhantomData<A>);
        impl<A> AdcConfig<A> {
            pub fn new() -> Self {
                Self(PhantomData)
            }
            pub fn enable_pin_with_cal<P, C>(&mut self, pin: P, _: Attenuation) -> AdcPin<P, A, C> {
                AdcPin {
                    pin,
                    _marker: PhantomData,
                }
            }
        }
        pub struct Adc<'a, A, M> {
            active: Option<u8>,
            _marker: PhantomData<(&'a (), A, M)>,
        }
        impl<'a, A> Adc<'a, A, Blocking> {
            pub fn new(_: A, _: AdcConfig<A>) -> Self {
                Self {
                    active: None,
                    _marker: PhantomData,
                }
            }
            /// One conversion with the esp-hal 1.2 riscv.rs active-channel
            /// semantics: a pending conversion holds the channel; the test
            /// decides via `adc_ready` when it completes.
            pub fn read_oneshot<P: AdcChannel, C: AdcCalScheme<A>>(
                &mut self,
                pin: &mut AdcPin<P, A, C>,
            ) -> nb::Result<u16, ()> {
                let channel = pin.pin.adc_channel();
                if self.active.is_some_and(|active| active != channel) {
                    return Err(nb::Error::WouldBlock);
                }
                self.active = Some(channel);
                trace(format!("adc:{channel}"));
                if !with(|s| s.adc_ready) {
                    return Err(nb::Error::WouldBlock);
                }
                self.active = None;
                Ok(if channel == 2 { 1200 } else { 1800 })
            }
        }
    }
}

pub mod gpio {
    use crate::trace;
    use std::marker::PhantomData;

    pub enum Level {
        High,
    }
    #[derive(Default)]
    pub struct OutputConfig;
    pub struct Output<'a> {
        name: &'static str,
        _marker: PhantomData<&'a ()>,
    }
    impl<'a> Output<'a> {
        pub fn new<P>(_: P, _: Level, _: OutputConfig) -> Self {
            Self {
                name: std::any::type_name::<P>(),
                _marker: PhantomData,
            }
        }
        pub fn set_low(&mut self) {
            trace(format!("low:{}", self.name));
        }
        pub fn set_high(&mut self) {
            trace(format!("high:{}", self.name));
        }
    }
}

pub mod time {
    use super::with;

    /// Bus/clock rate for the c61 SPI config.
    #[derive(Copy, Clone)]
    pub struct Rate(pub u32);
    impl Rate {
        pub fn from_khz(khz: u32) -> Self {
            Self(khz)
        }
    }

    #[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Instant(u64);
    #[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Duration(u64);
    impl Duration {
        pub fn from_millis(ms: u64) -> Self {
            Duration(ms * 1000)
        }
        pub fn as_millis(&self) -> u64 {
            self.0 / 1000
        }
        pub fn as_micros(&self) -> u64 {
            self.0
        }
    }
    impl Instant {
        pub fn now() -> Self {
            Instant(with(|s| s.now_us))
        }
        pub fn duration_since_epoch(&self) -> Duration {
            Duration(self.0)
        }
    }
    impl core::ops::Add<Duration> for Instant {
        type Output = Instant;
        fn add(self, d: Duration) -> Instant {
            Instant(self.0 + d.0)
        }
    }
    impl core::ops::Sub for Instant {
        type Output = Duration;
        fn sub(self, o: Instant) -> Duration {
            Duration(self.0 - o.0)
        }
    }
}

pub mod delay {
    use super::with;

    #[derive(Clone, Copy)]
    pub struct Delay;
    impl Delay {
        pub fn new() -> Self {
            Delay
        }
        /// X4 driver path: every delay is part of the wire trace and advances
        /// the simulated clock by the same amount (microseconds here; the old
        /// trace-only seam kept a separate unit, which nothing read back).
        pub fn delay_millis(&mut self, ms: u32) {
            with(|s| {
                s.log.push(format!("DELAY {ms}"));
                s.now_us += u64::from(ms) * 1000;
            });
        }
    }
    /// C61 adapter path (run_ops' `Operation::DelayNs`): silent, like the
    /// old seam.
    impl embedded_hal::delay::DelayNs for Delay {
        fn delay_ns(&mut self, _: u32) {}
    }
}

pub mod spi {
    pub mod master {
        use crate::time::Rate;
        use crate::{Blocking, peripherals, trace, with};
        use std::marker::PhantomData;

        #[derive(Default)]
        pub struct Config(pub u32);
        impl Config {
            pub fn with_frequency(self, r: Rate) -> Self {
                Self(r.0)
            }
        }
        pub struct Spi;
        impl Spi {
            pub fn new(_: peripherals::SPI2<'_>, cfg: Config) -> Result<Self, ()> {
                trace(format!("clock:{}", cfg.0));
                Ok(Self)
            }
            pub fn with_sck(self, _: peripherals::GPIO22<'_>) -> Self {
                self
            }
            pub fn with_mosi(self, _: peripherals::GPIO23<'_>) -> Self {
                self
            }
            pub fn with_miso(self, _: peripherals::GPIO24<'_>) -> Self {
                self
            }
            pub fn write(&mut self, b: &[u8]) -> Result<(), ()> {
                write(b)
            }
            pub fn with_dma(self, _: peripherals::DMA_CH0<'_>) -> SpiDma<'static, Blocking> {
                SpiDma(PhantomData)
            }
        }
        pub struct SpiDma<'a, M>(PhantomData<(&'a (), M)>);
        fn write(b: &[u8]) -> Result<(), ()> {
            trace(format!("write:{b:?}"));
            if with(|s| s.fail_write) {
                Err(())
            } else {
                Ok(())
            }
        }
        impl<M> SpiDma<'_, M> {
            pub fn with_buffers(self, _: (), _: ()) -> Self {
                self
            }
            pub fn apply_config(&mut self, cfg: &Config) -> Result<(), ()> {
                trace(format!("clock:{}", cfg.0));
                Ok(())
            }
        }
        #[derive(Debug)]
        pub struct Error;
        impl embedded_hal::spi::Error for Error {
            fn kind(&self) -> embedded_hal::spi::ErrorKind {
                embedded_hal::spi::ErrorKind::Other
            }
        }
        impl<M> embedded_hal::spi::ErrorType for SpiDma<'_, M> {
            type Error = Error;
        }
        impl<M> embedded_hal::spi::SpiBus for SpiDma<'_, M> {
            fn read(&mut self, _: &mut [u8]) -> Result<(), Error> {
                Ok(())
            }
            fn write(&mut self, b: &[u8]) -> Result<(), Error> {
                write(b).map_err(|_| Error)
            }
            fn transfer(&mut self, _: &mut [u8], b: &[u8]) -> Result<(), Error> {
                self.write(b)
            }
            fn transfer_in_place(&mut self, _: &mut [u8]) -> Result<(), Error> {
                Ok(())
            }
            fn flush(&mut self) -> Result<(), Error> {
                trace("flush".into());
                Ok(())
            }
        }
    }
}

#[macro_export]
macro_rules! dma_rx_buffer {
    ($n:expr) => {
        Ok::<(), ()>(())
    };
}
#[macro_export]
macro_rules! dma_tx_buffer {
    ($n:expr) => {
        Ok::<(), ()>(())
    };
}
