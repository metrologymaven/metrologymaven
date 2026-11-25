//! Acceptance criteria of spec node AUTO.PROG.FLOW. Programs, expected CFGs and expected
//! diagnostics come from `fixtures/program/flow/make_fixtures.py` (diagnostics hand-labelled
//! from the spec's rules, CFGs from an independent Python lowering); nothing here derives an
//! expected value from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_program::flow::{
    call_stop, document_diagnostics_value, iteration_stop, MAX_CALL_DEPTH, MAX_ITERATIONS, MIN_ITERATIONS,
};
use fid_program::{
    analyse_flow, check_types, iteration_limits, Branch, EdgeKind, FlowCode, FlowReport, LoopMode, Program, Severity,
    Step, StepKind, SubId, Target, TypeReport, Uuid7,
};
use fid_registry::{Catalogue, Manifest};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn dir() -> PathBuf {
    root().join("fixtures/program/flow")
}

fn expected() -> BTreeMap<String, CValue> {
    let v = parse(&std::fs::read(dir().join("expected.json")).unwrap()).unwrap();
    let CValue::Obj(m) = v else { panic!("expected.json root") };
    m
}

fn section(key: &str) -> BTreeMap<String, CValue> {
    let Some(CValue::Obj(c)) = expected().get(key).cloned() else { panic!("{key}") };
    c
}

fn path_of(key: &str) -> PathBuf {
    let CValue::Str(rel) = expected().get(key).cloned().unwrap() else { panic!("{key}") };
    root().join(rel)
}

fn types(p: &Program) -> TypeReport {
    let catalogue = Catalogue::from_json(&std::fs::read(path_of("catalogue")).unwrap()).unwrap();
    let manifest = Manifest::from_json(&std::fs::read(path_of("manifest")).unwrap()).unwrap();
    check_types(p, &manifest, &catalogue)
}

