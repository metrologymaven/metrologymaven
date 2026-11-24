//! Rule 11: the global cross-check of a free-direction fit by a branch and bound over a cap
//! cover of the axis directions (module documentation of [`super`]).
//!
//! All quantities are in the centred, scaled units of `contract::Centred`; `q` are the scaled
//! points (their centroid is the origin). For a unit direction `D`, `R_D` is the radius of the
//! smallest cylinder with direction `D` containing every point (the smallest circle enclosing
//! the points projected on the plane normal to `D`). The fit's radius `R` is the global minimum
//! within `tol` iff `R_D >= R - tol` for every `D`.
//!
//! # Bounds over a cap
//!
//! A cap is a spherical triangle of directions with centre `C` (unit) and chord radius `h`
//! (`|D - C| <= h` for every `D` in it). For any subset `S` of the points, `R_D` is at least the
//! smallest circle enclosing `S` (`mec(S, D)`), and for `D` in the cap with `R_D < R`:
//!
//! - **Pair** `{i, j}`, `w = q_i - q_j`: `R_D >= |w x D| / 2 >= (|w x C| - |w| h) / 2`.
//! - **Set** `S`: `R_D >= mec(S, C) - L_S h` with `L_S = sqrt(R^2 + max_(k in S) |q_k|^2)`.
//!   (Proof: let `P` be the foot of the origin on the axis of `mec(S, D)`, radius `rho < R`. For
//!   `k` in `S`, `|(q_k - P) x C| <= |(q_k - P) x D| + |q_k - P| h` and `|q_k - P|^2 = |(q_k - P)
//!   x D|^2 + (q_k . D)^2 <= R^2 + |q_k|^2`, so `mec(S, C) <= rho + L_S h`.)
//!
//! `mec` of three points is computed directly (half the longest side for a right or obtuse
//! triangle, else the circumradius); `mec` of up to five points is the largest `mec` of its
//! triples (a smallest enclosing circle is fixed by at most three points). Every lower bound is
//! reduced by `SLACK * R` for rounding. An upper bound at `C` is the largest distance of a
//! projected point from a centre (a feasible cylinder).
//!
//! # Procedure
//!
//! 1. **Cover.** The 20 faces of the icosahedron (vertices the cyclic permutations of `(0, +-1,
//!    +-phi)`, normalised) are found as the vertex triples with pairwise squared distance 4 (up
//!    to `1e-9`); of each antipodal pair of faces the one whose vertex sum has its first nonzero
//!    coordinate positive is kept (10 faces: every axis direction, as `D` or `-D`). A face is
//!    split into four by the normalised edge midpoints (`(a + b) / |a + b|`, the same for both
//!    faces of an edge, so the faces tile the sphere exactly as rays). `C` is the normalised
//!    vertex sum and `h` the largest `|C - v|` over the vertices times `1 + 1e-12` plus `1e-15`.
//! 2. **Cap test**, best first (smallest parent witness bound, then insertion order), with the witness set `W` (the
//!    fit's contacts, then the support sets and farthest pairs of full tests, at most
//!    [`MAX_WITNESSES`] points) and `S` the support set of `W`'s smallest circle at `C`:
//!    - `mec(S, C) - L_S h >= R - tol`: decided.
//!    - Else the full test at `C`, when `mec(W, C) < R - tol / 2` or the last full test on the
//!      cap's path added a witness. Otherwise (near the fit's own axis, where `W` already holds
//!      the support) the cap is split on the witness bound alone: a heuristic for speed, never
//!      for soundness, since splitting is always sound. The full test: the convex hull of the
//!      projected points (monotone chain), the
//!      farthest hull pair (rotating calipers) for the pair bound, the smallest enclosing circle
//!      of the hull points (iterative Welzl in a fixed pseudo-random order, `xorshift64` seeded
//!      [`ORDER_SEED`]) whose support set `S` gives the set bound and whose centre gives the
//!      upper bound over every point. Upper bound `< R - tol`: **better** (stop). The support
//!      set and the pair join `W`. Larger lower bound `>= R - tol`: decided. Else split.
//!    - A cap at depth [`MAX_DEPTH`] that is not decided: **inconclusive**, and the search goes
//!      on with the other caps (a later **better** still wins). [`MAX_CAPS`] cap tests or
//!      [`MAX_FULL`] full tests exceeded: **inconclusive** (stop).
//! 3. Every cap decided: **passed**. The hull and the circle need not be exact: every lower
//!    bound comes from actual points and every upper bound is checked over every point.

