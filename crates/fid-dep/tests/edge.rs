//! Acceptance criteria of spec node CORE.DEP.EDGE (AC-1 .. AC-5), plus fail-closed and
//! determinism checks, against the hand-labelled fixtures in `fixtures/core/dep/`.
//! Expected digests and query answers come from `fixtures/core/dep/make_fixtures.py`, a
//! separate reference implementation; the AC values below are copied from the spec text.
//!
//! The test kit registers element type `num` (value = sum of inputs) through one command,
//! `testkit.num_set@1`, whose parameters reference the target and the inputs by `x-ref`.
//! A test "journal" stores each committed event as canonical JSON bytes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_dep::{
    command_def, show_dependencies, EdgeError, EdgeIndex, EdgesSet, EntityId, Op, Output, ShowDependencies,
    EDGES_SET_EVENT, MAX_COMMIT_ELEMENTS, MAX_INPUTS, MAX_SCHEMA_DEPTH, MAX_WALK_NODES, SHOW_DEPENDENCIES,
};
use fid_types::CommandId;
use fid_uuid::Uuid;
use std::collections::BTreeMap;
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

fn ref_schema() -> CValue {
    obj(vec![("type", s("string")), ("x-ref", s("element"))])
}

/// Parameter schema of the test-kit command `testkit.num_set@1`.
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

fn num_params(target: Option<EntityId>, inputs: &[EntityId]) -> CValue {
    let mut o = vec![("inputs", CValue::Arr(inputs.iter().map(|i| s(&i.to_string())).collect()))];
    if let Some(t) = target {
        o.push(("element", s(&t.to_string())));
    }
    obj(o)
}

/// A journal stub: each entry is one committed event as canonical JSON.
#[derive(Debug, Clone, Default, PartialEq)]
struct Journal(Vec<Vec<u8>>);

impl Journal {
    fn head(&self) -> (usize, Option<Vec<u8>>) {
        (self.0.len(), self.0.last().cloned())
    }
    fn events(&self) -> Vec<EdgesSet> {
        self.0.iter().map(|b| EdgesSet::from_value(&parse(b).unwrap()).unwrap()).collect()
    }
}

/// A project: edge index plus journal; commits append only on success.
#[derive(Debug, Clone, Default)]
struct Project {
    index: EdgeIndex,
    journal: Journal,
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
    fn run(&mut self, op: Op, target: &str, inputs: &[&str]) -> Result<Vec<EdgesSet>, EdgeError> {
        let t = self.id(target);
        let ins: Vec<EntityId> = inputs.iter().map(|n| self.id(n)).collect();
        let params = num_params((op == Op::Update).then_some(t), &ins);
        let events = self.index.commit_command(&num_schema(), &params, &[Output { element: t, op }])?;
        for e in &events {
            self.journal.0.push(to_canonical_bytes(&e.to_value()).unwrap());
        }
        Ok(events)
    }
    fn query(&self, name: &str, transitive: bool) -> (Vec<String>, Vec<String>) {
        let p = ShowDependencies::from_value(&obj(vec![
            ("element", s(&self.id(name).to_string())),
            ("transitive", CValue::Bool(transitive)),
        ]))
        .unwrap();
        let r = show_dependencies(&self.index, &p).unwrap();
        (self.names(&r.dependencies), self.names(&r.dependents))
    }
}

fn chain5() -> Project {
    let mut p = Project::default();
    for t in read_lines("chain5/project.txt") {
        match t[0].as_str() {
            "elem" => {
                let fresh = p.names.insert(t[1].clone(), Uuid::parse(&t[2]).unwrap()).is_none();
                assert!(fresh, "duplicate elem name {}", t[1]);
            }
            "create" => {
                let ins: Vec<&str> = t[2..].iter().map(String::as_str).collect();
                p.run(Op::Create, &t[1], &ins).unwrap();
            }
            other => panic!("unknown line {other}"),
        }
    }
    p
}

fn expected(rel: &str) -> Vec<Vec<String>> {
    read_lines(rel)
}

