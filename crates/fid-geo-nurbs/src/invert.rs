//! Global point inversion on a single untrimmed NURBS curve or surface: the closest point, its
//! parameters, the distance and whether it is unique. Spec node `GEO.NURBS.INVERT`.
//!
//! # Algorithm (pinned in `spec/geo/nurbs/invert/NODE.md`, Design)
//!
//! 1. **Bézier decomposition** by knot insertion (`bezier::insert_all`).
//! 2. **Lines first** (curve: its breakpoints; surface: the four boundary curves and every
//!    interior knot line of multiplicity `p`, where the surface may have a crease): every
//!    breakpoint is a candidate, and every line is searched by the one-parameter version of
//!    step 3. A line whose control points all coincide (a pole) is one candidate at its start.
//! 3. **Branch and bound** over the Bézier pieces, in `(lower bound, sequence number)` order.
//!    The lower bound is the distance from the query to the box of the projected control
//!    points (REQ-005: a node is dropped only when this bound, less `PRUNE_REL * M`, exceeds
//!    the best distance plus the tie tolerance). A node is **excluded** when a Bernstein
//!    polynomial `G = w^3 S_x . (S - q)` of one parameter has all coefficients of one strict
//!    sign beyond the rounding margin (no stationary point in the closed node); it is
//!    **unique** when the symmetric part of the Jacobian of `(G_s, G_t)` is positive definite on
//!    the whole node by Bernstein bounds (at most one stationary point, a minimum; for curves:
//!    `G` strictly increasing), and then a box-constrained Newton iteration seeded at the
//!    node's closest control point finds it; otherwise the node is halved (along its longer
//!    direction) until `MAX_DEPTH` or `BUDGET`, after which it is unresolved and only its seed
//!    point is a candidate. The global minimum is either a breakpoint, a line point or an
//!    interior stationary point, so it is never discarded (no silent local minima).
//! 4. **Selection**: the smallest distance; among candidates within [`TIE`] of it, the
//!    lexicographically smallest parameters (REQ-004).
//! 5. **Certification** (REQ-002, REQ-003): orthogonality `|S_x . r| <= |S_x| T` with
//!    `T = max(1e-12 max(d, 1e-9 m), NOISE * M)`, or the domain boundary with the gradient
//!    pointing out of the domain, or a crease or kink parameter, or a pole. A direction where
//!    `|S_x|` vanishes in the interior (a cusp) cannot be certified. An uncertified winner is
//!    `NonConvergence { best }`.
//!
//! Determinism (D0, KNP-1): binary64 `+ - * /` written out one operation at a time, square
//! roots through `fid_num::f::sqrt`, comparisons instead of `f64::min`/`max`, no hash maps, a
//! binary heap keyed by bit patterns and sequence numbers, every loop bounded. The bit
//! patterns are reproduced by `fixtures/geo/nurbs/invert/invert_model.py`.

use core::cmp::Reverse;
use core::fmt;
use std::collections::BinaryHeap;

use crate::analytic::Uniqueness;
use crate::bezier::{bez1, bez1_pos, bez2, bez2_pos, g_poly, insert_all, segments, Arr, Binom, Net, H};
use crate::data::{NurbsCurve, NurbsSurface};
use crate::eval::{Curve, EvalError, Surface};

/// REQ-003: Newton iterations per run.
pub const MAX_ITER: usize = 50;
/// Step halvings per Newton iteration.
pub const MAX_HALVINGS: usize = 30;
/// REQ-004: candidates within this distance (metres) of the best are tied.
pub const TIE: f64 = 1e-15;
/// REQ-002: relative orthogonality tolerance.
pub const REL: f64 = 1e-12;
/// REQ-002: distance floor in the orthogonality tolerance, metres.
pub const FLOOR: f64 = 1e-9;
/// Evaluation-noise floor of the orthogonality tolerance, times the coordinate scale `M`
/// (2^-46).
pub const NOISE: f64 = 1.0 / 70_368_744_177_664.0;
/// Singular direction: `|S_x|^2 W_x^2 <= SING2 L^2` (2^-52).
pub const SING2: f64 = 1.0 / 4_503_599_627_370_496.0;
/// Bernstein sign margin, times the magnitude polynomial (2^-36).
pub const GAMMA: f64 = 1.0 / 68_719_476_736.0;
/// Box lower bound safety, times `M` (2^-48).
pub const PRUNE_REL: f64 = 1.0 / 281_474_976_710_656.0;
/// A line is degenerate (a pole) when all its control points agree within this times the
/// largest extent of the control net (2^-40).
pub const DEGEN_REL: f64 = 1.0 / 1_099_511_627_776.0;
/// Tied feet further apart than this (metres, infinity norm) make the result non-unique.
pub const FOOT_SEP: f64 = 1e-9;
/// Binary subdivisions below a Bézier piece.
pub const MAX_DEPTH: usize = 60;
/// Subdivisions per inversion.
pub const BUDGET: usize = 1024;

