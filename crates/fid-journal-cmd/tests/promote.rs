//! CORE.JRN.BR.PROMO, bus half: the three handlers against the oracle
//! (`fixtures/core/jrn/br/promo/cases.json`) with the caller derived by the bus from the
//! channel, every parameter and result checked with `fid_bus::validate::Compiled`, the
//! projections and class checks of AC-4 and AC-5, and the v3 mapping of every refusal.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::authorise_class;
use fid_bus::error::{BusError, Code, Detail, ValidationReason};
use fid_bus::handler::{
    effects_check, CancelToken, EntityId, EntityRef, Handler, HandlerCtx, HandlerError, HandlerSet, SettingKey,
    StateView, ValidParams,
};
use fid_bus::policy::projection::{authorise_projection, Projected};
use fid_bus::principal::{Channel, ChannelKind, Identity, PrincipalId, SessionId, SessionKey, SessionTable};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse, CValue};
use fid_journal::branch::{
    BranchEvent, BranchName, Branches, EntryOrigin, EventSource, ProtectCause, CREATE_BRANCH, MAIN, PROTECT_BRANCH,
};
use fid_journal::diff::{diff_rows, BranchDiff, DiffError, DiffOptions};
use fid_journal::promote::{
    command_defs, AssertionCheck, PromoteError, PromoteOp, RebaseStep, Rebaser, StepOutcome, PROMOTE_BRANCH,
    PROMOTE_PROTECTED_BRANCH, REQUEST_PROMOTE_BRANCH,
};
use fid_journal::verify::StoredEntry;
use fid_journal::PrincipalKind;
use fid_journal_cmd::{
    promote_bus_error, register_promote, run_promote, BranchSource, DiffSource, PromoteHandler, PromoteWiring,
};
use fid_registry::CommandDef;
use fid_types::{Digest, SideEffect};
use std::collections::BTreeMap;
use std::sync::Arc;

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

fn digest(s: &str) -> Digest {
    Digest::parse(s).unwrap()
}

// ------------------------------------------------------------------ the journal

struct Journal {
    rows: Vec<StoredEntry>,
    table: Branches,
}

impl BranchSource for Journal {
    fn branches(&self) -> Result<Branches, String> {
        Ok(self.table.clone())
    }
    fn is_head(&self, head: &Digest) -> bool {
        self.rows.iter().any(|r| r.hash == head.as_str())
    }
}

impl DiffSource for Journal {
    fn diff(&self, b: &Branches, s: &str, t: &str, opts: &DiffOptions) -> Result<BranchDiff, DiffError> {
        diff_rows(STREAM, self.rows.iter().cloned().map(Ok), b, s, t, opts)
    }
}

fn journals() -> BTreeMap<String, Arc<Journal>> {
    let v = load("fixtures/core/jrn/br/promo/journals.json");
    let f = obj(&v);
    let mut table: Option<Branches> = None;
    let mut out = BTreeMap::new();
    for (name, rows) in obj(get(f, "journals")) {
        let CValue::Arr(rows) = rows else { panic!() };
        let rows: Vec<StoredEntry> = rows
            .iter()
            .map(|r| {
                let r = obj(r);
                let header = st(r, "header").as_bytes().to_vec();
                let CValue::Num(seq) = get(r, "seq") else { panic!() };
                StoredEntry {
                    seq: *seq as i64,
                    hash: st(r, "hash").into(),
                    stream: st(r, "stream").into(),
                    branch: st(r, "branch").into(),
                    kind: st(r, "kind").into(),
                    prev_hash: Some(st(r, "prev_hash").into()),
                    header_len: header.len() as u64,
                    header,
                    refs: Vec::new(),
                }
            })
            .collect();
        let t = table.get_or_insert_with(|| build_table(f, digest(&rows[0].hash))).clone();
        out.insert(name.clone(), Arc::new(Journal { rows, table: t }));
    }
    out
}

