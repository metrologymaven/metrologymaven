//! Bézier decomposition and Bernstein arithmetic for point inversion. Spec node
//! `GEO.NURBS.INVERT` (Design, items 1 to 4).
//!
//! Every value-affecting choice is pinned in `spec/geo/nurbs/invert/NODE.md` and mirrored bit for
//! bit by `fixtures/geo/nurbs/invert/invert_model.py`: plain IEEE 754 binary64 `+ - * /`, each
//! written out (no `mul_add`), comparisons instead of `f64::min`/`f64::max`, and every loop in
//! the order given here. Arrays are row-major `(m + 1) x (n + 1)`: rows along the first
//! Bézier parameter `s`, columns along the second `t` (one column for a curve).

use crate::data::MAX_DEGREE;

/// Largest Bernstein degree that occurs: `3 * MAX_DEGREE` (the `G` polynomials).
pub(crate) const MAX_BINOM: usize = 3 * MAX_DEGREE;

/// A homogeneous control point `(w x, w y, w z, w)`.
pub(crate) type H = [f64; 4];

/// Binomial coefficients `C(n, k)` for `n <= MAX_BINOM` by Pascal's rule in binary64 (exact up
/// to `n = 56`, rounded above; the rounding is pinned, not a source of error bounds).
pub(crate) struct Binom {
    rows: Vec<Vec<f64>>,
}

impl Binom {
    pub(crate) fn new() -> Self {
        let mut rows: Vec<Vec<f64>> = vec![vec![1.0]];
        for n in 1..=MAX_BINOM {
            let mut row = vec![1.0; n + 1];
            if let Some(prev) = rows.last() {
                for (k, x) in row.iter_mut().enumerate().take(n).skip(1) {
                    if let (Some(a), Some(b)) = (prev.get(k - 1), prev.get(k)) {
                        *x = a + b;
                    }
                }
            }
            rows.push(row);
        }
        Binom { rows }
    }

    fn c(&self, n: usize, k: usize) -> f64 {
        self.rows.get(n).and_then(|r| r.get(k)).copied().unwrap_or(0.0)
    }

    /// Product weight `(C(m, i) C(n, j)) / C(m + n, i + j)`.
    fn weight(&self, m: usize, n: usize, i: usize, j: usize) -> f64 {
        (self.c(m, i) * self.c(n, j)) / self.c(m + n, i + j)
    }
}

/// A tensor-product Bernstein array of degree `(m, n)`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Arr {
    pub(crate) m: usize,
    pub(crate) n: usize,
    pub(crate) a: Vec<f64>,
}

impl Arr {
    pub(crate) fn zeros(m: usize, n: usize) -> Self {
        Arr { m, n, a: vec![0.0; (m + 1) * (n + 1)] }
    }

    #[inline]
    pub(crate) fn at(&self, i: usize, j: usize) -> f64 {
        // in range by construction; 0.0 is never read for a well-formed array
        self.a.get(i * (self.n + 1) + j).copied().unwrap_or(0.0)
    }

    #[inline]
    fn set(&mut self, i: usize, j: usize, x: f64) {
        let n = self.n;
        if let Some(y) = self.a.get_mut(i * (n + 1) + j) {
            *y = x;
        }
    }

    fn map2(&self, b: &Arr, f: impl Fn(f64, f64) -> f64) -> Arr {
        Arr { m: self.m, n: self.n, a: self.a.iter().zip(&b.a).map(|(x, y)| f(*x, *y)).collect() }
    }

    pub(crate) fn add(&self, b: &Arr) -> Arr {
        self.map2(b, |x, y| x + y)
    }

    pub(crate) fn sub(&self, b: &Arr) -> Arr {
        self.map2(b, |x, y| x - y)
    }

    pub(crate) fn abs(&self) -> Arr {
        Arr { m: self.m, n: self.n, a: self.a.iter().map(|x| x.abs()).collect() }
    }

    /// Forward differences along `s`: degree `(m - 1, n)`, `a[i+1][j] - a[i][j]` (no factor).
    pub(crate) fn diff_s(&self) -> Arr {
        let mut out = Arr::zeros(self.m - 1, self.n);
        for i in 0..self.m {
            for j in 0..=self.n {
                out.set(i, j, self.at(i + 1, j) - self.at(i, j));
            }
        }
        out
    }

    /// Forward differences along `t`: degree `(m, n - 1)`.
    pub(crate) fn diff_t(&self) -> Arr {
        let mut out = Arr::zeros(self.m, self.n - 1);
        for i in 0..=self.m {
            for j in 0..self.n {
                out.set(i, j, self.at(i, j + 1) - self.at(i, j));
            }
        }
        out
    }

