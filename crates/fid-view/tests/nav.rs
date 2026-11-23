//! Integration tests for `VIZ.VIEW.CAMERA.NAV` (transient camera navigation, standard
//! orientations, deterministic fit-to-view, reduced-motion transitions and keymap export).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_geo_frame::{AxisAngle, Frame, FrameId, FrameKind, FrameTree, Rigid, Transform};
use fid_view::nav::{
    fit_camera, keybinding_for, std_view_camera, BoundingSphere, CameraController, NavAction, NavDiagnostic, StdView,
    FIT_FILL_RATIO, MAX_ANIM_MS, NAV_KEYMAP,
};
use fid_view::state::{Camera, Projection, ViewState, ViewStateError, Viewport, SCHEMA_VERSION};
use std::collections::BTreeSet;

fn make_view_state(camera: Camera, viewport: Viewport) -> Result<ViewState, ViewStateError> {
    let vs = ViewState {
        schema: SCHEMA_VERSION,
        camera,
        viewport,
        clips: Vec::new(),
        visibility: Vec::new(),
        transparency: Vec::new(),
        colormap: None,
        legend: None,
        labels: None,
    };
    vs.validate()?;
    Ok(vs)
}

fn default_camera() -> Camera {
    Camera {
        eye_m: [3.0, -4.0, 2.5],
        target_m: [0.0, 0.0, 0.0],
        up: [0.0, 0.0, 1.0],
        projection: Projection::Perspective { fov_y_rad: std::f64::consts::FRAC_PI_4 },
        near_m: 0.01,
        far_m: 100.0,
    }
}

fn cube_bounds() -> BoundingSphere {
    // Unit golden-cube centred at origin ([-0.5, 0.5]^3).
    BoundingSphere::from_aabb([-0.5, -0.5, -0.5], [0.5, 0.5, 0.5]).expect("cube bounds")
}

fn rotated_frame_tree() -> (FrameTree, FrameId, Transform) {
    let mut tree = FrameTree::new();
    let axis = [1.0 / 3.0_f64.sqrt(), 1.0 / 3.0_f64.sqrt(), 1.0 / 3.0_f64.sqrt()];
    let angle = 0.7_f64;
    let rot = AxisAngle { axis, angle }.to_matrix().expect("axis-angle");
    let rigid = Rigid::new(rot, [0.2, -0.3, 0.5]).expect("rigid");
    let xform = Transform::Rigid(rigid);
    let fid = FrameId::parse("00000000000000000000000000000001").expect("frame id");
    let frame = Frame {
        id: fid,
        name: "Rotated Part Frame".to_owned(),
        kind: FrameKind::Part,
        parent: Some(tree.world()),
        to_parent: xform,
    };
    tree.insert(frame).expect("insert frame");
    (tree, fid, xform)
}

fn unit(v: [f64; 3]) -> [f64; 3] {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / n, v[1] / n, v[2] / n]
}

