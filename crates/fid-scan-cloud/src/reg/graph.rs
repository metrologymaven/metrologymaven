//! The scan overlap matrix and pair graph. Spec node `SCAN.CLOUD.REG.GRAPH`.
//!
//! [`pair_graph`] decides which scan pairs take part in global registration: it computes the
//! overlap fraction of every ordered pair of N scans at their current poses (the
//! [`overlap_fraction`](super::overlap::overlap_fraction) of `SCAN.CLOUD.REG.CTGT`, the one
//! definition of overlap), keeps the pairs whose overlap reaches a minimum in both directions,
//! and checks that the resulting graph is connected. A disconnected graph is the typed error
//! [`RegError::Disconnected`] listing the components, never a silently unconstrained pose.
//!
//! # Pinned rule (first failure wins)
//!
//! [`pair_graph_with`]`(scans, poses, d, min_overlap, max_samples, threads)`:
//!
//! 1. `2 <= N <=` [`MAX_SCANS`] scans and exactly N poses; `d` finite and `> 0`;
//!    `min_overlap` in `(0, 1]`; `max_samples >= 1`; `1 <= threads <=`
//!    [`MAX_THREADS`]. Each refusal is `scan.cloud.invalid_parameter` naming the parameter.
//! 2. For each scan j in ascending order (a failure is [`RegError::Scan`] with the index and
//!    the error): the scan is in the frame of scan 0 (`geo.frame.mismatch`; poses are rigid
//!    motions within that one frame, nothing is converted) and not empty; its sample is
//!    [`stride_sample`]`(len, max_samples)`; each sampled point `x` maps to
//!    `y = Transform::Rigid(pose).apply_point(x)` (GEO.FRAME.XFORM), which must be finite and
//!    within [`COORD_LIMIT`](super::target::COORD_LIMIT) (`pose`); the target is
//!    [`OrientedCloudTarget::new`]`(transform_element(cloud, pose), valid)` (GEO.FRAME.APPLY,
//!    same point arithmetic), so the target holds scan j at pose j and the CTGT construction
//!    checks apply (mask length, normals, coordinate limit).
//! 3. Rounding floor: with `M` the largest magnitude among every scan's sampled points, their
//!    images, every translation and every target's valid coordinates, `d` must be at least
//!    [`FLOOR_FACTOR`]` * 2^-53 * M` (`d`). This is one global check, at least as strict as
//!    the per-pair floor of the overlap fraction, so no pair is ever refused later.
//! 4. Cells in ascending `(i, j)`, row-major: the diagonal is [`PairOverlap::Diagonal`]
//!    (f = 1). For `i != j`, with box A the axis-aligned box of the sampled images of scan i
//!    and box B the box of the valid target points of scan j, the pair is
//!    [`PairOverlap::Disjoint`] (f = 0, no neighbour query; REQ-004) when on some axis the gap
//!    `g = A.lo - B.hi` or `g = B.lo - A.hi` has `g > 0` and `sqrt(g * g) > d`
//!    (`fid_num::f::sqrt`). Otherwise the cell is [`PairOverlap::Measured`] with
//!    `overlap_fraction_with(scan i, pose i, target j, d, max_samples, threads)` (a failure is
//!    [`RegError::Pair`] with both indexes).
//! 5. Edge `(i, j)`, `i < j`, exactly when `min(f(i, j), f(j, i)) >= min_overlap`, comparing
//!    the reported (correctly rounded) fractions with `total_cmp`; edges in ascending `(i, j)`.
//! 6. Components by union-find over the edges, the lowest index always the root; each
//!    component is a sorted index list, components ordered by their smallest index. More than
//!    one component is [`RegError::Disconnected`] with the components and the whole
//!    [`PairGraph`] (so a caller can report the overlap that was found).
//!
//! # The prefilter never changes a value
//!
//! For a sampled image `y` in A and a target point `q` in B, `y_k - q_k >= A.lo_k - B.hi_k`
//! exactly, so the computed `fl(y_k - q_k) >= g` (rounding is monotone), the GEO.INDEX.KDTREE
//! `dist2 = (dx*dx + dy*dy) + dz*dz` is at least `fl(g * g)` (adding non-negative terms never
//! lowers a rounded sum), and `sqrt(dist2) >= sqrt(fl(g * g)) > d`; the same holds for the
//! other side with `|fl(y_k - q_k)| = fl(q_k - y_k)`. So every sampled point of a disjoint
//! pair is beyond `d`, and the full computation would also give f = 0: the matrix equals the
//! one computed without the prefilter, bit for bit. The test is computed, not approximated,
//! so it holds for tiny coordinates as well (where `g * g` may underflow, the test simply
//! fails and the pair is measured).
//!
//! # Determinism (REQ-005, D0)
//!
//! Every cell is a function of the inputs only; `threads` is passed to the overlap fraction,
//! whose integer counts do not depend on it, so the matrix, the edges and the components are
//! bit-identical for every thread count.
//!
//! # Cost
//!
//! Each scan is copied twice while its target is built (the transformed cloud, then the
//! target's valid points; the transformed cloud is dropped at once), and every target is kept
//! until the end: about 60 bytes per valid point plus the input. The matrix has N² cells, N is
//! at most [`MAX_SCANS`]; each measured cell is one nearest-neighbour query per sampled point.

