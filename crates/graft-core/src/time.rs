//! Persisted timestamps.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// A UTC instant with millisecond precision, stored as milliseconds since
/// the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

impl Timestamp {
    /// The current time. Clocks before 1970 are clamped to the epoch.
    #[must_use]
    pub fn now() -> Self {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        Self(ms)
    }

    /// Wraps milliseconds since the Unix epoch.
    #[must_use]
    pub const fn from_millis(ms: i64) -> Self {
        Self(ms)
    }

    /// Milliseconds since the Unix epoch.
    #[must_use]
    pub const fn as_millis(self) -> i64 {
        self.0
    }
}

/// Converts days since 1970-01-01 to a proleptic Gregorian (year, month,
/// day). Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    // day is 1..=31 and month 1..=12 by construction.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (year, month as u32, day as u32)
}

impl fmt::Display for Timestamp {
    /// RFC 3339 in UTC, e.g. `2026-10-31T10:32:58.123Z`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ms = self.0.rem_euclid(1000);
        let secs = self.0.div_euclid(1000);
        let (year, month, day) = civil_from_days(secs.div_euclid(86_400));
        let sod = secs.rem_euclid(86_400);
        let (hour, minute, second) = (sod / 3600, sod % 3600 / 60, sod % 60);
        write!(
            f,
            "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{ms:03}Z"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_rfc3339_utc() {
        assert_eq!(
            Timestamp::from_millis(0).to_string(),
            "1970-01-01T00:00:00.000Z"
        );
        assert_eq!(
            Timestamp::from_millis(1_793_442_778_123).to_string(),
            "2026-10-31T10:32:58.123Z"
        );
        // Leap day.
        assert_eq!(
            Timestamp::from_millis(951_782_400_000).to_string(),
            "2000-02-29T00:00:00.000Z"
        );
        assert_eq!(
            Timestamp::from_millis(-1).to_string(),
            "1969-12-31T23:59:59.999Z"
        );
    }

    #[test]
    fn now_is_after_2026() {
        assert!(Timestamp::now() > Timestamp::from_millis(1_767_225_600_000));
    }
}
