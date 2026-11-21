//! The bus side of spec node CORE.PRJ.STAGE.MODEL: registry definitions (REQ-007), the
//! published schemas against the fixtures' real parameters and results, handler
//! registration, the bus error (catalogue v3) of every hand-labelled refusal, the stage
//! entities in the bus state view, and AC-5 (replay of the handlers). Fixtures from
//! `fixtures/core/prj/stage/model/make_fixtures.py` (independent Python reference). The
//! model rules are tested in `fid-project`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Code, Detail, InternalReason, ValidationReason};
use fid_bus::handler::{effects_check, EntityId, EntityRef, Event, HandlerSet, SettingKey, StateView};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse, to_canonical_bytes, CValue, Digest};
use fid_project::{
    Bits, ElementTypeDef, ElementTypeRegistry, Observation, ObservationSource, Recording, ReplaySource, Tree,
    TreeError, Uuid, STAGE_KIND, STAGE_LIST_KIND,
};
use fid_project_cmd::stage::{command_defs, list_result, register, result_of, run_stage, StageOp};
use fid_project_cmd::tree::{to_bus_error, to_replay_error, TreeState};
use fid_project_cmd::SharedSource;
use fid_registry::{check_schema, validate, Catalogue, CommandDecl, CommandDef};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/prj/stage/model/");

type Obj = BTreeMap<String, CValue>;

fn read(name: &str) -> Vec<u8> {
    std::fs::read(format!("{DIR}{name}")).unwrap()
}

fn obj(v: &CValue) -> &Obj {
    match v {
        CValue::Obj(o) => o,
        other => panic!("expected object, got {other:?}"),
    }
}

fn get<'a>(o: &'a Obj, k: &str) -> &'a CValue {
    o.get(k).unwrap_or_else(|| panic!("missing {k}"))
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

fn cases() -> CValue {
    parse(&read("cases.json")).unwrap()
}

fn setup() -> CValue {
    parse(&read("p100/setup.json")).unwrap()
}

fn namespace() -> Uuid {
    Uuid::parse(st(obj(&setup()), "namespace")).unwrap()
}

fn event(v: &CValue) -> Event {
    Event::from_value(v.clone()).unwrap()
}

fn empty_params() -> CValue {
    CValue::Obj(Obj::new())
}

// ------------------------------------------------------------------ bus state view

/// The project as the bus sees it: the element tree (with its stages) and the containers,
/// which the tree takes through an empty domain translation of `ContainerCreated`.
#[derive(Clone)]
struct Project {
    tree: TreeState,
    containers: BTreeMap<EntityId, (String, CValue)>,
}

impl StateView for Project {
    fn entity(&self, id: &EntityId) -> Option<EntityRef<'_>> {
        if let Some((id, (kind, body))) = self.containers.get_key_value(id) {
            return Some(EntityRef { id, kind, body });
        }
        self.tree.entity(id)
    }
    fn head(&self) -> Digest {
        self.tree.head()
    }
    fn setting(&self, key: &SettingKey) -> Option<CValue> {
        self.tree.setting(key)
    }
}

impl Project {
    fn commit(&mut self, events: &[Event]) {
        self.tree.apply_commit(events).unwrap();
        for e in events {
            if e.type_id().as_str() == "ContainerCreated" {
                let b = obj(e.body());
                let key = EntityId::new(st(b, "id")).unwrap();
                self.containers.insert(key, (st(b, "kind").to_string(), e.body().clone()));
            }
        }
    }

    fn digest(&self) -> String {
        self.tree.tree().state_digest().unwrap().as_str().to_string()
    }
}

fn p100() -> Project {
    let mut reg = ElementTypeRegistry::new();
    let mut schema = Obj::new();
    schema.insert("type".into(), CValue::Str("object".into()));
    reg.register(ElementTypeDef::new("test.point", CValue::Obj(schema), "point").unwrap()).unwrap();
    reg.register_event("ContainerCreated", |_| Ok(Vec::new())).unwrap();
    let tree = Tree::new(namespace(), Arc::new(reg)).unwrap();
    let mut p = Project { tree: TreeState::new(tree, Digest([0; 32])).unwrap(), containers: BTreeMap::new() };
    for line in String::from_utf8(read("p100/events.jsonl")).unwrap().lines() {
        p.commit(&[event(&parse(line.as_bytes()).unwrap())]);
    }
    let expected = String::from_utf8(read("p100/expected_digest.txt")).unwrap();
    assert_eq!(p.digest(), expected.trim_end());
    p
}

