//! Surface best-fit correspondence search (spec node `ALN.BFIT.ICP.CORR`).
//!
//! [`correspond`] finds, for every measured point (already moved by the current pose), its
//! closest point on the reference, the outward unit normal there, the reference entity and
//! the unsigned distance, or says why there is none. It is the only place the ICP loop
//! touches the reference. The closest-point arithmetic and the tie-break are GEO.DIST's; this
//! module chooses the normal and records every point's outcome.
//!
//! # References
//!
//! A [`Reference`] is either a mesh ([`SignTarget`]: GEO.DIST.MESH closest point and the
//! GEO.DIST.SIGN pseudo-normals, built once per mesh) or a set of CAD faces
//! ([`CadReference`]: a GEO.DIST.FACE.BODY [`FaceSetTarget`] and the GEO.DIST.FACE.INCID
//! incident normals of its body, built once). For a [`fid_geo_dist::BodyTarget`] or
//! [`fid_geo_dist::FaceTarget`] pass its `set()`. The `ProjectionTarget` trait of GEO.DIST.BATCH
//! does not expose the foot's feature or normal, so the search calls the targets' own
//! closest-point queries ([`fid_geo_dist::MeshTarget::closest`],
//! [`FaceSetTarget::closest`]) directly; it never computes the sign, which ICP does not need.
//!
//! # The normal (REQ-ALN.BFIT.ICP.CORR-005)
//!
//! - **Mesh, interior foot:** the triangle normal `(b - a) x (c - a)` (right-hand rule, the
//!   GEO.DIST.SIGN orientation), normalised.
//! - **Mesh, edge or vertex foot:** the GEO.DIST.SIGN pseudo-normal of that edge
//!   (`pi n1 + pi n2`) or vertex (angle-weighted sum), normalised.
//! - **CAD face:** the outward surface normal of the hit face at the foot's parameters
//!   (`Su x Sv / |Su x Sv|`, negated for a face of sense `Reversed`), as GEO.DIST.FACE.INCID
//!   computes it for an interior foot: also for a foot on the face's boundary edge or corner,
//!   since the leaf asks for the surface normal at the foot parameters. Where INCID's
//!   degenerate-normal fallback gives the normal (a surface pole), the point is listed in
//!   [`CorrOutput::notes`] as [`NoteKind::NormalFallback`]. Where INCID routes a foot on a NURBS
//!   seam to its edge rule (two incident normals), the normal is their angle-weighted sum,
//!   normalised, and the point is listed as [`NoteKind::SeamCombined`].
//!
//! Normalising first scales `n` by exact powers of two until `1 <= max |n_k| < 2` (so the
//! squares can neither overflow nor underflow, as GEO.DIST.FACE.BODY's `normalise_pow2`
//! does), then divides by `sqrt(n . n)`, `n . n = (n0 n0 + n1 n1) + n2 n2`, the square root
//! from `fid_num::f` (correctly rounded), one IEEE division per component.
//!
//! **Never a guessed normal.** Where GEO.DIST.SIGN or GEO.DIST.FACE.INCID give no normal (a
//! boundary or non-manifold edge, a vertex touching one, a misoriented component, a degenerate
//! triangle, a cone apex), or the normal has no certainly positive length, the point gets
//! [`CorrReason::NormalUndefined`] with that reason: it is not a valid correspondence, so
//! `ALN.BFIT.ICP.METRIC` builds no row for it, but its foot, entity and distance are the real
//! projection (they are recorded, not used). On an open reference mesh this drops the points
//! whose foot is on the mesh boundary, as many ICP variants do on purpose.
//!
//! # Outcome per point (REQ-001, REQ-002)
//!
//! In measured-index order, exactly one of:
//!
//! - [`CorrReason::Hit`]: foot, unit normal, entity (triangle index or face index of the body)
//!   and unsigned distance; `valid` is true.
//! - [`CorrReason::NormalUndefined`]: as above but the normal is the zero placeholder and
//!   `valid` is false.
//! - [`CorrReason::BeyondMaxDistance`]: a maximum distance is set and no allowed entity lies
//!   within it (GEO.DIST returns no hit), or the hit's computed distance exceeds it.
//! - [`CorrReason::NoHit`]: no maximum distance is set and the search found no foot (not
//!   expected: an empty reference or an empty patch list is refused before the search).
//!
//! For the last two every field but `reason` is a zero placeholder. [`CorrOutput::summary`]
//! counts each outcome, so nothing is dropped silently.
//!
//! # Ties (REQ-004)
//!
//! When several entities are equally close, GEO.DIST picks the lowest triangle index
//! (GEO.DIST.MESH, equal computed squared distances) or the lowest face topology ID within
//! 1e-15 m (GEO.DIST.FACE.BODY). This module does not break ties itself and cannot see that a
//! tie happened: GEO.DIST returns only the winner (recorded in the leaf's Open questions).
//!
//! # Refusals (fail closed, checked in this order)
//!
//! 1. The points' frame equals the reference's (`frame_mismatch`), before anything else.
//! 2. `1 <= threads <= 256` (`invalid_threads`).
//! 3. `max_distance` finite and at least [`MAX_DISTANCE_FLOOR_M`] (`invalid_max_distance`).
//!    The floor is GEO.DIST.FACE's foot accuracy ([`fid_geo_dist::FOOT_ERROR_M`], 1e-9 m): a
//!    smaller limit could not be told apart from the rounding of the foot, so it is refused,
//!    never clamped.
//! 4. Patches: not empty (`empty_patches`), strictly ascending and below the triangle count
//!    of the mesh or the face count of the body (`invalid_patches` with the position).
//! 5. The reference after the patches are applied: no triangle or face left to search
//!    (`empty_reference`; for CAD also a patch list that names no face of the set), or, for a
//!    mesh, no searchable triangle whose interior has a GEO.DIST.SIGN normal (every one
//!    degenerate or in a misoriented or non-manifold component: `degenerate_reference`, since
//!    no point could get a valid correspondence). A sliver among good triangles is not
//!    refused: a foot on it gets `normal_undefined`.
//! 6. The coordinate arrays have one entry per point (`inconsistent_points`), and there is
//!    at least one point (`no_points`).
//! 7. Every point, in index order: finite (`non_finite_query`) and within `2^250` m
//!    (`query_out_of_range`).
//!
//! Then the points are searched. A search or normal computation that fails is an error of
//! the whole call naming the lowest failing index (`Point`); a cancelled run returns
//! `cancelled` and no results (REQ-006 below).
//!
//! # Determinism (D0, REQ-006, KNP-1)
//!
//! Each point's outcome is a function of that point, the reference and the options only; no
//! state is shared between points. The points are cut into blocks of [`BLOCK`]; workers take
//! blocks from a shared queue and write each outcome into its slot by index, so the thread
//! count and the schedule cannot change a byte of the output. When several points fail, the
//! error of the lowest index is returned (blocks before a known failure always run). Binary64
//! `+ - * /` one operation at a time (no FMA, no `f64::min`/`max`), `fid_num` square root,
//! vectors only (no hash containers). Nothing panics; buffers are reserved fallibly.
//!
//! # Performance
//!
//! REQ-007 (one pass of 1,000,000 points against a 1,000,000-triangle mesh under 200 ms) is
//! `[unverified]`: not measured.

