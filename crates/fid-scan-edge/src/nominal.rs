//! Nominal edge chains and station frames (spec node `SCAN.EDGE.NOMEDGE`).
//!
//! [`stations`] turns a nominal edge into an ordered list of [`Station`]s at a stated arc-length
//! spacing, each with the right-handed orthonormal frame `(t, n, b)` that edge extraction and
//! edge deviation split a deviation with: `t` the unit edge tangent, `n` the unit outward normal
//! of the adjacent nominal face, and `b = t x n`, the in-surface direction perpendicular to the
//! edge that points away from the face material.
//!
//! # Input
//!
//! A [`NominalEdge`] is either
//!
//! * [`NominalEdge::CadEdges`]: a face and a set of its boundary edges, by stable
//!   [`TopologyId`], resolved through a [`CadResolver`] (a GEO.CAD [`Body`]); or
//! * [`NominalEdge::Constructed`]: one curve definition over a parameter range plus the face it
//!   lies on (the form a KRN.CONS edge or trim curve takes), with the side of the curve the face
//!   material lies on, since a free curve has no loop to say so.
//!
//! # Orientation (REQ-001, REQ-003)
//!
//! GEO.CAD keeps the face material on the left of every coedge when the face is seen from its
//! outward normal (the natural normal `Su x Sv`, negated for a `Reversed` face; see
//! `fid_geo_cad::topo::trim`). Each chosen edge must be used by exactly one coedge of the face
//! (no use: not on the face; two uses: a seam, the face on both sides), and the chain runs in
//! that coedge's direction. Then `n x t` points into the material and `b = t x n` away from it.
//! A constructed curve runs in its parameter direction for [`MaterialSide::Left`] and against it
//! for [`MaterialSide::Right`].
//!
//! # Chaining (REQ-001)
//!
//! The oriented pieces are sorted by edge ID. A piece has a predecessor when another piece ends
//! at its start vertex, or (for two different vertices) when an end point lies within the chain
//! tolerance of its start point. The chain starts at the one piece without a predecessor; a
//! closed chain (every piece has one) starts at the lowest edge ID. From there each step takes
//! the unused piece that starts where the current piece ends: by shared vertex, else by the
//! chain tolerance. Two candidates are [`EdgeError::ChainBranch`]; none while pieces remain is
//! [`EdgeError::ChainGap`] with the end point, the nearest remaining start and their distance.
//! Consecutive pieces whose evaluated end and start points are further apart than the tolerance
//! are a gap too, even when they share a vertex. The chain tolerance is
//! [`CHAIN_GAP_TOL_M`] = 1e-9 m, floored at `8 eps` times the largest coordinate magnitude of the
//! two points. The tolerance search is linear per unconnected end, so the chain is limited to
//! [`MAX_CHAIN_EDGES`] pieces.
//!
//! # Arc length and stations (REQ-002)
//!
//! Each piece's arc length is the integral of `|C'(t)|` by adaptive five-point Gauss-Legendre
//! quadrature (fixed nodes; eight equal starting panels; a panel is split in two until the two
//! halves agree with the whole within `(QUAD_ABS_M + QUAD_REL * L) * width / range` plus 64
//! rounding units of the halves; at most [`QUAD_MAX_DEPTH`] halvings and
//! [`QUAD_MAX_PANELS`] panels per piece, otherwise [`EdgeError::ArcLengthNotConverged`]). The
//! pieces are summed in chain order. Station `i` is at `s_i = i * d` from the chain start. On an
//! open chain stations run while `s_i <= L + tol`, the last one at or before the end (a station
//! within `tol` past the end is placed at the end); on a closed chain while `s_i < L - tol`, so
//! no station repeats the start. `tol` is `LENGTH_TOL_ABS_M + LENGTH_TOL_REL * L`
//! (1e-12 m + 1e-12 L), well above the quadrature error. A station within `tol` of a junction
//! between two pieces belongs to the later piece, at its start (so the frame at a corner is the
//! frame of the piece that leaves it). Within a piece the parameter is found by
//! [`BISECT_ITERS`] = 60 bisection steps on the panel's arc-length function.
//!
//! # Frames (REQ-003, REQ-004)
//!
//! At the station parameter the curve gives the point and the unit tangent (oriented as above).
//! The face normal comes from the exact surface at the closest surface parameter of the point:
//! closed-form inversion for analytic surfaces, GEO.NURBS global inversion for NURBS surfaces.
//! The foot must be unique and certified ([`EdgeError::Foot`]), at most [`ON_FACE_TOL_M`] from
//! the point ([`EdgeError::OffFace`]) and regular ([`EdgeError::DegenerateNormal`]). The raw
//! tangent's cosine with `n` must be at most [`TANGENT_TOL`] ([`EdgeError::NotTangent`]); it is
//! reported per station. The tangent is then made exactly perpendicular to `n` (one
//! Gram-Schmidt step) and normalised, and `b = t x n` is normalised, so the frame is orthonormal
//! to a few rounding units (REQ-003 asks for 1e-12). Nothing is tessellated.
//!
//! # Determinism (D0, KNP-1)
//!
//! IEEE 754 binary64 `+ - * /` written out one operation at a time (no FMA), square roots and
//! `hypot` from `fid_num::f`, comparisons in total order, no hash containers, every loop
//! bounded, nothing panics. The function is pure: it reads the body and the edge and returns a
//! value.

use core::cmp::Ordering;
use core::fmt;
use std::collections::BTreeMap;

