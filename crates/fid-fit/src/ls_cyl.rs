//! Least-squares cylinder. Spec node `KRN.LSQ.AXIS.CYL`.
//!
//! [`CylinderLs`] implements [`Fit`] for `fit.cylinder.ls@1.0.0`, the orthogonal-distance
//! least-squares cylinder: it minimises `sum_i r_i^2` with `r_i = dist(p_i, axis) - R`.
//! [`fit_cylinder`] returns the same result with the multi-start record ([`CylinderFit`]), and
//! [`fit_cylinder_from`] runs it from a caller-supplied [`AxialStart`] (the `StartSpec`
//! override of the command).
//!
//! # Pinned procedure
//!
//! 1. **Parameters.** [`FitParams::validate`]; the method must be least squares, the weights
//!    uniform ([`FitError::InvalidParams`] `method` / `weights`); the accepted constraints are a
//!    fixed radius and a fixed axis direction (any other is `InvalidParams` `constraints`).
//! 2. **Checks and data.** [`check_points`] for the cylinder under `params.thresholds` (so 4
//!    points are `TooFewPoints`, coplanar data `Coplanar` with its condition number); the fit
//!    runs on the centred, scaled data `q_i` of [`Centred`].
//! 3. **Starts.** Without a fixed direction, [`init::cylinder_start`] with the caller's seed
//!    (or [`DEFAULT_SEED`] when `seed` is `None`): its frame `(P0, A, E1, E2)` (the axis anchored
//!    at the data's mid-height) and its `StartSpec` `[a, b, u, v, r]`, run by
//!    `fid_solv::mstart::run`. With a fixed radius the `r` entry is dropped from the spec
//!    (primary, half widths, tolerances), which changes the random starts (they depend on the
//!    parameter count). With a fixed direction `F` the problem is a circle fit of the
//!    projections: frame `A = normalize_direction(F)`, `P0` the centroid, `(E1, E2)` from the
//!    basis rule of `KRN.LSQ.INIT` step 3, one start from INIT's algebraic circle of `A`
//!    (`[u_c, v_c, r_a]`, or `r` dropped with a fixed radius), run by `mstart::run_listed`.
//! 4. **Model.** For parameters `(a, b, u, v, r)` relative to a frame: `D = A + a E1 + b E2`,
//!    `n = red::norm2(D)`, `d = D / n`, `P = P0 + u E1 + v E2` (component formulas, no FMA);
//!    per point `w = q_i - P`, `t = red::dot(w, d)`, `perp = w - t d`, `rho = red::norm2(perp)`,
//!    `r_i = rho - r`. Analytic Jacobian with `g1 = red::dot(perp, E1) / rho`, `g2` likewise
//!    (both 0 when `rho == 0`): `[-t g1 / n, -t g2 / n, -g1, -g2, -1]`, the columns of fixed
//!    parameters left out.
//! 5. **Solve and re-parameterisation.** `fid_solv::nlls::solve` under `params.nlls` from the
//!    start. The converged axis then becomes the new frame: `A' = d`, `P0'` the foot of the
//!    perpendicular from the centroid (the origin) onto the axis, `(E1', E2')` by the basis rule,
//!    and the solve is repeated from `a = b = u = v = 0` (and the current `r`). This stops after
//!    a round with no accepted step, or after [`REPARAM_ROUNDS`] rounds. A later round that
//!    fails (any solver error) ends the rounds and keeps the last converged round, so a start
//!    that converged is never discarded by its re-anchoring. So the reported
//!    gradient, rank and condition are those of the tangent-plane parameters about the final
//!    axis, and the iteration count is the sum over the rounds. (The NLLS driver has no
//!    per-step hook, so the axis is re-parameterised after each converged solve, not after each
//!    accepted step; see the leaf's Open questions.)
//! 6. **Comparable parameters.** Each start's outcome is reported to `mstart` as its
//!    parameters in the start frame, in SI units: the final direction (sign chosen so that
//!    `d . A > 0`) as `a = (d . E1) / (d . A)`, `b = (d . E2) / (d . A)`, the axis point where the
//!    final axis meets the plane through `P0` normal to `A` as `u`, `v` (metres), and `r`
//!    (metres). The objective is the final round's `red::sum_sq` of the scaled residuals.
//!    Selection, agreement and `local_optimum_only` are `mstart`'s rules with INIT's
//!    tolerances (`1e-6` angles, `1e-6` times the RMS radius for lengths).
//! 7. **Result.** The selected cylinder: `p` the foot of the perpendicular from the centroid,
//!    `d`, `r`, mapped back to metres and canonicalised by [`FitResult::assemble`]; residuals
//!    in metres.
//! 8. **Wrong-axis check.** Multi-start agreement is not proof of correctness: every start
//!    can converge to the same wrong axis (the review of #358). Two references, with `rms` the
//!    fit's residual RMS:
//!    - **Certificate.** `bound_rms` is the RMS of the best plane the constrained cylinder
//!      family tends to as its radius grows: without constraints the least-squares plane of
//!      the data (all planes are limits), with a fixed axis direction `d` the best plane that
//!      contains `d` (the least-squares line of the projections onto `(E1, E2)`: square root of
//!      the smaller eigenvalue of their `2 x 2` scatter over `n`). The infimum of the
//!      least-squares objective over the family is then at most the bound's, so a converged fit
//!      with `rms >= (1 + 1e-9) bound_rms` is not the global optimum, or none exists (the
//!      infimum is approached only as the radius grows without limit): [`FitError::ImplausibleFit`].
//!      The `1e-9` keeps a fit that equals the bound to rounding from being refused. A fixed
//!      radius has no plane limit, so it has no certificate (`bound_rms` is `None`).
//!    - **Flag.** `plane_rms` is the RMS distance of the points to their least-squares plane
//!      (normal: the smallest principal axis of [`data_shape`]), the data's spread out of its
//!      best plane, and `ratio = rms / plane_rms`. `ratio > SUSPECT_RATIO` (0.3) sets
//!      [`CylinderFit::suspect_axis`]: the axis is weakly determined or wrong (the cylinder
//!      explains less than `1 - 0.3^2 = 91 %` of the out-of-plane variance). The review of
//!      #362 searched the minima of the fixture and found no wrong minimum below a ratio of
//!      0.44 (a converged wrong axis on a half cylinder is about 0.47), and correct fits of
//!      well-determined data stay below 0.01; 0.3 leaves a margin below the 0.44 floor. On short
//!      noisy arcs whose sagitta is comparable to the noise a correct axis can also exceed 0.3:
//!      the flag then means weakly determined, not wrong. With a fixed radius the ratio mostly
//!      measures the mismatch between the fixed and the data's radius, not the axis. The value
//!      0.3 is [unverified] against ORS cases.
//! 9. **Trait (fail closed).** [`Fit::fit`] returns only clean fits: a fit with
//!    `local_optimum_only` or `suspect_axis` is [`FitError::FlaggedFit`], so a generic caller
//!    can never receive a flagged fit unflagged. Callers that accept flagged fits use
//!    [`fit_cylinder`], which returns them with their flags.
//!
//! # Determinism
//!
//! Every sum, dot product and norm is a `fid_num::red` reduction, square roots go through
//! `fid_num::f`, comparisons use total order, there is no FMA and no hash container, and every
//! loop is bounded. The result is bit-identical on x86-64, ARM64 and wasm32 for a given seed
//! (D0). Nothing panics.