fn op_of(command: &str) -> StageOp {
    StageOp::ALL.into_iter().find(|o| format!("{}@1", o.command()) == command).unwrap()
}

fn def_of(command: &str) -> CommandDef {
    command_defs().unwrap().into_iter().find(|d| format!("{}@{}", d.id, d.major) == command).unwrap()
}

/// Runs an accepted case through the handler body on the bus view: one event, equal to the
/// fixture's, passing the effects check of its definition, with the fixture's result.
fn accept(p: &mut Project, case: &Obj, source: &mut dyn ObservationSource) -> Event {
    let command = st(case, "command");
    let events = run_stage(op_of(command), &*p, get(case, "params"), namespace(), source).unwrap();
    assert_eq!(events.len(), 1, "{command}");
    let e = events.into_iter().next().unwrap();
    assert_eq!(canon(&e.to_value()), canon(get(case, "event")), "{command}");
    effects_check(&def_of(command), std::slice::from_ref(&e)).unwrap();
    assert_eq!(canon(&result_of(&e).unwrap()), canon(get(case, "result")), "{command}");
    p.commit(std::slice::from_ref(&e));
    e
}

fn refuse(p: &Project, case: &Obj) {
    let name = st(case, "name");
    let mut src = Recording::new(ReplaySource::new(Vec::new()));
    let before = p.digest();
    let e = run_stage(op_of(st(case, "command")), p, get(case, "params"), namespace(), &mut src)
        .err()
        .unwrap_or_else(|| panic!("{name}: accepted"));
    assert_eq!((e.code().as_str(), e.reason()), (st(case, "code"), st(case, "reason")), "{name}");
    assert!(src.observations().is_empty(), "{name}: drew an ID");
    assert_eq!(p.digest(), before, "{name}");
}

fn none() -> ReplaySource {
    ReplaySource::new(Vec::new())
}

/// The project after AC-1, AC-3 (S2 with data, removed) and the AC-4 setup, with every
/// accepted case and list case seen on the way.
struct Walk {
    after_ac1: Project,
    after_ac3: Project,
    after_ac4_setup: Project,
    accepted: Vec<Obj>,
    lists: Vec<Obj>,
}

fn walk() -> Walk {
    let c = cases();
    let top = obj(&c);
    let mut p = p100();
    let mut accepted = vec![obj(get(top, "ac1")).clone()];
    accept(&mut p, obj(get(top, "ac1")), &mut none());
    let after_ac1 = p.clone();
    let ac3 = obj(get(top, "ac3"));
    for e in arr(ac3, "setup") {
        p.commit(&[event(e)]);
    }
    assert_eq!(p.digest(), st(ac3, "digest_before"));
    accept(&mut p, obj(get(ac3, "remove")), &mut none());
    accepted.push(obj(get(ac3, "remove")).clone());
    assert_eq!(p.digest(), st(ac3, "digest_after"));
    let after_ac3 = p.clone();
    let ac4 = obj(get(top, "ac4"));
    for step in arr(ac4, "setup") {
        accept(&mut p, obj(step), &mut none());
        accepted.push(obj(step).clone());
    }
    assert_eq!(p.digest(), st(ac4, "digest_before"));
    let after_ac4_setup = p.clone();
    accept(&mut p, obj(get(ac4, "ok")), &mut none());
    accepted.push(obj(get(ac4, "ok")).clone());
    assert_eq!(p.digest(), st(ac4, "digest_after"));
    let mut lists = Vec::new();
    for key in ["list", "list_removed"] {
        let case = obj(get(ac3, key));
        assert_eq!(
            canon(&list_result(&after_ac3, get(case, "params"), namespace()).unwrap()),
            canon(get(case, "result"))
        );
        lists.push(case.clone());
    }
    for key in ["list", "list_run2", "list_all"] {
        let case = obj(get(ac4, key));
        assert_eq!(canon(&list_result(&p, get(case, "params"), namespace()).unwrap()), canon(get(case, "result")));
        lists.push(case.clone());
    }
    let mut q = after_ac1.clone();
    for case in arr(top, "more_ok") {
        accept(&mut q, obj(case), &mut none());
        accepted.push(obj(case).clone());
    }
    Walk { after_ac1, after_ac3, after_ac4_setup, accepted, lists }
}

// ------------------------------------------------------------------ AC-1 .. AC-4 on the bus

