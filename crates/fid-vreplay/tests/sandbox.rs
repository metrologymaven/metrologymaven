//! Integration tests for spec node `TRUST.REPLAY.SANDBOX` (REQ-001..REQ-007, AC-2..AC-5 and
//! the AC-1 path with the protocol test kernel). Expected values come from
//! `fixtures/replay/sandbox/make_fixtures.py` (hand-labelled hostile set; envelope computed by
//! an exact Python model), never from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{digest_bytes, parse_with_limits, CValue};
use fid_types::Digest;
use fid_vreplay::kart::KernelManifest;
use fid_vreplay::sandbox::{
    CallInput, CallRequest, CallResult, ErrorCode, Limits, Sandbox, SandboxError, DEFAULT_MEMORY_BYTES,
    IMPORT_ALLOWLIST, TRAPPED_IMPORTS,
};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/replay/sandbox");

fn read(name: &str) -> Vec<u8> {
    std::fs::read(format!("{FIX}/{name}")).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn sha(bytes: &[u8]) -> Digest {
    Digest::from_bytes(digest_bytes(bytes).0)
}

fn expect() -> BTreeMap<String, CValue> {
    match parse_with_limits(&read("expect.json"), 1 << 20, 16).unwrap() {
        CValue::Obj(o) => o.into_iter().collect(),
        _ => panic!("expect.json is not an object"),
    }
}

fn obj(v: &CValue) -> BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o.clone().into_iter().collect(),
        _ => panic!("not an object: {v:?}"),
    }
}

fn s(o: &BTreeMap<String, CValue>, k: &str) -> String {
    match o.get(k) {
        Some(CValue::Str(x)) => x.clone(),
        other => panic!("{k}: {other:?}"),
    }
}

fn kernel_manifest() -> KernelManifest {
    KernelManifest::parse(&read("kernel-test.manifest.json")).unwrap()
}

fn kernel_request(e: &BTreeMap<String, CValue>) -> CallRequest {
    let k = obj(&e["kernel"]);
    let params = (0..s(&k, "params_hex").len() / 2)
        .map(|i| u8::from_str_radix(&s(&k, "params_hex")[2 * i..2 * i + 2], 16).unwrap())
        .collect();
    CallRequest {
        algorithm: s(&k, "algorithm"),
        params,
        seed: Some(s(&k, "seed").parse().unwrap()),
        inputs: vec![CallInput::new(read("circle-100.bin"))],
    }
}

fn load_kernel(limits: Limits) -> Result<Sandbox, SandboxError> {
    Sandbox::load(&read("kernel-test.wasm"), &kernel_manifest(), limits)
}

/// AC-1 (path): the test kernel's envelope for the pinned call equals the independently
/// computed one, and the call bytes equal the Python canonical encoding.
#[test]
fn ac1_kernel_call_returns_the_pinned_envelope() {
    let e = expect();
    let k = obj(&e["kernel"]);
    let req = kernel_request(&e);
    assert_eq!(sha(&req.canonical_bytes().unwrap()).as_str(), s(&k, "call_sha256"));
    let mut sb = load_kernel(Limits::default()).unwrap();
    let out = sb.call(&req).unwrap();
    let CallResult::Envelope(bytes) = &out else { panic!("not an envelope: {out:?}") };
    assert_eq!(String::from_utf8(bytes.clone()).unwrap(), s(&k, "envelope"));
    assert_eq!(sha(bytes).as_str(), s(&k, "envelope_sha256"));
}

/// The kernel's typed refusal passes through as `KernelError`.
#[test]
fn kernel_error_passes_through() {
    let e = expect();
    let ke = obj(&e["kernel_error"]);
    let mut req = kernel_request(&e);
    req.algorithm = s(&ke, "algorithm");
    req.seed = None;
    assert_eq!(sha(&req.canonical_bytes().unwrap()).as_str(), s(&ke, "call_sha256"));
    let mut sb = load_kernel(Limits::default()).unwrap();
    let out = sb.call(&req).unwrap();
    assert_eq!(
        out,
        CallResult::KernelError { exit_code: 1, bytes: s(&ke, "stdout").into_bytes() },
        "kernel error not passed through"
    );
}

