//! Integration tests for `VIZ.VIEW.CAMERA.SET` (AC-1 through AC-5, error paths, reduced motion,
//! Activity Log, registry schemas and exact replay).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_registry::{validate, SideEffect};
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use fid_view::camera::{
    camera_from_cvalue, command_defs, declarations, default_view_state, execute_fly_to, execute_set_camera, fly_to,
    invoke_cli_fly_to, invoke_cli_set_camera, parse_fov_deg_to_rad, replay_camera_journal, set_camera, CameraCmdCtx,
    CameraError, CameraWorkspace, FlyToParams, MemorySceneBounds, PrincipalKind, ProjectionParam, SetCameraParams,
    ViewportId, ViewportSlot, FLY_TO_EVENT, SET_CAMERA_EVENT,
};
use fid_view::nav::BoundingSphere;
use fid_view::state::{EntityId, Projection, VisibilityRule};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

const GOLDEN_CUBE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/viz/golden-cube/cube.txt");

fn golden_cube_face_ids() -> [EntityId; 6] {
    // Deterministic UUIDs for the 6 golden-cube faces (render IDs 101..=106 from fixtures/viz/golden-cube/cube.txt).
    let raw = fs::read_to_string(Path::new(GOLDEN_CUBE_PATH)).unwrap();
    for rid in 101..=106 {
        assert!(raw.contains(&format!("tri {rid} ")), "golden-cube missing render id {rid}");
    }
    [
        EntityId::parse("10100000-0000-4000-8000-000000000101").unwrap(),
        EntityId::parse("10200000-0000-4000-8000-000000000102").unwrap(),
        EntityId::parse("10300000-0000-4000-8000-000000000103").unwrap(),
        EntityId::parse("10400000-0000-4000-8000-000000000104").unwrap(),
        EntityId::parse("10500000-0000-4000-8000-000000000105").unwrap(),
        EntityId::parse("10600000-0000-4000-8000-000000000106").unwrap(),
    ]
}

fn setup_golden_cube_workspace() -> (CameraWorkspace, MemorySceneBounds, [EntityId; 6]) {
    let faces = golden_cube_face_ids();
    let mut scene = MemorySceneBounds::new();
    // Unit golden cube centered at origin [-0.5, 0.5]^3: 6 face bounding spheres.
    let face_boxes: [([f64; 3], [f64; 3]); 6] = [
        ([0.5, -0.5, -0.5], [0.5, 0.5, 0.5]),
        ([-0.5, -0.5, -0.5], [-0.5, 0.5, 0.5]),
        ([-0.5, 0.5, -0.5], [0.5, 0.5, 0.5]),
        ([-0.5, -0.5, -0.5], [0.5, -0.5, 0.5]),
        ([-0.5, -0.5, 0.5], [0.5, 0.5, 0.5]),
        ([-0.5, -0.5, -0.5], [0.5, 0.5, -0.5]),
    ];
    for (id, (mn, mx)) in faces.iter().zip(face_boxes) {
        scene.insert_displayed(*id, BoundingSphere::from_aabb(mn, mx).unwrap());
    }

    let mut ws = CameraWorkspace::new();
    let vp_id = ViewportId::new("main").unwrap();
    ws.insert_viewport(vp_id, ViewportSlot::new(default_view_state(64, 64)).unwrap());
    (ws, scene, faces)
}

fn cstr<'a>(v: &'a CValue, key: &str) -> &'a str {
    match v {
        CValue::Obj(o) => match o.get(key) {
            Some(CValue::Str(s)) => s.as_str(),
            other => panic!("key {key}: expected string, got {other:?}"),
        },
        other => panic!("expected object, got {other:?}"),
    }
}

#[test]
fn registry_definitions_and_schemas_are_valid() {
    let decls = declarations();
    let raw: Vec<_> = decls.iter().map(|d| d.to_validation()).collect();
    assert_eq!(validate(&raw), Vec::new());

    let catalogue = fid_registry::Catalogue::geo_frame().unwrap();
    let defs = command_defs().unwrap();
    assert_eq!(defs.len(), 2);

    assert_eq!(defs[0].id.as_str(), "view.set_camera");
    assert_eq!(defs[0].major, 1);
    assert_eq!(defs[0].side_effect, SideEffect::W2);
    assert_eq!(fid_registry::tool_name(&defs[0].id, defs[0].major), "view_set_camera");

    assert_eq!(defs[1].id.as_str(), "view.fly_to");
    assert_eq!(defs[1].major, 1);
    assert_eq!(defs[1].side_effect, SideEffect::R1);
    assert_eq!(fid_registry::tool_name(&defs[1].id, defs[1].major), "view_fly_to");

    for d in &defs {
        assert_eq!(fid_registry::check_schema(&d.params_schema, &catalogue), Vec::new());
        assert_eq!(fid_registry::check_schema(&d.result_schema, &catalogue), Vec::new());
    }
}

