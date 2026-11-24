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

// ------------------------------------------------------------------ module builder

fn sleb(mut v: i64, out: &mut Vec<u8>) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if (v == 0 && b & 0x40 == 0) || (v == -1 && b & 0x40 != 0) {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn i32c(v: i32) -> Vec<u8> {
    let mut o = vec![0x41];
    sleb(i64::from(v), &mut o);
    o
}

fn vector(items: &[Vec<u8>]) -> Vec<u8> {
    let mut o = Vec::new();
    uleb(items.len() as u32, &mut o);
    for i in items {
        o.extend_from_slice(i);
    }
    o
}

fn name(s: &str) -> Vec<u8> {
    let mut o = Vec::new();
    uleb(s.len() as u32, &mut o);
    o.extend_from_slice(s.as_bytes());
    o
}

fn functype(params: &[u8], results: &[u8]) -> Vec<u8> {
    let mut o = vec![0x60];
    uleb(params.len() as u32, &mut o);
    o.extend_from_slice(params);
    uleb(results.len() as u32, &mut o);
    o.extend_from_slice(results);
    o
}

fn export_func(n: &str, idx: u32) -> Vec<u8> {
    let mut o = name(n);
    o.push(0);
    uleb(idx, &mut o);
    o
}

/// A code-section entry: local groups `(count, type)` and the instructions (ending in `end`).
fn body(locals: &[(u32, u8)], instrs: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    uleb(locals.len() as u32, &mut b);
    for &(n, t) in locals {
        uleb(n, &mut b);
        b.push(t);
    }
    b.extend_from_slice(instrs);
    let mut o = Vec::new();
    uleb(b.len() as u32, &mut o);
    o.extend_from_slice(&b);
    o
}

fn build(sections: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut m = b"\0asm\x01\0\0\0".to_vec();
    for (id, p) in sections {
        section(*id, p, &mut m);
    }
    m
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn host() -> Host<'static> {
    Host { stdin: &[], cursor: 0, stdout: Vec::new(), stderr_bytes: 0, max_output: 1 << 20 }
}

fn run(inst: &mut Instance<'_>, m: &super::module::Module, f: &str) -> Result<End, super::SandboxError> {
    inst.invoke(m.export_func(f).unwrap(), &mut host(), &limits())
}

fn assert_trap(got: Result<End, super::SandboxError>, kind: TrapKind) {
    let err = got.expect_err("expected a trap");
    assert_eq!((err.code, err.trap), (ErrorCode::Trap, Some(kind)), "{err}");
}

fn instantiate_err(m: &super::module::Module, cap_pages: u32) -> super::SandboxError {
    match Instance::new(m, cap_pages, 1_000) {
        Ok(_) => panic!("instantiation succeeded"),
        Err(e) => e,
    }
}

// ------------------------------------------------------------------ load-time bounds

use super::module::{parse_counted, Work, WORK_MAX_STEP};
use std::time::{Duration, Instant};

/// Generous wall-clock backstop for the worst-case load shapes (debug build, wasm32 under
/// wasmtime included); the quadratic versions of these loads take minutes.
const LOAD_BACKSTOP: Duration = Duration::from_secs(10);

/// 100 000 distinct function types (the type-count cap): signature identity is a sorted map,
/// not a linear search per type.
#[test]
fn many_distinct_types_load_in_n_log_n() {
    let n = 100_000u32;
    let mut types = Vec::new();
    uleb(n, &mut types);
    for i in 0..n - 1 {
        let params: Vec<u8> = (0..17).map(|b| if (i >> b) & 1 == 1 { 0x7e } else { 0x7f }).collect();
        types.extend_from_slice(&functype(&params, &[]));
    }
    // The last type repeats type 7.
    let params7: Vec<u8> = (0..17).map(|b| if (7u32 >> b) & 1 == 1 { 0x7e } else { 0x7f }).collect();
    types.extend_from_slice(&functype(&params7, &[]));
    let bytes = build(&[(1, types)]);
    let t0 = Instant::now();
    let mut work = Work::for_module(bytes.len());
    let m = parse_counted(&bytes, &mut work).unwrap();
    assert!(t0.elapsed() < LOAD_BACKSTOP, "took {:?}", t0.elapsed());
    assert!(work.used <= work.cap);
    assert_eq!(m.canon_types.len(), n as usize);
    assert!(m.canon_types[..n as usize - 1].iter().enumerate().all(|(i, &c)| c as usize == i));
    assert_eq!(m.canon_types[n as usize - 1], 7);
}

/// 100 000 exports (the export-count cap): duplicate names are found with a sorted set.
#[test]
fn many_exports_load_in_n_log_n_and_duplicates_are_refused() {
    let n = 100_000u32;
    let exports: Vec<Vec<u8>> = (0..n).map(|i| export_func(&format!("e{i}"), 0)).collect();
    let sections = |exports: Vec<Vec<u8>>| {
        build(&[
            (1, vector(&[functype(&[], &[])])),
            (3, vec![1, 0]),
            (7, vector(&exports)),
            (10, vector(&[body(&[], &[0x0b])])),
        ])
    };
    let bytes = sections(exports.clone());
    let t0 = Instant::now();
    let mut work = Work::for_module(bytes.len());
    let m = parse_counted(&bytes, &mut work).unwrap();
    assert!(t0.elapsed() < LOAD_BACKSTOP, "took {:?}", t0.elapsed());
    assert!(work.used <= work.cap);
    assert_eq!(m.exports.len(), n as usize);
    let mut dup = exports;
    *dup.last_mut().unwrap() = export_func("e4242", 0);
    let err = parse(&sections(dup)).unwrap_err();
    assert_eq!(err.code, ErrorCode::MalformedModule);
    assert!(err.detail.contains("duplicate export"), "{err}");
}

/// A `br_table` with 100 000 targets to a label of 1 000 values: the budget is checked per
/// target, so the refusal comes within one target's work of the cap instead of after all
/// 2 * 10^8 units.
#[test]
fn br_table_work_is_checked_per_target() {
    let arity = 1_000u32;
    let targets = 100_000u32;
    let types = vector(&[functype(&[], &[]), functype(&vec![0x7f; arity as usize], &[])]);
    let mut code = Vec::new();
    for _ in 0..arity {
        code.extend_from_slice(&[0x41, 0x00]);
    }
    code.extend_from_slice(&[0x03, 0x01, 0x41, 0x00, 0x0e]);
    uleb(targets, &mut code);
    code.extend(std::iter::repeat_n(0u8, targets as usize + 1));
    code.extend_from_slice(&[0x0b, 0x0b]);
    let bytes = build(&[(1, types), (3, vec![1, 0]), (10, vector(&[body(&[], &code)]))]);
    let t0 = Instant::now();
    let mut work = Work::for_module(bytes.len());
    let err = parse_counted(&bytes, &mut work).unwrap_err();
    assert!(t0.elapsed() < LOAD_BACKSTOP, "took {:?}", t0.elapsed());
    assert_eq!(err.code, ErrorCode::MalformedModule);
    assert!(err.detail.contains("too expensive"), "{err}");
    assert!(work.used > work.cap);
    assert!(work.used <= work.cap + WORK_MAX_STEP, "work {} overshoots the cap {}", work.used, work.cap);
}

/// Thousands of tiny bodies each declaring 50 000 locals: locals are charged to the
/// module-wide budget, so the total stays linear in the artifact size.
#[test]
fn locals_are_charged_to_the_module_budget() {
    let funcs = 2_000u32;
    let mut fsec = Vec::new();
    uleb(funcs, &mut fsec);
    fsec.extend(std::iter::repeat_n(0u8, funcs as usize));
    let bodies: Vec<Vec<u8>> = (0..funcs).map(|_| body(&[(50_000, 0x7f)], &[0x0b])).collect();
    let bytes = build(&[(1, vector(&[functype(&[], &[])])), (3, fsec), (10, vector(&bodies))]);
    let t0 = Instant::now();
    let mut work = Work::for_module(bytes.len());
    let err = parse_counted(&bytes, &mut work).unwrap_err();
    assert!(t0.elapsed() < LOAD_BACKSTOP, "took {:?}", t0.elapsed());
    assert!(err.detail.contains("too expensive"), "{err}");
    assert!(work.used <= work.cap + 50_000, "work {} overshoots the cap {}", work.used, work.cap);
}

/// Parameters plus declared locals may total exactly 50 000, split over several groups.
#[test]
fn locals_are_counted_once() {
    let module = |groups: &[(u32, u8)]| {
        build(&[(1, vector(&[functype(&[0x7f, 0x7e], &[])])), (3, vec![1, 0]), (10, vector(&[body(groups, &[0x0b])]))])
    };
    let m = parse(&module(&[(20_000, 0x7f), (20_000, 0x7e), (9_998, 0x7c)])).unwrap();
    assert_eq!(m.funcs[0].locals, 49_998);
    let err = parse(&module(&[(20_000, 0x7f), (20_000, 0x7e), (9_999, 0x7c)])).unwrap_err();
    assert!(err.detail.contains("too many locals"), "{err}");
}

// ------------------------------------------------------------------ memory addressing

/// `u32` address plus `u32` offset is a 33-bit effective address: it never wraps to a valid
/// low address.
#[test]
fn address_plus_offset_does_not_wrap() {
    let mut code_load = i32c(1);
    code_load.extend_from_slice(&[0x28, 0x02]);
    uleb(u32::MAX, &mut code_load);
    code_load.push(0x0b);
    let mut code_store = cat(&[&i32c(-1), &i32c(7), &[0x36, 0x02]]);
    uleb(1, &mut code_store);
    code_store.extend_from_slice(&i32c(0));
    code_store.push(0x0b);
    let mut code_ok = i32c(0);
    code_ok.extend_from_slice(&[0x28, 0x02]);
    uleb(65_532, &mut code_ok);
    code_ok.push(0x0b);
    let bytes = build(&[
        (1, vector(&[functype(&[], &[0x7f])])),
        (3, vec![3, 0, 0, 0]),
        (5, vec![1, 0, 1]),
        (7, vector(&[export_func("load", 0), export_func("store", 1), export_func("ok", 2)])),
        (10, vector(&[body(&[], &code_load), body(&[], &code_store), body(&[], &code_ok)])),
    ]);
    let m = parse(&bytes).unwrap();
    let mut inst = Instance::new(&m, 1, 1_000_000).unwrap();
    assert_trap(run(&mut inst, &m, "load"), TrapKind::OutOfBoundsMemoryAccess);
    assert_trap(run(&mut inst, &m, "store"), TrapKind::OutOfBoundsMemoryAccess);
    assert_eq!(run(&mut inst, &m, "ok").unwrap(), End::Returned(vec![0]));
}

/// Accesses at the last bytes of a full 4 GiB memory: the address arithmetic must not
/// overflow `usize` on a 32-bit host (the wasm32 CI job runs this).
#[test]
fn four_gib_edge_access() {
    let edge = cat(&[&i32c(-1), &i32c(0xab), &[0x3a, 0x00, 0x00], &i32c(-4), &[0x28, 0x02, 0x00, 0x0b]]);
    let offset_edge = cat(&[&i32c(-2), &[0x2d, 0x00, 0x01, 0x0b]]);
    let past = cat(&[&i32c(-3), &[0x28, 0x02, 0x00, 0x0b]]);
    let bulk = cat(&[
        &i32c(-2),
        &i32c(0x5a),
        &i32c(2),
        &[0xfc, 0x0b, 0x00],
        &i32c(0),
        &i32c(-2),
        &i32c(2),
        &[0xfc, 0x0a, 0x00, 0x00],
        &i32c(0),
        &[0x2f, 0x01, 0x00, 0x0b],
    ]);
    let bytes = build(&[
        (1, vector(&[functype(&[], &[0x7f])])),
        (3, vec![4, 0, 0, 0, 0]),
        (5, vec![1, 0, 0x80, 0x80, 0x04]),
        (
            7,
            vector(&[
                export_func("edge", 0),
                export_func("offset_edge", 1),
                export_func("past", 2),
                export_func("bulk", 3),
            ]),
        ),
        (10, vector(&[body(&[], &edge), body(&[], &offset_edge), body(&[], &past), body(&[], &bulk)])),
    ]);
    let m = parse(&bytes).unwrap();
    let mut inst = Instance::new(&m, 65_536, 1_000_000).unwrap();
    assert_eq!(run(&mut inst, &m, "edge").unwrap(), End::Returned(vec![0xab00_0000]));
    assert_eq!(run(&mut inst, &m, "offset_edge").unwrap(), End::Returned(vec![0xab]));
    assert_trap(run(&mut inst, &m, "past"), TrapKind::OutOfBoundsMemoryAccess);
    assert_eq!(run(&mut inst, &m, "bulk").unwrap(), End::Returned(vec![0x5a5a]));
    let mut last = [0u8; 1];
    inst.memory.read(0xffff_ffff, &mut last).unwrap();
    assert_eq!(last, [0x5a]);
    assert!(inst.memory.read(0xffff_ffff, &mut [0u8; 2]).is_err());
}

// ------------------------------------------------------------------ tables and segments

fn table_module(elem_offset: i32, elem_funcs: &[u8]) -> Vec<u8> {
    let call = |i: i32| cat(&[&i32c(i), &[0x11, 0x00, 0x00, 0x0b]]);
    let elem =
        cat(&[&[1, 0], &i32c(elem_offset), &[0x0b], &vector(&elem_funcs.iter().map(|&f| vec![f]).collect::<Vec<_>>())]);
    build(&[
        (1, vector(&[functype(&[], &[])])),
        (3, vec![3, 0, 0, 0]),
        (4, vec![1, 0x70, 0, 1]),
        (7, vector(&[export_func("ok", 1), export_func("oob", 2)])),
        (9, elem),
        (10, vector(&[body(&[], &[0x0b]), body(&[], &call(0)), body(&[], &call(1))])),
    ])
}

/// `call_indirect` with an index at or past the table length is `table_out_of_bounds`.
#[test]
fn call_indirect_outside_the_table_is_table_out_of_bounds() {
    let m = parse(&table_module(0, &[0])).unwrap();
    let mut inst = Instance::new(&m, 0, 1_000_000).unwrap();
    assert_eq!(run(&mut inst, &m, "ok").unwrap(), End::Returned(vec![]));
    assert_trap(run(&mut inst, &m, "oob"), TrapKind::TableOutOfBounds);
}

/// An element segment that does not fit the table fails instantiation with
/// `table_out_of_bounds`; one ending exactly at the table's end fits.
#[test]
fn element_segment_out_of_bounds_at_instantiation() {
    for (offset, funcs) in [(1, &[0u8][..]), (0, &[0, 0][..]), (-1, &[0][..]), (2, &[][..])] {
        let m = parse(&table_module(offset, funcs)).unwrap();
        let err = instantiate_err(&m, 0);
        assert_eq!((err.code, err.trap), (ErrorCode::Trap, Some(TrapKind::TableOutOfBounds)), "{offset}: {err}");
    }
    for (offset, funcs) in [(0, &[0u8][..]), (1, &[][..])] {
        let m = parse(&table_module(offset, funcs)).unwrap();
        assert!(Instance::new(&m, 0, 1_000).is_ok(), "{offset} {funcs:?}");
    }
}

/// A data segment that does not fit linear memory fails instantiation with
/// `out_of_bounds_memory_access`; one ending exactly at the memory's end fits.
#[test]
fn data_segment_out_of_bounds_at_instantiation() {
    let module = |offset: i32, len: u32| {
        let mut seg = cat(&[&[1, 0], &i32c(offset), &[0x0b]]);
        uleb(len, &mut seg);
        seg.extend(std::iter::repeat_n(0x11u8, len as usize));
        build(&[(5, vec![1, 0, 1]), (11, seg)])
    };
    for (offset, len) in [(65_535, 2), (-1, 1), (65_537, 0), (i32::MIN, 1)] {
        let m = parse(&module(offset, len)).unwrap();
        let err = instantiate_err(&m, 1);
        assert_eq!(
            (err.code, err.trap),
            (ErrorCode::Trap, Some(TrapKind::OutOfBoundsMemoryAccess)),
            "{offset} {len}: {err}"
        );
    }
    for (offset, len) in [(65_534, 2), (65_536, 0)] {
        let m = parse(&module(offset, len)).unwrap();
        let inst = Instance::new(&m, 1, 1_000).map_err(|e| e.to_string()).unwrap();
        if len > 0 {
            let mut b = [0u8; 2];
            inst.memory.read(65_534, &mut b).unwrap();
            assert_eq!(b, [0x11, 0x11]);
        }
    }
}

// ------------------------------------------------------------------ WASI host loops

fn wasi_module() -> super::module::Module {
    let call = |fd: i32, n: i32, import: u8| cat(&[&i32c(fd), &i32c(0), &i32c(n), &i32c(8_192), &[0x10, import, 0x0b]]);
    let import = |n: &str| cat(&[&name("wasi_snapshot_preview1"), &name(n), &[0x00, 0x00]]);
    let bytes = build(&[
        (1, vector(&[functype(&[0x7f; 4], &[0x7f]), functype(&[], &[0x7f])])),
        (2, vector(&[import("fd_write"), import("fd_read")])),
        (3, vec![4, 1, 1, 1, 1]),
        (5, vec![1, 0, 1]),
        (
            7,
            vector(&[
                export_func("w1025", 2),
                export_func("w1024", 3),
                export_func("r1025", 4),
                export_func("r1024", 5),
            ]),
        ),
        (
            10,
            vector(&[
                body(&[], &call(1, 1025, 0)),
                body(&[], &call(1, 1024, 0)),
                body(&[], &call(0, 1025, 1)),
                body(&[], &call(0, 1024, 1)),
            ]),
        ),
    ]);
    parse(&bytes).unwrap()
}

/// `fd_read` and `fd_write` refuse more than 1 024 iovecs with `EINVAL` (28).
#[test]
fn wasi_iovec_count_is_capped() {
    let m = wasi_module();
    let mut inst = Instance::new(&m, 1, 1_000_000).unwrap();
    for (f, errno) in [("w1025", 28), ("w1024", 0), ("r1025", 28), ("r1024", 0)] {
        assert_eq!(run(&mut inst, &m, f).unwrap(), End::Returned(vec![errno]), "{f}");
    }
}

thread_local! {
    static TICKS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// A clock that advances 1 ms on every reading (per thread, so parallel tests do not interfere).
fn ticking_clock() -> Duration {
    TICKS.with(|t| {
        t.set(t.get() + 1);
        Duration::from_millis(t.get())
    })
}

/// The watchdog is polled inside WASI iovec loops: a 1 024-iovec write with a 100-tick
/// deadline stops in the host loop, though the guest executed far fewer than 65 536 operations.
#[test]
fn watchdog_is_polled_inside_host_loops() {
    use super::exec::Deadline;
    let m = wasi_module();
    let f = m.export_func("w1024").unwrap();
    let mut inst = Instance::new(&m, 1, 1_000_000).unwrap();
    let near = RunLimits {
        max_depth: 1000,
        max_slots: 1 << 20,
        deadline: Some(Deadline { clock: ticking_clock, at: ticking_clock() + Duration::from_millis(100) }),
    };
    let err = inst.invoke(f, &mut host(), &near).unwrap_err();
    assert_eq!(err.code, ErrorCode::BudgetExhausted, "{err}");
    let far = RunLimits {
        max_depth: 1000,
        max_slots: 1 << 20,
        deadline: Some(Deadline { clock: ticking_clock, at: ticking_clock() + Duration::from_secs(3600) }),
    };
    let mut inst = Instance::new(&m, 1, 1_000_000).unwrap();
    assert_eq!(inst.invoke(f, &mut host(), &far).unwrap(), End::Returned(vec![0]));
}
