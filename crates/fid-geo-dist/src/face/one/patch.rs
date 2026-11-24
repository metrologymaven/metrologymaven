//! Interior candidates on a NURBS face: Bézier decomposition and a branch and bound over the
//! Bézier patches and their halves (REQ-GEO.DIST.FACE.ONE-003).
//!
//! GEO.NURBS.INVERT keeps its patch loop private and returns only the global foot of the
//! untrimmed surface, so this leaf re-implements the loop (NODE.md, Open questions):
//!
//! 1. **Decomposition.** Every distinct knot in the domain is inserted (Boehm, homogeneous
//!    coordinates) up to multiplicity `p`; each non-empty knot span pair is one Bézier patch,
//!    numbered u-span major. A face whose original knots have an interior multiplicity of at
//!    least `p` (a possible C0 crease, where the closest point need not be a stationary point
//!    of either patch) is refused with [`Unsupported::Crease`].
//! 2. **Nodes** are patches and their halves, popped in `(lower bound, sequence number)` order;
//!    the root patches get sequence numbers in patch order. The lower bound is the distance
//!    from the query to the box of the projected control points (convex hull, weights
//!    positive) less `2^-44 M`. A node is dropped when the bound exceeds the best distance.
//! 3. **Exclusion.** With `X, W` the homogeneous patch, `H_s = (X_s W - X W_s) . (X - q W)`
//!    is `W^3 S_s . (S - q)` (for a polynomial patch `X_s . (X - q)`): a Bernstein polynomial
//!    whose zeros are the stationary points. When every coefficient of `H_s` or of `H_t` has
//!    one strict sign beyond `2^-36` times its magnitude polynomial, the node holds no
//!    stationary point and is dropped.
//! 4. **Uniqueness.** When the symmetric part of the Jacobian of `(H_s, H_t)` is positive
//!    definite on the node (Bernstein coefficient ranges), `H` is injective there: at most one
//!    stationary point, a strict minimum that is also the minimum of the node. A box-limited
//!    damped Newton iteration from the node centre finds it; a point that passes the
//!    orthogonality test `|S_x . r| <= |S_x| max(1e-12 max(d, 1e-9 m), 2^-46 M)`, or the
//!    point after a full Newton step (positive definite Hessian, inside the node) that moves
//!    the foot by at most `2^-30 M`, is a candidate (then classified against the trim by the
//!    caller). A run that ends neither there nor at a point of the node's box meeting the
//!    Karush-Kuhn-Tucker conditions leaves the node unresolved, and the query then fails
//!    with `NonConvergence` (fails closed).
//! 5. Otherwise the node is halved along the direction where its control net extends most,
//!    until [`MAX_DEPTH`] (a Newton run from the centre, flagged unresolved) or [`BUDGET`]
//!    nodes (flagged unresolved).
//!
//! The minimum over the trimmed region is a stationary point inside it or a boundary point
//! (edges and vertices, handled by the caller first, so their distance prunes this search).
//!
//! Determinism: binary64 `+ - * /` one operation at a time, no FMA, no `f64::min`/`max`, a
//! binary heap keyed by bit patterns and sequence numbers, every loop bounded.

use core::cmp::Reverse;
use std::collections::BinaryHeap;

use fid_geo_nurbs::{NurbsSurface, Surface};

use super::{dot3, fmax, ord_key, sub3, FaceError, Unsupported};

/// Deepest halving below a Bézier patch.
pub const MAX_DEPTH: u32 = 48;
/// Nodes examined per query.
pub const BUDGET: usize = 1 << 14;
/// Newton iterations per run.
pub const MAX_ITER: usize = 60;
/// Step halvings per Newton iteration.
pub const MAX_HALVINGS: usize = 40;
/// Relative orthogonality tolerance (as GEO.NURBS.INVERT REQ-002).
pub const REL: f64 = 1e-12;
/// Distance floor of the orthogonality tolerance, metres.
pub const FLOOR: f64 = 1e-9;
/// Evaluation noise of the orthogonality tolerance, times the coordinate scale (`2^-46`).
pub const NOISE: f64 = 1.0 / 70_368_744_177_664.0;
/// A stationary point is also accepted when the full Newton step (positive definite Hessian,
/// inside the box) moves the foot by at most this times the coordinate scale (`2^-30`): the
/// line search cannot resolve the last digits of `|S - q|^2`.
pub const STEP_REL: f64 = 1.0 / 1_073_741_824.0;
/// A full Newton step (positive definite Hessian) is taken when it does not raise
/// `|S - q|^2` by more than this relative amount (rounding, `2^-48`).
pub const FLAT_REL: f64 = 1.0 / 281_474_976_710_656.0;
/// Bernstein sign margin, times the magnitude polynomial (`2^-36`).
pub const GAMMA: f64 = 1.0 / 68_719_476_736.0;
/// Lower-bound safety, times the coordinate scale (`2^-44`).
pub const PRUNE_REL: f64 = 1.0 / 17_592_186_044_416.0;

