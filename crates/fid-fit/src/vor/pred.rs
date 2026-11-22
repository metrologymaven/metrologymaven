//! Exact-sign adaptive-precision planar geometric predicates (REQ-KRN.MZ.VOR-004).
//!
//! Follows Shewchuk (1997), "Adaptive Precision Floating-Point Arithmetic and Fast Robust
//! Geometric Predicates", built purely from IEEE-754 binary64 `+`, `-`, `*` without FMA
//! (`mul_add` is never used; two-product uses Dekker/Veltkamp splitting with `2^27 + 1`),
//! zero `unsafe`, zero heap allocation, and KNP-1 compliance.

/// Veltkamp/Dekker split constant: `2^27 + 1` for IEEE-754 binary64 (53-bit significand).
const SPLITTER: f64 = 134_217_729.0;

/// Shewchuk `resulterrbound` = `(3 + 8 * eps) * eps` where `eps = 2^-53`.
const ORIENT2D_ERRBOUND_A: f64 = 3.330_669_073_875_471_6e-16;
/// Shewchuk `iccerrboundA` = `(10 + 96 * eps) * eps`.
const INCIRCLE_ERRBOUND_A: f64 = 1.110_223_024_625_156_5e-15;

#[inline]
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let x = a + b;
    let b_virt = x - a;
    let a_virt = x - b_virt;
    let b_round = b - b_virt;
    let a_round = a - a_virt;
    (x, a_round + b_round)
}

#[inline]
fn two_diff(a: f64, b: f64) -> (f64, f64) {
    let x = a - b;
    let b_virt = a - x;
    let a_virt = x + b_virt;
    let b_round = b_virt - b;
    let a_round = a - a_virt;
    (x, a_round + b_round)
}

#[inline]
fn split(a: f64) -> (f64, f64) {
    let c = SPLITTER * a;
    let a_big = c - a;
    let a_hi = c - a_big;
    let a_lo = a - a_hi;
    (a_hi, a_lo)
}

/// Exact error-free product `a * b = x + y` using Dekker splitting (no FMA).
#[inline]
fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let x = a * b;
    let (a_hi, a_lo) = split(a);
    let (b_hi, b_lo) = split(b);
    let err1 = x - (a_hi * b_hi);
    let err2 = err1 - (a_lo * b_hi);
    let err3 = err2 - (a_hi * b_lo);
    let y = (a_lo * b_lo) - err3;
    (x, y)
}

#[inline]
fn two_one_diff(a1: f64, a0: f64, b: f64) -> (f64, f64, f64) {
    let (i, x0) = two_diff(a0, b);
    let (x2, x1) = two_sum(a1, i);
    (x2, x1, x0)
}

#[inline]
fn two_two_diff(a1: f64, a0: f64, b1: f64, b0: f64) -> [f64; 4] {
    let (j, r0, x0) = two_one_diff(a1, a0, b0);
    let (x3, x2, x1) = two_one_diff(j, r0, b1);
    [x0, x1, x2, x3]
}

/// Fixed-capacity non-overlapping floating-point expansion stored on the stack.
/// Components are ordered from smallest to largest magnitude (Shewchuk convention).
struct Expansion<const CAP: usize> {
    buf: [f64; CAP],
    len: usize,
}

impl<const CAP: usize> Expansion<CAP> {
    fn from_slice(src: &[f64]) -> Self {
        let mut buf = [0.0; CAP];
        let mut len = 0;
        for &v in src {
            if v != 0.0 && len < CAP {
                buf[len] = v;
                len += 1;
            }
        }
        Expansion { buf, len }
    }

    fn as_slice(&self) -> &[f64] {
        &self.buf[..self.len]
    }

    /// Exact sign of the expansion (`-1`, `0`, or `+1`).
    fn sign(&self) -> i8 {
        // Compress first to guarantee the most-significant non-zero component carries the true sign.
        let compressed = compress::<CAP>(self.as_slice());
        for &v in compressed.as_slice().iter().rev() {
            if v > 0.0 {
                return 1;
            }
            if v < 0.0 {
                return -1;
            }
        }
        0
    }
}

/// Shewchuk `fast_expansion_sum_zeroelim`: exact sum of two non-overlapping expansions.
fn expansion_sum_zeroelim<const CAP: usize>(e: &[f64], f: &[f64]) -> Expansion<CAP> {
    let mut out = [0.0; CAP];
    let mut h_idx = 0usize;

    let mut ei = 0usize;
    let mut fi = 0usize;

    let mut q = match (e.get(ei), f.get(fi)) {
        (Some(&ev), Some(&fv)) => {
            if (fv > ev) == (fv > -ev) {
                ei += 1;
                ev
            } else {
                fi += 1;
                fv
            }
        }
        (Some(&ev), None) => {
            ei += 1;
            ev
        }
        (None, Some(&fv)) => {
            fi += 1;
            fv
        }
        (None, None) => return Expansion { buf: out, len: 0 },
    };

    while ei < e.len() || fi < f.len() {
        let next = match (e.get(ei), f.get(fi)) {
            (Some(&ev), Some(&fv)) => {
                if (fv > ev) == (fv > -ev) {
                    ei += 1;
                    ev
                } else {
                    fi += 1;
                    fv
                }
            }
            (Some(&ev), None) => {
                ei += 1;
                ev
            }
            (None, Some(&fv)) => {
                fi += 1;
                fv
            }
            (None, None) => break,
        };
        let (q_new, hh) = two_sum(q, next);
        q = q_new;
        if hh != 0.0 && h_idx < CAP {
            out[h_idx] = hh;
            h_idx += 1;
        }
    }

    if q != 0.0 && h_idx < CAP {
        out[h_idx] = q;
        h_idx += 1;
    }

    Expansion { buf: out, len: h_idx }
}

