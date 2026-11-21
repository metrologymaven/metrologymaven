//! Initial estimates for axial primitives. Spec node `KRN.LSQ.INIT`.
//!
//! [`cylinder_start`], [`cone_start`] and [`torus_start`] turn a point set into an
//! [`AxialStart`]: a reference frame on the start axis and the seeded multi-start
//! specification ([`StartSpec`] of `KRN.SOLV.MSTART`) that the axial least-squares fits
//! (`KRN.LSQ.AXIS`) run, so every axial fit starts the same way on every host (D0).
//!
//! # Pinned procedure
//!
//! 1. **Checks.** [`check_points`] with [`Thresholds::default`] for the primitive: too few
//!    points is [`FitError::TooFewPoints`] (cylinder 5, cone 6, torus 7), collinear or
//!    coplanar data is [`FitError::Collinear`] / [`FitError::Coplanar`]. Everything below
//!    runs on the centred, scaled data `q_i` of [`Centred`] (RMS distance in `[1, 2)`).
//! 2. **Candidate directions** (cylinder and cone), in this order:
//!    - when the view has unit normals, the right singular vector of the `n x 3` normal matrix
//!      with the smallest singular value (the smallest-eigenvalue eigenvector of the normals'
//!      scatter, by the blocked QR and Jacobi rules of `contract::data_shape`);
//!    - the three principal axes of the centred data ([`data_shape`], descending singular
//!      values);
//!    - the 13 lattice directions [`LATTICE`] (a `3 x 3 x 3` lattice up to sign), normalised.
//! 3. **Basis of a direction** `d` (unit): `k` = the coordinate axis with the smallest `|d_k|`
//!    (ties to the lower index); `e1 = unit(d x E_k)`, `e2 = d x e1` (component formulas, no
//!    FMA).
//! 4. **Algebraic circle** (Kasa) of the projections `u_i = red::dot(q_i, e1)`,
//!    `v_i = red::dot(q_i, e2)`: the blocked QR fold of rows
//!    `[u_i, v_i, 1, -(u_i u_i + v_i v_i)]` gives a `4 x 4` factor `R`; `(D, E, F)` =
//!    `fid_num::la::lstsq` of its leading `3 x 3` block and the first three entries of its last
//!    column (`rel_tol = DEFAULT_RANK_TOL`); the algebraic residual is `|R_33|` (the norm of the
//!    least-squares residual); centre `(-D / 2, -E / 2)`, `r^2 = (D^2 + E^2) / 4 - F`. A
//!    candidate whose solve is rank deficient, or whose `r^2` is not positive and finite, is
//!    degenerate (`residual = None`).
//! 5. **Primary** (cylinder): the non-degenerate candidates are ranked by residual in total
//!    order (a stable sort, so ties keep the candidate order). No non-degenerate candidate is
//!    [`FitError::IllConditioned`] `{ cond: None }`. Each of the [`REFINED`] best-ranked is
//!    refined by a pinned pattern search on the same residual (see `refine`): steps
//!    [`REFINE_STEPS`], at most [`REFINE_MOVES`] moves each. The lowest refined residual wins
//!    (total order, ties to the better-ranked candidate); `selected` is the index of the
//!    candidate it was refined from. The 16 fixed candidates alone can be 15 degrees off when
//!    the principal axes are mixed (a cylinder whose length is near `sqrt(6)` radii), and
//!    refining only the best one can converge about 90 degrees off: on a half cylinder of
//!    length near 1.066 radii (where `L^2 / 12` equals the transverse variance of a half
//!    circle) the best fixed candidate can lie in the basin of a perpendicular direction.
//! 6. **Cone**: the candidates of step 2 are scored, ranked and refined as in steps 4 and 5
//!    but with the algebraic cone of `cone_circle` (a circle whose radius varies linearly with
//!    the axial coordinate) instead of the circle, which a cone's projections do not fit at
//!    its true axis; that gives the axis and the centre `P`. Then, per point
//!    `t_i = red::dot(q_i - P, d)`
//!    and `rho_i = red::norm2(q_i - P - t_i d)`; a line `rho = r_ref + s t` by the blocked QR of
//!    rows `[1, t_i, rho_i]` and `lstsq` of the leading `2 x 2` block. If `s < 0` the axis is
//!    reversed (`d -> -d`, `e2 -> -e2`, `s -> -s`), so `d` points from the apex into the cone.
//!    `half_angle = f::atan(s)`; the apex is `P - (r_ref / s) d` (none when `s == 0`). `r_ref`
//!    not positive is [`FitError::IllConditioned`] `{ cond: None }`.
//! 7. **Torus**: candidates are the principal axes, the plane-fit normal (smallest singular
//!    value) first, then the other two, then the [`LATTICE`] directions. Each is scored by a
//!    fixed-axis torus fit (see `tube`: Gauss-Newton on the algebraic tube residuals from
//!    [`TORUS_START_SCALES`] starts on both sides of the data); the candidates are ranked and
//!    the [`REFINED`] best refined and selected as in step 5 on that score. Centre, major radius
//!    `R`, the tube-centre plane height `h0` and `r = f::sqrt(s)` come from the fixed-axis fit
//!    at the final axis. The plane
//!    fit alone is about 30 degrees off on a fat quarter torus, and the algebraic circle of a
//!    thick partial annulus is far from the tube-centre circle, which is why the score is a fit.
//! 8. **Anchor.** The frame point is the circle centre `P = u_c e1 + v_c e2` in the plane
//!    through the centroid, the foot of the perpendicular from the centroid onto the axis (the
//!    axis point at the data's mid-height; for the torus lifted by `h0` to the tube-centre
//!    plane), mapped back to SI metres; directions are unchanged
//!    by the map; lengths are multiplied by the power-of-two scale (exact).
//! 9. **Start specification.** Parameters relative to the frame (see [`AxisFrame::line`]):
//!    cylinder `[a, b, u, v, r]`, cone `[a, b, u, v, r_ref, half_angle]`, torus
//!    `[a, b, u, v, R, r]`; the axis is `unit(d + a e1 + b e2)` through `point + u e1 + v e2`.
//!    The primary has `a = b = u = v = 0`. Half widths: `a, b`: `f::tan(pi / 12)` (15 degrees
//!    [unverified]); `u, v`: `0.1 * rms` with `rms` the RMS distance of the points to the start
//!    axis in metres (10 % [unverified]); every other parameter 0. Count [`DEFAULT_COUNT`],
//!    the caller's seed, stream [`STREAM_CYLINDER`] / [`STREAM_CONE`] / [`STREAM_TORUS`].
//!    Agreement tolerances [unverified]: `1e-6` for `a`, `b` and `half_angle`, `1e-6 * rms`
//!    for every length.
//!
//! # Determinism
//!
//! Every sum, dot product and norm is a `fid_num::red` reduction; square roots, tangents and
//! arctangents go through `fid_num::f`; every minimum is a total-order comparison; no FMA, no
//! hash containers, fixed-order loops bounded by the point count. The result is bit-identical
//! on x86-64, ARM64 and wasm32 (D0). Nothing panics.

