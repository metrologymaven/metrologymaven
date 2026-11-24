//! Spec leaf VIZ.OVERLAY.LABEL.MODEL: integration tests covering AC-1 .. AC-5 on fixture
//! `fixtures/viz/labels/part20`, registry command validation (`fid_registry::validate`), pinned
//! placement protection (REQ-004), canonical roundtrip, and exact replay (`fid_replay::classify`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse as parse_json, CValue};
use fid_overlay::label::model::{
    add_label, command_defs, edit_label, remove_label, AddLabelParams, EditLabelParams, Field, LabelError, LabelEvent,
    LabelSet, RemoveLabelParams, SourceRef, Template, ViewKey, ADD_LABEL, EDIT_LABEL, REMOVE_LABEL,
    TYPE_CHARACTERISTIC, TYPE_COMPARISON_POINT, TYPE_DEVIATION_LABEL, TYPE_DIMENSION,
};
use fid_project::{live_source, ElementTypeDef, ElementTypeRegistry, ReplaySource, Tree, Uuid, ELEMENT_CREATED};
use fid_registry::{tool_name, validate_with, CommandDecl, GeoFrameUnits, Limits, QuantityKinds};
use fid_replay::{classify, Determinism, Output, ReplayOutcome, StepComparison, Tolerance};
use fid_types::SideEffect;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

struct Part20Fixture {
    namespace: Uuid,
    label_set_id: Uuid,
    tree: Tree,
}

fn load_part20() -> Part20Fixture {
    let text = fs::read_to_string(repo_root().join("fixtures/viz/labels/part20/project.json")).unwrap();
    let val = parse_json(text.as_bytes()).unwrap();
    let CValue::Obj(top) = val else {
        panic!("project.json must be an object");
    };
    let CValue::Str(ns_str) = top.get("namespace").unwrap() else {
        panic!("namespace");
    };
    let CValue::Str(ls_str) = top.get("label_set_id").unwrap() else {
        panic!("label_set_id");
    };
    let namespace = Uuid::parse(ns_str).unwrap();
    let label_set_id = Uuid::parse(ls_str).unwrap();

    let mut reg = ElementTypeRegistry::new();
    let empty_schema = CValue::Obj(BTreeMap::new());
    for (tid, kind) in [
        (TYPE_COMPARISON_POINT, "point"),
        (TYPE_DEVIATION_LABEL, "label"),
        (TYPE_CHARACTERISTIC, "characteristic"),
        (TYPE_DIMENSION, "dimension"),
        ("scan.mesh", "mesh"),
        ("geo.brep", "cad"),
    ] {
        reg.register(ElementTypeDef::new(tid, empty_schema.clone(), kind).unwrap()).unwrap();
    }

    let mut tree = Tree::new(namespace, Arc::new(reg)).unwrap();
    let root_id = tree.root().to_string();

    let CValue::Arr(elems) = top.get("elements").unwrap() else {
        panic!("elements");
    };
    for e in elems {
        let CValue::Obj(em) = e else {
            panic!("elem obj");
        };
        let mut body = BTreeMap::new();
        body.insert("id".into(), em.get("id").unwrap().clone());
        body.insert("meta".into(), CValue::Obj(BTreeMap::new()));
        body.insert("name".into(), em.get("name").unwrap().clone());
        body.insert("parent".into(), CValue::Str(root_id.clone()));
        body.insert("type".into(), em.get("type").unwrap().clone());
        body.insert("visible".into(), CValue::Bool(true));
        tree.apply(ELEMENT_CREATED, &CValue::Obj(body)).unwrap();
    }

    Part20Fixture { namespace, label_set_id, tree }
}

fn contains_number(v: &CValue) -> bool {
    match v {
        CValue::Num(_) | CValue::I64(_) | CValue::U64(_) | CValue::F64Bits(_) => true,
        CValue::Arr(items) => items.iter().any(contains_number),
        CValue::Obj(map) => map.values().any(contains_number),
        CValue::Null | CValue::Bool(_) | CValue::Str(_) => false,
    }
}

