//! Acceptance criteria of spec node CORE.PRJ.STAGE.SWITCH on the model (AC-1 .. AC-5) with
//! fixtures from `fixtures/core/prj/stage/switch/make_fixtures.py` (independent Python
//! reference): the switch moves no data, values are read by slot digest through a cache that
//! is never trusted, and nothing is recomputed. The bus side is tested in `fid-project-cmd`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_project::{
    set_active_stage, value_digest, ContainerView, ElementTypeDef, ElementTypeRegistry, MemoryValueCache, Tree,
    TreeError, TreeEvent, Uuid, ValueCache, ValueError, ValueStore, ACTIVE_STAGE_SET, STAGE_ADDED,
};
use fid_project::{StageContext, ELEMENT_CREATED};
use fid_types::Digest;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::Arc;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/prj/stage/switch/");

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

fn tree_event(v: &CValue) -> TreeEvent {
    let o = obj(v);
    TreeEvent::new(st(o, "type"), get(o, "body").clone())
}

fn ev_value(e: &TreeEvent) -> CValue {
    let mut o = Obj::new();
    o.insert("body".into(), e.body.clone());
    o.insert("type".into(), CValue::Str(e.event_type.clone()));
    CValue::Obj(o)
}

fn setup() -> CValue {
    parse(&read("p3s/setup.json")).unwrap()
}

fn cases() -> CValue {
    parse(&read("cases.json")).unwrap()
}

fn namespace() -> Uuid {
    uuid(obj(&setup()), "namespace")
}

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

fn p3s_events() -> Vec<TreeEvent> {
    String::from_utf8(read("p3s/events.jsonl"))
        .unwrap()
        .lines()
        .map(|l| tree_event(&parse(l.as_bytes()).unwrap()))
        .collect()
}

fn p3s() -> Project {
    let mut p = empty();
    for e in p3s_events() {
        p.apply(&e).unwrap();
    }
    p
}

/// The content-addressed store of p3s's values, counting loads.
struct Store {
    values: BTreeMap<Digest, Vec<u8>>,
    loads: Cell<u64>,
}

impl Store {
    fn p3s() -> Store {
        let v = parse(&read("p3s/values.json")).unwrap();
        let values = obj(&v)
            .iter()
            .map(|(d, t)| match t {
                CValue::Str(t) => (Digest::parse(d).unwrap(), t.as_bytes().to_vec()),
                other => panic!("{other:?}"),
            })
            .collect();
        Store { values, loads: Cell::new(0) }
    }
}

impl ValueStore for Store {
    fn load(&self, digest: &Digest) -> Result<Vec<u8>, ValueError> {
        self.loads.set(self.loads.get() + 1);
        self.values.get(digest).cloned().ok_or(ValueError::BlobMissing(*digest))
    }
}

fn stages() -> Vec<Uuid> {
    let s = setup();
    arr(obj(&s), "stages")
        .iter()
        .map(|v| match v {
            CValue::Str(x) => Uuid::parse(x).unwrap(),
            other => panic!("{other:?}"),
        })
        .collect()
}

