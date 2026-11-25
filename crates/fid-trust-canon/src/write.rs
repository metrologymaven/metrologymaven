//! RFC 8785 serialisation of a [`Value`].

use crate::number::format_number;
use crate::parse::parse;
use crate::{CanonError, InvalidKind, Value, MAX_DEPTH};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};

/// Serialises `value` to canonical bytes. A constructed value deeper than
/// [`MAX_DEPTH`], with a non-finite number or with duplicate member names is
/// `canon.invalid`; no bytes are produced on error.
pub fn serialize(value: &Value) -> Result<Vec<u8>, CanonError> {
    let mut out = Vec::with_capacity(256);
    write_value(value, 0, &mut out)?;
    Ok(out)
}

/// Serialises a top-level object's members while omitting `omit_key` (for [`crate::entry_hash_value`]),
/// avoiding cloning the object AST.
pub(crate) fn serialize_object_without(members: &[(String, Value)], omit_key: &str) -> Result<Vec<u8>, CanonError> {
    let mut out = Vec::with_capacity(1024);
    write_object(members, Some(omit_key), 1, &mut out)?;
    Ok(out)
}

/// Parses JSON text (default limits) and returns its canonical bytes.
pub fn canonicalize(input: &[u8]) -> Result<Vec<u8>, CanonError> {
    serialize(&parse(input)?)
}

/// SHA-256 of already-canonical bytes, in `sha256:` wire form.
pub fn digest_canonical(canonical: &[u8]) -> Digest {
    let h = Sha256::digest(canonical);
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&h);
    Digest::from_bytes(bytes)
}

pub(crate) fn cmp_utf16(a: &str, b: &str) -> std::cmp::Ordering {
    if a.is_ascii() && b.is_ascii() {
        a.as_bytes().cmp(b.as_bytes())
    } else {
        a.encode_utf16().cmp(b.encode_utf16())
    }
}

fn write_object(
    members: &[(String, Value)],
    omit_key: Option<&str>,
    depth: usize,
    out: &mut Vec<u8>,
) -> Result<(), CanonError> {
    if depth > MAX_DEPTH {
        return Err(CanonError::invalid(InvalidKind::TooDeep, 0));
    }
    // Fast path when input members are already in strictly ascending UTF-16 key order
    // (true for canonical journal segments): zero extra allocation or sorting.
    let mut prev_name: Option<&str> = None;
    let mut already_sorted = true;
    for (name, _) in members {
        if omit_key == Some(name.as_str()) {
            continue;
        }
        if let Some(prev) = prev_name {
            if cmp_utf16(prev, name) != std::cmp::Ordering::Less {
                already_sorted = false;
                break;
            }
        }
        prev_name = Some(name.as_str());
    }

    out.push(b'{');
    if already_sorted {
        let mut first = true;
        for (name, item) in members {
            if omit_key == Some(name.as_str()) {
                continue;
            }
            if !first {
                out.push(b',');
            }
            first = false;
            write_string(name, out);
            out.push(b':');
            write_value(item, depth, out)?;
        }
    } else {
        let mut sorted: Vec<&(String, Value)> = members.iter().filter(|(k, _)| omit_key != Some(k.as_str())).collect();
        // RFC 8785: order by UTF-16 code units of the member names.
        sorted.sort_by(|a, b| cmp_utf16(&a.0, &b.0));
        if sorted.windows(2).any(|w| w[0].0 == w[1].0) {
            return Err(CanonError::invalid(InvalidKind::DuplicateKey, 0));
        }
        for (i, (name, item)) in sorted.into_iter().enumerate() {
            if i > 0 {
                out.push(b',');
            }
            write_string(name, out);
            out.push(b':');
            write_value(item, depth, out)?;
        }
    }
    out.push(b'}');
    Ok(())
}

fn write_value(v: &Value, depth: usize, out: &mut Vec<u8>) -> Result<(), CanonError> {
    match v {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Number(x) => out.extend_from_slice(format_number(*x)?.as_bytes()),
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            if depth + 1 > MAX_DEPTH {
                return Err(CanonError::invalid(InvalidKind::TooDeep, 0));
            }
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_value(item, depth + 1, out)?;
            }
            out.push(b']');
        }
        Value::Object(members) => write_object(members, None, depth + 1, out)?,
    }
    Ok(())
}

const HEX: &[u8; 16] = b"0123456789abcdef";

/// Minimal escaping: quote, backslash and C0 controls only; everything else literal UTF-8.
pub(crate) fn write_string(s: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    let bytes = s.as_bytes();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'"' || b == b'\\' || b < 0x20 {
            if start < i {
                out.extend_from_slice(&bytes[start..i]);
            }
            match b {
                b'"' => out.extend_from_slice(b"\\\""),
                b'\\' => out.extend_from_slice(b"\\\\"),
                0x08 => out.extend_from_slice(b"\\b"),
                b'\t' => out.extend_from_slice(b"\\t"),
                b'\n' => out.extend_from_slice(b"\\n"),
                0x0c => out.extend_from_slice(b"\\f"),
                b'\r' => out.extend_from_slice(b"\\r"),
                c => {
                    let n = usize::from(c);
                    out.extend_from_slice(b"\\u00");
                    out.push(HEX[n >> 4]);
                    out.push(HEX[n & 0xf]);
                }
            }
            start = i + 1;
        }
    }
    if start < bytes.len() {
        out.extend_from_slice(&bytes[start..]);
    }
    out.push(b'"');
}