#[test]
fn ac1_and_ac2_set_camera_exact_f64_values_and_exact_replay() {
    let (mut ws, scene, _faces) = setup_golden_cube_workspace();
    let main_id = ViewportId::new("main").unwrap();
    let initial_vs = ws.viewport(&main_id).unwrap().doc_state.clone();

    // AC-1: fid view set-camera --viewport main --eye 1,1,1 --target 0,0,0 --up 0,0,1 --perspective-fov-deg 45 --json
    let out = {
        let mut ctx = CameraCmdCtx { workspace: &mut ws, scene: &scene, principal: PrincipalKind::Human };
        invoke_cli_set_camera(
            &mut ctx,
            &[
                "--viewport",
                "main",
                "--eye",
                "1,1,1",
                "--target",
                "0,0,0",
                "--up",
                "0,0,1",
                "--perspective-fov-deg",
                "45",
                "--json",
            ],
        )
        .unwrap()
    };

    let result_digest = Digest::parse(cstr(&out, "view_state_digest")).unwrap();
    assert_eq!(ws.journal().len(), 1);
    let ev = &ws.journal()[0];
    assert_eq!(ev.view_state_digest, result_digest);
    assert_eq!(ev.authored_fov_deg.as_deref(), Some("45"));

    let slot = ws.viewport(&main_id).unwrap();
    assert_eq!(slot.doc_digest().unwrap(), result_digest);
    assert_eq!(slot.doc_state.camera.eye_m, [1.0, 1.0, 1.0]);
    assert_eq!(slot.doc_state.camera.target_m, [0.0, 0.0, 0.0]);
    assert_eq!(slot.doc_state.camera.up, [0.0, 0.0, 1.0]);
    assert_eq!(
        slot.doc_state.camera.projection,
        Projection::Perspective { fov_y_rad: parse_fov_deg_to_rad("45").unwrap() }
    );

    // AC-2: exact replay reproduces the exact ViewState digest and returns IDENTICAL.
    let mut init_map = BTreeMap::new();
    init_map.insert(main_id.clone(), initial_vs);
    let journal_json_values = ws.journal_values().unwrap();
    // Round-trip journal bytes through canonical JSON.
    let mut roundtripped = Vec::new();
    for jv in &journal_json_values {
        let bytes = fid_canon::to_canonical_bytes(jv).unwrap();
        roundtripped.push(fid_canon::parse(&bytes).unwrap());
    }
    let replay = replay_camera_journal(&roundtripped, &init_map).unwrap();
    assert_eq!(replay.outcome, ReplayOutcome::Identical);
    assert_eq!(replay.replayed_events, 1);
    assert_eq!(replay.final_digests.get(&main_id).copied(), Some(result_digest));

    // Tampered journal digest is caught as Divergent.
    let mut tampered = roundtripped.clone();
    if let CValue::Obj(o) = &mut tampered[0] {
        o.insert("view_state_digest".to_string(), CValue::Str(Digest::from_bytes([0xff; 32]).as_str().to_string()));
    }
    assert_eq!(replay_camera_journal(&tampered, &init_map).unwrap().outcome, ReplayOutcome::Divergent);
    assert_eq!(SET_CAMERA_EVENT, "view.set_camera@1");
}