type H4 = [f64; 4];

/// A tensor Bernstein polynomial of degree `(m, n)`, coefficient `(i, j)` at `i * (n + 1) + j`,
/// with a magnitude bound per coefficient.
#[derive(Debug, Clone)]
struct Poly {
    m: usize,
    n: usize,
    c: Vec<f64>,
    mag: Vec<f64>,
}

impl Poly {
    fn zero(m: usize, n: usize) -> Poly {
        let len = (m + 1) * (n + 1);
        Poly { m, n, c: vec![0.0; len], mag: vec![0.0; len] }
    }

    /// True when every coefficient has one strict sign beyond its margin.
    fn signed(&self) -> bool {
        let pos = self.c.iter().zip(&self.mag).all(|(&c, &g)| c > GAMMA * g);
        let neg = self.c.iter().zip(&self.mag).all(|(&c, &g)| c < -(GAMMA * g));
        pos || neg
    }

    fn max_mag(&self) -> f64 {
        let mut m = 0.0;
        for &g in &self.mag {
            m = fmax(m, g);
        }
        m
    }

    /// Range of the coefficients of the derivative along the first (`along_t == false`) or
    /// second index, widened by a margin; `None` for a direction of degree zero.
    fn deriv_range(&self, along_t: bool) -> Option<(f64, f64)> {
        let (m, n) = (self.m, self.n);
        let deg = if along_t { n } else { m };
        if deg == 0 {
            return None;
        }
        let at = |i: usize, j: usize| self.c[i * (n + 1) + j];
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        let (ie, je) = if along_t { (m, n - 1) } else { (m - 1, n) };
        for i in 0..=ie {
            for j in 0..=je {
                let d = if along_t { at(i, j + 1) - at(i, j) } else { at(i + 1, j) - at(i, j) };
                let d = deg as f64 * d;
                if d < lo {
                    lo = d;
                }
                if d > hi {
                    hi = d;
                }
            }
        }
        let margin = 4.0 * (deg as f64) * (GAMMA * self.max_mag());
        Some((lo - margin, hi + margin))
    }
}

/// Binomial coefficients as `f64` (Pascal's triangle, rows `0..=n`).
#[derive(Debug, Clone)]
pub(super) struct Binom(Vec<Vec<f64>>);

impl Binom {
    fn new(n: usize) -> Binom {
        let mut rows: Vec<Vec<f64>> = Vec::with_capacity(n + 1);
        for r in 0..=n {
            let mut row = vec![1.0; r + 1];
            if r >= 2 {
                if let Some(prev) = rows.last() {
                    for k in 1..r {
                        row[k] = prev[k - 1] + prev[k];
                    }
                }
            }
            rows.push(row);
        }
        Binom(rows)
    }

    fn get(&self, n: usize, k: usize) -> f64 {
        self.0.get(n).and_then(|r| r.get(k)).copied().unwrap_or(0.0)
    }
}

