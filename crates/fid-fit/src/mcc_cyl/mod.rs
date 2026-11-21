//! Minimum circumscribed cylinder. Spec node `KRN.ENV.MCC.CYL`.
//!
//! The smallest cylinder containing every point: the mating envelope of a shaft and the datum
//! simulator of an external cylindrical datum feature. [`fit_mcc_cylinder`] returns the full
//! [`MccCylinder`] (contacts, certificate, flags, multi-start record); [`MccCylinderFit`]
//! implements [`Fit`] for `fit.cylinder.mc@1.0.0` and fails closed on a flagged fit.
//!
//! # Pinned procedure
//!
//! 1. **Parameters.** [`FitParams::validate`]; the method must be
//!    [`Method::MinimumCircumscribed`], the weights uniform; the only constraint accepted is a
//!    fixed axis direction (`InvalidParams` `method` / `weights` / `constraints` otherwise).
//! 2. **Checks.** [`check_points`] for the cylinder (5 points, rank 3); the fit runs on the
//!    centred, scaled data `q_i` of [`Centred`].
//! 3. **Disc test** (REQ-004). With `n` the smallest principal axis of [`data_shape`] and
//!    `(e1, e2)` its basis (`KRN.LSQ.INIT` step 3), `thickness` is the range of `q . n` and
//!    `width` half the larger of the ranges of `q . e1` and `q . e2`. `thickness < DISC_RATIO *
//!    width` is [`FitError::IllConditioned`] with `cond = width / thickness` (a disc rather
//!    than a cylinder: the axis is then the disc normal and the radius is set by a ring of
//!    points with no axial extent to fix the tilt).
//! 4. **Frame and start.** Free direction: the least-squares cylinder of
//!    `ls_cyl::fit_cylinder` (seed as given) gives the axis `A` and `P0`, the foot of the
//!    perpendicular from the centroid (mid-height); its errors pass through, and a flagged
//!    least-squares fit (`local_optimum_only` or `suspect_axis`) makes this fit
//!    `local_optimum_only` (its start may be on a wrong axis). Fixed direction: `A` the
//!    constraint made unit length, `P0` the centroid. `(E1, E2)` by the basis rule. At the frame
//!    axis, `R0` is the largest distance of a point to it (a feasible radius), `w0 = R0 -` the
//!    smallest (the zone about that axis) and `L` the range of `q . A`.
//! 5. **Formulation** (`problem`): parameters `(a, b, u, v, R)` (fixed direction `(u, v, R)`)
//!    relative to the frame, rows `rho_i - R <= 0`, objective `R`; each `fid_solv::seq` step is
//!    the LP of the linearised rows. The merit is `R + 2 max(0, max_i (rho_i - R))` (the
//!    multipliers sum to 1). Step scales `1` for `a`, `b` and `R0` for `u`, `v`, `R`.
//! 6. **Starts.** `fid_solv::seq::solve` (`CertKind::Envelope`, default constants) through
//!    `fid_solv::mstart::run_listed` from the starts of the `StartSpec` with primary `[0, 0, 0, 0, R0]`, half widths `[w0 / L, w0 / L, w0, w0, 0]`
//!    (the tilt and offset the zone allows), [`DEFAULT_COUNT`] starts, the seed (or
//!    [`DEFAULT_SEED`]) and stream [`STREAM`]; tolerances [`AGREE_TOL`] (tangents) and
//!    `AGREE_TOL * R0` (lengths). Each start's radius is then lifted to the largest point
//!    distance to its axis, so every start is feasible (an infeasible start's first trust
//!    region can be too small to restore feasibility, `subproblem_infeasible`). As in
//!    `seq::solve_multi`, a start that met its stop rule but did not certify sets
//!    `local_optimum_only`. Fixed direction: one start `[0, 0, R0]`. The radius's step scale
//!    is `problem::R_SCALE` times `R0`, so the radius can always absorb an iterate's violation. Every start's result
//!    is certified by `fid_solv::cert::certify` inside `seq` (fail closed); no certified start
//!    is [`FitError::SeqFailed`] with the first start's cause.
//! 7. **Result.** From the selected certificate: the axis, the contacts (the points of its
//!    active set, ascending), the radius **recomputed from the contacts and the final axis**
//!    (the largest contact distance) and every point's distance to the axis. The largest excess
//!    of a point over that radius is `max_violation`; above [`INSIDE_TOL`] (metres) it is
//!    [`FitError::SideViolation`] (fail closed).
//! 8. **Optimality.** The certificate proves a KKT point with `n` independent active rows and
//!    positive multipliers: a **strict local minimum** ([`Optimality::StrictLocal`]). With a
//!    fixed direction the problem is convex (the smallest circle enclosing the projections), so
//!    the certified point is the **global** minimum ([`Optimality::Global`]) and
//!    `local_optimum_only` is false (REQ-003).
//! 9. **Wrong-axis guards.** Agreement of the starts is not proof of correctness (review of
//!    #358): every start can converge to the same wrong axis.
//!    - **Certificate.** `R0` is the radius of a feasible cylinder (the frame axis), so the
//!      global minimum is at most `R0`: a certified radius above `(1 + CERT_MARGIN) R0` is not
//!      the global optimum ([`FitError::ImplausibleFit`] with `rms` the radius and `bound`
//!      `R0`).
//!    - **Flag.** `zone_ratio = w / w0` with `w = R -` the smallest point distance (the zone
//!      about the fitted axis). The envelope of the measured surface has about the zone of the
//!      least-squares axis; a fit whose zone exceeds [`SUSPECT_ZONE_RATIO`] times it has moved
//!      the axis off the surface's (for example a short arc whose enclosing cylinder collapses
//!      onto its chord) and sets `suspect_axis`: axis weakly determined or wrong
//!      \[unverified threshold\].
//! 10. **Trait (fail closed).** [`MccCylinderFit`]'s [`Fit::fit`] refuses a fit with
//!     `local_optimum_only` or `suspect_axis`, and a [`Optimality::StrictLocal`] fit whose
//!     global cross-check (rule 11) did not pass, as [`FitError::FlaggedFit`];
//!     [`fit_mcc_cylinder`] returns flagged fits with their flags.
//! 11. **Global cross-check** (free direction only; reviews of #374). Every start lies in a
//!     small box about the least-squares axis, so a certified strict local minimum can be far
//!     from the global one (a short pilot diameter with ovality: the near-axis minimum is
//!     beaten by a tilted or crosswise cylinder), and a check along a few fixed directions
//!     can miss the better one. `global` runs a branch and bound over a cap cover of every
//!     axis direction (icosahedral faces split into four), with lower bounds over a cap from
//!     a farthest pair and from a support set (Lipschitz in the direction) and upper bounds
//!     from feasible cylinders at cap centres; its documentation has the bounds and their
//!     proofs. With `R` the certified radius and `tol = CROSS_TOL * R +` the certificate's
//!     value bound, the outcome is `passed` (no cylinder smaller than `R - tol`), `better` (a
//!     feasible cylinder smaller than that) or `inconclusive` (the depth or work limit ran
//!     out). Anything but `passed` makes the fit `local_optimum_only` (so refused by the
//!     trait). The better cylinder is not returned (it is feasible, not a certified minimum).
//!
//! # Determinism
//!
//! D0 for a given seed: single thread, no FMA, no `f64::min`/`max`, every sum, dot product and
//! norm through `fid_num::red`, the KNP-1 conic solver of `fid-solv`, no hash containers.

