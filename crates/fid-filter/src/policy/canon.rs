//! Canonical encoding of a [`PolicySpec`] (REQ-KRN.OUT.POL-001, AC-1).
//!
//! # Pinned format
//!
//! Compact JSON (no whitespace), ASCII only, keys in the fixed order below (not sorted):
//!
//! ```text
//! {"policy":{"kind":"<method>",<fields>},"max_excluded_fraction":<f>,"target":"<primitive>"}
//! ```
//!
//! `<method>` is the [`PolicyKind`] slug and `<primitive>` the `fid_fit` primitive slug. The
//! fields follow the declaration order of [`OutlierPolicy`]: `nsigma: k, side`;
//! `robust_scale: k, scale, side`; `ransac: threshold_m, iterations, min_inliers, seed`;
//! `nominal_gate: max_distance_m`; `tolerance_band: lower_m, upper_m`. `<f>` is an `f64` as a
//! JSON string of exactly 16 lowercase hexadecimal digits, its IEEE-754 binary64 bit pattern
//! (the convention of `fid_fit::contract::encode`); integers are JSON strings of decimal
//! digits without sign or leading zeros; `side` and `scale` are their slugs.
//!
//! # Decoding (pinned)
//!
//! Input longer than [`MAX_ENCODED_BYTES`]: `encoding` / `too_long`. The bytes are parsed by a
//! strict reader that accepts only objects and strings, no whitespace, strings of printable
//! ASCII without escapes, and at most three nested objects; anything else is `encoding` /
//! `malformed`. Then, for the top object and the policy object, each expected key in encoding
//! order: absent is `<key>` / `missing`; an object where a string belongs (or the reverse for
//! `policy`) is `wrong_type`; a float that is not 16 lowercase hex digits is `bad_float`; an
//! integer that is not canonical decimal is `bad_integer`, and one too large for its type is
//! `out_of_range`; an unknown slug is `unknown_value`. After the expected keys of an object,
//! any key not expected is `encoding` / `unexpected_key` and a repeated key is
//! `encoding` / `duplicate_key`. The decoded spec is validated
//! ([`PolicySpec::validate`]); finally, input that is not byte-identical to the encoding of
//! the decoded spec (for example reordered keys) is `encoding` / `not_canonical`.

use fid_fit::contract::Primitive;

use super::error::{alloc, invalid, PolicyError};
use super::{OutlierPolicy, PolicyKind, PolicySpec, ScaleEstimator, Side};

/// Longest accepted encoding, in bytes. The longest valid encoding (RANSAC) is under 300.
pub const MAX_ENCODED_BYTES: usize = 1024;

/// Deepest accepted object nesting (the top object is level 1).
const MAX_OBJECT_NESTING: usize = 3;

const OP: &str = "policy.encode";

struct Out {
    buf: Vec<u8>,
}

impl Out {
    fn put(&mut self, b: &[u8]) -> Result<(), PolicyError> {
        self.buf.try_reserve(b.len()).map_err(|_| alloc(OP))?;
        self.buf.extend_from_slice(b);
        Ok(())
    }

    fn key(&mut self, k: &str) -> Result<(), PolicyError> {
        self.put(b",\"")?;
        self.put(k.as_bytes())?;
        self.put(b"\":")
    }

    fn s(&mut self, v: &str) -> Result<(), PolicyError> {
        self.put(b"\"")?;
        self.put(v.as_bytes())?;
        self.put(b"\"")
    }

    fn f(&mut self, x: f64) -> Result<(), PolicyError> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let bits = x.to_bits();
        let mut b = [0u8; 18];
        b[0] = b'"';
        b[17] = b'"';
        for (k, slot) in b[1..17].iter_mut().enumerate() {
            *slot = HEX[((bits >> (60 - 4 * k)) & 0xF) as usize];
        }
        self.put(&b)
    }

    fn i(&mut self, v: u64) -> Result<(), PolicyError> {
        let mut digits = [0u8; 20];
        let mut pos = digits.len();
        let mut x = v;
        loop {
            pos -= 1;
            digits[pos] = b'0' + (x % 10) as u8;
            x /= 10;
            if x == 0 {
                break;
            }
        }
        self.put(b"\"")?;
        self.put(&digits[pos..])?;
        self.put(b"\"")
    }
}

