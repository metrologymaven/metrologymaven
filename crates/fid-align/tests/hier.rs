//! Acceptance and invariant tests for `ALN.MODEL.HIER.CHAIN` (`spec/aln/model/hier/chain/NODE.md`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_align::element::transform_from;
use fid_align::{
    check_parent, command_defs, compose_to_root, invoke_set_parent_cli, invoke_set_parent_mcp, mirror_frame_id,
    replay_set_parent, set_parent, update_alignment_transform, AlgorithmId, Alignment, AlignmentHierarchy,
    AlignmentModel, AlignmentSolver, AlignmentStatus, CreateRequest, ElementRef, HierError, InputRef, ParamSet,
    ParentTarget, PrincipalKind, Provenance, ProvenanceState, ReportRef, SetParentEvent, SetParentParams, SolveStatus,
    SolverError, SolverOutput, SolverRegistry, Transform, MAX_HIERARCHY_DEPTH, SET_PARENT,
};
use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse_with_limits, CValue};
use fid_dep::{EdgeIndex, EdgesSet, ElementState, ElementStates};
use fid_geo_frame::FrameKind;
use fid_project::{Bits, IdService, Observation, ReplaySource};
use fid_types::Digest;
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};

const CHAIN3_JSON: &str = include_str!("../../../fixtures/aln/model/hier/chain3.fidproj");
const CHAIN64_JSON: &str = include_str!("../../../fixtures/aln/model/hier/chain64.json");

fn digest_tag(byte: u8) -> Digest {
    Digest::from_bytes([byte; 32])
}

fn parse_hex_f64(v: &CValue) -> f64 {
    let CValue::Str(s) = v else { panic!("expected hex float string") };
    let hex = s.strip_prefix("0x").expect("0x prefix");
    let bits = u64::from_str_radix(hex, 16).expect("valid hex");
    f64::from_bits(bits)
}

fn parse_point(v: &CValue) -> [f64; 3] {
    let CValue::Arr(a) = v else { panic!("expected 3-element array") };
    assert_eq!(a.len(), 3);
    [parse_hex_f64(&a[0]), parse_hex_f64(&a[1]), parse_hex_f64(&a[2])]
}