fn program(name: &str) -> Program {
    Program::parse(&std::fs::read(dir().join(format!("{name}.json"))).unwrap())
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn run(name: &str) -> FlowReport {
    let p = program(name);
    analyse_flow(&p, &types(&p))
}

fn bytes(v: &CValue) -> String {
    String::from_utf8(to_canonical_bytes(v).unwrap()).unwrap()
}

fn case(name: &str) -> BTreeMap<String, CValue> {
    let Some(CValue::Obj(c)) = section("cases").get(name).cloned() else { panic!("no expected case {name}") };
    c
}

/// The report's CFGs and diagnostics equal the expected ones, byte for byte.
fn assert_case(name: &str) -> FlowReport {
    let want = case(name);
    let got = run(name);
    assert_eq!(bytes(&got.cfg_value()), bytes(&want["cfg"]), "{name}: cfg");
    assert_eq!(bytes(&got.diagnostics_value()), bytes(&want["diagnostics"]), "{name}: diagnostics");
    got
}

#[test]
fn every_case_matches() {
    let names: Vec<String> = section("cases").keys().cloned().collect();
    assert_eq!(names.len(), 16);
    for n in names {
        assert_case(&n);
    }
}

/// AC-1: valid programs have no error, `recursion` once for the recursive one, and the CFG
/// equals `*.cfg.json`.
#[test]
fn ac1_valid_programs() {
    let mut names: Vec<String> = std::fs::read_dir(dir().join("valid"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter_map(|f| f.strip_suffix(".cfg.json").map(str::to_string))
        .collect();
    names.sort();
    assert_eq!(names, ["early-return", "else-if", "nested-loops", "recursion"]);
    for n in names {
        let name = format!("valid/{n}");
        let p = program(&name);
        let t = types(&p);
        assert!(t.is_clean(), "{name}: {:?}", t.diagnostics);
        let r = analyse_flow(&p, &t);
        assert!(!r.has_errors(), "{name}: {:?}", r.diagnostics);
        let cfg = std::fs::read(dir().join(format!("{name}.cfg.json"))).unwrap();
        assert_eq!(to_canonical_bytes(&r.cfg_value()).unwrap(), cfg, "{name}");
        let rec: Vec<_> = r.diagnostics.iter().filter(|d| d.code == FlowCode::Recursion).collect();
        if n == "recursion" {
            assert_eq!(r.diagnostics.len(), 1);
            assert_eq!(rec.len(), 1);
            assert_eq!(rec[0].severity(), Severity::Info);
            assert_eq!(rec[0].names, ["fact"]);
        } else {
            assert!(r.diagnostics.is_empty(), "{name}: {:?}", r.diagnostics);
        }
        assert_case(&name);
    }
}

/// AC-2: each bad jump yields one `illegal_jump` naming the jump step and the label, and no
/// jump edge.
#[test]
fn ac2_illegal_jumps() {
    for (name, label, reason) in [
        ("jump-into-loop", "inside", "into_body"),
        ("jump-other-sub", "there", "other_subroutine"),
        ("jump-undefined", "nowhere", "undefined"),
    ] {
        let r = assert_case(name);
        assert_eq!(r.diagnostics.len(), 1, "{name}");
        let d = &r.diagnostics[0];
        assert_eq!(d.code, FlowCode::IllegalJump);
        assert_eq!(d.names, [label, reason]);
        let jump = program(name).main[0].step_id;
        assert_eq!(d.step, Some(jump));
        let main = &r.cfgs[&SubId::Main];
        assert!(main.successors(jump).iter().all(|(_, k)| *k != EdgeKind::Jump), "{name}");
    }
}

/// AC-3: a loop without `max_iterations`, with 0, and above the limit.
#[test]
fn ac3_iteration_limits() {
    let docs = section("documents");
    assert_eq!(docs.len(), 2);
    for (name, want) in docs {
        let raw = std::fs::read(dir().join(format!("{name}.json"))).unwrap();
        assert_eq!(Program::parse(&raw).unwrap_err().code(), "schema_invalid", "{name}");
        let got = iteration_limits(&raw).unwrap();
        assert_eq!(bytes(&document_diagnostics_value(&got)), bytes(&want), "{name}");
        let code =
            if name.ends_with("no-limit") { FlowCode::MissingIterationLimit } else { FlowCode::BadIterationLimit };
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].code, code);
    }
    // Above the limit: the format accepts it; both checks refuse it.
    let raw = std::fs::read(dir().join("loop-limit-high.json")).unwrap();
    let d = iteration_limits(&raw).unwrap();
    assert_eq!(d.len(), 1);
    assert_eq!((d[0].code, d[0].names.clone()), (FlowCode::BadIterationLimit, vec!["2000000".to_string()]));
    assert_case("loop-limit-high");
    // In memory, 0 and the bounds.
    let mut p = program("loop-limit-high");
    for (n, bad) in [(0, true), (MIN_ITERATIONS, false), (MAX_ITERATIONS, false), (MAX_ITERATIONS + 1, true)] {
        let StepKind::Loop { max_iterations, .. } = &mut p.main[0].kind else { panic!("loop") };
        *max_iterations = n;
        let r = analyse_flow(&p, &TypeReport::default());
        let codes: Vec<_> = r.diagnostics.iter().map(|d| d.code).collect();
        assert_eq!(codes == [FlowCode::BadIterationLimit], bad, "{n}");
    }
    // Valid documents have no iteration diagnostic.
    for n in ["valid/nested-loops", "valid/early-return", "bad-condition"] {
        let raw = std::fs::read(dir().join(format!("{n}.json"))).unwrap();
        assert!(iteration_limits(&raw).unwrap().is_empty(), "{n}");
    }
    assert!(iteration_limits(b"{").is_err());
}

/// AC-4: `area(w, h) -> area` with a path ending without return; `area(w = 3[mm])`.
#[test]
fn ac4_returns_and_calls() {
    let r = assert_case("area-missing-return");
    assert_eq!(r.diagnostics.len(), 1);
    assert_eq!(r.diagnostics[0].code, FlowCode::MissingReturn);
    assert_eq!(r.diagnostics[0].subroutine, SubId::Sub(0));
    let r = assert_case("area-bad-call");
    assert_eq!(r.diagnostics.len(), 1);
    assert_eq!(r.diagnostics[0].code, FlowCode::BadCall);
    assert_eq!(r.diagnostics[0].names, ["h", "missing"]);
    assert_case("bad-calls");
    assert_case("bad-return");
}

/// AC-5: exactly the steps after the unconditional jump are unreachable.
#[test]
fn ac5_dead_code() {
    let r = assert_case("dead-code");
    let p = program("dead-code");
    let dead: Vec<_> = r.diagnostics.iter().map(|d| (d.code, d.step)).collect();
    let StepKind::If { branches, .. } = &p.main[3].kind else { panic!("if") };
    let want = [p.main[2].step_id, p.main[3].step_id, branches[0].body[0].step_id];
    assert_eq!(dead, want.map(|s| (FlowCode::UnreachableStep, Some(s))));
    assert!(r.diagnostics.iter().all(|d| d.severity() == Severity::Warning));
}

/// AC-6: the same program gives byte-identical output (and equal to the committed bytes on
/// every CI platform, see `every_case_matches`).
#[test]
fn ac6_deterministic() {
    for name in ["valid/early-return", "bad-calls", "mutual-recursion", "bad-condition"] {
        let p = program(name);
        let t = types(&p);
        let a = to_canonical_bytes(&analyse_flow(&p, &t).to_value()).unwrap();
        let b = to_canonical_bytes(&analyse_flow(&p, &t).to_value()).unwrap();
        assert_eq!(a, b, "{name}");
    }
}

#[test]
fn other_rules() {
    let r = assert_case("duplicate-label");
    assert_eq!(r.diagnostics.len(), 1);
    assert_eq!(r.diagnostics[0].code, FlowCode::DuplicateLabel);
    let r = assert_case("bad-condition");
    let codes: Vec<_> = r.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(
        codes,
        [FlowCode::BadCondition, FlowCode::BadCondition, FlowCode::BadLoopBounds, FlowCode::BadCondition]
    );
    let r = assert_case("mutual-recursion");
    assert_eq!(r.diagnostics.last().unwrap().names, ["ping", "pong"]);
    assert_eq!(r.call_graph.edges.len(), 3);
}

#[test]
fn run_time_stops() {
    assert_eq!(MAX_CALL_DEPTH, 64);
    assert_eq!(iteration_stop(10, 10), None);
    assert_eq!(iteration_stop(11, 10).map(|s| s.as_str()), Some("iteration_limit"));
    assert_eq!(call_stop(64), None);
    assert_eq!(call_stop(65).map(|s| s.as_str()), Some("depth_limit"));
}

fn mint(n: u64) -> Uuid7 {
    Uuid7::from_parts(n, [0; 10])
}

/// Deep nesting and 10 000 steps are walked with explicit stacks; no recursion in the analysis.
#[test]
fn deep_and_wide() {
    let mut body: Vec<Step> = vec![Step { step_id: mint(1), kind: StepKind::Comment { text: "x".into() } }];
    for n in 2..400u64 {
        let kind = if n % 2 == 0 {
            StepKind::If { branches: vec![Branch { cond: "true".into(), body }], else_body: Vec::new() }
        } else {
            StepKind::Loop { mode: LoopMode::While { cond: "true".into() }, max_iterations: 2, body }
        };
        body = vec![Step { step_id: mint(n), kind }];
    }
    let mut p = program("jump-undefined");
    p.main = body;
    let r = analyse_flow(&p, &TypeReport::default());
    assert!(r.diagnostics.is_empty(), "{:?}", r.diagnostics);
    assert_eq!(r.cfgs[&SubId::Main].nodes.len(), 399);

    p.main = (0..10_000u64)
        .map(|n| Step { step_id: mint(n + 1000), kind: StepKind::Comment { text: "c".into() } })
        .collect();
    let t = std::time::Instant::now();
    let r = analyse_flow(&p, &TypeReport::default());
    eprintln!("10 000 steps analysed in {:?} (unoptimised build)", t.elapsed());
    assert!(r.diagnostics.is_empty());
    assert_eq!(r.cfgs[&SubId::Main].successors(mint(10_999)), [(Target::Exit, EdgeKind::Next)]);
}
