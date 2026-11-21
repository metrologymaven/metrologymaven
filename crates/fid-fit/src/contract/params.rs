//! Fit parameters: method, constraints, thresholds, weights and convergence. Every
//! value-affecting choice of a fit is a field here; nothing is read from global state.

use fid_num::red;
use fid_solv::nlls::{NllsConfig, MAX_ITER};

use super::error::FitError;
use super::points::PointsView;
use super::primitive::{Method, V3};

/// Default relative singular-value tolerance of the rank rule (see [`Thresholds::rank_tol`]).
pub const DEFAULT_RANK_TOL: f64 = 1e-9;

/// Default minimum angular coverage of circle-type data: 20 degrees in radians, pinned as the
/// `f64` with bits `0x3FD6_5718_4AE7_4487` (`core::f64::consts::PI / 9.0`).
pub const DEFAULT_MIN_COVERAGE: f64 = 0.3490658503988659;

/// Default largest accepted eigenvalue ratio of a closed-form direction (see
/// [`Thresholds::eig_ratio_max`]): `0.9`, so the direction's sensitivity to a perturbation of
/// the scatter matrix is amplified at most `1 / (1 - 0.9) = 10` times (Davis-Kahan bound).
pub const DEFAULT_EIG_RATIO_MAX: f64 = 0.9;

/// Default smallest accepted ratio of the data's RMS distance from its least-squares plane to
/// the fit's residual RMS (see [`Thresholds::planarity_ratio_min`]): `10` [unverified].
pub const DEFAULT_PLANARITY_RATIO_MIN: f64 = 10.0;

/// Default largest accepted condition estimate of a converged iterative fit (see
/// [`Thresholds::cond_max`]): `1e8` [unverified].
pub const DEFAULT_COND_MAX: f64 = 1e8;

/// `2 * pi` as an `f64` (exactly twice `core::f64::consts::PI`).
pub const TWO_PI: f64 = 2.0 * core::f64::consts::PI;

/// A fixed constraint that keeps a fit well-posed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Constraint {
    /// The radius (metres) is fixed: finite, positive and normal.
    FixedRadius(f64),
    /// The axis or normal direction is fixed: finite, norm at least `2^-1022`.
    FixedAxisDirection(V3),
    /// The data is projected onto the plane through `p` with normal `n` before the fit.
    ProjectionPlane { p: V3, n: V3 },
}

impl Constraint {
    fn kind(&self) -> usize {
        match self {
            Constraint::FixedRadius(_) => 0,
            Constraint::FixedAxisDirection(_) => 1,
            Constraint::ProjectionPlane { .. } => 2,
        }
    }
}

/// Thresholds of the degeneracy checks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    /// Rank rule: with the singular values `s0 >= s1 >= s2` of the centred, scaled data, the
    /// numerical rank is the count of `s_i > rank_tol * s0` (`fid_num::la::rank_by_rule`).
    /// Finite, in `[0, 1)`.
    pub rank_tol: f64,
    /// Smallest accepted angular coverage of circle-type data, radians, in `[0, 2 pi]`.
    pub min_coverage: f64,
    /// Largest accepted ratio `lb / la` of the competing eigenvalue `lb` to the eigenvalue
    /// `la` that defines a closed-form direction (least-squares lines and planes, spec node
    /// `KRN.LSQ.LIN`); a larger ratio means no unique direction (`IllConditioned`). Finite, in
    /// `[0, 1)`.
    pub eig_ratio_max: f64,
    /// Smallest accepted ratio `plane_rms / rms` of a surface fit (least-squares sphere, spec
    /// node `KRN.LSQ.RND`): `plane_rms` is the RMS distance of the points from their
    /// least-squares plane, `rms` the fit's residual RMS. A ratio that is not above this value
    /// means the data is no further from a plane than from the fitted surface, which it does
    /// not then determine (`Coplanar`). Finite, at least 1.
    pub planarity_ratio_min: f64,
    /// Largest accepted condition estimate of the Jacobian of a converged iterative fit
    /// (least-squares circles and sphere, spec node `KRN.LSQ.RND`), checked on the result,
    /// unlike `nlls.cond_limit`, which the solver checks at every iterate; a larger estimate is
    /// `IllConditioned` (a weakly determined fit). Finite, at least 1.
    pub cond_max: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            rank_tol: DEFAULT_RANK_TOL,
            min_coverage: DEFAULT_MIN_COVERAGE,
            eig_ratio_max: DEFAULT_EIG_RATIO_MAX,
            planarity_ratio_min: DEFAULT_PLANARITY_RATIO_MIN,
            cond_max: DEFAULT_COND_MAX,
        }
    }
}

/// Per-point weights of the objective.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Weights {
    /// Every point has weight 1.
    #[default]
    Uniform,
    /// One weight per point: finite, positive and normal.
    PerPoint(Vec<f64>),
}

