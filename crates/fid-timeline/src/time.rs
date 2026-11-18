//! Exact RFC 3339 timestamp parsing to integer UTC seconds.
//!
//! Accepted form: `YYYY-MM-DDTHH:MM:SS` followed by `Z` or a `+HH:MM` / `-HH:MM` offset,
//! years 0001..=9999. No fractional seconds, no leap second (`:60`), no local (offset-less)
//! times: each of those is ambiguous for an integer-second clock and is rejected.

use crate::TimelineError;

/// Seconds in one calendar day. The clock counts elapsed wall time: every day is 24 h,
/// with no business-hours, weekend or holiday exclusions (the spec defines none).
pub const SECONDS_PER_DAY: i64 = 86_400;

/// Days from 1970-01-01 to the given proleptic Gregorian date (Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Reads `n` ASCII digits at `pos`.
fn digits(b: &[u8], pos: usize, n: usize) -> Option<i64> {
    let s = b.get(pos..pos + n)?;
    let mut v: i64 = 0;
    for &c in s {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v * 10 + i64::from(c - b'0');
    }
    Some(v)
}

fn sep(b: &[u8], pos: usize, c: u8) -> Option<()> {
    (b.get(pos) == Some(&c)).then_some(())
}

fn parse_inner(b: &[u8]) -> Option<i64> {
    let year = digits(b, 0, 4)?;
    sep(b, 4, b'-')?;
    let month = digits(b, 5, 2)?;
    sep(b, 7, b'-')?;
    let day = digits(b, 8, 2)?;
    sep(b, 10, b'T')?;
    let hour = digits(b, 11, 2)?;
    sep(b, 13, b':')?;
    let minute = digits(b, 14, 2)?;
    sep(b, 16, b':')?;
    let second = digits(b, 17, 2)?;
    if year < 1 || !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let offset = match b.get(19..)? {
        b"Z" => 0,
        [sign @ (b'+' | b'-'), ..] if b.len() == 25 => {
            let oh = digits(b, 20, 2)?;
            sep(b, 22, b':')?;
            let om = digits(b, 23, 2)?;
            if oh > 23 || om > 59 {
                return None;
            }
            let o = oh * 3600 + om * 60;
            if *sign == b'-' {
                -o
            } else {
                o
            }
        }
        _ => return None,
    };
    let local = days_from_civil(year, month, day) * SECONDS_PER_DAY + hour * 3600 + minute * 60 + second;
    Some(local - offset)
}

/// Parses an RFC 3339 timestamp (see the module docs) to seconds since
/// 1970-01-01T00:00:00Z. Errors are `TIMELINE-INPUT`.
pub fn parse_timestamp(s: &str) -> Result<i64, TimelineError> {
    parse_inner(s.as_bytes()).ok_or_else(|| {
        TimelineError::input(format!(
            "invalid timestamp {s:?}: expected YYYY-MM-DDTHH:MM:SS with Z or +HH:MM, no fractions"
        ))
    })
}