use core::cmp::Ordering;

use fid_kci::AlgorithmId;
use fid_num::{f, red, NumError};
use fid_solv::mstart::{self, MultiStartError, StartError, StartOrigin, StartSpec, StartSummary};
use fid_solv::nlls::{self, NllsConfig, NllsProblem, NllsResult};
use fid_types::Determinism;

use crate::contract::{
    algorithm_id, check_points, command_row, data_shape, normalize_direction, unit_direction, Centred, CommandRow,
    Constraint, Field, Fit, FitDiagnostics, FitError, FitParams, FitResult, FittedFeature, Method, PointsView,
    Primitive, ValueType, Weights, V3,
};
use crate::init::{self, AxialStart, AxisFrame};

/// Algorithm version of `fit.cylinder.ls`.
pub const VERSION: (u32, u32, u32) = (1, 0, 0);

/// The seed used when [`Fit::fit`] is called with `seed: None`.
pub const DEFAULT_SEED: u64 = 0;

/// Most solve rounds per start (rule 5).
pub const REPARAM_ROUNDS: usize = 8;

/// `rms / plane_rms` above which [`CylinderFit::suspect_axis`] is set (rule 8) [unverified].
pub const SUSPECT_RATIO: f64 = 0.3;

/// Relative margin of the certificate: refuse when `rms >= (1 + CERT_MARGIN) bound_rms`.
pub const CERT_MARGIN: f64 = 1e-9;