    /// Bernstein product: `c[k][l] = sum_i sum_j ((wu wv) a[i][j]) b[k-i][l-j]`, `i` then `j`
    /// ascending, accumulator starting at `0.0`, `wu = weight(m1, m2, i, k - i)`.
    pub(crate) fn prod(&self, b: &Arr, bn: &Binom) -> Arr {
        let (m1, n1, m2, n2) = (self.m, self.n, b.m, b.n);
        let mut out = Arr::zeros(m1 + m2, n1 + n2);
        for k in 0..=m1 + m2 {
            let i_lo = k.saturating_sub(m2);
            let i_hi = if m1 < k { m1 } else { k };
            for l in 0..=n1 + n2 {
                let j_lo = l.saturating_sub(n2);
                let j_hi = if n1 < l { n1 } else { l };
                let mut acc = 0.0;
                for i in i_lo..=i_hi {
                    let wu = bn.weight(m1, m2, i, k - i);
                    for j in j_lo..=j_hi {
                        let wv = bn.weight(n1, n2, j, l - j);
                        acc += ((wu * wv) * self.at(i, j)) * b.at(k - i, l - j);
                    }
                }
                out.set(k, l, acc);
            }
        }
        out
    }

    /// De Casteljau subdivision at `s = 1/2`: midpoints `(x + y) * 0.5`.
    pub(crate) fn split_s(&self) -> (Arr, Arr) {
        let (m, n) = (self.m, self.n);
        let mut work = self.clone();
        let mut left = Arr::zeros(m, n);
        let mut right = Arr::zeros(m, n);
        for j in 0..=n {
            left.set(0, j, work.at(0, j));
            right.set(m, j, work.at(m, j));
        }
        for r in 1..=m {
            for i in 0..=m - r {
                for j in 0..=n {
                    work.set(i, j, (work.at(i, j) + work.at(i + 1, j)) * 0.5);
                }
            }
            for j in 0..=n {
                left.set(r, j, work.at(0, j));
                right.set(m - r, j, work.at(m - r, j));
            }
        }
        (left, right)
    }

    /// De Casteljau subdivision at `t = 1/2`.
    pub(crate) fn split_t(&self) -> (Arr, Arr) {
        let (l, r) = self.transpose().split_s();
        (l.transpose(), r.transpose())
    }

    fn transpose(&self) -> Arr {
        let mut out = Arr::zeros(self.n, self.m);
        for i in 0..=self.m {
            for j in 0..=self.n {
                out.set(j, i, self.at(i, j));
            }
        }
        out
    }

    pub(crate) fn all_finite(&self) -> bool {
        self.a.iter().all(|x| x.is_finite())
    }
}

/// A homogeneous Bézier net: the four components `w x`, `w y`, `w z`, `w` as arrays.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Net {
    pub(crate) c: [Arr; 4],
}

impl Net {
    pub(crate) fn from_points(m: usize, n: usize, pts: &[H]) -> Net {
        let comp = |k: usize| Arr { m, n, a: pts.iter().map(|p| p[k]).collect() };
        Net { c: [comp(0), comp(1), comp(2), comp(3)] }
    }

    pub(crate) fn m(&self) -> usize {
        self.c[0].m
    }

    pub(crate) fn n(&self) -> usize {
        self.c[0].n
    }

    pub(crate) fn point(&self, i: usize, j: usize) -> H {
        [self.c[0].at(i, j), self.c[1].at(i, j), self.c[2].at(i, j), self.c[3].at(i, j)]
    }

    /// Projected control point `(wx / w, wy / w, wz / w)`.
    pub(crate) fn projected(&self, i: usize, j: usize) -> [f64; 3] {
        let h = self.point(i, j);
        [h[0] / h[3], h[1] / h[3], h[2] / h[3]]
    }

    pub(crate) fn split_s(&self) -> (Net, Net) {
        let [a, b, c, d] = &self.c;
        let (a0, a1) = a.split_s();
        let (b0, b1) = b.split_s();
        let (c0, c1) = c.split_s();
        let (d0, d1) = d.split_s();
        (Net { c: [a0, b0, c0, d0] }, Net { c: [a1, b1, c1, d1] })
    }

    pub(crate) fn split_t(&self) -> (Net, Net) {
        let [a, b, c, d] = &self.c;
        let (a0, a1) = a.split_t();
        let (b0, b1) = b.split_t();
        let (c0, c1) = c.split_t();
        let (d0, d1) = d.split_t();
        (Net { c: [a0, b0, c0, d0] }, Net { c: [a1, b1, c1, d1] })
    }

