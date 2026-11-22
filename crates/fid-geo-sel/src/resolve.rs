//! CPU resolution of picks and lassos (spec node `GEO.SEL.RESOLVE`).
//!
//! Turns explicit camera parameters and screen-space pixel coordinates (a pick pixel or a lasso
//! polygon) into canonical [`IndexSet`] values on the CPU only (CONSTITUTION C2, KNP-1, D0).
//!
//! # Pinned arithmetic (REQ-GEO.SEL.RESOLVE-003)
//!
//! Every floating-point expression below uses plain IEEE-754 binary64 arithmetic (`+`, `-`, `*`,
//! `/`) in a fixed parenthesised order with no fused multiply-add (`mul_add`), and KNP-1
//! `fid-num` for transcendentals (`tan`) and total-order comparisons (`total_min`, `total_max`,
//! `total_cmp`), so results are bit-identical across x86-64, ARM64 and wasm32.
//!
//! - **Camera frame:** right-handed (`+X` right, `+Y` up, camera looks along `-Z`). `camera.view`
//!   maps element-frame coordinates into camera space via `fid_geo_frame::Transform::apply_point`.
//!   Forward depth is `depth = -z_cam`; a point is inside the depth slab iff `near <= depth <= far`.
//! - **Projection to pixel coordinates `(sx, sy)`** (`(0, 0)` top-left, `+x` right, `+y` down):
//!   - Perspective (`fov_y` in `(0, pi)`, `aspect > 0`, `0 < near < far`):
//!     `half_h = fid_num::f::tan(fov_y * 0.5)`, `half_w = half_h * aspect`,
//!     `ndc_x = cx / (depth * half_w)`, `ndc_y = cy / (depth * half_h)`.
//!   - Orthographic (`height > 0`, `aspect > 0`, `near < far`):
//!     `half_h = height * 0.5`, `half_w = half_h * aspect`,
//!     `ndc_x = cx / half_w`, `ndc_y = cy / half_h`.
//!   - Pixel coordinates: `sx = (ndc_x + 1.0) * 0.5 * w`, `sy = (1.0 - ndc_y) * 0.5 * h`.
//! - **Point-in-polygon (even-odd rule, half-open edge convention):**
//!   For each closed edge `(a, b)`, order endpoints by `y` so `y0 <= y1`:
//!   `(x0, y0, x1, y1) = if a.1 < b.1 { (a.0, a.1, b.0, b.1) } else { (b.0, b.1, a.0, a.1) }`.
//!   The horizontal `+x` ray from `(px, py)` crosses the edge iff
//!   `y0 <= py && py < y1` and `(x1 - x0) * (py - y0) > (px - x0) * (y1 - y0)`.
//!   A point is inside iff the crossing count is odd. On any axis-aligned rectangle
//!   `[x_min, x_max] x [y_min, y_max]`, this selects exactly `[x_min, x_max) x [y_min, y_max)`,
//!   and adjacent polygons sharing an edge partition the plane with no gaps and no overlap.
//! - **Visible mode (REQ-GEO.SEL.RESOLVE-004):** uses [`DepthBuffer`] at viewport resolution
//!   (rasterising triangles in ascending index order for meshes, or splatting points in ascending
//!   index order for point sets/clouds); default depth tolerance is `1e-6 * (far - near)`.
//! - **Triangle pick:** unprojects `pixel` to a ray `o + t d` with `t in [near, far]` (where `t`
//!   equals forward camera-space depth `depth`), queries `PrimitiveBvh::ray`, evaluates fixed-order
//!   two-sided Möller-Trumbore intersection, and selects the nearest hit with ties broken by
//!   ascending triangle index.

use crate::model::{domain_count, Domain, IndexSet, IndexSetBuilder, SelError};
use crate::raster::{clip_triangle_depth, DepthBuffer};
use core::cmp::Ordering;
use fid_geo_frame::{Rigid, Transform};
use fid_geo_index::PrimitiveBvh;
use fid_geo_types::{GeometryElement, Mesh, PointBuffer};
use fid_num::f::{tan, total_max, total_min};

/// Minimum accepted vertex count of a lasso polygon (REQ-GEO.SEL.RESOLVE-005).
pub const MIN_POLYGON_VERTICES: usize = 3;

/// Maximum accepted vertex count of a lasso polygon (REQ-GEO.SEL.RESOLVE-005).
pub const MAX_POLYGON_VERTICES: usize = 4_096;

/// Largest accepted absolute pixel coordinate in a lasso polygon or pick pixel.
pub const MAX_PIXEL_COORD: f64 = 1.0e12;

/// Default depth tolerance as a fraction of `far - near` (REQ-GEO.SEL.RESOLVE-004).
pub const DEFAULT_DEPTH_TOL_FRACTION: f64 = 1.0e-6;

/// Perspective or orthographic camera projection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projection {
    /// Perspective projection: vertical field of view `fov_y` in radians `(0, pi)`,
    /// positive `aspect` (`width / height`), and `0 < near < far` in metres.
    Perspective { fov_y: f64, aspect: f64, near: f64, far: f64 },
    /// Orthographic projection: full vertical `height` in metres (`> 0`), positive `aspect`,
    /// and `near < far` in metres.
    Ortho { height: f64, aspect: f64, near: f64, far: f64 },
}