/// AC-1, AC-3 and AC-4 through the handler bodies on the bus view: each accepted command's
/// event, effects check, result and resulting state digest match the reference; list results
/// match too (`walk` asserts them).
#[test]
fn accepted_commands_on_the_bus_view() {
    let w = walk();
    // AC-1: every element has an S2 slot, read back through the bus view.
    let c = cases();
    let ac1 = obj(get(obj(&c), "ac1"));
    assert_eq!(w.after_ac1.digest(), st(ac1, "digest_after"));
    assert_eq!(w.accepted.len(), 7);
    // A list with no explicit filter is R0: no event.
    let events = run_stage(StageOp::List, &w.after_ac3, &empty_params(), namespace(), &mut none()).unwrap();
    assert!(events.is_empty());
}

/// The drawn ID of a stage added without `id`: the UUIDv7 of the recorded observation.
#[test]
fn drawn_id_on_the_bus_view() {
    let c = cases();
    let d = obj(get(obj(&c), "drawn"));
    let o = obj(get(d, "observation"));
    let ms: u64 = st(o, "now_ms").parse().unwrap();
    let bits = u128::from_str_radix(st(o, "random_bits").trim_start_matches("0x"), 16).unwrap();
    let mut src =
        ReplaySource::new(vec![Observation::NowMs(ms), Observation::RandomBits(Bits::new(74, bits).unwrap())]);
    let mut p = p100();
    accept(&mut p, d, &mut src);
    assert_eq!(p.digest(), st(d, "digest_after"));
}

/// Every hand-labelled refusal maps to its catalogue v3 code and reason, draws nothing and
/// changes nothing (AC-3 `stage_active`, AC-4 `validation`).
#[test]
fn refusals_map_to_their_bus_errors() {
    let c = cases();
    let top = obj(&c);
    let w = walk();
    let mut n = 0;
    for e in arr(obj(get(top, "ac3")), "errors") {
        refuse(&w.after_ac3, obj(e));
        n += 1;
    }
    for e in arr(obj(get(top, "ac4")), "errors") {
        refuse(&w.after_ac4_setup, obj(e));
        n += 1;
    }
    for e in arr(top, "add_errors") {
        refuse(&w.after_ac1, obj(e));
        n += 1;
    }
    assert_eq!(n, 38);
    let ac3 = obj(get(top, "ac3"));
    let active = obj(&arr(ac3, "errors")[0]);
    assert_eq!((st(active, "code"), st(active, "reason")), ("precondition", "stage_active"));
}

/// A journaled stage event the state refuses aborts its commit with the mapped bus error and
/// leaves the view unchanged; on replay the same refusal is `internal/invariant_violated`.
#[test]
fn refused_stage_events_on_the_tree_state() {
    let w = walk();
    let mut p = w.after_ac3.clone();
    let s1 = p.tree.tree().active_stage();
    let rm = Event::from_value(
        parse(format!("{{\"body\":{{\"stage\":\"{s1}\"}},\"type\":\"StageRemoved\"}}").as_bytes()).unwrap(),
    )
    .unwrap();
    let before = p.digest();
    let e = p.tree.apply_commit(std::slice::from_ref(&rm)).unwrap_err();
    assert_eq!((e.code(), e.reason()), (Code::Precondition, "stage_active"));
    let e = p.tree.replay_commit(std::slice::from_ref(&rm)).unwrap_err();
    assert_eq!((e.code(), e.reason()), (Code::Internal, InternalReason::InvariantViolated.as_str()));
    assert_eq!(p.digest(), before);
}

#[test]
fn stage_errors_map_to_the_catalogue() {
    let id = Uuid::from_u128(1);
    let cases: Vec<(TreeError, Code, &str, Code, &str)> = vec![
        (TreeError::Label, Code::Validation, "label", Code::Validation, "label"),
        (TreeError::BadOrder("x".into()), Code::Validation, "bad_stage", Code::Internal, "invariant_violated"),
        (
            TreeError::StageNotFound(id.to_string()),
            Code::NotFound,
            "unknown_entity",
            Code::Internal,
            "invariant_violated",
        ),
        (TreeError::AmbiguousStage("x".into()), Code::Validation, "bad_stage", Code::Validation, "bad_stage"),
        (TreeError::StageActive(id), Code::Precondition, "stage_active", Code::Internal, "invariant_violated"),
    ];
    for (err, code, reason, rcode, rreason) in cases {
        let b = to_bus_error(&err);
        assert_eq!((b.code(), b.reason()), (code, reason), "{err}");
        let r = to_replay_error(&err);
        assert_eq!((r.code(), r.reason()), (rcode, rreason), "{err}");
    }
}

// ------------------------------------------------------------------ state view