const OP: &str = "fit.ls_cyl";

/// The least-squares cylinder `fit.cylinder.ls@1.0.0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CylinderLs;

/// A cylinder fit with its multi-start record (REQ-KRN.LSQ.AXIS.CYL-002, -005).
#[derive(Debug, Clone, PartialEq)]
pub struct CylinderFit {
    pub result: FitResult,
    /// The frame the start and outcome parameters refer to (SI metres): INIT's frame, or the
    /// fixed-direction frame.
    pub frame: AxisFrame,
    /// Parameter names of `starts` and of each converged outcome, in order (a subset of
    /// `a, b, u, v, r`).
    pub param_names: Vec<&'static str>,
    /// The starts, in index order (SI: tangents and metres).
    pub starts: Vec<Vec<f64>>,
    /// One outcome per start, in index order: objective and parameters, or the start's error.
    pub per_start: Vec<StartSummary<FitError>>,
    /// The selected start.
    pub best_index: u32,
    /// The converged starts disagree beyond the tolerances (REQ-002).
    pub local_optimum_only: bool,
    /// Seed and stream of the starts, or `Listed` (fixed direction).
    pub origin: StartOrigin,
    /// RMS distance of the points to their least-squares plane (metres, rule 8).
    pub plane_rms: f64,
    /// The certificate's bound (metres, rule 8): `plane_rms` without constraints, the best
    /// plane containing the fixed direction, `None` with a fixed radius.
    pub bound_rms: Option<f64>,
    /// `result.stats.rms / plane_rms` (`+inf` when `plane_rms` is 0).
    pub rms_ratio: f64,
    /// `rms_ratio > SUSPECT_RATIO`: the axis is weakly determined or wrong (rule 8).
    pub suspect_axis: bool,
}

fn alloc<T>(n: usize) -> Result<Vec<T>, FitError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "allocation_failed" }))?;
    Ok(v)
}

fn id() -> AlgorithmId {
    // Least squares is defined for the cylinder, so this cannot fail; the fallback keeps the
    // function total without a panic path.
    algorithm_id(Primitive::Cylinder, Method::LeastSquares, VERSION).unwrap_or_else(|_| AlgorithmId {
        module: "fit".to_string(),
        feature: Primitive::Cylinder.slug().to_string(),
        method: Method::LeastSquares.slug().to_string(),
        version: VERSION,
    })
}

