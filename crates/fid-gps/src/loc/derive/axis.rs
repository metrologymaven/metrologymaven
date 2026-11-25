//! Axis of an associated feature (spec node `GDT.LOC.DERIV.AXIS`), pure.
//!
//! The derived axis of a cylindrical or conical feature: associate the cylinder or cone to the
//! feature's points with exactly the `fid-fit` fit the chain names, take its axis, orient it to
//! the nominal direction's hemisphere and bound it by the extent of the points along it. It is
//! registered in [`DeriveRegistry::builtin`](super::DeriveRegistry::builtin) under the reduction
//! operator [`AXIS_REDUCTION_ALG`]; [`derive_axis`] is the registered [`DeriveFn`](super::DeriveFn).
//! GDT never decides conformance here: the element goes to the consumers (orientation, position,
//! coaxiality), which evaluate it.
//!
//! # Pinned procedure
//!
//! 1. **Feature.** The request's feature kind must be `cylinder` or `cone`; `width` is
//!    `UNSUPPORTED{alg}` with the reduction's algorithm ID.
//! 2. **Association step.** The chain's steps of kind `association` *before* the reduction step
//!    are counted; exactly one is required (none, or more than one, is `UNSUPPORTED{alg}` with
//!    the reduction's algorithm ID: the chain would not name one fit). The reduction and the
//!    association must carry no parameters (none are defined for them; a parameter is
//!    `UNSUPPORTED{alg}` with the step's ID), so nothing in the chain is silently ignored.
//! 3. **Fit** (REQ-001). The association's algorithm ID selects exactly one `fid-fit` fit, with
//!    no fallback; any other ID, or a fit of the other primitive (a cylinder fit on a cone, a
//!    cone fit on a cylinder), is `UNSUPPORTED{alg}` with the association's ID:
//!    - `fit.cylinder.ls@1.0.0` (cylinder): `fid_fit::ls_cyl::fit_cylinder`, least squares, no
//!      constraints, seed [`FIT_SEED`]; the axis is the result's `Cylinder { p, d }`.
//!    - `fit.cone.ls@1.0.0` (cone): `fid_fit::ls_cone::fit_cone` with `ConeOptions::default()`,
//!      seed [`FIT_SEED`]; the axis is the fit's reference `point` and `axis`.
//!    - `fit.cylinder.mc@1.0.0` (cylinder): `fid_fit::mcc_cyl::fit_mcc_cylinder`, free
//!      direction, seed [`FIT_SEED`]; the axis is the result's `Cylinder { p, d }`.
//!
//!    The minimum-zone and maximum-inscribed cylinders and cones the leaf also names are not in
//!    `fid-fit` yet (`KRN.MZ.RND.CYL`, `KRN.ENV.MIC.CYL`), so their IDs are `UNSUPPORTED{alg}`
//!    until they land. Every fit error (too few points, degenerate data, a solver or trust-region
//!    failure, an implausible fit) is `derivation_failed{reason: fit_failed}` (REQ-004). A fit
//!    `fid-fit` reports as `suspect_axis` (axis weakly determined or wrong), or a cone with
//!    `weak_axis`, is `fit_failed` too: the flag says the axis itself may be wrong, which is more
//!    than a local optimum, so it fails closed.
//! 4. **Optimality** (REQ-004, D-045). Never claimed without a `fid-fit` certificate:
//!    - the least-squares cylinder and cone have no global certificate (their multi-start
//!      agreement and plane bound only refuse wrong fits), so their axis is always
//!      [`Optimality::LocalOnly`], flagged `local_optimum_only`, whether or not `fid-fit` set
//!      the flag;
//!    - the minimum circumscribed cylinder is `Certified { GlobalFit { by } }` only when its
//!      record has no `local_optimum_only` (neither on the record nor on the certificate), it
//!      is a certified strict local minimum and its global cross-check ran and `passed` (the
//!      same acceptance as the DRF's cylindrical datum, `drf::cyl_sim::accept_cylinder`);
//!      anything else is `LocalOnly`.
//! 5. **Orientation** (REQ-003). With the fit's unit direction `d` and the nominal direction
//!    `n`: when `(d0 n0 + d1 n1) + d2 n2 < 0`, `d` is negated. Without a nominal direction the
//!    fit's canonical direction is kept (see the leaf's Open questions).
//! 6. **Extent** (REQ-002). With `c` the fit's axis point, `t_i = (w0 d0 + w1 d1) + w2 d2` for
//!    `w = p_i - c`, in index order; `t_min`, `t_max` their total-order minimum and maximum
//!    (the first index wins a tie; no value can be NaN). The extent is `t_max - t_min` and the
//!    axis point `c + m d` with `m = (t_min + t_max) / 2`, component by component.
//! 7. **Element.** `Axis::new(point, d, extent)`; it cannot fail on a finite fit, and if it did
//!    the derivation is `fit_failed`.
//!
//! # Determinism
//!
//! D3 per the leaf (iterative axial fits); for a given seed the fits are bit-identical across
//! platforms (D0 per seed, see `fid-fit`), and the arithmetic here is in the pinned order above
//! with no fused multiply-add and no hash container. Nothing panics on any input.