fn model_code(code: &str, reason: &str) -> &'static str {
    match (code, reason) {
        ("validation", "schema") => "schema",
        ("validation", "bad_stage") => "bad_stage",
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn p3s_matches_the_reference() {
    let p = p3s();
    let expected = String::from_utf8(read("p3s/expected_digest.txt")).unwrap();
    assert_eq!(p.digest(), expected.trim_end());
    p.tree.verify().unwrap();
    let s = stages();
    assert_eq!(p.tree.live_stages(), s);
    assert_eq!(p.tree.active_stage(), s[0]);
    // Every slot is current with a distinct value; the store holds all 150.
    let store = Store::p3s();
    assert_eq!(store.values.len(), 150);
    for (d, b) in &store.values {
        assert_eq!(value_digest(b), *d);
    }
}

/// AC-1: switching to S2 is one `ActiveStageSet` event that moves no data: every per-stage
/// digest is unchanged, e1 now presents S2's value, read by slot digest (no recompute path).
#[test]
fn ac1_switch_presents_the_new_stage() {
    let c = cases();
    let ac1 = obj(get(obj(&c), "ac1"));
    let mut p = p3s();
    let s = stages();
    let before: Vec<Digest> = s.iter().map(|x| p.tree.stage_digest(x).unwrap()).collect();
    let e = set_active_stage(&p, get(ac1, "params"), namespace()).unwrap();
    assert_eq!(e.event_type, ACTIVE_STAGE_SET);
    assert_eq!(canon(&ev_value(&e)), canon(get(ac1, "event")));
    p.apply(&e).unwrap();
    p.tree.verify().unwrap();
    assert_eq!(p.digest(), st(ac1, "digest_after"));
    assert_eq!(p.tree.active_stage(), s[1]);
    let after: Vec<Digest> = s.iter().map(|x| p.tree.stage_digest(x).unwrap()).collect();
    assert_eq!(before, after);
    let digests = obj(get(ac1, "stage_digests"));
    for (x, d) in s.iter().zip(&after) {
        assert_eq!(d.as_str(), st(digests, &x.to_string()));
    }
    let (_, body) = p.tree.entity(&p.tree.stage_list_id()).unwrap();
    assert_eq!(canon(body), canon(get(ac1, "list_entity")));

    let store = Store::p3s();
    let mut cache = MemoryValueCache::new(1 << 20);
    let e1 = uuid(ac1, "e1");
    let mut ctx = StageContext::new(&p.tree);
    assert_eq!(ctx.active(), s[1]);
    let v = ctx.value(&e1, &mut cache, &store).unwrap();
    assert_eq!(v, st(ac1, "e1_value").as_bytes());
    assert_eq!(ctx.slot(&e1, &s[1]).unwrap().digest.unwrap().as_str(), st(ac1, "e1_digest"));
    assert_eq!(ctx.value_in(&e1, &s[0], &mut cache, &store).unwrap(), st(ac1, "e1_value_s1").as_bytes());
    // The second read of e1 in S2 is a verified cache hit; nothing else ran.
    assert_eq!(ctx.value(&e1, &mut cache, &store).unwrap(), st(ac1, "e1_value").as_bytes());
    let stats = ctx.stats();
    assert_eq!((stats.loads, stats.hits, stats.stale), (2, 1, 0));
    assert_eq!(store.loads.get(), 2);

    // Back to S1: the state is p3s again.
    let back = TreeEvent::new(ACTIVE_STAGE_SET, parse(format!(r#"{{"stage":"{}"}}"#, s[0]).as_bytes()).unwrap());
    p.apply(&back).unwrap();
    assert_eq!(p.digest(), st(ac1, "back_to_s1_digest"));
}

/// AC-3: a removed, unknown or non-stage target is `bad_stage`; a malformed one `schema`;
/// nothing changes. The active stage itself is accepted.
#[test]
fn ac3_refusals() {
    let c = cases();
    let ac3 = obj(get(obj(&c), "ac3"));
    let mut p = p3s();
    for e in arr(ac3, "setup") {
        p.apply(&tree_event(e)).unwrap();
    }
    assert_eq!(p.digest(), st(ac3, "digest_before"));
    for case in arr(ac3, "errors") {
        let o = obj(case);
        let name = st(o, "name");
        let err = set_active_stage(&p, get(o, "params"), namespace()).err().unwrap_or_else(|| panic!("{name}"));
        assert_eq!(err.code(), model_code(st(o, "code"), st(o, "reason")), "{name}: {err}");
    }
    assert_eq!(p.digest(), st(ac3, "digest_before"));
    // A journaled switch to a removed stage is refused by the tree too.
    let removed = obj(&arr(ac3, "errors")[0]);
    let bad = TreeEvent::new(ACTIVE_STAGE_SET, get(removed, "params").clone());
    assert_eq!(p.apply(&bad).unwrap_err().code(), "bad_stage");
    assert_eq!(p.digest(), st(ac3, "digest_before"));
    let same = obj(get(ac3, "same_stage"));
    let e = set_active_stage(&p, get(same, "params"), namespace()).unwrap();
    assert_eq!(canon(&ev_value(&e)), canon(get(same, "event")));
    p.apply(&e).unwrap();
    assert_eq!(p.digest(), st(same, "digest_after"));
}

/// AC-4: with the cache cleared, values come from the store and equal those before; no
/// handler runs. A damaged cache entry is detected and never served; a damaged or missing
/// blob is an error, not a recompute; a slot without a digest has no value.
#[test]
fn ac4_cache_is_never_a_source_of_truth() {
    let mut p = p3s();
    let s = stages();
    let store = Store::p3s();
    let mut cache = MemoryValueCache::new(1 << 20);
    let mut first = Vec::new();
    {
        let mut ctx = StageContext::new(&p.tree);
        for e in p.tree.elements() {
            first.push(ctx.value(&e.id(), &mut cache, &store).unwrap());
        }
        assert_eq!(ctx.stats().loads, 50);
    }
    cache.clear();
    assert!(cache.is_empty());
    {
        let mut ctx = StageContext::new(&p.tree);
        let again: Vec<Vec<u8>> = p.tree.elements().map(|e| ctx.value(&e.id(), &mut cache, &store).unwrap()).collect();
        assert_eq!(again, first);
        assert_eq!((ctx.stats().loads, ctx.stats().hits), (50, 0));
    }
    // A stale or damaged cache entry is detected and replaced from the store.
    let e0 = p.tree.elements().next().unwrap().id();
    let d0 = p.tree.element(&e0).unwrap().slots()[&s[0]].digest.unwrap();
    cache.corrupt(&d0, b"not the value".to_vec());
    let mut ctx = StageContext::new(&p.tree);
    assert_eq!(ctx.value(&e0, &mut cache, &store).unwrap(), first[0]);
    assert_eq!((ctx.stats().stale, ctx.stats().loads), (1, 1));
    // The repaired entry is a verified hit afterwards.
    assert_eq!(ctx.value(&e0, &mut cache, &store).unwrap(), first[0]);
    assert_eq!(ctx.stats().hits, 1);
    // Bytes in the store that do not hash to the slot digest are refused.
    let mut bad = Store::p3s();
    bad.values.insert(d0, b"tampered".to_vec());
    let mut empty_cache = MemoryValueCache::new(1 << 20);
    let mut ctx = StageContext::new(&p.tree);
    assert_eq!(ctx.value(&e0, &mut empty_cache, &bad).unwrap_err().code(), "blob_corrupt");
    assert!(empty_cache.is_empty());
    // A missing blob is `blob_missing`.
    let mut gone = Store::p3s();
    gone.values.remove(&d0);
    assert_eq!(ctx.value(&e0, &mut empty_cache, &gone).unwrap_err().code(), "blob_missing");
    // A new element's slot holds no digest: `no_value`, never computed.
    let root = p.tree.root();
    let id = Uuid::from_u128(0x0190_cccc_0000_7000_8000_0000_0000_0001);
    let body =
        format!(r#"{{"id":"{id}","meta":{{}},"name":"new","parent":"{root}","type":"test.point","visible":true}}"#);
    p.apply(&TreeEvent::new(ELEMENT_CREATED, parse(body.as_bytes()).unwrap())).unwrap();
    let mut ctx = StageContext::new(&p.tree);
    assert_eq!(ctx.value(&id, &mut empty_cache, &store).unwrap_err().code(), "no_value");
    assert_eq!(ctx.value(&Uuid::from_u128(5), &mut empty_cache, &store).unwrap_err().code(), "bad_reference");
    let unknown = uuid(obj(&setup()), "unknown_stage");
    assert_eq!(ctx.value_in(&id, &unknown, &mut empty_cache, &store).unwrap_err().code(), "stage_not_found");
}

/// The in-memory cache is bounded and evicts the least recently used entry.
#[test]
fn memory_cache_is_bounded_lru() {
    let mut c = MemoryValueCache::new(10);
    let d = |b: &[u8]| value_digest(b);
    c.put(&d(b"aaaa"), b"aaaa");
    c.put(&d(b"bbbb"), b"bbbb");
    assert!(c.get(&d(b"aaaa")).is_some());
    c.put(&d(b"cccc"), b"cccc");
    assert!(c.get(&d(b"bbbb")).is_none(), "least recently used is evicted");
    assert!(c.get(&d(b"aaaa")).is_some() && c.get(&d(b"cccc")).is_some());
    assert!(c.used() <= 10);
    c.put(&d(b"0123456789ab"), b"0123456789ab");
    assert!(c.get(&d(b"0123456789ab")).is_none(), "larger than the limit is not stored");
    c.remove(&d(b"aaaa"));
    assert_eq!(c.len(), 1);
}

/// A commit holding a switch that is refused later restores the active stage and the stage
/// list; a domain may not register the switch event.
#[test]
fn refused_commit_restores_the_active_stage() {
    let mut p = p3s();
    let s = stages();
    let before = p.digest();
    let (_, list_before) = p.tree.entity(&p.tree.stage_list_id()).map(|(k, b)| (k.to_string(), b.clone())).unwrap();
    let sw = TreeEvent::new(ACTIVE_STAGE_SET, parse(format!(r#"{{"stage":"{}"}}"#, s[2]).as_bytes()).unwrap());
    let bad = TreeEvent::new(ACTIVE_STAGE_SET, parse(br#"{"stage":"x"}"#).unwrap());
    assert_eq!(p.tree.apply_commit(&[sw, bad]).unwrap_err().code(), "schema");
    assert_eq!(p.digest(), before);
    assert_eq!(p.tree.active_stage(), s[0]);
    assert_eq!(canon(p.tree.entity(&p.tree.stage_list_id()).unwrap().1), canon(&list_before));
    p.tree.verify().unwrap();
    let mut reg = ElementTypeRegistry::new();
    assert_eq!(reg.register_event(ACTIVE_STAGE_SET, |_| Ok(Vec::new())).unwrap_err().code(), "duplicate_event");
}

/// AC-5: the journal of AC-1 applied twice from the empty project gives the same digest, the
/// reference's.
#[test]
fn ac5_replay_is_identical() {
    let c = cases();
    let top = obj(&c);
    let mut journal = p3s_events();
    journal.push(tree_event(get(obj(get(top, "ac1")), "event")));
    let run = || {
        let mut p = empty();
        for e in &journal {
            p.apply(e).unwrap();
        }
        p.digest()
    };
    assert_eq!(run(), run());
    assert_eq!(run(), st(obj(get(top, "ac5")), "digest"));
}

/// AC-2 (`bench_switch_100k`): 100,000 elements, 2 stages; applying a switch swaps a pointer
/// only, so it is far inside 200 ms even in a debug build [unverified on reference hardware].
#[test]
fn bench_switch_100k() {
    let mut p = empty();
    let run = Uuid::from_u128(0x0190_dddd_0000_7000_8000_0000_0000_0001);
    let s1 = Uuid::from_u128(0x0190_dddd_0000_7000_8000_0000_0000_0011);
    let s2 = Uuid::from_u128(0x0190_dddd_0000_7000_8000_0000_0000_0012);
    let default = p.tree.default_stage_id();
    let add = |id: Uuid, adopts: Option<Uuid>| {
        let a = adopts.map_or("null".to_string(), |d| format!("\"{d}\""));
        let b = format!(r#"{{"adopts":{a},"id":"{id}","label":"S","metadata":{{}},"run":"{run}","serial":null}}"#);
        TreeEvent::new(STAGE_ADDED, parse(b.as_bytes()).unwrap())
    };
    p.tree.apply_commit(&[add(s1, Some(default)), add(s2, None)]).unwrap();
    let root = p.tree.root().to_string();
    let mut batch = Vec::with_capacity(100_000);
    for i in 0..100_000u128 {
        let mut b = Obj::new();
        b.insert("id".into(), CValue::Str(Uuid::from_u128(0x0190_eeee_0000_7000_8000_0000_0000_0000 | i).to_string()));
        b.insert("meta".into(), CValue::Obj(Obj::new()));
        b.insert("name".into(), CValue::Str(String::new()));
        b.insert("parent".into(), CValue::Str(root.clone()));
        b.insert("type".into(), CValue::Str("test.point".into()));
        b.insert("visible".into(), CValue::Bool(true));
        batch.push(TreeEvent::new(ELEMENT_CREATED, CValue::Obj(b)));
    }
    p.tree.apply_commit(&batch).unwrap();
    assert_eq!(p.tree.element_count(), 100_000);
    let sw = TreeEvent::new(ACTIVE_STAGE_SET, parse(format!(r#"{{"stage":"{s2}"}}"#).as_bytes()).unwrap());
    let start = std::time::Instant::now();
    p.tree.apply(&sw.event_type, &sw.body).unwrap();
    let took = start.elapsed();
    assert_eq!(p.tree.active_stage(), s2);
    assert!(took < std::time::Duration::from_millis(200), "{took:?}");
}