use core::cmp::Ordering;

use fid_num::la::{lstsq, Mat};
use fid_num::{f, red};
use fid_solv::mstart::StartSpec;

use crate::contract::{
    check_points, data_shape, shape_of_rows, tsqr, unit_direction, Centred, FitError, PointsView, Primitive,
    Thresholds, DEFAULT_RANK_TOL, V3,
};

/// Default start count, the primary included (REQ-KRN.LSQ.INIT-005).
pub const DEFAULT_COUNT: u32 = 8;

/// `fid_num::rng` stream of the cylinder starts.
pub const STREAM_CYLINDER: u64 = 1;
/// `fid_num::rng` stream of the cone starts.
pub const STREAM_CONE: u64 = 2;
/// `fid_num::rng` stream of the torus starts.
pub const STREAM_TORUS: u64 = 3;

/// Relative half width of the position box: 10 % of the RMS radius [unverified].
pub const POSITION_BOX: f64 = 0.1;

/// Steps of the primary refinement (tangent offsets, about 8, 4, 2, 1, 0.5, 0.25, 0.12 and
/// 0.06 degrees: `2^-3` to `2^-10`).
pub const REFINE_STEPS: [f64; 8] = [0.125, 0.0625, 0.03125, 0.015625, 0.0078125, 0.00390625, 0.001953125, 0.0009765625];

/// Largest major radius of a torus candidate in the centred units (RMS distance in `[1, 2)`):
/// a larger circle is treated as degenerate (the tube is then nearly straight and the score no
/// longer locates the axis).
pub const TORUS_MAX_MAJOR: f64 = 8.0;

/// Start scales of the fixed-axis torus fit (multiples of the algebraic circle's radius).
pub const TORUS_START_SCALES: [f64; 3] = [1.0, 2.0, 4.0];

/// Relative decrease of `red::norm2(f)` at or below which the fixed-axis torus fit stops.
pub const TORUS_GN_RTOL: f64 = 1e-6;

/// Candidates refined (the lowest-scoring ones), for every primitive (REQ-001, -003, -004).
pub const REFINED: usize = 3;

/// Torus candidates refined: [`REFINED`] (kept for callers of the first version).
pub const TORUS_REFINED: usize = REFINED;

/// Gauss-Newton steps of the fixed-axis torus fit that scores a torus axis candidate.
pub const TORUS_GN_STEPS: usize = 30;

/// Moves per refinement step, at most.
pub const REFINE_MOVES: usize = 16;

/// Relative agreement tolerance of every start parameter [unverified].
pub const AGREEMENT_TOL: f64 = 1e-6;

/// The 13 fixed hemisphere directions (a `3 x 3 x 3` lattice up to sign, first nonzero
/// component positive), unnormalised, in this pinned order.
pub const LATTICE: [[f64; 3]; 13] = [
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [1.0, 1.0, 0.0],
    [1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0],
    [1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0],
    [0.0, 1.0, -1.0],
    [1.0, 1.0, 1.0],
    [1.0, 1.0, -1.0],
    [1.0, -1.0, 1.0],
    [1.0, -1.0, -1.0],
];