/// The bus view serves every stage (kind `project.stage`, body the stage wire form) and the
/// stage list, including stages added by a commit after the view was built.
#[test]
fn state_view_serves_stages_and_the_stage_list() {
    let w = walk();
    let s = setup();
    let so = obj(&s);
    let p = &w.after_ac3;
    let list_id = EntityId::new(st(so, "stage_list")).unwrap();
    let l = p.entity(&list_id).unwrap();
    assert_eq!(l.kind, STAGE_LIST_KIND);
    for s in arr(obj(l.body), "stages") {
        let CValue::Str(id) = s else { panic!("{s:?}") };
        let e = p.entity(&EntityId::new(id).unwrap()).unwrap();
        assert_eq!(e.kind, STAGE_KIND);
        assert_eq!(st(obj(e.body), "id"), id);
    }
    assert_eq!(arr(obj(l.body), "stages").len(), 2);
    // A view built from a tree that already has stages serves them too.
    let rebuilt = TreeState::new(p.tree.tree().clone(), Digest([1; 32])).unwrap();
    assert_eq!(rebuilt.entity(&list_id).unwrap().kind, STAGE_LIST_KIND);
    assert_eq!(rebuilt.entity(&EntityId::new(st(so, "s1")).unwrap()).unwrap().kind, STAGE_KIND);
}

// ------------------------------------------------------------------ AC-5 replay

/// AC-5: the commands of AC-1 (one imported and one drawn ID) and AC-3 are run live with a
/// recording source, then replayed from p100 with a replay source of the recorded
/// observations: identical events and an identical final state digest (exact replay
/// outcome IDENTICAL). The journal events of AC-3's setup are applied as recorded.
#[test]
fn ac5_replay_is_identical() {
    let c = cases();
    let top = obj(&c);
    let d = obj(get(top, "drawn"));
    let o = obj(get(d, "observation"));
    let ms: u64 = st(o, "now_ms").parse().unwrap();
    let bits = u128::from_str_radix(st(o, "random_bits").trim_start_matches("0x"), 16).unwrap();
    let live_obs = vec![Observation::NowMs(ms), Observation::RandomBits(Bits::new(74, bits).unwrap())];
    let ac3 = obj(get(top, "ac3"));
    let steps: Vec<(StageOp, CValue)> = vec![
        (StageOp::Add, get(obj(get(top, "ac1")), "params").clone()),
        (StageOp::Add, get(d, "params").clone()),
        (StageOp::Remove, get(obj(get(ac3, "remove")), "params").clone()),
    ];
    let run = |source: &mut dyn ObservationSource| {
        let mut p = p100();
        let mut journal = Vec::new();
        for (i, (op, params)) in steps.iter().enumerate() {
            if i == 2 {
                for e in arr(ac3, "setup") {
                    p.commit(&[event(e)]);
                    journal.push(canon(e));
                }
            }
            let events = run_stage(*op, &p, params, namespace(), source).unwrap();
            for e in &events {
                journal.push(canon(&e.to_value()));
            }
            p.commit(&events);
        }
        (journal, p.digest())
    };
    let mut live = Recording::new(ReplaySource::new(live_obs));
    let (j1, d1) = run(&mut live);
    let recorded = live.observations().to_vec();
    assert_eq!(recorded.len(), 2);
    let mut replay = ReplaySource::new(recorded);
    let (j2, d2) = run(&mut replay);
    assert_eq!(j1, j2);
    assert_eq!(d1, d2);
}

// ------------------------------------------------------------------ definitions

/// REQ-007: add, remove and reorder are W2 (undo head_move, one declared effect each), list
/// is R0; all on CLI and MCP; schemas pass the keyword checker and the definitions pass VAL.
#[test]
fn command_definitions() {
    let defs = command_defs().unwrap();
    let ids: Vec<String> = defs.iter().map(|d| format!("{}@{}", d.id, d.major)).collect();
    assert_eq!(
        ids,
        [
            "project.add_stage@1",
            "project.remove_stage@1",
            "project.reorder_stages@1",
            "project.list_stages@1",
            "project.set_active_stage@1"
        ]
    );
    let want = [
        ("W2", "head_move", vec!["StageAdded"]),
        ("W2", "head_move", vec!["StageRemoved"]),
        ("W2", "head_move", vec!["StagesReordered"]),
        ("R0", "not_undoable", vec![]),
        ("W2", "head_move", vec!["ActiveStageSet"]),
    ];
    let catalogue = Catalogue::geo_frame().unwrap();
    for (d, (class, undo, effects)) in defs.iter().zip(want) {
        let v = d.to_value();
        let o = obj(&v);
        assert_eq!((st(o, "side_effect"), st(o, "undo"), st(o, "determinism")), (class, undo, "D0"), "{}", d.id);
        assert_eq!(d.effects, effects, "{}", d.id);
        let exposure: Vec<&CValue> = arr(o, "exposure").iter().collect();
        assert_eq!(exposure, [&CValue::Str("cli".into()), &CValue::Str("mcp".into())]);
        assert_eq!(check_schema(&d.params_schema, &catalogue), vec![], "{}", d.id);
        assert_eq!(check_schema(&d.result_schema, &catalogue), vec![], "{}", d.id);
    }
    let all = fid_project_cmd::command_defs().unwrap();
    let decls: Vec<CommandDecl> = all.iter().map(CommandDecl::from_def).collect();
    assert_eq!(validate(&decls), vec![]);
}

