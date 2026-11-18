//! Journal entry hash (REQ-CORE.JRN.ENTRY-002 as read by the verifier) and the
//! journal's numeric conventions (REQ-TRUST.CHAIN.CANON-006).
//!
//! CORE.JRN.ENTRY has not yet published `entry.schema.json`, so the convention
//! check uses the field list from that node's design (`seq`, `time.mono_ns` and
//! the command `seed` are 64-bit integers) and one general rule: any other JSON
//! number in a header must be an integer of magnitude at most 2^53 - 1. Anything
//! else can only be a 64-bit integer (which the journal writes as a decimal
//! string) or a bit-significant double (which it writes as `"0x"` + 16 hex digits
//! of the IEEE 754 bits), so a raw number there is `canon.convention`.

use crate::parse::parse;
use crate::write::{digest_canonical, serialize};
use crate::{CanonError, InvalidKind, Value, MAX_DEPTH};
use fid_types::Digest;

/// Header fields (JSON Pointers) that hold unsigned 64-bit integers as decimal strings.
pub const U64_FIELDS: [&str; 3] = ["/seq", "/time/mono_ns", "/command/seed"];

/// Largest integer a raw header number may hold: 2^53 - 1.
pub const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Which numeric convention a header field breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConventionRule {
    /// A [`U64_FIELDS`] field is not a string of decimal digits (no sign, no
    /// leading zero) fitting in 64 bits.
    DecimalString,
    /// A raw number that is fractional or larger in magnitude than [`MAX_SAFE_INTEGER`].
    RawNumber,
    /// The header is not a JSON object.
    HeaderNotObject,
}

impl ConventionRule {
    /// Stable detail name.
    pub fn name(self) -> &'static str {
        match self {
            ConventionRule::DecimalString => "u64_decimal_string",
            ConventionRule::RawNumber => "raw_number",
            ConventionRule::HeaderNotObject => "header_not_object",
        }
    }
}

/// Entry hash of a header given as JSON text: parse (`canon.invalid`), check the
/// numeric conventions (`canon.convention`), drop the top-level `hash` member,
/// canonicalise and take SHA-256.
pub fn entry_hash(header: &[u8]) -> Result<Digest, CanonError> {
    entry_hash_value(&parse(header)?)
}

/// [`entry_hash`] for a parsed header.
pub fn entry_hash_value(header: &Value) -> Result<Digest, CanonError> {
    let Value::Object(members) = header else {
        return Err(CanonError::Convention { path: String::new(), rule: ConventionRule::HeaderNotObject });
    };
    let mut path = String::new();
    check(header, 0, &mut path)?;
    let without_hash: Vec<(String, Value)> = members.iter().filter(|(k, _)| k != "hash").cloned().collect();
    Ok(digest_canonical(&serialize(&Value::Object(without_hash))?))
}

fn convention(path: &str, rule: ConventionRule) -> CanonError {
    CanonError::Convention { path: path.to_string(), rule }
}

fn is_u64_decimal(s: &str) -> bool {
    let plain = !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    plain && s.parse::<u64>().is_ok()
}

/// Walks the header in document order; reports the first violation. The top-level
/// `hash` member is not hashed and is not checked.
fn check(v: &Value, depth: usize, path: &mut String) -> Result<(), CanonError> {
    let u64_field = U64_FIELDS.contains(&path.as_str());
    match v {
        Value::String(s) if u64_field => {
            if !is_u64_decimal(s) {
                return Err(convention(path, ConventionRule::DecimalString));
            }
        }
        _ if u64_field => return Err(convention(path, ConventionRule::DecimalString)),
        Value::Number(x) => {
            if x.fract() != 0.0 || x.abs() > MAX_SAFE_INTEGER {
                return Err(convention(path, ConventionRule::RawNumber));
            }
        }
        Value::Array(items) => {
            if depth + 1 > MAX_DEPTH {
                return Err(CanonError::invalid(InvalidKind::TooDeep, 0));
            }
            for (i, item) in items.iter().enumerate() {
                let len = path.len();
                path.push('/');
                path.push_str(&i.to_string());
                check(item, depth + 1, path)?;
                path.truncate(len);
            }
        }
        Value::Object(members) => {
            if depth + 1 > MAX_DEPTH {
                return Err(CanonError::invalid(InvalidKind::TooDeep, 0));
            }
            for (name, item) in members {
                if depth == 0 && name == "hash" {
                    continue;
                }
                let len = path.len();
                path.push('/');
                // RFC 6901 escaping.
                path.push_str(&name.replace('~', "~0").replace('/', "~1"));
                check(item, depth + 1, path)?;
                path.truncate(len);
            }
        }
        Value::Null | Value::Bool(_) | Value::String(_) => {}
    }
    Ok(())
}
