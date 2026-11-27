//! Minimum enclosing 2D circle and sphere by iterative move-to-front Welzl
//! (REQ-KRN.ENV.XCHK-001, -002, -005).
//!
//! # Pinned rules
//!
//! 1. **Checks** (REQ-005), in order: fewer points than the primitive needs (`circle2`: 3,
//!    `sphere`: 4) is `TooFewPoints`; then [`PointsView::new`] (`TooLarge`, `NonFinite`,
//!    `Subnormal`); then the data are centred and scaled by [`Centred::new`] and their numerical
//!    rank is taken with `fid_num::la::rank_by_rule` under `DEFAULT_RANK_TOL` on the singular
//!    values of [`data_shape`]: rank below 2 is `Collinear`; for a sphere rank 2 is `Coplanar`.
//!    Every later step runs on the centred, scaled points.
//! 2. **Balls.** The ball of a boundary set `R` (in push order) is: empty for `R = {}` (nothing
//!    is inside); the point for one point; the midpoint ball for two; for three or four points
//!    `a = R[0]`, `u_j = R[j] - a`, the centre `a + sum_j beta_j u_j` from the Gram system
//!    `G beta = h`, `G_jl = u_j . u_l`, `h_j = (u_j . u_j) / 2`, solved by `fid_num::la::lstsq`
//!    with `rel_tol` [`GRAM_RANK_TOL`]. A rank-deficient Gram matrix (affinely dependent `R`,
//!    possible only through rounding) uses the midpoint ball of the first farthest pair of `R`
//!    instead; every other solver error is returned. The squared radius is the total-order
//!    maximum of `|p - c|^2` over `R`. Dot products are left-to-right sums of plain products.
//! 3. **Move-to-front Welzl** (REQ-001). The order is `0..n` shuffled by
//!    `fid_num::rng::shuffle` with the cursor at index 0 of stream [`SHUFFLE_STREAM`] of the
//!    caller's seed. A frame `(end, i)` scans `order[i]` for `i < end`; a point is inside a ball
//!    when `|p - c|^2 <= r2 * (1 + INSIDE_REL)`. A point outside is pushed on `R`, the ball is
//!    rebuilt from `R`, and a child frame `(i, 0)` (or `(0, 0)` once `R` holds `dim + 1`
//!    points) is pushed. When a frame ends, its point is popped from `R` and moved to the
//!    front of the order (`order[..=i].rotate_right(1)`), and the parent continues at `i + 1`.
//!    The stack never holds more than `dim + 2` frames; more than [`STEP_LIMIT`] scan steps is
//!    `Num(Domain{op: "xchk.welzl", detail: "step_limit"})`.
//! 4. **Canonical support** (REQ-002). The contacts are the points with
//!    `| |p - c| - r | <= CONTACT_REL * r` about the first run's ball, plus that ball's own
//!    boundary set. Step 3 runs again on the contacts in ascending index order shuffled with
//!    the fixed [`CANON_SEED`] on stream [`CANON_STREAM`], so its result depends on the contact
//!    set and not on the caller's seed. Its boundary set, sorted ascending, is the support (2
//!    to `dim + 1` points); the ball is rebuilt from it by rule 2.
//! 5. **Enclosure** (fail closed). A point with `|p - c|^2 > r2 * (1 + ENCLOSE_REL)` is
//!    `Num(Domain{op: "xchk.welzl", detail: "enclosure_violated"})`. A returned ball may
//!    therefore leave a point outside by up to `ENCLOSE_REL` (`2^-36`) relative on `r^2` (about
//!    `2^-37` relative on `r`, in the scaled coordinates); no point beyond that slack is ever
//!    outside a returned ball.
//! 6. **Result.** The centre is mapped back by [`Normalisation::inverse`] (a circle's `z` is
//!    `+0.0`); the radius is recomputed in metres from the support, the total-order maximum of
//!    `red::norm2(p - centre)` over the support points (REQ-002).
//!
//! [`Normalisation::inverse`]: crate::contract::Normalisation::inverse

