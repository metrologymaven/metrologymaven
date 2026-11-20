//! The bus side of spec node CORE.PRJ.CONT.HIER: registry definitions (REQ-005), handler
//! registration, the bus state view adapter, `created_by` from the call's principal and the
//! bus error of every hand-labelled refusal (AC-2, AC-3), with fixtures from
//! `fixtures/core/prj/cont/hier/make_fixtures.py` (independent Python reference). The
//! container rules themselves are tested in `fid-project`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::Code;
use fid_bus::handler::{
    effects_check, CancelToken, EntityId, EntityRef, HandlerCtx, HandlerSet, SettingKey, StateView,
};
use fid_bus::principal::{Channel, ChannelKind, Identity, PrincipalId, SessionId, SessionKey, SessionTable};
use fid_canon::{parse, to_canonical_bytes, CValue, Digest};
use fid_project::{
    Bits, Container, ContainerError, ContainerKind, CreatedBy, IdError, Observation, Recording, ReplaySource, Uuid,
    CONTAINER_CREATED,
};
use fid_project_cmd::{command_defs, created_by, register, run_create, to_bus_error, SharedSource};
use fid_registry::{check_schema, validate, Catalogue, CommandDecl, CommandDef};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/prj/cont/hier/");
const NAMESPACE: Uuid = Uuid::from_u128(0x6ba7_b810_9dad_11d1_80b4_00c0_4fd4_30c8);

type Obj = BTreeMap<String, CValue>;

fn cases() -> CValue {
    parse(&std::fs::read(format!("{DIR}cases.json")).unwrap()).unwrap()
}

fn obj(v: &CValue) -> &Obj {
    match v {
        CValue::Obj(o) => o,
        other => panic!("expected object, got {other:?}"),
    }
}

fn arr<'a>(o: &'a Obj, k: &str) -> &'a [CValue] {
    match o.get(k) {
        Some(CValue::Arr(a)) => a,
        other => panic!("{k}: expected array, got {other:?}"),
    }
}

fn st<'a>(o: &'a Obj, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("{k}: expected string, got {other:?}"),
    }
}

fn canon(v: &CValue) -> Vec<u8> {
    to_canonical_bytes(v).unwrap()
}

// ------------------------------------------------------------------ bus state view

/// A bus state view holding committed entities, folded from `ContainerCreated` events.
#[derive(Clone)]
struct MemState {
    entities: BTreeMap<EntityId, (String, CValue)>,
}

impl MemState {
    fn new() -> MemState {
        MemState { entities: BTreeMap::new() }
    }

    fn apply(&mut self, e: &fid_bus::handler::Event) {
        assert_eq!(e.type_id().as_str(), CONTAINER_CREATED);
        let c = Container::from_value(e.body()).unwrap();
        let key = EntityId::new(&c.id().to_string()).unwrap();
        assert!(self.entities.insert(key, (c.kind().as_str().to_string(), e.body().clone())).is_none());
    }
}

impl StateView for MemState {
    fn entity(&self, id: &EntityId) -> Option<EntityRef<'_>> {
        self.entities.get_key_value(id).map(|(id, (kind, body))| EntityRef { id, kind, body })
    }
    fn head(&self) -> Digest {
        Digest([0; 32])
    }
    fn setting(&self, _: &SettingKey) -> Option<CValue> {
        None
    }
}

fn principal() -> CreatedBy {
    let c = cases();
    let p = obj(obj(&c).get("principal").unwrap());
    CreatedBy::new(st(p, "kind"), st(p, "id")).unwrap()
}

fn kind_of(command: &str) -> ContainerKind {
    ContainerKind::ALL.into_iter().find(|k| format!("{}@1", k.command()) == command).unwrap()
}

fn def_of(command: &str) -> CommandDef {
    command_defs().unwrap().into_iter().find(|d| format!("{}@{}", d.id, d.major) == command).unwrap()
}

fn observations_of(case: &Obj) -> Vec<Observation> {
    match case.get("observation") {
        Some(CValue::Null) => Vec::new(),
        Some(CValue::Obj(o)) => {
            let ms: u64 = st(o, "now_ms").parse().unwrap();
            let bits = u128::from_str_radix(st(o, "random_bits").trim_start_matches("0x"), 16).unwrap();
            vec![Observation::NowMs(ms), Observation::RandomBits(Bits::new(74, bits).unwrap())]
        }
        other => panic!("observation: {other:?}"),
    }
}

/// Runs an accepted case through the handler body on the bus view; the event must equal the
/// fixture's and pass the effects check of its W2 definition.
fn accept(state: &mut MemState, case: &Obj) {
    let command = st(case, "command");
    let mut src = ReplaySource::new(observations_of(case));
    let e =
        run_create(kind_of(command), state, case.get("params").unwrap(), &principal(), NAMESPACE, &mut src).unwrap();
    assert_eq!(canon(&e.to_value()), canon(case.get("event").unwrap()), "{command}");
    effects_check(&def_of(command), std::slice::from_ref(&e)).unwrap();
    state.apply(&e);
}

fn after_ac1() -> MemState {
    let c = cases();
    let mut s = MemState::new();
    for step in arr(obj(&c), "ac1") {
        accept(&mut s, obj(step));
    }
    s
}

// ------------------------------------------------------------------ AC-1 on the bus view

/// AC-1 through the bus view: four `ContainerCreated` events, each declared by its W2
/// command, and the fixture's accepted extras.
#[test]
fn ac1_events_pass_the_effects_check() {
    let c = cases();
    let state = after_ac1();
    assert_eq!(state.entities.len(), 4);
    for case in arr(obj(&c), "more_ok") {
        accept(&mut state.clone(), obj(case));
    }
}

