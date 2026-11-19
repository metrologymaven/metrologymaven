//! The string form of an `f64` in kernel canonical JSON: ECMAScript `Number::toString`
//! (the RFC 8785 §3.2.2.3 number form), written inside a JSON string because kernel
//! canonical JSON emits numbers as strings (KRN.MAN.KCI).
//!
//! Same algorithm as the crate-private `es_number` of `fid-scan-cmp` (a kernel crate may not
//! depend on another crate's private items). The shortest round-trip digits come from
//! `core`'s `{:e}` formatting, which is integer code (no float arithmetic), so the string is
//! the same on every platform, wasm32 included.

/// The ECMAScript string of a finite `v`; `None` for NaN or an infinity. Both zeros give
/// `"0"`.
pub(crate) fn es_number(v: f64) -> Option<String> {
    if !v.is_finite() {
        return None;
    }
    if v == 0.0 {
        return Some("0".to_string());
    }
    // `{:e}` of a finite non-zero f64 is "<d>[.<ddd>]e<exp>" with the shortest digits.
    let sci = format!("{:e}", v.abs());
    let (mantissa, exp) = sci.split_once('e')?;
    let exp: i64 = exp.parse().ok()?;
    let all: String = mantissa.chars().filter(|c| *c != '.').collect();
    let digits = all.trim_end_matches('0');
    if digits.is_empty() {
        return None;
    }
    let k = digits.len() as i64;
    // value = 0.<digits> * 10^n
    let n = exp + 1;
    let mut out = String::new();
    if v < 0.0 {
        out.push('-');
    }
    if (k..=21).contains(&n) {
        out.push_str(digits);
        out.push_str(&"0".repeat((n - k) as usize));
    } else if 0 < n && n <= 21 {
        let (a, b) = digits.split_at(n as usize);
        out.push_str(a);
        out.push('.');
        out.push_str(b);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        out.push_str(&"0".repeat((-n) as usize));
        out.push_str(digits);
    } else {
        let e = n - 1;
        let (first, rest) = digits.split_at(1);
        out.push_str(first);
        if !rest.is_empty() {
            out.push('.');
            out.push_str(rest);
        }
        out.push('e');
        out.push(if e >= 0 { '+' } else { '-' });
        out.push_str(&e.unsigned_abs().to_string());
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::es_number;

    /// Hand-labelled ECMAScript forms (RFC 8785 §3.2.2.3 and its Appendix B style).
    #[test]
    fn es_forms() {
        let cases = [
            (0.0, "0"),
            (-0.0, "0"),
            (0.5, "0.5"),
            (2.0, "2"),
            (1e-7, "1e-7"),
            (0.000001, "0.000001"),
            (1e20, "100000000000000000000"),
            (1e21, "1e+21"),
            (1.5e300, "1.5e+300"),
            (5e-324, "5e-324"),
            (-123.456, "-123.456"),
            (core::f64::consts::FRAC_PI_4, "0.7853981633974483"),
        ];
        for (v, want) in cases {
            assert_eq!(es_number(v).as_deref(), Some(want), "{v:e}");
        }
        assert_eq!(es_number(f64::NAN), None);
        assert_eq!(es_number(f64::INFINITY), None);
    }
}
