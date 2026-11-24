//! Closest point on a bounded curve piece `C(t)`, `t` in `[t0, t1]`, given a Lipschitz bound
//! `L >= |C'|` (ellipse arcs, NURBS edge curves, and edges given only by a pcurve on the face
//! surface, `C = S o c`).
//!
//! 1. **Branch and bound** on `f(t) = |C(t) - q|`: [`PIECES`] equal pieces, then halving in
//!    `(lower bound, sequence number)` order. A piece of half-width `h` around its midpoint `m`
//!    has the lower bound `f(m) - L h - G` (`G = 2^-40 (|q| + |C(m)|)`, max norms, for the
//!    rounding of the evaluation); a piece is dropped once its bound exceeds the best value
//!    seen, and kept as a leaf once `L h <= 2^-12 L (t1 - t0)` (or after [`MAX_PIECES`]).
//!    Both ends are evaluated first.
//! 2. **Refinement.** Among the surviving leaves (ordered by `t`), every leaf whose midpoint
//!    value is not above its touching neighbours' seeds a safeguarded Newton iteration on
//!    `g(t) = (C - q) . C'` (bisection whenever the Newton step leaves the bracket of the
//!    neighbouring midpoints), at most [`MAX_SEEDS`] seeds in `(value, t)` order.
//! 3. The answer is the smallest distance found (earliest on a tie).

use core::cmp::Reverse;
use std::collections::BinaryHeap;

use super::{dot3, fmax, ord_key, sub3, FaceError};

/// Initial pieces.
pub const PIECES: usize = 64;
/// Most pieces examined.
pub const MAX_PIECES: usize = 1 << 14;
/// Leaf resolution relative to `L (t1 - t0)` (`2^-12`).
pub const RES_REL: f64 = 1.0 / 4096.0;
/// Evaluation guard relative to the coordinates (`2^-40`).
pub const GUARD_REL: f64 = 1.0 / 1_099_511_627_776.0;
/// Newton seeds refined.
pub const MAX_SEEDS: usize = 8;
/// Newton and bisection steps per seed.
pub const MAX_STEPS: usize = 200;

/// Position and first two derivatives at a parameter.
pub(super) type Eval3 = [[f64; 3]; 3];

/// The closest point found: parameter, foot, distance.
#[derive(Debug, Clone, Copy)]
pub(super) struct CurveFoot {
    pub t: f64,
    pub foot: [f64; 3],
    pub distance: f64,
}

#[derive(Debug, Clone, Copy)]
struct Piece {
    lo: f64,
    hi: f64,
    mid: f64,
    value: f64,
    lb: f64,
}

fn inf_norm(a: [f64; 3]) -> f64 {
    fmax(fmax(a[0].abs(), a[1].abs()), a[2].abs())
}

