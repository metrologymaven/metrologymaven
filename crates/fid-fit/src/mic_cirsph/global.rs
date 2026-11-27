//! The global cross-check of a certified inscribed circle or sphere (module documentation of
//! [`super`], rule 10): a branch and bound over the trust region that either proves that no
//! centre in the region has an empty circle or sphere larger than the fit's radius plus a
//! tolerance, or finds one, or runs out of its work limits.
//!
//! # The bound
//!
//! `f(c) = min_i |q_i - c|` is the radius of the largest empty ball about `c`. It is
//! 1-Lipschitz, so over a box cell with centre `m` and half-diagonal `h`, `f <= f(m) + h`. The
//! search keeps a stack of cells (depth first, children in a fixed order), starting from the
//! axis-aligned box of half side `radius` about the region's centre, which contains the region.
//! With `E` a bound on the rounding error of a computed distance and `Es` of a computed facet
//! slack (both below), a cell is
//!
//! - **outside** when some facet slack `b_k - a_k . m` is below `-(h + Es)`: no point of the
//!   cell is in the region (the normals are unit length within rounding);
//! - **pruned** when `f(m) + h + E <= r + tol`: no centre in the cell beats the fit;
//! - **better** when `m` is inside the region by at least `Es` and `f(m) - E > r + tol`: a
//!   feasible centre with a larger empty ball exists, so the fit is not the global optimum;
//! - otherwise **split** into `2^dim` children of half the side.
//!
//! # Candidate lists
//!
//! For every `c` in a cell, the nearest point `p*` satisfies `|p* - m| <= |p* - c| + |c - m|
//! = f(c) + |c - m| <= f(m) + 2 |c - m| <= f(m) + 2 h`. So a cell's children need only the
//! points of its list within `f(m) + 2 h + 2 E` of `m`; the root's list is every point. Deep
//! cells near the optimum keep only the few points that bound it, which makes the search
//! cheap even for 100 000 points.
//!
//! # Rounding margins
//!
//! A distance is `red::norm2` of a coordinate difference: every difference is one rounding
//! (relative `u`) and the norm a few more, so its absolute error is below
//! `E = 64 EPSILON (Q + M)`, with `Q` the largest `|q_ij|` and `M` the largest `|coordinate|`
//! of the root box's corners. A slack is `b_k - red::dot(a_k, m)`: `Es = 64 EPSILON (B + M)`
//! with `B` the largest `|b_k|`. The half-diagonal is `half * sqrt(dim)` times `1 + 2^-40`
//! (rounded up generously).
//!
//! # Limits
//!
//! [`MAX_CELLS`] cells, [`MAX_WORK`] distance evaluations, depth [`MAX_DEPTH`]. Reaching any
//! is `Inconclusive`; the outcome never depends on time or the platform.

// Indexed loops over the coordinates mirror the formulas term by term.
#![allow(clippy::needless_range_loop)]

use fid_num::f::{total_max, total_min};
use fid_num::{red, NumError};

use super::CrossOutcome;
use crate::contract::{FitError, V3};

/// Most cells examined.
pub const MAX_CELLS: u32 = 1 << 20;
/// Most point-distance evaluations.
pub const MAX_WORK: u64 = 1 << 30;
/// Deepest cell level.
pub const MAX_DEPTH: u32 = 64;

const OP: &str = "fit.mic_cirsph.global";
// sqrt(2) and sqrt(3), correctly rounded; the half-diagonal is widened by `WIDEN` anyway.
const SQRT2: f64 = core::f64::consts::SQRT_2;
const SQRT3: f64 = 1.732_050_807_568_877_2;
const WIDEN: f64 = 1.0 + 1.0 / 1_099_511_627_776.0;

/// What the search found.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Search {
    pub outcome: CrossOutcome,
    pub cells: u32,
    pub work: u64,
    pub depth: u32,
    /// `Better`: a centre (scaled, `dim` components used) and its empty-ball radius (scaled).
    pub better: Option<(V3, f64)>,
}

/// The region and points of one search, in scaled units.
pub(crate) struct Region<'a> {
    pub q: &'a [f64],
    pub dim: usize,
    pub a: &'a [V3],
    pub b: &'a [f64],
    pub centre: V3,
    /// The region's circumradius (scaled).
    pub radius: f64,
}

struct Node {
    m: V3,
    half: f64,
    depth: u32,
    list: usize,
}

fn domain(detail: &'static str) -> FitError {
    FitError::Num(NumError::Domain { op: OP, detail })
}

fn alloc<T>(n: usize) -> Result<Vec<T>, FitError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| domain("allocation_failed"))?;
    Ok(v)
}

fn dist(q: &[f64], i: usize, m: &V3, dim: usize) -> Result<f64, FitError> {
    let s = q.get(3 * i..3 * i + 3).ok_or(domain("point_index"))?;
    let mut w = [0.0; 3];
    for k in 0..dim {
        w[k] = s[k] - m[k];
    }
    Ok(red::norm2(&w[..dim])?)
}

fn min_slack(reg: &Region<'_>, m: &V3) -> Result<f64, FitError> {
    let mut lo = f64::INFINITY;
    for (ak, bk) in reg.a.iter().zip(reg.b) {
        lo = total_min(lo, *bk - red::dot(&ak[..reg.dim], &m[..reg.dim])?)?;
    }
    Ok(lo)
}

