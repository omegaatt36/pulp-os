//! Host seam: ADC active-channel semantics mirror esp-hal 1.2.0 riscv.rs.
//! SPI records clock/CS/transfer order; no silicon timing claims.
use std::{cell::RefCell, marker::PhantomData};
#[derive(Clone, Copy)]
pub struct Blocking;
thread_local! {
    static ADC_READY: RefCell<bool> = const { RefCell::new(false) };
    static TRACE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static FAIL_WRITE: RefCell<bool> = const { RefCell::new(false) };
}
pub fn adc_ready(ready: bool) {
    ADC_READY.with(|v| *v.borrow_mut() = ready);
}
pub fn take_trace() -> Vec<String> {
    TRACE.with(|v| std::mem::take(&mut *v.borrow_mut()))
}
pub fn fail_write(fail: bool) {
    FAIL_WRITE.with(|v| *v.borrow_mut() = fail);
}
fn trace(s: String) {
    TRACE.with(|v| v.borrow_mut().push(s));
}
pub mod peripherals {
    use super::PhantomData;
    macro_rules! peripheral { ($($p:ident),*) => { $(pub struct $p<'a>(pub PhantomData<&'a ()>);)* }; }
    peripheral!(
        ADC1, GPIO4, GPIO5, GPIO22, GPIO23, GPIO24, GPIO25, GPIO26, SPI2, DMA_CH0
    );
}
pub mod analog {
    pub mod adc {
        use super::super::*;
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
                if !ADC_READY.with(|v| *v.borrow()) {
                    return Err(nb::Error::WouldBlock);
                }
                self.active = None;
                Ok(if channel == 2 { 1200 } else { 1800 })
            }
        }
    }
}
pub mod gpio {
    use super::*;
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
    pub struct Rate(pub u32);
    impl Rate {
        pub fn from_khz(khz: u32) -> Self {
            Self(khz)
        }
    }
}
pub mod delay {
    #[derive(Clone, Copy)]
    pub struct Delay;
    impl Delay {
        pub fn new() -> Self {
            Self
        }
    }
    impl embedded_hal::delay::DelayNs for Delay {
        fn delay_ns(&mut self, _: u32) {}
    }
}
pub mod spi {
    pub mod master {
        use super::super::*;
        #[derive(Default)]
        pub struct Config(pub u32);
        impl Config {
            pub fn with_frequency(self, r: time::Rate) -> Self {
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
            if FAIL_WRITE.with(|v| *v.borrow()) {
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
