//! Strict readers over `fid_canon` values and small writers over `fid_kci` canonical JSON.

use std::collections::BTreeMap;

use fid_canon::CValue;
use fid_kci::kci::json::{to_canonical, Json};
use fid_kci::Digest;

pub(crate) type Obj = BTreeMap<String, CValue>;

/// Parses `bytes` as JSON (duplicate keys refused) and checks that they are already in
/// canonical form, so a re-encoding gives the same bytes.
pub(crate) fn parse_canonical(bytes: &[u8], max_bytes: usize) -> Result<CValue, String> {
    let v = fid_canon::parse_with_limits(bytes, max_bytes, 8).map_err(|e| format!("json: {}", e.code()))?;
    let again = fid_canon::to_canonical_bytes(&v).map_err(|e| format!("json: {}", e.code()))?;
    if again != bytes {
        return Err("not_canonical".to_string());
    }
    Ok(v)
}

/// The object, with exactly the `required` keys plus any of the `optional` ones.
pub(crate) fn object<'a>(v: &'a CValue, required: &[&str], optional: &[&str]) -> Result<&'a Obj, String> {
    let CValue::Obj(m) = v else {
        return Err("not_an_object".to_string());
    };
    for k in required {
        if !m.contains_key(*k) {
            return Err(format!("missing_key: {k}"));
        }
    }
    if let Some(k) = m.keys().find(|k| !required.contains(&k.as_str()) && !optional.contains(&k.as_str())) {
        return Err(format!("unknown_key: {k}"));
    }
    Ok(m)
}

pub(crate) fn string<'a>(m: &'a Obj, key: &str) -> Result<&'a str, String> {
    match m.get(key) {
        Some(CValue::Str(s)) => Ok(s),
        _ => Err(format!("not_a_string: {key}")),
    }
}

pub(crate) fn array<'a>(m: &'a Obj, key: &str) -> Result<&'a [CValue], String> {
    match m.get(key) {
        Some(CValue::Arr(a)) => Ok(a),
        _ => Err(format!("not_an_array: {key}")),
    }
}

pub(crate) fn digest(m: &Obj, key: &str) -> Result<Digest, String> {
    Digest::parse(string(m, key)?).map_err(|_| format!("bad_digest: {key}"))
}

/// A decimal `usize` without leading zeros.
pub(crate) fn decimal(s: &str) -> Option<usize> {
    let ok =
        !s.is_empty() && s.len() <= 19 && s.bytes().all(|c| c.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    if ok {
        s.parse().ok()
    } else {
        None
    }
}

pub(crate) fn s(x: &str) -> Json {
    Json::Str(x.to_string())
}

pub(crate) fn obj(members: Vec<(&str, Json)>) -> Json {
    Json::Obj(members.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// Canonical bytes of a value whose keys the caller fixed (no duplicates, shallow).
pub(crate) fn write(v: &Json) -> Result<Vec<u8>, String> {
    to_canonical(v).map_err(|e| format!("json: {}", e.code()))
}