fn build_table(f: &Obj, first: Digest) -> Branches {
    let mut table = Branches::genesis(first);
    let human = |command: &str, class: SideEffect| EntryOrigin {
        kind: PrincipalKind::Human,
        command: command.into(),
        major: 1,
        class,
        branch: MAIN.into(),
        source: EventSource::Handler,
        human_assertion: None,
    };
    for b in arr(f, "branches") {
        let b = obj(b);
        let name = BranchName::parse(st(b, "name")).unwrap();
        if name.as_str() != MAIN {
            let from = digest(st(b, "created_from"));
            let origin = if name.as_str().starts_with("agent/") {
                EntryOrigin {
                    kind: PrincipalKind::Agent,
                    command: "feature.set_nominal".into(),
                    major: 1,
                    class: SideEffect::W2,
                    branch: name.as_str().into(),
                    source: EventSource::BusRoute,
                    human_assertion: None,
                }
            } else {
                human(CREATE_BRANCH, SideEffect::W2)
            };
            table.apply(&origin, &BranchEvent::Created { name: name.clone(), from }).unwrap();
        }
        if matches!(get(b, "protected"), CValue::Bool(true)) {
            table
                .apply(
                    &human(PROTECT_BRANCH, SideEffect::W3),
                    &BranchEvent::Protected { name, cause: ProtectCause::Manual },
                )
                .unwrap();
        }
    }
    table
}

struct FakeRebaser(Vec<RebaseStep>);

impl Rebaser for FakeRebaser {
    fn rebase(&self, _: &Digest, _: Option<&Digest>, _: &dyn Fn() -> bool) -> Result<Vec<RebaseStep>, String> {
        Ok(self.0.clone())
    }
}

struct FakeCheck(Digest, String);

impl AssertionCheck for FakeCheck {
    fn confirm(&self, pre_image: &Digest, assertion: &str) -> bool {
        *pre_image == self.0 && assertion == self.1
    }
}