// Indexed loops over small fixed sets mirror the formulas.
#![allow(clippy::needless_range_loop)]

use core::cmp::Ordering;
use std::collections::BinaryHeap;

use fid_num::{f, NumError};

use super::CrossOutcome;
use crate::contract::{FitError, V3};
use crate::init;

/// Deepest cap level (a face has chord radius about 0.65; level 40 about 6e-13).
pub const MAX_DEPTH: u32 = 40;
/// Cap tests per cross-check.
pub const MAX_CAPS: u32 = 1 << 18;
/// Full (all-point) tests per cross-check.
pub const MAX_FULL: u32 = 1 << 12;
/// Largest witness set (rule 11 step 2).
pub const MAX_WITNESSES: usize = 64;
/// Rounding allowance of every lower bound, relative to `R`.
pub const SLACK: f64 = 1e-12;
/// `mec3` falls back to half the longest side when twice the area is at most this times the
/// longest side squared (a nearly degenerate triangle).
pub const MEC3_THIN: f64 = 1e-6;
/// Seed of the Welzl order.
pub const ORDER_SEED: u64 = 0x9e37_79b9_7f4a_7c15;

const OP: &str = "fit.mcc_cyl.global";

/// A projected point.
pub type V2 = [f64; 2];

/// A cap waiting for its test: popped smallest `key` (the parent's witness lower bound) first,
/// then smallest `seq` (insertion order); a total order, so the search is deterministic.
#[derive(Debug, Clone, Copy)]
struct Pending {
    key: f64,
    seq: u64,
    tri: [V3; 3],
    depth: u32,
    full_ok: bool,
}

impl PartialEq for Pending {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}

impl Eq for Pending {}

impl PartialOrd for Pending {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for Pending {
    /// Reversed, so that `BinaryHeap` (a max-heap) pops the smallest key and sequence.
    fn cmp(&self, o: &Self) -> Ordering {
        o.key.total_cmp(&self.key).then(o.seq.cmp(&self.seq))
    }
}

/// The cross-check's outcome in scaled units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Search {
    pub outcome: CrossOutcome,
    pub caps: u32,
    pub full: u32,
    pub depth: u32,
    /// `better`: the cap centre and the feasible radius there.
    pub better: Option<(V3, f64)>,
}

fn dom(detail: &'static str) -> FitError {
    FitError::Num(NumError::Domain { op: OP, detail })
}

/// Fallible growth of a buffer to hold `n` more elements (allocation failure is an error).
fn reserve<T>(v: &mut Vec<T>, n: usize) -> Result<(), FitError> {
    v.try_reserve(n).map_err(|_| dom("allocation_failed"))
}

fn sqrt(x: f64) -> Result<f64, FitError> {
    Ok(f::sqrt(x)?)
}

fn gt(a: f64, b: f64) -> Result<bool, FitError> {
    Ok(f::total_cmp(a, b)? == Ordering::Greater)
}

fn ge(a: f64, b: f64) -> Result<bool, FitError> {
    Ok(f::total_cmp(a, b)? != Ordering::Less)
}

fn d2(a: V2, b: V2) -> f64 {
    let (x, y) = (a[0] - b[0], a[1] - b[1]);
    x * x + y * y
}

fn norm3(v: V3) -> Result<f64, FitError> {
    sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2])
}

fn unit3(v: V3) -> Result<V3, FitError> {
    let n = norm3(v)?;
    if !n.is_finite() || n <= 0.0 {
        return Err(dom("direction"));
    }
    Ok([v[0] / n, v[1] / n, v[2] / n])
}

fn cross2(o: V2, a: V2, b: V2) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