/// Explicit camera parameters in the element's coordinate frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Rigid transform mapping element-frame points into camera space (`+X` right, `+Y` up,
    /// camera looks along `-Z`).
    pub view: Rigid,
    /// Projection parameters.
    pub proj: Projection,
    /// Viewport size `(width, height)` in pixels; both must be positive.
    pub viewport: (u32, u32),
}

impl Camera {
    /// Default depth tolerance for `ResolveMode::Visible`: `1e-6 * (far - near)`.
    pub fn default_depth_tol(&self) -> Result<f64, SelError> {
        let vc = ValidatedCamera::validate(self)?;
        Ok((vc.far - vc.near) * DEFAULT_DEPTH_TOL_FRACTION)
    }

    /// Near and far clipping plane distances.
    pub const fn near_far(&self) -> (f64, f64) {
        match self.proj {
            Projection::Perspective { near, far, .. } | Projection::Ortho { near, far, .. } => (near, far),
        }
    }
}

/// A closed 2D lasso polygon in screen pixel coordinates (`3..=4096` finite vertices).
#[derive(Debug, Clone, PartialEq)]
pub struct Lasso {
    pub pixels: Vec<(f64, f64)>,
}

impl Lasso {
    /// Validated construction of a lasso polygon.
    pub fn new(pixels: Vec<(f64, f64)>) -> Result<Lasso, SelError> {
        let lasso = Lasso { pixels };
        let _ = ValidatedPolygon::validate(&lasso.pixels)?;
        Ok(lasso)
    }
}

/// Occlusion handling mode for lasso resolution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResolveMode {
    /// Select every element inside the lasso regardless of occlusion.
    Through,
    /// Select only elements not occluded by the CPU depth buffer within `depth_tol`
    /// (`None` uses `1e-6 * (far - near)` per REQ-GEO.SEL.RESOLVE-004).
    Visible { depth_tol: Option<f64> },
}

impl ResolveMode {
    /// `Visible` mode using the default depth tolerance `1e-6 * (far - near)`.
    pub const fn visible_default() -> ResolveMode {
        ResolveMode::Visible { depth_tol: None }
    }
}

/// How a triangle is tested against the lasso (and depth buffer in `Visible` mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TriangleRule {
    /// All three vertices must satisfy the lasso (and visibility) test.
    AllVertices,
    /// At least one vertex must satisfy the lasso (and visibility) test.
    AnyVertex,
    /// The triangle centroid `((a + b) + c) / 3` must satisfy the lasso (and visibility) test.
    Centroid,
}

impl TriangleRule {
    /// Every triangle rule, in canonical order.
    pub const ALL: [TriangleRule; 3] = [TriangleRule::AllVertices, TriangleRule::AnyVertex, TriangleRule::Centroid];

    /// Stable wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            TriangleRule::AllVertices => "all_vertices",
            TriangleRule::AnyVertex => "any_vertex",
            TriangleRule::Centroid => "centroid",
        }
    }
}

/// One projected point in pixel coordinates together with its camera-space forward depth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectedPoint {
    pub sx: f64,
    pub sy: f64,
    pub depth: f64,
}

/// Detailed result of a successful pick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PickHit {
    pub domain: Domain,
    pub index: u64,
    pub depth: f64,
}

// ------------------------------------------------------------------ validated internals

#[derive(Debug, Clone, Copy)]
struct ValidatedCamera {
    view_tf: Transform,
    r: [[f64; 3]; 3],
    t: [f64; 3],
    perspective: bool,
    half_w: f64,
    half_h: f64,
    near: f64,
    far: f64,
    width: u32,
    height: u32,
    wf: f64,
    hf: f64,
}