/// Closest point on a surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Inversion {
    pub u: f64,
    pub v: f64,
    /// `S(u, v)` from the GEO.NURBS.EVAL evaluator.
    pub foot: [f64; 3],
    pub distance: f64,
    /// A parameter equals a limit of its domain.
    pub on_boundary: bool,
    pub uniqueness: Uniqueness,
}

/// Closest point on a curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveInversion {
    pub t: f64,
    pub foot: [f64; 3],
    pub distance: f64,
    pub on_boundary: bool,
    pub uniqueness: Uniqueness,
}

/// Why an inversion has no certified answer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InvertError<I> {
    /// REQ-003: the best candidate does not meet the convergence criterion (for example a cusp,
    /// where the tangent vanishes); `best` is that candidate, finite.
    NonConvergence { best: I },
    /// The query point has a NaN or infinite coordinate.
    NonFiniteQuery,
    /// An intermediate overflowed (extreme coordinates or weights).
    NonFinite,
    /// The evaluator refused a parameter (unreachable for validated input).
    Eval(EvalError),
}

impl<I> InvertError<I> {
    /// Stable variant name.
    pub fn code(&self) -> &'static str {
        match self {
            InvertError::NonConvergence { .. } => "NonConvergence",
            InvertError::NonFiniteQuery => "NonFiniteQuery",
            InvertError::NonFinite => "NonFinite",
            InvertError::Eval(_) => "Eval",
        }
    }
}

impl<I> fmt::Display for InvertError<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InvertError::NonConvergence { .. } => write!(f, "point inversion did not converge"),
            InvertError::NonFiniteQuery => write!(f, "query point is not finite"),
            InvertError::NonFinite => write!(f, "non-finite intermediate in point inversion"),
            InvertError::Eval(e) => write!(f, "evaluation failed: {e}"),
        }
    }
}

/// Internal error: converted to `InvertError<I>` at the API boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Fail {
    NonFinite,
    Eval(EvalError),
}

impl From<EvalError> for Fail {
    fn from(e: EvalError) -> Self {
        Fail::Eval(e)
    }
}

impl<I> From<Fail> for InvertError<I> {
    fn from(e: Fail) -> Self {
        match e {
            Fail::NonFinite => InvertError::NonFinite,
            Fail::Eval(e) => InvertError::Eval(e),
        }
    }
}

// ---------------------------------------------------------------- small helpers

fn sqrt(x: f64) -> Result<f64, Fail> {
    fid_num::f::sqrt(x).map_err(|_| Fail::NonFinite)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `a + f (b - a)`, exactly `a` at `f = 0` and `b` at `f = 1`, clamped to `[a, b]`.
fn lerp(a: f64, b: f64, f: f64) -> f64 {
    if f == 0.0 {
        return a;
    }
    if f == 1.0 {
        return b;
    }
    let x = a + f * (b - a);
    if x < a {
        a
    } else if x > b {
        b
    } else {
        x
    }
}

/// `b` if `b > a`, else `a` (no `f64::max`).
fn fmax(a: f64, b: f64) -> f64 {
    if b > a {
        b
    } else {
        a
    }
}

fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

fn inf_dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    let mut m = 0.0;
    for (x, y) in a.iter().zip(&b) {
        let d = (x - y).abs();
        if d > m {
            m = d;
        }
    }
    m
}

/// Squared orthogonality tolerance `e T^2`, `T = max(REL max(d, FLOOR), NOISE M)`.
fn ortho_tol2(e: f64, dist: f64, m: f64) -> f64 {
    let inner = REL * fmax(dist, FLOOR);
    let t = fmax(inner, NOISE * m);
    e * (t * t)
}

/// Rounding noise of `|S - q|^2` near a foot: `(NOISE M) (2 d + NOISE M)`.
fn slack(dist: f64, m: f64) -> f64 {
    let n = NOISE * m;
    n * (2.0 * dist + n)
}

/// One direction of the box (or domain) Karush-Kuhn-Tucker test.
fn dir_ok(x: f64, lo: f64, hi: f64, g: f64, tol2: f64) -> bool {
    if x == lo && g >= 0.0 {
        return true;
    }
    if x == hi && g <= 0.0 {
        return true;
    }
    g * g <= tol2
}

/// Final certification of one direction against the whole domain (singular interior
/// directions fail).
#[allow(clippy::too_many_arguments)]
fn final_dir_ok(x: f64, lo: f64, hi: f64, g: f64, e: f64, dist: f64, m: f64, l2: f64) -> bool {
    if x == lo && g >= 0.0 {
        return true;
    }
    if x == hi && g <= 0.0 {
        return true;
    }
    if x != lo && x != hi {
        let w = hi - lo;
        if e * (w * w) <= SING2 * l2 {
            return false;
        }
    }
    g * g <= ortho_tol2(e, dist, m)
}