/// Bernstein product of `a` (degree `(m1, n1)`) and `b` (degree `(m2, n2)`), magnitudes from
/// the magnitudes of the factors.
fn prod(a: &Poly, b: &Poly, bin: &Binom) -> Poly {
    let (m, n) = (a.m + b.m, a.n + b.n);
    let mut out = Poly::zero(m, n);
    let wu = |i: usize, k: usize| (bin.get(a.m, i) * bin.get(b.m, k)) / bin.get(m, i + k);
    let wv = |j: usize, l: usize| (bin.get(a.n, j) * bin.get(b.n, l)) / bin.get(n, j + l);
    for i in 0..=a.m {
        for j in 0..=a.n {
            let (ac, ag) = (a.c[i * (a.n + 1) + j], a.mag[i * (a.n + 1) + j]);
            for k in 0..=b.m {
                let w1 = wu(i, k);
                for l in 0..=b.n {
                    let w = w1 * wv(j, l);
                    let (bc, bg) = (b.c[k * (b.n + 1) + l], b.mag[k * (b.n + 1) + l]);
                    let ix = (i + k) * (n + 1) + (j + l);
                    out.c[ix] += w * (ac * bc);
                    out.mag[ix] += w * (ag * bg);
                }
            }
        }
    }
    out
}

/// One Bézier patch: parameter box and homogeneous net `(p + 1) x (q + 1)`, row-major in u.
#[derive(Debug, Clone)]
pub(super) struct Patch {
    u: [f64; 2],
    v: [f64; 2],
    net: Vec<H4>,
}

/// The Bézier patches of a NURBS face surface.
#[derive(Debug, Clone)]
pub(super) struct Patches {
    p: usize,
    q: usize,
    rational: bool,
    list: Vec<Patch>,
    bin: Binom,
    /// Largest control-point coordinate magnitude.
    scale: f64,
}

/// A stationary point found inside the search.
#[derive(Debug, Clone, Copy)]
pub(super) struct Found {
    pub uv: [f64; 2],
    pub foot: [f64; 3],
    pub distance: f64,
}

/// What the search returns: accepted candidates (in the order found) and whether some node
/// could not be resolved.
#[derive(Debug, Clone, Default)]
pub(super) struct Search {
    pub found: Vec<Found>,
    pub unresolved: bool,
}

#[derive(Debug, Clone)]
struct Node {
    u: [f64; 2],
    v: [f64; 2],
    net: Vec<H4>,
    depth: u32,
    lb: f64,
}

fn mult(knots: &[f64], x: f64) -> usize {
    knots.iter().filter(|&&k| k == x).count()
}

/// Inserts `x` once into `knots` (degree `p`) and every curve of `curves` (Boehm).
fn insert(knots: &mut Vec<f64>, p: usize, curves: &mut [Vec<H4>], x: f64) -> Result<(), Unsupported> {
    // Span k: knots[k] <= x < knots[k + 1].
    let mut k = None;
    for i in 0..knots.len().saturating_sub(1) {
        if knots[i] <= x && x < knots[i + 1] {
            k = Some(i);
        }
    }
    let k = k.ok_or(Unsupported::KnotLayout)?;
    if k < p {
        return Err(Unsupported::KnotLayout);
    }
    for c in curves.iter_mut() {
        let n = c.len();
        let mut out: Vec<H4> = Vec::with_capacity(n + 1);
        for i in 0..=n {
            let q = if i + p <= k {
                c.get(i).copied()
            } else if i > k {
                c.get(i - 1).copied()
            } else {
                let den = knots[i + p] - knots[i];
                let a = (x - knots[i]) / den;
                let prev = c.get(i - 1).copied();
                if a == 0.0 {
                    prev
                } else {
                    let here = c.get(i).copied();
                    match (prev, here) {
                        (Some(pv), Some(h)) => {
                            let b = 1.0 - a;
                            Some(core::array::from_fn(|d| b * pv[d] + a * h[d]))
                        }
                        _ => None,
                    }
                }
            };
            out.push(q.ok_or(Unsupported::KnotLayout)?);
        }
        *c = out;
    }
    knots.insert(k + 1, x);
    Ok(())
}

/// Bézier spans `(start index k, [lo, hi])` of a knot vector refined to multiplicity `p`.
fn spans(knots: &[f64], p: usize, n_ctrl: usize) -> Result<Vec<(usize, [f64; 2])>, Unsupported> {
    let mut out = Vec::new();
    for k in p..n_ctrl {
        let (a, b) = (knots[k], knots[k + 1]);
        if a < b {
            let left = knots.get(k + 1 - p..=k).ok_or(Unsupported::KnotLayout)?;
            let right = knots.get(k + 1..=k + p).ok_or(Unsupported::KnotLayout)?;
            if !left.iter().all(|&x| x == a) || !right.iter().all(|&x| x == b) {
                return Err(Unsupported::KnotLayout);
            }
            out.push((k, [a, b]));
        }
    }
    Ok(out)
}

