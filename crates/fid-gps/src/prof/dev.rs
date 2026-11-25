//! Signed deviations from the nominal CAD faces with their approximation error. Spec node
//! `GDT.PROF.DEV` (REQ-001 to REQ-004; REQ-005 open: the ORS-PROJ cases are unpublished).
//!
//! [`FaceSet::new`] prepares the toleranced faces of one B-rep [`Body`] (GEO.CAD.TOPO.MODEL),
//! each with its declared `nominal_approximation_error`; [`deviations`] maps every measured
//! point into the nominal frame by the pose and returns its [`Deviation`]: the closest point
//! on the toleranced faces, the face ID, the outward unit normal, the signed deviation and
//! the face's approximation error, or [`DevOutcome::NoProjection`]. The profile and runout
//! zones (GDT.PROF.ZDEF, LINE, SURF, RUNOUT) consume the result.
//!
//! # Projection (REQ-001)
//!
//! - **Pose.** A point `p` of the measured frame maps to `q = R p + t` in the nominal (CAD)
//!   frame ([`Rigid`], GEO.FRAME.XFORM `Transform::apply_point`). Feet and normals are in the
//!   nominal frame.
//! - **Closest point.** GEO.DIST.FACE.BODY [`FaceSetTarget::closest`] over the toleranced
//!   faces only, with the search radius as its maximum distance: exact surfaces and edge
//!   curves, never a tessellation; equal distances within 1e-15 m resolve to the lowest face
//!   ID (AC-3). Nothing within the radius is [`DevOutcome::NoProjection`] (REQ-004); the
//!   caller excludes such points from zone evaluation.
//! - **Sign.** GEO.DIST.FACE.INCID [`Incid::signed`] on the whole body: the sign of
//!   `(q - foot) . N` with `N` the angle-weighted outward normals of the faces incident at
//!   the foot (one face for an interior foot, two at an edge, the fan at a vertex). The
//!   signed deviation is `+d` on the outward side of the nominal (the point lies beyond the
//!   material boundary: excess material) and `-d` inside the material, per the leaf's
//!   Algorithm (`sign = sign of (p - foot) . n`, `n` outward from material).
//! - **Edge of the toleranced set.** At an edge or vertex foot whose adjacent faces
//!   (GEO.CAD.TOPO.ADJ `faces_of_edge` / `faces_of_vertex`) include a face that is **not**
//!   toleranced, no sign is given: the incident normals there would mix in a face that is not
//!   part of the profile, so the side of the toleranced surface is not defined (a point just
//!   below the base of a dome, beside a toleranced dome, would otherwise read as inside the
//!   dome). Such a deviation has `signed_dev: None`, status
//!   `Undefined(SignUndefined::Boundary)` (the foot is on the boundary of the toleranced set)
//!   and **no normal** (`normal: None`); its foot, face, unsigned distance and `on_boundary`
//!   are still given.
//! - **Normal.** The face's outward unit normal at an interior foot (face sense applied);
//!   at an edge or vertex foot inside the toleranced set, `N / |N|` (the direction the sign
//!   is decided on).
//!
//! **Honesty (D-045).** A deviation never looks measured when it is not:
//! [`Projected::signed_dev`] is `None` whenever GEO.DIST.SIGN gives no sign of record
//! (`Undecidable`: the point is within the foot's error bound of the surface; `Undefined`:
//! an open boundary or a non-manifold or degenerate foot), and [`Projected::normal`] is
//! `None` when no normal is defined there. The unsigned [`Projected::distance`] and the
//! [`Projected::sign`] status are always given.
//!
//! # Approximation error (REQ-002)
//!
//! Every projected deviation carries the hit face's approximation error (`0.0` for an exact
//! analytic face, never omitted); [`DeviationSet::max_approx_error`] is the largest over the
//! faces hit, `None` only when no point was projected.
//!
//! The value is **declared by the caller** in [`TolerancedFace`] and is **not verified**
//! here (D-045): no GEO.CAD or IO.CAD node computes or stores it yet, so it is carried as a
//! declared contributor, never as a measured or checked one. Deviations are measured from the
//! surface the body represents (for an approximated face, the B-spline, not the design
//! nominal); when `|signed_dev| <= approx_error` the sign is relative to that represented
//! surface and may differ from the sign against the design nominal.
//!
//! # Boundary flag (REQ-003)
//!
//! `on_boundary` is set when the foot is an edge or vertex foot of GEO.DIST.FACE.ONE, or an
//! interior foot whose surface parameters GEO.CAD.TOPO.TRIM [`classify_uv`] classifies
//! `OnBoundary` with the band [`BOUNDARY_TOL_M`] = 1e-9 m (every point within 1e-9 m of the
//! face boundary image is `OnBoundary`). Seams of periodic surfaces are not boundary.
//!
//! # Fail closed
//!
//! - **Torus faces.** GEO.CAD.TOPO.TRIM cannot yet classify a torus face with a seam
//!   (`Undecidable(DoublyPeriodicSeam)`, the open AC-1 gap of GEO.DIST.FACE.ONE and BODY),
//!   so a face set naming any torus face is refused ([`DevError::TorusUnsupported`]) whether
//!   or not its trim would happen to classify: no profile is evaluated on a torus face.
//! - Any error of the projection, the sign or the boundary test fails the whole call with
//!   the point's index; no partial result is returned.
//!
//! # Determinism (D0, KNP-1)
//!
//! Each point is a pure function of its coordinates, the pose and the face set, visited in
//! input order. The only arithmetic here is the angle-weighted normal sum, in a fixed order,
//! one binary64 operation at a time (no FMA, no `f64::min`/`max`), the square root through
//! `fid_num::f`, and the maximum by `>` over non-negative finite values. NaN and infinite
//! inputs are errors. No hash containers; nothing panics.

