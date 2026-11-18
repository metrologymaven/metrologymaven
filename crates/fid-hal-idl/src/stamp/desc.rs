//! Field descriptors. One table per record type drives both the structural validator and
//! the generated JSON Schemas, so field names, types, patterns, enums, required fields and
//! list and length bounds agree between the two.
//!
//! The schemas do **not** express everything the validator enforces. These rules are
//! enforced by the code only:
//!
//! - decimal strings within the `u64` / `i64` range (the patterns only bound the digits);
//! - a finite claim value (the pattern also admits the bits of NaN and infinity);
//! - reference and feature lists in strictly ascending order (the schema says only
//!   `uniqueItems`);
//! - a sequence range whose last number is not below its first;
//! - gaps with `from <= to`, inside the sequence range, ascending and not overlapping;
//! - `compat_status` is `NOT_RUN` exactly when `compat_report_digest` is null;
//! - `tier` is `TIER1` exactly when `host_digest` is null;
//! - an envelope's batch ID, payload schema and digest, sequence range and gaps equal its
//!   stamp's;
//! - the input limits of 1 MiB and 16 levels of nesting;
//! - the error codes and the "first offending field" order.

use super::model::{
    AlgorithmSource, CalibrationStatus, CompatStatus, DriverTier, GapReason, RecordKind, SourceKind, Trust, WarmupState,
};
use super::{child, StampError, StampErrorKind as K, MAX_GAPS, MAX_LIST, MAX_METHOD_CHARS, MAX_TEXT_CHARS};
use fid_canon::CValue;
use std::collections::BTreeMap;

/// A string pattern: the JSON Schema regex and the Rust check that implements it.
pub(crate) struct Pat {
    pub regex: &'static str,
    pub check: fn(&str) -> bool,
}

/// The type of one field.
pub(crate) enum Ty {
    Bool,
    /// A canonical decimal string of a `u64` (the schema pattern allows up to 20 digits;
    /// the validator also refuses values above `u64::MAX`).
    U64,
    /// A canonical decimal string of an `i64` (range checked as for [`Ty::U64`]).
    I64,
    /// `sha256:` + 64 lowercase hex digits.
    Digest,
    /// A lowercase hyphenated UUIDv7 (RFC 9562).
    Uuid7,
    /// A finite `f64` as `"0x"` + 16 lowercase hex digits of its bits.
    F64,
    Pat(&'static Pat),
    /// Untrusted text of 1 to N Unicode scalar values.
    Text(usize),
    /// A closed enum of wire strings (`stamp_enum_invalid` otherwise).
    Enum(&'static [&'static str]),
    /// The record's `schema` tag `<kind>/<major>`; only major 0 is accepted.
    Tag(&'static str),
    Nullable(&'static Ty),
    /// A list of at most `max` items; `sorted` lists hold strings in strictly ascending order.
    Arr {
        item: &'static Ty,
        max: usize,
        sorted: bool,
    },
    /// A sequence range `[first, last]` of two decimal `u64` strings, `last >= first`.
    Seq,
    Obj(&'static [Field]),
}

/// One field of an object type.
pub(crate) struct Field {
    pub name: &'static str,
    pub ty: Ty,
}

const fn f(name: &'static str, ty: Ty) -> Field {
    Field { name, ty }
}

// ------------------------------------------------------------------ patterns

fn is_dec(s: &str, max_digits: usize) -> bool {
    let b = s.as_bytes();
    match b.first() {
        Some(b'0') => b.len() == 1,
        Some(b'1'..=b'9') => b.len() <= max_digits && b.iter().all(u8::is_ascii_digit),
        _ => false,
    }
}

fn is_ref(s: &str) -> bool {
    (1..=256).contains(&s.len())
        && s.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b':' | b'/' | b'@' | b'+' | b'-'))
}

fn is_ident(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=64).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

fn is_cap_name(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=64).contains(&b.len()) && b[0].is_ascii_uppercase() && b.iter().all(u8::is_ascii_alphanumeric)
}