/// Refines one direction: every distinct knot in `[lo, hi]` to multiplicity `p`.
fn refine(knots: &mut Vec<f64>, p: usize, curves: &mut [Vec<H4>], lo: f64, hi: f64) -> Result<(), Unsupported> {
    let mut values: Vec<f64> = Vec::new();
    for &k in knots.iter() {
        if (lo..=hi).contains(&k) && values.last() != Some(&k) {
            values.push(k);
        }
    }
    for x in values {
        let have = mult(knots, x);
        for _ in have..p {
            insert(knots, p, curves, x)?;
        }
    }
    Ok(())
}

fn crease(knots: &[f64], p: usize, lo: f64, hi: f64) -> bool {
    let mut last = None;
    for &k in knots {
        if k > lo && k < hi && last != Some(k) {
            last = Some(k);
            if mult(knots, k) >= p {
                return true;
            }
        }
    }
    false
}

impl Patches {
    /// Decomposes `s` into Bézier patches.
    pub(super) fn new(s: &NurbsSurface) -> Result<Patches, Unsupported> {
        let (p, q) = (s.degree_u(), s.degree_v());
        let (n_u, n_v) = s.shape();
        let ((ulo, uhi), (vlo, vhi)) = s.domain();
        let mut ku = s.knots_u().as_slice().to_vec();
        let mut kv = s.knots_v().as_slice().to_vec();
        if p == 0 || q == 0 {
            return Err(Unsupported::KnotLayout);
        }
        if crease(&ku, p, ulo, uhi) || crease(&kv, q, vlo, vhi) {
            return Err(Unsupported::Crease);
        }
        let ctrl = s.ctrl();
        let w = s.weights();
        let mut scale = 0.0;
        // Columns (fixed j) as curves in u.
        let mut cols: Vec<Vec<H4>> = Vec::with_capacity(n_v);
        for j in 0..n_v {
            let mut col = Vec::with_capacity(n_u);
            for i in 0..n_u {
                let ix = i * n_v + j;
                let pt = ctrl.get(ix).copied().ok_or(Unsupported::KnotLayout)?;
                let wt = match w {
                    Some(ws) => ws.get(ix).copied().ok_or(Unsupported::KnotLayout)?,
                    None => 1.0,
                };
                for &c in &pt {
                    scale = fmax(scale, c.abs());
                }
                col.push([pt[0] * wt, pt[1] * wt, pt[2] * wt, wt]);
            }
            cols.push(col);
        }
        refine(&mut ku, p, &mut cols, ulo, uhi)?;
        let n_u2 = cols.first().map_or(0, |c| c.len());
        // Rows (fixed i) as curves in v.
        let mut rows: Vec<Vec<H4>> = Vec::with_capacity(n_u2);
        for i in 0..n_u2 {
            let mut row = Vec::with_capacity(n_v);
            for col in &cols {
                row.push(col.get(i).copied().ok_or(Unsupported::KnotLayout)?);
            }
            rows.push(row);
        }
        refine(&mut kv, q, &mut rows, vlo, vhi)?;
        let n_v2 = rows.first().map_or(0, |r| r.len());
        let su = spans(&ku, p, n_u2)?;
        let sv = spans(&kv, q, n_v2)?;
        let mut list = Vec::with_capacity(su.len() * sv.len());
        for &(k, ur) in &su {
            for &(l, vr) in &sv {
                let mut net = Vec::with_capacity((p + 1) * (q + 1));
                for a in 0..=p {
                    let row = rows.get(k - p + a).ok_or(Unsupported::KnotLayout)?;
                    for b in 0..=q {
                        net.push(row.get(l - q + b).copied().ok_or(Unsupported::KnotLayout)?);
                    }
                }
                list.push(Patch { u: ur, v: vr, net });
            }
        }
        if list.is_empty() {
            return Err(Unsupported::KnotLayout);
        }
        let rational = s.is_rational();
        let bin = Binom::new(3 * (p + q) + 2);
        Ok(Patches { p, q, rational, list, bin, scale })
    }