use core::cmp::Ordering;

use fid_kci::AlgorithmId;
use fid_num::la::{lstsq, rank_by_rule, LaError, Mat};
use fid_num::rng::{shuffle, Address, Cursor};
use fid_num::{f, red, NumError};

use crate::contract::{data_shape, Centred, FitError, PointsView, Primitive, DEFAULT_RANK_TOL, MAX_POINTS, V3};

use super::{mcc_circle2d_id, mcc_sphere_id};

const OP: &str = "xchk.welzl";

/// The RNG stream of the caller-seeded shuffle (rule 3).
pub const SHUFFLE_STREAM: u64 = 0;
/// The fixed seed of the canonical support pass (rule 4).
pub const CANON_SEED: u64 = 0x5843_484B_5745_4C5A;
/// The RNG stream of the canonical support pass (rule 4).
pub const CANON_STREAM: u64 = 1;

/// Relative slack of the inside test of the scan, `2^-44` (rule 3).
pub const INSIDE_REL: f64 = 1.0 / (1u64 << 44) as f64;
/// Relative contact band of the canonical pass, `2^-30` (rule 4).
pub const CONTACT_REL: f64 = 1.0 / (1u64 << 30) as f64;
/// Relative slack of the final enclosure check, `2^-36` (rule 5).
pub const ENCLOSE_REL: f64 = 1.0 / (1u64 << 36) as f64;
/// `rel_tol` of the Gram solve (rule 2).
pub const GRAM_RANK_TOL: f64 = 1e-13;
/// Largest number of scan steps of one Welzl pass (rule 3).
pub const STEP_LIMIT: u64 = 1 << 36;

/// A minimum enclosing circle or sphere.
#[derive(Debug, Clone, PartialEq)]
pub struct Enclosing {
    /// Centre in metres; `z = +0.0` for a circle.
    pub centre: V3,
    /// Radius in metres, recomputed from the support (rule 6).
    pub radius: f64,
    /// The canonical support set: 2 to 4 point indices, ascending (rule 4).
    pub support: Vec<u32>,
    /// `fit.circle2d.mcc_welzl@1.0.0` or `fit.sphere.mcc_welzl@1.0.0`.
    pub algorithm: AlgorithmId,
}

#[derive(Debug, Clone, Copy)]
struct Ball {
    c: V3,
    /// Squared radius; negative for the empty ball.
    r2: f64,
}

const EMPTY: Ball = Ball { c: [0.0; 3], r2: -1.0 };

fn alloc<T>(n: usize) -> Result<Vec<T>, FitError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| FitError::Num(NumError::Domain { op: OP, detail: "allocation_failed" }))?;
    Ok(v)
}

fn domain(detail: &'static str) -> FitError {
    FitError::Num(NumError::Domain { op: OP, detail })
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn dist2(a: V3, b: V3) -> f64 {
    let d = sub(a, b);
    dot(d, d)
}

fn finite_ball(b: Ball) -> Result<Ball, FitError> {
    if b.c.iter().all(|x| x.is_finite()) && b.r2.is_finite() {
        Ok(b)
    } else {
        Err(FitError::Num(NumError::Infinite { op: OP, index: None }))
    }
}

/// The ball of a boundary set (rule 2).
fn ball_of(pts: &[V3], set: &[u32]) -> Result<Ball, FitError> {
    let get = |k: usize| -> Result<V3, FitError> {
        set.get(k).and_then(|&i| pts.get(i as usize)).copied().ok_or(domain("index"))
    };
    let c = match set.len() {
        0 => return Ok(EMPTY),
        1 => get(0)?,
        2 => {
            let (a, b) = (get(0)?, get(1)?);
            [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1]), 0.5 * (a[2] + b[2])]
        }
        k => {
            let a = get(0)?;
            let mut u: Vec<V3> = alloc(k - 1)?;
            for j in 1..k {
                u.push(sub(get(j)?, a));
            }
            let m = k - 1;
            let mut g = alloc(m * m)?;
            let mut h = alloc(m)?;
            for uj in &u {
                for ul in &u {
                    g.push(dot(*uj, *ul));
                }
                h.push(0.5 * dot(*uj, *uj));
            }
            let gm = Mat::new(m, m, g)?;
            match lstsq(&gm, &h, GRAM_RANK_TOL) {
                Ok(sol) => {
                    let mut c = a;
                    for (beta, uj) in sol.x.iter().zip(&u) {
                        for (ci, ui) in c.iter_mut().zip(uj) {
                            *ci += *beta * *ui;
                        }
                    }
                    c
                }
                Err(e) if matches!(LaError::of(&e), Some(LaError::RankDeficient { .. })) => {
                    farthest_pair_mid(pts, set)?
                }
                Err(e) => return Err(FitError::Num(e)),
            }
        }
    };
    let mut r2 = 0.0_f64;
    for &i in set {
        let p = pts.get(i as usize).copied().ok_or(domain("index"))?;
        r2 = f::total_max(r2, dist2(p, c))?;
    }
    finite_ball(Ball { c, r2 })
}