use core::fmt;
use std::collections::BTreeMap;

use fid_canon::{digest_bytes, to_canonical_bytes, CValue};
use fid_geo_cad::{classify_uv, AdjError, Body, EntityIx, FaceIx, TopologyId, TrimClass, TrimError, TrimIndex};
use fid_geo_dist::{
    BodyError, FaceFeature, FaceHit, FaceSetTarget, Incid, IncidError, Incident, ProjectionOptions, SignStatus,
    SignUndefined, Undefined,
};
use fid_geo_frame::{Rigid, Transform, XformError};
use fid_geo_nurbs::AnalyticSurface;
use fid_geo_types::FrameRef;
use fid_num::f as numf;
use fid_types::Digest;

type V3 = [f64; 3];

/// REQ-003: a foot within this distance of a face boundary is `on_boundary`, metres.
pub const BOUNDARY_TOL_M: f64 = 1e-9;

/// Most points one call accepts: indexes are `u32`.
pub const MAX_POINTS: usize = u32::MAX as usize;

/// Smallest `|N|` of an angle-weighted edge or vertex normal that is normalised; below it
/// the normal is `None` (the incident normals cancel).
const MIN_NORMAL_LEN: f64 = 1e-12;

// ---------------------------------------------------------------- errors

/// Every error of this module. [`DevError::code`] is stable.
#[derive(Debug, Clone, PartialEq)]
pub enum DevError {
    /// `empty_face_set`: no toleranced face.
    EmptyFaceSet,
    /// `face_unknown`: a toleranced face ID is not a face of the body.
    FaceUnknown { face: TopologyId },
    /// `duplicate_face`: a face was named twice.
    DuplicateFace { face: TopologyId },
    /// `bad_approx_error`: a declared approximation error is NaN, infinite or negative.
    BadApproxError { face: TopologyId },
    /// `torus_unsupported`: a toleranced face is a torus (fail closed, module docs).
    TorusUnsupported { face: TopologyId },
    /// `bad_search_radius`: the search radius is NaN, infinite or negative.
    BadSearchRadius,
    /// `too_many_points`: more than [`MAX_POINTS`] points.
    TooManyPoints,
    /// `nan_input`: a point coordinate is NaN or infinite.
    NanInput { index: u32 },
    /// `target`: GEO.DIST.FACE.BODY refused the face set.
    Target(BodyError),
    /// `incid`: GEO.DIST.FACE.INCID could not prepare the body.
    Incid(IncidError),
    /// `transform`: the pose image of a point is not finite.
    Transform { index: u32, error: XformError },
    /// `projection`: a projection error, passed through (fail closed).
    Projection { index: u32, error: BodyError },
    /// `sign`: the sign or the incident normals of a foot could not be computed.
    Sign { index: u32, error: IncidError },
    /// `adjacency`: the faces around an edge or vertex foot could not be listed.
    Adjacency { index: u32, error: AdjError },
    /// `boundary`: the boundary test of an interior foot failed (its trim index could not
    /// be built, or the classification failed).
    Boundary { index: u32, face: TopologyId, error: TrimError },
    /// `pose_digest`: the pose could not be encoded (never expected for a validated pose).
    PoseDigest,
    /// `inconsistent`: an internal invariant failed (never expected).
    Inconsistent,
}