impl ValidatedCamera {
    fn validate(cam: &Camera) -> Result<ValidatedCamera, SelError> {
        let (width, height) = cam.viewport;
        if width == 0 || height == 0 {
            return Err(SelError::BadCamera { reason: "viewport width and height must be positive" });
        }
        let r = cam.view.r();
        let t = cam.view.t();
        if !r.iter().flatten().all(|x| x.is_finite()) || !t.iter().all(|x| x.is_finite()) {
            return Err(SelError::BadCamera { reason: "non-finite view transform" });
        }
        let (perspective, half_w, half_h, near, far) = match cam.proj {
            Projection::Perspective { fov_y, aspect, near, far } => {
                if !fov_y.is_finite() || !aspect.is_finite() || !near.is_finite() || !far.is_finite() {
                    return Err(SelError::BadCamera { reason: "non-finite perspective parameter" });
                }
                if fov_y <= 0.0 || fov_y >= core::f64::consts::PI {
                    return Err(SelError::BadCamera { reason: "fov_y must be in (0, pi)" });
                }
                if aspect <= 0.0 {
                    return Err(SelError::BadCamera { reason: "aspect must be positive" });
                }
                if near <= 0.0 {
                    return Err(SelError::BadCamera { reason: "perspective near must be positive" });
                }
                if near >= far {
                    return Err(SelError::BadCamera { reason: "near must be strictly less than far" });
                }
                let half_h = tan(fov_y * 0.5).map_err(|_| SelError::BadCamera { reason: "tan(fov_y/2) failed" })?;
                if !half_h.is_finite() || half_h <= 0.0 {
                    return Err(SelError::BadCamera { reason: "degenerate fov_y" });
                }
                let half_w = half_h * aspect;
                if !half_w.is_finite() || half_w <= 0.0 {
                    return Err(SelError::BadCamera { reason: "degenerate perspective frustum width" });
                }
                (true, half_w, half_h, near, far)
            }
            Projection::Ortho { height: h_m, aspect, near, far } => {
                if !h_m.is_finite() || !aspect.is_finite() || !near.is_finite() || !far.is_finite() {
                    return Err(SelError::BadCamera { reason: "non-finite orthographic parameter" });
                }
                if h_m <= 0.0 {
                    return Err(SelError::BadCamera { reason: "ortho height must be positive" });
                }
                if aspect <= 0.0 {
                    return Err(SelError::BadCamera { reason: "aspect must be positive" });
                }
                if near >= far {
                    return Err(SelError::BadCamera { reason: "near must be strictly less than far" });
                }
                let half_h = h_m * 0.5;
                let half_w = half_h * aspect;
                if !half_h.is_finite() || !half_w.is_finite() || half_h <= 0.0 || half_w <= 0.0 {
                    return Err(SelError::BadCamera { reason: "degenerate orthographic frustum" });
                }
                (false, half_w, half_h, near, far)
            }
        };
        if !(far - near).is_finite() || far - near <= 0.0 {
            return Err(SelError::BadCamera { reason: "near-far range overflow" });
        }
        Ok(ValidatedCamera {
            view_tf: Transform::from(cam.view),
            r,
            t,
            perspective,
            half_w,
            half_h,
            near,
            far,
            width,
            height,
            wf: f64::from(width),
            hf: f64::from(height),
        })
    }

    /// Exact pinned rigid-transform expression of `fid_geo_frame::Transform::apply_point`
    /// (`s = 1`, `y[i] = ((R[i][0]*x0 + R[i][1]*x1) + R[i][2]*x2) + t[i]`).
    #[inline]
    fn world_to_camera(&self, p: [f64; 3]) -> Option<[f64; 3]> {
        let r = &self.r;
        let t = &self.t;
        let y0 = ((r[0][0] * p[0] + r[0][1] * p[1]) + r[0][2] * p[2]) + t[0];
        let y1 = ((r[1][0] * p[0] + r[1][1] * p[1]) + r[1][2] * p[2]) + t[1];
        let y2 = ((r[2][0] * p[0] + r[2][1] * p[1]) + r[2][2] * p[2]) + t[2];
        if y0.is_finite() && y1.is_finite() && y2.is_finite() {
            Some([y0, y1, y2])
        } else {
            None
        }
    }

    #[inline]
    fn project_cam(&self, c: [f64; 3]) -> Option<ProjectedPoint> {
        let depth = -c[2];
        if !depth.is_finite() || depth < self.near || depth > self.far {
            return None;
        }
        let (ndc_x, ndc_y) = if self.perspective {
            let denom_x = depth * self.half_w;
            let denom_y = depth * self.half_h;
            if denom_x == 0.0 || denom_y == 0.0 || !denom_x.is_finite() || !denom_y.is_finite() {
                return None;
            }
            (c[0] / denom_x, c[1] / denom_y)
        } else {
            (c[0] / self.half_w, c[1] / self.half_h)
        };
        let sx = (ndc_x + 1.0) * 0.5 * self.wf;
        let sy = (1.0 - ndc_y) * 0.5 * self.hf;
        if !sx.is_finite() || !sy.is_finite() {
            return None;
        }
        Some(ProjectedPoint { sx, sy, depth })
    }

    #[inline]
    fn project_world(&self, p: [f64; 3]) -> Option<ProjectedPoint> {
        let c = self.world_to_camera(p)?;
        self.project_cam(c)
    }

    fn resolve_depth_tol(&self, mode: ResolveMode) -> Result<Option<f64>, SelError> {
        match mode {
            ResolveMode::Through => Ok(None),
            ResolveMode::Visible { depth_tol } => {
                let tol = match depth_tol {
                    Some(t) => t,
                    None => (self.far - self.near) * DEFAULT_DEPTH_TOL_FRACTION,
                };
                if !tol.is_finite() || tol < 0.0 {
                    return Err(SelError::BadTolerance { reason: "depth_tol must be finite and non-negative" });
                }
                Ok(Some(tol))
            }
        }
    }

    fn unproject_ray(&self, pixel: (f64, f64)) -> Result<([f64; 3], [f64; 3]), SelError> {
        let (px, py) = pixel;
        if !px.is_finite() || !py.is_finite() || px.abs() > MAX_PIXEL_COORD || py.abs() > MAX_PIXEL_COORD {
            return Err(SelError::BadTolerance { reason: "pick pixel must be finite and within MAX_PIXEL_COORD" });
        }
        let ndc_x = (px / self.wf) * 2.0 - 1.0;
        let ndc_y = 1.0 - (py / self.hf) * 2.0;
        let (ray_o_cam, ray_d_cam) = if self.perspective {
            ([0.0, 0.0, 0.0], [ndc_x * self.half_w, ndc_y * self.half_h, -1.0])
        } else {
            ([ndc_x * self.half_w, ndc_y * self.half_h, 0.0], [0.0, 0.0, -1.0])
        };
        let inv = self.view_tf.inverse().map_err(|_| SelError::BadCamera { reason: "failed to invert camera view" })?;
        let o = inv.apply_point(&ray_o_cam).map_err(|_| SelError::BadCamera { reason: "non-finite ray origin" })?;
        let d = inv.apply_dir(&ray_d_cam).map_err(|_| SelError::BadCamera { reason: "non-finite ray direction" })?;
        Ok((o, d))
    }
}