use fid_geo_frame::{require_same_frame_ref, transform_element, Rigid, Transform};
use fid_geo_types::{GeometryElement, PointCloud};

use super::error::RegError;
use super::mask::Bitmask;
use super::overlap::{overlap_fraction_with, stride_sample, Overlap, FLOOR_FACTOR};
use super::target::{max_abs, within_limit, OrientedCloudTarget, MAX_THREADS};

/// The default minimum overlap of an edge (REQ-SCAN.CLOUD.REG.GRAPH-002).
pub const DEFAULT_MIN_OVERLAP: f64 = 0.1;

/// The largest number of scans [`pair_graph`] accepts. The matrix has N² cells, so the bound
/// keeps it (and the pair work) explicit; the spec's budget is 50 scans.
pub const MAX_SCANS: usize = 1024;

const EPS: f64 = f64::from_bits((1023 - 53) << 52);

type V3 = [f64; 3];

/// One scan of the pair graph: its cloud (in its own scanner coordinates) and the
/// valid-normal mask that selects its target points (see [`OrientedCloudTarget::new`]).
#[derive(Debug, Clone, Copy)]
pub struct ScanInput<'a> {
    /// The scan's points.
    pub cloud: &'a PointCloud,
    /// One bit per point; only set points are target points.
    pub valid: &'a Bitmask,
}

/// One cell of the overlap matrix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PairOverlap {
    /// `i == j`; f = 1 by definition, nothing computed.
    Diagonal,
    /// The bounding boxes, inflated by `d`, do not meet (REQ-004); f = 0, no neighbour query.
    Disjoint,
    /// The overlap fraction of scan i at pose i against scan j at pose j.
    Measured(Overlap),
}

impl PairOverlap {
    /// The overlap fraction f of the cell (1 on the diagonal, 0 when disjoint).
    pub fn fraction(&self) -> f64 {
        match self {
            PairOverlap::Diagonal => 1.0,
            PairOverlap::Disjoint => 0.0,
            PairOverlap::Measured(o) => o.fraction(),
        }
    }
}

/// The overlap matrix of N scans, the pair graph and its components. The fields are private,
/// so a graph always satisfies the pinned rule of the [module documentation](self).
#[derive(Debug, Clone, PartialEq)]
pub struct PairGraph {
    n: u32,
    d: f64,
    min_overlap: f64,
    max_samples: u64,
    cells: Vec<PairOverlap>,
    overlap: Vec<f64>,
    edges: Vec<(u32, u32)>,
    components: Vec<Vec<u32>>,
}

impl PairGraph {
    /// The number of scans N.
    pub fn scan_count(&self) -> u32 {
        self.n
    }

    /// The overlap distance in metres the matrix was computed with.
    pub fn d(&self) -> f64 {
        self.d
    }

    /// The minimum overlap of an edge.
    pub fn min_overlap(&self) -> f64 {
        self.min_overlap
    }

    /// The sample cap per scan.
    pub fn max_samples(&self) -> u64 {
        self.max_samples
    }

    /// The overlap matrix, row-major N × N: entry `i * N + j` is f(i, j), the diagonal 1.
    pub fn overlap_matrix(&self) -> &[f64] {
        &self.overlap
    }

