//! The canonical JSON writer of this module.
//!
//! Every byte form this module defines (parameters, dependency declaration, output element,
//! journal event payload, command row) is RFC 8785 canonical JSON, the form `fid-canon`
//! (`CORE.JRN.CANON`) writes: object members sorted by key, no insignificant whitespace,
//! strings escaped as RFC 8785 §3.2.2.2 prescribes. By kernel convention (`KRN.MAN.KCI`) no
//! JSON number is ever written: an `f64` is a string of 16 lowercase hexadecimal digits (its
//! IEEE-754 binary64 bit pattern) and an integer a string of decimal digits. Booleans appear
//! only in command rows (`required`).
//!
//! Every key is a `&'static str` of this module's fixed schemas, ASCII, so byte order equals
//! the UTF-16 order RFC 8785 asks for. The writer cannot fail: keys are unique by schema
//! (unit-tested against `fid_kci`'s general writer) and nesting is at most four levels.

/// A JSON value of a fixed schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum J {
    S(String),
    B(bool),
    A(Vec<J>),
    /// Members in any order; [`write`] sorts them.
    O(Vec<(&'static str, J)>),
}

/// The canonical bytes of `v`.
pub(crate) fn write(v: &J) -> Vec<u8> {
    let mut out = String::new();
    value(&mut out, v);
    out.into_bytes()
}

fn value(out: &mut String, v: &J) {
    match v {
        J::S(s) => string(out, s),
        J::B(b) => out.push_str(if *b { "true" } else { "false" }),
        J::A(items) => {
            out.push('[');
            for (i, x) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                value(out, x);
            }
            out.push(']');
        }
        J::O(members) => {
            let mut sorted: Vec<&(&'static str, J)> = members.iter().collect();
            sorted.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            out.push('{');
            for (i, (k, x)) in sorted.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                string(out, k);
                out.push(':');
                value(out, x);
            }
            out.push('}');
        }
    }
}

fn string(out: &mut String, s: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let b = u32::from(c) as usize;
                out.push_str("\\u00");
                out.push(char::from(HEX[b >> 4]));
                out.push(char::from(HEX[b & 15]));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `x` as 16 lowercase hexadecimal digits of its bit pattern.
pub(crate) fn hex_f64(x: f64) -> String {
    hex_u64(x.to_bits())
}

fn hex_u64(bits: u64) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(16);
    for k in 0..16 {
        s.push(char::from(HEX[((bits >> (60 - 4 * k)) & 0xF) as usize]));
    }
    s
}

/// A string value holding `x` in hex form.
pub(crate) fn f(x: f64) -> J {
    J::S(hex_f64(x))
}

/// An array of hex-form numbers.
pub(crate) fn fs(xs: &[f64]) -> J {
    J::A(xs.iter().map(|&x| f(x)).collect())
}

/// A string value.
pub(crate) fn s(x: &str) -> J {
    J::S(x.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fid_kci::kci::json::{to_canonical, Json};

    fn to_kci(v: &J) -> Option<Json> {
        Some(match v {
            J::S(s) => Json::Str(s.clone()),
            J::B(_) => return None,
            J::A(items) => Json::Arr(items.iter().map(to_kci).collect::<Option<_>>()?),
            J::O(m) => Json::Obj(m.iter().map(|(k, x)| Some(((*k).to_string(), to_kci(x)?))).collect::<Option<_>>()?),
        })
    }

    /// The writer agrees with the kernel's general RFC 8785 writer on string-only values,
    /// including key sorting and escaping.
    #[test]
    fn agrees_with_kci_writer() {
        let v = J::O(vec![
            ("zeta", s("a\"b\\c\n\u{1}")),
            ("alpha", J::A(vec![f(1.0), f(-0.0)])),
            ("mid", J::O(vec![("b", s("x")), ("a", s("y"))])),
        ]);
        assert_eq!(Ok(write(&v)), to_canonical(&to_kci(&v).unwrap_or(Json::Str(String::new()))));
        assert_eq!(hex_f64(1.0), "3ff0000000000000");
        assert_eq!(hex_f64(-0.0), "8000000000000000");
    }
}
