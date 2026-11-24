//! Signed deviation primitive and deterministic batch evaluation (spec node
//! `SCAN.CMP.DEV.PRIM`).
//!
//! Pure Rust, `f64` metres, CPU-only (KNP-1, CONSTITUTION C2): no FMA, `fid_num::f` for
//! `sqrt`/`acos`/`total_clamp`/`total_max`, no hash containers, and bit-identical output
//! for any thread count or block size.

use core::f64::consts::PI;
use core::fmt;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

pub use fid_geo_dist::batch::{BatchConfig, BLOCK_QUANTUM, MAX_BLOCK, MAX_THREADS};
pub use fid_geo_dist::mesh::COORD_LIMIT;
use fid_geo_dist::{Feature, MeshHit, MeshProjection, MeshQuery, SignError, SignTarget, SignUndefined};
use fid_geo_frame::require_same_frame_ref;
use fid_geo_types::{FrameRef, PointSet};

use crate::error::DevError;
use crate::field::{DeviationField, FieldBuilder, FieldProvenance};

/// How a deviation is measured (REQ-SCAN.CMP.DEV-001). Its wire name is recorded in the
/// field provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Method {
    /// `(a - f) . n_f` at the closest-point foot `f` (SCAN.CMP.DEV.PRIM).
    Normal,
    /// Signed Euclidean distance to the closest point.
    Shortest,
}

impl Method {
    /// Every method, in declaration order.
    pub const ALL: [Method; 2] = [Method::Normal, Method::Shortest];

    /// The stable wire name: `normal` or `shortest`.
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Normal => "normal",
            Method::Shortest => "shortest",
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a point has no value (REQ-SCAN.CMP.DEV-002). The field stores only the status
/// `NO_MATCH`, not the reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NoMatchReason {
    /// No nominal point within the maximum search distance.
    BeyondDistance,
    /// The actual normal is further than the angle cutoff from the nominal normal.
    Angle,
    /// Method `normal` and the foot point lies on an open boundary of the nominal.
    OffEdge,
}

/// The result of the deviation primitive for one actual point: a signed deviation in
/// metres (positive on the material-outside side of the nominal), no match, or excluded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DevResult {
    /// A value of record in metres. Must be finite (`scan.cmp.non_finite` otherwise).
    Valid(f64),
    /// No value (REQ-SCAN.CMP.DEV-002).
    NoMatch(NoMatchReason),
    /// Not compared, for example a vertex used only by synthesized triangles
    /// (REQ-SCAN.CMP.DEV-004).
    Excluded,
}

/// Parameters governing [`deviation`], [`deviation_batch`], and [`deviation_field`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DevParams {
    /// Measurement method (`normal` or `shortest`).
    pub method: Method,
    /// Maximum search distance in metres; must be finite and strictly positive (`> 0.0`).
    pub max_distance: f64,
    /// Maximum normal-angle discrepancy in radians; must lie in `[0.0, PI]`.
    pub max_angle: f64,
}

impl DevParams {
    /// Validates parameter domains (`scan.cmp.invalid_parameter`).
    pub fn validate(&self) -> Result<(), DevError> {
        if !(self.max_distance.is_finite() && self.max_distance > 0.0) {
            return Err(DevError::InvalidParameter { name: "max_distance" });
        }
        if !(0.0..=PI).contains(&self.max_angle) {
            return Err(DevError::InvalidParameter { name: "max_angle" });
        }
        Ok(())
    }
}

/// Closest-point projection result on a CAD nominal surface or body (`SCAN.CMP.DEV.PRIM`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CadHit {
    /// Closest foot point `f` on the CAD nominal, in metres.
    pub foot: [f64; 3],
    /// Outward unit nominal normal `n_f` at `f`.
    pub normal: [f64; 3],
    /// Signed Euclidean distance from the query point to `f` (`+d` outside, `-d` inside).
    pub signed_distance: f64,
    /// Whether `signed_distance` carries a certified sign of record.
    pub sign_defined: bool,
    /// Whether `f` lies on an open boundary edge or vertex of the CAD face set.
    pub on_open_boundary: bool,
}

/// A CAD face set or body nominal target (`GEO.DIST.FACE.BODY` / analytic CAD).
pub trait CadNominal: Sync {
    /// Coordinate frame of this nominal.
    fn frame(&self) -> FrameRef;
    /// Closest point on the CAD nominal within `max_distance` metres (`Ok(None)` when beyond).
    fn project_cad(&self, p: [f64; 3], max_distance: f64) -> Result<Option<CadHit>, DevError>;
}

/// A nominal surface or shape against which actual points are compared (`SCAN.CMP.DEV.PRIM`).
///
/// Left `#[non_exhaustive]` so downstream consumers (`SCAN.CMP.ACT`, `SCAN.SECT.WHISK`) can
/// introduce variants such as `OrientedCloud` and `PlanarCurve`.
#[non_exhaustive]
#[derive(Clone, Copy)]
pub enum NominalRef<'a> {
    /// CAD face set or body.
    Cad(&'a dyn CadNominal),
    /// Triangulated mesh with precomputed pseudo-normals (`GEO.DIST.SIGN`).
    Mesh(&'a SignTarget<'a>),
}

impl fmt::Debug for NominalRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NominalRef::Cad(c) => f.debug_tuple("Cad").field(&c.frame()).finish(),
            NominalRef::Mesh(m) => f.debug_tuple("Mesh").field(&m.target().mesh().frame()).finish(),
        }
    }
}

impl NominalRef<'_> {
    /// The coordinate frame of the nominal geometry.
    pub fn frame(&self) -> FrameRef {
        match self {
            NominalRef::Cad(c) => c.frame(),
            NominalRef::Mesh(m) => m.target().mesh().frame(),
        }
    }
}