/// `(M, L^2, X)`: largest absolute coordinate of the control points and the query, squared
/// diagonal and largest extent of the control points' box.
fn scale(ctrl: &[[f64; 3]], q: [f64; 3]) -> (f64, f64, f64) {
    let mut m = 0.0;
    let first = ctrl.first().copied().unwrap_or([0.0; 3]);
    let (mut lo, mut hi) = (first, first);
    for c in ctrl {
        for (k, &ck) in c.iter().enumerate() {
            if ck.abs() > m {
                m = ck.abs();
            }
            if ck < lo[k] {
                lo[k] = ck;
            }
            if ck > hi[k] {
                hi[k] = ck;
            }
        }
    }
    for x in q {
        if x.abs() > m {
            m = x.abs();
        }
    }
    let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    let mut x = 0.0;
    for e in ext {
        if e > x {
            x = e;
        }
    }
    (m, dot(ext, ext), x)
}

fn homogeneous(ctrl: &[[f64; 3]], weights: Option<&[f64]>) -> Vec<H> {
    match weights {
        None => ctrl.iter().map(|c| [c[0], c[1], c[2], 1.0]).collect(),
        Some(w) => ctrl.iter().zip(w).map(|(c, w)| [c[0] * w, c[1] * w, c[2] * w, *w]).collect(),
    }
}

/// Distinct domain values that are interior knots of multiplicity at least `p` (kinks of a
/// curve, crease lines of a surface).
fn crease_values(knots: &[f64], p: usize, values: &[f64]) -> Vec<f64> {
    let n = values.len();
    values
        .iter()
        .enumerate()
        .filter(|(i, x)| *i > 0 && *i + 1 < n && knots.iter().filter(|y| *y == *x).count() >= p)
        .map(|(_, x)| *x)
        .collect()
}

// ---------------------------------------------------------------- search state

#[derive(Debug, Clone, Copy)]
struct Cand {
    params: [f64; 2],
    foot: [f64; 3],
    dist: f64,
    /// The single candidate of a degenerate line (a pole).
    pole: bool,
}

struct Ctx {
    q: [f64; 3],
    m: f64,
    bn: Binom,
    best: f64,
    cands: Vec<Cand>,
    splits: usize,
    unresolved: Vec<(f64, Vec<[f64; 3]>)>,
    seq: u64,
}

impl Ctx {
    fn add(&mut self, params: [f64; 2], foot: [f64; 3]) -> Result<(), Fail> {
        self.add_kind(params, foot, false)
    }

    fn add_kind(&mut self, params: [f64; 2], foot: [f64; 3], pole: bool) -> Result<(), Fail> {
        let r = sub3(foot, self.q);
        let d = sqrt(dot(r, r))?;
        self.cands.push(Cand { params, foot, dist: d, pole });
        if d < self.best {
            self.best = d;
        }
        Ok(())
    }
}

fn params_lt(a: [f64; 2], b: [f64; 2]) -> bool {
    a[0] < b[0] || (a[0] == b[0] && a[1] < b[1])
}

/// REQ-004: smallest distance, then the lexicographically smallest parameters within [`TIE`];
/// a pole tied with that winner at the same foot (within [`FOOT_SEP`]) replaces it, so a pole
/// is always reported at its canonical parameters.
fn select(ctx: &Ctx) -> Option<Cand> {
    let mut dmin = f64::INFINITY;
    for c in &ctx.cands {
        if c.dist < dmin {
            dmin = c.dist;
        }
    }
    let mut w: Option<Cand> = None;
    for c in &ctx.cands {
        if c.dist <= dmin + TIE {
            match w {
                Some(b) if !params_lt(c.params, b.params) => {}
                _ => w = Some(*c),
            }
        }
    }
    let win = w?;
    for c in &ctx.cands {
        if c.pole && c.dist <= dmin + TIE && inf_dist(c.foot, win.foot) <= FOOT_SEP {
            return Some(*c);
        }
    }
    Some(win)
}

/// Ties with a distinct foot, or an unresolved region that may hold one.
fn nonunique(ctx: &Ctx, w: &Cand) -> bool {
    let dmin = w.dist;
    if ctx.cands.iter().any(|c| c.dist <= dmin + TIE && inf_dist(c.foot, w.foot) > FOOT_SEP) {
        return true;
    }
    ctx.unresolved.iter().any(|(lb, pts)| *lb <= dmin + TIE && pts.iter().any(|x| inf_dist(*x, w.foot) > FOOT_SEP))
}

// ---------------------------------------------------------------- Newton (box constrained)

/// One-parameter Newton on a one-column Bézier net in `[lo, hi]` from `x0`. After the box
/// criterion first holds, one further (polishing) step is taken.
fn newton1(net: &Net, lo: f64, hi: f64, x0: f64, ctx: &Ctx) -> Result<f64, Fail> {
    let (q, m) = (ctx.q, ctx.m);
    let mut x = x0;
    let mut it = 0;
    let mut polished = false;
    loop {
        let (p, d1, d2) = bez1(net, x);
        let r = sub3(p, q);
        let f = dot(r, r);
        let g = dot(d1, r);
        let e = dot(d1, d1);
        let dist = sqrt(f)?;
        if dir_ok(x, lo, hi, g, ortho_tol2(e, dist, m)) {
            if polished {
                return Ok(x);
            }
            polished = true;
        }
        if it == MAX_ITER {
            return Ok(x);
        }
        it += 1;
        let h = e + dot(d2, r);
        let step = if h > 0.0 {
            -g / h
        } else if e > 0.0 {
            -g / e
        } else {
            return Ok(x);
        };
        let mut lam = 1.0;
        let fcap = f + slack(dist, m);
        let mut next = None;
        for _ in 0..=MAX_HALVINGS {
            let xt = clamp(x + lam * step, lo, hi);
            if xt == x {
                return Ok(x);
            }
            let rt = sub3(bez1_pos(net, xt), q);
            if dot(rt, rt) <= fcap {
                next = Some(xt);
                break;
            }
            lam *= 0.5;
        }
        match next {
            Some(xt) => x = xt,
            None => return Ok(x),
        }
    }
}

