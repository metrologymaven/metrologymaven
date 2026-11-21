//! The measured planar path of a DRF (spec node `GDT.DRF.BUILD`, P1a: planar datum features at
//! RMB): from the measured points of one to three planar datum features, in precedence order,
//! to certified simulated datum planes and the DRF [`build_drf`](super::build::build_drf) assembles from them.
//!
//! Each datum plane is the tangential plane of `fid-fit` (`KRN.ENV.TAN.PLANE`) in the variant
//! the DMT row's association step names ([`variant_of`]; constrained L2 is refused because
//! `fid-fit` cannot certify it). Only a certified or closed-form result is ever used; an
//! iterate, a failed start, an uncertified fit or a certified fit flagged
//! `local_optimum_only` (its starts disagreed: `flagged_fit{label}`) never becomes a datum.
//!
//! # Orientation constraint per datum (pinned)
//!
//! The constraint a datum's simulator must respect follows from the DoFs its predecessors left
//! free, as the DoF table of [`build_drf`](super::build::build_drf) on the predecessors states it, and from the datum's
//! **nominal** outward normal compared with its predecessors' nominal outward normals (the
//! basic relationship comes from the nominal, never from the measured planes):
//!
//! | free rotations before it | nominal relationship | simulated plane |
//! |---|---|---|
//! | all (primary) | any | free normal: `fit_tan_plane` without constraint; must be certified (`strict_local`, with its `fid.cert@1` certificate) |
//! | `rz` only (after a primary plane) | perpendicular to the primary nominal | normal held perpendicular to the DRF `z`, free about it (below) |
//! | `rz` only | parallel to the primary nominal | normal fixed at `+-z` (then refused by [`build_drf`](super::build::build_drf) as a degenerate pair) |
//! | none (after two planes) | parallel to the nominal `z`, `y` or `x` (`x = y x z` of the nominals) | normal fixed at the measured DRF axis, signed by the nominal |
//! | any | any other angle | refused, `oblique_datum_unsupported{label}` |
//!
//! "Parallel" and "perpendicular" between nominals use [`ANGLE_TOL`] (sine and cosine of the
//! angle), as the builder does.
//!
//! # Held perpendicular to `z`, free about it (exact, by enumeration)
//!
//! The normal is `n(t) = cos t e1 + sin t e2`, `e1` the nominal made perpendicular to `z` and
//! unit, `e2 = z x e1`, `t` in `[-pi/2, pi/2]` (the material-free half). For a fixed normal,
//! `fid-fit` gives the exact optimum in closed form, and its objective is a function of `t`
//! alone. With `q_i` the points' coordinates `(u_i, v_i)` along `(e1, e2)`:
//!
//! - external minimax: the largest depth is `max_i q_i.n - min_j q_j.n`; between the angles where
//!   the maximising or the minimising point changes, it is `(q_a - q_b).n(t)`, a non-negative
//!   sinusoid, so concave;
//! - constrained L1: the sum of depths is `N max_i q_i.n - sum_k q_k.n`; between the angles where
//!   the maximising point changes it is `N (q_a - mean).n(t)`, non-negative, so concave.
//!
//! A concave piece takes its minimum at an end, and the ends are the normals perpendicular to an
//! edge of the convex hull of the `q_i`, or the window ends `+-e2`. So the optimum is the best of
//! the `fid-fit` fixed-normal fits at the hull-edge normals (signed into the half `n.e1 > 0`).
//! The hull is Andrew's monotone chain (sorted with `total_cmp`, collinear points dropped);
//! candidates are tried in hull order and the first with the smallest objective wins, so the
//! choice is deterministic. When a window end is at least as good as the best candidate the
//! optimum lies at 90 degrees to the nominal and the datum is refused (`off_nominal{label}`);
//! so is a selected normal more than 60 degrees from the nominal ([`MIN_NOMINAL_COS`]), which
//! catches a face seen edge-on whose hull rounding hides the window end.
//! The orientation of near-collinear hull points is decided in floating point
//! [unverified: no robust predicate]. Constrained L2 is not concave on its pieces, so it is
//! refused here (`simulation_failed{label:not_certifiable}`), as `fid-fit` refuses it with a free
//! normal.
//!
//! # Determinism
//!
//! D0: `fid-fit` with exactly [`STARTS`] deterministic starts (no random start, so no seed), a
//! fixed candidate order, square roots through `fid_num::f::sqrt`, no FMA, no `f64::min` or
//! `f64::max`, every NaN refused.