/// AC-1: `view.add_label@1` on `cp-07` with `name,deviation,status` creates one label,
/// journals the new LabelSet digest, and the label contains no number.
#[test]
fn ac1_add_label_on_part20_cp07_has_no_number() {
    let fx = load_part20();
    let mut set = LabelSet::empty(fx.label_set_id);
    let empty_digest = set.digest().unwrap();
    let mut obs = live_source();

    let params = AddLabelParams {
        set: Some(fx.label_set_id),
        source: SourceRef::parse("cp-07").unwrap(),
        template: Template::parse_csv("name,deviation,status").unwrap(),
    };
    let out = add_label(&set, &fx.tree, &params, fx.namespace, &mut obs).unwrap();
    assert_eq!(out.event.command, ADD_LABEL);
    assert_eq!(out.event.side_effect, SideEffect::W2);
    assert_ne!(out.event.label_set_digest, empty_digest);

    set.apply_event(&out.event).unwrap();
    assert_eq!(set.len(), 1);
    assert_eq!(set.digest().unwrap(), out.event.label_set_digest);
    assert_eq!(set.digest().unwrap(), out.result.label_set_digest);

    let lbl = set.get(out.result.label).unwrap();
    assert_eq!(lbl.template().fields(), &[Field::Name, Field::Deviation, Field::Status]);
    assert!(!lbl.pinned());
    assert!(!contains_number(&lbl.to_value()), "label must hold no number: {:?}", lbl.to_value());
}

/// AC-2: mesh, CAD, and missing sources fail with `NotLabelable` and journal nothing;
/// all four labelable kinds succeed.
#[test]
fn ac2_non_labelable_sources_fail_and_journal_nothing() {
    let fx = load_part20();
    let mut set = LabelSet::empty(fx.label_set_id);
    let before_digest = set.digest().unwrap();
    let mut obs = live_source();
    let tmpl = Template::parse_csv("name,status").unwrap();

    for bad in ["mesh-part", "cad-body", "nonexistent-cp"] {
        let params = AddLabelParams { set: None, source: SourceRef::parse(bad).unwrap(), template: tmpl.clone() };
        let err = add_label(&set, &fx.tree, &params, fx.namespace, &mut obs).unwrap_err();
        assert_eq!(err.code(), "NotLabelable", "expected NotLabelable for {bad}, got {err:?}");
        assert!(set.is_empty());
        assert_eq!(set.digest().unwrap(), before_digest);
        assert!(obs.observations().is_empty(), "failed add_label must not draw observations");
    }

    // All four allowed kinds succeed.
    for good in ["cp-01", "dl-01", "char-01", "dim-01"] {
        let params = AddLabelParams { set: None, source: SourceRef::parse(good).unwrap(), template: tmpl.clone() };
        let out = add_label(&set, &fx.tree, &params, fx.namespace, &mut obs).unwrap();
        set.apply_event(&out.event).unwrap();
    }
    assert_eq!(set.len(), 4);
}

