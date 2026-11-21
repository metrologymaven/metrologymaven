//! RFC 3339 UTC instants as the trust files and signature statements write them.

use std::cmp::Ordering;
use std::fmt;

/// An RFC 3339 instant in UTC: `YYYY-MM-DDTHH:MM:SS[.fraction]Z`, with an uppercase `T` and
/// `Z`, a fraction of 1 to 9 digits, and no leap second (seconds 00 to 59). Ordering is by
/// instant, so `...:00.5Z` equals `...:00.50Z`; [`UtcTime::as_str`] keeps the text as written.
///
/// Comparison only: no clock is read and no arithmetic is done (determinism D0).
#[derive(Debug, Clone)]
pub struct UtcTime {
    key: (u16, u8, u8, u8, u8, u8, u32),
    text: String,
}

impl UtcTime {
    /// Parses the text; `None` when it is not an RFC 3339 UTC instant of the form above.
    pub fn parse(text: &str) -> Option<UtcTime> {
        let b = text.as_bytes();
        if b.len() < 20 || b.len() > 30 || b.last() != Some(&b'Z') {
            return None;
        }
        let num = |from: usize, len: usize| -> Option<u32> {
            let digits = b.get(from..from + len)?;
            digits.iter().try_fold(0u32, |acc, &c| c.is_ascii_digit().then(|| acc * 10 + u32::from(c - b'0')))
        };
        let sep = |at: usize, c: u8| b.get(at) == Some(&c);
        if !(sep(4, b'-') && sep(7, b'-') && sep(10, b'T') && sep(13, b':') && sep(16, b':')) {
            return None;
        }
        let year = num(0, 4)?;
        let month = num(5, 2)?;
        let day = num(8, 2)?;
        let hour = num(11, 2)?;
        let minute = num(14, 2)?;
        let second = num(17, 2)?;
        if !(1..=12).contains(&month) || !(1..=days_in_month(year, month)).contains(&day) {
            return None;
        }
        if hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        let rest = b.get(19..b.len() - 1)?;
        let nanos = match rest {
            [] => 0,
            [b'.', frac @ ..] if (1..=9).contains(&frac.len()) && frac.iter().all(u8::is_ascii_digit) => {
                let mut n = 0u32;
                for i in 0..9 {
                    n = n * 10 + frac.get(i).map_or(0, |&c| u32::from(c - b'0'));
                }
                n
            }
            _ => return None,
        };
        let narrow = |v: u32| u8::try_from(v).ok();
        let key = (
            u16::try_from(year).ok()?,
            narrow(month)?,
            narrow(day)?,
            narrow(hour)?,
            narrow(minute)?,
            narrow(second)?,
            nanos,
        );
        Some(UtcTime { key, text: text.to_owned() })
    }

    /// The text as written.
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

impl PartialEq for UtcTime {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for UtcTime {}

impl PartialOrd for UtcTime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for UtcTime {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key.cmp(&other.key)
    }
}

impl fmt::Display for UtcTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::UtcTime;

    fn t(s: &str) -> UtcTime {
        UtcTime::parse(s).unwrap()
    }

    #[test]
    fn accepts_utc_forms_and_orders_by_instant() {
        assert_eq!(t("2026-03-02T09:30:00.5Z"), t("2026-03-02T09:30:00.500000000Z"));
        assert!(t("2026-03-02T09:30:00.25Z") < t("2026-03-02T09:30:00.5Z"));
        assert!(t("2026-03-02T09:30:00Z") < t("2026-03-02T09:30:00.000000001Z"));
        assert!(t("2025-12-31T23:59:59.999Z") < t("2026-01-01T00:00:00Z"));
        assert!(t("2024-02-29T00:00:00Z") < t("2024-03-01T00:00:00Z"));
        assert_eq!(t("2000-02-29T12:00:00Z").as_str(), "2000-02-29T12:00:00Z");
    }

    #[test]
    fn refuses_everything_else() {
        for bad in [
            "",
            "2026-03-01T10:00:00",
            "2026-03-01T10:00:00z",
            "2026-03-01t10:00:00Z",
            "2026-03-01 10:00:00Z",
            "2026-03-01T10:00:00+00:00",
            "2026-03-01T10:00:00.Z",
            "2026-03-01T10:00:00.1234567890Z",
            "2026-02-29T00:00:00Z",
            "1900-02-29T00:00:00Z",
            "2026-02-30T00:00:00Z",
            "2026-04-31T00:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-00-01T00:00:00Z",
            "2026-01-00T00:00:00Z",
            "2026-01-01T24:00:00Z",
            "2026-01-01T00:60:00Z",
            "2026-01-01T00:00:60Z",
            "+026-01-01T00:00:00Z",
            "2026-1-01T00:00:00Z",
            "2026-01-01T00:00:0aZ",
            "2026-01-01T00:00:00.1aZ",
            "\u{661}026-01-01T00:00:00Z",
        ] {
            assert!(UtcTime::parse(bad).is_none(), "{bad:?}");
        }
    }
}