/// The smallest circle enclosing three points: half the longest side for a right or obtuse
/// triangle, else the circumradius.
pub fn mec3(a: V2, b: V2, c: V2) -> Result<f64, FitError> {
    let s = [d2(a, b), d2(b, c), d2(c, a)];
    let mut k = 0;
    for i in 1..3 {
        if gt(s[i], s[k])? {
            k = i;
        }
    }
    let (o1, o2) = (s[(k + 1) % 3], s[(k + 2) % 3]);
    if ge(s[k], o1 + o2)? {
        return Ok(sqrt(s[k])? / 2.0);
    }
    // Twice the area from the vertex opposite the longest side (its two edges are the shorter
    // ones, so the cross product has a relative rounding error of a few ulps even for thin
    // triangles; from another vertex it can reach `u * longest / shortest`, review of #374).
    let (o, e1, e2) = match k {
        0 => (c, a, b),
        1 => (a, b, c),
        _ => (b, c, a),
    };
    let area2 = cross2(o, e1, e2).abs();
    let half = sqrt(s[k])? / 2.0;
    // A nearly degenerate triangle: half the longest side, a valid (smaller) lower bound.
    if !gt(area2, MEC3_THIN * s[k])? {
        return Ok(half);
    }
    let r = sqrt(s[0])? * sqrt(s[1])? * sqrt(s[2])? / (2.0 * area2);
    // An acute triangle's circumradius is at least half its longest side.
    if r.is_finite() {
        Ok(f::total_max(r, half)?)
    } else {
        Ok(half)
    }
}

/// `mec` of up to five points: the largest `mec` of its pairs and triples.
fn mec_small(p: &[V2]) -> Result<f64, FitError> {
    let mut best = 0.0_f64;
    for i in 0..p.len() {
        for j in i + 1..p.len() {
            best = f::total_max(best, sqrt(d2(p[i], p[j]))? / 2.0)?;
            for k in j + 1..p.len() {
                best = f::total_max(best, mec3(p[i], p[j], p[k])?)?;
            }
        }
    }
    Ok(best)
}

/// Projection of every point on the plane normal to `c` (unit), basis of `init::basis`.
fn project(q: &[f64], c: V3, out: &mut Vec<V2>) -> Result<(), FitError> {
    let (e1, e2) = init::basis(c)?;
    out.clear();
    reserve(out, q.len() / 3)?;
    for p in q.chunks_exact(3) {
        out.push([p[0] * e1[0] + p[1] * e1[1] + p[2] * e1[2], p[0] * e2[0] + p[1] * e2[1] + p[2] * e2[2]]);
    }
    Ok(())
}

/// Convex hull (monotone chain, counter-clockwise, indices into `p`), O(n log n).
fn hull(p: &[V2], idx: &mut Vec<usize>, out: &mut Vec<usize>) -> Result<(), FitError> {
    idx.clear();
    reserve(idx, p.len())?;
    idx.extend(0..p.len());
    idx.sort_by(|&i, &j| p[i][0].total_cmp(&p[j][0]).then(p[i][1].total_cmp(&p[j][1])).then(i.cmp(&j)));
    out.clear();
    reserve(out, p.len() + 1)?;
    if p.len() < 3 {
        out.extend_from_slice(idx);
        return Ok(());
    }
    for &i in idx.iter() {
        while out.len() >= 2 && cross2(p[out[out.len() - 2]], p[out[out.len() - 1]], p[i]) <= 0.0 {
            out.pop();
        }
        out.push(i);
    }
    let lower = out.len() + 1;
    for &i in idx.iter().rev().skip(1) {
        while out.len() >= lower && cross2(p[out[out.len() - 2]], p[out[out.len() - 1]], p[i]) <= 0.0 {
            out.pop();
        }
        out.push(i);
    }
    out.pop();
    Ok(())
}

/// The larger of two finite squared distances (KNP-1: no `f64::max`).
fn larger(a: f64, b: f64) -> f64 {
    if b > a {
        b
    } else {
        a
    }
}