/// AC-3 & AC-4: manual placement pins `main` at `(0.62, 0.18)` and blocks automatic layout;
/// remove deletes the label and changes the digest.
#[test]
fn ac3_edit_pins_placement_and_ac4_remove_changes_digest() {
    let fx = load_part20();
    let mut set = LabelSet::empty(fx.label_set_id);
    let empty_digest = set.digest().unwrap();
    let mut obs = live_source();

    let add_out = add_label(
        &set,
        &fx.tree,
        &AddLabelParams {
            set: None,
            source: SourceRef::parse("cp-07").unwrap(),
            template: Template::parse_csv("name,deviation,status").unwrap(),
        },
        fx.namespace,
        &mut obs,
    )
    .unwrap();
    let l1 = add_out.result.label;
    set.apply_event(&add_out.event).unwrap();
    let after_add = set.digest().unwrap();

    // AC-3: edit-label --label L1 --view main --at 0.62,0.18
    let main_view = ViewKey::new("main").unwrap();
    let edit_out = edit_label(
        &set,
        &EditLabelParams {
            label: l1,
            template: None,
            pinned: None,
            view: Some(main_view.clone()),
            at: Some((0.62, 0.18)),
        },
    )
    .unwrap();
    assert_eq!(edit_out.event.command, EDIT_LABEL);
    assert_eq!(edit_out.event.side_effect, SideEffect::W2);
    set.apply_event(&edit_out.event).unwrap();
    let after_edit = set.digest().unwrap();
    assert_ne!(after_edit, after_add);

    let lbl = set.get(l1).unwrap();
    let pl = lbl.placement(&main_view).unwrap();
    assert_eq!(pl.at(), (0.62, 0.18));
    assert!(pl.pinned(), "manual placement must pin the view key (REQ-004)");

    // REQ-004: automatic layout cannot move a pinned placement.
    let pin_err = set.set_unpinned_placement(l1, main_view.clone(), 0.1, 0.2).unwrap_err();
    assert!(matches!(pin_err, LabelError::PlacementPinned { .. }));
    assert_eq!(set.get(l1).unwrap().placement(&main_view).unwrap().at(), (0.62, 0.18));

    // Another view key is unpinned and CAN be placed by automatic layout.
    let side_view = ViewKey::new("side").unwrap();
    set.set_unpinned_placement(l1, side_view.clone(), 0.25, 0.75).unwrap();
    assert!(!set.get(l1).unwrap().placement(&side_view).unwrap().pinned());

    // Canonical roundtrip preserves all placements & pinned flags.
    let roundtrip = LabelSet::from_value(&set.to_value()).unwrap();
    assert_eq!(roundtrip, set);
    assert_eq!(roundtrip.to_canonical_bytes().unwrap(), set.to_canonical_bytes().unwrap());

    // AC-4: remove-label --label L1
    let rem_out = remove_label(&set, &RemoveLabelParams { label: l1 }).unwrap();
    assert_eq!(rem_out.event.command, REMOVE_LABEL);
    assert_eq!(rem_out.event.side_effect, SideEffect::W2);
    set.apply_event(&rem_out.event).unwrap();
    assert!(set.is_empty());
    let after_remove = set.digest().unwrap();
    assert_ne!(after_remove, after_edit);
    assert_eq!(after_remove, empty_digest);
}

/// AC-5: replaying the journal of AC-1..AC-4 in exact mode reproduces every LabelSet digest
/// and classifies as `IDENTICAL`.
#[test]
fn ac5_exact_replay_is_identical() {
    let fx = load_part20();
    let mut live_set = LabelSet::empty(fx.label_set_id);
    let mut live_obs = live_source();
    let mut journal: Vec<LabelEvent> = Vec::new();

    // Step 1: add cp-07
    let add_params = AddLabelParams {
        set: Some(fx.label_set_id),
        source: SourceRef::parse("cp-07").unwrap(),
        template: Template::parse_csv("name,deviation,status").unwrap(),
    };
    let o1 = add_label(&live_set, &fx.tree, &add_params, fx.namespace, &mut live_obs).unwrap();
    let l1 = o1.result.label;
    live_set.apply_event(&o1.event).unwrap();
    journal.push(o1.event);

    // Step 2 (AC-2): rejected add on mesh-part journals nothing
    let bad_params = AddLabelParams {
        set: None,
        source: SourceRef::parse("mesh-part").unwrap(),
        template: Template::parse_csv("name").unwrap(),
    };
    assert!(add_label(&live_set, &fx.tree, &bad_params, fx.namespace, &mut live_obs).is_err());

    // Step 3 (AC-3): manual placement on main
    let edit_params = EditLabelParams {
        label: l1,
        template: None,
        pinned: None,
        view: Some(ViewKey::new("main").unwrap()),
        at: Some((0.62, 0.18)),
    };
    let o2 = edit_label(&live_set, &edit_params).unwrap();
    live_set.apply_event(&o2.event).unwrap();
    journal.push(o2.event);

    // Step 4 (AC-4): remove L1
    let rem_params = RemoveLabelParams { label: l1 };
    let o3 = remove_label(&live_set, &rem_params).unwrap();
    live_set.apply_event(&o3.event).unwrap();
    journal.push(o3.event);

    // Replay from scratch with recorded UUIDv7 observations.
    let mut replay_set = LabelSet::empty(fx.label_set_id);
    let mut replay_obs = ReplaySource::new(live_obs.into_observations());
    let table: BTreeMap<String, Tolerance> = BTreeMap::new();

    let r1 = add_label(&replay_set, &fx.tree, &add_params, fx.namespace, &mut replay_obs).unwrap();
    replay_set.apply_event(&r1.event).unwrap();
    let r2 = edit_label(&replay_set, &edit_params).unwrap();
    replay_set.apply_event(&r2.event).unwrap();
    let r3 = remove_label(&replay_set, &rem_params).unwrap();
    replay_set.apply_event(&r3.event).unwrap();
    assert_eq!(replay_obs.remaining(), 0);

    let replayed = [r1.event, r2.event, r3.event];
    for (rec, rep) in journal.iter().zip(replayed.iter()) {
        assert_eq!(rec.label_id, rep.label_id);
        assert_eq!(rec.label_set_digest, rep.label_set_digest);
        assert_eq!(rec.body(), rep.body());

        // Classify digest byte equality under fid-replay exact (D0) mode.
        let cmp = StepComparison {
            class: Determinism::D0,
            same_kernel: true,
            same_platform_class: true,
            outputs: vec![Output {
                kind: "label_set".into(),
                recorded: 0.0,
                replayed: 0.0,
                recorded_digest: rec.label_set_digest.as_str().to_string(),
                replayed_digest: rep.label_set_digest.as_str().to_string(),
            }],
            decisions: Vec::new(),
            limits: Vec::new(),
            missing: None,
        };
        let res = classify(&cmp, &table).unwrap();
        assert_eq!(res.outcome, ReplayOutcome::Identical);
    }
}