#[test]
fn handlers_register_on_the_bus() {
    let source: SharedSource = Arc::new(Mutex::new(Recording::new(ReplaySource::new(Vec::new()))));
    let mut set = HandlerSet::new();
    register(&mut set, namespace(), &source).unwrap();
    assert_eq!(set.len(), 5);
    fid_project_cmd::register_containers(&mut set, namespace(), &source).unwrap();
    assert_eq!(set.len(), 9);
    for op in StageOp::ALL {
        let id = format!("{}@1", op.command());
        assert!(set.get(&id).is_ok());
        assert_eq!(set.def(&id).unwrap().id, op.command());
    }
    let err = register(&mut set, namespace(), &source).unwrap_err();
    assert_eq!(err.code(), Code::Internal);
    assert_eq!(set.len(), 9);
}

// ------------------------------------------------------------------ published schemas

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no literals in stage schemas")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

/// Validates `v` against `schema` with the bus's own validator (`CORE.BUS.PIPE.VALID`): the
/// schema must compile and the value must pass it.
fn conforms(schema: &CValue, v: &CValue, at: &str) -> Result<(), String> {
    let c = Compiled::compile(schema).map_err(|e| format!("{at}: schema does not compile: {e:?}"))?;
    c.evaluate(v, &NoHooks).map(|_| ()).map_err(|f| format!("{at}: fails {} ({})", f.keyword, f.message))
}

/// Every accepted fixture command's real parameters conform to its `params_schema` and its
/// real result to its `result_schema`; every list case's parameters and result too. Shape
/// refusals fail the params schema; text, reference and state refusals pass it (the
/// handler gives the specific reason).
#[test]
fn real_params_and_results_conform_to_the_schemas() {
    let w = walk();
    for case in &w.accepted {
        let command = st(case, "command");
        let def = def_of(command);
        conforms(&def.params_schema, get(case, "params"), command).unwrap();
        conforms(&def.result_schema, get(case, "result"), command).unwrap();
    }
    let list = def_of("project.list_stages@1");
    for case in &w.lists {
        conforms(&list.params_schema, get(case, "params"), "list params").unwrap();
        let r = get(case, "result");
        conforms(&list.result_schema, r, "list result").unwrap();
    }
    let c = cases();
    let top = obj(&c);
    let d = obj(get(top, "drawn"));
    conforms(&def_of("project.add_stage@1").params_schema, get(d, "params"), "drawn").unwrap();
    // Refusals: shape ones fail the schema, the others pass it.
    let shape_only_in_handler = ["add_id_nil"];
    let mut errors: Vec<&CValue> = arr(top, "add_errors").iter().collect();
    errors.extend(arr(obj(get(top, "ac3")), "errors"));
    errors.extend(arr(obj(get(top, "ac4")), "errors"));
    let mut shape = 0;
    for e in errors {
        let o = obj(e);
        let name = st(o, "name");
        let verdict = conforms(&def_of(st(o, "command")).params_schema, get(o, "params"), name);
        if st(o, "reason") == "schema" && !shape_only_in_handler.contains(&name) {
            assert!(verdict.is_err(), "{name} conforms to params_schema");
            shape += 1;
        } else {
            assert!(verdict.is_ok(), "{name}: {verdict:?}");
        }
    }
    assert!(shape >= 10, "{shape}");
    // A malformed list result does not conform.
    let one = CValue::Obj([("stages".to_string(), CValue::Arr(vec![CValue::Obj(Obj::new())]))].into_iter().collect());
    assert!(conforms(&list.result_schema, &one, "bad").is_err());
}
