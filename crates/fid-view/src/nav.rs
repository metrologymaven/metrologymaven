//! Transient camera navigation, standard orientations and fit (spec leaf `VIZ.VIEW.CAMERA.NAV`).
//!
//! Pure presentation state (`R0`, `D0` for end cameras): updates only the viewport's live
//! [`Camera`] and never writes to the journal or Activity Log (REQ-VIZ.VIEW.CAMERA.NAV-001).
//!
//! - Gestures: [`CameraController::orbit`], [`CameraController::pan`],
//!   [`CameraController::zoom`] and [`CameraController::roll`] operate around `pivot_m`.
//! - Standard orientations ([`StdView`]): front, back, top, bottom, left, right and isometric
//!   relative to the active kernel frame from [`FrameTree`] (`GEO.FRAME.TREE`); falls back to
//!   the world frame with [`NavDiagnostic::NoActiveFrame`] when no active frame is present.
//! - Fit-to-view and zoom-to-selection ([`fit_camera`], [`CameraController::fit_bounds`]):
//!   frame an `f64` [`BoundingSphere`] so it fills 90 % of the shorter viewport side.
//! - Animations: cubic ease-out up to [`MAX_ANIM_MS`] (250 ms); when `reduced_motion` is `true`,
//!   orientation changes and fits jump to the end camera on the next frame with no intermediate
//!   frames (REQ-VIZ.VIEW.CAMERA.NAV-004).
//! - Keyboard palette actions: every [`NavAction`] has a registered default key chord in
//!   [`NAV_KEYMAP`] (REQ-VIZ.VIEW.CAMERA.NAV-005).

use crate::state::{Camera, Projection, Viewport};
use fid_geo_frame::{FrameId, FrameTree, Transform};

/// Default animation duration in milliseconds (at most [`MAX_ANIM_MS`]).
pub const DEFAULT_ANIM_MS: u32 = 200;
/// Maximum allowed animation duration in milliseconds (REQ-VIZ.VIEW.CAMERA.NAV-004).
pub const MAX_ANIM_MS: u32 = 250;
/// Fraction of the shorter viewport side filled by a fitted bounding sphere (REQ-003).
pub const FIT_FILL_RATIO: f64 = 0.90;
/// Minimum bounding-sphere radius (1 mm) when fitting a single point or degenerate bounds.
pub const MIN_FIT_RADIUS_M: f64 = 1.0e-3;
/// Minimum orbit/zoom eye-to-target distance in metres.
const MIN_EYE_DIST_M: f64 = 1.0e-6;
/// Minimum vector norm treated as non-degenerate.
const EPS_LEN: f64 = 1.0e-15;

/// The seven standard orientations relative to the active coordinate frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StdView {
    Front,
    Back,
    Top,
    Bottom,
    Left,
    Right,
    Isometric,
}

impl StdView {
    /// Every standard orientation in canonical order.
    pub const ALL: [StdView; 7] = [
        StdView::Front,
        StdView::Back,
        StdView::Top,
        StdView::Bottom,
        StdView::Left,
        StdView::Right,
        StdView::Isometric,
    ];

    /// Stable snake_case identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            StdView::Front => "front",
            StdView::Back => "back",
            StdView::Top => "top",
            StdView::Bottom => "bottom",
            StdView::Left => "left",
            StdView::Right => "right",
            StdView::Isometric => "isometric",
        }
    }

    /// Unit view direction (`normalize(target - eye)`) and reference `up` vector in the active
    /// frame's local coordinates (REQ-VIZ.VIEW.CAMERA.NAV-002).
    ///
    /// Engineering right-handed Z-up convention:
    /// - `Top`: looking along `-Z`, up `+Y` (AC-1);
    /// - `Bottom`: looking along `+Z`, up `+Y`;
    /// - `Front`: looking along `+Y`, up `+Z`;
    /// - `Back`: looking along `-Y`, up `+Z`;
    /// - `Right`: looking along `-X`, up `+Z`;
    /// - `Left`: looking along `+X`, up `+Z`;
    /// - `Isometric`: looking along `(-1, -1, -1) / sqrt(3)` (the `(1, 1, 1)` diagonal), up `+Z`.
    pub fn local_dir_and_up(self) -> ([f64; 3], [f64; 3]) {
        match self {
            StdView::Front => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            StdView::Back => ([0.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
            StdView::Top => ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
            StdView::Bottom => ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            StdView::Left => ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            StdView::Right => ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            StdView::Isometric => {
                let inv = 1.0 / 3.0_f64.sqrt();
                ([-inv, -inv, -inv], [0.0, 0.0, 1.0])
            }
        }
    }
}

/// Non-fatal diagnostic returned when a navigation operation falls back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavDiagnostic {
    /// No active frame was supplied or found in the frame tree; fell back to the world frame.
    NoActiveFrame,
}

