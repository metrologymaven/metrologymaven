//! CORE.JRN.BR.DIFF, bus half: `journal.diff_branch@1` against the oracle
//! (`fixtures/core/jrn/br/diff/cases.json`, written by make_fixtures.py from the spec rules)
//! through the registered handler, every parameter and result checked with
//! `fid_bus::validate::Compiled`, the projection of the R0 command, cancellation, and the v3
//! mapping of every refusal.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Code, Detail, ValidationReason};
use fid_bus::handler::{
    CancelSource, CancelToken, EntityId, EntityRef, Handler, HandlerCtx, HandlerError, HandlerSet, SettingKey,
    StateView, ValidParams,
};
use fid_bus::policy::projection::{authorise_projection, Projected};
use fid_bus::principal::{Channel, ChannelKind, Identity, PrincipalId, SessionId, SessionKey, SessionTable};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse, CValue};
use fid_journal::branch::{BranchEvent, BranchName, Branches, EntryOrigin, EventSource, CREATE_BRANCH, MAIN};
use fid_journal::diff::{command_defs, diff_rows, BranchDiff, DiffError, DiffOptions, DIFF_BRANCH};
use fid_journal::verify::{FailureKind, ReadError, StoredEntry};
use fid_journal::PrincipalKind;
use fid_journal_cmd::{diff_bus_error, register_diff, run_diff, BranchSource, DiffHandler, DiffSource};
use fid_registry::CommandDef;
use fid_types::{Digest, SideEffect};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const STREAM: &str = "record";
const BUS_V3: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/bus/pipe/err/bus-errors.v3.json");

type Obj = BTreeMap<String, CValue>;

fn load(rel: &str) -> CValue {
    parse(&std::fs::read(format!("{ROOT}/{rel}")).unwrap()).unwrap()
}

fn obj(v: &CValue) -> &Obj {
    match v {
        CValue::Obj(o) => o,
        other => panic!("object expected, got {other:?}"),
    }
}

fn get<'a>(o: &'a Obj, k: &str) -> &'a CValue {
    o.get(k).unwrap_or_else(|| panic!("missing {k}"))
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

fn params(source: &str, target: &str) -> CValue {
    CValue::Obj(BTreeMap::from([
        ("source".to_string(), CValue::Str(source.into())),
        ("target".to_string(), CValue::Str(target.into())),
    ]))
}

// ------------------------------------------------------------------ the journal

/// The `fork` journal's rows and its origin-bound branch table.
struct Journal {
    rows: Vec<StoredEntry>,
    table: Mutex<Branches>,
}

fn journal() -> Arc<Journal> {
    let v = load("fixtures/core/jrn/br/diff/fork.json");
    let f = obj(&v);
    let rows: Vec<StoredEntry> = arr(f, "entries")
        .iter()
        .map(|e| {
            let e = obj(e);
            let header = st(e, "header").as_bytes().to_vec();
            let CValue::Num(seq) = get(e, "seq") else { panic!() };
            StoredEntry {
                seq: *seq as i64,
                hash: st(e, "hash").into(),
                stream: st(e, "stream").into(),
                branch: st(e, "branch").into(),
                kind: st(e, "kind").into(),
                prev_hash: Some(st(e, "prev_hash").into()),
                header_len: header.len() as u64,
                header,
                refs: Vec::new(),
            }
        })
        .collect();
    let mut t = Branches::genesis(Digest::parse(&rows[0].hash).unwrap());
    for b in arr(f, "branches") {
        let b = obj(b);
        let name = st(b, "name");
        if name == MAIN {
            continue;
        }
        let agent = name.starts_with("agent/");
        let origin = EntryOrigin {
            kind: if agent { PrincipalKind::Agent } else { PrincipalKind::Human },
            command: if agent { "feature.set_nominal".into() } else { CREATE_BRANCH.into() },
            major: 1,
            class: SideEffect::W2,
            branch: if agent { name.to_string() } else { MAIN.to_string() },
            source: if agent { EventSource::BusRoute } else { EventSource::Handler },
        };
        let event = BranchEvent::Created {
            name: BranchName::parse(name).unwrap(),
            from: Digest::parse(st(b, "created_from")).unwrap(),
        };
        t.apply(&origin, &event).unwrap();
    }
    Arc::new(Journal { rows, table: Mutex::new(t) })
}

impl BranchSource for Journal {
    fn branches(&self) -> Result<Branches, String> {
        Ok(self.table.lock().unwrap().clone())
    }
    fn is_head(&self, head: &Digest) -> bool {
        self.rows.iter().any(|r| r.hash == head.as_str())
    }
}

impl DiffSource for Journal {
    fn diff(
        &self,
        branches: &Branches,
        source: &str,
        target: &str,
        opts: &DiffOptions,
    ) -> Result<BranchDiff, DiffError> {
        diff_rows(STREAM, self.rows.iter().cloned().map(Ok), branches, source, target, opts)
    }
}