use fid_geo_cad::{Body, CurveDef, EntityIx, EntityKind, Sense, SurfaceDef, TopologyId, VertexIx, DEFAULT_VALID_TOL_M};
use fid_geo_nurbs::{
    invert_surface, AnalyticError, Curve, CurveDerivs, EvalError, Reason, Regularity, Status, Surface, Uniqueness,
};
use fid_num::f::{hypot, total_cmp, total_max};

/// REQ-001: pieces whose ends are further apart than this (metres) are not connected.
pub const CHAIN_GAP_TOL_M: f64 = 1e-9;

/// Largest distance (metres) from a station point to the face surface, the GEO.CAD validation
/// default (`DEFAULT_VALID_TOL_M`) [unverified: needs evidence from imported CAD].
pub const ON_FACE_TOL_M: f64 = DEFAULT_VALID_TOL_M;

/// Largest cosine between the curve tangent and the face normal at a station, i.e. how far the
/// curve may leave the surface's tangent plane [unverified: provisional].
pub const TANGENT_TOL: f64 = 1e-6;

/// Most pieces (edges) in one chain.
pub const MAX_CHAIN_EDGES: usize = 4096;

/// Most stations in one result.
pub const MAX_STATIONS: usize = 1_000_000;

/// Bisection steps for a station parameter (Design: fixed 60 iterations).
pub const BISECT_ITERS: usize = 60;

/// Relative arc-length tolerance of the adaptive quadrature.
pub const QUAD_REL: f64 = 1e-13;

/// Absolute arc-length tolerance of the adaptive quadrature, metres.
pub const QUAD_ABS_M: f64 = 1e-16;

/// Most halvings below a starting panel.
pub const QUAD_MAX_DEPTH: usize = 40;

/// Most accepted panels per piece.
pub const QUAD_MAX_PANELS: usize = 1 << 16;

/// Absolute part of the station-placement length tolerance, metres.
pub const LENGTH_TOL_ABS_M: f64 = 1e-12;

/// Relative part of the station-placement length tolerance.
pub const LENGTH_TOL_REL: f64 = 1e-12;

/// Starting panels per piece.
const START_PANELS: usize = 8;

/// Five-point Gauss-Legendre nodes and weights on `[-1, 1]` (nearest doubles).
const GL_X1: f64 = 0.5384693101056831;
const GL_X2: f64 = 0.906179845938664;
const GL_W0: f64 = 0.5688888888888889;
const GL_W1: f64 = 0.47862867049936647;
const GL_W2: f64 = 0.23692688505618908;

/// Gives the CAD body that nominal edges refer to.
pub trait CadResolver {
    /// The body whose faces and edges the IDs name.
    fn body(&self) -> &Body;
}

impl CadResolver for Body {
    fn body(&self) -> &Body {
        self
    }
}

/// Which side of a constructed curve the face material lies on, seen from the face's outward
/// normal with the curve running in its increasing-parameter direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialSide {
    /// Material on the left (the GEO.CAD coedge convention): the chain runs with the parameter.
    Left,
    /// Material on the right: the chain runs against the parameter.
    Right,
}

/// A nominal edge.
#[derive(Debug, Clone, PartialEq)]
pub enum NominalEdge {
    /// Boundary edges of one face, by stable ID; their order in the list does not matter.
    CadEdges { face: TopologyId, edges: Vec<TopologyId> },
    /// One curve over `range` (`t0 < t1`) lying on `face` (a KRN.CONS edge or trim curve).
    Constructed { curve: CurveDef, range: [f64; 2], face: TopologyId, material: MaterialSide },
}

/// One station along the chain.
#[derive(Debug, Clone, PartialEq)]
pub struct Station {
    s_arc: f64,
    point: [f64; 3],
    t: [f64; 3],
    n: [f64; 3],
    b: [f64; 3],
    edge: Option<TopologyId>,
    param: f64,
    face_distance: f64,
    tangent_normal_cos: f64,
}

impl Station {
    /// Arc length from the chain start, metres (`i * spacing`).
    pub fn s_arc(&self) -> f64 {
        self.s_arc
    }
    /// The point on the nominal edge.
    pub fn point(&self) -> [f64; 3] {
        self.point
    }
    /// Unit edge tangent, in the chain direction.
    pub fn t(&self) -> [f64; 3] {
        self.t
    }
    /// Unit outward normal of the face at the station.
    pub fn n(&self) -> [f64; 3] {
        self.n
    }
    /// `t x n`: in the face's tangent plane, perpendicular to the edge, away from the material.
    pub fn b(&self) -> [f64; 3] {
        self.b
    }
    /// The CAD edge the station lies on; `None` for a constructed curve.
    pub fn edge(&self) -> Option<&TopologyId> {
        self.edge.as_ref()
    }
    /// The curve parameter of the station.
    pub fn param(&self) -> f64 {
        self.param
    }
    /// Distance from the point to the face surface (the inversion foot), metres.
    pub fn face_distance(&self) -> f64 {
        self.face_distance
    }
    /// Cosine between the curve's own tangent and `n`, before `t` was made perpendicular.
    pub fn tangent_normal_cos(&self) -> f64 {
        self.tangent_normal_cos
    }
}

/// One piece of the chain, in chain order.
#[derive(Debug, Clone, PartialEq)]
pub struct ChainPiece {
    edge: Option<TopologyId>,
    reversed: bool,
    length: f64,
}