/// An `f64` bounding sphere in world metres, used by fit-to-view and zoom-to-selection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingSphere {
    pub center_m: [f64; 3],
    pub radius_m: f64,
}

impl BoundingSphere {
    /// Builds a bounding sphere from an axis-aligned bounding box `[min_m, max_m]`.
    /// Returns `None` if any coordinate is non-finite or `min > max` on any axis.
    pub fn from_aabb(min_m: [f64; 3], max_m: [f64; 3]) -> Option<Self> {
        if !vec_finite(min_m) || !vec_finite(max_m) {
            return None;
        }
        if min_m[0] > max_m[0] || min_m[1] > max_m[1] || min_m[2] > max_m[2] {
            return None;
        }
        let center_m = [0.5 * (min_m[0] + max_m[0]), 0.5 * (min_m[1] + max_m[1]), 0.5 * (min_m[2] + max_m[2])];
        let half = [0.5 * (max_m[0] - min_m[0]), 0.5 * (max_m[1] - min_m[1]), 0.5 * (max_m[2] - min_m[2])];
        let radius_m = vec_norm(half);
        if !radius_m.is_finite() {
            return None;
        }
        Some(Self { center_m, radius_m })
    }

    /// Builds a deterministic bounding sphere enclosing a non-empty slice of 3D points.
    /// Returns `None` if `points` is empty or contains any non-finite coordinate.
    pub fn from_points(points: &[[f64; 3]]) -> Option<Self> {
        let first = *points.first()?;
        if !vec_finite(first) {
            return None;
        }
        let mut min_m = first;
        let mut max_m = first;
        for &p in points.iter().skip(1) {
            if !vec_finite(p) {
                return None;
            }
            for k in 0..3 {
                if p[k] < min_m[k] {
                    min_m[k] = p[k];
                }
                if p[k] > max_m[k] {
                    max_m[k] = p[k];
                }
            }
        }
        let center_m = [0.5 * (min_m[0] + max_m[0]), 0.5 * (min_m[1] + max_m[1]), 0.5 * (min_m[2] + max_m[2])];
        let mut max_r2 = 0.0_f64;
        for &p in points {
            let d = vec_sub(p, center_m);
            let r2 = vec_dot(d, d);
            if r2 > max_r2 {
                max_r2 = r2;
            }
        }
        Some(Self { center_m, radius_m: max_r2.sqrt() })
    }

    /// Combines two bounding spheres into a deterministic enclosing sphere.
    pub fn union(self, other: Self) -> Self {
        let diff = vec_sub(other.center_m, self.center_m);
        let dist = vec_norm(diff);
        if dist + other.radius_m <= self.radius_m {
            return self;
        }
        if dist + self.radius_m <= other.radius_m {
            return other;
        }
        let new_r = 0.5 * (dist + self.radius_m + other.radius_m);
        let t = if dist > EPS_LEN { (new_r - self.radius_m) / dist } else { 0.0 };
        Self { center_m: vec_add(self.center_m, vec_scale(diff, t)), radius_m: new_r }
    }
}

/// Active transition animation between two cameras.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraAnim {
    pub start: Camera,
    pub end: Camera,
    pub elapsed_ms: f64,
    pub duration_ms: f64,
}

