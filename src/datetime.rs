//! Just enough calendar arithmetic to date a typing test.
//!
//! Results are timestamped in *local* time rather than UTC: "days practised"
//! and "streak" are about the typist's day, and an evening session should not
//! land on tomorrow. The standard library has no local time, so the conversion
//! goes through `localtime_r`; the day arithmetic on top of it is Howard
//! Hinnant's civil-date algorithm, which needs no timezone database.

/// A calendar date, with no time and no zone attached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    /// Days since 1970-01-01.
    pub fn to_days(self) -> i64 {
        let year = if self.month <= 2 {
            self.year - 1
        } else {
            self.year
        } as i64;
        let era = if year >= 0 { year } else { year - 399 } / 400;
        let year_of_era = year - era * 400;
        let shifted_month = ((self.month as i64) + 9) % 12;
        let day_of_year = (153 * shifted_month + 2) / 5 + self.day as i64 - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;

        era * 146_097 + day_of_era - 719_468
    }

    /// Whole days from `other` to `self`, positive when `self` is later.
    pub fn diff(self, other: Date) -> i64 {
        self.to_days() - other.to_days()
    }
}

/// The local date and time right now, as `(date, hour, minute, second)`.
pub fn local_now() -> (Date, u32, u32, u32) {
    // SAFETY: `time` with a null argument returns the current epoch seconds and
    // writes nothing; `localtime_r` fills a caller-owned `tm` and, unlike
    // `localtime`, keeps no shared state.
    unsafe {
        let epoch_seconds = libc::time(std::ptr::null_mut());
        let mut parts: libc::tm = std::mem::zeroed();
        libc::localtime_r(&epoch_seconds, &mut parts);

        (
            Date {
                year: parts.tm_year + 1900,
                month: parts.tm_mon as u32 + 1,
                day: parts.tm_mday as u32,
            },
            parts.tm_hour as u32,
            parts.tm_min as u32,
            parts.tm_sec as u32,
        )
    }
}

/// Today's local date.
pub fn today() -> Date {
    local_now().0
}

/// The current local time as an ISO 8601 string, e.g. `2026-08-31T09:15:00`.
pub fn timestamp() -> String {
    let (date, hour, minute, second) = local_now();

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        date.year, date.month, date.day, hour, minute, second
    )
}

/// The date part of an ISO 8601 timestamp, or `None` if it cannot be read.
pub fn parse_date(text: &str) -> Option<Date> {
    let date = text.split('T').next()?;
    let mut parts = date.split('-');

    let year: i32 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;

    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    Some(Date { year, month, day })
}

/// An ISO 8601 timestamp for local midnight `days` ago. Used to date test
/// fixtures the way a real session would be dated.
#[cfg(test)]
pub fn days_ago(days: i64) -> String {
    let date = from_days(today().to_days() - days);

    format!(
        "{:04}-{:02}-{:02}T12:00:00",
        date.year, date.month, date.day
    )
}

/// The inverse of [`Date::to_days`].
#[cfg(test)]
pub fn from_days(days: i64) -> Date {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };

    Date {
        year: (year + i64::from(month <= 2)) as i32,
        month: month as u32,
        day: day as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_epoch_is_day_zero() {
        assert_eq!(
            Date {
                year: 1970,
                month: 1,
                day: 1
            }
            .to_days(),
            0
        );
    }

    #[test]
    fn days_advance_one_at_a_time() {
        let first = Date {
            year: 2026,
            month: 8,
            day: 31,
        };
        let second = Date {
            year: 2026,
            month: 9,
            day: 1,
        };

        assert_eq!(second.diff(first), 1);
    }

    #[test]
    fn diff_spans_a_leap_day() {
        let before = Date {
            year: 2024,
            month: 2,
            day: 28,
        };
        let after = Date {
            year: 2024,
            month: 3,
            day: 1,
        };

        assert_eq!(after.diff(before), 2);
    }

    #[test]
    fn diff_spans_a_year_boundary() {
        let before = Date {
            year: 2025,
            month: 12,
            day: 31,
        };
        let after = Date {
            year: 2026,
            month: 1,
            day: 1,
        };

        assert_eq!(after.diff(before), 1);
    }

    #[test]
    fn a_century_that_is_not_a_leap_year_is_handled() {
        let before = Date {
            year: 1900,
            month: 2,
            day: 28,
        };
        let after = Date {
            year: 1900,
            month: 3,
            day: 1,
        };

        assert_eq!(after.diff(before), 1);
    }

    #[test]
    fn days_round_trip_through_dates() {
        for offset in -2000..2000 {
            let days = today().to_days() + offset;
            assert_eq!(from_days(days).to_days(), days);
        }
    }

    #[test]
    fn timestamps_parse_back_to_their_date() {
        assert_eq!(
            parse_date("2026-08-31T09:15:00"),
            Some(Date {
                year: 2026,
                month: 8,
                day: 31
            })
        );
    }

    #[test]
    fn a_bare_date_parses_too() {
        assert_eq!(
            parse_date("2026-08-31"),
            Some(Date {
                year: 2026,
                month: 8,
                day: 31
            })
        );
    }

    #[test]
    fn nonsense_does_not_parse() {
        assert_eq!(parse_date("this is not a date"), None);
        assert_eq!(parse_date("2026-13-01"), None);
        assert_eq!(parse_date(""), None);
    }

    #[test]
    fn the_current_timestamp_round_trips() {
        assert_eq!(parse_date(&timestamp()), Some(today()));
    }

    #[test]
    fn days_ago_counts_backwards_from_today() {
        assert_eq!(parse_date(&days_ago(0)), Some(today()));
        assert_eq!(today().diff(parse_date(&days_ago(3)).unwrap()), 3);
    }
}
