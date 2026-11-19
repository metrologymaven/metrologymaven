//! Acceptance criteria of spec node CORE.DEP.PROP (AC-1 .. AC-6), plus the hand-labelled
//! propagation cases, fail-closed and determinism checks, against `fixtures/core/dep/`.
//! The cases in `prop/cases.txt` were labelled by hand and re-derived by
//! `make_prop_fixtures.py`, a separate reference; `wide_100k/prop_expected.txt` comes from
//! that script. AC values below are copied from the spec text.
//!
//! A test "project" holds the edge index, element states, recompute mode, a journal whose
//! entries are lists of `(event type, canonical JSON)` and a queue of submitted recomputes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_dep::{
    plan, propagate, set_command_def, set_recompute_mode, Adjacency, DepGraph, EdgeIndex, EdgesSet, ElementState,
    ElementStates, ElementsMarked, EntityId, Op, Output, PropError, RecomputeMode, RecomputeModeSet,
    RecomputeSubmission, SetRecomputeMode, EDGES_SET_EVENT, ELEMENTS_MARKED_EVENT, MAX_CHANGED, MAX_PASS_ELEMENTS,
    RECOMPUTE_DEPENDENTS, RECOMPUTE_MODE_SET_EVENT, SET_RECOMPUTE_MODE,
};
use fid_registry::{SideEffect, Undo};
use fid_types::Determinism;
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

// ------------------------------------------------------------------ test kit

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/core/dep")
}

fn read_lines(rel: &str) -> Vec<Vec<String>> {
    let text = std::fs::read_to_string(fixtures().join(rel)).unwrap();
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split(' ').map(str::to_string).collect())
        .collect()
}