/// The farthest pair of hull vertices by rotating calipers (indices into `p`), O(h).
fn farthest(p: &[V2], h: &[usize]) -> (usize, usize) {
    let m = h.len();
    if m < 3 {
        return (h[0], h[m - 1]);
    }
    let mut best = (h[0], h[1], d2(p[h[0]], p[h[1]]));
    let mut j = 1;
    let mut steps = 0usize;
    for i in 0..m {
        let (a, b) = (p[h[i]], p[h[(i + 1) % m]]);
        while steps < 4 * m && cross2(a, b, p[h[(j + 1) % m]]).abs() > cross2(a, b, p[h[j]]).abs() {
            j = (j + 1) % m;
            steps += 1;
        }
        for &k in &[i, (i + 1) % m] {
            let d = d2(p[h[k]], p[h[j]]);
            if d > best.2 {
                best = (h[k], h[j], d);
            }
        }
    }
    (best.0, best.1)
}

fn circle2(a: V2, b: V2) -> (V2, f64) {
    let c = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    (c, larger(d2(a, c), d2(b, c)))
}

fn circle3(a: V2, b: V2, c: V2) -> (V2, f64) {
    let (bx, by) = (b[0] - a[0], b[1] - a[1]);
    let (cx, cy) = (c[0] - a[0], c[1] - a[1]);
    let d = 2.0 * (bx * cy - by * cx);
    let (b2, c2) = (bx * bx + by * by, cx * cx + cy * cy);
    let ux = (cy * b2 - by * c2) / d;
    let uy = (bx * c2 - cx * b2) / d;
    let o = [a[0] + ux, a[1] + uy];
    if o[0].is_finite() && o[1].is_finite() {
        let r = larger(larger(d2(a, o), d2(b, o)), d2(c, o));
        return (o, r);
    }
    // Collinear: the circle on the farthest pair.
    let pairs = [(a, b), (b, c), (c, a)];
    let mut best = circle2(a, b);
    for (x, y) in pairs {
        let cand = circle2(x, y);
        if cand.1 > best.1 {
            best = cand;
        }
    }
    best
}

/// A support set: up to three indices into the projected points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Support {
    idx: [usize; 3],
    len: usize,
}

impl Support {
    fn of(idx: &[usize]) -> Support {
        let mut s = Support { idx: [0; 3], len: idx.len().min(3) };
        s.idx[..s.len].copy_from_slice(&idx[..s.len]);
        s
    }

    fn as_slice(&self) -> &[usize] {
        &self.idx[..self.len]
    }
}

/// Iterative Welzl on the points `p[h[..]]` in the fixed order: the centre and the support set.
/// An empty `h` is an error.
fn welzl(p: &[V2], h: &[usize], order: &mut Vec<usize>) -> Result<(V2, Support), FitError> {
    order.clear();
    reserve(order, h.len())?;
    order.extend_from_slice(h);
    let mut s = ORDER_SEED;
    for i in (1..order.len()).rev() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        order.swap(i, (s % (i as u64 + 1)) as usize);
    }
    let first = *order.first().ok_or(dom("welzl_empty"))?;
    let out = |x: V2, c: V2, r2: f64| d2(x, c) > r2 * (1.0 + 1e-14);
    let mut c = *p.get(first).ok_or(dom("welzl_index"))?;
    let mut r2 = 0.0;
    let mut sup = Support::of(&[first]);
    for i in 1..order.len() {
        let pi = p[order[i]];
        if !out(pi, c, r2) {
            continue;
        }
        c = pi;
        r2 = 0.0;
        sup = Support::of(&[order[i]]);
        for j in 0..i {
            let pj = p[order[j]];
            if !out(pj, c, r2) {
                continue;
            }
            (c, r2) = circle2(pi, pj);
            sup = Support::of(&[order[i], order[j]]);
            for k in 0..j {
                let pk = p[order[k]];
                if out(pk, c, r2) {
                    (c, r2) = circle3(pi, pj, pk);
                    sup = Support::of(&[order[i], order[j], order[k]]);
                }
            }
        }
    }
    Ok((c, sup))
}

