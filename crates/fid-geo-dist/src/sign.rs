//! Signed distance and pseudo-normal rules. Spec node `GEO.DIST.SIGN`.
//!
//! [`PseudoNormals::build`] precomputes, once per mesh, the angle-weighted vertex pseudo-normals
//! and the edge pseudo-normals of Bærentzen and Aanæs (IEEE TVCG 11(3), 2005; the code is our
//! own), together with a topology status for every vertex, edge and triangle. [`SignTarget`]
//! answers [`SignTarget::signed_distance`]: the unsigned distance of GEO.DIST.MESH with a sign
//! that is either **certified** or explicitly not given. [`cad_sign`] applies the same weighting
//! to normals and angles supplied by a CAD caller (GEO.DIST.FACE).
//!
//! # The rule of record (REQ-001, REQ-002, REQ-004)
//!
//! Let `c*` be an exact closest point of the mesh to `p` (exact real arithmetic on the binary64
//! inputs) and `F*` the feature it lies on. The sign is the sign of `(p - c*) . N(F*)`, where
//! `N` is the triangle normal `(b - a) x (c - a)` of an interior foot (right-hand rule), the edge
//! pseudo-normal `pi n1 + pi n2` (unit normals of the two triangles), or the vertex pseudo-normal
//! `sum theta_t n_t` (unit normals weighted by the incident angles, in triangle index order).
//! For a closed, consistently oriented manifold mesh this sign is the inside/outside side of `p`
//! (Bærentzen and Aanæs, Theorem 1). Positive is outside.
//!
//! # Never a guess
//!
//! The rule is evaluated in binary64 with a running error bound for every quantity (below), so
//! every value is an interval `v +- e` that contains the exact value. A sign is reported only
//! when it is certain for **every** feature that could be `F*`:
//!
//! 1. `U`: an upper bound of the exact distance `d*`, from the triangle of the given hit (its
//!    three corners, the points `s + tau (e - s)` of its edges with `tau` the clamped float
//!    projection parameter, and the plane distance when the projection is certainly inside).
//! 2. Candidates: every triangle whose box lower bound (the pinned squared distance from `p` to
//!    the box of its corners, times `1 - 2^-50`, minus `2^-1022`) is at most
//!    `U2 = (U*U) (1 + 2^-50) + 2^-1022`. Found through the `fid-geo-index` hierarchy; the set
//!    does not depend on it (a leaf's bound never exceeds the bound of a triangle box inside it).
//! 3. For each candidate triangle, each of its seven features (vertices, edges `a-b`, `b-c`,
//!    `c-a`, interior) is **possible** unless its closed Voronoi-region predicate is certainly
//!    false (vertex `i`: `tn_i <= 0` and `tm_(i-1) >= 0`; edge `k`: `side_k <= 0`, `tn_k >= 0`,
//!    `tm_k <= 0`; interior: all `side_k >= 0`; with, for edge `k` from `s` to `e`,
//!    `ev = e - s`, `w = p - s`, `tn = w . ev`, `ee = ev . ev`, `tm = tn - ee`,
//!    `side = n . (ev x w)`), and it **survives** unless the lower end of its distance interval
//!    exceeds `U`. The exact closest feature always survives.
//! 4. Over all survivors: if any has a topology status other than OK, the result is
//!    [`SignStatus::Undefined`] with the most severe reason (REQ-003); else if every decision
//!    interval is certainly positive, [`SignStatus::Outside`]; if every one is certainly
//!    negative, [`SignStatus::Inside`]; otherwise [`SignStatus::Undecidable`].
//!
//! A hit at distance exactly zero is [`SignStatus::OnSurface`] with signed distance `+0.0`
//! (REQ-004), before any of the above. Every result carries [`Signed::bound`], a proven bound on
//! `|d* - distance|`: `max(U - d, d - L)` rounded up, `L` the least lower end over survivors.
//!
//! Decision values: vertex `(p - v) . N_v`; edge `r . N_e` with `t = tn / ee` and
//! `r = w - t ev` (the exact foot on the edge line); interior `(p - a) . n` (equal to
//! `(p - c*) . n`), distance `|(p - a) . n| / |n|`. A degenerate candidate (repeated indexes, or
//! a normal whose length is not certainly positive) survives when its box bound does, and is
//! [`SignUndefined::Degenerate`].
//!
//! # Topology (REQ-003, REQ-006)
//!
//! Half-edges `[lo, hi, (triangle << 1) | forward]` of every triangle without repeated indexes,
//! sorted (a total order, no hash containers). An edge used once is `Boundary`, more than twice
//! `NonManifold`, twice in the same direction `Misoriented`. Components (union-find over shared
//! edges, smallest root) that contain a non-manifold or misoriented edge have no defined
//! orientation: every triangle, edge and vertex in them is undefined with that reason (which side
//! of a misoriented pair is flipped is not decidable locally). A vertex touching a bad edge or a
//! degenerate triangle takes its reason; a vertex whose triangles do not form one fan (walking
//! across its edges does not visit them all) is `NonManifoldVertex`. Severity, most first:
//! `Degenerate`, `NonManifold`, `Misoriented`, `NonManifoldVertex`, `Boundary`.
//!
//! # Running error arithmetic (pinned)
//!
//! A value is `(v, e)` with `|exact - v| <= e`. Inputs are exact (`e = 0`). With
//! `grow(t) = t * (1 + 2^-48) + 2^-1022` (covering the at most eight roundings of an error
//! expression and any underflow) and `u = 2^-52`:
//! `a +- b = (a.v +- b.v, grow((a.e + b.e) + |v| u))`;
//! `a * b = (a.v * b.v, grow(((|a.v| b.e + |b.v| a.e) + a.e b.e) + |v| u))`;
//! `a / b` only when `|b.v| - b.e > 0`: `(a.v / b.v, grow((a.e + |v| b.e) / (|b.v| - b.e) + |v| u))`;
//! `sqrt(a)` (exact value known `>= 0`): `a.v <= 0` gives `(0, grow(sqrt(a.e)))`, else
//! `r = sqrt(a.v)`, `(r, grow(min(a.e / r, sqrt(a.e)) + r u))`. `dot` and `cross` follow
//! GEO.DIST.MESH's pinned order. Certainly positive: `v > e`; certainly negative: `-v > e`.
//! Upper end `v + e` and lower end `v - e` are rounded outward by `(1 +- 2^-50)` and `2^-1022`.
//!
//! Angles: `x = e1 . e2`, `y = sqrt(|e1 x e2|^2)`, `theta = atan2(y, x)` (`fid_num`), with
//! `delta = x.e + y.e`, `r = max(|x.v| - x.e, |y.v| - y.e) (1 - 2^-50) - 2^-1022`; the triangle
//! is degenerate unless `r > delta (1 + 2^-50)`; then the error is
//! `grow(2 delta / r + 2^-49)`: `2 delta / r` bounds the angle between the computed and exact
//! `(x, y)`, and `2^-49` (four ulps of pi) bounds `fid_num::f::atan2`'s own error, taken from
//! KRN.NUM.FN's 1-ulp claim (REQ-KRN.NUM.FN-006, held on its self-test inputs; for every input it
//! is [unverified]). Unit normals are `n_k / sqrt(n . n)` with these rules.
//!
//! # Determinism (D0, KNP-1)
//!
//! Binary64 `+ - * /` one operation at a time (no FMA, no `f64::min`/`max`), `fid_num` sqrt and
//! atan2, sorted vectors only, no hash containers; candidate order does not affect the result
//! (it only takes minima and set unions). The independent model
//! `tools/fixtures/geo_dist_sign.py` reproduces statuses, distances and bounds bit for bit.

