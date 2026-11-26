//! Integer and real tokens (REQ-IO.CAD.IGES.SECT-005).
//!
//! A real token matches `[+-]?([0-9]+(\.[0-9]*)?|\.[0-9]+)([EeDd][+-]?[0-9]+)?` and has a
//! decimal point or an exponent (or both); an integer token matches `[+-]?[0-9]+`. The `D`
//! exponent marks double precision in IGES (5.3 §2.2.2 [unverified: clause]); it changes
//! nothing about the value, which is always the correctly rounded `f64`.

/// The shape of a numeric token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape {
    Int,
    Real,
}

/// The shape of `s`, or `None` if it is neither an integer nor a real token.
pub(crate) fn shape(s: &[u8]) -> Option<Shape> {
    let mut i = 0;
    if matches!(s.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    let int0 = i;
    while matches!(s.get(i), Some(c) if c.is_ascii_digit()) {
        i += 1;
    }
    let int_digits = i - int0;
    let mut point = false;
    let mut frac_digits = 0;
    if s.get(i) == Some(&b'.') {
        point = true;
        i += 1;
        let f0 = i;
        while matches!(s.get(i), Some(c) if c.is_ascii_digit()) {
            i += 1;
        }
        frac_digits = i - f0;
    }
    if int_digits == 0 && frac_digits == 0 {
        return None;
    }
    let mut exp = false;
    if matches!(s.get(i), Some(b'E' | b'e' | b'D' | b'd')) {
        exp = true;
        i += 1;
        if matches!(s.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let e0 = i;
        while matches!(s.get(i), Some(c) if c.is_ascii_digit()) {
            i += 1;
        }
        if i == e0 {
            return None;
        }
    }
    if i != s.len() {
        return None;
    }
    Some(if point || exp { Shape::Real } else { Shape::Int })
}

/// The correctly rounded value of a real token (any shape [`shape`] accepts), or `None`
/// if it is not finite. Rust's float parser rounds correctly (round-half-even), and its
/// accepted grammar covers ours once `D`/`d` is written `e`.
pub(crate) fn value(s: &[u8]) -> Option<f64> {
    let mut t = String::with_capacity(s.len());
    for &b in s {
        t.push(match b {
            b'D' | b'd' => 'e',
            other => char::from(other),
        });
    }
    match t.parse::<f64>() {
        Ok(v) if v.is_finite() => Some(v),
        _ => None,
    }
}

/// The shortest round-trip spelling of a finite `v` (Rust's `{:?}` digits with an upper
/// case `E`), e.g. `1.0`, `0.1`, `1E300`, `-0.0`.
pub(crate) fn shortest(v: f64) -> String {
    format!("{v:?}").replace('e', "E")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn shapes_are_hand_labelled() {
        for s in ["0", "-12", "+7", "007"] {
            assert_eq!(shape(s.as_bytes()), Some(Shape::Int), "{s}");
        }
        for s in ["1.", ".5", "-0.0", "1.5D-3", "1E5", "2d0", "+3.e+2", "1.0D+00"] {
            assert_eq!(shape(s.as_bytes()), Some(Shape::Real), "{s}");
        }
        for s in ["", ".", "+", "-.", "1.E", "1E+", "1.5x", "--1", "1..", " 1", "1 ", "1H", "inf", "NaN", "0x1"] {
            assert_eq!(shape(s.as_bytes()), None, "{s}");
        }
    }

    #[test]
    fn values_are_hand_labelled() {
        let cases: &[(&str, f64)] = &[
            ("1.5D-3", 0.0015),
            ("1.5d-3", 0.0015),
            ("-3.25D+2", -325.0),
            (".5", 0.5),
            ("1.", 1.0),
            ("1E2", 100.0),
            ("9007199254740993.0", 9_007_199_254_740_992.0),
        ];
        for &(s, v) in cases {
            assert_eq!(value(s.as_bytes()).unwrap().to_bits(), v.to_bits(), "{s}");
        }
        assert_eq!(value(b"1.D999"), None);
        assert_eq!(value(b"-1.E400"), None);
    }

    #[test]
    fn shortest_round_trips() {
        for v in [0.0, -0.0, 1.0, 0.1, 1e300, 1.5e-7, 5e-324, f64::MAX, -123.456] {
            let s = shortest(v);
            assert_eq!(shape(s.as_bytes()), Some(Shape::Real), "{s}");
            assert_eq!(value(s.as_bytes()).unwrap().to_bits(), v.to_bits(), "{s}");
        }
    }
}
