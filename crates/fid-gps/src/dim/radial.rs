//! Diameter and radius of a fitted feature (spec node `GDT.DIM.RADIAL`), pure.
//!
//! [`evaluate`] takes the measured points of one feature, the fit algorithm the feature is
//! defined with, the association steps of the resolved operator chain and the measure
//! (diameter or radius), and returns the value in metres with the fit's algorithm ID and the
//! digest of the fit's canonical encoding. The command `check.eval_diameter@1` /
//! `check.eval_radius@1` in `fid-gdt` wraps it behind the governing-spec gate and builds the
//! ResultRecord.
//!
//! # Pinned procedure
//!
//! 1. **Feature fit.** The feature's fit must be a `fid-fit` algorithm ID
//!    `fit.<primitive>.<method>@<x.y.z>` whose primitive is `circle2`, `circle3`, `cylinder`,
//!    `sphere` or `cone` (`not_radial_feature{alg}` otherwise).
//! 2. **Gauge.** A cone needs a gauge position (`gauge_position_missing`); any other feature
//!    refuses one (`gauge_not_applicable`). A gauge distance must be finite and positive
//!    (`bad_gauge{distance}`).
//! 3. **Chain.** The chain must have exactly one association step (`fit_method_mismatch{none}`
//!    for none, `ambiguous_association` for more), and its algorithm ID must equal the
//!    feature's fit exactly (`fit_method_mismatch{feature:chain}`, REQ-GDT.DIM.RADIAL-003). The
//!    feature is never re-fitted by another method to make them agree.
//! 4. **Available fits.** Only the least-squares fits of `fid-fit` are run:
//!    `fit.circle2.ls@1.0.0`, `fit.circle3.ls@1.0.0`, `fit.sphere.ls@1.0.0`
//!    (`KRN.LSQ.RND`, with `ls_rnd::default_params`) and `fit.cylinder.ls@1.0.0`
//!    (`KRN.LSQ.AXIS.CYL`, `FitParams::new(LeastSquares)`, seed 0). Any other fit, including
//!    the cone (`KRN.LSQ.AXIS.CONE` is not on main) and every envelope fit, is
//!    `fit_unavailable{alg}`; no value is ever made up.
//! 5. **Fit, fail closed.** Every fit runs through the `fid_fit::contract::Fit` trait, which
//!    refuses a cylinder flagged `local_optimum_only` or `suspect_axis`
//!    (`FitError::FlaggedFit`). Every `fid-fit` refusal (degenerate data, insufficient
//!    coverage, ill-conditioned, not converged, implausible, flagged) is passed through as
//!    `fit_refused{code}` with the flags it carries. A result whose condition estimate is not
//!    finite or above `fid_fit::contract::DEFAULT_COND_MAX` is refused here too
//!    (`fit_refused{ill_conditioned}`), whatever the fit's own limit was.
//! 6. **Value.** `r` is the fitted radius (finite and positive, else
//!    `fit_refused{bad_radius}`); the radius is `r` and the diameter `2 r` (exact in binary64),
//!    in metres.
//!
//! # Determinism
//!
//! The only arithmetic here is the doubling; the fits are `fid-fit`'s (D0, KNP-1). Nothing
//! panics, and every input is bounded by `fid-fit`'s own limits.

use crate::dmt::AlgId;
use fid_fit::contract::{
    algorithm_id, encode, Fit, FitError, FitParams, FitResult, FittedFeature, Method, PointsView, Primitive,
    DEFAULT_COND_MAX,
};
use fid_fit::{ls_cyl, ls_rnd};
use fid_types::Digest;
use std::fmt;

/// What is reported: the diameter `2 r` or the radius `r`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Measure {
    /// `diameter`.
    Diameter,
    /// `radius`.
    Radius,
}

impl Measure {
    /// `diameter` or `radius`: also the DMT characteristic type the chain is resolved for.
    pub const fn as_str(self) -> &'static str {
        match self {
            Measure::Diameter => "diameter",
            Measure::Radius => "radius",
        }
    }
}

/// Where a cone's diameter is taken. Only a distance from the apex along the axis is built;
/// the leaf's `from_plane(PlaneRef, m)` form waits for the cone fit (open AC-2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gauge {
    /// A distance from the apex along the axis, in metres.
    FromApex(f64),
}

