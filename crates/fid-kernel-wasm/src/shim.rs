//! Read, call, write (REQ-KRN.MAN.WASM-001, -002). [`respond`] turns the stdin bytes into the
//! stdout bytes and the exit code; `main` only moves bytes.
//!
//! | case | stdout | exit |
//! |---|---|---|
//! | `evaluate` returns a result | `KernelResult::canonical_json()` | 0 |
//! | `evaluate` returns an error | `{"error":"<KernelError code>"}` | 1 |
//! | the table cannot be built | `{"error":"table_unavailable"}` | 1 |
//! | malformed call or failed stdin read | `{"error":"malformed_call"}` | 2 |
//! | call larger than `MAX_CALL_BYTES` | `{"error":"oversized_call"}` | 2 |
//!
//! `main` exits 3 if stdout cannot be written. Nothing is written to stderr.

use fid_kci::{evaluate, AlgorithmTable, Digest};

use crate::call::parse_call;

/// Exit code: the kernel returned a result.
pub const EXIT_OK: u8 = 0;
/// Exit code: the kernel returned a typed error (written to stdout).
pub const EXIT_KERNEL_ERROR: u8 = 1;
/// Exit code: the input was malformed or oversized.
pub const EXIT_BAD_INPUT: u8 = 2;
/// Exit code: stdout could not be written.
pub const EXIT_IO: u8 = 3;

fn error(code: &str) -> Vec<u8> {
    let mut out = b"{\"error\":\"".to_vec();
    out.extend_from_slice(code.as_bytes());
    out.extend_from_slice(b"\"}");
    out
}

/// The algorithms of this build: the stand-in test algorithms (only with the
/// `test-algorithms` feature).
#[cfg(feature = "test-algorithms")]
fn algorithm_table() -> Option<AlgorithmTable> {
    crate::stand_in::table()
}

/// The algorithms of this build: none until the `fid-kernel` assembly crate exists.
#[cfg(not(feature = "test-algorithms"))]
fn algorithm_table() -> Option<AlgorithmTable> {
    AlgorithmTable::new(Vec::new(), fid_kci::CardRegistry::default()).ok()
}

/// Attaches the generated manifest core digest `embedded` (if any) to `table`. A constant that
/// is not a well-formed digest gives no table (`table_unavailable`), never a wrong answer.
pub fn with_embedded_digest(table: AlgorithmTable, embedded: Option<&str>) -> Option<AlgorithmTable> {
    match embedded {
        None => Some(table),
        Some(text) => Digest::parse(text).ok().map(|d| table.with_manifest_core_digest(d)),
    }
}

/// The table this build evaluates with, carrying the compiled-in manifest core digest
/// ([`crate::manifest_core_digest::MANIFEST_CORE_DIGEST`]) for the self-report query.
pub fn reference_table() -> Option<AlgorithmTable> {
    with_embedded_digest(algorithm_table()?, crate::manifest_core_digest::MANIFEST_CORE_DIGEST)
}

/// Stdout bytes and exit code for one call read from stdin.
pub fn respond(table: Option<&AlgorithmTable>, input: &[u8]) -> (u8, Vec<u8>) {
    let call = match parse_call(input) {
        Ok(c) => c,
        Err(e) => return (EXIT_BAD_INPUT, error(e.code())),
    };
    let Some(table) = table else {
        return (EXIT_KERNEL_ERROR, error("table_unavailable"));
    };
    match evaluate(table, &call) {
        Ok(result) => (EXIT_OK, result.canonical_json()),
        Err(e) => (EXIT_KERNEL_ERROR, error(e.code())),
    }
}

/// Stdout bytes and exit code when stdin could not be read.
pub fn read_failed() -> (u8, Vec<u8>) {
    (EXIT_BAD_INPUT, error("malformed_call"))
}
