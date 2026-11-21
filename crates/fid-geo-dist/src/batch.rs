//! Deterministic batch projection and projection options. Spec node `GEO.DIST.BATCH`.
//!
//! [`project_batch`] projects every point of a [`PointSet`] onto a [`ProjectionTarget`] on
//! `threads` threads and returns one [`Projection`] per point, in input order.
//! [`summary`] reduces the results to a [`BatchSummary`]. The mesh target is
//! [`SignTarget`] (GEO.DIST.MESH closest point with the GEO.DIST.SIGN sign of record).
//! GEO.DIST.FACE implements the same trait for CAD faces and bodies.
//!
//! # Determinism (pinned, REQ-001, REQ-004)
//!
//! Each point's result is a function of that point, the target and the options only. No
//! state is shared between points. The input is cut into blocks of `block` points (a
//! multiple of [`BLOCK_QUANTUM`]). Workers take blocks from a shared queue in any order and
//! write each result into its pre-allocated slot by input index. Neither the thread count,
//! the block size nor the schedule can change a slot's bytes. Statistics are reduced after
//! all blocks finish, on one thread, in input order: min and max with `fid_num::f`
//! total-order comparisons, the mean with `fid_num::red::mean` (fixed block-and-tree sum).
//! When several points fail, the error of the lowest input index is returned. Blocks
//! after a known failure may be skipped, but every block before it runs to completion, so
//! the reported index does not depend on scheduling either.
//!
//! # Order of checks (pinned)
//!
//! 1. Frames: `require_same_frame_ref(points, target)` ([`BatchError::FrameMismatch`]),
//!    before anything else, so no point is touched (REQ-002).
//! 2. The configuration: `1 <= threads <= MAX_THREADS`, and a block size that is a positive
//!    multiple of 64 and at most [`MAX_BLOCK`].
//! 3. The options: `max_distance` finite and `>= 0`; `direction` finite, within
//!    [`COORD_LIMIT`] and not zero.
//! 4. Every point, in index order: finite ([`BatchError::NonFiniteQuery`]) and within
//!    [`COORD_LIMIT`] ([`BatchError::QueryOutOfRange`]).
//! 5. The result buffer is allocated with `try_reserve` ([`BatchError::AllocationFailed`]).
//!
//! Then the blocks run. Each worker reads the cancellation flag before it takes a block.
//! If the flag is set at any point before the batch returns, the result is
//! [`BatchError::Cancelled`], and no partial results are returned (REQ-006).
//!
//! # Projection along a direction (pinned, REQ-003)
//!
//! For a direction `v`, the mesh target returns the nearest intersection of the whole line
//! `p + s v` (`s` of either sign) with the closed triangles. It is computed as follows:
//!
//! 1. `v` is scaled by exact powers of two (up only: by `2^600` while `max |v_k| < 2^-500`,
//!    then by 2 while `max |v_k| < 4`). This keeps the line unchanged and makes every
//!    intersection parameter at most `2^249` in magnitude.
//! 2. Candidates are `PrimitiveBvh::ray(p, v, -COORD_LIMIT, COORD_LIMIT)`, which is
//!    conservative and ascending. The candidate set does not depend on `max_distance`.
//! 3. Each candidate gets the watertight ray–triangle test of Woop, Benthin and Wald (JCGT
//!    2(1), 2013; our own code): `kz` is the axis of the largest `|v_k|` (lowest index on
//!    ties), `kx = kz + 1`, `ky = kx + 1` (mod 3), swapped when `v_kz < 0`;
//!    `sx = v_kx / v_kz`, `sy = v_ky / v_kz`, `sz = 1 / v_kz`. For each corner `q`, with
//!    `d = q - p`: `qx = d_kx - sx d_kz` and `qy = d_ky - sy d_kz`. Then
//!    `u = cx by - cy bx`, `w1 = ax cy - ay cx`, `w2 = bx ay - by ax`. The line misses when
//!    some of `u, w1, w2` are `< 0` and some are `> 0`, or when `det = (u + w1) + w2` is 0.
//!    Otherwise `t = ((u (sz a_kz) + w1 (sz b_kz)) + w2 (sz c_kz)) / det`. Shared edges
//!    and vertices are tested with the same sheared values from both sides, so a line
//!    through them is never lost between two triangles.
//! 4. The smallest `|t|` wins, ties going to the lowest triangle index. The distance is
//!    `|t| * |v|`, where `|v| = sqrt(v . v)` is correctly rounded. The result is `NoHit`
//!    when that distance exceeds `max_distance`. The foot is `p_k + t v_k` per axis, and
//!    the signed distance is `t * |v|` (positive along `+v`), with [`HitSign::Along`].
//!
//! The subset option restricts the triangles in both modes.