// ------------------------------------------------------------------ bus errors

/// AC-2 and AC-3 and every hand-labelled refusal: the bus code and reason, nothing drawn.
/// The two `id_collision` cases are `conflict/id_collision` (bus error catalogue v2).
#[test]
fn refusals_map_to_their_bus_errors() {
    let c = cases();
    let top = obj(&c);
    let state = after_ac1();
    let mut checked = 0;
    for case in arr(top, "errors") {
        let o = obj(case);
        let name = st(o, "name");
        let mut src = Recording::new(ReplaySource::new(Vec::new()));
        let e =
            run_create(kind_of(st(o, "command")), &state, o.get("params").unwrap(), &principal(), NAMESPACE, &mut src)
                .err()
                .unwrap_or_else(|| panic!("{name}: accepted"));
        assert_eq!(e.code().as_str(), st(o, "code"), "{name}");
        assert_eq!(e.reason(), st(o, "reason"), "{name}");
        assert!(src.observations().is_empty(), "{name}");
        checked += 1;
    }
    assert_eq!(checked, arr(top, "errors").len());

    let ac2 = obj(top.get("ac2").unwrap());
    let mut s = MemState::new();
    for step in arr(ac2, "setup") {
        accept(&mut s, obj(step));
    }
    let mut none = ReplaySource::new(Vec::new());
    let e = run_create(kind_of(st(ac2, "command")), &s, ac2.get("params").unwrap(), &principal(), NAMESPACE, &mut none)
        .unwrap_err();
    assert_eq!((e.code().as_str(), e.reason()), (st(ac2, "code"), st(ac2, "reason")));
}

#[test]
fn container_errors_map_to_the_catalogue() {
    let id = Uuid::from_u128(1);
    let cases = [
        (ContainerError::Schema("x".into()), Code::Validation, "schema"),
        (ContainerError::Label, Code::Validation, "label"),
        (ContainerError::BadReference("x".into()), Code::Validation, "bad_reference"),
        (ContainerError::Hierarchy("x".into()), Code::Validation, "hierarchy"),
        (ContainerError::IdCollision(id), Code::Conflict, "id_collision"),
        (ContainerError::Id(IdError::ReplayExhausted), Code::Internal, "handler_failed"),
        (ContainerError::BadState("x".into()), Code::Internal, "invariant_violated"),
    ];
    for (err, code, reason) in cases {
        let b = to_bus_error(&err);
        assert_eq!((b.code(), b.reason()), (code, reason), "{err}");
    }
}

// ------------------------------------------------------------------ principal

struct TestChannel {
    session: SessionKey,
}

impl Channel for TestChannel {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Gui
    }
    fn authenticated_identity(&self) -> Option<Identity> {
        Some(Identity::new(PrincipalId::new("alice").unwrap()))
    }
    fn session(&self) -> &SessionKey {
        &self.session
    }
}

/// `created_by` is the call's principal kind and identity: a GUI call by `alice` is the
/// fixture principal `{id: alice, kind: human}`.
#[test]
fn created_by_comes_from_the_principal() {
    let mut table = SessionTable::new();
    let session = table.open_session(SessionId::new("desk-1").unwrap()).unwrap();
    let ch = TestChannel { session };
    let ctx = HandlerCtx::derive(&table, &ch, 0, CancelToken::never()).unwrap();
    assert_eq!(created_by(ctx.principal()).unwrap(), principal());
}

// ------------------------------------------------------------------ definitions and registration

/// REQ-005: four registry commands, W2, D0, undo head_move, effect `ContainerCreated`, on
/// CLI and MCP; their schemas pass the keyword checker and the definitions pass VAL.
#[test]
fn command_definitions() {
    let defs = command_defs().unwrap();
    let ids: Vec<String> = defs.iter().map(|d| format!("{}@{}", d.id, d.major)).collect();
    assert_eq!(
        ids,
        ["project.create_part@1", "project.create_revision@1", "project.create_plan@1", "project.create_run@1"]
    );
    let catalogue = Catalogue::geo_frame().unwrap();
    for d in &defs {
        let v = d.to_value();
        let o = obj(&v);
        assert_eq!(st(o, "side_effect"), "W2");
        assert_eq!(st(o, "determinism"), "D0");
        assert_eq!(st(o, "undo"), "head_move");
        assert_eq!(d.effects, [CONTAINER_CREATED]);
        let exposure: Vec<&str> = arr(o, "exposure")
            .iter()
            .map(|x| match x {
                CValue::Str(s) => s.as_str(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(exposure, ["cli", "mcp"]);
        assert_eq!(check_schema(&d.params_schema, &catalogue), vec![]);
        assert_eq!(check_schema(&d.result_schema, &catalogue), vec![]);
    }
    let decls: Vec<CommandDecl> = defs.iter().map(CommandDecl::from_def).collect();
    assert_eq!(validate(&decls), vec![]);
}

/// The four handlers register under their command IDs with their own definitions; a second
/// registration of the same commands is refused.
#[test]
fn handlers_register_on_the_bus() {
    let source: SharedSource = Arc::new(Mutex::new(Recording::new(ReplaySource::new(Vec::new()))));
    let mut set = HandlerSet::new();
    register(&mut set, NAMESPACE, &source).unwrap();
    assert_eq!(set.len(), 4);
    for k in ContainerKind::ALL {
        let id = format!("{}@1", k.command());
        assert!(set.get(&id).is_ok());
        assert_eq!(set.def(&id).unwrap().id, k.command());
    }
    let err = register(&mut set, NAMESPACE, &source).unwrap_err();
    assert_eq!(err.code(), Code::Internal);
    assert_eq!(set.len(), 4);
}
