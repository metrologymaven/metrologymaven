//! Acceptance criteria of spec node CORE.PRJ.STAGE.MODEL (AC-1 .. AC-4 on the model, AC-5 on
//! the tree) with fixtures from `fixtures/core/prj/stage/model/make_fixtures.py`
//! (independent Python reference). The bus side (definitions, handlers, bus errors, replay
//! of the handlers) is tested in `fid-project-cmd`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_project::{
    add_stage, list_stages, remove_stage, reorder_stages, Bits, ContainerView, ElementState, ElementTypeDef,
    ElementTypeRegistry, IdIndex, Observation, Recording, ReplaySource, Tree, TreeError, TreeEvent, Uuid,
    STAGES_REORDERED, STAGE_ADDED, STAGE_KIND, STAGE_LIST_KIND, STAGE_REMOVED,
};
use std::collections::BTreeMap;
use std::sync::Arc;

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

fn uuid(o: &Obj, k: &str) -> Uuid {
    Uuid::parse(st(o, k)).unwrap()
}

fn canon(v: &CValue) -> Vec<u8> {
    to_canonical_bytes(v).unwrap()
}

fn ev_value(e: &TreeEvent) -> CValue {
    let mut o = Obj::new();
    o.insert("body".into(), e.body.clone());
    o.insert("type".into(), CValue::Str(e.event_type.clone()));
    CValue::Obj(o)
}

fn tree_event(v: &CValue) -> TreeEvent {
    let o = obj(v);
    TreeEvent::new(st(o, "type"), get(o, "body").clone())
}

fn setup() -> CValue {
    parse(&read("p100/setup.json")).unwrap()
}

fn cases() -> CValue {
    parse(&read("cases.json")).unwrap()
}

/// The test project: the tree plus the containers it does not hold (folded from the
/// `ContainerCreated` events, which the tree takes through an empty domain translation).
#[derive(Clone)]
struct Project {
    tree: Tree,
    containers: BTreeMap<Uuid, (String, CValue)>,
}

impl ContainerView for Project {
    fn entity(&self, id: &Uuid) -> Option<(&str, &CValue)> {
        if let Some((k, b)) = self.containers.get(id) {
            return Some((k.as_str(), b));
        }
        self.tree.entity(id)
    }
}

impl Project {
    fn apply(&mut self, e: &TreeEvent) -> Result<(), TreeError> {
        self.tree.apply(&e.event_type, &e.body)?;
        if e.event_type == "ContainerCreated" {
            let b = obj(&e.body);
            self.containers.insert(uuid(b, "id"), (st(b, "kind").to_string(), e.body.clone()));
        }
        Ok(())
    }

    fn digest(&self) -> String {
        self.tree.state_digest().unwrap().as_str().to_string()
    }
}

fn namespace() -> Uuid {
    uuid(obj(&setup()), "namespace")
}

fn registry() -> Arc<ElementTypeRegistry> {
    let mut reg = ElementTypeRegistry::new();
    let mut schema = Obj::new();
    schema.insert("type".into(), CValue::Str("object".into()));
    reg.register(ElementTypeDef::new("test.point", CValue::Obj(schema), "point").unwrap()).unwrap();
    reg.register_event("ContainerCreated", |_| Ok(Vec::new())).unwrap();
    Arc::new(reg)
}

fn empty() -> Project {
    Project { tree: Tree::new(namespace(), registry()).unwrap(), containers: BTreeMap::new() }
}

fn p100_events() -> Vec<TreeEvent> {
    String::from_utf8(read("p100/events.jsonl"))
        .unwrap()
        .lines()
        .map(|l| tree_event(&parse(l.as_bytes()).unwrap()))
        .collect()
}

/// p100, one commit per event.
fn p100() -> Project {
    let mut p = empty();
    for e in p100_events() {
        p.apply(&e).unwrap();
    }
    p
}

/// The model error code of a fixture's bus (code, reason).
fn model_code(code: &str, reason: &str) -> &'static str {
    match (code, reason) {
        ("validation", "schema") => "schema",
        ("validation", "label") => "label",
        ("validation", "bad_reference") => "bad_reference",
        ("validation", "bad_stage") => "bad_order",
        ("not_found", "unknown_entity") => "stage_not_found",
        ("precondition", "stage_active") => "stage_active",
        ("conflict", "id_collision") => "id_collision",
        other => panic!("unexpected {other:?}"),
    }
}