use core::f64::consts::TAU;
use core::fmt;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use fid_geo_dist::batch::MAX_THREADS;
use fid_geo_dist::mesh::COORD_LIMIT;
use fid_geo_dist::{
    BodyError, DistError, FaceFeature, FaceHit, FaceSetTarget, Feature, Incid, IncidError, MeshHit, MeshProjection,
    MeshQuery, ProjectionOptions, ProjectionTarget, SignTarget, SignUndefined, TriangleSubset, Undefined, FOOT_ERROR_M,
};
use fid_geo_frame::{require_same_frame_ref, GeoError};
use fid_geo_types::PointSet;

use super::{CorrReason, Correspondences};

type V3 = [f64; 3];

/// Points per work block of the parallel search.
pub const BLOCK: usize = 1024;

/// Smallest accepted maximum correspondence distance, metres: GEO.DIST.FACE's foot accuracy
/// [`fid_geo_dist::FOOT_ERROR_M`] (1e-9 m \[unverified\] upstream).
pub const MAX_DISTANCE_FLOOR_M: f64 = FOOT_ERROR_M;

// ------------------------------------------------------------------ references

/// A set of CAD faces prepared for correspondence search: the face-set target and the
/// incident-normal index of its body (GEO.DIST.FACE.INCID), built once.
pub struct CadReference<'s, 'b> {
    set: &'s FaceSetTarget<'b>,
    incid: Incid<'b>,
}