/// Writes the pinned form of `spec` (assumed valid; [`PolicySpec::encode`] validates).
pub(super) fn encode(spec: &PolicySpec) -> Result<Vec<u8>, PolicyError> {
    let mut o = Out { buf: Vec::new() };
    o.put(b"{\"policy\":{\"kind\":")?;
    o.s(spec.policy.kind().slug())?;
    match spec.policy {
        OutlierPolicy::NSigma { k, side } => {
            o.key("k")?;
            o.f(k)?;
            o.key("side")?;
            o.s(side.slug())?;
        }
        OutlierPolicy::RobustScale { k, scale, side } => {
            o.key("k")?;
            o.f(k)?;
            o.key("scale")?;
            o.s(scale.slug())?;
            o.key("side")?;
            o.s(side.slug())?;
        }
        OutlierPolicy::Ransac { threshold_m, iterations, min_inliers, seed } => {
            o.key("threshold_m")?;
            o.f(threshold_m)?;
            o.key("iterations")?;
            o.i(u64::from(iterations))?;
            o.key("min_inliers")?;
            o.i(u64::from(min_inliers))?;
            o.key("seed")?;
            o.i(seed)?;
        }
        OutlierPolicy::NominalGate { max_distance_m } => {
            o.key("max_distance_m")?;
            o.f(max_distance_m)?;
        }
        OutlierPolicy::ToleranceBand { lower_m, upper_m } => {
            o.key("lower_m")?;
            o.f(lower_m)?;
            o.key("upper_m")?;
            o.f(upper_m)?;
        }
    }
    o.put(b"}")?;
    o.key("max_excluded_fraction")?;
    o.f(spec.max_excluded_fraction)?;
    o.key("target")?;
    o.s(spec.target.slug())?;
    o.put(b"}")?;
    Ok(o.buf)
}

/// A parsed value: a string or an object with its members in input order.
enum Node<'a> {
    Str(&'a str),
    Obj(Vec<(&'a str, Node<'a>)>),
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

const MALFORMED: PolicyError = invalid("encoding", "malformed");

impl<'a> Reader<'a> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn string(&mut self) -> Result<&'a str, PolicyError> {
        if !self.eat(b'"') {
            return Err(MALFORMED);
        }
        let start = self.i;
        loop {
            match self.peek() {
                Some(b'"') => break,
                Some(c) if (0x20..=0x7e).contains(&c) && c != b'\\' => self.i += 1,
                _ => return Err(MALFORMED),
            }
        }
        let s = self.b.get(start..self.i).ok_or(MALFORMED)?;
        self.i += 1;
        core::str::from_utf8(s).map_err(|_| MALFORMED)
    }

    fn value(&mut self, level: usize) -> Result<Node<'a>, PolicyError> {
        if !self.eat(b'{') {
            return self.string().map(Node::Str);
        }
        if level > MAX_OBJECT_NESTING {
            return Err(MALFORMED);
        }
        let mut members = Vec::new();
        if self.eat(b'}') {
            return Ok(Node::Obj(members));
        }
        loop {
            let k = self.string()?;
            if !self.eat(b':') {
                return Err(MALFORMED);
            }
            let v = self.value(level + 1)?;
            members.try_reserve(1).map_err(|_| alloc("policy.decode"))?;
            members.push((k, v));
            if self.eat(b',') {
                continue;
            }
            if self.eat(b'}') {
                return Ok(Node::Obj(members));
            }
            return Err(MALFORMED);
        }
    }
}

fn get<'n, 'a>(members: &'n [(&'a str, Node<'a>)], key: &'static str) -> Result<&'n Node<'a>, PolicyError> {
    members.iter().find(|(k, _)| *k == key).map(|(_, v)| v).ok_or(invalid(key, "missing"))
}

fn get_str<'a>(members: &[(&'a str, Node<'a>)], key: &'static str) -> Result<&'a str, PolicyError> {
    match get(members, key)? {
        Node::Str(s) => Ok(s),
        Node::Obj(_) => Err(invalid(key, "wrong_type")),
    }
}