impl DevError {
    /// A stable machine-readable code.
    pub const fn code(&self) -> &'static str {
        match self {
            DevError::EmptyFaceSet => "empty_face_set",
            DevError::FaceUnknown { .. } => "face_unknown",
            DevError::DuplicateFace { .. } => "duplicate_face",
            DevError::BadApproxError { .. } => "bad_approx_error",
            DevError::TorusUnsupported { .. } => "torus_unsupported",
            DevError::BadSearchRadius => "bad_search_radius",
            DevError::TooManyPoints => "too_many_points",
            DevError::NanInput { .. } => "nan_input",
            DevError::Target(_) => "target",
            DevError::Incid(_) => "incid",
            DevError::Transform { .. } => "transform",
            DevError::Projection { .. } => "projection",
            DevError::Sign { .. } => "sign",
            DevError::Adjacency { .. } => "adjacency",
            DevError::Boundary { .. } => "boundary",
            DevError::PoseDigest => "pose_digest",
            DevError::Inconsistent => "inconsistent",
        }
    }
}

impl fmt::Display for DevError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DevError::EmptyFaceSet => f.write_str("empty_face_set"),
            DevError::FaceUnknown { face } => write!(f, "face_unknown{{{face}}}"),
            DevError::DuplicateFace { face } => write!(f, "duplicate_face{{{face}}}"),
            DevError::BadApproxError { face } => write!(f, "bad_approx_error{{{face}}}"),
            DevError::TorusUnsupported { face } => write!(f, "torus_unsupported{{{face}}}"),
            DevError::BadSearchRadius => f.write_str("bad_search_radius"),
            DevError::TooManyPoints => f.write_str("too_many_points"),
            DevError::NanInput { index } => write!(f, "nan_input{{point {index}}}"),
            DevError::Target(e) => write!(f, "target: {e}"),
            DevError::Incid(e) => write!(f, "incid: {e}"),
            DevError::Transform { index, error } => write!(f, "transform{{point {index}}}: {error}"),
            DevError::Projection { index, error } => write!(f, "projection{{point {index}}}: {error}"),
            DevError::Sign { index, error } => write!(f, "sign{{point {index}}}: {error}"),
            DevError::Adjacency { index, error } => write!(f, "adjacency{{point {index}}}: {error}"),
            DevError::Boundary { index, face, error } => write!(f, "boundary{{point {index}, face {face}}}: {error}"),
            DevError::PoseDigest => f.write_str("pose_digest"),
            DevError::Inconsistent => f.write_str("inconsistent"),
        }
    }
}

impl std::error::Error for DevError {}

// ---------------------------------------------------------------- the face set

/// One toleranced face and its declared `nominal_approximation_error` (metres, `0.0` for an
/// exact analytic face), as GEO.CAD / IO.CAD supply it.
#[derive(Debug, Clone, PartialEq)]
pub struct TolerancedFace {
    /// The face's topology ID.
    pub face: TopologyId,
    /// The face's nominal approximation error, metres (finite, `>= 0`): declared by the
    /// caller, not verified here (module docs).
    pub approx_error_m: f64,
}

#[derive(Debug)]
struct Member {
    ix: FaceIx,
    id: TopologyId,
    approx: f64,
    trim: Result<TrimIndex, TrimError>,
}