mod global;
mod problem;

use core::cmp::Ordering;

use fid_kci::AlgorithmId;
use fid_num::{f, red, NumError};
use fid_solv::cert::{CertKind, Certificate};
use fid_solv::mstart::{self, MultiStartError, StartError, StartOrigin, StartSpec, StartSummary};
use fid_solv::seq::{self, SeqConfig, SolverError};
use fid_types::Determinism;

use crate::contract::{
    self, algorithm_id, check_points, command_row, data_shape, normalize_direction, unit_direction, Centred,
    CommandRow, Constraint, Fit, FitDiagnostics, FitError, FitParams, FitResult, FittedFeature, Method, PointsView,
    Primitive, StopReason, Weights, V3,
};
use crate::init;
use crate::ls_cyl;
use problem::{Frame, Mcc};

/// Algorithm version of `fit.cylinder.mc`.
pub const VERSION: (u32, u32, u32) = (1, 0, 0);
/// Seed when `seed` is `None`.
pub const DEFAULT_SEED: u64 = 0;
/// Start count (the primary included).
pub const DEFAULT_COUNT: u32 = 8;
/// `fid_num::rng` stream of the starts (INIT uses 1 to 3).
pub const STREAM: u64 = 4;
/// Agreement tolerance of the certified starts: tangents, and times `R0` for lengths
/// \[unverified\].
pub const AGREE_TOL: f64 = 1e-9;
/// Disc test: `thickness < DISC_RATIO * width` is `IllConditioned` (rule 3) \[unverified\].
pub const DISC_RATIO: f64 = 0.01;
/// Largest accepted excess of a point over the recomputed radius, metres (rule 7).
pub const INSIDE_TOL: f64 = 1e-12;
/// Relative margin of the certificate rule (rule 9).
pub const CERT_MARGIN: f64 = 1e-9;
/// `zone_ratio` above which `suspect_axis` is set (rule 9) \[unverified\].
pub const SUSPECT_ZONE_RATIO: f64 = 3.0;
/// Relative tolerance of the global cross-check, on top of the certificates' value bounds
/// (rule 11).
pub const CROSS_TOL: f64 = 1e-9;

