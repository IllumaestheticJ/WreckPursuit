//! Wall-clock source. Meta services are not simulation code, so they may read real time, but
//! every read goes through [`Clock`] so tests can drive expiry, rate limits and daily/weekly
//! leaderboard resets deterministically with [`ManualClock`].

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch (UTC).
pub type UnixMs = i64;

/// Source of the current time.
pub trait Clock: Send + Sync {
    /// Current time in Unix milliseconds.
    fn now_ms(&self) -> UnixMs;
}

/// The real system clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> UnixMs {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }
}

/// A clock that only moves when told to (tests).
#[derive(Debug, Default)]
pub struct ManualClock {
    now: AtomicI64,
}

impl ManualClock {
    /// A clock starting at `start_ms`.
    pub fn new(start_ms: UnixMs) -> Self {
        Self {
            now: AtomicI64::new(start_ms),
        }
    }

    /// Move the clock forward by `ms`.
    pub fn advance(&self, ms: i64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }

    /// Set the clock to `ms`.
    pub fn set(&self, ms: UnixMs) {
        self.now.store(ms, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> UnixMs {
        self.now.load(Ordering::SeqCst)
    }
}

/// Days since 1970-01-01 → (year, month 1..=12, day 1..=31), proleptic Gregorian.
/// Howard Hinnant's `civil_from_days`.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

const MS_PER_DAY: i64 = 86_400_000;

/// UTC day index (days since the epoch) of a timestamp.
pub fn day_index(ms: UnixMs) -> i64 {
    ms.div_euclid(MS_PER_DAY)
}

/// `YYYY-MM-DD` of a day index.
pub fn date_string(day: i64) -> String {
    let (y, m, d) = civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Leaderboard key of the UTC day containing `ms`: `d:YYYY-MM-DD`.
pub fn daily_period(ms: UnixMs) -> String {
    format!("d:{}", date_string(day_index(ms)))
}

/// Leaderboard key of the week (Monday 00:00 UTC start) containing `ms`: `w:YYYY-MM-DD` of the
/// Monday.
pub fn weekly_period(ms: UnixMs) -> String {
    let day = day_index(ms);
    // 1970-01-01 was a Thursday: (day + 3) mod 7 == 0 on Mondays.
    let monday = day - (day + 3).rem_euclid(7);
    format!("w:{}", date_string(monday))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        // 2026-10-19 is day 20_745.
        assert_eq!(date_string(20_745), "2026-10-19");
    }

    #[test]
    fn periods() {
        let mon = 20_745 * MS_PER_DAY; // 2026-10-19 is a Monday
        assert_eq!(daily_period(mon), "d:2026-10-19");
        assert_eq!(weekly_period(mon), "w:2026-10-19");
        assert_eq!(weekly_period(mon + 6 * MS_PER_DAY + 1), "w:2026-10-19");
        assert_eq!(weekly_period(mon + 7 * MS_PER_DAY), "w:2026-10-26");
        assert_eq!(weekly_period(mon - 1), "w:2026-10-12");
    }
}
