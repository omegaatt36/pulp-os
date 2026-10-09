use super::State;
use crate::{fonts::cjk::measurement_log_us, kernel::uptime_us};

#[derive(Clone, Copy)]
pub(super) enum Phase {
    Text,
    Layout,
    Visible,
    Pause,
}

pub(super) struct Tick {
    at: u64,
    log_us: u32,
}

#[derive(Default)]
struct Accounting {
    us: [u64; 4],
    calls: [u32; 4],
}
impl Accounting {
    fn record(&mut self, phase: Phase, elapsed: u64, logging: u64) {
        let idx = phase as usize;
        self.us[idx] += elapsed.saturating_sub(logging);
        self.calls[idx] += 1;
    }

    fn other_us(&self, wall: u64, logging: u64) -> u64 {
        wall.saturating_sub(logging + self.us.iter().sum::<u64>())
    }
}

pub(super) struct Profile {
    began: Option<Tick>,
    ended: Option<u64>,
    step: u32,
    accounting: Accounting,
    own_log_us: u64,
    pub(super) sliced: bool,
}

impl Profile {
    pub(super) const fn new() -> Self {
        Self {
            began: None,
            ended: None,
            step: 0,
            accounting: Accounting {
                us: [0; 4],
                calls: [0; 4],
            },
            own_log_us: 0,
            sliced: false,
        }
    }

    pub(super) fn tick(&self) -> Tick {
        Tick {
            at: uptime_us(),
            log_us: measurement_log_us(),
        }
    }

    pub(super) fn begin(&mut self, state: State, resumed: bool, retained_window: bool) {
        let tick = self.tick();
        let (outside_us, outside_known) = self.outside_interval(tick.at);
        self.step = self.step.wrapping_add(1);
        self.accounting = Accounting::default();
        self.sliced = false;
        // The interval outside this call includes scheduler work, render/EPD
        // time and other logging. It is not font or text work of this step.
        log::info!(
            "reader-sd resume step={} state={:?} resumed={} retained_window={} outside_step_us={} outside_step_known={} previous_step_complete={}",
            self.step,
            state,
            resumed,
            retained_window,
            outside_us,
            outside_known,
            self.began.is_none(),
        );
        self.own_log_us = uptime_us().saturating_sub(tick.at);
        self.began = Some(tick);
    }

    fn outside_interval(&self, now: u64) -> (u64, bool) {
        // Cancellation leaves no completion boundary: the previous completed
        // step's end would also include work inside the abandoned call.
        match (self.began.is_none(), self.ended) {
            (true, Some(end)) => (now.saturating_sub(end), true),
            _ => (0, false),
        }
    }

    pub(super) fn record(&mut self, phase: Phase, tick: Tick) {
        if self.began.is_some() {
            self.accounting.record(
                phase,
                uptime_us().saturating_sub(tick.at),
                u64::from(measurement_log_us().wrapping_sub(tick.log_us)),
            );
        }
    }

    pub(super) fn finish(&mut self, state: State) {
        let Some(began) = self.began.take() else {
            return;
        };
        let wall = uptime_us().saturating_sub(began.at);
        let logging = self.own_log_us + u64::from(measurement_log_us().wrapping_sub(began.log_us));
        let outcome = if self.sliced {
            "slice"
        } else {
            match state {
                State::Ready => "ready",
                State::Error => "error",
                _ => "pending",
            }
        };
        log::info!(
            "reader-sd step={} outcome={} state={:?} wall_us={} text_us={} layout_us={} visible_us={} pause_us={} logging_us={} other_us={} text_calls={} layout_calls={} visible_calls={}",
            self.step,
            outcome,
            state,
            wall,
            self.accounting.us[0],
            self.accounting.us[1],
            self.accounting.us[2],
            self.accounting.us[3],
            logging,
            self.accounting.other_us(wall, logging),
            self.accounting.calls[0],
            self.accounting.calls[1],
            self.accounting.calls[2],
        );
        // Exclude this line's own cost from the next resume interval.
        self.ended = Some(uptime_us());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_are_disjoint_and_logging_is_counted_once() {
        let mut a = Accounting::default();
        a.record(Phase::Text, 120, 0);
        a.record(Phase::Text, 80, 0);
        a.record(Phase::Layout, 200, 30);
        a.record(Phase::Visible, 90, 10);
        a.record(Phase::Pause, 50, 0);
        assert_eq!(a.us, [200, 170, 80, 50]);
        assert_eq!(a.calls, [2, 1, 1, 1]);
        assert_eq!(a.other_us(650, 60), 90);
        assert_eq!(650, a.us.iter().sum::<u64>() + 60 + a.other_us(650, 60));
    }

    #[test]
    fn cancellation_does_not_report_abandoned_step_work_as_an_outside_interval() {
        let mut p = Profile::new();
        assert_eq!(p.outside_interval(100), (0, false));
        p.ended = Some(100);
        assert_eq!(p.outside_interval(200), (100, true));
        p.began = Some(Tick { at: 150, log_us: 0 });
        assert_eq!(p.outside_interval(300), (0, false));
        p.began = None;
        p.ended = Some(400);
        assert_eq!(p.outside_interval(450), (50, true));
    }
}
