//! CORE.JRN.UNDO.SUPER, bus half: `journal.supersede_record@1` against the oracle
//! (`fixtures/core/jrn/undo/super/cases.json`) with the caller derived by the bus from the
//! channel, every parameter and result checked with `fid_bus::validate::Compiled`, the
//! projection and class checks of AC-2, and the v3 mapping of every refusal.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::authorise_class;
use fid_bus::error::{BusError, Code, Detail, ValidationReason};
use fid_bus::handler::{
    effects_check, CancelSource, CancelToken, EntityId, EntityRef, Handler, HandlerCtx, HandlerError, HandlerSet,
    SettingKey, StateView, ValidParams,
};
use fid_bus::policy::projection::{authorise_projection, Projected};
use fid_bus::principal::{Channel, ChannelKind, Identity, PrincipalId, SessionId, SessionKey, SessionTable};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse, CValue};
use fid_journal::branch::{EntryOrigin, EventSource};
use fid_journal::supersede::{
    command_defs, SupersedeEntry, SupersedeError, Supersessions, RECORD_SUPERSEDED, SUPERSEDE_RECORD,
};
use fid_journal::PrincipalKind;
use fid_journal_cmd::{register_supersede, run_supersede, supersede_bus_error, SupersedeHandler, SupersedeSource};
use fid_registry::CommandDef;
use fid_types::{Digest, SideEffect};
use std::collections::BTreeMap;
use std::sync::Arc;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/jrn/undo/super");
const BUS_V3: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/bus/pipe/err/bus-errors.v3.json");

type Obj = BTreeMap<String, CValue>;