/// The command data row of `fit.ls_cylinder@1` (W2, dry run R1, D0 per seed): the contract's
/// common row plus the multi-start summary fields (optional, written by the command layer).
pub fn cylinder_command_row() -> Result<CommandRow, FitError> {
    let mut row = command_row(Primitive::Cylinder, Method::LeastSquares, VERSION, Determinism::D0)?;
    const C: Option<(&str, &str)> = Some(("count", "1"));
    const D: Option<(&str, &str)> = Some(("dimensionless", "1"));
    const L: Option<(&str, &str)> = Some(("length", "m"));
    let extra = [
        Field { name: "multistart.best_index", ty: ValueType::Integer, quantity: C, required: false },
        Field { name: "multistart.start_count", ty: ValueType::Integer, quantity: C, required: false },
        Field { name: "multistart.local_optimum_only", ty: ValueType::Boolean, quantity: None, required: false },
        Field { name: "check.plane_rms", ty: ValueType::Number, quantity: L, required: false },
        Field { name: "check.bound_rms", ty: ValueType::Number, quantity: L, required: false },
        Field { name: "check.rms_ratio", ty: ValueType::Number, quantity: D, required: false },
        Field { name: "check.suspect_axis", ty: ValueType::Boolean, quantity: None, required: false },
    ];
    row.result
        .try_reserve_exact(extra.len())
        .map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "allocation_failed" }))?;
    row.result.extend_from_slice(&extra);
    Ok(row)
}

/// Which parameters are free.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Layout {
    /// `a`, `b` free (no fixed direction).
    dir: bool,
    /// The fixed radius in scaled units, or `None` when `r` is free.
    radius: Option<f64>,
}

impl Layout {
    fn n(&self) -> usize {
        (if self.dir { 4 } else { 2 }) + usize::from(self.radius.is_none())
    }

    fn names(&self) -> Result<Vec<&'static str>, FitError> {
        let mut v = alloc(5)?;
        if self.dir {
            v.extend_from_slice(&["a", "b"]);
        }
        v.extend_from_slice(&["u", "v"]);
        if self.radius.is_none() {
            v.push("r");
        }
        Ok(v)
    }

    /// `(a, b, u, v, r)` from the free parameters `x`.
    fn full(&self, x: &[f64]) -> Option<[f64; 5]> {
        let mut it = x.iter().copied();
        let (a, b) = if self.dir { (it.next()?, it.next()?) } else { (0.0, 0.0) };
        let (u, v) = (it.next()?, it.next()?);
        let r = match self.radius {
            Some(r) => r,
            None => it.next()?,
        };
        Some([a, b, u, v, r])
    }

    /// The free parameters of `(a, b, u, v, r)`.
    fn free(&self, p: [f64; 5]) -> Result<Vec<f64>, FitError> {
        let mut v = alloc(5)?;
        if self.dir {
            v.extend_from_slice(&p[..2]);
        }
        v.extend_from_slice(&p[2..4]);
        if self.radius.is_none() {
            v.push(p[4]);
        }
        Ok(v)
    }
}

/// A frame in scaled coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Frame {
    p0: V3,
    a: V3,
    e1: V3,
    e2: V3,
}

/// The axis `(P, d)` and radius of the full parameters relative to `fr` (rule 4).
fn axis_of(fr: &Frame, p: &[f64; 5]) -> Result<(V3, V3, f64, f64), NumError> {
    let mut dd = [0.0; 3];
    let mut pp = [0.0; 3];
    for k in 0..3 {
        dd[k] = (fr.a[k] + p[0] * fr.e1[k]) + p[1] * fr.e2[k];
        pp[k] = (fr.p0[k] + p[2] * fr.e1[k]) + p[3] * fr.e2[k];
    }
    let n = red::norm2(&dd)?;
    if n.is_nan() || n <= 0.0 {
        return Err(NumError::Domain { op: OP, detail: "zero_direction" });
    }
    Ok((pp, [dd[0] / n, dd[1] / n, dd[2] / n], n, p[4]))
}

struct Problem<'a> {
    q: &'a [f64],
    fr: Frame,
    layout: Layout,
}

