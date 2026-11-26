//! Acceptance criteria of spec node AGENT.PROPOSE.IMPACT (AC-1 .. AC-4) and the pinned rules
//! of module `impact`: promotion target, ResultRecord reading, listing, move matrix, entry
//! count and classes, the D-045 interpretation label, refusals and forged rows.
//!
//! Expected results come from the independent Python reference
//! `fixtures/agent/propose/make_impact_fixtures.py` (its own RFC 8785 writer, header hashing,
//! chain check, branch diff and impact). The object-size and parameter cases are
//! hand-labelled from the rules in the module documentation.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_agent::{
    command_defs, compute_impact, compute_impact_command_def, invoke_compute_impact_cli, invoke_compute_impact_mcp,
    ImpactError, ImpactProject, MemoryObjects, ObjectRead, ObjectSource, COMPUTE_IMPACT,
};
use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_journal::branch::{BranchEvent, BranchName, Branches, EntryOrigin, EventSource, CREATE_BRANCH, MAIN};
use fid_journal::diff::DiffOptions;
use fid_journal::verify::StoredEntry;
use fid_journal::PrincipalKind;
use fid_types::{Digest, SideEffect};
use std::collections::BTreeMap;
use std::path::Path;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const BUS_ERRORS_V3: &[u8] = include_bytes!("../../../fixtures/core/bus/pipe/err/bus-errors.v3.json");

type Obj = BTreeMap<String, CValue>;

fn load(rel: &str) -> CValue {
    parse(&std::fs::read(Path::new(ROOT).join(rel)).unwrap()).unwrap()
}

fn obj(v: &CValue) -> &Obj {
    match v {
        CValue::Obj(o) => o,
        other => panic!("object expected, got {other:?}"),
    }
}

fn st<'a>(o: &'a Obj, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("field {k} is {other:?}"),
    }
}

fn arr<'a>(o: &'a Obj, k: &str) -> &'a [CValue] {
    match o.get(k) {
        Some(CValue::Arr(a)) => a,
        other => panic!("field {k} is {other:?}"),
    }
}

fn int(v: &CValue) -> i64 {
    match v {
        CValue::Num(n) if n.fract() == 0.0 => *n as i64,
        CValue::U64(n) => *n as i64,
        CValue::I64(n) => *n,
        other => panic!("integer expected, got {other:?}"),
    }
}

// ------------------------------------------------------------------ the fixture model

#[derive(Clone)]
struct Model {
    stream: String,
    rows: Vec<StoredEntry>,
    branches: Branches,
    objects: MemoryObjects,
}

impl Model {
    fn project(&self) -> ImpactProject<'_> {
        ImpactProject { stream: &self.stream, rows: &self.rows, branches: &self.branches, objects: &self.objects }
    }
}

/// The branch table as `CORE.JRN.BR.NAME`'s origin-bound fold builds it: `main` at genesis,
/// human branches from `journal.create_branch@1`'s handler, agent branches from the bus
/// routing module.
fn table(rows: &[StoredEntry], branches: &[CValue]) -> Branches {
    let first = Digest::parse(&rows[0].hash).unwrap();
    let mut b = Branches::genesis(first);
    for e in branches {
        let e = obj(e);
        let name = st(e, "name");
        if name == MAIN {
            continue;
        }
        let from = Digest::parse(st(e, "created_from")).unwrap();
        let name = BranchName::parse(name).unwrap();
        let agent = name.as_str().starts_with("agent/");
        let origin = EntryOrigin {
            kind: if agent { PrincipalKind::Agent } else { PrincipalKind::Human },
            command: if agent { "gdt.eval_flatness".into() } else { CREATE_BRANCH.into() },
            major: 1,
            class: SideEffect::W1,
            branch: if agent { name.as_str().to_string() } else { MAIN.to_string() },
            source: if agent { EventSource::BusRoute } else { EventSource::Handler },
            human_assertion: None,
        };
        b.apply(&origin, &BranchEvent::Created { name, from }).unwrap();
    }
    b
}