fn is_cap_version(s: &str) -> bool {
    matches!(s.split_once('.'), Some((a, b)) if is_dec(a, 9) && is_dec(b, 9))
}

fn is_schema_id(s: &str) -> bool {
    let Some((name, major)) = s.split_once('/') else { return false };
    let n = name.as_bytes();
    (1..=63).contains(&n.len())
        && n[0].is_ascii_lowercase()
        && n.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && is_dec(major, 9)
}

pub(crate) fn is_uuid7(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, &c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == b'-'
            } else {
                c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
            }
        })
        && b[14] == b'7'
        && matches!(b[19], b'8' | b'9' | b'a' | b'b')
}

pub(crate) fn f64_hex(s: &str) -> Option<f64> {
    let hex = s.strip_prefix("0x")?;
    if hex.len() != 16 || !hex.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
        return None;
    }
    u64::from_str_radix(hex, 16).ok().map(f64::from_bits)
}

const U64_RE: &str = "^(0|[1-9][0-9]{0,19})$";
const I64_RE: &str = "^(0|-?[1-9][0-9]{0,18})$";
const UUID7_RE: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$";
const F64_RE: &str = "^0x[0-9a-f]{16}$";

const REF: Pat = Pat { regex: "^[A-Za-z0-9._:/@+-]{1,256}$", check: is_ref };
const IDENT: Pat = Pat { regex: "^[a-z][a-z0-9_]{0,63}$", check: is_ident };
const CAP_NAME: Pat = Pat { regex: "^[A-Z][A-Za-z0-9]{0,63}$", check: is_cap_name };
const CAP_VERSION: Pat = Pat { regex: r"^(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})$", check: is_cap_version };
const SCHEMA_ID: Pat = Pat { regex: "^[a-z][a-z0-9-]{0,62}/(0|[1-9][0-9]{0,8})$", check: is_schema_id };

/// Wire strings of `units::TimeSource`, best first (checked against it in a test).
pub(crate) const TIME_SOURCES: &[&str] = &["HW_TRIGGER", "PTP", "DEVICE_CLOCK", "NTP", "HOST_RECEIVE"];
/// Wire symbols of `units::SiUnit` (checked against it in a test).
pub(crate) const SI_UNITS: &[&str] = &["m", "rad", "s", "K", "Pa", "1"];

// ------------------------------------------------------------------ tables

const GAP: &[Field] = &[f("from", Ty::U64), f("reason", Ty::Enum(GapReason::WIRE)), f("to", Ty::U64)];

const CAPABILITY: &[Field] = &[
    f("features", Ty::Arr { item: &Ty::Pat(&IDENT), max: MAX_LIST, sorted: true }),
    f("name", Ty::Pat(&CAP_NAME)),
    f("version", Ty::Pat(&CAP_VERSION)),
];

const DRIVER: &[Field] = &[
    f("compat_report_digest", Ty::Nullable(&Ty::Digest)),
    f("compat_status", Ty::Enum(CompatStatus::WIRE)),
    f("digest", Ty::Digest),
    f("host_digest", Ty::Nullable(&Ty::Digest)),
    f("id", Ty::Pat(&REF)),
    f("signer", Ty::Nullable(&Ty::Pat(&REF))),
    f("tier", Ty::Enum(DriverTier::WIRE)),
    f("version", Ty::Pat(&REF)),
];

const DEVICE: &[Field] = &[
    f("firmware", Ty::Nullable(&Ty::Text(MAX_TEXT_CHARS))),
    f("firmware_in_tested_range", Ty::Nullable(&Ty::Bool)),
    f("make", Ty::Text(MAX_TEXT_CHARS)),
    f("model", Ty::Text(MAX_TEXT_CHARS)),
    f("serial", Ty::Nullable(&Ty::Text(MAX_TEXT_CHARS))),
];

const CALIBRATION: &[Field] = &[
    f("refs", Ty::Arr { item: &Ty::Pat(&REF), max: MAX_LIST, sorted: true }),
    f("status", Ty::Enum(CalibrationStatus::WIRE)),
];