impl ChainPiece {
    /// The CAD edge; `None` for a constructed curve.
    pub fn edge(&self) -> Option<&TopologyId> {
        self.edge.as_ref()
    }
    /// The chain runs against the curve parameter on this piece.
    pub fn reversed(&self) -> bool {
        self.reversed
    }
    /// Arc length of the piece, metres.
    pub fn length(&self) -> f64 {
        self.length
    }
}

/// The stations of a nominal edge with the chain they were placed on.
#[derive(Debug, Clone, PartialEq)]
pub struct NominalStations {
    stations: Vec<Station>,
    chain: Vec<ChainPiece>,
    length: f64,
    closed: bool,
    spacing: f64,
}

impl NominalStations {
    /// The stations in chain order.
    pub fn stations(&self) -> &[Station] {
        &self.stations
    }
    /// The pieces in chain order.
    pub fn chain(&self) -> &[ChainPiece] {
        &self.chain
    }
    /// Total arc length of the chain, metres.
    pub fn length(&self) -> f64 {
        self.length
    }
    /// The chain ends where it starts.
    pub fn closed(&self) -> bool {
        self.closed
    }
    /// The spacing the stations were placed at, metres.
    pub fn spacing(&self) -> f64 {
        self.spacing
    }
}

/// Why the closest surface point of a station could not be used.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FootFault {
    /// Closed-form inversion refused the point.
    Analytic(AnalyticError),
    /// GEO.NURBS inversion failed (its stable error code).
    Invert(&'static str),
    /// Several closest points (for example on a surface's axis).
    NonUnique,
    /// GEO.NURBS found a tie between distinct feet.
    Tie,
    /// GEO.NURBS could not certify the closest point.
    Unresolved(Reason),
}

/// Why no stations were produced.
#[derive(Debug, Clone, PartialEq)]
pub enum EdgeError {
    /// The spacing is not a finite number above zero.
    BadSpacing { spacing: f64 },
    /// The spacing gives more than [`MAX_STATIONS`] stations on the chain.
    TooManyStations { length: f64, spacing: f64 },
    /// No edges were given.
    EmptyChain,
    /// More than [`MAX_CHAIN_EDGES`] edges were given.
    TooManyEdges { count: usize },
    /// No entity of the expected kind has this ID.
    UnknownEntity { id: TopologyId, expected: EntityKind },
    /// The edge is listed twice.
    DuplicateEdge { edge: TopologyId },
    /// The edge is used by `uses` coedges of the face, not exactly one: 0 means it is not on
    /// the face, 2 or more that the face lies on both of its sides (a seam).
    MultiFace { edge: TopologyId, uses: usize },
    /// The edge has no curve geometry.
    NoCurve { edge: TopologyId },
    /// A constructed curve's range is not finite with `t0 < t1`.
    BadRange { range: [f64; 2] },
    /// A piece is no longer than [`CHAIN_GAP_TOL_M`].
    ZeroLength { edge: Option<TopologyId>, length: f64 },
    /// The chain does not continue from `at` (the end of `after`): the nearest remaining start
    /// (of `before`, `None` when the gap is the chain's own closure) is `gap` metres away.
    ChainGap { after: Option<TopologyId>, before: Option<TopologyId>, at: [f64; 3], gap: f64 },
    /// Several pieces continue from `at`.
    ChainBranch { at: [f64; 3], edges: Vec<Option<TopologyId>> },
    /// The arc-length quadrature did not converge near parameter `t`.
    ArcLengthNotConverged { edge: Option<TopologyId>, t: f64 },
    /// The curve or surface evaluator refused.
    Eval { edge: Option<TopologyId>, err: EvalError },
    /// The curve has no tangent at the station's parameter.
    DegenerateTangent { station: usize, param: f64 },
    /// The face's closest point to the station could not be used.
    Foot { station: usize, fault: FootFault },
    /// The station point is further than [`ON_FACE_TOL_M`] from the face surface.
    OffFace { station: usize, distance: f64 },
    /// The face has no normal at the station's foot (a pole or apex).
    DegenerateNormal { station: usize },
    /// The curve leaves the face's tangent plane: `|cos(t, n)| > TANGENT_TOL`.
    NotTangent { station: usize, cos: f64 },
    /// An intermediate value was not finite.
    NonFinite,
}

impl EdgeError {
    /// Stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            EdgeError::BadSpacing { .. } | EdgeError::TooManyStations { .. } => "scan.edge.bad_spacing",
            EdgeError::EmptyChain => "scan.edge.empty_chain",
            EdgeError::TooManyEdges { .. } => "scan.edge.too_many_edges",
            EdgeError::UnknownEntity { .. } => "scan.edge.unknown_entity",
            EdgeError::DuplicateEdge { .. } => "scan.edge.duplicate_edge",
            EdgeError::MultiFace { .. } => "scan.edge.multi_face",
            EdgeError::NoCurve { .. } => "scan.edge.no_curve",
            EdgeError::BadRange { .. } => "scan.edge.bad_range",
            EdgeError::ZeroLength { .. } => "scan.edge.zero_length",
            EdgeError::ChainGap { .. } => "scan.edge.chain_gap",
            EdgeError::ChainBranch { .. } => "scan.edge.chain_branch",
            EdgeError::ArcLengthNotConverged { .. } => "scan.edge.arc_length_not_converged",
            EdgeError::Eval { .. } => "scan.edge.eval",
            EdgeError::DegenerateTangent { .. } => "scan.edge.degenerate_tangent",
            EdgeError::Foot { .. } => "scan.edge.foot",
            EdgeError::OffFace { .. } => "scan.edge.off_face",
            EdgeError::DegenerateNormal { .. } => "scan.edge.degenerate_normal",
            EdgeError::NotTangent { .. } => "scan.edge.not_tangent",
            EdgeError::NonFinite => "scan.edge.non_finite",
        }
    }
}

