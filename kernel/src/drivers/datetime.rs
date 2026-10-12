// Local wall time. A FAT mtime is an estimate; browser calibration supplies
// an explicit date. Neither survives a reset as a calibrated clock.
use core::cell::RefCell;

use critical_section::Mutex;
use embedded_sdmmc::{TimeSource, Timestamp};
use pulp_board_logic::calendar::{Clock, DateTime, Source};

static CLOCK: Mutex<RefCell<Clock>> = Mutex::new(RefCell::new(Clock::new()));

fn uptime() -> u64 {
    embassy_time::Instant::now().as_secs()
}

pub(crate) fn current() -> Option<(DateTime, Source)> {
    critical_section::with(|cs| {
        let clock = CLOCK.borrow(cs).borrow();
        clock.now(uptime()).map(|date| (date, clock.source()))
    })
}

pub(crate) fn calibrate(date: DateTime) {
    critical_section::with(|cs| CLOCK.borrow(cs).borrow_mut().calibrate(date, uptime()));
}

pub(crate) fn observe_mtime(timestamp: Timestamp) {
    let Some(date) = DateTime::new(
        u16::from(timestamp.year_since_1970) + 1970,
        timestamp.zero_indexed_month.saturating_add(1),
        timestamp.zero_indexed_day.saturating_add(1),
        timestamp.hours,
        timestamp.minutes,
        timestamp.seconds,
    ) else {
        return;
    };
    critical_section::with(|cs| CLOCK.borrow(cs).borrow_mut().observe_mtime(date, uptime()));
}

pub(crate) struct LocalTimeSource;

impl TimeSource for LocalTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        let date = current().map(|(date, _)| date);
        // FAT cannot represent "unknown". Use its epoch on disk, while the
        // public date API returns None so callers cannot mistake it for today.
        match date {
            Some(date) => Timestamp {
                year_since_1970: (date.year - 1970) as u8,
                zero_indexed_month: date.month - 1,
                zero_indexed_day: date.day - 1,
                hours: date.hour,
                minutes: date.minute,
                seconds: date.second,
            },
            None => Timestamp {
                year_since_1970: 10,
                zero_indexed_month: 0,
                zero_indexed_day: 0,
                hours: 0,
                minutes: 0,
                seconds: 0,
            },
        }
    }
}