const UTC_ANCHOR: &[Field] = &[f("t_host", Ty::I64), f("utc_ns", Ty::I64)];

const TIME: &[Field] = &[
    f("source", Ty::Enum(TIME_SOURCES)),
    f("sync_sigma_ns", Ty::U64),
    f("utc_anchor", Ty::Nullable(&Ty::Obj(UTC_ANCHOR))),
];

pub(crate) const STAMP: &[Field] = &[
    f("batch_id", Ty::Uuid7),
    f("calibration", Ty::Obj(CALIBRATION)),
    f("capability", Ty::Obj(CAPABILITY)),
    f("config_digest", Ty::Digest),
    f("device", Ty::Obj(DEVICE)),
    f("driver", Ty::Obj(DRIVER)),
    f("encoding_ref", Ty::Pat(&REF)),
    f("environment", Ty::Arr { item: &Ty::Pat(&REF), max: MAX_LIST, sorted: true }),
    f("gaps", Ty::Arr { item: &Ty::Obj(GAP), max: MAX_GAPS, sorted: false }),
    f("payload_digest", Ty::Digest),
    f("payload_schema", Ty::Pat(&SCHEMA_ID)),
    f("schema", Ty::Tag("stamp")),
    f("sensor_model", Ty::Nullable(&Ty::Pat(&REF))),
    f("seq", Ty::Seq),
    f("simulated", Ty::Bool),
    f("source_kind", Ty::Enum(SourceKind::WIRE)),
    f("time", Ty::Obj(TIME)),
    f("transcript_digest", Ty::Nullable(&Ty::Digest)),
    f("trust", Ty::Enum(Trust::WIRE)),
    f("warmup", Ty::Enum(WarmupState::WIRE)),
];

pub(crate) const BATCH: &[Field] = &[
    f("batch_id", Ty::Uuid7),
    f("gaps", Ty::Arr { item: &Ty::Obj(GAP), max: MAX_GAPS, sorted: false }),
    f("payload_digest", Ty::Digest),
    f("payload_schema", Ty::Pat(&SCHEMA_ID)),
    f("schema", Ty::Tag("acquisition-batch")),
    f("seq", Ty::Seq),
    f("stamp", Ty::Obj(STAMP)),
];

const QUANTITY: &[Field] = &[f("unit", Ty::Enum(SI_UNITS)), f("value", Ty::F64)];

const BATCH_REF: &[Field] = &[f("batch_digest", Ty::Digest), f("batch_id", Ty::Uuid7)];

pub(crate) const CLAIM: &[Field] = &[
    f("algorithm_source", Ty::Enum(AlgorithmSource::WIRE)),
    f("batch", Ty::Obj(BATCH_REF)),
    f("method", Ty::Text(MAX_METHOD_CHARS)),
    f("quantity", Ty::Pat(&IDENT)),
    f("schema", Ty::Tag("external-claim")),
    f("value", Ty::Obj(QUANTITY)),
];

// ------------------------------------------------------------------ validator

fn err(kind: K, ptr: &str) -> StampError {
    StampError::new(kind, ptr)
}

/// Splits a schema tag into kind and major; `None` if it is not `<name>/<decimal>`.
fn split_tag(s: &str) -> Option<(&str, &str)> {
    let (kind, major) = s.rsplit_once('/')?;
    is_dec(major, 9).then_some((kind, major))
}

fn check_tag(expected: &str, v: &CValue, ptr: &str) -> Result<(), StampError> {
    let CValue::Str(s) = v else { return Err(err(K::ValueInvalid, ptr)) };
    let (kind, major) = split_tag(s).ok_or_else(|| err(K::ValueInvalid, ptr))?;
    if kind != expected {
        return Err(err(K::EnumInvalid, ptr));
    }
    if major != "0" {
        return Err(err(K::SchemaVersionUnsupported, ptr));
    }
    Ok(())
}