impl fmt::Debug for CadReference<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CadReference").field("set", self.set).finish_non_exhaustive()
    }
}

impl<'s, 'b> CadReference<'s, 'b> {
    /// Prepares `set` (its body's adjacency and pcurves, GEO.DIST.FACE.INCID). Refuses a body
    /// whose pcurves cannot be derived (`cad_reference`).
    pub fn new(set: &'s FaceSetTarget<'b>) -> Result<CadReference<'s, 'b>, CorrError> {
        let incid = Incid::new(set.body()).map_err(CorrError::CadReference)?;
        Ok(CadReference { set, incid })
    }

    /// The face-set target.
    pub fn set(&self) -> &'s FaceSetTarget<'b> {
        self.set
    }
}

/// The reference geometry of a correspondence search.
#[derive(Debug, Clone, Copy)]
pub enum Reference<'r, 's, 'b> {
    /// A triangle mesh with its hierarchy and pseudo-normals.
    Mesh(&'r SignTarget<'b>),
    /// A set of faces of a CAD body.
    Cad(&'r CadReference<'s, 'b>),
}

impl Reference<'_, '_, '_> {
    fn frame(&self) -> fid_geo_types::FrameRef {
        match self {
            Reference::Mesh(t) => ProjectionTarget::frame(*t),
            Reference::Cad(c) => ProjectionTarget::frame(c.set),
        }
    }

    /// Number of entities a patch ID may name: triangles of the mesh, faces of the body.
    fn entity_count(&self) -> u64 {
        match self {
            Reference::Mesh(t) => t.target().mesh().triangle_count(),
            Reference::Cad(c) => c.set.body().faces().len() as u64,
        }
    }

    /// Entities the search can return after the patches: `(searchable, with a normal)`, where
    /// the second counts mesh triangles whose interior has a GEO.DIST.SIGN normal (CAD faces
    /// count as having one; theirs is decided per foot).
    fn searchable(&self, patches: Option<&[u64]>) -> (u64, u64) {
        let allowed = |id: u64| patches.map_or(true, |p| p.binary_search(&id).is_ok());
        match self {
            Reference::Mesh(t) => {
                let normals = t.normals();
                let ok = |id: u64| matches!(normals.triangle(id), Some(None));
                let (mut n, mut good) = (0u64, 0u64);
                match patches {
                    Some(p) => {
                        for &id in p {
                            n += 1;
                            good += u64::from(ok(id));
                        }
                    }
                    None => {
                        for id in 0..t.target().mesh().triangle_count() {
                            n += 1;
                            good += u64::from(ok(id));
                        }
                    }
                }
                (n, good)
            }
            Reference::Cad(c) => {
                let n = c.set.faces().iter().filter(|f| allowed(f.index() as u64)).count() as u64;
                (n, n)
            }
        }
    }
}

// ------------------------------------------------------------------ options, output

/// Search options.
#[derive(Debug, Clone, Copy)]
pub struct CorrOptions<'p> {
    /// Maximum correspondence distance in metres (finite, at least [`MAX_DISTANCE_FLOOR_M`]);
    /// `None` searches without limit.
    pub max_distance: Option<f64>,
    /// Reference patches of a local scope: strictly ascending triangle indexes of the mesh, or
    /// face indexes of the body (`FaceIx::index`). `None` searches the whole reference.
    pub patches: Option<&'p [u64]>,
    /// Worker threads, `1..=256` (1 runs on the calling thread). Does not change the output.
    pub threads: usize,
}

impl Default for CorrOptions<'_> {
    fn default() -> Self {
        CorrOptions { max_distance: None, patches: None, threads: 1 }
    }
}

/// Why a point is listed in [`CorrOutput::notes`]: a valid correspondence whose normal came
/// from a non-default rule upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NoteKind {
    /// GEO.DIST.FACE.INCID gave the normal by its degenerate-normal fallback (a surface pole).
    NormalFallback,
    /// The foot lies on a NURBS seam: the normal combines both sides' normals.
    SeamCombined,
}

