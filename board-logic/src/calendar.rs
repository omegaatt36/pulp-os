//! Local calendar time seeded from FAT metadata or explicitly set by a browser.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl DateTime {
    pub fn new(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> Option<Self> {
        if !(1980..=2107).contains(&year)
            || !(1..=12).contains(&month)
            || day == 0
            || day > days_in_month(year, month)
            || hour >= 24
            || minute >= 60
            || second >= 60
        {
            return None;
        }
        Some(Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
        })
    }

    pub fn add_seconds(self, seconds: u64) -> Option<Self> {
        let mut value = Self::new(
            self.year,
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
        )?;
        let within_day =
            u64::from(self.hour) * 3600 + u64::from(self.minute) * 60 + u64::from(self.second);
        let total = within_day.checked_add(seconds)?;
        let mut days = total / 86_400;
        // Even the entire FAT date range is shorter than this. Bound work for arbitrary uptime.
        if days > 128 * 366 {
            return None;
        }
        let within_day = total % 86_400;
        value.hour = (within_day / 3600) as u8;
        value.minute = (within_day % 3600 / 60) as u8;
        value.second = (within_day % 60) as u8;
        while days > 0 {
            let remaining = u64::from(days_in_month(value.year, value.month) - value.day);
            if days <= remaining {
                value.day += days as u8;
                break;
            }
            days -= remaining + 1;
            value.day = 1;
            if value.month == 12 {
                if value.year == 2107 {
                    return None;
                }
                value.year += 1;
                value.month = 1;
            } else {
                value.month += 1;
            }
        }
        Some(value)
    }

    pub fn parse_local(input: &[u8]) -> Option<Self> {
        if input.len() != 19
            || input[4] != b'-'
            || input[7] != b'-'
            || input[10] != b'T'
            || input[13] != b':'
            || input[16] != b':'
        {
            return None;
        }
        fn number(input: &[u8]) -> Option<u16> {
            input.iter().try_fold(0, |value, digit| {
                digit
                    .is_ascii_digit()
                    .then(|| value * 10 + u16::from(digit - b'0'))
            })
        }
        Self::new(
            number(&input[..4])?,
            number(&input[5..7])? as u8,
            number(&input[8..10])? as u8,
            number(&input[11..13])? as u8,
            number(&input[14..16])? as u8,
            number(&input[17..19])? as u8,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Unknown,
    FatMtime,
    Browser,
}

pub struct Clock {
    seed: Option<(DateTime, u64)>,
    source: Source,
}

impl Clock {
    pub const fn new() -> Self {
        Self {
            seed: None,
            source: Source::Unknown,
        }
    }

    pub fn now(&self, uptime_seconds: u64) -> Option<DateTime> {
        let (seed, seeded_at) = self.seed?;
        seed.add_seconds(uptime_seconds.checked_sub(seeded_at)?)
    }

    pub fn source(&self) -> Source {
        self.source
    }

    pub fn observe_mtime(&mut self, candidate: DateTime, uptime_seconds: u64) {
        if self.source == Source::Browser
            || candidate.add_seconds(0).is_none()
            || candidate
                == (DateTime {
                    year: 1980,
                    month: 1,
                    day: 1,
                    hour: 0,
                    minute: 0,
                    second: 0,
                })
        {
            return;
        }
        if self.seed.is_some() {
            let Some(current) = self.now(uptime_seconds) else {
                return;
            };
            if candidate <= current {
                return;
            }
        }
        self.seed = Some((candidate, uptime_seconds));
        self.source = Source::FatMtime;
    }

    pub fn calibrate(&mut self, date: DateTime, uptime_seconds: u64) {
        if date.add_seconds(0).is_none() {
            return;
        }
        self.seed = Some((date, uptime_seconds));
        self.source = Source::Browser;
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> DateTime {
        DateTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
        }
    }

    #[test]
    fn validates_gregorian_dates_in_fat_range() {
        for value in [
            date(1980, 1, 1, 0, 0, 0),
            date(2000, 2, 29, 23, 59, 59),
            date(2107, 12, 31, 23, 59, 59),
        ] {
            assert_eq!(
                DateTime::new(
                    value.year,
                    value.month,
                    value.day,
                    value.hour,
                    value.minute,
                    value.second
                ),
                Some(value)
            );
        }
        for value in [
            date(1979, 12, 31, 0, 0, 0),
            date(2108, 1, 1, 0, 0, 0),
            date(2100, 2, 29, 0, 0, 0),
            date(2025, 2, 29, 0, 0, 0),
            date(2026, 4, 31, 0, 0, 0),
            date(2026, 0, 1, 0, 0, 0),
            date(2026, 13, 1, 0, 0, 0),
            date(2026, 1, 0, 0, 0, 0),
            date(2026, 1, 1, 24, 0, 0),
            date(2026, 1, 1, 0, 60, 0),
            date(2026, 1, 1, 0, 0, 60),
        ] {
            assert_eq!(
                DateTime::new(
                    value.year,
                    value.month,
                    value.day,
                    value.hour,
                    value.minute,
                    value.second
                ),
                None
            );
        }
    }

    #[test]
    fn advances_across_midnight_leap_day_and_year() {
        assert_eq!(
            date(2026, 10, 12, 23, 59, 59).add_seconds(1),
            Some(date(2026, 10, 13, 0, 0, 0))
        );
        assert_eq!(
            date(2000, 2, 28, 23, 59, 59).add_seconds(1),
            Some(date(2000, 2, 29, 0, 0, 0))
        );
        assert_eq!(
            date(2100, 2, 28, 23, 59, 59).add_seconds(1),
            Some(date(2100, 3, 1, 0, 0, 0))
        );
        assert_eq!(
            date(2026, 12, 31, 23, 59, 59).add_seconds(1),
            Some(date(2027, 1, 1, 0, 0, 0))
        );
        assert_eq!(
            date(1980, 1, 1, 0, 0, 0).add_seconds(86_400 * 366),
            Some(date(1981, 1, 1, 0, 0, 0))
        );
    }

    #[test]
    fn rejects_range_overflow_and_enormous_advances() {
        assert_eq!(
            date(2107, 12, 31, 23, 59, 59).add_seconds(0),
            Some(date(2107, 12, 31, 23, 59, 59))
        );
        assert_eq!(date(2107, 12, 31, 23, 59, 59).add_seconds(1), None);
        assert_eq!(date(1980, 1, 1, 0, 0, 0).add_seconds(u64::MAX), None);
        assert_eq!(date(2026, 2, 30, 0, 0, 0).add_seconds(0), None);
    }

    #[test]
    fn parses_only_strict_local_calendar_format() {
        assert_eq!(
            DateTime::parse_local(b"2026-10-12T23:59:58"),
            Some(date(2026, 10, 12, 23, 59, 58))
        );
        for input in [
            b"2026-10-12T23:59:58Z".as_slice(),
            b"2026-10-12 23:59:58",
            b"2026/10/12T23:59:58",
            b"2026-10-12T23-59:58",
            b"2026-10-12T23:59",
            b"2026-1a-12T23:59:58",
            b"2026-02-29T23:59:58",
            b"2108-01-01T00:00:00",
            b"",
        ] {
            assert_eq!(DateTime::parse_local(input), None);
        }
    }

    #[test]
    fn starts_unknown_and_ignores_fat_epoch() {
        let mut clock = Clock::new();
        assert_eq!(clock.now(0), None);
        assert_eq!(clock.source(), Source::Unknown);
        clock.observe_mtime(date(1980, 1, 1, 0, 0, 0), 10);
        assert_eq!(clock.now(20), None);
        clock.observe_mtime(date(1980, 1, 1, 0, 0, 2), 20);
        assert_eq!(clock.now(21), Some(date(1980, 1, 1, 0, 0, 3)));
        assert_eq!(clock.source(), Source::FatMtime);
    }

    #[test]
    fn mtime_can_only_advance_the_running_clock() {
        let mut clock = Clock::new();
        clock.observe_mtime(date(2026, 10, 12, 12, 0, 0), 10);
        clock.observe_mtime(date(2026, 10, 12, 12, 0, 5), 20);
        clock.observe_mtime(date(2026, 10, 12, 12, 0, 10), 20);
        assert_eq!(clock.now(30), Some(date(2026, 10, 12, 12, 0, 20)));
        clock.observe_mtime(date(2026, 10, 13, 0, 0, 0), 30);
        assert_eq!(clock.now(31), Some(date(2026, 10, 13, 0, 0, 1)));
    }

    #[test]
    fn browser_calibration_can_move_backward_and_ignores_mtimes() {
        let mut clock = Clock::new();
        clock.observe_mtime(date(2026, 10, 12, 0, 0, 0), 0);
        clock.calibrate(date(2025, 1, 1, 0, 0, 0), 10);
        clock.observe_mtime(date(2027, 1, 1, 0, 0, 0), 20);
        assert_eq!(clock.source(), Source::Browser);
        assert_eq!(clock.now(30), Some(date(2025, 1, 1, 0, 0, 20)));
        assert_eq!(clock.now(9), None);
    }

    #[test]
    fn invalid_public_fields_cannot_seed_or_replace_clock() {
        let invalid = date(2026, 2, 30, 0, 0, 0);
        assert_eq!(invalid.add_seconds(0), None);
        let mut clock = Clock::new();
        clock.observe_mtime(invalid, 0);
        assert_eq!(clock.source(), Source::Unknown);
        clock.calibrate(invalid, 0);
        assert_eq!(clock.source(), Source::Unknown);
        clock.observe_mtime(date(2026, 1, 1, 0, 0, 0), 0);
        clock.calibrate(invalid, 10);
        assert_eq!(clock.source(), Source::FatMtime);
        assert_eq!(clock.now(10), Some(date(2026, 1, 1, 0, 0, 10)));
    }
}