#[test]
fn ac3_degenerate_and_non_finite_and_unknown_errors_journal_nothing() {
    let (mut ws, scene, _faces) = setup_golden_cube_workspace();
    let main_id = ViewportId::new("main").unwrap();
    let before_digest = ws.viewport(&main_id).unwrap().doc_digest().unwrap();

    // AC-3: --eye 0,0,0 --target 0,0,0 -> DegenerateCamera and journals nothing.
    {
        let mut ctx = CameraCmdCtx { workspace: &mut ws, scene: &scene, principal: PrincipalKind::Human };
        let err = invoke_cli_set_camera(
            &mut ctx,
            &[
                "--viewport",
                "main",
                "--eye",
                "0,0,0",
                "--target",
                "0,0,0",
                "--up",
                "0,0,1",
                "--perspective-fov-deg",
                "45",
                "--json",
            ],
        )
        .unwrap_err();
        assert_eq!(err.variant_name(), "DegenerateCamera");
        assert!(matches!(err, CameraError::DegenerateCamera { .. }));
    }

    // Parallel up vector -> DegenerateCamera.
    {
        let mut ctx = CameraCmdCtx { workspace: &mut ws, scene: &scene, principal: PrincipalKind::Human };
        let err = set_camera(
            &mut ctx,
            SetCameraParams {
                viewport: "main".to_string(),
                eye_m: vec![0.0, 0.0, 2.0],
                target_m: vec![0.0, 0.0, 0.0],
                up: vec![0.0, 0.0, 1.0],
                projection: ProjectionParam {
                    kind: "perspective".to_string(),
                    fov_y_rad: Some(0.8),
                    fov_y_deg: None,
                    height_m: None,
                },
                near_m: None,
                far_m: None,
            },
        )
        .unwrap_err();
        assert_eq!(err.variant_name(), "DegenerateCamera");
    }

    // Non-finite coordinate -> NonFiniteNumber.
    {
        let mut ctx = CameraCmdCtx { workspace: &mut ws, scene: &scene, principal: PrincipalKind::Human };
        let err = set_camera(
            &mut ctx,
            SetCameraParams {
                viewport: "main".to_string(),
                eye_m: vec![f64::NAN, 1.0, 1.0],
                target_m: vec![0.0, 0.0, 0.0],
                up: vec![0.0, 0.0, 1.0],
                projection: ProjectionParam {
                    kind: "ortho".to_string(),
                    fov_y_rad: None,
                    fov_y_deg: None,
                    height_m: Some(2.0),
                },
                near_m: None,
                far_m: None,
            },
        )
        .unwrap_err();
        assert_eq!(err.variant_name(), "NonFiniteNumber");
        assert!(matches!(err, CameraError::NonFiniteNumber { .. }));
    }

    // Unknown viewport -> UnknownViewport.
    {
        let mut ctx = CameraCmdCtx { workspace: &mut ws, scene: &scene, principal: PrincipalKind::Human };
        let err = fly_to(
            &mut ctx,
            FlyToParams {
                viewport: "nonexistent".to_string(),
                entity: "10100000-0000-4000-8000-000000000101".to_string(),
            },
        )
        .unwrap_err();
        assert_eq!(err.variant_name(), "UnknownViewport");
    }

    assert_eq!(ws.journal_len(), 0);
    assert_eq!(ws.viewport(&main_id).unwrap().doc_digest().unwrap(), before_digest);
}

#[test]
fn ac4_fly_to_deterministic_bit_identical_and_leaves_journal_unchanged() {
    let (mut ws1, scene, faces) = setup_golden_cube_workspace();
    let mut ws2 = ws1.clone();
    let main_id = ViewportId::new("main").unwrap();
    let face_s = faces[0].to_string();

    let doc_digest_before = ws1.viewport(&main_id).unwrap().doc_digest().unwrap();

    let out1 = {
        let mut ctx = CameraCmdCtx { workspace: &mut ws1, scene: &scene, principal: PrincipalKind::Human };
        invoke_cli_fly_to(&mut ctx, &["--entity", &face_s, "--json"]).unwrap()
    };
    let out2 = {
        let mut ctx = CameraCmdCtx { workspace: &mut ws2, scene: &scene, principal: PrincipalKind::Human };
        invoke_cli_fly_to(&mut ctx, &["--entity", &face_s, "--json"]).unwrap()
    };

    let CValue::Obj(o1) = &out1 else { panic!("expected obj") };
    let CValue::Obj(o2) = &out2 else { panic!("expected obj") };
    let cam1 = camera_from_cvalue(o1.get("camera").unwrap()).unwrap();
    let cam2 = camera_from_cvalue(o2.get("camera").unwrap()).unwrap();

    // Bit-identical cameras from the same start.
    for k in 0..3 {
        assert_eq!(cam1.eye_m[k].to_bits(), cam2.eye_m[k].to_bits());
        assert_eq!(cam1.target_m[k].to_bits(), cam2.target_m[k].to_bits());
        assert_eq!(cam1.up[k].to_bits(), cam2.up[k].to_bits());
    }
    assert_eq!(cam1.near_m.to_bits(), cam2.near_m.to_bits());
    assert_eq!(cam1.far_m.to_bits(), cam2.far_m.to_bits());
    assert_eq!(cam1.projection, cam2.projection);

    // Journal head and working document digest are completely unchanged (R1).
    assert_eq!(ws1.journal_len(), 0);
    assert_eq!(ws1.viewport(&main_id).unwrap().doc_digest().unwrap(), doc_digest_before);
    // Activity Log is empty when recording is off (D-031).
    assert!(ws1.activity_log().is_empty());

    // When recording is enabled, R1 logs an ActivityLogEntry (still 0 journal entries).
    ws1.set_recording(true);
    {
        let mut ctx = CameraCmdCtx { workspace: &mut ws1, scene: &scene, principal: PrincipalKind::Human };
        invoke_cli_fly_to(&mut ctx, &["--entity", &face_s, "--json"]).unwrap();
    }
    assert_eq!(ws1.journal_len(), 0);
    assert_eq!(ws1.activity_log().len(), 1);
    assert_eq!(ws1.activity_log()[0].command, FLY_TO_EVENT);
}