fn dist3(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn parse_uuid(v: &CValue) -> Uuid {
    let CValue::Str(s) = v else { panic!("expected uuid string") };
    Uuid::parse(s).expect("valid uuid")
}

/// Constructs an [`Alignment`] with the given ID, parent and transform via its canonical JSON form.
fn build_alignment(
    id: Uuid,
    input_id: Uuid,
    parent: Option<(Uuid, Digest)>,
    transform: &Transform,
    report_id: Uuid,
) -> Alignment {
    let algo = AlgorithmId::parse("align.test.fixed@1.0.0").unwrap();
    let input_ref = InputRef { id: input_id, digest: digest_tag(0x11) };
    let report_ref = ReportRef { id: report_id, digest: digest_tag(0x22) };
    let parent_ref = parent.map(|(pid, pd)| ElementRef { id: pid, digest: pd });
    let prov = Provenance::new("alice", PrincipalKind::Human, ProvenanceState::Human).unwrap();

    // Compute the exact content digest matching Alignment::from_value.
    let mut content = BTreeMap::new();
    content.insert("algorithm".into(), CValue::Str(algo.to_string()));
    content.insert("inputs".into(), CValue::Arr(vec![CValue::Str(input_ref.digest.as_str().into())]));
    content.insert("params".into(), CValue::Obj(BTreeMap::new()));
    content.insert("parent".into(), parent_ref.map_or(CValue::Null, |p| p.to_value()));
    content.insert("report".into(), CValue::Str(report_ref.digest.as_str().into()));
    content.insert("transform".into(), fid_align::element::transform_value(transform));
    let digest = Digest::from_bytes(fid_canon::digest(&CValue::Obj(content)).unwrap().0);

    let mut elem = BTreeMap::new();
    elem.insert("algorithm".into(), CValue::Str(algo.to_string()));
    elem.insert("digest".into(), CValue::Str(digest.as_str().into()));
    elem.insert("failure".into(), CValue::Null);
    elem.insert("id".into(), CValue::Str(id.to_string()));
    elem.insert("inputs".into(), CValue::Arr(vec![input_ref.to_value()]));
    elem.insert("params".into(), CValue::Obj(BTreeMap::new()));
    elem.insert("parent".into(), parent_ref.map_or(CValue::Null, |p| p.to_value()));
    elem.insert("provenance".into(), prov.to_value());
    elem.insert("report".into(), report_ref.to_value());
    elem.insert("status".into(), CValue::Str("ok".into()));
    elem.insert("transform".into(), fid_align::element::transform_value(transform));

    let bytes = fid_canon::to_canonical_bytes(&CValue::Obj(elem)).expect("canonical json");
    Alignment::from_json(&bytes).expect("valid alignment")
}

struct Chain3Fixture {
    model: AlignmentModel,
    hier: AlignmentHierarchy,
    edges: EdgeIndex,
    states: ElementStates,
    a: Uuid,
    b: Uuid,
    c: Uuid,
    dependent_result: Uuid,
    test_points: Vec<([f64; 3], [f64; 3])>,
}

fn load_chain3() -> Chain3Fixture {
    let root = parse_with_limits(CHAIN3_JSON.as_bytes(), 1024 * 1024, 16).unwrap();
    let CValue::Obj(o) = root else { panic!("root object") };
    let input_id = parse_uuid(o.get("input_element").unwrap());
    let dep_res = parse_uuid(o.get("dependent_result").unwrap());
    let CValue::Arr(aligns) = o.get("alignments").unwrap() else { panic!("alignments array") };

    let mut model = AlignmentModel::new();
    let mut edges = EdgeIndex::new();
    let mut states = ElementStates::new();

    edges.apply(&[EdgesSet::from_set(input_id, &BTreeSet::new()).unwrap()]).unwrap();
    states.insert(input_id, ElementState::Current);

    let mut ids_by_label: BTreeMap<String, Uuid> = BTreeMap::new();
    let mut digests: BTreeMap<Uuid, Digest> = BTreeMap::new();
    for (idx, item) in aligns.iter().enumerate() {
        let CValue::Obj(ao) = item else { panic!("alignment entry") };
        let CValue::Str(label) = ao.get("label").unwrap() else { panic!("label") };
        let id = parse_uuid(ao.get("id").unwrap());
        let parent_id = match ao.get("parent").unwrap() {
            CValue::Null => None,
            v => Some(parse_uuid(v)),
        };
        let parent_with_digest = parent_id.map(|pid| (pid, *digests.get(&pid).unwrap()));
        let xform = transform_from(ao.get("transform").unwrap()).unwrap();
        let report_id = Uuid::from_u128(0x9000 + idx as u128);
        let a = build_alignment(id, input_id, parent_with_digest, &xform, report_id);
        digests.insert(id, a.digest());
        model.insert_stored(a).unwrap();

        let mut in_set = BTreeSet::new();
        in_set.insert(input_id);
        if let Some(pid) = parent_id {
            in_set.insert(pid);
        }
        edges.apply(&[EdgesSet::from_set(id, &in_set).unwrap()]).unwrap();
        states.insert(id, ElementState::Current);
        ids_by_label.insert(label.clone(), id);
    }

    let a = ids_by_label["A"];
    let b = ids_by_label["B"];
    let c = ids_by_label["C"];

    // Dependent result depends on C.
    let mut dep_inputs = BTreeSet::new();
    dep_inputs.insert(c);
    edges.apply(&[EdgesSet::from_set(dep_res, &dep_inputs).unwrap()]).unwrap();
    states.insert(dep_res, ElementState::Current);

    let hier = AlignmentHierarchy::from_model(&model).unwrap();

    let CValue::Arr(pts) = o.get("test_points").unwrap() else { panic!("test_points") };
    let test_points = pts
        .iter()
        .map(|entry| {
            let CValue::Obj(po) = entry else { panic!("point entry") };
            (parse_point(po.get("point").unwrap()), parse_point(po.get("expected_root").unwrap()))
        })
        .collect();

    Chain3Fixture { model, hier, edges, states, a, b, c, dependent_result: dep_res, test_points }
}

/// AC-1 + REQ-001 + REQ-002:
/// Every alignment has a mirror frame in GEO.FRAME.TREE, `compose_to_root(C)` equals
/// `T_A · T_B · T_C` within 1e-12 m on all fixture test points, and composing it with its
/// inverse maps every point within 10 m of the origin back within 1e-12 m.
#[test]
fn ac1_compose_to_root_matches_chain3_and_round_trips() {
    let f = load_chain3();

    // REQ-001: mirror frames exist with FrameKind::Alignment and matching parent/transform.
    assert_eq!(f.hier.mirrors().len(), 3);
    for (id, expected_parent) in
        [(f.a, f.hier.frame_tree().world()), (f.b, mirror_frame_id(f.a)), (f.c, mirror_frame_id(f.b))]
    {
        let mf = f.hier.mirror_of(&id).expect("mirror exists");
        assert_eq!(mf.alignment, id);
        let frame = f.hier.frame_tree().get(mf.frame).expect("frame in tree");
        assert_eq!(frame.kind, FrameKind::Alignment);
        assert_eq!(frame.parent, Some(expected_parent));
        assert_eq!(frame.to_parent, *f.model.get(&id).unwrap().transform());
    }

    // Both AlignmentHierarchy::compose_to_root and free compose_to_root give identical transforms.
    let from_hier = f.hier.compose_to_root(f.c).unwrap();
    let from_model = compose_to_root(&f.model, f.c).unwrap();
    assert_eq!(from_hier, from_model);

    // Direct T_A * T_B * T_C check and independent fixture points check (<= 1e-12 m).
    let ta = f.model.get(&f.a).unwrap().transform();
    let tb = f.model.get(&f.b).unwrap().transform();
    let tc = f.model.get(&f.c).unwrap().transform();
    let direct = ta.compose(tb).unwrap().compose(tc).unwrap();
    let inv = from_hier.inverse().unwrap();
    let roundtrip = inv.compose(&from_hier).unwrap();

    for (pt, expected_root) in &f.test_points {
        let got_hier = from_hier.apply_point(pt).unwrap();
        let got_direct = direct.apply_point(pt).unwrap();
        assert!(
            dist3(got_hier, *expected_root) <= 1e-12,
            "compose_to_root error {} > 1e-12",
            dist3(got_hier, *expected_root)
        );
        assert!(dist3(got_hier, got_direct) <= 1e-12, "direct compose diff {} > 1e-12", dist3(got_hier, got_direct));
        let back = roundtrip.apply_point(pt).unwrap();
        assert!(dist3(back, *pt) <= 1e-12, "round-trip error {} > 1e-12", dist3(back, *pt));
    }
}

/// AC-2 + REQ-003 (error path):
/// `fid align set-parent --alignment A --parent C --json` fails with `hierarchy_cycle{[A, C, B, A]}`
/// and leaves the model, mirror tree, edge index, element states untouched (and journals nothing).
#[test]
fn ac2_cycle_fails_with_exact_path_and_journals_nothing() {
    let mut f = load_chain3();
    let model_before = f.model.clone();
    let hier_before = f.hier.clone();
    let edges_before = f.edges.clone();
    let states_before = f.states.clone();

    let a_s = f.a.to_string();
    let c_s = f.c.to_string();
    let err = invoke_set_parent_cli(
        &mut f.model,
        &mut f.hier,
        &mut f.edges,
        &mut f.states,
        &["--alignment", &a_s, "--parent", &c_s, "--json"],
    )
    .expect_err("must reject cycle");

    assert_eq!(err, HierError::HierarchyCycle { path: vec![f.a, f.c, f.b, f.a] });
    assert_eq!(err.code(), "hierarchy_cycle");
    // Nothing changed, nothing journaled.
    assert_eq!(f.model, model_before);
    assert_eq!(f.hier, hier_before);
    assert_eq!(f.edges, edges_before);
    assert_eq!(f.states, states_before);

    // Self-parent also fails with [A, A].
    let self_err = check_parent(&f.model, f.a, Some(f.a)).expect_err("self cycle");
    assert_eq!(self_err, HierError::HierarchyCycle { path: vec![f.a, f.a] });
}

struct FixedSolver {
    algo: AlgorithmId,
    xform: Transform,
}

impl AlignmentSolver for FixedSolver {
    fn algorithm_id(&self) -> AlgorithmId {
        self.algo.clone()
    }

    fn solve(&self, _: &[InputRef], _: &ParamSet, _: Option<&Transform>) -> Result<SolverOutput, SolverError> {
        Ok(SolverOutput {
            transform: self.xform,
            report: ReportRef { id: Uuid::from_u128(0xdead_beef), digest: digest_tag(0x55) },
            status: SolveStatus::Ok,
        })
    }
}

/// AC-3 + REQ-004:
/// Given a chain of 64 alignments, parenting a 65th alignment to the deepest fails with
/// `hierarchy_too_deep` (both through `align.set_parent@1` and `AlignmentModel::create`) and
/// journals nothing.
#[test]
fn ac3_depth_64_succeeds_and_65th_fails_hierarchy_too_deep() {
    let root = parse_with_limits(CHAIN64_JSON.as_bytes(), 2 * 1024 * 1024, 16).unwrap();
    let CValue::Obj(o) = root else { panic!("root object") };
    let input_id = parse_uuid(o.get("input_element").unwrap());
    let CValue::Arr(aligns) = o.get("alignments").unwrap() else { panic!("alignments") };
    assert_eq!(aligns.len(), MAX_HIERARCHY_DEPTH);

    let mut model = AlignmentModel::new();
    let mut edges = EdgeIndex::new();
    let mut states = ElementStates::new();
    let mut world = BTreeMap::new();
    world.insert(input_id, digest_tag(0x11));
    edges.apply(&[EdgesSet::from_set(input_id, &BTreeSet::new()).unwrap()]).unwrap();
    states.insert(input_id, ElementState::Current);

    let mut digests: BTreeMap<Uuid, Digest> = BTreeMap::new();
    let mut deepest = Uuid::NIL;
    for (idx, item) in aligns.iter().enumerate() {
        let CValue::Obj(ao) = item else { panic!("align") };
        let id = parse_uuid(ao.get("id").unwrap());
        let parent_id = match ao.get("parent").unwrap() {
            CValue::Null => None,
            v => Some(parse_uuid(v)),
        };
        let parent_ref = parent_id.map(|pid| (pid, digests[&pid]));
        let xform = transform_from(ao.get("transform").unwrap()).unwrap();
        let a = build_alignment(id, input_id, parent_ref, &xform, Uuid::from_u128(0xa000 + idx as u128));
        digests.insert(id, a.digest());
        model.insert_stored(a).unwrap();

        let mut in_set = BTreeSet::new();
        in_set.insert(input_id);
        if let Some(pid) = parent_id {
            in_set.insert(pid);
        }
        edges.apply(&[EdgesSet::from_set(id, &in_set).unwrap()]).unwrap();
        states.insert(id, ElementState::Current);
        deepest = id;
    }

    // 64-deep chain builds and composes accurately!
    let mut hier = AlignmentHierarchy::from_model(&model).unwrap();
    let composed = hier.compose_to_root(deepest).unwrap();
    let probe = parse_point(o.get("probe_point").unwrap());
    let expected = parse_point(o.get("expected_deepest_probe_root").unwrap());
    let got = composed.apply_point(&probe).unwrap();
    assert!(dist3(got, expected) <= 1e-12, "depth-64 compose error {}", dist3(got, expected));

    // Add a 65th alignment at root first, then try parenting it under the 64th -> hierarchy_too_deep.
    let CValue::Obj(extra) = o.get("extra_65th").unwrap() else { panic!("extra_65th") };
    let extra_id = parse_uuid(extra.get("id").unwrap());
    let extra_xform = transform_from(extra.get("transform").unwrap()).unwrap();
    let extra_align = build_alignment(extra_id, input_id, None, &extra_xform, Uuid::from_u128(0xb000));
    model.insert_stored(extra_align.clone()).unwrap();
    let mut in_set = BTreeSet::new();
    in_set.insert(input_id);
    edges.apply(&[EdgesSet::from_set(extra_id, &in_set).unwrap()]).unwrap();
    states.insert(extra_id, ElementState::Current);
    hier.register_alignment(&extra_align).unwrap();

    let model_before = model.clone();
    let hier_before = hier.clone();
    let edges_before = edges.clone();
    let states_before = states.clone();

    let err = set_parent(
        &mut model,
        &mut hier,
        &mut edges,
        &mut states,
        &SetParentParams { alignment: extra_id, parent: ParentTarget::Alignment(deepest) },
    )
    .expect_err("65th depth must fail");
    assert_eq!(err, HierError::HierarchyTooDeep { depth: 65 });
    assert_eq!(err.code(), "hierarchy_too_deep");
    assert_eq!(model, model_before);
    assert_eq!(hier, hier_before);
    assert_eq!(edges, edges_before);
    assert_eq!(states, states_before);

    // AlignmentModel::create also enforces the 64-alignment depth cap.
    let algo = AlgorithmId::parse("align.test.fixed@1.0.0").unwrap();
    let mut solvers = SolverRegistry::new();
    solvers.register(Box::new(FixedSolver { algo: algo.clone(), xform: Transform::IDENTITY })).unwrap();
    let mut src = ReplaySource::new(vec![
        Observation::NowMs(1_700_000_000_000),
        Observation::RandomBits(Bits::new(74, 0x42).unwrap()),
    ]);
    let existing = BTreeSet::new();
    let mut ids = IdService::new(Uuid::from_u128(0xc000), &mut src, &existing);
    let req = CreateRequest {
        algorithm: algo,
        inputs: vec![input_id],
        params: ParamSet::new(),
        parent: Some(deepest),
        principal: "alice".into(),
        kind: PrincipalKind::Human,
    };
    let create_err = model.create(&req, &solvers, &world, &mut edges, &mut ids).expect_err("create at 65 fails");
    assert_eq!(create_err.code(), "hierarchy_too_deep");
}

/// AC-4 + AC-5 + REQ-005 + REQ-007:
/// `fid align set-parent --alignment C --parent A` replaces C's parent edge in CORE.DEP,
/// marks C and its dependent result `outdated` in the same commit, writes one journal event,
/// matches MCP bytes identically, and exact replay produces `IDENTICAL` state.
/// Also verifies `parent: root` makes an alignment top-level (REQ-007).
#[test]
fn ac4_ac5_set_parent_invalidation_replay_and_root() {
    let mut f = load_chain3();
    let mut f_mcp = load_chain3();
    let mut f_replay = load_chain3();

    let c_s = f.c.to_string();
    let a_s = f.a.to_string();

    // CLI invocation.
    let (cli_bytes, event) = invoke_set_parent_cli(
        &mut f.model,
        &mut f.hier,
        &mut f.edges,
        &mut f.states,
        &["--alignment", &c_s, "--parent", &a_s],
    )
    .unwrap();

    // MCP invocation produces byte-identical output and identical event.
    let mcp_payload = format!(r#"{{"alignment":"{c_s}","parent":"{a_s}"}}"#);
    let (mcp_bytes, mcp_event) = invoke_set_parent_mcp(
        &mut f_mcp.model,
        &mut f_mcp.hier,
        &mut f_mcp.edges,
        &mut f_mcp.states,
        mcp_payload.as_bytes(),
    )
    .unwrap();
    assert_eq!(cli_bytes, mcp_bytes);
    assert_eq!(event, mcp_event);

    // C's parent is now A (in model, mirror tree, and CORE.DEP edges).
    assert_eq!(f.model.get(&f.c).unwrap().parent().map(|p| p.id), Some(f.a));
    assert_eq!(f.hier.frame_tree().get(mirror_frame_id(f.c)).unwrap().parent, Some(mirror_frame_id(f.a)));
    assert!(f.edges.inputs_of(&f.c).unwrap().contains(&f.a));
    assert!(!f.edges.inputs_of(&f.c).unwrap().contains(&f.b));

    // C and its dependent result are marked outdated in that commit; A and B stay current.
    assert_eq!(f.states.get(&f.c), Some(ElementState::Outdated));
    assert_eq!(f.states.get(&f.dependent_result), Some(ElementState::Outdated));
    assert_eq!(f.model.get(&f.c).unwrap().status(), AlignmentStatus::Outdated);
    assert_eq!(f.states.get(&f.a), Some(ElementState::Current));
    assert_eq!(f.states.get(&f.b), Some(ElementState::Current));
    assert_eq!(event.marked.as_ref().unwrap().elements().len(), 2);

    // Composition of C now equals T_A * T_C (B is no longer in the chain).
    let expected_ac = f.model.get(&f.a).unwrap().transform().compose(f.model.get(&f.c).unwrap().transform()).unwrap();
    let got_ac = f.hier.compose_to_root(f.c).unwrap();
    for (pt, _) in &f.test_points {
        assert!(dist3(got_ac.apply_point(pt).unwrap(), expected_ac.apply_point(pt).unwrap()) <= 1e-12);
    }

    // AC-5: Serialize the single journal event to canonical JSON, parse it back, and replay.
    let event_bytes = event.to_json().unwrap();
    let decoded_event = SetParentEvent::from_json(&event_bytes).unwrap();
    assert_eq!(decoded_event, event);

    let replayed_result = replay_set_parent(
        &mut f_replay.model,
        &mut f_replay.hier,
        &mut f_replay.edges,
        &mut f_replay.states,
        &decoded_event,
    )
    .unwrap();
    assert_eq!(replayed_result.to_json().unwrap(), cli_bytes);
    assert_eq!(f_replay.model, f.model);
    assert_eq!(f_replay.hier.frame_tree().digest(), f.hier.frame_tree().digest());
    assert_eq!(f_replay.edges, f.edges);
    assert_eq!(f_replay.states, f.states);

    // REQ-007: `align.set_parent` with `root` makes B a top-level alignment under root.
    let b_s = f.b.to_string();
    let (root_bytes, root_event) = invoke_set_parent_cli(
        &mut f.model,
        &mut f.hier,
        &mut f.edges,
        &mut f.states,
        &["--alignment", &b_s, "--parent", "root", "--json"],
    )
    .unwrap();
    assert_eq!(f.model.get(&f.b).unwrap().parent(), None);
    assert_eq!(f.hier.frame_tree().get(mirror_frame_id(f.b)).unwrap().parent, Some(f.hier.frame_tree().world()));
    assert!(!f.edges.inputs_of(&f.b).unwrap().contains(&f.a));
    assert_eq!(f.states.get(&f.b), Some(ElementState::Outdated));
    assert_eq!(root_event.parent, ParentTarget::Root);
    assert_eq!(root_event.parent_digest, None);
    assert!(std::str::from_utf8(&root_bytes).unwrap().contains(r#""parent":"root""#));
}

/// REQ-006:
/// When an alignment's transform changes, its mirror frame updates in the same commit and
/// every descendant alignment and dependent result is marked `outdated` through CORE.DEP.
#[test]
fn req006_transform_update_syncs_mirror_and_invalidates_descendants() {
    let mut f = load_chain3();
    let new_t = Transform::Rigid(
        fid_geo_frame::Rigid::new([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [3.0, -4.0, 5.0]).unwrap(),
    );

    let updated = update_alignment_transform(&mut f.model, &mut f.hier, &f.edges, &mut f.states, f.a, new_t).unwrap();
    assert_eq!(updated.alignment, f.a);
    // B, C and dependent_result are all downstream of A.
    assert_eq!(updated.marked_outdated, 3);
    assert_eq!(f.hier.frame_tree().get(mirror_frame_id(f.a)).unwrap().to_parent, new_t);
    assert_eq!(f.states.get(&f.a), Some(ElementState::Current));
    assert_eq!(f.states.get(&f.b), Some(ElementState::Outdated));
    assert_eq!(f.states.get(&f.c), Some(ElementState::Outdated));
    assert_eq!(f.states.get(&f.dependent_result), Some(ElementState::Outdated));
    assert_eq!(f.model.get(&f.b).unwrap().status(), AlignmentStatus::Outdated);
    assert_eq!(f.model.get(&f.c).unwrap().status(), AlignmentStatus::Outdated);
}

struct AlignHooks(BTreeSet<String>);

impl Hooks for AlignHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no quantity literals here")))
    }

    fn entity(&self, id: &str) -> RefLookup {
        if self.0.contains(id) {
            RefLookup::Kind("alignment".to_string())
        } else {
            RefLookup::Missing
        }
    }
}

/// Registry definition and JSON Schema validation via `fid-bus::validate::Compiled`.
#[test]
fn set_parent_command_def_schemas_validate_params_and_results() {
    let defs = command_defs().unwrap();
    let def = defs.iter().find(|d| format!("{}@{}", d.id, d.major) == SET_PARENT).expect("set_parent registered");
    assert_eq!(def.side_effect.as_str(), "W2");
    assert_eq!(def.determinism.as_str(), "D0");
    assert_eq!(def.undo.as_str(), "head_move");

    let params_schema = Compiled::compile(&def.params_schema).expect("params schema compiles");
    let result_schema = Compiled::compile(&def.result_schema).expect("result schema compiles");

    let f = load_chain3();
    let hooks = AlignHooks([f.a.to_string(), f.b.to_string(), f.c.to_string()].into_iter().collect());
    let p_align = SetParentParams { alignment: f.c, parent: ParentTarget::Alignment(f.a) }.to_value();
    let p_root = SetParentParams { alignment: f.c, parent: ParentTarget::Root }.to_value();
    assert!(params_schema.evaluate(&p_align, &hooks).is_ok());
    assert!(params_schema.evaluate(&p_root, &hooks).is_ok());

    let res = fid_align::SetParentResult { alignment: f.c, parent: ParentTarget::Root, marked_outdated: 2 }.to_value();
    assert!(result_schema.evaluate(&res, &hooks).is_ok());
}