/// Registry definitions for view.add_label@1, view.edit_label@1 and view.remove_label@1 pass
/// fid-registry validation and project the exact CLI and MCP names from the spec.
#[test]
fn registry_command_defs_validate_cleanly() {
    let defs = command_defs().unwrap();
    assert_eq!(defs.len(), 3);

    let ids: Vec<(&str, String)> = defs.iter().map(|d| (d.id.as_str(), tool_name(&d.id, d.major))).collect();
    assert_eq!(
        ids,
        vec![
            ("view.add_label", "view_add_label".to_string()),
            ("view.edit_label", "view_edit_label".to_string()),
            ("view.remove_label", "view_remove_label".to_string()),
        ]
    );

    struct WorkspaceKinds;
    impl QuantityKinds for WorkspaceKinds {
        fn has_kind(&self, kind: &str) -> bool {
            kind == "dimensionless" || GeoFrameUnits.has_kind(kind)
        }
        fn accepts(&self, kind: &str, unit: &str) -> bool {
            if kind == "dimensionless" {
                unit == "1"
            } else {
                GeoFrameUnits.accepts(kind, unit)
            }
        }
    }

    let decls: Vec<CommandDecl> = defs.iter().map(|d| CommandDecl(d.to_value())).collect();
    let errors = validate_with(&decls, &Limits::default(), &WorkspaceKinds);
    assert!(errors.is_empty(), "registry validation errors: {errors:?}");
}

/// Tampered event digest fails `apply_event` atomically without mutating the `LabelSet`.
#[test]
fn apply_event_rejects_tampered_digest() {
    let fx = load_part20();
    let mut set = LabelSet::empty(fx.label_set_id);
    let mut obs = live_source();
    let mut out = add_label(
        &set,
        &fx.tree,
        &AddLabelParams {
            set: None,
            source: SourceRef::parse("cp-07").unwrap(),
            template: Template::parse_csv("name").unwrap(),
        },
        fx.namespace,
        &mut obs,
    )
    .unwrap();
    out.event.label_set_digest = fid_types::Digest::TOMBSTONE;
    let err = set.apply_event(&out.event).unwrap_err();
    assert!(matches!(err, LabelError::DigestMismatch { .. }));
    assert!(set.is_empty());
}
