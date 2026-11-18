//! Time sources and timestamp fields (REQ-HAL.IDL.UNITS-004).

use super::{i64_dec, kept, object, string, take, UnitsError};
use fid_canon::CValue;
use std::collections::BTreeMap;
use std::fmt;

/// Where a timestamp came from, best first. The derived total order is the quality order
/// of the leaf: `HwTrigger < Ptp < DeviceClock < Ntp < HostReceive`, so sorting puts the
/// best source first and `min` picks the best.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TimeSource {
    /// A hardware trigger line latched by the device.
    HwTrigger,
    /// Device clock disciplined by IEEE 1588 PTP.
    Ptp,
    /// Free-running device clock.
    DeviceClock,
    /// Clock disciplined by NTP.
    Ntp,
    /// Host receive time only.
    HostReceive,
}

impl TimeSource {
    /// Every source, best first.
    pub const ALL: [TimeSource; 5] =
        [TimeSource::HwTrigger, TimeSource::Ptp, TimeSource::DeviceClock, TimeSource::Ntp, TimeSource::HostReceive];

    /// The wire string, e.g. `HW_TRIGGER`.
    pub fn as_str(self) -> &'static str {
        match self {
            TimeSource::HwTrigger => "HW_TRIGGER",
            TimeSource::Ptp => "PTP",
            TimeSource::DeviceClock => "DEVICE_CLOCK",
            TimeSource::Ntp => "NTP",
            TimeSource::HostReceive => "HOST_RECEIVE",
        }
    }

    /// Parses the wire string; anything else is `bad_value`.
    pub fn parse(s: &str) -> Result<TimeSource, UnitsError> {
        TimeSource::ALL
            .into_iter()
            .find(|t| t.as_str() == s)
            .ok_or_else(|| UnitsError::BadValue { field: "time_source", value: kept(s) })
    }

    /// Reads a wire string value.
    pub fn from_value(v: CValue) -> Result<TimeSource, UnitsError> {
        TimeSource::parse(&string("time_source", v)?)
    }

    /// The wire value.
    pub fn to_value(self) -> CValue {
        CValue::Str(self.as_str().to_string())
    }
}

impl fmt::Display for TimeSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Timestamp fields of an observation, in integer nanoseconds: `t_dev` on the device clock
/// (absent when the device has none) and `t_host` on the host clock. Relating the two is
/// `HAL.ACQ.TIME`. Wire form: `{"t_dev": "123", "t_host": "456"}`, decimal strings, with
/// `t_dev` omitted when absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp {
    pub t_dev: Option<i64>,
    pub t_host: i64,
}

impl Timestamp {
    /// The wire fields.
    pub const FIELDS: [&'static str; 2] = ["t_dev", "t_host"];

    /// The wire value.
    pub fn to_value(self) -> CValue {
        let mut o = BTreeMap::new();
        if let Some(t) = self.t_dev {
            o.insert("t_dev".to_string(), CValue::I64(t));
        }
        o.insert("t_host".to_string(), CValue::I64(self.t_host));
        CValue::Obj(o)
    }

    /// Reads the wire value.
    pub fn from_value(v: CValue) -> Result<Timestamp, UnitsError> {
        let mut o = object(v, "timestamp", &Timestamp::FIELDS)?;
        let t_host = i64_dec("t_host", take(&mut o, "t_host")?)?;
        let t_dev = o.remove("t_dev").map(|v| i64_dec("t_dev", v)).transpose()?;
        Ok(Timestamp { t_dev, t_host })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_is_best_first() {
        let mut v = TimeSource::ALL.to_vec();
        v.reverse();
        v.sort();
        assert_eq!(v, TimeSource::ALL.to_vec());
        assert!(TimeSource::HwTrigger < TimeSource::HostReceive);
    }

    #[test]
    fn timestamp_round_trip() {
        for t in [Timestamp { t_dev: None, t_host: 0 }, Timestamp { t_dev: Some(i64::MIN), t_host: i64::MAX }] {
            assert_eq!(Timestamp::from_value(t.to_value()), Ok(t));
        }
    }
}