    /// Row `i` (fixed `s` index) as a one-column net along `t`.
    pub(crate) fn row(&self, i: usize) -> Net {
        let n = self.n();
        let pts: Vec<H> = (0..=n).map(|j| self.point(i, j)).collect();
        Net::from_points(n, 0, &pts)
    }

    /// Column `j` (fixed `t` index) as a one-column net along `s`.
    pub(crate) fn col(&self, j: usize) -> Net {
        let m = self.m();
        let pts: Vec<H> = (0..=m).map(|i| self.point(i, j)).collect();
        Net::from_points(m, 0, &pts)
    }
}

/// `G` and its magnitude bound `Gm` along `s` (`axis_t = false`) or `t` for the query `q`:
/// `G = sum_c (dA_c (x) w - A_c (x) dw) (x) (A_c - q_c w)`, `c = x, y, z` accumulated in that
/// order, and `Gm` the same with absolute values and `|A_c| + |q_c| w` (Design item 3).
pub(crate) fn g_poly(net: &Net, q: [f64; 3], axis_t: bool, bn: &Binom) -> (Arr, Arr) {
    let diff = |a: &Arr| if axis_t { a.diff_t() } else { a.diff_s() };
    let w = &net.c[3];
    let dw = diff(w);
    let adw = dw.abs();
    let mut g: Option<(Arr, Arr)> = None;
    for (c, qc) in q.iter().enumerate() {
        let a = &net.c[c];
        let da = diff(a);
        let nn = da.prod(w, bn).sub(&a.prod(&dw, bn));
        let nm = da.abs().prod(w, bn).add(&a.abs().prod(&adw, bn));
        let aq = qc.abs();
        let r = Arr { m: a.m, n: a.n, a: a.a.iter().zip(&w.a).map(|(x, y)| x - qc * y).collect() };
        let rm = Arr { m: a.m, n: a.n, a: a.a.iter().zip(&w.a).map(|(x, y)| x.abs() + aq * y).collect() };
        let (p, pm) = (nn.prod(&r, bn), nm.prod(&rm, bn));
        g = Some(match g {
            None => (p, pm),
            Some((g0, gm0)) => (g0.add(&p), gm0.add(&pm)),
        });
    }
    g.unwrap_or_else(|| (Arr::zeros(0, 0), Arr::zeros(0, 0)))
}

/// De Casteljau evaluation of a 4-vector polygon at `s`: `(A, A', A'')` in `s` (Design item 6).
/// `oms = 1 - s`, each level `oms * a + s * b`; `A' = (b1 - b0) * p` from level `p - 1`,
/// `A'' = ((c0 - 2 c1) + c2) * (p (p - 1))` from level `p - 2` (zero for `p = 1` or `nd < 2`).
pub(crate) fn casteljau(pts: &[H], s: f64, nd: usize) -> (H, H, H) {
    let p = pts.len().saturating_sub(1);
    let oms = 1.0 - s;
    let mut c: Vec<H> = pts.to_vec();
    let z: H = [0.0; 4];
    let at = |c: &[H], i: usize| c.get(i).copied().unwrap_or(z);
    let mut keep1: [H; 2] = [z; 2];
    let mut keep2: [H; 3] = [z; 3];
    if p == 1 {
        keep1 = [at(pts, 0), at(pts, 1)];
    }
    for r in 1..=p {
        if r + 1 == p {
            keep2 = [at(&c, 0), at(&c, 1), at(&c, 2)];
        }
        if r == p {
            keep1 = [at(&c, 0), at(&c, 1)];
        }
        for i in 0..=p - r {
            let (a, b) = (at(&c, i), at(&c, i + 1));
            if let Some(ci) = c.get_mut(i) {
                *ci = [oms * a[0] + s * b[0], oms * a[1] + s * b[1], oms * a[2] + s * b[2], oms * a[3] + s * b[3]];
            }
        }
    }
    let pf = p as f64;
    let a0 = at(&c, 0);
    let d = |x: f64, y: f64| (y - x) * pf;
    let a1 = [
        d(keep1[0][0], keep1[1][0]),
        d(keep1[0][1], keep1[1][1]),
        d(keep1[0][2], keep1[1][2]),
        d(keep1[0][3], keep1[1][3]),
    ];
    let mut a2 = [0.0; 4];
    if nd >= 2 && p >= 2 {
        let k = pf * (pf - 1.0);
        for (x, ((a, b), c2)) in a2.iter_mut().zip(keep2[0].iter().zip(&keep2[1]).zip(&keep2[2])) {
            *x = ((a - 2.0 * b) + c2) * k;
        }
    }
    (a0, a1, a2)
}

