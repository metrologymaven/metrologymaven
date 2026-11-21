//! CORE.JRN.BR.NAME, bus half: the four handlers against the oracle scenario
//! (`fixtures/core/jrn/br/name/cases.json`) with the caller taken from the bus context, the
//! command schemas checked with `fid_bus::validate::Compiled` (real parameters and real
//! results), the projection and class checks of AC-3 and AC-5, and the v3 mapping of every
//! refusal.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::authorise_class;
use fid_bus::error::{BusError, Code, Detail, ValidationReason};
use fid_bus::handler::{
    effects_check, CancelToken, EntityId, EntityRef, Handler, HandlerCtx, SettingKey, StateView, ValidParams,
};
use fid_bus::policy::projection::{authorise_projection, Projected};
use fid_bus::principal::{Channel, ChannelKind, Identity, PrincipalId, SessionId, SessionKey, SessionTable};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse, CValue};
use fid_journal::branch::{command_defs, BranchError, BranchEvent, Branches, EntryOrigin, EventSource};
use fid_journal::PrincipalKind;
use fid_journal_cmd::{run, to_bus_error, BranchHandler, BranchSource, Op};
use fid_registry::CommandDef;
use fid_types::Digest;
use fid_types::SideEffect;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

const CASES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/jrn/br/name/cases.json");
const BUS_V3: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/bus/pipe/err/bus-errors.v3.json");

fn cases() -> CValue {
    parse(&std::fs::read(CASES).unwrap()).unwrap()
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(o) => o.get(k).unwrap_or_else(|| panic!("missing {k}")),
        _ => panic!("not an object"),
    }
}

fn opt<'a>(v: &'a CValue, k: &str) -> Option<&'a CValue> {
    match v {
        CValue::Obj(o) => o.get(k),
        _ => None,
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("not an array"),
    }
}

fn s(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("not a string: {v:?}"),
    }
}