    /// The cell `(i, j)` (`None` when an index is not below N).
    pub fn cell(&self, i: u32, j: u32) -> Option<PairOverlap> {
        self.index(i, j).and_then(|k| self.cells.get(k)).copied()
    }

    /// f(i, j) (`None` when an index is not below N).
    pub fn overlap(&self, i: u32, j: u32) -> Option<f64> {
        self.index(i, j).and_then(|k| self.overlap.get(k)).copied()
    }

    /// The edges `(i, j)`, `i < j`, in ascending order.
    pub fn edges(&self) -> &[(u32, u32)] {
        &self.edges
    }

    /// The connected components: sorted index lists ordered by their smallest index. A graph
    /// returned by [`pair_graph`] has exactly one.
    pub fn components(&self) -> &[Vec<u32>] {
        &self.components
    }

    fn index(&self, i: u32, j: u32) -> Option<usize> {
        if i >= self.n || j >= self.n {
            return None;
        }
        let k = u64::from(i) * u64::from(self.n) + u64::from(j);
        usize::try_from(k).ok()
    }
}

/// An axis-aligned box; `lo <= hi` once a point was added.
#[derive(Debug, Clone, Copy)]
struct Box3 {
    lo: V3,
    hi: V3,
}

impl Box3 {
    fn new(p: &V3) -> Box3 {
        Box3 { lo: *p, hi: *p }
    }

    fn add(&mut self, p: &V3) {
        for ((lo, hi), &x) in self.lo.iter_mut().zip(self.hi.iter_mut()).zip(p) {
            if x.total_cmp(lo).is_lt() {
                *lo = x;
            }
            if x.total_cmp(hi).is_gt() {
                *hi = x;
            }
        }
    }
}

/// `g > 0 && sqrt(g * g) > d` (a proven gap, see the module documentation).
fn gap_beyond(g: f64, d: f64) -> bool {
    g > 0.0 && fid_num::f::sqrt(g * g).is_ok_and(|s| s > d)
}

/// Step 4's prefilter: some axis separates the boxes by more than `d`.
fn disjoint(a: &Box3, b: &Box3, d: f64) -> bool {
    (0..3).any(|k| match (a.lo.get(k), a.hi.get(k), b.lo.get(k), b.hi.get(k)) {
        (Some(&alo), Some(&ahi), Some(&blo), Some(&bhi)) => gap_beyond(alo - bhi, d) || gap_beyond(blo - ahi, d),
        _ => false,
    })
}

fn bigger(m: &mut f64, a: f64) {
    if a.total_cmp(m).is_gt() {
        *m = a;
    }
}

/// What step 2 keeps of a scan.
struct Prepared {
    target: OrientedCloudTarget,
    sample_box: Box3,
    target_box: Box3,
    scale: f64,
}

fn alloc_err<T>(n: usize) -> RegError {
    RegError::AllocationFailed {
        bytes: u64::try_from(n)
            .unwrap_or(u64::MAX)
            .saturating_mul(u64::try_from(core::mem::size_of::<T>()).unwrap_or(8)),
    }
}

fn index_u32(i: usize) -> Result<u32, RegError> {
    u32::try_from(i).map_err(|_| RegError::Inconsistent)
}