fn quot(a: H, w0: f64) -> [f64; 3] {
    [a[0] / w0, a[1] / w0, a[2] / w0]
}

/// Position and derivatives in `s` of a one-column net (the quotient rule of GEO.NURBS.EVAL).
pub(crate) fn bez1(net: &Net, s: f64) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let pts: Vec<H> = (0..=net.m()).map(|i| net.point(i, 0)).collect();
    let (a0, a1, a2) = casteljau(&pts, s, 2);
    let (w0, w1, w2) = (a0[3], a1[3], a2[3]);
    let c0 = quot(a0, w0);
    let c1 = [(a1[0] - w1 * c0[0]) / w0, (a1[1] - w1 * c0[1]) / w0, (a1[2] - w1 * c0[2]) / w0];
    let c2f = |k: usize| ((a2[k] - (2.0 * w1) * c1[k]) - w2 * c0[k]) / w0;
    (c0, c1, [c2f(0), c2f(1), c2f(2)])
}

/// Position only of a one-column net.
pub(crate) fn bez1_pos(net: &Net, s: f64) -> [f64; 3] {
    let pts: Vec<H> = (0..=net.m()).map(|i| net.point(i, 0)).collect();
    let (a0, _, _) = casteljau(&pts, s, 0);
    quot(a0, a0[3])
}

/// Surface derivatives in `(s, t)` of a tensor net: `(S, S_s, S_t, S_ss, S_st, S_tt)`. Each row
/// is reduced along `t` first (giving value, first and second derivative per row), then the
/// three resulting polygons along `s`.
pub(crate) type SDerivs = ([f64; 3], [f64; 3], [f64; 3], [f64; 3], [f64; 3], [f64; 3]);

pub(crate) fn bez2(net: &Net, s: f64, t: f64) -> SDerivs {
    let (m, n) = (net.m(), net.n());
    let mut t0 = Vec::with_capacity(m + 1);
    let mut t1 = Vec::with_capacity(m + 1);
    let mut t2 = Vec::with_capacity(m + 1);
    for i in 0..=m {
        let row: Vec<H> = (0..=n).map(|j| net.point(i, j)).collect();
        let (a, b, c) = casteljau(&row, t, 2);
        t0.push(a);
        t1.push(b);
        t2.push(c);
    }
    let (a00, a10, a20) = casteljau(&t0, s, 2);
    let (a01, a11, _) = casteljau(&t1, s, 1);
    let (a02, _, _) = casteljau(&t2, s, 0);
    let w00 = a00[3];
    let s00 = quot(a00, w00);
    let (w10, w01) = (a10[3], a01[3]);
    let s10 = [(a10[0] - w10 * s00[0]) / w00, (a10[1] - w10 * s00[1]) / w00, (a10[2] - w10 * s00[2]) / w00];
    let s01 = [(a01[0] - w01 * s00[0]) / w00, (a01[1] - w01 * s00[1]) / w00, (a01[2] - w01 * s00[2]) / w00];
    let (w20, w11, w02) = (a20[3], a11[3], a02[3]);
    let f20 = |k: usize| ((a20[k] - (2.0 * w10) * s10[k]) - w20 * s00[k]) / w00;
    let f11 = |k: usize| (((a11[k] - w10 * s01[k]) - w01 * s10[k]) - w11 * s00[k]) / w00;
    let f02 = |k: usize| ((a02[k] - (2.0 * w01) * s01[k]) - w02 * s00[k]) / w00;
    (s00, s10, s01, [f20(0), f20(1), f20(2)], [f11(0), f11(1), f11(2)], [f02(0), f02(1), f02(2)])
}

/// Position only of a tensor net.
pub(crate) fn bez2_pos(net: &Net, s: f64, t: f64) -> [f64; 3] {
    let (m, n) = (net.m(), net.n());
    let mut t0 = Vec::with_capacity(m + 1);
    for i in 0..=m {
        let row: Vec<H> = (0..=n).map(|j| net.point(i, j)).collect();
        t0.push(casteljau(&row, t, 0).0);
    }
    let (a00, _, _) = casteljau(&t0, s, 0);
    quot(a00, a00[3])
}