/// Two-parameter Newton on a tensor Bézier net in the box `[lo, hi]` from `x0`: the full
/// Hessian when positive definite, else the first fundamental form, else steepest descent;
/// active box bounds fix their parameter.
fn newton2(net: &Net, lo: [f64; 2], hi: [f64; 2], x0: [f64; 2], ctx: &Ctx) -> Result<[f64; 2], Fail> {
    let (q, m) = (ctx.q, ctx.m);
    let [mut u, mut v] = x0;
    let mut it = 0;
    let mut polished = false;
    loop {
        let (p, su, sv, suu, suv, svv) = bez2(net, u, v);
        let r = sub3(p, q);
        let f = dot(r, r);
        let gu = dot(su, r);
        let gv = dot(sv, r);
        let ee = dot(su, su);
        let ff = dot(su, sv);
        let gg = dot(sv, sv);
        let dist = sqrt(f)?;
        let oku = dir_ok(u, lo[0], hi[0], gu, ortho_tol2(ee, dist, m));
        let okv = dir_ok(v, lo[1], hi[1], gv, ortho_tol2(gg, dist, m));
        if oku && okv {
            if polished {
                return Ok([u, v]);
            }
            polished = true;
        }
        if it == MAX_ITER {
            return Ok([u, v]);
        }
        it += 1;
        let huu = ee + dot(suu, r);
        let huv = ff + dot(suv, r);
        let hvv = gg + dot(svv, r);
        let au = (u == lo[0] && gu > 0.0) || (u == hi[0] && gu < 0.0);
        let av = (v == lo[1] && gv > 0.0) || (v == hi[1] && gv < 0.0);
        let (du, dv) = if !au && !av {
            let det = huu * hvv - huv * huv;
            let det_i = ee * gg - ff * ff;
            if huu > 0.0 && det > 0.0 {
                ((huv * gv - hvv * gu) / det, (huv * gu - huu * gv) / det)
            } else if ee > 0.0 && det_i > 0.0 {
                ((ff * gv - gg * gu) / det_i, (ff * gu - ee * gv) / det_i)
            } else {
                let s = ee + gg;
                if s > 0.0 {
                    (-gu / s, -gv / s)
                } else {
                    return Ok([u, v]);
                }
            }
        } else if !au {
            if huu > 0.0 {
                (-gu / huu, 0.0)
            } else if ee > 0.0 {
                (-gu / ee, 0.0)
            } else {
                return Ok([u, v]);
            }
        } else if !av {
            if hvv > 0.0 {
                (0.0, -gv / hvv)
            } else if gg > 0.0 {
                (0.0, -gv / gg)
            } else {
                return Ok([u, v]);
            }
        } else {
            return Ok([u, v]);
        };
        let mut lam = 1.0;
        let fcap = f + slack(dist, m);
        let mut next = None;
        for _ in 0..=MAX_HALVINGS {
            let ut = clamp(u + lam * du, lo[0], hi[0]);
            let vt = clamp(v + lam * dv, lo[1], hi[1]);
            if ut == u && vt == v {
                return Ok([u, v]);
            }
            let rt = sub3(bez2_pos(net, ut, vt), q);
            if dot(rt, rt) <= fcap {
                next = Some((ut, vt));
                break;
            }
            lam *= 0.5;
        }
        match next {
            Some((ut, vt)) => {
                u = ut;
                v = vt;
            }
            None => return Ok([u, v]),
        }
    }
}

// ---------------------------------------------------------------- Bernstein tests

/// All coefficients strictly positive, or all strictly negative, beyond `GAMMA * Gm`.
fn signed(g: &Arr, gm: &Arr) -> bool {
    let mut pos = true;
    let mut neg = true;
    for (x, xm) in g.a.iter().zip(&gm.a) {
        let t = GAMMA * xm;
        // G is finite (checked by the caller), so `<=` is the negation of `>`
        if *x <= t {
            pos = false;
        }
        if *x >= -t {
            neg = false;
        }
    }
    pos || neg
}

/// `+1` strictly increasing, `-1` strictly decreasing, `0` otherwise (one-column `G`).
fn monotone_1d(g: &Arr, gm: &Arr) -> i32 {
    let mut inc = true;
    let mut dec = true;
    for k in 0..g.m {
        let d = g.at(k + 1, 0) - g.at(k, 0);
        let t = GAMMA * (gm.at(k + 1, 0) + gm.at(k, 0));
        if d <= t {
            inc = false;
        }
        if d >= -t {
            dec = false;
        }
    }
    if inc {
        1
    } else if dec {
        -1
    } else {
        0
    }
}