impl CameraAnim {
    /// Cubic ease-out `1 - (1 - t)^3` clamped to `[0, 1]`.
    pub fn ease_cubic(t: f64) -> f64 {
        let u = t.clamp(0.0, 1.0);
        let inv = 1.0 - u;
        1.0 - inv * inv * inv
    }

    /// Evaluates the interpolated camera at `elapsed_ms`.
    fn sample(&self) -> Camera {
        if self.duration_ms <= 0.0 || self.elapsed_ms >= self.duration_ms {
            return self.end;
        }
        let s = Self::ease_cubic(self.elapsed_ms / self.duration_ms);
        let eye_m = vec_lerp(self.start.eye_m, self.end.eye_m, s);
        let target_m = vec_lerp(self.start.target_m, self.end.target_m, s);
        let view = vec_norm_or(vec_sub(target_m, eye_m), [0.0, 0.0, -1.0]);
        let raw_up = vec_lerp(self.start.up, self.end.up, s);
        let up = orthonormal_up(view, raw_up, self.end.up);
        let projection = match (self.start.projection, self.end.projection) {
            (Projection::Perspective { fov_y_rad: a }, Projection::Perspective { fov_y_rad: b }) => {
                Projection::Perspective { fov_y_rad: a + (b - a) * s }
            }
            (Projection::Ortho { height_m: a }, Projection::Ortho { height_m: b }) => {
                Projection::Ortho { height_m: a + (b - a) * s }
            }
            (_, end_p) => end_p,
        };
        let near_m = self.start.near_m + (self.end.near_m - self.start.near_m) * s;
        let far_m = self.start.far_m + (self.end.far_m - self.start.far_m) * s;
        Camera { eye_m, target_m, up, projection, near_m, far_m }
    }
}

/// Live viewport camera controller (transient presentation state; never journals).
#[derive(Debug, Clone, PartialEq)]
pub struct CameraController {
    /// Current live camera.
    pub live: Camera,
    /// Orbit and zoom pivot in world metres.
    pub pivot_m: [f64; 3],
    /// Ongoing eased transition, if any.
    pub anim: Option<CameraAnim>,
}

impl CameraController {
    /// Creates a controller centred on `initial.target_m`.
    pub fn new(initial: Camera) -> Self {
        Self { live: initial, pivot_m: initial.target_m, anim: None }
    }

    /// Sets the orbit/zoom pivot point (e.g. a CPU-picked world point or scene centre).
    pub fn set_pivot(&mut self, pivot_m: [f64; 3]) {
        if vec_finite(pivot_m) {
            self.pivot_m = pivot_m;
        }
    }

    /// Cancels any active animation and returns `true` if one was running.
    pub fn cancel_animation(&mut self) -> bool {
        self.anim.take().is_some()
    }

    /// Advances an active transition by `dt_ms` milliseconds.
    ///
    /// Under `reduced_motion`, an animation has `duration_ms == 0.0`, so the very first `step`
    /// jumps straight to `end` and clears `anim` (AC-3).
    pub fn step(&mut self, dt_ms: f64) -> bool {
        let Some(mut a) = self.anim else {
            return false;
        };
        if !dt_ms.is_finite() || dt_ms < 0.0 {
            return false;
        }
        if a.duration_ms <= 0.0 {
            self.live = a.end;
            self.anim = None;
            return true;
        }
        a.elapsed_ms += dt_ms;
        if a.elapsed_ms >= a.duration_ms {
            self.live = a.end;
            self.anim = None;
        } else {
            self.live = a.sample();
            self.anim = Some(a);
        }
        true
    }

