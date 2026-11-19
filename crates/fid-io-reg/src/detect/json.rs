//! The JSON shape of a format descriptor, used by the listing commands and by registry
//! documents (`fixtures/io/reg/detect/registry.json`). Parsing is strict: bounded input
//! (`fid-canon` with [`MAX_REGISTRY_JSON_BYTES`] and [`MAX_REGISTRY_JSON_DEPTH`]), duplicate
//! keys refused by the parser, unknown fields refused, integers only where integers belong.
//!
//! ```json
//! {"id": "mesh.ply", "versions": {"min": "1.0", "max": "1.0"},
//!  "signatures": [{"offset": 0, "priority": 10, "bytes": "706c790a"}],
//!  "extensions": ["ply"], "options_schema": {"type": "object"},
//!  "phase": "P0", "origin": "first_party", "direction": "read"}
//! ```
//!
//! A signature has `offset`, `priority` and exactly one of `bytes` (lowercase hex),
//! `contains` (lowercase hex) with `window`, or `predicate` (a [`Predicate`] name).

use super::{
    direction_from_wire, direction_wire, DetectError, FormatDescriptor, Matcher, Origin, Phase, Predicate, Registry,
    Signature, VersionRange,
};
use crate::report::FormatId;
use fid_canon::CValue;
use std::collections::BTreeMap;

/// Largest registry document accepted, in bytes.
pub const MAX_REGISTRY_JSON_BYTES: usize = 1024 * 1024;
/// Deepest nesting accepted in a registry document (options schemas included).
pub const MAX_REGISTRY_JSON_DEPTH: usize = 32;

type Obj = BTreeMap<String, CValue>;

fn bad(field: &'static str, reason: &'static str) -> DetectError {
    DetectError::Invalid { field, reason }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from(DIGITS[usize::from(b >> 4)]));
        s.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    s
}

fn unhex(s: &str, field: &'static str) -> Result<Vec<u8>, DetectError> {
    fn nibble(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        }
    }
    let b = s.as_bytes();
    if b.len() % 2 != 0 {
        return Err(bad(field, "hex string of odd length"));
    }
    b.chunks_exact(2)
        .map(|p| match p {
            [h, l] => match (nibble(*h), nibble(*l)) {
                (Some(h), Some(l)) => Ok((h << 4) | l),
                _ => Err(bad(field, "not lowercase hex")),
            },
            _ => Err(bad(field, "not lowercase hex")),
        })
        .collect()
}

fn num(n: u64) -> CValue {
    // Every integer written here is below 2^32, so the f64 is exact.
    CValue::Num(n as f64)
}