fn obj(pairs: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn s(x: &str) -> CValue {
    CValue::Str(x.to_string())
}

fn canon(v: &CValue) -> String {
    String::from_utf8(to_canonical_bytes(v).unwrap()).unwrap()
}

fn ref_schema() -> CValue {
    obj(vec![("type", s("string")), ("x-ref", s("element"))])
}

/// Parameter schema of the test-kit command `testkit.num_set@1` (as in the edge tests).
fn num_schema() -> CValue {
    obj(vec![
        ("type", s("object")),
        (
            "properties",
            obj(vec![
                ("element", ref_schema()),
                ("inputs", obj(vec![("type", s("array")), ("items", ref_schema())])),
                ("value", obj(vec![("type", s("number"))])),
            ]),
        ),
    ])
}

fn num_params(target: Option<EntityId>, inputs: &[EntityId], value: f64) -> CValue {
    let mut o = vec![
        ("inputs", CValue::Arr(inputs.iter().map(|i| s(&i.to_string())).collect())),
        ("value", CValue::Num(value)),
    ];
    if let Some(t) = target {
        o.push(("element", s(&t.to_string())));
    }
    obj(o)
}

type Entry = Vec<(String, String)>;

#[derive(Debug, Clone, Default)]
struct Project {
    index: EdgeIndex,
    states: ElementStates,
    mode: RecomputeMode,
    journal: Vec<Entry>,
    jobs: Vec<RecomputeSubmission>,
    names: BTreeMap<String, EntityId>,
}

impl Project {
    fn id(&self, name: &str) -> EntityId {
        self.names[name]
    }
    fn name(&self, id: &EntityId) -> String {
        self.names.iter().find(|(_, v)| *v == id).map(|(k, _)| k.clone()).unwrap()
    }
    fn names(&self, ids: &[EntityId]) -> Vec<String> {
        ids.iter().map(|i| self.name(i)).collect()
    }
    fn state(&self, name: &str) -> ElementState {
        self.states.get(&self.id(name)).unwrap()
    }

    /// One `testkit.num_set@1` commit: the edge hook, then the propagation hook, then the
    /// journal append of one entry holding every event; the recompute (auto mode) is
    /// submitted only after the entry is appended. Nothing is appended on error.
    fn commit(&mut self, op: Op, target: &str, inputs: &[&str], value: f64) -> Result<(), String> {
        let t = self.id(target);
        let ins: Vec<EntityId> = inputs.iter().map(|n| self.id(n)).collect();
        let params = num_params((op == Op::Update).then_some(t), &ins, value);
        let mut index = self.index.clone();
        let mut states = self.states.clone();
        let sets =
            index.commit_command(&num_schema(), &params, &[Output { element: t, op }]).map_err(|e| e.to_string())?;
        let mut entry: Entry = sets.iter().map(|e| (EDGES_SET_EVENT.to_string(), canon(&e.to_value()))).collect();
        states.insert(t, ElementState::Current);
        let p = propagate(&index, &mut states, self.mode, &[t], &[]).map_err(|e| e.to_string())?;
        if let Some(ev) = &p.event {
            entry.push((ELEMENTS_MARKED_EVENT.to_string(), canon(&ev.to_value())));
        }
        self.index = index;
        self.states = states;
        self.journal.push(entry);
        self.jobs.extend(p.submission);
        Ok(())
    }
}

fn diamond(mode: RecomputeMode) -> Project {
    let mut p = Project { mode, ..Project::default() };
    let mut states = Vec::new();
    for t in read_lines("diamond/project.txt") {
        match t[0].as_str() {
            "elem" => assert!(p.names.insert(t[1].clone(), Uuid::parse(&t[2]).unwrap()).is_none()),
            "create" => {
                let ins: Vec<&str> = t[2..].iter().map(String::as_str).collect();
                p.commit(Op::Create, &t[1], &ins, 1.0).unwrap();
            }
            "state" => states.push((t[1].clone(), ElementState::parse(&t[2]).unwrap())),
            other => panic!("unknown line {other}"),
        }
    }
    for (n, st) in states {
        p.states.insert(p.id(&n), st);
    }
    p.journal.clear();
    p.jobs.clear();
    p
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

// ------------------------------------------------------------------ AC-1 .. AC-3

#[test]
fn ac1_manual_marks_b_c_d_in_the_update_entry_and_submits_nothing() {
    let mut p = diamond(RecomputeMode::Manual);
    p.commit(Op::Update, "a", &[], 2.0).unwrap();
    for n in ["b", "c", "d"] {
        assert_eq!(p.state(n), ElementState::Outdated, "{n}");
    }
    assert_eq!(p.state("a"), ElementState::Current);
    assert_eq!(p.journal.len(), 1, "one journal entry");
    let entry = &p.journal[0];
    let types: Vec<&str> = entry.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(types, vec![EDGES_SET_EVENT, ELEMENTS_MARKED_EVENT], "marks share the update's entry");
    let ev = ElementsMarked::from_value(&parse(entry[1].1.as_bytes()).unwrap()).unwrap();
    assert_eq!(p.names(ev.elements()), strs(&["c", "b", "d"]), "ascending by ID; c < b in the fixture");
    assert!(p.jobs.is_empty(), "no recompute job in manual mode");
}

#[test]
fn ac2_auto_submits_exactly_one_recompute_of_b_c_d_after_the_commit() {
    let mut p = diamond(RecomputeMode::Auto);
    p.commit(Op::Update, "a", &[], 2.0).unwrap();
    assert_eq!(p.jobs.len(), 1);
    let job = &p.jobs[0];
    assert_eq!(job.command(), RECOMPUTE_DEPENDENTS);
    assert_eq!(job.command(), "project.recompute_dependents@1");
    let set: BTreeSet<String> = p.names(job.elements()).into_iter().collect();
    assert_eq!(set, ["b", "c", "d"].iter().map(|x| x.to_string()).collect());
    assert_eq!(
        canon(&job.params()),
        r#"{"scope":{"elements":["00000000-0000-7000-8000-000000000002","00000000-0000-7000-8000-000000000003","00000000-0000-7000-8000-000000000004"]}}"#
    );
    // Submitted after the commit: the marking entry is already journaled.
    assert_eq!(p.journal.len(), 1);
    for n in ["b", "c", "d"] {
        assert_eq!(p.state(n), ElementState::Outdated, "{n}");
    }
}

#[test]
fn ac3_not_computed_stays_not_computed() {
    for mode in [RecomputeMode::Manual, RecomputeMode::Auto] {
        let mut p = diamond(mode);
        let d = p.id("d");
        p.states.insert(d, ElementState::NotComputed);
        p.commit(Op::Update, "a", &[], 2.0).unwrap();
        assert_eq!(p.state("d"), ElementState::NotComputed);
        assert_eq!(p.state("b"), ElementState::Outdated);
        assert_eq!(p.state("c"), ElementState::Outdated);
        for job in &p.jobs {
            assert!(!job.elements().contains(&d), "d is not resubmitted");
        }
    }
}

// ------------------------------------------------------------------ hand-labelled cases

#[derive(Debug, Default)]
struct Case {
    name: String,
    edges: Vec<(String, String)>,
    states: Vec<(String, ElementState)>,
    changed: Vec<String>,
    recomputed: Vec<String>,
    mode: Option<RecomputeMode>,
    expect: BTreeMap<String, Vec<String>>,
}

fn cases() -> (BTreeMap<String, EntityId>, Vec<Case>) {
    let mut ids = BTreeMap::new();
    let mut out = Vec::new();
    let mut cur: Option<Case> = None;
    for t in read_lines("prop/cases.txt") {
        let rest = t[1..].to_vec();
        match t[0].as_str() {
            "elem" => assert!(ids.insert(t[1].clone(), Uuid::parse(&t[2]).unwrap()).is_none()),
            "case" => cur = Some(Case { name: t[1].clone(), ..Case::default() }),
            "end" => out.push(cur.take().unwrap()),
            "edge" => cur.as_mut().unwrap().edges.push((t[1].clone(), t[2].clone())),
            "state" => cur.as_mut().unwrap().states.push((t[1].clone(), ElementState::parse(&t[2]).unwrap())),
            "changed" => cur.as_mut().unwrap().changed = rest,
            "recomputed" => cur.as_mut().unwrap().recomputed = rest,
            "mode" => cur.as_mut().unwrap().mode = RecomputeMode::parse(&t[1]),
            k @ ("order" | "pending" | "marked" | "event" | "submit" | "error") => {
                assert!(cur.as_mut().unwrap().expect.insert(k.to_string(), rest).is_none());
            }
            other => panic!("unknown line {other}"),
        }
    }
    (ids, out)
}

/// The case's graph as an edge index, if it is acyclic (built in dependency order).
fn as_edge_index(ids: &BTreeMap<String, EntityId>, c: &Case) -> Option<EdgeIndex> {
    let mut inputs: BTreeMap<EntityId, BTreeSet<EntityId>> = BTreeMap::new();
    for (i, e) in &c.edges {
        inputs.entry(ids[i]).or_default();
        inputs.entry(ids[e]).or_default().insert(ids[i]);
    }
    let mut index = EdgeIndex::new();
    while !inputs.is_empty() {
        let ready: Vec<EntityId> =
            inputs.iter().filter(|(_, ins)| ins.iter().all(|i| index.contains(i))).map(|(e, _)| *e).collect();
        if ready.is_empty() {
            return None;
        }
        for e in ready {
            let ins = inputs.remove(&e).unwrap();
            index.apply(&[EdgesSet::from_set(e, &ins).unwrap()]).unwrap();
        }
    }
    Some(index)
}

fn run_case<G: DepGraph>(graph: &G, ids: &BTreeMap<String, EntityId>, c: &Case) {
    let name = |id: &EntityId| ids.iter().find(|(_, v)| *v == id).map(|(k, _)| k.clone()).unwrap();
    let names = |v: &[EntityId]| v.iter().map(name).collect::<Vec<_>>();
    let mut states = ElementStates::new();
    for (n, st) in &c.states {
        states.insert(ids[n], *st);
    }
    let before = states.clone();
    let changed: Vec<EntityId> = c.changed.iter().map(|n| ids[n]).collect();
    let recomputed: Vec<EntityId> = c.recomputed.iter().map(|n| ids[n]).collect();
    let mode = c.mode.unwrap();
    match propagate(graph, &mut states, mode, &changed, &recomputed) {
        Ok(p) => {
            assert!(!c.expect.contains_key("error"), "{}: expected an error, got {p:?}", c.name);
            assert_eq!(names(p.marking.order()), c.expect["order"], "{} order", c.name);
            assert_eq!(names(p.marking.pending()), c.expect["pending"], "{} pending", c.name);
            assert_eq!(names(p.marking.marked()), c.expect["marked"], "{} marked", c.name);
            let ev = p.event.as_ref().map(|e| names(e.elements())).unwrap_or_default();
            assert_eq!(ev, c.expect["event"], "{} event", c.name);
            let sub = p.submission.as_ref().map(|s| names(s.elements())).unwrap_or_default();
            assert_eq!(sub, c.expect["submit"], "{} submit", c.name);
            // States: exactly the marked elements moved to outdated, nothing else changed.
            for (id, st) in before.iter() {
                let want = if p.marking.marked().contains(id) { ElementState::Outdated } else { *st };
                assert_eq!(states.get(id), Some(want), "{} state of {}", c.name, name(id));
            }
            assert_eq!(states.len(), before.len());
        }
        Err(e) => {
            let mut got = vec![e.code().to_string(), e.reason().to_string()];
            got.extend(names(&e.elements()));
            assert_eq!(Some(&got), c.expect.get("error"), "{}: {e}", c.name);
            assert_eq!(states, before, "{}: a refused pass changes nothing", c.name);
        }
    }
}

#[test]
fn hand_labelled_cases_on_both_graph_types() {
    let (ids, cases) = cases();
    assert_eq!(cases.len(), 20);
    let mut on_index = 0;
    for c in &cases {
        let mut adj = Adjacency::new();
        for (i, e) in &c.edges {
            adj.add_edge(ids[i], ids[e]);
        }
        run_case(&adj, &ids, c);
        if let Some(index) = as_edge_index(&ids, c) {
            run_case(&index, &ids, c);
            on_index += 1;
        }
    }
    assert_eq!(on_index, 17, "every acyclic case also runs on the edge index");
}

// ------------------------------------------------------------------ AC-4

struct Recipe {
    elements: u64,
    seed: u64,
    max_inputs: u64,
}

fn recipe() -> Recipe {
    let mut m = BTreeMap::new();
    for t in read_lines("wide_100k/recipe.txt") {
        assert!(m.insert(t[0].clone(), t[1..].to_vec()).is_none(), "duplicate key {}", t[0]);
    }
    let one = |k: &str| m[k][0].parse::<u64>().unwrap();
    Recipe { elements: one("elements"), seed: one("seed"), max_inputs: one("max_inputs") }
}

fn wide_id(i: u64) -> EntityId {
    Uuid::from_u128(0x0000_0000_0000_7000_8000_0000_0000_0000 | u128::from(i))
}

fn wide_index(r: &Recipe) -> EdgeIndex {
    let mut state = r.seed;
    let mut next = || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let mut index = EdgeIndex::new();
    index.apply(&[EdgesSet::new(wide_id(0), Vec::new()).unwrap()]).unwrap();
    for i in 1..r.elements {
        let k = 1 + next() % r.max_inputs;
        let ins: BTreeSet<EntityId> = (0..k).map(|_| wide_id(next() % i)).collect();
        index.apply(&[EdgesSet::from_set(wide_id(i), &ins).unwrap()]).unwrap();
    }
    index
}

fn sha(bytes: &[u8]) -> String {
    fid_canon::digest_bytes(bytes).to_string()
}

/// The budget is 100 ms on reference hardware (REQ-006, unverified until measured).
const AC4_BUDGET: Duration = Duration::from_millis(100);

/// Marks the whole wide_100k graph from its root (best of three fresh copies) and checks
/// the result against `wide_100k/prop_expected.txt`. Returns the best time.
fn ac4_run() -> Duration {
    let r = recipe();
    let index = wide_index(&r);
    assert_eq!(index.len(), 100_000);
    let exp: BTreeMap<String, String> =
        read_lines("wide_100k/prop_expected.txt").into_iter().map(|t| (t[0].clone(), t[1].clone())).collect();
    let num = |k: &str| exp[k].parse::<usize>().unwrap();
    let mut all = ElementStates::new();
    for id in index.elements() {
        all.insert(*id, ElementState::Current);
    }
    let root = [wide_id(0)];
    let mut best = Duration::MAX;
    let mut done = None;
    for _ in 0..3 {
        let mut states = all.clone();
        let t = Instant::now();
        let p = propagate(&index, &mut states, RecomputeMode::Manual, &root, &[]).unwrap();
        best = best.min(t.elapsed());
        done = Some((p, states));
    }
    let (p, states) = done.unwrap();
    assert_eq!(p.marking.order().len(), num("region"));
    assert_eq!(p.marking.pending().len(), num("pending"));
    assert_eq!(p.marking.marked().len(), num("marked"));
    assert!(p.submission.is_none() && num("submit") == 0, "manual mode submits nothing");
    let order = CValue::Arr(p.marking.order().iter().map(|i| s(&i.to_string())).collect());
    assert_eq!(sha(&to_canonical_bytes(&order).unwrap()), exp["order_digest"]);
    let ev = p.event.unwrap();
    assert_eq!(sha(&to_canonical_bytes(&ev.to_value()).unwrap()), exp["event_digest"]);
    assert_eq!(states.get(&wide_id(0)), Some(ElementState::Current));
    assert_eq!(states.iter().filter(|(_, s)| **s == ElementState::Outdated).count(), 99_999);
    best
}

/// Runs in every `cargo test`: the full result is checked, and the time is held to a guard
/// against super-linear regressions only (CI builds tests unoptimised, 5x to 10x slower),
/// 40x the budget unoptimised and 10x optimised.
#[test]
fn ac4_marking_100k_from_the_root() {
    let best = ac4_run();
    let limit = if cfg!(debug_assertions) { AC4_BUDGET * 40 } else { AC4_BUDGET * 10 };
    println!("AC-4: best {best:?} (guard {limit:?})");
    assert!(best < limit, "{best:?}");
}

/// The AC-4 benchmark itself, on an optimised build of an otherwise idle machine:
/// `cargo test --release -p fid-dep --test prop -- --ignored ac4_benchmark`.
#[test]
#[ignore = "benchmark: run with --release on reference hardware"]
fn ac4_benchmark_under_100_ms() {
    let best = ac4_run();
    println!("AC-4 benchmark: best {best:?} (budget {AC4_BUDGET:?})");
    if cfg!(debug_assertions) {
        panic!("run the benchmark with --release");
    }
    assert!(best < AC4_BUDGET, "{best:?}");
}

// ------------------------------------------------------------------ AC-5, AC-6

/// A journal of mode events with `head_move` undo: the head is an index into the entries.
#[derive(Debug, Default)]
struct ModeJournal {
    entries: Vec<(String, String)>,
    head: usize,
}

impl ModeJournal {
    fn run(&mut self, params: &CValue) -> Result<CValue, PropError> {
        let ev = set_recompute_mode(params)?;
        self.entries.truncate(self.head);
        self.entries.push((SET_RECOMPUTE_MODE.to_string(), canon(&ev.to_value())));
        self.head = self.entries.len();
        Ok(ev.mode.result_value())
    }
    fn undo(&mut self) {
        self.head = self.head.saturating_sub(1);
    }
    fn mode(&self) -> RecomputeMode {
        let events: Vec<RecomputeModeSet> = self.entries[..self.head]
            .iter()
            .map(|(_, b)| RecomputeModeSet::from_value(&parse(b.as_bytes()).unwrap()).unwrap())
            .collect();
        RecomputeMode::replay(&events)
    }
}

#[test]
fn ac5_set_manual_journals_one_entry_and_undo_restores_auto() {
    let mut j = ModeJournal::default();
    assert_eq!(j.mode(), RecomputeMode::Auto, "a new project is auto");
    assert_eq!(RecomputeMode::default(), RecomputeMode::Auto);
    let result = j.run(&parse(br#"{"mode":"manual"}"#).unwrap()).unwrap();
    assert_eq!(canon(&result), r#"{"mode":"manual"}"#);
    assert_eq!(j.entries.len(), 1);
    assert_eq!(j.entries[0], (SET_RECOMPUTE_MODE.to_string(), r#"{"mode":"manual"}"#.to_string()));
    assert_eq!(j.mode(), RecomputeMode::Manual);
    j.undo();
    assert_eq!(j.mode(), RecomputeMode::Auto);

    let def = set_command_def().unwrap();
    assert_eq!(format!("{}@{}", def.id, def.major), SET_RECOMPUTE_MODE);
    assert_eq!(def.side_effect, SideEffect::W2);
    assert_eq!(def.determinism, Determinism::D0);
    assert_eq!(def.undo, Undo::HeadMove);
    assert_eq!(def.effects, vec![RECOMPUTE_MODE_SET_EVENT.to_string()]);
    assert_eq!(RECOMPUTE_MODE_SET_EVENT, "project.recompute_mode_set@1");
}

#[test]
fn ac6_unknown_mode_fails_with_validation_and_journals_nothing() {
    let mut j = ModeJournal::default();
    let err = j.run(&parse(br#"{"mode":"sometimes"}"#).unwrap()).unwrap_err();
    assert_eq!(err.code(), "validation");
    assert!(j.entries.is_empty());
    for bad in [
        r#"{}"#,
        r#"{"mode":"Auto"}"#,
        r#"{"mode":1}"#,
        r#"{"mode":"auto","extra":true}"#,
        r#"["auto"]"#,
        r#"{"mode":"auto "}"#,
    ] {
        let err = j.run(&parse(bad.as_bytes()).unwrap()).unwrap_err();
        assert_eq!(err.code(), "validation", "{bad}");
    }
    assert!(j.entries.is_empty());
    assert_eq!(SetRecomputeMode::from_value(&parse(br#"{"mode":"auto"}"#).unwrap()).unwrap().mode, RecomputeMode::Auto);
}

// ------------------------------------------------------------------ fail closed

fn id(n: u128) -> EntityId {
    Uuid::from_u128(0x0000_0000_0000_7000_8000_0000_0000_0000 | n)
}

#[test]
fn marked_event_round_trips_and_refuses_malformed_input() {
    let ev = ElementsMarked::new(vec![id(1), id(2)]).unwrap();
    let text = canon(&ev.to_value());
    assert_eq!(text, r#"{"elements":["00000000-0000-7000-8000-000000000001","00000000-0000-7000-8000-000000000002"]}"#);
    assert_eq!(ElementsMarked::from_value(&parse(text.as_bytes()).unwrap()).unwrap(), ev);
    assert_eq!(ELEMENTS_MARKED_EVENT, "dep.elements_marked@1");
    let a = "00000000-0000-7000-8000-000000000001";
    let b = "00000000-0000-7000-8000-000000000002";
    for bad in [
        r#"{"elements":[]}"#.to_string(),
        format!(r#"{{"elements":["{b}","{a}"]}}"#),
        format!(r#"{{"elements":["{a}","{a}"]}}"#),
        r#"{"elements":["00000000-0000-7000-8000-00000000000A"]}"#.to_string(),
        format!(r#"{{"elements":["{a}"],"x":1}}"#),
        r#"{"elements":[1]}"#.to_string(),
        r#"{"elements":"x"}"#.to_string(),
        r#"{}"#.to_string(),
        r#"[]"#.to_string(),
    ] {
        let err = ElementsMarked::from_value(&parse(bad.as_bytes()).unwrap()).unwrap_err();
        assert_eq!(err.code(), "bad_event", "{bad}");
    }
    let too_many: Vec<EntityId> = (0..=MAX_PASS_ELEMENTS as u128).map(id).collect();
    assert_eq!(ElementsMarked::new(too_many).unwrap_err().code(), "input_limit");
}

#[test]
fn mode_event_refuses_malformed_input() {
    for bad in [r#"{"mode":"sometimes"}"#, r#"{"mode":true}"#, r#"{}"#, r#"{"mode":"auto","x":1}"#, r#""auto""#] {
        let err = RecomputeModeSet::from_value(&parse(bad.as_bytes()).unwrap()).unwrap_err();
        assert_eq!(err.code(), "bad_event", "{bad}");
    }
}

#[test]
fn applying_marks_is_all_or_nothing() {
    let mut states = ElementStates::new();
    states.insert(id(1), ElementState::Current);
    states.insert(id(2), ElementState::NotComputed);
    states.insert(id(3), ElementState::Failed);
    let before = states.clone();
    let ev = ElementsMarked::new(vec![id(1), id(2), id(3)]).unwrap();
    let err = states.apply_marked(&ev).unwrap_err();
    assert_eq!(err, PropError::InvalidTransition { element: id(2), from: ElementState::NotComputed });
    assert_eq!(states, before);
    let ev = ElementsMarked::new(vec![id(1), id(4)]).unwrap();
    assert_eq!(states.apply_marked(&ev).unwrap_err().reason(), "missing_state");
    assert_eq!(states, before);
    let ev = ElementsMarked::new(vec![id(1), id(3)]).unwrap();
    states.apply_marked(&ev).unwrap();
    assert_eq!(states.get(&id(1)), Some(ElementState::Outdated));
    assert_eq!(states.get(&id(3)), Some(ElementState::Outdated));
    // Replaying the same event again is an invalid transition (outdated -> outdated).
    assert_eq!(states.apply_marked(&ev).unwrap_err().code(), "invalid_transition");
}

#[test]
fn too_many_changed_elements_is_refused() {
    let mut g = Adjacency::new();
    let changed: Vec<EntityId> = (0..=MAX_CHANGED as u128).map(id).collect();
    for c in &changed {
        g.add_element(*c);
    }
    let err = plan(&g, &ElementStates::new(), &changed, &[]).unwrap_err();
    assert_eq!(err, PropError::InputLimit("changed"));
    let err = plan(&g, &ElementStates::new(), &changed[..1], &vec![id(0); MAX_PASS_ELEMENTS + 1]).unwrap_err();
    assert_eq!(err, PropError::InputLimit("recomputed"));
}

// ------------------------------------------------------------------ determinism, bounds

#[test]
fn result_does_not_depend_on_list_order() {
    let (ids, cases) = cases();
    for c in &cases {
        let mut g = Adjacency::new();
        for (i, e) in &c.edges {
            g.add_edge(ids[i], ids[e]);
        }
        let mut states = ElementStates::new();
        for (n, st) in &c.states {
            states.insert(ids[n], *st);
        }
        let fwd: Vec<EntityId> = c.changed.iter().map(|n| ids[n]).collect();
        let rev: Vec<EntityId> = fwd.iter().rev().copied().collect();
        let rc: Vec<EntityId> = c.recomputed.iter().map(|n| ids[n]).collect();
        let rc_rev: Vec<EntityId> = rc.iter().rev().copied().collect();
        assert_eq!(plan(&g, &states, &fwd, &rc), plan(&g, &states, &rev, &rc_rev), "{}", c.name);
    }
}

#[test]
fn journaled_marks_replay_to_the_same_states() {
    let mut p = diamond(RecomputeMode::Manual);
    let start = p.states.clone();
    p.commit(Op::Update, "a", &[], 2.0).unwrap();
    let mut replayed = start;
    for entry in &p.journal {
        for (t, body) in entry {
            if t == ELEMENTS_MARKED_EVENT {
                replayed.apply_marked(&ElementsMarked::from_value(&parse(body.as_bytes()).unwrap()).unwrap()).unwrap();
            }
        }
    }
    // The commit also set a (the changed element) to current; it already was.
    assert_eq!(replayed, p.states);
}

#[test]
fn deep_chain_is_iterative_and_ordered() {
    // 200,000 elements in one chain: recursion would overflow the test thread's stack.
    const N: u128 = 200_000;
    let mut g = Adjacency::new();
    let mut states = ElementStates::new();
    for i in 0..N {
        states.insert(id(i), ElementState::Current);
        if i > 0 {
            g.add_edge(id(i - 1), id(i));
        }
    }
    let t = Instant::now();
    let m = plan(&g, &states, &[id(0)], &[]).unwrap();
    println!("deep chain: {N} elements, plan {:?}", t.elapsed());
    assert_eq!(m.order().len(), N as usize);
    assert!(m.order().windows(2).all(|w| w[0] < w[1]));
    assert_eq!(m.marked().len(), N as usize - 1);
    // Close the chain into a cycle: refused, cycle listed from the smallest ID.
    g.add_edge(id(N - 1), id(1));
    let err = plan(&g, &states, &[id(0)], &[]).unwrap_err();
    let PropError::Cycle(c) = err else { panic!("{err:?}") };
    assert_eq!(c.len(), N as usize - 1);
    assert_eq!(c[0], id(1));
    assert_eq!(c[c.len() - 1], id(N - 1));
}

#[test]
fn errors_have_stable_codes_and_text() {
    let e = PropError::Cycle(vec![id(1), id(2)]);
    assert_eq!(
        e.to_string(),
        "cycle/cycle: [00000000-0000-7000-8000-000000000001, 00000000-0000-7000-8000-000000000002]"
    );
    assert_eq!(
        PropError::MissingState(id(1)).to_string(),
        "not_found/missing_state: 00000000-0000-7000-8000-000000000001"
    );
    assert_eq!(PropError::Validation("x").code(), "validation");
}
