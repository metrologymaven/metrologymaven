//! `format` as an assertion. JSON Schema 2020-12 makes `format` an annotation by default;
//! the bus asserts it (fail closed) for the formats below and refuses, when the schema is
//! compiled, every other format name, so a format is never silently ignored. Each format
//! applies to strings only; other instance types pass, as the 2020-12 specification says.
//!
//! - `date`, `time`, `date-time`: RFC 3339 §5.6 `full-date`, `full-time` (offset required)
//!   and `date-time`; `T` and `Z` in either case; day within the month of that year
//!   (Gregorian leap years); seconds `60` only when the time in UTC is 23:59.
//! - `duration`: RFC 3339 Appendix A (`P1Y2M`, `PT1H`, `P3W`; ASCII digits, no fractions).
//! - `uuid`: RFC 4122 hyphenated form, hex digits of either case.
//! - `ipv4`: dotted quad, each octet `0`..`255` in decimal without leading zeros.
//! - `ipv6`: RFC 4291 §2.2 text forms (at most one `::`, optional trailing dotted quad);
//!   no zone index, no brackets.
//! - `hostname`: RFC 1123: labels of 1..=63 ASCII letters, digits and `-`, not starting or
//!   ending with `-`, separated by `.`; at most 253 characters in all; no trailing dot.
//! - `json-pointer`: RFC 6901 (`~` only as `~0` or `~1`).
//! - `regex`: a pattern of the ECMA-262 subset of [`super::ecma`].

use super::ecma::Pattern;
use regex::Regex;
use std::sync::OnceLock;

/// A format the bus asserts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Date,
    Time,
    DateTime,
    Duration,
    Uuid,
    Ipv4,
    Ipv6,
    Hostname,
    JsonPointer,
    Regex,
}

impl Format {
    /// Every asserted format with its name.
    pub const ALL: [(Format, &'static str); 10] = [
        (Format::Date, "date"),
        (Format::Time, "time"),
        (Format::DateTime, "date-time"),
        (Format::Duration, "duration"),
        (Format::Uuid, "uuid"),
        (Format::Ipv4, "ipv4"),
        (Format::Ipv6, "ipv6"),
        (Format::Hostname, "hostname"),
        (Format::JsonPointer, "json-pointer"),
        (Format::Regex, "regex"),
    ];

    /// The format with this exact name, or `None` when the bus does not assert it.
    pub fn parse(name: &str) -> Option<Format> {
        Format::ALL.iter().find(|(_, n)| *n == name).map(|(f, _)| *f)
    }

    /// Whether `s` is valid for this format.
    pub fn check(self, s: &str) -> bool {
        match self {
            Format::Date => date(s.as_bytes()),
            Format::Time => full_time(s.as_bytes()),
            Format::DateTime => date_time(s),
            Format::Duration => duration(s),
            Format::Uuid => uuid(s.as_bytes()),
            Format::Ipv4 => ipv4(s),
            Format::Ipv6 => ipv6(s),
            Format::Hostname => hostname(s),
            Format::JsonPointer => json_pointer(s),
            Format::Regex => Pattern::compile(s).is_ok(),
        }
    }
}

fn num(b: &[u8]) -> Option<u32> {
    if b.is_empty() || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    b.iter().try_fold(0u32, |acc, d| acc.checked_mul(10)?.checked_add(u32::from(d - b'0')))
}

fn is_leap(y: u32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn date(b: &[u8]) -> bool {
    if b.len() != 10 || b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') {
        return false;
    }
    let (Some(y), Some(m), Some(d)) = (num(&b[0..4]), num(&b[5..7]), num(&b[8..10])) else { return false };
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&d)
}

/// RFC 3339 `full-time`: `HH:MM:SS[.frac](Z|±HH:MM)`.
fn full_time(b: &[u8]) -> bool {
    if b.len() < 9 || b.get(2) != Some(&b':') || b.get(5) != Some(&b':') {
        return false;
    }
    let (Some(h), Some(mi), Some(s)) = (num(&b[0..2]), num(&b[3..5]), num(&b[6..8])) else { return false };
    if h > 23 || mi > 59 || s > 60 {
        return false;
    }
    let mut i = 8;
    if b.get(i) == Some(&b'.') {
        let start = i + 1;
        i = start;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    let Some(rest) = b.get(i..) else { return false };
    let offset_minutes: i64 = match rest {
        [b'Z' | b'z'] => 0,
        [sign @ (b'+' | b'-'), oh1, oh2, b':', om1, om2] => {
            let (Some(oh), Some(om)) = (num(&[*oh1, *oh2]), num(&[*om1, *om2])) else { return false };
            if oh > 23 || om > 59 {
                return false;
            }
            let m = i64::from(oh * 60 + om);
            if *sign == b'-' {
                -m
            } else {
                m
            }
        }
        _ => return false,
    };
    if s == 60 {
        // A leap second is the last second of a UTC day: local time minus offset is 23:59.
        let utc = (i64::from(h * 60 + mi) - offset_minutes).rem_euclid(1440);
        return utc == 23 * 60 + 59;
    }
    true
}

fn date_time(s: &str) -> bool {
    let b = s.as_bytes();
    match (b.get(..10), b.get(10), b.get(11..)) {
        (Some(d), Some(b'T' | b't'), Some(t)) => date(d) && full_time(t),
        _ => false,
    }
}

fn duration(s: &str) -> bool {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    const T: &str = "T(?:[0-9]+H(?:[0-9]+M(?:[0-9]+S)?)?|[0-9]+M(?:[0-9]+S)?|[0-9]+S)";
    let re = RE.get_or_init(|| {
        let date = "(?:[0-9]+D|[0-9]+M(?:[0-9]+D)?|[0-9]+Y(?:[0-9]+M(?:[0-9]+D)?)?)";
        Regex::new(&format!("^P(?:{date}(?:{T})?|{T}|[0-9]+W)$")).ok()
    });
    re.as_ref().is_some_and(|r| r.is_match(s))
}

fn uuid(b: &[u8]) -> bool {
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
}

fn ipv4(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|p| {
            let b = p.as_bytes();
            (1..=3).contains(&b.len()) && !(b.len() > 1 && b.first() == Some(&b'0')) && num(b).is_some_and(|v| v <= 255)
        })
}