/// Where a candidate direction came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateSource {
    /// The smallest-eigenvalue eigenvector of the normals' scatter (REQ-002).
    Normals,
    /// Principal axis `k` of the centred data (0 = largest singular value).
    Principal(u8),
    /// Entry `k` of [`LATTICE`].
    Lattice(u8),
}

/// One evaluated candidate direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    pub source: CandidateSource,
    /// Unit direction (the same in SI and in the centred frame).
    pub direction: V3,
    /// The candidate's score on the centred, scaled data, before refinement: the algebraic
    /// circle residual (cylinder), the algebraic cone residual (cone) or the final
    /// `red::norm2` of the fixed-axis torus fit (torus); `None` when degenerate.
    pub residual: Option<f64>,
}

/// The start axis: a point on it (SI metres), its unit direction and two unit vectors that
/// complete a right-handed orthonormal frame `(e1, e2, axis)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AxisFrame {
    pub point: V3,
    pub axis: V3,
    pub e1: V3,
    pub e2: V3,
}

impl AxisFrame {
    /// The axis line of the parameters `p = [a, b, u, v, ..]`: direction
    /// `unit(axis + a e1 + b e2)` through `point + u e1 + v e2` (component formulas, no FMA).
    ///
    /// # Errors
    ///
    /// [`FitError::InvalidParams`] for fewer than 4 parameters; [`FitError::InvalidDirection`]
    /// or a numeric error for a degenerate direction.
    pub fn line(&self, p: &[f64]) -> Result<(V3, V3), FitError> {
        let (Some(&a), Some(&b), Some(&u), Some(&v)) = (p.first(), p.get(1), p.get(2), p.get(3)) else {
            return Err(FitError::InvalidParams { field: "params" });
        };
        let mut d = [0.0; 3];
        let mut x = [0.0; 3];
        for k in 0..3 {
            d[k] = (self.axis[k] + a * self.e1[k]) + b * self.e2[k];
            x[k] = (self.point[k] + u * self.e1[k]) + v * self.e2[k];
        }
        Ok((x, unit_direction(d)?))
    }
}

/// The start of one axial fit (REQ-KRN.LSQ.INIT-005), every field echoed for journaling.
#[derive(Debug, Clone, PartialEq)]
pub struct AxialStart {
    pub primitive: Primitive,
    /// The frame the parameters are relative to.
    pub frame: AxisFrame,
    /// The multi-start specification for `fid_solv::mstart`.
    pub spec: StartSpec,
    /// Every evaluated candidate direction, in evaluation order: for the cylinder and cone the
    /// normals' direction (when normals are given), the three principal axes and the 13
    /// [`LATTICE`] directions; for the torus the three principal axes (plane-fit normal first)
    /// and the 13 [`LATTICE`] directions.
    pub candidates: Vec<Candidate>,
    /// Index into `candidates` of the candidate whose refinement gave the primary direction
    /// (the primary is the refined direction, not `candidates[selected].direction`).
    pub selected: usize,
    /// RMS distance of the points to the start axis (metres).
    pub rms_radius: f64,
    /// The cone apex (SI metres), when the radius line is not flat.
    pub apex: Option<V3>,
}

/// The algebraic circle of one direction (centred frame).
#[derive(Debug, Clone, Copy)]
struct Circle {
    e1: V3,
    e2: V3,
    centre: [f64; 2],
    r: f64,
    residual: f64,
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Step 3.
fn basis(d: V3) -> Result<(V3, V3), FitError> {
    let mut k = 0;
    for j in 1..3 {
        if f::total_cmp(d[j].abs(), d[k].abs())? == Ordering::Less {
            k = j;
        }
    }
    let mut ek = [0.0; 3];
    ek[k] = 1.0;
    let e1 = unit_direction(cross(d, ek))?;
    let e2 = cross(d, e1);
    Ok((e1, e2))
}

fn point(xyz: &[f64], i: usize) -> Result<V3, FitError> {
    let s = xyz.get(3 * i..3 * i + 3).ok_or(FitError::BadLength { len: xyz.len() })?;
    Ok([s[0], s[1], s[2]])
}

/// The algebraic model a direction is scored with: a circle (cylinder, torus) or a circle whose
/// radius varies linearly with the axial coordinate (cone, step 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Model {
    Circle,
    Cone,
    Torus,
}

/// Step 4 (or step 6's scoring) for the unit direction `d`; `None` when degenerate.
fn fit(c: &Centred, d: V3, model: Model) -> Result<Option<Circle>, FitError> {
    match model {
        Model::Circle => circle(c, d),
        Model::Cone => cone_circle(c, d),
        Model::Torus => tube(c, d).map(|t| t.map(|t| t.k)),
    }
}