// ------------------------------------------------------------------ bus scaffolding

struct NoView;

impl StateView for NoView {
    fn entity(&self, _: &EntityId) -> Option<EntityRef<'_>> {
        None
    }
    fn head(&self) -> fid_canon::Digest {
        fid_canon::Digest([0; 32])
    }
    fn setting(&self, _: &SettingKey) -> Option<CValue> {
        None
    }
}

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantities here")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

struct TestChannel {
    kind: ChannelKind,
    session: SessionKey,
}

impl Channel for TestChannel {
    fn kind(&self) -> ChannelKind {
        self.kind
    }
    fn authenticated_identity(&self) -> Option<Identity> {
        Some(Identity::new(PrincipalId::new("alice").unwrap()))
    }
    fn session(&self) -> &SessionKey {
        &self.session
    }
}

fn ctx_with(kind: ChannelKind, cancel: CancelToken) -> HandlerCtx {
    let mut table = SessionTable::new();
    let session = table.open_session(SessionId::new("s-1").unwrap()).unwrap();
    HandlerCtx::derive(&table, &TestChannel { kind, session }, 0, cancel).unwrap()
}

fn ctx(kind: ChannelKind) -> HandlerCtx {
    ctx_with(kind, CancelToken::never())
}

fn def() -> CommandDef {
    let mut defs = command_defs().unwrap();
    assert_eq!(defs.len(), 1);
    defs.remove(0)
}

fn assert_valid(schema: &Compiled, v: &CValue, what: &str) {
    if let Err(f) = schema.evaluate(v, &NoHooks) {
        panic!("{what}: {} at {} ({})", f.keyword, f.pointer, f.message);
    }
}

// ------------------------------------------------------------------ tests

/// Every oracle diff through the handler: no events, the oracle's result or v3 error, the same
/// result whichever channel (and so whichever principal) asks, every result valid against the
/// result schema and every parameter set against the params schema.
#[test]
fn handler_follows_the_oracle_and_results_validate() {
    let c = load("fixtures/core/jrn/br/diff/cases.json");
    let j = journal();
    let def = def();
    let result_schema = Compiled::compile(&def.result_schema).unwrap();
    let params_schema = Compiled::compile(&def.params_schema).unwrap();
    let branches: Arc<dyn BranchSource> = j.clone();
    let diffs: Arc<dyn DiffSource> = j.clone();
    let handler = DiffHandler::new(Arc::clone(&branches), Arc::clone(&diffs));
    let mut n = 0;
    for d in arr(obj(&c), "diffs") {
        let d = obj(d);
        let name = st(d, "name");
        let p = params(st(d, "source"), st(d, "target"));
        let expect = obj(get(d, "expect"));
        let schema_ok = params_schema.evaluate(&p, &NoHooks).is_ok();
        for kind in [ChannelKind::Gui, ChannelKind::Mcp, ChannelKind::Cli] {
            let cx = ctx(kind);
            let ran = handler.run(&NoView, &ValidParams::for_tests(p.clone()), &cx);
            let got = run_diff(j.as_ref(), j.as_ref(), &p, &cx);
            match (got, expect.get("result"), expect.get("error")) {
                (Ok(v), Some(want), None) => {
                    assert!(schema_ok, "{name}: params refused by the schema");
                    assert_eq!(&v, want, "{name} via {kind:?}");
                    assert_valid(&result_schema, &v, name);
                    assert!(ran.unwrap().is_empty(), "{name}: R0 returns no events");
                }
                (Err(e), None, Some(want)) => {
                    let want = obj(want);
                    assert_eq!((e.code().as_str(), e.reason()), (st(want, "code"), st(want, "reason")), "{name}");
                    let back = ran.unwrap_err().into_bus_error();
                    assert_eq!(back, e, "{name}: the handler returns the same refusal");
                }
                (got, _, _) => panic!("{name}: got {got:?}, expected {expect:?}"),
            }
        }
        n += 1;
    }
    assert!(n >= 11);
}

/// Tampered journals: the handler refuses with the oracle's v3 error.
#[test]
fn a_dangling_created_from_is_refused_through_the_handler() {
    let j = journal();
    {
        let mut t = j.table.lock().unwrap();
        let origin = EntryOrigin {
            kind: PrincipalKind::Human,
            command: CREATE_BRANCH.into(),
            major: 1,
            class: SideEffect::W2,
            branch: MAIN.into(),
            source: EventSource::Handler,
        };
        let nowhere = Digest::parse(&format!("sha256:{}", "ab".repeat(32))).unwrap();
        t.apply(&origin, &BranchEvent::Created { name: BranchName::parse("ghost").unwrap(), from: nowhere }).unwrap();
    }
    let e = run_diff(j.as_ref(), j.as_ref(), &params("ghost", "main"), &ctx(ChannelKind::Gui)).unwrap_err();
    assert_eq!((e.code(), e.reason()), (Code::Internal, "invariant_violated"));
}