/// The symmetric part of the Jacobian of `(G_s, G_t)` is positive definite on the node:
/// `a > 0`, `d > 0` and `a d > b^2` with `a`, `d` lower bounds of the diagonal derivatives and
/// `b` an upper bound of the mean of the off-diagonal ones.
fn unique_2d(gu: &Arr, gum: &Arr, gv: &Arr, gvm: &Arr) -> bool {
    let (m1, n1, m2, n2) = (gu.m, gu.n, gv.m, gv.n);
    let mut a = f64::INFINITY;
    for k in 0..m1 {
        for l in 0..=n1 {
            let x = (gu.at(k + 1, l) - gu.at(k, l)) - GAMMA * (gum.at(k + 1, l) + gum.at(k, l));
            if x < a {
                a = x;
            }
        }
    }
    let a = m1 as f64 * a;
    let mut d = f64::INFINITY;
    for k in 0..=m2 {
        for l in 0..n2 {
            let x = (gv.at(k, l + 1) - gv.at(k, l)) - GAMMA * (gvm.at(k, l + 1) + gvm.at(k, l));
            if x < d {
                d = x;
            }
        }
    }
    let d = n2 as f64 * d;
    let mut b1 = 0.0;
    for k in 0..=m1 {
        for l in 0..n1 {
            let x = (gu.at(k, l + 1) - gu.at(k, l)).abs() + GAMMA * (gum.at(k, l + 1) + gum.at(k, l));
            if x > b1 {
                b1 = x;
            }
        }
    }
    let b1 = n1 as f64 * b1;
    let mut b2 = 0.0;
    for k in 0..m2 {
        for l in 0..=n2 {
            let x = (gv.at(k + 1, l) - gv.at(k, l)).abs() + GAMMA * (gvm.at(k + 1, l) + gvm.at(k, l));
            if x > b2 {
                b2 = x;
            }
        }
    }
    let b2 = m2 as f64 * b2;
    let b = (b1 + b2) * 0.5;
    a > 0.0 && d > 0.0 && a * d > b * b
}

// ---------------------------------------------------------------- branch and bound

struct Root {
    net: Net,
    x: (f64, f64),
    y: (f64, f64),
}

struct Node {
    net: Net,
    root: usize,
    s0: f64,
    s1: f64,
    t0: f64,
    t1: f64,
    depth: usize,
    lb: f64,
    si: usize,
    sj: usize,
}

fn projected_all(net: &Net) -> Vec<[f64; 3]> {
    let mut out = Vec::with_capacity((net.m() + 1) * (net.n() + 1));
    for i in 0..=net.m() {
        for j in 0..=net.n() {
            out.push(net.projected(i, j));
        }
    }
    out
}

/// Box lower bound and the closest control point (first in row-major order).
fn box_lb_seed(net: &Net, q: [f64; 3]) -> Result<(f64, usize, usize), Fail> {
    let first = net.projected(0, 0);
    let (mut lo, mut hi) = (first, first);
    let mut best = f64::INFINITY;
    let mut have = false;
    let (mut bi, mut bj) = (0, 0);
    for i in 0..=net.m() {
        for j in 0..=net.n() {
            let x = net.projected(i, j);
            for (c, &xc) in x.iter().enumerate() {
                if xc < lo[c] {
                    lo[c] = xc;
                }
                if xc > hi[c] {
                    hi[c] = xc;
                }
            }
            let r = sub3(x, q);
            let d2 = dot(r, r);
            if !have || d2 < best {
                have = true;
                best = d2;
                bi = i;
                bj = j;
            }
        }
    }
    let mut dl = [0.0; 3];
    for (c, d) in dl.iter_mut().enumerate() {
        if q[c] < lo[c] {
            *d = lo[c] - q[c];
        } else if q[c] > hi[c] {
            *d = q[c] - hi[c];
        }
    }
    let lb = sqrt(dot(dl, dl))?;
    if !lb.is_finite() {
        return Err(Fail::NonFinite);
    }
    Ok((lb, bi, bj))
}

#[allow(clippy::too_many_arguments)]
fn make_node(net: Net, root: usize, s: (f64, f64), t: (f64, f64), depth: usize, ctx: &Ctx) -> Result<Node, Fail> {
    let (lb, si, sj) = box_lb_seed(&net, ctx.q)?;
    Ok(Node { net, root, s0: s.0, s1: s.1, t0: t.0, t1: t.1, depth, lb, si, sj })
}

/// True when the projected net extends further along `t` than along `s` (largest control
/// point difference, infinity norm); ties split along `s`.
fn split_dir_t(net: &Net) -> bool {
    let p = projected_all(net);
    let (m, n) = (net.m(), net.n());
    let at = |i: usize, j: usize| p[i * (n + 1) + j];
    let (mut ds, mut dt) = (0.0, 0.0);
    for i in 0..=m {
        for j in 0..=n {
            let here = at(i, j);
            let below = if i < m { Some(at(i + 1, j)) } else { None };
            let right = if j < n { Some(at(i, j + 1)) } else { None };
            for (c, &h) in here.iter().enumerate() {
                if let Some(b) = below {
                    let x = (b[c] - h).abs();
                    if x > ds {
                        ds = x;
                    }
                }
                if let Some(r) = right {
                    let x = (r[c] - h).abs();
                    if x > dt {
                        dt = x;
                    }
                }
            }
        }
    }
    dt > ds
}