fn wiring_of(c: &Obj) -> PromoteWiring {
    let rebaser: Option<Arc<dyn Rebaser>> = match get(c, "rebaser") {
        CValue::Arr(steps) => Some(Arc::new(FakeRebaser(
            steps
                .iter()
                .map(|s| {
                    let s = obj(s);
                    RebaseStep {
                        command: st(s, "command").into(),
                        digest: digest(st(s, "digest")),
                        outcome: StepOutcome::parse(st(s, "outcome")).unwrap(),
                    }
                })
                .collect(),
        ))),
        _ => None,
    };
    let assertion: Option<Arc<dyn AssertionCheck>> = match get(c, "assertion_check") {
        CValue::Obj(o) => {
            let CValue::Arr(pair) = get(o, "confirms") else { panic!() };
            let (CValue::Str(p), CValue::Str(a)) = (&pair[0], &pair[1]) else { panic!() };
            Some(Arc::new(FakeCheck(digest(p), a.clone())))
        }
        _ => None,
    };
    PromoteWiring { rebaser, assertion }
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

/// GUI is a human, MCP an agent, CLI a script (CORE.BUS.PRIN).
fn ctx(kind: ChannelKind) -> HandlerCtx {
    let mut table = SessionTable::new();
    let session = table.open_session(SessionId::new("s-1").unwrap()).unwrap();
    HandlerCtx::derive(&table, &TestChannel { kind, session }, 0, CancelToken::never()).unwrap()
}

fn channel_of(actor: &str) -> ChannelKind {
    match actor {
        "human" => ChannelKind::Gui,
        "agent" => ChannelKind::Mcp,
        "script" => ChannelKind::Cli,
        other => panic!("{other}"),
    }
}

fn op_of(command: &str) -> PromoteOp {
    match command {
        PROMOTE_BRANCH => PromoteOp::Promote,
        PROMOTE_PROTECTED_BRANCH => PromoteOp::Protected,
        REQUEST_PROMOTE_BRANCH => PromoteOp::Request,
        other => panic!("{other}"),
    }
}

fn defs() -> BTreeMap<String, CommandDef> {
    command_defs().unwrap().into_iter().map(|d| (d.id.clone(), d)).collect()
}

fn assert_valid(schema: &Compiled, v: &CValue, what: &str) {
    if let Err(f) = schema.evaluate(v, &NoHooks) {
        panic!("{what}: {} at {} ({})", f.keyword, f.pointer, f.message);
    }
}

// ------------------------------------------------------------------ tests

/// Every oracle call through the registered handler with the bus-derived principal: the
/// oracle's event or v3 error, every event declared in the definition's effects, every
/// parameter set the oracle accepts valid against the params schema, every result valid
/// against the result schema.
#[test]
fn handlers_follow_the_oracle_and_validate() {
    let c = load("fixtures/core/jrn/br/promo/cases.json");
    let js = journals();
    let defs = defs();
    let mut n = 0;
    for case in arr(obj(&c), "plans") {
        let case = obj(case);
        let name = st(case, "name");
        let command = st(case, "command");
        let def = &defs[command];
        let op = op_of(command);
        let params = get(case, "params").clone();
        let j = &js[st(case, "journal")];
        let wiring = wiring_of(case);
        let cx = ctx(channel_of(st(case, "actor")));
        let expect = obj(get(case, "expect"));
        let branches: Arc<dyn BranchSource> = j.clone();
        let diffs: Arc<dyn DiffSource> = j.clone();
        let handler = PromoteHandler::new(op, branches, diffs, wiring.clone());
        let ran = handler.run(&NoView, &ValidParams::for_tests(params.clone()), &cx);
        let params_ok = Compiled::compile(&def.params_schema).unwrap().evaluate(&params, &NoHooks).is_ok();
        match (run_promote(op, j.as_ref(), j.as_ref(), &wiring, &params, &cx), expect.get("event"), expect.get("error"))
        {
            (Ok((events, result)), Some(want), None) => {
                let want = obj(want);
                assert!(params_ok, "{name}: params refused by the schema");
                assert_eq!(events.len(), 1, "{name}");
                assert_eq!(events[0].type_id().as_str(), st(want, "type"), "{name}");
                assert_eq!(events[0].body(), get(want, "body"), "{name}");
                effects_check(def, &events).unwrap();
                assert_valid(&Compiled::compile(&def.result_schema).unwrap(), &result, name);
                assert_eq!(ran.unwrap(), events, "{name}: the handler returns the same event");
            }
            (Err(e), None, Some(want)) => {
                let want = obj(want);
                assert_eq!((e.code().as_str(), e.reason()), (st(want, "code"), st(want, "reason")), "{name}: {e}");
                assert_eq!(ran.unwrap_err().into_bus_error(), e, "{name}");
            }
            (got, _, _) => panic!("{name}: got {got:?}, expected {expect:?}"),
        }
        n += 1;
    }
    assert!(n >= 30);
}

#[test]
fn params_schemas_check_real_values() {
    let defs = defs();
    let d = format!("sha256:{}", "a".repeat(64));
    let base = |extra: &[(&str, CValue)]| {
        let mut o: Obj = [
            ("source", CValue::Str("trial".into())),
            ("source_head", CValue::Str(d.clone())),
            ("target", CValue::Str("main".into())),
            ("target_head", CValue::Null),
            ("diff", CValue::Str(d.clone())),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        for (k, v) in extra {
            o.insert(k.to_string(), v.clone());
        }
        CValue::Obj(o)
    };
    let promote = Compiled::compile(&defs[PROMOTE_BRANCH].params_schema).unwrap();
    let protected = Compiled::compile(&defs[PROMOTE_PROTECTED_BRANCH].params_schema).unwrap();
    let request = Compiled::compile(&defs[REQUEST_PROMOTE_BRANCH].params_schema).unwrap();
    assert_valid(&promote, &base(&[]), "promote");
    assert_valid(&protected, &base(&[("assertion", CValue::Str("A-1".into()))]), "protected");
    assert_valid(&request, &base(&[("note", CValue::Str("é please".into()))]), "request");
    assert_valid(&request, &base(&[]), "request without note");
    // No principal in the parameters, no assertion outside the W3 command, no missing binding.
    assert!(promote.evaluate(&base(&[("principal", CValue::Str("human".into()))]), &NoHooks).is_err());
    assert!(promote.evaluate(&base(&[("assertion", CValue::Str("A-1".into()))]), &NoHooks).is_err());
    assert!(protected.evaluate(&base(&[]), &NoHooks).is_err());
    assert!(request.evaluate(&base(&[("note", CValue::Str("x".repeat(2001)))]), &NoHooks).is_err());
    let CValue::Obj(mut no_diff) = base(&[]) else { panic!() };
    no_diff.remove("diff");
    assert!(promote.evaluate(&CValue::Obj(no_diff), &NoHooks).is_err());
    assert!(promote.evaluate(&base(&[("source_head", CValue::Str("sha256:XY".into()))]), &NoHooks).is_err());
}

/// AC-4 and AC-5 at the bus: an agent's promote over MCP becomes a pending request (and so
/// moves no head), the W3 command is not on MCP, the request runs; the class checks refuse
/// a script and an agent; a human passes.
#[test]
fn ac4_ac5_projection_and_class() {
    let defs = defs();
    let promote = &defs[PROMOTE_BRANCH];
    let protected = &defs[PROMOTE_PROTECTED_BRANCH];
    let request = &defs[REQUEST_PROMOTE_BRANCH];
    assert_eq!(authorise_projection(ChannelKind::Mcp, promote).unwrap(), Projected::PendingRequest);
    let e = authorise_projection(ChannelKind::Mcp, protected).unwrap_err();
    assert_eq!((e.code(), e.reason()), (Code::Policy, "not_on_mcp"));
    assert_eq!(authorise_projection(ChannelKind::Mcp, request).unwrap(), Projected::Run);
    let script = ctx(ChannelKind::Cli);
    let e = authorise_class(script.principal(), promote).unwrap_err();
    assert_eq!((e.code(), e.reason()), (Code::Policy, "human_only"));
    let e = authorise_class(script.principal(), protected).unwrap_err();
    assert_eq!((e.code(), e.reason()), (Code::Policy, "class_forbidden"));
    authorise_class(ctx(ChannelKind::Mcp).principal(), request).unwrap();
    let human = ctx(ChannelKind::Gui);
    authorise_class(human.principal(), promote).unwrap();
    authorise_class(human.principal(), protected).unwrap();
}

#[test]
fn register_promote_adds_the_three_commands() {
    let js = journals();
    let j = &js["ff"];
    let branches: Arc<dyn BranchSource> = j.clone();
    let diffs: Arc<dyn DiffSource> = j.clone();
    let mut set = HandlerSet::new();
    register_promote(&mut set, &branches, &diffs, &PromoteWiring::default()).unwrap();
    for id in [PROMOTE_BRANCH, PROMOTE_PROTECTED_BRANCH, REQUEST_PROMOTE_BRANCH] {
        assert_eq!(set.def(&format!("{id}@1")).unwrap().id, id);
    }
}

#[test]
fn a_cancelled_call_is_cancelled() {
    let js = journals();
    let j = &js["fork"];
    let branches: Arc<dyn BranchSource> = j.clone();
    let diffs: Arc<dyn DiffSource> = j.clone();
    let src = fid_bus::handler::CancelSource::new();
    src.cancel();
    let mut table = SessionTable::new();
    let session = table.open_session(SessionId::new("s-1").unwrap()).unwrap();
    let cx = HandlerCtx::derive(&table, &TestChannel { kind: ChannelKind::Gui, session }, 0, src.token()).unwrap();
    let h = PromoteHandler::new(PromoteOp::Promote, branches, diffs, PromoteWiring::default());
    let r = h.run(&NoView, &ValidParams::for_tests(CValue::Obj(BTreeMap::new())), &cx);
    assert!(matches!(r, Err(HandlerError::Cancelled)));
}

#[test]
fn refusals_map_to_v3() {
    let v3 = fid_bus::parse_catalog(&std::fs::read(BUS_V3).unwrap()).unwrap();
    let all = [
        PromoteError::HumanOnly,
        PromoteError::ClassForbidden,
        PromoteError::Schema("x".into()),
        PromoteError::UnknownBranch("x".into()),
        PromoteError::NotPromotable("x".into()),
        PromoteError::TargetProtected("x".into()),
        PromoteError::HeadMismatch("x".into()),
        PromoteError::DigestMismatch,
        PromoteError::CapabilityMissing("Rebaser"),
        PromoteError::NoAssertion,
        PromoteError::DivergentStep { step: 2, outcome: StepOutcome::Divergent },
        PromoteError::InputLimit("x".into()),
        PromoteError::Rebase("x".into()),
        PromoteError::Cancelled,
        PromoteError::Diff(DiffError::InputLimit("x".into())),
    ];
    for e in all {
        let b = promote_bus_error(&e);
        assert_eq!((b.code().as_str(), b.reason()), e.bus_code(), "{e}");
        assert!(v3.get(b.code().as_str()).is_some_and(|r| r.contains(b.reason())), "{e} not in v3");
        assert_eq!(HandlerError::Refused(b.clone()).into_bus_error(), b);
    }
}