/// Shewchuk `scale_expansion_zeroelim`: exact product of an expansion with a scalar `b`.
fn scale_expansion_zeroelim<const CAP: usize>(e: &[f64], b: f64) -> Expansion<CAP> {
    let mut out = [0.0; CAP];
    if e.is_empty() || b == 0.0 {
        return Expansion { buf: out, len: 0 };
    }
    let mut h_idx = 0usize;
    let (mut q, hh0) = two_prod(e[0], b);
    if hh0 != 0.0 && h_idx < CAP {
        out[h_idx] = hh0;
        h_idx += 1;
    }
    for &en in e.iter().skip(1) {
        let (p1, p0) = two_prod(en, b);
        let (sum, hh1) = two_sum(q, p0);
        if hh1 != 0.0 && h_idx < CAP {
            out[h_idx] = hh1;
            h_idx += 1;
        }
        let (q_new, hh2) = two_sum(p1, sum);
        q = q_new;
        if hh2 != 0.0 && h_idx < CAP {
            out[h_idx] = hh2;
            h_idx += 1;
        }
    }
    if q != 0.0 && h_idx < CAP {
        out[h_idx] = q;
        h_idx += 1;
    }
    Expansion { buf: out, len: h_idx }
}

/// Shewchuk `compress`: removes redundant overlaps while preserving exact sum.
fn compress<const CAP: usize>(e: &[f64]) -> Expansion<CAP> {
    let mut out = [0.0; CAP];
    if e.is_empty() {
        return Expansion { buf: out, len: 0 };
    }
    let mut tmp = [0.0; CAP];
    let mut bottom = e.len() - 1;
    let mut q = e[bottom];
    let mut top = bottom;
    for idx in (0..bottom).rev() {
        let (q_new, small) = two_sum(q, e[idx]);
        if small != 0.0 {
            tmp[top] = q_new;
            top -= 1;
            q = small;
        } else {
            q = q_new;
        }
    }
    tmp[top] = q;

    let mut h_idx = 0usize;
    let mut q2 = tmp[top];
    for &v in tmp.iter().take(bottom + 1).skip(top + 1) {
        let (q_new, small) = two_sum(v, q2);
        if small != 0.0 && h_idx < CAP {
            out[h_idx] = small;
            h_idx += 1;
        }
        q2 = q_new;
    }
    if q2 != 0.0 && h_idx < CAP {
        out[h_idx] = q2;
        h_idx += 1;
    }
    bottom = h_idx;
    Expansion { buf: out, len: bottom }
}

/// Negates every term of an expansion in place.
fn negate_slice<const CAP: usize>(src: &[f64]) -> Expansion<CAP> {
    let mut buf = [0.0; CAP];
    let mut len = 0;
    for &v in src {
        if v != 0.0 && len < CAP {
            buf[len] = -v;
            len += 1;
        }
    }
    Expansion { buf, len }
}

/// Exact expansion of `det([[pa.x, pa.y], [pb.x, pb.y]]) = pa.x * pb.y - pa.y * pb.x`.
fn cross_exact(a: [f64; 2], b: [f64; 2]) -> [f64; 4] {
    let (p1, p0) = two_prod(a[0], b[1]);
    let (q1, q0) = two_prod(a[1], b[0]);
    two_two_diff(p1, p0, q1, q0)
}

