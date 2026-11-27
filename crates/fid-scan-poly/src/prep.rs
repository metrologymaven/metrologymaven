//! Vertex-set preparation for cloud polygonization. Spec node `SCAN.POLY.SURF.PREP`.
//!
//! [`prepare`] picks one measured representative per occupied grid cell and, when smoothing
//! is on, moves each representative onto the least-squares plane of its neighbours, clamped
//! to half a cell. Every value-affecting choice is pinned below; the independent reference
//! `fixtures/scan/poly/make_prep_fixtures.py` checks the selection bit for bit and the
//! smoothed positions within 1e-12 m with exact and 80-digit decimal arithmetic.
//!
//! # Input checks (first failure wins)
//!
//! 1. `params.h` finite, `> 0` and at most [`MAX_H`] = 2^400 (`h`);
//! 2. `params.edge_angle_rad` finite, `> 0` and at most pi (`edge_angle_rad`);
//! 3. `1 <= params.threads <= MAX_THREADS` (`threads`);
//! 4. `valid` has one flag per cloud point (`valid`);
//! 5. the cloud carries normals (`cloud.normal`).
//!
//! Each is [`PolyError::InvalidParameter`] (`scan.poly.invalid_parameter`) naming the
//! parameter. The cloud itself was validated when it was built (finite coordinates, unit
//! normals, GEO.TYPES.POINTS).
//!
//! # Selection (REQ-SCAN.POLY.SURF.PREP-001)
//!
//! - Origin `o` = the bounding-box minimum of all cloud points (totalOrder, GEO.TYPES).
//! - Only points with `valid[i]` take part. Cell of point `p`, per axis `a`:
//!   `c_a = floor((p_a - o_a) / h)` (one subtraction, one division, each rounded once). A
//!   cell index above [`MAX_CELL_INDEX`] = 2^32 - 1 is refused as `invalid_parameter` on `h`
//!   (the resolution is too fine for the cloud's extent).
//! - Cells are visited in ascending Morton order: bit `b` of `c_x`, `c_y`, `c_z` goes to bit
//!   `3b`, `3b + 1`, `3b + 2` of a 96-bit key.
//! - The centroid of a cell is `(sx / k, sy / k, sz / k)`, with `s` the plain IEEE sums of its
//!   valid points in ascending index order and `k` their count. A non-finite sum is
//!   [`PolyError::OutOfRange`].
//! - The representative is the valid point with the smallest `(dx*dx + dy*dy) + dz*dz` to
//!   the centroid ([`fid_geo_index::dist2`]), lowest index on ties. A cell without a valid
//!   point has no representative. With smoothing off the representative is output unchanged
//!   ([`VertexStatus::Unmoved`]).
//!
//! # Smoothing (REQ-SCAN.POLY.SURF.PREP-002, -003)
//!
//! For each representative `p` (source index `s`), in this order:
//!
//! 1. Neighbours: every valid point with pinned squared distance `<= (2h)*(2h)` to `p`
//!    (the GEO.INDEX.KDTREE radius query over all cloud points, then filtered on `valid`),
//!    in ascending index order. `p` itself is one of them.
//! 2. Sharp edges: when `params.sharp_edges` is on and some neighbour normal `nq` has
//!    `(nr_x*nq_x + nr_y*nq_y) + nr_z*nq_z < cos(edge_angle_rad)` with `nr` the normal of
//!    `p` (`fid_num::f::cos`), `p` stays where it is ([`VertexStatus::KeptEdge`]).
//! 3. Fewer than [`MIN_NEIGHBOURS`] neighbours: kept ([`VertexStatus::KeptFewNeighbours`]).
//! 4. Plane: centroid `m` as in selection; covariance `C = sum (q - m)(q - m)^T` summed in
//!    neighbour order, the six upper entries each as a plain running sum; the KRN.NUM.LA.EIG
//!    cyclic Jacobi eigensolver with `rel_tol` = [`PLANE_RANK_TOL`] = 2^-40. Rank below 2
//!    (collinear or coincident neighbours) keeps `p` ([`VertexStatus::KeptDegenerate`]).
//!    The plane normal `n` is the eigenvector of the smallest eigenvalue.
//! 5. Projection: `d = (n_x*(p_x - m_x) + n_y*(p_y - m_y)) + n_z*(p_z - m_z)`,
//!    `delta = -(d * n)` per axis, `len = sqrt((delta_x^2 + delta_y^2) + delta_z^2)`.
//!    With `rho = (h * 0.5) * CLAMP_FACTOR` (`CLAMP_FACTOR` = 1 - 2^-40, a margin for
//!    rounding), when `len > rho` every component is scaled by `rho / len`
//!    ([`VertexStatus::Clamped`]); else [`VertexStatus::Smoothed`].
//! 6. Bound: the new position is `p + delta` per axis and its displacement is
//!    `sqrt(dist2(new, p))`. If that exceeds `h / 2` (not reachable in practice) the vertex
//!    is kept instead ([`VertexStatus::KeptBound`]), so REQ-003 holds after rounding.
//!
//! Kept vertices are bit-identical to their source point with displacement 0.
//!
//! # Determinism (D0) and threads
//!
//! Each representative's result depends only on the cloud, `valid` and the parameters.
//! With `threads > 1` the representatives are split into contiguous blocks, one scoped
//! thread per block (a block whose thread cannot start runs on the calling thread), and the
//! blocks are joined in order, so the output and the first error (in vertex order) do not
//! depend on the thread count (AC-4). Sums have a fixed order, there is no fused
//! multiply-add and no hash container, and NaN cannot arise from validated input.

