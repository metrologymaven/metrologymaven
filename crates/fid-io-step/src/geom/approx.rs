//! Approximation of `offset_surface` by a NURBS, with the sampled approximation error
//! (REQ-IO.CAD.STEP.GEOM-003, -004).
//!
//! The exact surface is `E(u, v) = S(u, v) + d N(u, v)`, `S` the mapped basis surface and `N` its
//! unit normal (`Su x Sv / |Su x Sv|`, ISO 10303-42 `offset_surface` [unverified clause]). The
//! approximant is a non-rational tensor-product B-spline over the same parameters: in each
//! direction a clamped knot vector with single interior knots on a breakpoint grid, cubic in a
//! direction along which `E` is curved and linear in a direction along which it is exactly
//! linear (the rulings of a plane, cylinder, cone or extrusion basis). It interpolates `E` at the
//! tensor grid of Greville abscissae (Schoenberg-Whitney holds, the collocation matrices are
//! totally positive, so banded Gaussian elimination without pivoting is stable [de Boor, A
//! Practical Guide to Splines, 1978]), solved first along `u` for every `v` column, then along
//! `v` for every row.
//!
//! The error is `max |A(u, v) - E(u, v)|` over a grid of [`SAMPLES_PER_SPAN`] points per knot
//! span in each direction (so at least 33 x 33 per span pair), span end points shared. Measured
//! at equal parameters it bounds the distance from each sampled approximant point to the exact
//! surface from above. While it exceeds the tolerance, every curved direction's spans are halved,
//! at most [`MAX_REFINEMENTS`] times; a result still above is `approx_out_of_tolerance` with the
//! achieved error. Everything runs in a fixed order with `+ - * /` written out and `fid_num`
//! square roots, so the result is bit-identical on every platform (D0).

use super::place::{axpy, dot, sub};
use super::{Approx, ApproxStatus, Fail, GeomOptions, ParamBox, SurfaceRef};
use fid_geo_nurbs::{AnalyticSurface, NurbsSurface, NurbsSurfaceParts, Surface};

use core::f64::consts::TAU;

/// Refinements after the first fit (REQ-IO.CAD.STEP.GEOM-004).
pub const MAX_REFINEMENTS: u32 = 4;
/// Sample points per knot span and direction, end points included (REQ-IO.CAD.STEP.GEOM-003).
pub const SAMPLES_PER_SPAN: usize = 33;
/// Initial spans per full turn of a periodic analytic parameter.
pub const INITIAL_SPANS_PER_TURN: f64 = 8.0;
/// Largest number of knot spans in one direction of an approximant.
pub const MAX_FIT_SPANS: usize = 4096;

const CUBIC: usize = 3;

/// One parameter direction of the fit.
#[derive(Clone)]
struct Axis {
    /// Breakpoints, strictly increasing.
    breaks: Vec<f64>,
    /// True when `E` is curved along this direction (cubic, refined).
    curved: bool,
}

impl Axis {
    fn spans(&self) -> usize {
        self.breaks.len().saturating_sub(1)
    }

    fn degree(&self) -> usize {
        if self.curved {
            CUBIC
        } else {
            1
        }
    }

    /// Clamped knot vector with single interior knots.
    fn knots(&self) -> Vec<f64> {
        let q = self.degree();
        let mut k = Vec::with_capacity(self.breaks.len() + 2 * q);
        if let (Some(&lo), Some(&hi)) = (self.breaks.first(), self.breaks.last()) {
            k.extend(core::iter::repeat_n(lo, q));
            k.extend(self.breaks.iter().copied());
            k.extend(core::iter::repeat_n(hi, q));
        }
        k
    }

    /// Sample parameters: [`SAMPLES_PER_SPAN`] per span, span ends shared.
    fn samples(&self) -> Vec<f64> {
        let per = (SAMPLES_PER_SPAN - 1) as f64;
        let mut out = Vec::with_capacity(self.spans() * (SAMPLES_PER_SPAN - 1) + 1);
        for w in self.breaks.windows(2) {
            let (lo, hi) = (w[0], w[1]);
            for k in 0..SAMPLES_PER_SPAN - 1 {
                out.push(lo + (hi - lo) * (k as f64 / per));
            }
        }
        if let Some(&last) = self.breaks.last() {
            out.push(last);
        }
        out
    }

