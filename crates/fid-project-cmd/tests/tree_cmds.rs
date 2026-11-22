//! Acceptance, bus, registry, undo, replay, and scale tests for `CORE.PRJ.TREE.GROUP`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use fid_bus::error::{BusError, Code, Detail, ValidationReason};
use fid_bus::handler::{effects_check, Event, HandlerSet};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse, to_canonical_bytes, CValue, Digest as CanonDigest};
use fid_project::{
    Bits, ElementTypeDef, ElementTypeRegistry, Observation, ObservationSource, Recording, ReplaySource, Tree,
    TreeEvent, Uuid, ELEMENTS_MOVED, ELEMENT_CREATED, GROUP_CREATED, MAX_GROUP_DEPTH, RENAMED, V7_RANDOM_BITS,
    VISIBILITY_SET,
};
use fid_project_cmd::tree::TreeState;
use fid_project_cmd::tree_cmds::{command_defs, register, result_of, run_tree, TreeOp};
use fid_project_cmd::SharedSource;
use fid_registry::{check_schema, validate, Catalogue, CommandDecl, CommandDef};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

fn obj<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn make_observations(n: u64) -> Vec<Observation> {
    let mut out = Vec::new();
    for seq in 1..=n {
        out.push(Observation::NowMs(1_760_000_000_000 + seq));
        out.push(Observation::RandomBits(Bits::new(V7_RANDOM_BITS, u128::from(0x0000_0010_0000_0000 + seq)).unwrap()));
    }
    out
}

fn make_source(n: u64) -> SharedSource {
    Arc::new(Mutex::new(Recording::new(ReplaySource::new(make_observations(n)))))
}

struct NoHooks;
impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no literals in tree_cmds schemas")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

fn validate_params_and_result(def: &CommandDef, params: &CValue, result: &CValue) {
    let compiled_p = Compiled::compile(&def.params_schema).unwrap();
    compiled_p.evaluate(params, &NoHooks).expect("params must conform to params_schema");
    let compiled_r = Compiled::compile(&def.result_schema).unwrap();
    compiled_r.evaluate(result, &NoHooks).expect("result must conform to result_schema");
}

struct P30Fixture {
    namespace: Uuid,
    root: Uuid,
    element_ids: Vec<Uuid>,
    state: TreeState,
}

fn load_p30() -> P30Fixture {
    let path = repo_root().join("fixtures/core/prj/tree/group/p30/events.json");
    let raw = fs::read(&path).expect("read p30 fixture");
    let CValue::Obj(root_obj) = parse(&raw).expect("parse p30 JSON") else {
        panic!("p30 root must be object");
    };
    let ns_str = match root_obj.get("namespace") {
        Some(CValue::Str(s)) => s.as_str(),
        _ => panic!("missing namespace"),
    };
    let namespace = Uuid::parse(ns_str).unwrap();
    let root = match root_obj.get("root") {
        Some(CValue::Str(s)) => Uuid::parse(s).unwrap(),
        _ => panic!("missing root"),
    };
    let element_ids: Vec<Uuid> = match root_obj.get("element_ids") {
        Some(CValue::Arr(arr)) => arr
            .iter()
            .map(|v| match v {
                CValue::Str(s) => Uuid::parse(s).unwrap(),
                _ => panic!("element_id must be string"),
            })
            .collect(),
        _ => panic!("missing element_ids"),
    };
    assert_eq!(element_ids.len(), 30);

    let mut reg = ElementTypeRegistry::new();
    reg.register(ElementTypeDef::new("test.plane", obj([]), "plane").unwrap()).unwrap();
    let mut tree = Tree::new(namespace, Arc::new(reg)).unwrap();
    assert_eq!(tree.root(), root);

    let Some(CValue::Arr(ev_arr)) = root_obj.get("events") else {
        panic!("missing events");
    };
    let mut tree_events = Vec::new();
    for item in ev_arr {
        let CValue::Obj(eo) = item else { panic!("event must be object") };
        let et = match eo.get("type") {
            Some(CValue::Str(s)) => s.clone(),
            _ => panic!("event.type"),
        };
        let body = eo.get("body").cloned().unwrap();
        assert_eq!(et, ELEMENT_CREATED);
        tree_events.push(TreeEvent::new(&et, body));
    }
    tree.apply_commit(&tree_events).unwrap();
    tree.verify().unwrap();
    let state = TreeState::new(tree, CanonDigest([0u8; 32])).unwrap();
    P30Fixture { namespace, root, element_ids, state }
}