impl NoteKind {
    /// Stable code: `normal_fallback`, `seam_combined`.
    pub const fn code(self) -> &'static str {
        match self {
            NoteKind::NormalFallback => "normal_fallback",
            NoteKind::SeamCombined => "seam_combined",
        }
    }
}

/// A note on one measured point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CorrNote {
    /// The measured index.
    pub index: u64,
    /// What happened.
    pub kind: NoteKind,
}

/// Counts of the outcomes of one search; they add up to `points`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CorrSummary {
    /// Measured points searched.
    pub points: u64,
    /// Valid correspondences ([`CorrReason::Hit`]).
    pub hits: u64,
    /// Points with a foot but no defined normal ([`CorrReason::NormalUndefined`]).
    pub normal_undefined: u64,
    /// Points beyond the maximum distance ([`CorrReason::BeyondMaxDistance`]).
    pub beyond_max_distance: u64,
    /// Points without a foot ([`CorrReason::NoHit`]).
    pub no_hit: u64,
}

/// The correspondences of one search with their counts and notes. The parts agree by
/// construction, so they are only readable.
#[derive(Debug, Clone, PartialEq)]
pub struct CorrOutput {
    corr: Correspondences,
    summary: CorrSummary,
    notes: Vec<CorrNote>,
}

impl CorrOutput {
    /// One entry per measured point, in measured-index order.
    pub fn correspondences(&self) -> &Correspondences {
        &self.corr
    }

    /// The outcome counts.
    pub fn summary(&self) -> CorrSummary {
        self.summary
    }

    /// Notes on valid correspondences, ascending index.
    pub fn notes(&self) -> &[CorrNote] {
        &self.notes
    }

    /// The correspondences, for `ALN.BFIT.ICP.METRIC`.
    pub fn into_correspondences(self) -> Correspondences {
        self.corr
    }
}

// ------------------------------------------------------------------ errors

/// A failure of the search or the normal of one point.
#[derive(Debug, Clone, PartialEq)]
pub enum PointError {
    /// The mesh closest-point query refused (GEO.DIST.MESH).
    Dist(DistError),
    /// The CAD face-set query refused (GEO.DIST.FACE.BODY).
    Body(BodyError),
    /// The CAD incident normal could not be computed (GEO.DIST.FACE.INCID).
    Incid(IncidError),
    /// A hit names a triangle or vertex the mesh does not have (never expected).
    Inconsistent,
}

impl PointError {
    /// Stable code of the underlying error.
    pub fn code(&self) -> &'static str {
        match self {
            PointError::Dist(e) => e.code(),
            PointError::Body(e) => e.code(),
            PointError::Incid(e) => e.code(),
            PointError::Inconsistent => "inconsistent",
        }
    }
}

impl fmt::Display for PointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PointError::Dist(e) => write!(f, "{e}"),
            PointError::Body(e) => write!(f, "{e}"),
            PointError::Incid(e) => write!(f, "{e}"),
            PointError::Inconsistent => f.write_str("hit does not belong to the reference"),
        }
    }
}

/// Every way [`correspond`] refuses. Codes are stable; nothing panics.
#[derive(Debug, Clone, PartialEq)]
pub enum CorrError {
    /// The points and the reference are in different frames. Nothing was searched.
    FrameMismatch(GeoError),
    /// The thread count is zero or above 256.
    InvalidThreads { threads: usize },
    /// `max_distance` is NaN, infinite or below [`MAX_DISTANCE_FLOOR_M`].
    InvalidMaxDistance,
    /// The patch list is empty: nothing could correspond.
    EmptyPatches,
    /// The patch list is not strictly ascending or names an entity the reference lacks.
    InvalidPatches { position: u64 },
    /// The reference, after the patches, has no triangle or face to search.
    EmptyReference,
    /// No searchable mesh triangle has a defined normal: no point could correspond.
    DegenerateReference,
    /// The point set's coordinate arrays differ in length.
    InconsistentPoints,
    /// There are no measured points.
    NoPoints,
    /// A point coordinate is NaN or infinite.
    NonFiniteQuery { index: u64 },
    /// A point coordinate exceeds `2^250` m.
    QueryOutOfRange { index: u64 },
    /// A buffer could not be reserved.
    AllocationFailed { bytes: u64 },
    /// The search or normal of this point failed (the lowest such index).
    Point { index: u64, error: PointError },
    /// The cancellation flag was set. No results are returned.
    Cancelled,
    /// A worker thread or lock failed (never expected).
    WorkerFailed,
    /// [`CadReference::new`]: the body's incident-normal index could not be built.
    CadReference(IncidError),
}

