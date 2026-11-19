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
    Channel, ChannelKind, Identity, PrincipalId, PrincipalKind, SessionId, SessionKey, SessionTable,
};
use fid_bus::{bus_errors_digest, BusError, Code, Detail};
use fid_canon::{parse, CValue, Digest};
use fid_registry::CommandDef;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

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

fn obj<'a>(v: &'a CValue) -> &'a Obj {
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
fn commit(journal: &mut Vec<Event>, r: Result<fid_bus::CheckedEvents, BusError>) -> Result<(), BusError> {
    journal.extend(r?.into_events());
    Ok(())
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
    ValidParams::from_validated(CValue::Obj(Obj::new()))
}

fn assert_err(e: &BusError, code: Code, reason: &str) {
    assert_eq!((e.code(), e.reason()), (code, reason), "{e:?}");
}

// ------------------------------------------------------------------ AC-1

/// AC-1 is the `compile_fail,E0053` doctest built from this file; this guards that the file
/// still asks for `&mut` state and is wired into the doctest.
#[test]
fn ac1_compile_fail_file_asks_for_mut_state() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let case = std::fs::read_to_string(dir.join("tests/compile_fail/mut_state.rs")).unwrap();
    assert!(case.contains("view: &mut dyn StateView"));
    let src = std::fs::read_to_string(dir.join("src/handler.rs")).unwrap();
    assert!(src.contains("#[doc = \"```compile_fail,E0053\"]"));
    assert!(src.contains("include_str!(\"../tests/compile_fail/mut_state.rs\")"));
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
        let r = commit(&mut journal, run_handler(&def, &Fixed(events.clone()), &view, &params(), &c));
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
    let r = commit(&mut journal, run_handler(&def, &Fixed(many(MAX_EVENTS + 1)), &view, &params(), &c));
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
    let checked = run_handler(&def, &Fixed(events.clone()), &MemState::new(), &params(), &c).unwrap();
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
        (HandlerError::Cancelled, BusError::Cancelled(CancelledReason::Requested, Detail::new("handler stopped on cancel"))),
        (
            HandlerError::Failed(Detail::new("disk")),
            BusError::Internal(InternalReason::HandlerFailed, Detail::new("disk")),
        ),
    ];
    for (handler_error, want) in cases {
        let mut journal = Vec::new();
        let r = commit(&mut journal, run_handler(&def, &Failing(handler_error), &view, &params(), &c));
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
    let r = commit(&mut journal, run_handler(&def, &CancelsDuring(source), &MemState::new(), &params(), &c));
    assert_err(&r.unwrap_err(), Code::Cancelled, "requested");
    assert!(journal.is_empty());
}

// ------------------------------------------------------------------ principal and borrows

/// Records the principal, seed and state it saw.
struct Observer(Mutex<Vec<String>>);

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
    let p = ValidParams::from_validated(
        parse(br#"{"principal":{"kind":"human","id":"mallory"},"as":"mallory"}"#).unwrap(),
    );
    let h = Observer(Mutex::new(Vec::new()));
    run_handler(&def, &h, &MemState::new(), &p, &c).unwrap();
    assert_eq!(c.principal().kind(), PrincipalKind::Agent);
    let seen = h.0.lock().unwrap().clone();
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

#[test]
fn handler_set_refusals() {
    let mut set = HandlerSet::new();
    assert!(set.is_empty());
    set.register("test.create_element@1", boxed()).unwrap();
    assert_eq!(set.len(), 1);
    assert!(set.get("test.create_element@1").is_ok());
    // Unknown command, unknown major, malformed and non-canonical IDs.
    assert_err(&set.get("test.delete_element@1").err().unwrap(), Code::NotFound, "unknown_command");
    assert_err(&set.get("test.create_element@2").err().unwrap(), Code::NotFound, "unknown_major");
    for bad in ["", "test.create_element", "Test.create@1", "test.create_element@01", "x@123456", "x@-1"] {
        assert_err(&set.get(bad).err().unwrap(), Code::Validation, "schema");
    }
    // Duplicate keys: the same ID, or the same (name, major) spelled with a leading zero.
    let e = set.register("test.create_element@1", boxed()).unwrap_err();
    assert_eq!(
        e,
        BusError::Internal(
            InternalReason::InvariantViolated,
            Detail::new("a handler for test.create_element@1 is already registered")
        )
    );
    assert_err(&set.register("test.create_element@01", boxed()).unwrap_err(), Code::Internal, "invariant_violated");
    assert_err(&set.register("not an id", boxed()).unwrap_err(), Code::Internal, "invariant_violated");
    assert_eq!(set.len(), 1);
    assert_eq!(format!("{set:?}"), "{\"test.create_element@1\"}");
}

#[test]
fn handler_set_is_bounded() {
    let mut set = HandlerSet::new();
    for i in 0..MAX_HANDLERS {
        set.register(&format!("cmd.c{i}@1"), boxed()).unwrap();
    }
    assert_err(&set.register("cmd.extra@1", boxed()).unwrap_err(), Code::Internal, "invariant_violated");
    assert_eq!(set.len(), MAX_HANDLERS);
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
