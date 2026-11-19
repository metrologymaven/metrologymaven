//! The string form of an `f64` in kernel canonical JSON: ECMAScript `Number::toString`
//! (the RFC 8785 §3.2.2.3 number form), written inside a JSON string because kernel
//! canonical JSON emits no numbers (KRN.MAN.KCI).
//!
//! The shortest round-trip digits come from `core`'s `{:e}` formatting, which is integer
//! code (no float arithmetic), so the string is the same on every platform, wasm32 included.

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