const OP: &str = "fit.mcc_cyl";

/// The trait implementation of `fit.cylinder.mc@1.0.0` (rule 10).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MccCylinderFit;

/// What the certificate proves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Optimality {
    /// A strict local minimum (free direction).
    StrictLocal,
    /// The global minimum (fixed direction: a convex problem).
    Global,
}

impl Optimality {
    /// The wire string.
    pub const fn as_str(self) -> &'static str {
        match self {
            Optimality::StrictLocal => "strict_local",
            Optimality::Global => "global",
        }
    }
}

/// The outcome of the global cross-check (rule 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossOutcome {
    /// No cylinder, in any direction, is smaller than the fit's radius less `tol`.
    Passed,
    /// A feasible cylinder is smaller: the fit is not the global minimum.
    Better,
    /// The work limits ran out before every cap was decided.
    Inconclusive,
}

impl CrossOutcome {
    /// The wire string.
    pub const fn as_str(self) -> &'static str {
        match self {
            CrossOutcome::Passed => "passed",
            CrossOutcome::Better => "better",
            CrossOutcome::Inconclusive => "inconclusive",
        }
    }
}

/// The global cross-check of a free-direction fit (rule 11).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrossCheck {
    pub outcome: CrossOutcome,
    /// Cap tests and full (all-point) tests run, and the deepest cap level reached.
    pub caps: u32,
    pub full_tests: u32,
    pub depth: u32,
    /// `better`: a direction (unit) and the radius of a feasible cylinder along it (metres),
    /// smaller than the fit's.
    pub better: Option<(V3, f64)>,
}

/// A minimum circumscribed cylinder with its record (REQ-002).
#[derive(Debug, Clone, PartialEq)]
pub struct MccCylinder {
    pub algorithm: AlgorithmId,
    /// The cylinder through the foot of the perpendicular from the centroid (metres), axis
    /// canonical, radius recomputed from the contacts (rule 7).
    pub feature: FittedFeature,
    /// Signed distance of each point to the surface (`rho_i - R`, metres; not above
    /// `INSIDE_TOL`).
    pub residuals: Vec<f64>,
    /// Point indices of the certified active set, ascending.
    pub contacts: Vec<u32>,
    /// `max(0, max_i (rho_i - R))`, metres.
    pub max_violation: f64,
    /// The certificate of the selected start, `local_optimum_only` set when flagged.
    pub certificate: Certificate,
    pub optimality: Optimality,
    /// The certified starts agree (`mstart`'s agreement).
    pub agreement: bool,
    /// Starts disagree, a start converged but did not certify, the least-squares start was
    /// flagged, or the global cross-check did not pass.
    pub local_optimum_only: bool,
    /// The global cross-check (rule 11; `None` for a fixed direction) and whether it passed
    /// (true for a fixed direction, whose certified minimum is global).
    pub cross_check: Option<CrossCheck>,
    pub cross_check_passed: bool,
    /// Axis weakly determined or wrong (rule 9).
    pub suspect_axis: bool,
    /// `R0` (metres) and `w / w0` (rule 9).
    pub start_radius: f64,
    pub zone_ratio: f64,
    /// The starts (frame parameters, scaled units) and one outcome per start: the certified
    /// value and parameters, or the start's error.
    pub starts: Vec<Vec<f64>>,
    pub per_start: Vec<StartSummary<SolverError>>,
    pub best_index: u32,
    pub origin: StartOrigin,
}