fn dispatch(
    set: &HandlerSet,
    state: &mut TreeState,
    op: TreeOp,
    params: CValue,
    namespace: Uuid,
    source: &SharedSource,
    journal: &mut Vec<(String, Vec<Event>)>,
) -> Result<Vec<Event>, BusError> {
    let cmd = format!("{}@1", op.command());
    let def = set.def(&cmd)?;
    let events = {
        let mut src = source.lock().unwrap();
        run_tree(op, state, &params, namespace, &mut *src)?
    };
    effects_check(def, &events)?;
    let result = result_of(&events[0])?;
    validate_params_and_result(def, &params, &result);
    state.apply_commit(&events)?;
    state.tree().verify().unwrap();
    journal.push((cmd, events.clone()));
    Ok(events)
}

fn group_id_from_event(ev: &Event) -> Uuid {
    match result_of(ev).unwrap() {
        CValue::Obj(o) => match o.get("group") {
            Some(CValue::Str(s)) => Uuid::parse(s).unwrap(),
            _ => panic!("missing group"),
        },
        _ => panic!("result must be object"),
    }
}

/// AC-1, AC-2, AC-3, AC-4:
/// - AC-1: create-group "Datums", move e1,e2,e3 into it preserving order; 2 W2 commits recorded.
/// - AC-2: groups A ⊃ B; moving A into B fails validation/hierarchy and commits nothing.
/// - AC-3: rename-element e1 and e2 both to "Plane 1" succeeds (non-unique untrusted names).
/// - AC-4: set-visibility e4,e5 false hides both; head-move undo (prefix refold) restores them.
#[test]
fn ac1_through_ac4_full_workflow_and_undo() {
    let mut p30 = load_p30();
    let source = make_source(16);
    let mut set = HandlerSet::new();
    register(&mut set, p30.namespace, &source).unwrap();

    let mut journal: Vec<(String, Vec<Event>)> = Vec::new();

    // AC-1: create group "Datums" under root (omitted parent defaults to root), move e1,e2,e3.
    let evs1 = dispatch(
        &set,
        &mut p30.state,
        TreeOp::CreateGroup,
        obj([("name", text("Datums"))]),
        p30.namespace,
        &source,
        &mut journal,
    )
    .expect("create Datums");
    assert_eq!(evs1.len(), 1);
    assert_eq!(evs1[0].type_id().as_str(), GROUP_CREATED);
    let datums_id = group_id_from_event(&evs1[0]);
    assert_eq!(p30.state.tree().group(&datums_id).unwrap().parent(), Some(p30.root));
    assert_eq!(p30.state.tree().group(&datums_id).unwrap().name().as_untrusted_str(), "Datums");

    let e1 = p30.element_ids[0];
    let e2 = p30.element_ids[1];
    let e3 = p30.element_ids[2];
    let evs2 = dispatch(
        &set,
        &mut p30.state,
        TreeOp::MoveElement,
        obj([
            ("ids", CValue::Arr(vec![text(&e1.to_string()), text(&e2.to_string()), text(&e3.to_string())])),
            ("to", text(&datums_id.to_string())),
        ]),
        p30.namespace,
        &source,
        &mut journal,
    )
    .expect("move e1..e3");
    assert_eq!(evs2.len(), 1);
    assert_eq!(evs2[0].type_id().as_str(), ELEMENTS_MOVED);
    assert_eq!(p30.state.tree().children_of(&datums_id), &[e1, e2, e3]);
    assert_eq!(p30.state.tree().element(&e1).unwrap().parent(), datums_id);
    assert_eq!(p30.state.tree().element(&e2).unwrap().parent(), datums_id);
    assert_eq!(p30.state.tree().element(&e3).unwrap().parent(), datums_id);
    assert_eq!(journal.len(), 2);

    // AC-2: groups A ⊃ B; moving A into B fails validation/hierarchy and journals nothing.
    let evs_a = dispatch(
        &set,
        &mut p30.state,
        TreeOp::CreateGroup,
        obj([("name", text("Group A"))]),
        p30.namespace,
        &source,
        &mut journal,
    )
    .unwrap();
    let group_a = group_id_from_event(&evs_a[0]);

    let evs_b = dispatch(
        &set,
        &mut p30.state,
        TreeOp::CreateGroup,
        obj([("name", text("Group B")), ("parent", text(&group_a.to_string()))]),
        p30.namespace,
        &source,
        &mut journal,
    )
    .unwrap();
    let group_b = group_id_from_event(&evs_b[0]);

    let before_cycle_digest = p30.state.tree().state_digest().unwrap();
    let before_cycle_len = journal.len();
    let err = dispatch(
        &set,
        &mut p30.state,
        TreeOp::MoveElement,
        obj([("ids", CValue::Arr(vec![text(&group_a.to_string())])), ("to", text(&group_b.to_string()))]),
        p30.namespace,
        &source,
        &mut journal,
    )
    .expect_err("A -> B must fail hierarchy");
    assert!(matches!(err, BusError::Validation(ValidationReason::Hierarchy, _)), "got: {err:?}");
    assert_eq!(journal.len(), before_cycle_len, "failed move must journal nothing");
    assert_eq!(p30.state.tree().state_digest().unwrap(), before_cycle_digest);

    // Also moving A into A itself must fail validation/hierarchy.
    let err_self = dispatch(
        &set,
        &mut p30.state,
        TreeOp::MoveElement,
        obj([("ids", CValue::Arr(vec![text(&group_a.to_string())])), ("to", text(&group_a.to_string()))]),
        p30.namespace,
        &source,
        &mut journal,
    )
    .expect_err("A -> A must fail hierarchy");
    assert!(matches!(err_self, BusError::Validation(ValidationReason::Hierarchy, _)));

    // AC-3: rename e1 and e2 both to "Plane 1" (duplicate names allowed).
    let evs_r1 = dispatch(
        &set,
        &mut p30.state,
        TreeOp::RenameElement,
        obj([("id", text(&e1.to_string())), ("name", text("Plane 1"))]),
        p30.namespace,
        &source,
        &mut journal,
    )
    .unwrap();
    assert_eq!(evs_r1[0].type_id().as_str(), RENAMED);

    let evs_r2 = dispatch(
        &set,
        &mut p30.state,
        TreeOp::RenameElement,
        obj([("id", text(&e2.to_string())), ("name", text("Plane 1"))]),
        p30.namespace,
        &source,
        &mut journal,
    )
    .unwrap();
    assert_eq!(evs_r2[0].type_id().as_str(), RENAMED);
    assert_eq!(p30.state.tree().element(&e1).unwrap().name().as_untrusted_str(), "Plane 1");
    assert_eq!(p30.state.tree().element(&e2).unwrap().name().as_untrusted_str(), "Plane 1");

    // AC-4: set-visibility e4,e5 false hides both; head-move undo restores them.
    let e4 = p30.element_ids[3];
    let e5 = p30.element_ids[4];
    let pre_vis_digest = p30.state.tree().state_digest().unwrap();
    let evs_v = dispatch(
        &set,
        &mut p30.state,
        TreeOp::SetVisibility,
        obj([
            ("ids", CValue::Arr(vec![text(&e4.to_string()), text(&e5.to_string())])),
            ("visible", CValue::Bool(false)),
        ]),
        p30.namespace,
        &source,
        &mut journal,
    )
    .unwrap();
    assert_eq!(evs_v[0].type_id().as_str(), VISIBILITY_SET);
    assert!(!p30.state.tree().element(&e4).unwrap().visible());
    assert!(!p30.state.tree().element(&e5).unwrap().visible());

    // Head-move undo: folding the prefix before the last commit restores e4,e5 to visible=true.
    let mut undone = load_p30().state;
    for (_, evs) in &journal[..journal.len() - 1] {
        undone.replay_commit(evs).unwrap();
    }
    assert!(undone.tree().element(&e4).unwrap().visible());
    assert!(undone.tree().element(&e5).unwrap().visible());
    assert_eq!(undone.tree().state_digest().unwrap(), pre_vis_digest);
}