    /// Arcball / spherical orbit about `self.pivot_m`: `yaw_rad` rotates around the camera's up
    /// axis, `pitch_rad` rotates around the camera's right axis. Updates only `self.live`.
    pub fn orbit(&mut self, yaw_rad: f64, pitch_rad: f64) {
        if !yaw_rad.is_finite() || !pitch_rad.is_finite() {
            return;
        }
        self.anim = None;
        let (right, up, _fwd) = camera_basis(&self.live);
        let offset_eye = vec_sub(self.live.eye_m, self.pivot_m);
        let offset_target = vec_sub(self.live.target_m, self.pivot_m);

        // Rotate first by yaw around `up`, then by pitch around the yawed `right`.
        let eye1 = rotate_around_axis(offset_eye, up, -yaw_rad);
        let target1 = rotate_around_axis(offset_target, up, -yaw_rad);
        let right1 = vec_norm_or(rotate_around_axis(right, up, -yaw_rad), right);

        let eye2 = rotate_around_axis(eye1, right1, -pitch_rad);
        let target2 = rotate_around_axis(target1, right1, -pitch_rad);
        let up2 = vec_norm_or(rotate_around_axis(up, right1, -pitch_rad), up);

        self.live.eye_m = vec_add(self.pivot_m, eye2);
        self.live.target_m = vec_add(self.pivot_m, target2);
        let fwd2 = vec_norm_or(vec_sub(self.live.target_m, self.live.eye_m), [0.0, 0.0, -1.0]);
        self.live.up = orthonormal_up(fwd2, up2, up);
    }

    /// Pans the camera in the view plane by `(dx_px, dy_px)` pixels, scaled by the world depth
    /// at `self.pivot_m` (or orthographic height). Updates only `self.live` and `self.pivot_m`.
    pub fn pan(&mut self, dx_px: f64, dy_px: f64, viewport: Viewport) {
        if !dx_px.is_finite() || !dy_px.is_finite() || viewport.height_px == 0 {
            return;
        }
        self.anim = None;
        let (right, up, fwd) = camera_basis(&self.live);
        let world_height = match self.live.projection {
            Projection::Ortho { height_m } => height_m,
            Projection::Perspective { fov_y_rad } => {
                let depth = vec_dot(vec_sub(self.pivot_m, self.live.eye_m), fwd)
                    .abs()
                    .max(vec_norm(vec_sub(self.live.target_m, self.live.eye_m)))
                    .max(MIN_EYE_DIST_M);
                2.0 * depth * (0.5 * fov_y_rad).tan()
            }
        };
        let m_per_px = world_height / f64::from(viewport.height_px);
        let delta = vec_add(vec_scale(right, -dx_px * m_per_px), vec_scale(up, dy_px * m_per_px));
        self.live.eye_m = vec_add(self.live.eye_m, delta);
        self.live.target_m = vec_add(self.live.target_m, delta);
        self.pivot_m = vec_add(self.pivot_m, delta);
    }

    /// Zooms by positive `factor` (`> 1.0` zooms in, `< 1.0` zooms out), optionally pulling
    /// toward `cursor_ndc` in `[-1, 1]^2`. Updates only `self.live`.
    pub fn zoom(&mut self, factor: f64, cursor_ndc: Option<[f64; 2]>, viewport: Viewport) {
        if !factor.is_finite() || factor <= 0.0 || viewport.height_px == 0 {
            return;
        }
        self.anim = None;
        let scale = 1.0 / factor;
        let (right, up, fwd) = camera_basis(&self.live);
        let aspect = f64::from(viewport.width_px) / f64::from(viewport.height_px);

        match self.live.projection {
            Projection::Ortho { height_m } => {
                let new_h = (height_m * scale).max(MIN_EYE_DIST_M);
                if let Some([nx, ny]) = cursor_ndc {
                    if nx.is_finite() && ny.is_finite() {
                        let dh = height_m - new_h;
                        let shift = vec_add(vec_scale(right, 0.5 * nx * aspect * dh), vec_scale(up, 0.5 * ny * dh));
                        self.live.eye_m = vec_add(self.live.eye_m, shift);
                        self.live.target_m = vec_add(self.live.target_m, shift);
                        self.pivot_m = vec_add(self.pivot_m, shift);
                    }
                }
                self.live.projection = Projection::Ortho { height_m: new_h };
            }
            Projection::Perspective { fov_y_rad } => {
                let cur_off = vec_sub(self.live.eye_m, self.pivot_m);
                let cur_dist = vec_norm(cur_off).max(MIN_EYE_DIST_M);
                let new_dist = (cur_dist * scale).max(MIN_EYE_DIST_M);
                let new_eye = vec_add(self.pivot_m, vec_scale(vec_norm_or(cur_off, vec_scale(fwd, -1.0)), new_dist));
                self.live.eye_m = new_eye;
                if let Some([nx, ny]) = cursor_ndc {
                    if nx.is_finite() && ny.is_finite() {
                        let half_h = (cur_dist - new_dist) * (0.5 * fov_y_rad).tan();
                        let shift = vec_add(vec_scale(right, nx * aspect * half_h), vec_scale(up, ny * half_h));
                        self.live.eye_m = vec_add(self.live.eye_m, shift);
                        self.live.target_m = vec_add(self.live.target_m, shift);
                        self.pivot_m = vec_add(self.pivot_m, shift);
                    }
                }
            }
        }
    }

