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

fn bad(reason: &'static str) -> MeshOpError {
    MeshOpError::BadParams { reason }
}

impl ParamsWriter {
    /// An empty writer.
    pub fn new() -> ParamsWriter {
        ParamsWriter::default()
    }

    fn put(&mut self, key: &str, value: String) -> Result<(), MeshOpError> {
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
        // "key":value plus a comma.
        let add = kb.len() + value.len() + 4;
        self.bytes = self.bytes.saturating_add(add);
        if self.bytes > MAX_PARAMS_BYTES {
            return Err(bad("too large"));
        }
        self.entries.try_reserve(1).map_err(|_| MeshOpError::AllocationFailed)?;
        self.entries.push((key.to_string(), value));
        Ok(())
    }

    /// An unsigned integer.
    pub fn u64(&mut self, key: &str, v: u64) -> Result<(), MeshOpError> {
        self.put(key, format!("\"{v}\""))
    }

    /// A signed integer.
    pub fn i64(&mut self, key: &str, v: i64) -> Result<(), MeshOpError> {
        self.put(key, format!("\"{v}\""))
    }

    /// A finite `f64` (metres, radians, ...). NaN and infinities are refused.
    pub fn f64(&mut self, key: &str, v: f64) -> Result<(), MeshOpError> {
        let s = es_number(v).ok_or_else(|| bad("non-finite number"))?;
        self.put(key, format!("\"{s}\""))
    }

    /// A boolean.
    pub fn bool(&mut self, key: &str, v: bool) -> Result<(), MeshOpError> {
        self.put(key, if v { "true" } else { "false" }.to_string())
    }

    /// A string of printable ASCII (0x20 to 0x7e) without `"` and `\`.
    pub fn str(&mut self, key: &str, v: &str) -> Result<(), MeshOpError> {
        if !v.bytes().all(|c| (0x20..=0x7e).contains(&c) && c != b'"' && c != b'\\') {
            return Err(bad("string characters"));
        }
        self.put(key, format!("\"{v}\""))
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
    }
}