struct Heap {
    heap: BinaryHeap<Reverse<(u64, u64, usize)>>,
    arena: Vec<Option<Node>>,
}

impl Heap {
    fn push(&mut self, node: Node, ctx: &mut Ctx) {
        let key = node.lb.to_bits();
        self.arena.push(Some(node));
        self.heap.push(Reverse((key, ctx.seq, self.arena.len() - 1)));
        ctx.seq += 1;
    }

    fn pop(&mut self) -> Option<Node> {
        let Reverse((_, _, idx)) = self.heap.pop()?;
        self.arena.get_mut(idx).and_then(Option::take)
    }
}

/// Branch and bound over `roots` (one-column nets when `two` is false). `point` evaluates the
/// GEO.NURBS.EVAL position at candidate parameters; `lift` maps a line parameter to them.
fn bnb(
    roots: &[Root],
    two: bool,
    ctx: &mut Ctx,
    point: &dyn Fn([f64; 2]) -> Result<[f64; 3], EvalError>,
    lift: &dyn Fn(f64) -> [f64; 2],
) -> Result<(), Fail> {
    let mut heap = Heap { heap: BinaryHeap::new(), arena: Vec::new() };
    for (k, r) in roots.iter().enumerate() {
        let nd = make_node(r.net.clone(), k, (0.0, 1.0), (0.0, 1.0), 0, ctx)?;
        heap.push(nd, ctx);
    }
    while let Some(nd) = heap.pop() {
        if nd.lb - PRUNE_REL * ctx.m > ctx.best + TIE {
            break;
        }
        let Some(root) = roots.get(nd.root) else { return Err(Fail::NonFinite) };
        let unique = if two {
            let (gu, gum) = g_poly(&nd.net, ctx.q, false, &ctx.bn);
            let (gv, gvm) = g_poly(&nd.net, ctx.q, true, &ctx.bn);
            if !(gu.all_finite() && gum.all_finite() && gv.all_finite() && gvm.all_finite()) {
                return Err(Fail::NonFinite);
            }
            if signed(&gu, &gum) || signed(&gv, &gvm) {
                continue;
            }
            unique_2d(&gu, &gum, &gv, &gvm)
        } else {
            let (g, gm) = g_poly(&nd.net, ctx.q, false, &ctx.bn);
            if !(g.all_finite() && gm.all_finite()) {
                return Err(Fail::NonFinite);
            }
            if signed(&g, &gm) {
                continue;
            }
            match monotone_1d(&g, &gm) {
                -1 => continue,
                mono => mono == 1,
            }
        };
        let mut ss = lerp(nd.s0, nd.s1, nd.si as f64 / nd.net.m() as f64);
        let mut st = if two { lerp(nd.t0, nd.t1, nd.sj as f64 / nd.net.n() as f64) } else { 0.0 };
        if unique {
            if two {
                [ss, st] = newton2(&root.net, [nd.s0, nd.t0], [nd.s1, nd.t1], [ss, st], ctx)?;
            } else {
                ss = newton1(&root.net, nd.s0, nd.s1, ss, ctx)?;
            }
        } else if nd.depth < MAX_DEPTH && ctx.splits < BUDGET {
            ctx.splits += 1;
            push_children(&mut heap, nd, two, ctx)?;
            continue;
        } else {
            ctx.unresolved.push((nd.lb, projected_all(&nd.net)));
        }
        let params = if two {
            [lerp(root.x.0, root.x.1, ss), lerp(root.y.0, root.y.1, st)]
        } else {
            lift(lerp(root.x.0, root.x.1, ss))
        };
        let foot = point(params)?;
        ctx.add(params, foot)?;
    }
    Ok(())
}

fn push_children(heap: &mut Heap, nd: Node, two: bool, ctx: &mut Ctx) -> Result<(), Fail> {
    let along_t = two && split_dir_t(&nd.net);
    let (l, r, boxes) = if along_t {
        let tm = (nd.t0 + nd.t1) * 0.5;
        let (l, r) = nd.net.split_t();
        (l, r, [((nd.s0, nd.s1), (nd.t0, tm)), ((nd.s0, nd.s1), (tm, nd.t1))])
    } else {
        let sm = (nd.s0 + nd.s1) * 0.5;
        let (l, r) = nd.net.split_s();
        (l, r, [((nd.s0, sm), (nd.t0, nd.t1)), ((sm, nd.s1), (nd.t0, nd.t1))])
    };
    let kid = make_node(l, nd.root, boxes[0].0, boxes[0].1, nd.depth + 1, ctx)?;
    heap.push(kid, ctx);
    let kid = make_node(r, nd.root, boxes[1].0, boxes[1].1, nd.depth + 1, ctx)?;
    heap.push(kid, ctx);
    Ok(())
}