fn opt_id(id: &Option<TopologyId>) -> String {
    match id {
        Some(i) => i.to_string(),
        None => "the constructed curve".to_string(),
    }
}

impl fmt::Display for EdgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.code())?;
        match self {
            EdgeError::BadSpacing { spacing } => write!(f, "spacing {spacing:e} m is not finite and positive"),
            EdgeError::TooManyStations { length, spacing } => {
                write!(f, "{length:e} m at {spacing:e} m spacing exceeds {MAX_STATIONS} stations")
            }
            EdgeError::EmptyChain => write!(f, "no edges given"),
            EdgeError::TooManyEdges { count } => write!(f, "{count} edges exceed the limit {MAX_CHAIN_EDGES}"),
            EdgeError::UnknownEntity { id, expected } => write!(f, "no {} with ID {id}", expected.as_str()),
            EdgeError::DuplicateEdge { edge } => write!(f, "edge {edge} is listed twice"),
            EdgeError::MultiFace { edge, uses } => {
                write!(f, "edge {edge} is used by {uses} coedges of the face, not exactly one")
            }
            EdgeError::NoCurve { edge } => write!(f, "edge {edge} has no curve"),
            EdgeError::BadRange { range } => write!(f, "range [{:e}, {:e}] is not t0 < t1", range[0], range[1]),
            EdgeError::ZeroLength { edge, length } => write!(f, "{} has zero length ({length:e} m)", opt_id(edge)),
            EdgeError::ChainGap { after, before, at, gap } => write!(
                f,
                "gap of {gap:e} m at ({:e}, {:e}, {:e}) after {} before {}",
                at[0],
                at[1],
                at[2],
                opt_id(after),
                match before {
                    Some(b) => b.to_string(),
                    None => "the chain start".to_string(),
                }
            ),
            EdgeError::ChainBranch { at, edges } => {
                write!(f, "{} pieces continue from ({:e}, {:e}, {:e})", edges.len(), at[0], at[1], at[2])
            }
            EdgeError::ArcLengthNotConverged { edge, t } => {
                write!(f, "arc length of {} did not converge near t = {t:e}", opt_id(edge))
            }
            EdgeError::Eval { edge, err } => write!(f, "evaluating {}: {err}", opt_id(edge)),
            EdgeError::DegenerateTangent { station, param } => {
                write!(f, "station {station}: no curve tangent at t = {param:e}")
            }
            EdgeError::Foot { station, fault } => write!(f, "station {station}: no usable face foot ({fault:?})"),
            EdgeError::OffFace { station, distance } => {
                write!(f, "station {station}: {distance:e} m from the face (limit {ON_FACE_TOL_M:e} m)")
            }
            EdgeError::DegenerateNormal { station } => write!(f, "station {station}: face normal is degenerate"),
            EdgeError::NotTangent { station, cos } => {
                write!(f, "station {station}: tangent-normal cosine {cos:e} exceeds {TANGENT_TOL:e}")
            }
            EdgeError::NonFinite => write!(f, "non-finite intermediate"),
        }
    }
}

impl core::error::Error for EdgeError {}

// ---------------------------------------------------------------- small vector helpers

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    let x = a[0] * b[0];
    let y = a[1] * b[1];
    let z = a[2] * b[2];
    x + y + z
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    let x0 = a[1] * b[2];
    let x1 = a[2] * b[1];
    let y0 = a[2] * b[0];
    let y1 = a[0] * b[2];
    let z0 = a[0] * b[1];
    let z1 = a[1] * b[0];
    [x0 - x1, y0 - y1, z0 - z1]
}

fn norm(a: [f64; 3]) -> Result<f64, EdgeError> {
    let h = hypot(a[0], a[1]).map_err(|_| EdgeError::NonFinite)?;
    hypot(h, a[2]).map_err(|_| EdgeError::NonFinite)
}

fn finite3(a: [f64; 3]) -> bool {
    a.iter().all(|x| x.is_finite())
}

fn tmax(a: f64, b: f64) -> Result<f64, EdgeError> {
    total_max(a, b).map_err(|_| EdgeError::NonFinite)
}

fn abs_max3(a: [f64; 3]) -> Result<f64, EdgeError> {
    tmax(tmax(a[0].abs(), a[1].abs())?, a[2].abs())
}

/// Distance between two points and the chain tolerance for them.
fn gap_and_tol(p: [f64; 3], q: [f64; 3]) -> Result<(f64, f64), EdgeError> {
    let d = norm(sub(p, q))?;
    let scale = tmax(abs_max3(p)?, abs_max3(q)?)?;
    let tol = tmax(CHAIN_GAP_TOL_M, 8.0 * f64::EPSILON * scale)?;
    Ok((d, tol))
}

// ---------------------------------------------------------------- curve evaluation

fn eval_curve(c: &CurveDef, t: f64, order: usize) -> Result<CurveDerivs, EvalError> {
    if let Some(a) = c.as_analytic() {
        a.eval(t, order)
    } else if let Some(n) = c.as_nurbs() {
        n.eval(t, order)
    } else {
        Err(EvalError::Inconsistent)
    }
}