use super::build::{Drf, DrfError, SimGeometry, SimulatedDatum, ANGLE_TOL, MAX_DATUMS};
use super::cyl_sim::{build_measured_drf, MeasuredFeature, MeasuredKind, Simulators};
use crate::dmt::{AlgId, DmtKey};
use fid_fit::contract::{Constraint, FitParams, FittedFeature, Method, PointsView};
use fid_fit::tan_plane::{algorithm_id, fit_tan_plane, output_digest, Optimality, TanPlaneParams, TanPlaneResult};
use fid_fit::tan_rock::{MaterialSide, TanVariant};
use fid_types::Digest;
use std::fmt;

/// Largest accepted violation of a simulated plane (m), passed to `fid-fit` as `side_tol`.
pub const SIDE_TOL: f64 = 1e-12;
/// Relative rocking tolerance passed to `fid-fit` as `rock_tol`.
pub const ROCK_TOL: f64 = 1e-9;
/// Starts of a free-normal fit: the least-squares start and the ring of four tilts, so no
/// random start and no seed.
pub const STARTS: u32 = 1 + fid_fit::tan_plane::RING_STARTS;
/// Smallest cosine between a rotation-constrained datum's selected normal and its nominal
/// (made perpendicular to `z`): 60 degrees; a larger angle is `off_nominal{label}`.
pub const MIN_NOMINAL_COS: f64 = 0.5;
/// Most hull-edge candidates of one rotation-constrained datum.
pub const MAX_CANDIDATES: usize = 4096;

/// One measured planar datum feature.
#[derive(Debug, Clone, Copy)]
pub struct MeasuredPlane<'a> {
    /// The datum label (`A`, `B`, ...).
    pub label: &'a str,
    /// The measured points as contiguous `x, y, z` triples (m, part frame).
    pub xyz: &'a [f64],
    /// The nominal outward unit normal of the datum face (from the CAD model): it names the
    /// material-free side and the basic relationship to the other datums.
    pub nominal_outward: [f64; 3],
}

/// How a datum plane's orientation was constrained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Orientation {
    /// Free normal (primary).
    Free,
    /// Held perpendicular to the DRF `z`, free about it.
    AboutZ,
    /// Normal fixed by the predecessors.
    Fixed,
}

impl Orientation {
    /// `free`, `about_z`, `fixed`.
    pub fn as_str(self) -> &'static str {
        match self {
            Orientation::Free => "free",
            Orientation::AboutZ => "about_z",
            Orientation::Fixed => "fixed",
        }
    }
}

/// The evidence behind one simulated datum (plane or, from [`super::cyl_sim`], axis): what was
/// fitted and how its optimality is known.
#[derive(Debug, Clone, PartialEq)]
pub struct DatumEvidence {
    /// The orientation constraint.
    pub orientation: Orientation,
    /// Planes: `strict_local` (free normal, certified), `closed_form` (fixed normal) or
    /// `closed_form_enumerated` (perpendicular to `z`, best of the hull-edge candidates).
    /// Cylinders: `strict_local_cross_checked` (free direction, certified, global cross-check
    /// passed) or `global` (fixed direction, a convex problem).
    pub optimality: &'static str,
    /// The `fid.cert@1` certificate digest (free plane normal, and every cylinder).
    pub certificate: Option<Digest>,
    /// The certificate's `local_optimum_only` flag: always false, since a flagged fit is
    /// refused (`flagged_fit{label}`); kept so the document states it.
    pub local_optimum_only: bool,
    /// `fid-fit`'s output digest of the selected fit.
    pub fit_digest: Digest,
    /// Number of measured points.
    pub points: u32,
    /// Candidates evaluated (1 unless enumerated).
    pub candidates: u32,
    /// The variant's objective (m); a cylinder's is its radius.
    pub objective: f64,
    /// The largest violation (m), at most [`SIDE_TOL`].
    pub max_violation: f64,
    /// The contact points, ascending.
    pub contacts: Vec<u32>,
    /// A free-direction cylinder's global cross-check outcome (always `passed`: any other is
    /// refused); `None` for planes and fixed-direction cylinders.
    pub cross_check: Option<&'static str>,
    /// The work of that cross-check: cap tests, full (all-point) tests and the deepest cap
    /// level reached; `None` where `cross_check` is. Recorded (and digested) so that a change
    /// in how the cross-check passes is visible on replay.
    pub cross_check_work: Option<[u32; 3]>,
    /// The multi-start seed of a cylinder fit; `None` for planes.
    pub seed: Option<u64>,
}