fn obj(pairs: &[(&str, CValue)]) -> CValue {
    CValue::Obj(pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
}

fn str_v(x: &str) -> CValue {
    CValue::Str(x.to_string())
}

// ------------------------------------------------------------------ source and view

struct Source {
    table: Mutex<Branches>,
    entries: Vec<Digest>,
}

impl BranchSource for Source {
    fn branches(&self) -> Result<Branches, String> {
        Ok(self.table.lock().unwrap().clone())
    }
    fn is_head(&self, head: &Digest) -> bool {
        self.entries.contains(head)
    }
}

fn source() -> Arc<Source> {
    let c = cases();
    let p = get(&c, "project");
    let entries: Vec<Digest> = arr(get(p, "entries")).iter().map(|d| Digest::parse(s(d)).unwrap()).collect();
    let head = Digest::parse(s(get(p, "main_head"))).unwrap();
    Arc::new(Source { table: Mutex::new(Branches::genesis(head)), entries })
}

/// Commits the events a handler returned to the source (what the bus's commit stage does).
/// Commits the events a handler returned to the source (what the bus's commit stage does),
/// each with the origin its entry records: the call's principal kind and the command.
fn commit(src: &Source, ctx: &HandlerCtx, op: Op, events: &[fid_bus::handler::Event]) {
    let mut t = src.table.lock().unwrap();
    let class = if op == Op::Protect { SideEffect::W3 } else { SideEffect::W2 };
    let origin = EntryOrigin {
        kind: ctx.principal().kind(),
        command: op.command().to_string(),
        major: 1,
        class,
        branch: "main".to_string(),
        source: EventSource::Handler,
        human_assertion: None,
    };
    for e in events {
        t.apply(&origin, &BranchEvent::from_parts(e.type_id().as_str(), e.body()).unwrap()).unwrap();
    }
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

/// A handler context whose principal the bus derives from a channel of this kind: GUI is a
/// human, MCP an agent, CLI a script (CORE.BUS.PRIN).
fn ctx(kind: ChannelKind) -> HandlerCtx {
    let mut table = SessionTable::new();
    let session = table.open_session(SessionId::new("s-1").unwrap()).unwrap();
    HandlerCtx::derive(&table, &TestChannel { kind, session }, 0, CancelToken::never()).unwrap()
}

fn channel_of(actor: &str) -> ChannelKind {
    match actor {
        "human" => ChannelKind::Gui,
        "agent" => ChannelKind::Mcp,
        _ => ChannelKind::Cli,
    }
}

fn defs() -> BTreeMap<String, CommandDef> {
    command_defs().unwrap().into_iter().map(|d| (d.id.clone(), d)).collect()
}

fn op_of(op: &str) -> Option<(Op, &'static str)> {
    match op {
        "create" => Some((Op::Create, fid_journal::branch::CREATE_BRANCH)),
        "protect" => Some((Op::Protect, fid_journal::branch::PROTECT_BRANCH)),
        "coediting" => Some((Op::Coediting, fid_journal::branch::SET_BRANCH_COEDITING)),
        _ => None,
    }
}

fn params_of(call: &CValue) -> CValue {
    let CValue::Obj(o) = call else { panic!() };
    CValue::Obj(
        o.iter().filter(|(k, _)| !matches!(k.as_str(), "op" | "actor")).map(|(k, v)| (k.clone(), v.clone())).collect(),
    )
}

fn assert_valid(schema: &Compiled, v: &CValue, what: &str) {
    if let Err(f) = schema.evaluate(v, &NoHooks) {
        panic!("{what}: {} at {} ({})", f.keyword, f.pointer, f.message);
    }
}

// ------------------------------------------------------------------ tests

/// The oracle scenario through the registered handlers, the caller derived by the bus from
/// the channel: the same events, no-ops and v3 errors as the oracle, every event declared in
/// the definition's effects, every result valid against its result schema.
#[test]
fn handlers_follow_the_oracle_with_the_bus_principal() {
    let c = cases();
    let src = source();
    let shared: Arc<dyn BranchSource> = src.clone();
    let defs = defs();
    let mut done = 0;
    for (i, call) in arr(get(get(&c, "scenario"), "calls")).iter().enumerate() {
        let call_v = get(call, "call");
        let want = get(call, "expect");
        if s(get(call_v, "op")) == "agent_branch" {
            // The routing module's path (CORE.BUS.ROUTE), not a command: its event is committed
            // with the bus_route origin, which no handler can give.
            if let Some(w) = opt(want, "event") {
                let e = BranchEvent::from_parts(s(get(w, "type")), get(w, "body")).unwrap();
                let route = EntryOrigin {
                    kind: PrincipalKind::Agent,
                    command: "project.set_tags".into(),
                    major: 1,
                    class: SideEffect::W2,
                    branch: "agent/claude/1".into(),
                    source: EventSource::BusRoute,
                    human_assertion: None,
                };
                src.table.lock().unwrap().apply(&route, &e).unwrap();
            }
            continue;
        }
        let Some((op, id)) = op_of(s(get(call_v, "op"))) else { panic!("op") };
        let def = &defs[id];
        let params = params_of(call_v);
        let actor = s(get(call_v, "actor"));
        let ctx = ctx(channel_of(actor));
        let handler = BranchHandler::new(op, Arc::clone(&shared));
        let got = handler.run(&NoView, &ValidParams::for_tests(params.clone()), &ctx);
        match (got, opt(want, "event"), opt(want, "error")) {
            (Ok(events), Some(w), None) => {
                assert_eq!(events.len(), 1, "call {i}");
                assert_eq!(events[0].type_id().as_str(), s(get(w, "type")), "call {i}");
                assert_eq!(events[0].body(), get(w, "body"), "call {i}");
                effects_check(def, &events).unwrap();
                // The result the bus returns, valid against the result schema.
                let (_, result) = run(op, src.as_ref(), &params, &ctx).unwrap();
                assert_valid(&Compiled::compile(&def.result_schema).unwrap(), &result, &format!("call {i} result"));
                commit(&src, &ctx, op, &events);
            }
            (Ok(events), None, None) => assert!(events.is_empty() && opt(want, "noop").is_some(), "call {i}"),
            (Err(e), None, Some(w)) => {
                let b = e.into_bus_error();
                assert_eq!(b.code().as_str(), s(get(w, "code")), "call {i}: {b}");
                assert_eq!(b.reason(), s(get(w, "reason")), "call {i}: {b}");
            }
            (got, _, _) => panic!("call {i}: got {got:?}, expected {want:?}"),
        }
        done += 1;
    }
    assert!(done >= 20);
    // List: no events, result valid and equal to the oracle's list.
    let (events, list) = run(Op::List, src.as_ref(), &obj(&[]), &ctx(ChannelKind::Gui)).unwrap();
    assert!(events.is_empty());
    assert_valid(&Compiled::compile(&defs[fid_journal::branch::LIST_BRANCHES].result_schema).unwrap(), &list, "list");
    let want = get(get(&c, "scenario"), "branches");
    let got = get(&list, "branches");
    assert_eq!(got, want);
}

/// Parameter schemas accept well-formed calls and refuse malformed ones (the name pattern,
/// missing or extra keys, wrong types); results of every shape validate.
#[test]
fn schemas_check_real_values() {
    let defs = defs();
    let c = cases();
    let head = s(get(get(&c, "project"), "main_head"));
    let compile = |v: &CValue| Compiled::compile(v).unwrap();
    let create = compile(&defs[fid_journal::branch::CREATE_BRANCH].params_schema);
    assert_valid(&create, &obj(&[("name", str_v("trial/a")), ("from", str_v(head))]), "create");
    for bad in [
        obj(&[("name", str_v("Trial A")), ("from", str_v(head))]),
        obj(&[("name", str_v("tri\u{430}l")), ("from", str_v(head))]),
        obj(&[("name", str_v(&"a".repeat(101))), ("from", str_v(head))]),
        obj(&[("name", str_v("trial/a")), ("from", str_v("sha256:XY"))]),
        obj(&[("name", str_v("trial/a"))]),
        obj(&[("name", str_v("trial/a")), ("from", str_v(head)), ("principal", str_v("human"))]),
        obj(&[("name", CValue::Num(1.0)), ("from", str_v(head))]),
    ] {
        assert!(create.evaluate(&bad, &NoHooks).is_err(), "{bad:?} should be refused");
    }
    // Every name the grammar pattern accepts is either accepted or refused as bad_branch_name
    // by the planner; none the pattern refuses gets past it.
    for n in arr(get(&c, "names")) {
        let name = s(get(n, "name"));
        let p = obj(&[("name", str_v(name)), ("from", str_v(head))]);
        if create.evaluate(&p, &NoHooks).is_err() {
            assert_eq!(get(n, "ok"), &CValue::Bool(false), "{name:?} refused by the schema, oracle accepts");
        }
    }
    let list = compile(&defs[fid_journal::branch::LIST_BRANCHES].params_schema);
    assert_valid(&list, &obj(&[]), "list");
    assert!(list.evaluate(&obj(&[("all", CValue::Bool(true))]), &NoHooks).is_err());
    let protect = compile(&defs[fid_journal::branch::PROTECT_BRANCH].params_schema);
    assert_valid(&protect, &obj(&[("branch", str_v("main"))]), "protect");
    assert!(protect.evaluate(&obj(&[("branch", str_v("main")), ("unprotect", CValue::Bool(true))]), &NoHooks).is_err());
    let co = compile(&defs[fid_journal::branch::SET_BRANCH_COEDITING].params_schema);
    assert_valid(
        &co,
        &obj(&[("branch", str_v("main")), ("agent_session", str_v("S")), ("allow", CValue::Bool(true))]),
        "co",
    );
    assert!(co
        .evaluate(&obj(&[("branch", str_v("main")), ("agent_session", str_v("S")), ("allow", str_v("yes"))]), &NoHooks)
        .is_err());
    assert!(co
        .evaluate(
            &obj(&[("branch", str_v("main")), ("agent_session", str_v("a b")), ("allow", CValue::Bool(true))]),
            &NoHooks
        )
        .is_err());
    // Results: a fresh main (created_from null), a protected branch with opt-ins.
    let src = source();
    let one = compile(&defs[fid_journal::branch::PROTECT_BRANCH].result_schema);
    let (_, r) = run(Op::Protect, src.as_ref(), &obj(&[("branch", str_v("main"))]), &ctx(ChannelKind::Gui)).unwrap();
    assert_valid(&one, &r, "protect result");
    let (ev, _) =
        run(Op::Create, src.as_ref(), &obj(&[("name", str_v("x")), ("from", str_v(head))]), &ctx(ChannelKind::Gui))
            .unwrap();
    commit(&src, &ctx(ChannelKind::Gui), Op::Create, &ev);
    let (_, r) =
        run(Op::Create, src.as_ref(), &obj(&[("name", str_v("y")), ("from", str_v(head))]), &ctx(ChannelKind::Gui))
            .unwrap();
    assert_valid(&one, &r, "create result");
}

/// AC-3 error path: an agent calling protect over MCP gets a pending request (the
/// projection check), never a change; a script gets policy/class_forbidden (the class
/// check), and so does the handler if reached. AC-5 error path: a script calling the
/// co-editing command gets policy/human_only; over MCP it is not_found/unknown_command, as if it did not exist.
#[test]
fn ac3_ac5_projection_and_class() {
    let defs = defs();
    let protect = &defs[fid_journal::branch::PROTECT_BRANCH];
    let co = &defs[fid_journal::branch::SET_BRANCH_COEDITING];
    assert_eq!(authorise_projection(ChannelKind::Mcp, protect).unwrap(), Projected::PendingRequest);
    // Co-editing is not on the MCP surface at all (CLI only): an agent cannot even file it.
    let e = authorise_projection(ChannelKind::Mcp, co).unwrap_err();
    assert_eq!((e.code(), e.reason()), (Code::NotFound, "unknown_command"));
    let create = &defs[fid_journal::branch::CREATE_BRANCH];
    assert_eq!(authorise_projection(ChannelKind::Mcp, create).unwrap(), Projected::Run);
    let script = ctx(ChannelKind::Cli);
    let e = authorise_class(script.principal(), protect).unwrap_err();
    assert_eq!((e.code(), e.reason()), (Code::Policy, "class_forbidden"));
    let e = authorise_class(script.principal(), co).unwrap_err();
    assert_eq!((e.code(), e.reason()), (Code::Policy, "human_only"));
    let human = ctx(ChannelKind::Gui);
    authorise_class(human.principal(), protect).unwrap();
    authorise_class(human.principal(), co).unwrap();
    // The handler holds the same line if a call ever reached it.
    let src: Arc<dyn BranchSource> = source();
    for (k, op, params) in [
        (ChannelKind::Mcp, Op::Protect, obj(&[("branch", str_v("main"))])),
        (ChannelKind::Cli, Op::Protect, obj(&[("branch", str_v("main"))])),
        (
            ChannelKind::Mcp,
            Op::Coediting,
            obj(&[("branch", str_v("main")), ("agent_session", str_v("S")), ("allow", CValue::Bool(true))]),
        ),
    ] {
        let h = BranchHandler::new(op, Arc::clone(&src));
        let e = h.run(&NoView, &ValidParams::for_tests(params), &ctx(k)).unwrap_err().into_bus_error();
        assert_eq!(e.code(), Code::Policy, "{op:?} via {k:?}");
    }
    // AC-5 happy path: Branches::coediting(main) holds S after a human's opt-in.
    let s2 = source();
    let (ev, _) = run(
        Op::Coediting,
        s2.as_ref(),
        &obj(&[("branch", str_v("main")), ("agent_session", str_v("S")), ("allow", CValue::Bool(true))]),
        &ctx(ChannelKind::Gui),
    )
    .unwrap();
    commit(&s2, &ctx(ChannelKind::Gui), Op::Coediting, &ev);
    let t = s2.branches().unwrap();
    assert!(t.coediting("main").unwrap().iter().any(|x| x.as_str() == "S"));
}

/// Every refusal maps to a code and reason of the frozen v3 catalogue, unchanged.
#[test]
fn refusals_map_to_v3() {
    let v3 = fid_bus::parse_catalog(&std::fs::read(BUS_V3).unwrap()).unwrap();
    let d = Digest::parse(&format!("sha256:{}", "0".repeat(64))).unwrap();
    for e in [
        BranchError::BadName("x".into()),
        BranchError::Schema("x".into()),
        BranchError::UnknownBranch("x".into()),
        BranchError::UnknownHead(d),
        BranchError::ClassForbidden("x".into()),
        BranchError::HumanOnly("x".into()),
        BranchError::Tampered("x".into()),
    ] {
        let b = to_bus_error(&e);
        assert_eq!((b.code().as_str(), b.reason()), (e.code(), e.reason()), "{e}");
        assert!(v3.get(e.code()).is_some_and(|r| r.contains(e.reason())), "{e} not in v3");
        // A handler may return it (no pipeline-only code).
        let back = fid_bus::handler::HandlerError::Refused(b.clone()).into_bus_error();
        assert_eq!(back, b);
    }
}
