//! The bus side of spec node CORE.PRJ.STAGE.SWITCH: `project.set_active_stage@1` on the bus
//! view (AC-1, AC-3, AC-5), its definition and schemas (validated with the bus's own
//! `Compiled`), and the value reads over the real `fid-store` blob store and cache (AC-4).
//! Fixtures from `fixtures/core/prj/stage/switch/make_fixtures.py` (independent Python
//! reference). The model rules are tested in `fid-project`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Code, Detail, ValidationReason};
use fid_bus::handler::{effects_check, EntityId, EntityRef, Event, SettingKey, StateView};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse, to_canonical_bytes, CValue, Digest as CanonDigest};
use fid_project::{
    ElementTypeDef, ElementTypeRegistry, ReplaySource, StageContext, Tree, Uuid, ValueError, ACTIVE_STAGE_SET,
};
use fid_project_cmd::stage::{command_defs, result_of, run_stage, StageOp};
use fid_project_cmd::stage_switch::{to_bus_error, value_key, CasValueStore, StoreValueCache};
use fid_project_cmd::tree::TreeState;
use fid_registry::CommandDef;
use fid_store::cache::{Cache, CacheConfig};
use fid_store::cas::{BlobStore, PutOpts};
use fid_types::Digest;
use std::collections::BTreeMap;
use std::path::PathBuf;
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

fn canon(v: &CValue) -> Vec<u8> {
    to_canonical_bytes(v).unwrap()
}

fn cases() -> CValue {
    parse(&read("cases.json")).unwrap()
}

fn setup() -> CValue {
    parse(&read("p3s/setup.json")).unwrap()
}

fn namespace() -> Uuid {
    Uuid::parse(st(obj(&setup()), "namespace")).unwrap()
}

fn event(v: &CValue) -> Event {
    Event::from_value(v.clone()).unwrap()
}

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
    fn head(&self) -> CanonDigest {
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
                self.containers
                    .insert(EntityId::new(st(b, "id")).unwrap(), (st(b, "kind").to_string(), e.body().clone()));
            }
        }
    }

    fn digest(&self) -> String {
        self.tree.tree().state_digest().unwrap().as_str().to_string()
    }
}

fn p3s() -> Project {
    let mut reg = ElementTypeRegistry::new();
    let mut schema = Obj::new();
    schema.insert("type".into(), CValue::Str("object".into()));
    reg.register(ElementTypeDef::new("test.point", CValue::Obj(schema), "point").unwrap()).unwrap();
    reg.register_event("ContainerCreated", |_| Ok(Vec::new())).unwrap();
    let tree = Tree::new(namespace(), Arc::new(reg)).unwrap();
    let mut p = Project { tree: TreeState::new(tree, CanonDigest([0; 32])).unwrap(), containers: BTreeMap::new() };
    for line in String::from_utf8(read("p3s/events.jsonl")).unwrap().lines() {
        p.commit(&[event(&parse(line.as_bytes()).unwrap())]);
    }
    let expected = String::from_utf8(read("p3s/expected_digest.txt")).unwrap();
    assert_eq!(p.digest(), expected.trim_end());
    p
}

fn def() -> CommandDef {
    command_defs().unwrap().into_iter().find(|d| d.id == "project.set_active_stage").unwrap()
}

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no literals")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

fn conforms(schema: &CValue, v: &CValue) -> bool {
    Compiled::compile(schema).unwrap().evaluate(v, &NoHooks).is_ok()
}

/// AC-1 on the bus: one `ActiveStageSet` event (effects check, result, schemas), and the state
/// after it is the reference's; the per-stage data is unchanged.
#[test]
fn ac1_set_active_stage_on_the_bus() {
    let c = cases();
    let ac1 = obj(get(obj(&c), "ac1"));
    let mut p = p3s();
    let d = def();
    assert!(conforms(&d.params_schema, get(ac1, "params")));
    let events =
        run_stage(StageOp::SetActive, &p, get(ac1, "params"), namespace(), &mut ReplaySource::new(Vec::new())).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(canon(&events[0].to_value()), canon(get(ac1, "event")));
    effects_check(&d, &events).unwrap();
    let result = result_of(&events[0]).unwrap();
    assert_eq!(canon(&result), canon(get(ac1, "result")));
    assert!(conforms(&d.result_schema, &result));
    p.commit(&events);
    assert_eq!(p.digest(), st(ac1, "digest_after"));
    let digests = obj(get(ac1, "stage_digests"));
    for s in digests.keys() {
        let id = Uuid::parse(s).unwrap();
        assert_eq!(p.tree.tree().stage_digest(&id).unwrap().as_str(), st(digests, s));
    }
    // The stage list entity the bus view serves names the new active stage.
    let list = EntityId::new(&p.tree.tree().stage_list_id().to_string()).unwrap();
    assert_eq!(canon(p.entity(&list).unwrap().body), canon(get(ac1, "list_entity")));
}