/// The evidence of a simulated plane (the name before cylinders were added).
pub type PlaneEvidence = DatumEvidence;

/// A DRF built from measured planes: the frame, the simulated datums and their evidence, in
/// precedence order.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasuredDrf {
    pub(super) drf: Drf,
    pub(super) datums: Vec<SimulatedDatum>,
    pub(super) evidence: Vec<DatumEvidence>,
}

impl MeasuredDrf {
    /// The DRF.
    pub fn drf(&self) -> &Drf {
        &self.drf
    }
    /// The simulated datums.
    pub fn datums(&self) -> &[SimulatedDatum] {
        &self.datums
    }
    /// The evidence of each simulated datum.
    pub fn evidence(&self) -> &[DatumEvidence] {
        &self.evidence
    }
}

/// Why the measured planar path refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimError {
    /// The builder refused the simulated datums ([`DrfError`]).
    Drf(DrfError),
    /// `unsupported_simulator{alg}`: the row's association algorithm is not a tangential plane
    /// of `fid-fit`.
    UnsupportedSimulator {
        /// The algorithm ID.
        alg: String,
    },
    /// `nominal_invalid{label}`: the nominal outward normal is not finite and unit within
    /// [`UNIT_TOL`](super::build::UNIT_TOL).
    NominalInvalid {
        /// The datum.
        label: String,
    },
    /// `oblique_datum_unsupported{label}`: the nominal is neither parallel nor perpendicular
    /// to the predecessors' nominals as the table above requires.
    ObliqueDatum {
        /// The datum.
        label: String,
    },
    /// `simulation_failed{label:code}`: the fit refused (the `fid-fit` error code).
    Fit {
        /// The datum.
        label: String,
        /// The `fid-fit` code, or `not_certifiable`, `too_few_points`, `too_many_candidates`.
        code: &'static str,
    },
    /// `not_certified{label}`: a free-normal fit returned without a certificate.
    NotCertified {
        /// The datum.
        label: String,
    },
    /// `flagged_fit{label}`: a certified fit whose starts disagreed (`local_optimum_only`),
    /// refused as `fid-fit`'s `FitError::FlaggedFit` refuses it behind the `Fit` trait: a
    /// flagged fit is never a datum.
    FlaggedFit {
        /// The datum.
        label: String,
    },
    /// `off_nominal{label}`: the constrained optimum lies at 90 degrees to the nominal.
    OffNominal {
        /// The datum.
        label: String,
    },
    /// `unsupported_datum_type_p1a{label}`: an internal cylindrical datum feature (a bore). Its
    /// simulator, the maximum inscribed cylinder (`KRN.ENV.MIC.CYL`), is not built, so no axis
    /// is made up for it.
    UnsupportedDatumType {
        /// The datum.
        label: String,
    },
    /// `orientation_constraint_unsupported{label}`: the datum's simulator would have to hold an
    /// orientation the fit cannot take: a cylinder whose nominal axis lies perpendicular to the
    /// frame's `z` while `rz` is free (its axis free about `z` only; `KRN.ENV.MCC.CYL` takes a
    /// free or a fixed direction).
    OrientationUnsupported {
        /// The datum.
        label: String,
    },
}

impl SimError {
    /// The stable code ([`DrfError::code`] for builder refusals).
    pub fn code(&self) -> &'static str {
        match self {
            SimError::Drf(e) => e.code(),
            SimError::UnsupportedSimulator { .. } => "unsupported_simulator",
            SimError::NominalInvalid { .. } => "nominal_invalid",
            SimError::ObliqueDatum { .. } => "oblique_datum_unsupported",
            SimError::Fit { .. } => "simulation_failed",
            SimError::NotCertified { .. } => "not_certified",
            SimError::FlaggedFit { .. } => "flagged_fit",
            SimError::OffNominal { .. } => "off_nominal",
            SimError::UnsupportedDatumType { .. } => "unsupported_datum_type_p1a",
            SimError::OrientationUnsupported { .. } => "orientation_constraint_unsupported",
        }
    }
}

impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SimError::Drf(e) => e.fmt(f),
            SimError::UnsupportedSimulator { alg: s }
            | SimError::NominalInvalid { label: s }
            | SimError::ObliqueDatum { label: s }
            | SimError::NotCertified { label: s }
            | SimError::FlaggedFit { label: s }
            | SimError::OffNominal { label: s }
            | SimError::UnsupportedDatumType { label: s }
            | SimError::OrientationUnsupported { label: s } => write!(f, "{}{{{}}}", self.code(), s.escape_debug()),
            SimError::Fit { label, code } => write!(f, "{}{{{}:{code}}}", self.code(), label.escape_debug()),
        }
    }
}

impl std::error::Error for SimError {}

impl From<DrfError> for SimError {
    fn from(e: DrfError) -> SimError {
        SimError::Drf(e)
    }
}

/// The tangential variant an association algorithm ID names: `fit.plane.tan_minimax@1.0.0`,
/// `fit.plane.tan_l1@1.0.0` or `fit.plane.tan_l2@1.0.0` exactly (`fid-fit`'s IDs); anything
/// else is `None`.
pub fn variant_of(alg: &AlgId) -> Option<TanVariant> {
    TanVariant::ALL.iter().copied().find(|v| algorithm_id(*v).to_string() == alg.as_str())
}

pub(super) type V3 = [f64; 3];

pub(super) fn dot(a: V3, b: V3) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

pub(super) fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub(super) fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub(super) fn reject(a: V3, u: V3) -> V3 {
    let d = dot(a, u);
    [a[0] - d * u[0], a[1] - d * u[1], a[2] - d * u[2]]
}

pub(super) fn norm(a: V3) -> Option<f64> {
    fid_num::f::sqrt(dot(a, a)).ok().filter(|n| n.is_finite())
}

pub(super) fn unit(a: V3) -> Option<V3> {
    let n = norm(a)?;
    let v = [a[0] / n, a[1] / n, a[2] / n];
    v.iter().all(|c| c.is_finite()).then_some(v)
}

pub(super) fn parallel(a: V3, b: V3) -> bool {
    norm(cross(a, b)).is_some_and(|s| s < ANGLE_TOL)
}

pub(super) fn perpendicular(a: V3, b: V3) -> bool {
    dot(a, b).abs() < ANGLE_TOL
}

pub(super) fn sign_toward(axis: V3, nominal: V3) -> V3 {
    if dot(axis, nominal) < 0.0 {
        scale(axis, -1.0)
    } else {
        axis
    }
}

pub(super) struct Ctx<'a> {
    pub(super) label: &'a str,
    pub(super) variant: TanVariant,
}

impl Ctx<'_> {
    pub(super) fn fit_err(&self, code: &'static str) -> SimError {
        SimError::Fit { label: self.label.to_owned(), code }
    }

    fn params(&self, side: V3) -> Result<TanPlaneParams, SimError> {
        let material_side = MaterialSide::outward(side).map_err(|e| self.fit_err(e.code()))?;
        Ok(TanPlaneParams {
            variant: Some(self.variant),
            material_side,
            side_tol: SIDE_TOL,
            rock_tol: ROCK_TOL,
            starts: STARTS,
        })
    }

    pub(super) fn fit(&self, pts: &PointsView<'_>, fixed: Option<V3>, side: V3) -> Result<TanPlaneResult, SimError> {
        let mut fp = FitParams::new(Method::Tangential);
        if let Some(n) = fixed {
            fp.constraints.push(Constraint::FixedAxisDirection(n));
        }
        fit_tan_plane(pts, &fp, &self.params(side)?, None).map_err(|e| self.fit_err(e.code()))
    }

    pub(super) fn evidence(
        &self,
        r: &TanPlaneResult,
        orientation: Orientation,
        optimality: &'static str,
        candidates: u32,
    ) -> Result<DatumEvidence, SimError> {
        let (certificate, local_optimum_only) = match (&r.certificate, r.optimality) {
            (Some(c), Optimality::StrictLocal) => (Some(*c.digest()), c.flags().local_optimum_only),
            (None, Optimality::ClosedForm) => (None, false),
            // Results are certified only: a free-normal result without its certificate (or a
            // closed form carrying one) is never used.
            _ => return Err(SimError::NotCertified { label: self.label.to_owned() }),
        };
        if orientation == Orientation::Free && certificate.is_none() {
            return Err(SimError::NotCertified { label: self.label.to_owned() });
        }
        // Every datum: a certificate flagged `local_optimum_only` (the starts disagreed) is
        // refused, never accepted with its flag.
        if local_optimum_only {
            return Err(SimError::FlaggedFit { label: self.label.to_owned() });
        }
        Ok(DatumEvidence {
            orientation,
            optimality,
            certificate,
            local_optimum_only,
            fit_digest: output_digest(r).map_err(|e| self.fit_err(e.code()))?,
            points: r.residuals.len() as u32,
            candidates,
            objective: r.objective,
            max_violation: r.max_violation,
            contacts: r.contacts.clone(),
            cross_check: None,
            cross_check_work: None,
            seed: None,
        })
    }
}

