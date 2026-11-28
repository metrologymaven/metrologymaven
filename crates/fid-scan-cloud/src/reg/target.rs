//! Oriented-cloud projection target, overlap fraction and stride sampling. Spec node
//! `SCAN.CLOUD.REG.CTGT`.
//!
//! [`OrientedCloudTarget`] lets ALN's best-fit loop (and every other GEO.DIST.BATCH caller)
//! use an oriented point cloud as the reference: a query point is matched with the nearest
//! cloud point that has a valid normal and projected onto that point's tangent plane
//! (point-to-plane). [`overlap_fraction`] is the one definition of scan overlap that the pair
//! command, the pair graph and the report share, and [`stride_sample`] is the one sampling
//! rule every `SCAN.CLOUD.REG` child uses.
//!
//! # Construction (pinned, first failure wins)
//!
//! [`OrientedCloudTarget::new`]`(cloud, valid)`:
//!
//! 1. `valid.len() == cloud.len()`, else `scan.cloud.invalid_parameter` (`valid`);
//! 2. the cloud has a normal channel, else `scan.cloud.no_normals` (`no_normal_channel`);
//! 3. at least one bit of `valid` is set, else `scan.cloud.no_normals` (`none_marked_valid`);
//! 4. every valid point is within [`COORD_LIMIT`] = 2^250 m on every axis, else
//!    `scan.cloud.invalid_parameter` (`cloud`), so every difference, product and sum below is
//!    finite;
//! 5. the GEO.INDEX.KDTREE tree is built over the valid points only, in ascending original
//!    index order (`scan.cloud.index_failed` carries any error of the build).
//!
//! The normals are used exactly as stored. GEO.TYPES.POINTS guarantees `| |n| - 1 | <= 1e-9`
//! and never normalises, so neither does this module; a signed distance therefore carries a
//! relative error of up to about `1e-9` from the normal's length, on top of rounding. Points
//! whose bit is clear take no part in anything: they are never matched, and they do not count
//! as target points for the overlap fraction either.
//!
//! # Closest-point projection (pinned, REQ-001)
//!
//! For a query `p` (finite, within `COORD_LIMIT`; else GEO.DIST's `non_finite_query` or
//! `query_out_of_range`):
//!
//! 1. `q` = the valid point with the smallest `(dist2(p, q), original index)`, where `dist2 =
//!    (dx*dx + dy*dy) + dz*dz` with `dx = p.x - q.x` (the GEO.INDEX.KDTREE pinned value), so a
//!    tie goes to the lowest original index;
//! 2. with a maximum distance `m`: `NoHit` ([`NoHitReason::BeyondMaxDistance`]) when
//!    `sqrt(dist2) > m` (`fid_num::f::sqrt`, correctly rounded): no valid point lies within
//!    `m` of `p`;
//! 3. `s = (dx*nx + dy*ny) + dz*nz` with `d = p - q` and `n` the normal of `q`; a `-0.0` is
//!    stored as `+0.0` (`s + 0.0`), so a query on the tangent plane has one encoding;
//! 4. `foot[k] = p[k] - s * n[k]`; `NoHit` ([`NoHitReason::IntersectionOutOfRange`]) when the
//!    foot is beyond `COORD_LIMIT` on any axis (possible for a far query and a tilted plane;
//!    every GEO.DIST foot stays within the limit);
//! 5. otherwise the hit is `foot`, `distance = |s|`, `signed_distance = s` (positive on the
//!    side the normal points to), sign [`HitSign::PointNormal`], `entity_id` = the original
//!    index of `q`, no surface parameters.
//!
//! # Projection along a direction (pinned)
//!
//! For a query `p` and a direction `v` (finite, within `COORD_LIMIT`, not zero; else
//! `invalid_direction`):
//!
//! 1. `q`, `n`, `d` and `s` as above, and the same `NoHit` when `q` is beyond `m`;
//! 2. `w` = `v` scaled up by exact powers of two (by `2^600` while `max |w_k| < 2^-500`, then
//!    by 2 while `max |w_k| < 1`), which keeps the line and avoids underflow in `w . n`;
//! 3. `vn = (w0*n0 + w1*n1) + w2*n2`; `NoHit` ([`NoHitReason::ParallelToTangentPlane`]) when
//!    `vn == 0` (the line is parallel to the plane, even when it lies in it);
//! 4. `t = -s / vn`, `|w| = sqrt((w0*w0 + w1*w1) + w2*w2)`, `foot[k] = p[k] + t * w[k]`,
//!    `distance = |t| * |w|`, `signed_distance = t * |w| + 0.0` (positive along `+v`), sign
//!    [`HitSign::Along`] (as for every GEO.DIST directional projection), `entity_id` as above;
//! 5. `NoHit` ([`NoHitReason::IntersectionOutOfRange`]) when the foot is not finite or beyond
//!    `COORD_LIMIT`, or the distance is not finite; `NoHit`
//!    ([`NoHitReason::AlongBeyondMaxDistance`]) when `distance > m`.
//!
//! A cloud has no triangles or faces, so any `subset` option is refused with GEO.DIST's
//! `subset_mismatch` rather than ignored. The [`ProjectionTarget`] methods return
//! [`Projection`], whose `NoHit` carries no reason; [`OrientedCloudTarget::closest`] and
//! [`OrientedCloudTarget::along`] return the same result with the [`NoHitReason`].
//!
//! # Determinism (REQ-005)
//!
//! Each projection is a function of the query, the target and the options only, so
//! `fid_geo_dist::project_batch` gives the same bytes for every thread count and block size.
//! [`overlap_fraction_with`] splits the sample into contiguous ranges, one per thread, and adds
//! integer counts, so its result does not depend on the thread count either; when several
//! sampled points fail, the error of the lowest sample position is returned.
//!
//! # Cost
//!
//! Building copies the valid points, their normals and original indexes (56 bytes per valid
//! point, plus 4 for the tree's permutation).
//! Each query is one GEO.INDEX.KDTREE nearest-neighbour search: logarithmic for scan data, but
//! linear in the worst case for adversarial layouts (for example every point at the same
//! distance from the query), as for any exact nearest-neighbour index.

