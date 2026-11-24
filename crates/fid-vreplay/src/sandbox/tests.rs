//! Unit tests of the interpreter against the independent numeric conformance set
//! `fixtures/replay/sandbox/ops.wasm` + `ops-expect.txt` (exact rational model in
//! `make_fixtures.py`), and of the module reader on edge cases.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::exec::{End, Host, Instance, RunLimits};
use super::module::parse;
use super::{ErrorCode, TrapKind};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/replay/sandbox");

fn limits() -> RunLimits {
    RunLimits { max_depth: 1000, max_slots: 1 << 20, deadline: None }
}

fn trap_name(k: TrapKind) -> &'static str {
    k.as_str()
}

#[test]
fn numeric_conformance_matches_the_exact_model() {
    let bytes = std::fs::read(format!("{FIX}/ops.wasm")).unwrap();
    let expect = std::fs::read_to_string(format!("{FIX}/ops-expect.txt")).unwrap();
    let m = parse(&bytes).unwrap();
    let mut inst = Instance::new(&m, 1, 1_000_000).unwrap();
    let mut host = Host { stdin: &[], cursor: 0, stdout: Vec::new(), stderr_bytes: 0, max_output: 0 };
    let mut failures = Vec::new();
    for line in expect.lines() {
        let parts: Vec<&str> = line.split(' ').collect();
        let [name, kind, want] = parts[..] else { panic!("bad line {line}") };
        let f = m.export_func(name).unwrap();
        let got = inst.invoke(f, &mut host, &limits());
        let ok = match (kind, &got) {
            ("trap", Err(e)) => e.code == ErrorCode::Trap && e.trap.map(trap_name) == Some(want),
            ("i32", Ok(End::Returned(v))) => {
                v.len() == 1 && v[0] == u64::from_str_radix(want, 16).unwrap() && v[0] >> 32 == 0
            }
            ("i64", Ok(End::Returned(v))) => v.len() == 1 && v[0] == u64::from_str_radix(want, 16).unwrap(),
            _ => false,
        };
        if !ok {
            failures.push(format!("{line}: got {got:?}"));
        }
    }
    let n = expect.lines().count();
    assert!(n > 5000, "only {n} cases");
    assert!(failures.is_empty(), "{} of {n} cases differ:\n{}", failures.len(), failures.join("\n"));
}

/// Builds a module with one `_start` body (no locals) and the given extra sections.
fn module_with_body(body: &[u8]) -> Vec<u8> {
    let mut code = vec![0u8]; // no locals
    code.extend_from_slice(body);
    let mut out = b"\0asm\x01\0\0\0".to_vec();
    out.extend_from_slice(&[1, 4, 1, 0x60, 0, 0]); // type [] -> []
    out.extend_from_slice(&[3, 2, 1, 0]); // one function
    out.extend_from_slice(&[7, 10, 1, 6]);
    out.extend_from_slice(b"_start");
    out.extend_from_slice(&[0, 0]);
    let mut sec = vec![1u8, code.len() as u8];
    sec.extend_from_slice(&code);
    out.push(10);
    out.push(sec.len() as u8);
    out.extend_from_slice(&sec);
    out
}

#[test]
fn reader_refuses_malformed_bodies() {
    // A valid empty body.
    assert!(parse(&module_with_body(&[0x0b])).is_ok());
    let cases: &[(&str, &[u8])] = &[
        ("stack underflow", &[0x1a, 0x0b]),
        ("type mismatch", &[0x42, 0x01, 0x41, 0x01, 0x6a, 0x1a, 0x0b]),
        ("value left over", &[0x41, 0x01, 0x0b]),
        ("missing end", &[0x01]),
        ("branch out of range", &[0x0c, 0x01, 0x0b]),
        ("else without if", &[0x05, 0x0b]),
        ("unsupported opcode", &[0xd0, 0x70, 0x1a, 0x0b]),
        ("memory op without memory", &[0x41, 0x00, 0x28, 0x02, 0x00, 0x1a, 0x0b]),
        ("bytes after end", &[0x0b, 0x01]),
        ("overlong leb", &[0x41, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00, 0x1a, 0x0b]),
        ("i32 const out of range", &[0x41, 0xff, 0xff, 0xff, 0xff, 0x0f, 0x1a, 0x0b]),
    ];
    for (what, body) in cases {
        let err = parse(&module_with_body(body)).expect_err(what);
        assert_eq!(err.code, ErrorCode::MalformedModule, "{what}: {err}");
    }
}

#[test]
fn reader_refuses_bad_headers_and_sections() {
    let ok = module_with_body(&[0x0b]);
    let mut bad_magic = ok.clone();
    bad_magic[0] = b'X';
    let mut component = ok.clone();
    component[4] = 0x0d;
    let mut order = b"\0asm\x01\0\0\0".to_vec();
    order.extend_from_slice(&[3, 1, 0, 1, 1, 0]); // function section before type section
    let empty_ok = b"\0asm\x01\0\0\0".to_vec();
    assert!(parse(&empty_ok).is_ok());
    for (what, bytes) in [
        ("magic", bad_magic),
        ("component", component),
        ("order", order),
        ("truncated", ok[..ok.len() - 1].to_vec()),
        ("unknown section", [&b"\0asm\x01\0\0\0"[..], &[13, 0]].concat()),
    ] {
        let err = parse(&bytes).expect_err(what);
        assert_eq!(err.code, ErrorCode::MalformedModule, "{what}: {err}");
    }
}

