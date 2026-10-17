use std::fmt;

use chrono::DateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum UserTimezone {
    #[default]
    Utc,
    Local,
}

impl UserTimezone {
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
        } else if datetime.format("%H:%M").to_string() == "00:00" {
            datetime.format("%-d").to_string()
        } else {
            datetime.format("%H:%M").to_string()
        }
    }

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
