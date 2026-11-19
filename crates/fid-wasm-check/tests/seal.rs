//! KRN.MAN.WASM `clock = "trap"`: the post-link seal renames WASI clock imports to the
//! `fid_trap` module. Expected bytes come from the independent Python `seal` in
//! `fixtures/man/wasm/make_fixtures.py`; running a sealed clock call under wasmtime (it must be
//! `NOT_REPLAYABLE(trap)`) is the CI wasm32 step.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_wasm_check::allow::{self, Allowlist};
use fid_wasm_check::module::{self, WasmErrorKind};
use fid_wasm_check::seal::{seal, TRAP_MODULE};
use std::path::{Path, PathBuf};

fn fx(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/man/wasm").join(rel)
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(fx(rel)).unwrap()
}

#[test]
fn seal_matches_the_independent_python_seal() {
    let sealed = seal(&read("modules/clock-call.wasm")).unwrap();
    assert_eq!(sealed, read("modules/clock-call.sealed.wasm"));
    // Idempotent, and a module without clock imports is unchanged.
    assert_eq!(seal(&sealed).unwrap(), sealed);
    for f in ["ok-wasi.wasm", "ok-empty.wasm", "loop.wasm", "bad-random-get.wasm"] {
        let b = read(&format!("modules/{f}"));
        assert_eq!(seal(&b).unwrap(), b, "{f}");
    }
}

#[test]
fn function_indices_and_descriptors_are_kept() {
    let before = module::parse(&read("modules/clock-call.wasm")).unwrap();
    let after = module::parse(&read("modules/clock-call.sealed.wasm")).unwrap();
    assert_eq!(before.imports.len(), after.imports.len());
    assert_eq!(before.type_count, after.type_count);
    for (b, a) in before.imports.iter().zip(&after.imports) {
        assert_eq!((&b.name, b.kind), (&a.name, a.kind));
        assert_eq!(a.module, if b.name == "clock_time_get" { TRAP_MODULE } else { b.module.as_str() });
    }
}

#[test]
fn sealed_clock_passes_as_trapped_and_unsealed_is_denied() {
    let allow = Allowlist::parse(
        &std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../fid-kernel-wasm/wasm-imports.allow")).unwrap(),
    )
    .unwrap();
    let unsealed = allow::check(&module::parse(&read("modules/clock-call.wasm")).unwrap(), &allow);
    assert!(!unsealed.passed());
    assert!(unsealed.lines().contains(&"DENIED wasi_snapshot_preview1 clock_time_get (func)".to_string()));
    let sealed = allow::check(&module::parse(&read("modules/clock-call.sealed.wasm")).unwrap(), &allow);
    assert!(sealed.passed());
    assert_eq!(sealed.lines()[0], "trapped fid_trap clock_time_get");
}

#[test]
fn seal_refuses_what_the_reader_refuses() {
    assert_eq!(seal(&read("modules/err-magic.wasm")).unwrap_err().kind, WasmErrorKind::BadMagic);
    assert_eq!(seal(&read("modules/err-import-short.wasm")).unwrap_err().kind, WasmErrorKind::Truncated);
}