/// AC-2: one changed byte anywhere gives `sandbox.artifact_mismatch` before parsing (a byte
/// that breaks the module header still reports the digest, not a parse error).
#[test]
fn ac2_changed_byte_is_artifact_mismatch() {
    let good = read("kernel-test.wasm");
    let manifest = kernel_manifest();
    for at in [0, 3, 7, good.len() / 2, good.len() - 1] {
        let mut bad = good.clone();
        bad[at] ^= 0x01;
        let err = Sandbox::load(&bad, &manifest, Limits::default()).unwrap_err();
        assert_eq!(err.code, ErrorCode::ArtifactMismatch, "byte {at}: {err}");
    }
    // A manifest without a wasm32-wasip1 entry is a mismatch too.
    let no_wasm = KernelManifest { wasm32_wasip1_digest: None, ..manifest };
    assert_eq!(Sandbox::load(&good, &no_wasm, Limits::default()).unwrap_err().code, ErrorCode::ArtifactMismatch);
}

fn limits_from(o: &BTreeMap<String, CValue>) -> Limits {
    let mut l = Limits::default();
    for (k, v) in o {
        let CValue::Str(v) = v else { panic!("limit {k}") };
        let n: u64 = v.parse().unwrap();
        match k.as_str() {
            "fuel_per_call" => l.fuel_per_call = n,
            "memory_bytes" => l.memory_bytes = n,
            "max_output_bytes" => l.max_output_bytes = n,
            "wall_clock_ms" => l.wall_clock = Some(Duration::from_millis(n)),
            other => panic!("unknown limit {other}"),
        }
    }
    l
}

/// AC-3, AC-4 and the hostile-module set (AC-H2 on this host, without the syscall audit): every
/// module gets its hand-labelled outcome within 10 s and the process stays alive.
#[test]
fn hostile_modules_are_contained() {
    let e = expect();
    let CValue::Arr(cases) = &e["hostile"] else { panic!("hostile") };
    assert!(cases.len() >= 25);
    for c in cases {
        let c = obj(c);
        let module = s(&c, "module");
        let stdin = s(&c, "stdin");
        let want_code = s(&c, "code");
        let want_trap = c.get("trap").map(|_| s(&c, "trap"));
        let limits = limits_from(&obj(&c["limits"]));
        let bytes = read(&module);
        let t0 = Instant::now();
        let got =
            Sandbox::load_with_digest(&bytes, &sha(&bytes), limits).and_then(|mut sb| sb.run_command(stdin.as_bytes()));
        let elapsed = t0.elapsed();
        let err = match got {
            Err(err) => err,
            Ok(out) => panic!("{module} {stdin:?}: expected {want_code}, got {out:?}"),
        };
        assert_eq!(err.code.as_str(), want_code, "{module} {stdin:?}: {err}");
        assert_eq!(err.trap.map(|t| t.as_str().to_string()), want_trap, "{module} {stdin:?}: {err}");
        assert!(elapsed < Duration::from_secs(10), "{module} {stdin:?} took {elapsed:?}");
    }
}

/// AC-3 by name: the WASI file-function import is refused at load.
#[test]
fn ac3_file_import_is_forbidden() {
    let bytes = read("imports-fd.wasm");
    let err = Sandbox::load_with_digest(&bytes, &sha(&bytes), Limits::default()).unwrap_err();
    assert_eq!(err.code, ErrorCode::ForbiddenImport);
    assert!(err.detail.contains("path_open"), "{err}");
}