    /// Number of Bézier patches.
    pub(super) fn len(&self) -> usize {
        self.list.len()
    }

    fn projected(h: &H4) -> [f64; 3] {
        [h[0] / h[3], h[1] / h[3], h[2] / h[3]]
    }

    /// Box lower bound of a net (distance from `q` to the box of its projected points, less
    /// the safety margin).
    fn lower_bound(&self, net: &[H4], q: [f64; 3]) -> Result<f64, FaceError> {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for h in net {
            let x = Self::projected(h);
            for c in 0..3 {
                if x[c] < lo[c] {
                    lo[c] = x[c];
                }
                if x[c] > hi[c] {
                    hi[c] = x[c];
                }
            }
        }
        let mut dl = [0.0; 3];
        let mut m = self.scale;
        for c in 0..3 {
            if q[c] < lo[c] {
                dl[c] = lo[c] - q[c];
            } else if q[c] > hi[c] {
                dl[c] = q[c] - hi[c];
            }
            m = fmax(m, q[c].abs());
        }
        let d = super::norm3(dl)?;
        Ok(d - PRUNE_REL * m)
    }

    /// `H_s` (`along_t == false`) or `H_t` of a node net for query `q`.
    fn grad_poly(&self, net: &[H4], q: [f64; 3], along_t: bool) -> Poly {
        let (p, qd) = (self.p, self.q);
        let at = |a: usize, b: usize| net[a * (qd + 1) + b];
        // Component `c` (0..3 for X, 3 for W) as a polynomial of degree (p, q).
        let comp = |c: usize| -> Poly {
            let mut out = Poly::zero(p, qd);
            for a in 0..=p {
                for b in 0..=qd {
                    let x = at(a, b)[c];
                    out.c[a * (qd + 1) + b] = x;
                    out.mag[a * (qd + 1) + b] = x.abs();
                }
            }
            out
        };
        // Derivative of component `c` along the chosen direction.
        let deriv = |c: usize| -> Poly {
            let (dm, dn, deg) = if along_t { (p, qd - 1, qd) } else { (p - 1, qd, p) };
            let mut out = Poly::zero(dm, dn);
            for a in 0..=dm {
                for b in 0..=dn {
                    let (x1, x0) =
                        if along_t { (at(a, b + 1)[c], at(a, b)[c]) } else { (at(a + 1, b)[c], at(a, b)[c]) };
                    let ix = a * (dn + 1) + b;
                    out.c[ix] = deg as f64 * (x1 - x0);
                    out.mag[ix] = deg as f64 * (x1.abs() + x0.abs());
                }
            }
            out
        };
        let w = comp(3);
        let mut total: Option<Poly> = None;
        for (c, &qc) in q.iter().enumerate() {
            let x = comp(c);
            // E = X - q W.
            let mut e = x.clone();
            for k in 0..e.c.len() {
                e.c[k] = x.c[k] - qc * w.c[k];
                e.mag[k] = x.mag[k] + qc.abs() * w.mag[k];
            }
            let d = if self.rational {
                let a = prod(&deriv(c), &w, &self.bin);
                let b = prod(&x, &deriv(3), &self.bin);
                let mut d = a.clone();
                for k in 0..d.c.len() {
                    d.c[k] = a.c[k] - b.c[k];
                    d.mag[k] = a.mag[k] + b.mag[k];
                }
                d
            } else {
                deriv(c)
            };
            let h = prod(&d, &e, &self.bin);
            total = Some(match total {
                None => h,
                Some(mut t) => {
                    for k in 0..t.c.len() {
                        t.c[k] += h.c[k];
                        t.mag[k] += h.mag[k];
                    }
                    t
                }
            });
        }
        total.unwrap_or_else(|| Poly::zero(0, 0))
    }

    /// Positive definiteness of the symmetric part of the Jacobian of `(H_s, H_t)`.
    fn unique(hs: &Poly, ht: &Poly) -> bool {
        let (Some(a), Some(b), Some(c), Some(d)) =
            (hs.deriv_range(false), hs.deriv_range(true), ht.deriv_range(false), ht.deriv_range(true))
        else {
            return false;
        };
        if !(a.0 > 0.0 && d.0 > 0.0) {
            return false;
        }
        let (lo, hi) = (b.0 + c.0, b.1 + c.1);
        let mx = fmax(lo.abs(), hi.abs()) * 0.5;
        a.0 * d.0 > mx * mx
    }