impl NllsProblem for Problem<'_> {
    fn n_params(&self) -> usize {
        self.layout.n()
    }

    fn n_resid(&self) -> usize {
        self.q.len() / 3
    }

    fn eval(&self, x: &[f64], r: &mut [f64], j: &mut [f64]) -> Result<(), NumError> {
        let full = self.layout.full(x).ok_or(NumError::Domain { op: OP, detail: "param_count" })?;
        let (pp, d, n, rad) = axis_of(&self.fr, &full)?;
        let np = self.layout.n();
        for (i, qi) in self.q.chunks_exact(3).enumerate() {
            let w = [qi[0] - pp[0], qi[1] - pp[1], qi[2] - pp[2]];
            let t = red::dot(&w, &d)?;
            let perp = [w[0] - t * d[0], w[1] - t * d[1], w[2] - t * d[2]];
            let rho = red::norm2(&perp)?;
            let (g1, g2) = if rho > 0.0 {
                (red::dot(&perp, &self.fr.e1)? / rho, red::dot(&perp, &self.fr.e2)? / rho)
            } else {
                (0.0, 0.0)
            };
            let ri = r.get_mut(i).ok_or(NumError::Domain { op: OP, detail: "resid_len" })?;
            *ri = rho - rad;
            let row = j.get_mut(i * np..(i + 1) * np).ok_or(NumError::Domain { op: OP, detail: "jac_len" })?;
            let mut k = 0;
            if self.layout.dir {
                row[0] = -(t * g1) / n;
                row[1] = -(t * g2) / n;
                k = 2;
            }
            row[k] = -g1;
            row[k + 1] = -g2;
            if self.layout.radius.is_none() {
                row[k + 2] = -1.0;
            }
        }
        Ok(())
    }
}

/// One start's converged outcome (scaled units).
#[derive(Debug, Clone, PartialEq)]
struct Outcome {
    p: V3,
    d: V3,
    r: f64,
    last: NllsResult,
    iterations: usize,
}

/// Rule 5: solve, re-anchor, repeat.
fn solve_rounds(q: &[f64], fr0: Frame, layout: Layout, x0: Vec<f64>, cfg: &NllsConfig) -> Result<Outcome, FitError> {
    let mut fr = fr0;
    let mut x = x0;
    let mut iterations = 0usize;
    let mut done: Option<Outcome> = None;
    for _ in 0..REPARAM_ROUNDS {
        let prob = Problem { q, fr, layout };
        let res = match nlls::solve(&prob, &x, cfg) {
            Ok(res) => res,
            // A later round's failure keeps the converged earlier round (rule 5).
            Err(_) if done.is_some() => break,
            Err(e) => return Err(FitError::from_solver(e, cfg)),
        };
        iterations = iterations.saturating_add(res.iterations);
        let full = layout.full(&res.p).ok_or(FitError::InvalidParams { field: "params" })?;
        let (pp, d, _, r) = axis_of(&fr, &full)?;
        // Foot of the perpendicular from the centroid (the origin) onto the axis.
        let s = red::dot(&pp, &d)?;
        let foot = [pp[0] - s * d[0], pp[1] - s * d[1], pp[2] - s * d[2]];
        let stop = res.iterations == 0;
        done = Some(Outcome { p: foot, d, r, last: res, iterations });
        if stop {
            break;
        }
        let (e1, e2) = if layout.dir { init::basis(d)? } else { (fr.e1, fr.e2) };
        fr = Frame { p0: foot, a: if layout.dir { d } else { fr.a }, e1, e2 };
        x = layout.free([0.0, 0.0, 0.0, 0.0, r])?;
    }
    let mut out = done.ok_or(FitError::InvalidParams { field: "reparam_rounds" })?;
    out.iterations = iterations;
    Ok(out)
}