fn release(lists: &mut [Vec<u32>], refs: &mut [u32], l: usize) {
    if let (Some(r), Some(list)) = (refs.get_mut(l), lists.get_mut(l)) {
        *r = r.saturating_sub(1);
        if *r == 0 {
            *list = Vec::new();
        }
    }
}

/// Runs the search for the fit radius `r` (scaled) with tolerance `tol` (scaled).
pub(crate) fn search(reg: &Region<'_>, r: f64, tol: f64) -> Result<Search, FitError> {
    let dim = reg.dim;
    if !(dim == 2 || dim == 3) || reg.a.len() != reg.b.len() {
        return Err(domain("region"));
    }
    let n = reg.q.len() / 3;
    let mut qmax = 0.0_f64;
    for &x in reg.q {
        qmax = total_max(qmax, x.abs())?;
    }
    let mut bmax = 0.0_f64;
    for &x in reg.b {
        bmax = total_max(bmax, x.abs())?;
    }
    let half0 = reg.radius * WIDEN;
    let mut cmax = 0.0_f64;
    for k in 0..dim {
        cmax = total_max(cmax, reg.centre[k].abs() + half0)?;
    }
    let e = (64.0 * f64::EPSILON) * (qmax + cmax);
    let es = (64.0 * f64::EPSILON) * (bmax + cmax);
    let goal = r + tol;
    let diag = if dim == 2 { SQRT2 } else { SQRT3 };
    if ![e, es, goal, half0].iter().all(|x| x.is_finite()) {
        return Err(FitError::Num(NumError::Infinite { op: OP, index: None }));
    }

    let mut root = alloc(n)?;
    root.extend(0..n as u32);
    let mut lists: Vec<Vec<u32>> = vec![root];
    let mut refs: Vec<u32> = vec![1];
    let mut stack = vec![Node { m: reg.centre, half: half0, depth: 0, list: 0 }];
    let mut scratch: Vec<f64> = alloc(n)?;
    let mut out = Search { outcome: CrossOutcome::Passed, cells: 0, work: 0, depth: 0, better: None };
    let children = 1usize << dim;

    while let Some(node) = stack.pop() {
        out.cells += 1;
        out.depth = out.depth.max(node.depth);
        if out.cells > MAX_CELLS {
            out.outcome = CrossOutcome::Inconclusive;
            return Ok(out);
        }
        let h = node.half * diag * WIDEN;
        let slack = min_slack(reg, &node.m)?;
        if slack < -(h + es) {
            release(&mut lists, &mut refs, node.list);
            continue;
        }
        let list = lists.get(node.list).ok_or(domain("list_index"))?;
        if list.is_empty() {
            return Err(domain("empty_candidate_list"));
        }
        scratch.clear();
        let mut f = f64::INFINITY;
        for &i in list {
            let d = dist(reg.q, i as usize, &node.m, dim)?;
            f = total_min(f, d)?;
            scratch.push(d);
        }
        out.work += list.len() as u64;
        if out.work > MAX_WORK {
            out.outcome = CrossOutcome::Inconclusive;
            return Ok(out);
        }
        if f + h + e <= goal {
            release(&mut lists, &mut refs, node.list);
            continue;
        }
        if slack >= es && f - e > goal {
            out.outcome = CrossOutcome::Better;
            out.better = Some((node.m, f));
            return Ok(out);
        }
        if node.depth >= MAX_DEPTH {
            out.outcome = CrossOutcome::Inconclusive;
            return Ok(out);
        }
        // The children's candidate list.
        let thr = f + 2.0 * h + 2.0 * e;
        let mut next: Vec<u32> = alloc(list.len())?;
        for (&i, &d) in list.iter().zip(&scratch) {
            if d <= thr {
                next.push(i);
            }
        }
        release(&mut lists, &mut refs, node.list);
        let id = lists.len();
        lists.push(next);
        refs.push(children as u32);
        let half = node.half / 2.0;
        for ch in (0..children).rev() {
            let mut m = node.m;
            for k in 0..dim {
                if ch & (1 << k) != 0 {
                    m[k] += half;
                } else {
                    m[k] -= half;
                }
            }
            stack.push(Node { m, half, depth: node.depth + 1, list: id });
        }
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn square() -> (Vec<V3>, Vec<f64>) {
        (vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, -1.0, 0.0]], vec![0.25; 4])
    }

    #[test]
    fn proves_better_and_inconclusive() {
        // Three points at 120 degrees on the unit circle: the inscribed circle about the origin
        // has radius 1 and is the global optimum in the square |x|, |y| <= 0.25.
        let s = 0.75_f64.sqrt();
        let q = [1.0, 0.0, 0.0, -0.5, s, 0.0, -0.5, -s, 0.0];
        let (a, b) = square();
        let reg = Region { q: &q, dim: 2, a: &a, b: &b, centre: [0.0; 3], radius: 0.25 * SQRT2 };
        let ok = search(&reg, 1.0, 1e-9).unwrap();
        assert_eq!(ok.outcome, CrossOutcome::Passed);
        // A claimed radius below the optimum is beaten.
        let worse = search(&reg, 0.9, 1e-9).unwrap();
        assert_eq!(worse.outcome, CrossOutcome::Better);
        let (_, rb) = worse.better.unwrap();
        assert!(rb > 0.9 + 1e-9);
        // A tolerance below the rounding margin never terminates the peak cell: the depth limit
        // gives Inconclusive, not a false pass.
        let tiny = search(&reg, 1.0, 0.0).unwrap();
        assert_eq!(tiny.outcome, CrossOutcome::Inconclusive);
    }
}