/// REQ-001: W2, D0, undo head_move, effect `ActiveStageSet`, on CLI and MCP.
#[test]
fn definition() {
    let d = def();
    let v = d.to_value();
    let o = obj(&v);
    assert_eq!((st(o, "side_effect"), st(o, "determinism"), st(o, "undo")), ("W2", "D0", "head_move"));
    assert_eq!(d.effects, [ACTIVE_STAGE_SET]);
    assert!(Compiled::compile(&d.params_schema).is_ok() && Compiled::compile(&d.result_schema).is_ok());
}

/// AC-3: every refusal is its catalogue v3 code and reason and nothing is committed; shape
/// refusals fail the params schema, stage refusals pass it (the handler gives `bad_stage`).
#[test]
fn ac3_refusals_on_the_bus() {
    let c = cases();
    let ac3 = obj(get(obj(&c), "ac3"));
    let mut p = p3s();
    for e in arr(ac3, "setup") {
        p.commit(&[event(e)]);
    }
    assert_eq!(p.digest(), st(ac3, "digest_before"));
    let d = def();
    for case in arr(ac3, "errors") {
        let o = obj(case);
        let name = st(o, "name");
        let e = run_stage(StageOp::SetActive, &p, get(o, "params"), namespace(), &mut ReplaySource::new(Vec::new()))
            .err()
            .unwrap_or_else(|| panic!("{name}: accepted"));
        assert_eq!((e.code().as_str(), e.reason()), (st(o, "code"), st(o, "reason")), "{name}");
        assert_eq!(conforms(&d.params_schema, get(o, "params")), st(o, "reason") != "schema", "{name}");
    }
    assert_eq!(p.digest(), st(ac3, "digest_before"));
    // A journaled switch to the removed stage is refused by the state, nothing committed.
    let removed = obj(&arr(ac3, "errors")[0]);
    let mut b = Obj::new();
    b.insert("body".into(), get(removed, "params").clone());
    b.insert("type".into(), CValue::Str(ACTIVE_STAGE_SET.into()));
    let err = p.tree.apply_commit(&[event(&CValue::Obj(b))]).unwrap_err();
    assert_eq!((err.code(), err.reason()), (Code::Validation, "bad_stage"));
    assert_eq!(p.digest(), st(ac3, "digest_before"));
}