/// Rule 6: the outcome as SI parameters in the start frame `fr` (scaled), `scale` the map's.
fn comparable(o: &Outcome, fr: &Frame, layout: Layout, c: &Centred) -> Result<Vec<f64>, FitError> {
    let mut d = o.d;
    let da = red::dot(&d, &fr.a)?;
    if da < 0.0 {
        d = [-d[0], -d[1], -d[2]];
    }
    let da = red::dot(&d, &fr.a)?;
    if da.is_nan() || da <= 0.0 {
        return Err(FitError::Num(NumError::Domain { op: OP, detail: "axis_perpendicular_to_start" }));
    }
    let a = red::dot(&d, &fr.e1)? / da;
    let b = red::dot(&d, &fr.e2)? / da;
    let w = [fr.p0[0] - o.p[0], fr.p0[1] - o.p[1], fr.p0[2] - o.p[2]];
    let s = red::dot(&w, &fr.a)? / da;
    let x = [(o.p[0] + s * d[0]) - fr.p0[0], (o.p[1] + s * d[1]) - fr.p0[1], (o.p[2] + s * d[2]) - fr.p0[2]];
    let m = c.map();
    let full = [
        a,
        b,
        m.inverse_length(red::dot(&x, &fr.e1)?)?,
        m.inverse_length(red::dot(&x, &fr.e2)?)?,
        m.inverse_length(o.r)?,
    ];
    layout.free(full)
}

/// Rule 1. Returns `(fixed radius in metres, fixed direction)`.
fn constraints(pts: &PointsView<'_>, params: &FitParams) -> Result<(Option<f64>, Option<V3>), FitError> {
    params.validate(pts)?;
    if params.method != Method::LeastSquares {
        return Err(FitError::InvalidParams { field: "method" });
    }
    if params.weights != Weights::Uniform {
        return Err(FitError::InvalidParams { field: "weights" });
    }
    if params.constraints.iter().any(|c| !matches!(c, Constraint::FixedRadius(_) | Constraint::FixedAxisDirection(_))) {
        return Err(FitError::InvalidParams { field: "constraints" });
    }
    Ok((params.fixed_radius(), params.fixed_axis_direction()))
}

/// Rule 8's reference: RMS distance of the scaled points to their least-squares plane.
fn plane_rms(c: &Centred) -> Result<f64, FitError> {
    let n = data_shape(c)?.axes[2];
    let mut r = alloc(c.len())?;
    for q in c.xyz().chunks_exact(3) {
        r.push(red::dot(q, &n)?);
    }
    Ok(red::norm2(&r)? / f::sqrt(c.len() as f64)?)
}

/// Rule 8's bound with a fixed direction: RMS of the best plane containing the axis direction
/// of frame `fr` (scaled), from the `2 x 2` scatter of the projections onto `(e1, e2)`.
fn bound_with_direction(c: &Centred, e1: V3, e2: V3) -> Result<f64, FitError> {
    let n = c.len();
    let (mut u, mut v) = (alloc(n)?, alloc(n)?);
    for q in c.xyz().chunks_exact(3) {
        u.push(red::dot(q, &e1)?);
        v.push(red::dot(q, &e2)?);
    }
    let (suu, svv, suv) = (red::dot(&u, &u)?, red::dot(&v, &v)?, red::dot(&u, &v)?);
    let lam = (suu + svv) / 2.0 - f::hypot((suu - svv) / 2.0, suv)?;
    let lam = if f::total_cmp(lam, 0.0)? == Ordering::Less { 0.0 } else { lam };
    Ok(f::sqrt(lam / n as f64)?)
}

fn to_frame(c: &Centred, fr: &AxisFrame) -> Result<Frame, FitError> {
    Ok(Frame { p0: c.map().forward(fr.point)?, a: fr.axis, e1: fr.e1, e2: fr.e2 })
}

/// The multi-start error as a fit error.
fn ms_error(e: MultiStartError<FitError>) -> FitError {
    match e {
        MultiStartError::InvalidSpec(_) => FitError::InvalidParams { field: "start_spec" },
        MultiStartError::AllFailed(errs) => {
            let first = match errs.first() {
                Some(StartError::Solver(e)) => e.code(),
                Some(StartError::Numeric(_)) => "numeric",
                Some(StartError::ParamLength { .. }) => "param_length",
                None => "none",
            };
            FitError::MultiStartFailed { starts: errs.len() as u32, first }
        }
    }
}