    fn sample_count(&self) -> usize {
        self.spans().saturating_mul(SAMPLES_PER_SPAN - 1).saturating_add(1)
    }

    /// Halves every span (curved directions only).
    fn refine(&mut self) {
        let mut out = Vec::with_capacity(self.breaks.len() * 2);
        for w in self.breaks.windows(2) {
            out.push(w[0]);
            out.push(w[0] + (w[1] - w[0]) / 2.0);
        }
        if let Some(&last) = self.breaks.last() {
            out.push(last);
        }
        self.breaks = out;
    }
}

/// The knot span index of `t` in `knots` (degree `q`, `n` basis functions): the largest `s` in
/// `q..n` with `knots[s] <= t`, `n - 1` at the right end.
fn find_span(knots: &[f64], q: usize, n: usize, t: f64) -> usize {
    if t >= knots[n] {
        return n - 1;
    }
    let (mut lo, mut hi) = (q, n);
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if t < knots[mid] {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    lo
}

/// The `q + 1` non-zero basis functions at `t` in span `s` (Cox-de Boor, The NURBS Book A2.2).
fn basis_funs(knots: &[f64], q: usize, s: usize, t: f64) -> [f64; CUBIC + 1] {
    let mut nb = [0.0_f64; CUBIC + 1];
    let mut left = [0.0_f64; CUBIC + 1];
    let mut right = [0.0_f64; CUBIC + 1];
    nb[0] = 1.0;
    for j in 1..=q {
        left[j] = t - knots[s + 1 - j];
        right[j] = knots[s + j] - t;
        let mut saved = 0.0;
        for (r, slot) in nb.iter_mut().enumerate().take(j) {
            let tmp = *slot / (right[r + 1] + left[j - r]);
            *slot = saved + right[r + 1] * tmp;
            saved = left[j - r] * tmp;
        }
        nb[j] = saved;
    }
    nb
}

/// LU factors (no pivoting) of a banded collocation matrix, stored dense row-major.
struct Banded {
    n: usize,
    band: usize,
    a: Vec<f64>,
}

impl Banded {
    /// Collocation matrix `N_j(xi_i)` at the Greville abscissae of `knots`, factored.
    fn collocation(knots: &[f64], q: usize) -> Result<(Banded, Vec<f64>), Fail> {
        let n = knots.len() - q - 1;
        let mut xi = Vec::with_capacity(n);
        for i in 0..n {
            let mut s = 0.0;
            for k in 1..=q {
                s += knots[i + k];
            }
            xi.push(s / q as f64);
        }
        let mut a = vec![0.0_f64; n * n];
        let mut band = 0usize;
        for (i, &t) in xi.iter().enumerate() {
            let s = find_span(knots, q, n, t);
            let nb = basis_funs(knots, q, s, t);
            for (r, &b) in nb.iter().enumerate().take(q + 1) {
                let j = s - q + r;
                a[i * n + j] = b;
                if b != 0.0 {
                    band = band.max(i.abs_diff(j));
                }
            }
        }
        let mut m = Banded { n, band, a };
        m.factor()?;
        Ok((m, xi))
    }

    fn factor(&mut self) -> Result<(), Fail> {
        let (n, w) = (self.n, self.band);
        for k in 0..n {
            let piv = self.a[k * n + k];
            if !piv.is_finite() || piv == 0.0 {
                return Err(Fail::Fit);
            }
            for i in k + 1..n.min(k + w + 1) {
                let f = self.a[i * n + k] / piv;
                self.a[i * n + k] = f;
                if f != 0.0 {
                    for j in k + 1..n.min(k + w + 1) {
                        self.a[i * n + j] -= f * self.a[k * n + j];
                    }
                }
            }
        }
        Ok(())
    }

    /// Solves in place for one right-hand side of points.
    fn solve(&self, b: &mut [[f64; 3]]) -> Result<(), Fail> {
        let (n, w) = (self.n, self.band);
        for k in 0..n {
            for i in k + 1..n.min(k + w + 1) {
                let f = self.a[i * n + k];
                if f != 0.0 {
                    b[i] = axpy(b[i], -f, b[k]);
                }
            }
        }
        for i in (0..n).rev() {
            let mut s = b[i];
            for (j, bj) in b.iter().enumerate().skip(i + 1).take(w) {
                s = axpy(s, -self.a[i * n + j], *bj);
            }
            let d = self.a[i * n + i];
            b[i] = [s[0] / d, s[1] / d, s[2] / d];
            if !b[i].iter().all(|c| c.is_finite()) {
                return Err(Fail::NonFinite);
            }
        }
        Ok(())
    }
}

/// The base surface behind the evaluation traits.
fn as_surface(base: &SurfaceRef) -> Result<&dyn Surface, Fail> {
    match base {
        SurfaceRef::Analytic(a) => Ok(a),
        SurfaceRef::Nurbs(s) | SurfaceRef::Extrusion { nurbs: s, .. } | SurfaceRef::Revolution { nurbs: s, .. } => {
            Ok(s)
        }
        SurfaceRef::Approximated(_) => Err(Fail::Unsupported { at: 0 }),
    }
}

/// `(u curved, v curved)` of the offset of `base`.
fn curvature_dirs(base: &SurfaceRef) -> (bool, bool) {
    match base {
        SurfaceRef::Analytic(AnalyticSurface::Plane(_)) => (false, false),
        SurfaceRef::Analytic(AnalyticSurface::Cylinder(_) | AnalyticSurface::Cone(_)) => (true, false),
        SurfaceRef::Extrusion { .. } => (true, false),
        _ => (true, true),
    }
}

/// Initial breakpoints of one direction over `[lo, hi]`.
fn initial_axis(base: &SurfaceRef, dir_u: bool, lo: f64, hi: f64, curved: bool) -> Result<Axis, Fail> {
    if !curved {
        return Ok(Axis { breaks: vec![lo, hi], curved });
    }
    let mut breaks = vec![lo];
    match base {
        SurfaceRef::Analytic(_) => {
            let k = INITIAL_SPANS_PER_TURN * ((hi - lo) / TAU);
            let k = if k.is_finite() && k > 2.0 { k.ceil() } else { 2.0 };
            if k > MAX_FIT_SPANS as f64 {
                return Err(Fail::ApproxBudget);
            }
            let k = k as usize;
            for i in 1..k {
                breaks.push(lo + (hi - lo) * (i as f64 / k as f64));
            }
        }
        SurfaceRef::Nurbs(s) | SurfaceRef::Extrusion { nurbs: s, .. } | SurfaceRef::Revolution { nurbs: s, .. } => {
            let kv = if dir_u { s.knots_u() } else { s.knots_v() };
            for &k in kv.as_slice() {
                if k > lo && k < hi && breaks.last().is_some_and(|&b| k > b) {
                    breaks.push(k);
                }
            }
        }
        SurfaceRef::Approximated(_) => return Err(Fail::Unsupported { at: 0 }),
    }
    breaks.push(hi);
    let mut axis = Axis { breaks, curved };
    if axis.spans() < 2 {
        axis.refine();
    }
    if axis.spans() > MAX_FIT_SPANS {
        return Err(Fail::ApproxBudget);
    }
    Ok(axis)
}

/// `E(u, v) = S(u, v) + d N(u, v)`.
fn exact(base: &dyn Surface, d: f64, u: f64, v: f64) -> Result<[f64; 3], Fail> {
    let e = base.eval(u, v, 1).map_err(|_| Fail::Fit)?;
    let (su, sv) = match (e.su, e.sv) {
        (Some(su), Some(sv)) => (su, sv),
        _ => return Err(Fail::Fit),
    };
    let nrm = fid_geo_nurbs::surface_normal(su, sv).map_err(|_| Fail::Fit)?.ok_or(Fail::DegenerateNormal)?;
    Ok(axpy(e.p, d, nrm))
}

/// One fit on the grid `(au, av)`.
fn fit(base: &dyn Surface, d: f64, au: &Axis, av: &Axis) -> Result<NurbsSurface, Fail> {
    let (ku, kv) = (au.knots(), av.knots());
    let (qu, qv) = (au.degree(), av.degree());
    let (mu, xi) = Banded::collocation(&ku, qu)?;
    let (mv, eta) = Banded::collocation(&kv, qv)?;
    let (n_u, n_v) = (xi.len(), eta.len());
    // net[i * n_v + j] = E(xi_i, eta_j)
    let mut net = Vec::with_capacity(n_u * n_v);
    for &u in &xi {
        for &v in &eta {
            net.push(exact(base, d, u, v)?);
        }
    }
    let mut col = vec![[0.0_f64; 3]; n_u];
    for j in 0..n_v {
        for (i, c) in col.iter_mut().enumerate() {
            *c = net[i * n_v + j];
        }
        mu.solve(&mut col)?;
        for (i, c) in col.iter().enumerate() {
            net[i * n_v + j] = *c;
        }
    }
    for row in net.chunks_mut(n_v) {
        mv.solve(row)?;
    }
    NurbsSurface::new(NurbsSurfaceParts {
        degree_u: qu,
        degree_v: qv,
        knots_u: ku,
        knots_v: kv,
        n_u,
        n_v,
        ctrl: net,
        weights: None,
        periodic_u: false,
        periodic_v: false,
    })
    .map_err(|_| Fail::Fit)
}

/// `max |A - E|` over the sample grid.
fn measure(base: &dyn Surface, d: f64, approx: &NurbsSurface, au: &Axis, av: &Axis) -> Result<f64, Fail> {
    let (us, vs) = (au.samples(), av.samples());
    let mut max2 = 0.0_f64;
    for &u in &us {
        for &v in &vs {
            let a = approx.eval(u, v, 0).map_err(|_| Fail::Fit)?.p;
            let e = exact(base, d, u, v)?;
            let r = sub(a, e);
            let d2 = dot(r, r);
            if !d2.is_finite() {
                return Err(Fail::NonFinite);
            }
            if d2 > max2 {
                max2 = d2;
            }
        }
    }
    fid_num::f::sqrt(max2).map_err(|_| Fail::NonFinite)
}

/// The parameter box of the fit: the base's domain where finite, else the caller's box, else
/// `[0, 1]` (then `defaulted`). A caller box always wins.
fn fit_domain(base: &dyn Surface, hint: Option<&ParamBox>) -> Result<(ParamBox, bool), Fail> {
    if let Some(b) = hint {
        let ok = |r: (f64, f64)| r.0.is_finite() && r.1.is_finite() && r.0 < r.1;
        if !(ok(b.u) && ok(b.v)) {
            return Err(Fail::BadDomain);
        }
        return Ok((*b, false));
    }
    let (du, dv) = base.domain();
    let mut defaulted = false;
    let mut fix = |r: (f64, f64)| {
        if r.0.is_finite() && r.1.is_finite() {
            r
        } else {
            defaulted = true;
            (0.0, 1.0)
        }
    };
    let b = ParamBox { u: fix(du), v: fix(dv) };
    Ok((b, defaulted))
}

/// Approximates the offset of `base` by `distance_m` (REQ-IO.CAD.STEP.GEOM-003, -004).
pub(crate) fn offset(
    base: &SurfaceRef,
    distance_m: f64,
    opts: &GeomOptions,
    hint: Option<&ParamBox>,
) -> Result<(NurbsSurface, Approx), Fail> {
    let s = as_surface(base)?;
    let (domain, domain_defaulted) = fit_domain(s, hint)?;
    let (cu, cv) = curvature_dirs(base);
    let mut au = initial_axis(base, true, domain.u.0, domain.u.1, cu)?;
    let mut av = initial_axis(base, false, domain.v.0, domain.v.1, cv)?;
    let mut best: Option<(NurbsSurface, Approx)> = None;
    let mut refined = 0u32;
    loop {
        let samples = (au.sample_count() as u64).saturating_mul(av.sample_count() as u64);
        if samples > opts.max_approx_samples || au.spans() > MAX_FIT_SPANS || av.spans() > MAX_FIT_SPANS {
            break;
        }
        let nurbs = fit(s, distance_m, &au, &av)?;
        let dev = measure(s, distance_m, &nurbs, &au, &av)?;
        let within = dev <= opts.approx_tol_m;
        let approx = Approx {
            max_dev_m: dev,
            tolerance_m: opts.approx_tol_m,
            samples,
            grid: [au.sample_count(), av.sample_count()],
            spans: [au.spans(), av.spans()],
            refined,
            domain,
            domain_defaulted,
            status: if within { ApproxStatus::WithinTolerance } else { ApproxStatus::OutOfTolerance },
        };
        best = Some((nurbs, approx));
        if within || refined >= MAX_REFINEMENTS || !(cu || cv) {
            break;
        }
        if cu {
            au.refine();
        }
        if cv {
            av.refine();
        }
        refined += 1;
    }
    best.ok_or(Fail::ApproxBudget)
}
