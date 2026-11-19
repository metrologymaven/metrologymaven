//! Acceptance criteria of spec node CORE.BUS.PIPE.HANDLER (AC-1 .. AC-3) and the pinned
//! Design rules, with the hand-labelled fixtures
//! `fixtures/core/bus/pipe/handler/{effects,envelopes}.json`. AC-1 itself is the
//! `compile_fail` doctest on `fid_bus::handler::Handler`, built from
//! `tests/compile_fail/mut_state.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{
    CancelledReason, InternalReason, NotFoundReason, PreconditionReason, UndeclaredEffectReason, ValidationReason,
};
use fid_bus::handler::{
    effects_check, run_handler, CancelSource, CancelToken, EntityId, EntityRef, Event, EventType, Handler, HandlerCtx,
    HandlerError, HandlerSet, SettingKey, StateView, ValidParams, MAX_ENTITY_ID_BYTES, MAX_EVENTS,
    MAX_EVENT_BODY_BYTES, MAX_EVENT_TYPE_BYTES, MAX_HANDLERS, MAX_SETTING_KEY_BYTES,
};
use fid_bus::principal::{
    Channel, ChannelKind, Identity, PrincipalId, PrincipalKind, PrincipalRef, SessionId, SessionKey, SessionTable,
};
use fid_bus::{bus_errors_digest, BusError, CheckedEvents, Code, Detail};
use fid_canon::{parse, to_canonical_bytes, CValue, Digest};
use fid_registry::CommandDef;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

type Obj = BTreeMap<String, CValue>;

fn fixture(name: &str) -> Obj {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/core/bus/pipe/handler").join(name);
    match parse(&std::fs::read(&path).unwrap()).unwrap() {
        CValue::Obj(o) => o,
        other => panic!("{name} is {other:?}"),
    }
}

fn s<'a>(o: &'a Obj, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(v)) => v,
        other => panic!("{k} is {other:?}"),
    }
}

fn arr<'a>(o: &'a Obj, k: &str) -> &'a [CValue] {
    match o.get(k) {
        Some(CValue::Arr(v)) => v,
        other => panic!("{k} is {other:?}"),
    }
}

fn obj(v: &CValue) -> &Obj {
    match v {
        CValue::Obj(o) => o,
        other => panic!("not an object: {other:?}"),
    }
}

fn effects_fixture() -> (CommandDef, Obj) {
    let f = fixture("effects.json");
    let def = CommandDef::from_value(f.get("command").unwrap().clone()).unwrap();
    (def, f)
}

// ------------------------------------------------------------------ test doubles

struct MemState {
    entities: BTreeMap<EntityId, (String, CValue)>,
    head: Digest,
    settings: BTreeMap<SettingKey, CValue>,
}

impl MemState {
    fn new() -> Self {
        let mut entities = BTreeMap::new();
        entities.insert(EntityId::new("e1").unwrap(), ("feature.plane".to_string(), CValue::Str("p".into())));
        let mut settings = BTreeMap::new();
        settings.insert(SettingKey::new("units.length").unwrap(), CValue::Str("mm".into()));
        MemState { entities, head: Digest([7; 32]), settings }
    }
}

impl StateView for MemState {
    fn entity(&self, id: &EntityId) -> Option<EntityRef<'_>> {
        self.entities.get_key_value(id).map(|(id, (kind, body))| EntityRef { id, kind, body })
    }
    fn head(&self) -> Digest {
        self.head
    }
    fn setting(&self, key: &SettingKey) -> Option<CValue> {
        self.settings.get(key).cloned()
    }
}

struct TestChannel {
    kind: ChannelKind,
    identity: Option<Identity>,
    session: SessionKey,
}

impl Channel for TestChannel {
    fn kind(&self) -> ChannelKind {
        self.kind
    }
    fn authenticated_identity(&self) -> Option<Identity> {
        self.identity.clone()
    }
    fn session(&self) -> &SessionKey {
        &self.session
    }
}