/// AC-5 (matching stage.rs `ac5_replay_is_identical`):
/// Run the full sequence of tree commands live with a `Recording` observation source (drawing
/// UUIDv7 IDs for `create_group`), then re-run the handlers from clean `p30` against a
/// `ReplaySource` seeded with the recorded observations: identical canonical events and
/// identical final state digest.
#[test]
fn ac5_replay_is_identical() {
    let run = |source: &mut dyn ObservationSource| {
        let mut p30 = load_p30();
        let mut journal: Vec<Vec<u8>> = Vec::new();
        let mut exec = |op: TreeOp, params: &CValue| -> Vec<Event> {
            let evs = run_tree(op, &p30.state, params, p30.namespace, source).unwrap();
            for e in &evs {
                journal.push(to_canonical_bytes(&e.to_value()).unwrap());
            }
            p30.state.apply_commit(&evs).unwrap();
            p30.state.tree().verify().unwrap();
            evs
        };

        let evs1 = exec(TreeOp::CreateGroup, &obj([("name", text("Datums"))]));
        let datums_id = group_id_from_event(&evs1[0]);
        let evs2 = exec(TreeOp::CreateGroup, &obj([("name", text("Sub")), ("parent", text(&datums_id.to_string()))]));
        let sub_id = group_id_from_event(&evs2[0]);

        let e1 = p30.element_ids[0];
        let e2 = p30.element_ids[1];
        let e3 = p30.element_ids[2];
        let e4 = p30.element_ids[3];
        let e5 = p30.element_ids[4];

        exec(
            TreeOp::MoveElement,
            &obj([
                ("ids", CValue::Arr(vec![text(&e1.to_string()), text(&e2.to_string()), text(&e3.to_string())])),
                ("to", text(&sub_id.to_string())),
            ]),
        );
        exec(TreeOp::RenameElement, &obj([("id", text(&e1.to_string())), ("name", text("Plane 1"))]));
        exec(TreeOp::RenameElement, &obj([("id", text(&e2.to_string())), ("name", text("Plane 1"))]));
        exec(
            TreeOp::SetVisibility,
            &obj([
                ("ids", CValue::Arr(vec![text(&e4.to_string()), text(&e5.to_string())])),
                ("visible", CValue::Bool(false)),
            ]),
        );
        (journal, p30.state.tree().state_digest().unwrap())
    };

    let mut live = Recording::new(ReplaySource::new(make_observations(4)));
    let (j1, d1) = run(&mut live);
    let recorded = live.observations().to_vec();
    assert_eq!(recorded.len(), 4, "two UUIDv7 draws record 4 observations (NowMs + RandomBits each)");
    let mut replay = ReplaySource::new(recorded);
    let (j2, d2) = run(&mut replay);
    assert_eq!(j1, j2);
    assert_eq!(d1, d2);
}

