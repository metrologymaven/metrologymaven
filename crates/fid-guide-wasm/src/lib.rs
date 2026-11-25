//! Safe-Rust Field Guide kernel runner (spec node `OUTR.DOCS.GUIDE.WASM`).
//!
//! Compiles both natively and to `wasm32-wasip1` with `#![forbid(unsafe_code)]`, zero panics
//! and zero third-party dependencies beyond the workspace's pure L0/model crates. Reads one
//! bounded JSON request on stdin and emits one RFC 8785 canonical JSON response on stdout with
//! its SHA-256 `result_digest`.

pub mod json_helpers;
pub mod ops;

use std::collections::BTreeMap;

use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue};

/// Largest accepted stdin payload (256 KiB).
pub const MAX_REQUEST_BYTES: usize = 256 * 1024;
/// Maximum JSON nesting depth accepted by the request parser.
pub const MAX_REQUEST_DEPTH: usize = 32;

/// Exit code: operation succeeded.
pub const EXIT_OK: u8 = 0;
/// Exit code: operation returned a typed kernel/validation error.
pub const EXIT_OP_ERROR: u8 = 1;
/// Exit code: request was malformed or exceeded `MAX_REQUEST_BYTES`.
pub const EXIT_BAD_INPUT: u8 = 2;
/// Exit code: stdout write failed.
pub const EXIT_IO: u8 = 3;

/// Typed error returned by a Field Guide lab operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabError {
    pub code: &'static str,
    pub message: String,
}

impl LabError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

/// Dispatches one bounded JSON request into `(exit_code, canonical_json_bytes)`.
pub fn dispatch(input: &[u8]) -> (u8, Vec<u8>) {
    if input.len() > MAX_REQUEST_BYTES {
        return (EXIT_BAD_INPUT, error_bytes("oversized_request", "request exceeds 256 KiB limit"));
    }
    let root = match parse_with_limits(input, MAX_REQUEST_BYTES, MAX_REQUEST_DEPTH) {
        Ok(v) => v,
        Err(e) => return (EXIT_BAD_INPUT, error_bytes("malformed_json", &e.to_string())),
    };
    let CValue::Obj(map) = root else {
        return (EXIT_BAD_INPUT, error_bytes("malformed_request", "request root must be a JSON object"));
    };
    let Some(CValue::Str(op)) = map.get("op") else {
        return (EXIT_BAD_INPUT, error_bytes("missing_op", "request must include string field `op`"));
    };
    let params = match map.get("params") {
        Some(CValue::Obj(p)) => p,
        _ => return (EXIT_BAD_INPUT, error_bytes("missing_params", "request must include object field `params`")),
    };

    let outcome = match op.as_str() {
        "fit.circle2d" => ops::fit_circle2d(params),
        "fit.plane_datum" => ops::fit_plane_datum(params),
        "fit.cylinder_mcc" => ops::fit_cylinder_mcc(params),
        "gdt.zone_eval" => ops::gdt_zone_eval(params),
        "unc.propagate" => ops::unc_propagate(params),
        "dec.evaluate" => ops::dec_evaluate(params),
        "canon.digest" => ops::canon_digest(params),
        _ => Err(LabError::new("unknown_op", format!("unsupported lab operation `{op}`"))),
    };

    match outcome {
        Ok(result) => ok_bytes(op, result),
        Err(err) => (EXIT_OP_ERROR, error_bytes(err.code, &err.message)),
    }
}

/// Encodes a read failure on stdin.
pub fn read_failed() -> (u8, Vec<u8>) {
    (EXIT_BAD_INPUT, error_bytes("stdin_read_failed", "could not read request from stdin"))
}

fn ok_bytes(op: &str, result: CValue) -> (u8, Vec<u8>) {
    let result_bytes = match to_canonical_bytes(&result) {
        Ok(b) => b,
        Err(e) => return (EXIT_OP_ERROR, error_bytes("canonical_encode_failed", &e.to_string())),
    };
    let digest = digest_bytes(&result_bytes).to_string();
    let mut env = BTreeMap::new();
    env.insert("ok".to_string(), CValue::Bool(true));
    env.insert("op".to_string(), CValue::Str(op.to_string()));
    env.insert("result".to_string(), result);
    env.insert("result_digest".to_string(), CValue::Str(digest));
    match to_canonical_bytes(&CValue::Obj(env)) {
        Ok(bytes) => (EXIT_OK, bytes),
        Err(e) => (EXIT_OP_ERROR, error_bytes("canonical_encode_failed", &e.to_string())),
    }
}

fn error_bytes(code: &str, message: &str) -> Vec<u8> {
    let mut env = BTreeMap::new();
    env.insert("code".to_string(), CValue::Str(code.to_string()));
    env.insert("message".to_string(), CValue::Str(message.to_string()));
    env.insert("ok".to_string(), CValue::Bool(false));
    to_canonical_bytes(&CValue::Obj(env)).unwrap_or_else(|_| {
        b"{\"code\":\"internal_encode\",\"message\":\"canonical encode error\",\"ok\":false}".to_vec()
    })
}