/// `|C'(t)|`.
fn speed(c: &CurveDef, t: f64) -> Result<f64, EvalError> {
    let d = eval_curve(c, t, 1)?;
    let d1 = d.d1.ok_or(EvalError::Inconsistent)?;
    let h = hypot(d1[0], d1[1]).map_err(|_| EvalError::NonFinite)?;
    hypot(h, d1[2]).map_err(|_| EvalError::NonFinite)
}

/// Five-point Gauss-Legendre estimate of the arc length over `[a, b]`.
fn gl5(c: &CurveDef, a: f64, b: f64) -> Result<f64, EvalError> {
    let h = (b - a) * 0.5;
    let m = a * 0.5 + b * 0.5;
    let o1 = h * GL_X1;
    let o2 = h * GL_X2;
    let f0 = speed(c, m)?;
    let f1 = speed(c, m - o1)? + speed(c, m + o1)?;
    let f2 = speed(c, m - o2)? + speed(c, m + o2)?;
    let s0 = GL_W0 * f0;
    let s1 = GL_W1 * f1;
    let s2 = GL_W2 * f2;
    let s = (s0 + s1) + s2;
    let r = s * h;
    if r.is_finite() {
        Ok(r)
    } else {
        Err(EvalError::NonFinite)
    }
}

/// An accepted quadrature panel: `[a, b]` and the arc length before `a` within the piece.
#[derive(Debug, Clone, Copy)]
struct Panel {
    a: f64,
    b: f64,
    cum: f64,
}

/// One oriented piece of the chain.
struct Piece<'a> {
    edge: Option<TopologyId>,
    curve: &'a CurveDef,
    t0: f64,
    t1: f64,
    reversed: bool,
    start_v: Option<VertexIx>,
    end_v: Option<VertexIx>,
    p_start: [f64; 3],
    p_end: [f64; 3],
}

impl<'a> Piece<'a> {
    fn new(
        edge: Option<TopologyId>,
        curve: &'a CurveDef,
        range: [f64; 2],
        reversed: bool,
        verts: Option<(VertexIx, VertexIx)>,
    ) -> Result<Piece<'a>, EdgeError> {
        let at =
            |t: f64| eval_curve(curve, t, 0).map(|d| d.p).map_err(|err| EdgeError::Eval { edge: edge.clone(), err });
        let (p0, p1) = (at(range[0])?, at(range[1])?);
        let (start_v, end_v) = match verts {
            Some((s, e)) if reversed => (Some(e), Some(s)),
            Some((s, e)) => (Some(s), Some(e)),
            None => (None, None),
        };
        let (p_start, p_end) = if reversed { (p1, p0) } else { (p0, p1) };
        Ok(Piece { edge, curve, t0: range[0], t1: range[1], reversed, start_v, end_v, p_start, p_end })
    }

    fn eval_err(&self, err: EvalError) -> EdgeError {
        EdgeError::Eval { edge: self.edge.clone(), err }
    }

    /// The accepted panels over `[t0, t1]` in ascending `t`, and the piece's arc length.
    fn panels(&self) -> Result<(Vec<Panel>, f64), EdgeError> {
        let (t0, t1) = (self.t0, self.t1);
        let range = t1 - t0;
        if !range.is_finite() {
            return Err(EdgeError::NonFinite);
        }
        let mut starts = Vec::with_capacity(START_PANELS + 1);
        for k in 0..=START_PANELS {
            let x = if k == START_PANELS { t1 } else { t0 + range * (k as f64 / START_PANELS as f64) };
            starts.push(x);
        }
        let mut stack: Vec<(f64, f64, f64, usize)> = Vec::new();
        let mut total = 0.0;
        for w in starts.windows(2).rev() {
            if let [a, b] = *w {
                let g = gl5(self.curve, a, b).map_err(|e| self.eval_err(e))?;
                total += g;
                stack.push((a, b, g, 0));
            }
        }
        let base = QUAD_ABS_M + QUAD_REL * total;
        let mut out: Vec<Panel> = Vec::new();
        let mut cum = 0.0;
        while let Some((a, b, g, depth)) = stack.pop() {
            let m = a * 0.5 + b * 0.5;
            let gl = gl5(self.curve, a, m).map_err(|e| self.eval_err(e))?;
            let gr = gl5(self.curve, m, b).map_err(|e| self.eval_err(e))?;
            let halves = gl + gr;
            let diff = (halves - g).abs();
            let frac = (b - a) / range;
            let tol = base * frac + 64.0 * f64::EPSILON * (gl.abs() + gr.abs());
            if diff <= tol {
                if out.len() + 2 > QUAD_MAX_PANELS {
                    return Err(EdgeError::ArcLengthNotConverged { edge: self.edge.clone(), t: a });
                }
                out.push(Panel { a, b: m, cum });
                cum += gl;
                out.push(Panel { a: m, b, cum });
                cum += gr;
            } else if depth >= QUAD_MAX_DEPTH || m <= a || m >= b {
                return Err(EdgeError::ArcLengthNotConverged { edge: self.edge.clone(), t: m });
            } else {
                stack.push((m, b, gr, depth + 1));
                stack.push((a, m, gl, depth + 1));
            }
        }
        if !cum.is_finite() {
            return Err(EdgeError::NonFinite);
        }
        Ok((out, cum))
    }

    /// The curve parameter at arc length `sigma` (ascending, from `t0`) by fixed bisection.
    fn param_at(&self, panels: &[Panel], len: f64, sigma: f64) -> Result<f64, EdgeError> {
        if sigma <= 0.0 {
            return Ok(self.t0);
        }
        if sigma >= len {
            return Ok(self.t1);
        }
        let k = panels.partition_point(|p| p.cum <= sigma);
        let p = match k.checked_sub(1).and_then(|i| panels.get(i)) {
            Some(p) => *p,
            None => return Ok(self.t0),
        };
        let (mut lo, mut hi) = (p.a, p.b);
        for _ in 0..BISECT_ITERS {
            let mid = lo * 0.5 + hi * 0.5;
            let v = p.cum + gl5(self.curve, p.a, mid).map_err(|e| self.eval_err(e))?;
            if v < sigma {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Ok(lo * 0.5 + hi * 0.5)
    }
}

// ---------------------------------------------------------------- resolution

fn lookup(body: &Body, id: &TopologyId, expected: EntityKind) -> Result<EntityIx, EdgeError> {
    match body.lookup(id) {
        Some(ix) if ix.kind() == expected => Ok(ix),
        _ => Err(EdgeError::UnknownEntity { id: id.clone(), expected }),
    }
}

fn resolve_face<'b>(body: &'b Body, id: &TopologyId) -> Result<&'b fid_geo_cad::Face, EdgeError> {
    match lookup(body, id, EntityKind::Face)? {
        EntityIx::Face(fx) => {
            body.face(fx).ok_or_else(|| EdgeError::UnknownEntity { id: id.clone(), expected: EntityKind::Face })
        }
        _ => Err(EdgeError::UnknownEntity { id: id.clone(), expected: EntityKind::Face }),
    }
}

