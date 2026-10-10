// embassy spawned tasks: input polling, housekeeping, idle sleep

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Ticker, Timer};

#[cfg(feature = "board-x4")]
use crate::drivers::battery;
use crate::drivers::input::Event;
#[cfg(feature = "board-x4")]
use crate::drivers::input::InputDriver;

use super::timing;

pub const INPUT_CHANNEL_CAP: usize = 8;
pub static INPUT_EVENTS: Channel<CriticalSectionRawMutex, Event, INPUT_CHANNEL_CAP> =
    Channel::new();

// signal input_task to reset hold timers after a navigation event is consumed
pub static RESET_HOLD: Signal<CriticalSectionRawMutex, ()> = Signal::new();

#[inline]
pub fn request_hold_reset() {
    RESET_HOLD.signal(());
}

// wakes a parked scheduler loop (scheduler.rs `park`): signalled by the tasks
// below whenever they publish something the loop polls for. Input needs no
// signal, the loop already waits on INPUT_EVENTS
pub static LOOP_WAKE: Signal<CriticalSectionRawMutex, ()> = Signal::new();

pub static BATTERY_MV: Signal<CriticalSectionRawMutex, u16> = Signal::new();

#[cfg(feature = "board-x4")]
#[embassy_executor::task]
pub async fn input_task(mut input: InputDriver) -> ! {
    let mut battery_counter: u32 = 0;
    let mut idle_ticks: u32 = 0;

    let raw = input.read_battery_mv();
    BATTERY_MV.signal(battery::adc_to_battery_mv(raw));

    loop {
        // adaptive polling: fast rate during active input, slow when idle
        let tick_ms = if idle_ticks >= timing::INPUT_IDLE_TICKS {
            timing::INPUT_TICK_SLOW_MS
        } else {
            timing::INPUT_TICK_FAST_MS
        };
        Timer::after(Duration::from_millis(tick_ms)).await;

        if RESET_HOLD.try_take().is_some() {
            input.reset_hold_state();
        }

        if let Some(ev) = input.poll() {
            let _ = INPUT_EVENTS.try_send(ev);
            IDLE_RESET.signal(());
            idle_ticks = 0; // reset to fast polling on any event
        } else {
            idle_ticks = idle_ticks.saturating_add(1);
        }

        battery_counter += 1;
        if battery_counter >= timing::BATTERY_INTERVAL_TICKS {
            battery_counter = 0;
            let raw = input.read_battery_mv();
            BATTERY_MV.signal(battery::adc_to_battery_mv(raw));
        }
    }
}

// OnePage C61 input task: same adaptive polling (10 ms while active, 50 ms after
// INPUT_IDLE_TICKS quiet ticks) and the same channel / idle-reset / hold-reset
// signalling as the X4 task. Differences, all on purpose:
//   * the key driver is pulp_board_logic::keys::KeyInput (2.5 s startup grace,
//     BSP ladder windows) instead of the two X4 ladders + Power button;
//   * the battery is NOT read here: it is owned by the kernel
//     (`board_c61::hw::C61Hw::battery`) and measured from the housekeeping poll,
//     because the sleep sequence needs it too (see hw.rs);
//   * USB detect (GPIO11) is sampled every iteration (3-sample debounce)
//     and published through `USB_PLUGGED`; there is no USB UI in this version.
// the WAKE key (GPIO2) was held for `SLEEP_HOLD_US` and has been let go: sleep now.
// A signal, not an INPUT_EVENTS item: a blocking refresh can fill the channel and
// drop events, while a signal keeps its value until the main loop takes it
#[cfg(feature = "board-onepage-c61")]
pub static SLEEP_REQUESTED: Signal<CriticalSectionRawMutex, ()> = Signal::new();

#[cfg(feature = "board-onepage-c61")]
pub static USB_PLUGGED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