fn degenerate_line(nets: &[Net], x: f64) -> bool {
    let mut p0: Option<[f64; 3]> = None;
    for net in nets {
        for p in projected_all(net) {
            let base = *p0.get_or_insert(p);
            if p.iter().zip(&base).any(|(a, b)| (a - b).abs() > DEGEN_REL * x) {
                return false;
            }
        }
    }
    true
}

fn check_query(q: [f64; 3]) -> bool {
    q.iter().all(|x| x.is_finite())
}

// ---------------------------------------------------------------- curves

/// Globally closest point on `c` to `q` (REQ-GEO.NURBS.INVERT-001 for curves).
pub fn invert_curve(c: &NurbsCurve, q: [f64; 3]) -> Result<CurveInversion, InvertError<CurveInversion>> {
    if !check_query(q) {
        return Err(InvertError::NonFiniteQuery);
    }
    let (res, cert) = curve_search(c, q)?;
    if cert {
        Ok(res)
    } else {
        Err(InvertError::NonConvergence { best: res })
    }
}

fn curve_search(c: &NurbsCurve, q: [f64; 3]) -> Result<(CurveInversion, bool), Fail> {
    let (m, l2, x) = scale(c.ctrl(), q);
    let mut ctx = Ctx {
        q,
        m,
        bn: Binom::new(),
        best: f64::INFINITY,
        cands: Vec::new(),
        splits: 0,
        unresolved: Vec::new(),
        seq: 0,
    };
    let p = c.degree();
    let knots = c.knots().as_slice();
    let mut polys = vec![homogeneous(c.ctrl(), c.weights())];
    let (u2, values) = insert_all(knots, p, &mut polys);
    let poly = polys.pop().unwrap_or_default();
    let segs = segments(&u2, p, &values);
    let (lo, hi) = (values.first().copied().unwrap_or(0.0), values.last().copied().unwrap_or(0.0));
    let nets: Vec<Net> =
        segs.iter().map(|(s, _, _)| Net::from_points(p, 0, poly.get(*s..*s + p + 1).unwrap_or(&[]))).collect();
    if nets.iter().any(|n| n.c[0].a.len() != p + 1) {
        return Err(Fail::NonFinite);
    }
    let pos = |t: f64| -> Result<[f64; 3], EvalError> { Ok(c.eval(t, 0)?.p) };
    let degenerate = degenerate_line(&nets, x);
    if degenerate {
        ctx.add_kind([lo, 0.0], pos(lo)?, true)?;
    } else {
        for &t in &values {
            ctx.add([t, 0.0], pos(t)?)?;
        }
        let roots: Vec<Root> =
            nets.into_iter().zip(&segs).map(|(net, (_, x0, x1))| Root { net, x: (*x0, *x1), y: (0.0, 0.0) }).collect();
        bnb(&roots, false, &mut ctx, &|pr| pos(pr[0]), &|t| [t, 0.0])?;
    }
    let Some(w) = select(&ctx) else { return Err(Fail::NonFinite) };
    let t = w.params[0];
    let unique = !nonunique(&ctx, &w) && !degenerate;
    let cert = if degenerate || crease_values(knots, p, &values).contains(&t) {
        true
    } else {
        let d1 = c.eval(t, 1)?.d1.unwrap_or([0.0; 3]);
        let r = sub3(w.foot, q);
        final_dir_ok(t, lo, hi, dot(d1, r), dot(d1, d1), w.dist, m, l2)
    };
    let res = CurveInversion {
        t,
        foot: w.foot,
        distance: w.dist,
        on_boundary: t == lo || t == hi,
        uniqueness: if unique { Uniqueness::Unique } else { Uniqueness::NonUnique },
    };
    Ok((res, cert))
}

// ---------------------------------------------------------------- surfaces

/// Globally closest point on the untrimmed surface `s` to `q`, including its boundary
/// (REQ-GEO.NURBS.INVERT-001).
pub fn invert_surface(s: &NurbsSurface, q: [f64; 3]) -> Result<Inversion, InvertError<Inversion>> {
    if !check_query(q) {
        return Err(InvertError::NonFiniteQuery);
    }
    let (res, cert) = surface_search(s, q)?;
    if cert {
        Ok(res)
    } else {
        Err(InvertError::NonConvergence { best: res })
    }
}

