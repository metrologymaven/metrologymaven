//! Integers that `f64` cannot hold exactly (maintainer decision in the review of #341: fail
//! closed). A parameter document is parsed into `f64` numbers, so `9007199254740993` would
//! silently become `9007199254740992`. Before validation, [`first_inexact_integer`] scans the
//! JSON text (already accepted by the parser) for number tokens whose value is an integer of
//! magnitude above 2^53 that no `f64` equals; the first one in document order is refused
//! with `validation/schema` under the pseudo-keyword `json`. Integers up to 2^53, integers
//! above it that are exactly representable (`1e20`, `2^60` written out) and numbers with a
//! fractional part are accepted; only the text entry point can see this, since a parsed
//! value has already been rounded.
//!
//! An integer `D * 10^e` (`D` without trailing zeros) is exactly representable when the odd
//! part of `D * 5^e` is below 2^53; the work is charged against [`MAX_SCAN_WORK`] limb
//! operations, beyond which the scan stops with `input_limit`.

use super::pointer::JsonPointer;
use fid_canon::{parse, CValue};

/// Most base-10^9 limb operations spent deciding exactness for one document.
pub const MAX_SCAN_WORK: u64 = 20_000_000;

/// 2^53.
const TWO_53: u128 = 9_007_199_254_740_992;

/// What the scan found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scan {
    /// No inexact integer.
    Clean,
    /// The first inexact integer in document order.
    Inexact(JsonPointer),
    /// The work budget ran out.
    TooMuchWork,
}

enum Frame {
    Obj { key: Option<String>, expect_key: bool },
    Arr(usize),
}

fn pointer(stack: &[Frame]) -> JsonPointer {
    let mut p = JsonPointer::root();
    for f in stack {
        p = match f {
            Frame::Obj { key: Some(k), .. } => p.key(k),
            Frame::Obj { key: None, .. } => p,
            Frame::Arr(i) => p.index(*i),
        };
    }
    p
}

/// Decimal digits as little-endian base-10^9 limbs.
fn limbs(digits: &[u8]) -> Vec<u32> {
    let mut out = Vec::with_capacity(digits.len() / 9 + 1);
    let mut end = digits.len();
    while end > 0 {
        let start = end.saturating_sub(9);
        let v = digits.get(start..end).unwrap_or(&[]).iter().fold(0u32, |a, d| a * 10 + u32::from(d - b'0'));
        out.push(v);
        end = start;
    }
    out
}

/// Whether the number token `tok` (valid JSON) is exact, or `None` when out of work.
fn exact(tok: &[u8], work: &mut u64) -> Option<bool> {
    let t = tok.strip_prefix(b"-").unwrap_or(tok);
    let (mant, exp) = match t.iter().position(|b| *b == b'e' || *b == b'E') {
        Some(i) => {
            let text = std::str::from_utf8(t.get(i + 1..)?).ok()?;
            match text.parse::<i64>() {
                Ok(e) => (t.get(..i)?, e),
                // An exponent beyond i64: negative means a value below any integer but zero,
                // so not an integer above 2^53 (re-review of #341); positive is huge.
                Err(_) if text.starts_with('-') => return Some(true),
                Err(_) => (t.get(..i)?, i64::MAX),
            }
        }
        None => (t, 0),
    };
    let (int, frac) = match mant.iter().position(|b| *b == b'.') {
        Some(i) => (mant.get(..i)?, mant.get(i + 1..)?),
        None => (mant, &mant[..0]),
    };
    let digits: Vec<u8> = int.iter().chain(frac.iter()).copied().collect();
    let trailing = digits.iter().rev().take_while(|d| **d == b'0').count();
    let leading = digits.iter().take_while(|d| **d == b'0').count();
    if leading == digits.len() {
        return Some(true);
    }
    let d = digits.get(leading..digits.len() - trailing)?;
    let e = exp.saturating_sub(frac.len() as i64).saturating_add(trailing as i64);
    if e < 0 {
        return Some(true); // not an integer
    }
    if (d.len() as i64).saturating_add(e) <= 15 {
        return Some(true); // below 10^15 < 2^53
    }
    if (d.len() as i64).saturating_add(e) <= 17 {
        let mut v: u128 = d.iter().fold(0u128, |a, x| a * 10 + u128::from(x - b'0'));
        for _ in 0..e {
            v *= 10;
        }
        if v <= TWO_53 {
            return Some(true);
        }
    }
    // Above 2^53: exact iff odd(D) * 5^e < 2^53.
    if e > 22 {
        return Some(false); // 5^23 > 2^53
    }
    let mut l = limbs(d);
    loop {
        let low = l.first().copied().unwrap_or(0);
        if low % 2 == 1 {
            break;
        }
        *work = work.saturating_add(l.len() as u64);
        if *work > MAX_SCAN_WORK {
            return None;
        }
        let mut carry: u64 = 0;
        for limb in l.iter_mut().rev() {
            let cur = carry * 1_000_000_000 + u64::from(*limb);
            *limb = (cur / 2) as u32;
            carry = cur % 2;
        }
        while l.len() > 1 && l.last() == Some(&0) {
            l.pop();
        }
    }
    if l.len() > 2 {
        return Some(false); // odd part >= 10^18 > 2^53
    }
    let odd = l.iter().rev().fold(0u128, |a, x| a * 1_000_000_000 + u128::from(*x));
    Some(odd.saturating_mul(5u128.pow(e as u32)) < TWO_53)
}