fn cross2(o: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

/// Andrew's monotone chain, counter-clockwise from the lowest `(u, v)`, collinear points
/// dropped; fewer than three distinct points are returned sorted and deduplicated.
fn hull(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut s = points.to_vec();
    s.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    s.dedup();
    if s.len() < 3 {
        return s;
    }
    let mut lower: Vec<[f64; 2]> = Vec::new();
    for p in &s {
        while lower.len() >= 2 && cross2(lower[lower.len() - 2], lower[lower.len() - 1], *p) <= 0.0 {
            lower.pop();
        }
        lower.push(*p);
    }
    let mut upper: Vec<[f64; 2]> = Vec::new();
    for p in s.iter().rev() {
        while upper.len() >= 2 && cross2(upper[upper.len() - 2], upper[upper.len() - 1], *p) <= 0.0 {
            upper.pop();
        }
        upper.push(*p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// The objective at a window end `+-e2` (module documentation), from the 2D coordinates.
fn end_value(variant: TanVariant, v: &[f64]) -> f64 {
    let mut hi = f64::NEG_INFINITY;
    let mut lo = f64::INFINITY;
    let mut sum = 0.0;
    for &x in v {
        if x > hi {
            hi = x;
        }
        if x < lo {
            lo = x;
        }
        sum += x;
    }
    match variant {
        TanVariant::ConstrainedL1 => (v.len() as f64) * hi - sum,
        _ => hi - lo,
    }
}

/// A plane held perpendicular to `z` and free about it (module documentation).
pub(super) fn about_z(
    ctx: &Ctx<'_>,
    pts: &PointsView<'_>,
    xyz: &[f64],
    z: V3,
    nominal: V3,
) -> Result<(TanPlaneResult, u32), SimError> {
    if ctx.variant == TanVariant::ConstrainedL2 {
        return Err(ctx.fit_err("not_certifiable"));
    }
    let e1 = unit(reject(nominal, z)).ok_or_else(|| SimError::ObliqueDatum { label: ctx.label.to_owned() })?;
    let e2 = cross(z, e1);
    let Some(o) = xyz.get(..3).map(|o| [o[0], o[1], o[2]]) else { return Err(ctx.fit_err("too_few_points")) };
    let uv: Vec<[f64; 2]> = xyz
        .chunks_exact(3)
        .map(|p| {
            let d = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
            [dot(d, e1), dot(d, e2)]
        })
        .collect();
    let h = hull(&uv);
    if h.len() < 2 {
        return Err(ctx.fit_err("too_few_points"));
    }
    let edges = if h.len() == 2 { 1 } else { h.len() };
    if edges > MAX_CANDIDATES {
        return Err(ctx.fit_err("too_many_candidates"));
    }
    let mut best: Option<TanPlaneResult> = None;
    let mut tried = 0_u32;
    for k in 0..edges {
        let (a, b) = (h[k], h[(k + 1) % h.len()]);
        let mut m = [b[1] - a[1], a[0] - b[0]];
        if m[0] < 0.0 {
            m = [-m[0], -m[1]];
        }
        if m[0] == 0.0 {
            // A normal perpendicular to the nominal: a window end, covered below.
            continue;
        }
        let n = [m[0] * e1[0] + m[1] * e2[0], m[0] * e1[1] + m[1] * e2[1], m[0] * e1[2] + m[1] * e2[2]];
        let r = ctx.fit(pts, Some(n), e1)?;
        tried += 1;
        let better = match &best {
            None => true,
            Some(b) => r.objective < b.objective,
        };
        if better {
            best = Some(r);
        }
    }
    let best = best.ok_or_else(|| SimError::OffNominal { label: ctx.label.to_owned() })?;
    // Guard against rounding defeating the window-end test (a face seen edge-on): the
    // selected normal must lie within 60 degrees of the nominal.
    let tilted = match best.feature {
        FittedFeature::Plane { n, .. } => dot(n, e1) < MIN_NOMINAL_COS,
        _ => true,
    };
    if tilted {
        return Err(SimError::OffNominal { label: ctx.label.to_owned() });
    }
    let v: Vec<f64> = uv.iter().map(|p| p[1]).collect();
    let neg: Vec<f64> = v.iter().map(|x| -x).collect();
    if end_value(ctx.variant, &v) <= best.objective || end_value(ctx.variant, &neg) <= best.objective {
        return Err(SimError::OffNominal { label: ctx.label.to_owned() });
    }
    Ok((best, tried))
}

pub(super) fn plane_of(label: &str, r: &TanPlaneResult) -> Result<SimGeometry, SimError> {
    match r.feature {
        FittedFeature::Plane { p, n } => Ok(SimGeometry::Plane { point: p, normal: n }),
        _ => Err(SimError::Fit { label: label.to_owned(), code: "not_a_plane" }),
    }
}

/// Simulates every datum plane in precedence order and builds the DRF (module documentation).
/// `alg` and `row` are the association algorithm of the resolved DMT row and its key; they are
/// recorded on every datum. Pure and deterministic; every outcome is a returned `Result`.
/// Planes only: [`super::cyl_sim::build_measured_drf`] takes planes and cylinders.
pub fn build_measured_planar_drf(
    planes: &[MeasuredPlane<'_>],
    alg: &AlgId,
    row: &DmtKey,
) -> Result<MeasuredDrf, SimError> {
    if planes.is_empty() {
        return Err(DrfError::NoDatums.into());
    }
    if planes.len() > MAX_DATUMS {
        return Err(DrfError::TooManyDatums { count: planes.len() }.into());
    }
    if variant_of(alg).is_none() {
        return Err(SimError::UnsupportedSimulator { alg: alg.as_str().to_owned() });
    }
    let sims = Simulators::from_association(&[alg])?;
    let features: Vec<MeasuredFeature<'_>> = planes
        .iter()
        .map(|p| MeasuredFeature { label: p.label, xyz: p.xyz, nominal: p.nominal_outward, kind: MeasuredKind::Plane })
        .collect();
    build_measured_drf(&features, &sims, row)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn closed_form() -> TanPlaneResult {
        let xyz = [0.0, 0.0, 0.0, 1.0, 0.0, -1e-6, 0.0, 1.0, -2e-6];
        let pts = PointsView::new(&xyz, None).unwrap();
        let ctx = Ctx { label: "A", variant: TanVariant::ExternalMinimax };
        ctx.fit(&pts, Some([0.0, 0.0, 1.0]), [0.0, 0.0, 1.0]).unwrap()
    }

    /// Results are certified only: a free-normal result without a certificate, or a
    /// strict-local one without its certificate, is `not_certified{label}`.
    #[test]
    fn an_uncertified_result_is_never_a_datum() {
        let ctx = Ctx { label: "A", variant: TanVariant::ExternalMinimax };
        let r = closed_form();
        assert_eq!(r.optimality, Optimality::ClosedForm);
        assert!(ctx.evidence(&r, Orientation::Fixed, "closed_form", 1).is_ok());
        let e = ctx.evidence(&r, Orientation::Free, "strict_local", 1).unwrap_err();
        assert_eq!(e.to_string(), "not_certified{A}");
        let mut uncertified = r.clone();
        uncertified.optimality = Optimality::StrictLocal;
        let e = ctx.evidence(&uncertified, Orientation::Free, "strict_local", 1).unwrap_err();
        assert_eq!(e.code(), "not_certified");
    }
}