/// Exact-sign 2D orientation predicate (`orient2d`).
///
/// Returns:
/// - `+1` if `(a, b, c)` are strictly counter-clockwise (`det > 0`),
/// - `-1` if strictly clockwise (`det < 0`),
/// - `0` if collinear (`det == 0`).
pub fn orient2d(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> i8 {
    let det_left = (a[0] - c[0]) * (b[1] - c[1]);
    let det_right = (a[1] - c[1]) * (b[0] - c[0]);
    let det = det_left - det_right;
    let det_sum = det_left.abs() + det_right.abs();
    let err_bound = ORIENT2D_ERRBOUND_A * det_sum;
    if det > err_bound {
        return 1;
    }
    if det < -err_bound {
        return -1;
    }
    orient2d_exact(a, b, c)
}

/// Exact fallback for `orient2d` using expansion arithmetic over `a×b + b×c + c×a`.
pub fn orient2d_exact(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> i8 {
    let ab = Expansion::<16>::from_slice(&cross_exact(a, b));
    let bc = Expansion::<16>::from_slice(&cross_exact(b, c));
    let ca = Expansion::<16>::from_slice(&cross_exact(c, a));
    let ab_bc = expansion_sum_zeroelim::<16>(ab.as_slice(), bc.as_slice());
    let total = expansion_sum_zeroelim::<16>(ab_bc.as_slice(), ca.as_slice());
    total.sign()
}

/// Exact-sign 2D in-circle predicate (`incircle`).
///
/// Assuming `(a, b, c)` is counter-clockwise:
/// - `+1` if `d` lies strictly inside the circumcircle of `(a, b, c)`,
/// - `-1` if `d` lies strictly outside,
/// - `0` if `a, b, c, d` are cocircular.
///
/// (If `(a, b, c)` is clockwise, the sign is reversed, matching the determinant sign.)
pub fn incircle(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> i8 {
    let adx = a[0] - d[0];
    let bdx = b[0] - d[0];
    let cdx = c[0] - d[0];
    let ady = a[1] - d[1];
    let bdy = b[1] - d[1];
    let cdy = c[1] - d[1];

    let bdxcdy = bdx * cdy;
    let cdxbdy = cdx * bdy;
    let alift = adx * adx + ady * ady;

    let cdxady = cdx * ady;
    let adxcdy = adx * cdy;
    let blift = bdx * bdx + bdy * bdy;

    let adxbdy = adx * bdy;
    let bdxady = bdx * ady;
    let clift = cdx * cdx + cdy * cdy;

    let det = alift * (bdxcdy - cdxbdy) + blift * (cdxady - adxcdy) + clift * (adxbdy - bdxady);
    let permanent = (bdxcdy.abs() + cdxbdy.abs()) * alift
        + (cdxady.abs() + adxcdy.abs()) * blift
        + (adxbdy.abs() + bdxady.abs()) * clift;
    let err_bound = INCIRCLE_ERRBOUND_A * permanent;
    if det > err_bound {
        return 1;
    }
    if det < -err_bound {
        return -1;
    }
    incircle_exact(a, b, c, d)
}

/// Exact fallback for `incircle` (Shewchuk `incircleexact` formulation).
///
/// Evaluates the exact 4x4 determinant:
/// `| ax  ay  ax^2+ay^2  1 |`
/// `| bx  by  bx^2+by^2  1 |`
/// `| cx  cy  cx^2+cy^2  1 |`
/// `| dx  dy  dx^2+dy^2  1 |`
/// expanded by the third column (`l_i = x_i^2 + y_i^2`):
/// `l_a * (bc + cd + db) - l_b * (cd + da + ac) + l_c * (da + ab + bd) - l_d * (ab + bc + ca)`
/// where each 2x2 minor is exact (len 4) and multiplied by `x^2` and `y^2` via two `scale_expansion`s.
pub fn incircle_exact(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> i8 {
    let ab = cross_exact(a, b);
    let bc = cross_exact(b, c);
    let cd = cross_exact(c, d);
    let da = cross_exact(d, a);
    let ac = cross_exact(a, c);
    let bd = cross_exact(b, d);

    let ca = negate_slice::<4>(&ac);
    let db = negate_slice::<4>(&bd);

    // Minor for a: bc + cd + db (sign +1)
    let m_a = sum3_16(&bc, &cd, db.as_slice());
    // Minor for b: cd + da + ac (sign -1)
    let m_b = sum3_16(&cd, &da, &ac);
    // Minor for c: da + ab + bd (sign +1)
    let m_c = sum3_16(&da, &ab, &bd);
    // Minor for d: ab + bc + ca (sign -1)
    let m_d = sum3_16(&ab, &bc, ca.as_slice());

    let term_a = lift_term(m_a.as_slice(), a);
    let term_b = negate_slice::<128>(lift_term(m_b.as_slice(), b).as_slice());
    let term_c = lift_term(m_c.as_slice(), c);
    let term_d = negate_slice::<128>(lift_term(m_d.as_slice(), d).as_slice());

    let ab_tot = expansion_sum_zeroelim::<256>(term_a.as_slice(), term_b.as_slice());
    let cd_tot = expansion_sum_zeroelim::<256>(term_c.as_slice(), term_d.as_slice());
    let total = expansion_sum_zeroelim::<512>(ab_tot.as_slice(), cd_tot.as_slice());
    total.sign()
}

fn sum3_16(x: &[f64], y: &[f64], z: &[f64]) -> Expansion<16> {
    let xy = expansion_sum_zeroelim::<16>(x, y);
    expansion_sum_zeroelim::<16>(xy.as_slice(), z)
}

/// Computes `minor * (p.x * p.x + p.y * p.y)` exactly.
fn lift_term(minor: &[f64], p: [f64; 2]) -> Expansion<128> {
    let mx = scale_expansion_zeroelim::<64>(minor, p[0]);
    let mxx = scale_expansion_zeroelim::<64>(mx.as_slice(), p[0]);
    let my = scale_expansion_zeroelim::<64>(minor, p[1]);
    let myy = scale_expansion_zeroelim::<64>(my.as_slice(), p[1]);
    expansion_sum_zeroelim::<128>(mxx.as_slice(), myy.as_slice())
}