fn channel(table: &mut SessionTable, kind: ChannelKind, subject: Option<&str>) -> TestChannel {
    let session = table.open_session(SessionId::new("desk-1").unwrap()).unwrap();
    let identity = subject.map(|s| Identity::new(PrincipalId::new(s).unwrap()));
    TestChannel { kind, identity, session }
}

fn ctx(cancel: CancelToken) -> (SessionTable, HandlerCtx) {
    let mut table = SessionTable::new();
    let ch = channel(&mut table, ChannelKind::Gui, Some("alice"));
    let c = HandlerCtx::derive(&table, &ch, 42, cancel).unwrap();
    (table, c)
}

/// Returns a fixed list of events.
struct Fixed(Vec<Event>);

impl Handler for Fixed {
    fn run(&self, _: &dyn StateView, _: &ValidParams, _: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        Ok(self.0.clone())
    }
}

/// Fails with a fixed error.
struct Failing(HandlerError);

impl Handler for Failing {
    fn run(&self, _: &dyn StateView, _: &ValidParams, _: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        Err(self.0.clone())
    }
}

/// The commit stage's stand-in: journals only checked events.
fn commit(journal: &mut Vec<Event>, r: Result<CheckedEvents, BusError>) -> Result<(), BusError> {
    let (command, _principal, events) = r?.into_parts();
    assert_eq!(command.as_str(), ID);
    journal.extend(events);
    Ok(())
}

/// The fixture command's ID.
const ID: &str = "test.create_element@1";

/// Registers `h` for `def` in a fresh set and runs it as command `def.id@def.major`.
fn run(
    def: &CommandDef,
    h: impl Handler + 'static,
    view: &dyn StateView,
    params: &ValidParams,
    ctx: &HandlerCtx,
) -> Result<CheckedEvents, BusError> {
    let mut set = HandlerSet::new();
    set.register(def.clone(), Box::new(h)).unwrap();
    run_handler(&set, &format!("{}@{}", def.id, def.major), view, params, ctx)
}

fn events_of(v: &[CValue]) -> Vec<Event> {
    v.iter()
        .map(|e| {
            let o = obj(e);
            Event::new(EventType::new(s(o, "type")).unwrap(), o.get("body").unwrap().clone())
        })
        .collect()
}

fn params() -> ValidParams {
    ValidParams::for_tests(CValue::Obj(Obj::new()))
}

fn assert_err(e: &BusError, code: Code, reason: &str) {
    assert_eq!((e.code(), e.reason()), (code, reason), "{e:?}");
}

// ------------------------------------------------------------------ AC-1

/// AC-1 is the `compile_fail` doctest built from `mut_state.rs`. Stable rustdoc does not
/// check the error code, so the control doctest built from `mut_state_control.rs` carries
/// the proof: this guards that both are wired in and differ only in `&mut` on the view.
#[test]
fn ac1_compile_fail_file_differs_from_control_only_in_mut() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let read = |p: &str| std::fs::read_to_string(dir.join(p)).unwrap().replace("\r\n", "\n");
    let case = read("tests/compile_fail/mut_state.rs");
    let control = read("tests/compile_fail/mut_state_control.rs");
    assert_eq!(case.matches("&mut").count(), 1, "exactly one &mut");
    assert!(case.contains("view: &mut dyn StateView"));
    assert_eq!(case.replacen("&mut ", "&", 1), control);
    assert_ne!(case, control);
    let diff: Vec<(&str, &str)> = case.lines().zip(control.lines()).filter(|(a, b)| a != b).collect();
    assert_eq!(diff.len(), 1);
    assert_eq!(case.lines().count(), control.lines().count());
    let src = read("src/handler.rs");
    assert!(src.contains("#[doc = \"```compile_fail\"]\n#[doc = include_str!(\"../tests/compile_fail/mut_state.rs\")]"));
    assert!(src.contains("#[doc = \"```\"]\n#[doc = include_str!(\"../tests/compile_fail/mut_state_control.rs\")]"));
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_effects_fixture() {
    let (def, f) = effects_fixture();
    assert_eq!(def.effects, vec!["ElementCreated".to_string()]);
    let (_table, c) = ctx(CancelToken::never());
    let view = MemState::new();
    let cases = arr(&f, "cases");
    assert!(cases.len() >= 6);
    for case in cases {
        let case = obj(case);
        let name = s(case, "name");
        let events = events_of(arr(case, "events"));
        let expect = obj(case.get("expect").unwrap());
        let mut journal = Vec::new();
        let r = commit(&mut journal, run(&def, Fixed(events.clone()), &view, &params(), &c));
        let direct = effects_check(&def, &events);
        if expect.contains_key("ok") {
            assert_eq!(r, Ok(()), "{name}");
            assert_eq!(direct, Ok(()), "{name}");
            let CValue::Num(n) = expect.get("journaled").unwrap() else { panic!("{name}") };
            assert_eq!(journal.len() as f64, *n, "{name}");
            assert_eq!(journal, events, "{name}: journaled in handler order");
        } else {
            let e = r.unwrap_err();
            assert_eq!(e.code().as_str(), s(expect, "code"), "{name}");
            assert_eq!(e.reason(), s(expect, "reason"), "{name}");
            assert_eq!(e.detail().as_untrusted_str(), s(expect, "detail"), "{name}");
            assert_eq!(direct, Err(e), "{name}");
            assert!(journal.is_empty(), "{name}: nothing journaled");
        }
    }
}