use core::f64::consts::{PI, TAU};
use core::fmt;
use core::ops::ControlFlow;

use fid_geo_types::{FrameRef, Mesh};

use crate::mesh::{DistError, MeshHit, MeshProjection, MeshQuery, MeshTarget, COORD_LIMIT};

type V3 = [f64; 3];

/// `2^-52`, the rounding unit used by the error rules (twice the unit roundoff).
const U52: f64 = f64::from_bits((1023 - 52) << 52);
/// `1 + 2^-48`.
const GROW: f64 = f64::from_bits(0x3FF0_0000_0000_0010);
/// `1 + 2^-50`.
const UP: f64 = f64::from_bits(0x3FF0_0000_0000_0004);
/// `1 - 2^-50`.
const DOWN: f64 = f64::from_bits(0x3FEF_FFFF_FFFF_FFF8);
/// `2^-1022`, covering underflow.
const ETA: f64 = f64::MIN_POSITIVE;
/// Assumed absolute error of `fid_num::f::atan2` on `[0, pi]`: `2^-49` [unverified].
pub const ATAN2_ERROR: f64 = f64::from_bits((1023 - 49) << 52);

// ------------------------------------------------------------------ public types

/// Why a sign is not defined at a feature (REQ-GEO.DIST.SIGN-003). Declared most severe first;
/// when several apply, the most severe is reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SignUndefined {
    /// A triangle with repeated indexes, or whose normal is not certainly non-zero.
    Degenerate,
    /// An edge used by more than two triangles, or a component containing one.
    NonManifold,
    /// An edge used twice in the same direction, or a component containing one.
    Misoriented,
    /// A vertex whose triangles form more than one fan.
    NonManifoldVertex,
    /// An edge used by one triangle, or a vertex touching one.
    Boundary,
}

impl SignUndefined {
    /// A stable code.
    pub fn code(self) -> &'static str {
        match self {
            SignUndefined::Degenerate => "degenerate",
            SignUndefined::NonManifold => "non_manifold",
            SignUndefined::Misoriented => "misoriented",
            SignUndefined::NonManifoldVertex => "non_manifold_vertex",
            SignUndefined::Boundary => "boundary",
        }
    }

    fn rank(self) -> u8 {
        match self {
            SignUndefined::Degenerate => 0,
            SignUndefined::NonManifold => 1,
            SignUndefined::Misoriented => 2,
            SignUndefined::NonManifoldVertex => 3,
            SignUndefined::Boundary => 4,
        }
    }
}

/// The topology status of a vertex, edge or triangle.
pub type Topology = Option<SignUndefined>;

fn worse(a: Topology, b: Topology) -> Topology {
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some(x), Some(y)) => Some(if y.rank() < x.rank() { y } else { x }),
    }
}

/// The outcome of a sign query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignStatus {
    /// Certainly on the side of the outward normal.
    Outside,
    /// Certainly on the other side.
    Inside,
    /// The computed distance is exactly zero; the signed distance is `+0.0`.
    OnSurface,
    /// Rounding (or a nearer-than-rounding second sheet) could change the sign; none is given.
    Undecidable,
    /// The rule does not define a sign at a possible foot feature (REQ-003).
    Undefined(SignUndefined),
}

impl SignStatus {
    /// A stable numeric code: 0 outside, 1 inside, 2 on surface, 3 undecidable, `10 + rank`
    /// undefined (in [`SignUndefined`] declaration order).
    pub fn code(self) -> u8 {
        match self {
            SignStatus::Outside => 0,
            SignStatus::Inside => 1,
            SignStatus::OnSurface => 2,
            SignStatus::Undecidable => 3,
            SignStatus::Undefined(r) => 10 + r.rank(),
        }
    }
}

/// A signed distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Signed {
    /// `+d` outside, `-d` inside, `+0.0` on the surface; the unsigned `d` when no sign is given.
    pub distance: f64,
    /// The unsigned distance `d` (the hit's distance).
    pub unsigned: f64,
    /// The status.
    pub status: SignStatus,
    /// A proven bound on `|exact distance - unsigned|`, in metres.
    pub bound: f64,
}

impl Signed {
    /// True when `distance` carries a sign of record (outside, inside or on the surface).
    pub fn sign_defined(&self) -> bool {
        matches!(self.status, SignStatus::Outside | SignStatus::Inside | SignStatus::OnSurface)
    }
}

/// Every error of this module. Errors are values; nothing panics.
#[derive(Debug, Clone, PartialEq)]
pub enum SignError {
    /// An error of the underlying mesh projection.
    Dist(DistError),
    /// A corner of a mesh triangle exceeds [`COORD_LIMIT`].
    BadTriangle {
        /// Triangle index.
        triangle: u64,
        /// Corner index (0, 1, 2).
        corner: u8,
    },
    /// The pseudo-normals were built for another mesh (frame or counts differ).
    NormalsMismatch,
    /// The hit names no triangle of the mesh or has a negative or non-finite distance.
    InvalidHit,
    /// The mesh has no triangle, so there is nothing to measure against.
    EmptyMesh,
    /// An input of [`cad_sign`] is out of its domain.
    BadCadInput {
        /// The offending field.
        field: &'static str,
        /// The incident entry, for per-entry fields.
        index: u64,
    },
    /// A working allocation failed.
    AllocationFailed {
        /// Bytes requested.
        bytes: u64,
    },
    /// An internal invariant failed (never expected).
    Inconsistent,
}