    /// Rolls the camera counter-clockwise by `angle_rad` around its view direction.
    pub fn roll(&mut self, angle_rad: f64) {
        if !angle_rad.is_finite() {
            return;
        }
        self.anim = None;
        let (_right, up, fwd) = camera_basis(&self.live);
        let rolled = rotate_around_axis(up, fwd, angle_rad);
        self.live.up = orthonormal_up(fwd, rolled, up);
    }

    /// Transitions to a standard orientation relative to `active_frame` in `tree` (falling back
    /// to world frame with [`NavDiagnostic::NoActiveFrame`] when absent), framing `bounds` if
    /// supplied or keeping the current distance about `pivot_m`.
    ///
    /// When `reduced_motion` is `true`, sets a zero-duration transition so the camera jumps to
    /// the exact end camera on the very next frame (`step`), with no intermediate frames (AC-3).
    pub fn apply_std_view(
        &mut self,
        view: StdView,
        tree: &FrameTree,
        active_frame: Option<FrameId>,
        bounds: Option<BoundingSphere>,
        viewport: Viewport,
        reduced_motion: bool,
    ) -> (Camera, Option<NavDiagnostic>) {
        let (end, diag) = std_view_camera(&self.live, view, tree, active_frame, bounds, viewport);
        self.pivot_m = end.target_m;
        let duration_ms = if reduced_motion { 0.0 } else { f64::from(DEFAULT_ANIM_MS.min(MAX_ANIM_MS)) };
        self.anim = Some(CameraAnim { start: self.live, end, elapsed_ms: 0.0, duration_ms });
        (end, diag)
    }

    /// Transitions to fit `bounds` so its bounding sphere fills 90 % of the shorter viewport
    /// side along the current view direction (REQ-VIZ.VIEW.CAMERA.NAV-003, -006).
    pub fn fit_bounds(&mut self, bounds: BoundingSphere, viewport: Viewport, reduced_motion: bool) -> Camera {
        let end = fit_camera(&self.live, bounds, viewport);
        self.pivot_m = end.target_m;
        let duration_ms = if reduced_motion { 0.0 } else { f64::from(DEFAULT_ANIM_MS.min(MAX_ANIM_MS)) };
        self.anim = Some(CameraAnim { start: self.live, end, elapsed_ms: 0.0, duration_ms });
        end
    }
}

/// Computes the deterministic `f64` end camera for `view` relative to `active_frame` in `tree`
/// (REQ-VIZ.VIEW.CAMERA.NAV-002, -006).
///
/// Falls back to the world frame and returns `Some(NavDiagnostic::NoActiveFrame)` if
/// `active_frame` is `None` or not present in `tree`.
pub fn std_view_camera(
    current: &Camera,
    view: StdView,
    tree: &FrameTree,
    active_frame: Option<FrameId>,
    bounds: Option<BoundingSphere>,
    viewport: Viewport,
) -> (Camera, Option<NavDiagnostic>) {
    let (frame_to_world, diag) = match active_frame {
        Some(fid) => match tree.resolve(fid, tree.world()) {
            Ok(xform) => (xform, None),
            Err(_) => (Transform::IDENTITY, Some(NavDiagnostic::NoActiveFrame)),
        },
        None => (Transform::IDENTITY, Some(NavDiagnostic::NoActiveFrame)),
    };

    let (local_dir, local_up) = view.local_dir_and_up();
    let world_dir = frame_to_world.apply_dir(&local_dir).map(|d| vec_norm_or(d, local_dir)).unwrap_or(local_dir);
    let world_up = frame_to_world.apply_dir(&local_up).map(|u| vec_norm_or(u, local_up)).unwrap_or(local_up);
    let up = orthonormal_up(world_dir, world_up, [0.0, 0.0, 1.0]);

    let cur_dist = vec_norm(vec_sub(current.target_m, current.eye_m)).max(MIN_EYE_DIST_M);
    let oriented = Camera {
        eye_m: vec_sub(current.target_m, vec_scale(world_dir, cur_dist)),
        target_m: current.target_m,
        up,
        projection: current.projection,
        near_m: current.near_m,
        far_m: current.far_m,
    };

    let end = match bounds {
        Some(b) => fit_camera(&oriented, b, viewport),
        None => oriented,
    };
    (end, diag)
}

