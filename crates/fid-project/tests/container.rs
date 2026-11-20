//! Acceptance criteria of spec node CORE.PRJ.CONT.HIER (AC-1 .. AC-4) at the handler level,
//! with fixtures from `fixtures/core/prj/cont/hier/make_fixtures.py` (independent Python
//! reference) and the empty project `fixtures/core/prj/cont/hier/empty/state.json`.
//!
//! The CLI, the bus commit stage, journal undo and the replay loop are not built yet, so the
//! tests drive [`fid_project::create`] (the body of each command's handler) against an
//! in-memory state view, fold the returned events into that state as the commit stage would,
//! model undo as a head move (the state of the first three entries) and replay as re-running
//! the commands with a [`ReplaySource`] of the recorded observations.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::Code;
use fid_bus::handler::{effects_check, EntityId, EntityRef, Event, HandlerSet, SettingKey, StateView};
use fid_bus::principal::{PrincipalId, PrincipalKind, PrincipalRef};
use fid_canon::{parse, to_canonical_bytes, CValue, Digest};
use fid_project::{
    command_defs, create, created_event, register, Container, ContainerError, ContainerKind, Hierarchy, Label,
    Observation, ObservationSource, Recording, ReplaySource, SharedSource, Uuid, CONTAINER_CREATED, MAX_LABEL_CHARS,
};
use fid_registry::{check_schema, validate, Catalogue, CommandDecl, CommandDef};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/prj/cont/hier/");
const NAMESPACE: Uuid = Uuid::from_u128(0x6ba7_b810_9dad_11d1_80b4_00c0_4fd4_30c8);

type Obj = BTreeMap<String, CValue>;

fn load(name: &str) -> CValue {
    parse(&std::fs::read(format!("{DIR}{name}")).unwrap()).unwrap()
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

fn uuid(s: &str) -> Uuid {
    Uuid::parse(s).unwrap()
}

// ------------------------------------------------------------------ in-memory state

/// The project state as the commit stage would hold it: entities by ID, folded from events.
#[derive(Clone)]
struct MemState {
    entities: BTreeMap<EntityId, (String, CValue)>,
    head: Digest,
}

impl MemState {
    /// The empty project of `empty/state.json`.
    fn empty() -> MemState {
        let f = load("empty/state.json");
        let o = obj(&f);
        assert!(arr(o, "entities").is_empty());
        assert_eq!(o.get("settings"), Some(&CValue::Obj(BTreeMap::new())));
        let head = Digest([0; 32]);
        assert_eq!(head.to_string(), st(o, "head"));
        MemState { entities: BTreeMap::new(), head }
    }

    /// Applies one `ContainerCreated` event.
    fn apply(&mut self, e: &Event) {
        assert_eq!(e.type_id().as_str(), CONTAINER_CREATED);
        let c = Container::from_value(e.body()).unwrap();
        let key = EntityId::new(&c.id().to_string()).unwrap();
        assert!(!self.entities.contains_key(&key), "duplicate entity {key:?}");
        self.entities.insert(key, (c.kind().as_str().to_string(), e.body().clone()));
        self.head = fid_canon::digest(&e.to_value()).unwrap();
    }

    fn insert_raw(&mut self, id: &str, kind: &str, body: CValue) {
        self.entities.insert(EntityId::new(id).unwrap(), (kind.to_string(), body));
    }

    fn ids(&self) -> Vec<String> {
        self.entities.keys().map(|k| k.as_str().to_string()).collect()
    }
}

impl StateView for MemState {
    fn entity(&self, id: &EntityId) -> Option<EntityRef<'_>> {
        self.entities.get_key_value(id).map(|(id, (kind, body))| EntityRef { id, kind, body })
    }
    fn head(&self) -> Digest {
        self.head
    }
    fn setting(&self, _: &SettingKey) -> Option<CValue> {
        None
    }
}

// ------------------------------------------------------------------ fixture helpers

fn principal() -> PrincipalRef {
    let cases = load("cases.json");
    let p = obj(obj(&cases).get("principal").unwrap());
    assert_eq!(st(p, "kind"), "human");
    PrincipalRef { kind: PrincipalKind::Human, id: PrincipalId::new(st(p, "id")).unwrap() }
}

fn kind_of(command: &str) -> ContainerKind {
    ContainerKind::ALL.into_iter().find(|k| format!("{}@1", k.command()) == command).unwrap()
}