use core::fmt;

use fid_geo_index::{dist2, PointIndex, PointIndexError};
use fid_geo_types::{PointBuffer, PointCloud};
use fid_num::la::{sym_eig, Mat};
use fid_num::NumError;

/// The default sharp-edge threshold, 30 degrees [unverified: a starting value, not
/// measured against scanned parts].
pub const DEFAULT_EDGE_ANGLE_RAD: f64 = core::f64::consts::FRAC_PI_6;
/// The largest accepted thread count.
pub const MAX_THREADS: usize = 256;
/// The largest accepted cell edge `h` in metres, 2^400 (keeps every covariance sum finite).
pub const MAX_H: f64 = f64::from_bits((1023 + 400) << 52);
/// The largest cell index per axis, 2^32 - 1.
pub const MAX_CELL_INDEX: f64 = 4_294_967_295.0;
/// The fewest valid neighbours (the representative included) for a plane fit.
pub const MIN_NEIGHBOURS: usize = 3;
/// The eigensolver rank tolerance of the plane fit, 2^-40.
pub const PLANE_RANK_TOL: f64 = f64::from_bits((1023 - 40) << 52);
/// The clamp radius factor `1 - 2^-40`: the clamp aims at `(h/2) * CLAMP_FACTOR` so that
/// the rounded displacement stays at most `h/2`.
pub const CLAMP_FACTOR: f64 = 1.0 - f64::from_bits((1023 - 40) << 52);

/// The preparation parameters (REQ-SCAN.POLY.SURF-002).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrepParams {
    /// The grid cell edge (target resolution) in metres.
    pub h: f64,
    /// Move representatives onto their local least-squares plane.
    pub smoothing: bool,
    /// Keep representatives in place where the neighbour normals spread beyond
    /// `edge_angle_rad`.
    pub sharp_edges: bool,
    /// The sharp-edge threshold in radians, `0 < angle <= pi`.
    pub edge_angle_rad: f64,
    /// Worker threads, `1..=MAX_THREADS` (1 runs on the calling thread).
    pub threads: usize,
}

impl PrepParams {
    /// Cell edge `h`, smoothing and sharp-edge preservation off, the default edge angle and
    /// one thread.
    pub fn new(h: f64) -> PrepParams {
        PrepParams { h, smoothing: false, sharp_edges: false, edge_angle_rad: DEFAULT_EDGE_ANGLE_RAD, threads: 1 }
    }
}

/// What happened to a representative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VertexStatus {
    /// Smoothing is off: the vertex is its source point.
    Unmoved,
    /// Moved onto the local plane, by at most `h/2`.
    Smoothed,
    /// Moved towards the local plane, the displacement clamped to `(h/2) * CLAMP_FACTOR`.
    Clamped,
    /// Kept in place: sharp-edge preservation is on and the neighbour normals spread.
    KeptEdge,
    /// Kept in place: fewer than [`MIN_NEIGHBOURS`] valid neighbours.
    KeptFewNeighbours,
    /// Kept in place: the neighbours are collinear or coincident (plane rank below 2).
    KeptDegenerate,
    /// Kept in place: the rounded displacement would exceed `h/2`.
    KeptBound,
}