fn alloc<T>(n: usize) -> Result<Vec<T>, FitError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "allocation_failed" }))?;
    Ok(v)
}

fn cert_err(e: fid_solv::cert::CertError) -> FitError {
    match e {
        fid_solv::cert::CertError::Numeric(n) => FitError::Num(n),
        _ => FitError::Num(NumError::Domain { op: OP, detail: "problem" }),
    }
}

/// The algorithm ID.
pub fn algorithm() -> AlgorithmId {
    algorithm_id(Primitive::Cylinder, Method::MinimumCircumscribed, VERSION).unwrap_or_else(|_| AlgorithmId {
        module: "fit".to_string(),
        feature: Primitive::Cylinder.slug().to_string(),
        method: Method::MinimumCircumscribed.slug().to_string(),
        version: VERSION,
    })
}

/// The command data row `fit.mc_cylinder@1` (W2, dry run R1, D0 per seed).
pub fn mcc_command_row() -> Result<CommandRow, FitError> {
    command_row(Primitive::Cylinder, Method::MinimumCircumscribed, VERSION, Determinism::D0)
}

fn seq_cause(e: &StartError<SolverError>) -> &'static str {
    match e {
        StartError::Solver(s) => s.cause.code(),
        StartError::Numeric(_) => "numeric",
        StartError::ParamLength { .. } => "param_length",
    }
}

/// Rule 3.
fn disc_test(c: &Centred) -> Result<(), FitError> {
    let n = data_shape(c)?.axes[2];
    let (e1, e2) = init::basis(n)?;
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for q in c.xyz().chunks_exact(3) {
        for (k, d) in [n, e1, e2].iter().enumerate() {
            let x = red::dot(q, d)?;
            lo[k] = f::total_min(lo[k], x)?;
            hi[k] = f::total_max(hi[k], x)?;
        }
    }
    let thickness = hi[0] - lo[0];
    let width = f::total_max(hi[1] - lo[1], hi[2] - lo[2])? / 2.0;
    if f::total_cmp(thickness, DISC_RATIO * width)? == Ordering::Less {
        let cond = if thickness > 0.0 { Some(width / thickness) } else { None };
        return Err(FitError::IllConditioned { cond });
    }
    Ok(())
}

/// Rule 1. Returns the fixed direction, if any.
fn constraints(pts: &PointsView<'_>, params: &FitParams) -> Result<Option<V3>, FitError> {
    params.validate(pts)?;
    if params.method != Method::MinimumCircumscribed {
        return Err(FitError::InvalidParams { field: "method" });
    }
    if params.weights != Weights::Uniform {
        return Err(FitError::InvalidParams { field: "weights" });
    }
    if params.constraints.iter().any(|c| !matches!(c, Constraint::FixedAxisDirection(_))) {
        return Err(FitError::InvalidParams { field: "constraints" });
    }
    Ok(params.fixed_axis_direction())
}

/// The frame from the least-squares fit (rule 4) and whether that fit was flagged.
fn ls_frame(pts: &PointsView<'_>, params: &FitParams, c: &Centred, seed: u64) -> Result<(Frame, bool), FitError> {
    let mut lp = FitParams::new(Method::LeastSquares);
    lp.thresholds = params.thresholds;
    let ls = ls_cyl::fit_cylinder(pts, &lp, Some(seed))?;
    let (p, d) = match ls.result.feature {
        FittedFeature::Cylinder { p, d, .. } => (p, d),
        _ => return Err(FitError::InvalidParams { field: "start" }),
    };
    let a = unit_direction(d)?;
    let (e1, e2) = init::basis(a)?;
    Ok((Frame { p0: c.map().forward(p)?, a, e1, e2 }, ls.local_optimum_only || ls.suspect_axis))
}