    /// Halves a net along u (`along_t == false`) or v at its midpoint (de Casteljau).
    fn split(&self, net: &[H4], along_t: bool) -> (Vec<H4>, Vec<H4>) {
        let (p, q) = (self.p, self.q);
        let mut left = net.to_vec();
        let mut right = net.to_vec();
        let (lines, deg) = if along_t { (p + 1, q) } else { (q + 1, p) };
        for line in 0..lines {
            let ix = |k: usize| if along_t { line * (q + 1) + k } else { k * (q + 1) + line };
            let mut pts: Vec<H4> = (0..=deg).map(|k| net[ix(k)]).collect();
            // left[k] = pts[0] after k rounds; right[deg - k] = pts[deg - k] after k rounds.
            left[ix(0)] = pts[0];
            right[ix(deg)] = pts[deg];
            for r in 1..=deg {
                for k in 0..=(deg - r) {
                    pts[k] = core::array::from_fn(|c| (pts[k][c] + pts[k + 1][c]) * 0.5);
                }
                left[ix(r)] = pts[0];
                right[ix(deg - r)] = pts[deg - r];
            }
        }
        (left, right)
    }

    /// True when the projected net extends further along v than along u.
    fn split_along_t(&self, net: &[H4]) -> bool {
        let (p, q) = (self.p, self.q);
        let pr = |a: usize, b: usize| Self::projected(&net[a * (q + 1) + b]);
        let (mut ds, mut dt) = (0.0, 0.0);
        for a in 0..=p {
            for b in 0..=q {
                let h = pr(a, b);
                if a < p {
                    let n = pr(a + 1, b);
                    for c in 0..3 {
                        ds = fmax(ds, (n[c] - h[c]).abs());
                    }
                }
                if b < q {
                    let n = pr(a, b + 1);
                    for c in 0..3 {
                        dt = fmax(dt, (n[c] - h[c]).abs());
                    }
                }
            }
        }
        dt > ds
    }

    /// Branch and bound for stationary points with distance at most `best`. `accept(uv, foot)`
    /// says whether a stationary point lies in the trimmed face; accepted points lower
    /// `best`.
    pub(super) fn search(
        &self,
        s: &NurbsSurface,
        q: [f64; 3],
        mut best: f64,
        accept: &mut dyn FnMut([f64; 2], [f64; 3]) -> Result<bool, FaceError>,
    ) -> Result<Search, FaceError> {
        let mut out = Search::default();
        let mut heap: BinaryHeap<Reverse<(u64, u64)>> = BinaryHeap::new();
        let mut store: Vec<Option<Node>> = Vec::new();
        for pt in &self.list {
            let lb = self.lower_bound(&pt.net, q)?;
            let seq = store.len() as u64;
            store.push(Some(Node { u: pt.u, v: pt.v, net: pt.net.clone(), depth: 0, lb }));
            heap.push(Reverse((ord_key(lb), seq)));
        }
        let mut m = self.scale;
        for &c in &q {
            m = fmax(m, c.abs());
        }
        let mut visited = 0usize;
        while let Some(Reverse((_, seq))) = heap.pop() {
            let Some(node) = store.get_mut(seq as usize).and_then(Option::take) else { continue };
            if node.lb > best {
                break;
            }
            visited += 1;
            if visited > BUDGET {
                out.unresolved = true;
                break;
            }
            let hs = self.grad_poly(&node.net, q, false);
            let ht = self.grad_poly(&node.net, q, true);
            if hs.signed() || ht.signed() {
                continue;
            }
            let unique = Self::unique(&hs, &ht);
            if unique || node.depth >= MAX_DEPTH {
                if !unique {
                    out.unresolved = true;
                }
                let seed = [mid(node.u), mid(node.v)];
                match newton(s, q, [node.u[0], node.v[0]], [node.u[1], node.v[1]], seed, m)? {
                    NewtonEnd::Critical(f) => {
                        if f.distance <= best && accept(f.uv, f.foot)? {
                            best = f.distance;
                            out.found.push(f);
                        }
                    }
                    // The minimum of the node is on its box: no stationary point inside.
                    NewtonEnd::Boundary => {}
                    // Neither: the node is not resolved (fails closed).
                    NewtonEnd::Failed => out.unresolved = true,
                }
                continue;
            }
            let along_t = self.split_along_t(&node.net);
            let (l, r) = self.split(&node.net, along_t);
            let (lu, ru, lv, rv) = if along_t {
                (node.u, node.u, [node.v[0], mid(node.v)], [mid(node.v), node.v[1]])
            } else {
                ([node.u[0], mid(node.u)], [mid(node.u), node.u[1]], node.v, node.v)
            };
            for (net, u, v) in [(l, lu, lv), (r, ru, rv)] {
                let lb = self.lower_bound(&net, q)?;
                if lb <= best {
                    let seq = store.len() as u64;
                    store.push(Some(Node { u, v, net, depth: node.depth + 1, lb }));
                    heap.push(Reverse((ord_key(lb), seq)));
                }
            }
        }
        Ok(out)
    }
}