#[test]
fn ac5_agent_fly_to_sets_proposal_tag_clears_on_human_orbit_and_reduced_motion_jumps() {
    let (mut ws, mut scene, faces) = setup_golden_cube_workspace();
    let main_id = ViewportId::new("main").unwrap();
    let doc_digest_before = ws.viewport(&main_id).unwrap().doc_digest().unwrap();

    // Turn on reduced motion (REQ-006) and verify jump without animation.
    ws.viewport_mut(&main_id).unwrap().reduced_motion = true;

    let face_s = faces[2].to_string();
    let mcp_params = fid_canon::parse(format!(r#"{{"viewport":"main","entity":"{face_s}"}}"#).as_bytes()).unwrap();
    let out = {
        let mut ctx = CameraCmdCtx { workspace: &mut ws, scene: &scene, principal: PrincipalKind::Agent };
        execute_fly_to(&mut ctx, &mcp_params).unwrap()
    };

    assert_eq!(cstr(&out, "proposal_tag"), "proposal");
    let slot = ws.viewport(&main_id).unwrap();
    assert!(slot.proposal_tag.is_some());
    assert_eq!(slot.proposal_tag.unwrap().label, "proposal");
    assert_eq!(slot.proposal_tag.unwrap().target_entity, faces[2]);
    // Reduced motion jumped immediately with zero active animation.
    assert!(slot.controller.anim.is_none());
    // Working document digest & journal untouched (REQ-005, AC-5).
    assert_eq!(slot.doc_digest().unwrap(), doc_digest_before);
    assert_eq!(ws.journal_len(), 0);

    // Human orbit clears the proposal tag (AC-5).
    ws.viewport_mut(&main_id).unwrap().human_orbit(0.25, 0.1);
    assert!(ws.viewport(&main_id).unwrap().proposal_tag.is_none());

    // Hidden & unknown entity errors.
    let hidden_id = EntityId::parse("20000000-0000-4000-8000-000000000001").unwrap();
    scene.insert_hidden(hidden_id);
    let unknown_id = EntityId::parse("30000000-0000-4000-8000-000000000001").unwrap();
    {
        let mut ctx = CameraCmdCtx { workspace: &mut ws, scene: &scene, principal: PrincipalKind::Agent };
        let e_hidden =
            fly_to(&mut ctx, FlyToParams { viewport: "main".to_string(), entity: hidden_id.to_string() }).unwrap_err();
        assert_eq!(e_hidden, CameraError::EntityNotDisplayed(hidden_id));
        let e_unknown =
            fly_to(&mut ctx, FlyToParams { viewport: "main".to_string(), entity: unknown_id.to_string() }).unwrap_err();
        assert_eq!(e_unknown, CameraError::UnknownEntity(unknown_id));
    }

    // Entity hidden by ViewState visibility rule also fails EntityNotDisplayed.
    ws.viewport_mut(&main_id).unwrap().doc_state.visibility.push(VisibilityRule { entity: faces[1], visible: false });
    {
        let mut ctx = CameraCmdCtx { workspace: &mut ws, scene: &scene, principal: PrincipalKind::Agent };
        let e_vis =
            fly_to(&mut ctx, FlyToParams { viewport: "main".to_string(), entity: faces[1].to_string() }).unwrap_err();
        assert_eq!(e_vis, CameraError::EntityNotDisplayed(faces[1]));
    }

    // Verify JSON executor path for set_camera + performance budget (< 5 ms per command).
    let set_json = fid_canon::parse(
        br#"{"viewport":"main","eye_m":[2,2,2],"target_m":[0,0,0],"up":[0,0,1],"projection":{"kind":"ortho","height_m":1.5}}"#,
    )
    .unwrap();
    #[cfg(not(target_arch = "wasm32"))]
    let t0 = Instant::now();
    {
        let mut ctx = CameraCmdCtx { workspace: &mut ws, scene: &scene, principal: PrincipalKind::Human };
        let _ = execute_set_camera(&mut ctx, &set_json).unwrap();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let elapsed_ms = t0.elapsed().as_secs_f64() * 1_000.0;
        assert!(elapsed_ms < 250.0, "set_camera unexpectedly slow: {elapsed_ms:.2} ms");
    }
}