fn no_source() -> ReplaySource {
    ReplaySource::new(Vec::new())
}

/// Runs an accepted command case against `p`: its event must equal the fixture's; applies it.
fn accept(p: &mut Project, case: &Obj, source: &mut ReplaySource) {
    let params = get(case, "params");
    let ns = namespace();
    let e = match st(case, "command") {
        "project.add_stage@1" => add_stage(p, params, ns, source),
        "project.remove_stage@1" => remove_stage(p, params, ns),
        "project.reorder_stages@1" => reorder_stages(p, params, ns),
        other => panic!("{other}"),
    }
    .unwrap();
    assert_eq!(canon(&ev_value(&e)), canon(get(case, "event")), "{}", st(case, "command"));
    p.apply(&e).unwrap();
    p.tree.verify().unwrap();
}

fn refuse(p: &Project, case: &Obj) {
    let name = st(case, "name");
    let params = get(case, "params");
    let ns = namespace();
    let mut src = Recording::new(no_source());
    let e = match st(case, "command") {
        "project.add_stage@1" => add_stage(p, params, ns, &mut src).err(),
        "project.remove_stage@1" => remove_stage(p, params, ns).err(),
        "project.reorder_stages@1" => reorder_stages(p, params, ns).err(),
        "project.list_stages@1" => list_stages(p, params, ns).err(),
        other => panic!("{other}"),
    }
    .unwrap_or_else(|| panic!("{name}: accepted"));
    assert_eq!(e.code(), model_code(st(case, "code"), st(case, "reason")), "{name}: {e}");
    assert!(src.observations().is_empty(), "{name}: drew an ID");
}

fn list(p: &Project, params: &CValue) -> CValue {
    list_stages(p, params, namespace()).unwrap()
}

fn after_ac1() -> Project {
    let mut p = p100();
    accept(&mut p, obj(get(obj(&cases()), "ac1")), &mut no_source());
    p
}

// ------------------------------------------------------------------ p100

/// p100 matches the reference: digest, one explicit stage that took the default stage's
/// place (with its data), the stage list entity and the per-stage digest.
#[test]
fn p100_matches_the_reference() {
    let p = p100();
    let s = setup();
    let so = obj(&s);
    let expected = String::from_utf8(read("p100/expected_digest.txt")).unwrap();
    assert_eq!(p.digest(), expected.trim_end());
    p.tree.verify().unwrap();
    let s1 = uuid(so, "s1");
    assert_eq!(p.tree.stages(), &[s1][..]);
    assert_eq!(p.tree.live_stages(), vec![s1]);
    assert_eq!(p.tree.active_stage(), s1);
    assert!(p.tree.has_explicit_stages());
    assert_eq!(p.tree.element_count(), 100);
    assert_eq!(p.tree.stage_digest(&s1).unwrap().as_str(), st(so, "stage_digest_s1"));
    // The implicit default stage is gone; its data moved to S1.
    let default = uuid(so, "default_stage");
    assert_eq!(p.tree.stage_digest(&default).unwrap_err().code(), "bad_stage");
    let first = Uuid::parse(match &arr(so, "elements")[0] {
        CValue::Str(s) => s,
        other => panic!("{other:?}"),
    })
    .unwrap();
    assert_eq!(p.tree.displayed_state(&first), Some(ElementState::Outdated));
    let list_id = uuid(so, "stage_list");
    assert_eq!(p.tree.stage_list_id(), list_id);
    let (kind, body) = p.tree.entity(&list_id).unwrap();
    assert_eq!((kind, canon(body)), (STAGE_LIST_KIND, canon(get(so, "list_entity"))));
    assert_eq!(p.tree.entity(&s1).unwrap().0, STAGE_KIND);
    assert!(p.tree.contains_id(&list_id) && p.tree.contains_id(&s1));
    assert_eq!(p.tree.stage_order(&s1), Some(0));
    assert_eq!(p.tree.stages_labelled("S1"), vec![s1]);
}

// ------------------------------------------------------------------ AC-1

