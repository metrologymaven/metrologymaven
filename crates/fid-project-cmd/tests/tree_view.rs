//! The bus half of spec node CORE.PRJ.TREE.MODEL: the element tree as the bus `StateView`
//! and the bus errors of tree refusals (AC-3 on the bus: `validation/unregistered_type` and
//! nothing committed). The tree rules themselves are tested in `fid-project`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{Code, InternalReason};
use fid_bus::handler::{EntityId, Event, EventType, SettingKey, StateView};
use fid_canon::{CValue, Digest};
use fid_project::{
    ElementState, ElementTypeDef, ElementTypeRegistry, IdError, Tree, TreeError, Uuid, ELEMENT_CREATED,
    ELEMENT_SLOT_SET, GROUP_CREATED, GROUP_KIND,
};
use fid_project_cmd::tree::{to_bus_error, to_replay_error, tree_event, TreeState};
use fid_project_cmd::BusView;
use std::sync::Arc;

const NAMESPACE: Uuid = Uuid::from_u128(0x3f6c_2a1e_8b4d_4e0f_9a7b_5c1d_2e3f_4a6b);
const D1: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn o<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn t(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn id(n: u128) -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000u128 | n)
}

fn ev(ty: &str, body: CValue) -> Event {
    Event::new(EventType::new(ty).unwrap(), body)
}

fn element(eid: Uuid, parent: Uuid, ty: &str, name: &str) -> Event {
    ev(
        ELEMENT_CREATED,
        o([
            ("id", t(&eid.to_string())),
            ("meta", o([])),
            ("name", t(name)),
            ("parent", t(&parent.to_string())),
            ("type", t(ty)),
            ("visible", CValue::Bool(true)),
        ]),
    )
}

fn state() -> TreeState {
    let mut reg = ElementTypeRegistry::new();
    reg.register(ElementTypeDef::new("test.alpha", o([("type", t("object"))]), "point").unwrap()).unwrap();
    let tree = Tree::new(NAMESPACE, Arc::new(reg)).unwrap();
    TreeState::new(tree, Digest([7; 32])).unwrap()
}

fn key(id: &Uuid) -> EntityId {
    EntityId::new(&id.to_string()).unwrap()
}

#[test]
fn state_view_reads_elements_groups_head_and_settings() {
    let mut s = state();
    let root = s.tree().root();
    let g = id(0xa1);
    s.apply_commit(&[
        ev(GROUP_CREATED, o([("id", t(&g.to_string())), ("name", t("Datums")), ("parent", t(&root.to_string()))])),
        element(id(0xb2), g, "test.alpha", "Plane 1"),
    ])
    .unwrap();
    let view: &dyn StateView = &s;
    let e = view.entity(&key(&id(0xb2))).unwrap();
    assert_eq!((e.id.as_str(), e.kind), (id(0xb2).to_string().as_str(), "test.alpha"));
    assert_eq!(e.body, s.tree().element(&id(0xb2)).unwrap().body());
    assert_eq!(view.entity(&key(&g)).unwrap().kind, GROUP_KIND);
    assert_eq!(view.entity(&key(&root)).unwrap().kind, GROUP_KIND);
    // Only the canonical lowercase text names an entity.
    let upper = EntityId::new(&id(0xb2).to_string().to_uppercase()).unwrap();
    assert!(view.entity(&upper).is_none());
    assert!(view.entity(&EntityId::new("not-a-uuid").unwrap()).is_none());
    assert!(view.entity(&key(&id(0xff))).is_none());
    assert_eq!(view.head(), Digest([7; 32]));
    let k = SettingKey::new("project.recompute_mode").unwrap();
    assert_eq!(view.setting(&k), None);
    s.set_setting(&k, t("auto"));
    s.set_head(Digest([8; 32]));
    let view: &dyn StateView = &s;
    assert_eq!(view.setting(&k), Some(t("auto")));
    assert_eq!(view.head(), Digest([8; 32]));
    // The container rules read the same view: a tree element is no container.
    assert!(fid_project::Hierarchy::new(&BusView(view)).get(&id(0xb2)).unwrap().is_none());
}

#[test]
fn unregistered_type_is_validation_and_nothing_is_committed() {
    let mut s = state();
    let root = s.tree().root();
    let before = s.tree().state_digest().unwrap();
    let err = s
        .apply_commit(&[element(id(1), root, "test.alpha", "ok"), element(id(2), root, "geo.mesh", "bad")])
        .unwrap_err();
    assert_eq!((err.code(), err.reason()), (Code::Validation, "unregistered_type"));
    assert_eq!(s.tree().state_digest().unwrap(), before);
    assert!(s.entity(&key(&id(1))).is_none());
}