pub use super::error::{NoNormalsReason, RegError};
pub use super::mask::Bitmask;
pub use super::overlap::{overlap_fraction, overlap_fraction_with, stride_sample, Overlap, StrideSample, FLOOR_FACTOR};

pub use fid_geo_dist::batch::MAX_THREADS;
pub use fid_geo_dist::mesh::COORD_LIMIT;

use fid_geo_dist::{
    DistError, HitSign, ProjectError, Projection, ProjectionHit, ProjectionOptions, ProjectionTarget, SignError,
};
use fid_geo_index::kdtree::{PointIndexError, PointTree};
use fid_geo_types::{FrameRef, PointBuffer, PointCloud};

type V3 = [f64; 3];

/// Why a projection found nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoHitReason {
    /// No valid point lies within the maximum distance of the query.
    BeyondMaxDistance,
    /// The direction is parallel to the tangent plane of the nearest valid point.
    ParallelToTangentPlane,
    /// The foot on the tangent plane (closest point or along a direction) lies beyond
    /// `COORD_LIMIT` (or overflows).
    IntersectionOutOfRange,
    /// The intersection along the direction lies beyond the maximum distance.
    AlongBeyondMaxDistance,
}

impl NoHitReason {
    /// A stable machine-readable reason.
    pub fn code(self) -> &'static str {
        match self {
            NoHitReason::BeyondMaxDistance => "beyond_max_distance",
            NoHitReason::ParallelToTangentPlane => "parallel_to_tangent_plane",
            NoHitReason::IntersectionOutOfRange => "intersection_out_of_range",
            NoHitReason::AlongBeyondMaxDistance => "along_beyond_max_distance",
        }
    }
}

/// A projection onto an [`OrientedCloudTarget`], with the reason when nothing was found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CloudProjection {
    /// A foot point.
    Hit(ProjectionHit),
    /// Nothing, and why.
    NoHit(NoHitReason),
}