/// Number of 16-bit groups in a `:`-separated run (an empty run has none), or `None` if
/// a group is malformed. A dotted quad is allowed as the last group of the last run.
fn ipv6_groups(run: &str, last: bool) -> Option<usize> {
    if run.is_empty() {
        return Some(0);
    }
    let groups: Vec<&str> = run.split(':').collect();
    let mut n = 0;
    for (i, g) in groups.iter().enumerate() {
        let is_last = i + 1 == groups.len();
        if last && is_last && g.contains('.') {
            if !ipv4(g) {
                return None;
            }
            n += 2;
        } else if (1..=4).contains(&g.len()) && g.bytes().all(|c| c.is_ascii_hexdigit()) {
            n += 1;
        } else {
            return None;
        }
    }
    Some(n)
}

fn ipv6(s: &str) -> bool {
    match s.split_once("::") {
        Some((head, tail)) => {
            if tail.contains("::") {
                return false;
            }
            match (ipv6_groups(head, false), ipv6_groups(tail, true)) {
                (Some(a), Some(b)) => a + b <= 7,
                _ => false,
            }
        }
        None => ipv6_groups(s, true) == Some(8),
    }
}

fn hostname(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && s.split('.').all(|l| {
            let b = l.as_bytes();
            (1..=63).contains(&b.len())
                && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-')
                && b.first() != Some(&b'-')
                && b.last() != Some(&b'-')
        })
}

fn json_pointer(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    if !s.starts_with('/') {
        return false;
    }
    let b = s.as_bytes();
    b.iter().enumerate().all(|(i, c)| *c != b'~' || matches!(b.get(i + 1), Some(b'0' | b'1')))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Hand-labelled from the RFCs cited in the module docs.
    #[test]
    fn formats() {
        let cases: &[(Format, &str, bool)] = &[
            (Format::Date, "2024-02-29", true),
            (Format::Date, "2023-02-29", false),
            (Format::Date, "1900-02-29", false),
            (Format::Date, "2000-02-29", true),
            (Format::Date, "2026-04-31", false),
            (Format::Date, "2026-13-01", false),
            (Format::Date, "2026-1-01", false),
            (Format::Date, "\u{0662}026-01-01", false),
            (Format::Time, "23:59:60Z", true),
            (Format::Time, "15:59:60-08:00", true),
            (Format::Time, "23:59:60+01:00", false),
            (Format::Time, "12:00:00.5+05:30", true),
            (Format::Time, "12:00:00", false),
            (Format::Time, "12:00:00.Z", false),
            (Format::Time, "24:00:00Z", false),
            (Format::DateTime, "2026-11-20T10:00:00z", true),
            (Format::DateTime, "2026-11-20t10:00:00.123+01:00", true),
            (Format::DateTime, "2026-11-20 10:00:00Z", false),
            (Format::Duration, "P1Y2M3DT4H5M6S", true),
            (Format::Duration, "P3W", true),
            (Format::Duration, "PT1M", true),
            (Format::Duration, "P", false),
            (Format::Duration, "PT", false),
            (Format::Duration, "P1Y2D", false),
            (Format::Duration, "P1W2D", false),
            (Format::Uuid, "2EB8AA08-AA98-11EA-B4AA-73B441D16380", true),
            (Format::Uuid, "2eb8aa08aa9811eab4aa73b441d16380", false),
            (Format::Ipv4, "192.168.0.1", true),
            (Format::Ipv4, "192.168.00.1", false),
            (Format::Ipv4, "256.0.0.1", false),
            (Format::Ipv4, "1.2.3", false),
            (Format::Ipv6, "::", true),
            (Format::Ipv6, "::ffff:1.2.3.4", true),
            (Format::Ipv6, "1:2:3:4:5:6:7:8", true),
            (Format::Ipv6, "1:2:3:4:5:6:7::", true),
            (Format::Ipv6, "1:2:3:4:5:6:7:8:9", false),
            (Format::Ipv6, "1::2::3", false),
            (Format::Ipv6, "fe80::1%eth0", false),
            (Format::Ipv6, "12345::", false),
            (Format::Ipv6, ":1:2:3:4:5:6:7", false),
            (Format::Hostname, "metrologymaven.io", true),
            (Format::Hostname, "-bad.io", false),
            (Format::Hostname, "a..b", false),
            (Format::Hostname, "trailing.", false),
            (Format::JsonPointer, "/a~1b/~0", true),
            (Format::JsonPointer, "/a~2", false),
            (Format::JsonPointer, "a", false),
            (Format::Regex, "^[a-z]+$", true),
            (Format::Regex, "(?=x)", false),
        ];
        for (f, s, want) in cases {
            assert_eq!(f.check(s), *want, "{f:?} {s:?}");
        }
        assert!(Format::parse("email").is_none() && Format::parse("date-time") == Some(Format::DateTime));
    }
}