/// AC-5: the handler run of AC-1, replayed from p3s, gives identical events and digest.
#[test]
fn ac5_replay_is_identical() {
    let c = cases();
    let ac1 = obj(get(obj(&c), "ac1"));
    let run = || {
        let mut p = p3s();
        let ev = run_stage(StageOp::SetActive, &p, get(ac1, "params"), namespace(), &mut ReplaySource::new(Vec::new()))
            .unwrap();
        p.commit(&ev);
        (canon(&ev[0].to_value()), p.digest())
    };
    assert_eq!(run(), run());
    assert_eq!(run().1, st(obj(get(obj(&c), "ac5")), "digest"));
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-stage-switch-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// AC-4 over the real store: values are put in the content-addressed store (their CAS digest
/// is the slot digest), read through the `fid-store` cache; with the cache cleared they are
/// loaded from the store and equal; a damaged cache item or blob is never served.
#[test]
fn ac4_values_from_the_store_and_cache() {
    let dir = scratch("ac4");
    let store = BlobStore::open(&dir.join("blobs")).unwrap();
    let values = parse(&read("p3s/values.json")).unwrap();
    for (d, t) in obj(&values) {
        let CValue::Str(t) = t else { panic!("{t:?}") };
        let addr = store.put_bytes(t.as_bytes(), PutOpts::default()).unwrap();
        assert_eq!(addr.digest().as_str(), d, "the CAS address is the slot digest");
    }
    let mut p = p3s();
    let c = cases();
    let ac1 = obj(get(obj(&c), "ac1"));
    let ev =
        run_stage(StageOp::SetActive, &p, get(ac1, "params"), namespace(), &mut ReplaySource::new(Vec::new())).unwrap();
    p.commit(&ev);
    let tree = p.tree.tree();
    let mut cache = Cache::open(&dir.join("cache"), CacheConfig::default());
    let cas = CasValueStore(&store);
    let ids: Vec<Uuid> = tree.elements().map(|e| e.id()).collect();
    let first: Vec<Vec<u8>> = {
        let mut ctx = StageContext::new(tree);
        let mut vc = StoreValueCache(&mut cache);
        let v = ids.iter().map(|id| ctx.value(id, &mut vc, &cas).unwrap()).collect();
        assert_eq!(ctx.stats().loads, 50);
        let again: Vec<Vec<u8>> = ids.iter().map(|id| ctx.value(id, &mut vc, &cas).unwrap()).collect();
        assert_eq!(again, v);
        assert_eq!(ctx.stats().hits, 50);
        v
    };
    let e1 = Uuid::parse(st(ac1, "e1")).unwrap();
    let pos = ids.iter().position(|x| *x == e1).unwrap();
    assert_eq!(first[pos], st(ac1, "e1_value").as_bytes());
    // Cache cleared: everything is loaded from the store again, equal.
    drop(cache);
    std::fs::remove_dir_all(dir.join("cache")).unwrap();
    let mut cache = Cache::open(&dir.join("cache"), CacheConfig::default());
    {
        let mut ctx = StageContext::new(tree);
        let mut vc = StoreValueCache(&mut cache);
        let v: Vec<Vec<u8>> = ids.iter().map(|id| ctx.value(id, &mut vc, &cas).unwrap()).collect();
        assert_eq!(v, first);
        assert_eq!((ctx.stats().loads, ctx.stats().hits), (50, 0));
    }
    // A cache item holding other bytes is detected and never served.
    let d1 = Digest::parse(st(ac1, "e1_digest")).unwrap();
    cache.put(&value_key(&d1).unwrap(), b"stale bytes");
    {
        let mut ctx = StageContext::new(tree);
        let mut vc = StoreValueCache(&mut cache);
        assert_eq!(ctx.value(&e1, &mut vc, &cas).unwrap(), st(ac1, "e1_value").as_bytes());
        assert_eq!((ctx.stats().stale, ctx.stats().loads), (1, 1));
    }
    // A damaged blob is `blob_corrupt`, a missing one `blob_missing`; both internal/storage.
    let mut nocache = Cache::disabled();
    let path = store.path_of(&d1);
    let good = std::fs::read(&path).unwrap();
    std::fs::write(&path, vec![b'x'; good.len()]).unwrap();
    let mut ctx = StageContext::new(tree);
    let err = ctx.value(&e1, &mut StoreValueCache(&mut nocache), &cas).unwrap_err();
    assert_eq!(err.code(), "blob_corrupt");
    std::fs::remove_file(&path).unwrap();
    let err = ctx.value(&e1, &mut StoreValueCache(&mut nocache), &cas).unwrap_err();
    assert_eq!(err.code(), "blob_missing");
    let b = to_bus_error(&err);
    assert_eq!((b.code(), b.reason()), (Code::Internal, "storage"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn value_errors_map_to_the_catalogue() {
    let id = Uuid::from_u128(1);
    let d = Digest::TOMBSTONE;
    let cases: Vec<(ValueError, Code, &str)> = vec![
        (ValueError::BadReference(id), Code::Validation, "bad_reference"),
        (ValueError::StageNotFound(id), Code::Validation, "bad_stage"),
        (
            ValueError::NoValue { element: id, stage: id, state: fid_project::ElementState::NotComputed },
            Code::NotFound,
            "unknown_entity",
        ),
        (ValueError::BlobMissing(d), Code::Internal, "storage"),
        (ValueError::BlobCorrupt(d), Code::Internal, "storage"),
        (ValueError::Storage("x".into()), Code::Internal, "storage"),
    ];
    for (e, code, reason) in cases {
        let b = to_bus_error(&e);
        assert_eq!((b.code(), b.reason()), (code, reason), "{e}");
    }
}