fn model_of(v: &CValue) -> Model {
    let f = obj(v);
    let rows: Vec<StoredEntry> = arr(f, "entries")
        .iter()
        .map(|e| {
            let e = obj(e);
            let header = st(e, "header").as_bytes().to_vec();
            let refs = arr(e, "refs")
                .iter()
                .map(|r| match r {
                    CValue::Arr(p) => match (&p[0], &p[1]) {
                        (CValue::Str(a), CValue::Str(b)) => (a.clone(), b.clone()),
                        _ => panic!(),
                    },
                    _ => panic!(),
                })
                .collect();
            StoredEntry {
                seq: int(e.get("seq").unwrap()),
                hash: st(e, "hash").into(),
                stream: st(e, "stream").into(),
                branch: st(e, "branch").into(),
                kind: st(e, "kind").into(),
                prev_hash: Some(st(e, "prev_hash").into()),
                header_len: header.len() as u64,
                header,
                refs,
            }
        })
        .collect();
    let branches = table(&rows, arr(f, "branches"));
    let mut objects = MemoryObjects::new();
    for (d, text) in obj(f.get("objects").unwrap()) {
        let CValue::Str(text) = text else { panic!() };
        objects.insert(Digest::parse(d).unwrap(), text.as_bytes().to_vec());
    }
    Model { stream: st(f, "stream").to_string(), rows, branches, objects }
}

fn fixture(name: &str) -> (Model, Vec<u8>) {
    let m = model_of(&load(&format!("fixtures/agent/propose/{name}/project.json")));
    let golden = std::fs::read(Path::new(ROOT).join(format!("fixtures/agent/propose/{name}/impact.json"))).unwrap();
    (m, golden)
}

fn cli(m: &Model, branch: &str) -> Result<Vec<u8>, ImpactError> {
    invoke_compute_impact_cli(&m.project(), PrincipalKind::Human, &["--branch", branch, "--json"])
}

fn mcp(m: &Model, branch: &str) -> Result<Vec<u8>, ImpactError> {
    let params = to_canonical_bytes(&CValue::Obj([("branch".to_string(), CValue::Str(branch.into()))].into())).unwrap();
    invoke_compute_impact_mcp(&m.project(), &params)
}

// ------------------------------------------------------------------ acceptance criteria

#[test]
fn ac1_two_moves_lists_c1_c2_with_one_move_and_golden_classes() {
    let (m, golden) = fixture("impact-two-moves");
    let out = cli(&m, "agent/s1").unwrap();
    assert_eq!(out, golden, "CLI result differs from the oracle's golden bytes");
    assert_eq!(mcp(&m, "agent/s1").unwrap(), golden, "MCP and CLI give the same bytes");

    let v = parse(&out).unwrap();
    let r = obj(&v);
    let chars: Vec<&str> = arr(r, "characteristics").iter().map(|c| st(obj(c), "characteristic")).collect();
    assert_eq!(chars, ["C1", "C2"]);
    let mut want = vec![vec![0i64; 4]; 4];
    want[0][1] = 1;
    let moves: Vec<Vec<i64>> = arr(r, "moves")
        .iter()
        .map(|row| match row {
            CValue::Arr(a) => a.iter().map(int).collect(),
            _ => panic!(),
        })
        .collect();
    assert_eq!(moves, want);
    let golden_v = parse(&golden).unwrap();
    assert_eq!(arr(r, "classes"), arr(obj(&golden_v), "classes"));
    assert_eq!(arr(r, "classes"), [CValue::Str("W1".into()), CValue::Str("W2".into())]);
    // C2 changed in value only; C1's new record is provisional (D-045), so the result is too.
    let c2 = obj(&arr(r, "characteristics")[1]);
    assert_eq!(st(c2, "old_outcome"), st(c2, "new_outcome"));
    assert_eq!(c2.get("value_changed"), Some(&CValue::Bool(true)));
    assert_eq!(st(obj(&arr(r, "characteristics")[0]), "new_interpretation"), "provisional");
    assert_eq!(st(r, "interpretation"), "provisional");
}

#[test]
fn ac2_branch_without_commands_has_empty_impact() {
    let (m, golden) = fixture("impact-empty");
    let out = cli(&m, "agent/e1").unwrap();
    assert_eq!(out, golden);
    let res = compute_impact(&m.project(), "agent/e1", &DiffOptions::default()).unwrap();
    assert!(res.characteristics.is_empty());
    assert_eq!(res.moves, [[0u32; 4]; 4]);
    assert_eq!((res.added, res.removed, res.entry_count), (0, 0, 0));
    assert!(res.classes.is_empty());
    assert_eq!(res.interpretation(), None);
}

