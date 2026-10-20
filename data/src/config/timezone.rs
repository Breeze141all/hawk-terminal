use std::fmt;

use chrono::{DateTime, Datelike, Months, TimeZone};
use serde::{Deserialize, Serialize};

const ORIGIN_YEAR: i32 = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum UserTimezone {
    #[default]
    Utc,
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeLabelKind<'a> {
    Axis { timeframe: exchange::Timeframe },
    Crosshair { show_millis: bool },
    Custom(&'a str),
}

impl UserTimezone {
    /// Formats a Unix timestamp (milliseconds) according to the kind.
    pub fn format_with_kind(&self, timestamp_ms: i64, kind: TimeLabelKind<'_>) -> Option<String> {
        DateTime::from_timestamp_millis(timestamp_ms).map(|datetime| match self {
            UserTimezone::Local => {
                let time_with_zone = datetime.with_timezone(&chrono::Local);
                match kind {
                    TimeLabelKind::Axis { timeframe } => {
                        Self::format_by_timeframe(&time_with_zone, timeframe)
                    }
                    TimeLabelKind::Crosshair { show_millis } => {
                        if show_millis {
                            time_with_zone.format("%H:%M:%S.%3f").to_string()
                        } else {
                            time_with_zone.format("%a %b %-d %H:%M").to_string()
                        }
                    }
                    TimeLabelKind::Custom(fmt) => time_with_zone.format(fmt).to_string(),
                }
            }
            UserTimezone::Utc => {
                let time_with_zone = datetime.with_timezone(&chrono::Utc);
                match kind {
                    TimeLabelKind::Axis { timeframe } => {
                        Self::format_by_timeframe(&time_with_zone, timeframe)
                    }
                    TimeLabelKind::Crosshair { show_millis } => {
                        if show_millis {
                            time_with_zone.format("%H:%M:%S.%3f").to_string()
                        } else {
                            time_with_zone.format("%a %b %-d %H:%M").to_string()
                        }
                    }
                    TimeLabelKind::Custom(fmt) => time_with_zone.format(fmt).to_string(),
                }
            }
        })
    }
    /// Converts UTC timestamp to the appropriate timezone and formats it according to timeframe
    pub fn format_timestamp(&self, timestamp: i64, timeframe: exchange::Timeframe) -> String {
        if let Some(datetime) = DateTime::from_timestamp(timestamp, 0) {
            match self {
                UserTimezone::Local => {
                    let time_with_zone = datetime.with_timezone(&chrono::Local);
                    Self::format_by_timeframe(&time_with_zone, timeframe)
                }
                UserTimezone::Utc => {
                    let time_with_zone = datetime.with_timezone(&chrono::Utc);
                    Self::format_by_timeframe(&time_with_zone, timeframe)
                }
            }
        } else {
            String::new()
        }
    }

    /// Formats a `DateTime` with appropriate format based on timeframe
    fn format_by_timeframe<Tz: chrono::TimeZone>(
        datetime: &DateTime<Tz>,
        timeframe: exchange::Timeframe,
    ) -> String
    where
        Tz::Offset: std::fmt::Display,
    {
        let interval = timeframe.to_milliseconds();

        if interval < 10000 {
            datetime.format("%M:%S").to_string()
        } else {
            datetime.format("%H:%M").to_string()
        }
    }

    /// The calendar date of `datetime` in the user's timezone.
    pub fn local_date(&self, datetime: DateTime<chrono::Utc>) -> chrono::NaiveDate {
        match self {
            UserTimezone::Utc => datetime.date_naive(),
            UserTimezone::Local => datetime.with_timezone(&chrono::Local).date_naive(),
        }
    }

    /// The UTC timestamp of local midnight (00:00 in the user's timezone) on
    /// `date`. Uses the earliest candidate for ambiguous local times (DST
    /// fall-back); midnight is never in a spring-forward gap in practice.
    pub fn local_midnight_utc_ms(&self, date: chrono::NaiveDate) -> Option<u64> {
        let midnight = date.and_hms_opt(0, 0, 0)?;
        let utc = match self {
            UserTimezone::Utc => midnight.and_utc(),
            UserTimezone::Local => chrono::Local
                .from_local_datetime(&midnight)
                .earliest()?
                .with_timezone(&chrono::Utc),
        };
        utc.timestamp_millis().try_into().ok()
    }

    /// Start of the user's local day containing `timestamp_ms`, as UTC ms.
    pub fn start_of_local_day_utc_ms(&self, timestamp_ms: u64) -> Option<u64> {
        let datetime = DateTime::from_timestamp_millis(timestamp_ms as i64)?;
        self.local_midnight_utc_ms(self.local_date(datetime))
    }

    /// Start of the user's local month containing `timestamp_ms`, as UTC ms.
    pub fn start_of_local_month_utc_ms(&self, timestamp_ms: u64) -> Option<u64> {
        let datetime = DateTime::from_timestamp_millis(timestamp_ms as i64)?;
        self.local_midnight_utc_ms(self.local_date(datetime).with_day(1)?)
    }

    /// Start of the user's local year containing `timestamp_ms`, as UTC ms.
    pub fn start_of_local_year_utc_ms(&self, timestamp_ms: u64) -> Option<u64> {
        let datetime = DateTime::from_timestamp_millis(timestamp_ms as i64)?;
        self.local_midnight_utc_ms(self.local_date(datetime).with_month(1)?.with_day(1)?)
    }

    /// Start of the user's local day after `timestamp_ms`, as UTC ms.
    pub fn next_local_day_utc_ms(&self, timestamp_ms: u64) -> Option<u64> {
        let datetime = DateTime::from_timestamp_millis(timestamp_ms as i64)?;
        self.local_midnight_utc_ms(self.local_date(datetime).succ_opt()?)
    }

    /// Start of the user's local month after `timestamp_ms`, as UTC ms.
    pub fn next_local_month_utc_ms(&self, timestamp_ms: u64) -> Option<u64> {
        let datetime = DateTime::from_timestamp_millis(timestamp_ms as i64)?;
        self.local_midnight_utc_ms(
            self.local_date(datetime)
                .checked_add_months(Months::new(1))?,
        )
    }

    /// Start of the user's local year after `timestamp_ms`, as UTC ms.
    pub fn next_local_year_utc_ms(&self, timestamp_ms: u64) -> Option<u64> {
        let datetime = DateTime::from_timestamp_millis(timestamp_ms as i64)?;
        self.local_midnight_utc_ms(
            self.local_date(datetime)
                .checked_add_months(Months::new(12))?,
        )
    }

    /// Advance `timestamp_ms` to the first local midnight whose date lies on
    /// the `skip_days`-period grid anchored at a fixed origin date
    /// (2000-01-01). Used to phase-align thinned boundary series (e.g. daily
    /// marks) to *absolute* dates so the collected set stays fixed while the
    /// view pans; without this, the thinning re-anchors to the sliding visible
    /// edge and the whole day-label set shifts by one day whenever the edge
    /// crosses a midnight.
    pub fn align_to_skip_grid(&self, timestamp_ms: u64, skip_days: u64) -> Option<u64> {
        let origin = chrono::NaiveDate::from_ymd_opt(ORIGIN_YEAR, 1, 1)?;
        let datetime = DateTime::from_timestamp_millis(timestamp_ms as i64)?;
        let date = self.local_date(datetime);
        let days = date.signed_duration_since(origin).num_days();
        let advance = advance_to_grid(skip_days, days);
        self.local_midnight_utc_ms(date.checked_add_days(chrono::Days::new(advance))?)
    }

    /// Advance `timestamp_ms` to the first month-start that lies on the
    /// `step_months`-period grid anchored at 2000-01, in the user's timezone.
    pub fn align_to_month_grid(&self, timestamp_ms: u64, step_months: u64) -> Option<u64> {
        let datetime = DateTime::from_timestamp_millis(timestamp_ms as i64)?;
        let date = self.local_date(datetime);
        let months = (date.year() - ORIGIN_YEAR) as i64 * 12 + i64::from(date.month0());
        let first = date.with_day(1)?;
        self.local_midnight_utc_ms(
            first.checked_add_months(Months::new(advance_to_grid(step_months, months) as u32))?,
        )
    }

    /// Advance `timestamp_ms` to the first year-start that lies on the
    /// `step_years`-period grid anchored at 2000-01-01, in the user's timezone.
    pub fn align_to_year_grid(&self, timestamp_ms: u64, step_years: u64) -> Option<u64> {
        let datetime = DateTime::from_timestamp_millis(timestamp_ms as i64)?;
        let date = self.local_date(datetime);
        let years = (date.year() - ORIGIN_YEAR) as i64;
        let first = date.with_month(1)?.with_day(1)?;
        self.local_midnight_utc_ms(first.checked_add_months(Months::new(
            (advance_to_grid(step_years, years) * 12) as u32,
        ))?)
    }
}

/// Forward distance from `offset` to the next multiple of `step` (0 when already aligned).
fn advance_to_grid(step: u64, offset: i64) -> u64 {
    let step = step.max(1) as i64;
    ((step - offset.rem_euclid(step)) % step) as u64
}

impl UserTimezone {
    /// Formats a `DateTime` with detailed format for crosshair display
    pub fn format_crosshair_timestamp(&self, timestamp_millis: i64, interval: u64) -> String {
        if let Some(datetime) = DateTime::from_timestamp_millis(timestamp_millis) {
            if interval < 10000 {
                return datetime.format("%M:%S.%3f").to_string();
            }

            if interval >= 28 * 86_400_000 {
                return match self {
                    UserTimezone::Local => datetime
                        .with_timezone(&chrono::Local)
                        .format("%b %Y")
                        .to_string(),
                    UserTimezone::Utc => datetime
                        .with_timezone(&chrono::Utc)
                        .format("%b %Y")
                        .to_string(),
                };
            }

            if interval >= 86_400_000 {
                return match self {
                    UserTimezone::Local => datetime
                        .with_timezone(&chrono::Local)
                        .format("%a %b %-d %Y")
                        .to_string(),
                    UserTimezone::Utc => datetime
                        .with_timezone(&chrono::Utc)
                        .format("%a %b %-d %Y")
                        .to_string(),
                };
            }

            match self {
                UserTimezone::Local => datetime
                    .with_timezone(&chrono::Local)
                    .format("%a %b %-d %H:%M")
                    .to_string(),
                UserTimezone::Utc => datetime
                    .with_timezone(&chrono::Utc)
                    .format("%a %b %-d %H:%M")
                    .to_string(),
            }
        } else {
            String::new()
        }
    }
}

impl fmt::Display for UserTimezone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UserTimezone::Utc => write!(f, "UTC"),
            UserTimezone::Local => {
                let local_offset = chrono::Local::now().offset().local_minus_utc();
                let hours = local_offset / 3600;
                let minutes = (local_offset % 3600) / 60;
                write!(f, "Local (UTC {hours:+03}:{minutes:02})")
            }
        }
    }
}