impl CorrError {
    /// Stable code; for [`CorrError::FrameMismatch`] the frame error's code and for
    /// [`CorrError::Point`] the point error's.
    pub fn code(&self) -> &'static str {
        match self {
            CorrError::FrameMismatch(e) => e.code(),
            CorrError::InvalidThreads { .. } => "invalid_threads",
            CorrError::InvalidMaxDistance => "invalid_max_distance",
            CorrError::EmptyPatches => "empty_patches",
            CorrError::InvalidPatches { .. } => "invalid_patches",
            CorrError::EmptyReference => "empty_reference",
            CorrError::DegenerateReference => "degenerate_reference",
            CorrError::InconsistentPoints => "inconsistent_points",
            CorrError::NoPoints => "no_points",
            CorrError::NonFiniteQuery { .. } => "non_finite_query",
            CorrError::QueryOutOfRange { .. } => "query_out_of_range",
            CorrError::AllocationFailed { .. } => "allocation_failed",
            CorrError::Point { error, .. } => error.code(),
            CorrError::Cancelled => "cancelled",
            CorrError::WorkerFailed => "worker_failed",
            CorrError::CadReference(_) => "cad_reference",
        }
    }
}

impl fmt::Display for CorrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CorrError::FrameMismatch(e) => write!(f, "{e}"),
            CorrError::InvalidThreads { threads } => write!(f, "thread count {threads} is not in 1..=256"),
            CorrError::InvalidMaxDistance => f.write_str("maximum distance must be finite and at least 1e-9 m"),
            CorrError::EmptyPatches => f.write_str("the patch list is empty"),
            CorrError::InvalidPatches { position } => {
                write!(f, "patch entry {position} is not ascending or names no entity of the reference")
            }
            CorrError::EmptyReference => f.write_str("the reference has no triangle or face to search"),
            CorrError::DegenerateReference => f.write_str("no searchable triangle of the reference has a normal"),
            CorrError::InconsistentPoints => f.write_str("the point coordinate arrays differ in length"),
            CorrError::NoPoints => f.write_str("there are no measured points"),
            CorrError::NonFiniteQuery { index } => write!(f, "measured point {index} is not finite"),
            CorrError::QueryOutOfRange { index } => write!(f, "measured point {index} exceeds 2^250 m"),
            CorrError::AllocationFailed { bytes } => write!(f, "allocation of {bytes} bytes failed"),
            CorrError::Point { index, error } => write!(f, "measured point {index}: {error}"),
            CorrError::Cancelled => f.write_str("correspondence search cancelled"),
            CorrError::WorkerFailed => f.write_str("a worker thread failed"),
            CorrError::CadReference(e) => write!(f, "CAD reference: {e}"),
        }
    }
}

impl std::error::Error for CorrError {}

// ------------------------------------------------------------------ per point

/// The outcome of one point before it is spread into the structure of arrays.
#[derive(Debug, Clone, Copy)]
struct Item {
    foot: V3,
    normal: V3,
    entity: u64,
    dist: f64,
    reason: CorrReason,
    note: Option<NoteKind>,
}

impl Item {
    const EMPTY: Item =
        Item { foot: [0.0; 3], normal: [0.0; 3], entity: 0, dist: 0.0, reason: CorrReason::NoHit, note: None };

    fn missing(max_distance: Option<f64>) -> Item {
        let reason = if max_distance.is_some() { CorrReason::BeyondMaxDistance } else { CorrReason::NoHit };
        Item { reason, ..Item::EMPTY }
    }
}