#[test]
fn branches_keep_results_and_drop_the_rest() {
    // block (result i32): push 1, 2, 3; br 0 keeps 3 and drops 1, 2; then i32.const 10 add.
    let body = [0x02, 0x7f, 0x41, 0x01, 0x41, 0x02, 0x41, 0x03, 0x0c, 0x00, 0x0b, 0x41, 0x0a, 0x6a, 0x1a, 0x0b];
    let m = parse(&module_with_body(&body)).unwrap();
    let mut inst = Instance::new(&m, 0, 1_000_000).unwrap();
    let mut host = Host { stdin: &[], cursor: 0, stdout: Vec::new(), stderr_bytes: 0, max_output: 0 };
    assert_eq!(inst.invoke(0, &mut host, &limits()).unwrap(), End::Returned(vec![]));
}

#[test]
fn loop_counts_and_br_table_dispatches() {
    // f(): local i; loop: i += 1; br_if 0 (i < 5); end; i; br_table over a block result
    let src = [
        0x01, 0x01, 0x7f, // one i32 local
        0x03, 0x40, // loop
        0x20, 0x00, 0x41, 0x01, 0x6a, 0x22, 0x00, // i = i + 1 (tee)
        0x41, 0x05, 0x48, 0x0d, 0x00, // br_if 0 when i < 5
        0x0b, // end loop
        0x02, 0x7f, // block (result i32)
        0x41, 0x2a, // 42
        0x20, 0x00, // index = i (5) -> default target 0
        0x0e, 0x01, 0x00, 0x00, // br_table [0] default 0
        0x0b, 0x0b,
    ];
    let mut out = b"\0asm\x01\0\0\0".to_vec();
    out.extend_from_slice(&[1, 5, 1, 0x60, 0, 1, 0x7f]);
    out.extend_from_slice(&[3, 2, 1, 0]);
    out.extend_from_slice(&[7, 5, 1, 1, b'f', 0, 0]);
    out.push(10);
    out.push((src.len() + 2) as u8);
    out.push(1);
    out.push(src.len() as u8);
    out.extend_from_slice(&src);
    let m = parse(&out).unwrap();
    let mut inst = Instance::new(&m, 0, 1_000_000).unwrap();
    let mut host = Host { stdin: &[], cursor: 0, stdout: Vec::new(), stderr_bytes: 0, max_output: 0 };
    let f = m.export_func("f").unwrap();
    assert_eq!(inst.invoke(f, &mut host, &limits()).unwrap(), End::Returned(vec![42]));
}

fn uleb(mut n: u32, out: &mut Vec<u8>) {
    loop {
        let b = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn section(id: u8, payload: &[u8], out: &mut Vec<u8>) {
    out.push(id);
    uleb(payload.len() as u32, out);
    out.extend_from_slice(payload);
}

#[test]
fn validation_work_is_bounded() {
    // loop with 900 i32 params, then br_table with 10 000 targets to it: legal, but each
    // target re-checks 900 operands, so validation work exceeds 64 units per body byte.
    let mut types = vec![2u8, 0x60, 0, 0, 0x60];
    uleb(900, &mut types);
    types.extend(std::iter::repeat_n(0x7f, 900));
    types.push(0);
    let mut body = vec![0u8];
    for _ in 0..900 {
        body.extend_from_slice(&[0x41, 0x00]);
    }
    body.extend_from_slice(&[0x03, 0x01, 0x41, 0x00, 0x0e]);
    uleb(10_000, &mut body);
    body.extend(std::iter::repeat_n(0u8, 10_001));
    body.extend_from_slice(&[0x0b, 0x0b]);
    let mut code = vec![1u8];
    uleb(body.len() as u32, &mut code);
    code.extend_from_slice(&body);
    let mut m = b"\0asm\x01\0\0\0".to_vec();
    section(1, &types, &mut m);
    section(3, &[1, 0], &mut m);
    section(10, &code, &mut m);
    let err = parse(&m).expect_err("expensive body accepted");
    assert_eq!(err.code, ErrorCode::MalformedModule);
    assert!(err.detail.contains("too expensive"), "{err}");
}

#[test]
fn bulk_memory_costs_fuel_per_kib() {
    // memory.fill of 64 KiB costs 1 + 64 units: 60 units of fuel are not enough, 80 are.
    let body = [0x00, 0x41, 0x00, 0x41, 0x01, 0x41, 0x80, 0x80, 0x04, 0xfc, 0x0b, 0x00, 0x0b];
    let mut code = vec![1u8, body.len() as u8];
    code.extend_from_slice(&body);
    let mut m = b"\0asm\x01\0\0\0".to_vec();
    section(1, &[1, 0x60, 0, 0], &mut m);
    section(3, &[1, 0], &mut m);
    section(5, &[1, 0, 1], &mut m);
    section(7, &[1, 1, b'f', 0, 0], &mut m);
    section(10, &code, &mut m);
    let m = parse(&m).unwrap();
    let mut host = Host { stdin: &[], cursor: 0, stdout: Vec::new(), stderr_bytes: 0, max_output: 0 };
    let mut poor = Instance::new(&m, 1, 60).unwrap();
    assert_eq!(poor.invoke(0, &mut host, &limits()).unwrap_err().code, ErrorCode::BudgetExhausted);
    let mut rich = Instance::new(&m, 1, 80).unwrap();
    assert_eq!(rich.invoke(0, &mut host, &limits()).unwrap(), End::Returned(vec![]));
    let mut b = [0u8; 2];
    rich.memory.read(65_534, &mut b).unwrap();
    assert_eq!(b, [1, 1]);
}
