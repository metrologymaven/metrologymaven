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
    let mut out = Vec::new();
    write_value(value, 0, &mut out)?;
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
        Value::Object(members) => {
            if depth + 1 > MAX_DEPTH {
                return Err(CanonError::invalid(InvalidKind::TooDeep, 0));
            }
            let mut sorted: Vec<&(String, Value)> = members.iter().collect();
            // RFC 8785: order by UTF-16 code units of the member names.
            sorted.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
            if sorted.windows(2).any(|w| w[0].0 == w[1].0) {
                return Err(CanonError::invalid(InvalidKind::DuplicateKey, 0));
            }
            out.push(b'{');
            for (i, (name, item)) in sorted.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_string(name, out);
                out.push(b':');
                write_value(item, depth + 1, out)?;
            }
            out.push(b'}');
        }
    }
    Ok(())
}

const HEX: &[u8; 16] = b"0123456789abcdef";

/// Minimal escaping: quote, backslash and C0 controls only; everything else literal UTF-8.
fn write_string(s: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    let mut tmp = [0u8; 4];
    for ch in s.chars() {
        match ch {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\u{08}' => out.extend_from_slice(b"\\b"),
            '\t' => out.extend_from_slice(b"\\t"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\u{0c}' => out.extend_from_slice(b"\\f"),
            '\r' => out.extend_from_slice(b"\\r"),
            c if (c as u32) < 0x20 => {
                let n = c as usize;
                out.extend_from_slice(b"\\u00");
                out.push(HEX[n >> 4]);
                out.push(HEX[n & 0xf]);
            }
            c => out.extend_from_slice(c.encode_utf8(&mut tmp).as_bytes()),
        }
    }
    out.push(b'"');
}