/// Reads the root `schema` tag and picks the record kind.
pub(crate) fn record_kind(v: &CValue) -> Result<RecordKind, StampError> {
    let CValue::Obj(o) = v else { return Err(err(K::ValueInvalid, "")) };
    let tag = o.get("schema").ok_or_else(|| err(K::FieldMissing, "/schema"))?;
    let CValue::Str(s) = tag else { return Err(err(K::ValueInvalid, "/schema")) };
    let (kind, _) = split_tag(s).ok_or_else(|| err(K::ValueInvalid, "/schema"))?;
    let kind =
        RecordKind::ALL.into_iter().find(|k| k.as_str() == kind).ok_or_else(|| err(K::EnumInvalid, "/schema"))?;
    check_tag(kind.as_str(), tag, "/schema")?;
    Ok(kind)
}

fn string<'a>(v: &'a CValue, ptr: &str) -> Result<&'a str, StampError> {
    match v {
        CValue::Str(s) => Ok(s),
        _ => Err(err(K::ValueInvalid, ptr)),
    }
}

pub(crate) fn u64_dec(s: &str) -> Option<u64> {
    if is_dec(s, 20) {
        s.parse().ok()
    } else {
        None
    }
}

pub(crate) fn i64_dec(s: &str) -> Option<i64> {
    let ok = match s.strip_prefix('-') {
        Some(d) => d != "0" && is_dec(d, 19),
        None => is_dec(s, 19),
    };
    if ok {
        s.parse().ok()
    } else {
        None
    }
}

/// Structural check of `v` against `ty`; `ptr` is the pointer of `v`.
pub(crate) fn check(ty: &Ty, v: &CValue, ptr: &str) -> Result<(), StampError> {
    let bad = || err(K::ValueInvalid, ptr);
    match ty {
        Ty::Bool => matches!(v, CValue::Bool(_)).then_some(()).ok_or_else(bad),
        Ty::U64 => u64_dec(string(v, ptr)?).map(|_| ()).ok_or_else(bad),
        Ty::I64 => i64_dec(string(v, ptr)?).map(|_| ()).ok_or_else(bad),
        Ty::Digest => fid_types::Digest::is_valid(string(v, ptr)?).then_some(()).ok_or_else(bad),
        Ty::Uuid7 => is_uuid7(string(v, ptr)?).then_some(()).ok_or_else(bad),
        // The in-memory form `units::Quantity::to_value` writes is accepted too.
        Ty::F64 if matches!(v, CValue::F64Bits(_)) => {
            matches!(v, CValue::F64Bits(x) if x.is_finite()).then_some(()).ok_or_else(bad)
        }
        Ty::F64 => f64_hex(string(v, ptr)?).filter(|x| x.is_finite()).map(|_| ()).ok_or_else(bad),
        Ty::Pat(p) => (p.check)(string(v, ptr)?).then_some(()).ok_or_else(bad),
        Ty::Text(max) => {
            let n = string(v, ptr)?.chars().count();
            (1..=*max).contains(&n).then_some(()).ok_or_else(bad)
        }
        Ty::Enum(names) => match v {
            CValue::Str(s) if names.contains(&s.as_str()) => Ok(()),
            _ => Err(err(K::EnumInvalid, ptr)),
        },
        Ty::Tag(kind) => check_tag(kind, v, ptr),
        Ty::Nullable(inner) => match v {
            CValue::Null => Ok(()),
            v => check(inner, v, ptr),
        },
        Ty::Arr { item, max, sorted } => {
            let CValue::Arr(a) = v else { return Err(bad()) };
            if a.len() > *max {
                return Err(bad());
            }
            for (i, x) in a.iter().enumerate() {
                let p = child(ptr, &i.to_string());
                check(item, x, &p)?;
                if *sorted && i > 0 {
                    let ascending = matches!((&a[i - 1], x), (CValue::Str(prev), CValue::Str(s)) if prev < s);
                    if !ascending {
                        return Err(err(K::ValueInvalid, &p));
                    }
                }
            }
            Ok(())
        }
        Ty::Seq => {
            let CValue::Arr(a) = v else { return Err(bad()) };
            let [first, last] = a.as_slice() else { return Err(bad()) };
            let first_ptr = child(ptr, "0");
            let last_ptr = child(ptr, "1");
            let first = u64_dec(string(first, &first_ptr)?).ok_or_else(|| err(K::ValueInvalid, &first_ptr))?;
            let last = u64_dec(string(last, &last_ptr)?).ok_or_else(|| err(K::ValueInvalid, &last_ptr))?;
            if last < first {
                return Err(err(K::SeqInvalid, ptr));
            }
            Ok(())
        }
        Ty::Obj(fields) => {
            let CValue::Obj(o) = v else { return Err(bad()) };
            if let (Some(tag), Some(value)) = (fields.iter().find(|x| x.name == "schema"), o.get("schema")) {
                check(&tag.ty, value, &child(ptr, "schema"))?;
            }
            let mut names: Vec<&str> = fields.iter().map(|x| x.name).chain(o.keys().map(String::as_str)).collect();
            names.sort_unstable();
            names.dedup();
            for name in names {
                let p = child(ptr, name);
                match (fields.iter().find(|x| x.name == name), o.get(name)) {
                    (None, _) => return Err(err(K::FieldUnknown, &p)),
                    (Some(_), None) => return Err(err(K::FieldMissing, &p)),
                    (Some(field), Some(value)) => {
                        if name != "schema" {
                            check(&field.ty, value, &p)?;
                        }
                    }
                }
            }
            Ok(())
        }
    }
}