fn surface_search(s: &NurbsSurface, q: [f64; 3]) -> Result<(Inversion, bool), Fail> {
    let (m, l2, x_ext) = scale(s.ctrl(), q);
    let mut ctx = Ctx {
        q,
        m,
        bn: Binom::new(),
        best: f64::INFINITY,
        cands: Vec::new(),
        splits: 0,
        unresolved: Vec::new(),
        seq: 0,
    };
    let (p, qd) = (s.degree_u(), s.degree_v());
    let (nu, nv) = s.shape();
    let pw = homogeneous(s.ctrl(), s.weights());
    // u insertion on every column of the net, then v insertion on every row
    let mut cols: Vec<Vec<H>> =
        (0..nv).map(|j| (0..nu).map(|i| pw.get(i * nv + j).copied().unwrap_or([0.0; 4])).collect()).collect();
    let (u2, uvals) = insert_all(s.knots_u().as_slice(), p, &mut cols);
    let nu2 = cols.first().map_or(0, Vec::len);
    let mut rows: Vec<Vec<H>> =
        (0..nu2).map(|i| cols.iter().map(|c| c.get(i).copied().unwrap_or([0.0; 4])).collect()).collect();
    let (v2, vvals) = insert_all(s.knots_v().as_slice(), qd, &mut rows);
    let useg = segments(&u2, p, &uvals);
    let vseg = segments(&v2, qd, &vvals);
    let mut patches: Vec<Root> = Vec::with_capacity(useg.len() * vseg.len());
    for &(iu, u0, u1) in &useg {
        for &(iv, v0, v1) in &vseg {
            let mut pts = Vec::with_capacity((p + 1) * (qd + 1));
            for i in 0..=p {
                for j in 0..=qd {
                    let pt = rows.get(iu + i).and_then(|r| r.get(iv + j)).copied().ok_or(Fail::NonFinite)?;
                    pts.push(pt);
                }
            }
            patches.push(Root { net: Net::from_points(p, qd, &pts), x: (u0, u1), y: (v0, v1) });
        }
    }
    let nvs = vseg.len();
    let first_last = |v: &[f64]| (v.first().copied().unwrap_or(0.0), v.last().copied().unwrap_or(0.0));
    let ((ulo, uhi), (vlo, vhi)) = (first_last(&uvals), first_last(&vvals));
    let ucre = crease_values(s.knots_u().as_slice(), p, &uvals);
    let vcre = crease_values(s.knots_v().as_slice(), qd, &vvals);
    let pos = |pr: [f64; 2]| -> Result<[f64; 3], EvalError> { Ok(s.eval(pr[0], pr[1], 0)?.p) };
    let mut degen_u: Vec<f64> = Vec::new();
    let mut degen_v: Vec<f64> = Vec::new();
    // lines: u fixed (start, creases, end), then v fixed
    for along_v in [true, false] {
        let (vals, own, own_seg, other_vals, other_seg) = if along_v {
            ([vec![ulo], ucre.clone(), vec![uhi]].concat(), &uvals, &useg, &vvals, &vseg)
        } else {
            ([vec![vlo], vcre.clone(), vec![vhi]].concat(), &vvals, &vseg, &uvals, &useg)
        };
        for x in vals {
            let k = own.iter().position(|y| *y == x).ok_or(Fail::NonFinite)?;
            let (kk, edge) = if k < own_seg.len() { (k, 0) } else { (k - 1, if along_v { p } else { qd }) };
            let mut roots: Vec<Root> = Vec::with_capacity(other_seg.len());
            for (ko, &(_, a, b)) in other_seg.iter().enumerate() {
                let idx = if along_v { kk * nvs + ko } else { ko * nvs + kk };
                let net = &patches.get(idx).ok_or(Fail::NonFinite)?.net;
                let line = if along_v { net.row(edge) } else { net.col(edge) };
                roots.push(Root { net: line, x: (a, b), y: (0.0, 0.0) });
            }
            let lift = |t: f64| if along_v { [x, t] } else { [t, x] };
            let nets: Vec<Net> = roots.iter().map(|r| r.net.clone()).collect();
            if degenerate_line(&nets, x_ext) {
                if along_v {
                    degen_u.push(x);
                } else {
                    degen_v.push(x);
                }
                let pr = lift(other_vals.first().copied().unwrap_or(0.0));
                ctx.add_kind(pr, pos(pr)?, true)?;
                continue;
            }
            for &t in other_vals.iter() {
                let pr = lift(t);
                ctx.add(pr, pos(pr)?)?;
            }
            bnb(&roots, false, &mut ctx, &pos, &lift)?;
        }
    }
    bnb(&patches, true, &mut ctx, &pos, &|t| [t, 0.0])?;
    let Some(w) = select(&ctx) else { return Err(Fail::NonFinite) };
    let [u, v] = w.params;
    let on_degen = degen_u.contains(&u) || degen_v.contains(&v);
    let unique = !nonunique(&ctx, &w) && !on_degen;
    let d = s.eval(u, v, 1)?;
    let (su, sv) = (d.su.unwrap_or([0.0; 3]), d.sv.unwrap_or([0.0; 3]));
    let r = sub3(w.foot, q);
    let mut cert = true;
    for (x, lo, hi, d1, cre, skip) in
        [(u, ulo, uhi, su, &ucre, degen_v.contains(&v)), (v, vlo, vhi, sv, &vcre, degen_u.contains(&u))]
    {
        if skip || cre.contains(&x) {
            continue;
        }
        if !final_dir_ok(x, lo, hi, dot(d1, r), dot(d1, d1), w.dist, m, l2) {
            cert = false;
        }
    }
    let res = Inversion {
        u,
        v,
        foot: w.foot,
        distance: w.dist,
        on_boundary: u == ulo || u == uhi || v == vlo || v == vhi,
        uniqueness: if unique { Uniqueness::Unique } else { Uniqueness::NonUnique },
    };
    Ok((res, cert))
}