/// The fit from the least-squares start (or the fixed direction), with its record.
///
/// # Errors
///
/// `InvalidParams` (rule 1), the checks' errors, `IllConditioned` (disc, rule 3), the
/// least-squares fit's errors, [`FitError::SeqFailed`] when no start certifies,
/// [`FitError::SideViolation`] and [`FitError::ImplausibleFit`] (rules 7 and 9).
pub fn fit_mcc_cylinder(pts: &PointsView<'_>, params: &FitParams, seed: Option<u64>) -> Result<MccCylinder, FitError> {
    let fixed = constraints(pts, params)?;
    let (c, _) = check_points(Primitive::Cylinder, pts, &params.thresholds)?;
    disc_test(&c)?;
    let seed = seed.unwrap_or(DEFAULT_SEED);
    let (fr, ls_flagged) = match fixed {
        Some(fd) => {
            let a = normalize_direction(fd)?;
            let (e1, e2) = init::basis(a)?;
            (Frame { p0: [0.0; 3], a, e1, e2 }, false)
        }
        None => ls_frame(pts, params, &c, seed)?,
    };
    let mut m = run(&c, fr, fixed.is_none(), ls_flagged, seed)?;
    if fixed.is_none() {
        cross_check(&c, &mut m)?;
    }
    Ok(m)
}

/// The fit from a caller-supplied frame (SI metres: a point on the start axis and its
/// direction), as the command's start override. Free direction only.
///
/// # Errors
///
/// As [`fit_mcc_cylinder`].
pub fn fit_mcc_cylinder_from(
    pts: &PointsView<'_>,
    params: &FitParams,
    point: V3,
    axis: V3,
    seed: Option<u64>,
) -> Result<MccCylinder, FitError> {
    if constraints(pts, params)?.is_some() {
        return Err(FitError::InvalidParams { field: "constraints" });
    }
    let (c, _) = check_points(Primitive::Cylinder, pts, &params.thresholds)?;
    disc_test(&c)?;
    let a = unit_direction(axis)?;
    let (e1, e2) = init::basis(a)?;
    let fr = Frame { p0: c.map().forward(point)?, a, e1, e2 };
    let seed = seed.unwrap_or(DEFAULT_SEED);
    let mut m = run(&c, fr, true, false, seed)?;
    cross_check(&c, &mut m)?;
    Ok(m)
}

/// Rule 11: the global cross-check of a free-direction fit `m`. Sets `m.cross_check`,
/// `m.cross_check_passed` and, when it did not pass, `local_optimum_only` (on the record and
/// the certificate).
fn cross_check(c: &Centred, m: &mut MccCylinder) -> Result<(), FitError> {
    let map = *c.map();
    let r = match m.feature {
        FittedFeature::Cylinder { r, .. } => map.forward_length(r)?,
        _ => return Err(FitError::InvalidParams { field: "feature" }),
    };
    let tol = CROSS_TOL * r + m.certificate.value_bound();
    let s = global::search(c.xyz(), &m.contacts, r, tol)?;
    let better = match s.better {
        Some((d, ub)) => Some((normalize_direction(d)?, map.inverse_length(ub)?)),
        None => None,
    };
    let passed = s.outcome == CrossOutcome::Passed;
    m.cross_check = Some(CrossCheck { outcome: s.outcome, caps: s.caps, full_tests: s.full, depth: s.depth, better });
    m.cross_check_passed = passed;
    if !passed {
        m.local_optimum_only = true;
        if !m.certificate.flags().local_optimum_only {
            m.certificate
                .set_local_optimum_only()
                .map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "cert_flag" }))?;
        }
    }
    Ok(())
}