impl VertexStatus {
    /// A stable machine-readable name.
    pub fn as_str(self) -> &'static str {
        match self {
            VertexStatus::Unmoved => "unmoved",
            VertexStatus::Smoothed => "smoothed",
            VertexStatus::Clamped => "clamped",
            VertexStatus::KeptEdge => "kept_edge",
            VertexStatus::KeptFewNeighbours => "kept_few",
            VertexStatus::KeptDegenerate => "kept_degenerate",
            VertexStatus::KeptBound => "kept_bound",
        }
    }

    /// Whether the vertex left its source point.
    pub fn moved(self) -> bool {
        matches!(self, VertexStatus::Smoothed | VertexStatus::Clamped)
    }
}

/// One output vertex.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrepVertex {
    /// The index of its source input point.
    pub source: u64,
    /// The grid cell it represents.
    pub cell: [u32; 3],
    /// Its position in metres.
    pub position: [f64; 3],
    /// Its distance from the source point in metres (0 unless moved; at most `h/2`).
    pub displacement_m: f64,
    /// What happened to it.
    pub status: VertexStatus,
}

/// The prepared vertex set, one vertex per occupied cell in Morton order.
#[derive(Debug, Clone, PartialEq)]
pub struct VertexSet {
    /// The vertices.
    pub vertices: Vec<PrepVertex>,
    /// The largest displacement in metres (0 for an empty set).
    pub max_displacement_m: f64,
}

/// Every error of [`prepare`]. Errors are values; nothing panics.
#[derive(Debug, Clone, PartialEq)]
pub enum PolyError {
    /// A parameter or input is outside its domain.
    InvalidParameter {
        /// The parameter.
        name: &'static str,
        /// Why it was refused.
        reason: &'static str,
    },
    /// A cell's coordinate sum is not finite (coordinates too large for fixed-order sums).
    OutOfRange {
        /// The first point of the offending cell, in index order.
        index: u64,
    },
    /// The neighbour index refused the cloud or a query.
    Index(PointIndexError),
    /// A numeric facade or eigensolver call failed.
    Numeric(NumError),
    /// A buffer sized from the input could not be allocated.
    Allocation,
    /// A worker thread ended without a result.
    WorkerFailed,
    /// An internal invariant does not hold (unreachable for a validated cloud).
    Inconsistent,
}

impl PolyError {
    /// A stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            PolyError::InvalidParameter { .. } => "scan.poly.invalid_parameter",
            PolyError::OutOfRange { .. } => "scan.poly.out_of_range",
            PolyError::Index(_) => "scan.poly.index",
            PolyError::Numeric(_) => "scan.poly.numeric",
            PolyError::Allocation => "scan.poly.allocation",
            PolyError::WorkerFailed => "scan.poly.worker_failed",
            PolyError::Inconsistent => "scan.poly.inconsistent",
        }
    }
}

impl fmt::Display for PolyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PolyError::InvalidParameter { name, reason } => write!(f, "{}: {name}: {reason}", self.code()),
            PolyError::OutOfRange { index } => write!(f, "{}: cell of point {index}", self.code()),
            PolyError::Index(e) => write!(f, "{}: {e}", self.code()),
            PolyError::Numeric(e) => write!(f, "{}: {e}", self.code()),
            _ => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for PolyError {}

impl From<PointIndexError> for PolyError {
    fn from(e: PointIndexError) -> PolyError {
        PolyError::Index(e)
    }
}

impl From<NumError> for PolyError {
    fn from(e: NumError) -> PolyError {
        PolyError::Numeric(e)
    }
}

fn invalid(name: &'static str, reason: &'static str) -> PolyError {
    PolyError::InvalidParameter { name, reason }
}

/// A selected representative.
#[derive(Debug, Clone, Copy)]
struct Rep {
    source: usize,
    cell: [u32; 3],
    point: [f64; 3],
}

/// The result of smoothing one representative.
#[derive(Debug, Clone, Copy)]
struct Moved {
    position: [f64; 3],
    displacement_m: f64,
    status: VertexStatus,
}