/// Errors of [`evaluate`]. [`RadialError::code`] is the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RadialError {
    /// `not_radial_feature{alg}`: the feature's fit is not a `fid-fit` circle, cylinder,
    /// sphere or cone fit.
    NotRadialFeature {
        /// The feature's fit algorithm ID.
        alg: String,
    },
    /// `gauge_position_missing`: a cone without a gauge position (REQ-002).
    GaugePositionMissing,
    /// `gauge_not_applicable`: a gauge position for a feature that is not a cone.
    GaugeNotApplicable,
    /// `bad_gauge{distance}`: a gauge distance that is not finite and positive.
    BadGauge,
    /// `fit_method_mismatch{feature:chain}`: the feature's fit differs from the chain's
    /// association (REQ-003); `chain` is `none` when the chain has no association step.
    FitMethodMismatch {
        /// The feature's fit.
        feature: String,
        /// The chain's association, or `none`.
        chain: String,
    },
    /// `ambiguous_association{count}`: the chain has more than one association step.
    AmbiguousAssociation {
        /// How many.
        count: usize,
    },
    /// `fit_unavailable{alg}`: the fit is not one `fid-fit` provides on main (step 4).
    FitUnavailable {
        /// The algorithm ID.
        alg: String,
    },
    /// `fit_refused{code}`: `fid-fit` refused the fit, or its result is not a basis for a value
    /// (step 5). `code` is `fid-fit`'s error code, `ill_conditioned` or `bad_radius`.
    FitRefused {
        /// The refusal code.
        code: &'static str,
        /// The fit was flagged `local_optimum_only`.
        local_optimum_only: bool,
        /// The fit was flagged `suspect_axis`.
        suspect_axis: bool,
    },
}

impl RadialError {
    /// The stable code.
    pub const fn code(&self) -> &'static str {
        match self {
            RadialError::NotRadialFeature { .. } => "not_radial_feature",
            RadialError::GaugePositionMissing => "gauge_position_missing",
            RadialError::GaugeNotApplicable => "gauge_not_applicable",
            RadialError::BadGauge => "bad_gauge",
            RadialError::FitMethodMismatch { .. } => "fit_method_mismatch",
            RadialError::AmbiguousAssociation { .. } => "ambiguous_association",
            RadialError::FitUnavailable { .. } => "fit_unavailable",
            RadialError::FitRefused { .. } => "fit_refused",
        }
    }

    fn refused(code: &'static str) -> RadialError {
        RadialError::FitRefused { code, local_optimum_only: false, suspect_axis: false }
    }

    /// The refusal of a `fid-fit` error, keeping the flags of a flagged fit.
    pub fn from_fit(e: &FitError) -> RadialError {
        match e {
            FitError::FlaggedFit { local_optimum_only, suspect_axis, .. } => RadialError::FitRefused {
                code: e.code(),
                local_optimum_only: *local_optimum_only,
                suspect_axis: *suspect_axis,
            },
            other => RadialError::refused(other.code()),
        }
    }
}

impl fmt::Display for RadialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = self.code();
        match self {
            RadialError::NotRadialFeature { alg } | RadialError::FitUnavailable { alg } => {
                write!(f, "{c}{{{}}}", alg.escape_debug())
            }
            RadialError::FitMethodMismatch { feature, chain } => {
                write!(f, "{c}{{{}:{}}}", feature.escape_debug(), chain.escape_debug())
            }
            RadialError::AmbiguousAssociation { count } => write!(f, "{c}{{{count}}}"),
            RadialError::BadGauge => write!(f, "{c}{{distance}}"),
            RadialError::FitRefused { code, local_optimum_only, suspect_axis } => {
                write!(f, "{c}{{{code}}}")?;
                if *local_optimum_only {
                    f.write_str("[local_optimum_only]")?;
                }
                if *suspect_axis {
                    f.write_str("[suspect_axis]")?;
                }
                Ok(())
            }
            RadialError::GaugePositionMissing | RadialError::GaugeNotApplicable => f.write_str(c),
        }
    }
}

impl std::error::Error for RadialError {}

/// The inputs of one evaluation.
#[derive(Debug, Clone, Copy)]
pub struct RadialInput<'a> {
    /// Diameter or radius.
    pub measure: Measure,
    /// The fit the feature is defined with.
    pub feature_fit: &'a AlgId,
    /// The association steps of the resolved chain, in chain order.
    pub association: &'a [&'a AlgId],
    /// The measured points, xyz triples in metres.
    pub xyz: &'a [f64],
    /// The gauge position (cones only).
    pub gauge: Option<Gauge>,
}

/// The result of [`evaluate`].
#[derive(Debug, Clone, PartialEq)]
pub struct RadialValue {
    measure: Measure,
    value: f64,
    radius: f64,
    primitive: Primitive,
    algorithm: String,
    fit_digest: Digest,
    points: usize,
}