impl SignError {
    /// A stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            SignError::Dist(e) => e.code(),
            SignError::BadTriangle { .. } => "bad_triangle",
            SignError::NormalsMismatch => "normals_mismatch",
            SignError::InvalidHit => "invalid_hit",
            SignError::EmptyMesh => "empty_mesh",
            SignError::BadCadInput { .. } => "bad_cad_input",
            SignError::AllocationFailed { .. } => "allocation_failed",
            SignError::Inconsistent => "inconsistent",
        }
    }
}

impl fmt::Display for SignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SignError::Dist(e) => write!(f, "{e}"),
            SignError::BadTriangle { triangle, corner } => {
                write!(f, "corner {corner} of triangle {triangle} exceeds 2^250 m")
            }
            SignError::NormalsMismatch => write!(f, "pseudo-normals were built for another mesh"),
            SignError::InvalidHit => write!(f, "hit does not belong to this mesh"),
            SignError::EmptyMesh => write!(f, "mesh has no triangle"),
            SignError::BadCadInput { field, index } => write!(f, "cad_sign input {field} (entry {index}) is invalid"),
            SignError::AllocationFailed { bytes } => write!(f, "allocation of {bytes} bytes failed"),
            SignError::Inconsistent => write!(f, "internal invariant failed"),
        }
    }
}

impl std::error::Error for SignError {}

impl From<DistError> for SignError {
    fn from(e: DistError) -> SignError {
        SignError::Dist(e)
    }
}

/// A precomputed pseudo-normal with its proven componentwise error bound.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeatureNormal {
    /// The computed pseudo-normal (zero when `status` is not OK).
    pub normal: [f64; 3],
    /// `|normal_k - exact_k| <= error_k`.
    pub error: [f64; 3],
    /// The topology status (`None` is OK).
    pub status: Topology,
}

/// One undirected edge of the sorted edge table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeNormal {
    /// Smaller vertex index.
    pub lo: u64,
    /// Larger vertex index.
    pub hi: u64,
    /// The pseudo-normal `pi n1 + pi n2` (OK edges) and its status.
    pub feature: FeatureNormal,
    /// The two triangles of an OK edge, ascending (else the first two uses).
    tris: [u64; 2],
}

/// Angle-weighted pseudo-normals and topology statuses of one mesh (REQ-002, REQ-006).
#[derive(Debug, Clone, PartialEq)]
pub struct PseudoNormals {
    frame: FrameRef,
    vertex: Vec<FeatureNormal>,
    edge: Vec<EdgeNormal>,
    triangle: Vec<Topology>,
}

// ------------------------------------------------------------------ running error arithmetic

#[derive(Debug, Clone, Copy)]
struct Ev {
    v: f64,
    e: f64,
}

type E3 = [Ev; 3];

fn grow(t: f64) -> f64 {
    t * GROW + ETA
}

fn fsqrt(x: f64) -> Option<f64> {
    fid_num::f::sqrt(x).ok()
}

impl Ev {
    const fn x(v: f64) -> Ev {
        Ev { v, e: 0.0 }
    }

    fn add(self, o: Ev) -> Ev {
        let v = self.v + o.v;
        Ev { v, e: grow((self.e + o.e) + v.abs() * U52) }
    }

    fn sub(self, o: Ev) -> Ev {
        let v = self.v - o.v;
        Ev { v, e: grow((self.e + o.e) + v.abs() * U52) }
    }

    fn mul(self, o: Ev) -> Ev {
        let v = self.v * o.v;
        Ev { v, e: grow(((self.v.abs() * o.e + o.v.abs() * self.e) + self.e * o.e) + v.abs() * U52) }
    }

    fn div(self, o: Ev) -> Option<Ev> {
        let d = o.v.abs() - o.e;
        if d.is_nan() || d <= 0.0 {
            return None;
        }
        let v = self.v / o.v;
        Some(Ev { v, e: grow((self.e + v.abs() * o.e) / d + v.abs() * U52) })
    }

    /// Square root of a quantity whose exact value is known to be non-negative.
    fn sqrt(self) -> Option<Ev> {
        if self.v <= 0.0 {
            return Some(Ev { v: 0.0, e: grow(fsqrt(self.e)?) });
        }
        let r = fsqrt(self.v)?;
        let a = self.e / r;
        let b = fsqrt(self.e)?;
        let m = if a < b { a } else { b };
        Some(Ev { v: r, e: grow(m + r * U52) })
    }

    fn abs(self) -> Ev {
        Ev { v: self.v.abs(), e: self.e }
    }

    /// The exact value is certainly `> 0`.
    fn pos(self) -> bool {
        self.v > self.e
    }

    /// The exact value is certainly `< 0`.
    fn neg(self) -> bool {
        -self.v > self.e
    }

    fn hi(self) -> f64 {
        let s = self.v + self.e;
        if s >= 0.0 {
            s * UP + ETA
        } else {
            s * DOWN + ETA
        }
    }

    fn lo(self) -> f64 {
        let s = self.v - self.e;
        if s >= 0.0 {
            s * DOWN - ETA
        } else {
            s * UP - ETA
        }
    }
}

fn ex3(p: &V3) -> E3 {
    [Ev::x(p[0]), Ev::x(p[1]), Ev::x(p[2])]
}

fn vsub(x: &E3, y: &E3) -> E3 {
    [x[0].sub(y[0]), x[1].sub(y[1]), x[2].sub(y[2])]
}

fn vdot(x: &E3, y: &E3) -> Ev {
    x[0].mul(y[0]).add(x[1].mul(y[1])).add(x[2].mul(y[2]))
}

fn vcross(x: &E3, y: &E3) -> E3 {
    [x[1].mul(y[2]).sub(x[2].mul(y[1])), x[2].mul(y[0]).sub(x[0].mul(y[2])), x[0].mul(y[1]).sub(x[1].mul(y[0]))]
}

fn stored(n: &FeatureNormal) -> E3 {
    [Ev { v: n.normal[0], e: n.error[0] }, Ev { v: n.normal[1], e: n.error[1] }, Ev { v: n.normal[2], e: n.error[2] }]
}