#[test]
fn ac2_created_then_deleted_names_deleted() {
    let (def, _) = effects_fixture();
    let created = Event::new(EventType::new("ElementCreated").unwrap(), CValue::Null);
    let deleted = Event::new(EventType::new("ElementDeleted").unwrap(), CValue::Null);
    assert_eq!(effects_check(&def, std::slice::from_ref(&created)), Ok(()));
    let e = effects_check(&def, &[created, deleted]).unwrap_err();
    assert_eq!(
        e,
        BusError::UndeclaredEffect(
            UndeclaredEffectReason::UndeclaredEventType,
            Detail::new("undeclared event type ElementDeleted at index 1")
        )
    );
}

// ------------------------------------------------------------------ AC-3

fn many(n: usize) -> Vec<Event> {
    let t = EventType::new("ElementCreated").unwrap();
    (0..n).map(|_| Event::new(t.clone(), CValue::Null)).collect()
}

#[test]
fn ac3_too_many_events_is_effect_limit() {
    let (def, _) = effects_fixture();
    let (_table, c) = ctx(CancelToken::never());
    let view = MemState::new();
    let mut journal = Vec::new();
    let r = commit(&mut journal, run(&def, Fixed(many(MAX_EVENTS + 1)), &view, &params(), &c));
    let e = r.unwrap_err();
    assert_eq!(e.code(), Code::Validation);
    assert_eq!(e.reason(), ValidationReason::EffectLimit.as_str());
    assert_eq!(e.detail().as_untrusted_str(), "handler returned 100001 events; the limit is 100000");
    assert!(journal.is_empty());
}

#[test]
fn ac3_exactly_the_limit_passes() {
    assert_eq!(MAX_EVENTS, 100_000);
    let (def, _) = effects_fixture();
    assert_eq!(effects_check(&def, &many(MAX_EVENTS)), Ok(()));
}

#[test]
fn count_limit_comes_before_undeclared_type() {
    let (def, _) = effects_fixture();
    let mut events = many(MAX_EVENTS);
    events.insert(0, Event::new(EventType::new("ElementDeleted").unwrap(), CValue::Null));
    assert_err(&effects_check(&def, &events).unwrap_err(), Code::Validation, "effect_limit");
}