impl RadialValue {
    /// Diameter or radius.
    pub fn measure(&self) -> Measure {
        self.measure
    }
    /// The value in metres (`2 r` or `r`).
    pub fn value(&self) -> f64 {
        self.value
    }
    /// The fitted radius in metres.
    pub fn radius(&self) -> f64 {
        self.radius
    }
    /// The fitted primitive.
    pub fn primitive(&self) -> Primitive {
        self.primitive
    }
    /// The algorithm ID of the fit that produced the feature (REQ-003).
    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }
    /// SHA-256 of `fid_fit::contract::encode` of the fit result.
    pub fn fit_digest(&self) -> Digest {
        self.fit_digest
    }
    /// How many points were fitted.
    pub fn points(&self) -> usize {
        self.points
    }
}

/// The primitive and method of a `fid-fit` algorithm ID (`fit.<primitive>.<method>@<v>`), or
/// `None`.
fn fit_parts(alg: &AlgId) -> Option<(Primitive, Method)> {
    let (body, _) = alg.as_str().split_once('@')?;
    let mut it = body.split('.');
    let (module, primitive, method) = (it.next()?, it.next()?, it.next()?);
    if module != "fit" || it.next().is_some() {
        return None;
    }
    Some((Primitive::parse(primitive)?, Method::parse(method)?))
}

/// The radial primitives (step 1).
const RADIAL: [Primitive; 5] =
    [Primitive::Circle2, Primitive::Circle3, Primitive::Cylinder, Primitive::Sphere, Primitive::Cone];

/// The fits run here (step 4): primitive and version.
fn available(primitive: Primitive) -> Option<(u32, u32, u32)> {
    match primitive {
        Primitive::Circle2 | Primitive::Circle3 | Primitive::Sphere => Some(ls_rnd::VERSION),
        Primitive::Cylinder => Some(ls_cyl::VERSION),
        _ => None,
    }
}

// `FitError` is `fid-fit`'s own error (about 136 bytes, see the note in `fid-fit`); it is
// returned at most once per evaluation and mapped at once, so it is not boxed.
#[allow(clippy::result_large_err)]
fn run_fit(primitive: Primitive, pts: &PointsView<'_>) -> Result<FitResult, FitError> {
    match primitive {
        Primitive::Circle2 => ls_rnd::Circle2Ls.fit(pts, &ls_rnd::default_params(), None),
        Primitive::Circle3 => ls_rnd::Circle3Ls.fit(pts, &ls_rnd::default_params(), None),
        Primitive::Sphere => ls_rnd::SphereLs.fit(pts, &ls_rnd::default_params(), None),
        // The trait entry point: a flagged cylinder is `FlaggedFit`, never returned.
        Primitive::Cylinder => ls_cyl::CylinderLs.fit(pts, &FitParams::new(Method::LeastSquares), Some(0)),
        _ => Err(FitError::MethodUndefined { primitive, method: Method::LeastSquares }),
    }
}

/// Checks a fit result before it becomes a value (steps 5 and 6) and returns its radius.
pub fn accept(result: &FitResult) -> Result<f64, RadialError> {
    let cond = result.diagnostics.cond;
    if !cond.is_finite() || cond > DEFAULT_COND_MAX {
        return Err(RadialError::refused("ill_conditioned"));
    }
    let r = match result.feature {
        FittedFeature::Circle2 { r, .. }
        | FittedFeature::Circle3 { r, .. }
        | FittedFeature::Sphere { r, .. }
        | FittedFeature::Cylinder { r, .. } => r,
        _ => return Err(RadialError::refused("bad_radius")),
    };
    if !r.is_finite() || r <= 0.0 || r.is_subnormal() {
        return Err(RadialError::refused("bad_radius"));
    }
    Ok(r)
}