/// Step 7's score of the direction `d`: a torus with axis direction `d` fitted to the
/// algebraic residuals `f_i = (rho_i - R)^2 + (h_i - h0)^2 - s` in the unknowns
/// `(u_c, v_c, R, h0, s)` (`rho_i = f::hypot(u_i - u_c, v_i - v_c)`, `h_i = red::dot(q_i, d)`,
/// `s = r^2`) by at most [`TORUS_GN_STEPS`] Gauss-Newton steps, each halved up to 10 times until
/// `red::norm2(f)` decreases (no decrease, or a decrease of at most [`TORUS_GN_RTOL`] of the norm
/// after the step is taken, ends the iteration); each step solves the blocked QR
/// fold of rows `[J_i, -f_i]` (`J_i` the analytic gradient of `f_i`) with `lstsq`.
///
/// The fit is run from up to `2 * TORUS_START_SCALES.len()` starts and the lowest final
/// `red::norm2(f)` wins (ties to the earlier start). The algebraic circle of step 4 (centre `c`,
/// radius `R_a`) and `g`, the unit vector from the projected centroid (the origin) to `c`, give
/// first, for each `m` of [`TORUS_START_SCALES`], `R = m R_a` and the centre `c + g (R - R_a)`
/// (for `m > 1` only when `c` is not the origin), then, when `c` is not the origin, for each `m`
/// the mirrored start `R = m R_a` with the centre `-g R` (on the other side of the data). `h0`
/// is `red::mean(h)` and `s` the mean of `(rho_i - R)^2 + (h_i - h0)^2`. The algebraic circle
/// of a thick partial annulus underestimates `R` and pulls the centre towards the data, so the
/// larger starts reach the true basin of a quarter torus; on a fat quarter torus (minor radius
/// 0.45 major radii and more) it can even curve the wrong way, with `c` on the far side of the
/// data from the true centre, which the mirrored starts cover.
///
/// A start is discarded when a solve is rank deficient, a value is not finite, `R` or `s` is not
/// positive, or `R` exceeds [`TORUS_MAX_MAJOR`] (a near-straight tube fits any direction along
/// it); all discarded is degenerate (`None`).
fn tube(c: &Centred, d: V3) -> Result<Option<Tube>, FitError> {
    let Some(k0) = circle(c, d)? else { return Ok(None) };
    let xyz = c.xyz();
    let n = c.len();
    let mut uvh = Vec::with_capacity(n);
    for i in 0..n {
        let q = point(xyz, i)?;
        uvh.push([red::dot(&q, &k0.e1)?, red::dot(&q, &k0.e2)?, red::dot(&q, &d)?]);
    }
    let hs: Vec<f64> = uvh.iter().map(|x| x[2]).collect();
    let h_mean = red::mean(&hs)?;
    let residuals = |x: &[f64; 5]| -> Result<(Vec<f64>, Vec<f64>), FitError> {
        let mut f = Vec::with_capacity(n);
        let mut rho = Vec::with_capacity(n);
        for p in &uvh {
            let r = f::hypot(p[0] - x[0], p[1] - x[1])?;
            let (a, b) = (r - x[2], p[2] - x[3]);
            rho.push(r);
            f.push((a * a + b * b) - x[4]);
        }
        Ok((f, rho))
    };
    let gn = |mut x: [f64; 5]| -> Result<Option<[f64; 5]>, FitError> {
        let mut current = red::norm2(&residuals(&x)?.0)?;
        for _ in 0..TORUS_GN_STEPS {
            let r = tsqr::<6, _>(n, |i, out| {
                let p = uvh.get(i).ok_or(FitError::BadLength { len: n })?;
                let rho = f::hypot(p[0] - x[0], p[1] - x[1])?;
                let (a, b) = (rho - x[2], p[2] - x[3]);
                let fi = (a * a + b * b) - x[4];
                let (gu, gv) = if rho > 0.0 { (-(p[0] - x[0]) / rho, -(p[1] - x[1]) / rho) } else { (0.0, 0.0) };
                *out = [2.0 * a * gu, 2.0 * a * gv, -2.0 * a, -2.0 * b, -1.0, -fi];
                Ok(())
            })?;
            let mut rows = [[0.0; 5]; 5];
            let mut rhs = [0.0; 5];
            for i in 0..5 {
                rows[i].copy_from_slice(&r[i][..5]);
                rhs[i] = r[i][5];
            }
            let sol = match lstsq(&Mat::from_rows(&rows)?, &rhs, DEFAULT_RANK_TOL) {
                Ok(s) if s.diag.rank == 5 => s,
                Ok(_) => return Ok(None),
                Err(e) if fid_num::la::LaError::of(&e).is_some() => return Ok(None),
                Err(e) => return Err(e.into()),
            };
            let mut alpha = 1.0;
            let mut moved = false;
            for _ in 0..11 {
                let mut trial = x;
                for (tj, dj) in trial.iter_mut().zip(&sol.x) {
                    *tj += alpha * dj;
                }
                if trial.iter().all(|v| v.is_finite()) {
                    let v = red::norm2(&residuals(&trial)?.0)?;
                    if f::total_cmp(v, current)? == Ordering::Less {
                        x = trial;
                        // Converged: the decrease is at most TORUS_GN_RTOL of the norm.
                        moved = current - v > TORUS_GN_RTOL * current;
                        current = v;
                        break;
                    }
                }
                alpha *= 0.5;
            }
            if !moved {
                break;
            }
        }
        if x[2] > 0.0 && x[4] > 0.0 && x[2] <= TORUS_MAX_MAJOR {
            Ok(Some(x))
        } else {
            Ok(None)
        }
    };
    let cn = f::hypot(k0.centre[0], k0.centre[1])?;
    let mut starts: Vec<[f64; 5]> = Vec::with_capacity(2 * TORUS_START_SCALES.len());
    for &m in &TORUS_START_SCALES {
        if m != 1.0 && cn <= 0.0 {
            continue;
        }
        let big = m * k0.r;
        let shift = if m == 1.0 { 0.0 } else { (big - k0.r) / cn };
        starts.push([k0.centre[0] + shift * k0.centre[0], k0.centre[1] + shift * k0.centre[1], big, h_mean, 0.0]);
    }
    if cn > 0.0 {
        let g = [k0.centre[0] / cn, k0.centre[1] / cn];
        for &m in &TORUS_START_SCALES {
            let big = m * k0.r;
            starts.push([-(g[0] * big), -(g[1] * big), big, h_mean, 0.0]);
        }
    }
    let mut best: Option<([f64; 5], f64)> = None;
    for mut x0 in starts {
        x0[4] = red::mean(&residuals(&x0)?.0)?;
        if let Some(x) = gn(x0)? {
            let v = red::norm2(&residuals(&x)?.0)?;
            let better = match &best {
                None => true,
                Some((_, b)) => f::total_cmp(v, *b)? == Ordering::Less,
            };
            if better {
                best = Some((x, v));
            }
        }
    }
    let Some((x, v)) = best else { return Ok(None) };
    let (_, rho) = residuals(&x)?;
    let k = Circle { e1: k0.e1, e2: k0.e2, centre: [x[0], x[1]], r: x[2], residual: v };
    Ok(Some(Tube { k, h0: x[3], s: x[4], rho }))
}