/// Handler and apply enforce the exact same subtree-aware depth limit (`to_depth + 1 + h <= MAX_GROUP_DEPTH`).
#[test]
fn move_group_with_subtree_under_deep_target_rejected_by_handler() {
    let mut p30 = load_p30();
    let mut src = ReplaySource::new(make_observations(80));

    // Build a chain of depth MAX_GROUP_DEPTH - 1 (so a leaf group fits at depth MAX_GROUP_DEPTH,
    // but a group with a child group of height 1 would reach MAX_GROUP_DEPTH + 1).
    let mut deep_parent = p30.root;
    for i in 1..MAX_GROUP_DEPTH {
        let evs = run_tree(
            TreeOp::CreateGroup,
            &p30.state,
            &obj([("name", text(&format!("D{i}"))), ("parent", text(&deep_parent.to_string()))]),
            p30.namespace,
            &mut src,
        )
        .unwrap();
        deep_parent = group_id_from_event(&evs[0]);
        p30.state.apply_commit(&evs).unwrap();
    }

    // Create group A (depth 1) with child group B (subtree height of A is 1), and leaf group C (height 0).
    let ev_a = run_tree(TreeOp::CreateGroup, &p30.state, &obj([("name", text("A"))]), p30.namespace, &mut src).unwrap();
    let group_a = group_id_from_event(&ev_a[0]);
    p30.state.apply_commit(&ev_a).unwrap();

    let ev_b = run_tree(
        TreeOp::CreateGroup,
        &p30.state,
        &obj([("name", text("B")), ("parent", text(&group_a.to_string()))]),
        p30.namespace,
        &mut src,
    )
    .unwrap();
    let _group_b = group_id_from_event(&ev_b[0]);
    p30.state.apply_commit(&ev_b).unwrap();

    let ev_c = run_tree(TreeOp::CreateGroup, &p30.state, &obj([("name", text("C"))]), p30.namespace, &mut src).unwrap();
    let group_c = group_id_from_event(&ev_c[0]);
    p30.state.apply_commit(&ev_c).unwrap();

    // Moving A (subtree height 1) under deep_parent (depth MAX_GROUP_DEPTH - 1) must be refused by
    // the handler itself before emit, with Validation(Hierarchy).
    let err = run_tree(
        TreeOp::MoveElement,
        &p30.state,
        &obj([("ids", CValue::Arr(vec![text(&group_a.to_string())])), ("to", text(&deep_parent.to_string()))]),
        p30.namespace,
        &mut src,
    )
    .expect_err("handler must reject subtree exceeding MAX_GROUP_DEPTH");
    assert!(matches!(err, BusError::Validation(ValidationReason::Hierarchy, _)), "got: {err:?}");

    // Moving leaf group C (height 0) under deep_parent reaches exactly MAX_GROUP_DEPTH and succeeds.
    let ev_ok = run_tree(
        TreeOp::MoveElement,
        &p30.state,
        &obj([("ids", CValue::Arr(vec![text(&group_c.to_string())])), ("to", text(&deep_parent.to_string()))]),
        p30.namespace,
        &mut src,
    )
    .expect("leaf group at depth MAX_GROUP_DEPTH must succeed");
    p30.state.apply_commit(&ev_ok).unwrap();
    p30.state.tree().verify().unwrap();
}