// ------------------------------------------------------------------ schema generation

fn o<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn s(text: &str) -> CValue {
    CValue::Str(text.to_string())
}

fn n(x: usize) -> CValue {
    // Bounds are small (at most a few thousand), so the conversion is exact.
    CValue::Num(x as f64)
}

fn pattern(re: &str) -> CValue {
    o([("pattern", s(re)), ("type", s("string"))])
}

/// The JSON Schema of one type.
pub(crate) fn schema(ty: &Ty) -> CValue {
    match ty {
        Ty::Bool => o([("type", s("boolean"))]),
        Ty::U64 => pattern(U64_RE),
        Ty::I64 => pattern(I64_RE),
        Ty::Digest => pattern(fid_types::Digest::PATTERN),
        Ty::Uuid7 => pattern(UUID7_RE),
        Ty::F64 => pattern(F64_RE),
        Ty::Pat(p) => pattern(p.regex),
        Ty::Text(max) => o([("maxLength", n(*max)), ("minLength", n(1)), ("type", s("string"))]),
        Ty::Enum(names) => o([("enum", CValue::Arr(names.iter().map(|x| s(x)).collect()))]),
        Ty::Tag(kind) => o([("const", s(&format!("{kind}/0")))]),
        Ty::Nullable(inner) => o([("anyOf", CValue::Arr(vec![schema(inner), o([("type", s("null"))])]))]),
        Ty::Arr { item, max, sorted } => {
            let mut m = BTreeMap::new();
            m.insert("items".to_string(), schema(item));
            m.insert("maxItems".to_string(), n(*max));
            m.insert("type".to_string(), s("array"));
            if *sorted {
                m.insert("uniqueItems".to_string(), CValue::Bool(true));
            }
            CValue::Obj(m)
        }
        Ty::Seq => o([("items", pattern(U64_RE)), ("maxItems", n(2)), ("minItems", n(2)), ("type", s("array"))]),
        Ty::Obj(fields) => {
            let mut names: Vec<&str> = fields.iter().map(|x| x.name).collect();
            names.sort_unstable();
            o([
                ("additionalProperties", CValue::Bool(false)),
                ("properties", CValue::Obj(fields.iter().map(|x| (x.name.to_string(), schema(&x.ty))).collect())),
                ("required", CValue::Arr(names.into_iter().map(s).collect())),
                ("type", s("object")),
            ])
        }
    }
}