fn dist3(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// AC-1 & REQ-002: with a rotated active frame, applying `"top"` places the camera direction
/// along the frame's `-Z` axis within `1e-12` and `up` along the frame's `+Y` axis within `1e-12`.
/// Also verifies all 7 standard views and `NoActiveFrame` fallback to world.
#[test]
fn ac1_rotated_frame_top_and_all_std_views() {
    let (tree, active_id, xform) = rotated_frame_tree();
    let vp = Viewport { width_px: 1280, height_px: 720 };
    let bounds = cube_bounds();
    let cam = default_camera();

    let (top_cam, diag) = std_view_camera(&cam, StdView::Top, &tree, Some(active_id), Some(bounds), vp);
    assert_eq!(diag, None);

    let view_dir = unit([
        top_cam.target_m[0] - top_cam.eye_m[0],
        top_cam.target_m[1] - top_cam.eye_m[1],
        top_cam.target_m[2] - top_cam.eye_m[2],
    ]);
    let expected_dir = xform.apply_dir(&[0.0, 0.0, -1.0]).unwrap();
    let expected_up = xform.apply_dir(&[0.0, 1.0, 0.0]).unwrap();

    assert!(dist3(view_dir, expected_dir) < 1e-12, "top view_dir {view_dir:?} != frame -Z {expected_dir:?}");
    assert!(dist3(unit(top_cam.up), expected_up) < 1e-12, "top up {:?} != frame +Y {expected_up:?}", top_cam.up);

    // Every standard orientation produces a valid ViewState camera aligned to the active frame.
    for sv in StdView::ALL {
        let (c, d) = std_view_camera(&cam, sv, &tree, Some(active_id), Some(bounds), vp);
        assert_eq!(d, None);
        let vs = make_view_state(c, vp).expect("std view must produce valid ViewState");
        assert!(!vs.to_canonical_bytes().unwrap().is_empty());
    }

    // Absent active frame falls back to world frame with NavDiagnostic::NoActiveFrame.
    let (world_top, diag_none) = std_view_camera(&cam, StdView::Top, &tree, None, Some(bounds), vp);
    assert_eq!(diag_none, Some(NavDiagnostic::NoActiveFrame));
    let world_dir = unit([
        world_top.target_m[0] - world_top.eye_m[0],
        world_top.target_m[1] - world_top.eye_m[1],
        world_top.target_m[2] - world_top.eye_m[2],
    ]);
    assert!(dist3(world_dir, [0.0, 0.0, -1.0]) < 1e-12);
    assert!(dist3(unit(world_top.up), [0.0, 1.0, 0.0]) < 1e-12);
}

/// AC-2 & REQ-001: a scripted 5 s orbit (300 frames at 60 Hz) updates only the transient live
/// camera (maintaining valid ViewState and constant distance to pivot) and touches no journal.
#[test]
fn ac2_scripted_5s_orbit_is_pure_transient_and_stable() {
    let vp = Viewport { width_px: 1024, height_px: 768 };
    let initial = default_camera();
    let initial_dist = dist3(initial.eye_m, initial.target_m);
    let mut ctrl = CameraController::new(initial);

    // 5 seconds at 60 Hz = 300 orbit steps plus pan, zoom and roll checks.
    for _ in 0..300 {
        ctrl.orbit(0.02, 0.005);
    }
    let after_dist = dist3(ctrl.live.eye_m, ctrl.pivot_m);
    assert!((after_dist - initial_dist).abs() < 1e-9, "orbit changed pivot radius");
    assert!(make_view_state(ctrl.live, vp).is_ok());

    ctrl.pan(15.0, -10.0, vp);
    ctrl.zoom(1.25, Some([0.2, -0.1]), vp);
    ctrl.roll(0.3);
    assert!(make_view_state(ctrl.live, vp).is_ok());

    // Non-finite gesture inputs are ignored safely without corrupting the camera.
    let snap = ctrl.live;
    ctrl.orbit(f64::NAN, 0.1);
    ctrl.pan(f64::INFINITY, 0.0, vp);
    ctrl.zoom(-1.0, None, vp);
    ctrl.roll(f64::NAN);
    assert_eq!(ctrl.live, snap);
}

/// AC-3 & REQ-004: with `reduced_motion = true`, applying `"isometric"` jumps to the exact end
/// camera on the very next frame (`step`); with `reduced_motion = false`, it animates cleanly
/// in at most 250 ms.
#[test]
fn ac3_reduced_motion_jumps_in_one_frame_and_normal_animates_within_250ms() {
    let (tree, active_id, _) = rotated_frame_tree();
    let vp = Viewport { width_px: 1280, height_px: 720 };
    let bounds = cube_bounds();

    // Reduced motion ON: jumps in 1 frame.
    let mut ctrl_rm = CameraController::new(default_camera());
    let (end_rm, _) = ctrl_rm.apply_std_view(StdView::Isometric, &tree, Some(active_id), Some(bounds), vp, true);
    assert!(ctrl_rm.step(16.0));
    assert_eq!(ctrl_rm.live, end_rm);
    assert!(ctrl_rm.anim.is_none());

    // Reduced motion OFF: animates smoothly and completes within MAX_ANIM_MS (250 ms).
    let mut ctrl_anim = CameraController::new(default_camera());
    let (end_anim, _) = ctrl_anim.apply_std_view(StdView::Isometric, &tree, Some(active_id), Some(bounds), vp, false);
    assert_eq!(end_anim, end_rm);
    let anim = ctrl_anim.anim.expect("animation started");
    assert!(anim.duration_ms > 0.0 && anim.duration_ms <= f64::from(MAX_ANIM_MS));

    // Mid-way frame is distinct from start and end.
    assert!(ctrl_anim.step(50.0));
    assert!(ctrl_anim.anim.is_some());
    assert_ne!(ctrl_anim.live.eye_m, end_anim.eye_m);

    // After reaching MAX_ANIM_MS total, camera equals exact end camera.
    assert!(ctrl_anim.step(f64::from(MAX_ANIM_MS)));
    assert_eq!(ctrl_anim.live, end_anim);
    assert!(ctrl_anim.anim.is_none());
}

/// AC-4 & REQ-003, REQ-006: zoom-to-selection of two features is bit-identical across runs,
/// and frames the bounding sphere to fill 90 % of the shorter viewport side for both
/// perspective and orthographic projections in landscape and portrait viewports.
#[test]
fn ac4_zoom_to_selection_is_bit_identical_and_fills_90_percent() {
    let feat_a = BoundingSphere::from_aabb([0.0, 0.0, 0.0], [0.2, 0.1, 0.1]).unwrap();
    let feat_b = BoundingSphere::from_aabb([0.5, 0.2, -0.1], [0.7, 0.4, 0.2]).unwrap();
    let selection = feat_a.union(feat_b);

    for vp in [Viewport { width_px: 1600, height_px: 900 }, Viewport { width_px: 600, height_px: 1000 }] {
        // Perspective fit: two calls are bit-identical.
        let persp_start = default_camera();
        let c1 = fit_camera(&persp_start, selection, vp);
        let c2 = fit_camera(&persp_start, selection, vp);
        assert_eq!(
            make_view_state(c1, vp).unwrap().to_canonical_bytes().unwrap(),
            make_view_state(c2, vp).unwrap().to_canonical_bytes().unwrap()
        );
        for k in 0..3 {
            assert_eq!(c1.eye_m[k].to_bits(), c2.eye_m[k].to_bits());
            assert_eq!(c1.target_m[k].to_bits(), c2.target_m[k].to_bits());
            assert_eq!(c1.up[k].to_bits(), c2.up[k].to_bits());
        }

        // Verify projected sphere fills 90 % of the shorter side in perspective.
        let Projection::Perspective { fov_y_rad } = c1.projection else {
            panic!("expected perspective");
        };
        let d = dist3(c1.eye_m, c1.target_m);
        let aspect = f64::from(vp.width_px) / f64::from(vp.height_px);
        let tan_half_min = (0.5 * fov_y_rad).tan() * aspect.min(1.0);
        let sin_theta = selection.radius_m / d;
        let tan_theta = sin_theta / (1.0 - sin_theta * sin_theta).sqrt();
        let fill = tan_theta / tan_half_min;
        assert!((fill - FIT_FILL_RATIO).abs() < 1e-12, "perspective fill {fill} != {FIT_FILL_RATIO}");

        // Orthographic fit: fills 90 % of shorter side.
        let mut ortho_start = default_camera();
        ortho_start.projection = Projection::Ortho { height_m: 10.0 };
        let o1 = fit_camera(&ortho_start, selection, vp);
        let o2 = fit_camera(&ortho_start, selection, vp);
        assert_eq!(o1, o2);
        let Projection::Ortho { height_m } = o1.projection else {
            panic!("expected ortho");
        };
        let short_side = height_m * aspect.min(1.0);
        let ortho_fill = (2.0 * selection.radius_m) / short_side;
        assert!((ortho_fill - FIT_FILL_RATIO).abs() < 1e-12, "ortho fill {ortho_fill} != {FIT_FILL_RATIO}");
    }
}

/// AC-5 & REQ-005: every navigation action has a unique palette ID and keyboard binding in
/// `NAV_KEYMAP`.
#[test]
fn ac5_every_navigation_action_has_a_keymap_binding() {
    let mut palette_ids = BTreeSet::new();
    let mut chords = BTreeSet::new();

    for action in NavAction::ALL {
        let b = keybinding_for(action).unwrap_or_else(|| panic!("missing keybinding for {action:?}"));
        assert!(!b.palette_id.is_empty());
        assert!(!b.label.is_empty());
        assert!(!b.key_chord.is_empty());
        assert!(palette_ids.insert(b.palette_id), "duplicate palette_id {}", b.palette_id);
        assert!(chords.insert(b.key_chord), "duplicate key_chord {}", b.key_chord);
    }
    assert_eq!(NAV_KEYMAP.len(), NavAction::ALL.len());
}
