// Idle sleep policy shared by the firmware task and host validation.

use core::future::Future;

use embassy_futures::select::{Either, Either3, select, select3};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer};

// Keep the clock as the only test seam: signals and the async policy below are
// the same ones used on-device, while host tests can advance time without waits.
pub trait IdleClock {
    fn now(&self) -> Instant;
    fn wait_until(&self, deadline: Instant) -> impl Future<Output = ()>;
}

pub struct EmbassyClock;

impl IdleClock for EmbassyClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn wait_until(&self, deadline: Instant) -> impl Future<Output = ()> {
        Timer::at(deadline)
    }
}

fn deadline(clock: &impl IdleClock, minutes: u16) -> Option<Instant> {
    (minutes != 0).then(|| clock.now() + Duration::from_secs(u64::from(minutes) * 60))
}

pub async fn run(
    clock: &impl IdleClock,
    timeout: &Signal<CriticalSectionRawMutex, u16>,
    reset: &Signal<CriticalSectionRawMutex, ()>,
    sleep_due: &Signal<CriticalSectionRawMutex, ()>,
) -> ! {
    let mut minutes = timeout.wait().await;
    let _ = reset.try_take();
    let mut sleep_at = deadline(clock, minutes);

    enum Event {
        Activity,
        Timeout(u16),
        Elapsed,
    }

    loop {
        let event = if let Some(at) = sleep_at {
            match select3(reset.wait(), timeout.wait(), clock.wait_until(at)).await {
                Either3::First(()) => Event::Activity,
                Either3::Second(new) => Event::Timeout(new),
                Either3::Third(()) => Event::Elapsed,
            }
        } else {
            match select(reset.wait(), timeout.wait()).await {
                Either::First(()) => Event::Activity,
                Either::Second(new) => Event::Timeout(new),
            }
        };

        match event {
            Event::Activity => sleep_at = deadline(clock, minutes),
            Event::Timeout(new) if new != minutes => {
                minutes = new;
                sleep_at = deadline(clock, minutes);
            }
            // Status polling republishes the setting every five seconds. Keep
            // its original deadline, or stay disarmed after issuing sleep.
            Event::Timeout(_) => {}
            Event::Elapsed => {
                sleep_due.signal(());
                sleep_at = None;
            }
        }
    }
}