/// The ten kept icosahedron faces (rule 11 step 1).
fn faces() -> Result<Vec<[V3; 3]>, FitError> {
    let phi = (1.0 + sqrt(5.0)?) / 2.0;
    let mut v: Vec<V3> = Vec::with_capacity(12);
    for &s1 in &[1.0, -1.0] {
        for &s2 in &[1.0, -1.0] {
            let (a, b) = (s1, s2 * phi);
            v.push([0.0, a, b]);
            v.push([a, b, 0.0]);
            v.push([b, 0.0, a]);
        }
    }
    let close = |a: V3, b: V3| {
        let (x, y, z) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
        ((x * x + y * y + z * z) - 4.0).abs() < 1e-9
    };
    let mut out = Vec::with_capacity(10);
    for i in 0..12 {
        for j in i + 1..12 {
            for k in j + 1..12 {
                if close(v[i], v[j]) && close(v[j], v[k]) && close(v[k], v[i]) {
                    let s = [v[i][0] + v[j][0] + v[k][0], v[i][1] + v[j][1] + v[k][1], v[i][2] + v[j][2] + v[k][2]];
                    let first = s.iter().copied().find(|&x| x != 0.0).unwrap_or(0.0);
                    if first > 0.0 {
                        out.push([unit3(v[i])?, unit3(v[j])?, unit3(v[k])?]);
                    }
                }
            }
        }
    }
    if out.len() != 10 {
        return Err(dom("icosahedron"));
    }
    Ok(out)
}

fn mid(a: V3, b: V3) -> Result<V3, FitError> {
    unit3([a[0] + b[0], a[1] + b[1], a[2] + b[2]])
}

/// The set bound at a cap: the smallest circle of the points `w` (indices into `q`) projected
/// with basis `(e1, e2)` (Welzl in the fixed order), and `mec` of its support set less `L_S h`.
/// Returns the bound and `mec(S, C)`.
#[allow(clippy::too_many_arguments)]
fn set_bound(
    q: &[f64],
    w: &[usize],
    e: (V3, V3),
    r: f64,
    h: f64,
    wp: &mut Vec<V2>,
    all: &mut Vec<usize>,
    order: &mut Vec<usize>,
) -> Result<(f64, f64), FitError> {
    let (e1, e2) = e;
    wp.clear();
    reserve(wp, w.len())?;
    for &k in w {
        let p = q.get(3 * k..3 * k + 3).ok_or(dom("point"))?;
        wp.push([p[0] * e1[0] + p[1] * e1[1] + p[2] * e1[2], p[0] * e2[0] + p[1] * e2[1] + p[2] * e2[2]]);
    }
    all.clear();
    reserve(all, wp.len())?;
    all.extend(0..wp.len());
    let (_, sup) = welzl(wp, all, order)?;
    let mut ms = 0.0_f64;
    let mut sp = [[0.0; 2]; 3];
    for (slot, &i) in sp.iter_mut().zip(sup.as_slice()) {
        let k = *w.get(i).ok_or(dom("support"))?;
        let p = q.get(3 * k..3 * k + 3).ok_or(dom("point"))?;
        ms = f::total_max(ms, norm3([p[0], p[1], p[2]])?)?;
        *slot = *wp.get(i).ok_or(dom("support"))?;
    }
    let m = mec_small(&sp[..sup.len])?;
    Ok((m - sqrt(r * r + ms * ms)? * h, m))
}