fn expected_digest(rel: &str, stage: &str) -> String {
    expected(rel).into_iter().find(|t| t[0] == "digest" && t[1] == stage).unwrap()[2].clone()
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// Checks every `query <stage> ...` line of chain5/expected.txt.
fn check_queries(p: &Project, stage: &str) {
    let mut n = 0;
    for t in expected("chain5/expected.txt") {
        if t[0] != "query" || t[1] != stage {
            continue;
        }
        let (deps, dents) = p.query(&t[2], t[3] == "transitive");
        let got = if t[4] == "dependencies" { deps } else { dents };
        assert_eq!(got, t[5..].to_vec(), "{t:?}");
        n += 1;
    }
    assert_eq!(n, 20, "every element, both modes, both directions");
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_direct_and_transitive_dependencies() {
    let p = chain5();
    assert_eq!(p.query("c", false), (strs(&["b"]), strs(&["d"])));
    assert_eq!(p.query("c", true), (strs(&["a", "b"]), strs(&["d", "e"])));
    check_queries(&p, "load");
    assert_eq!(p.index.digest().unwrap().as_str(), expected_digest("chain5/expected.txt", "load"));
}

#[test]
fn ac1_result_is_the_pinned_json() {
    let p = chain5();
    let params = parse(format!(r#"{{"element":"{}"}}"#, p.id("c")).as_bytes()).unwrap();
    let r = show_dependencies(&p.index, &ShowDependencies::from_value(&params).unwrap()).unwrap();
    let json = String::from_utf8(to_canonical_bytes(&r.to_value()).unwrap()).unwrap();
    assert_eq!(
        json,
        r#"{"dependencies":["00000000-0000-7000-8000-00000000000b"],"dependents":["00000000-0000-7000-8000-00000000000d"]}"#
    );
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_cycle_is_rejected_and_nothing_is_committed() {
    let mut p = chain5();
    let before_index = p.index.clone();
    let before_head = p.journal.head();
    let err = p.run(Op::Update, "a", &["e"]).unwrap_err();
    assert_eq!(err.code(), "cycle");
    let EdgeError::Cycle(ids) = &err else { panic!("{err:?}") };
    assert_eq!(p.names(ids), strs(&["a", "b", "c", "d", "e"]));
    let line = expected("chain5/expected.txt").into_iter().find(|t| t[0] == "reject").unwrap();
    assert_eq!(p.names(ids), line[6..].to_vec());
    assert_eq!(p.journal.head(), before_head);
    assert_eq!(p.index, before_index);
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_update_replaces_incoming_edges() {
    let mut p = chain5();
    let events = p.run(Op::Update, "c", &["a"]).unwrap();
    assert_eq!(events, vec![EdgesSet::new(p.id("c"), vec![p.id("a")]).unwrap()]);
    assert_eq!(p.query("c", false).0, strs(&["a"]));
    assert_eq!(p.query("b", false).1, Vec::<String>::new());
    check_queries(&p, "ac3");
    assert_eq!(p.index.digest().unwrap().as_str(), expected_digest("chain5/expected.txt", "ac3"));
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_reopen_rebuilds_an_identical_index() {
    let mut p = chain5();
    p.run(Op::Update, "c", &["a"]).unwrap();
    let saved: Vec<u8> = p.journal.0.join(&b'\n');
    // "Close and reopen": only the journal bytes survive.
    let reopened = Journal(saved.split(|b| *b == b'\n').map(<[u8]>::to_vec).collect());
    let index = EdgeIndex::rebuild(&reopened.events()).unwrap();
    assert_eq!(index, p.index);
    assert_eq!(index.digest().unwrap(), p.index.digest().unwrap());
    assert_eq!(index.digest().unwrap().as_str(), expected_digest("chain5/expected.txt", "ac3"));
    assert_eq!(EDGES_SET_EVENT, "dep.edges_set@1");
}

// ------------------------------------------------------------------ AC-5

struct Recipe {
    elements: u64,
    seed: u64,
    max_inputs: u64,
    extra: Vec<u64>,
}

fn recipe() -> Recipe {
    let mut m = BTreeMap::new();
    for t in read_lines("wide_100k/recipe.txt") {
        assert!(m.insert(t[0].clone(), t[1..].to_vec()).is_none(), "duplicate key {}", t[0]);
    }
    let one = |k: &str| m[k][0].parse::<u64>().unwrap();
    let extra: Vec<u64> = m["extra"][1..].iter().map(|x| x.parse().unwrap()).collect();
    assert_eq!(extra.len() as u64, one("extra"));
    Recipe { elements: one("elements"), seed: one("seed"), max_inputs: one("max_inputs"), extra }
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
        let ins: std::collections::BTreeSet<EntityId> = (0..k).map(|_| wide_id(next() % i)).collect();
        index.apply(&[EdgesSet::from_set(wide_id(i), &ins).unwrap()]).unwrap();
    }
    index
}

#[test]
fn ac5_one_element_with_ten_inputs_on_100k_under_10_ms() {
    let r = recipe();
    let index = wide_index(&r);
    let exp = expected("wide_100k/expected.txt");
    let num = |key: &str| exp.iter().find(|t| t[0] == key).unwrap()[1].parse::<usize>().unwrap();
    assert_eq!(index.len(), 100_000);
    assert_eq!(index.edge_count(), num("edges"));
    assert_eq!(index.digest().unwrap().as_str(), expected_digest("wide_100k/expected.txt", "wide"));

    let new = wide_id(r.elements);
    let inputs: Vec<EntityId> = r.extra.iter().map(|i| wide_id(*i)).collect();
    assert_eq!(inputs.len(), 10);
    let params = num_params(None, &inputs);
    let out = [Output { element: new, op: Op::Create }];
    // Best of three fresh copies, so one scheduler hiccup on a shared CI runner does not fail it.
    let mut best = Duration::MAX;
    let mut done = None;
    for _ in 0..3 {
        let mut copy = index.clone();
        let t = Instant::now();
        let events = copy.commit_command(&num_schema(), &params, &out).unwrap();
        best = best.min(t.elapsed());
        assert_eq!(events.len(), 1);
        done = Some(copy);
    }
    println!("AC-5: best {best:?}");
    assert!(best < Duration::from_millis(10), "{best:?}");
    let done = done.unwrap();
    assert_eq!(done.edge_count(), num("edges_extra"));
    assert_eq!(done.digest().unwrap().as_str(), expected_digest("wide_100k/expected.txt", "extra"));
}

// ------------------------------------------------------------------ determinism

fn id(n: u128) -> EntityId {
    Uuid::from_u128(0x0000_0000_0000_7000_8000_0000_0000_0000 | n)
}

fn set(e: u128, ins: &[u128]) -> EdgesSet {
    EdgesSet::new(id(e), ins.iter().map(|i| id(*i)).collect()).unwrap()
}

/// Diamond 1 -> {2, 3} -> 4, hand-labelled.
fn diamond() -> EdgeIndex {
    let mut x = EdgeIndex::new();
    x.apply(&[set(1, &[])]).unwrap();
    x.apply(&[set(2, &[1]), set(3, &[1])]).unwrap();
    x.apply(&[set(4, &[2, 3])]).unwrap();
    x
}

#[test]
fn cycle_report_follows_the_pinned_dfs_order() {
    let mut x = diamond();
    // DFS from 1 visits 2 before 3, so the path goes through 2.
    assert_eq!(x.apply(&[set(1, &[4])]).unwrap_err(), EdgeError::Cycle(vec![id(1), id(2), id(4)]));
    // Several new inputs: the first one reached in DFS preorder wins (4 via 2, not 3).
    assert_eq!(x.apply(&[set(1, &[3, 4])]).unwrap_err(), EdgeError::Cycle(vec![id(1), id(2), id(4)]));
    // Direct closing edge: 2 -> 4 -> 2.
    assert_eq!(x.apply(&[set(2, &[1, 4])]).unwrap_err(), EdgeError::Cycle(vec![id(2), id(4)]));
    assert_eq!(x, diamond());
}

#[test]
fn cycle_report_starts_at_the_smallest_id() {
    let mut x = EdgeIndex::new();
    x.apply(&[set(5, &[])]).unwrap();
    x.apply(&[set(2, &[5])]).unwrap();
    x.apply(&[set(3, &[2])]).unwrap();
    // DFS path from 5 is [5, 2, 3]; the cycle 5 -> 2 -> 3 -> 5 is listed from 2.
    assert_eq!(x.apply(&[set(5, &[3])]).unwrap_err(), EdgeError::Cycle(vec![id(2), id(3), id(5)]));
}

#[test]
fn self_edge_is_a_cycle_of_one() {
    let mut x = diamond();
    assert_eq!(x.apply(&[set(4, &[4])]).unwrap_err(), EdgeError::Cycle(vec![id(4)]));
    assert_eq!(x, diamond());
}

#[test]
fn insertion_order_does_not_change_the_index() {
    let mut a = EdgeIndex::new();
    a.apply(&[set(9, &[]), set(5, &[]), set(7, &[])]).unwrap();
    a.apply(&[set(8, &[5, 9])]).unwrap();
    let mut b = EdgeIndex::new();
    b.apply(&[set(7, &[])]).unwrap();
    b.apply(&[set(5, &[])]).unwrap();
    b.apply(&[set(9, &[])]).unwrap();
    b.apply(&[set(8, &[5, 7])]).unwrap();
    b.apply(&[set(8, &[5, 9])]).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.digest().unwrap(), b.digest().unwrap());
    let edges: Vec<_> = a.edges().collect();
    assert_eq!(edges, vec![(id(5), id(8)), (id(9), id(8))]);
}

#[test]
fn commit_events_are_in_ascending_element_order() {
    let mut x = EdgeIndex::new();
    let events = x.apply(&[set(3, &[]), set(1, &[]), set(2, &[])]).unwrap();
    let order: Vec<EntityId> = events.iter().map(EdgesSet::element).collect();
    assert_eq!(order, vec![id(1), id(2), id(3)]);
    assert_eq!(EdgeIndex::rebuild(&events).unwrap(), x);
}

// ------------------------------------------------------------------ fail closed

#[test]
fn dangling_edge_is_not_found() {
    let mut x = diamond();
    let e = x.apply(&[set(5, &[1, 99])]).unwrap_err();
    assert_eq!(e, EdgeError::DanglingInput { element: id(5), input: id(99) });
    assert_eq!((e.code(), e.reason()), ("not_found", "dangling_input"));
    assert_eq!(x, diamond());
}

#[test]
fn duplicate_and_unsorted_edges_are_refused() {
    let e = EdgesSet::new(id(5), vec![id(1), id(1)]).unwrap_err();
    assert_eq!(e, EdgeError::DuplicateEdge { element: id(5), input: id(1) });
    assert_eq!(e.code(), "duplicate_edge");
    let e = EdgesSet::new(id(5), vec![id(2), id(1)]).unwrap_err();
    assert_eq!(e.code(), "unsorted_inputs");
    let too_many: Vec<EntityId> = (0..=MAX_INPUTS as u128).map(id).collect();
    assert_eq!(EdgesSet::new(id(u128::from(u32::MAX)), too_many).unwrap_err().code(), "input_limit");
}

#[test]
fn duplicate_elements_in_one_commit_are_refused() {
    let mut x = diamond();
    let e = x.apply(&[set(5, &[1]), set(5, &[2])]).unwrap_err();
    assert_eq!(e, EdgeError::DuplicateElement(id(5)));
    let outs = [Output { element: id(5), op: Op::Create }, Output { element: id(5), op: Op::Create }];
    let e = x.commit_command(&num_schema(), &num_params(None, &[]), &outs).unwrap_err();
    assert_eq!(e.code(), "duplicate_element");
    assert_eq!(x, diamond());
}

#[test]
fn failed_commit_rolls_back_sets_already_applied() {
    let mut x = EdgeIndex::new();
    x.apply(&[set(1, &[]), set(2, &[])]).unwrap();
    let before = x.clone();
    // Sets apply in element order: 1 gets input 2 first, then 2 getting input 1 closes 1 -> 2 -> 1.
    let e = x.apply(&[set(2, &[1]), set(1, &[2])]).unwrap_err();
    assert_eq!(e, EdgeError::Cycle(vec![id(1), id(2)]));
    assert_eq!(x, before);
    // A new element applied first, then a dangling one: the new element is removed again.
    let e = x.apply(&[set(3, &[1]), set(4, &[77])]).unwrap_err();
    assert_eq!(e.code(), "not_found");
    assert_eq!(x, before);
    assert!(!x.contains(&id(3)));
}

#[test]
fn create_and_update_are_checked_against_the_index() {
    let mut x = diamond();
    let create = [Output { element: id(1), op: Op::Create }];
    let e = x.commit_command(&num_schema(), &num_params(None, &[]), &create).unwrap_err();
    assert_eq!(e, EdgeError::AlreadyExists(id(1)));
    let update = [Output { element: id(50), op: Op::Update }];
    let e = x.commit_command(&num_schema(), &num_params(Some(id(50)), &[]), &update).unwrap_err();
    assert_eq!((e.code(), e.reason()), ("not_found", "unknown_element"));
    assert_eq!(x, diamond());
}

#[test]
fn xref_walk_finds_nested_references_and_skips_outputs() {
    let mut x = diamond();
    let schema = obj(vec![
        ("type", s("object")),
        (
            "properties",
            obj(vec![
                ("target", ref_schema()),
                ("label", obj(vec![("type", s("string"))])),
                (
                    "pairs",
                    obj(vec![
                        ("type", s("array")),
                        (
                            "items",
                            obj(vec![
                                ("type", s("object")),
                                ("properties", obj(vec![("a", ref_schema()), ("b", ref_schema())])),
                            ]),
                        ),
                    ]),
                ),
            ]),
        ),
    ]);
    let params = obj(vec![
        ("target", s(&id(4).to_string())),
        ("label", s("not a reference")),
        (
            "pairs",
            CValue::Arr(vec![
                obj(vec![("a", s(&id(3).to_string())), ("b", s(&id(1).to_string()))]),
                obj(vec![("a", s(&id(3).to_string()))]),
            ]),
        ),
    ]);
    let events = x.commit_command(&schema, &params, &[Output { element: id(4), op: Op::Update }]).unwrap();
    // 3 is referenced twice but gives one edge; 4 is the target, not an input.
    assert_eq!(events, vec![set(4, &[1, 3])]);
    assert_eq!(x.dependencies(&id(4)).unwrap(), vec![id(1), id(3)]);
    assert_eq!(x.dependents(&id(2)).unwrap(), Vec::<EntityId>::new());
}

#[test]
fn xref_outside_properties_and_items_is_unsupported() {
    let mut x = diamond();
    let hidden = obj(vec![
        ("type", s("object")),
        ("properties", obj(vec![("p", obj(vec![("anyOf", CValue::Arr(vec![ref_schema()]))]))])),
    ]);
    let e = x.commit_command(&hidden, &obj(vec![]), &[Output { element: id(6), op: Op::Create }]).unwrap_err();
    assert_eq!(e.code(), "unsupported_schema");
    let extra = obj(vec![("type", s("object")), ("additionalProperties", ref_schema())]);
    let e = x.commit_command(&extra, &obj(vec![]), &[Output { element: id(6), op: Op::Create }]).unwrap_err();
    assert_eq!(e.code(), "unsupported_schema");
    let not_string = obj(vec![("type", s("integer")), ("x-ref", s("element"))]);
    let e = x.commit_command(&not_string, &CValue::Num(1.0), &[Output { element: id(6), op: Op::Create }]);
    assert_eq!(e.unwrap_err().code(), "unsupported_schema");
    assert_eq!(x, diamond());
}

#[test]
fn bad_reference_values_are_refused() {
    let mut x = diamond();
    let out = [Output { element: id(6), op: Op::Create }];
    let upper = id(1).to_string().to_uppercase();
    let p = obj(vec![("inputs", CValue::Arr(vec![s(&upper)]))]);
    let e = x.commit_command(&num_schema(), &p, &out).unwrap_err();
    assert_eq!(e, EdgeError::BadReference("/inputs/0".to_string()));
    let p = obj(vec![("inputs", CValue::Arr(vec![CValue::Null]))]);
    assert_eq!(x.commit_command(&num_schema(), &p, &out).unwrap_err().code(), "bad_reference");
    assert_eq!(x, diamond());
}

#[test]
fn walkers_are_bounded() {
    let mut x = diamond();
    let out = [Output { element: id(6), op: Op::Create }];
    let big = obj(vec![("value", CValue::Arr(vec![CValue::Null; MAX_WALK_NODES + 1]))]);
    assert_eq!(x.commit_command(&num_schema(), &big, &out).unwrap_err(), EdgeError::InputLimit("param_nodes"));
    let mut deep = ref_schema();
    for _ in 0..=MAX_SCHEMA_DEPTH {
        deep = obj(vec![("type", s("object")), ("properties", obj(vec![("p", deep)]))]);
    }
    assert_eq!(x.commit_command(&deep, &obj(vec![]), &out).unwrap_err(), EdgeError::InputLimit("schema_depth"));
    assert_eq!(x, diamond());
}

#[test]
fn events_round_trip_and_fail_closed() {
    let e = set(4, &[2, 3]);
    let bytes = to_canonical_bytes(&e.to_value()).unwrap();
    assert_eq!(
        String::from_utf8(bytes.clone()).unwrap(),
        r#"{"element":"00000000-0000-7000-8000-000000000004","inputs":["00000000-0000-7000-8000-000000000002","00000000-0000-7000-8000-000000000003"]}"#
    );
    assert_eq!(EdgesSet::from_value(&parse(&bytes).unwrap()).unwrap(), e);
    let bad = [
        (r#"[]"#, "bad_event"),
        (r#"{"element":"00000000-0000-7000-8000-000000000004"}"#, "bad_event"),
        (r#"{"element":"00000000-0000-7000-8000-000000000004","inputs":[],"x":1}"#, "bad_event"),
        (r#"{"element":"00000000-0000-7000-8000-00000000000A","inputs":[]}"#, "bad_event"),
        (r#"{"element":"00000000-0000-7000-8000-000000000004","inputs":[1]}"#, "bad_event"),
        (
            r#"{"element":"00000000-0000-7000-8000-000000000004","inputs":["00000000-0000-7000-8000-000000000002","00000000-0000-7000-8000-000000000002"]}"#,
            "duplicate_edge",
        ),
        (
            r#"{"element":"00000000-0000-7000-8000-000000000004","inputs":["00000000-0000-7000-8000-000000000003","00000000-0000-7000-8000-000000000002"]}"#,
            "unsorted_inputs",
        ),
    ];
    for (text, code) in bad {
        assert_eq!(EdgesSet::from_value(&parse(text.as_bytes()).unwrap()).unwrap_err().code(), code, "{text}");
    }
}

#[test]
fn rebuild_refuses_a_journal_that_was_never_valid() {
    // Dangling: 2 before 1 exists.
    assert_eq!(EdgeIndex::rebuild(&[set(2, &[1]), set(1, &[])]).unwrap_err().code(), "not_found");
    // A cycle smuggled in through events.
    let events = [set(1, &[]), set(2, &[1]), set(1, &[2])];
    assert_eq!(EdgeIndex::rebuild(&events).unwrap_err(), EdgeError::Cycle(vec![id(1), id(2)]));
}

// ------------------------------------------------------------------ query and registry

#[test]
fn query_params_fail_closed() {
    let cases = [
        (r#"[]"#, "bad_params"),
        (r#"{}"#, "bad_params"),
        (r#"{"element":"00000000-0000-7000-8000-000000000001","depth":2}"#, "bad_params"),
        (r#"{"element":"00000000-0000-7000-8000-000000000001","transitive":"yes"}"#, "bad_params"),
        (r#"{"element":"not-an-id"}"#, "bad_params"),
    ];
    for (text, code) in cases {
        assert_eq!(ShowDependencies::from_value(&parse(text.as_bytes()).unwrap()).unwrap_err().code(), code, "{text}");
    }
    let p = ShowDependencies::from_value(&parse(br#"{"element":"00000000-0000-7000-8000-000000000063"}"#).unwrap())
        .unwrap();
    assert!(!p.transitive);
    let e = show_dependencies(&diamond(), &p).unwrap_err();
    assert_eq!((e.code(), e.reason()), ("not_found", "unknown_element"));
}

#[test]
fn transitive_query_on_the_diamond() {
    let x = diamond();
    let p = ShowDependencies { element: id(4), transitive: true };
    let r = show_dependencies(&x, &p).unwrap();
    assert_eq!(r.dependencies, vec![id(1), id(2), id(3)]);
    assert!(r.dependents.is_empty());
    let r = show_dependencies(&x, &ShowDependencies { element: id(1), transitive: true }).unwrap();
    assert_eq!(r.dependents, vec![id(2), id(3), id(4)]);
}

#[test]
fn command_definition_is_registered_as_pinned() {
    let def = command_def().unwrap();
    assert_eq!(format!("{}@{}", def.id, def.major), SHOW_DEPENDENCIES);
    assert!(CommandId::parse(SHOW_DEPENDENCIES).is_ok());
    assert_eq!(def.side_effect.as_str(), "R0");
    assert_eq!(def.determinism.as_str(), "D0");
    assert!(def.effects.is_empty());
    let CValue::Obj(o) = &def.params_schema else { panic!() };
    let CValue::Obj(props) = &o["properties"] else { panic!() };
    let CValue::Obj(element) = &props["element"] else { panic!() };
    assert_eq!(element["x-ref"], s("element"));
}

#[test]
fn error_codes_are_stable() {
    let all = [
        (EdgeError::Cycle(vec![id(1)]), "cycle", "cycle"),
        (EdgeError::UnknownElement(id(1)), "not_found", "unknown_element"),
        (EdgeError::DanglingInput { element: id(1), input: id(2) }, "not_found", "dangling_input"),
        (EdgeError::AlreadyExists(id(1)), "already_exists", "already_exists"),
        (EdgeError::DuplicateElement(id(1)), "duplicate_element", "duplicate_element"),
        (EdgeError::DuplicateEdge { element: id(1), input: id(2) }, "duplicate_edge", "duplicate_edge"),
        (EdgeError::UnsortedInputs(id(1)), "unsorted_inputs", "unsorted_inputs"),
        (EdgeError::BadReference("/x".into()), "bad_reference", "bad_reference"),
        (EdgeError::UnsupportedSchema("x"), "unsupported_schema", "unsupported_schema"),
        (EdgeError::BadParams("x"), "bad_params", "bad_params"),
        (EdgeError::BadEvent("x"), "bad_event", "bad_event"),
        (EdgeError::InputLimit("x"), "input_limit", "input_limit"),
        (EdgeError::Internal("x"), "internal", "internal"),
    ];
    for (e, code, reason) in all {
        assert_eq!((e.code(), e.reason()), (code, reason));
        assert!(e.to_string().starts_with(&format!("{code}/{reason}")));
    }
    assert_eq!(
        EdgeError::Cycle(vec![id(1), id(2)]).to_string(),
        "cycle/cycle: [00000000-0000-7000-8000-000000000001, 00000000-0000-7000-8000-000000000002]"
    );
}

// ------------------------------------------------------------------ review follow-ups

/// An index of `n` sources `id(1) ..= id(n)` plus `target` (no inputs).
fn sources(n: u128, target: u128) -> EdgeIndex {
    let mut x = EdgeIndex::new();
    let mut sets: Vec<EdgesSet> = (1..=n).map(|i| set(i, &[])).collect();
    sets.push(set(target, &[]));
    for chunk in sets.chunks(MAX_COMMIT_ELEMENTS) {
        x.apply(chunk).unwrap();
    }
    x
}

fn update_params(target: u128, inputs: impl Iterator<Item = u128>) -> CValue {
    let ins: Vec<EntityId> = inputs.map(id).collect();
    num_params(Some(id(target)), &ins)
}

#[test]
fn input_limit_counts_inputs_after_removing_outputs() {
    let n = MAX_INPUTS as u128;
    let target = 1_000_000;
    let mut x = sources(n + 1, target);
    let out = [Output { element: id(target), op: Op::Update }];
    // Exactly MAX_INPUTS real inputs plus a self-reference (the `element` param): accepted.
    let events = x.commit_command(&num_schema(), &update_params(target, 1..=n), &out).unwrap();
    assert_eq!(events[0].inputs().len(), MAX_INPUTS);
    // The self-reference listed among the inputs too: still MAX_INPUTS real inputs.
    let with_self = update_params(target, (1..=n).chain(std::iter::once(target)));
    x.commit_command(&num_schema(), &with_self, &out).unwrap();
    // One real input more: refused, nothing changes.
    let before = x.clone();
    let e = x.commit_command(&num_schema(), &update_params(target, 1..=n + 1), &out).unwrap_err();
    assert_eq!(e, EdgeError::InputLimit("inputs"));
    assert_eq!(x, before);
}

#[test]
fn deep_chain_cycle_is_reported_without_recursion() {
    const N: u128 = 100_000;
    let mut x = EdgeIndex::new();
    x.apply(&[set(1, &[])]).unwrap();
    for i in 2..=N {
        x.apply(&[set(i, &[i - 1])]).unwrap();
    }
    let before = x.clone();
    let t = Instant::now();
    let e = x.apply(&[set(1, &[N])]).unwrap_err();
    println!("deep chain: {N} elements, cycle check {:?}", t.elapsed());
    let EdgeError::Cycle(ids) = &e else { panic!("{e:?}") };
    assert_eq!(ids.len(), N as usize);
    assert_eq!(*ids, (1..=N).map(id).collect::<Vec<_>>());
    assert_eq!(x, before);
    let t = Instant::now();
    let r = show_dependencies(&x, &ShowDependencies { element: id(1), transitive: true }).unwrap();
    println!("deep chain: transitive dependents {:?}", t.elapsed());
    assert_eq!(r.dependents.len(), N as usize - 1);
}

#[test]
fn output_errors_do_not_depend_on_listing_order() {
    let x0 = diamond();
    let create = |e| Output { element: id(e), op: Op::Create };
    let update = |e| Output { element: id(e), op: Op::Update };
    let cases: [(Vec<Output>, EdgeError); 3] = [
        // Two duplicated elements: the smaller one is reported.
        (vec![create(9), create(8), create(9), create(8)], EdgeError::DuplicateElement(id(8))),
        // Create of a known element (3) and update of an unknown one (7): 3 is smaller.
        (vec![update(7), create(3)], EdgeError::AlreadyExists(id(3))),
        (vec![create(6), update(5)], EdgeError::UnknownElement(id(5))),
    ];
    for (outs, want) in cases {
        for rev in [false, true] {
            let mut list = outs.clone();
            if rev {
                list.reverse();
            }
            let mut x = x0.clone();
            assert_eq!(x.commit_command(&num_schema(), &num_params(None, &[]), &list).unwrap_err(), want);
            assert_eq!(x, x0);
        }
    }
}

#[test]
fn a_parameter_named_x_ref_is_refused_as_an_annotation() {
    let mut x = diamond();
    let schema = obj(vec![
        ("type", s("object")),
        ("properties", obj(vec![("x-ref", obj(vec![("type", s("string"))]))])),
    ]);
    let params = obj(vec![("x-ref", s("plain text"))]);
    let e = x.commit_command(&schema, &params, &[Output { element: id(6), op: Op::Create }]).unwrap_err();
    assert_eq!(e.code(), "unsupported_schema");
    assert_eq!(x, diamond());
}