use super::{
    Axis, Certificate, DerivationReason, DeriveCall, DerivedElement, FeatureKind, ImplError, Optimality, Produced,
};
use crate::dmt::{AlgId, Operator, OperatorKind};
use fid_fit::contract::{Fit, FitError, FitParams, FittedFeature, Method, PointsView};
use fid_fit::ls_cone::{fit_cone, ConeLs, ConeOptions};
use fid_fit::ls_cyl::{fit_cylinder, CylinderLs};
use fid_fit::mcc_cyl::{fit_mcc_cylinder, CrossOutcome, MccCylinderFit, Optimality as MccOptimality};

/// The reduction operator's algorithm ID this derivation is registered under.
pub const AXIS_REDUCTION_ALG: &str = "gps.derive.axis@1.0.0";

/// The seed of every fit this derivation runs (multi-start).
pub const FIT_SEED: u64 = 0;

/// The axial fits this derivation can run, by the association's algorithm ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AxialMethod {
    CylinderLs,
    ConeLs,
    CylinderMc,
}

impl AxialMethod {
    const ALL: [AxialMethod; 3] = [AxialMethod::CylinderLs, AxialMethod::ConeLs, AxialMethod::CylinderMc];

    fn id(self) -> String {
        match self {
            AxialMethod::CylinderLs => CylinderLs.algorithm_id().to_string(),
            AxialMethod::ConeLs => ConeLs.algorithm_id().to_string(),
            AxialMethod::CylinderMc => MccCylinderFit.algorithm_id().to_string(),
        }
    }

    fn kind(self) -> FeatureKind {
        match self {
            AxialMethod::CylinderLs | AxialMethod::CylinderMc => FeatureKind::Cylinder,
            AxialMethod::ConeLs => FeatureKind::Cone,
        }
    }
}

/// The algorithm IDs of the fits this derivation runs (rule 3), ascending.
pub fn supported_fits() -> Vec<String> {
    let mut v: Vec<String> = AxialMethod::ALL.iter().map(|m| m.id()).collect();
    v.sort();
    v
}

/// The axis an axial fit gave, with what the fit proves about its optimality.
#[derive(Debug, Clone, PartialEq)]
pub struct AxialFit {
    alg: AlgId,
    point: [f64; 3],
    direction: [f64; 3],
    local_optimum_only: bool,
    global: bool,
}

impl AxialFit {
    /// A fit outcome with no global certificate: its axis is always [`Optimality::LocalOnly`]
    /// (rule 4). `local_optimum_only` is the fit's own flag, kept for the record. This is the
    /// form a test stub of the fit, or a fit without a certificate, takes; only this module
    /// builds a certified outcome, from a `fid-fit` certificate.
    pub fn uncertified(alg: AlgId, point: [f64; 3], direction: [f64; 3], local_optimum_only: bool) -> AxialFit {
        AxialFit { alg, point, direction, local_optimum_only, global: false }
    }

    /// The fit's algorithm ID (the chain's association step).
    pub fn alg(&self) -> &AlgId {
        &self.alg
    }

    /// The fit's axis point, in metres.
    pub fn point(&self) -> [f64; 3] {
        self.point
    }