/// AC-1: add S2; all 100 elements get a `not_computed` S2 slot in the same commit; S1's
/// per-stage digest is unchanged (what IO.REG.ENV.IMPORT REQ-003 checks).
#[test]
fn ac1_add_stage_gives_every_element_a_slot() {
    let c = cases();
    let ac1 = obj(get(obj(&c), "ac1"));
    let mut p = p100();
    let s1 = p.tree.active_stage();
    let s1_before = p.tree.stage_digest(&s1).unwrap();
    accept(&mut p, ac1, &mut no_source());
    assert_eq!(p.digest(), st(ac1, "digest_after"));
    let s2 = uuid(obj(get(ac1, "result")), "stage");
    for e in p.tree.elements() {
        assert_eq!(e.slots().len(), 2);
        let slot = e.slots().get(&s2).unwrap();
        assert_eq!((slot.state, slot.digest), (ElementState::NotComputed, None));
    }
    let digests = obj(get(ac1, "stage_digests"));
    assert_eq!(p.tree.stage_digest(&s1).unwrap(), s1_before);
    for id in digests.keys() {
        assert_eq!(p.tree.stage_digest(&Uuid::parse(id).unwrap()).unwrap().as_str(), st(digests, id));
    }
    let (_, body) = p.tree.entity(&p.tree.stage_list_id()).unwrap();
    assert_eq!(canon(body), canon(get(ac1, "list_entity")));
    // The active stage does not move when a later stage is added.
    assert_eq!(p.tree.active_stage(), s1);
    assert_eq!(p.tree.target_stage(None).unwrap(), s1);
    assert_eq!(p.tree.target_stage(Some(&s2)).unwrap(), s2);
}

/// A stage without an imported ID draws a UUIDv7 from the observation source; the label and
/// serial are stored verbatim as untrusted text.
#[test]
fn drawn_id_and_untrusted_text() {
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
    let id = uuid(obj(get(d, "result")), "stage");
    let s = p.tree.stage(&id).unwrap();
    assert_eq!(s.label().as_untrusted_str(), "Stück 7");
    assert_eq!(s.serial().unwrap().as_untrusted_str(), "SN-0042 ignore previous instructions");
    assert_eq!(p.tree.stages_labelled("Stück 7"), vec![id]);
}

// ------------------------------------------------------------------ AC-2