#[cfg(feature = "board-onepage-c61")]
#[embassy_executor::task]
pub async fn input_task(
    mut input: crate::board_c61::keys::C61Input,
    mut usb: crate::board_c61::usb::UsbPort,
) -> ! {
    use core::sync::atomic::Ordering;

    use crate::board_c61::usb::UsbEvent;

    let mut idle_ticks: u32 = 0;
    let mut sleep_hold = pulp_board_logic::keys::SleepHold::new();
    USB_PLUGGED.store(usb.plugged(), Ordering::Relaxed);

    loop {
        // adaptive polling: fast rate during active input, slow when idle
        let tick_ms = if idle_ticks >= timing::INPUT_IDLE_TICKS {
            timing::INPUT_TICK_SLOW_MS
        } else {
            timing::INPUT_TICK_FAST_MS
        };
        Timer::after(Duration::from_millis(tick_ms)).await;

        if RESET_HOLD.try_take().is_some() {
            input.reset_hold_state();
        }

        if let Some(ev) = input.poll() {
            let _ = INPUT_EVENTS.try_send(ev);
            IDLE_RESET.signal(());
            idle_ticks = 0; // reset to fast polling on any event
        } else {
            idle_ticks = idle_ticks.saturating_add(1);
        }

        // hold WAKE past `SLEEP_HOLD_US`, then let go: sleep (the 1 s long press
        // has already gone Home by then)
        if sleep_hold.update(input.wake_held_us()) {
            log::info!("input: WAKE held and released, sleep requested");
            SLEEP_REQUESTED.signal(());
            LOOP_WAKE.signal(());
        }

        if let Some(ev) = usb.poll() {
            USB_PLUGGED.store(ev == UsbEvent::Plugged, Ordering::Relaxed);
            log::info!("usb: {:?}", ev);
        }
    }
}

pub static STATUS_DUE: Signal<CriticalSectionRawMutex, ()> = Signal::new();
pub static SD_CHECK_DUE: Signal<CriticalSectionRawMutex, ()> = Signal::new();
pub static BOOKMARK_FLUSH_DUE: Signal<CriticalSectionRawMutex, ()> = Signal::new();

#[embassy_executor::task]
pub async fn housekeeping_task() -> ! {
    Timer::after(Duration::from_secs(timing::HOUSEKEEPING_INITIAL_DELAY_SECS)).await;

    let mut status_ticker = Ticker::every(Duration::from_secs(timing::STATUS_INTERVAL_SECS));
    let mut sd_ticker = Ticker::every(Duration::from_secs(timing::SD_CHECK_INTERVAL_SECS));

    Timer::after(Duration::from_secs(timing::BOOKMARK_FLUSH_STAGGER_SECS)).await;
    let mut bm_ticker = Ticker::every(Duration::from_secs(timing::BOOKMARK_FLUSH_INTERVAL_SECS));

    loop {
        use embassy_futures::select::{Either3, select3};

        match select3(status_ticker.next(), sd_ticker.next(), bm_ticker.next()).await {
            Either3::First(_) => STATUS_DUE.signal(()),
            Either3::Second(_) => SD_CHECK_DUE.signal(()),
            Either3::Third(_) => BOOKMARK_FLUSH_DUE.signal(()),
        }
        LOOP_WAKE.signal(());
    }
}

pub static IDLE_TIMEOUT_MINS: Signal<CriticalSectionRawMutex, u16> = Signal::new();
pub static IDLE_RESET: Signal<CriticalSectionRawMutex, ()> = Signal::new();
// the idle sleep is not signalled to LOOP_WAKE: the 5 s status tick above
// wakes the loop at least that often, which is plenty for a minutes-long timeout
pub static IDLE_SLEEP_DUE: Signal<CriticalSectionRawMutex, ()> = Signal::new();

#[inline]
pub fn set_idle_timeout(minutes: u16) {
    IDLE_TIMEOUT_MINS.signal(minutes);
}

#[embassy_executor::task]
pub async fn idle_timeout_task() -> ! {
    super::idle::run(
        &super::idle::EmbassyClock,
        &IDLE_TIMEOUT_MINS,
        &IDLE_RESET,
        &IDLE_SLEEP_DUE,
    )
    .await
}