/// Prepares the vertex set of `cloud` (module documentation). `valid[i]` says whether point
/// `i` has a valid normal (SCAN.CLOUD.NORM).
pub fn prepare(cloud: &PointCloud, valid: &[bool], params: &PrepParams) -> Result<VertexSet, PolyError> {
    check_params(params)?;
    let n = usize::try_from(cloud.len()).map_err(|_| invalid("cloud", "more points than this target addresses"))?;
    if valid.len() != n {
        return Err(invalid("valid", "not one flag per cloud point"));
    }
    let normals =
        cloud.attributes().normal.as_ref().ok_or_else(|| invalid("cloud.normal", "the cloud has no normals"))?;
    if !valid.iter().any(|v| *v) {
        return Ok(VertexSet { vertices: Vec::new(), max_displacement_m: 0.0 });
    }
    let origin = cloud.aabb().map_err(|_| PolyError::Inconsistent)?.min();
    let buf = cloud.buffer();
    let reps = select(buf, valid, origin, params.h)?;

    let moved = if params.smoothing {
        let cos_thr = fid_num::f::cos(params.edge_angle_rad)?;
        let index = PointIndex::build(buf, 1)?;
        let ctx = Ctx { index: &index, valid, normals, h: params.h, sharp_edges: params.sharp_edges, cos_thr };
        Some(smooth_all(&ctx, &reps, params.threads)?)
    } else {
        None
    };

    let mut vertices = Vec::new();
    vertices.try_reserve_exact(reps.len()).map_err(|_| PolyError::Allocation)?;
    let mut max_d = 0.0;
    for (k, r) in reps.iter().enumerate() {
        let m = match &moved {
            Some(all) => *all.get(k).ok_or(PolyError::Inconsistent)?,
            None => Moved { position: r.point, displacement_m: 0.0, status: VertexStatus::Unmoved },
        };
        max_d = fid_num::f::total_max(max_d, m.displacement_m)?;
        let source = u64::try_from(r.source).map_err(|_| PolyError::Inconsistent)?;
        vertices.push(PrepVertex {
            source,
            cell: r.cell,
            position: m.position,
            displacement_m: m.displacement_m,
            status: m.status,
        });
    }
    Ok(VertexSet { vertices, max_displacement_m: max_d })
}

fn check_params(p: &PrepParams) -> Result<(), PolyError> {
    if !(p.h.is_finite() && p.h > 0.0 && p.h <= MAX_H) {
        return Err(invalid("h", "not a finite value in (0, 2^400] m"));
    }
    if !(p.edge_angle_rad.is_finite() && p.edge_angle_rad > 0.0 && p.edge_angle_rad <= core::f64::consts::PI) {
        return Err(invalid("edge_angle_rad", "not a finite angle in (0, pi]"));
    }
    if p.threads == 0 || p.threads > MAX_THREADS {
        return Err(invalid("threads", "not in 1..=256"));
    }
    Ok(())
}

/// The cell of `p` (module documentation, Selection).
fn cell_of(p: [f64; 3], o: [f64; 3], h: f64) -> Result<[u32; 3], PolyError> {
    let mut c = [0u32; 3];
    for ((slot, &pa), &oa) in c.iter_mut().zip(p.iter()).zip(o.iter()) {
        let f = ((pa - oa) / h).floor();
        // Also refuses NaN and infinity (an extent beyond the f64 range).
        if !(0.0..=MAX_CELL_INDEX).contains(&f) {
            return Err(invalid("h", "the grid would have more than 2^32 cells on an axis"));
        }
        // Exact: `f` is an integer in [0, 2^32 - 1].
        *slot = f as u32;
    }
    Ok(c)
}

/// The 96-bit Morton key: bit `b` of x, y, z at bits `3b`, `3b + 1`, `3b + 2`.
fn morton(c: [u32; 3]) -> u128 {
    let mut key = 0u128;
    for b in 0..32 {
        for (axis, &v) in c.iter().enumerate() {
            key |= u128::from((v >> b) & 1) << (3 * b + axis);
        }
    }
    key
}