#[test]
fn body_bytes_limit_is_effect_limit() {
    assert_eq!(MAX_EVENT_BODY_BYTES, 256 * 1024 * 1024);
    let (def, _) = effects_fixture();
    let t = EventType::new("ElementCreated").unwrap();
    // A string of n plain ASCII bytes has n + 2 canonical bytes (the quotes).
    let half = usize::try_from(MAX_EVENT_BODY_BYTES / 2).unwrap() - 2;
    let mut events = vec![
        Event::new(t.clone(), CValue::Str("x".repeat(half))),
        Event::new(t.clone(), CValue::Str("y".repeat(half))),
    ];
    assert_eq!(effects_check(&def, &events), Ok(()), "exactly 256 MiB passes");
    events.push(Event::new(t, CValue::Null));
    let e = effects_check(&def, &events).unwrap_err();
    assert_err(&e, Code::Validation, "effect_limit");
    assert_eq!(e.detail().as_untrusted_str(), "event bodies exceed 268435456 bytes at index 2");
    // An undeclared type is reported before the size limit, wherever it sits.
    events.push(Event::new(EventType::new("ElementDeleted").unwrap(), CValue::Null));
    assert_err(&effects_check(&def, &events).unwrap_err(), Code::UndeclaredEffect, "undeclared_event_type");
}