/// The observations a case's `observation` field names (none for an imported ID).
fn observations_of(case: &Obj) -> Vec<Observation> {
    match case.get("observation") {
        Some(CValue::Null) => Vec::new(),
        Some(CValue::Obj(o)) => {
            let ms: u64 = st(o, "now_ms").parse().unwrap();
            let bits = u128::from_str_radix(st(o, "random_bits").trim_start_matches("0x"), 16).unwrap();
            vec![Observation::NowMs(ms), Observation::RandomBits(fid_project::Bits::new(74, bits).unwrap())]
        }
        other => panic!("observation: {other:?}"),
    }
}

/// A spare observation pair, so a refused command that drew anything would show it.
fn spare() -> Vec<Observation> {
    vec![Observation::NowMs(1_700_000_000_000), Observation::RandomBits(fid_project::Bits::new(74, 12345).unwrap())]
}

fn run_case(
    state: &MemState,
    command: &str,
    params: &CValue,
    src: &mut dyn ObservationSource,
) -> Result<Event, ContainerError> {
    let c = create(kind_of(command), state, params, &principal(), NAMESPACE, src)?;
    Ok(created_event(&c).unwrap())
}

fn def_of(command: &str) -> CommandDef {
    command_defs().unwrap().into_iter().find(|d| format!("{}@{}", d.id, d.major) == command).unwrap()
}

/// Runs an accepted case: its event must equal the fixture's and pass the effects check.
fn accept(state: &mut MemState, case: &Obj) -> Event {
    let command = st(case, "command");
    let mut src = Recording::new(ReplaySource::new(observations_of(case)));
    let e = run_case(state, command, case.get("params").unwrap(), &mut src).unwrap();
    assert_eq!(canon(&e.to_value()), canon(case.get("event").unwrap()), "{command}");
    assert_eq!(src.observations(), observations_of(case).as_slice());
    assert_eq!(src.inner().remaining(), 0);
    effects_check(&def_of(command), std::slice::from_ref(&e)).unwrap();
    state.apply(&e);
    e
}

/// The state after AC-1, and its events.
fn after_ac1() -> (MemState, Vec<Event>) {
    let cases = load("cases.json");
    let mut s = MemState::empty();
    let events = arr(obj(&cases), "ac1").iter().map(|c| accept(&mut s, obj(c))).collect();
    (s, events)
}

// ------------------------------------------------------------------ AC-1

/// AC-1: part, revision, plan, run on the empty project give four `ContainerCreated`
/// events, one per W2 command, and each entity references its parent.
#[test]
fn ac1_four_creates_build_the_hierarchy() {
    let cases = load("cases.json");
    let steps = arr(obj(&cases), "ac1");
    assert_eq!(steps.len(), 4);
    let (state, events) = after_ac1();
    assert_eq!(events.len(), 4);
    for (step, kind) in steps.iter().zip(ContainerKind::ALL) {
        let o = obj(step);
        let def = def_of(st(o, "command"));
        assert_eq!(def.side_effect.as_str(), "W2");
        assert_eq!(kind_of(st(o, "command")), kind);
    }
    let ids: Vec<Uuid> = steps.iter().map(|c| uuid(st(obj(c), "id"))).collect();
    let h = Hierarchy::new(&state);
    let chain = h.parent_chain(&ids[3]).unwrap();
    let chain_ids: Vec<Uuid> = chain.iter().map(Container::id).collect();
    assert_eq!(chain_ids, [ids[3], ids[2], ids[1], ids[0]]);
    let Some(Container::Run(run)) = h.get(&ids[3]).unwrap() else { panic!("run") };
    assert_eq!((run.plan, run.revision), (ids[2], ids[1]));
    let Some(Container::Plan(plan)) = h.get(&ids[2]).unwrap() else { panic!("plan") };
    assert_eq!(plan.revision, ids[1]);
    assert!(plan.label.is_none());
    let Some(Container::Revision(rev)) = h.get(&ids[1]).unwrap() else { panic!("revision") };
    assert_eq!(rev.part, ids[0]);
    assert_eq!(rev.label.as_untrusted_str(), "B");
    let Some(Container::Part(part)) = h.get(&ids[0]).unwrap() else { panic!("part") };
    assert_eq!(part.label.as_untrusted_str(), "Bracket");
    assert_eq!(part.created_by, principal());
    for id in &ids {
        assert_eq!(id.version(), 7, "new entities get a UUIDv7 (REQ-004)");
    }
}

// ------------------------------------------------------------------ AC-2