fn resolve_cad<'a>(
    body: &'a Body,
    face_id: &TopologyId,
    edges: &[TopologyId],
) -> Result<(&'a fid_geo_cad::Face, Vec<Piece<'a>>), EdgeError> {
    let face = resolve_face(body, face_id)?;
    if edges.is_empty() {
        return Err(EdgeError::EmptyChain);
    }
    if edges.len() > MAX_CHAIN_EDGES {
        return Err(EdgeError::TooManyEdges { count: edges.len() });
    }
    // Selected edges by dense index, each with its coedge senses on this face.
    let mut chosen: BTreeMap<fid_geo_cad::EdgeIx, (&TopologyId, Vec<Sense>)> = BTreeMap::new();
    for id in edges {
        let ex = match lookup(body, id, EntityKind::Edge)? {
            EntityIx::Edge(ex) => ex,
            _ => return Err(EdgeError::UnknownEntity { id: id.clone(), expected: EntityKind::Edge }),
        };
        if chosen.insert(ex, (id, Vec::new())).is_some() {
            return Err(EdgeError::DuplicateEdge { edge: id.clone() });
        }
    }
    for lx in face.loops() {
        let Some(lp) = body.r#loop(*lx) else { continue };
        for cx in lp.coedges() {
            let Some(co) = body.coedge(*cx) else { continue };
            if let Some((_, uses)) = chosen.get_mut(&co.edge()) {
                uses.push(co.sense());
            }
        }
    }
    let mut pieces = Vec::with_capacity(chosen.len());
    for (ex, (id, uses)) in &chosen {
        let sense = match uses.as_slice() {
            [s] => *s,
            other => return Err(EdgeError::MultiFace { edge: (*id).clone(), uses: other.len() }),
        };
        let edge =
            body.edge(*ex).ok_or_else(|| EdgeError::UnknownEntity { id: (*id).clone(), expected: EntityKind::Edge })?;
        let curve = edge.curve().ok_or_else(|| EdgeError::NoCurve { edge: (*id).clone() })?;
        let reversed = sense == Sense::Reversed;
        pieces.push(Piece::new(
            Some((*id).clone()),
            curve,
            edge.param_range(),
            reversed,
            Some((edge.start(), edge.end())),
        )?);
    }
    // "Lowest edge ID first": sort by the stable ID (total order of TopologyId).
    pieces.sort_by(|a, b| a.edge.cmp(&b.edge));
    Ok((face, pieces))
}

// ---------------------------------------------------------------- chaining