/// Evaluates one diameter or radius by the pinned procedure of the module documentation.
pub fn evaluate(inp: &RadialInput<'_>) -> Result<RadialValue, RadialError> {
    let feature_alg = inp.feature_fit.as_str();
    let (primitive, method) = fit_parts(inp.feature_fit)
        .filter(|(p, _)| RADIAL.contains(p))
        .ok_or_else(|| RadialError::NotRadialFeature { alg: feature_alg.to_owned() })?;
    match (primitive, inp.gauge) {
        (Primitive::Cone, None) => return Err(RadialError::GaugePositionMissing),
        (Primitive::Cone, Some(Gauge::FromApex(d))) if !d.is_finite() || d <= 0.0 || d.is_subnormal() => {
            return Err(RadialError::BadGauge)
        }
        (Primitive::Cone, Some(_)) | (_, None) => {}
        (_, Some(_)) => return Err(RadialError::GaugeNotApplicable),
    }
    match inp.association {
        [] => return Err(RadialError::FitMethodMismatch { feature: feature_alg.to_owned(), chain: "none".to_owned() }),
        [one] if one.as_str() != feature_alg => {
            return Err(RadialError::FitMethodMismatch {
                feature: feature_alg.to_owned(),
                chain: one.as_str().to_owned(),
            })
        }
        [_] => {}
        many => return Err(RadialError::AmbiguousAssociation { count: many.len() }),
    }
    let unavailable = || RadialError::FitUnavailable { alg: feature_alg.to_owned() };
    let version = available(primitive).ok_or_else(unavailable)?;
    if method != Method::LeastSquares {
        return Err(unavailable());
    }
    let id = algorithm_id(primitive, method, version).map_err(|e| RadialError::from_fit(&e))?;
    if id.to_string() != feature_alg {
        return Err(unavailable());
    }
    let pts = PointsView::new(inp.xyz, None).map_err(|e| RadialError::from_fit(&e))?;
    let result = run_fit(primitive, &pts).map_err(|e| RadialError::from_fit(&e))?;
    if result.algorithm.to_string() != feature_alg {
        return Err(unavailable());
    }
    let radius = accept(&result)?;
    let value = match inp.measure {
        Measure::Diameter => 2.0 * radius,
        Measure::Radius => radius,
    };
    let bytes = encode(&result).map_err(|e| RadialError::from_fit(&e))?;
    let fit_digest = Digest::from_bytes(fid_canon::digest_bytes(&bytes).0);
    Ok(RadialValue {
        measure: inp.measure,
        value,
        radius,
        primitive,
        algorithm: feature_alg.to_owned(),
        fit_digest,
        points: pts.len(),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn alg(s: &str) -> AlgId {
        AlgId::parse(s).unwrap()
    }

    /// A flagged cylinder (REQ-004 / AC-5 under the maintainer rule): the refusal keeps the
    /// flags and is never a value.
    #[test]
    fn a_flagged_fit_is_refused_with_its_flags() {
        let e = RadialError::from_fit(&FitError::FlaggedFit {
            local_optimum_only: true,
            suspect_axis: false,
            rms_ratio: 0.1,
        });
        assert_eq!(e.code(), "fit_refused");
        assert_eq!(e.to_string(), "fit_refused{flagged_fit}[local_optimum_only]");
        let e = RadialError::from_fit(&FitError::FlaggedFit {
            local_optimum_only: false,
            suspect_axis: true,
            rms_ratio: 0.5,
        });
        assert_eq!(e.to_string(), "fit_refused{flagged_fit}[suspect_axis]");
        assert_eq!(
            RadialError::from_fit(&FitError::IllConditioned { cond: None }).to_string(),
            "fit_refused{ill_conditioned}"
        );
    }

    #[test]
    fn an_ill_conditioned_or_degenerate_result_is_never_a_value() {
        let pts = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, -1.0, 0.0];
        let view = PointsView::new(&pts, None).unwrap();
        let mut r = ls_rnd::Circle3Ls.fit(&view, &ls_rnd::default_params(), None).unwrap();
        assert!((accept(&r).unwrap() - 1.0).abs() < 1e-12);
        r.diagnostics.cond = f64::INFINITY;
        assert_eq!(accept(&r).unwrap_err().to_string(), "fit_refused{ill_conditioned}");
        r.diagnostics.cond = DEFAULT_COND_MAX * 2.0;
        assert_eq!(accept(&r).unwrap_err().to_string(), "fit_refused{ill_conditioned}");
        r.diagnostics.cond = 1.0;
        r.feature = FittedFeature::Point { c: [0.0; 3] };
        assert_eq!(accept(&r).unwrap_err().to_string(), "fit_refused{bad_radius}");
    }

    #[test]
    fn fit_ids_are_parsed_strictly() {
        assert_eq!(fit_parts(&alg("fit.circle3.ls@1.0.0")), Some((Primitive::Circle3, Method::LeastSquares)));
        assert_eq!(fit_parts(&alg("fit.cylinder.mi@1.0.0")), Some((Primitive::Cylinder, Method::MaximumInscribed)));
        assert_eq!(fit_parts(&alg("syn.assoc.least_squares@0.1.0")), None);
        assert_eq!(fit_parts(&alg("fit.circle.ls@1.0.0")), None);
        assert_eq!(fit_parts(&alg("fit.circle3.lsq@1.0.0")), None);
    }

    #[test]
    fn only_the_least_squares_fits_on_main_are_available() {
        let pts = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, -1.0, 0.0];
        for s in ["fit.circle3.mi@1.0.0", "fit.circle3.ls@2.0.0", "fit.cylinder.mc@1.0.0"] {
            let a = alg(s);
            let inp =
                RadialInput { measure: Measure::Radius, feature_fit: &a, association: &[&a], xyz: &pts, gauge: None };
            assert_eq!(evaluate(&inp).unwrap_err().to_string(), format!("fit_unavailable{{{s}}}"));
        }
    }
}