/// The typed parameters of a fit (REQ-KRN.LSQ.TRAIT-001).
#[derive(Debug, Clone, PartialEq)]
pub struct FitParams {
    pub method: Method,
    /// At most one constraint of each kind.
    pub constraints: Vec<Constraint>,
    pub thresholds: Thresholds,
    pub weights: Weights,
    /// Convergence configuration of iterative methods (`fid_solv::nlls`), echoed in results.
    pub nlls: NllsConfig,
}

impl FitParams {
    /// Parameters with no constraint, the default thresholds, uniform weights and the
    /// `fid_solv` default NLLS configuration.
    pub fn new(method: Method) -> Self {
        FitParams {
            method,
            constraints: Vec::new(),
            thresholds: Thresholds::default(),
            weights: Weights::Uniform,
            nlls: NllsConfig::default(),
        }
    }

    /// The fixed radius, if constrained.
    pub fn fixed_radius(&self) -> Option<f64> {
        self.constraints.iter().find_map(|c| match c {
            Constraint::FixedRadius(r) => Some(*r),
            _ => None,
        })
    }

    /// The fixed axis direction, if constrained.
    pub fn fixed_axis_direction(&self) -> Option<V3> {
        self.constraints.iter().find_map(|c| match c {
            Constraint::FixedAxisDirection(d) => Some(*d),
            _ => None,
        })
    }

    /// The projection plane `(p, n)`, if constrained.
    pub fn projection_plane(&self) -> Option<(V3, V3)> {
        self.constraints.iter().find_map(|c| match c {
            Constraint::ProjectionPlane { p, n } => Some((*p, *n)),
            _ => None,
        })
    }

    /// Checks every field against `pts`, in this order: thresholds, constraints, weights,
    /// NLLS configuration. The first violation is [`FitError::InvalidParams`] naming the
    /// field.
    pub fn validate(&self, pts: &PointsView<'_>) -> Result<(), FitError> {
        let bad = |field| Err(FitError::InvalidParams { field });
        let t = &self.thresholds;
        if !((0.0..1.0).contains(&t.rank_tol)) {
            return bad("thresholds.rank_tol");
        }
        if !((0.0..=TWO_PI).contains(&t.min_coverage)) {
            return bad("thresholds.min_coverage");
        }
        if !((0.0..1.0).contains(&t.eig_ratio_max)) {
            return bad("thresholds.eig_ratio_max");
        }
        if !(t.planarity_ratio_min.is_finite() && t.planarity_ratio_min >= 1.0) {
            return bad("thresholds.planarity_ratio_min");
        }
        if !(t.cond_max.is_finite() && t.cond_max >= 1.0) {
            return bad("thresholds.cond_max");
        }
        let mut seen = [false; 3];
        for c in &self.constraints {
            let k = c.kind();
            if seen[k] {
                return bad("constraints");
            }
            seen[k] = true;
            match c {
                Constraint::FixedRadius(r) => {
                    if !(r.is_finite() && r.is_normal() && *r > 0.0) {
                        return bad("constraints.fixed_radius");
                    }
                }
                Constraint::FixedAxisDirection(d) => {
                    if !direction_ok(d) {
                        return bad("constraints.fixed_axis_direction");
                    }
                }
                Constraint::ProjectionPlane { p, n } => {
                    if !p.iter().all(|x| x.is_finite()) {
                        return bad("constraints.projection_plane.p");
                    }
                    if !direction_ok(n) {
                        return bad("constraints.projection_plane.n");
                    }
                }
            }
        }
        if let Weights::PerPoint(w) = &self.weights {
            if w.len() != pts.len() || !w.iter().all(|x| x.is_finite() && x.is_normal() && *x > 0.0) {
                return bad("weights");
            }
        }
        let c = &self.nlls;
        if c.max_iter > MAX_ITER {
            return bad("nlls.max_iter");
        }
        let nonneg = |x: f64| x.is_finite() && x >= 0.0;
        if !nonneg(c.tol_grad) {
            return bad("nlls.tol_grad");
        }
        if !nonneg(c.tol_step) {
            return bad("nlls.tol_step");
        }
        if !nonneg(c.tol_cost) {
            return bad("nlls.tol_cost");
        }
        if !(c.cond_limit.is_finite() && c.cond_limit >= 1.0) {
            return bad("nlls.cond_limit");
        }
        if !(nonneg(c.rank_tol) && c.rank_tol < 1.0) {
            return bad("nlls.rank_tol");
        }
        if !(c.lm_tau.is_finite() && c.lm_tau > 0.0) {
            return bad("nlls.lm_tau");
        }
        Ok(())
    }
}

fn direction_ok(d: &V3) -> bool {
    d.iter().all(|x| x.is_finite()) && red::norm2(d).is_ok_and(|n| n >= f64::MIN_POSITIVE)
}