/// The toleranced faces of one body, prepared for [`deviations`] (Design: `FaceSet`).
#[derive(Debug)]
pub struct FaceSet<'b> {
    target: FaceSetTarget<'b>,
    incid: Incid<'b>,
    /// Ascending face index.
    members: Vec<Member>,
}

impl<'b> FaceSet<'b> {
    /// The faces `faces` of `body` (any order, no repeats), whose coordinates are in `frame`
    /// (the nominal frame).
    pub fn new(body: &'b Body, frame: FrameRef, faces: &[TolerancedFace]) -> Result<FaceSet<'b>, DevError> {
        if faces.is_empty() {
            return Err(DevError::EmptyFaceSet);
        }
        let mut list: Vec<(FaceIx, &TolerancedFace)> = Vec::with_capacity(faces.len());
        for tf in faces {
            let ix = match body.lookup(&tf.face) {
                Some(EntityIx::Face(ix)) => ix,
                _ => return Err(DevError::FaceUnknown { face: tf.face.clone() }),
            };
            if !(tf.approx_error_m.is_finite() && tf.approx_error_m >= 0.0) {
                return Err(DevError::BadApproxError { face: tf.face.clone() });
            }
            let face = body.face(ix).ok_or(DevError::Inconsistent)?;
            if let Some(AnalyticSurface::Torus(_)) = face.surface().as_analytic() {
                return Err(DevError::TorusUnsupported { face: tf.face.clone() });
            }
            list.push((ix, tf));
        }
        list.sort_by_key(|a| a.0);
        for w in list.windows(2) {
            if w[0].0 == w[1].0 {
                return Err(DevError::DuplicateFace { face: w[1].1.face.clone() });
            }
        }
        let ixs: Vec<FaceIx> = list.iter().map(|(ix, _)| *ix).collect();
        let target = FaceSetTarget::new(body, frame, &ixs).map_err(DevError::Target)?;
        let incid = Incid::new(body).map_err(DevError::Incid)?;
        let mut members = Vec::with_capacity(list.len());
        for (ix, tf) in list {
            let id = body.face(ix).ok_or(DevError::Inconsistent)?.id().clone();
            let trim = TrimIndex::build(body, incid.pcurves(), ix);
            members.push(Member { ix, id, approx: tf.approx_error_m, trim });
        }
        Ok(FaceSet { target, incid, members })
    }

    /// The body.
    pub fn body(&self) -> &'b Body {
        self.target.body()
    }

    /// The toleranced faces and their approximation errors, ascending face index.
    pub fn faces(&self) -> Vec<TolerancedFace> {
        self.members.iter().map(|m| TolerancedFace { face: m.id.clone(), approx_error_m: m.approx }).collect()
    }

    fn contains(&self, id: &TopologyId) -> bool {
        self.members.iter().any(|m| &m.id == id)
    }

    /// Whether every face adjacent to an edge or vertex foot is toleranced (an interior foot:
    /// always).
    fn all_toleranced(&self, feature: &FaceFeature) -> Result<bool, AdjError> {
        let adj = self.incid.adjacency();
        let around = match feature {
            FaceFeature::Interior => return Ok(true),
            FaceFeature::Edge(e) => adj.faces_of_edge(e)?,
            FaceFeature::Vertex(v) => adj.faces_of_vertex(v)?,
        };
        Ok(around.iter().all(|f| self.contains(f)))
    }

    fn member(&self, ix: FaceIx) -> Result<&Member, DevError> {
        match self.members.binary_search_by(|m| m.ix.cmp(&ix)) {
            Ok(k) => self.members.get(k).ok_or(DevError::Inconsistent),
            Err(_) => Err(DevError::Inconsistent),
        }
    }
}

// ---------------------------------------------------------------- results

/// A projected point (Design: `Deviation` fields).
#[derive(Debug, Clone, PartialEq)]
pub struct Projected {
    /// The hit face (the lowest face ID on a tie).
    pub face_id: TopologyId,
    /// The closest point on the toleranced faces, nominal frame.
    pub foot_point: V3,
    /// The outward unit normal there (module docs); `None` where none is defined.
    pub normal: Option<V3>,
    /// The unsigned distance `|q - foot|`, metres.
    pub distance: f64,
    /// The signed deviation, metres: `+distance` outside the material, `-distance` inside;
    /// `None` when GEO.DIST.SIGN gives no sign of record or the foot is on the boundary of
    /// the toleranced set (D-045).
    pub signed_dev: Option<f64>,
    /// The sign status of GEO.DIST.SIGN.
    pub sign: SignStatus,
    /// The hit face's declared nominal approximation error, metres (never omitted; not
    /// verified here).
    pub approx_error: f64,
    /// REQ-003: the foot lies on the face boundary within [`BOUNDARY_TOL_M`].
    pub on_boundary: bool,
}

