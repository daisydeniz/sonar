//! A calendar date (UTC, no time of day) for license issue/expiry.
//!
//! Stored as days since 1970-01-01 so comparing and arithmetic are trivial;
//! the civil-calendar conversions are Howard Hinnant's public-domain
//! `days_from_civil` / `civil_from_days` algorithms. Serialized as the string
//! `YYYY-MM-DD`.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::LicenseError;

const SECS_PER_DAY: i64 = 86_400;

/// A UTC calendar date, ordered chronologically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date(i64);

impl Date {
    /// The date containing unix time `secs` (UTC; negative times floor).
    pub fn from_unix(secs: i64) -> Self {
        Self(secs.div_euclid(SECS_PER_DAY))
    }

    /// Builds a date from year/month/day; `None` if it does not exist
    /// (month 13, 30 February, year outside 1..=9999, ...).
    pub fn from_ymd(y: i64, m: u32, d: u32) -> Option<Self> {
        if !(1..=9999).contains(&y) || !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) {
            return None;
        }
        Some(Self(days_from_civil(y, m, d)))
    }

    /// Parses exactly `YYYY-MM-DD` (four-digit year, two-digit month/day).
    ///
    /// # Errors
    /// [`LicenseError::Malformed`] for any other shape or a non-existent date.
    pub fn parse(s: &str) -> Result<Self, LicenseError> {
        let bad = || LicenseError::Malformed {
            reason: format!("'{s}' is not a date (expected YYYY-MM-DD)"),
        };
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return Err(bad());
        }
        let num = |r: std::ops::Range<usize>| -> Option<u32> {
            let part = s.get(r)?;
            part.bytes()
                .all(|c| c.is_ascii_digit())
                .then(|| part.parse().ok())
                .flatten()
        };
        let (y, m, d) = (num(0..4), num(5..7), num(8..10));
        match (y, m, d) {
            (Some(y), Some(m), Some(d)) => Self::from_ymd(i64::from(y), m, d).ok_or_else(bad),
            _ => Err(bad()),
        }
    }

    /// Unix time (seconds) of 00:00:00 UTC of this date.
    pub fn start_unix(self) -> i64 {
        self.0 * SECS_PER_DAY
    }

    fn ymd(self) -> (i64, u32, u32) {
        civil_from_days(self.0)
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (y, m, d) = self.ymd();
        write!(f, "{y:04}-{m:02}-{d:02}")
    }
}

impl Serialize for Date {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Date {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        2 if is_leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_and_known_dates() {
        assert_eq!(Date::from_unix(0).to_string(), "1970-01-01");
        assert_eq!(Date::from_unix(-1).to_string(), "1969-12-31");
        assert_eq!(Date::from_unix(1_709_164_800).to_string(), "2024-02-29");
        assert_eq!(
            Date::parse("2024-02-29").expect("date").start_unix(),
            1_709_164_800
        );
    }

    #[test]
    fn parse_round_trips_and_rejects_nonsense() {
        for s in ["2026-11-14", "2000-02-29", "9999-12-31", "0001-01-01"] {
            let d = Date::parse(s).expect(s);
            assert_eq!(d.to_string(), s);
            assert_eq!(Date::from_unix(d.start_unix()), d);
        }
        for s in [
            "",
            "2026-1-14",
            "2026-13-01",
            "2026-02-30",
            "2023-02-29",
            "1900-02-29",
            "2026/11/14",
            "2026-11-14 ",
            "+026-11-14",
            "2026-11-1x",
            "٢٠٢٦-11-14",
            "0000-01-01",
        ] {
            assert!(Date::parse(s).is_err(), "{s:?}");
        }
    }

    #[test]
    fn ordering_is_chronological() {
        let a = Date::parse("2026-12-31").expect("a");
        let b = Date::parse("2027-01-01").expect("b");
        assert!(a < b);
        assert_eq!(b.start_unix() - a.start_unix(), 86_400);
    }
}
