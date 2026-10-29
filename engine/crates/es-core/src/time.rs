//! Engine time. All trading logic uses [`Ts`], never the wall clock, so a
//! replay produces exactly the same decisions every time.

use serde::{Deserialize, Serialize};
use std::fmt;

/// India Standard Time offset from UTC, in seconds (+05:30).
pub const IST_OFFSET_SECS: i64 = 19_800;

const NS_PER_SEC: i64 = 1_000_000_000;
const NS_PER_MS: i64 = 1_000_000;

/// Nanoseconds since the Unix epoch (UTC).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Ts(pub i64);

impl Ts {
    pub const fn from_secs(s: i64) -> Ts {
        Ts(s * NS_PER_SEC)
    }

    pub const fn from_millis(ms: i64) -> Ts {
        Ts(ms * NS_PER_MS)
    }

    pub const fn secs(self) -> i64 {
        self.0.div_euclid(NS_PER_SEC)
    }

    pub const fn millis(self) -> i64 {
        self.0.div_euclid(NS_PER_MS)
    }

    pub const fn plus_ms(self, ms: i64) -> Ts {
        Ts(self.0 + ms * NS_PER_MS)
    }

    /// Milliseconds elapsed from `earlier` to `self` (negative if earlier is later).
    pub const fn ms_since(self, earlier: Ts) -> i64 {
        (self.0 - earlier.0) / NS_PER_MS
    }

    /// Calendar date in IST.
    pub fn ist_date(self) -> Date {
        let local = self.secs() + IST_OFFSET_SECS;
        Date::from_days(local.div_euclid(86_400))
    }

    /// Minutes since IST midnight (0..1440).
    pub fn ist_minute_of_day(self) -> u32 {
        let local = self.secs() + IST_OFFSET_SECS;
        (local.rem_euclid(86_400) / 60) as u32
    }

    /// Seconds since IST midnight.
    pub fn ist_second_of_day(self) -> u32 {
        let local = self.secs() + IST_OFFSET_SECS;
        local.rem_euclid(86_400) as u32
    }

    /// The timestamp for an IST wall time on a date.
    pub fn at_ist(date: Date, hour: u32, minute: u32, second: u32) -> Ts {
        let secs = date.days() * 86_400 + (hour * 3600 + minute * 60 + second) as i64
            - IST_OFFSET_SECS;
        Ts::from_secs(secs)
    }
}

/// A calendar date (proleptic Gregorian).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Date {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid date: {0}")]
pub struct DateParseError(pub String);

impl Date {
    pub fn new(year: i32, month: u8, day: u8) -> Option<Date> {
        let d = Date { year, month, day };
        if month == 0 || month > 12 || day == 0 || day > d.days_in_month() {
            return None;
        }
        Some(d)
    }

    fn days_in_month(self) -> u8 {
        match self.month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            _ => {
                let y = self.year;
                if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                    29
                } else {
                    28
                }
            }
        }
    }

    /// Parses `YYYY-MM-DD` (the instrument master format).
    pub fn parse(s: &str) -> Result<Date, DateParseError> {
        let err = || DateParseError(s.to_string());
        let mut it = s.trim().splitn(3, '-');
        let y = it.next().and_then(|v| v.parse().ok()).ok_or_else(err)?;
        let m = it.next().and_then(|v| v.parse().ok()).ok_or_else(err)?;
        let d = it.next().and_then(|v| v.parse().ok()).ok_or_else(err)?;
        Date::new(y, m, d).ok_or_else(err)
    }

    /// Days since 1970-01-01 (Howard Hinnant's days_from_civil).
    pub fn days(self) -> i64 {
        let y = self.year as i64 - if self.month <= 2 { 1 } else { 0 };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = self.month as i64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + self.day as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn from_days(z: i64) -> Date {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let year = (y + if m <= 2 { 1 } else { 0 }) as i32;
        Date { year, month: m, day: d }
    }

    /// 0 = Monday ... 6 = Sunday.
    pub fn weekday(self) -> u8 {
        // 1970-01-01 was a Thursday (3).
        ((self.days() + 3).rem_euclid(7)) as u8
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_round_trip() {
        let d = Date::parse("2026-11-03").unwrap();
        assert_eq!(Date::from_days(d.days()), d);
        assert_eq!(d.to_string(), "2026-11-03");
        assert_eq!(d.weekday(), 1); // Tuesday
        assert_eq!(Date::parse("1970-01-01").unwrap().days(), 0);
        assert!(Date::parse("2026-02-30").is_err());
        assert!(Date::parse("nope").is_err());
    }

    #[test]
    fn ist_conversion() {
        let d = Date::parse("2026-11-03").unwrap();
        let t = Ts::at_ist(d, 9, 15, 0);
        assert_eq!(t.ist_date(), d);
        assert_eq!(t.ist_minute_of_day(), 9 * 60 + 15);
        // 09:15 IST is 03:45 UTC
        assert_eq!(t.secs().rem_euclid(86_400), 3 * 3600 + 45 * 60);
        // 23:59 IST on the same date is still the same IST date
        assert_eq!(Ts::at_ist(d, 23, 59, 0).ist_date(), d);
    }
}
