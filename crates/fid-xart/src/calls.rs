//! The call set: one canonical `KernelCall` per line (`fixtures/man/xart-calls-v1/calls.txt`
//! until `UNC.RDL` provides the frozen reference set).
//!
//! UTF-8, LF line ends (a CR anywhere is refused), `#` comment lines and blank lines, and call
//! lines `<name><TAB><call JSON>`. The call JSON must be canonical and name its algorithm in
//! the exact `AlgorithmId` form; the artifact checks the rest. Names are unique. The call-set
//! digest is the SHA-256 of the file bytes, so any edit, comments included, changes it.

use std::collections::BTreeSet;

use fid_kci::{kci::sha256_digest, Digest};

use crate::table::is_algorithm;
use crate::{is_name, json, XartError};

/// Largest call-set file read.
pub const MAX_CALL_SET_BYTES: usize = 256 * 1024 * 1024;
/// Largest single call (the `fid-kernel-wasm` limit, `MAX_CALL_BYTES`).
pub const MAX_CALL_BYTES: usize = 64 * 1024 * 1024;
/// Most calls in one set.
pub const MAX_CALLS: usize = 1_000_000;

/// One call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// Unique name.
    pub name: String,
    /// Algorithm ID, exactly as written in the call.
    pub algorithm: String,
    /// The canonical call bytes sent to the artifact on stdin.
    pub bytes: Vec<u8>,
}

/// A parsed call set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSet {
    /// SHA-256 of the file bytes.
    pub digest: Digest,
    /// The calls in file order.
    pub calls: Vec<Call>,
}

/// Parses a call-set file.
pub fn parse_calls(bytes: &[u8]) -> Result<CallSet, XartError> {
    let bad = |line: usize, reason: &'static str| XartError::Calls { line, reason };
    if bytes.len() > MAX_CALL_SET_BYTES {
        return Err(bad(0, "too_large"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| bad(0, "not_utf8"))?;
    if text.contains('\r') {
        return Err(bad(0, "carriage_return"));
    }
    if !text.is_empty() && !text.ends_with('\n') {
        return Err(bad(0, "no_final_newline"));
    }
    let mut names = BTreeSet::new();
    let mut calls = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, call) = line.split_once('\t').ok_or_else(|| bad(n, "no_tab"))?;
        if !is_name(name) {
            return Err(bad(n, "bad_name"));
        }
        if !names.insert(name) {
            return Err(bad(n, "duplicate_name"));
        }
        let v =
            json::parse_canonical(call.as_bytes(), MAX_CALL_BYTES).map_err(|_| bad(n, "call_not_canonical_json"))?;
        let m = json::object(&v, &["algorithm", "inputs", "params"], &["seed"]).map_err(|_| bad(n, "call_shape"))?;
        let algorithm = json::string(m, "algorithm").map_err(|_| bad(n, "call_shape"))?;
        if !is_algorithm(algorithm) {
            return Err(bad(n, "bad_algorithm"));
        }
        if calls.len() == MAX_CALLS {
            return Err(bad(n, "too_many_calls"));
        }
        calls.push(Call { name: name.to_string(), algorithm: algorithm.to_string(), bytes: call.as_bytes().to_vec() });
    }
    if calls.is_empty() {
        return Err(bad(0, "no_calls"));
    }
    Ok(CallSet { digest: sha256_digest(bytes), calls })
}