/// Registry definitions pass schema keyword check and registry validator, and all 4 commands
/// carry W2, D0, head_move, cli+mcp exposure.
#[test]
fn registry_definitions_and_bus_registration() {
    let defs = command_defs().unwrap();
    assert_eq!(defs.len(), 4);
    let catalogue = Catalogue::geo_frame().unwrap();
    for d in &defs {
        assert_eq!(check_schema(&d.params_schema, &catalogue), vec![], "{}", d.id);
        assert_eq!(check_schema(&d.result_schema, &catalogue), vec![], "{}", d.id);
        let CValue::Obj(o) = d.to_value() else { panic!() };
        assert_eq!(o.get("side_effect"), Some(&text("W2")));
        assert_eq!(o.get("determinism"), Some(&text("D0")));
        assert_eq!(o.get("undo"), Some(&text("head_move")));
        assert_eq!(o.get("exposure"), Some(&CValue::Arr(vec![text("cli"), text("mcp")])));
    }
    let all = fid_project_cmd::command_defs().unwrap();
    assert_eq!(all.len(), 15);
    let decls: Vec<CommandDecl> = all.iter().map(CommandDecl::from_def).collect();
    assert_eq!(validate(&decls), vec![]);

    let source = make_source(4);
    let mut set = HandlerSet::new();
    fid_project_cmd::register(&mut set, Uuid::from_u128(1), &source).unwrap();
    assert_eq!(set.len(), 15);
    for op in TreeOp::ALL {
        assert!(set.get(&format!("{}@1", op.command())).is_ok());
    }
}