/// The cylinder fit with its multi-start record, starts from [`init::cylinder_start`]
/// (`seed`, or [`DEFAULT_SEED`]).
///
/// # Errors
///
/// `InvalidParams` (rule 1), the checks' `TooFewPoints` / `Collinear` / `Coplanar`, INIT's
/// `IllConditioned`, [`FitError::MultiStartFailed`] when every start fails, and
/// [`FitError::ImplausibleFit`] (rule 8). Flagged fits are returned with their flags (the
/// trait refuses them, rule 9).
pub fn fit_cylinder(pts: &PointsView<'_>, params: &FitParams, seed: Option<u64>) -> Result<CylinderFit, FitError> {
    let (_, dir) = constraints(pts, params)?;
    if dir.is_some() {
        return run(pts, params, None);
    }
    check_points(Primitive::Cylinder, pts, &params.thresholds)?;
    let start = init::cylinder_start(pts, seed.unwrap_or(DEFAULT_SEED))?;
    run(pts, params, Some(&start))
}

/// The cylinder fit from a caller-supplied start (the command's `StartSpec` override; its
/// spec must be INIT's `[a, b, u, v, r]` relative to its frame). A fixed axis direction
/// ignores the start (rule 3).
///
/// # Errors
///
/// As [`fit_cylinder`].
pub fn fit_cylinder_from(
    pts: &PointsView<'_>,
    params: &FitParams,
    start: &AxialStart,
) -> Result<CylinderFit, FitError> {
    let (_, dir) = constraints(pts, params)?;
    run(pts, params, if dir.is_some() { None } else { Some(start) })
}

fn run(pts: &PointsView<'_>, params: &FitParams, start: Option<&AxialStart>) -> Result<CylinderFit, FitError> {
    let (radius, dir) = constraints(pts, params)?;
    let (c, _) = check_points(Primitive::Cylinder, pts, &params.thresholds)?;
    let map = *c.map();
    let layout = Layout {
        dir: dir.is_none(),
        radius: match radius {
            Some(r) => Some(map.forward_length(r)?),
            None => None,
        },
    };
    let names = layout.names()?;
    // The start frame (scaled) and the multi-start run.
    let (frame_si, outcome, bound_c) = match (dir, start) {
        (Some(fd), _) => {
            let a = normalize_direction(fd)?;
            // INIT's algebraic circle of the projections, in the basis of `a` (rule 3).
            let (e1, e2, cu, cv, ra) = match init::kasa(&c, a)? {
                Some((e1, e2, cen, r)) => (e1, e2, cen[0], cen[1], r),
                None => return Err(FitError::IllConditioned { cond: None }),
            };
            let fr = Frame { p0: [0.0; 3], a, e1, e2 };
            let x0 = [0.0, 0.0, map.inverse_length(cu)?, map.inverse_length(cv)?, map.inverse_length(ra)?];
            let s0 = layout.free(x0)?;
            let rms_len = map.inverse_length(ra)?;
            let tol = {
                let mut t = alloc(names.len())?;
                for _ in 0..names.len() {
                    t.push(init::AGREEMENT_TOL * rms_len);
                }
                t
            };
            let frame_si = AxisFrame { point: map.inverse([0.0; 3])?, axis: a, e1, e2 };
            let o = mstart::run_listed(&[s0], &tol, |s: &[f64]| solve_start(&c, fr, layout, s, &params.nlls))
                .map_err(ms_error)?;
            let bound = if layout.radius.is_none() { Some(bound_with_direction(&c, e1, e2)?) } else { None };
            (frame_si, o, bound)
        }
        (None, Some(st)) => {
            let fr = to_frame(&c, &st.frame)?;
            let spec = spec_for(&st.spec, layout)?;
            let o = mstart::run(&spec, |s: &[f64]| solve_start(&c, fr, layout, s, &params.nlls)).map_err(ms_error)?;
            let bound = if layout.radius.is_none() { Some(plane_rms(&c)?) } else { None };
            (st.frame, o, bound)
        }
        (None, None) => return Err(FitError::InvalidParams { field: "start" }),
    };
    let best = &outcome.best;
    let feature = FittedFeature::Cylinder { p: best.p, d: unit_direction(best.d)?, r: best.r };
    let feature = map.inverse_feature(&feature)?;
    let mut res = alloc(c.len())?;
    for &ri in &best.last.residuals {
        res.push(map.inverse_length(ri)?);
    }
    let mut diag = FitDiagnostics::from_nlls(&best.last);
    diag.iterations = best.iterations;
    let result = FitResult::assemble(&feature, res, diag, None, id())?;
    let plane = map.inverse_length(plane_rms(&c)?)?;
    let ratio = if plane > 0.0 { result.stats.rms / plane } else { f64::INFINITY };
    let bound = match bound_c {
        Some(b) => Some(map.inverse_length(b)?),
        None => None,
    };
    if let Some(b) = bound {
        let limit = (1.0 + CERT_MARGIN) * b;
        if f::total_cmp(result.stats.rms, limit)? != Ordering::Less {
            return Err(FitError::ImplausibleFit { rms: result.stats.rms, bound: b });
        }
    }
    let suspect = f::total_cmp(ratio, SUSPECT_RATIO)? == Ordering::Greater;
    let per_start = outcome.per_start;
    Ok(CylinderFit {
        result,
        frame: frame_si,
        param_names: names,
        starts: outcome.starts,
        per_start,
        best_index: outcome.best_index,
        local_optimum_only: outcome.local_optimum_only,
        origin: outcome.origin,
        plane_rms: plane,
        bound_rms: bound,
        rms_ratio: ratio,
        suspect_axis: suspect,
    })
}

