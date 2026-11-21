//! Operation parameters as kernel canonical JSON (the `params` object of the record).

use super::MeshOpError;
use crate::number::es_number;

/// The most parameter entries one operation may record.
pub const MAX_PARAM_ENTRIES: usize = 256;
/// The most bytes of the canonical parameter object.
pub const MAX_PARAMS_BYTES: usize = 64 * 1024;
/// The longest parameter key.
pub const MAX_KEY_BYTES: usize = 64;

/// The parameters of a [`MeshOp`](super::MeshOp). Every value that changes what the
/// operation computes must be written, so the record is a faithful account of the run.
pub trait OpParams {
    /// Writes every parameter. Keys are `[a-z0-9_]`, 1 to [`MAX_KEY_BYTES`] bytes, unique,
    /// and never `seed` (the seed has its own record field).
    fn write(&self, w: &mut ParamsWriter) -> Result<(), MeshOpError>;

    /// The seed of a D2 operation; `None` for D0. Recorded in the result.
    fn seed(&self) -> Option<u64> {
        None
    }
}

/// No parameters.
impl OpParams for () {
    fn write(&self, _: &mut ParamsWriter) -> Result<(), MeshOpError> {
        Ok(())
    }
}

/// Collects parameters and writes them as one canonical JSON object: keys in ascending
/// byte order, numbers written as strings (integers in decimal, `f64` in the ECMAScript
/// number form), booleans as `true`/`false`, strings of printable ASCII without `"` or `\`
/// (so no escaping is ever needed and the text is canonical).
#[derive(Debug, Default)]
pub struct ParamsWriter {
    entries: Vec<(String, String)>,
    bytes: usize,
}

/// The number of decimal digits of `v`.
fn decimal_len(mut v: u64) -> usize {
    let mut n = 1;
    while v >= 10 {
        v /= 10;
        n += 1;
    }
    n
}

fn bad(reason: &'static str) -> MeshOpError {
    MeshOpError::BadParams { reason }
}

impl ParamsWriter {
    /// An empty writer.
    pub fn new() -> ParamsWriter {
        ParamsWriter::default()
    }