/// The angle between exact vectors with `x = e1 . e2`, `y = |e1 x e2|`, or `None` if it cannot
/// be bounded.
fn angle(x: Ev, y: Ev) -> Option<Ev> {
    let delta = x.e + y.e;
    let rx = x.v.abs() - x.e;
    let ry = y.v.abs() - y.e;
    let r = if rx > ry { rx } else { ry };
    let r = r * DOWN - ETA;
    let need = delta * UP;
    if r.is_nan() || need.is_nan() || r <= need {
        return None;
    }
    let theta = fid_num::f::atan2(y.v, x.v).ok()?;
    Some(Ev { v: theta, e: grow((2.0 * delta) / r + ATAN2_ERROR) })
}

// ------------------------------------------------------------------ inputs

fn in_range(p: &V3) -> bool {
    p.iter().all(|&x| (-COORD_LIMIT..=COORD_LIMIT).contains(&x))
}

fn check_query(p: &V3) -> Result<(), SignError> {
    if !p.iter().all(|x| x.is_finite()) {
        return Err(SignError::Dist(DistError::NonFiniteQuery));
    }
    if !in_range(p) {
        return Err(SignError::Dist(DistError::QueryOutOfRange));
    }
    Ok(())
}

fn tri_index(mesh: &Mesh, t: u64) -> Result<[u64; 3], SignError> {
    mesh.triangle(t).ok_or(SignError::Inconsistent)
}

fn corners(mesh: &Mesh, tri: &[u64; 3]) -> Result<[V3; 3], SignError> {
    let mut c = [[0.0; 3]; 3];
    for (slot, &v) in c.iter_mut().zip(tri.iter()) {
        *slot = mesh.vertex(v).ok_or(SignError::Inconsistent)?;
    }
    Ok(c)
}

fn repeated(t: &[u64; 3]) -> bool {
    t[0] == t[1] || t[1] == t[2] || t[0] == t[2]
}

fn reserve<T>(v: &mut Vec<T>, n: u64) -> Result<(), SignError> {
    let bytes = n.saturating_mul(core::mem::size_of::<T>() as u64);
    let n = usize::try_from(n).map_err(|_| SignError::AllocationFailed { bytes })?;
    v.try_reserve_exact(n).map_err(|_| SignError::AllocationFailed { bytes })
}

// ------------------------------------------------------------------ precomputation

/// Unit normal and corner angles of one triangle, or `None` when degenerate.
struct TriGeo {
    nhat: E3,
    angle: [Ev; 3],
}

fn tri_geo(c: &[V3; 3]) -> Option<TriGeo> {
    let [a, b, cc] = [ex3(&c[0]), ex3(&c[1]), ex3(&c[2])];
    let n = vcross(&vsub(&b, &a), &vsub(&cc, &a));
    let len = vdot(&n, &n).sqrt()?;
    if !len.pos() {
        return None;
    }
    let nhat = [n[0].div(len)?, n[1].div(len)?, n[2].div(len)?];
    let mut ang = [Ev::x(0.0); 3];
    let pts = [&a, &b, &cc];
    for (i, slot) in ang.iter_mut().enumerate() {
        let q = pts[i];
        let e1 = vsub(pts[(i + 1) % 3], q);
        let e2 = vsub(pts[(i + 2) % 3], q);
        let cr = vcross(&e1, &e2);
        *slot = angle(vdot(&e1, &e2), vdot(&cr, &cr).sqrt()?)?;
    }
    Some(TriGeo { nhat, angle: ang })
}

fn find(parent: &mut [u64], mut x: u64) -> u64 {
    // Parents never exceed their child, so the walk strictly decreases and ends.
    loop {
        let p = match usize::try_from(x).ok().and_then(|i| parent.get(i)) {
            Some(&p) => p,
            None => return x,
        };
        if p == x {
            return x;
        }
        let gp = usize::try_from(p).ok().and_then(|i| parent.get(i)).copied().unwrap_or(p);
        if let Some(slot) = usize::try_from(x).ok().and_then(|i| parent.get_mut(i)) {
            *slot = gp;
        }
        x = gp;
    }
}

fn union(parent: &mut [u64], a: u64, b: u64) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
        if let Some(slot) = usize::try_from(hi).ok().and_then(|i| parent.get_mut(i)) {
            *slot = lo;
        }
    }
}

fn at<T: Copy>(v: &[T], i: u64) -> Result<T, SignError> {
    usize::try_from(i).ok().and_then(|i| v.get(i)).copied().ok_or(SignError::Inconsistent)
}

fn at_mut<T>(v: &mut [T], i: u64) -> Result<&mut T, SignError> {
    usize::try_from(i).ok().and_then(|i| v.get_mut(i)).ok_or(SignError::Inconsistent)
}

const ZERO_NORMAL: FeatureNormal = FeatureNormal { normal: [0.0; 3], error: [0.0; 3], status: None };

impl PseudoNormals {
    /// Precomputes the pseudo-normals and topology statuses of `mesh` (rules in the
    /// [module documentation](self)). Cost `O(T log T)` for `T` triangles; deterministic (D0).
    #[allow(clippy::needless_range_loop)] // per-axis and per-corner indexing of parallel arrays, as pinned
    pub fn build(mesh: &Mesh) -> Result<PseudoNormals, SignError> {
        let nt = mesh.triangle_count();
        let nv = mesh.vertex_count();
        // Triangle geometry, in index order.
        let mut geo: Vec<Option<TriGeo>> = Vec::new();
        reserve(&mut geo, nt)?;
        let mut tri_status: Vec<Topology> = Vec::new();
        reserve(&mut tri_status, nt)?;
        let mut half: Vec<[u64; 3]> = Vec::new();
        reserve(&mut half, nt.saturating_mul(3))?;
        let mut incid: Vec<[u64; 2]> = Vec::new();
        reserve(&mut incid, nt.saturating_mul(3))?;
        for t in 0..nt {
            let tri = tri_index(mesh, t)?;
            let c = corners(mesh, &tri)?;
            for (k, v) in c.iter().enumerate() {
                if !in_range(v) {
                    return Err(SignError::BadTriangle { triangle: t, corner: k as u8 });
                }
            }
            if repeated(&tri) {
                geo.push(None);
                tri_status.push(Some(SignUndefined::Degenerate));
                for (k, &v) in tri.iter().enumerate() {
                    if !tri[..k].contains(&v) {
                        incid.push([v, t]);
                    }
                }
                continue;
            }
            let g = tri_geo(&c);
            tri_status.push(if g.is_some() { None } else { Some(SignUndefined::Degenerate) });
            geo.push(g);
            let tag = t.checked_mul(2).ok_or(SignError::Inconsistent)?;
            for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                half.push(if a < b { [a, b, tag | 1] } else { [b, a, tag] });
            }
            for &v in &tri {
                incid.push([v, t]);
            }
        }
        half.sort_unstable();
        incid.sort_unstable();