fn mid(r: [f64; 2]) -> f64 {
    r[0] + (r[1] - r[0]) * 0.5
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

/// How a [`newton`] run ended.
#[derive(Debug, Clone, Copy)]
pub(super) enum NewtonEnd {
    /// A stationary point (orthogonality test, or a converged full Newton step).
    Critical(Found),
    /// A point of the box boundary that meets the Karush-Kuhn-Tucker conditions of the box
    /// without being stationary: the minimum of the box lies on its boundary.
    Boundary,
    /// Neither (the iteration stalled or ran out of steps).
    Failed,
}

/// Per direction: orthogonal (`g^2 <= e T^2`), or at a bound with the gradient pointing out
/// of the box.
fn kkt_dir(x: f64, lo: f64, hi: f64, g: f64, e: f64, t2: f64) -> (bool, bool) {
    let ortho = g * g <= e * t2;
    let outward = (x == lo && g >= 0.0) || (x == hi && g <= 0.0);
    (ortho, ortho || outward)
}

/// Damped Newton iteration for a stationary point of `|S - q|^2 / 2` in the box
/// `[lo, hi]`. A point is stationary when it passes the orthogonality test
/// `|S_x . r| <= |S_x| T`, `T = max(REL max(d, FLOOR), NOISE m)`, in both directions, or when
/// the full Newton step (positive definite Hessian, inside the box) moves the foot by at most
/// `STEP_REL m`; the point returned is then the one after that step, evaluated.
pub(super) fn newton(
    s: &NurbsSurface,
    q: [f64; 3],
    lo: [f64; 2],
    hi: [f64; 2],
    seed: [f64; 2],
    m: f64,
) -> Result<NewtonEnd, FaceError> {
    let ev = |x: [f64; 2], order: usize| s.eval(x[0], x[1], order).map_err(|_| FaceError::NonFinite);
    let found = |x: [f64; 2]| -> Result<Found, FaceError> {
        let p = ev(x, 0)?.p;
        Ok(Found { uv: x, foot: p, distance: super::norm3(sub3(p, q))? })
    };
    let mut x = [clamp(seed[0], lo[0], hi[0]), clamp(seed[1], lo[1], hi[1])];
    for _ in 0..=MAX_ITER {
        let d = ev(x, 2)?;
        let (Some(su), Some(sv), Some(suu), Some(suv), Some(svv)) = (d.su, d.sv, d.suu, d.suv, d.svv) else {
            return Err(FaceError::NonFinite);
        };
        let r = sub3(d.p, q);
        let dist = super::norm3(r)?;
        let (gu, gv) = (dot3(su, r), dot3(sv, r));
        let (eu, ev2) = (dot3(su, su), dot3(sv, sv));
        let t = fmax(REL * fmax(dist, FLOOR), NOISE * m);
        let t2 = t * t;
        let (ou, ku) = kkt_dir(x[0], lo[0], hi[0], gu, eu, t2);
        let (ov, kv) = kkt_dir(x[1], lo[1], hi[1], gv, ev2, t2);
        if ou && ov {
            return Ok(NewtonEnd::Critical(Found { uv: x, foot: d.p, distance: dist }));
        }
        if ku && kv {
            return Ok(NewtonEnd::Boundary);
        }
        let fu = !((x[0] == lo[0] && gu > 0.0) || (x[0] == hi[0] && gu < 0.0));
        let fv = !((x[1] == lo[1] && gv > 0.0) || (x[1] == hi[1] && gv < 0.0));
        let a = eu + dot3(r, suu);
        let b = dot3(su, sv) + dot3(r, suv);
        let c = ev2 + dot3(r, svv);
        let step1 = |g: f64, h: f64, e: f64| -> f64 {
            if h > 0.0 {
                -g / h
            } else if e > 0.0 {
                -g / e
            } else {
                0.0
            }
        };
        let mut newton_pd = false;
        let (du, dv) = if fu && fv {
            let det = a * c - b * b;
            if a > 0.0 && det > 0.0 {
                newton_pd = true;
                let (du, dv) = (-(c * gu - b * gv) / det, -(a * gv - b * gu) / det);
                // Converged: the full Newton step moves the foot by at most STEP_REL m; the
                // point after the step is returned.
                let mv: [f64; 3] = core::array::from_fn(|k| du * su[k] + dv * sv[k]);
                let xn = [x[0] + du, x[1] + dv];
                let inside = (lo[0]..=hi[0]).contains(&xn[0]) && (lo[1]..=hi[1]).contains(&xn[1]);
                if inside && super::norm3(mv)? <= STEP_REL * m {
                    return Ok(NewtonEnd::Critical(found(xn)?));
                }
                (du, dv)
            } else {
                (step1(gu, 0.0, eu), step1(gv, 0.0, ev2))
            }
        } else {
            // One direction is held at a bound with the gradient pointing out of the box: a
            // one-dimensional Newton step in the other. A converged step (as above) ends at a
            // point that meets the box conditions.
            let (du, dv, h, sx) = if fu { (step1(gu, a, eu), 0.0, a, su) } else { (0.0, step1(gv, c, ev2), c, sv) };
            if h > 0.0 {
                newton_pd = true;
                let st = if fu { du } else { dv };
                let mv: [f64; 3] = core::array::from_fn(|k| st * sx[k]);
                let xn = [x[0] + du, x[1] + dv];
                let inside = (lo[0]..=hi[0]).contains(&xn[0]) && (lo[1]..=hi[1]).contains(&xn[1]);
                if inside && super::norm3(mv)? <= STEP_REL * m {
                    return Ok(NewtonEnd::Boundary);
                }
            }
            (du, dv)
        };
        let f0 = dot3(r, r);
        let mut lam = 1.0;
        let mut next = None;
        for _ in 0..MAX_HALVINGS {
            let xn = [clamp(x[0] + lam * du, lo[0], hi[0]), clamp(x[1] + lam * dv, lo[1], hi[1])];
            if xn == x {
                break;
            }
            let pn = ev(xn, 0)?.p;
            let rn = sub3(pn, q);
            let fnew = dot3(rn, rn);
            if fnew < f0 || (newton_pd && lam == 1.0 && fnew <= f0 + FLAT_REL * f0) {
                next = Some(xn);
                break;
            }
            lam *= 0.5;
        }
        match next {
            Some(xn) => x = xn,
            None => break,
        }
    }
    // Final test at the last point.
    let d = ev(x, 1)?;
    let (Some(su), Some(sv)) = (d.su, d.sv) else { return Err(FaceError::NonFinite) };
    let r = sub3(d.p, q);
    let dist = super::norm3(r)?;
    let t = fmax(REL * fmax(dist, FLOOR), NOISE * m);
    let t2 = t * t;
    let (ou, ku) = kkt_dir(x[0], lo[0], hi[0], dot3(su, r), dot3(su, su), t2);
    let (ov, kv) = kkt_dir(x[1], lo[1], hi[1], dot3(sv, r), dot3(sv, sv), t2);
    Ok(if ou && ov {
        NewtonEnd::Critical(Found { uv: x, foot: d.p, distance: dist })
    } else if ku && kv {
        NewtonEnd::Boundary
    } else {
        NewtonEnd::Failed
    })
}