/// The outcome for one point.
#[derive(Debug, Clone, PartialEq)]
pub enum DevOutcome {
    /// The closest point within the search radius.
    Projected(Projected),
    /// REQ-004: farther than the search radius from every toleranced face.
    NoProjection,
}

/// The deviation of one point (Design: `Deviation`).
#[derive(Debug, Clone, PartialEq)]
pub struct Deviation {
    /// Position of the point in the input.
    pub index: u32,
    /// The projection or [`DevOutcome::NoProjection`].
    pub outcome: DevOutcome,
}

impl Deviation {
    /// Flag `no_projection` (REQ-004).
    pub fn no_projection(&self) -> bool {
        matches!(self.outcome, DevOutcome::NoProjection)
    }

    /// Flag `on_boundary` (REQ-003); `false` without a projection.
    pub fn on_boundary(&self) -> bool {
        match &self.outcome {
            DevOutcome::Projected(p) => p.on_boundary,
            DevOutcome::NoProjection => false,
        }
    }

    /// The projection, if any.
    pub fn projected(&self) -> Option<&Projected> {
        match &self.outcome {
            DevOutcome::Projected(p) => Some(p),
            DevOutcome::NoProjection => None,
        }
    }
}

/// All deviations of one call (Design: `DeviationSet`).
#[derive(Debug, Clone, PartialEq)]
pub struct DeviationSet {
    /// One per input point, in input order.
    pub items: Vec<Deviation>,
    /// REQ-002: the largest approximation error over the faces hit; `None` only when no
    /// point was projected.
    pub max_approx_error: Option<f64>,
    /// [`pose_digest`] of the pose used.
    pub pose_digest: Digest,
}

impl DeviationSet {
    /// How many points have no projection.
    pub fn no_projection_count(&self) -> usize {
        self.items.iter().filter(|d| d.no_projection()).count()
    }
}

// ---------------------------------------------------------------- pose digest

/// RFC 8785 canonical JSON of a pose, `{"r": [[r00, r01, r02], [r10, ..], [r20, ..]], "t":
/// [t0, t1, t2]}`, every number an IEEE-754 hex string `"0x..."`.
pub fn pose_canonical_bytes(pose: &Rigid) -> Result<Vec<u8>, DevError> {
    let r = pose.r();
    let t = pose.t();
    let row = |x: [f64; 3]| CValue::Arr(x.iter().map(|v| CValue::F64Bits(*v)).collect());
    let mut m = BTreeMap::new();
    m.insert("r".to_owned(), CValue::Arr(vec![row(r[0]), row(r[1]), row(r[2])]));
    m.insert("t".to_owned(), row(t));
    to_canonical_bytes(&CValue::Obj(m)).map_err(|_| DevError::PoseDigest)
}

/// SHA-256 of [`pose_canonical_bytes`] (`DeviationSet::pose_digest`).
pub fn pose_digest(pose: &Rigid) -> Result<Digest, DevError> {
    let bytes = pose_canonical_bytes(pose)?;
    Ok(Digest::from_bytes(digest_bytes(&bytes).0))
}

// ---------------------------------------------------------------- deviations

fn finite3(a: &V3) -> bool {
    a.iter().all(|x| x.is_finite())
}

/// The outward unit normal of a foot from its incident faces (module docs).
fn foot_normal(list: &[Incident]) -> Option<V3> {
    if let [one] = list {
        return finite3(&one.normal).then_some(one.normal);
    }
    let mut n = [0.0f64; 3];
    for inc in list {
        for (k, c) in n.iter_mut().enumerate() {
            *c += inc.angle * inc.normal[k];
        }
    }
    let len = numf::sqrt((n[0] * n[0] + n[1] * n[1]) + n[2] * n[2]).ok()?;
    if !(len.is_finite() && len > MIN_NORMAL_LEN) {
        return None;
    }
    let u = [n[0] / len, n[1] / len, n[2] / len];
    finite3(&u).then_some(u)
}