        // Edge runs: base status, components.
        let mut parent: Vec<u64> = Vec::new();
        reserve(&mut parent, nt)?;
        parent.extend(0..nt);
        let mut edges: Vec<EdgeNormal> = Vec::new();
        reserve(&mut edges, half.len() as u64)?;
        for run in half.chunk_by(|x, y| x[0] == y[0] && x[1] == y[1]) {
            let (first, rest) = run.split_first().ok_or(SignError::Inconsistent)?;
            let status = match run {
                [_] => Some(SignUndefined::Boundary),
                [x, y] if (x[2] & 1) == (y[2] & 1) => Some(SignUndefined::Misoriented),
                [_, _] => None,
                _ => Some(SignUndefined::NonManifold),
            };
            let t0 = first[2] >> 1;
            let t1 = rest.first().map_or(t0, |h| h[2] >> 1);
            for h in rest {
                union(&mut parent, t0, h[2] >> 1);
            }
            let feature = FeatureNormal { status, ..ZERO_NORMAL };
            edges.push(EdgeNormal { lo: first[0], hi: first[1], feature, tris: [t0, t1] });
        }
        // Component flags: the most severe non-manifold or misoriented edge of each root.
        let mut comp: Vec<Topology> = Vec::new();
        reserve(&mut comp, nt)?;
        comp.extend((0..nt).map(|_| None));
        for run in half.chunk_by(|x, y| x[0] == y[0] && x[1] == y[1]) {
            let status = match run {
                [_] => None,
                [x, y] if (x[2] & 1) == (y[2] & 1) => Some(SignUndefined::Misoriented),
                [_, _] => None,
                _ => Some(SignUndefined::NonManifold),
            };
            if status.is_some() {
                let root = find(&mut parent, run[0][2] >> 1);
                let slot = at_mut(&mut comp, root)?;
                *slot = worse(*slot, status);
            }
        }
        for t in 0..nt {
            if at(&tri_status, t)?.is_none() {
                let root = find(&mut parent, t);
                let flag = at(&comp, root)?;
                *at_mut(&mut tri_status, t)? = flag;
            }
        }
        // Edge statuses and pseudo-normals.
        for run_start in 0..edges.len() {
            let ed = edges.get(run_start).copied().ok_or(SignError::Inconsistent)?;
            let mut status = ed.feature.status;
            for &h in half_run(&half, ed.lo, ed.hi) {
                status = worse(status, at(&tri_status, h[2] >> 1)?);
            }
            let mut feature = FeatureNormal { status, ..ZERO_NORMAL };
            if status.is_none() {
                let g0 = geo.get(ix(ed.tris[0])?).and_then(|g| g.as_ref()).ok_or(SignError::Inconsistent)?;
                let g1 = geo.get(ix(ed.tris[1])?).and_then(|g| g.as_ref()).ok_or(SignError::Inconsistent)?;
                let w = Ev::x(PI);
                for k in 0..3 {
                    let s = w.mul(g0.nhat[k]).add(w.mul(g1.nhat[k]));
                    feature.normal[k] = s.v;
                    feature.error[k] = s.e;
                }
            }
            if let Some(slot) = edges.get_mut(run_start) {
                slot.feature = feature;
            }
        }
        // Vertex statuses, fan check and pseudo-normals.
        let mut vertex: Vec<FeatureNormal> = Vec::new();
        reserve(&mut vertex, nv)?;
        vertex.extend((0..nv).map(|_| FeatureNormal { status: Some(SignUndefined::Boundary), ..ZERO_NORMAL }));
        for run in incid.chunk_by(|x, y| x[0] == y[0]) {
            let v = run[0][0];
            let mut status: Topology = None;
            for &[_, t] in run {
                status = worse(status, at(&tri_status, t)?);
                let tri = tri_index(mesh, t)?;
                if !repeated(&tri) {
                    for &w in &tri {
                        if w != v {
                            status = worse(status, edge_of(&edges, v, w)?.feature.status);
                        }
                    }
                }
            }
            if status.is_none() && !single_fan(mesh, &edges, v, run)? {
                status = Some(SignUndefined::NonManifoldVertex);
            }
            let mut feature = FeatureNormal { status, ..ZERO_NORMAL };
            if status.is_none() {
                let mut acc = [Ev::x(0.0); 3];
                for &[_, t] in run {
                    let tri = tri_index(mesh, t)?;
                    let g = geo.get(ix(t)?).and_then(|g| g.as_ref()).ok_or(SignError::Inconsistent)?;
                    let corner = tri.iter().position(|&w| w == v).ok_or(SignError::Inconsistent)?;
                    let th = g.angle.get(corner).copied().ok_or(SignError::Inconsistent)?;
                    for (k, slot) in acc.iter_mut().enumerate() {
                        *slot = slot.add(th.mul(g.nhat[k]));
                    }
                }
                for k in 0..3 {
                    feature.normal[k] = acc[k].v;
                    feature.error[k] = acc[k].e;
                }
            }
            *at_mut(&mut vertex, v)? = feature;
        }
        Ok(PseudoNormals { frame: mesh.frame(), vertex, edge: edges, triangle: tri_status })
    }

    /// The pseudo-normal and status of vertex `v`.
    pub fn vertex(&self, v: u64) -> Option<&FeatureNormal> {
        usize::try_from(v).ok().and_then(|i| self.vertex.get(i))
    }

    /// The edge table, sorted by `(lo, hi)`.
    pub fn edges(&self) -> &[EdgeNormal] {
        &self.edge
    }

    /// The pseudo-normal and status of the edge between vertices `a` and `b`.
    pub fn edge(&self, a: u64, b: u64) -> Option<&EdgeNormal> {
        edge_of(&self.edge, a, b).ok()
    }

    /// The status of triangle `t`'s interior.
    pub fn triangle(&self, t: u64) -> Option<Topology> {
        usize::try_from(t).ok().and_then(|i| self.triangle.get(i)).copied()
    }

    fn matches(&self, mesh: &Mesh) -> bool {
        self.frame == mesh.frame()
            && self.vertex.len() as u64 == mesh.vertex_count()
            && self.triangle.len() as u64 == mesh.triangle_count()
    }
}