impl<'de> Deserialize<'de> for UserTimezone {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let timezone_str = String::deserialize(deserializer)?;
        match timezone_str.to_lowercase().as_str() {
            "utc" => Ok(UserTimezone::Utc),
            "local" => Ok(UserTimezone::Local),
            _ => Err(serde::de::Error::custom("Invalid UserTimezone")),
        }
    }
}

impl Serialize for UserTimezone {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            UserTimezone::Utc => serializer.serialize_str("UTC"),
            UserTimezone::Local => serializer.serialize_str("Local"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_crosshair_timestamp_higher_timeframes() {
        let tz = UserTimezone::Utc;
        // 2026-10-15 12:30:00 UTC = 1792067400000 ms
        let ts = 1792067400000i64;

        // Subminute (< 10000 ms)
        let sub = tz.format_crosshair_timestamp(ts, 1000);
        assert!(sub.contains("30:00.000"));

        // Intraday (1h = 3_600_000 ms)
        let intra = tz.format_crosshair_timestamp(ts, 3_600_000);
        assert_eq!(intra, "Thu Oct 15 12:30");

        // Daily / Weekly (7 days = 604_800_000 ms)
        let weekly = tz.format_crosshair_timestamp(ts, 604_800_000);
        assert_eq!(weekly, "Thu Oct 15 2026");

        // Monthly (30 days = 2_592_000_000 ms)
        let monthly = tz.format_crosshair_timestamp(ts, 2_592_000_000);
        assert_eq!(monthly, "Oct 2026");
    }
}