fn prepare(
    scan: &ScanInput<'_>,
    pose: &Rigid,
    frame: fid_geo_types::FrameRef,
    max_samples: u64,
) -> Result<Prepared, RegError> {
    let cloud = scan.cloud;
    require_same_frame_ref(cloud.frame(), frame).map_err(RegError::FrameMismatch)?;
    if cloud.is_empty() {
        return Err(RegError::invalid("cloud", "the scan has no points".to_string()));
    }
    let xf = Transform::Rigid(*pose);
    let mut scale = max_abs(&pose.t());
    let mut sample_box: Option<Box3> = None;
    for i in stride_sample(cloud.len(), max_samples)? {
        let x = cloud.point(i).ok_or(RegError::Inconsistent)?;
        let bad = || RegError::invalid("pose", format!("point {i} maps beyond 2^250 m"));
        let y = xf.apply_point(&x).map_err(|_| bad())?;
        if !within_limit(&y) {
            return Err(bad());
        }
        bigger(&mut scale, max_abs(&x));
        bigger(&mut scale, max_abs(&y));
        match sample_box.as_mut() {
            Some(b) => b.add(&y),
            None => sample_box = Some(Box3::new(&y)),
        }
    }
    let sample_box = sample_box.ok_or(RegError::Inconsistent)?;
    let world = transform_element(&GeometryElement::PointCloud(cloud.clone()), &xf, frame)
        .map_err(|e| RegError::invalid("pose", format!("the scan cannot be mapped by its pose: {} ({e})", e.code())))?;
    let GeometryElement::PointCloud(world) = world else {
        return Err(RegError::Inconsistent);
    };
    let target = OrientedCloudTarget::new(&world, scan.valid)?;
    bigger(&mut scale, target.coord_scale());
    let mut target_box: Option<Box3> = None;
    for i in 0..world.len() {
        if !scan.valid.get(i) {
            continue;
        }
        let q = world.point(i).ok_or(RegError::Inconsistent)?;
        match target_box.as_mut() {
            Some(b) => b.add(&q),
            None => target_box = Some(Box3::new(&q)),
        }
    }
    let target_box = target_box.ok_or(RegError::Inconsistent)?;
    Ok(Prepared { target, sample_box, target_box, scale })
}

/// Lowest-index-root union-find (path halving, no recursion).
fn find(parent: &mut [usize], mut x: usize) -> Result<usize, RegError> {
    loop {
        let p = *parent.get(x).ok_or(RegError::Inconsistent)?;
        if p == x {
            return Ok(x);
        }
        let gp = *parent.get(p).ok_or(RegError::Inconsistent)?;
        if let Some(slot) = parent.get_mut(x) {
            *slot = gp;
        }
        x = gp;
    }
}

fn components(n: usize, edges: &[(u32, u32)]) -> Result<Vec<Vec<u32>>, RegError> {
    let mut parent: Vec<usize> = Vec::new();
    parent.try_reserve_exact(n).map_err(|_| alloc_err::<usize>(n))?;
    parent.extend(0..n);
    for &(i, j) in edges {
        let a = find(&mut parent, usize::try_from(i).map_err(|_| RegError::Inconsistent)?)?;
        let b = find(&mut parent, usize::try_from(j).map_err(|_| RegError::Inconsistent)?)?;
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        if let Some(slot) = parent.get_mut(hi) {
            *slot = lo;
        }
    }
    // Roots are the smallest index of their component, so components come out ordered by their
    // smallest index and each list ascending.
    let mut slot_of: Vec<Option<usize>> = Vec::new();
    slot_of.try_reserve_exact(n).map_err(|_| alloc_err::<Option<usize>>(n))?;
    slot_of.resize(n, None);
    let mut out: Vec<Vec<u32>> = Vec::new();
    for i in 0..n {
        let root = find(&mut parent, i)?;
        let slot = match slot_of.get(root).copied().flatten() {
            Some(s) => s,
            None => {
                out.push(Vec::new());
                let s = out.len() - 1;
                if let Some(e) = slot_of.get_mut(root) {
                    *e = Some(s);
                }
                s
            }
        };
        out.get_mut(slot).ok_or(RegError::Inconsistent)?.push(index_u32(i)?);
    }
    Ok(out)
}

