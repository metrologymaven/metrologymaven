//! Centring and isotropic scaling with an exact inverse map (REQ-KRN.LSQ.TRAIT-003).
//!
//! # Pinned rule
//!
//! For a view of `n >= 1` points:
//!
//! 1. **Centroid.** `c_k = red::mean(column k)` for `k = 0, 1, 2`, each column gathered in
//!    point order.
//! 2. **Centred data.** `d[3i + k] = x[3i + k] - c_k` (one subtraction).
//! 3. **RMS distance.** `rms = red::norm2(d) / f::sqrt(n)` over the whole interleaved buffer
//!    `d` (so `rms^2` is the mean squared distance to the centroid).
//! 4. **Scale.** `s = 2^e` with `2^e <= rms < 2^(e + 1)` (the binary exponent of `rms`,
//!    subnormal values included); `s = 1` when `rms == 0`. The scale is the RMS distance
//!    rounded down to a power of two, so the scaled data has RMS distance in `[1, 2)` and
//!    scaling and unscaling are exact.
//! 5. **Forward map.** `q = (x - c) / s` per coordinate; lengths `l / s`.
//! 6. **Inverse map.** `x = q * s + c` per coordinate (a product and a sum, no FMA); lengths
//!    `l * s`; directions, angles, spline knots and degrees unchanged.
//!
//! Dividing or multiplying by a power of two is exact unless the result leaves the normal
//! range, so the only roundings are the centring subtraction and the final addition: a round
//! trip `x -> q -> x'` is within one rounding of each, and exact whenever `x - c` is exact
//! (for example when `x` and `c` are within a factor of 2, Sterbenz).
//!
//! Errors: an empty view is [`FitError::TooFewPoints`]; an overflowing mean, difference or
//! norm is [`FitError::Num`]; allocation failure is [`FitError::Num`] with detail
//! `allocation_failed`.

use fid_num::{f, red, NumError};

use super::error::{alloc, FitError};
use super::points::PointsView;
use super::primitive::{FittedFeature, V2, V3};

const OP: &str = "fit.normalise";

/// The centring-and-scaling map of one point set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Normalisation {
    centroid: V3,
    exp: i32,
}

/// A centred and scaled copy of a point set with its map.
#[derive(Debug, Clone, PartialEq)]
pub struct Centred {
    map: Normalisation,
    xyz: Vec<f64>,
}

/// `2^k` for `k` in `[-1074, 1023]` (subnormal below `-1022`), exact.
fn pow2(k: i32) -> f64 {
    if k >= -1022 {
        f64::from_bits(((k + 1023) as u64) << 52)
    } else {
        f64::from_bits(1u64 << (k + 1074))
    }
}

/// Exponent `e` with `2^e <= m < 2^(e + 1)`, for finite `m > 0` (subnormals included).
fn exponent(m: f64) -> i32 {
    let b = m.to_bits();
    let biased = ((b >> 52) & 0x7FF) as i32;
    if biased == 0 {
        (63 - b.leading_zeros() as i32) - 1074
    } else {
        biased - 1023
    }
}

fn finite(x: f64) -> Result<f64, FitError> {
    if x.is_finite() {
        Ok(x)
    } else {
        Err(FitError::Num(NumError::Infinite { op: OP, index: None }))
    }
}

impl Normalisation {
    /// The identity map (centroid 0, scale 1).
    pub const IDENTITY: Normalisation = Normalisation { centroid: [0.0; 3], exp: 0 };

    /// The centroid `c` (metres).
    pub fn centroid(&self) -> V3 {
        self.centroid
    }

    /// The binary exponent `e` of the scale `s = 2^e`.
    pub fn scale_exp(&self) -> i32 {
        self.exp
    }

    /// The scale `s = 2^e` (metres per scaled unit).
    pub fn scale(&self) -> f64 {
        pow2(self.exp)
    }

    /// `(p - c) / s` per coordinate.
    pub fn forward(&self, p: V3) -> Result<V3, FitError> {
        let s = self.scale();
        let mut q = [0.0; 3];
        for ((qk, pk), ck) in q.iter_mut().zip(p).zip(self.centroid) {
            *qk = finite(finite(pk - ck)? / s)?;
        }
        Ok(q)
    }

    /// `q * s + c` per coordinate.
    pub fn inverse(&self, q: V3) -> Result<V3, FitError> {
        let s = self.scale();
        let mut p = [0.0; 3];
        for ((pk, qk), ck) in p.iter_mut().zip(q).zip(self.centroid) {
            *pk = finite(finite(qk * s)? + ck)?;
        }
        Ok(p)
    }

    /// The 2D inverse map, with the centroid's x and y.
    pub fn inverse2(&self, q: V2) -> Result<V2, FitError> {
        let s = self.scale();
        Ok([finite(finite(q[0] * s)? + self.centroid[0])?, finite(finite(q[1] * s)? + self.centroid[1])?])
    }