use crate::mesh::{check_query, corners, DistError, MeshProjection, MeshQuery, TriangleSubset, COORD_LIMIT};
use crate::sign::{SignError, SignStatus, SignTarget};
use core::fmt;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use fid_geo_frame::{require_same_frame_ref, GeoError};
use fid_geo_types::{FrameRef, PointSet};
use std::sync::Mutex;

/// The default block size (points per block).
pub const DEFAULT_BLOCK: usize = 4096;
/// Block sizes must be positive multiples of this.
pub const BLOCK_QUANTUM: usize = 64;
/// The largest accepted block size.
pub const MAX_BLOCK: usize = 1 << 20;
/// The largest accepted thread count.
pub const MAX_THREADS: usize = 256;
/// Bytes per record of [`Projection::write_bytes`].
pub const RECORD_BYTES: usize = 67;
/// The sign code [`Projection::write_bytes`] writes for [`HitSign::Along`].
pub const ALONG_SIGN_CODE: u8 = 254;

type V3 = [f64; 3];

// ------------------------------------------------------------------ types

/// The sign carried by a hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitSign {
    /// The GEO.DIST.SIGN status of a closest-point projection.
    Mesh(SignStatus),
    /// A projection along a direction: the sign is the side of `p` along `v`.
    Along,
}

/// One projected point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectionHit {
    /// The foot point.
    pub foot: V3,
    /// The unsigned distance from the query to the foot.
    pub distance: f64,
    /// The signed distance: the GEO.DIST.SIGN distance, or `t |v|` along a direction.
    pub signed_distance: f64,
    /// Where the sign comes from.
    pub sign: HitSign,
    /// The target entity hit (the triangle index for a mesh).
    pub entity_id: u64,
    /// Surface parameters of the foot, for targets that have them (`None` for meshes).
    pub params: Option<(f64, f64)>,
}

impl ProjectionHit {
    /// True when `signed_distance` carries a sign of record.
    pub fn sign_defined(&self) -> bool {
        match self.sign {
            HitSign::Mesh(s) => matches!(s, SignStatus::Outside | SignStatus::Inside | SignStatus::OnSurface),
            HitSign::Along => true,
        }
    }
}

/// The result for one point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projection {
    /// A foot point.
    Hit(ProjectionHit),
    /// Nothing (allowed) within the maximum distance.
    NoHit,
}

impl Projection {
    /// Appends the pinned [`RECORD_BYTES`]-byte record: tag `u8` (0 no hit, 1 hit), entity
    /// `u64` LE, foot x y z, distance and signed distance as `f64` LE bit patterns, sign
    /// code `u8` (`SignStatus::code`, or [`ALONG_SIGN_CODE`]), params flag `u8` and two
    /// `f64`. A no-hit record is the tag followed by zeros.
    pub fn write_bytes(&self, out: &mut Vec<u8>) {
        match self {
            Projection::NoHit => out.extend_from_slice(&[0u8; RECORD_BYTES]),
            Projection::Hit(h) => {
                out.push(1);
                out.extend_from_slice(&h.entity_id.to_le_bytes());
                for x in [h.foot[0], h.foot[1], h.foot[2], h.distance, h.signed_distance] {
                    out.extend_from_slice(&x.to_bits().to_le_bytes());
                }
                out.push(match h.sign {
                    HitSign::Mesh(s) => s.code(),
                    HitSign::Along => ALONG_SIGN_CODE,
                });
                let (flag, (u, v)) = match h.params {
                    Some(uv) => (1u8, uv),
                    None => (0u8, (0.0, 0.0)),
                };
                out.push(flag);
                out.extend_from_slice(&u.to_bits().to_le_bytes());
                out.extend_from_slice(&v.to_bits().to_le_bytes());
            }
        }
    }
}