/// The pieces in chain order and whether the chain is closed.
fn chain(pieces: Vec<Piece<'_>>) -> Result<(Vec<Piece<'_>>, bool), EdgeError> {
    let n = pieces.len();
    let mut by_start: BTreeMap<VertexIx, Vec<usize>> = BTreeMap::new();
    let mut end_count: BTreeMap<VertexIx, usize> = BTreeMap::new();
    for (i, p) in pieces.iter().enumerate() {
        if let Some(v) = p.start_v {
            by_start.entry(v).or_default().push(i);
        }
        if let Some(v) = p.end_v {
            *end_count.entry(v).or_insert(0) += 1;
        }
    }
    let topo_pred = |p: &Piece<'_>| p.start_v.is_some_and(|v| end_count.contains_key(&v));
    let topo_succ = |p: &Piece<'_>| p.end_v.is_some_and(|v| by_start.contains_key(&v));
    // Pieces without a topological predecessor, and ends without a topological successor.
    let open_starts: Vec<usize> = (0..n).filter(|&i| pieces.get(i).is_some_and(|p| !topo_pred(p))).collect();
    let open_ends: Vec<usize> = (0..n).filter(|&i| pieces.get(i).is_some_and(|p| !topo_succ(p))).collect();
    let mut first = None;
    for &i in &open_starts {
        let Some(p) = pieces.get(i) else { continue };
        let mut has_pred = false;
        for &j in &open_ends {
            let Some(q) = pieces.get(j) else { continue };
            let (d, tol) = gap_and_tol(q.p_end, p.p_start)?;
            if d <= tol {
                has_pred = true;
                break;
            }
        }
        if !has_pred {
            first = Some(i);
            break;
        }
    }
    let had_open_start = first.is_some();
    let start = first.unwrap_or(0);
    let mut used = vec![false; n];
    let mut order = Vec::with_capacity(n);
    if let Some(u) = used.get_mut(start) {
        *u = true;
    }
    order.push(start);
    let mut cur = start;
    for _ in 1..n {
        let Some(cp) = pieces.get(cur) else { return Err(EdgeError::NonFinite) };
        let topo: Vec<usize> = cp
            .end_v
            .and_then(|v| by_start.get(&v))
            .map(|c| c.iter().copied().filter(|&j| !used.get(j).copied().unwrap_or(true)).collect())
            .unwrap_or_default();
        let next = match topo.as_slice() {
            [j] => {
                let q = pieces.get(*j).ok_or(EdgeError::NonFinite)?;
                let (d, tol) = gap_and_tol(cp.p_end, q.p_start)?;
                if d > tol {
                    return Err(EdgeError::ChainGap {
                        after: cp.edge.clone(),
                        before: q.edge.clone(),
                        at: cp.p_end,
                        gap: d,
                    });
                }
                *j
            }
            [] => {
                // Geometric fallback: unused starts within the tolerance, else the nearest one.
                let mut near: Vec<usize> = Vec::new();
                let mut best: Option<(f64, usize)> = None;
                for (j, q) in pieces.iter().enumerate() {
                    if used.get(j).copied().unwrap_or(true) {
                        continue;
                    }
                    let (d, tol) = gap_and_tol(cp.p_end, q.p_start)?;
                    if d <= tol {
                        near.push(j);
                    }
                    let better = match best {
                        None => true,
                        Some((bd, _)) => total_cmp(d, bd).map_err(|_| EdgeError::NonFinite)? == Ordering::Less,
                    };
                    if better {
                        best = Some((d, j));
                    }
                }
                match (near.as_slice(), best) {
                    ([j], _) => *j,
                    ([], Some((gap, j))) => {
                        return Err(EdgeError::ChainGap {
                            after: cp.edge.clone(),
                            before: pieces.get(j).and_then(|q| q.edge.clone()),
                            at: cp.p_end,
                            gap,
                        })
                    }
                    ([], None) => return Err(EdgeError::NonFinite),
                    (many, _) => {
                        return Err(EdgeError::ChainBranch {
                            at: cp.p_end,
                            edges: many.iter().filter_map(|&j| pieces.get(j)).map(|q| q.edge.clone()).collect(),
                        })
                    }
                }
            }
            many => {
                return Err(EdgeError::ChainBranch {
                    at: cp.p_end,
                    edges: many.iter().filter_map(|&j| pieces.get(j)).map(|q| q.edge.clone()).collect(),
                })
            }
        };
        if let Some(u) = used.get_mut(next) {
            *u = true;
        }
        order.push(next);
        cur = next;
    }
    // Closure: the last end meets the first start.
    let (last, head) = match (order.last().and_then(|&i| pieces.get(i)), pieces.get(start)) {
        (Some(l), Some(h)) => (l, h),
        _ => return Err(EdgeError::NonFinite),
    };
    let (d, tol) = gap_and_tol(last.p_end, head.p_start)?;
    let closed = if had_open_start {
        false
    } else if d <= tol {
        true
    } else {
        // Every piece had a predecessor but the loop does not close on itself (or closes only
        // topologically, with the curves apart): report the closing gap.
        return Err(EdgeError::ChainGap { after: last.edge.clone(), before: None, at: last.p_end, gap: d });
    };
    let mut slots: Vec<Option<Piece<'_>>> = pieces.into_iter().map(Some).collect();
    let mut out = Vec::with_capacity(n);
    for i in order {
        if let Some(p) = slots.get_mut(i).and_then(Option::take) {
            out.push(p);
        }
    }
    Ok((out, closed))
}

// ---------------------------------------------------------------- frames

fn foot_on(surface: &SurfaceDef, q: [f64; 3]) -> Result<(f64, f64, f64), FootFault> {
    if let Some(a) = surface.as_analytic() {
        let f = a.invert(q).map_err(FootFault::Analytic)?;
        match f.uniqueness {
            Uniqueness::Unique => Ok((f.u, f.v, f.distance)),
            Uniqueness::NonUnique => Err(FootFault::NonUnique),
        }
    } else if let Some(s) = surface.as_nurbs() {
        let inv = invert_surface(s, q).map_err(|e| FootFault::Invert(e.code()))?;
        match inv.status {
            Status::Certified { tie: false } => Ok((inv.u, inv.v, inv.distance)),
            Status::Certified { tie: true } => Err(FootFault::Tie),
            Status::Unresolved { reason } => Err(FootFault::Unresolved(reason)),
        }
    } else {
        Err(FootFault::Invert("Inconsistent"))
    }
}

fn surface_normal_at(surface: &SurfaceDef, u: f64, v: f64) -> Result<Option<[f64; 3]>, EvalError> {
    if let Some(a) = surface.as_analytic() {
        a.normal(u, v)
    } else if let Some(s) = surface.as_nurbs() {
        s.normal(u, v)
    } else {
        Err(EvalError::Inconsistent)
    }
}