fn get_f64(members: &[(&str, Node<'_>)], key: &'static str) -> Result<f64, PolicyError> {
    let s = get_str(members, key)?;
    if s.len() != 16 {
        return Err(invalid(key, "bad_float"));
    }
    let mut bits = 0u64;
    for c in s.bytes() {
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => return Err(invalid(key, "bad_float")),
        };
        bits = (bits << 4) | u64::from(d);
    }
    Ok(f64::from_bits(bits))
}

fn get_u64(members: &[(&str, Node<'_>)], key: &'static str, max: u64) -> Result<u64, PolicyError> {
    let s = get_str(members, key)?;
    let b = s.as_bytes();
    if b.is_empty() || !b.iter().all(u8::is_ascii_digit) || (b.len() > 1 && b[0] == b'0') {
        return Err(invalid(key, "bad_integer"));
    }
    let mut v = 0u64;
    for &c in b {
        v = v.checked_mul(10).and_then(|v| v.checked_add(u64::from(c - b'0'))).ok_or(invalid(key, "out_of_range"))?;
    }
    if v > max {
        return Err(invalid(key, "out_of_range"));
    }
    Ok(v)
}

fn get_u32(members: &[(&str, Node<'_>)], key: &'static str) -> Result<u32, PolicyError> {
    let v = get_u64(members, key, u64::from(u32::MAX))?;
    u32::try_from(v).map_err(|_| invalid(key, "out_of_range"))
}

fn get_enum<'a, T>(
    members: &[(&'a str, Node<'a>)],
    key: &'static str,
    parse: fn(&str) -> Option<T>,
) -> Result<T, PolicyError> {
    parse(get_str(members, key)?).ok_or(invalid(key, "unknown_value"))
}

/// After the expected keys were read: an extra key is `unexpected_key`, otherwise a count
/// above the expected one means a repeated key.
fn check_keys(members: &[(&str, Node<'_>)], expected: &[&str]) -> Result<(), PolicyError> {
    if members.len() == expected.len() {
        return Ok(());
    }
    if members.iter().any(|(k, _)| !expected.contains(k)) {
        return Err(invalid("encoding", "unexpected_key"));
    }
    Err(invalid("encoding", "duplicate_key"))
}

fn policy_of(pm: &[(&str, Node<'_>)]) -> Result<OutlierPolicy, PolicyError> {
    let kind = get_enum(pm, "kind", PolicyKind::parse)?;
    let (policy, keys): (OutlierPolicy, &[&str]) = match kind {
        PolicyKind::NSigma => (
            OutlierPolicy::NSigma { k: get_f64(pm, "k")?, side: get_enum(pm, "side", Side::parse)? },
            &["kind", "k", "side"],
        ),
        PolicyKind::RobustScale => (
            OutlierPolicy::RobustScale {
                k: get_f64(pm, "k")?,
                scale: get_enum(pm, "scale", ScaleEstimator::parse)?,
                side: get_enum(pm, "side", Side::parse)?,
            },
            &["kind", "k", "scale", "side"],
        ),
        PolicyKind::Ransac => (
            OutlierPolicy::Ransac {
                threshold_m: get_f64(pm, "threshold_m")?,
                iterations: get_u32(pm, "iterations")?,
                min_inliers: get_u32(pm, "min_inliers")?,
                seed: get_u64(pm, "seed", u64::MAX)?,
            },
            &["kind", "threshold_m", "iterations", "min_inliers", "seed"],
        ),
        PolicyKind::NominalGate => {
            (OutlierPolicy::NominalGate { max_distance_m: get_f64(pm, "max_distance_m")? }, &["kind", "max_distance_m"])
        }
        PolicyKind::ToleranceBand => (
            OutlierPolicy::ToleranceBand { lower_m: get_f64(pm, "lower_m")?, upper_m: get_f64(pm, "upper_m")? },
            &["kind", "lower_m", "upper_m"],
        ),
    };
    check_keys(pm, keys)?;
    Ok(policy)
}

/// Parses, validates and re-encodes (pinned order in the module documentation).
pub(super) fn decode(bytes: &[u8]) -> Result<PolicySpec, PolicyError> {
    if bytes.len() > MAX_ENCODED_BYTES {
        return Err(invalid("encoding", "too_long"));
    }
    let mut r = Reader { b: bytes, i: 0 };
    let top = r.value(1)?;
    if r.i != bytes.len() {
        return Err(MALFORMED);
    }
    let Node::Obj(tm) = top else {
        return Err(MALFORMED);
    };
    let policy = match get(&tm, "policy")? {
        Node::Obj(pm) => policy_of(pm)?,
        Node::Str(_) => return Err(invalid("policy", "wrong_type")),
    };
    let max_excluded_fraction = get_f64(&tm, "max_excluded_fraction")?;
    let target = get_enum(&tm, "target", Primitive::parse)?;
    check_keys(&tm, &["policy", "max_excluded_fraction", "target"])?;
    let spec = PolicySpec { policy, max_excluded_fraction, target };
    if spec.encode()? != bytes {
        return Err(invalid("encoding", "not_canonical"));
    }
    Ok(spec)
}