fn ix(t: u64) -> Result<usize, SignError> {
    usize::try_from(t).map_err(|_| SignError::Inconsistent)
}

fn half_run(half: &[[u64; 3]], lo: u64, hi: u64) -> &[[u64; 3]] {
    let start = half.partition_point(|h| (h[0], h[1]) < (lo, hi));
    let end = half.partition_point(|h| (h[0], h[1]) <= (lo, hi));
    half.get(start..end).unwrap_or(&[])
}

fn edge_of(edges: &[EdgeNormal], a: u64, b: u64) -> Result<&EdgeNormal, SignError> {
    let key = if a < b { (a, b) } else { (b, a) };
    let i = edges.binary_search_by(|e| (e.lo, e.hi).cmp(&key)).map_err(|_| SignError::Inconsistent)?;
    edges.get(i).ok_or(SignError::Inconsistent)
}

/// Walks across the edges at `v` starting from its lowest triangle; one fan visits all of them.
/// Called only when every edge at `v` is OK (two opposite uses).
fn single_fan(mesh: &Mesh, edges: &[EdgeNormal], v: u64, run: &[[u64; 2]]) -> Result<bool, SignError> {
    let k = run.len();
    let t0 = run.first().ok_or(SignError::Inconsistent)?[1];
    let mut cur = t0;
    for step in 1..=k {
        let tri = tri_index(mesh, cur)?;
        let i = tri.iter().position(|&w| w == v).ok_or(SignError::Inconsistent)?;
        let x = tri[(i + 1) % 3];
        let e = edge_of(edges, v, x)?;
        cur = if e.tris[0] == cur { e.tris[1] } else { e.tris[0] };
        if cur == t0 {
            return Ok(step == k);
        }
    }
    Ok(false)
}

// ------------------------------------------------------------------ per-triangle analysis

/// The quantities of one triangle shared by the upper bound and the feature analysis.
struct TriVals {
    pts: [E3; 3],
    n: E3,
    w: [E3; 3],
    ev: [E3; 3],
    tn: [Ev; 3],
    ee: [Ev; 3],
    tm: [Ev; 3],
    side: [Ev; 3],
}

#[allow(clippy::needless_range_loop)] // per-axis and per-corner indexing of parallel arrays, as pinned
fn tri_vals(p: &V3, c: &[V3; 3]) -> TriVals {
    let pts = [ex3(&c[0]), ex3(&c[1]), ex3(&c[2])];
    let pe = ex3(p);
    let n = vcross(&vsub(&pts[1], &pts[0]), &vsub(&pts[2], &pts[0]));
    let mut w = [[Ev::x(0.0); 3]; 3];
    let mut ev = [[Ev::x(0.0); 3]; 3];
    let mut tn = [Ev::x(0.0); 3];
    let mut ee = [Ev::x(0.0); 3];
    let mut tm = [Ev::x(0.0); 3];
    let mut side = [Ev::x(0.0); 3];
    for k in 0..3 {
        let s = &pts[k];
        let e = &pts[(k + 1) % 3];
        ev[k] = vsub(e, s);
        w[k] = vsub(&pe, s);
        tn[k] = vdot(&w[k], &ev[k]);
        ee[k] = vdot(&ev[k], &ev[k]);
        tm[k] = tn[k].sub(ee[k]);
        side[k] = vdot(&n, &vcross(&ev[k], &w[k]));
    }
    TriVals { pts, n, w, ev, tn, ee, tm, side }
}

/// Distance of the exact point `p - r` where `r` is given as a running-error vector.
fn norm(r: &E3) -> Option<Ev> {
    vdot(r, r).sqrt()
}

/// An upper bound of the exact distance from `p` to the closed triangle.
#[allow(clippy::needless_range_loop)] // per-axis and per-corner indexing of parallel arrays, as pinned
fn upper_on_triangle(p: &V3, c: &[V3; 3]) -> Result<f64, SignError> {
    let tv = tri_vals(p, c);
    let pe = ex3(p);
    let mut best = f64::INFINITY;
    let mut take = |d: Option<Ev>| {
        if let Some(d) = d {
            let h = d.hi();
            if h < best {
                best = h;
            }
        }
    };
    for q in &tv.pts {
        take(norm(&vsub(&pe, q)));
    }
    for k in 0..3 {
        if tv.ee[k].v > 0.0 {
            let t = tv.tn[k].v / tv.ee[k].v;
            // KNP-1 bans f64::clamp; the comparisons are the pinned clamp.
            #[allow(clippy::manual_clamp)]
            let tau = Ev::x(if t < 0.0 {
                0.0
            } else if t > 1.0 {
                1.0
            } else {
                t
            });
            let r = [
                tv.w[k][0].sub(tau.mul(tv.ev[k][0])),
                tv.w[k][1].sub(tau.mul(tv.ev[k][1])),
                tv.w[k][2].sub(tau.mul(tv.ev[k][2])),
            ];
            take(norm(&r));
        }
    }
    if tv.side.iter().all(|s| s.v >= s.e) {
        let s = vdot(&tv.w[0], &tv.n);
        if let Some(len) = norm(&tv.n) {
            take(s.abs().div(len));
        }
    }
    if best.is_finite() {
        Ok(best)
    } else {
        Err(SignError::Inconsistent)
    }
}

/// The running verdict over all surviving features.
#[derive(Debug, Clone, Copy)]
struct Verdict {
    any: bool,
    pos: bool,
    neg: bool,
    unsure: bool,
    undefined: Topology,
    low: f64,
}

impl Verdict {
    fn survivor(&mut self, status: Topology, decision: Option<Ev>, dist_lo: f64) {
        self.any = true;
        self.undefined = worse(self.undefined, status);
        match decision {
            Some(d) if d.pos() => self.pos = true,
            Some(d) if d.neg() => self.neg = true,
            _ => self.unsure = true,
        }
        let lo = if dist_lo > 0.0 { dist_lo } else { 0.0 };
        if lo < self.low {
            self.low = lo;
        }
    }
}