/// Error paths: unknown entity -> not_found, bad/overlong name -> validation/name, group in
/// set_visibility -> validation/bad_reference, moving root -> validation/hierarchy, 10_001 IDs /
/// duplicate IDs / non-canonical uppercase UUID -> validation/schema.
#[test]
fn error_paths_are_typed_and_commit_nothing() {
    let p30 = load_p30();
    let mut src = ReplaySource::new(make_observations(2));
    let missing = Uuid::from_u128(0x0190_0000_0000_7000_8000_ffff_ffff_ffff);

    // Unknown ID on rename -> NotFound
    let err = run_tree(
        TreeOp::RenameElement,
        &p30.state,
        &obj([("id", text(&missing.to_string())), ("name", text("X"))]),
        p30.namespace,
        &mut src,
    )
    .unwrap_err();
    assert_eq!(err.code(), Code::NotFound);

    // Control char in name -> Validation/Name
    let err = run_tree(
        TreeOp::RenameElement,
        &p30.state,
        &obj([("id", text(&p30.element_ids[0].to_string())), ("name", text("bad\nname"))]),
        p30.namespace,
        &mut src,
    )
    .unwrap_err();
    assert!(matches!(err, BusError::Validation(ValidationReason::Name, _)));

    // 257-character name -> Validation/Name
    let long_name = "a".repeat(257);
    let err = run_tree(
        TreeOp::RenameElement,
        &p30.state,
        &obj([("id", text(&p30.element_ids[0].to_string())), ("name", text(&long_name))]),
        p30.namespace,
        &mut src,
    )
    .unwrap_err();
    assert!(matches!(err, BusError::Validation(ValidationReason::Name, _)));

    // Group passed to set_visibility -> Validation/BadReference
    let err = run_tree(
        TreeOp::SetVisibility,
        &p30.state,
        &obj([("ids", CValue::Arr(vec![text(&p30.root.to_string())])), ("visible", CValue::Bool(false))]),
        p30.namespace,
        &mut src,
    )
    .unwrap_err();
    assert!(matches!(err, BusError::Validation(ValidationReason::BadReference, _)));

    // Moving root group -> Validation/Hierarchy
    let err = run_tree(
        TreeOp::MoveElement,
        &p30.state,
        &obj([("ids", CValue::Arr(vec![text(&p30.root.to_string())])), ("to", text(&p30.root.to_string()))]),
        p30.namespace,
        &mut src,
    )
    .unwrap_err();
    assert!(matches!(err, BusError::Validation(ValidationReason::Hierarchy, _)));

    // Duplicate IDs -> Validation/Schema
    let e0 = p30.element_ids[0].to_string();
    let err = run_tree(
        TreeOp::MoveElement,
        &p30.state,
        &obj([("ids", CValue::Arr(vec![text(&e0), text(&e0)])), ("to", text(&p30.root.to_string()))]),
        p30.namespace,
        &mut src,
    )
    .unwrap_err();
    assert!(matches!(err, BusError::Validation(ValidationReason::Schema, _)));

    // Non-canonical (uppercase) UUID -> Validation/Schema
    let upper = "01930000-0001-7001-8000-00000000100A";
    let err = run_tree(
        TreeOp::RenameElement,
        &p30.state,
        &obj([("id", text(upper)), ("name", text("ok"))]),
        p30.namespace,
        &mut src,
    )
    .unwrap_err();
    assert!(matches!(err, BusError::Validation(ValidationReason::Schema, _)));

    // 10,001 IDs -> Validation/Schema
    let too_many: Vec<CValue> = (1u128..=10_001)
        .map(|i| text(&Uuid::from_u128(0x0193_0000_0003_7000_8000_0000_0000_0000 | i).to_string()))
        .collect();
    let err = run_tree(
        TreeOp::MoveElement,
        &p30.state,
        &obj([("ids", CValue::Arr(too_many)), ("to", text(&p30.root.to_string()))]),
        p30.namespace,
        &mut src,
    )
    .unwrap_err();
    assert!(matches!(err, BusError::Validation(ValidationReason::Schema, _)));

    // Empty name is allowed (0..=256 chars, REQ-004).
    let evs = run_tree(
        TreeOp::RenameElement,
        &p30.state,
        &obj([("id", text(&p30.element_ids[0].to_string())), ("name", text(""))]),
        p30.namespace,
        &mut src,
    )
    .unwrap();
    assert_eq!(evs.len(), 1);
}