/// AC-4 by name, with the default limits plus a short wall clock: the infinite loop and the
/// unbounded growth end in their errors within 10 s.
#[test]
fn ac4_loop_and_grow_end_within_ten_seconds() {
    for (module, limits, code) in [
        (
            "loop.wasm",
            Limits { wall_clock: Some(Duration::from_secs(2)), ..Limits::default() },
            ErrorCode::BudgetExhausted,
        ),
        ("grow.wasm", Limits::default(), ErrorCode::MemoryLimit),
    ] {
        let bytes = read(module);
        let t0 = Instant::now();
        let mut sb = Sandbox::load_with_digest(&bytes, &sha(&bytes), limits).unwrap();
        let err = sb.run_command(b"").unwrap_err();
        assert_eq!(err.code, code, "{module}: {err}");
        assert!(t0.elapsed() < Duration::from_secs(10), "{module} took {:?}", t0.elapsed());
    }
}

/// REQ-005: the wall clock is per sandbox (package): once spent, later calls are refused.
#[test]
fn wall_clock_budget_spans_the_package() {
    let bytes = read("loop.wasm");
    let limits = Limits { wall_clock: Some(Duration::from_millis(300)), ..Limits::default() };
    let mut sb = Sandbox::load_with_digest(&bytes, &sha(&bytes), limits).unwrap();
    assert_eq!(sb.run_command(b"").unwrap_err().code, ErrorCode::BudgetExhausted);
    let t0 = Instant::now();
    assert_eq!(sb.run_command(b"").unwrap_err().code, ErrorCode::BudgetExhausted);
    assert!(t0.elapsed() < Duration::from_millis(250), "second call ran instead of being refused");
}

/// REQ-003: the probe sees an empty environment and EBADF for every fd but stdin/stdout/stderr.
#[test]
fn probe_sees_no_host_resources() {
    let e = expect();
    let p = obj(&e["probe"]);
    let bytes = read(&s(&p, "module"));
    let mut sb = Sandbox::load_with_digest(&bytes, &sha(&bytes), Limits::default()).unwrap();
    let out = sb.run_command(b"").unwrap();
    assert_eq!(out, CallResult::Envelope(s(&p, "envelope").into_bytes()));
}

/// AC-5 / REQ-007: the same call twice gives byte-identical envelopes; the digest pinned by the
/// Python model is the same on every CI host (Linux, Windows, macOS, wasm32 under wasmtime).
#[test]
fn ac5_repeat_calls_are_byte_identical() {
    let e = expect();
    let k = obj(&e["kernel"]);
    let req = kernel_request(&e);
    let mut sb = load_kernel(Limits::default()).unwrap();
    let a = sb.call(&req).unwrap();
    let b = sb.call(&req).unwrap();
    let mut fresh = load_kernel(Limits::default()).unwrap();
    let c = fresh.call(&req).unwrap();
    assert_eq!(a, b);
    assert_eq!(a, c);
    assert_eq!(sha(a.bytes()).as_str(), s(&k, "envelope_sha256"));
}

/// No partial result: a call that runs out of fuel after writing output returns only the error.
#[test]
fn budget_exhaustion_returns_no_partial_result() {
    let e = expect();
    let req = kernel_request(&e);
    let mut sb = load_kernel(Limits { fuel_per_call: 5_000, ..Limits::default() }).unwrap();
    let err = sb.call(&req).unwrap_err();
    assert_eq!(err.code, ErrorCode::BudgetExhausted);
}

/// The call encoding refuses IDs that cannot be written without escaping, and huge calls.
#[test]
fn bad_requests_are_refused() {
    let mut sb = load_kernel(Limits::default()).unwrap();
    for id in ["", "a\"b", "a\\b", "with space", "x".repeat(129).as_str()] {
        let req = CallRequest { algorithm: id.to_string(), params: vec![], seed: None, inputs: vec![] };
        assert_eq!(sb.call(&req).unwrap_err().code, ErrorCode::BadRequest, "{id:?}");
    }
    let many = CallRequest {
        algorithm: "a@1".into(),
        params: vec![],
        seed: None,
        inputs: (0..4097).map(|_| CallInput::new(vec![])).collect(),
    };
    assert_eq!(sb.call(&many).unwrap_err().code, ErrorCode::BadRequest);
}