#[inline]
fn clean_zero(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

#[inline]
fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

#[inline]
fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

#[inline]
fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn max_abs3(v: [f64; 3]) -> Option<f64> {
    let m0 = fid_num::f::total_max(v[0].abs(), v[1].abs()).ok()?;
    fid_num::f::total_max(m0, v[2].abs()).ok()
}

/// Normalises a non-zero finite 3-vector using `fid_num::f::sqrt` and no FMA.
fn normalize_vec3(v: [f64; 3]) -> Option<[f64; 3]> {
    if !(v[0].is_finite() && v[1].is_finite() && v[2].is_finite()) {
        return None;
    }
    let m = max_abs3(v)?;
    if m == 0.0 {
        return None;
    }
    let (sv, len) = if (1e-100..=1e100).contains(&m) {
        let n2 = dot3(v, v);
        let n = fid_num::f::sqrt(n2).ok()?;
        if n == 0.0 {
            return None;
        }
        (v, n)
    } else {
        let s = [v[0] / m, v[1] / m, v[2] / m];
        let n2 = dot3(s, s);
        let n = fid_num::f::sqrt(n2).ok()?;
        if n == 0.0 {
            return None;
        }
        (s, n)
    };
    let u = [sv[0] / len, sv[1] / len, sv[2] / len];
    if u[0].is_finite() && u[1].is_finite() && u[2].is_finite() {
        Some(u)
    } else {
        None
    }
}

fn check_query_point(p: [f64; 3], index: Option<u64>) -> Result<(), DevError> {
    if !(p[0].is_finite() && p[1].is_finite() && p[2].is_finite()) {
        return Err(DevError::NonFiniteQuery { index });
    }
    if !p.iter().all(|x| (-COORD_LIMIT..=COORD_LIMIT).contains(x)) {
        return Err(DevError::QueryOutOfRange { index });
    }
    Ok(())
}

fn validate_actual_normal(n: [f64; 3], index: Option<u64>) -> Result<[f64; 3], DevError> {
    if !(n[0].is_finite() && n[1].is_finite() && n[2].is_finite()) {
        return Err(DevError::InvalidNormal { index });
    }
    if !n.iter().all(|x| (-COORD_LIMIT..=COORD_LIMIT).contains(x)) {
        return Err(DevError::InvalidNormal { index });
    }
    normalize_vec3(n).ok_or(DevError::InvalidNormal { index })
}

fn normalize_nominal(n: [f64; 3]) -> Result<[f64; 3], DevError> {
    normalize_vec3(n).ok_or(DevError::UndefinedNormal)
}

fn angle_exceeds(n_a: [f64; 3], n_f: [f64; 3], max_angle: f64) -> Result<bool, DevError> {
    let raw_dot = dot3(n_a, n_f);
    let clamped = fid_num::f::total_clamp(raw_dot, -1.0, 1.0).map_err(|_| DevError::NonFiniteQuery { index: None })?;
    let ang = fid_num::f::acos(clamped).map_err(|_| DevError::NonFiniteQuery { index: None })?;
    Ok(ang > max_angle)
}

fn is_mesh_open_boundary(target: &SignTarget<'_>, hit: &MeshHit) -> Result<bool, DevError> {
    let mesh = target.target().mesh();
    let tri = mesh.triangle(hit.tri).ok_or(DevError::Sign(SignError::InvalidHit))?;
    match hit.feature {
        Feature::Interior => Ok(false),
        Feature::Edge(e) => {
            let e_idx = usize::from(e);
            if e_idx >= 3 {
                return Err(DevError::Sign(SignError::InvalidHit));
            }
            let va = tri[e_idx];
            let vb = tri[(e_idx + 1) % 3];
            let en = target.normals().edge(va, vb).ok_or(DevError::Sign(SignError::Inconsistent))?;
            Ok(en.feature.status == Some(SignUndefined::Boundary))
        }
        Feature::Vertex(v) => {
            let v_idx = usize::from(v);
            if v_idx >= 3 {
                return Err(DevError::Sign(SignError::InvalidHit));
            }
            let vid = tri[v_idx];
            let vn = target.normals().vertex(vid).ok_or(DevError::Sign(SignError::Inconsistent))?;
            Ok(vn.status == Some(SignUndefined::Boundary))
        }
    }
}

fn mesh_foot_normal(target: &SignTarget<'_>, hit: &MeshHit, on_open_boundary: bool) -> Result<[f64; 3], DevError> {
    let mesh = target.target().mesh();
    let tri = mesh.triangle(hit.tri).ok_or(DevError::Sign(SignError::InvalidHit))?;
    let a = mesh.vertex(tri[0]).ok_or(DevError::Sign(SignError::InvalidHit))?;
    let b = mesh.vertex(tri[1]).ok_or(DevError::Sign(SignError::InvalidHit))?;
    let c = mesh.vertex(tri[2]).ok_or(DevError::Sign(SignError::InvalidHit))?;
    let tri_n_raw = cross3(sub3(b, a), sub3(c, a));
    match hit.feature {
        Feature::Interior => {
            let status = target.normals().triangle(hit.tri).ok_or(DevError::Sign(SignError::InvalidHit))?;
            if status.is_some() {
                return Err(DevError::UndefinedNormal);
            }
            normalize_nominal(tri_n_raw)
        }
        Feature::Edge(e) => {
            let e_idx = usize::from(e);
            if e_idx >= 3 {
                return Err(DevError::Sign(SignError::InvalidHit));
            }
            let en = target
                .normals()
                .edge(tri[e_idx], tri[(e_idx + 1) % 3])
                .ok_or(DevError::Sign(SignError::Inconsistent))?;
            if en.feature.status.is_none() {
                normalize_nominal(en.feature.normal)
            } else if on_open_boundary {
                normalize_nominal(tri_n_raw)
            } else {
                Err(DevError::UndefinedNormal)
            }
        }
        Feature::Vertex(v) => {
            let v_idx = usize::from(v);
            if v_idx >= 3 {
                return Err(DevError::Sign(SignError::InvalidHit));
            }
            let vn = target.normals().vertex(tri[v_idx]).ok_or(DevError::Sign(SignError::Inconsistent))?;
            if vn.status.is_none() {
                normalize_nominal(vn.normal)
            } else if on_open_boundary {
                normalize_nominal(tri_n_raw)
            } else {
                Err(DevError::UndefinedNormal)
            }
        }
    }
}

fn evaluate_validated(
    point: [f64; 3],
    unit_actual_normal: Option<[f64; 3]>,
    nominal: NominalRef<'_>,
    params: &DevParams,
) -> Result<DevResult, DevError> {
    match nominal {
        NominalRef::Mesh(target) => {
            let query = MeshQuery { max_distance: Some(params.max_distance), subset: None };
            match target.target().closest(point, &query)? {
                MeshProjection::NoHit => Ok(DevResult::NoMatch(NoMatchReason::BeyondDistance)),
                MeshProjection::Hit(hit) => {
                    let on_open_boundary = is_mesh_open_boundary(target, &hit)?;
                    match params.method {
                        Method::Normal => {
                            if on_open_boundary {
                                return Ok(DevResult::NoMatch(NoMatchReason::OffEdge));
                            }
                            let n_f = mesh_foot_normal(target, &hit, on_open_boundary)?;
                            if let Some(n_a) = unit_actual_normal {
                                if angle_exceeds(n_a, n_f, params.max_angle)? {
                                    return Ok(DevResult::NoMatch(NoMatchReason::Angle));
                                }
                            }
                            let val = dot3(sub3(point, hit.foot), n_f);
                            if !val.is_finite() {
                                return Err(DevError::NonFiniteQuery { index: None });
                            }
                            Ok(DevResult::Valid(clean_zero(val)))
                        }
                        Method::Shortest => {
                            if let Some(n_a) = unit_actual_normal {
                                let n_f = mesh_foot_normal(target, &hit, on_open_boundary)?;
                                if angle_exceeds(n_a, n_f, params.max_angle)? {
                                    return Ok(DevResult::NoMatch(NoMatchReason::Angle));
                                }
                            }
                            let signed = target.signed(point, &hit)?;
                            if !signed.distance.is_finite() {
                                return Err(DevError::NonFiniteQuery { index: None });
                            }
                            Ok(DevResult::Valid(clean_zero(signed.distance)))
                        }
                    }
                }
            }
        }
        NominalRef::Cad(cad) => {
            let Some(hit) = cad.project_cad(point, params.max_distance)? else {
                return Ok(DevResult::NoMatch(NoMatchReason::BeyondDistance));
            };
            if !(hit.foot[0].is_finite() && hit.foot[1].is_finite() && hit.foot[2].is_finite()) {
                return Err(DevError::NonFiniteQuery { index: None });
            }
            match params.method {
                Method::Normal => {
                    if hit.on_open_boundary {
                        return Ok(DevResult::NoMatch(NoMatchReason::OffEdge));
                    }
                    let n_f = normalize_nominal(hit.normal)?;
                    if let Some(n_a) = unit_actual_normal {
                        if angle_exceeds(n_a, n_f, params.max_angle)? {
                            return Ok(DevResult::NoMatch(NoMatchReason::Angle));
                        }
                    }
                    let val = dot3(sub3(point, hit.foot), n_f);
                    if !val.is_finite() {
                        return Err(DevError::NonFiniteQuery { index: None });
                    }
                    Ok(DevResult::Valid(clean_zero(val)))
                }
                Method::Shortest => {
                    if let Some(n_a) = unit_actual_normal {
                        let n_f = normalize_nominal(hit.normal)?;
                        if angle_exceeds(n_a, n_f, params.max_angle)? {
                            return Ok(DevResult::NoMatch(NoMatchReason::Angle));
                        }
                    }
                    if !hit.signed_distance.is_finite() {
                        return Err(DevError::NonFiniteQuery { index: None });
                    }
                    Ok(DevResult::Valid(clean_zero(hit.signed_distance)))
                }
            }
        }
    }
}

/// Evaluates the signed deviation of one actual point against `nominal` (`SCAN.CMP.DEV.PRIM`).
pub fn deviation(
    point: [f64; 3],
    normal: Option<[f64; 3]>,
    nominal: NominalRef<'_>,
    params: &DevParams,
) -> Result<DevResult, DevError> {
    params.validate()?;
    check_query_point(point, None)?;
    let unit_n = match normal {
        Some(n) => Some(validate_actual_normal(n, None)?),
        None => None,
    };
    evaluate_validated(point, unit_n, nominal, params)
}

fn tag_point_error(err: DevError, index: u64) -> DevError {
    match err {
        DevError::NonFiniteQuery { index: None } => DevError::NonFiniteQuery { index: Some(index) },
        DevError::QueryOutOfRange { index: None } => DevError::QueryOutOfRange { index: Some(index) },
        DevError::InvalidNormal { index: None } => DevError::InvalidNormal { index: Some(index) },
        other => other,
    }
}

/// Deterministic CPU batch evaluation writing into the crate-private [`FieldBuilder`]
/// (REQ-SCAN.CMP.DEV.PRIM-005).
pub(crate) fn deviation_batch(
    points: &PointSet,
    normals: Option<&[[f64; 3]]>,
    nominal: NominalRef<'_>,
    params: &DevParams,
    provenance: FieldProvenance,
    config: BatchConfig,
    cancel: &AtomicBool,
) -> Result<FieldBuilder, DevError> {
    require_same_frame_ref(points.frame(), nominal.frame())?;
    params.validate()?;
    provenance.validate()?;
    if provenance.method != params.method {
        return Err(DevError::InvalidParameter { name: "method" });
    }
    if provenance.max_distance_m != params.max_distance {
        return Err(DevError::InvalidParameter { name: "max_distance_m" });
    }
    if provenance.max_angle_rad != params.max_angle {
        return Err(DevError::InvalidParameter { name: "max_angle_rad" });
    }
    if config.threads == 0 || config.threads > MAX_THREADS {
        return Err(DevError::InvalidThreads { threads: config.threads });
    }
    if config.block == 0 || config.block % BLOCK_QUANTUM != 0 || config.block > MAX_BLOCK {
        return Err(DevError::InvalidBlockSize { block: config.block });
    }

    let (xs, ys, zs) = (points.x(), points.y(), points.z());
    let n = xs.len();
    if ys.len() != n || zs.len() != n {
        return Err(DevError::WorkerFailed);
    }
    if let Some(ns) = normals {
        if ns.len() != n {
            return Err(DevError::InvalidParameter { name: "normals" });
        }
    }

    let mut unit_normals: Vec<[f64; 3]> = Vec::new();
    if let Some(ns) = normals {
        let bytes = (n as u64).saturating_mul(core::mem::size_of::<[f64; 3]>() as u64);
        unit_normals.try_reserve_exact(n).map_err(|_| DevError::AllocationFailed { needed: bytes })?;
        for (i, ((&x, &y), &z)) in xs.iter().zip(ys).zip(zs).enumerate() {
            check_query_point([x, y, z], Some(i as u64))?;
            unit_normals.push(validate_actual_normal(ns[i], Some(i as u64))?);
        }
    } else {
        for (i, ((&x, &y), &z)) in xs.iter().zip(ys).zip(zs).enumerate() {
            check_query_point([x, y, z], Some(i as u64))?;
        }
    }

    if cancel.load(Ordering::Acquire) {
        return Err(DevError::Cancelled);
    }

    let mut builder = FieldBuilder::new(n, provenance)?;
    if n == 0 {
        return Ok(builder);
    }

    let res_bytes = (n as u64).saturating_mul(core::mem::size_of::<DevResult>() as u64);
    let mut results: Vec<DevResult> = Vec::new();
    results.try_reserve_exact(n).map_err(|_| DevError::AllocationFailed { needed: res_bytes })?;
    results.resize(n, DevResult::Excluded);

    let first_error = AtomicU64::new(u64::MAX);
    let error: Mutex<Option<(u64, DevError)>> = Mutex::new(None);
    let failed = AtomicBool::new(false);
    let unit_slice = if normals.is_some() { Some(unit_normals.as_slice()) } else { None };

    {
        let count = n.div_ceil(config.block);
        let block_bytes = (count as u64).saturating_mul(core::mem::size_of::<(usize, &mut [DevResult])>() as u64);
        let mut blocks: Vec<(usize, &mut [DevResult])> = Vec::new();
        blocks.try_reserve_exact(count).map_err(|_| DevError::AllocationFailed { needed: block_bytes })?;
        blocks.extend(results.chunks_mut(config.block).enumerate().rev());
        let queue = Mutex::new(blocks);

        let worker = || loop {
            if cancel.load(Ordering::Acquire) {
                return;
            }
            let item = match queue.lock() {
                Ok(mut q) => q.pop(),
                Err(_) => {
                    failed.store(true, Ordering::Release);
                    return;
                }
            };
            let Some((bi, chunk)) = item else { return };
            let start = bi.saturating_mul(config.block);
            if start as u64 > first_error.load(Ordering::Acquire) {
                continue;
            }
            for (k, slot) in chunk.iter_mut().enumerate() {
                let i = start + k;
                let (Some(&x), Some(&y), Some(&z)) = (xs.get(i), ys.get(i), zs.get(i)) else {
                    failed.store(true, Ordering::Release);
                    return;
                };
                let n_a = match unit_slice {
                    Some(us) => match us.get(i) {
                        Some(&u) => Some(u),
                        None => {
                            failed.store(true, Ordering::Release);
                            return;
                        }
                    },
                    None => None,
                };
                match evaluate_validated([x, y, z], n_a, nominal, params) {
                    Ok(r) => *slot = r,
                    Err(e) => {
                        let idx = i as u64;
                        first_error.fetch_min(idx, Ordering::AcqRel);
                        match error.lock() {
                            Ok(mut g) => {
                                if g.as_ref().map_or(true, |(j, _)| idx < *j) {
                                    *g = Some((idx, tag_point_error(e, idx)));
                                }
                            }
                            Err(_) => failed.store(true, Ordering::Release),
                        }
                        break;
                    }
                }
            }
        };

        std::thread::scope(|s| {
            let mut handles = Vec::new();
            for _ in 1..config.threads {
                // Single-threaded fallback (e.g. wasm32) when thread spawning is unavailable.
                match std::thread::Builder::new().spawn_scoped(s, worker) {
                    Ok(h) => handles.push(h),
                    Err(_) => break,
                }
            }
            worker();
            for h in handles {
                if h.join().is_err() {
                    failed.store(true, Ordering::Release);
                }
            }
        });
    }

    if cancel.load(Ordering::Acquire) {
        return Err(DevError::Cancelled);
    }
    if failed.load(Ordering::Acquire) {
        return Err(DevError::WorkerFailed);
    }
    match error.into_inner() {
        Ok(Some((_, e))) => return Err(e),
        Ok(None) => {}
        Err(_) => return Err(DevError::WorkerFailed),
    }

    for (i, r) in results.into_iter().enumerate() {
        builder.set(i, r)?;
    }
    Ok(builder)
}

/// Evaluates the signed deviation field for `points` against `nominal` on the CPU
/// (`SCAN.CMP.DEV.PRIM` + `SCAN.CMP.DEV.FIELD`).
pub fn deviation_field(
    points: &PointSet,
    normals: Option<&[[f64; 3]]>,
    nominal: NominalRef<'_>,
    params: &DevParams,
    provenance: FieldProvenance,
    config: BatchConfig,
    cancel: &AtomicBool,
) -> Result<DeviationField, DevError> {
    Ok(deviation_batch(points, normals, nominal, params, provenance, config, cancel)?.finish()?)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use fid_geo_dist::{cad_sign, MeshTarget};
    use fid_geo_index::PrimitiveBvh;
    use fid_geo_nurbs::{Placement, Sphere, Surface};
    use fid_geo_types::{ElementKind, Mesh, PointSet};
    use fid_kci::AlgorithmId;
    use fid_types::Digest;
    use std::collections::BTreeMap;

    const FIX: &str = "/fixtures/scan/cmp/";

    fn read_fixture(name: &str) -> String {
        std::fs::read_to_string(format!("{}{FIX}{name}", fid_testkit::WORKSPACE_ROOT)).unwrap()
    }

    fn f64_hex(s: &str) -> f64 {
        assert_eq!(s.len(), 16, "bad hex {s:?}");
        f64::from_bits(u64::from_str_radix(s, 16).unwrap())
    }

    #[derive(Debug, Clone)]
    enum JVal {
        Str(String),
        Num(f64),
        Arr(Vec<JVal>),
        Obj(BTreeMap<String, JVal>),
        Null,
    }

    impl JVal {
        fn as_obj(&self) -> &BTreeMap<String, JVal> {
            match self {
                JVal::Obj(m) => m,
                other => panic!("expected obj, got {other:?}"),
            }
        }
        fn as_arr(&self) -> &[JVal] {
            match self {
                JVal::Arr(a) => a,
                other => panic!("expected arr, got {other:?}"),
            }
        }
        fn as_str(&self) -> &str {
            match self {
                JVal::Str(s) => s,
                other => panic!("expected str, got {other:?}"),
            }
        }
        fn as_f64(&self) -> f64 {
            match self {
                JVal::Num(n) => *n,
                other => panic!("expected num, got {other:?}"),
            }
        }
        fn as_v3(&self) -> [f64; 3] {
            let a = self.as_arr();
            assert_eq!(a.len(), 3);
            [a[0].as_f64(), a[1].as_f64(), a[2].as_f64()]
        }
        fn as_u64_3(&self) -> [u64; 3] {
            let a = self.as_arr();
            assert_eq!(a.len(), 3);
            [a[0].as_f64() as u64, a[1].as_f64() as u64, a[2].as_f64() as u64]
        }
    }

    fn parse_json(src: &str) -> JVal {
        let bytes = src.as_bytes();
        let mut idx = 0usize;
        fn skip_ws(b: &[u8], i: &mut usize) {
            while *i < b.len() && b[*i].is_ascii_whitespace() {
                *i += 1;
            }
        }
        fn parse_str(b: &[u8], i: &mut usize) -> String {
            assert_eq!(b[*i], b'"');
            *i += 1;
            let mut out = String::new();
            while *i < b.len() {
                let c = b[*i];
                *i += 1;
                if c == b'"' {
                    return out;
                }
                if c == b'\\' {
                    let esc = b[*i];
                    *i += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        other => panic!("unexpected escape {}", other as char),
                    }
                } else {
                    out.push(c as char);
                }
            }
            panic!("unterminated string");
        }
        fn parse_val(b: &[u8], i: &mut usize) -> JVal {
            skip_ws(b, i);
            match b[*i] {
                b'"' => JVal::Str(parse_str(b, i)),
                b'{' => {
                    *i += 1;
                    let mut m = BTreeMap::new();
                    skip_ws(b, i);
                    if b[*i] == b'}' {
                        *i += 1;
                        return JVal::Obj(m);
                    }
                    loop {
                        skip_ws(b, i);
                        let k = parse_str(b, i);
                        skip_ws(b, i);
                        assert_eq!(b[*i], b':');
                        *i += 1;
                        let v = parse_val(b, i);
                        m.insert(k, v);
                        skip_ws(b, i);
                        if b[*i] == b',' {
                            *i += 1;
                        } else {
                            assert_eq!(b[*i], b'}');
                            *i += 1;
                            break;
                        }
                    }
                    JVal::Obj(m)
                }
                b'[' => {
                    *i += 1;
                    let mut a = Vec::new();
                    skip_ws(b, i);
                    if b[*i] == b']' {
                        *i += 1;
                        return JVal::Arr(a);
                    }
                    loop {
                        a.push(parse_val(b, i));
                        skip_ws(b, i);
                        if b[*i] == b',' {
                            *i += 1;
                        } else {
                            assert_eq!(b[*i], b']');
                            *i += 1;
                            break;
                        }
                    }
                    JVal::Arr(a)
                }
                b'n' => {
                    assert_eq!(&b[*i..*i + 4], b"null");
                    *i += 4;
                    JVal::Null
                }
                _ => {
                    let start = *i;
                    while *i < b.len() && (b[*i].is_ascii_digit() || matches!(b[*i], b'-' | b'+' | b'.' | b'e' | b'E'))
                    {
                        *i += 1;
                    }
                    let s = core::str::from_utf8(&b[start..*i]).unwrap();
                    JVal::Num(s.parse::<f64>().unwrap())
                }
            }
        }
        parse_val(bytes, &mut idx)
    }

    fn frame(tag: u8) -> FrameRef {
        FrameRef::from_bytes([tag; 16])
    }

    fn sample_prov(method: Method, max_distance_m: f64, max_angle_rad: f64, kind: ElementKind) -> FieldProvenance {
        FieldProvenance {
            method,
            max_distance_m,
            max_angle_rad,
            alignment_id: "aln-001".to_string(),
            alignment_digest: Digest::from_bytes([1u8; 32]),
            actual_digest: Digest::from_bytes([2u8; 32]),
            nominal_digest: Digest::from_bytes([3u8; 32]),
            nominal_kind: kind,
            actual_selection: None,
            nominal_selection: None,
            algorithm_id: AlgorithmId {
                module: "scan".to_string(),
                feature: "cmp_dev".to_string(),
                method: method.as_str().to_string(),
                version: (1, 0, 0),
            },
            kernel_version: "0.1.0".to_string(),
        }
    }

    struct CadSphereNominal {
        frame: FrameRef,
        sphere: Sphere,
    }

    impl CadNominal for CadSphereNominal {
        fn frame(&self) -> FrameRef {
            self.frame
        }

        fn project_cad(&self, p: [f64; 3], max_distance: f64) -> Result<Option<CadHit>, DevError> {
            let foot_res = self.sphere.invert(p).map_err(|_| DevError::ProjectionFailed { reason: "sphere_invert" })?;
            if foot_res.distance > max_distance {
                return Ok(None);
            }
            let mut normal = match self
                .sphere
                .normal(foot_res.u, foot_res.v)
                .map_err(|_| DevError::ProjectionFailed { reason: "sphere_eval" })?
            {
                Some(n) => n,
                None => normalize_vec3(foot_res.foot).ok_or(DevError::UndefinedNormal)?,
            };
            if dot3(normal, foot_res.foot) < 0.0 {
                normal = [-normal[0], -normal[1], -normal[2]];
            }
            let signed = cad_sign(p, foot_res.foot, 1e-15, &[(normal, core::f64::consts::TAU)])?;
            Ok(Some(CadHit {
                foot: foot_res.foot,
                normal,
                signed_distance: signed.distance,
                sign_defined: signed.sign_defined(),
                on_open_boundary: false,
            }))
        }
    }

    fn build_bvh(mesh: &Mesh) -> PrimitiveBvh {
        let payload: Vec<u64> = (0..mesh.triangle_count()).collect();
        PrimitiveBvh::build(mesh, &payload, 2).unwrap()
    }

    fn build_sign<'a>(mesh: &'a Mesh, bvh: &'a PrimitiveBvh) -> SignTarget<'a> {
        let mt = MeshTarget::new(mesh, bvh).unwrap();
        SignTarget::new(mt).unwrap()
    }

    #[test]
    fn ac1_dev_sphere_cad() {
        let doc = parse_json(&read_fixture("dev_sphere.json"));
        let root = doc.as_obj();
        let radius = root["radius_m"].as_f64();
        let max_distance = root["max_distance_m"].as_f64();
        let max_angle = root["max_angle_rad"].as_f64();
        let f = frame(1);
        let pl = Placement::new([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]).unwrap();
        let sphere = Sphere::new(pl, radius).unwrap();
        let cad = CadSphereNominal { frame: f, sphere };

        for method in [Method::Normal, Method::Shortest] {
            let params = DevParams { method, max_distance, max_angle };
            for case in root["cases"].as_arr() {
                let c = case.as_obj();
                let name = c["name"].as_str();
                let pt = c["point"].as_v3();
                let n = c["normal"].as_v3();
                let exp = f64_hex(c["expected_hex"].as_str());
                let res = deviation(pt, Some(n), NominalRef::Cad(&cad), &params).unwrap();
                match res {
                    DevResult::Valid(got) => {
                        assert!((got - exp).abs() <= 1e-12, "{name}: got {got} exp {exp}");
                    }
                    other => panic!("{name}: unexpected {other:?}"),
                }
            }
            let exact = deviation([radius, 0.0, 0.0], Some([1.0, 0.0, 0.0]), NominalRef::Cad(&cad), &params).unwrap();
            match exact {
                DevResult::Valid(got) => assert_eq!(got.to_bits(), 0, "expected +0.0 at [r, 0, 0]"),
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    #[test]
    fn ac2_dev_cube_edges() {
        let doc = parse_json(&read_fixture("dev_cube_edges.json"));
        let root = doc.as_obj();
        let max_distance = root["max_distance_m"].as_f64();
        let max_angle = root["max_angle_rad"].as_f64();
        let verts: Vec<[f64; 3]> = root["vertices"].as_arr().iter().map(JVal::as_v3).collect();
        let tris: Vec<[u64; 3]> = root["triangles"].as_arr().iter().map(JVal::as_u64_3).collect();

        let f = frame(2);
        let mesh = Mesh::from_points(f, &verts, &tris).unwrap();
        let bvh = build_bvh(&mesh);
        let sign_target = build_sign(&mesh, &bvh);

        let p_short = DevParams { method: Method::Shortest, max_distance, max_angle };
        let p_norm = DevParams { method: Method::Normal, max_distance, max_angle };

        for case in root["cases"].as_arr() {
            let c = case.as_obj();
            let name = c["name"].as_str();
            let pt = c["point"].as_v3();
            let exp_s = f64_hex(c["shortest_hex"].as_str());
            let exp_n = f64_hex(c["normal_hex"].as_str());

            match deviation(pt, None, NominalRef::Mesh(&sign_target), &p_short).unwrap() {
                DevResult::Valid(got) => {
                    assert!((got - exp_s).abs() <= 1e-12, "{name} shortest: got {got} exp {exp_s}");
                }
                other => panic!("{name} shortest: {other:?}"),
            }
            match deviation(pt, None, NominalRef::Mesh(&sign_target), &p_norm).unwrap() {
                DevResult::Valid(got) => {
                    assert!((got - exp_n).abs() <= 1e-12, "{name} normal: got {got} exp {exp_n}");
                }
                other => panic!("{name} normal: {other:?}"),
            }
        }
    }

    #[test]
    fn ac3_dev_open_plate() {
        let doc = parse_json(&read_fixture("dev_open_plate.json"));
        let root = doc.as_obj();
        let max_distance = root["max_distance_m"].as_f64();
        let max_angle = root["max_angle_rad"].as_f64();
        let verts: Vec<[f64; 3]> = root["vertices"].as_arr().iter().map(JVal::as_v3).collect();
        let tris: Vec<[u64; 3]> = root["triangles"].as_arr().iter().map(JVal::as_u64_3).collect();

        let f = frame(3);
        let mesh = Mesh::from_points(f, &verts, &tris).unwrap();
        let bvh = build_bvh(&mesh);
        let sign_target = build_sign(&mesh, &bvh);

        let p_short = DevParams { method: Method::Shortest, max_distance, max_angle };
        let p_norm = DevParams { method: Method::Normal, max_distance, max_angle };

        for case in root["cases"].as_arr() {
            let c = case.as_obj();
            let name = c["name"].as_str();
            let pt = c["point"].as_v3();
            let exp_s = f64_hex(c["shortest_hex"].as_str());

            match deviation(pt, Some([0.0, 0.0, 1.0]), NominalRef::Mesh(&sign_target), &p_short).unwrap() {
                DevResult::Valid(got) => {
                    assert!((got - exp_s).abs() <= 1e-12, "{name} shortest: got {got} exp {exp_s}");
                }
                other => panic!("{name} shortest: {other:?}"),
            }

            let res_n = deviation(pt, Some([0.0, 0.0, 1.0]), NominalRef::Mesh(&sign_target), &p_norm).unwrap();
            match c["normal_status"].as_str() {
                "off_edge" => assert_eq!(res_n, DevResult::NoMatch(NoMatchReason::OffEdge), "{name}"),
                "valid" => {
                    let exp_n = f64_hex(c["normal_hex"].as_str());
                    match res_n {
                        DevResult::Valid(got) => {
                            assert!((got - exp_n).abs() <= 1e-12, "{name} normal: got {got} exp {exp_n}");
                        }
                        other => panic!("{name} normal: {other:?}"),
                    }
                }
                other => panic!("unexpected status {other}"),
            }
        }

        // Flipped actual normal beyond border triggers NoMatch(Angle) under Method::Shortest
        // (uses boundary triangle face normal for angle check), while Method::Normal returns OffEdge.
        let beyond = [0.055, 0.025, 0.001];
        assert_eq!(
            deviation(beyond, Some([0.0, 0.0, -1.0]), NominalRef::Mesh(&sign_target), &p_short).unwrap(),
            DevResult::NoMatch(NoMatchReason::Angle)
        );
        assert_eq!(
            deviation(beyond, Some([0.0, 0.0, -1.0]), NominalRef::Mesh(&sign_target), &p_norm).unwrap(),
            DevResult::NoMatch(NoMatchReason::OffEdge)
        );
    }

    #[test]
    fn ac4_distance_and_angle_cutoffs() {
        let f = frame(4);
        let mesh = Mesh::from_points(f, &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], &[[0, 1, 2]]).unwrap();
        let bvh = build_bvh(&mesh);
        let sign_target = build_sign(&mesh, &bvh);

        // s = 1 mm, point 2 mm away -> BeyondDistance
        let p_dist = DevParams { method: Method::Normal, max_distance: 0.001, max_angle: PI };
        assert_eq!(
            deviation([0.2, 0.2, 0.002], None, NominalRef::Mesh(&sign_target), &p_dist).unwrap(),
            DevResult::NoMatch(NoMatchReason::BeyondDistance)
        );

        // theta = 30 deg, actual normal at 45 deg -> Angle; normal = None skips angle test
        let theta = 30.0_f64.to_radians();
        let p_ang = DevParams { method: Method::Normal, max_distance: 0.01, max_angle: theta };
        let inv_s2 = 1.0 / 2.0_f64.sqrt();
        assert_eq!(
            deviation([0.2, 0.2, 0.001], Some([inv_s2, 0.0, inv_s2]), NominalRef::Mesh(&sign_target), &p_ang).unwrap(),
            DevResult::NoMatch(NoMatchReason::Angle)
        );
        assert_eq!(
            deviation([0.2, 0.2, 0.001], None, NominalRef::Mesh(&sign_target), &p_ang).unwrap(),
            DevResult::Valid(0.001)
        );

        // Boundary dot products strictly outside [-1, 1] clamp safely without Domain error
        // (note: `1.0 + 1e-16` rounds to `1.0` in IEEE-754 f64, so also test 1 ULP past +/-1.0).
        let over_one = f64::from_bits(1.0_f64.to_bits() + 1);
        assert!(!angle_exceeds([0.0, 0.0, over_one], [0.0, 0.0, 1.0], 0.0).unwrap());
        assert!(!angle_exceeds([0.0, 0.0, -over_one], [0.0, 0.0, 1.0], PI).unwrap());
        let p_zero = DevParams { method: Method::Normal, max_distance: 0.01, max_angle: 0.0 };
        assert_eq!(
            deviation([0.2, 0.2, 0.001], Some([0.0, 0.0, over_one]), NominalRef::Mesh(&sign_target), &p_zero).unwrap(),
            DevResult::Valid(0.001)
        );
        let p_pi = DevParams { method: Method::Normal, max_distance: 0.01, max_angle: PI };
        assert_eq!(
            deviation([0.2, 0.2, 0.001], Some([0.0, 0.0, -over_one]), NominalRef::Mesh(&sign_target), &p_pi).unwrap(),
            DevResult::Valid(0.001)
        );
    }

    #[test]
    fn ac5_batch_determinism() {
        let f = frame(5);
        let mesh = Mesh::from_points(
            f,
            &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
            &[[0, 1, 2], [0, 2, 3]],
        )
        .unwrap();
        let bvh = build_bvh(&mesh);
        let sign_target = build_sign(&mesh, &bvh);

        // Full 10^6 points on native; smaller count on wasm32 so wasmtime finishes quickly.
        let n = if cfg!(target_family = "wasm") { 8_192 } else { 1_000_000 };
        let mut pts = Vec::with_capacity(n);
        let mut s = 0x1234_5678_9abc_def0_u64;
        for _ in 0..n {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let x = ((s & 0xffff) as f64 / 65535.0) * 1.1 - 0.05;
            let y = (((s >> 16) & 0xffff) as f64 / 65535.0) * 1.1 - 0.05;
            let z = (((s >> 32) & 0xffff) as f64 / 65535.0) * 0.004 - 0.002;
            pts.push([x, y, z]);
        }
        let point_set = PointSet::from_points(f, &pts).unwrap();
        let params = DevParams { method: Method::Normal, max_distance: 0.01, max_angle: PI / 4.0 };
        let prov = sample_prov(Method::Normal, params.max_distance, params.max_angle, ElementKind::Mesh);
        let cancel = AtomicBool::new(false);

        let mut digests = Vec::new();
        for threads in [1, 3, 16] {
            for block in [64, 4096] {
                let field = deviation_field(
                    &point_set,
                    None,
                    NominalRef::Mesh(&sign_target),
                    &params,
                    prov.clone(),
                    BatchConfig { threads, block },
                    &cancel,
                )
                .unwrap();
                assert_eq!(field.len(), n);
                digests.push(field.digest());
            }
        }
        for d in &digests[1..] {
            assert_eq!(d, &digests[0], "batch digest must not depend on threads or block size");
        }
    }

    #[test]
    fn error_paths_and_edge_cases() {
        let f1 = frame(10);
        let f2 = frame(11);
        let mesh = Mesh::from_points(f1, &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], &[[0, 1, 2]]).unwrap();
        let bvh = build_bvh(&mesh);
        let sign_target = build_sign(&mesh, &bvh);

        // DevParams validation
        for bad in [
            DevParams { method: Method::Normal, max_distance: 0.0, max_angle: 0.5 },
            DevParams { method: Method::Normal, max_distance: -0.0, max_angle: 0.5 },
            DevParams { method: Method::Normal, max_distance: -1e-3, max_angle: 0.5 },
            DevParams { method: Method::Normal, max_distance: f64::NAN, max_angle: 0.5 },
            DevParams { method: Method::Normal, max_distance: f64::INFINITY, max_angle: 0.5 },
            DevParams { method: Method::Normal, max_distance: 1.0, max_angle: -1e-300 },
            DevParams { method: Method::Normal, max_distance: 1.0, max_angle: PI + 1e-15 },
            DevParams { method: Method::Normal, max_distance: 1.0, max_angle: f64::NAN },
            DevParams { method: Method::Normal, max_distance: 1.0, max_angle: f64::INFINITY },
        ] {
            let err = bad.validate().unwrap_err();
            assert_eq!(err.code(), "scan.cmp.invalid_parameter");
        }
        assert!(DevParams { method: Method::Shortest, max_distance: 1.0, max_angle: -0.0 }.validate().is_ok());
        assert!(DevParams { method: Method::Shortest, max_distance: 1.0, max_angle: PI }.validate().is_ok());

        let ok_params = DevParams { method: Method::Normal, max_distance: 1.0, max_angle: PI };
        let ok_prov = sample_prov(Method::Normal, 1.0, PI, ElementKind::Mesh);

        // Single-point bad inputs
        assert_eq!(
            deviation([f64::NAN, 0.0, 0.0], None, NominalRef::Mesh(&sign_target), &ok_params).unwrap_err().code(),
            "scan.cmp.non_finite"
        );
        assert_eq!(
            deviation([COORD_LIMIT * 2.0, 0.0, 0.0], None, NominalRef::Mesh(&sign_target), &ok_params)
                .unwrap_err()
                .code(),
            "scan.cmp.query_out_of_range"
        );
        assert_eq!(
            deviation([0.2, 0.2, 0.0], Some([0.0, 0.0, 0.0]), NominalRef::Mesh(&sign_target), &ok_params)
                .unwrap_err()
                .code(),
            "scan.cmp.invalid_normal"
        );
        assert_eq!(
            deviation([0.2, 0.2, 0.0], Some([f64::NAN, 0.0, 1.0]), NominalRef::Mesh(&sign_target), &ok_params)
                .unwrap_err()
                .code(),
            "scan.cmp.invalid_normal"
        );

        // Negative zero normalization on surface
        let res_zero = deviation([0.2, 0.2, -0.0], None, NominalRef::Mesh(&sign_target), &ok_params).unwrap();
        assert_eq!(res_zero, DevResult::Valid(0.0));
        if let DevResult::Valid(v) = res_zero {
            assert_eq!(v.to_bits(), 0);
        }

        // FrameMismatch checked BEFORE invalid config or params
        let bad_pts_frame = PointSet::from_points(f2, &[[0.2, 0.2, 0.0]]).unwrap();
        let bad_params = DevParams { method: Method::Normal, max_distance: -1.0, max_angle: PI };
        let cancel = AtomicBool::new(false);
        let err_frame = deviation_field(
            &bad_pts_frame,
            None,
            NominalRef::Mesh(&sign_target),
            &bad_params,
            ok_prov.clone(),
            BatchConfig { threads: 0, block: 0 },
            &cancel,
        )
        .unwrap_err();
        assert_eq!(err_frame.code(), "geo.frame.mismatch");

        // Provenance vs params mismatch
        let ok_pts = PointSet::from_points(f1, &[[0.2, 0.2, 0.0]]).unwrap();
        let wrong_method_prov = sample_prov(Method::Shortest, 1.0, PI, ElementKind::Mesh);
        assert_eq!(
            deviation_field(
                &ok_pts,
                None,
                NominalRef::Mesh(&sign_target),
                &ok_params,
                wrong_method_prov,
                BatchConfig::default(),
                &cancel,
            )
            .unwrap_err()
            .code(),
            "scan.cmp.invalid_parameter"
        );

        // Normals length mismatch
        assert_eq!(
            deviation_field(
                &ok_pts,
                Some(&[]),
                NominalRef::Mesh(&sign_target),
                &ok_params,
                ok_prov.clone(),
                BatchConfig::default(),
                &cancel,
            )
            .unwrap_err()
            .code(),
            "scan.cmp.invalid_parameter"
        );

        // Pre-cancelled batch
        let pre_cancel = AtomicBool::new(true);
        assert_eq!(
            deviation_field(
                &ok_pts,
                None,
                NominalRef::Mesh(&sign_target),
                &ok_params,
                ok_prov.clone(),
                BatchConfig::default(),
                &pre_cancel,
            )
            .unwrap_err()
            .code(),
            "scan.cmp.cancelled"
        );

        // Empty PointSet
        let empty_pts = PointSet::from_points(f1, &[]).unwrap();
        let empty_field = deviation_field(
            &empty_pts,
            None,
            NominalRef::Mesh(&sign_target),
            &ok_params,
            ok_prov.clone(),
            BatchConfig::default(),
            &cancel,
        )
        .unwrap();
        assert!(empty_field.is_empty());

        // Lowest failing index determinism across multi-block thread schedule (both pre-check and worker scope)
        struct FailAtCad {
            frame: FrameRef,
        }
        impl CadNominal for FailAtCad {
            fn frame(&self) -> FrameRef {
                self.frame
            }
            fn project_cad(&self, p: [f64; 3], _: f64) -> Result<Option<CadHit>, DevError> {
                if p[0] > 10.0 && p[0] < 25.0 {
                    return Err(DevError::ProjectionFailed { reason: "idx_777" });
                }
                if p[0] >= 25.0 {
                    return Err(DevError::ProjectionFailed { reason: "idx_4321" });
                }
                Ok(Some(CadHit {
                    foot: [0.0, 0.0, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    signed_distance: p[2],
                    sign_defined: true,
                    on_open_boundary: false,
                }))
            }
        }
        let mut pts_fail = vec![[0.0, 0.0, 0.1]; 5000];
        pts_fail[777] = [20.0, 0.0, 0.0];
        pts_fail[4321] = [30.0, 0.0, 0.0];
        let ps_fail = PointSet::from_points(f1, &pts_fail).unwrap();
        let cad_fail = FailAtCad { frame: f1 };
        let err_worker = deviation_field(
            &ps_fail,
            None,
            NominalRef::Cad(&cad_fail),
            &ok_params,
            ok_prov.clone(),
            BatchConfig { threads: 4, block: 64 },
            &cancel,
        )
        .unwrap_err();
        assert_eq!(err_worker, DevError::ProjectionFailed { reason: "idx_777" });

        let mut normals_fail = vec![[0.0, 0.0, 1.0]; 5000];
        normals_fail[777] = [0.0, 0.0, 0.0];
        normals_fail[4321] = [0.0, 0.0, 0.0];
        let err_idx = deviation_field(
            &ps_fail,
            Some(&normals_fail),
            NominalRef::Cad(&cad_fail),
            &ok_params,
            ok_prov,
            BatchConfig { threads: 4, block: 64 },
            &cancel,
        )
        .unwrap_err();
        assert_eq!(err_idx, DevError::InvalidNormal { index: Some(777) });
    }
}