/// The records of `results` in order ([`Projection::write_bytes`]).
pub fn encode(results: &[Projection]) -> Result<Vec<u8>, BatchError> {
    let bytes = results.len().saturating_mul(RECORD_BYTES);
    let mut out = Vec::new();
    out.try_reserve_exact(bytes).map_err(|_| BatchError::AllocationFailed { bytes: bytes as u64 })?;
    for r in results {
        r.write_bytes(&mut out);
    }
    Ok(out)
}

/// Projection options, uniform for every target kind.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProjectionOptions<'s, 'a> {
    /// Maximum distance in metres (finite, `>= 0`); `None` searches without limit.
    pub max_distance: Option<f64>,
    /// Restrict the search to these entities (triangles of a mesh; faces of a body).
    pub subset: Option<&'s TriangleSubset<'a>>,
    /// Project along `+-direction` instead of to the closest point.
    pub direction: Option<V3>,
}

/// An error of one projection.
#[derive(Debug, Clone, PartialEq)]
pub enum ProjectError {
    /// An error of the closest-point search or the sign.
    Sign(SignError),
    /// The direction is non-finite, beyond [`COORD_LIMIT`] or zero.
    InvalidDirection,
}

impl From<SignError> for ProjectError {
    fn from(e: SignError) -> ProjectError {
        ProjectError::Sign(e)
    }
}

impl From<DistError> for ProjectError {
    fn from(e: DistError) -> ProjectError {
        ProjectError::Sign(SignError::Dist(e))
    }
}

impl ProjectError {
    /// A stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            ProjectError::Sign(e) => e.code(),
            ProjectError::InvalidDirection => "invalid_direction",
        }
    }
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectError::Sign(e) => write!(f, "{e}"),
            ProjectError::InvalidDirection => write!(f, "direction must be finite, within 2^250 and not zero"),
        }
    }
}

impl std::error::Error for ProjectError {}

/// A geometry points can be projected onto.
pub trait ProjectionTarget: Sync {
    /// The frame the target's coordinates are in.
    fn frame(&self) -> FrameRef;
    /// The closest point to `p` under `opts` (its `direction` is ignored).
    fn project(&self, p: V3, opts: &ProjectionOptions<'_, '_>) -> Result<Projection, ProjectError>;
    /// The nearest intersection of the line `p + s v` with the target under `opts`.
    fn project_along(&self, p: V3, v: V3, opts: &ProjectionOptions<'_, '_>) -> Result<Projection, ProjectError>;
}

/// Thread count and block size of a batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchConfig {
    /// Worker threads, `1..=MAX_THREADS` (1 runs on the calling thread).
    pub threads: usize,
    /// Points per block, a positive multiple of [`BLOCK_QUANTUM`], at most [`MAX_BLOCK`].
    pub block: usize,
}

impl Default for BatchConfig {
    fn default() -> Self {
        BatchConfig { threads: 1, block: DEFAULT_BLOCK }
    }
}