impl CloudProjection {
    /// The GEO.DIST.BATCH result (the reason is dropped).
    pub fn projection(self) -> Projection {
        match self {
            CloudProjection::Hit(h) => Projection::Hit(h),
            CloudProjection::NoHit(_) => Projection::NoHit,
        }
    }
}

/// The nearest valid point of a query.
pub(crate) struct Nearest {
    pub(crate) original: u64,
    pub(crate) point: V3,
    pub(crate) normal: V3,
    pub(crate) dist2: f64,
}

/// An oriented point cloud as a GEO.DIST [`ProjectionTarget`]: point-to-plane against the
/// nearest point with a valid normal. See the [module documentation](self) for the pinned
/// rules. The fields are private, so a target always satisfies the construction checks.
#[derive(Debug, Clone, PartialEq)]
pub struct OrientedCloudTarget {
    frame: FrameRef,
    points: PointBuffer,
    normals: Vec<V3>,
    original: Vec<u64>,
    tree: PointTree,
    source_len: u64,
    coord_scale: f64,
}

pub(crate) fn dot3(a: &V3, b: &V3) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

pub(crate) fn within_limit(p: &V3) -> bool {
    p.iter().all(|x| x.is_finite() && x.abs() <= COORD_LIMIT)
}

/// The largest `|x_k|` (finite inputs; total-order comparison).
pub(crate) fn max_abs(p: &V3) -> f64 {
    let mut m = 0.0f64;
    for x in p {
        let a = x.abs();
        if a.total_cmp(&m).is_gt() {
            m = a;
        }
    }
    m
}

fn alloc_err<T>(n: usize) -> RegError {
    RegError::AllocationFailed {
        bytes: u64::try_from(n)
            .unwrap_or(u64::MAX)
            .saturating_mul(u64::try_from(core::mem::size_of::<T>()).unwrap_or(8)),
    }
}

fn index_error(e: PointIndexError) -> ProjectError {
    match e {
        PointIndexError::NonFiniteQuery { .. } => DistError::NonFiniteQuery.into(),
        PointIndexError::QueryOutOfRange { .. } => DistError::QueryOutOfRange.into(),
        PointIndexError::AllocationFailed { bytes } => ProjectError::Sign(SignError::AllocationFailed { bytes }),
        _ => ProjectError::Sign(SignError::Inconsistent),
    }
}

fn check_query(p: &V3) -> Result<(), ProjectError> {
    if !p.iter().all(|x| x.is_finite()) {
        return Err(DistError::NonFiniteQuery.into());
    }
    if !p.iter().all(|x| x.abs() <= COORD_LIMIT) {
        return Err(DistError::QueryOutOfRange.into());
    }
    Ok(())
}

fn check_opts(opts: &ProjectionOptions<'_, '_>) -> Result<Option<f64>, ProjectError> {
    if opts.subset.is_some() {
        return Err(DistError::SubsetMismatch.into());
    }
    match opts.max_distance {
        Some(m) if !(m.is_finite() && m >= 0.0) => Err(DistError::InvalidMaxDistance.into()),
        m => Ok(m),
    }
}

fn check_direction(v: &V3) -> Result<(), ProjectError> {
    if within_limit(v) && v.iter().any(|&x| x != 0.0) {
        Ok(())
    } else {
        Err(ProjectError::InvalidDirection)
    }
}

/// `v` scaled up by exact powers of two until `max |v_k| >= 1` (step 2 of the pinned rule).
fn scale_up(v: &V3) -> V3 {
    const BIG: f64 = f64::from_bits((1023 + 600) << 52);
    const SMALL: f64 = f64::from_bits((1023 - 500) << 52);
    let mut w = *v;
    let mut m = max_abs(&w);
    // Each loop runs a bounded number of times for a non-zero finite input (m >= 2^-1074).
    while m > 0.0 && m < SMALL {
        w = [w[0] * BIG, w[1] * BIG, w[2] * BIG];
        m *= BIG;
    }
    while m > 0.0 && m < 1.0 {
        w = [w[0] * 2.0, w[1] * 2.0, w[2] * 2.0];
        m *= 2.0;
    }
    w
}