#[test]
fn bus_error_mapping() {
    let cases: Vec<(TreeError, Code, &str)> = vec![
        (TreeError::Schema("x".into()), Code::Validation, "schema"),
        (TreeError::Id(IdError::BadId), Code::Validation, "schema"),
        (TreeError::Name, Code::Validation, "name"),
        (TreeError::UnregisteredType("geo.mesh".into()), Code::Validation, "unregistered_type"),
        (TreeError::BadReference("x".into()), Code::Validation, "bad_reference"),
        (TreeError::BadStage(id(1)), Code::Validation, "bad_stage"),
        (TreeError::Hierarchy("x".into()), Code::Validation, "hierarchy"),
        (TreeError::IdCollision(id(1)), Code::Conflict, "id_collision"),
        (TreeError::Id(IdError::ReplayExhausted), Code::Internal, InternalReason::HandlerFailed.as_str()),
        (
            TreeError::InvalidTransition { from: ElementState::Current, to: ElementState::Failed },
            Code::Precondition,
            "invalid_transition",
        ),
        (TreeError::DuplicateType("geo.mesh".into()), Code::Internal, "invariant_violated"),
        (TreeError::DuplicateEvent("x".into()), Code::Internal, "invariant_violated"),
        (TreeError::UnknownEvent("x".into()), Code::Internal, "invariant_violated"),
        (TreeError::Invariant("x".into()), Code::Internal, "invariant_violated"),
    ];
    for (e, code, reason) in cases {
        let b = to_bus_error(&e);
        assert_eq!((b.code(), b.reason()), (code, reason), "{e}");
        assert!(b.detail().as_untrusted_str().starts_with(e.code()), "{e}: detail names the tree code");
        // Replay maps the same, except an invalid transition, which there means corrupt state.
        let r = to_replay_error(&e);
        if matches!(e, TreeError::InvalidTransition { .. }) {
            assert_eq!((r.code(), r.reason()), (Code::Internal, "invariant_violated"));
            assert!(r.detail().as_untrusted_str().starts_with("invalid_transition"));
        } else {
            assert_eq!(r, b, "{e}");
        }
    }
}

#[test]
fn invalid_transition_is_a_precondition_on_commit_internal_on_replay_and_rolled_back() {
    let mut s = state();
    let root = s.tree().root();
    s.apply_commit(&[element(id(1), root, "test.alpha", "a")]).unwrap();
    let stage = s.tree().active_stage();
    let set = |state: &str, digest: Option<&str>| {
        ev(
            ELEMENT_SLOT_SET,
            o([
                ("digest", digest.map_or(CValue::Null, t)),
                ("element", t(&id(1).to_string())),
                ("stage", t(&stage.to_string())),
                ("state", t(state)),
            ]),
        )
    };
    let before = s.tree().state_digest().unwrap();
    let err = s.apply_commit(&[set("current", Some(D1)), set("failed", None)]).unwrap_err();
    assert_eq!((err.code(), err.reason()), (Code::Precondition, "invalid_transition"));
    assert_eq!(s.tree().state_digest().unwrap(), before);
    let err = s.replay_commit(&[set("current", Some(D1)), set("failed", None)]).unwrap_err();
    assert_eq!((err.code(), err.reason()), (Code::Internal, "invariant_violated"));
    assert!(err.detail().as_untrusted_str().starts_with("invalid_transition"));
    assert_eq!(s.tree().state_digest().unwrap(), before);
    // A commit that creates an entity and then fails leaves no trace in the view either.
    let err = s.apply_commit(&[element(id(2), root, "test.alpha", "b"), set("failed", None)]);
    assert!(err.is_ok(), "not_computed -> failed is allowed");
    let err = s.apply_commit(&[element(id(3), root, "test.alpha", "c"), set("current", None)]).unwrap_err();
    assert_eq!((err.code(), err.reason()), (Code::Validation, "schema"));
    assert!(s.entity(&key(&id(3))).is_none() && s.tree().element(&id(3)).is_none());
    assert!(s.entity(&key(&id(2))).is_some());
    // The tree event of a bus event keeps its type and body.
    let e = set("current", Some(D1));
    let te = tree_event(&e);
    assert_eq!((te.event_type.as_str(), &te.body), (ELEMENT_SLOT_SET, e.body()));
}