    /// Checks the key, the entry count and the byte budget for a value of `value_len`
    /// bytes, before any value text is formatted or allocated.
    fn admit(&mut self, key: &str, value_len: usize) -> Result<(), MeshOpError> {
        let kb = key.as_bytes();
        if kb.is_empty() || kb.len() > MAX_KEY_BYTES {
            return Err(bad("key length"));
        }
        if !kb.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_') {
            return Err(bad("key characters"));
        }
        if key == "seed" {
            return Err(bad("reserved key seed"));
        }
        if self.entries.len() >= MAX_PARAM_ENTRIES {
            return Err(bad("too many entries"));
        }
        if value_len > MAX_PARAMS_BYTES {
            return Err(bad("too large"));
        }
        // "key":value plus a comma.
        let total = self.bytes.saturating_add(kb.len() + value_len + 4);
        if total > MAX_PARAMS_BYTES {
            return Err(bad("too large"));
        }
        self.entries.try_reserve(1).map_err(|_| MeshOpError::AllocationFailed)?;
        self.bytes = total;
        Ok(())
    }

    fn push(&mut self, key: &str, value: String) {
        self.entries.push((key.to_string(), value));
    }

    /// An unsigned integer.
    pub fn u64(&mut self, key: &str, v: u64) -> Result<(), MeshOpError> {
        self.admit(key, decimal_len(v) + 2)?;
        self.push(key, format!("\"{v}\""));
        Ok(())
    }

    /// A signed integer.
    pub fn i64(&mut self, key: &str, v: i64) -> Result<(), MeshOpError> {
        self.admit(key, decimal_len(v.unsigned_abs()) + usize::from(v < 0) + 2)?;
        self.push(key, format!("\"{v}\""));
        Ok(())
    }

    /// A finite `f64` (metres, radians, ...). NaN and infinities are refused. The
    /// ECMAScript form is at most 25 bytes.
    pub fn f64(&mut self, key: &str, v: f64) -> Result<(), MeshOpError> {
        let s = es_number(v).ok_or_else(|| bad("non-finite number"))?;
        self.admit(key, s.len() + 2)?;
        self.push(key, format!("\"{s}\""));
        Ok(())
    }

    /// A boolean.
    pub fn bool(&mut self, key: &str, v: bool) -> Result<(), MeshOpError> {
        let t = if v { "true" } else { "false" };
        self.admit(key, t.len())?;
        self.push(key, t.to_string());
        Ok(())
    }

    /// A string of printable ASCII (0x20 to 0x7e) without `"` and `\`.
    pub fn str(&mut self, key: &str, v: &str) -> Result<(), MeshOpError> {
        if v.len() > MAX_PARAMS_BYTES {
            return Err(bad("too large"));
        }
        if !v.bytes().all(|c| (0x20..=0x7e).contains(&c) && c != b'"' && c != b'\\') {
            return Err(bad("string characters"));
        }
        self.admit(key, v.len() + 2)?;
        self.push(key, format!("\"{v}\""));
        Ok(())
    }

    /// The canonical object. Duplicate keys are refused.
    pub fn finish(mut self) -> Result<Vec<u8>, MeshOpError> {
        self.entries.sort_unstable_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        if self.entries.windows(2).any(|w| w[0].0 == w[1].0) {
            return Err(bad("duplicate key"));
        }
        let mut out = Vec::new();
        out.try_reserve_exact(self.bytes.saturating_add(2)).map_err(|_| MeshOpError::AllocationFailed)?;
        out.push(b'{');
        for (i, (k, v)) in self.entries.iter().enumerate() {
            if i > 0 {
                out.push(b',');
            }
            out.push(b'"');
            out.extend_from_slice(k.as_bytes());
            out.extend_from_slice(b"\":");
            out.extend_from_slice(v.as_bytes());
        }
        out.push(b'}');
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn canonical_object() {
        let mut w = ParamsWriter::new();
        w.f64("max_move", 0.0005).unwrap();
        w.u64("target", 100).unwrap();
        w.bool("keep_boundary", true).unwrap();
        w.i64("delta", -3).unwrap();
        w.str("mode", "fill all").unwrap();
        let s = String::from_utf8(w.finish().unwrap()).unwrap();
        assert_eq!(s, r#"{"delta":"-3","keep_boundary":true,"max_move":"0.0005","mode":"fill all","target":"100"}"#);
        assert_eq!(ParamsWriter::new().finish().unwrap(), b"{}".to_vec());
        let mut w = ParamsWriter::new();
        w.u64("a", u64::MAX).unwrap();
        w.i64("b", i64::MIN).unwrap();
        let s = String::from_utf8(w.finish().unwrap()).unwrap();
        assert_eq!(s, r#"{"a":"18446744073709551615","b":"-9223372036854775808"}"#);
    }

    #[test]
    fn decimal_lengths() {
        for (v, n) in [(0, 1), (9, 1), (10, 2), (99, 2), (100, 3), (u64::MAX, 20)] {
            assert_eq!(decimal_len(v), n, "{v}");
            assert_eq!(v.to_string().len(), n);
        }
    }

    #[test]
    fn refusals() {
        let code = |r: Result<(), MeshOpError>| r.unwrap_err().code();
        let mut w = ParamsWriter::new();
        assert_eq!(code(w.u64("seed", 1)), "scan.mesh.bad_params");
        assert_eq!(code(w.u64("Big", 1)), "scan.mesh.bad_params");
        assert_eq!(code(w.u64("", 1)), "scan.mesh.bad_params");
        assert_eq!(code(w.u64(&"k".repeat(65), 1)), "scan.mesh.bad_params");
        assert_eq!(code(w.f64("x", f64::NAN)), "scan.mesh.bad_params");
        assert_eq!(code(w.f64("x", f64::INFINITY)), "scan.mesh.bad_params");
        assert_eq!(code(w.str("s", "a\"b")), "scan.mesh.bad_params");
        assert_eq!(code(w.str("s", "a\\b")), "scan.mesh.bad_params");
        assert_eq!(code(w.str("s", "a\nb")), "scan.mesh.bad_params");
        assert_eq!(code(w.str("s", "\u{e9}")), "scan.mesh.bad_params");
        let mut w = ParamsWriter::new();
        w.u64("a", 1).unwrap();
        w.u64("a", 2).unwrap();
        assert_eq!(w.finish().unwrap_err().code(), "scan.mesh.bad_params");
        let mut w = ParamsWriter::new();
        for i in 0..MAX_PARAM_ENTRIES {
            w.u64(&format!("k{i}"), 0).unwrap();
        }
        assert_eq!(code(w.u64("more", 0)), "scan.mesh.bad_params");
        let mut w = ParamsWriter::new();
        let long = "x".repeat(40_000);
        w.str("a", &long).unwrap();
        assert_eq!(code(w.str("b", &long)), "scan.mesh.bad_params");
        // A value over the whole budget is refused before it is formatted.
        let huge = "y".repeat(MAX_PARAMS_BYTES + 1);
        assert_eq!(code(ParamsWriter::new().str("c", &huge)), "scan.mesh.bad_params");
    }
}