/// Pinned lower bound of the squared exact distance from `p` to the box of the corners.
#[allow(clippy::needless_range_loop)] // per-axis and per-corner indexing of parallel arrays, as pinned
fn box_lb(p: &V3, c: &[V3; 3]) -> f64 {
    let mut d2 = 0.0;
    for k in 0..3 {
        let (mut lo, mut hi) = (c[0][k], c[0][k]);
        for q in [c[1][k], c[2][k]] {
            if q < lo {
                lo = q;
            }
            if q > hi {
                hi = q;
            }
        }
        let g = if p[k] < lo {
            lo - p[k]
        } else if p[k] > hi {
            p[k] - hi
        } else {
            0.0
        };
        d2 = if k == 0 { g * g } else { d2 + g * g };
    }
    d2 * DOWN - ETA
}

#[allow(clippy::needless_range_loop)] // per-axis and per-corner indexing of parallel arrays, as pinned
fn analyse(
    pn: &PseudoNormals,
    mesh: &Mesh,
    p: &V3,
    t: u64,
    upper: f64,
    verdict: &mut Verdict,
) -> Result<(), SignError> {
    let tri = tri_index(mesh, t)?;
    let c = corners(mesh, &tri)?;
    let status = at(&pn.triangle, t)?;
    if status == Some(SignUndefined::Degenerate) {
        let lb = box_lb(p, &c);
        let lo = if lb > 0.0 { fsqrt(lb).map_or(0.0, |r| r * DOWN) } else { 0.0 };
        verdict.survivor(status, None, lo);
        return Ok(());
    }
    let tv = tri_vals(p, &c);
    let pe = ex3(p);
    // Vertices.
    for i in 0..3 {
        let prev = (i + 2) % 3;
        if tv.tn[i].pos() || tv.tm[prev].neg() {
            continue;
        }
        let r = vsub(&pe, &tv.pts[i]);
        let vn = pn.vertex(tri[i]).ok_or(SignError::Inconsistent)?;
        let dist = norm(&r);
        if dist.is_some_and(|d| d.lo() > upper) {
            continue;
        }
        let decision = if vn.status.is_none() { Some(vdot(&r, &stored(vn))) } else { None };
        verdict.survivor(vn.status, decision, dist.map_or(0.0, Ev::lo));
    }
    // Edges.
    for k in 0..3 {
        if tv.side[k].pos() || tv.tn[k].neg() || tv.tm[k].pos() {
            continue;
        }
        let en = edge_of(&pn.edge, tri[k], tri[(k + 1) % 3])?;
        let r = tv.tn[k].div(tv.ee[k]).map(|t| {
            [tv.w[k][0].sub(t.mul(tv.ev[k][0])), tv.w[k][1].sub(t.mul(tv.ev[k][1])), tv.w[k][2].sub(t.mul(tv.ev[k][2]))]
        });
        let dist = r.as_ref().and_then(norm);
        if dist.is_some_and(|d| d.lo() > upper) {
            continue;
        }
        let decision = match (&r, en.feature.status) {
            (Some(r), None) => Some(vdot(r, &stored(&en.feature))),
            _ => None,
        };
        verdict.survivor(en.feature.status, decision, dist.map_or(0.0, Ev::lo));
    }
    // Interior.
    if !tv.side.iter().any(|s| s.neg()) {
        let s = vdot(&tv.w[0], &tv.n);
        let dist = norm(&tv.n).and_then(|len| s.abs().div(len));
        if !dist.is_some_and(|d| d.lo() > upper) {
            verdict.survivor(status, Some(s), dist.map_or(0.0, Ev::lo));
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ queries

/// A mesh, its hierarchy and its pseudo-normals, for many signed-distance queries.
#[derive(Debug, Clone)]
pub struct SignTarget<'m> {
    target: MeshTarget<'m>,
    normals: PseudoNormals,
}

impl<'m> SignTarget<'m> {
    /// Builds the pseudo-normals of `target`'s mesh.
    pub fn new(target: MeshTarget<'m>) -> Result<SignTarget<'m>, SignError> {
        let normals = PseudoNormals::build(target.mesh())?;
        Ok(SignTarget { target, normals })
    }

    /// Uses pseudo-normals built earlier; fails with [`SignError::NormalsMismatch`] when their
    /// frame, vertex count or triangle count differs from the mesh's.
    pub fn with_normals(target: MeshTarget<'m>, normals: PseudoNormals) -> Result<SignTarget<'m>, SignError> {
        if !normals.matches(target.mesh()) {
            return Err(SignError::NormalsMismatch);
        }
        Ok(SignTarget { target, normals })
    }

    /// The pseudo-normals.
    pub fn normals(&self) -> &PseudoNormals {
        &self.normals
    }

    /// The mesh target.
    pub fn target(&self) -> &MeshTarget<'m> {
        &self.target
    }

    /// The closest point (GEO.DIST.MESH, no search options) with its certified sign.
    pub fn signed_distance(&self, p: [f64; 3]) -> Result<Signed, SignError> {
        match self.target.closest(p, &MeshQuery::default())? {
            MeshProjection::Hit(h) => self.signed(p, &h),
            MeshProjection::NoHit => Err(SignError::EmptyMesh),
        }
    }

    /// The sign of record for `hit`, a projection of `p` onto this mesh (any search options;
    /// the sign and the bound always refer to the whole mesh, the reported distance is the
    /// hit's). Rules in the [module documentation](self).
    pub fn signed(&self, p: [f64; 3], hit: &MeshHit) -> Result<Signed, SignError> {
        check_query(&p)?;
        let mesh = self.target.mesh();
        let d = hit.dist;
        if hit.tri >= mesh.triangle_count() || !d.is_finite() || d < 0.0 {
            return Err(SignError::InvalidHit);
        }
        let c = corners(mesh, &tri_index(mesh, hit.tri)?)?;
        let upper = upper_on_triangle(&p, &c)?;
        if d == 0.0 {
            return Ok(Signed { distance: 0.0, unsigned: 0.0, status: SignStatus::OnSurface, bound: upper });
        }
        let u2 = (upper * upper) * UP + ETA;
        let leaf_limit = u2 * GROW + 4.0 * ETA;
        let mut verdict =
            Verdict { any: false, pos: false, neg: false, unsure: false, undefined: None, low: f64::INFINITY };
        let mut fail: Option<SignError> = None;
        self.target
            .bvh()
            .ordered_visit(p, |leaf| {
                if leaf.lower_bound_sq() > leaf_limit {
                    return ControlFlow::Break(());
                }
                for t in leaf.triangles() {
                    let step = tri_index(mesh, t).and_then(|tri| corners(mesh, &tri)).and_then(|c| {
                        if box_lb(&p, &c) > u2 {
                            Ok(())
                        } else {
                            analyse(&self.normals, mesh, &p, t, upper, &mut verdict)
                        }
                    });
                    if let Err(e) = step {
                        fail = Some(e);
                        return ControlFlow::Break(());
                    }
                }
                ControlFlow::Continue(())
            })
            .map_err(|e| SignError::Dist(DistError::Bvh(e)))?;
        if let Some(e) = fail {
            return Err(e);
        }
        Ok(conclude(d, upper, &verdict))
    }
}

fn round_up(x: f64) -> f64 {
    if x > 0.0 {
        x * UP + ETA
    } else {
        0.0
    }
}

fn conclude(d: f64, upper: f64, v: &Verdict) -> Signed {
    let status = if !v.any {
        SignStatus::Undecidable
    } else if let Some(r) = v.undefined {
        SignStatus::Undefined(r)
    } else if v.pos && !v.neg && !v.unsure {
        SignStatus::Outside
    } else if v.neg && !v.pos && !v.unsure {
        SignStatus::Inside
    } else {
        SignStatus::Undecidable
    };
    let low = if v.any { v.low } else { 0.0 };
    let (a, b) = (round_up(upper - d), round_up(d - low));
    let bound = if a > b { a } else { b };
    let distance = if status == SignStatus::Inside { -d } else { d };
    Signed { distance, unsigned: d, status, bound }
}

// ------------------------------------------------------------------ CAD form

/// The same weighting for a CAD foot point (REQ-005): `N = sum angle_i normal_i` over the
/// incident faces (unit normals with face orientation applied, incident angles in radians, in
/// the given order), decision `(p - foot) . N`, widened by `foot_error * |N|_1` for a foot known
/// to within `foot_error` metres of the exact one. The caller supplies the topology: this form
/// never reports [`SignStatus::Undefined`]. `p == foot` is [`SignStatus::OnSurface`].
///
/// Domain: `p`, `foot` and every normal component finite and within [`COORD_LIMIT`];
/// `foot_error` finite, `>= 0`, within [`COORD_LIMIT`]; at least one incident entry; every angle
/// finite with `0 < angle <= 2 pi` ([`SignError::BadCadInput`] otherwise).
pub fn cad_sign(
    p: [f64; 3],
    foot: [f64; 3],
    foot_error: f64,
    incident: &[([f64; 3], f64)],
) -> Result<Signed, SignError> {
    let bad = |field: &'static str, index: u64| SignError::BadCadInput { field, index };
    check_query(&p)?;
    if !(foot.iter().all(|x| x.is_finite()) && in_range(&foot)) {
        return Err(bad("foot", 0));
    }
    if !(foot_error.is_finite() && (0.0..=COORD_LIMIT).contains(&foot_error)) {
        return Err(bad("foot_error", 0));
    }
    if incident.is_empty() {
        return Err(bad("incident", 0));
    }
    let mut acc: Option<E3> = None;
    for (i, (n, a)) in incident.iter().enumerate() {
        let i = i as u64;
        if !(n.iter().all(|x| x.is_finite()) && in_range(n)) {
            return Err(bad("normal", i));
        }
        if !(a.is_finite() && *a > 0.0 && *a <= TAU) {
            return Err(bad("angle", i));
        }
        let w = Ev::x(*a);
        let term = [w.mul(Ev::x(n[0])), w.mul(Ev::x(n[1])), w.mul(Ev::x(n[2]))];
        acc = Some(match acc {
            None => term,
            Some(s) => [s[0].add(term[0]), s[1].add(term[1]), s[2].add(term[2])],
        });
    }
    let nn = acc.ok_or(SignError::Inconsistent)?;
    let r = vsub(&ex3(&p), &ex3(&foot));
    let s = vdot(&r, &nn);
    let l1 = (nn[0].v.abs() + nn[0].e) + (nn[1].v.abs() + nn[1].e) + (nn[2].v.abs() + nn[2].e);
    let s = Ev { v: s.v, e: grow(s.e + foot_error * l1) };
    let dist = norm(&r).ok_or(SignError::Inconsistent)?;
    let bound = round_up(dist.e + foot_error);
    if p == foot {
        return Ok(Signed { distance: 0.0, unsigned: 0.0, status: SignStatus::OnSurface, bound });
    }
    let d = dist.v;
    let (status, distance) = if s.pos() {
        (SignStatus::Outside, d)
    } else if s.neg() {
        (SignStatus::Inside, -d)
    } else {
        (SignStatus::Undecidable, d)
    };
    Ok(Signed { distance, unsigned: d, status, bound })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn constants() {
        assert_eq!(U52, f64::EPSILON);
        assert_eq!(GROW, 1.0 + f64::EPSILON * 16.0);
        assert_eq!(UP, 1.0 + f64::EPSILON * 4.0);
        assert_eq!(DOWN, 1.0 - f64::EPSILON * 4.0);
        assert_eq!(ATAN2_ERROR, f64::EPSILON * 8.0);
    }

    /// The running error contains the exact value for sums and products of dyadic inputs whose
    /// exact result is known.
    #[test]
    fn running_error_contains_exact() {
        let third = Ev::x(1.0).div(Ev::x(3.0)).unwrap();
        let back = third.mul(Ev::x(3.0));
        assert!(back.v - back.e <= 1.0 && 1.0 <= back.v + back.e);
        let s = Ev::x(2.0).sqrt().unwrap();
        let sq = s.mul(s);
        assert!(sq.v - sq.e <= 2.0 && 2.0 <= sq.v + sq.e);
        assert!(!Ev { v: 1e-300, e: 1e-300 }.pos());
        assert!(Ev { v: -1.0, e: 0.5 }.neg());
        assert!(Ev::x(0.0).div(Ev { v: 1.0, e: 1.0 }).is_none());
    }

    #[test]
    fn worse_takes_most_severe() {
        assert_eq!(worse(None, Some(SignUndefined::Boundary)), Some(SignUndefined::Boundary));
        assert_eq!(
            worse(Some(SignUndefined::Boundary), Some(SignUndefined::Misoriented)),
            Some(SignUndefined::Misoriented)
        );
        assert_eq!(
            worse(Some(SignUndefined::Degenerate), Some(SignUndefined::NonManifold)),
            Some(SignUndefined::Degenerate)
        );
    }
}