fn str_v(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

/// The JSON value of a signature.
pub(super) fn signature_value(s: &Signature) -> CValue {
    let mut o = Obj::new();
    o.insert("offset".into(), num(u64::from(s.offset)));
    o.insert("priority".into(), num(u64::from(s.priority)));
    match &s.matcher {
        Matcher::Bytes(b) => {
            o.insert("bytes".into(), CValue::Str(hex(b)));
        }
        Matcher::Contains { needle, window } => {
            o.insert("contains".into(), CValue::Str(hex(needle)));
            o.insert("window".into(), num(u64::from(*window)));
        }
        Matcher::Predicate(p) => {
            o.insert("predicate".into(), str_v(p.as_str()));
        }
    }
    CValue::Obj(o)
}

/// The JSON value of a descriptor (every REQ-001 field).
pub(super) fn descriptor_value(d: &FormatDescriptor) -> CValue {
    let mut versions = Obj::new();
    versions.insert("min".into(), str_v(d.versions.min()));
    versions.insert("max".into(), str_v(d.versions.max()));
    let mut o = Obj::new();
    o.insert("id".into(), str_v(d.id.as_str()));
    o.insert("versions".into(), CValue::Obj(versions));
    o.insert("signatures".into(), CValue::Arr(d.signatures.iter().map(signature_value).collect()));
    o.insert("extensions".into(), CValue::Arr(d.extensions.iter().map(|e| str_v(e)).collect()));
    o.insert("options_schema".into(), d.options_schema.clone());
    o.insert("phase".into(), CValue::Str(d.phase.to_string()));
    o.insert("origin".into(), str_v(d.origin.as_str()));
    o.insert("direction".into(), str_v(direction_wire(d.direction)));
    CValue::Obj(o)
}

fn take(o: &mut Obj, field: &'static str) -> Result<CValue, DetectError> {
    o.remove(field).ok_or_else(|| bad(field, "missing field"))
}

fn take_str(o: &mut Obj, field: &'static str) -> Result<String, DetectError> {
    match take(o, field)? {
        CValue::Str(s) => Ok(s),
        _ => Err(bad(field, "must be a string")),
    }
}

fn take_uint(o: &mut Obj, field: &'static str, max: u32) -> Result<u32, DetectError> {
    match take(o, field)? {
        CValue::Num(n) if n.fract() == 0.0 && n >= 0.0 && n <= f64::from(max) => Ok(n as u32),
        _ => Err(bad(field, "must be an integer in range")),
    }
}

fn no_more(o: &Obj, field: &'static str) -> Result<(), DetectError> {
    if o.is_empty() {
        Ok(())
    } else {
        Err(bad(field, "unknown field"))
    }
}

fn signature_from(v: CValue) -> Result<Signature, DetectError> {
    let CValue::Obj(mut o) = v else { return Err(bad("signatures", "signature must be an object")) };
    let offset = take_uint(&mut o, "offset", u32::MAX)?;
    let priority = take_uint(&mut o, "priority", u32::from(u8::MAX))? as u8;
    let bytes = o.remove("bytes");
    let contains = o.remove("contains");
    let predicate = o.remove("predicate");
    let matcher = match (bytes, contains, predicate) {
        (Some(CValue::Str(b)), None, None) => Matcher::Bytes(unhex(&b, "bytes")?),
        (None, Some(CValue::Str(c)), None) => {
            let window = take_uint(&mut o, "window", u32::MAX)?;
            Matcher::Contains { needle: unhex(&c, "contains")?, window }
        }
        (None, None, Some(CValue::Str(p))) => {
            Matcher::Predicate(Predicate::from_wire(&p).ok_or_else(|| bad("predicate", "unknown predicate"))?)
        }
        _ => return Err(bad("signatures", "need exactly one string of bytes, contains or predicate")),
    };
    no_more(&o, "signatures")?;
    Signature::new(offset, matcher, priority)
}

fn descriptor_from(v: CValue) -> Result<FormatDescriptor, DetectError> {
    let CValue::Obj(mut o) = v else { return Err(bad("formats", "descriptor must be an object")) };
    let id = FormatId::new(&take_str(&mut o, "id")?).map_err(|_| bad("id", "invalid format ID"))?;
    let versions = match take(&mut o, "versions")? {
        CValue::Obj(mut vo) => {
            let min = take_str(&mut vo, "min")?;
            let max = take_str(&mut vo, "max")?;
            no_more(&vo, "versions")?;
            VersionRange::new(&min, &max)?
        }
        _ => return Err(bad("versions", "must be an object")),
    };
    let signatures = match take(&mut o, "signatures")? {
        CValue::Arr(a) if a.len() <= super::MAX_SIGNATURES => {
            a.into_iter().map(signature_from).collect::<Result<Vec<_>, _>>()?
        }
        CValue::Arr(_) => return Err(bad("signatures", "too many signatures")),
        _ => return Err(bad("signatures", "must be an array")),
    };
    let extensions = match take(&mut o, "extensions")? {
        CValue::Arr(a) if a.len() <= super::MAX_EXTENSIONS => a
            .into_iter()
            .map(|e| match e {
                CValue::Str(s) => Ok(s),
                _ => Err(bad("extensions", "must be strings")),
            })
            .collect::<Result<Vec<_>, _>>()?,
        CValue::Arr(_) => return Err(bad("extensions", "too many extensions")),
        _ => return Err(bad("extensions", "must be an array")),
    };
    let options_schema = take(&mut o, "options_schema")?;
    let phase = Phase::parse(&take_str(&mut o, "phase")?).ok_or_else(|| bad("phase", "must be P0 to P9"))?;
    let origin = Origin::from_wire(&take_str(&mut o, "origin")?).ok_or_else(|| bad("origin", "unknown origin"))?;
    let direction =
        direction_from_wire(&take_str(&mut o, "direction")?).ok_or_else(|| bad("direction", "must be read or write"))?;
    no_more(&o, "formats")?;
    FormatDescriptor::new(id, versions, signatures, extensions, options_schema, phase, origin, direction)
}

impl FormatDescriptor {
    /// The descriptor as JSON, the shape the listing commands return.
    pub fn to_value(&self) -> CValue {
        descriptor_value(self)
    }

    /// Parses one descriptor from its JSON value.
    pub fn from_value(v: CValue) -> Result<FormatDescriptor, DetectError> {
        descriptor_from(v)
    }
}

impl Registry {
    /// Builds a registry from a registry document `{"formats": [descriptor, ...]}`,
    /// registering in document order (so a repeated ID and direction fails
    /// `format_duplicate`). Any other fault is `malformed` with a static reason.
    pub fn from_json(bytes: &[u8]) -> Result<Registry, DetectError> {
        let v = fid_canon::parse_with_limits(bytes, MAX_REGISTRY_JSON_BYTES, MAX_REGISTRY_JSON_DEPTH)
            .map_err(|_| bad("registry", "not acceptable JSON"))?;
        let CValue::Obj(mut o) = v else { return Err(bad("registry", "must be an object")) };
        let formats = match take(&mut o, "formats")? {
            CValue::Arr(a) if a.len() <= super::MAX_DESCRIPTORS => a,
            CValue::Arr(_) => return Err(bad("formats", "too many descriptors")),
            _ => return Err(bad("formats", "must be an array")),
        };
        no_more(&o, "registry")?;
        let mut reg = Registry::new();
        for f in formats {
            reg.register(descriptor_from(f)?)?;
        }
        Ok(reg)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(unhex(&hex(&all), "bytes").unwrap(), all);
        assert!(unhex("0", "bytes").is_err());
        assert!(unhex("0A", "bytes").is_err());
        assert!(unhex("zz", "bytes").is_err());
        assert_eq!(unhex("", "bytes").unwrap(), Vec::<u8>::new());
    }
}
