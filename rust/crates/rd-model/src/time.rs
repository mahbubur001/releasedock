use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Seconds between 1970-01-01 and 2001-01-01, Foundation's reference date.
const REFERENCE_OFFSET: f64 = 978_307_200.0;

/// A point in time encoded like Swift's `Date` with the default `JSONEncoder`: a
/// floating-point number of seconds since 2001-01-01 00:00:00 UTC.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SwiftDate(pub f64);

impl SwiftDate {
    pub fn now() -> Self {
        Self::from(SystemTime::now())
    }

    pub fn unix_seconds(self) -> f64 {
        self.0 + REFERENCE_OFFSET
    }

    pub fn from_unix_seconds(seconds: f64) -> Self {
        Self(seconds - REFERENCE_OFFSET)
    }

    pub fn adding(self, seconds: f64) -> Self {
        Self(self.0 + seconds)
    }
}

impl From<SystemTime> for SwiftDate {
    fn from(time: SystemTime) -> Self {
        let unix = match time.duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_secs_f64(),
            Err(e) => -e.duration().as_secs_f64(),
        };
        Self::from_unix_seconds(unix)
    }
}

impl From<SwiftDate> for SystemTime {
    fn from(date: SwiftDate) -> Self {
        let unix = date.unix_seconds();
        if unix >= 0.0 {
            UNIX_EPOCH + Duration::from_secs_f64(unix)
        } else {
            UNIX_EPOCH - Duration::from_secs_f64(-unix)
        }
    }
}

/// A UUID written as an uppercase string, like Swift's `UUID`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id(pub uuid::Uuid);

impl Id {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    pub fn parse(text: &str) -> Option<Self> {
        uuid::Uuid::parse_str(text).ok().map(Self)
    }
}

impl Default for Id {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buffer = uuid::Uuid::encode_buffer();
        f.write_str(self.0.hyphenated().encode_upper(&mut buffer))
    }
}

impl Serialize for Id {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Id::parse(&text).ok_or_else(|| serde::de::Error::custom(format!("invalid UUID {text}")))
    }
}