/// Computes the deterministic `f64` end camera that frames `bounds` along `current`'s view
/// direction so the bounding sphere fills [`FIT_FILL_RATIO`] (90 %) of the shorter viewport
/// side (REQ-VIZ.VIEW.CAMERA.NAV-003, -006).
pub fn fit_camera(current: &Camera, bounds: BoundingSphere, viewport: Viewport) -> Camera {
    let r = if bounds.radius_m.is_finite() && bounds.radius_m > MIN_FIT_RADIUS_M {
        bounds.radius_m
    } else {
        MIN_FIT_RADIUS_M
    };
    let center = if vec_finite(bounds.center_m) { bounds.center_m } else { current.target_m };
    let (_right, up, fwd) = camera_basis(current);

    let aspect = if viewport.height_px > 0 && viewport.width_px > 0 {
        f64::from(viewport.width_px) / f64::from(viewport.height_px)
    } else {
        1.0
    };
    let min_aspect = aspect.min(1.0);

    match current.projection {
        Projection::Ortho { .. } => {
            // Shorter side S = height_m * min(aspect, 1.0); require 2*r / S = FIT_FILL_RATIO.
            let short_side_m = (2.0 * r) / FIT_FILL_RATIO;
            let height_m = short_side_m / min_aspect;
            let dist = (3.0 * r).max(MIN_EYE_DIST_M);
            let near_m = (dist - 2.0 * r).max(MIN_EYE_DIST_M);
            let far_m = (dist + 4.0 * r).max(near_m * 100.0);
            Camera {
                eye_m: vec_sub(center, vec_scale(fwd, dist)),
                target_m: center,
                up,
                projection: Projection::Ortho { height_m },
                near_m,
                far_m,
            }
        }
        Projection::Perspective { fov_y_rad } => {
            // Exact apparent-radius fit: at NDC = FIT_FILL_RATIO along the shorter side, the
            // ray tangent is `tan_eff = FIT_FILL_RATIO * tan(half_fov_min)`. The bounding sphere
            // of radius `r` is tangent to that cone at distance `d = r / sin(atan(tan_eff))`.
            let tan_half_y = (0.5 * fov_y_rad).tan();
            let tan_half_min = (tan_half_y * min_aspect).max(1.0e-6);
            let tan_eff = FIT_FILL_RATIO * tan_half_min;
            let sin_eff = tan_eff / (1.0 + tan_eff * tan_eff).sqrt();
            let dist = (r / sin_eff).max(r + MIN_EYE_DIST_M);
            let near_m = (dist - 1.5 * r).max(dist * 1.0e-3).max(MIN_EYE_DIST_M);
            let far_m = (dist + 4.0 * r).max(near_m * 100.0);
            Camera {
                eye_m: vec_sub(center, vec_scale(fwd, dist)),
                target_m: center,
                up,
                projection: Projection::Perspective { fov_y_rad },
                near_m,
                far_m,
            }
        }
    }
}

// ------------------------------------------------------------------ keyboard palette bindings