#[test]
fn checked_events_report_body_bytes() {
    let (def, _) = effects_fixture();
    let (_table, c) = ctx(CancelToken::never());
    let t = EventType::new("ElementCreated").unwrap();
    // {"id":"e1"} is 11 canonical bytes; null is 4.
    let events = vec![Event::new(t.clone(), parse(br#"{"id":"e1"}"#).unwrap()), Event::new(t, CValue::Null)];
    let checked = run(&def, Fixed(events.clone()), &MemState::new(), &params(), &c).unwrap();
    assert_eq!(checked.body_bytes(), 15);
    assert_eq!(checked.events(), events.as_slice());
}

#[test]
fn body_without_canonical_form_is_schema() {
    let (def, _) = effects_fixture();
    let events = vec![Event::new(EventType::new("ElementCreated").unwrap(), CValue::Num(f64::NAN))];
    let e = effects_check(&def, &events).unwrap_err();
    assert_err(&e, Code::Validation, "schema");
    assert_eq!(e.detail().as_untrusted_str(), "event 0 body has no canonical form: non_finite_number");
}

// ------------------------------------------------------------------ handler errors and cancel

#[test]
fn handler_errors_map_to_catalogue() {
    let (def, _) = effects_fixture();
    let (_table, c) = ctx(CancelToken::never());
    let view = MemState::new();
    let refusal = BusError::Precondition(PreconditionReason::False, Detail::new("no plane"));
    let cases = [
        (HandlerError::Refused(refusal.clone()), refusal),
        (
            HandlerError::Refused(BusError::NotFound(NotFoundReason::UnknownEntity, Detail::new("e9"))),
            BusError::NotFound(NotFoundReason::UnknownEntity, Detail::new("e9")),
        ),
        (
            HandlerError::Refused(BusError::UndeclaredEffect(
                UndeclaredEffectReason::UndeclaredEventType,
                Detail::new("spoof"),
            )),
            BusError::Internal(
                InternalReason::HandlerFailed,
                Detail::new("handler returned pipeline-only code undeclared_effect"),
            ),
        ),
        (
            HandlerError::Cancelled,
            BusError::Cancelled(CancelledReason::Requested, Detail::new("handler stopped on cancel")),
        ),
        (
            HandlerError::Failed(Detail::new("disk")),
            BusError::Internal(InternalReason::HandlerFailed, Detail::new("disk")),
        ),
    ];
    for (handler_error, want) in cases {
        let mut journal = Vec::new();
        let r = commit(&mut journal, run(&def, Failing(handler_error), &view, &params(), &c));
        assert_eq!(r, Err(want));
        assert!(journal.is_empty());
    }
}

#[test]
fn every_pipeline_only_code_becomes_handler_failed() {
    for code in Code::ALL {
        let reason = code.reasons()[0];
        let e = BusError::from_wire(code.as_str(), reason, "x").unwrap();
        let mapped = HandlerError::Refused(e.clone()).into_bus_error();
        let pipeline_only = matches!(
            code,
            Code::HiddenCommand
                | Code::UndeclaredEffect
                | Code::Unauthenticated
                | Code::IdempotencyMismatch
                | Code::UpgradeRefused
        );
        if pipeline_only {
            assert_err(&mapped, Code::Internal, "handler_failed");
        } else {
            assert_eq!(mapped, e);
        }
    }
}

/// Cancels its own call from inside, then returns declared events.
struct CancelsDuring(CancelSource);

impl Handler for CancelsDuring {
    fn run(&self, _: &dyn StateView, _: &ValidParams, ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        assert!(!ctx.cancel().is_cancelled());
        self.0.cancel();
        Ok(many(1))
    }
}

#[test]
fn cancel_during_handler_commits_nothing() {
    let (def, _) = effects_fixture();
    let source = CancelSource::new();
    let (_table, c) = ctx(source.token());
    let mut journal = Vec::new();
    let r = commit(&mut journal, run(&def, CancelsDuring(source), &MemState::new(), &params(), &c));
    assert_err(&r.unwrap_err(), Code::Cancelled, "requested");
    assert!(journal.is_empty());
}

// ------------------------------------------------------------------ principal and borrows

/// Records the principal, seed and state it saw.
struct Observer(Arc<Mutex<Vec<String>>>);

impl Handler for Observer {
    fn run(&self, view: &dyn StateView, params: &ValidParams, ctx: &HandlerCtx) -> Result<Vec<Event>, HandlerError> {
        let mut seen = self.0.lock().unwrap();
        seen.push(format!("{}:{}", ctx.principal().kind().as_str(), ctx.principal().id().as_str()));
        seen.push(ctx.seed().to_string());
        let e = view.entity(&EntityId::new("e1").unwrap()).unwrap();
        seen.push(format!("{}={}", e.id.as_str(), e.kind));
        seen.push(format!("{:?}", view.setting(&SettingKey::new("units.length").unwrap())));
        seen.push(format!("{}", view.entity(&EntityId::new("nope").unwrap()).is_none()));
        assert_eq!(view.head(), Digest([7; 32]));
        assert!(matches!(params.value(), CValue::Obj(_)));
        Ok(Vec::new())
    }
}

#[test]
fn principal_comes_from_the_channel_not_the_params() {
    let (def, _) = effects_fixture();
    let mut table = SessionTable::new();
    let ch = channel(&mut table, ChannelKind::Mcp, Some("client-7"));
    let c = HandlerCtx::derive(&table, &ch, 9, CancelToken::never()).unwrap();
    // Parameters that claim a principal have no effect.
    let p =
        ValidParams::for_tests(parse(br#"{"principal":{"kind":"human","id":"mallory"},"as":"mallory"}"#).unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let checked = run(&def, Observer(Arc::clone(&seen)), &MemState::new(), &p, &c).unwrap();
    assert_eq!(c.principal().kind(), PrincipalKind::Agent);
    assert_eq!(checked.principal(), &c.principal().reference());
    assert_eq!(checked.principal().id.as_str(), "client-7");
    let seen = seen.lock().unwrap().clone();
    assert_eq!(
        seen,
        vec!["agent:client-7", "9", "e1=feature.plane", "Some(Str(\"mm\"))", "true"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
    );
}

#[test]
fn unauthenticated_channel_never_reaches_the_handler() {
    let mut table = SessionTable::new();
    let ch = channel(&mut table, ChannelKind::Gui, None);
    let e = HandlerCtx::derive(&table, &ch, 0, CancelToken::never()).unwrap_err();
    assert_err(&e, Code::Unauthenticated, "no_identity");
}

#[test]
fn ctx_holds_no_table_borrow() {
    let mut table = SessionTable::new();
    let ch = channel(&mut table, ChannelKind::Gui, Some("alice"));
    let c = HandlerCtx::derive(&table, &ch, 1, CancelToken::never()).unwrap();
    // Mutating the table while the context is alive builds only because the context
    // borrows nothing from it.
    let other = table.open_session(SessionId::new("desk-2").unwrap()).unwrap();
    assert!(!table.end_session(other));
    let moved = std::thread::spawn(move || c.principal().id().as_str().to_string()).join().unwrap();
    assert_eq!(moved, "alice");
}

// ------------------------------------------------------------------ envelopes

#[test]
fn envelope_fixture() {
    let f = fixture("envelopes.json");
    let cases = arr(&f, "cases");
    assert!(cases.len() >= 16);
    for case in cases {
        let case = obj(case);
        let name = s(case, "name");
        let expect = obj(case.get("expect").unwrap());
        let r = Event::from_json(s(case, "json").as_bytes());
        if let Some(CValue::Str(t)) = expect.get("type") {
            let ev = r.unwrap_or_else(|e| panic!("{name}: {e:?}"));
            assert_eq!(ev.type_id().as_str(), t, "{name}");
            assert_eq!(Event::from_value(ev.to_value()), Ok(ev.clone()), "{name}: round trip");
        } else {
            let e = r.expect_err(name);
            assert_eq!(e.code().as_str(), s(expect, "code"), "{name}");
            assert_eq!(e.reason(), s(expect, "reason"), "{name}");
        }
    }
}

#[test]
fn envelope_too_deep_is_input_limit() {
    let mut json = String::from("{\"type\":\"ElementCreated\",\"body\":");
    json.push_str(&"[".repeat(200));
    json.push_str(&"]".repeat(200));
    json.push('}');
    assert_err(&Event::from_json(json.as_bytes()).unwrap_err(), Code::Validation, "input_limit");
}

// ------------------------------------------------------------------ names

#[test]
fn event_type_grammar() {
    for ok in ["ElementCreated", "align.run_321@1", "a", "A".repeat(MAX_EVENT_TYPE_BYTES).as_str()] {
        assert!(EventType::new(ok).is_ok(), "{ok}");
    }
    for bad in ["", "1a", "_a", "a b", "a-b", "é", "A".repeat(MAX_EVENT_TYPE_BYTES + 1).as_str()] {
        assert_err(&EventType::new(bad).unwrap_err(), Code::Validation, "schema");
    }
}

#[test]
fn entity_id_and_setting_key_grammar() {
    assert!(EntityId::new("018f2c3e-0000-7000-8000-000000000001").is_ok());
    assert!(EntityId::new("PLN-1 (imported)").is_ok());
    assert!(EntityId::new("x".repeat(MAX_ENTITY_ID_BYTES).as_str()).is_ok());
    for bad in ["", "a\nb", "\u{7f}", "x".repeat(MAX_ENTITY_ID_BYTES + 1).as_str()] {
        assert_err(&EntityId::new(bad).unwrap_err(), Code::Validation, "schema");
    }
    assert!(SettingKey::new("units.length").is_ok());
    assert!(SettingKey::new("k".repeat(MAX_SETTING_KEY_BYTES).as_str()).is_ok());
    for bad in ["", "Units", ".a", "a.", "a b", "k".repeat(MAX_SETTING_KEY_BYTES + 1).as_str()] {
        assert_err(&SettingKey::new(bad).unwrap_err(), Code::Validation, "schema");
    }
}

// ------------------------------------------------------------------ handler set

fn boxed() -> Box<dyn Handler> {
    Box::new(Fixed(Vec::new()))
}

fn def_named(id: &str, major: u32, effects: &[&str]) -> CommandDef {
    let mut def = effects_fixture().0;
    def.id = id.to_string();
    def.major = major;
    def.effects = effects.iter().map(|e| e.to_string()).collect();
    def
}

#[test]
fn handler_set_refusals() {
    let (def, _) = effects_fixture();
    let mut set = HandlerSet::new();
    assert!(set.is_empty());
    set.register(def.clone(), boxed()).unwrap();
    assert_eq!(set.len(), 1);
    assert!(set.get(ID).is_ok());
    assert_eq!(set.def(ID).unwrap(), &def);
    // Unknown command, unknown major, malformed and non-canonical IDs.
    assert_err(&set.get("test.delete_element@1").err().unwrap(), Code::NotFound, "unknown_command");
    assert_err(&set.get("test.create_element@2").err().unwrap(), Code::NotFound, "unknown_major");
    for bad in ["", "test.create_element", "Test.create@1", "test.create_element@01", "x@123456", "x@-1"] {
        assert_err(&set.get(bad).err().unwrap(), Code::Validation, "schema");
        assert_err(&set.def(bad).unwrap_err(), Code::Validation, "schema");
    }
    // Duplicate keys: a second definition with the same name and major, even with other effects.
    let e = set.register(def_named("test.create_element", 1, &["ElementDeleted"]), boxed()).unwrap_err();
    assert_eq!(
        e,
        BusError::Internal(
            InternalReason::InvariantViolated,
            Detail::new("a handler for test.create_element@1 is already registered")
        )
    );
    // A definition whose ID is not a valid name@major (fields are public, so not checked yet).
    assert_err(&set.register(def_named("Not An Id", 1, &[]), boxed()).unwrap_err(), Code::Internal, "invariant_violated");
    assert_err(&set.register(def_named("big", 123_456, &[]), boxed()).unwrap_err(), Code::Internal, "invariant_violated");
    assert_eq!(set.len(), 1);
    assert_eq!(set.def(ID).unwrap().effects, vec!["ElementCreated".to_string()], "first definition kept");
    assert_eq!(format!("{set:?}"), "{\"test.create_element@1\"}");
}

#[test]
fn unknown_command_never_runs_a_handler() {
    let (def, _) = effects_fixture();
    let (_table, c) = ctx(CancelToken::never());
    let mut set = HandlerSet::new();
    set.register(def, boxed()).unwrap();
    for (id, code, reason) in [
        ("test.delete_element@1", Code::NotFound, "unknown_command"),
        ("test.create_element@3", Code::NotFound, "unknown_major"),
        ("test.create_element@01", Code::Validation, "schema"),
    ] {
        assert_err(&run_handler(&set, id, &MemState::new(), &params(), &c).unwrap_err(), code, reason);
    }
}

#[test]
fn handler_set_is_bounded() {
    let mut set = HandlerSet::new();
    let base = def_named("cmd.c", 1, &[]);
    for i in 0..MAX_HANDLERS {
        let mut d = base.clone();
        d.id = format!("cmd.c{i}");
        set.register(d, boxed()).unwrap();
    }
    assert_err(&set.register(def_named("cmd.extra", 1, &[]), boxed()).unwrap_err(), Code::Internal, "invariant_violated");
    assert_eq!(set.len(), MAX_HANDLERS);
}

// ------------------------------------------------------------------ binding to command and principal

#[test]
fn checked_events_are_bound_to_their_command_and_principal() {
    let created = Event::new(EventType::new("ElementCreated").unwrap(), CValue::Null);
    let deleted = Event::new(EventType::new("ElementDeleted").unwrap(), CValue::Null);
    let mut set = HandlerSet::new();
    set.register(def_named("test.create_element", 1, &["ElementCreated"]), Box::new(Fixed(vec![deleted.clone()])))
        .unwrap();
    set.register(def_named("test.delete_element", 1, &["ElementDeleted"]), Box::new(Fixed(vec![deleted.clone()])))
        .unwrap();
    set.register(def_named("test.create_element", 2, &["ElementCreated"]), Box::new(Fixed(vec![created.clone()])))
        .unwrap();

    let mut table = SessionTable::new();
    let alice = channel(&mut table, ChannelKind::Gui, Some("alice"));
    let bot = channel(&mut table, ChannelKind::Mcp, Some("bot"));
    let ca = HandlerCtx::derive(&table, &alice, 1, CancelToken::never()).unwrap();
    let cb = HandlerCtx::derive(&table, &bot, 1, CancelToken::never()).unwrap();

    // The same events are checked against the definition stored with the handler that ran,
    // never against another command's effects.
    let e = run_handler(&set, "test.create_element@1", &MemState::new(), &params(), &ca).unwrap_err();
    assert_err(&e, Code::UndeclaredEffect, "undeclared_event_type");
    let ok = run_handler(&set, "test.delete_element@1", &MemState::new(), &params(), &ca).unwrap();
    assert_eq!(ok.command().as_str(), "test.delete_element@1");
    assert_eq!(ok.events(), std::slice::from_ref(&deleted));

    // Each result names the principal of the ctx that produced it.
    let a = run_handler(&set, "test.create_element@2", &MemState::new(), &params(), &ca).unwrap();
    let b = run_handler(&set, "test.create_element@2", &MemState::new(), &params(), &cb).unwrap();
    assert_eq!(a.command().as_str(), "test.create_element@2");
    assert_eq!(a.principal(), &ca.principal().reference());
    assert_eq!(b.principal(), &cb.principal().reference());
    assert_eq!(a.principal(), &PrincipalRef { kind: PrincipalKind::Human, id: PrincipalId::new("alice").unwrap() });
    assert_eq!(b.principal(), &PrincipalRef { kind: PrincipalKind::Agent, id: PrincipalId::new("bot").unwrap() });
    assert_ne!(a, b);
    let (command, principal, events) = b.into_parts();
    assert_eq!((command.as_str(), principal.id.as_str(), events), ("test.create_element@2", "bot", vec![created]));
}

// ------------------------------------------------------------------ canonical length

#[test]
fn canonical_len_matches_the_canonical_writer() {
    let values = [
        r#"null"#,
        r#"true"#,
        r#"false"#,
        r#""""#,
        r#""plain""#,
        r#""q\"b\\s\n\t\r\b\f\u0001\u001f é 😀 \u2028""#,
        r#"[]"#,
        r#"{}"#,
        r#"[1,2.5,-0.0,1e300,-1e-7,123456789012345678901234567890]"#,
        r#"{"b":[null,{"z":1,"a":"x"}],"a":{},"é":[[],[[]]]}"#,
    ];
    let mut all: Vec<CValue> = values.iter().map(|v| parse(v.as_bytes()).unwrap()).collect();
    all.extend([
        CValue::I64(0),
        CValue::I64(-7),
        CValue::I64(i64::MIN),
        CValue::I64(i64::MAX),
        CValue::U64(0),
        CValue::U64(u64::MAX),
        CValue::F64Bits(-0.0),
        CValue::F64Bits(1.5),
        CValue::Arr(vec![CValue::I64(-1), CValue::U64(10), CValue::F64Bits(f64::MIN_POSITIVE)]),
    ]);
    for v in &all {
        let want = to_canonical_bytes(v).unwrap().len() as u64;
        assert_eq!(fid_bus::canonical_len(v, u64::MAX), Ok(want), "{v:?}");
        // Counting stops above the limit, with some value above it.
        if want > 0 {
            assert!(fid_bus::canonical_len(v, want - 1).unwrap() > want - 1, "{v:?}");
        }
        assert_eq!(fid_bus::canonical_len(v, want), Ok(want), "{v:?}");
    }
    for bad in [CValue::Num(f64::NAN), CValue::F64Bits(f64::INFINITY), CValue::Arr(vec![CValue::Num(f64::INFINITY)])] {
        assert_eq!(fid_bus::canonical_len(&bad, u64::MAX).unwrap_err().code(), "non_finite_number");
    }
}

#[test]
fn canonical_len_stops_early_on_huge_bodies() {
    // 1,000,000 nulls: counting stops at the first item past the limit.
    let v = CValue::Arr(vec![CValue::Null; 1_000_000]);
    let n = fid_bus::canonical_len(&v, 100).unwrap();
    assert!(n > 100 && n < 1_000_000 * 5, "{n}");
}

// ------------------------------------------------------------------ catalogue

/// This leaf adds no error code or reason: the catalogue digest is the one committed by
/// CORE.BUS.PIPE.ERR.
#[test]
fn catalogue_digest_unchanged() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/core/bus/pipe/err/bus-errors.v1.digest");
    let committed = std::fs::read_to_string(path).unwrap();
    assert_eq!(bus_errors_digest().unwrap().to_string(), committed.trim());
}