/// AC-2: a run naming a plan of r1 with revision r2 is validation/hierarchy, returns no
/// event and draws no observation.
#[test]
fn ac2_plan_of_another_revision_is_hierarchy() {
    let cases = load("cases.json");
    let ac2 = obj(obj(&cases).get("ac2").unwrap());
    let mut s = MemState::empty();
    for c in arr(ac2, "setup") {
        accept(&mut s, obj(c));
    }
    let before = s.ids();
    let mut src = Recording::new(ReplaySource::new(spare()));
    let err = run_case(&s, st(ac2, "command"), ac2.get("params").unwrap(), &mut src).unwrap_err();
    assert_eq!(err.code(), st(ac2, "error"));
    let bus = err.to_bus_error();
    assert_eq!(bus.code().as_str(), st(ac2, "code"));
    assert_eq!(bus.reason(), st(ac2, "reason"));
    assert!(src.observations().is_empty());
    assert_eq!(src.inner().remaining(), 2);
    assert_eq!(s.ids(), before);
}

// ------------------------------------------------------------------ AC-3 and other refusals

/// AC-3 (`plan_names_a_part`) and the other hand-labelled refusals, each against the state
/// after AC-1: the container code, the bus code and reason, and nothing drawn.
#[test]
fn ac3_and_hand_labelled_refusals() {
    let cases = load("cases.json");
    let errors = arr(obj(&cases), "errors");
    assert!(errors.iter().any(|c| st(obj(c), "name") == "plan_names_a_part"));
    let (state, _) = after_ac1();
    for c in errors {
        let o = obj(c);
        let name = st(o, "name");
        let mut src = Recording::new(ReplaySource::new(spare()));
        let err = run_case(&state, st(o, "command"), o.get("params").unwrap(), &mut src)
            .err()
            .unwrap_or_else(|| panic!("{name}: accepted"));
        assert_eq!(err.code(), st(o, "error"), "{name}: {err}");
        let bus = err.to_bus_error();
        assert_eq!(bus.code().as_str(), st(o, "code"), "{name}");
        assert_eq!(bus.reason(), st(o, "reason"), "{name}");
        assert!(src.observations().is_empty(), "{name}: drew an observation");
    }
}

/// More accepted commands against the state after AC-1: imported IDs kept unchanged with
/// nothing drawn, a 256-character label, a second revision and run under the same parents
/// (labels need not be unique).
#[test]
fn more_accepted_commands() {
    let cases = load("cases.json");
    let (state, _) = after_ac1();
    for c in arr(obj(&cases), "more_ok") {
        let o = obj(c);
        let mut s = state.clone();
        let e = accept(&mut s, o);
        assert_eq!(st(obj(e.body()), "id"), st(o, "id"), "{}", st(o, "name"));
    }
}

// ------------------------------------------------------------------ AC-4