/// The full schema document of a record kind.
pub(crate) fn schema_document(kind: RecordKind) -> CValue {
    let mut doc = match schema(&Ty::Obj(kind.descriptor_fields())) {
        CValue::Obj(m) => m,
        _ => BTreeMap::new(),
    };
    let file = kind.schema_file();
    doc.insert("$id".to_string(), s(&format!("https://metrologymaven.io/schemas/hal/{file}")));
    doc.insert("$schema".to_string(), s("https://json-schema.org/draft/2020-12/schema"));
    doc.insert("title".to_string(), s(&format!("{}/0", kind.as_str())));
    doc.insert("description".to_string(), s(kind.description()));
    CValue::Obj(doc)
}

/// Writes `v` as JSON with sorted keys and a two-space indent (the layout of Python's
/// `json.dumps(v, indent=2, sort_keys=True, ensure_ascii=False)`); scalars are written in
/// their RFC 8785 form.
pub(crate) fn pretty(v: &CValue, indent: usize, out: &mut String) -> Result<(), StampError> {
    let pad = |out: &mut String, k: usize| out.push_str(&" ".repeat(k));
    match v {
        CValue::Arr(a) if !a.is_empty() => {
            out.push_str("[\n");
            for (i, x) in a.iter().enumerate() {
                pad(out, indent + 2);
                pretty(x, indent + 2, out)?;
                out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
            }
            pad(out, indent);
            out.push(']');
        }
        CValue::Obj(m) if !m.is_empty() => {
            out.push_str("{\n");
            for (i, (k, x)) in m.iter().enumerate() {
                pad(out, indent + 2);
                pretty(&CValue::Str(k.clone()), indent + 2, out)?;
                out.push_str(": ");
                pretty(x, indent + 2, out)?;
                out.push_str(if i + 1 < m.len() { ",\n" } else { "\n" });
            }
            pad(out, indent);
            out.push('}');
        }
        scalar => {
            let bytes = fid_canon::to_canonical_bytes(scalar).map_err(|_| err(K::JsonInvalid, ""))?;
            out.push_str(&String::from_utf8(bytes).map_err(|_| err(K::JsonInvalid, ""))?);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{SiUnit, TimeSource};

    #[test]
    fn enum_tables_match_units() {
        let ts: Vec<&str> = TimeSource::ALL.iter().map(|t| t.as_str()).collect();
        assert_eq!(ts, TIME_SOURCES);
        let us: Vec<&str> = SiUnit::ALL.iter().map(|u| u.as_str()).collect();
        assert_eq!(us, SI_UNITS);
    }

    #[test]
    fn tables_are_sorted() {
        for t in
            [GAP, CAPABILITY, DRIVER, DEVICE, CALIBRATION, UTC_ANCHOR, TIME, STAMP, BATCH, QUANTITY, BATCH_REF, CLAIM]
        {
            assert!(t.windows(2).all(|w| w[0].name < w[1].name));
        }
    }

    #[test]
    fn patterns() {
        assert!(is_ref("cal:2026-01/abc@1+x_y.z"));
        assert!(!is_ref("") && !is_ref("a b") && !is_ref(&"a".repeat(257)));
        assert!(is_cap_version("1.0") && is_cap_version("0.12"));
        assert!(!is_cap_version("01.0") && !is_cap_version("1") && !is_cap_version("1.0.0"));
        assert!(is_schema_id("line-scan-profile/1") && !is_schema_id("LineScan/1") && !is_schema_id("a/01"));
        assert!(is_uuid7("01890a5d-ac96-774b-bcce-b302099a8057"));
        assert!(!is_uuid7("01890A5D-AC96-774B-BCCE-B302099A8057"));
        assert!(!is_uuid7("01890a5d-ac96-574b-bcce-b302099a8057"));
        assert!(!is_uuid7("01890a5d-ac96-774b-ccce-b302099a8057"));
        assert_eq!(u64_dec("18446744073709551615"), Some(u64::MAX));
        assert_eq!(u64_dec("18446744073709551616"), None);
        assert_eq!(u64_dec("01"), None);
        assert_eq!(i64_dec("-9223372036854775808"), Some(i64::MIN));
        assert_eq!(i64_dec("-0"), None);
        assert_eq!(f64_hex("0x3ff0000000000000"), Some(1.0));
        assert_eq!(f64_hex("0x3FF0000000000000"), None);
    }
}