/// Step 7's fitted torus for one axis direction (centred units).
#[derive(Debug, Clone)]
struct Tube {
    k: Circle,
    h0: f64,
    s: f64,
    rho: Vec<f64>,
}

/// Step 6's algebraic cone of the direction `d`: `(u - u_c)^2 + (v - v_c)^2 = (r0 + s t)^2`
/// written linearly as `u^2 + v^2 + D u + E v + F + G t + H t^2 = 0` with
/// `t_i = red::dot(q_i, d)`; rows `[u_i, v_i, 1, t_i, t_i t_i, -(u_i u_i + v_i v_i)]`, the
/// blocked QR fold, `lstsq` of the leading `5 x 5` block; residual `|R_55|`; centre
/// `(-D / 2, -E / 2)`. Rank below 5 is degenerate. (`r` is not used for the cone and is 0.)
fn cone_circle(c: &Centred, d: V3) -> Result<Option<Circle>, FitError> {
    let (e1, e2) = basis(d)?;
    let xyz = c.xyz();
    let r = tsqr::<6, _>(c.len(), |i, out| {
        let q = point(xyz, i)?;
        let (u, v, t) = (red::dot(&q, &e1)?, red::dot(&q, &e2)?, red::dot(&q, &d)?);
        *out = [u, v, 1.0, t, t * t, -(u * u + v * v)];
        Ok(())
    })?;
    let mut rows = [[0.0; 5]; 5];
    let mut rhs = [0.0; 5];
    for i in 0..5 {
        rows[i].copy_from_slice(&r[i][..5]);
        rhs[i] = r[i][5];
    }
    let a = Mat::from_rows(&rows)?;
    let sol = match lstsq(&a, &rhs, DEFAULT_RANK_TOL) {
        Ok(s) => s,
        Err(e) if fid_num::la::LaError::of(&e).is_some() => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if sol.diag.rank < 5 {
        return Ok(None);
    }
    let centre = [-sol.x[0] / 2.0, -sol.x[1] / 2.0];
    Ok(Some(Circle { e1, e2, centre, r: 0.0, residual: r[5][5].abs() }))
}

/// Step 4 for the unit direction `d`; `None` when degenerate.
fn circle(c: &Centred, d: V3) -> Result<Option<Circle>, FitError> {
    let (e1, e2) = basis(d)?;
    let xyz = c.xyz();
    let r = tsqr::<4, _>(c.len(), |i, out| {
        let q = point(xyz, i)?;
        let (u, v) = (red::dot(&q, &e1)?, red::dot(&q, &e2)?);
        *out = [u, v, 1.0, -(u * u + v * v)];
        Ok(())
    })?;
    let a = Mat::from_rows(&[[r[0][0], r[0][1], r[0][2]], [r[1][0], r[1][1], r[1][2]], [r[2][0], r[2][1], r[2][2]]])?;
    let sol = match lstsq(&a, &[r[0][3], r[1][3], r[2][3]], DEFAULT_RANK_TOL) {
        Ok(s) => s,
        Err(e) if fid_num::la::LaError::of(&e).is_some() => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if sol.diag.rank < 3 {
        return Ok(None);
    }
    let (dd, ee, ff) = (sol.x[0], sol.x[1], sol.x[2]);
    let centre = [-dd / 2.0, -ee / 2.0];
    let r2 = (dd * dd + ee * ee) / 4.0 - ff;
    if !(r2.is_finite() && r2 > 0.0) {
        return Ok(None);
    }
    Ok(Some(Circle { e1, e2, centre, r: f::sqrt(r2)?, residual: r[3][3].abs() }))
}

/// Step 5b: pattern search on the algebraic residual from the selected direction `d`. For each
/// step `h` of [`REFINE_STEPS`] (radians, as tangent offsets): with the basis of the current
/// direction, the four neighbours `unit(d + h e1)`, `unit(d - h e1)`, `unit(d + h e2)`,
/// `unit(d - h e2)` are evaluated in that order; the one with the lowest residual (total order,
/// ties to the earlier) replaces the current direction if it is strictly lower, and the step is
/// repeated (at most [`REFINE_MOVES`] moves per step); otherwise the next, smaller step follows.
/// Degenerate neighbours are skipped.
fn refine(c: &Centred, mut d: V3, mut k: Circle, model: Model) -> Result<(V3, Circle), FitError> {
    for &h in &REFINE_STEPS {
        for _ in 0..REFINE_MOVES {
            let (e1, e2) = (k.e1, k.e2);
            let mut best: Option<(V3, Circle)> = None;
            for (e, sg) in [(e1, 1.0), (e1, -1.0), (e2, 1.0), (e2, -1.0)] {
                let t = sg * h;
                let nd = unit_direction([d[0] + t * e[0], d[1] + t * e[1], d[2] + t * e[2]])?;
                if let Some(nk) = fit(c, nd, model)? {
                    let better = match &best {
                        None => true,
                        Some((_, b)) => f::total_cmp(nk.residual, b.residual)? == Ordering::Less,
                    };
                    if better {
                        best = Some((nd, nk));
                    }
                }
            }
            match best {
                Some((nd, nk)) if f::total_cmp(nk.residual, k.residual)? == Ordering::Less => {
                    d = nd;
                    k = nk;
                }
                _ => break,
            }
        }
    }
    Ok((d, k))
}

/// Steps 2 and 5: the candidates, the index the primary was refined from and the primary.
fn axis_candidates(
    pts: &PointsView<'_>,
    c: &Centred,
    model: Model,
) -> Result<(Vec<Candidate>, usize, V3, Circle), FitError> {
    let mut dirs: Vec<(CandidateSource, V3)> = Vec::with_capacity(17);
    if let Some(normals) = pts.normals() {
        dirs.push((CandidateSource::Normals, shape_of_rows(normals)?.axes[2]));
    }
    let shape = data_shape(c)?;
    for (k, &a) in shape.axes.iter().enumerate() {
        dirs.push((CandidateSource::Principal(k as u8), a));
    }
    for (k, l) in LATTICE.iter().enumerate() {
        dirs.push((CandidateSource::Lattice(k as u8), unit_direction(*l)?));
    }
    let mut cands = Vec::with_capacity(dirs.len());
    let mut scored_dirs: Vec<(usize, V3, Circle)> = Vec::with_capacity(dirs.len());
    for (idx, (source, d)) in dirs.into_iter().enumerate() {
        let scored = fit(c, d, model)?;
        cands.push(Candidate { source, direction: d, residual: scored.map(|k| k.residual) });
        if let Some(k) = scored {
            scored_dirs.push((idx, d, k));
        }
    }
    let (sel, d, k) = refine_best(c, scored_dirs, model)?;
    Ok((cands, sel, d, k))
}

/// Steps 5 and 7: ranks the scored candidates `(index, direction, score)` by score (a stable
/// insertion sort in total order, so ties keep the candidate order), refines each of the
/// [`REFINED`] best-ranked and returns the lowest refined score (ties to the better-ranked
/// candidate) with the index of the candidate it was refined from. None scored is
/// [`FitError::IllConditioned`] `{ cond: None }`.
fn refine_best(
    c: &Centred,
    mut scored_dirs: Vec<(usize, V3, Circle)>,
    model: Model,
) -> Result<(usize, V3, Circle), FitError> {
    for k in 1..scored_dirs.len() {
        let mut p = k;
        while p > 0 && f::total_cmp(scored_dirs[p].2.residual, scored_dirs[p - 1].2.residual)? == Ordering::Less {
            scored_dirs.swap(p, p - 1);
            p -= 1;
        }
    }
    let mut best: Option<(usize, V3, Circle)> = None;
    for &(idx, d, k) in scored_dirs.iter().take(REFINED) {
        let (rd, rk) = refine(c, d, k, model)?;
        let better = match &best {
            None => true,
            Some((_, _, b)) => f::total_cmp(rk.residual, b.residual)? == Ordering::Less,
        };
        if better {
            best = Some((idx, rd, rk));
        }
    }
    best.ok_or(FitError::IllConditioned { cond: None })
}

/// The circle centre `u_c e1 + v_c e2` in the centred frame (step 8).
fn centre_point(k: &Circle) -> V3 {
    let mut p = [0.0; 3];
    for (j, pj) in p.iter_mut().enumerate() {
        *pj = k.centre[0] * k.e1[j] + k.centre[1] * k.e2[j];
    }
    p
}

/// The frame of a circle along `d` (centred frame), mapped to SI (step 8).
fn frame_of(c: &Centred, d: V3, k: &Circle) -> Result<AxisFrame, FitError> {
    Ok(AxisFrame { point: c.map().inverse(centre_point(k))?, axis: d, e1: k.e1, e2: k.e2 })
}

/// RMS distance of the centred points to the line through `p` with unit direction `d`
/// (centred units), with the per-point `t_i` and `rho_i`.
fn radial(c: &Centred, p: V3, d: V3) -> Result<(Vec<f64>, Vec<f64>, f64), FitError> {
    let xyz = c.xyz();
    let n = c.len();
    let mut ts = Vec::with_capacity(n);
    let mut rhos = Vec::with_capacity(n);
    for i in 0..n {
        let q = point(xyz, i)?;
        let w = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
        let t = red::dot(&w, &d)?;
        let perp = [w[0] - t * d[0], w[1] - t * d[1], w[2] - t * d[2]];
        ts.push(t);
        rhos.push(red::norm2(&perp)?);
    }
    let rms = red::norm2(&rhos)? / f::sqrt(n as f64)?;
    Ok((ts, rhos, rms))
}

fn spec(primary: Vec<f64>, rms: f64, seed: u64, stream: u64, angle_params: &[usize]) -> Result<StartSpec, FitError> {
    let t15 = f::tan(core::f64::consts::PI / 12.0)?;
    let n = primary.len();
    let mut half_widths = vec![0.0; n];
    let mut tolerances = vec![AGREEMENT_TOL * rms; n];
    half_widths[0] = t15;
    half_widths[1] = t15;
    half_widths[2] = POSITION_BOX * rms;
    half_widths[3] = POSITION_BOX * rms;
    tolerances[0] = AGREEMENT_TOL;
    tolerances[1] = AGREEMENT_TOL;
    for &k in angle_params {
        if let Some(t) = tolerances.get_mut(k) {
            *t = AGREEMENT_TOL;
        }
    }
    Ok(StartSpec { primary, half_widths, count: DEFAULT_COUNT, seed, stream, tolerances })
}

/// The cylinder start (REQ-KRN.LSQ.INIT-001, -002, -005, -006): parameters `[a, b, u, v, r]`.
///
/// # Errors
///
/// [`FitError::TooFewPoints`] below 5 points, [`FitError::Collinear`] / [`FitError::Coplanar`]
/// for degenerate data, [`FitError::IllConditioned`] `{ cond: None }` when every candidate is
/// degenerate (REQ-006), and numeric errors.
pub fn cylinder_start(pts: &PointsView<'_>, seed: u64) -> Result<AxialStart, FitError> {
    let (c, _) = check_points(Primitive::Cylinder, pts, &Thresholds::default())?;
    let (candidates, selected, d, k) = axis_candidates(pts, &c, Model::Circle)?;
    let frame = frame_of(&c, d, &k)?;
    let p = centre_point(&k);
    let (_, _, rms_c) = radial(&c, p, d)?;
    let rms = c.map().inverse_length(rms_c)?;
    let r = c.map().inverse_length(k.r)?;
    let spec = spec(vec![0.0, 0.0, 0.0, 0.0, r], rms, seed, STREAM_CYLINDER, &[])?;
    Ok(AxialStart { primitive: Primitive::Cylinder, frame, spec, candidates, selected, rms_radius: rms, apex: None })
}

/// The cone start (REQ-KRN.LSQ.INIT-003, -005, -006): parameters
/// `[a, b, u, v, r_ref, half_angle]`, `r_ref` the radius at the frame point (mid-height).
///
/// # Errors
///
/// As [`cylinder_start`] (below 6 points), and [`FitError::IllConditioned`] when the radius at
/// mid-height is not positive.
pub fn cone_start(pts: &PointsView<'_>, seed: u64) -> Result<AxialStart, FitError> {
    let (c, _) = check_points(Primitive::Cone, pts, &Thresholds::default())?;
    let (candidates, selected, d, k) = axis_candidates(pts, &c, Model::Cone)?;
    let p = centre_point(&k);
    let (ts, rhos, rms_c) = radial(&c, p, d)?;
    let r = tsqr::<3, _>(c.len(), |i, out| {
        *out = [
            1.0,
            *ts.get(i).ok_or(FitError::BadLength { len: ts.len() })?,
            *rhos.get(i).ok_or(FitError::BadLength { len: rhos.len() })?,
        ];
        Ok(())
    })?;
    let a = Mat::from_rows(&[[r[0][0], r[0][1]], [r[1][0], r[1][1]]])?;
    let sol = lstsq(&a, &[r[0][2], r[1][2]], DEFAULT_RANK_TOL)?;
    let (r_ref_c, mut s) = (sol.x[0], sol.x[1]);
    if !(r_ref_c.is_finite() && r_ref_c > 0.0 && s.is_finite()) {
        return Err(FitError::IllConditioned { cond: None });
    }
    let mut frame = frame_of(&c, d, &k)?;
    if s < 0.0 {
        s = -s;
        frame.axis = [-d[0] + 0.0, -d[1] + 0.0, -d[2] + 0.0];
        frame.e2 = [-frame.e2[0] + 0.0, -frame.e2[1] + 0.0, -frame.e2[2] + 0.0];
    }
    let half_angle = f::atan(s)?;
    let r_ref = c.map().inverse_length(r_ref_c)?;
    let apex = if s > 0.0 {
        let back = r_ref / s;
        let ax = frame.axis;
        Some([frame.point[0] - back * ax[0], frame.point[1] - back * ax[1], frame.point[2] - back * ax[2]])
    } else {
        None
    };
    let rms = c.map().inverse_length(rms_c)?;
    let spec = spec(vec![0.0, 0.0, 0.0, 0.0, r_ref, half_angle], rms, seed, STREAM_CONE, &[5])?;
    Ok(AxialStart { primitive: Primitive::Cone, frame, spec, candidates, selected, rms_radius: rms, apex })
}

/// The torus start (REQ-KRN.LSQ.INIT-004, -005, -006): parameters `[a, b, u, v, R, r]`.
///
/// # Errors
///
/// As [`cylinder_start`] (below 7 points): [`FitError::IllConditioned`] `{ cond: None }` when
/// the fixed-axis torus fit is degenerate for every candidate direction (or, which the refinement
/// rules out in practice, at the refined axis).
pub fn torus_start(pts: &PointsView<'_>, seed: u64) -> Result<AxialStart, FitError> {
    let (c, deg) = check_points(Primitive::Torus, pts, &Thresholds::default())?;
    let mut candidates = Vec::with_capacity(16);
    let mut scored_dirs: Vec<(usize, V3, Circle)> = Vec::with_capacity(16);
    let mut dirs: Vec<(CandidateSource, V3)> = Vec::with_capacity(16);
    for k in [2_usize, 1, 0] {
        dirs.push((CandidateSource::Principal(k as u8), deg.shape.axes[k]));
    }
    for (k, l) in LATTICE.iter().enumerate() {
        dirs.push((CandidateSource::Lattice(k as u8), unit_direction(*l)?));
    }
    for (idx, (source, d)) in dirs.into_iter().enumerate() {
        let scored = fit(&c, d, Model::Torus)?;
        candidates.push(Candidate { source, direction: d, residual: scored.map(|t| t.residual) });
        if let Some(t) = scored {
            scored_dirs.push((idx, d, t));
        }
    }
    let (selected, d, _) = refine_best(&c, scored_dirs, Model::Torus)?;
    let t = tube(&c, d)?.ok_or(FitError::IllConditioned { cond: None })?;
    let k = t.k;
    let minor_c = f::sqrt(t.s)?;
    let rms_c = red::norm2(&t.rho)? / f::sqrt(c.len() as f64)?;
    // The frame point is the torus centre: the circle centre lifted to the tube-centre plane.
    let mut frame = frame_of(&c, d, &k)?;
    let lift = c.map().inverse_length(t.h0)?;
    for j in 0..3 {
        frame.point[j] += lift * d[j];
    }
    let (major, minor, rms) =
        (c.map().inverse_length(k.r)?, c.map().inverse_length(minor_c)?, c.map().inverse_length(rms_c)?);
    let spec = spec(vec![0.0, 0.0, 0.0, 0.0, major, minor], rms, seed, STREAM_TORUS, &[])?;
    Ok(AxialStart { primitive: Primitive::Torus, frame, spec, candidates, selected, rms_radius: rms, apex: None })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn lattice_is_thirteen_directions_up_to_sign() {
        for (i, a) in LATTICE.iter().enumerate() {
            let first = a.iter().find(|x| **x != 0.0).copied().unwrap();
            assert!(first > 0.0);
            for b in &LATTICE[i + 1..] {
                let neg = [-b[0], -b[1], -b[2]];
                assert!(a != b && *a != neg);
            }
        }
    }

    /// REQ-006: every candidate degenerate is `IllConditioned { cond: None }`, for every model.
    /// From valid input this is unreachable for the circle score (rank-3 data has a candidate
    /// whose projections have rank 3, and the Kasa `r^2` of rank-3 projections is a mean of
    /// squares), so the selection rule is tested with no scored candidate.
    #[test]
    fn all_degenerate_is_ill_conditioned() {
        let xyz = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let c = Centred::new(&PointsView::new(&xyz, None).unwrap()).unwrap();
        for model in [Model::Circle, Model::Cone, Model::Torus] {
            assert_eq!(refine_best(&c, Vec::new(), model).unwrap_err(), FitError::IllConditioned { cond: None });
        }
    }

    #[test]
    fn basis_is_orthonormal() {
        for l in LATTICE {
            let d = unit_direction(l).unwrap();
            let (e1, e2) = basis(d).unwrap();
            let dot = |x: V3, y: V3| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
            assert!(dot(e1, d).abs() < 1e-15 && dot(e2, d).abs() < 1e-15 && dot(e1, e2).abs() < 1e-15);
            assert!((dot(e1, e1) - 1.0).abs() < 1e-15 && (dot(e2, e2) - 1.0).abs() < 1e-15);
            // right-handed: e1 x e2 = d
            let c = cross(e1, e2);
            assert!((0..3).all(|k| (c[k] - d[k]).abs() < 1e-15));
        }
    }
}