fn run(c: &Centred, fr: Frame, dir: bool, ls_flagged: bool, seed: u64) -> Result<MccCylinder, FitError> {
    let map = *c.map();
    let mut data_scale = 0.0_f64;
    for &x in c.xyz() {
        data_scale = f::total_max(data_scale, x.abs())?;
    }
    let mut prob = Mcc { q: c.xyz(), fr, dir, len_scale: 1.0, data_scale, r0: 1.0 };
    let zero: Vec<f64> = if dir { vec![0.0; 5] } else { vec![0.0; 3] };
    let (r0, lo0) = prob.extremes(&zero).map_err(cert_err)?;
    let w0 = r0 - lo0;
    let mut tlo = f64::INFINITY;
    let mut thi = f64::NEG_INFINITY;
    for q in c.xyz().chunks_exact(3) {
        let t = red::dot(q, &fr.a)?;
        tlo = f::total_min(tlo, t)?;
        thi = f::total_max(thi, t)?;
    }
    let len = thi - tlo;
    prob.r0 = r0;
    prob.len_scale = r0;
    data_scale = f::total_max(data_scale, r0)?;
    prob.data_scale = data_scale;
    let tilt = if len > 0.0 { w0 / len } else { 0.0 };
    let spec = if dir {
        StartSpec {
            primary: vec![0.0, 0.0, 0.0, 0.0, r0],
            half_widths: vec![tilt, tilt, w0, w0, 0.0],
            count: DEFAULT_COUNT,
            seed,
            stream: STREAM,
            tolerances: vec![AGREE_TOL, AGREE_TOL, AGREE_TOL * r0, AGREE_TOL * r0, AGREE_TOL * r0],
        }
    } else {
        StartSpec {
            primary: vec![0.0, 0.0, r0],
            half_widths: vec![0.0; 3],
            count: 1,
            seed,
            stream: STREAM,
            tolerances: vec![AGREE_TOL * r0; 3],
        }
    };
    let cfg = SeqConfig::new(CertKind::Envelope);
    // Rule 6: the seeded starts, each lifted to a feasible radius (the largest distance to its
    // axis), so every start's first subproblem is feasible at a zero step.
    let generated = mstart::generate_starts(&spec).map_err(|_| FitError::InvalidParams { field: "start_spec" })?;
    let mut starts = alloc(generated.len())?;
    for mut st in generated {
        let (hi, _) = prob.extremes(&st).map_err(cert_err)?;
        if let Some(r) = st.last_mut() {
            *r = hi;
        }
        starts.push(st);
    }
    let out = match mstart::run_listed(&starts, &spec.tolerances, |start| {
        let o = seq::solve(&prob, start, &cfg)?;
        let value = o.certified.value();
        let params = o.certified.params().to_vec();
        Ok((o, value, params))
    }) {
        Ok(o) => o,
        Err(MultiStartError::AllFailed(errs)) => {
            let cause = errs.first().map_or("no_starts", seq_cause);
            return Err(FitError::SeqFailed { cause, starts: spec.count });
        }
        Err(MultiStartError::InvalidSpec(_)) => return Err(FitError::InvalidParams { field: "start_spec" }),
    };
    // As `seq::solve_multi`: a start that met its stop rule but did not certify found a point
    // the run cannot vouch for, so it never counts as agreeing.
    let converged_but_failed =
        out.per_start.iter().any(|s| matches!(s, StartSummary::Failed(StartError::Solver(e)) if e.converged));
    let certified = &out.best.certified;
    let p = certified.params().to_vec();
    let mut contacts: Vec<u32> = alloc(certified.certificate().active_set().len())?;
    contacts.extend(certified.certificate().active_set().iter().map(|l| l.point));
    contacts.sort_unstable();
    let dists = prob.distances(&p).map_err(cert_err)?;
    // Rule 7: the radius from the contacts and the final axis.
    let mut r = 0.0_f64;
    for &k in &contacts {
        r = f::total_max(r, *dists.get(k as usize).ok_or(FitError::InvalidParams { field: "contacts" })?)?;
    }
    let mut lo = f64::INFINITY;
    let mut res = alloc(dists.len())?;
    let mut worst = (0usize, 0.0_f64);
    for (i, &d) in dists.iter().enumerate() {
        let e = map.inverse_length(d - r)?;
        if f::total_cmp(e, worst.1)? == Ordering::Greater {
            worst = (i, e);
        }
        lo = f::total_min(lo, d)?;
        res.push(e);
    }
    if f::total_cmp(worst.1, INSIDE_TOL)? == Ordering::Greater {
        return Err(FitError::SideViolation { index: worst.0, violation: worst.1 });
    }
    let r_si = map.inverse_length(r)?;
    let r0_si = map.inverse_length(r0)?;
    if f::total_cmp(r, (1.0 + CERT_MARGIN) * r0)? == Ordering::Greater {
        return Err(FitError::ImplausibleFit { rms: r_si, bound: r0_si });
    }
    let zone_ratio = if w0 > 0.0 { (r - lo) / w0 } else { f64::INFINITY };
    let suspect = f::total_cmp(zone_ratio, SUSPECT_ZONE_RATIO)? == Ordering::Greater;
    let (foot, d) = prob.line(&p).map_err(cert_err)?;
    let feature = map.inverse_feature(&FittedFeature::Cylinder { p: foot, d, r })?.canonical()?;
    let local_optimum_only = dir && (out.local_optimum_only || converged_but_failed || ls_flagged);
    let mut certificate = out.best.certified.clone().into_certificate();
    if local_optimum_only && !certificate.flags().local_optimum_only {
        certificate
            .set_local_optimum_only()
            .map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "cert_flag" }))?;
    }
    Ok(MccCylinder {
        algorithm: algorithm(),
        feature,
        residuals: res,
        contacts,
        max_violation: f::total_max(0.0, worst.1)?,
        certificate,
        optimality: if dir { Optimality::StrictLocal } else { Optimality::Global },
        agreement: out.agreement,
        local_optimum_only,
        cross_check: None,
        // A fixed direction is global (rule 8); a free one is set by rule 11.
        cross_check_passed: !dir,
        suspect_axis: suspect,
        start_radius: r0_si,
        zone_ratio,
        starts: out.starts,
        per_start: out.per_start,
        best_index: out.best_index,
        origin: out.origin,
    })
}