/// Every error of a batch. Errors are values; nothing panics.
#[derive(Debug, Clone, PartialEq)]
pub enum BatchError {
    /// The points and the target are in different frames (GEO.FRAME). No point was projected.
    FrameMismatch(GeoError),
    /// The cancellation flag was set. No results are returned.
    Cancelled,
    /// A query coordinate is NaN or infinite.
    NonFiniteQuery {
        /// The first such point.
        index: u64,
    },
    /// A query coordinate exceeds [`COORD_LIMIT`].
    QueryOutOfRange {
        /// The first such point.
        index: u64,
    },
    /// `max_distance` is NaN, infinite or negative.
    InvalidMaxDistance,
    /// The direction is non-finite, beyond [`COORD_LIMIT`] or zero.
    InvalidDirection,
    /// The block size is zero, not a multiple of [`BLOCK_QUANTUM`] or above [`MAX_BLOCK`].
    InvalidBlockSize {
        /// The block size given.
        block: usize,
    },
    /// The thread count is zero or above [`MAX_THREADS`].
    InvalidThreads {
        /// The thread count given.
        threads: usize,
    },
    /// The result buffer could not be allocated.
    AllocationFailed {
        /// Bytes requested.
        bytes: u64,
    },
    /// The projection of one point failed (the lowest such index).
    Point {
        /// The point index.
        index: u64,
        /// The error.
        error: ProjectError,
    },
    /// A worker thread or lock failed (never expected).
    WorkerFailed,
}

impl BatchError {
    /// A stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            BatchError::FrameMismatch(e) => e.code(),
            BatchError::Cancelled => "cancelled",
            BatchError::NonFiniteQuery { .. } => "non_finite_query",
            BatchError::QueryOutOfRange { .. } => "query_out_of_range",
            BatchError::InvalidMaxDistance => "invalid_max_distance",
            BatchError::InvalidDirection => "invalid_direction",
            BatchError::InvalidBlockSize { .. } => "invalid_block_size",
            BatchError::InvalidThreads { .. } => "invalid_threads",
            BatchError::AllocationFailed { .. } => "allocation_failed",
            BatchError::Point { error, .. } => error.code(),
            BatchError::WorkerFailed => "worker_failed",
        }
    }
}

impl fmt::Display for BatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BatchError::FrameMismatch(e) => write!(f, "{e}"),
            BatchError::Cancelled => write!(f, "batch cancelled"),
            BatchError::NonFiniteQuery { index } => write!(f, "query point {index} is not finite"),
            BatchError::QueryOutOfRange { index } => write!(f, "query point {index} exceeds 2^250 m"),
            BatchError::InvalidMaxDistance => write!(f, "maximum distance must be finite and >= 0"),
            BatchError::InvalidDirection => write!(f, "direction must be finite, within 2^250 and not zero"),
            BatchError::InvalidBlockSize { block } => {
                write!(f, "block size {block} is not a positive multiple of 64 up to 2^20")
            }
            BatchError::InvalidThreads { threads } => write!(f, "thread count {threads} is not in 1..=256"),
            BatchError::AllocationFailed { bytes } => write!(f, "allocation of {bytes} bytes failed"),
            BatchError::Point { index, error } => write!(f, "point {index}: {error}"),
            BatchError::WorkerFailed => write!(f, "a worker thread failed"),
        }
    }
}

impl std::error::Error for BatchError {}

// ------------------------------------------------------------------ mesh target

fn check_direction(v: &V3) -> Result<(), ProjectError> {
    let ok = v.iter().all(|x| x.is_finite() && (-COORD_LIMIT..=COORD_LIMIT).contains(x)) && v.iter().any(|&x| x != 0.0);
    if ok {
        Ok(())
    } else {
        Err(ProjectError::InvalidDirection)
    }
}

fn check_max(d: Option<f64>) -> Result<(), DistError> {
    match d {
        Some(x) if !(x.is_finite() && x >= 0.0) => Err(DistError::InvalidMaxDistance),
        _ => Ok(()),
    }
}

fn max_abs(v: &V3) -> f64 {
    let mut m = 0.0;
    for x in v {
        let a = x.abs();
        if a > m {
            m = a;
        }
    }
    m
}

/// `v` scaled up by exact powers of two until `max |v_k| >= 4` (step 1 of the pinned rule).
fn scale_up(v: &V3) -> V3 {
    const BIG: f64 = f64::from_bits((1023 + 600) << 52);
    const SMALL: f64 = f64::from_bits((1023 - 500) << 52);
    let mut w = *v;
    let mut m = max_abs(&w);
    // Each loop runs at most a few times for any non-zero finite input (m >= 2^-1074).
    while m > 0.0 && m < SMALL {
        w = [w[0] * BIG, w[1] * BIG, w[2] * BIG];
        m *= BIG;
    }
    while m > 0.0 && m < 4.0 {
        w = [w[0] * 2.0, w[1] * 2.0, w[2] * 2.0];
        m *= 2.0;
    }
    w
}