fn scale3(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn unit(a: [f64; 3]) -> Result<[f64; 3], EdgeError> {
    let l = norm(a)?;
    let u = [a[0] / l, a[1] / l, a[2] / l];
    if finite3(u) && l > 0.0 {
        Ok(u)
    } else {
        Err(EdgeError::NonFinite)
    }
}

fn frame(
    face: &fid_geo_cad::Face,
    piece: &Piece<'_>,
    param: f64,
    s_arc: f64,
    index: usize,
) -> Result<Station, EdgeError> {
    let d = eval_curve(piece.curve, param, 1).map_err(|e| piece.eval_err(e))?;
    let d1 = d.d1.ok_or_else(|| piece.eval_err(EvalError::Inconsistent))?;
    if d.regularity != Some(Regularity::Regular) {
        return Err(EdgeError::DegenerateTangent { station: index, param });
    }
    let raw = unit(d1)?;
    let tan = if piece.reversed { scale3(raw, -1.0) } else { raw };
    let point = d.p;
    let surface = face.surface();
    let (u, v, dist) = foot_on(surface, point).map_err(|fault| EdgeError::Foot { station: index, fault })?;
    let tol = tmax(ON_FACE_TOL_M, 8.0 * f64::EPSILON * abs_max3(point)?)?;
    if dist > tol {
        return Err(EdgeError::OffFace { station: index, distance: dist });
    }
    let natural = surface_normal_at(surface, u, v)
        .map_err(|e| piece.eval_err(e))?
        .ok_or(EdgeError::DegenerateNormal { station: index })?;
    let n = match face.sense() {
        Sense::Same => natural,
        Sense::Reversed => scale3(natural, -1.0),
    };
    let c = dot(tan, n);
    if c.abs() > TANGENT_TOL {
        return Err(EdgeError::NotTangent { station: index, cos: c });
    }
    let t = unit(sub(tan, scale3(n, c)))?;
    let b = unit(cross(t, n))?;
    Ok(Station { s_arc, point, t, n, b, edge: piece.edge.clone(), param, face_distance: dist, tangent_normal_cos: c })
}

// ---------------------------------------------------------------- the function

/// Stations every `spacing` metres of arc length along the nominal edge, with their frames
/// (module documentation). Pure, D0.
pub fn stations(edge: &NominalEdge, spacing: f64, resolver: &dyn CadResolver) -> Result<NominalStations, EdgeError> {
    if !(spacing.is_finite() && spacing > 0.0) {
        return Err(EdgeError::BadSpacing { spacing });
    }
    let body = resolver.body();
    let (face, pieces) = match edge {
        NominalEdge::CadEdges { face, edges } => resolve_cad(body, face, edges)?,
        NominalEdge::Constructed { curve, range, face, material } => {
            let f = resolve_face(body, face)?;
            if !(range[0].is_finite() && range[1].is_finite() && range[0] < range[1]) {
                return Err(EdgeError::BadRange { range: *range });
            }
            let reversed = *material == MaterialSide::Right;
            (f, vec![Piece::new(None, curve, *range, reversed, None)?])
        }
    };
    let (pieces, closed) = chain(pieces)?;
    // Arc-length tables, piece starts along the chain.
    let mut tables = Vec::with_capacity(pieces.len());
    let mut chain_out = Vec::with_capacity(pieces.len());
    let mut starts = Vec::with_capacity(pieces.len());
    let mut total = 0.0;
    for p in &pieces {
        let (panels, len) = p.panels()?;
        if len <= CHAIN_GAP_TOL_M {
            return Err(EdgeError::ZeroLength { edge: p.edge.clone(), length: len });
        }
        starts.push(total);
        total += len;
        chain_out.push(ChainPiece { edge: p.edge.clone(), reversed: p.reversed, length: len });
        tables.push((panels, len));
    }
    if !total.is_finite() {
        return Err(EdgeError::NonFinite);
    }
    let ratio = total / spacing;
    if !ratio.is_finite() || ratio >= MAX_STATIONS as f64 {
        return Err(EdgeError::TooManyStations { length: total, spacing });
    }
    let tol = LENGTH_TOL_ABS_M + LENGTH_TOL_REL * total;
    let mut out = Vec::new();
    for i in 0..=MAX_STATIONS {
        let s = i as f64 * spacing;
        let keep = if closed { s < total - tol } else { s <= total + tol };
        if !keep {
            break;
        }
        // The piece holding s: the last start at or before s, or the next one within tol.
        let mut k = starts.partition_point(|&a| a <= s).saturating_sub(1);
        if let Some(&next) = starts.get(k + 1) {
            if next - s <= tol {
                k += 1;
            }
        }
        let (piece, (panels, len), start) = match (pieces.get(k), tables.get(k), starts.get(k)) {
            (Some(p), Some(t), Some(a)) => (p, t, *a),
            _ => return Err(EdgeError::NonFinite),
        };
        let local = s - start;
        let local = if local < 0.0 {
            0.0
        } else if local > *len {
            *len
        } else {
            local
        };
        let sigma = if piece.reversed { *len - local } else { local };
        let param = piece.param_at(panels, *len, sigma)?;
        out.push(frame(face, piece, param, s, i)?);
    }
    Ok(NominalStations { stations: out, chain: chain_out, length: total, closed, spacing })
}