#[test]
fn ac3_unknown_branch_fails_and_writes_nothing() {
    let (m, _) = fixture("impact-two-moves");
    let before = (m.rows.clone(), m.branches.clone(), m.objects.clone());
    for e in [cli(&m, "agent/nope").unwrap_err(), mcp(&m, "agent/nope").unwrap_err()] {
        assert_eq!(e.code(), "unknown_branch");
        assert_eq!(e.bus_code(), ("not_found", "unknown_branch"));
    }
    // The command holds only shared references; the project is unchanged.
    assert_eq!((m.rows.clone(), m.branches.clone(), m.objects.clone()), before);
}

#[test]
fn ac4_two_runs_are_byte_identical() {
    let (m, _) = fixture("impact-two-moves");
    let a = cli(&m, "agent/s1").unwrap();
    let b = cli(&m, "agent/s1").unwrap();
    assert_eq!(a, b);
    let v = parse(&a).unwrap();
    assert_eq!(to_canonical_bytes(&v).unwrap(), a, "result is RFC 8785 canonical");
}

// ------------------------------------------------------------------ oracle cases

#[test]
fn every_oracle_case_matches() {
    let v = load("fixtures/agent/propose/impact-cases.json");
    let cases = arr(obj(&v), "cases");
    assert!(cases.len() >= 18);
    for c in cases {
        let c = obj(c);
        let name = st(c, "name");
        let m = model_of(c.get("project").unwrap());
        let got = cli(&m, st(c, "branch"));
        let expect = obj(c.get("expect").unwrap());
        match (expect.get("result"), expect.get("error"), got) {
            (Some(want), None, Ok(bytes)) => {
                assert_eq!(bytes, to_canonical_bytes(want).unwrap(), "{name}: result differs from the oracle");
            }
            (None, Some(err), Err(e)) => {
                let err = obj(err);
                assert_eq!(e.code(), st(err, "code"), "{name}: {e}");
                let bus = arr(err, "bus");
                assert_eq!(
                    (CValue::Str(e.bus_code().0.into()), CValue::Str(e.bus_code().1.into())),
                    (bus[0].clone(), bus[1].clone()),
                    "{name}"
                );
                let want_digest = err.get("digest").map(|d| match d {
                    CValue::Str(s) => Digest::parse(s).unwrap(),
                    _ => panic!(),
                });
                assert_eq!(e.digest(), want_digest, "{name}");
            }
            (_, _, got) => panic!("{name}: expected {expect:?}, got {got:?}"),
        }
    }
}

#[test]
fn every_bus_code_is_in_catalogue_v3() {
    let v = parse(BUS_ERRORS_V3).unwrap();
    let codes = obj(obj(&v).get("codes").unwrap());
    let d = Digest::from_bytes([7; 32]);
    let errs = [
        ImpactError::UnknownBranch("x".into()),
        ImpactError::NoTarget("x".into()),
        ImpactError::RecordMissing { digest: d },
        ImpactError::RecordInvalid { digest: d, reason: "r".into() },
        ImpactError::BadParams("p".into()),
        ImpactError::InputLimit("l"),
        ImpactError::Internal("i"),
    ];
    for e in errs {
        let (code, reason) = e.bus_code();
        let reasons = arr(codes, code);
        assert!(reasons.contains(&CValue::Str(reason.into())), "{code}/{reason}");
    }
}

// ------------------------------------------------------------------ hand-labelled cases

/// A store that reports some objects as too large to read.
struct Capped<'a> {
    inner: &'a MemoryObjects,
    large: Vec<Digest>,
}

impl ObjectSource for Capped<'_> {
    fn read_object(&self, digest: &Digest, max_len: usize) -> ObjectRead {
        if self.large.contains(digest) {
            return ObjectRead::TooLarge;
        }
        self.inner.read_object(digest, max_len)
    }
}

fn output_digest(m: &Model, seq: i64, index: usize) -> Digest {
    let row = m.rows.iter().find(|r| r.seq == seq).unwrap();
    let h = parse(&row.header).unwrap();
    let outs = arr(obj(obj(&h).get("command").unwrap()), "outputs");
    Digest::parse(st(obj(&outs[index]), "digest")).unwrap()
}