fn project_one(faces: &FaceSet<'_>, index: u32, q: V3, hit: FaceHit, ix: FaceIx) -> Result<Projected, DevError> {
    let member = faces.member(ix)?;
    let on_boundary = match &hit.feature {
        FaceFeature::Interior => {
            let boundary = |error: TrimError| DevError::Boundary { index, face: member.id.clone(), error };
            let trim = member.trim.as_ref().map_err(|e| boundary(*e))?;
            classify_uv(trim, hit.uv[0], hit.uv[1], BOUNDARY_TOL_M).map_err(boundary)? == TrimClass::OnBoundary
        }
        FaceFeature::Edge(_) | FaceFeature::Vertex(_) => true,
    };
    if !faces.all_toleranced(&hit.feature).map_err(|error| DevError::Adjacency { index, error })? {
        // The foot is on the boundary of the toleranced set (module docs): no sign, no normal.
        return Ok(Projected {
            face_id: member.id.clone(),
            foot_point: hit.foot,
            normal: None,
            distance: hit.distance,
            signed_dev: None,
            sign: SignStatus::Undefined(SignUndefined::Boundary),
            approx_error: member.approx,
            on_boundary,
        });
    }
    let signed = faces.incid.signed(q, &hit).map_err(|error| DevError::Sign { index, error })?;
    let normal = match faces.incid.incident_at(&hit) {
        Ok(list) => foot_normal(&list),
        Err(Undefined::Sign { .. }) => None,
        Err(Undefined::Error(error)) => return Err(DevError::Sign { index, error }),
    };
    let signed_dev = if signed.sign_defined() { Some(signed.distance) } else { None };
    Ok(Projected {
        face_id: member.id.clone(),
        foot_point: hit.foot,
        normal,
        distance: hit.distance,
        signed_dev,
        sign: signed.status,
        approx_error: member.approx,
        on_boundary,
    })
}

/// The signed deviation of every point of `points` (measured frame) from the toleranced
/// faces of `faces`, after mapping by `pose` into the nominal frame; points farther than
/// `search_radius_m` from every toleranced face get [`DevOutcome::NoProjection`]
/// (module docs).
pub fn deviations(
    points: &[V3],
    pose: &Rigid,
    faces: &FaceSet<'_>,
    search_radius_m: f64,
) -> Result<DeviationSet, DevError> {
    if points.len() > MAX_POINTS {
        return Err(DevError::TooManyPoints);
    }
    if !(search_radius_m.is_finite() && search_radius_m >= 0.0) {
        return Err(DevError::BadSearchRadius);
    }
    let pose_digest = pose_digest(pose)?;
    let t = Transform::Rigid(*pose);
    let opts = ProjectionOptions { max_distance: Some(search_radius_m), ..ProjectionOptions::default() };
    let mut items = Vec::with_capacity(points.len());
    let mut max_approx: Option<f64> = None;
    for (k, p) in points.iter().enumerate() {
        let index = u32::try_from(k).map_err(|_| DevError::TooManyPoints)?;
        if !finite3(p) {
            return Err(DevError::NanInput { index });
        }
        let q = t.apply_point(p).map_err(|error| DevError::Transform { index, error })?;
        let found = faces.target.closest(q, &opts).map_err(|error| DevError::Projection { index, error })?;
        let outcome = match found {
            None => DevOutcome::NoProjection,
            Some((ix, hit)) => {
                let pr = project_one(faces, index, q, hit, ix)?;
                // Both values are finite and >= 0 (checked in `FaceSet::new`).
                max_approx = match max_approx {
                    Some(m) if pr.approx_error <= m => Some(m),
                    _ => Some(pr.approx_error),
                };
                DevOutcome::Projected(pr)
            }
        };
        items.push(Deviation { index, outcome });
    }
    Ok(DeviationSet { items, max_approx_error: max_approx, pose_digest })
}
