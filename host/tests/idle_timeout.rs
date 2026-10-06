// Exercise the exact production async idle loop with real Embassy signals.
// Only the clock is virtual; each poll is bounded, so regressions cannot hang
// this suite or require minute-long wall-clock waits.

use std::cell::Cell;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::Instant;
use pulp_host::kernel::idle::{self, IdleClock};

#[derive(Default)]
struct VirtualClock {
    seconds: Cell<u64>,
}

impl VirtualClock {
    fn advance_to(&self, seconds: u64) {
        assert!(seconds >= self.seconds.get());
        self.seconds.set(seconds);
    }
}

impl IdleClock for VirtualClock {
    fn now(&self) -> Instant {
        Instant::from_secs(self.seconds.get())
    }

    fn wait_until(&self, deadline: Instant) -> impl Future<Output = ()> {
        poll_fn(move |_| {
            if self.now() >= deadline {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
    }
}

#[derive(Default)]
struct Signals {
    timeout: Signal<CriticalSectionRawMutex, u16>,
    reset: Signal<CriticalSectionRawMutex, ()>,
    sleep_due: Signal<CriticalSectionRawMutex, ()>,
}

// Drive a single executor turn after signalling or advancing the virtual clock.
// The never-ending production task must always settle at a pending wait.
fn poll_task(task: Pin<&mut impl Future>) {
    let mut context = Context::from_waker(Waker::noop());
    assert!(task.poll(&mut context).is_pending());
}

#[test]
fn periodic_status_notifications_preserve_deadline_and_do_not_repeat_sleep() {
    let clock = VirtualClock::default();
    let signals = Signals::default();
    let mut task = std::pin::pin!(idle::run(
        &clock,
        &signals.timeout,
        &signals.reset,
        &signals.sleep_due,
    ));

    // The task waits for the initial setting, even if time or input arrives.
    signals.reset.signal(());
    poll_task(task.as_mut());
    signals.timeout.signal(1);
    poll_task(task.as_mut());

    for seconds in (5..60).step_by(5) {
        clock.advance_to(seconds);
        signals.timeout.signal(1); // scheduler STATUS_DUE every five seconds
        poll_task(task.as_mut());
        assert!(signals.sleep_due.try_take().is_none(), "at {seconds}s");
    }

    // A same-value update coinciding with expiry must not postpone sleep.
    clock.advance_to(60);
    signals.timeout.signal(1);
    poll_task(task.as_mut());
    assert_eq!(signals.sleep_due.try_take(), Some(()));

    for seconds in (65..=180).step_by(5) {
        clock.advance_to(seconds);
        signals.timeout.signal(1);
        poll_task(task.as_mut());
        assert!(signals.sleep_due.try_take().is_none(), "at {seconds}s");
    }
}

#[test]
fn activity_restarts_timer_while_periodic_settings_continue() {
    let clock = VirtualClock::default();
    let signals = Signals::default();
    signals.timeout.signal(1);
    let mut task = std::pin::pin!(idle::run(
        &clock,
        &signals.timeout,
        &signals.reset,
        &signals.sleep_due,
    ));
    poll_task(task.as_mut());

    clock.advance_to(55);
    signals.reset.signal(());
    signals.timeout.signal(1);
    poll_task(task.as_mut());
    for seconds in (60..115).step_by(5) {
        clock.advance_to(seconds);
        signals.timeout.signal(1);
        poll_task(task.as_mut());
        assert!(signals.sleep_due.try_take().is_none(), "at {seconds}s");
    }
    clock.advance_to(115);
    poll_task(task.as_mut());
    assert_eq!(signals.sleep_due.try_take(), Some(()));
}

#[test]
fn changed_setting_starts_full_new_interval() {
    for (initial, changed, expected) in [(1, 2, 170), (2, 1, 110)] {
        let clock = VirtualClock::default();
        let signals = Signals::default();
        signals.timeout.signal(initial);
        let mut task = std::pin::pin!(idle::run(
            &clock,
            &signals.timeout,
            &signals.reset,
            &signals.sleep_due,
        ));
        poll_task(task.as_mut());

        clock.advance_to(50);
        signals.timeout.signal(changed);
        poll_task(task.as_mut());
        for seconds in (55..expected).step_by(5) {
            clock.advance_to(seconds);
            signals.timeout.signal(changed);
            poll_task(task.as_mut());
            assert!(signals.sleep_due.try_take().is_none(), "at {seconds}s");
        }
        clock.advance_to(expected);
        poll_task(task.as_mut());
        assert_eq!(signals.sleep_due.try_take(), Some(()));
    }
}

#[test]
fn zero_cancels_timer_and_reenabling_starts_new_interval() {
    let clock = VirtualClock::default();
    let signals = Signals::default();
    signals.timeout.signal(1);
    let mut task = std::pin::pin!(idle::run(
        &clock,
        &signals.timeout,
        &signals.reset,
        &signals.sleep_due,
    ));
    poll_task(task.as_mut());

    clock.advance_to(50);
    signals.timeout.signal(0);
    poll_task(task.as_mut());
    clock.advance_to(60);
    poll_task(task.as_mut());
    assert!(signals.sleep_due.try_take().is_none());

    clock.advance_to(3600);
    signals.reset.signal(());
    signals.timeout.signal(0);
    poll_task(task.as_mut());
    assert!(signals.sleep_due.try_take().is_none());

    signals.timeout.signal(1);
    poll_task(task.as_mut());
    clock.advance_to(3659);
    poll_task(task.as_mut());
    assert!(signals.sleep_due.try_take().is_none());
    clock.advance_to(3660);
    poll_task(task.as_mut());
    assert_eq!(signals.sleep_due.try_take(), Some(()));
}

#[test]
fn sleep_abort_reset_and_changed_setting_rearm_after_expiry() {
    let clock = VirtualClock::default();
    let signals = Signals::default();
    signals.timeout.signal(1);
    let mut task = std::pin::pin!(idle::run(
        &clock,
        &signals.timeout,
        &signals.reset,
        &signals.sleep_due,
    ));
    poll_task(task.as_mut());
    clock.advance_to(60);
    poll_task(task.as_mut());
    assert_eq!(signals.sleep_due.try_take(), Some(()));

    // Scheduler sleep-abort signalling uses the same reset as user activity.
    clock.advance_to(90);
    signals.reset.signal(());
    poll_task(task.as_mut());
    clock.advance_to(149);
    signals.timeout.signal(1);
    poll_task(task.as_mut());
    assert!(signals.sleep_due.try_take().is_none());
    clock.advance_to(150);
    poll_task(task.as_mut());
    assert_eq!(signals.sleep_due.try_take(), Some(()));

    clock.advance_to(160);
    signals.timeout.signal(2);
    poll_task(task.as_mut());
    clock.advance_to(279);
    poll_task(task.as_mut());
    assert!(signals.sleep_due.try_take().is_none());
    clock.advance_to(280);
    poll_task(task.as_mut());
    assert_eq!(signals.sleep_due.try_take(), Some(()));
}