impl OrientedCloudTarget {
    /// A target over the points of `cloud` whose bit in `valid` is set (see the
    /// [module documentation](self) for the order of checks).
    pub fn new(cloud: &PointCloud, valid: &Bitmask) -> Result<OrientedCloudTarget, RegError> {
        let n = cloud.len();
        if valid.len() != n {
            return Err(RegError::invalid(
                "valid",
                format!("the mask has {} bits for a cloud of {n} points", valid.len()),
            ));
        }
        let attrs = cloud.attributes();
        if attrs.normal.is_none() {
            return Err(RegError::NoNormals { reason: NoNormalsReason::NoNormalChannel });
        }
        let count = valid.count_ones();
        if count == 0 {
            return Err(RegError::NoNormals { reason: NoNormalsReason::NoneMarkedValid });
        }
        let cap = usize::try_from(count).map_err(|_| alloc_err::<V3>(usize::MAX))?;
        let mut xs: Vec<f64> = Vec::new();
        let mut ys: Vec<f64> = Vec::new();
        let mut zs: Vec<f64> = Vec::new();
        let mut normals: Vec<V3> = Vec::new();
        let mut original: Vec<u64> = Vec::new();
        for col in [&mut xs, &mut ys, &mut zs] {
            col.try_reserve_exact(cap).map_err(|_| alloc_err::<f64>(cap))?;
        }
        normals.try_reserve_exact(cap).map_err(|_| alloc_err::<V3>(cap))?;
        original.try_reserve_exact(cap).map_err(|_| alloc_err::<u64>(cap))?;
        let mut coord_scale = 0.0f64;
        for i in 0..n {
            if !valid.get(i) {
                continue;
            }
            let (Some(p), Some(nv)) = (cloud.point(i), attrs.normal_at(i)) else {
                return Err(RegError::Inconsistent);
            };
            if !within_limit(&p) {
                return Err(RegError::invalid("cloud", format!("valid point {i} lies beyond 2^250 m")));
            }
            let a = max_abs(&p);
            if a.total_cmp(&coord_scale).is_gt() {
                coord_scale = a;
            }
            xs.push(p[0]);
            ys.push(p[1]);
            zs.push(p[2]);
            normals.push(nv);
            original.push(i);
        }
        let points = PointBuffer::new(xs, ys, zs);
        let tree = PointTree::build(&points, 1)?;
        Ok(OrientedCloudTarget { frame: cloud.frame(), points, normals, original, tree, source_len: n, coord_scale })
    }

    /// The number of points of the source cloud.
    pub fn source_len(&self) -> u64 {
        self.source_len
    }

    /// The number of valid points (the points that can be matched).
    pub fn valid_len(&self) -> u64 {
        u64::try_from(self.original.len()).unwrap_or(u64::MAX)
    }

    /// The largest coordinate magnitude of the valid points, in metres.
    pub fn coord_scale(&self) -> f64 {
        self.coord_scale
    }

    /// The stored normal of the source point `entity_id` (an original index, as in
    /// [`ProjectionHit::entity_id`]), or `None` when that point is not a valid point of this
    /// target. The correspondence normal for ALN.BFIT.ICP.CORR.
    pub fn normal_at(&self, entity_id: u64) -> Option<V3> {
        let k = self.original.binary_search(&entity_id).ok()?;
        self.normals.get(k).copied()
    }

    /// The nearest valid point to `p` by `(dist2, original index)`; `p` must be checked.
    pub(crate) fn nearest(&self, p: V3) -> Result<Nearest, PointIndexError> {
        let hits = self.tree.knn(&self.points, p, 1)?;
        let first = hits.first().ok_or(PointIndexError::Inconsistent)?;
        let k = usize::try_from(first.index).map_err(|_| PointIndexError::Inconsistent)?;
        let (Some(point), Some(&normal), Some(&original)) =
            (self.points.point(first.index), self.normals.get(k), self.original.get(k))
        else {
            return Err(PointIndexError::Inconsistent);
        };
        Ok(Nearest { original, point, normal, dist2: first.dist2 })
    }

