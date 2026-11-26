//! `JSONEncoder.DateEncodingStrategy.iso8601` compatibility: dates are written as
//! `2026-11-26T09:30:00Z` (UTC, whole seconds), which is what Swift's `ISO8601DateFormatter`
//! produces and accepts by default.

use crate::time::SwiftDate;
use serde::{Deserialize, Deserializer, Serializer};

pub fn format(date: SwiftDate) -> String {
    let seconds = date.unix_seconds().floor() as i64;
    let days = seconds.div_euclid(86_400);
    let time = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time / 3600,
        (time % 3600) / 60,
        time % 60
    )
}

/// Parses `YYYY-MM-DDTHH:MM:SS[.fff](Z|±HH:MM|±HHMM)`.
pub fn parse(text: &str) -> Option<SwiftDate> {
    let bytes = text.as_bytes();
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    if bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    let number = |range: std::ops::Range<usize>| -> Option<i64> {
        let part = text.get(range)?;
        part.bytes().all(|b| b.is_ascii_digit()).then(|| part.parse().ok())?
    };
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut rest = &text[19..];
    let mut fraction = 0.0;
    if let Some(after) = rest.strip_prefix('.') {
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        fraction = format!("0.{}", &after[..digits]).parse().ok()?;
        rest = &after[digits..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let digits: String = rest[1..].chars().filter(|c| *c != ':').collect();
            if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let hours: i64 = digits[..2].parse().ok()?;
            let minutes: i64 = digits[2..].parse().ok()?;
            sign * (hours * 3600 + minutes * 60)
        }
    };
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset;
    Some(SwiftDate::from_unix_seconds(seconds as f64 + fraction))
}

pub fn serialize<S: Serializer>(date: &SwiftDate, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format(*date))
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<SwiftDate, D::Error> {
    let text = String::deserialize(deserializer)?;
    parse(&text)
        .ok_or_else(|| serde::de::Error::custom(format!("Expected date string to be ISO8601-formatted: {text}")))
}

// Howard Hinnant's civil calendar algorithms (proleptic Gregorian, days since 1970-01-01).

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_and_parses_utc_dates() {
        // 2001-01-01T00:00:00Z is Swift's reference date.
        assert_eq!(format(SwiftDate(0.0)), "2001-01-01T00:00:00Z");
        assert_eq!(
            format(SwiftDate(0.75)),
            "2001-01-01T00:00:00Z",
            "whole seconds, truncated"
        );
        let date = parse("2026-11-26T09:30:15Z").unwrap();
        assert_eq!(format(date), "2026-11-26T09:30:15Z");
        assert_eq!(date.unix_seconds(), 1_795_685_415.0);
        assert_eq!(format(parse("2024-02-29T23:59:59Z").unwrap()), "2024-02-29T23:59:59Z");
        assert_eq!(
            format(parse("1999-12-31T22:00:00-02:00").unwrap()),
            "2000-01-01T00:00:00Z"
        );
        assert_eq!(
            parse("2026-11-26T09:30:15.250Z").unwrap().unix_seconds(),
            1_795_685_415.25
        );
        assert!(parse("2026-11-26 09:30:15").is_none());
        assert!(parse("not a date").is_none());
    }
}