/// The pinned sheared frame of a line direction.
struct Shear {
    kx: usize,
    ky: usize,
    kz: usize,
    sx: f64,
    sy: f64,
    sz: f64,
}

fn shear(v: &V3) -> Shear {
    let (a0, a1, a2) = (v[0].abs(), v[1].abs(), v[2].abs());
    let kz = if a2 > a1 && a2 > a0 {
        2
    } else if a1 > a0 {
        1
    } else {
        0
    };
    let mut kx = (kz + 1) % 3;
    let mut ky = (kx + 1) % 3;
    if v[kz] < 0.0 {
        core::mem::swap(&mut kx, &mut ky);
    }
    Shear { kx, ky, kz, sx: v[kx] / v[kz], sy: v[ky] / v[kz], sz: 1.0 / v[kz] }
}

/// The watertight line–triangle test: the line parameter `t` of the intersection of
/// `p + t v` with the closed triangle `(a, b, c)`, or `None`.
fn line_triangle(p: &V3, s: &Shear, tri: &[V3; 3]) -> Option<f64> {
    let d = |q: &V3| [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
    let (da, db, dc) = (d(&tri[0]), d(&tri[1]), d(&tri[2]));
    let xy = |q: &V3| (q[s.kx] - s.sx * q[s.kz], q[s.ky] - s.sy * q[s.kz]);
    let (ax, ay) = xy(&da);
    let (bx, by) = xy(&db);
    let (cx, cy) = xy(&dc);
    let u = cx * by - cy * bx;
    let w1 = ax * cy - ay * cx;
    let w2 = bx * ay - by * ax;
    let neg = u < 0.0 || w1 < 0.0 || w2 < 0.0;
    let pos = u > 0.0 || w1 > 0.0 || w2 > 0.0;
    if neg && pos {
        return None;
    }
    let det = (u + w1) + w2;
    if det == 0.0 {
        return None;
    }
    let (az, bz, cz) = (s.sz * da[s.kz], s.sz * db[s.kz], s.sz * dc[s.kz]);
    let t = ((u * az + w1 * bz) + w2 * cz) / det;
    if t.is_finite() {
        Some(t)
    } else {
        None
    }
}

impl SignTarget<'_> {
    /// [`ProjectionTarget::project_along`] by scanning every (allowed) triangle in index
    /// order without the hierarchy. The reference for tests.
    pub fn project_along_brute(
        &self,
        p: V3,
        v: V3,
        opts: &ProjectionOptions<'_, '_>,
    ) -> Result<Projection, ProjectError> {
        self.along(p, v, opts, true)
    }

    fn along(&self, p: V3, v: V3, opts: &ProjectionOptions<'_, '_>, brute: bool) -> Result<Projection, ProjectError> {
        check_query(&p)?;
        check_direction(&v)?;
        check_max(opts.max_distance)?;
        let target = self.target();
        let mesh = target.mesh();
        if let Some(s) = opts.subset {
            if s.triangle_count() != mesh.triangle_count() {
                return Err(DistError::SubsetMismatch.into());
            }
        }
        let w = scale_up(&v);
        let sh = shear(&w);
        let wlen = fid_num::f::sqrt((w[0] * w[0] + w[1] * w[1]) + w[2] * w[2])
            .map_err(|_| ProjectError::Sign(SignError::Inconsistent))?;
        let candidates: Vec<u64> = if brute {
            (0..mesh.triangle_count()).collect()
        } else {
            target.bvh().ray(p, w, -COORD_LIMIT, COORD_LIMIT).map_err(DistError::Bvh)?
        };
        let mut best: Option<(f64, f64, u64)> = None;
        for t in candidates {
            if opts.subset.is_some_and(|s| !s.contains(t)) {
                continue;
            }
            let tri = corners(mesh, t)?;
            if let Some(param) = line_triangle(&p, &sh, &tri) {
                let at = param.abs();
                let better = match best {
                    None => true,
                    Some((b, _, bt)) => at < b || (at == b && t < bt),
                };
                if better {
                    best = Some((at, param, t));
                }
            }
        }
        let Some((at, param, tri)) = best else {
            return Ok(Projection::NoHit);
        };
        let distance = at * wlen;
        if opts.max_distance.is_some_and(|d| distance > d) {
            return Ok(Projection::NoHit);
        }
        let foot = [p[0] + param * w[0], p[1] + param * w[1], p[2] + param * w[2]];
        Ok(Projection::Hit(ProjectionHit {
            foot,
            distance,
            signed_distance: param * wlen,
            sign: HitSign::Along,
            entity_id: tri,
            params: None,
        }))
    }
}