/// Every navigation action that requires a keyboard equivalent (REQ-VIZ.VIEW.CAMERA.NAV-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NavAction {
    OrbitLeft,
    OrbitRight,
    OrbitUp,
    OrbitDown,
    PanLeft,
    PanRight,
    PanUp,
    PanDown,
    ZoomIn,
    ZoomOut,
    RollLeft,
    RollRight,
    ViewFront,
    ViewBack,
    ViewTop,
    ViewBottom,
    ViewLeft,
    ViewRight,
    ViewIsometric,
    FitToView,
    ZoomToSelection,
}

impl NavAction {
    /// Complete list of navigation actions.
    pub const ALL: [NavAction; 21] = [
        NavAction::OrbitLeft,
        NavAction::OrbitRight,
        NavAction::OrbitUp,
        NavAction::OrbitDown,
        NavAction::PanLeft,
        NavAction::PanRight,
        NavAction::PanUp,
        NavAction::PanDown,
        NavAction::ZoomIn,
        NavAction::ZoomOut,
        NavAction::RollLeft,
        NavAction::RollRight,
        NavAction::ViewFront,
        NavAction::ViewBack,
        NavAction::ViewTop,
        NavAction::ViewBottom,
        NavAction::ViewLeft,
        NavAction::ViewRight,
        NavAction::ViewIsometric,
        NavAction::FitToView,
        NavAction::ZoomToSelection,
    ];
}

/// Palette action and default keyboard shortcut for one [`NavAction`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavKeyBinding {
    pub action: NavAction,
    pub palette_id: &'static str,
    pub label: &'static str,
    pub key_chord: &'static str,
}

/// Default keymap export for all navigation actions (REQ-VIZ.VIEW.CAMERA.NAV-005, AC-5).
pub const NAV_KEYMAP: &[NavKeyBinding] = &[
    NavKeyBinding {
        action: NavAction::OrbitLeft,
        palette_id: "view.nav.orbit_left",
        label: "Orbit Left",
        key_chord: "ArrowLeft",
    },
    NavKeyBinding {
        action: NavAction::OrbitRight,
        palette_id: "view.nav.orbit_right",
        label: "Orbit Right",
        key_chord: "ArrowRight",
    },
    NavKeyBinding {
        action: NavAction::OrbitUp,
        palette_id: "view.nav.orbit_up",
        label: "Orbit Up",
        key_chord: "ArrowUp",
    },
    NavKeyBinding {
        action: NavAction::OrbitDown,
        palette_id: "view.nav.orbit_down",
        label: "Orbit Down",
        key_chord: "ArrowDown",
    },
    NavKeyBinding {
        action: NavAction::PanLeft,
        palette_id: "view.nav.pan_left",
        label: "Pan Left",
        key_chord: "Shift+ArrowLeft",
    },
    NavKeyBinding {
        action: NavAction::PanRight,
        palette_id: "view.nav.pan_right",
        label: "Pan Right",
        key_chord: "Shift+ArrowRight",
    },
    NavKeyBinding {
        action: NavAction::PanUp,
        palette_id: "view.nav.pan_up",
        label: "Pan Up",
        key_chord: "Shift+ArrowUp",
    },
    NavKeyBinding {
        action: NavAction::PanDown,
        palette_id: "view.nav.pan_down",
        label: "Pan Down",
        key_chord: "Shift+ArrowDown",
    },
    NavKeyBinding { action: NavAction::ZoomIn, palette_id: "view.nav.zoom_in", label: "Zoom In", key_chord: "Equal" },
    NavKeyBinding {
        action: NavAction::ZoomOut,
        palette_id: "view.nav.zoom_out",
        label: "Zoom Out",
        key_chord: "Minus",
    },
    NavKeyBinding {
        action: NavAction::RollLeft,
        palette_id: "view.nav.roll_left",
        label: "Roll Left",
        key_chord: "Alt+ArrowLeft",
    },
    NavKeyBinding {
        action: NavAction::RollRight,
        palette_id: "view.nav.roll_right",
        label: "Roll Right",
        key_chord: "Alt+ArrowRight",
    },
    NavKeyBinding {
        action: NavAction::ViewFront,
        palette_id: "view.nav.std_front",
        label: "Front View",
        key_chord: "Numpad1",
    },
    NavKeyBinding {
        action: NavAction::ViewBack,
        palette_id: "view.nav.std_back",
        label: "Back View",
        key_chord: "Ctrl+Numpad1",
    },
    NavKeyBinding {
        action: NavAction::ViewRight,
        palette_id: "view.nav.std_right",
        label: "Right View",
        key_chord: "Numpad3",
    },
    NavKeyBinding {
        action: NavAction::ViewLeft,
        palette_id: "view.nav.std_left",
        label: "Left View",
        key_chord: "Ctrl+Numpad3",
    },
    NavKeyBinding {
        action: NavAction::ViewTop,
        palette_id: "view.nav.std_top",
        label: "Top View",
        key_chord: "Numpad7",
    },
    NavKeyBinding {
        action: NavAction::ViewBottom,
        palette_id: "view.nav.std_bottom",
        label: "Bottom View",
        key_chord: "Ctrl+Numpad7",
    },
    NavKeyBinding {
        action: NavAction::ViewIsometric,
        palette_id: "view.nav.std_isometric",
        label: "Isometric View",
        key_chord: "Numpad0",
    },
    NavKeyBinding {
        action: NavAction::FitToView,
        palette_id: "view.nav.fit_all",
        label: "Fit to View",
        key_chord: "KeyF",
    },
    NavKeyBinding {
        action: NavAction::ZoomToSelection,
        palette_id: "view.nav.zoom_selection",
        label: "Zoom to Selection",
        key_chord: "Shift+KeyF",
    },
];

