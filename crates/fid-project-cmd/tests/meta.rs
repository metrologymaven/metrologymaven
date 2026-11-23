//! Bus handler, CommandDef and error mapping tests for `CORE.PRJ.META` (`project.set_tags@1`,
//! `project.set_metadata@1`, `project.find_elements_by_tag@1`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::handler::{effects_check, EntityId, EntityRef, HandlerSet, SettingKey, StateView};
use fid_canon::CValue;
use fid_project::meta::{
    MetaEvent, MetaStore, FIND_ELEMENTS_BY_TAG, METADATA_SET, PROJECT_TARGET, SET_METADATA, SET_TAGS, TAGS_SET,
};
use fid_project::Uuid;
use fid_project_cmd::meta::{command_defs, register_meta, run_meta, MetaOp, MetaSource};
use fid_registry::Undo;
use fid_types::SideEffect;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

struct StubView {
    known: BTreeMap<EntityId, CValue>,
}

impl StateView for StubView {
    fn entity(&self, id: &EntityId) -> Option<EntityRef<'_>> {
        self.known.get_key_value(id).map(|(id, body)| EntityRef { id, kind: "geo.plane", body })
    }
    fn head(&self) -> fid_canon::Digest {
        fid_canon::Digest([0; 32])
    }
    fn setting(&self, _k: &SettingKey) -> Option<CValue> {
        None
    }
}

struct MemMetaSource {
    store: RwLock<MetaStore>,
}

impl MetaSource for MemMetaSource {
    fn store(&self) -> Result<MetaStore, String> {
        Ok(self.store.read().unwrap().clone())
    }
}

#[test]
fn command_defs_register_and_execute_on_bus() {
    let defs = command_defs().unwrap();
    assert_eq!(defs.len(), 3);
    assert_eq!(defs[0].id, SET_TAGS);
    assert_eq!(defs[0].side_effect, SideEffect::W2);
    assert_eq!(defs[0].undo, Undo::HeadMove);
    assert_eq!(defs[1].id, SET_METADATA);
    assert_eq!(defs[1].side_effect, SideEffect::W2);
    assert_eq!(defs[1].undo, Undo::HeadMove);
    assert_eq!(defs[2].id, FIND_ELEMENTS_BY_TAG);
    assert_eq!(defs[2].side_effect, SideEffect::R0);

    let src = Arc::new(MemMetaSource { store: RwLock::new(MetaStore::new()) });
    let dyn_src: Arc<dyn MetaSource> = src.clone();

    let mut set = HandlerSet::new();
    register_meta(&mut set, &dyn_src).unwrap();
    assert!(set.get(&format!("{SET_TAGS}@1")).is_ok());
    assert!(set.get(&format!("{SET_METADATA}@1")).is_ok());
    assert!(set.get(&format!("{FIND_ELEMENTS_BY_TAG}@1")).is_ok());

    let e1 = Uuid::parse("01900000-0000-7000-8000-000000000001").unwrap();
    let e1_key = EntityId::new(&e1.to_string()).unwrap();
    let view = StubView { known: BTreeMap::from([(e1_key, obj([]))]) };

    // Set tags -> 1 TagsSet event matching declared effects, then apply to store and query R0.
    let p_tags = obj([("target", s(&e1.to_string())), ("tags", CValue::Arr(vec![s("critical"), s("datum")]))]);
    let (evs, _res) = run_meta(MetaOp::SetTags, &view, dyn_src.as_ref(), &p_tags).unwrap();
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0].type_id().as_str(), TAGS_SET);
    effects_check(&defs[0], &evs).unwrap();
    let me = MetaEvent::from_wire(evs[0].type_id().as_str(), evs[0].body()).unwrap();
    src.store.write().unwrap().apply(&me, None).unwrap();

    // Find elements by tag (R0 -> 0 events, passes effects_check).
    let (find_evs, find_res) =
        run_meta(MetaOp::FindElementsByTag, &view, dyn_src.as_ref(), &obj([("tag", s("critical"))])).unwrap();
    assert!(find_evs.is_empty());
    effects_check(&defs[2], &find_evs).unwrap();
    assert_eq!(find_res, obj([("elements", CValue::Arr(vec![s(&e1.to_string())]))]));

    // Set metadata on project -> 1 MetadataSet event matching declared effects.
    let p_meta = obj([("target", s(PROJECT_TARGET)), ("set", obj([("customer", s("ACME"))]))]);
    let (mevs, _) = run_meta(MetaOp::SetMetadata, &view, dyn_src.as_ref(), &p_meta).unwrap();
    assert_eq!(mevs.len(), 1);
    assert_eq!(mevs[0].type_id().as_str(), METADATA_SET);
    effects_check(&defs[1], &mevs).unwrap();

    // Error paths map to typed bus errors.
    let err_nf = run_meta(
        MetaOp::SetTags,
        &view,
        dyn_src.as_ref(),
        &obj([("target", s("01900000-0000-7000-8000-000000009999")), ("tags", CValue::Arr(vec![s("a")]))]),
    )
    .unwrap_err();
    assert_eq!(err_nf.code().as_str(), "not_found");
    assert_eq!(err_nf.reason(), "unknown_entity");

    let err_val = run_meta(
        MetaOp::SetMetadata,
        &view,
        dyn_src.as_ref(),
        &obj([("target", s(PROJECT_TARGET)), ("set", obj([("Bad Key", s("v"))]))]),
    )
    .unwrap_err();
    assert_eq!(err_val.code().as_str(), "validation");
    assert_eq!(err_val.reason(), "schema");
}