/// Scans JSON text the parser accepted for integers no `f64` equals.
pub fn first_inexact_integer(text: &[u8]) -> Scan {
    let mut stack: Vec<Frame> = Vec::new();
    let mut first: Option<JsonPointer> = None;
    let mut work: u64 = 0;
    let mut i = 0;
    while let Some(&b) = text.get(i) {
        match b {
            b'{' => {
                stack.push(Frame::Obj { key: None, expect_key: true });
                i += 1;
            }
            b'[' => {
                stack.push(Frame::Arr(0));
                i += 1;
            }
            b'}' | b']' => {
                stack.pop();
                i += 1;
            }
            b',' => {
                match stack.last_mut() {
                    Some(Frame::Arr(n)) => *n += 1,
                    Some(Frame::Obj { expect_key, .. }) => *expect_key = true,
                    None => {}
                }
                i += 1;
            }
            b'"' => {
                let mut j = i + 1;
                while let Some(&c) = text.get(j) {
                    match c {
                        b'\\' => j += 2,
                        b'"' => break,
                        _ => j += 1,
                    }
                }
                if let Some(Frame::Obj { key, expect_key }) = stack.last_mut() {
                    if *expect_key {
                        *key = match text.get(i..=j).map(parse) {
                            Some(Ok(CValue::Str(k))) => Some(k),
                            _ => None,
                        };
                        *expect_key = false;
                    }
                }
                i = j + 1;
            }
            b'-' | b'0'..=b'9' => {
                let start = i;
                while text.get(i).is_some_and(|c| matches!(c, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')) {
                    i += 1;
                }
                match exact(text.get(start..i).unwrap_or(&[]), &mut work) {
                    None => return Scan::TooMuchWork,
                    Some(true) => {}
                    Some(false) => {
                        let p = pointer(&stack);
                        if first.as_ref().map_or(true, |f| p.doc_cmp(f).is_lt()) {
                            first = Some(p);
                        }
                    }
                }
            }
            _ => i += 1,
        }
    }
    match first {
        Some(p) => Scan::Inexact(p),
        None => Scan::Clean,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn ex(s: &str) -> bool {
        exact(s.as_bytes(), &mut 0).unwrap()
    }

    /// Hand-labelled: 2^53 = 9007199254740992, 2^60 = 1152921504606846976,
    /// 2^200 = 1606938044258990275541962092341162602522202993782792835301376.
    #[test]
    fn exactness() {
        for s in ["0", "-0", "1", "9007199254740992", "-9007199254740992", "9007199254740994", "1e20", "1E22"] {
            assert!(ex(s), "{s}");
        }
        for s in
            ["1152921504606846976", "1606938044258990275541962092341162602522202993782792835301376", "0.5", "1.25e1"]
        {
            assert!(ex(s), "{s}");
        }
        assert!(ex("1e-99999999999999999999999") && ex("12345678901234567890e-99999999999999999999"));
        for s in
            ["9007199254740993", "-9007199254740993", "1e23", "1e30", "123456789012345678901", "3e99999999999999999999"]
        {
            assert!(!ex(s), "{s}");
        }
    }

    #[test]
    fn pointers_in_document_order() {
        let t = br#"{"z":9007199254740993,"a":[1,{"k\"ey":18014398509481985}]}"#;
        assert_eq!(first_inexact_integer(t), Scan::Inexact(JsonPointer::root().key("a").index(1).key("k\"ey")));
        assert_eq!(first_inexact_integer(br#"{"a":"9007199254740993","b":1.5}"#), Scan::Clean);
        assert_eq!(first_inexact_integer(b"9007199254740993"), Scan::Inexact(JsonPointer::root()));
    }
}