/// Looks up the default keyboard binding for `action`.
pub fn keybinding_for(action: NavAction) -> Option<&'static NavKeyBinding> {
    NAV_KEYMAP.iter().find(|b| b.action == action)
}

// ------------------------------------------------------------------ deterministic f64 vec math

fn vec_finite(v: [f64; 3]) -> bool {
    v[0].is_finite() && v[1].is_finite() && v[2].is_finite()
}

fn vec_add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn vec_sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn vec_scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn vec_lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn vec_dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn vec_cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn vec_norm(a: [f64; 3]) -> f64 {
    vec_dot(a, a).sqrt()
}

fn vec_norm_or(a: [f64; 3], fallback: [f64; 3]) -> [f64; 3] {
    let n = vec_norm(a);
    if n.is_finite() && n > EPS_LEN {
        vec_scale(a, 1.0 / n)
    } else {
        fallback
    }
}

/// Orthonormal camera basis `(right, up, forward)` in world coordinates.
fn camera_basis(cam: &Camera) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let fwd = vec_norm_or(vec_sub(cam.target_m, cam.eye_m), [0.0, 0.0, -1.0]);
    let up = orthonormal_up(fwd, cam.up, [0.0, 0.0, 1.0]);
    let right = vec_norm_or(vec_cross(fwd, up), [1.0, 0.0, 0.0]);
    let up_ortho = vec_norm_or(vec_cross(right, fwd), up);
    (right, up_ortho, fwd)
}

/// Returns a unit vector perpendicular to unit `fwd`, projected from `desired_up` (or `fallback`).
fn orthonormal_up(fwd: [f64; 3], desired_up: [f64; 3], fallback: [f64; 3]) -> [f64; 3] {
    for candidate in [desired_up, fallback, [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]] {
        let proj = vec_sub(candidate, vec_scale(fwd, vec_dot(candidate, fwd)));
        let n = vec_norm(proj);
        if n.is_finite() && n > EPS_LEN {
            return vec_scale(proj, 1.0 / n);
        }
    }
    [0.0, 0.0, 1.0]
}

/// Rodrigues rotation of `v` around unit `axis` by `angle_rad`.
fn rotate_around_axis(v: [f64; 3], axis: [f64; 3], angle_rad: f64) -> [f64; 3] {
    let k = vec_norm_or(axis, [0.0, 0.0, 1.0]);
    let c = angle_rad.cos();
    let s = angle_rad.sin();
    let k_dot_v = vec_dot(k, v);
    let k_cross_v = vec_cross(k, v);
    vec_add(vec_add(vec_scale(v, c), vec_scale(k_cross_v, s)), vec_scale(k, k_dot_v * (1.0 - c)))
}