/// One representative per occupied cell, in Morton order.
fn select(buf: &PointBuffer, valid: &[bool], o: [f64; 3], h: f64) -> Result<Vec<Rep>, PolyError> {
    let count = valid.iter().filter(|v| **v).count();
    let mut keyed: Vec<(u128, usize, [u32; 3], [f64; 3])> = Vec::new();
    keyed.try_reserve_exact(count).map_err(|_| PolyError::Allocation)?;
    let points = buf.x.iter().zip(&buf.y).zip(&buf.z).zip(valid).enumerate();
    for (i, (((&x, &y), &z), &ok)) in points {
        if ok {
            let p = [x, y, z];
            let cell = cell_of(p, o, h)?;
            keyed.push((morton(cell), i, cell, p));
        }
    }
    if keyed.len() != count {
        return Err(PolyError::Inconsistent);
    }
    // Keys are unique (the index), so the unstable sort is a pure function of the input.
    keyed.sort_unstable_by_key(|e| (e.0, e.1));

    let mut reps = Vec::new();
    for group in keyed.chunk_by(|a, b| a.0 == b.0) {
        let first = group.first().ok_or(PolyError::Inconsistent)?;
        let (mut sx, mut sy, mut sz) = (0.0f64, 0.0f64, 0.0f64);
        for e in group {
            sx += e.3[0];
            sy += e.3[1];
            sz += e.3[2];
        }
        if !(sx.is_finite() && sy.is_finite() && sz.is_finite()) {
            return Err(PolyError::OutOfRange { index: u64::try_from(first.1).map_err(|_| PolyError::Inconsistent)? });
        }
        // Exact: a cell holds at most 2^32 points.
        let k = group.len() as f64;
        let cen = [sx / k, sy / k, sz / k];
        let mut best = (dist2(first.3, cen), first);
        for e in group.iter().skip(1) {
            let d = dist2(e.3, cen);
            if d < best.0 {
                best = (d, e);
            }
        }
        if !best.0.is_finite() {
            return Err(PolyError::OutOfRange { index: u64::try_from(first.1).map_err(|_| PolyError::Inconsistent)? });
        }
        reps.try_reserve(1).map_err(|_| PolyError::Allocation)?;
        reps.push(Rep { source: best.1 .1, cell: best.1 .2, point: best.1 .3 });
    }
    Ok(reps)
}

/// Read-only smoothing context shared by the workers.
struct Ctx<'a> {
    index: &'a PointIndex<'a>,
    valid: &'a [bool],
    normals: &'a [Vec<f64>; 3],
    h: f64,
    sharp_edges: bool,
    cos_thr: f64,
}

impl Ctx<'_> {
    fn normal(&self, i: usize) -> Result<[f64; 3], PolyError> {
        let [nx, ny, nz] = self.normals;
        match (nx.get(i), ny.get(i), nz.get(i)) {
            (Some(&a), Some(&b), Some(&c)) => Ok([a, b, c]),
            _ => Err(PolyError::Inconsistent),
        }
    }

    fn point(&self, i: usize) -> Result<[f64; 3], PolyError> {
        let b = self.index.points();
        match (b.x.get(i), b.y.get(i), b.z.get(i)) {
            (Some(&x), Some(&y), Some(&z)) => Ok([x, y, z]),
            _ => Err(PolyError::Inconsistent),
        }
    }
}

/// Smooths every representative, in contiguous blocks on up to `threads` threads.
fn smooth_all(ctx: &Ctx<'_>, reps: &[Rep], threads: usize) -> Result<Vec<Moved>, PolyError> {
    let workers = threads.min(reps.len()).max(1);
    if workers == 1 {
        return smooth_block(ctx, reps);
    }
    let block = reps.len().div_ceil(workers);
    let results: Vec<Result<Vec<Moved>, PolyError>> = std::thread::scope(|s| {
        let mut started = Vec::new();
        for part in reps.chunks(block) {
            let handle = std::thread::Builder::new().spawn_scoped(s, move || smooth_block(ctx, part)).ok();
            started.push((handle, part));
        }
        started
            .into_iter()
            .map(|(handle, part)| match handle {
                Some(h) => h.join().unwrap_or(Err(PolyError::WorkerFailed)),
                None => smooth_block(ctx, part),
            })
            .collect()
    });
    let mut out = Vec::new();
    out.try_reserve_exact(reps.len()).map_err(|_| PolyError::Allocation)?;
    for r in results {
        out.extend(r?);
    }
    Ok(out)
}

/// Smooths a block of representatives in order; the first error stops the block.
fn smooth_block(ctx: &Ctx<'_>, reps: &[Rep]) -> Result<Vec<Moved>, PolyError> {
    let mut out = Vec::new();
    out.try_reserve_exact(reps.len()).map_err(|_| PolyError::Allocation)?;
    for r in reps {
        out.push(smooth_one(ctx, r)?);
    }
    Ok(out)
}

fn kept(p: [f64; 3], status: VertexStatus) -> Moved {
    Moved { position: p, displacement_m: 0.0, status }
}