/// The sandbox's import allowlist is exactly KRN.MAN.WASM's `wasm-imports.allow`.
#[test]
fn allowlist_matches_krn_man_wasm() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../fid-kernel-wasm/wasm-imports.allow");
    let text = std::fs::read_to_string(path).unwrap();
    let mut listed: Vec<(String, String)> = text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut it = l.split(' ');
            (it.next().unwrap().to_string(), it.next().unwrap().to_string())
        })
        .collect();
    listed.sort();
    let mut ours: Vec<(String, String)> =
        IMPORT_ALLOWLIST.iter().map(|(m, n)| (m.to_string(), n.to_string())).collect();
    ours.sort();
    assert_eq!(listed, ours);
    assert!(TRAPPED_IMPORTS.iter().all(|(m, _)| *m == "fid_trap"));
}

/// Maintainer decision (2026-11-23): the default memory cap is KRN.MAN.WASM's pinned
/// `max_memory_size`, read here from `crates/fid-kernel-wasm/wasm-runtime.toml`.
#[test]
fn default_memory_cap_matches_krn_wasm_runtime() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../fid-kernel-wasm/wasm-runtime.toml");
    let text = std::fs::read_to_string(path).unwrap();
    let values: Vec<u64> = text
        .lines()
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| k.trim() == "max_memory_size")
        .map(|(_, v)| v.trim().parse().unwrap())
        .collect();
    assert_eq!(values.len(), 1, "max_memory_size must appear exactly once");
    assert_eq!(Limits::default().memory_bytes, values[0]);
    assert_eq!(DEFAULT_MEMORY_BYTES, values[0]);
    assert_eq!(DEFAULT_MEMORY_BYTES, 1_073_741_824);
}

fn panicking_clock() -> Duration {
    panic!("the clock was read without a wall-clock budget")
}

/// The clock is read only when a wall-clock budget is set.
#[test]
fn clock_is_not_read_without_a_wall_clock_budget() {
    let e = expect();
    let req = kernel_request(&e);
    let mut sb = load_kernel(Limits { wall_clock: None, ..Limits::default() }).unwrap().with_clock(panicking_clock);
    assert!(matches!(sb.call(&req).unwrap(), CallResult::Envelope(_)));
    let bytes = read("loop.wasm");
    let limits = Limits { wall_clock: None, fuel_per_call: 1_000_000, ..Limits::default() };
    let mut sb = Sandbox::load_with_digest(&bytes, &sha(&bytes), limits).unwrap().with_clock(panicking_clock);
    assert_eq!(sb.run_command(b"").unwrap_err().code, ErrorCode::BudgetExhausted);
}

thread_local! {
    static SECONDS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// A caller-provided clock that advances one second per reading (per thread).
fn fast_clock() -> Duration {
    SECONDS.with(|s| {
        s.set(s.get() + 1);
        Duration::from_secs(s.get())
    })
}

/// A caller-provided clock drives the wall-clock budget (the only way to set one on
/// `wasm32-unknown-unknown`): the infinite loop stops after ten readings of a clock that
/// gains a second per reading, long before its fuel would run out.
#[test]
fn caller_clock_drives_the_wall_clock_budget() {
    let bytes = read("loop.wasm");
    let limits = Limits { wall_clock: Some(Duration::from_secs(10)), fuel_per_call: u64::MAX, ..Limits::default() };
    let mut sb = Sandbox::load_with_digest(&bytes, &sha(&bytes), limits).unwrap().with_clock(fast_clock);
    let t0 = Instant::now();
    assert_eq!(sb.run_command(b"").unwrap_err().code, ErrorCode::BudgetExhausted);
    assert!(t0.elapsed() < Duration::from_secs(10), "took {:?}", t0.elapsed());
    // The package budget is spent: the next call is refused at once.
    assert_eq!(sb.run_command(b"").unwrap_err().code, ErrorCode::BudgetExhausted);
}
