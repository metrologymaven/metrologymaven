//! KRN.MAN.WASM: the shim's read-call-write contract, run natively against the calls of
//! `fixtures/man/wasm/calls-v1.txt` (written by `make_fixtures.py` from Python's json and
//! hashlib, and from the KRN.MAN.KCI golden results). Rows marked `wasm` need the pinned WASM
//! runtime limits; CI runs every row through the built module with `fid-wasm-check golden`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kci::{evaluate, AlgorithmId, KernelCall};
use fid_kernel_wasm::call::{encode_call, parse_call, CallError, MAX_CALL_BYTES, MAX_INPUTS};
use fid_kernel_wasm::shim::{reference_table, respond, EXIT_BAD_INPUT, EXIT_KERNEL_ERROR};
use fid_kernel_wasm::stand_in;
use std::path::Path;

struct Case {
    name: String,
    wasm_only: bool,
    call: Vec<u8>,
    exit: u8,
    stdout: Vec<u8>,
}

fn cases() -> Vec<Case> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/man/wasm/calls-v1.txt");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut out = Vec::new();
    for line in text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let f: Vec<&str> = line.split('\t').collect();
        assert_eq!(f.len(), 5, "{line}");
        out.push(Case {
            name: f[0].to_string(),
            wasm_only: match f[1] {
                "any" => false,
                "wasm" => true,
                m => panic!("mode {m}"),
            },
            call: f[2].as_bytes().to_vec(),
            exit: f[3].parse().unwrap(),
            stdout: f[4].as_bytes().to_vec(),
        });
    }
    out
}

#[test]
fn ac2_every_native_row_gives_the_golden_stdout_and_exit_code() {
    let table = stand_in::table().expect("stand-in table builds");
    let all = cases();
    assert_eq!(all.len(), 43);
    let mut ran = 0;
    for c in all.iter().filter(|c| !c.wasm_only) {
        let (code, out) = respond(Some(&table), &c.call);
        assert_eq!(code, c.exit, "{}", c.name);
        assert_eq!(String::from_utf8(out).unwrap(), String::from_utf8(c.stdout.clone()).unwrap(), "{}", c.name);
        ran += 1;
    }
    assert_eq!(ran, 41);
    // The 20 KRN.MAN.KCI calls are all present and all succeed.
    assert_eq!(all.iter().filter(|c| c.name.starts_with("kci-call-") && c.exit == 0).count(), 20);
}

#[test]
fn well_formed_calls_round_trip_byte_for_byte_and_malformed_ones_are_refused() {
    for c in cases() {
        if c.exit == EXIT_BAD_INPUT {
            assert!(matches!(parse_call(&c.call), Err(CallError::Malformed { .. })), "{}", c.name);
        } else {
            let call = parse_call(&c.call).unwrap_or_else(|e| panic!("{}: {e:?}", c.name));
            assert_eq!(encode_call(&call), c.call, "{}", c.name);
        }
    }
}

#[test]
fn the_shim_answers_exactly_what_evaluate_returns() {
    let table = stand_in::table().unwrap();
    for c in cases().iter().filter(|c| !c.wasm_only && c.exit == 0) {
        let call = parse_call(&c.call).unwrap();
        let direct = evaluate(&table, &call).unwrap().canonical_json();
        assert_eq!(respond(Some(&table), &c.call).1, direct, "{}", c.name);
    }
}

fn echo_call() -> KernelCall {
    KernelCall {
        algorithm: AlgorithmId::parse(stand_in::ECHO_ID).unwrap(),
        params: Vec::new(),
        seed: None,
        inputs: Vec::new(),
    }
}

#[test]
fn oversized_and_bounded_inputs() {
    let big = vec![b'{'; MAX_CALL_BYTES + 1];
    assert_eq!(parse_call(&big), Err(CallError::Oversized));
    assert_eq!(respond(None, &big), (EXIT_BAD_INPUT, b"{\"error\":\"oversized_call\"}".to_vec()));

    let mut call = echo_call();
    call.inputs = vec![(Vec::new(), fid_kci::kci::sha256_digest(b"")); MAX_INPUTS];
    let at_limit = encode_call(&call);
    assert_eq!(parse_call(&at_limit).unwrap().inputs.len(), MAX_INPUTS);
    call.inputs.push((Vec::new(), fid_kci::kci::sha256_digest(b"")));
    assert!(matches!(parse_call(&encode_call(&call)), Err(CallError::Malformed { .. })));
}

#[test]
fn malformed_edges() {
    let good = encode_call(&echo_call());
    assert_eq!(parse_call(&good).unwrap(), echo_call());
    let mut nl = good.clone();
    nl.push(b'\n');
    assert!(parse_call(&nl).is_err(), "trailing newline");
    assert!(parse_call(&good[..good.len() - 1]).is_err(), "truncated");
    let long_id = format!("{{\"algorithm\":\"a.b.{}@1.0.0\",\"inputs\":[],\"params\":\"\"}}", "c".repeat(200));
    assert!(parse_call(long_id.as_bytes()).is_err(), "ID over 128 bytes");
    let max_seed =
        b"{\"algorithm\":\"test.echo.identity@1.0.0\",\"inputs\":[],\"params\":\"\",\"seed\":\"18446744073709551615\"}";
    assert_eq!(parse_call(max_seed).unwrap().seed, Some(u64::MAX));
    let empty_seed = b"{\"algorithm\":\"test.echo.identity@1.0.0\",\"inputs\":[],\"params\":\"\",\"seed\":\"\"}";
    assert!(parse_call(empty_seed).is_err());
    let dangling_comma = b"{\"algorithm\":\"test.echo.identity@1.0.0\",\"inputs\":[,],\"params\":\"\"}";
    assert!(parse_call(dangling_comma).is_err());
}

#[test]
fn without_a_table_and_with_the_feature() {
    let good = encode_call(&echo_call());
    assert_eq!(respond(None, &good), (EXIT_KERNEL_ERROR, b"{\"error\":\"table_unavailable\"}".to_vec()));
    let reference = reference_table().unwrap();
    // With the test-algorithms feature (required by this test file) the stand-ins answer.
    assert_eq!(respond(Some(&reference), &good).0, 0);
}

#[test]
fn stand_in_table_has_cards_for_every_algorithm() {
    let t = stand_in::table().unwrap();
    let ids: Vec<String> = t.ids().iter().map(|i| i.to_string()).collect();
    assert_eq!(ids, [stand_in::ECHO_ID, stand_in::ALLOC_ID, stand_in::RECURSE_ID]);
    assert!(fid_kci::check(&t.ids(), &t.cards().ids()).is_clean());
}