/// Rule 11 in scaled units: `q` the points, `contacts` the fit's contact indices, `r` its
/// radius and `tol` the tolerance. The witness set `W` starts as the contacts and gains the
/// support sets of full tests (at most [`MAX_WITNESSES`] points); a cap's full test runs when
/// the witness bound does not decide it and either `mec(W, C) < R - tol / 2` or the last full
/// test on its path added a witness (else the cap is split with the witness bound only, which
/// may end inconclusive: fail closed).
pub(crate) fn search(q: &[f64], contacts: &[u32], r: f64, tol: f64) -> Result<Search, FitError> {
    let n = q.len() / 3;
    let pt = |k: usize| -> Result<V3, FitError> {
        let s = q.get(3 * k..3 * k + 3).ok_or(dom("point"))?;
        Ok([s[0], s[1], s[2]])
    };
    let mut w: Vec<usize> = Vec::new();
    reserve(&mut w, MAX_WITNESSES)?;
    for &k in contacts {
        let k = k as usize;
        if k >= n {
            return Err(dom("contact"));
        }
        if !w.contains(&k) {
            w.push(k);
        }
    }
    let target = r - tol;
    let slack = SLACK * r;
    let mut st = Search { outcome: CrossOutcome::Passed, caps: 0, full: 0, depth: 0, better: None };
    let mut heap = BinaryHeap::new();
    heap.try_reserve(16).map_err(|_| dom("allocation_failed"))?;
    let mut seq = 0u64;
    for t in faces()? {
        heap.push(Pending { key: f64::NEG_INFINITY, seq, tri: t, depth: 0, full_ok: true });
        seq += 1;
    }
    let (mut proj, mut idx, mut hl, mut order) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut wp, mut all) = (Vec::new(), Vec::new());
    while let Some(Pending { tri: t, depth, full_ok, .. }) = heap.pop() {
        st.caps = st.caps.saturating_add(1);
        st.depth = st.depth.max(depth);
        if st.caps > MAX_CAPS {
            st.outcome = CrossOutcome::Inconclusive;
            return Ok(st);
        }
        let c = unit3([t[0][0] + t[1][0] + t[2][0], t[0][1] + t[1][1] + t[2][1], t[0][2] + t[1][2] + t[2][2]])?;
        let mut h = 0.0_f64;
        for v in &t {
            h = f::total_max(h, norm3([v[0] - c[0], v[1] - c[1], v[2] - c[2]])?)?;
        }
        let h = h * (1.0 + 1e-12) + 1e-15;
        let e = init::basis(c)?;
        let (lb_w, mc_w) = set_bound(q, &w, e, r, h, &mut wp, &mut all, &mut order)?;
        if ge(lb_w - slack, target)? {
            continue;
        }
        let mut child_ok = full_ok;
        if full_ok || !ge(mc_w, r - tol / 2.0)? {
            st.full = st.full.saturating_add(1);
            if st.full > MAX_FULL {
                st.outcome = CrossOutcome::Inconclusive;
                return Ok(st);
            }
            project(q, c, &mut proj)?;
            hull(&proj, &mut idx, &mut hl)?;
            if hl.is_empty() {
                return Err(dom("hull"));
            }
            let (i, j) = farthest(&proj, &hl);
            let wn = norm3({
                let (a, b) = (pt(i)?, pt(j)?);
                [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
            })?;
            let lb_pair = (sqrt(d2(proj[i], proj[j]))? - wn * h) / 2.0 - slack;
            let (centre, sup) = welzl(&proj, &hl, &mut order)?;
            let mut ms = 0.0_f64;
            let mut sp = [[0.0; 2]; 3];
            for (slot, &k) in sp.iter_mut().zip(sup.as_slice()) {
                ms = f::total_max(ms, norm3(pt(k)?)?)?;
                *slot = *proj.get(k).ok_or(dom("support"))?;
            }
            let lb_set = mec_small(&sp[..sup.len])? - sqrt(r * r + ms * ms)? * h - slack;
            let mut ub2 = 0.0_f64;
            for p in &proj {
                ub2 = f::total_max(ub2, d2(*p, centre))?;
            }
            let ub = sqrt(ub2)?;
            if gt(target, ub)? {
                st.outcome = CrossOutcome::Better;
                st.better = Some((c, ub));
                return Ok(st);
            }
            let mut added = false;
            for k in sup.as_slice().iter().copied().chain([i, j]) {
                if w.len() < MAX_WITNESSES && !w.contains(&k) {
                    w.push(k);
                    added = true;
                }
            }
            child_ok = added;
            if ge(f::total_max(lb_pair, lb_set)?, target)? {
                continue;
            }
        }
        if depth >= MAX_DEPTH {
            // Undecided: keep searching the other caps for a better cylinder.
            st.outcome = CrossOutcome::Inconclusive;
            continue;
        }
        let (ab, bc, ca) = (mid(t[0], t[1])?, mid(t[1], t[2])?, mid(t[2], t[0])?);
        heap.try_reserve(4).map_err(|_| dom("allocation_failed"))?;
        for child in [[ca, bc, t[2]], [ab, t[1], bc], [t[0], ab, ca], [ab, bc, ca]] {
            heap.push(Pending { key: lb_w, seq, tri: child, depth: depth + 1, full_ok: child_ok });
            seq += 1;
        }
    }
    Ok(st)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn mec3_cases() {
        // Right triangle: half the hypotenuse.
        assert_eq!(mec3([0.0, 0.0], [2.0, 0.0], [0.0, 2.0]).unwrap(), 2.0_f64.sqrt());
        // Equilateral (side 1): circumradius 1 / sqrt(3).
        let r = mec3([0.0, 0.0], [1.0, 0.0], [0.5, 0.75_f64.sqrt()]).unwrap();
        assert!((r - 1.0 / 3.0_f64.sqrt()).abs() < 1e-15);
        // Obtuse and collinear: half the longest side.
        assert_eq!(mec3([0.0, 0.0], [4.0, 0.0], [1.0, 0.5]).unwrap(), 2.0);
        assert_eq!(mec3([0.0, 0.0], [4.0, 0.0], [1.0, 0.0]).unwrap(), 2.0);
    }

    #[test]
    fn hull_calipers_welzl_on_a_circle() {
        let mut p = Vec::new();
        for k in 0..97 {
            let t = k as f64 * 0.37;
            p.push([2.0 * t.cos() + 1.0, 2.0 * t.sin() - 3.0]);
        }
        p.push([1.0, -3.0]);
        let (mut idx, mut h, mut order) = (Vec::new(), Vec::new(), Vec::new());
        hull(&p, &mut idx, &mut h).unwrap();
        assert!(!h.contains(&97));
        let (i, j) = farthest(&p, &h);
        let brute = p.iter().flat_map(|a| p.iter().map(move |b| d2(*a, *b))).fold(0.0, larger);
        assert_eq!(d2(p[i], p[j]), brute);
        let (c, sup) = welzl(&p, &h, &mut order).unwrap();
        let r = p.iter().map(|x| d2(*x, c)).fold(0.0, larger).sqrt();
        assert!((r - 2.0).abs() < 1e-3, "{r}");
        let sp: Vec<V2> = sup.as_slice().iter().map(|&k| p[k]).collect();
        assert!(mec_small(&sp).unwrap() <= r * (1.0 + 1e-12));
    }

    /// Review 3 of #374: an empty point set is an error, not a panic.
    #[test]
    fn welzl_empty_is_an_error() {
        let mut order = Vec::new();
        assert!(welzl(&[], &[], &mut order).is_err());
    }

    #[test]
    fn ten_faces() {
        assert_eq!(faces().unwrap().len(), 10);
    }

    /// Cost of one full test (projection, hull, calipers, Welzl, upper bound) on 200 000
    /// points of a cylinder: `cargo test --release -p fid-fit --lib full_test_cost -- --ignored
    /// --nocapture`.
    #[test]
    #[ignore = "timing benchmark on 200k points (no assertions; ~2.2s debug); run with --release --ignored --nocapture"]
    fn full_test_cost() {
        let n = 200_000;
        let mut q = Vec::with_capacity(3 * n);
        let mut s = 1u64;
        for _ in 0..n {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let t = (s % 1_000_000) as f64 / 1_000_000.0 * core::f64::consts::TAU;
            let z = ((s >> 20) % 1_000_000) as f64 / 1_000_000.0 - 0.5;
            q.extend_from_slice(&[0.5 * t.cos(), 0.5 * t.sin(), z]);
        }
        let (mut proj, mut idx, mut hl, mut order) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for c in [[0.0, 0.0, 1.0], [0.6, 0.0, 0.8], [1.0, 0.0, 0.0]] {
            let t0 = std::time::Instant::now();
            project(&q, c, &mut proj).unwrap();
            hull(&proj, &mut idx, &mut hl).unwrap();
            let (i, j) = farthest(&proj, &hl);
            let (centre, _) = welzl(&proj, &hl, &mut order).unwrap();
            let ub = proj.iter().map(|p| d2(*p, centre)).fold(0.0, larger).sqrt();
            eprintln!(
                "direction {c:?}: hull {} points, diameter {:.6}, radius {ub:.6}, {:.2?}",
                hl.len(),
                d2(proj[i], proj[j]).sqrt(),
                t0.elapsed()
            );
        }
    }
}