/// `n` scaled by exact powers of two until `1 <= max |n_k| < 2` (zero stays zero).
fn normalise_pow2(v: V3) -> V3 {
    let mut m = 0.0;
    for x in v {
        if x.abs() > m {
            m = x.abs();
        }
    }
    let mut w = v;
    // Each loop is bounded by the exponent range (at most about 1100 steps in total).
    let mut steps = 0;
    while m >= 2.0 && steps < 2200 {
        w = [w[0] * 0.5, w[1] * 0.5, w[2] * 0.5];
        m *= 0.5;
        steps += 1;
    }
    while m > 0.0 && m < 1.0 && steps < 2200 {
        w = [w[0] * 2.0, w[1] * 2.0, w[2] * 2.0];
        m *= 2.0;
        steps += 1;
    }
    w
}

/// `n / |n|` after [`normalise_pow2`], or `None` when the length is not a positive finite
/// number.
fn unit(n: V3) -> Option<V3> {
    if !n.iter().all(|x| x.is_finite()) {
        return None;
    }
    let n = normalise_pow2(n);
    let nn = (n[0] * n[0] + n[1] * n[1]) + n[2] * n[2];
    let len = fid_num::f::sqrt(nn).ok()?;
    if !(len > 0.0 && len.is_finite()) {
        return None;
    }
    let u = [n[0] / len, n[1] / len, n[2] / len];
    u.iter().all(|x| x.is_finite()).then_some(u)
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// The outward unit normal of a mesh foot, or why GEO.DIST.SIGN defines none there.
fn mesh_normal(st: &SignTarget<'_>, hit: &MeshHit) -> Result<Result<V3, SignUndefined>, PointError> {
    let mesh = st.target().mesh();
    let normals = st.normals();
    let [ia, ib, ic] = mesh.triangle(hit.tri).ok_or(PointError::Inconsistent)?;
    let (normal, status) = match hit.feature {
        Feature::Interior => {
            let status = normals.triangle(hit.tri).ok_or(PointError::Inconsistent)?;
            let corner = |i: u64| mesh.vertex(i).ok_or(PointError::Inconsistent);
            let (a, b, c) = (corner(ia)?, corner(ib)?, corner(ic)?);
            (cross(sub(b, a), sub(c, a)), status)
        }
        Feature::Edge(k) => {
            let (s, e) = match k {
                0 => (ia, ib),
                1 => (ib, ic),
                2 => (ic, ia),
                _ => return Err(PointError::Inconsistent),
            };
            let en = normals.edge(s, e).ok_or(PointError::Inconsistent)?;
            (en.feature.normal, en.feature.status)
        }
        Feature::Vertex(k) => {
            let v = match k {
                0 => ia,
                1 => ib,
                2 => ic,
                _ => return Err(PointError::Inconsistent),
            };
            let vn = normals.vertex(v).ok_or(PointError::Inconsistent)?;
            (vn.normal, vn.status)
        }
    };
    if let Some(reason) = status {
        return Ok(Err(reason));
    }
    Ok(unit(normal).ok_or(SignUndefined::Degenerate))
}

/// The parameters of one search, shared by every point.
struct Query<'r, 's, 'b, 'p> {
    reference: Reference<'r, 's, 'b>,
    max_distance: Option<f64>,
    subset: Option<&'p TriangleSubset<'p>>,
}

impl Query<'_, '_, '_, '_> {
    fn beyond(&self, dist: f64) -> bool {
        self.max_distance.is_some_and(|d| dist > d)
    }

    fn one(&self, p: V3) -> Result<Item, PointError> {
        match self.reference {
            Reference::Mesh(st) => {
                let q = MeshQuery { max_distance: self.max_distance, subset: self.subset };
                let hit = match st.target().closest(p, &q).map_err(PointError::Dist)? {
                    MeshProjection::NoHit => return Ok(Item::missing(self.max_distance)),
                    MeshProjection::Hit(h) => h,
                };
                if self.beyond(hit.dist) {
                    return Ok(Item::missing(self.max_distance));
                }
                let base = Item { foot: hit.foot, entity: hit.tri, dist: hit.dist, ..Item::EMPTY };
                Ok(match mesh_normal(st, &hit)? {
                    Ok(normal) => Item { normal, reason: CorrReason::Hit, ..base },
                    Err(r) => Item { reason: CorrReason::NormalUndefined(r), ..base },
                })
            }
            Reference::Cad(cad) => {
                let opts = ProjectionOptions { max_distance: self.max_distance, subset: self.subset, direction: None };
                let (ix, hit) = match cad.set.closest(p, &opts).map_err(PointError::Body)? {
                    None => return Ok(Item::missing(self.max_distance)),
                    Some(x) => x,
                };
                if self.beyond(hit.distance) {
                    return Ok(Item::missing(self.max_distance));
                }
                let base = Item { foot: hit.foot, entity: ix.index() as u64, dist: hit.distance, ..Item::EMPTY };
                // The surface normal at the foot parameters: INCID's interior rule.
                let at = FaceHit { feature: FaceFeature::Interior, ..hit };
                match cad.incid.incident_at(&at) {
                    Ok(list) => Ok(cad_normal(&list, base)),
                    Err(Undefined::Sign { reason, .. }) => {
                        Ok(Item { reason: CorrReason::NormalUndefined(reason), ..base })
                    }
                    Err(Undefined::Error(e)) => Err(PointError::Incid(e)),
                }
            }
        }
    }
}

/// The normal of a CAD foot from INCID's incident list (one entry, or two on a NURBS seam).
fn cad_normal(list: &[fid_geo_dist::Incident], base: Item) -> Item {
    let undefined = Item { reason: CorrReason::NormalUndefined(SignUndefined::Degenerate), ..base };
    let (sum, fallback) = match list {
        [] => return undefined,
        [one] => (one.normal, one.fallback),
        many => {
            let mut s = [0.0; 3];
            let mut fb = false;
            for i in many {
                let w = i.angle / TAU;
                s = [s[0] + w * i.normal[0], s[1] + w * i.normal[1], s[2] + w * i.normal[2]];
                fb |= i.fallback;
            }
            (s, fb)
        }
    };
    let Some(normal) = unit(sum) else { return undefined };
    let note = if fallback {
        Some(NoteKind::NormalFallback)
    } else if list.len() > 1 {
        Some(NoteKind::SeamCombined)
    } else {
        None
    };
    Item { normal, reason: CorrReason::Hit, note, ..base }
}

// ------------------------------------------------------------------ the search

fn reserve<T>(v: &mut Vec<T>, n: usize) -> Result<(), CorrError> {
    v.try_reserve_exact(n)
        .map_err(|_| CorrError::AllocationFailed { bytes: (n as u64).saturating_mul(core::mem::size_of::<T>() as u64) })
}

/// Finds the correspondence of every point of `points` on `reference` (module
/// documentation): the order of refusals, the normal rule, the outcome per point and the
/// determinism rule. `cancel` is read before every block.
pub fn correspond(
    points: &PointSet,
    reference: Reference<'_, '_, '_>,
    opts: &CorrOptions<'_>,
    cancel: &AtomicBool,
) -> Result<CorrOutput, CorrError> {
    require_same_frame_ref(points.frame(), reference.frame()).map_err(CorrError::FrameMismatch)?;
    if opts.threads == 0 || opts.threads > MAX_THREADS {
        return Err(CorrError::InvalidThreads { threads: opts.threads });
    }
    if let Some(d) = opts.max_distance {
        if !(d.is_finite() && d >= MAX_DISTANCE_FLOOR_M) {
            return Err(CorrError::InvalidMaxDistance);
        }
    }
    let subset = match opts.patches {
        None => None,
        Some([]) => return Err(CorrError::EmptyPatches),
        Some(ids) => Some(TriangleSubset::new(ids, reference.entity_count()).map_err(|e| match e {
            DistError::InvalidSubset { position } => CorrError::InvalidPatches { position },
            _ => CorrError::InvalidPatches { position: 0 },
        })?),
    };
    let (searchable, with_normal) = reference.searchable(subset.as_ref().map(|s| s.ids()));
    if searchable == 0 {
        return Err(CorrError::EmptyReference);
    }
    if with_normal == 0 {
        return Err(CorrError::DegenerateReference);
    }
    let (xs, ys, zs) = (points.x(), points.y(), points.z());
    let n = xs.len();
    if ys.len() != n || zs.len() != n {
        return Err(CorrError::InconsistentPoints);
    }
    if n == 0 {
        return Err(CorrError::NoPoints);
    }
    for (i, ((&x, &y), &z)) in xs.iter().zip(ys).zip(zs).enumerate() {
        let p = [x, y, z];
        if !p.iter().all(|c| c.is_finite()) {
            return Err(CorrError::NonFiniteQuery { index: i as u64 });
        }
        if !p.iter().all(|c| (-COORD_LIMIT..=COORD_LIMIT).contains(c)) {
            return Err(CorrError::QueryOutOfRange { index: i as u64 });
        }
    }
    let query = Query { reference, max_distance: opts.max_distance, subset: subset.as_ref() };
    let items = search(&query, xs, ys, zs, opts.threads, cancel)?;
    assemble(&items)
}

/// The per-point outcomes in index order, computed on `threads` workers.
fn search(
    query: &Query<'_, '_, '_, '_>,
    xs: &[f64],
    ys: &[f64],
    zs: &[f64],
    threads: usize,
    cancel: &AtomicBool,
) -> Result<Vec<Item>, CorrError> {
    let n = xs.len();
    let mut out: Vec<Item> = Vec::new();
    reserve(&mut out, n)?;
    out.resize(n, Item::EMPTY);

    let first_error = AtomicU64::new(u64::MAX);
    let error: Mutex<Option<(u64, PointError)>> = Mutex::new(None);
    let failed = AtomicBool::new(false);
    {
        let mut blocks: Vec<(usize, &mut [Item])> = Vec::new();
        reserve(&mut blocks, n.div_ceil(BLOCK))?;
        // Reversed, so popping takes the blocks in ascending order.
        blocks.extend(out.chunks_mut(BLOCK).enumerate().rev());
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
            let start = bi.saturating_mul(BLOCK);
            if start as u64 > first_error.load(Ordering::Acquire) {
                continue;
            }
            for (k, slot) in chunk.iter_mut().enumerate() {
                let i = start + k;
                let (Some(&x), Some(&y), Some(&z)) = (xs.get(i), ys.get(i), zs.get(i)) else {
                    failed.store(true, Ordering::Release);
                    return;
                };
                match query.one([x, y, z]) {
                    Ok(it) => *slot = it,
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
            for _ in 1..threads {
                // A thread that cannot be started is not an error: the remaining workers (at
                // least the calling thread) take its blocks.
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
        return Err(CorrError::Cancelled);
    }
    if failed.load(Ordering::Acquire) {
        return Err(CorrError::WorkerFailed);
    }
    match error.into_inner() {
        Ok(Some((index, error))) => Err(CorrError::Point { index, error }),
        Ok(None) => Ok(out),
        Err(_) => Err(CorrError::WorkerFailed),
    }
}

/// Spreads the outcomes into the structure of arrays, with counts and notes.
fn assemble(items: &[Item]) -> Result<CorrOutput, CorrError> {
    let n = items.len();
    let mut corr = Correspondences::default();
    reserve(&mut corr.foot, n)?;
    reserve(&mut corr.normal, n)?;
    reserve(&mut corr.entity, n)?;
    reserve(&mut corr.dist, n)?;
    reserve(&mut corr.valid, n)?;
    reserve(&mut corr.reason, n)?;
    let mut summary = CorrSummary { points: n as u64, ..CorrSummary::default() };
    let mut notes = Vec::new();
    for (i, it) in items.iter().enumerate() {
        corr.foot.push(it.foot);
        corr.normal.push(it.normal);
        corr.entity.push(it.entity);
        corr.dist.push(it.dist);
        corr.valid.push(it.reason == CorrReason::Hit);
        corr.reason.push(it.reason);
        match it.reason {
            CorrReason::Hit => summary.hits += 1,
            CorrReason::NormalUndefined(_) => summary.normal_undefined += 1,
            CorrReason::BeyondMaxDistance => summary.beyond_max_distance += 1,
            CorrReason::NoHit => summary.no_hit += 1,
        }
        if let Some(kind) = it.note {
            notes.try_reserve(1).map_err(|_| CorrError::AllocationFailed { bytes: 16 })?;
            notes.push(CorrNote { index: i as u64, kind });
        }
    }
    Ok(CorrOutput { corr, summary, notes })
}