/// REQ-CORE.PRJ.TREE.GROUP-002: moving 10,000 elements in one commit through the bus handler
/// preserves exact relative order.
#[test]
fn move_10000_elements_in_one_commit_preserves_order() {
    let ns = Uuid::from_u128(0x2222_3333_4444_4555_8666_7777_8888_9999);
    let mut reg = ElementTypeRegistry::new();
    reg.register(ElementTypeDef::new("test.pt", obj([]), "point").unwrap()).unwrap();
    let mut tree = Tree::new(ns, Arc::new(reg)).unwrap();
    let root = tree.root();

    let target_gid = Uuid::from_u128(0x0193_0000_0001_7000_8000_0000_0000_0001);
    tree.apply(
        GROUP_CREATED,
        &obj([("id", text(&target_gid.to_string())), ("name", text("Bulk")), ("parent", text(&root.to_string()))]),
    )
    .unwrap();

    let mut ids = Vec::with_capacity(10_000);
    let mut events = Vec::with_capacity(10_000);
    for i in 1u128..=10_000 {
        let eid = Uuid::from_u128(0x0193_0000_0002_7000_8000_0000_0000_0000 | i);
        ids.push(eid);
        events.push(TreeEvent::new(
            ELEMENT_CREATED,
            obj([
                ("id", text(&eid.to_string())),
                ("meta", CValue::Obj(BTreeMap::new())),
                ("name", text(&format!("p{i}"))),
                ("parent", text(&root.to_string())),
                ("type", text("test.pt")),
                ("visible", CValue::Bool(true)),
            ]),
        ));
    }
    tree.apply_commit(&events).unwrap();
    let mut state = TreeState::new(tree, CanonDigest([0u8; 32])).unwrap();

    // Reverse order move of all 10,000 elements in one single commit via the bus handler.
    let reversed: Vec<Uuid> = ids.iter().rev().copied().collect();
    let params = obj([
        ("ids", CValue::Arr(reversed.iter().map(|id| text(&id.to_string())).collect())),
        ("to", text(&target_gid.to_string())),
    ]);
    let mut src = ReplaySource::new(Vec::new());
    let move_events = run_tree(TreeOp::MoveElement, &state, &params, ns, &mut src).unwrap();
    assert_eq!(move_events.len(), 1);
    assert_eq!(move_events[0].type_id().as_str(), ELEMENTS_MOVED);
    state.apply_commit(&move_events).unwrap();
    assert_eq!(state.tree().children_of(&target_gid), reversed.as_slice());
    assert_eq!(state.tree().children_of(&root), &[target_gid]);
    state.tree().verify().unwrap();
}