#[test]
fn params_schema_checks_real_values() {
    let def = def();
    let p = Compiled::compile(&def.params_schema).unwrap();
    assert_valid(&p, &params("s", "main"), "s main");
    assert_valid(&p, &params("agent/claude/1", "main"), "agent");
    for bad in [
        CValue::Obj(BTreeMap::from([("source".to_string(), CValue::Str("s".into()))])),
        CValue::Obj(BTreeMap::from([
            ("source".to_string(), CValue::Str("s".into())),
            ("target".to_string(), CValue::Str("main".into())),
            ("principal".to_string(), CValue::Str("human".into())),
        ])),
        params("Bad Name", "main"),
        params("s", &"a".repeat(101)),
        CValue::Obj(BTreeMap::from([
            ("source".to_string(), CValue::Num(1.0)),
            ("target".to_string(), CValue::Str("main".into())),
        ])),
    ] {
        assert!(p.evaluate(&bad, &NoHooks).is_err(), "{bad:?} should be refused");
    }
}

/// R0 and exposed: an agent over MCP runs it directly, and so does a script; nothing in the
/// result depends on who asks.
#[test]
fn the_r0_command_runs_on_every_channel() {
    let def = def();
    assert_eq!(def.id, DIFF_BRANCH);
    assert_eq!(def.side_effect, SideEffect::R0);
    assert!(def.effects.is_empty());
    assert_eq!(authorise_projection(ChannelKind::Mcp, &def).unwrap(), Projected::Run);
    for kind in [ChannelKind::Gui, ChannelKind::Mcp, ChannelKind::Cli] {
        fid_bus::authorise_class(ctx(kind).principal(), &def).unwrap();
    }
}

#[test]
fn a_cancelled_call_is_cancelled() {
    let j = journal();
    let src = CancelSource::new();
    src.cancel();
    let cx = ctx_with(ChannelKind::Gui, src.token());
    let e = run_diff(j.as_ref(), j.as_ref(), &params("s", "main"), &cx).unwrap_err();
    assert_eq!((e.code(), e.reason()), (Code::Cancelled, "requested"));
    let branches: Arc<dyn BranchSource> = j.clone();
    let diffs: Arc<dyn DiffSource> = j.clone();
    let h = DiffHandler::new(branches, diffs);
    let r = h.run(&NoView, &ValidParams::for_tests(params("s", "main")), &cx);
    assert!(matches!(r, Err(HandlerError::Cancelled)));
}

#[test]
fn register_diff_adds_the_command() {
    let j = journal();
    let branches: Arc<dyn BranchSource> = j.clone();
    let diffs: Arc<dyn DiffSource> = j.clone();
    let mut set = HandlerSet::new();
    register_diff(&mut set, &branches, &diffs).unwrap();
    assert_eq!(set.def(&format!("{DIFF_BRANCH}@1")).unwrap().id, DIFF_BRANCH);
}

/// Every refusal maps to a code and reason of the frozen v3 catalogue, unchanged, and a
/// handler may return it.
#[test]
fn refusals_map_to_v3() {
    let v3 = fid_bus::parse_catalog(&std::fs::read(BUS_V3).unwrap()).unwrap();
    let fail = fid_journal::Failure {
        stream: STREAM.into(),
        seq: Some(1),
        kind: FailureKind::Link,
        reason: "broken",
        checkpoint: None,
        detail: String::new(),
    };
    let all = [
        DiffError::UnknownBranch("x".into()),
        DiffError::Corrupt(fail.clone()),
        DiffError::Tampered("x".into()),
        DiffError::InputLimit("x".into()),
        DiffError::Cancelled,
        DiffError::Read(ReadError::UnknownStream("x".into())),
        DiffError::Read(ReadError::UnknownBranch("x".into())),
        DiffError::Read(ReadError::BadRange("x".into())),
        DiffError::Read(ReadError::BadParam("x".into())),
        DiffError::Read(ReadError::BadToken),
        DiffError::Read(ReadError::StaleToken),
        DiffError::Read(ReadError::InputLimit("x".into())),
        DiffError::Read(ReadError::Cancelled),
        DiffError::Read(ReadError::Storage("x".into())),
        DiffError::Read(ReadError::Corrupt(fail)),
    ];
    for e in all {
        let b = diff_bus_error(&e);
        assert_eq!((b.code().as_str(), b.reason()), e.bus_code(), "{e}");
        assert!(v3.get(b.code().as_str()).is_some_and(|r| r.contains(b.reason())), "{e} not in v3");
        let back = HandlerError::Refused(b.clone()).into_bus_error();
        assert_eq!(back, b);
    }
}
