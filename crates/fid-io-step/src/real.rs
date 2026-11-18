//! Real tokens: lexeme check and shortest round-trip formatting (REQ-IO.CAD.STEP.P21-005).

/// True if `s` matches `[+-]?[0-9]+\.[0-9]*([Ee][+-]?[0-9]+)?`.
pub(crate) fn lexeme_ok(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    let d0 = i;
    while matches!(b.get(i), Some(c) if c.is_ascii_digit()) {
        i += 1;
    }
    if i == d0 || b.get(i) != Some(&b'.') {
        return false;
    }
    i += 1;
    while matches!(b.get(i), Some(c) if c.is_ascii_digit()) {
        i += 1;
    }
    if matches!(b.get(i), Some(b'E' | b'e')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let e0 = i;
        while matches!(b.get(i), Some(c) if c.is_ascii_digit()) {
            i += 1;
        }
        if i == e0 {
            return false;
        }
    }
    i == b.len()
}

/// The shortest decimal real token that parses back to exactly `v` (finite). Uses the
/// shortest round-trip digits of `{:e}`; positional form when the decimal exponent is in
/// -5..=15 (`0.001`, `123.`), `d.dddE<exp>` otherwise (`1.E300`, `1.5E-7`).
pub(crate) fn shortest(v: f64) -> String {
    let sci = format!("{v:e}");
    let (mant, exp) = match sci.split_once('e') {
        Some(parts) => parts,
        None => return sci,
    };
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mant.chars().filter(char::is_ascii_digit).collect();
    let mut s = String::new();
    if mant.starts_with('-') {
        s.push('-');
    }
    if (-5..=15).contains(&exp) {
        if exp >= 0 {
            let k = usize::try_from(exp).unwrap_or(0).saturating_add(1);
            if digits.len() <= k {
                s.push_str(&digits);
                s.push_str(&"0".repeat(k - digits.len()));
                s.push('.');
            } else {
                s.push_str(digits.get(..k).unwrap_or(""));
                s.push('.');
                s.push_str(digits.get(k..).unwrap_or(""));
            }
        } else {
            s.push_str("0.");
            s.push_str(&"0".repeat(usize::try_from(-exp - 1).unwrap_or(0)));
            s.push_str(&digits);
        }
    } else {
        s.push_str(digits.get(..1).unwrap_or("0"));
        s.push('.');
        s.push_str(digits.get(1..).unwrap_or(""));
        s.push('E');
        s.push_str(&exp.to_string());
    }
    s
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn shortest_forms_are_hand_labelled() {
        let cases: &[(f64, &str)] = &[
            (0.0, "0."),
            (-0.0, "-0."),
            (1.0, "1."),
            (-1.5, "-1.5"),
            (0.1, "0.1"),
            (100.0, "100."),
            (1e300, "1.E300"),
            (1.5e-7, "1.5E-7"),
            (1e-5, "0.00001"),
            (1e15, "1000000000000000."),
            (1e16, "1.E16"),
            (123.456, "123.456"),
        ];
        for &(v, want) in cases {
            assert_eq!(shortest(v), want, "{v:e}");
            let back: f64 = want.parse().unwrap();
            assert_eq!(back.to_bits(), v.to_bits());
            assert!(lexeme_ok(want));
        }
    }

    #[test]
    fn lexeme_grammar() {
        for ok in ["0.", "-0.", "+1.5", "1.E5", "1.e-5", "12.34E+02"] {
            assert!(lexeme_ok(ok), "{ok}");
        }
        for bad in ["", "1", ".5", "1e5", "1.E", "1.E+", "1.5x", "--1.", "1..", " 1."] {
            assert!(!lexeme_ok(bad), "{bad}");
        }
    }
}