/// Knot insertion (Boehm, one knot at a time): every distinct knot value of the domain
/// `[U[p], U[n]]`, ascending, is inserted until its multiplicity is at least `p`; every polygon
/// of `polys` (sharing the knot vector) is refined alike. New points
/// `Q_i = alpha * P_i + (1 - alpha) * P_{i-1}`, `alpha = (x - U[i]) / (U[i+p] - U[i])` for
/// `i = k-p+1 ..= k-m` (`k` the last index with `U[k] <= x`, `m` the current multiplicity).
/// Returns the refined knots, the polygons and the distinct domain values (Design item 1).
pub(crate) fn insert_all(knots: &[f64], p: usize, polys: &mut [Vec<H>]) -> (Vec<f64>, Vec<f64>) {
    let mut u = knots.to_vec();
    let n = u.len().saturating_sub(p + 1);
    let (Some(&a), Some(&b)) = (u.get(p), u.get(n)) else { return (u, Vec::new()) };
    let mut values: Vec<f64> = Vec::new();
    for &x in &u {
        if a <= x && x <= b && values.last() != Some(&x) {
            values.push(x);
        }
    }
    for &x in &values {
        loop {
            let m = u.iter().filter(|y| **y == x).count();
            if m >= p {
                break;
            }
            let Some(k) = u.iter().rposition(|y| *y <= x) else { break };
            if k < p {
                break;
            }
            let knot = |i: usize| u.get(i).copied().unwrap_or(0.0);
            for poly in polys.iter_mut() {
                let mut q: Vec<H> = Vec::with_capacity(p - m);
                for i in (k + 1 - p)..=(k - m) {
                    let alpha = (x - knot(i)) / (knot(i + p) - knot(i));
                    let oma = 1.0 - alpha;
                    let (Some(&pi), Some(&pm)) = (poly.get(i), poly.get(i - 1)) else { continue };
                    q.push([
                        alpha * pi[0] + oma * pm[0],
                        alpha * pi[1] + oma * pm[1],
                        alpha * pi[2] + oma * pm[2],
                        alpha * pi[3] + oma * pm[3],
                    ]);
                }
                let tail: Vec<H> = poly.get(k - m..).map(<[H]>::to_vec).unwrap_or_default();
                poly.truncate(k + 1 - p);
                poly.extend(q);
                poly.extend(tail);
            }
            u.insert(k + 1, x);
        }
    }
    (u, values)
}

/// Bézier pieces after [`insert_all`]: `(first control index, x0, x1)` per non-empty span.
pub(crate) fn segments(u: &[f64], p: usize, values: &[f64]) -> Vec<(usize, f64, f64)> {
    values
        .windows(2)
        .filter_map(|w| {
            let (&x0, &x1) = (w.first()?, w.get(1)?);
            let s = u.iter().rposition(|y| *y == x0)?;
            Some((s.checked_sub(p)?, x0, x1))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_of_linear_polynomials_is_hand_checked() {
        // (1 - s) * s in Bernstein form of degree 2: [0, 1/2, 0]
        let bn = Binom::new();
        let a = Arr { m: 1, n: 0, a: vec![1.0, 0.0] };
        let b = Arr { m: 1, n: 0, a: vec![0.0, 1.0] };
        assert_eq!(a.prod(&b, &bn).a, vec![0.0, 0.5, 0.0]);
    }

    #[test]
    fn split_of_quadratic_is_hand_checked() {
        let a = Arr { m: 2, n: 0, a: vec![0.0, 1.0, 0.0] };
        let (l, r) = a.split_s();
        assert_eq!(l.a, vec![0.0, 0.5, 0.5]);
        assert_eq!(r.a, vec![0.5, 0.5, 0.0]);
    }

    #[test]
    fn casteljau_of_quadratic_is_hand_checked() {
        let pts = [[0.0, 0.0, 0.0, 1.0], [1.0, 2.0, 0.0, 1.0], [2.0, 0.0, 0.0, 1.0]];
        let (a0, a1, a2) = casteljau(&pts, 0.5, 2);
        assert_eq!(a0, [1.0, 1.0, 0.0, 1.0]);
        assert_eq!(a1, [2.0, 0.0, 0.0, 0.0]);
        assert_eq!(a2, [0.0, -8.0, 0.0, 0.0]);
    }

    #[test]
    fn insertion_makes_bezier_pieces() {
        // degree 2, one interior knot 0.5: one insertion, two Bezier pieces
        let mut polys =
            vec![vec![[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0], [2.0, 0.0, 0.0, 1.0], [3.0, 0.0, 0.0, 1.0]]];
        let (u, values) = insert_all(&[0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0], 2, &mut polys);
        assert_eq!(u, vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0]);
        assert_eq!(values, vec![0.0, 0.5, 1.0]);
        assert_eq!(polys[0].len(), 5);
        assert_eq!(polys[0][2][0], 1.5);
        assert_eq!(segments(&u, 2, &values), vec![(0, 0.0, 0.5), (2, 0.5, 1.0)]);
    }
}
