//! TRUST.PROFILE.SELECT against the oracle (`fixtures/trust/profile/select/cases.json`, written
//! by the independent `make_cases.py`): the AC-1 to AC-4 scenario through the registered
//! handlers with the principal derived by the bus from the channel, the AC-3 class and
//! projection checks, the AC-5 exact replay, the forged-event fold cases, the registry checks
//! of both definitions (schemas compiled with `fid_bus::validate::Compiled`) and the file
//! policy of the production tenant-file reader. No keys or secrets are involved.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::authorise_class;
use fid_bus::error::{BusError, Code, Detail, ValidationReason};
use fid_bus::handler::{effects_check, CancelToken, EntityId, EntityRef, Event, EventType, Handler, HandlerCtx};
use fid_bus::handler::{SettingKey, StateView, ValidParams};
use fid_bus::policy::projection::{authorise_projection, params_commitment, EntryOrigin, Projected};
use fid_bus::principal::{
    Channel, ChannelKind, Identity, Principal, PrincipalId, PrincipalKind, PrincipalRef, SessionId, SessionKey,
    SessionTable,
};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_journal::branch::{EntryOrigin as JournalOrigin, EventSource};
use fid_profile_cmd::{
    command_defs, run_set, run_show, Op, ProfileEntry, ProfileHandler, ProfileSource, ProfileState, RootFiles,
    TenantFiles, CONFIG_KEY, GENESIS, PROFILE_SET, SET_PROFILE, SHOW_PROFILE,
};
use fid_registry::{CommandDef, Projection, Surface};
use fid_types::{CommandId, Digest, SideEffect};
use std::sync::{Arc, Mutex};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn cases() -> CValue {
    parse(&std::fs::read(format!("{ROOT}/fixtures/trust/profile/select/cases.json")).unwrap()).unwrap()
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

// ------------------------------------------------------------------ bus context

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
        "script" => ChannelKind::Cli,
        other => panic!("actor {other}"),
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

fn assert_valid(schema: &CValue, v: &CValue, what: &str) {
    if let Err(f) = Compiled::compile(schema).unwrap().evaluate(v, &NoHooks) {
        panic!("{what}: {} at {} ({})", f.keyword, f.pointer, f.message);
    }
}

fn defs() -> (CommandDef, CommandDef) {
    let mut d = command_defs().unwrap().into_iter();
    (d.next().unwrap(), d.next().unwrap())
}

// ------------------------------------------------------------------ journal and files

/// The synthetic hash the test journal gives its n-th committed entry (as the oracle).
fn entry_hash(n: usize) -> Digest {
    Digest::from_bytes(fid_canon::digest_bytes(format!("entry-{n}").as_bytes()).0)
}

/// The tenant's journal as the bus wiring would keep it: committed entries folded in order.
#[derive(Default)]
struct Journal {
    state: Mutex<ProfileState>,
    entries: Mutex<Vec<(CValue, Vec<Event>)>>,
    /// The branch calls commit to; `None` is `main`.
    branch: Option<String>,
}

impl ProfileSource for Journal {
    fn state(&self) -> Result<ProfileState, String> {
        Ok(self.state.lock().unwrap().clone())
    }
    fn call_branch(&self, _: &HandlerCtx) -> Result<String, String> {
        Ok(self.branch.clone().unwrap_or_else(|| "main".to_string()))
    }
}

impl Journal {
    /// What the commit stage does for a set call that returned events: the compare-and-swap
    /// check, then one entry on main with the call's principal, command and parameters
    /// commitment, folded.
    fn commit(&self, ctx: &HandlerCtx, params: &CValue, events: &[Event]) {
        let mut entries = self.entries.lock().unwrap();
        let hash = entry_hash(entries.len() + 1);
        let entry = ProfileEntry::for_call(ctx.principal(), params_commitment(params).unwrap(), hash).unwrap();
        let mut state = self.state.lock().unwrap();
        state.precommit(&entry, events).unwrap();
        state.apply_entry(&entry, events).unwrap();
        entries.push((params.clone(), events.to_vec()));
    }
}

/// Test-only reader: the parameter value is a path relative to the workspace root.
struct FixtureFiles;

impl TenantFiles for FixtureFiles {
    fn read(&self, _: &Principal, _: &CommandDef, value: &str, limit: usize) -> Result<Vec<u8>, BusError> {
        let b = std::fs::read(format!("{ROOT}/{value}")).unwrap();
        assert!(b.len() <= limit);
        Ok(b)
    }
}

fn event_bytes(events: &[Event]) -> Vec<u8> {
    let v = CValue::Arr(
        events
            .iter()
            .map(|e| {
                CValue::Obj(
                    [
                        ("body".to_string(), e.body().clone()),
                        ("type".to_string(), CValue::Str(e.type_id().to_string())),
                    ]
                    .into_iter()
                    .collect(),
                )
            })
            .collect(),
    );
    to_canonical_bytes(&v).unwrap()
}

fn expected_events(want: &CValue) -> Vec<u8> {
    to_canonical_bytes(get(want, "events")).unwrap()
}

// ------------------------------------------------------------------ AC-1 to AC-4, AC-5

/// The oracle scenario through the registered handlers (AC-1 to AC-4), then the exact replay
/// of the resulting journal (AC-5).
#[test]
fn scenario_follows_the_oracle_and_replays_identically() {
    let c = cases();
    let (set_def, show_def) = defs();
    let journal = Arc::new(Journal::default());
    let source: Arc<dyn ProfileSource> = journal.clone();
    let files: Arc<dyn TenantFiles> = Arc::new(FixtureFiles);
    let set_h = ProfileHandler::new(Op::Set, set_def.clone(), Arc::clone(&source), Arc::clone(&files));
    let show_h = ProfileHandler::new(Op::Show, show_def.clone(), Arc::clone(&source), Arc::clone(&files));
    let mut journaled = 0;
    for (i, call) in arr(get(&c, "scenario")).iter().enumerate() {
        let actor = s(get(call, "actor"));
        let params = get(call, "params");
        let want = get(call, "expect");
        let cx = ctx(channel_of(actor));
        match s(get(call, "op")) {
            "show" => {
                assert!(show_h.run(&NoView, &ValidParams::for_tests(params.clone()), &cx).unwrap().is_empty());
                let got = run_show(journal.as_ref()).unwrap();
                assert_eq!(&got, get(want, "result"), "call {i}");
                assert_valid(&show_def.result_schema, &got, &format!("call {i} result"));
            }
            "set" => {
                let got = set_h.run(&NoView, &ValidParams::for_tests(params.clone()), &cx);
                match (got, opt(want, "error")) {
                    (Ok(events), None) => {
                        assert_eq!(event_bytes(&events), expected_events(want), "call {i}");
                        effects_check(&set_def, &events).unwrap();
                        assert_valid(&set_def.params_schema, params, &format!("call {i} params"));
                        // The result the bus returns, computed before the commit.
                        let (_, result) = run_set(&set_def, journal.as_ref(), &FixtureFiles, params, &cx).unwrap();
                        assert_eq!(&result, get(want, "result"), "call {i}");
                        assert_valid(&set_def.result_schema, &result, &format!("call {i} result"));
                        if !events.is_empty() {
                            journal.commit(&cx, params, &events);
                            journaled += 1;
                        }
                    }
                    (Err(e), Some(w)) => {
                        let b = e.into_bus_error();
                        assert_eq!(b.code().as_str(), s(&arr(w)[0]), "call {i}: {b}");
                        assert_eq!(b.reason(), s(&arr(w)[1]), "call {i}: {b}");
                        if let Some(p) = opt(want, "detail_prefix") {
                            assert!(b.detail().as_untrusted_str().starts_with(s(p)), "call {i}: {b}");
                        }
                    }
                    (got, _) => panic!("call {i}: got {got:?}, expected {want:?}"),
                }
            }
            other => panic!("op {other}"),
        }
    }
    // AC-2 and AC-4: one entry per change, none for the unchanged set or a refusal.
    assert_eq!(journaled, 2);
    let state = journal.state().unwrap();
    assert_eq!(state.config_value(), Some(s(get(get(&c, "lean"), "document")).as_bytes()));
    assert_eq!(CONFIG_KEY, "trust.profile");

    // AC-5: replay in exact mode. Re-run each journaled call as the same human against the
    // state before it; the events must be the recorded ones, byte for byte.
    let recorded = journal.entries.lock().unwrap().clone();
    let replay = Journal::default();
    let human = ctx(ChannelKind::Gui);
    for (params, events) in &recorded {
        let (again, _) = run_set(&set_def, &replay, &FixtureFiles, params, &human).unwrap();
        assert_eq!(event_bytes(&again), event_bytes(events), "replay is not IDENTICAL");
        replay.commit(&human, params, &again);
    }
    assert_eq!(replay.state().unwrap(), state, "replayed state differs");
}

// ------------------------------------------------------------------ AC-3

/// No non-human principal can set the profile on any surface, and the MCP surface has no
/// `sign_set_profile` tool; showing is open to every principal.
#[test]
fn ac3_set_is_a0_human_only_and_never_on_mcp() {
    let (set_def, show_def) = defs();
    assert_eq!(set_def.side_effect, SideEffect::A0);
    assert!(set_def.human_only);
    assert_eq!(set_def.effective_projection(), Projection::Never);
    assert!(!set_def.exposure.contains(&Surface::Mcp));
    assert_eq!(show_def.side_effect, SideEffect::R0);
    for kind in [ChannelKind::Mcp, ChannelKind::ServerMcp] {
        let e = authorise_projection(kind, &set_def).unwrap_err();
        assert_eq!((e.code(), e.reason()), (Code::NotFound, "unknown_command"));
        assert_eq!(authorise_projection(kind, &show_def).unwrap(), Projected::Run);
    }
    // The class check refuses every principal but a human, wherever it arrives.
    for kind in ChannelKind::ALL {
        let mut table = SessionTable::new();
        let session = table.open_session(SessionId::new("s-2").unwrap()).unwrap();
        let Ok(cx) = HandlerCtx::derive(&table, &TestChannel { kind: *kind, session }, 0, CancelToken::never()) else {
            continue;
        };
        let class = authorise_class(cx.principal(), &set_def);
        let handler = run_set(&set_def, &Journal::default(), &FixtureFiles, &name_only("regulated"), &cx);
        if cx.principal().kind() == PrincipalKind::Human {
            class.unwrap();
            assert!(handler.is_ok());
        } else {
            assert_eq!(class.unwrap_err().code(), Code::Policy, "{kind:?}");
            let e = handler.unwrap_err();
            assert_eq!((e.code(), e.reason()), (Code::Policy, "human_only"), "{kind:?}");
        }
        if matches!(cx.principal().kind(), PrincipalKind::Human | PrincipalKind::Agent | PrincipalKind::Script) {
            authorise_class(cx.principal(), &show_def).unwrap();
        }
    }
    // The tools an MCP client lists: only the show command.
    let tools: Vec<String> = command_defs()
        .unwrap()
        .iter()
        .filter(|d| d.exposure.contains(&Surface::Mcp) && d.effective_projection() != Projection::Never)
        .map(|d| d.id.replace('.', "_"))
        .collect();
    assert_eq!(tools, vec!["sign_show_profile".to_string()]);
}

fn name_only(name: &str) -> CValue {
    CValue::Obj([("name".to_string(), CValue::Str(name.to_string()))].into_iter().collect())
}

// ------------------------------------------------------------------ forged events

fn kind_of(k: &str) -> PrincipalKind {
    match k {
        "human" => PrincipalKind::Human,
        "agent" => PrincipalKind::Agent,
        "script" => PrincipalKind::Script,
        "system" => PrincipalKind::System,
        other => panic!("kind {other}"),
    }
}

fn entry_of(o: &CValue) -> ProfileEntry {
    let source = match s(get(o, "source")) {
        "handler" => EventSource::Handler,
        "bus_route" => EventSource::BusRoute,
        "bus_release" => EventSource::BusRelease,
        other => panic!("source {other}"),
    };
    let j_major = match get(o, "j_major") {
        CValue::Num(n) => *n as u32,
        _ => panic!("j_major"),
    };
    ProfileEntry {
        origin: EntryOrigin::new(
            PrincipalRef { kind: kind_of(s(get(o, "kind"))), id: PrincipalId::new(s(get(o, "id"))).unwrap() },
            CommandId::parse(s(get(o, "command"))).unwrap(),
            Digest::parse(s(get(o, "params_commitment"))).unwrap(),
        ),
        journal: JournalOrigin {
            kind: kind_of(s(get(o, "j_kind"))),
            command: s(get(o, "j_command")).to_string(),
            major: j_major,
            class: SideEffect::parse(s(get(o, "class"))).unwrap(),
            branch: s(get(o, "branch")).to_string(),
            source,
            human_assertion: None,
        },
        promoted_from: match get(o, "promoted_from") {
            CValue::Null => None,
            v => Some(Digest::parse(s(v)).unwrap()),
        },
        hash: Digest::parse(s(get(o, "hash"))).unwrap(),
    }
}

fn events_of(v: &CValue) -> Vec<Event> {
    arr(v).iter().map(|e| Event::new(EventType::new(s(get(e, "type"))).unwrap(), get(e, "body").clone())).collect()
}

/// Every forged, spliced, replayed or raced ProfileSet event is refused with the state
/// unchanged; a genuine one is accepted; one off `main` is ignored. The record is the entry's,
/// never the event body's. Where the oracle gives one, the commit stage's precommit check
/// returns the expected error first (the compare-and-swap of a concurrent set).
#[test]
fn forged_events_are_refused_by_the_fold() {
    let c = cases();
    let all = arr(get(&c, "forged"));
    assert!(all.len() >= 20);
    for case in all {
        let name = s(get(case, "name"));
        let mut state = ProfileState::new();
        for prior in arr(get(case, "prior")) {
            let p = arr(prior);
            state.apply_entry(&entry_of(&p[0]), &events_of(&p[1])).unwrap();
        }
        let before = state.clone();
        let entry = entry_of(get(case, "entry"));
        let events = events_of(get(case, "events"));
        if let Some(w) = opt(case, "precommit") {
            let e = state.precommit(&entry, &events).unwrap_err();
            assert_eq!((e.code().as_str(), e.reason()), (s(&arr(w)[0]), s(&arr(w)[1])), "{name}");
        }
        let got = state.apply_entry(&entry, &events);
        match s(get(case, "expect")) {
            "accepted" => got.unwrap_or_else(|e| panic!("{name}: {e}")),
            "refused" => {
                let e = got.expect_err(name);
                assert_eq!((e.code(), e.reason()), (Code::Internal, "invariant_violated"), "{name}");
                if let Some(d) = opt(case, "detail") {
                    assert!(e.detail().as_untrusted_str().contains(s(d)), "{name}: {e}");
                }
                assert_eq!(state, before, "{name}: a refused entry changed the state");
            }
            other => panic!("{other}"),
        }
        let (active, origin) = state.active().unwrap();
        assert_eq!(active.digest().as_str(), s(get(case, "active")), "{name}");
        assert_eq!(origin.as_str(), s(get(case, "origin_after")), "{name}");
    }
}

/// The handler's own event passes the fold only from the record its entry has: the same event
/// committed under another principal kind, another command, off `main` (ignored), as a
/// promoted copy, or with a request event beside it is not applied.
#[test]
fn handler_events_bind_to_their_entry() {
    let (set_def, _) = defs();
    let human = ctx(ChannelKind::Gui);
    let c = cases();
    let params = get(&arr(get(&c, "scenario"))[4], "params").clone();
    let (events, _) = run_set(&set_def, &Journal::default(), &FixtureFiles, &params, &human).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].type_id().as_str(), PROFILE_SET);
    assert_eq!(get(events[0].body(), "decided_at"), &CValue::Str(GENESIS.to_string()));
    let commitment = params_commitment(&params).unwrap();
    let ok = ProfileEntry::for_call(human.principal(), commitment, entry_hash(1)).unwrap();
    let mut applied = ProfileState::new();
    applied.apply_entry(&ok, &events).unwrap();
    assert_eq!(applied.decided_at(), entry_hash(1).as_str());
    let agent = ctx(ChannelKind::Mcp);
    let as_agent = ProfileEntry::for_call(agent.principal(), commitment, entry_hash(1)).unwrap();
    assert!(ProfileState::new().apply_entry(&as_agent, &events).is_err());
    let mut show = ok.clone();
    show.origin = EntryOrigin::new(
        human.principal().reference(),
        CommandId::parse(&format!("{SHOW_PROFILE}@1")).unwrap(),
        commitment,
    );
    assert!(ProfileState::new().apply_entry(&show, &events).is_err());
    let mut side = ok.clone();
    side.journal.branch = "feature/qa".to_string();
    let mut st = ProfileState::new();
    st.apply_entry(&side, &events).unwrap();
    assert_eq!(st, ProfileState::new(), "an entry off main does not count");
    let mut promoted = ok.clone();
    promoted.promoted_from = Some(entry_hash(9));
    assert!(ProfileState::new().apply_entry(&promoted, &events).is_err());
    let mut with_request = events.clone();
    with_request.push(Event::new(EventType::new("RequestApproved").unwrap(), CValue::Obj(Default::default())));
    assert!(ProfileState::new().apply_entry(&ok, &with_request).is_err());
}