    /// The nearest valid point within `max` (`None` when beyond it).
    fn matched(&self, p: V3, max: Option<f64>) -> Result<Option<Nearest>, ProjectError> {
        let near = self.nearest(p).map_err(index_error)?;
        if let Some(m) = max {
            let dist = fid_num::f::sqrt(near.dist2).map_err(|_| ProjectError::Sign(SignError::Inconsistent))?;
            if dist > m {
                return Ok(None);
            }
        }
        Ok(Some(near))
    }

    /// The closest-point projection of `p` (pinned rule in the [module documentation](self));
    /// `opts.direction` is ignored.
    pub fn closest(&self, p: V3, opts: &ProjectionOptions<'_, '_>) -> Result<CloudProjection, ProjectError> {
        check_query(&p)?;
        let max = check_opts(opts)?;
        let Some(near) = self.matched(p, max)? else {
            return Ok(CloudProjection::NoHit(NoHitReason::BeyondMaxDistance));
        };
        let (q, n) = (near.point, near.normal);
        let d = [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
        let s = dot3(&d, &n) + 0.0;
        let foot = [p[0] - s * n[0], p[1] - s * n[1], p[2] - s * n[2]];
        if !within_limit(&foot) {
            return Ok(CloudProjection::NoHit(NoHitReason::IntersectionOutOfRange));
        }
        Ok(CloudProjection::Hit(ProjectionHit {
            foot,
            distance: s.abs(),
            signed_distance: s,
            sign: HitSign::PointNormal,
            entity_id: near.original,
            params: None,
        }))
    }

    /// The intersection of the line `p + t v` with the tangent plane of the nearest valid
    /// point (pinned rule in the [module documentation](self)).
    pub fn along(&self, p: V3, v: V3, opts: &ProjectionOptions<'_, '_>) -> Result<CloudProjection, ProjectError> {
        check_query(&p)?;
        check_direction(&v)?;
        let max = check_opts(opts)?;
        let Some(near) = self.matched(p, max)? else {
            return Ok(CloudProjection::NoHit(NoHitReason::BeyondMaxDistance));
        };
        let (q, n) = (near.point, near.normal);
        let d = [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
        let s = dot3(&d, &n);
        let w = scale_up(&v);
        let vn = dot3(&w, &n);
        if vn == 0.0 {
            return Ok(CloudProjection::NoHit(NoHitReason::ParallelToTangentPlane));
        }
        let t = -s / vn;
        let wlen = fid_num::f::sqrt(dot3(&w, &w)).map_err(|_| ProjectError::Sign(SignError::Inconsistent))?;
        let foot = [p[0] + t * w[0], p[1] + t * w[1], p[2] + t * w[2]];
        let distance = t.abs() * wlen;
        if !(within_limit(&foot) && distance.is_finite()) {
            return Ok(CloudProjection::NoHit(NoHitReason::IntersectionOutOfRange));
        }
        if max.is_some_and(|m| distance > m) {
            return Ok(CloudProjection::NoHit(NoHitReason::AlongBeyondMaxDistance));
        }
        Ok(CloudProjection::Hit(ProjectionHit {
            foot,
            distance,
            signed_distance: t * wlen + 0.0,
            sign: HitSign::Along,
            entity_id: near.original,
            params: None,
        }))
    }
}

impl ProjectionTarget for OrientedCloudTarget {
    fn frame(&self) -> FrameRef {
        self.frame
    }

    fn check_options(&self, opts: &ProjectionOptions<'_, '_>) -> Result<(), ProjectError> {
        check_opts(opts).map(|_| ())
    }

    fn project(&self, p: V3, opts: &ProjectionOptions<'_, '_>) -> Result<Projection, ProjectError> {
        self.closest(p, opts).map(CloudProjection::projection)
    }

    fn project_along(&self, p: V3, v: V3, opts: &ProjectionOptions<'_, '_>) -> Result<Projection, ProjectError> {
        self.along(p, v, opts).map(CloudProjection::projection)
    }
}