/// Smoothing steps 1 to 6 of the module documentation for one representative.
fn smooth_one(ctx: &Ctx<'_>, r: &Rep) -> Result<Moved, PolyError> {
    let p = r.point;
    let hits = ctx.index.radius(p, 2.0 * ctx.h)?;
    let mut nb: Vec<usize> = Vec::new();
    nb.try_reserve_exact(hits.len()).map_err(|_| PolyError::Allocation)?;
    for hit in &hits {
        let i = usize::try_from(hit.index).map_err(|_| PolyError::Inconsistent)?;
        if *ctx.valid.get(i).ok_or(PolyError::Inconsistent)? {
            nb.push(i);
        }
    }
    nb.sort_unstable();

    if ctx.sharp_edges {
        let nr = ctx.normal(r.source)?;
        for &i in &nb {
            let nq = ctx.normal(i)?;
            let d = (nr[0] * nq[0] + nr[1] * nq[1]) + nr[2] * nq[2];
            if d < ctx.cos_thr {
                return Ok(kept(p, VertexStatus::KeptEdge));
            }
        }
    }
    if nb.len() < MIN_NEIGHBOURS {
        return Ok(kept(p, VertexStatus::KeptFewNeighbours));
    }

    let mut qs: Vec<[f64; 3]> = Vec::new();
    qs.try_reserve_exact(nb.len()).map_err(|_| PolyError::Allocation)?;
    for &i in &nb {
        qs.push(ctx.point(i)?);
    }
    let (mut sx, mut sy, mut sz) = (0.0f64, 0.0f64, 0.0f64);
    for q in &qs {
        sx += q[0];
        sy += q[1];
        sz += q[2];
    }
    // Exact: at most 2^32 neighbours.
    let k = qs.len() as f64;
    let m = [sx / k, sy / k, sz / k];
    let mut c = [0.0f64; 6];
    for q in &qs {
        let d = [q[0] - m[0], q[1] - m[1], q[2] - m[2]];
        c[0] += d[0] * d[0];
        c[1] += d[0] * d[1];
        c[2] += d[0] * d[2];
        c[3] += d[1] * d[1];
        c[4] += d[1] * d[2];
        c[5] += d[2] * d[2];
    }
    let a = Mat::from_rows(&[[c[0], c[1], c[2]], [c[1], c[3], c[4]], [c[2], c[4], c[5]]])?;
    let eig = sym_eig(&a, PLANE_RANK_TOL)?;
    if eig.diag().rank < 2 {
        return Ok(kept(p, VertexStatus::KeptDegenerate));
    }
    let v = eig.vectors();
    let n = [v.get(0, 0)?, v.get(1, 0)?, v.get(2, 0)?];

    let d = (n[0] * (p[0] - m[0]) + n[1] * (p[1] - m[1])) + n[2] * (p[2] - m[2]);
    let mut delta = [-(d * n[0]), -(d * n[1]), -(d * n[2])];
    let len = fid_num::f::sqrt((delta[0] * delta[0] + delta[1] * delta[1]) + delta[2] * delta[2])?;
    let half = ctx.h * 0.5;
    let rho = half * CLAMP_FACTOR;
    let mut status = VertexStatus::Smoothed;
    if len > rho {
        let s = rho / len;
        for x in delta.iter_mut() {
            *x *= s;
        }
        status = VertexStatus::Clamped;
    }
    let q = [p[0] + delta[0], p[1] + delta[1], p[2] + delta[2]];
    let disp = fid_num::f::sqrt(dist2(q, p))?;
    if disp > half {
        return Ok(kept(p, VertexStatus::KeptBound));
    }
    Ok(Moved { position: q, displacement_m: disp, status })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn morton_bits() {
        assert_eq!(morton([1, 0, 0]), 1);
        assert_eq!(morton([0, 1, 0]), 2);
        assert_eq!(morton([0, 0, 1]), 4);
        assert_eq!(morton([2, 0, 0]), 8);
        assert_eq!(morton([3, 3, 3]), 63);
        assert_eq!(morton([u32::MAX, u32::MAX, u32::MAX]), (1u128 << 96) - 1);
    }

    #[test]
    fn constants() {
        assert_eq!(MAX_H, 2f64.powi(400));
        assert_eq!(PLANE_RANK_TOL, 2f64.powi(-40));
        assert_eq!(CLAMP_FACTOR, 1.0 - 2f64.powi(-40));
        assert_eq!(MAX_CELL_INDEX, f64::from(u32::MAX));
    }

    #[test]
    fn cell_limits() {
        assert_eq!(cell_of([2.5, 0.0, 0.99], [0.0; 3], 1.0).unwrap(), [2, 0, 0]);
        assert!(cell_of([5e9, 0.0, 0.0], [0.0; 3], 1.0).is_err());
        assert!(cell_of([f64::MAX, 0.0, 0.0], [-f64::MAX, 0.0, 0.0], 1.0).is_err());
    }
}