/// A set whose call would commit off `main` is refused before anything is read.
#[test]
fn set_runs_only_on_main() {
    let (set_def, _) = defs();
    let human = ctx(ChannelKind::Gui);
    let side = Journal { branch: Some("feature/qa".to_string()), ..Journal::default() };
    let e = run_set(&set_def, &side, &FixtureFiles, &name_only("regulated"), &human).unwrap_err();
    assert_eq!((e.code(), e.reason()), (Code::Precondition, "false"));
}

// ------------------------------------------------------------------ definitions and files

#[test]
fn definitions_pass_the_registry_checks() {
    let defs = command_defs().unwrap();
    let ids: Vec<&str> = defs.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids, [SET_PROFILE, SHOW_PROFILE]);
    let decls: Vec<fid_registry::CommandDecl> = defs.iter().map(|d| fid_registry::CommandDecl(d.to_value())).collect();
    assert_eq!(fid_registry::validate(&decls), vec![]);
    for d in &defs {
        Compiled::compile(&d.params_schema).unwrap();
        Compiled::compile(&d.result_schema).unwrap();
    }
    // The event is reserved to its command: no other command may declare it.
    assert!(fid_registry::RESERVED_EFFECTS.contains(&(PROFILE_SET, Some((SET_PROFILE, 1)))));
    let mut other = defs[1].to_value();
    if let CValue::Obj(o) = &mut other {
        o.insert("effects".to_string(), CValue::Arr(vec![CValue::Str(PROFILE_SET.to_string())]));
    }
    let errors = fid_registry::validate(&[fid_registry::CommandDecl(other)]);
    assert!(errors.iter().any(|e| e.rule.as_str() == "reserved_effect"), "{errors:?}");
}

/// The production reader goes through the bus's file policy: a human may pass a raw path, an
/// agent never; a handle to an unconfigured root is refused.
#[test]
fn root_files_apply_the_file_policy() {
    let (set_def, _) = defs();
    let files = RootFiles::default();
    let path = std::fs::canonicalize(format!("{ROOT}/fixtures/profile/regulated-with-xqa.json")).unwrap();
    let raw = path.to_str().unwrap();
    let human = ctx(ChannelKind::Gui);
    let got = files.read(human.principal(), &set_def, raw, 128 * 1024).unwrap();
    assert_eq!(got, std::fs::read(&path).unwrap());
    let e = files.read(human.principal(), &set_def, raw, 16).unwrap_err();
    assert_eq!((e.code(), e.reason()), (Code::Validation, "input_limit"));
    let agent = ctx(ChannelKind::Mcp);
    let e = files.read(agent.principal(), &set_def, raw, 128 * 1024).unwrap_err();
    assert_eq!((e.code(), e.reason()), (Code::Policy, "raw_path_refused"));
    let e = files.read(human.principal(), &set_def, "root:imports/tenant.json", 128 * 1024).unwrap_err();
    assert_eq!((e.code(), e.reason()), (Code::Policy, "bad_root"));
}