/// Closest point of `eval` on `[t0, t1]` to `q`; `lip` bounds `|C'|`.
pub(super) fn closest(
    eval: &dyn Fn(f64) -> Result<Eval3, FaceError>,
    lip: f64,
    t0: f64,
    t1: f64,
    q: [f64; 3],
) -> Result<CurveFoot, FaceError> {
    let point = |t: f64| -> Result<CurveFoot, FaceError> {
        let c = eval(t)?[0];
        Ok(CurveFoot { t, foot: c, distance: super::norm3(sub3(c, q))? })
    };
    let mut best = point(t0)?;
    let consider = |best: &mut CurveFoot, c: CurveFoot| {
        if c.distance < best.distance {
            *best = c;
        }
    };
    let len = t1 - t0;
    if len.is_nan() || len <= 0.0 || lip.is_nan() || lip <= 0.0 {
        return Ok(best);
    }
    consider(&mut best, point(t1)?);
    let res = RES_REL * (lip * len);
    let qn = inf_norm(q);
    let mut store: Vec<Piece> = Vec::new();
    let mut heap: BinaryHeap<Reverse<(u64, u64)>> = BinaryHeap::new();
    let make = |lo: f64, hi: f64, best: &mut CurveFoot| -> Result<Piece, FaceError> {
        let mid = lo + (hi - lo) * 0.5;
        let c = point(mid)?;
        let h = (hi - lo) * 0.5;
        let g = GUARD_REL * (qn + inf_norm(c.foot));
        let lb = (c.distance - lip * h) - g;
        if c.distance < best.distance {
            *best = c;
        }
        Ok(Piece { lo, hi, mid, value: c.distance, lb })
    };
    for k in 0..PIECES {
        let lo = if k == 0 { t0 } else { t0 + (len * k as f64) / PIECES as f64 };
        let hi = if k + 1 == PIECES { t1 } else { t0 + (len * (k + 1) as f64) / PIECES as f64 };
        let pc = make(lo, hi, &mut best)?;
        heap.push(Reverse((ord_key(pc.lb), store.len() as u64)));
        store.push(pc);
    }
    let mut leaves: Vec<Piece> = Vec::new();
    while let Some(Reverse((_, seq))) = heap.pop() {
        let Some(pc) = store.get(seq as usize).copied() else { continue };
        if pc.lb > best.distance {
            break;
        }
        let h = (pc.hi - pc.lo) * 0.5;
        if lip * h <= res || store.len() + 2 > MAX_PIECES || !(pc.lo < pc.mid && pc.mid < pc.hi) {
            leaves.push(pc);
            continue;
        }
        for (lo, hi) in [(pc.lo, pc.mid), (pc.mid, pc.hi)] {
            let ch = make(lo, hi, &mut best)?;
            heap.push(Reverse((ord_key(ch.lb), store.len() as u64)));
            store.push(ch);
        }
    }
    // Leaves that could still hold the minimum, in parameter order.
    leaves.retain(|pc| pc.lb <= best.distance);
    leaves.sort_by(|a, b| a.lo.total_cmp(&b.lo));
    let n = leaves.len();
    let mut seeds: Vec<(f64, f64, usize)> = Vec::new();
    for i in 0..n {
        let here = leaves[i];
        let left = if i > 0 && leaves[i - 1].hi == here.lo { Some(leaves[i - 1].value) } else { None };
        let right = if i + 1 < n && leaves[i + 1].lo == here.hi { Some(leaves[i + 1].value) } else { None };
        if left.map_or(true, |v| here.value <= v) && right.map_or(true, |v| here.value <= v) {
            seeds.push((here.value, here.mid, i));
        }
    }
    seeds.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    seeds.truncate(MAX_SEEDS);
    for &(_, _, i) in &seeds {
        let here = leaves[i];
        let a = if i > 0 && leaves[i - 1].hi == here.lo { leaves[i - 1].mid } else { here.lo };
        let b = if i + 1 < n && leaves[i + 1].lo == here.hi { leaves[i + 1].mid } else { here.hi };
        if let Some(t) = refine(eval, q, a, b, here.mid)? {
            consider(&mut best, point(t)?);
        }
    }
    Ok(best)
}

/// `g(t) = (C - q) . C'` and `g'(t) = C' . C' + (C - q) . C''`.
fn g_and_slope(e: &Eval3, q: [f64; 3]) -> (f64, f64) {
    let r = sub3(e[0], q);
    (dot3(r, e[1]), dot3(e[1], e[1]) + dot3(r, e[2]))
}

/// Safeguarded Newton iteration for a root of `g` in `[a, b]` with `g(a) < 0 < g(b)`;
/// `None` without such a bracket.
fn refine(
    eval: &dyn Fn(f64) -> Result<Eval3, FaceError>,
    q: [f64; 3],
    a: f64,
    b: f64,
    seed: f64,
) -> Result<Option<f64>, FaceError> {
    let (ga, _) = g_and_slope(&eval(a)?, q);
    let (gb, _) = g_and_slope(&eval(b)?, q);
    if !(ga < 0.0 && gb > 0.0) {
        return Ok(None);
    }
    let (mut lo, mut hi) = (a, b);
    let mut x = seed;
    for _ in 0..MAX_STEPS {
        let (g, slope) = g_and_slope(&eval(x)?, q);
        if g == 0.0 {
            break;
        }
        if g < 0.0 {
            lo = x;
        } else {
            hi = x;
        }
        let mut xn = if slope > 0.0 { x - g / slope } else { f64::NAN };
        if !(xn > lo && xn < hi) {
            xn = lo + (hi - lo) * 0.5;
        }
        if xn == x || !(xn > lo && xn < hi) {
            break;
        }
        x = xn;
    }
    Ok(Some(x))
}