    /// The fit's unit axis direction, before orientation.
    pub fn direction(&self) -> [f64; 3] {
        self.direction
    }

    /// Whether the fit itself flagged its result as only a local optimum.
    pub fn local_optimum_only(&self) -> bool {
        self.local_optimum_only
    }

    /// The optimality the axis inherits (rule 4).
    pub fn optimality(&self) -> Optimality {
        if self.global && !self.local_optimum_only {
            Optimality::Certified { cert: Certificate::GlobalFit { by: self.alg.clone() } }
        } else {
            Optimality::LocalOnly
        }
    }
}

fn failed() -> ImplError {
    ImplError::Failed(DerivationReason::FitFailed)
}

fn unsupported(alg: &AlgId) -> ImplError {
    ImplError::Unsupported { alg: alg.clone() }
}

fn fit_failed(_: FitError) -> ImplError {
    failed()
}

/// The chain's single association step before the reduction (rule 2).
fn association<'c>(call: &DeriveCall<'c>) -> Result<&'c Operator, ImplError> {
    let red = call.reduction;
    let steps = call.chain.steps();
    let Some(idx) = steps.iter().position(|s| std::ptr::eq(s, red)) else {
        return Err(unsupported(red.alg()));
    };
    let mut assoc = steps[..idx].iter().filter(|s| s.kind() == OperatorKind::Association);
    let (Some(a), None) = (assoc.next(), assoc.next()) else {
        return Err(unsupported(red.alg()));
    };
    if !red.params().is_empty() {
        return Err(unsupported(red.alg()));
    }
    if !a.params().is_empty() {
        return Err(unsupported(a.alg()));
    }
    Ok(a)
}

fn cylinder_axis(feature: &FittedFeature) -> Result<([f64; 3], [f64; 3]), ImplError> {
    match feature {
        FittedFeature::Cylinder { p, d, .. } => Ok((*p, *d)),
        _ => Err(failed()),
    }
}

/// Runs exactly the fit the chain's association names on the request's points (rules 1 to 4).
///
/// # Errors
///
/// `Unsupported{alg}` (rules 1 to 3) and `Failed(FitFailed)` (rule 3).
pub fn associate(call: &DeriveCall<'_>) -> Result<AxialFit, ImplError> {
    let kind = call.request.feature_kind;
    if kind == FeatureKind::Width {
        return Err(unsupported(call.reduction.alg()));
    }
    let assoc = association(call)?;
    let alg = assoc.alg();
    let Some(method) = AxialMethod::ALL.into_iter().find(|m| m.id() == alg.as_str()) else {
        return Err(unsupported(alg));
    };
    if method.kind() != kind {
        return Err(unsupported(alg));
    }
    let pts = call.request.points;
    let mut xyz: Vec<f64> = Vec::new();
    xyz.try_reserve_exact(pts.len().saturating_mul(3)).map_err(|_| failed())?;
    for p in pts {
        xyz.extend_from_slice(p);
    }
    let view = PointsView::new(&xyz, None).map_err(fit_failed)?;
    let seed = Some(FIT_SEED);
    match method {
        AxialMethod::CylinderLs => {
            let f = fit_cylinder(&view, &FitParams::new(Method::LeastSquares), seed).map_err(fit_failed)?;
            if f.suspect_axis {
                return Err(failed());
            }
            let (p, d) = cylinder_axis(&f.result.feature)?;
            Ok(AxialFit::uncertified(alg.clone(), p, d, f.local_optimum_only))
        }
        AxialMethod::ConeLs => {
            let params = FitParams::new(Method::LeastSquares);
            let f = fit_cone(&view, &params, &ConeOptions::default(), seed).map_err(fit_failed)?;
            if f.suspect_axis || f.weak_axis {
                return Err(failed());
            }
            Ok(AxialFit::uncertified(alg.clone(), f.point, f.axis, f.local_optimum_only))
        }
        AxialMethod::CylinderMc => {
            let params = FitParams::new(Method::MinimumCircumscribed);
            let m = fit_mcc_cylinder(&view, &params, seed).map_err(fit_failed)?;
            if m.suspect_axis {
                return Err(failed());
            }
            let (p, d) = cylinder_axis(&m.feature)?;
            let local = m.local_optimum_only || m.certificate.flags().local_optimum_only;
            let cross_checked = match (m.optimality, m.cross_check) {
                (MccOptimality::StrictLocal, Some(c)) => m.cross_check_passed && c.outcome == CrossOutcome::Passed,
                _ => false,
            };
            Ok(AxialFit { alg: alg.clone(), point: p, direction: d, local_optimum_only: local, global: cross_checked })
        }
    }
}