impl ProjectionTarget for SignTarget<'_> {
    fn frame(&self) -> FrameRef {
        self.target().mesh().frame()
    }

    fn project(&self, p: V3, opts: &ProjectionOptions<'_, '_>) -> Result<Projection, ProjectError> {
        let q = MeshQuery { max_distance: opts.max_distance, subset: opts.subset };
        match self.target().closest(p, &q)? {
            MeshProjection::NoHit => Ok(Projection::NoHit),
            MeshProjection::Hit(h) => {
                let s = self.signed(p, &h)?;
                Ok(Projection::Hit(ProjectionHit {
                    foot: h.foot,
                    distance: s.unsigned,
                    signed_distance: s.distance,
                    sign: HitSign::Mesh(s.status),
                    entity_id: h.tri,
                    params: None,
                }))
            }
        }
    }

    fn project_along(&self, p: V3, v: V3, opts: &ProjectionOptions<'_, '_>) -> Result<Projection, ProjectError> {
        self.along(p, v, opts, false)
    }
}

// ------------------------------------------------------------------ batch

/// Projects every point of `points` onto `target` (closest point, or along
/// `opts.direction`), on `config.threads` threads, and returns the results in input order.
/// See the [module documentation](self) for the pinned order of checks, the determinism
/// rule and cancellation.
pub fn project_batch<T: ProjectionTarget + ?Sized>(
    points: &PointSet,
    target: &T,
    opts: &ProjectionOptions<'_, '_>,
    config: &BatchConfig,
    cancel: &AtomicBool,
) -> Result<Vec<Projection>, BatchError> {
    require_same_frame_ref(points.frame(), target.frame()).map_err(BatchError::FrameMismatch)?;
    if config.threads == 0 || config.threads > MAX_THREADS {
        return Err(BatchError::InvalidThreads { threads: config.threads });
    }
    if config.block == 0 || config.block % BLOCK_QUANTUM != 0 || config.block > MAX_BLOCK {
        return Err(BatchError::InvalidBlockSize { block: config.block });
    }
    check_max(opts.max_distance).map_err(|_| BatchError::InvalidMaxDistance)?;
    if let Some(v) = &opts.direction {
        check_direction(v).map_err(|_| BatchError::InvalidDirection)?;
    }
    let (xs, ys, zs) = (points.x(), points.y(), points.z());
    let n = xs.len();
    if ys.len() != n || zs.len() != n {
        return Err(BatchError::WorkerFailed);
    }
    for (i, ((&x, &y), &z)) in xs.iter().zip(ys).zip(zs).enumerate() {
        let p = [x, y, z];
        if !p.iter().all(|x| x.is_finite()) {
            return Err(BatchError::NonFiniteQuery { index: i as u64 });
        }
        if !p.iter().all(|x| (-COORD_LIMIT..=COORD_LIMIT).contains(x)) {
            return Err(BatchError::QueryOutOfRange { index: i as u64 });
        }
    }

    let bytes = (n as u64).saturating_mul(core::mem::size_of::<Projection>() as u64);
    let mut out: Vec<Projection> = Vec::new();
    out.try_reserve_exact(n).map_err(|_| BatchError::AllocationFailed { bytes })?;
    out.resize(n, Projection::NoHit);

    let first_error = AtomicU64::new(u64::MAX);
    let error: Mutex<Option<(u64, ProjectError)>> = Mutex::new(None);
    let failed = AtomicBool::new(false);
    {
        let mut blocks: Vec<(usize, &mut [Projection])> = Vec::new();
        let count = n.div_ceil(config.block);
        blocks.try_reserve_exact(count).map_err(|_| BatchError::AllocationFailed {
            bytes: (count as u64).saturating_mul(core::mem::size_of::<(usize, &mut [Projection])>() as u64),
        })?;
        // Reversed, so popping takes the blocks in ascending order.
        blocks.extend(out.chunks_mut(config.block).enumerate().rev());
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
                let p = [x, y, z];
                let r = match opts.direction {
                    Some(v) => target.project_along(p, v, opts),
                    None => target.project(p, opts),
                };
                match r {
                    Ok(pr) => *slot = pr,
                    Err(e) => {
                        let idx = i as u64;
                        first_error.fetch_min(idx, Ordering::AcqRel);
                        match error.lock() {
                            Ok(mut g) => {
                                if g.as_ref().map_or(true, |(j, _)| idx < *j) {
                                    *g = Some((idx, e));
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
                // A thread that cannot be started is not an error: the remaining workers
                // (at least the calling thread) take its blocks.
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
        return Err(BatchError::Cancelled);
    }
    if failed.load(Ordering::Acquire) {
        return Err(BatchError::WorkerFailed);
    }
    match error.into_inner() {
        Ok(Some((index, error))) => Err(BatchError::Point { index, error }),
        Ok(None) => Ok(out),
        Err(_) => Err(BatchError::WorkerFailed),
    }
}

// ------------------------------------------------------------------ summary

/// Counts and signed-distance statistics of a batch (REQ-004).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BatchSummary {
    /// Number of results.
    pub points: u64,
    /// Results with a foot point.
    pub hits: u64,
    /// `NoHit` results.
    pub no_hits: u64,
    /// Hits whose sign is not defined (undecidable or undefined).
    pub sign_undefined: u64,
    /// Smallest signed distance over hits with a defined sign (total order, so `-0.0 < +0.0`).
    pub min_signed: Option<f64>,
    /// Largest signed distance over hits with a defined sign (total order).
    pub max_signed: Option<f64>,
    /// Mean signed distance over hits with a defined sign (`fid_num::red::mean`).
    pub mean_signed: Option<f64>,
}

/// The summary of `results`, reduced on one thread in input order.
pub fn summary(results: &[Projection]) -> Result<BatchSummary, BatchError> {
    let mut s = BatchSummary {
        points: 0,
        hits: 0,
        no_hits: 0,
        sign_undefined: 0,
        min_signed: None,
        max_signed: None,
        mean_signed: None,
    };
    let mut signed: Vec<f64> = Vec::new();
    signed
        .try_reserve_exact(results.len())
        .map_err(|_| BatchError::AllocationFailed { bytes: (results.len() as u64).saturating_mul(8) })?;
    for r in results {
        s.points += 1;
        match r {
            Projection::NoHit => s.no_hits += 1,
            Projection::Hit(h) => {
                s.hits += 1;
                if h.sign_defined() {
                    signed.push(h.signed_distance);
                } else {
                    s.sign_undefined += 1;
                }
            }
        }
    }
    let bad = |_| BatchError::WorkerFailed;
    for &d in &signed {
        s.min_signed = Some(match s.min_signed {
            None => d,
            Some(m) => fid_num::f::total_min(m, d).map_err(bad)?,
        });
        s.max_signed = Some(match s.max_signed {
            None => d,
            Some(m) => fid_num::f::total_max(m, d).map_err(bad)?,
        });
    }
    if !signed.is_empty() {
        s.mean_signed = Some(fid_num::red::mean(&signed).map_err(bad)?);
    }
    Ok(s)
}