#[test]
fn an_oversize_object_is_not_a_record() {
    let (m, golden) = fixture("impact-two-moves");
    // F1's new feature object (entry 6) too large to read: still not a characteristic.
    let f1_new = output_digest(&m, 6, 0);
    let capped = Capped { inner: &m.objects, large: vec![f1_new] };
    let p = ImpactProject { objects: &capped, ..m.project() };
    let out = compute_impact(&p, "agent/s1", &DiffOptions::default()).unwrap().to_json().unwrap();
    assert_eq!(out, golden);
    // C2's new record too large: a record replaced by a non-record is refused.
    let c2_new = output_digest(&m, 7, 1);
    let capped = Capped { inner: &m.objects, large: vec![c2_new] };
    let p = ImpactProject { objects: &capped, ..m.project() };
    let e = compute_impact(&p, "agent/s1", &DiffOptions::default()).unwrap_err();
    assert_eq!(e.code(), "record_invalid");
    assert_eq!(e.digest(), Some(c2_new));
}

#[test]
fn the_diff_bounds_apply() {
    let (m, _) = fixture("impact-two-moves");
    let opts = DiffOptions { max_entries: 3, ..DiffOptions::default() };
    let e = compute_impact(&m.project(), "agent/s1", &opts).unwrap_err();
    assert_eq!(e.bus_code(), ("validation", "input_limit"));
}

#[test]
fn bad_cli_and_mcp_parameters_are_refused() {
    let (m, _) = fixture("impact-two-moves");
    let p = m.project();
    let bad: [&[&str]; 6] = [
        &[],
        &["--json"],
        &["--branch"],
        &["--branch", "agent/s1", "--branch", "agent/s1"],
        &["--branch", "agent/s1", "--json", "--json"],
        &["--branch", "agent/s1", "--path", "/etc/passwd"],
    ];
    for args in bad {
        let e = invoke_compute_impact_cli(&p, PrincipalKind::Human, args).unwrap_err();
        assert_eq!(e.code(), "bad_params", "{args:?}");
    }
    assert!(invoke_compute_impact_cli(&p, PrincipalKind::Human, &["--branch", "agent/s1"]).is_ok());
    for params in [
        &b"[]"[..],
        &b"{}"[..],
        &br#"{"branch":1}"#[..],
        &br#"{"branch":"agent/s1","target":"main"}"#[..],
        &br#"{"branch":"agent/s1","branch":"agent/s1"}"#[..],
        &b"{"[..],
    ] {
        let e = invoke_compute_impact_mcp(&p, params).unwrap_err();
        assert_eq!(e.code(), "bad_params", "{}", String::from_utf8_lossy(params));
    }
    let big = vec![b' '; 5000];
    assert_eq!(invoke_compute_impact_mcp(&p, &big).unwrap_err().code(), "input_limit");
}

// ------------------------------------------------------------------ registry definition

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantities")))
    }

    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

#[test]
fn command_def_is_r1_d0_and_schemas_validate_with_bus() {
    let def = compute_impact_command_def().unwrap();
    assert_eq!(format!("{}@{}", def.id, def.major), COMPUTE_IMPACT);
    assert_eq!(def.side_effect.as_str(), "R1");
    assert_eq!(def.determinism.as_str(), "D0");
    assert_eq!(def.fs_effect.as_str(), "none");
    let defs = command_defs().unwrap();
    assert!(defs.iter().any(|d| format!("{}@{}", d.id, d.major) == COMPUTE_IMPACT));

    let params = Compiled::compile(&def.params_schema).expect("params schema compiles");
    let result = Compiled::compile(&def.result_schema).expect("result schema compiles");
    let ok = CValue::Obj([("branch".to_string(), CValue::Str("agent/s1".into()))].into());
    assert!(params.evaluate(&ok, &NoHooks).is_ok());
    let extra = CValue::Obj(
        [("branch".to_string(), CValue::Str("agent/s1".into())), ("target".to_string(), CValue::Str("main".into()))]
            .into(),
    );
    assert!(params.evaluate(&extra, &NoHooks).is_err());
    for name in ["impact-two-moves", "impact-empty"] {
        let (_, golden) = fixture(name);
        assert!(result.evaluate(&parse(&golden).unwrap(), &NoHooks).is_ok(), "{name}");
    }
    let v = load("fixtures/agent/propose/impact-cases.json");
    for c in arr(obj(&v), "cases") {
        if let Some(r) = obj(obj(c).get("expect").unwrap()).get("result") {
            assert!(result.evaluate(r, &NoHooks).is_ok(), "{}", st(obj(c), "name"));
        }
    }
}