fn dot(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

/// The axis element from a fit outcome (rules 5 to 7): oriented to `nominal`'s hemisphere,
/// bounded by the projections of `points`, with the fit's optimality.
///
/// # Errors
///
/// `Failed(TooFewPoints)` for no points; `Failed(FitFailed)` when the fit's values cannot make an
/// axis (a non-finite or non-unit direction).
pub fn axis_from_fit(points: &[[f64; 3]], nominal: Option<[f64; 3]>, fit: &AxialFit) -> Result<Produced, ImplError> {
    let c = fit.point;
    let mut d = fit.direction;
    if let Some(n) = nominal {
        if dot(&d, &n) < 0.0 {
            d = [-d[0], -d[1], -d[2]];
        }
    }
    let mut it = points.iter().map(|p| {
        let w = [p[0] - c[0], p[1] - c[1], p[2] - c[2]];
        dot(&w, &d)
    });
    let Some(first) = it.next() else {
        return Err(ImplError::Failed(DerivationReason::TooFewPoints));
    };
    let (mut tmin, mut tmax) = (first, first);
    for t in it {
        if t.total_cmp(&tmin).is_lt() {
            tmin = t;
        }
        if t.total_cmp(&tmax).is_gt() {
            tmax = t;
        }
    }
    let m = (tmin + tmax) / 2.0;
    let point = [c[0] + m * d[0], c[1] + m * d[1], c[2] + m * d[2]];
    let axis = Axis::new(point, d, tmax - tmin).map_err(|_| failed())?;
    Ok(Produced::new(DerivedElement::Axis(axis), fit.optimality()))
}

/// The registered derivation: [`associate`], then [`axis_from_fit`] with the request's points
/// and nominal direction.
///
/// # Errors
///
/// As [`associate`] and [`axis_from_fit`].
pub fn derive_axis(call: &DeriveCall<'_>) -> Result<Produced, ImplError> {
    derive_axis_with_fit(call).map(|(_, p)| p)
}

/// [`derive_axis`], also returning the fit outcome (with the fit's algorithm ID, REQ-001), which
/// the `fid.derived-element/1` record of `derive()` does not carry.
///
/// # Errors
///
/// As [`derive_axis`].
pub fn derive_axis_with_fit(call: &DeriveCall<'_>) -> Result<(AxialFit, Produced), ImplError> {
    let fit = associate(call)?;
    let produced = axis_from_fit(call.request.points, call.request.nominal_direction, &fit)?;
    Ok((fit, produced))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn supported_fit_ids_are_the_fit_crates() {
        assert_eq!(supported_fits(), vec!["fit.cone.ls@1.0.0", "fit.cylinder.ls@1.0.0", "fit.cylinder.mc@1.0.0"]);
        assert!(AlgId::parse(AXIS_REDUCTION_ALG).is_ok());
    }

    #[test]
    fn uncertified_is_always_local_only() {
        let a = AlgId::parse("fit.cylinder.ls@1.0.0").unwrap();
        for flag in [false, true] {
            let f = AxialFit::uncertified(a.clone(), [0.0; 3], [0.0, 0.0, 1.0], flag);
            assert_eq!(f.optimality(), Optimality::LocalOnly);
        }
        let mut g = AxialFit::uncertified(a.clone(), [0.0; 3], [0.0, 0.0, 1.0], false);
        g.global = true;
        assert_eq!(g.optimality(), Optimality::Certified { cert: Certificate::GlobalFit { by: a.clone() } });
        g.local_optimum_only = true;
        assert_eq!(g.optimality(), Optimality::LocalOnly);
    }
}
