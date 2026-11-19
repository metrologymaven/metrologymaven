//! KRN.MAN.WASM REQ-004/-005: the pinned runtime settings, their digest, the wasmtime
//! arguments and the outcome classification. The expected digest is computed by
//! `fixtures/man/wasm/make_fixtures.py` (hashlib); the error cases are hand-labelled from the
//! Design rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_wasm_check::runner::{classify, parse_cases, version_matches, Outcome};
use fid_wasm_check::runtime::{RuntimeConfig, RuntimeError};
use std::path::Path;

fn shipped() -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../fid-kernel-wasm/wasm-runtime.toml")).unwrap()
}

#[test]
fn shipped_settings_are_canonical_and_digest_matches_python() {
    let cfg = RuntimeConfig::parse_canonical(&shipped()).unwrap();
    assert_eq!(cfg.canonical_bytes(), shipped().as_slice());
    let expected = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/man/wasm/runtime-digest.txt"),
    )
    .unwrap();
    assert_eq!(cfg.config_digest().as_str(), expected.trim_end());
    assert_eq!(cfg.wasmtime_version, "49.0.6");
    assert_eq!((cfg.max_wasm_stack, cfg.max_memory_size), (1_048_576, 1_073_741_824));
}

#[test]
fn wasmtime_arguments_are_pinned() {
    let cfg = RuntimeConfig::parse_canonical(&shipped()).unwrap();
    assert_eq!(
        cfg.wasmtime_args("k.wasm").unwrap(),
        [
            "run",
            "-C",
            "cache=n",
            "-W",
            "nan-canonicalization=y",
            "-W",
            "relaxed-simd=n",
            "-W",
            "threads=n",
            "-W",
            "max-wasm-stack=1048576",
            "-W",
            "max-memory-size=1073741824",
            "k.wasm"
        ]
    );
    assert_eq!(cfg.wasmtime_args("-k.wasm"), None);
    assert_eq!(cfg.wasmtime_args(""), None);
}

fn with(from: &str, to: &str) -> Vec<u8> {
    let s = String::from_utf8(shipped()).unwrap();
    assert!(s.contains(from), "{from}");
    s.replacen(from, to, 1).into_bytes()
}

#[test]
fn settings_errors_are_typed_and_fail_closed() {
    let cases: Vec<(Vec<u8>, RuntimeError)> = vec![
        (with("relaxed_simd = false", "relaxed_simd = true"), RuntimeError::Policy("relaxed_simd")),
        (with("nan_canonicalization = true", "nan_canonicalization = false"), RuntimeError::Policy("nan_canonicalization")),
        (with("threads = false", "threads = true"), RuntimeError::Policy("threads")),
        (with("schema = \"1\"", "schema = \"2\""), RuntimeError::Policy("schema")),
        (with("schema = \"1\"", "schema = 1"), RuntimeError::BadValue("schema")),
        (with("threads = false", "threads = \"false\""), RuntimeError::BadValue("threads")),
        (with("max_wasm_stack = 1048576", "max_wasm_stack = 0"), RuntimeError::BadValue("max_wasm_stack")),
        (with("max_wasm_stack = 1048576", "max_wasm_stack = 16777217"), RuntimeError::BadValue("max_wasm_stack")),
        (
            with("max_memory_size = 1073741824", "max_memory_size = 4294967297"),
            RuntimeError::BadValue("max_memory_size"),
        ),
        (with("\"49.0.6\"", "\"49.0\""), RuntimeError::BadValue("wasmtime_version")),
        (with("\"49.0.6\"", "\"v49.0.6\""), RuntimeError::BadValue("wasmtime_version")),
        (with("\"49.0.6\"", "\"49.00.6\""), RuntimeError::BadValue("wasmtime_version")),
        (with("max_wasm_stack = 1048576", "max_wasm_stack = 01048576"), RuntimeError::Syntax { line: 2 }),
        (with("threads = false\n", "threads = false\r\n"), RuntimeError::Syntax { line: 6 }),
        (with("threads = false", "threads=false"), RuntimeError::Syntax { line: 6 }),
        (with("schema", "# pinned\nschema"), RuntimeError::Syntax { line: 5 }),
        (with("threads = false", "threads = false\nthreads = false"), RuntimeError::DuplicateKey("threads".into())),
        (with("threads = false", "threads = false\nsimd = false"), RuntimeError::UnknownKey("simd".into())),
        (with("threads = false\n", ""), RuntimeError::MissingKey("threads")),
        (with("threads = false\n", "threads = false\n\n"), RuntimeError::NonCanonical { offset: 132 }),
        (
            with("max_memory_size = 1073741824\nmax_wasm_stack = 1048576\n", "max_wasm_stack = 1048576\nmax_memory_size = 1073741824\n"),
            RuntimeError::NonCanonical { offset: 4 },
        ),
        (shipped()[..shipped().len() - 1].to_vec(), RuntimeError::NonCanonical { offset: shipped().len() - 1 }),
        (vec![b'\n'; 4097], RuntimeError::TooLarge),
        (vec![0xff], RuntimeError::Syntax { line: 1 }),
    ];
    for (bytes, err) in cases {
        let got = RuntimeConfig::parse_canonical(&bytes);
        assert_eq!(got, Err(err), "{}", String::from_utf8_lossy(&bytes));
    }
}