impl Fit for MccCylinderFit {
    fn algorithm_id(&self) -> AlgorithmId {
        algorithm()
    }

    /// Rule 10: fails closed on a flagged fit ([`to_fit_result`]).
    fn fit(&self, pts: &PointsView<'_>, params: &FitParams, seed: Option<u64>) -> Result<FitResult, FitError> {
        let m = fit_mcc_cylinder(pts, params, seed)?;
        let scale = Centred::new(pts)?.map().scale();
        to_fit_result(m, scale)
    }
}

/// Rule 10: the contract summary of a fit, refusing a flagged one ([`FitError::FlaggedFit`]).
/// `scale` is the centring scale of the fitted points (`Normalisation::scale`), for the
/// certificate's value bound in metres. Diagnostics: rank the parameter count, `cond` the
/// certificate's condition estimate, `grad_norm` 0 and `iterations` 0 (iterates are not values
/// of record), stop `exchange`; the contract certificate has the contacts as support and the
/// value bound as gap.
///
/// # Errors
///
/// [`FitError::FlaggedFit`] for a flagged fit, and the contract's assembly errors.
pub fn to_fit_result(m: MccCylinder, scale: f64) -> Result<FitResult, FitError> {
    // A strict local minimum is accepted only with a passed cross-check (rule 11); a fit
    // without one is reported as `local_optimum_only`.
    let unchecked = m.optimality == Optimality::StrictLocal && !m.cross_check_passed;
    if m.local_optimum_only || m.suspect_axis || unchecked {
        return Err(FitError::FlaggedFit {
            local_optimum_only: m.local_optimum_only || unchecked,
            suspect_axis: m.suspect_axis,
            rms_ratio: m.zone_ratio,
        });
    }
    let diagnostics = FitDiagnostics {
        grad_norm: 0.0,
        rank: m.certificate.params().len(),
        cond: m.certificate.cond(),
        iterations: 0,
        stop: StopReason::Exchange,
    };
    let mut support = alloc(m.contacts.len())?;
    support.extend(m.contacts.iter().map(|&i| i as usize));
    let cert = contract::Certificate { support, gap: m.certificate.value_bound() * scale };
    FitResult::assemble(&m.feature, m.residuals, diagnostics, Some(cert), m.algorithm)
}