/// The pair graph on the calling thread ([`pair_graph_with`] with one thread).
pub fn pair_graph(
    scans: &[ScanInput<'_>],
    poses: &[Rigid],
    d: f64,
    min_overlap: f64,
    max_samples: u64,
) -> Result<PairGraph, RegError> {
    pair_graph_with(scans, poses, d, min_overlap, max_samples, 1)
}

/// The overlap matrix and pair graph of `scans` at `poses` (pinned rule in the
/// [module documentation](self)); the overlap fractions are counted on `threads` threads
/// (`1..=MAX_THREADS`) and the result does not depend on `threads`.
pub fn pair_graph_with(
    scans: &[ScanInput<'_>],
    poses: &[Rigid],
    d: f64,
    min_overlap: f64,
    max_samples: u64,
    threads: usize,
) -> Result<PairGraph, RegError> {
    let n = scans.len();
    if !(2..=MAX_SCANS).contains(&n) {
        return Err(RegError::invalid("scans", format!("{n} scans is not in 2..={MAX_SCANS}")));
    }
    if poses.len() != n {
        return Err(RegError::invalid("poses", format!("{} poses for {n} scans", poses.len())));
    }
    if !(d.is_finite() && d > 0.0) {
        return Err(RegError::invalid("d", format!("the overlap distance must be finite and > 0, not {d}")));
    }
    if !(min_overlap > 0.0 && min_overlap <= 1.0) {
        return Err(RegError::invalid("min_overlap", format!("{min_overlap} is not in (0, 1]")));
    }
    if max_samples == 0 {
        return Err(RegError::invalid("max_samples", "the sample size must be at least 1".to_string()));
    }
    if threads == 0 || threads > MAX_THREADS {
        return Err(RegError::invalid("threads", format!("{threads} is not in 1..={MAX_THREADS}")));
    }
    let n32 = index_u32(n)?;
    let frame = scans.first().ok_or(RegError::Inconsistent)?.cloud.frame();

    let mut prepared: Vec<Prepared> = Vec::new();
    prepared.try_reserve_exact(n).map_err(|_| alloc_err::<Prepared>(n))?;
    for (j, (scan, pose)) in scans.iter().zip(poses).enumerate() {
        let p = prepare(scan, pose, frame, max_samples)
            .map_err(|e| RegError::Scan { index: index_u32(j).unwrap_or(u32::MAX), error: Box::new(e) })?;
        prepared.push(p);
    }

    let mut scale = 0.0f64;
    for p in &prepared {
        bigger(&mut scale, p.scale);
    }
    let floor = FLOOR_FACTOR * EPS * scale;
    if d < floor {
        return Err(RegError::invalid(
            "d",
            format!("{d} m is below the rounding floor {floor} m (64 * 2^-53 * {scale} m) of these coordinates"),
        ));
    }

    let cells_len = n.checked_mul(n).ok_or_else(|| alloc_err::<PairOverlap>(usize::MAX))?;
    let mut cells: Vec<PairOverlap> = Vec::new();
    cells.try_reserve_exact(cells_len).map_err(|_| alloc_err::<PairOverlap>(cells_len))?;
    for (i, (scan, (pose, pi))) in scans.iter().zip(poses.iter().zip(&prepared)).enumerate() {
        for (j, pj) in prepared.iter().enumerate() {
            let cell = if i == j {
                PairOverlap::Diagonal
            } else if disjoint(&pi.sample_box, &pj.target_box, d) {
                PairOverlap::Disjoint
            } else {
                let o = overlap_fraction_with(scan.cloud, pose, &pj.target, d, max_samples, threads).map_err(|e| {
                    RegError::Pair {
                        moving: index_u32(i).unwrap_or(u32::MAX),
                        target: index_u32(j).unwrap_or(u32::MAX),
                        error: Box::new(e),
                    }
                })?;
                PairOverlap::Measured(o)
            };
            cells.push(cell);
        }
    }
    let mut overlap: Vec<f64> = Vec::new();
    overlap.try_reserve_exact(cells_len).map_err(|_| alloc_err::<f64>(cells_len))?;
    overlap.extend(cells.iter().map(PairOverlap::fraction));

    let at = |i: usize, j: usize| -> Result<f64, RegError> {
        overlap
            .get(i.checked_mul(n).and_then(|k| k.checked_add(j)).ok_or(RegError::Inconsistent)?)
            .copied()
            .ok_or(RegError::Inconsistent)
    };
    let mut edges: Vec<(u32, u32)> = Vec::new();
    for i in 0..n {
        for j in (i + 1)..n {
            let (a, b) = (at(i, j)?, at(j, i)?);
            let low = if a.total_cmp(&b).is_lt() { a } else { b };
            if low.total_cmp(&min_overlap).is_ge() {
                edges.try_reserve(1).map_err(|_| alloc_err::<(u32, u32)>(edges.len().saturating_add(1)))?;
                edges.push((index_u32(i)?, index_u32(j)?));
            }
        }
    }
    let components = components(n, &edges)?;
    let graph = PairGraph { n: n32, d, min_overlap, max_samples, cells, overlap, edges, components };
    if graph.components.len() > 1 {
        return Err(RegError::Disconnected { components: graph.components.clone(), graph: Box::new(graph) });
    }
    Ok(graph)
}