#[test]
fn ac3_outcome_classification() {
    let ok = br#"{"algorithm":"a.b.c@1.0.0"}"#;
    let kerr = br#"{"error":"digest_mismatch"}"#;
    assert_eq!(classify(Some(0), ok, b""), Outcome::Shim { exit: 0, stdout: ok.to_vec() });
    assert_eq!(classify(Some(1), kerr, b""), Outcome::Shim { exit: 1, stdout: kerr.to_vec() });
    assert_eq!(classify(Some(2), b"{\"error\":\"malformed_call\"}", b"").exit_code(), 2);
    // A wasmtime start-up or usage error is never taken for a shim answer.
    assert_eq!(classify(Some(1), b"", b"Error: unknown option"), Outcome::Trap);
    assert_eq!(classify(Some(2), b"", b"error: unexpected argument"), Outcome::Trap);
    assert_eq!(classify(Some(0), kerr, b""), Outcome::Trap);
    assert_eq!(classify(Some(0), ok, b"warning"), Outcome::Trap);
    // Stack or memory exhaustion.
    for stderr in [
        "Error: failed to run main module\n\nCaused by:\n    wasm trap: call stack exhausted\n",
        "wasm trap: out of bounds memory access",
        "memory allocation of 1610612736 bytes failed\nError: wasm trap: wasm `unreachable` instruction executed",
    ] {
        let o = classify(Some(134), b"", stderr.as_bytes());
        assert_eq!(o, Outcome::Resource, "{stderr}");
        assert_eq!((o.exit_code(), o.stdout()), (3, b"NOT_REPLAYABLE(resource)".as_slice()));
    }
    let t = classify(Some(134), b"", b"wasm trap: integer divide by zero");
    assert_eq!((t.exit_code(), t.stdout()), (4, b"NOT_REPLAYABLE(trap)".as_slice()));
    assert_eq!(classify(None, b"", b""), Outcome::Trap);
}

#[test]
fn wasmtime_version_must_match_exactly() {
    assert!(version_matches("wasmtime 49.0.6 (abcdef 2025-12-01)\n", "49.0.6"));
    assert!(version_matches("wasmtime-cli 49.0.6\n", "49.0.6"));
    assert!(!version_matches("wasmtime 49.0.7 (abcdef 2025-12-01)\n", "49.0.6"));
    assert!(!version_matches("wasmtime 49.0.60\n", "49.0.6"));
    assert!(!version_matches("wasmer 49.0.6\n", "49.0.6"));
    assert!(!version_matches("", "49.0.6"));
}

#[test]
fn calls_file_parses() {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/man/wasm/calls-v1.txt"),
    )
    .unwrap();
    let cases = parse_cases(&text).unwrap();
    assert_eq!(cases.len(), 43);
    assert_eq!(cases.iter().filter(|c| c.wasm_only).count(), 2);
    assert!(cases.iter().filter(|c| c.wasm_only).all(|c| c.exit == 3 && c.stdout == b"NOT_REPLAYABLE(resource)"));
    assert_eq!(parse_cases("a\tany\t{}\t9\tx\n"), Err(1));
    assert_eq!(parse_cases("# c\na\tnative\t{}\t0\tx\n"), Err(2));
    assert_eq!(parse_cases("a\tany\t{}\t0\n"), Err(1));
}