struct ValidatedPolygon<'a> {
    verts: &'a [(f64, f64)],
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

impl<'a> ValidatedPolygon<'a> {
    fn validate(verts: &'a [(f64, f64)]) -> Result<ValidatedPolygon<'a>, SelError> {
        if verts.len() < MIN_POLYGON_VERTICES {
            return Err(SelError::BadPolygon { reason: "polygon must have at least 3 vertices" });
        }
        if verts.len() > MAX_POLYGON_VERTICES {
            return Err(SelError::BadPolygon { reason: "polygon exceeds 4 096 vertices" });
        }
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for &(x, y) in verts {
            if !x.is_finite() || !y.is_finite() {
                return Err(SelError::BadPolygon { reason: "non-finite polygon vertex" });
            }
            if x.abs() > MAX_PIXEL_COORD || y.abs() > MAX_PIXEL_COORD {
                return Err(SelError::BadPolygon { reason: "polygon vertex exceeds MAX_PIXEL_COORD" });
            }
            min_x = total_min(min_x, x).map_err(|_| SelError::BadPolygon { reason: "polygon bounds failed" })?;
            max_x = total_max(max_x, x).map_err(|_| SelError::BadPolygon { reason: "polygon bounds failed" })?;
            min_y = total_min(min_y, y).map_err(|_| SelError::BadPolygon { reason: "polygon bounds failed" })?;
            max_y = total_max(max_y, y).map_err(|_| SelError::BadPolygon { reason: "polygon bounds failed" })?;
        }
        Ok(ValidatedPolygon { verts, min_x, max_x, min_y, max_y })
    }

    #[inline]
    fn contains(&self, px: f64, py: f64) -> bool {
        // Exact y-interval pre-reject (y0 <= py < y1 can never hold outside [min_y, max_y))
        // and conservative x pre-reject (1.0 px margin avoids any float-rounding disagreement).
        if py < self.min_y || py >= self.max_y || px < self.min_x - 1.0 || px > self.max_x + 1.0 {
            return false;
        }
        contains_even_odd(px, py, self.verts)
    }
}

/// Pinned even-odd crossing test with half-open edge convention (no division, no FMA).
#[inline]
fn contains_even_odd(px: f64, py: f64, verts: &[(f64, f64)]) -> bool {
    let n = verts.len();
    let mut inside = false;
    let mut a = verts[n - 1];
    for &b in verts {
        let (x0, y0, x1, y1) = if a.1 < b.1 { (a.0, a.1, b.0, b.1) } else { (b.0, b.1, a.0, a.1) };
        if y0 <= py && py < y1 {
            let lhs = (x1 - x0) * (py - y0);
            let rhs = (px - x0) * (y1 - y0);
            if lhs > rhs {
                inside = !inside;
            }
        }
        a = b;
    }
    inside
}

// ------------------------------------------------------------------ public helpers

/// Projects one element-frame point through `camera` into pixel coordinates and forward depth.
///
/// Returns `Ok(None)` when the point lies outside `[near, far]`.
pub fn project_point(camera: &Camera, point: [f64; 3]) -> Result<Option<ProjectedPoint>, SelError> {
    let vc = ValidatedCamera::validate(camera)?;
    if !point.iter().all(|x| x.is_finite()) {
        return Ok(None);
    }
    Ok(vc.project_world(point))
}

/// Tests whether pixel `(px, py)` lies inside `polygon` under the pinned even-odd rule and
/// half-open edge convention (REQ-GEO.SEL.RESOLVE-003).
pub fn point_in_polygon(px: f64, py: f64, polygon: &[(f64, f64)]) -> Result<bool, SelError> {
    let poly = ValidatedPolygon::validate(polygon)?;
    if !px.is_finite() || !py.is_finite() {
        return Err(SelError::BadTolerance { reason: "test point must be finite" });
    }
    Ok(poly.contains(px, py))
}

// ------------------------------------------------------------------ lasso resolution

/// Resolves a lasso polygon against `element` on the CPU (REQ-GEO.SEL.RESOLVE-001..005).
///
/// Supports:
/// - `PointSet` / `PointCloud` with `Domain::Points` (`rule` is ignored);
/// - `Mesh` with `Domain::Points` (vertices; `rule` is ignored) or `Domain::Triangles` (by `rule`).
///
/// For CAD tessellations carrying per-triangle face IDs (`Domain::Faces`), use
/// [`resolve_lasso_faces`].
pub fn resolve_lasso(
    element: &GeometryElement,
    camera: &Camera,
    lasso: &Lasso,
    domain: Domain,
    rule: TriangleRule,
    mode: ResolveMode,
) -> Result<IndexSet, SelError> {
    let vc = ValidatedCamera::validate(camera)?;
    let poly = ValidatedPolygon::validate(&lasso.pixels)?;
    let depth_tol = vc.resolve_depth_tol(mode)?;

    let count =
        domain_count(element, domain)?.ok_or_else(|| SelError::DomainMismatch { domain, kind: element.kind() })?;
    if count == 0 {
        return Err(SelError::EmptyTarget);
    }

    match (element, domain) {
        (GeometryElement::PointSet(ps), Domain::Points) => resolve_points(ps.buffer(), &vc, &poly, depth_tol),
        (GeometryElement::PointCloud(pc), Domain::Points) => resolve_points(pc.buffer(), &vc, &poly, depth_tol),
        (GeometryElement::Mesh(mesh), Domain::Points) => resolve_mesh_points(mesh, &vc, &poly, depth_tol),
        (GeometryElement::Mesh(mesh), Domain::Triangles) => resolve_mesh_triangles(mesh, &vc, &poly, rule, depth_tol),
        _ => Err(SelError::DomainMismatch { domain, kind: element.kind() }),
    }
}

/// Resolves a lasso polygon on `mesh` in `Domain::Faces` using per-triangle `face_ids`
/// (REQ-GEO.SEL.RESOLVE-006, AC-6).
///
/// Returns every face ID with at least one selected triangle under `rule` and `mode`.
pub fn resolve_lasso_faces(
    mesh: &Mesh,
    face_ids: &[u64],
    camera: &Camera,
    lasso: &Lasso,
    rule: TriangleRule,
    mode: ResolveMode,
) -> Result<IndexSet, SelError> {
    let vc = ValidatedCamera::validate(camera)?;
    let poly = ValidatedPolygon::validate(&lasso.pixels)?;
    let depth_tol = vc.resolve_depth_tol(mode)?;

    let tri_count = mesh.triangle_count();
    if tri_count == 0 {
        return Err(SelError::EmptyTarget);
    }
    let found = u64::try_from(face_ids.len()).unwrap_or(u64::MAX);
    if found != tri_count {
        return Err(SelError::FaceIdsMismatch { expected: tri_count, found });
    }

    let tri_set = resolve_mesh_triangles(mesh, &vc, &poly, rule, depth_tol)?;
    let mut faces = IndexSetBuilder::new(Domain::Faces, u64::MAX);
    for t in tri_set.iter() {
        let Some(&fid) = usize::try_from(t).ok().and_then(|i| face_ids.get(i)) else {
            return Err(SelError::FaceIdsMismatch { expected: tri_count, found });
        };
        faces.push(fid)?;
    }
    Ok(faces.finish())
}

fn resolve_points(
    buf: &PointBuffer,
    vc: &ValidatedCamera,
    poly: &ValidatedPolygon<'_>,
    depth_tol: Option<f64>,
) -> Result<IndexSet, SelError> {
    let n = buf.len();
    let nu = usize::try_from(n).map_err(|_| SelError::AllocationFailed)?;

    let zbuf = match depth_tol {
        None => None,
        Some(_) => {
            let mut zb = DepthBuffer::new(vc.width, vc.height)?;
            for i in 0..nu {
                if let Some(p) = vc.project_world([buf.x[i], buf.y[i], buf.z[i]]) {
                    zb.splat_point(p.sx, p.sy, p.depth);
                }
            }
            Some(zb)
        }
    };

    let mut builder = IndexSetBuilder::new(Domain::Points, n);
    for i in 0..nu {
        let Some(p) = vc.project_world([buf.x[i], buf.y[i], buf.z[i]]) else {
            continue;
        };
        if !poly.contains(p.sx, p.sy) {
            continue;
        }
        if let (Some(zb), Some(tol)) = (&zbuf, depth_tol) {
            if !zb.is_visible(p.sx, p.sy, p.depth, tol) {
                continue;
            }
        }
        builder.push(i as u64)?;
    }
    Ok(builder.finish())
}

fn build_mesh_depth_buffer(
    mesh: &Mesh,
    cam_verts: &[Option<[f64; 3]>],
    screen_verts: &[Option<ProjectedPoint>],
    vc: &ValidatedCamera,
) -> Result<DepthBuffer, SelError> {
    let mut zb = DepthBuffer::new(vc.width, vc.height)?;
    let n_tris = mesh.triangle_count();
    for t in 0..n_tris {
        let Some([i0, i1, i2]) = mesh.triangle(t) else {
            continue;
        };
        let (Ok(u0), Ok(u1), Ok(u2)) = (usize::try_from(i0), usize::try_from(i1), usize::try_from(i2)) else {
            continue;
        };
        // Fast path: when all 3 vertices are already inside [near, far], Sutherland-Hodgman
        // clipping is the identity, so we reuse the pre-projected screen coordinates directly.
        if let (Some(Some(p0)), Some(Some(p1)), Some(Some(p2))) =
            (screen_verts.get(u0), screen_verts.get(u1), screen_verts.get(u2))
        {
            zb.rasterise_screen_triangle(
                [p0.sx, p0.sy, p0.depth],
                [p1.sx, p1.sy, p1.depth],
                [p2.sx, p2.sy, p2.depth],
                vc.perspective,
            );
            continue;
        }
        let Some(c0) = cam_verts.get(u0).and_then(|v| *v) else {
            continue;
        };
        let Some(c1) = cam_verts.get(u1).and_then(|v| *v) else {
            continue;
        };
        let Some(c2) = cam_verts.get(u2).and_then(|v| *v) else {
            continue;
        };
        let d0 = -c0[2];
        let d1 = -c1[2];
        let d2 = -c2[2];
        if (d0 < vc.near && d1 < vc.near && d2 < vc.near) || (d0 > vc.far && d1 > vc.far && d2 > vc.far) {
            continue;
        }
        let (clipped, count) = clip_triangle_depth(c0, c1, c2, vc.near, vc.far);
        if count < 3 {
            continue;
        }
        let mut screen = [[0.0; 3]; 5];
        let mut all_ok = true;
        for k in 0..count {
            match vc.project_cam(clipped[k]) {
                Some(p) => screen[k] = [p.sx, p.sy, p.depth],
                None => {
                    all_ok = false;
                    break;
                }
            }
        }
        if !all_ok {
            continue;
        }
        for k in 1..(count - 1) {
            zb.rasterise_screen_triangle(screen[0], screen[k], screen[k + 1], vc.perspective);
        }
    }
    Ok(zb)
}

struct ProjectedMeshVertices {
    cam: Vec<Option<[f64; 3]>>,
    screen: Vec<Option<ProjectedPoint>>,
}

fn transform_and_project_mesh_vertices(mesh: &Mesh, vc: &ValidatedCamera) -> Result<ProjectedMeshVertices, SelError> {
    let vbuf = mesh.vertices();
    let v_count = vbuf.len();
    let vu = usize::try_from(v_count).map_err(|_| SelError::AllocationFailed)?;
    let mut cam = Vec::new();
    let mut screen = Vec::new();
    cam.try_reserve_exact(vu).map_err(|_| SelError::AllocationFailed)?;
    screen.try_reserve_exact(vu).map_err(|_| SelError::AllocationFailed)?;
    for i in 0..vu {
        let c = vc.world_to_camera([vbuf.x[i], vbuf.y[i], vbuf.z[i]]);
        let p = c.and_then(|cv| vc.project_cam(cv));
        cam.push(c);
        screen.push(p);
    }
    Ok(ProjectedMeshVertices { cam, screen })
}

fn resolve_mesh_points(
    mesh: &Mesh,
    vc: &ValidatedCamera,
    poly: &ValidatedPolygon<'_>,
    depth_tol: Option<f64>,
) -> Result<IndexSet, SelError> {
    let pv = transform_and_project_mesh_vertices(mesh, vc)?;
    let zbuf = match depth_tol {
        None => None,
        Some(_) => Some(build_mesh_depth_buffer(mesh, &pv.cam, &pv.screen, vc)?),
    };
    let v_count = mesh.vertex_count();
    let mut builder = IndexSetBuilder::new(Domain::Points, v_count);
    for (idx, p_opt) in pv.screen.iter().enumerate() {
        let Some(p) = *p_opt else {
            continue;
        };
        if !poly.contains(p.sx, p.sy) {
            continue;
        }
        if let (Some(zb), Some(tol)) = (&zbuf, depth_tol) {
            if !zb.is_visible(p.sx, p.sy, p.depth, tol) {
                continue;
            }
        }
        builder.push(idx as u64)?;
    }
    Ok(builder.finish())
}

fn resolve_mesh_triangles(
    mesh: &Mesh,
    vc: &ValidatedCamera,
    poly: &ValidatedPolygon<'_>,
    rule: TriangleRule,
    depth_tol: Option<f64>,
) -> Result<IndexSet, SelError> {
    let pv = transform_and_project_mesh_vertices(mesh, vc)?;
    let zbuf = match depth_tol {
        None => None,
        Some(_) => Some(build_mesh_depth_buffer(mesh, &pv.cam, &pv.screen, vc)?),
    };

    let t_count = mesh.triangle_count();
    let mut builder = IndexSetBuilder::new(Domain::Triangles, t_count);

    match rule {
        TriangleRule::AllVertices | TriangleRule::AnyVertex => {
            let mut vert_hit = Vec::new();
            vert_hit.try_reserve_exact(pv.screen.len()).map_err(|_| SelError::AllocationFailed)?;
            for p_opt in &pv.screen {
                let ok = match *p_opt {
                    None => false,
                    Some(p) => {
                        if !poly.contains(p.sx, p.sy) {
                            false
                        } else if let (Some(zb), Some(tol)) = (&zbuf, depth_tol) {
                            zb.is_visible(p.sx, p.sy, p.depth, tol)
                        } else {
                            true
                        }
                    }
                };
                vert_hit.push(ok);
            }
            for t in 0..t_count {
                let Some([i0, i1, i2]) = mesh.triangle(t) else {
                    continue;
                };
                let h0 = usize::try_from(i0).ok().and_then(|i| vert_hit.get(i)).copied().unwrap_or(false);
                let h1 = usize::try_from(i1).ok().and_then(|i| vert_hit.get(i)).copied().unwrap_or(false);
                let h2 = usize::try_from(i2).ok().and_then(|i| vert_hit.get(i)).copied().unwrap_or(false);
                let selected = match rule {
                    TriangleRule::AllVertices => h0 && h1 && h2,
                    TriangleRule::AnyVertex => h0 || h1 || h2,
                    TriangleRule::Centroid => false,
                };
                if selected {
                    builder.push(t)?;
                }
            }
        }
        TriangleRule::Centroid => {
            let vbuf = mesh.vertices();
            for t in 0..t_count {
                let Some([i0, i1, i2]) = mesh.triangle(t) else {
                    continue;
                };
                let (Ok(u0), Ok(u1), Ok(u2)) = (usize::try_from(i0), usize::try_from(i1), usize::try_from(i2)) else {
                    continue;
                };
                if u0 >= vbuf.x.len() || u1 >= vbuf.x.len() || u2 >= vbuf.x.len() {
                    continue;
                }
                let centroid = [
                    ((vbuf.x[u0] + vbuf.x[u1]) + vbuf.x[u2]) / 3.0,
                    ((vbuf.y[u0] + vbuf.y[u1]) + vbuf.y[u2]) / 3.0,
                    ((vbuf.z[u0] + vbuf.z[u1]) + vbuf.z[u2]) / 3.0,
                ];
                let Some(p) = vc.project_world(centroid) else {
                    continue;
                };
                if !poly.contains(p.sx, p.sy) {
                    continue;
                }
                if let (Some(zb), Some(tol)) = (&zbuf, depth_tol) {
                    if !zb.is_visible(p.sx, p.sy, p.depth, tol) {
                        continue;
                    }
                }
                builder.push(t)?;
            }
        }
    }

    Ok(builder.finish())
}

// ------------------------------------------------------------------ pick resolution

/// Resolves a pixel pick against `element` on the CPU, returning an empty [`IndexSet`] on miss
/// or a singleton [`IndexSet`] on hit.
///
/// - For `Domain::Triangles` on a [`Mesh`], `bvh` must be provided and match the mesh's frame and
///   triangle count; picks the nearest hit triangle via `PrimitiveBvh::ray` (ties by ascending
///   triangle index).
/// - For `Domain::Points` (on `PointSet`, `PointCloud` or `Mesh` vertices), picks the nearest
///   in-frustum point within `tol_px` Euclidean pixels of `pixel` (ties by pixel distance squared,
///   then ascending point index).
pub fn resolve_pick(
    element: &GeometryElement,
    bvh: Option<&PrimitiveBvh>,
    camera: &Camera,
    pixel: (f64, f64),
    tol_px: f64,
    domain: Domain,
) -> Result<IndexSet, SelError> {
    let hit = resolve_pick_hit(element, bvh, camera, pixel, tol_px, domain)?;
    let count =
        domain_count(element, domain)?.ok_or_else(|| SelError::DomainMismatch { domain, kind: element.kind() })?;
    match hit {
        None => Ok(IndexSet::empty(domain)),
        Some(h) => IndexSet::from_indices(domain, &[h.index], count),
    }
}

/// Like [`resolve_pick`], returning the full [`PickHit`] (domain, index, depth) or `None` on miss.
pub fn resolve_pick_hit(
    element: &GeometryElement,
    bvh: Option<&PrimitiveBvh>,
    camera: &Camera,
    pixel: (f64, f64),
    tol_px: f64,
    domain: Domain,
) -> Result<Option<PickHit>, SelError> {
    let vc = ValidatedCamera::validate(camera)?;
    if !pixel.0.is_finite()
        || !pixel.1.is_finite()
        || pixel.0.abs() > MAX_PIXEL_COORD
        || pixel.1.abs() > MAX_PIXEL_COORD
    {
        return Err(SelError::BadTolerance { reason: "pick pixel must be finite and within MAX_PIXEL_COORD" });
    }
    if !tol_px.is_finite() || tol_px < 0.0 || tol_px > MAX_PIXEL_COORD {
        return Err(SelError::BadTolerance {
            reason: "tol_px must be finite, non-negative and within MAX_PIXEL_COORD",
        });
    }

    let count =
        domain_count(element, domain)?.ok_or_else(|| SelError::DomainMismatch { domain, kind: element.kind() })?;
    if count == 0 {
        return Err(SelError::EmptyTarget);
    }

    match (element, domain) {
        (GeometryElement::PointSet(ps), Domain::Points) => Ok(pick_point_buffer(ps.buffer(), &vc, pixel, tol_px)),
        (GeometryElement::PointCloud(pc), Domain::Points) => Ok(pick_point_buffer(pc.buffer(), &vc, pixel, tol_px)),
        (GeometryElement::Mesh(mesh), Domain::Points) => Ok(pick_point_buffer(mesh.vertices(), &vc, pixel, tol_px)),
        (GeometryElement::Mesh(mesh), Domain::Triangles) => {
            let Some(bvh) = bvh else {
                return Err(SelError::BvhMismatch { reason: "PrimitiveBvh is required for triangle pick" });
            };
            pick_mesh_triangle(mesh, bvh, &vc, pixel)
        }
        _ => Err(SelError::DomainMismatch { domain, kind: element.kind() }),
    }
}

/// Resolves a pixel pick on `mesh` in `Domain::Faces` using per-triangle `face_ids`
/// (REQ-GEO.SEL.RESOLVE-006).
pub fn resolve_pick_faces(
    mesh: &Mesh,
    face_ids: &[u64],
    bvh: &PrimitiveBvh,
    camera: &Camera,
    pixel: (f64, f64),
) -> Result<IndexSet, SelError> {
    let vc = ValidatedCamera::validate(camera)?;
    if !pixel.0.is_finite()
        || !pixel.1.is_finite()
        || pixel.0.abs() > MAX_PIXEL_COORD
        || pixel.1.abs() > MAX_PIXEL_COORD
    {
        return Err(SelError::BadTolerance { reason: "pick pixel must be finite and within MAX_PIXEL_COORD" });
    }
    let tri_count = mesh.triangle_count();
    if tri_count == 0 {
        return Err(SelError::EmptyTarget);
    }
    let found = u64::try_from(face_ids.len()).unwrap_or(u64::MAX);
    if found != tri_count {
        return Err(SelError::FaceIdsMismatch { expected: tri_count, found });
    }
    match pick_mesh_triangle(mesh, bvh, &vc, pixel)? {
        None => Ok(IndexSet::empty(Domain::Faces)),
        Some(hit) => {
            let Some(&fid) = usize::try_from(hit.index).ok().and_then(|i| face_ids.get(i)) else {
                return Err(SelError::FaceIdsMismatch { expected: tri_count, found });
            };
            IndexSet::from_indices(Domain::Faces, &[fid], u64::MAX)
        }
    }
}

fn pick_point_buffer(buf: &PointBuffer, vc: &ValidatedCamera, pixel: (f64, f64), tol_px: f64) -> Option<PickHit> {
    let tol2 = tol_px * tol_px;
    let nu = usize::try_from(buf.len()).ok()?;
    let mut best: Option<(f64, f64, u64)> = None;
    for i in 0..nu {
        let Some(p) = vc.project_world([buf.x[i], buf.y[i], buf.z[i]]) else {
            continue;
        };
        let dx = p.sx - pixel.0;
        let dy = p.sy - pixel.1;
        let d2 = dx * dx + dy * dy;
        if d2 > tol2 || !d2.is_finite() {
            continue;
        }
        let cand = (p.depth, d2, i as u64);
        let better = match best {
            None => true,
            Some(cur) => {
                cand.0.total_cmp(&cur.0).then(cand.1.total_cmp(&cur.1)).then(cand.2.cmp(&cur.2)) == Ordering::Less
            }
        };
        if better {
            best = Some(cand);
        }
    }
    best.map(|(depth, _, index)| PickHit { domain: Domain::Points, index, depth })
}

fn pick_mesh_triangle(
    mesh: &Mesh,
    bvh: &PrimitiveBvh,
    vc: &ValidatedCamera,
    pixel: (f64, f64),
) -> Result<Option<PickHit>, SelError> {
    if bvh.frame() != mesh.frame() {
        return Err(SelError::BvhMismatch { reason: "BVH frame differs from mesh frame" });
    }
    if bvh.triangle_count() != mesh.triangle_count() {
        return Err(SelError::BvhMismatch { reason: "BVH triangle count differs from mesh" });
    }
    let (o, d) = vc.unproject_ray(pixel)?;
    let candidates = bvh
        .ray(o, d, vc.near, vc.far)
        .map_err(|_| SelError::BvhMismatch { reason: "BVH ray query failed (non-finite or out-of-range ray)" })?;

    let mut best: Option<(f64, u64)> = None;
    for t_idx in candidates {
        let Some([i0, i1, i2]) = mesh.triangle(t_idx) else {
            continue;
        };
        let (Some(a), Some(b), Some(c)) = (mesh.vertex(i0), mesh.vertex(i1), mesh.vertex(i2)) else {
            continue;
        };
        if let Some(t) = ray_triangle_intersect(o, d, a, b, c, vc.near, vc.far) {
            let better = match best {
                None => true,
                Some((bt, bi)) => t.total_cmp(&bt).then(t_idx.cmp(&bi)) == Ordering::Less,
            };
            if better {
                best = Some((t, t_idx));
            }
        }
    }
    Ok(best.map(|(depth, index)| PickHit { domain: Domain::Triangles, index, depth }))
}

#[inline]
fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

#[inline]
fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

#[inline]
fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Fixed-order two-sided Möller-Trumbore ray-triangle intersection (no FMA).
///
/// Returns `Some(t)` with `t_min <= t <= t_max` when the ray `o + t d` meets triangle `(a, b, c)`
/// (boundaries included).
pub fn ray_triangle_intersect(
    o: [f64; 3],
    d: [f64; 3],
    a: [f64; 3],
    b: [f64; 3],
    c: [f64; 3],
    t_min: f64,
    t_max: f64,
) -> Option<f64> {
    let e1 = sub3(b, a);
    let e2 = sub3(c, a);
    let pvec = cross3(d, e2);
    let det = dot3(e1, pvec);
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    let inv_det = 1.0 / det;
    if !inv_det.is_finite() {
        return None;
    }
    let tvec = sub3(o, a);
    let u = dot3(tvec, pvec) * inv_det;
    if !u.is_finite() || !(0.0..=1.0).contains(&u) {
        return None;
    }
    let qvec = cross3(tvec, e1);
    let v = dot3(d, qvec) * inv_det;
    if !v.is_finite() || v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = dot3(e2, qvec) * inv_det;
    if !t.is_finite() || t < t_min || t > t_max {
        return None;
    }
    Some(t)
}
