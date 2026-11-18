//! ECMAScript Number::toString for finite doubles (the RFC 8785 number rule).

use crate::{CanonError, InvalidKind};

/// Formats a finite double as ECMAScript Number::toString does. Both zeros give
/// `"0"`. NaN and infinities are `canon.invalid` (`number_range`).
pub fn format_number(x: f64) -> Result<String, CanonError> {
    if !x.is_finite() {
        return Err(CanonError::invalid(InvalidKind::NumberRange, 0));
    }
    if x == 0.0 {
        return Ok("0".to_string());
    }
    let mut out = String::new();
    let a = if x < 0.0 {
        out.push('-');
        -x
    } else {
        x
    };
    let (digits, n) = shortest_digits(a);
    let k = digits.len() as i32;
    if k <= n && n <= 21 {
        // Integer: digits followed by n - k zeros.
        out.push_str(&digits);
        push_zeros(&mut out, n - k);
    } else if 0 < n && n <= 21 {
        let (int, frac) = digits.split_at(n as usize);
        out.push_str(int);
        out.push('.');
        out.push_str(frac);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        push_zeros(&mut out, -n);
        out.push_str(&digits);
    } else {
        let (first, rest) = digits.split_at(1);
        out.push_str(first);
        if !rest.is_empty() {
            out.push('.');
            out.push_str(rest);
        }
        let e = n - 1;
        out.push('e');
        out.push(if e >= 0 { '+' } else { '-' });
        out.push_str(&e.unsigned_abs().to_string());
    }
    Ok(out)
}

fn push_zeros(out: &mut String, count: i32) {
    for _ in 0..count {
        out.push('0');
    }
}

/// The shortest decimal digit string `d` (no leading or trailing zeros) and
/// exponent `n` with `a = 0.d * 10^n` that parses back to `a`, choosing the
/// nearest such string. `a` is finite and positive.
///
/// A correctly rounded p-digit rendering is the nearest p-digit decimal, so if any
/// p-digit decimal round-trips, it does; and round-tripping at p digits implies it
/// at p + 1. The smallest such p is found by binary search over 1..=17
/// (17 significant digits always round-trip a double).
fn shortest_digits(a: f64) -> (String, i32) {
    let render = |p: usize| format!("{:.*e}", p - 1, a);
    let round_trips = |s: &str| matches!(s.parse::<f64>(), Ok(b) if b == a);
    let (mut lo, mut hi) = (1usize, 17usize);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if round_trips(&render(mid)) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    split_scientific(&render(lo))
}

/// Splits Rust's `d.ddde[-]x` rendering into trimmed digits and `n` (point position).
fn split_scientific(s: &str) -> (String, i32) {
    let (mantissa, exp) = s.split_once('e').unwrap_or((s, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let mut digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
    (digits, exp + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hand_labelled_forms() {
        let cases: [(f64, &str); 12] = [
            (1.0, "1"),
            (-2.5, "-2.5"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1e-6, "0.000001"),
            (1e-7, "1e-7"),
            (-0.0, "0"),
            (123.25, "123.25"),
            (0.001953125, "0.001953125"),
            (6.02e23, "6.02e+23"),
            (f64::MIN_POSITIVE, "2.2250738585072014e-308"),
            (f64::MAX, "1.7976931348623157e+308"),
        ];
        for (x, want) in cases {
            assert_eq!(format_number(x).as_deref(), Ok(want), "{x:e}");
        }
        assert!(format_number(f64::NAN).is_err());
        assert!(format_number(f64::NEG_INFINITY).is_err());
    }
}