/// Midpoint of the first farthest pair of `set` (ascending pair order, total order).
fn farthest_pair_mid(pts: &[V3], set: &[u32]) -> Result<V3, FitError> {
    let mut best: Option<(f64, V3, V3)> = None;
    for (x, &i) in set.iter().enumerate() {
        for &j in set.iter().skip(x + 1) {
            let a = pts.get(i as usize).copied().ok_or(domain("index"))?;
            let b = pts.get(j as usize).copied().ok_or(domain("index"))?;
            let d = dist2(a, b);
            let better = match best {
                None => true,
                Some((bd, _, _)) => f::total_cmp(d, bd)? == Ordering::Greater,
            };
            if better {
                best = Some((d, a, b));
            }
        }
    }
    let (_, a, b) = best.ok_or(domain("index"))?;
    Ok([0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1]), 0.5 * (a[2] + b[2])])
}

fn inside(b: &Ball, p: V3) -> bool {
    b.r2 >= 0.0 && dist2(p, b.c) <= b.r2 * (1.0 + INSIDE_REL)
}

/// One move-to-front Welzl pass over `order` (rule 3). Returns the final ball and its boundary
/// set in push order.
fn mtf(pts: &[V3], order: &mut [u32], dim: usize) -> Result<(Ball, Vec<u32>), FitError> {
    let full = dim + 1;
    let mut set: Vec<u32> = alloc(full)?;
    let mut ball = EMPTY;
    let mut ball_set: Vec<u32> = alloc(full)?;
    let mut frames: Vec<(usize, usize)> = alloc(full + 1)?;
    frames.push((order.len(), 0));
    let mut steps: u64 = 0;
    while let Some(&(end, i)) = frames.last() {
        steps += 1;
        if steps > STEP_LIMIT {
            return Err(domain("step_limit"));
        }
        let top = frames.len() - 1;
        if i >= end {
            frames.pop();
            let Some(parent) = frames.last_mut() else { break };
            let pi = parent.1;
            set.pop();
            order.get_mut(..=pi).ok_or(domain("index"))?.rotate_right(1);
            parent.1 = pi + 1;
            continue;
        }
        let p = *order.get(i).ok_or(domain("index"))?;
        let q = pts.get(p as usize).copied().ok_or(domain("index"))?;
        if inside(&ball, q) {
            if let Some(fr) = frames.get_mut(top) {
                fr.1 = i + 1;
            }
            continue;
        }
        if set.len() >= full || frames.len() > full {
            // Unreachable by construction (a full set gets an empty child frame).
            return Err(domain("frame_depth"));
        }
        set.push(p);
        ball = ball_of(pts, &set)?;
        ball_set.clear();
        ball_set.extend_from_slice(&set);
        let child_end = if set.len() == full { 0 } else { i };
        frames.push((child_end, 0));
    }
    Ok((ball, ball_set))
}