    /// A length in metres to scaled units: `l / s`.
    pub fn forward_length(&self, l: f64) -> Result<f64, FitError> {
        finite(l / self.scale())
    }

    /// A scaled length back to metres: `l * s`.
    pub fn inverse_length(&self, l: f64) -> Result<f64, FitError> {
        finite(l * self.scale())
    }

    /// Maps a feature fitted in scaled coordinates back to metres: positions by
    /// [`Normalisation::inverse`] ([`Normalisation::inverse2`] for 2D features), radii,
    /// lengths, widths, semi-axes and focal lengths by [`Normalisation::inverse_length`];
    /// directions, angles, knots and degrees unchanged.
    pub fn inverse_feature(&self, feat: &FittedFeature) -> Result<FittedFeature, FitError> {
        use FittedFeature as F;
        let p3 = |v: &V3| self.inverse(*v);
        let l = |x: &f64| self.inverse_length(*x);
        Ok(match feat {
            F::Point { c } => F::Point { c: p3(c)? },
            F::Line2 { p, d } => F::Line2 { p: self.inverse2(*p)?, d: *d },
            F::Line3 { p, d } => F::Line3 { p: p3(p)?, d: *d },
            F::Plane { p, n } => F::Plane { p: p3(p)?, n: *n },
            F::Circle2 { c, r } => F::Circle2 { c: self.inverse2(*c)?, r: l(r)? },
            F::Circle3 { c, n, r } => F::Circle3 { c: p3(c)?, n: *n, r: l(r)? },
            F::Sphere { c, r } => F::Sphere { c: p3(c)?, r: l(r)? },
            F::Cylinder { p, d, r } => F::Cylinder { p: p3(p)?, d: *d, r: l(r)? },
            F::Cone { apex, d, half_angle } => F::Cone { apex: p3(apex)?, d: *d, half_angle: *half_angle },
            F::Torus { c, n, major_r, minor_r } => {
                F::Torus { c: p3(c)?, n: *n, major_r: l(major_r)?, minor_r: l(minor_r)? }
            }
            F::Slot { c, n, d, length, width } => {
                F::Slot { c: p3(c)?, n: *n, d: *d, length: l(length)?, width: l(width)? }
            }
            F::Rectangle { c, n, d, length, width } => {
                F::Rectangle { c: p3(c)?, n: *n, d: *d, length: l(length)?, width: l(width)? }
            }
            F::Ellipse { c, n, d, a, b } => F::Ellipse { c: p3(c)?, n: *n, d: *d, a: l(a)?, b: l(b)? },
            F::Paraboloid { apex, d, focal } => F::Paraboloid { apex: p3(apex)?, d: *d, focal: l(focal)? },
            F::Polyline { vertices } => F::Polyline { vertices: self.map_points(vertices)? },
            F::Spline { degree, knots, control } => {
                let mut k = alloc(OP, knots.len())?;
                k.extend_from_slice(knots);
                F::Spline { degree: *degree, knots: k, control: self.map_points(control)? }
            }
        })
    }

    fn map_points(&self, src: &[V3]) -> Result<Vec<V3>, FitError> {
        let mut out = alloc(OP, src.len())?;
        for v in src {
            out.push(self.inverse(*v)?);
        }
        Ok(out)
    }
}

impl Centred {
    /// Centres and scales `pts` by the pinned rule of the module documentation.
    pub fn new(pts: &PointsView<'_>) -> Result<Centred, FitError> {
        let n = pts.len();
        if n == 0 {
            return Err(FitError::TooFewPoints { n: 0, need: 1 });
        }
        let xyz = pts.xyz();
        let mut col = alloc(OP, n)?;
        let mut centroid = [0.0; 3];
        for (k, c) in centroid.iter_mut().enumerate() {
            col.clear();
            col.extend(xyz.iter().skip(k).step_by(3).copied());
            *c = red::mean(&col)?;
        }
        drop(col);
        let mut d = alloc(OP, xyz.len())?;
        for (i, &x) in xyz.iter().enumerate() {
            d.push(finite(x - centroid[i % 3])?);
        }
        let rms = finite(red::norm2(&d)? / f::sqrt(n as f64)?)?;
        let exp = if rms == 0.0 { 0 } else { exponent(rms) };
        let s = pow2(exp);
        for x in d.iter_mut() {
            *x /= s;
        }
        Ok(Centred { map: Normalisation { centroid, exp }, xyz: d })
    }

    /// The map.
    pub fn map(&self) -> &Normalisation {
        &self.map
    }

    /// The centred, scaled coordinates (same layout as [`PointsView::xyz`]).
    pub fn xyz(&self) -> &[f64] {
        &self.xyz
    }

    /// Number of points.
    pub fn len(&self) -> usize {
        self.xyz.len() / 3
    }

    /// Whether there is no point (never true for a value built by [`Centred::new`]).
    pub fn is_empty(&self) -> bool {
        self.xyz.is_empty()
    }
}