/// INIT's spec with the fixed parameters dropped (rule 3).
fn spec_for(s: &StartSpec, layout: Layout) -> Result<StartSpec, FitError> {
    let pick = |v: &[f64]| -> Result<Vec<f64>, FitError> {
        let arr: [f64; 5] = match v {
            [a, b, u, w, r] => [*a, *b, *u, *w, *r],
            _ => return Err(FitError::InvalidParams { field: "start_spec" }),
        };
        layout.free(arr)
    };
    Ok(StartSpec {
        primary: pick(&s.primary)?,
        half_widths: pick(&s.half_widths)?,
        count: s.count,
        seed: s.seed,
        stream: s.stream,
        tolerances: pick(&s.tolerances)?,
    })
}

/// One start (SI parameters in the start frame) through rules 5 and 6.
fn solve_start(
    c: &Centred,
    fr: Frame,
    layout: Layout,
    s: &[f64],
    cfg: &NllsConfig,
) -> mstart::SolveResult<Outcome, FitError> {
    let full = layout.full(s).ok_or(FitError::InvalidParams { field: "start" })?;
    let m = c.map();
    let scaled = [full[0], full[1], m.forward_length(full[2])?, m.forward_length(full[3])?, m.forward_length(full[4])?];
    let x0 = layout.free(scaled)?;
    let o = solve_rounds(c.xyz(), fr, layout, x0, cfg)?;
    let si = comparable(&o, &fr, layout, c)?;
    let cost = o.last.cost;
    Ok((o, cost, si))
}

impl Fit for CylinderLs {
    fn algorithm_id(&self) -> AlgorithmId {
        id()
    }

    /// Rule 9: fails closed on a flagged fit ([`FitError::FlaggedFit`]).
    fn fit(&self, pts: &PointsView<'_>, params: &FitParams, seed: Option<u64>) -> Result<FitResult, FitError> {
        let f = fit_cylinder(pts, params, seed)?;
        if f.local_optimum_only || f.suspect_axis {
            return Err(FitError::FlaggedFit {
                local_optimum_only: f.local_optimum_only,
                suspect_axis: f.suspect_axis,
                rms_ratio: f.rms_ratio,
            });
        }
        Ok(f.result)
    }
}