/// AC-4: replaying AC-1 with the recorded observations gives the same events and IDs and
/// consumes every observation; after an undo of the last entry (a head move to the third)
/// the Run is gone and the other three remain.
#[test]
fn ac4_replay_is_identical_and_undo_removes_the_run() {
    let cases = load("cases.json");
    let top = obj(&cases);
    let steps = arr(top, "ac1");

    // Live run: record every observation the four commands draw.
    let all: Vec<Observation> = steps.iter().flat_map(|c| observations_of(obj(c))).collect();
    let mut live = Recording::new(ReplaySource::new(all));
    let mut s = MemState::empty();
    let mut live_events = Vec::new();
    for c in steps {
        let o = obj(c);
        let e = run_case(&s, st(o, "command"), o.get("params").unwrap(), &mut live).unwrap();
        s.apply(&e);
        live_events.push(e);
    }
    let recorded: Vec<CValue> = live.observations().iter().map(Observation::to_cvalue).collect();
    let expected = arr(top, "ac1_observations");
    assert_eq!(canon(&CValue::Arr(recorded)), canon(&CValue::Arr(expected.to_vec())));

    // Replay: the journal form of the observations, read back, and nothing else.
    let journal: Vec<Observation> = expected.iter().map(|v| Observation::from_cvalue(v).unwrap()).collect();
    let mut replay = ReplaySource::new(journal);
    let mut r = MemState::empty();
    for (c, live_e) in steps.iter().zip(&live_events) {
        let o = obj(c);
        let e = run_case(&r, st(o, "command"), o.get("params").unwrap(), &mut replay).unwrap();
        assert_eq!(e, *live_e, "replay is IDENTICAL");
        r.apply(&e);
    }
    assert_eq!(replay.remaining(), 0);
    assert_eq!(r.ids(), s.ids());
    assert_eq!(r.head(), s.head());

    // Undo the last entry: the state at the third entry.
    let mut undone = MemState::empty();
    for e in &live_events[..3] {
        undone.apply(e);
    }
    let left: Vec<String> = arr(top, "ac1_after_undo")
        .iter()
        .map(|v| match v {
            CValue::Str(s) => s.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(undone.ids(), left);
    let run_id = uuid(st(obj(&steps[3]), "id"));
    assert_eq!(Hierarchy::new(&undone).get(&run_id).unwrap(), None);
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

// ------------------------------------------------------------------ labels, bodies, state faults

#[test]
fn labels_are_one_to_256_characters() {
    assert_eq!(Label::new(""), Err(ContainerError::Label));
    assert!(Label::new("a").is_ok());
    assert!(Label::new(&"\u{1F600}".repeat(MAX_LABEL_CHARS)).is_ok());
    assert_eq!(Label::new(&"a".repeat(MAX_LABEL_CHARS + 1)), Err(ContainerError::Label));
    // Untrusted text is kept as given, never interpreted.
    let l = Label::new("<b>{{x}}</b>\u{202e}").unwrap();
    assert_eq!(l.as_untrusted_str(), "<b>{{x}}</b>\u{202e}");
}

#[test]
fn container_bodies_round_trip_and_fail_closed() {
    let (_, events) = after_ac1();
    for e in &events {
        let c = Container::from_value(e.body()).unwrap();
        assert_eq!(c.to_value(), *e.body());
    }
    let part = obj(events[0].body()).clone();
    let bad = |f: &dyn Fn(&mut Obj)| {
        let mut o = part.clone();
        f(&mut o);
        assert_eq!(Container::from_value(&CValue::Obj(o)).unwrap_err().code(), "bad_state");
    };
    bad(&|o| {
        o.insert("extra".into(), CValue::Null);
    });
    bad(&|o| {
        o.insert("label".into(), CValue::Null);
    });
    bad(&|o| {
        o.insert("parent".into(), CValue::Str(st(&part, "id").to_string()));
    });
    bad(&|o| {
        o.insert("kind".into(), CValue::Str("project.stage".into()));
    });
    bad(&|o| {
        o.insert("id".into(), CValue::Str(st(&part, "id").to_uppercase()));
    });
    bad(&|o| {
        o.insert("created_by".into(), CValue::Str("alice".into()));
    });
    bad(&|o| {
        o.remove("parent");
    });
}

/// A non-container entity is not a valid parent (bad_reference), but its ID is taken
/// (id_collision); a malformed container entity is internal/invariant_violated.
#[test]
fn non_container_entities_and_malformed_state() {
    let (mut state, _) = after_ac1();
    let other = "0b1c2d3e-4f50-4617-8293-a4b5c6d7e8f9";
    state.insert_raw(other, "feature.plane", CValue::Str("p".into()));
    let p = |pairs: &[(&str, &str)]| {
        CValue::Obj(pairs.iter().map(|(k, v)| (k.to_string(), CValue::Str(v.to_string()))).collect())
    };
    let mut none = ReplaySource::new(Vec::new());
    let e = run_case(&state, "project.create_revision@1", &p(&[("part", other), ("label", "A")]), &mut none);
    assert_eq!(e.unwrap_err().code(), "bad_reference");
    let e = run_case(&state, "project.create_part@1", &p(&[("id", other), ("label", "A")]), &mut none);
    assert_eq!(e.unwrap_err(), ContainerError::IdCollision(uuid(other)));

    let broken = "1b1c2d3e-4f50-4617-8293-a4b5c6d7e8f9";
    state.insert_raw(broken, "project.part", CValue::Obj(BTreeMap::new()));
    let e =
        run_case(&state, "project.create_revision@1", &p(&[("part", broken), ("label", "A")]), &mut none).unwrap_err();
    assert_eq!(e.code(), "bad_state");
    let bus = e.to_bus_error();
    assert_eq!((bus.code(), bus.reason()), (Code::Internal, "invariant_violated"));
}

/// A drawn UUIDv7 that the project already holds fails closed with id_collision; the ID
/// service's own faults (here an exhausted replay log) are internal/handler_failed.
#[test]
fn drawn_id_collision_and_id_service_faults() {
    let cases = load("cases.json");
    let first = obj(&arr(obj(&cases), "ac1")[0]);
    let (state, _) = after_ac1();
    let mut again = ReplaySource::new(observations_of(first));
    let err = run_case(&state, "project.create_part@1", first.get("params").unwrap(), &mut again).unwrap_err();
    assert_eq!(err, ContainerError::IdCollision(uuid(st(first, "id"))));
    let mut empty = ReplaySource::new(Vec::new());
    let err = run_case(&state, "project.create_part@1", first.get("params").unwrap(), &mut empty).unwrap_err();
    assert_eq!(err.code(), "replay_exhausted");
    let bus = err.to_bus_error();
    assert_eq!((bus.code(), bus.reason()), (Code::Internal, "handler_failed"));
}