fn load(name: &str) -> CValue {
    parse(&std::fs::read(format!("{ROOT}/{name}")).unwrap()).unwrap()
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

fn opt_digest(o: &Obj, k: &str) -> Option<Digest> {
    match get(o, k) {
        CValue::Null => None,
        CValue::Str(s) => Some(digest(s)),
        other => panic!("{k} is {other:?}"),
    }
}

/// Folds one oracle log (only valid logs are used here).
fn fold(list: &[CValue]) -> Supersessions {
    let mut s = Supersessions::new();
    for v in list {
        let o = obj(v);
        let CValue::Num(major) = get(o, "major") else { panic!() };
        let e = SupersedeEntry {
            origin: EntryOrigin {
                kind: PrincipalKind::parse(st(o, "kind")).unwrap(),
                command: st(o, "command").to_string(),
                major: *major as u32,
                class: SideEffect::parse(st(o, "class")).unwrap(),
                branch: st(o, "branch").to_string(),
                source: EventSource::Handler,
                human_assertion: None,
            },
            hash: digest(st(o, "hash")),
            promoted_from: opt_digest(o, "promoted_from"),
            reason_commit: opt_digest(o, "reason_commit"),
            outputs: arr(o, "outputs")
                .iter()
                .map(|d| if let CValue::Str(s) = d { digest(s) } else { panic!() })
                .collect(),
            head_before: opt_digest(o, "head_before"),
        };
        let events: Vec<(String, CValue)> =
            arr(o, "events").iter().map(|x| (st(obj(x), "type").to_string(), get(obj(x), "body").clone())).collect();
        s.apply_entry(&e, &events).unwrap();
    }
    s
}

/// A journal as the bus wiring would give it: the folded state, the call's branch and its
/// head.
struct Journal {
    state: Supersessions,
    branch: String,
    head: Option<Digest>,
}

impl SupersedeSource for Journal {
    fn state(&self) -> Result<Supersessions, String> {
        Ok(self.state.clone())
    }
    fn call_branch(&self, _ctx: &HandlerCtx) -> Result<String, String> {
        Ok(self.branch.clone())
    }
    fn head(&self, branch: &str) -> Result<Option<Digest>, String> {
        assert_eq!(branch, self.branch);
        Ok(self.head)
    }
}

fn journal(log: &str, branch: &str, head: Option<Digest>) -> Arc<Journal> {
    let v = load("two_releases.json");
    let CValue::Arr(list) = get(obj(get(obj(&v), "logs")), log) else { panic!() };
    Arc::new(Journal { state: fold(list), branch: branch.to_string(), head })
}

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
fn ctx_with(kind: ChannelKind, cancel: CancelToken) -> HandlerCtx {
    let mut table = SessionTable::new();
    let session = table.open_session(SessionId::new("s-1").unwrap()).unwrap();
    HandlerCtx::derive(&table, &TestChannel { kind, session }, 0, cancel).unwrap()
}

fn ctx(kind: ChannelKind) -> HandlerCtx {
    ctx_with(kind, CancelToken::never())
}

fn channel(actor: &str) -> ChannelKind {
    match actor {
        "human" => ChannelKind::Gui,
        "agent" => ChannelKind::Mcp,
        "script" => ChannelKind::Cli,
        other => panic!("{other}"),
    }
}

fn def() -> CommandDef {
    command_defs().unwrap().remove(0)
}

fn assert_valid(schema: &Compiled, v: &CValue, what: &str) {
    if let Err(f) = schema.evaluate(v, &NoHooks) {
        panic!("{what}: {} at {} ({})", f.keyword, f.pointer, f.message);
    }
}

/// Every oracle call through the handler with the bus-derived principal: the oracle's event
/// or v3 error, the event declared in the definition's effects, accepted parameters valid
/// against the params schema, the result valid against the result schema.
#[test]
fn the_handler_follows_the_oracle_and_validates() {
    let c = load("cases.json");
    let def = def();
    let params_schema = Compiled::compile(&def.params_schema).unwrap();
    let result_schema = Compiled::compile(&def.result_schema).unwrap();
    let mut n = 0;
    for case in arr(obj(&c), "plans") {
        let case = obj(case);
        let name = st(case, "name");
        let j = journal(st(case, "log"), st(case, "branch"), opt_digest(case, "head"));
        let cx = ctx(channel(st(case, "actor")));
        let params = get(case, "params").clone();
        let handler = SupersedeHandler::new(j.clone());
        let ran = handler.run(&NoView, &ValidParams::for_tests(params.clone()), &cx);
        let expect = obj(get(case, "expect"));
        match (run_supersede(j.as_ref(), &params, &cx), expect.get("event"), expect.get("error")) {
            (Ok((events, result)), Some(want), None) => {
                let want = obj(want);
                assert_valid(&params_schema, &params, name);
                assert_eq!(events.len(), 1, "{name}");
                assert_eq!(events[0].type_id().as_str(), st(want, "type"), "{name}");
                assert_eq!(events[0].body(), get(want, "body"), "{name}");
                effects_check(&def, &events).unwrap();
                assert_valid(&result_schema, &result, name);
                assert_eq!(get(obj(&result), "supersession"), get(want, "body"), "{name}");
                assert_eq!(ran.unwrap(), events, "{name}: the handler returns the same event");
            }
            (Err(e), None, Some(CValue::Arr(want))) => {
                let want: Vec<&str> =
                    want.iter().map(|w| if let CValue::Str(s) = w { s.as_str() } else { panic!() }).collect();
                assert_eq!([e.code().as_str(), e.reason()], [want[0], want[1]], "{name}: {e:?}");
                match ran {
                    Err(HandlerError::Refused(b)) => assert_eq!(b, e, "{name}"),
                    other => panic!("{name}: handler gave {other:?}"),
                }
            }
            (got, _, _) => panic!("{name}: unexpected {:?}", got.map(|(e, _)| e)),
        }
        n += 1;
    }
    assert!(n >= 15);
}

#[test]
fn the_params_schema_refuses_bad_calls() {
    let def = def();
    let s = Compiled::compile(&def.params_schema).unwrap();
    let r = obj(get(obj(&load("two_releases.json")), "records"));
    let base = |extra: &[(&str, CValue)]| {
        let mut o: Obj = BTreeMap::from([
            ("old".to_string(), CValue::Str(st(r, "R1").into())),
            ("new".to_string(), CValue::Str(st(r, "R2").into())),
            ("reason".to_string(), CValue::Str("wrong part revision".into())),
        ]);
        for (k, v) in extra {
            o.insert((*k).to_string(), v.clone());
        }
        CValue::Obj(o)
    };
    assert_valid(&s, &base(&[]), "base");
    assert!(s.evaluate(&base(&[("reason", CValue::Str(String::new()))]), &NoHooks).is_err());
    assert!(s.evaluate(&base(&[("reason", CValue::Str("x".repeat(2001)))]), &NoHooks).is_err());
    assert!(s.evaluate(&base(&[("old", CValue::Str("sha256:XY".into()))]), &NoHooks).is_err());
    assert!(s.evaluate(&base(&[("principal", CValue::Str("human".into()))]), &NoHooks).is_err());
    let CValue::Obj(mut no_reason) = base(&[]) else { panic!() };
    no_reason.remove("reason");
    assert!(s.evaluate(&CValue::Obj(no_reason), &NoHooks).is_err());
}

/// AC-2 at the bus: an agent's call over MCP becomes a pending request (nothing is
/// superseded); the class check refuses an agent and a script; a human passes.
#[test]
fn ac2_projection_and_class() {
    let def = def();
    assert_eq!(authorise_projection(ChannelKind::Mcp, &def).unwrap(), Projected::PendingRequest);
    for kind in [ChannelKind::Mcp, ChannelKind::Cli] {
        let e = authorise_class(ctx(kind).principal(), &def).unwrap_err();
        assert_eq!((e.code(), e.reason()), (Code::Policy, "class_forbidden"), "{kind:?}");
    }
    authorise_class(ctx(ChannelKind::Gui).principal(), &def).unwrap();
}

#[test]
fn register_supersede_adds_the_command() {
    let j: Arc<dyn SupersedeSource> = journal("two_releases", "main", None);
    let mut set = HandlerSet::new();
    register_supersede(&mut set, &j).unwrap();
    assert_eq!(set.def(&format!("{SUPERSEDE_RECORD}@1")).unwrap().id, SUPERSEDE_RECORD);
}

#[test]
fn a_cancelled_call_is_cancelled() {
    let src = CancelSource::new();
    src.cancel();
    let cx = ctx_with(ChannelKind::Gui, src.token());
    let h = SupersedeHandler::new(journal("two_releases", "main", None));
    let r = h.run(&NoView, &ValidParams::for_tests(CValue::Obj(BTreeMap::new())), &cx);
    assert!(matches!(r, Err(HandlerError::Cancelled)));
}

/// Parameters the handler cannot read are `validation/schema`, after the caller check.
#[test]
fn malformed_parameters_are_schema_errors() {
    let j = journal("two_releases", "main", None);
    for p in [
        CValue::Obj(BTreeMap::new()),
        CValue::Arr(Vec::new()),
        CValue::Obj(BTreeMap::from([
            ("old".to_string(), CValue::Str("x".into())),
            ("new".to_string(), CValue::Str("y".into())),
            ("reason".to_string(), CValue::Str("z".into())),
        ])),
    ] {
        let e = run_supersede(j.as_ref(), &p, &ctx(ChannelKind::Gui)).unwrap_err();
        assert_eq!((e.code(), e.reason()), (Code::Validation, "schema"), "{p:?}");
    }
}

#[test]
fn refusals_map_to_v3() {
    let v3 = fid_bus::parse_catalog(&std::fs::read(BUS_V3).unwrap()).unwrap();
    let all = [
        SupersedeError::ClassForbidden,
        SupersedeError::Schema("x".into()),
        SupersedeError::NotSupersedable("x".into()),
        SupersedeError::Tampered("x".into()),
    ];
    for e in all {
        let b = supersede_bus_error(&e);
        assert_eq!((b.code().as_str(), b.reason()), e.bus_code(), "{e}");
        assert!(v3.get(b.code().as_str()).is_some_and(|r| r.contains(b.reason())), "{e} not in v3");
        assert_eq!(HandlerError::Refused(b.clone()).into_bus_error(), b);
    }
    assert_eq!(RECORD_SUPERSEDED, "RecordSuperseded");
}