fn welzl(points: &[V3], primitive: Primitive, seed: u64) -> Result<Enclosing, FitError> {
    let (dim, algorithm) = match primitive {
        Primitive::Sphere => (3, mcc_sphere_id()),
        _ => (2, mcc_circle2d_id()),
    };
    let n = points.len();
    let need = primitive.requirement().min_points;
    if n < need {
        return Err(FitError::TooFewPoints { n, need });
    }
    if n > MAX_POINTS {
        return Err(FitError::TooLarge { n, max: MAX_POINTS });
    }
    let mut xyz: Vec<f64> = alloc(3 * n)?;
    for p in points {
        xyz.extend_from_slice(p);
    }
    let view = PointsView::new(&xyz, None)?;
    let centred = Centred::new(&view)?;
    let s = data_shape(&centred)?.singular_values;
    let rank = rank_by_rule(&s, DEFAULT_RANK_TOL)?;
    if rank < dim {
        let cond = if s[0] == 0.0 || s[dim - 1] == 0.0 { f64::INFINITY } else { s[0] / s[dim - 1] };
        return Err(if rank <= 1 {
            FitError::Collinear { n, rank, cond }
        } else {
            FitError::Coplanar { n, rank, cond }
        });
    }
    let mut q: Vec<V3> = alloc(n)?;
    for c in centred.xyz().chunks_exact(3) {
        q.push([c[0], c[1], c[2]]);
    }

    // Rule 3: the caller-seeded pass.
    let mut order: Vec<u32> = alloc(n)?;
    order.extend(0..n as u32);
    let mut cur = Cursor::new(Address { seed, stream: SHUFFLE_STREAM }, 0);
    shuffle(&mut order, &mut cur)?;
    let (first, first_set) = mtf(&q, &mut order, dim)?;

    // Rule 4: canonical support from the contact set.
    let r = f::sqrt(first.r2)?;
    let band = CONTACT_REL * r;
    let mut contacts: Vec<u32> = alloc(n)?;
    for (i, p) in q.iter().enumerate() {
        let d = f::sqrt(dist2(*p, first.c))?;
        if (d - r).abs() <= band || first_set.contains(&(i as u32)) {
            contacts.push(i as u32);
        }
    }
    let mut cur = Cursor::new(Address { seed: CANON_SEED, stream: CANON_STREAM }, 0);
    shuffle(&mut contacts, &mut cur)?;
    let (_, mut support) = mtf(&q, &mut contacts, dim)?;
    support.sort_unstable();
    let ball = ball_of(&q, &support)?;

    // Rule 5: every point inside.
    let limit = ball.r2 * (1.0 + ENCLOSE_REL);
    for p in &q {
        if dist2(*p, ball.c) > limit {
            return Err(domain("enclosure_violated"));
        }
    }

    // Rule 6: back to metres, radius recomputed from the support.
    let mut centre = centred.map().inverse(ball.c)?;
    if dim == 2 {
        centre[2] = 0.0;
    }
    let mut radius = 0.0_f64;
    for &i in &support {
        let p = points.get(i as usize).copied().ok_or(domain("index"))?;
        radius = f::total_max(radius, red::norm2(&sub(p, centre))?)?;
    }
    Ok(Enclosing { centre, radius, support, algorithm })
}

/// The minimum enclosing circle of planar points `[x, y]` in metres (rules 1 to 6), visiting the
/// points in the order of a shuffle drawn with `seed`. The result does not depend on `seed`.
pub fn mcc_circle2d_welzl(points: &[[f64; 2]], seed: u64) -> Result<Enclosing, FitError> {
    let mut p3: Vec<V3> = alloc(points.len())?;
    for p in points {
        p3.push([p[0], p[1], 0.0]);
    }
    welzl(&p3, Primitive::Circle2, seed)
}

/// The minimum enclosing sphere of points `[x, y, z]` in metres (rules 1 to 6), visiting the
/// points in the order of a shuffle drawn with `seed`. The result does not depend on `seed`.
pub fn mcc_sphere_welzl(points: &[V3], seed: u64) -> Result<Enclosing, FitError> {
    welzl(points, Primitive::Sphere, seed)
}