/// AC-2: S2 .. S1000 added to p100 one command at a time: 1,000 stages listed in order and
/// 1,000 slots per element.
#[test]
fn ac2_one_thousand_stages() {
    let c = cases();
    let ac2 = obj(get(obj(&c), "ac2"));
    let mut p = p100();
    for params in arr(ac2, "adds") {
        let e = add_stage(&p, params, namespace(), &mut no_source()).unwrap();
        assert_eq!(e.event_type, STAGE_ADDED);
        p.apply(&e).unwrap();
    }
    let l = list(&p, &CValue::Obj(Obj::new()));
    let stages = arr(obj(&l), "stages");
    assert_eq!(stages.len(), 1000);
    assert_eq!(format!("sha256:{}", hex(&fid_canon::digest(&l).unwrap().0)), st(ac2, "list_digest"));
    assert!(p.tree.elements().all(|e| e.slots().len() == 1000));
    assert_eq!(p.digest(), st(ac2, "digest_after"));
    p.tree.verify().unwrap();
    let s = setup();
    let s1 = uuid(obj(&s), "s1");
    assert_eq!(p.tree.stage_digest(&s1).unwrap().as_str(), st(ac2, "s1_digest"));
    let last = *p.tree.stages().last().unwrap();
    assert_eq!(p.tree.stage_digest(&last).unwrap().as_str(), st(ac2, "s1000_digest"));
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

// ------------------------------------------------------------------ AC-3

/// AC-3: S2 with data is removed: the list omits it, include_removed shows it, its data stays,
/// and undo (the state at the previous head) has it live with its data. Removing the active
/// stage is `stage_active`; the other refusals are hand-labelled.
#[test]
fn ac3_soft_remove_and_undo() {
    let c = cases();
    let ac3 = obj(get(obj(&c), "ac3"));
    let mut p = after_ac1();
    let s1 = p.tree.active_stage();
    let s1_before = p.tree.stage_digest(&s1).unwrap();
    for e in arr(ac3, "setup") {
        p.apply(&tree_event(e)).unwrap();
    }
    // Slot changes in S2 leave S1's per-stage digest alone.
    assert_eq!(p.tree.stage_digest(&s1).unwrap(), s1_before);
    assert_eq!(p.digest(), st(ac3, "digest_before"));
    assert_eq!(canon(&list(&p, &CValue::Obj(Obj::new()))), canon(get(ac3, "list_before")));
    let undo_state = p.clone();
    let rm = obj(get(ac3, "remove"));
    accept(&mut p, rm, &mut no_source());
    let s2 = uuid(obj(get(rm, "result")), "stage");
    assert_eq!(p.digest(), st(ac3, "digest_after"));
    for key in ["list", "list_removed"] {
        let case = obj(get(ac3, key));
        assert_eq!(canon(&list(&p, get(case, "params"))), canon(get(case, "result")), "{key}");
    }
    assert_eq!(p.tree.stage_digest(&s2).unwrap().as_str(), st(ac3, "s2_digest_after"));
    assert!(p.tree.stage(&s2).unwrap().removed());
    assert_eq!(p.tree.live_stages(), vec![s1]);
    assert_eq!(p.tree.stages().len(), 2);
    assert_eq!(p.tree.target_stage(Some(&s2)).unwrap_err().code(), "stage_not_found");
    assert!(p.tree.stages_labelled("S2").is_empty());

    // A slot change in a removed stage is refused and changes nothing.
    let bad = obj(get(ac3, "slot_in_removed"));
    let before = p.digest();
    let err = p.apply(&tree_event(get(bad, "event"))).unwrap_err();
    assert_eq!(err.code(), "bad_stage");
    assert_eq!(p.digest(), before);
    // A new element gets slots for live stages only.
    let late = obj(get(ac3, "late_element"));
    let mut q = p.clone();
    let created = q.tree.apply_commit(&[tree_event(get(late, "event"))]).unwrap();
    let keys: Vec<String> = q.tree.element(&created[0]).unwrap().slots().keys().map(|k| k.to_string()).collect();
    let want: Vec<String> = arr(late, "slots")
        .iter()
        .map(|v| match v {
            CValue::Str(s) => s.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(keys, want);
    q.tree.verify().unwrap();

    // Refusals, nothing changed.
    for e in arr(ac3, "errors") {
        refuse(&p, obj(e));
    }
    assert_eq!(p.digest(), st(ac3, "digest_after"));

    // Undo by head move: the state at the previous head has S2 live with its data.
    assert_eq!(undo_state.digest(), st(ac3, "digest_before"));
    assert_eq!(undo_state.tree.stage_digest(&s2).unwrap().as_str(), st(ac3, "s2_digest"));
    assert_eq!(undo_state.tree.live_stages(), vec![s1, s2]);
}

/// REQ-005 on the tree too: a journaled `StageRemoved` of the active stage, of an unknown
/// stage or of a removed stage is refused and the commit changes nothing.
#[test]
fn tree_refuses_bad_stage_events() {
    let mut p = after_ac1();
    let s1 = p.tree.active_stage();
    let s2 = *p.tree.stages().last().unwrap();
    let rm = |s: &Uuid| TreeEvent::new(STAGE_REMOVED, parse(format!("{{\"stage\":\"{s}\"}}").as_bytes()).unwrap());
    let before = p.digest();
    assert_eq!(p.apply(&rm(&s1)).unwrap_err().code(), "stage_active");
    assert_eq!(p.apply(&rm(&Uuid::from_u128(77))).unwrap_err().code(), "stage_not_found");
    assert_eq!(p.digest(), before);
    p.apply(&rm(&s2)).unwrap();
    assert_eq!(p.apply(&rm(&s2)).unwrap_err().code(), "stage_not_found");
    // A malformed body is `schema`; a domain may not register a stage event type.
    let bad = TreeEvent::new(STAGE_REMOVED, parse(br#"{"stage":"x"}"#).unwrap());
    assert_eq!(p.apply(&bad).unwrap_err().code(), "schema");
    let added = TreeEvent::new(STAGE_ADDED, parse(br#"{"id":"x"}"#).unwrap());
    assert_eq!(p.apply(&added).unwrap_err().code(), "schema");
    let mut reg = ElementTypeRegistry::new();
    assert_eq!(reg.register_event(STAGE_ADDED, |_| Ok(Vec::new())).unwrap_err().code(), "duplicate_event");
    p.tree.verify().unwrap();
}

/// A commit holding a `StageAdded` that is refused later rolls back every slot, including the
/// adoption of the implicit default stage's data by the first explicit stage.
#[test]
fn refused_commit_rolls_back_stage_slots() {
    let events = p100_events();
    let at = events.iter().position(|e| e.event_type == STAGE_ADDED).unwrap();
    let mut p = empty();
    for e in &events[..at] {
        p.apply(e).unwrap();
    }
    let before = p.digest();
    let default = p.tree.active_stage();
    let bad =
        TreeEvent::new("ElementSlotSet", parse(br#"{"digest":null,"element":"x","stage":"y","state":"z"}"#).unwrap());
    let err = p.tree.apply_commit(&[events[at].clone(), bad.clone()]).unwrap_err();
    assert_eq!(err.code(), "schema");
    assert_eq!(p.digest(), before);
    assert_eq!(p.tree.active_stage(), default);
    p.tree.verify().unwrap();
    // The same after a later stage: the new slots are removed again.
    p.apply(&events[at]).unwrap();
    let before = p.digest();
    let c = cases();
    let ac1 = tree_event(get(obj(get(obj(&c), "ac1")), "event"));
    assert_eq!(p.tree.apply_commit(&[ac1, bad]).unwrap_err().code(), "schema");
    assert_eq!(p.digest(), before);
    p.tree.verify().unwrap();
}

// ------------------------------------------------------------------ AC-4

/// AC-4: a reorder that is not a permutation of the run's live stages is `bad_order` and
/// nothing changes; a valid one reorders only that run's live stages.
#[test]
fn ac4_reorder_requires_a_permutation() {
    let c = cases();
    let top = obj(&c);
    let ac3 = obj(get(top, "ac3"));
    let ac4 = obj(get(top, "ac4"));
    let mut p = after_ac1();
    for e in arr(ac3, "setup") {
        p.apply(&tree_event(e)).unwrap();
    }
    accept(&mut p, obj(get(ac3, "remove")), &mut no_source());
    for step in arr(ac4, "setup") {
        accept(&mut p, obj(step), &mut no_source());
    }
    assert_eq!(p.digest(), st(ac4, "digest_before"));
    for e in arr(ac4, "errors") {
        refuse(&p, obj(e));
    }
    assert_eq!(p.digest(), st(ac4, "digest_before"));
    // The same refusal on the tree: a journaled bad reorder aborts its commit.
    let ok = obj(get(ac4, "ok"));
    let run = st(obj(get(ok, "params")), "run");
    let bad =
        TreeEvent::new(STAGES_REORDERED, parse(format!("{{\"order\":[],\"run\":\"{run}\"}}").as_bytes()).unwrap());
    assert_eq!(p.apply(&bad).unwrap_err().code(), "bad_order");
    assert_eq!(p.digest(), st(ac4, "digest_before"));

    accept(&mut p, ok, &mut no_source());
    assert_eq!(p.digest(), st(ac4, "digest_after"));
    let seq: Vec<String> = p.tree.stages().iter().map(|s| s.to_string()).collect();
    let want: Vec<String> = arr(ac4, "sequence_after")
        .iter()
        .map(|v| match v {
            CValue::Str(s) => s.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(seq, want);
    for key in ["list", "list_run2", "list_all"] {
        let case = obj(get(ac4, key));
        assert_eq!(canon(&list(&p, get(case, "params"))), canon(get(case, "result")), "{key}");
    }
}

/// Hand-labelled refusals of add and list, and a 256-character label accepted.
#[test]
fn add_and_list_refusals() {
    let c = cases();
    let top = obj(&c);
    let mut p = after_ac1();
    for e in arr(top, "add_errors") {
        refuse(&p, obj(e));
    }
    for e in arr(top, "more_ok") {
        accept(&mut p, obj(e), &mut no_source());
    }
}

// ------------------------------------------------------------------ AC-5 (tree half)

/// AC-5 on the tree: the journal of AC-1 and AC-3 applied twice from the empty project gives
/// byte-identical state digests (the handler replay is tested in `fid-project-cmd`).
#[test]
fn ac5_same_events_same_digest() {
    let c = cases();
    let top = obj(&c);
    let ac3 = obj(get(top, "ac3"));
    let mut journal = p100_events();
    journal.push(tree_event(get(obj(get(top, "ac1")), "event")));
    journal.extend(arr(ac3, "setup").iter().map(tree_event));
    journal.push(tree_event(get(obj(get(ac3, "remove")), "event")));
    let run = || {
        let mut p = empty();
        for e in &journal {
            p.apply(e).unwrap();
        }
        p.digest()
    };
    assert_eq!(run(), run());
    assert_eq!(run(), st(ac3, "digest_after"));
}
