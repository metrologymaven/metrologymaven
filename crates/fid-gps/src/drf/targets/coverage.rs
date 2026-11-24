//! Datum target coverage check (spec node `GDT.DRF.COMMON.COVER`).
//!
//! Before a datum is simulated from its datum targets, every target must be touched by
//! measured data. [`check_coverage`] reports, for each target in definition order, its
//! coverage distance and refuses with `target_not_covered{target,distance}` when the first
//! target's distance exceeds the threshold the resolved row supplies. There is no default
//! threshold: none supplied is `threshold_missing`.
//!
//! # Coverage distance (REQ-GDT.DRF.COMMON.COVER-001)
//!
//! The distance from a sample to the nearest measured point is the square root (correctly
//! rounded, `fid_num::f::sqrt`) of the k-d tree's pinned squared distance
//! `(dx*dx + dy*dy) + dz*dz` (`GEO.INDEX.KDTREE`). A target's coverage distance is the
//! largest of these over its samples (total order); it is taken once over the squared
//! distances, which gives the same value because the square root is monotone.
//!
//! # Samples (pinned)
//!
//! All coordinates in metres; `s` is the caller's spacing; `t_i = i / n` in binary64; no
//! fused multiply-add; sums left to right.
//!
//! - [`TargetGeometry::Point`]: the point itself (one sample).
//! - [`TargetGeometry::Line`] from `a` to `b`: `n = max(1, ceil(|b - a| / s))` and, for
//!   `i = 0..=n`, `p = (1 - t_i) * a + t_i * b` per coordinate, so both end points are samples.
//! - [`TargetGeometry::Rect`] (a parallelogram from `corner` along `edge_u` and `edge_v`):
//!   `nu = max(1, ceil(|edge_u| / s))`, `nv` likewise, and for `i = 0..=nu` (outer),
//!   `j = 0..=nv` (inner), `p = (corner + t_i * edge_u) + t_j * edge_v` per coordinate.
//! - [`TargetGeometry::Disc`] (`centre`, `normal`, `radius`): an in-plane basis
//!   `n' = normal / |normal|`, `e` = the world axis with the smallest `|n'_k|` (lowest index on
//!   ties), `u = (e x n') / |e x n'|`, `v = n' x u`. Grid: `k = floor(radius / s)`, and for
//!   `j = -k..=k` (outer), `i = -k..=k` (inner), `x = i * s`, `y = j * s`, a sample
//!   `p = (centre + x * u) + y * v` when `x*x + y*y <= radius*radius`. Rim:
//!   `m = max(4, ceil((2*pi * radius) / s))` samples at `theta_i = (2*pi * i) / m`,
//!   `x = radius * cos(theta_i)`, `y = radius * sin(theta_i)` (`fid_num::f`).
//!
//! A target needing more than [`MAX_SAMPLES`] samples is refused (`too_many_samples`); for a
//! disc the bound is `(2k + 1)^2 + m`, the whole grid square plus the rim.
//!
//! # Checks, in this order (first failure wins)
//!
//! 1. `threshold_missing`: no threshold (REQ-003).
//! 2. `nan_input`: a NaN or infinity in the threshold, the spacing, a measured point or a
//!    target (in that order).
//! 3. `bad_threshold`: a negative threshold; `bad_spacing`: a spacing that is not positive.
//! 4. `empty_points`; `no_targets`.
//! 5. Per target in definition order: `degenerate_target` (a disc with a zero normal or a
//!    negative radius) and `too_many_samples`.
//! 6. The distances of every target, then `target_not_covered` for the first target in
//!    definition order whose distance exceeds the threshold (a distance equal to the
//!    threshold passes) (REQ-002).
//!
//! Pure, no `HashMap`/`HashSet`, nothing panics.

use core::fmt;
use std::f64::consts::TAU;

use fid_geo_index::{PointIndex, PointIndexError};
use fid_geo_types::PointBuffer;

/// The most samples one target may need.
pub const MAX_SAMPLES: u64 = 1 << 20;

type V3 = [f64; 3];

/// The geometry of one datum target, in metres in the frame of the measured points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TargetGeometry {
    /// A point target.
    Point {
        /// The target point.
        at: V3,
    },
    /// A line target: the segment from `start` to `end`.
    Line {
        /// One end.
        start: V3,
        /// The other end.
        end: V3,
    },
    /// A parallelogram area target (a rectangle when the edges are perpendicular).
    Rect {
        /// One corner.
        corner: V3,
        /// The first edge from `corner`.
        edge_u: V3,
        /// The second edge from `corner`.
        edge_v: V3,
    },
    /// A circular area target.
    Disc {
        /// The centre.
        centre: V3,
        /// The normal of the disc's plane (any non-zero length).
        normal: V3,
        /// The radius (zero is the centre alone).
        radius: f64,
    },
}

/// One datum target: its ID (`A1`) and geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    /// The target ID.
    pub label: String,
    /// The target geometry.
    pub geometry: TargetGeometry,
}

/// The coverage of one target.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetCoverage {
    /// The target ID.
    pub label: String,
    /// The coverage distance in metres.
    pub distance: f64,
    /// How many samples it was taken over.
    pub samples: u64,
}

/// The coverage of every target, in definition order.
#[derive(Debug, Clone, PartialEq)]
pub struct CoverageReport {
    /// One entry per target, in definition order.
    pub per_target: Vec<TargetCoverage>,
}

/// Every refusal of [`check_coverage`].
#[derive(Debug, Clone, PartialEq)]
pub enum CoverError {
    /// The chain supplied no threshold.
    ThresholdMissing,
    /// A NaN or infinity in the input (or an intermediate that overflowed).
    NanInput,
    /// The threshold is negative.
    BadThreshold,
    /// The spacing is not positive.
    BadSpacing,
    /// No measured points.
    EmptyPoints,
    /// No targets.
    NoTargets,
    /// A disc target with a zero normal or a negative radius.
    DegenerateTarget {
        /// The target ID.
        target: String,
    },
    /// The target needs more than [`MAX_SAMPLES`] samples at this spacing.
    TooManySamples {
        /// The target ID.
        target: String,
    },
    /// The first target, in definition order, whose coverage distance exceeds the threshold.
    TargetNotCovered {
        /// The target ID.
        target: String,
        /// Its coverage distance in metres.
        distance: f64,
    },
    /// A refusal of the k-d tree (`GEO.INDEX.KDTREE`), passed through by code.
    Index {
        /// The k-d tree's error code.
        code: &'static str,
    },
}

impl CoverError {
    /// The stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            CoverError::ThresholdMissing => "threshold_missing",
            CoverError::NanInput => "nan_input",
            CoverError::BadThreshold => "bad_threshold",
            CoverError::BadSpacing => "bad_spacing",
            CoverError::EmptyPoints => "empty_points",
            CoverError::NoTargets => "no_targets",
            CoverError::DegenerateTarget { .. } => "degenerate_target",
            CoverError::TooManySamples { .. } => "too_many_samples",
            CoverError::TargetNotCovered { .. } => "target_not_covered",
            CoverError::Index { .. } => "index",
        }
    }
}

impl fmt::Display for CoverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoverError::DegenerateTarget { target } | CoverError::TooManySamples { target } => {
                write!(f, "{}{{{target}}}", self.code())
            }
            CoverError::TargetNotCovered { target, distance } => write!(f, "target_not_covered{{{target},{distance}}}"),
            CoverError::Index { code } => write!(f, "index{{{code}}}"),
            _ => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for CoverError {}

fn finite3(v: &V3) -> bool {
    v.iter().all(|x| x.is_finite())
}

fn geometry_finite(g: &TargetGeometry) -> bool {
    match g {
        TargetGeometry::Point { at } => finite3(at),
        TargetGeometry::Line { start, end } => finite3(start) && finite3(end),
        TargetGeometry::Rect { corner, edge_u, edge_v } => finite3(corner) && finite3(edge_u) && finite3(edge_v),
        TargetGeometry::Disc { centre, normal, radius } => finite3(centre) && finite3(normal) && radius.is_finite(),
    }
}

fn dot(a: V3, b: V3) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn norm(a: V3) -> Result<f64, CoverError> {
    fid_num::f::sqrt(dot(a, a)).map_err(|_| CoverError::NanInput)
}

fn scale(a: V3, k: f64) -> V3 {
    [a[0] / k, a[1] / k, a[2] / k]
}

/// `max(1, ceil(q))` as a count, or `too_many_samples` above [`MAX_SAMPLES`].
fn steps(q: f64, label: &str) -> Result<u64, CoverError> {
    let c = q.ceil();
    if c.is_nan() {
        return Err(CoverError::NanInput);
    }
    if c >= MAX_SAMPLES as f64 {
        return Err(CoverError::TooManySamples { target: label.to_owned() });
    }
    Ok(if c < 1.0 { 1 } else { c as u64 })
}

/// The disc's in-plane basis `(u, v)` (module documentation).
fn disc_basis(normal: V3, label: &str) -> Result<(V3, V3), CoverError> {
    let len = norm(normal)?;
    if len == 0.0 {
        return Err(CoverError::DegenerateTarget { target: label.to_owned() });
    }
    let n = scale(normal, len);
    let k = if n[1].abs() < n[0].abs() { 1 } else { 0 };
    let k = if n[2].abs() < n[k].abs() { 2 } else { k };
    let mut e = [0.0; 3];
    e[k] = 1.0;
    let w = cross(e, n);
    let wl = norm(w)?;
    if wl == 0.0 {
        return Err(CoverError::DegenerateTarget { target: label.to_owned() });
    }
    let u = scale(w, wl);
    Ok((u, cross(n, u)))
}

/// The sample plan of one target, with an upper bound of its sample count checked against
/// [`MAX_SAMPLES`] before any sample is made (a disc's bound counts its whole grid square).
enum Plan {
    Point(V3),
    Line { a: V3, b: V3, n: u64 },
    Rect { c: V3, eu: V3, ev: V3, nu: u64, nv: u64 },
    Disc { c: V3, u: V3, v: V3, r: f64, s: f64, k: i64, m: u64 },
}

fn plan(t: &Target, s: f64) -> Result<(Plan, u64), CoverError> {
    let label = t.label.as_str();
    let too_many = || CoverError::TooManySamples { target: label.to_owned() };
    match t.geometry {
        TargetGeometry::Point { at } => Ok((Plan::Point(at), 1)),
        TargetGeometry::Line { start, end } => {
            let d = [end[0] - start[0], end[1] - start[1], end[2] - start[2]];
            let n = steps(norm(d)? / s, label)?;
            Ok((Plan::Line { a: start, b: end, n }, n + 1))
        }
        TargetGeometry::Rect { corner, edge_u, edge_v } => {
            let nu = steps(norm(edge_u)? / s, label)?;
            let nv = steps(norm(edge_v)? / s, label)?;
            let total = (nu + 1).checked_mul(nv + 1).ok_or_else(too_many)?;
            if total > MAX_SAMPLES {
                return Err(too_many());
            }
            Ok((Plan::Rect { c: corner, eu: edge_u, ev: edge_v, nu, nv }, total))
        }
        TargetGeometry::Disc { centre, normal, radius } => {
            if radius < 0.0 {
                return Err(CoverError::DegenerateTarget { target: label.to_owned() });
            }
            let (u, v) = disc_basis(normal, label)?;
            let kf = (radius / s).floor();
            if kf >= 1024.0 {
                return Err(too_many());
            }
            let k = kf as i64;
            let side = (2 * k + 1) as u64;
            let rim = (TAU * radius) / s;
            let m = steps(rim, label)?;
            let m = if m < 4 { 4 } else { m };
            let total = side * side + m;
            if total > MAX_SAMPLES {
                return Err(too_many());
            }
            Ok((Plan::Disc { c: centre, u, v, r: radius, s, k, m }, total))
        }
    }
}

/// Calls `f` on every sample of the plan, in the pinned order.
fn for_each_sample(p: &Plan, f: &mut dyn FnMut(V3) -> Result<(), CoverError>) -> Result<(), CoverError> {
    match *p {
        Plan::Point(at) => f(at),
        Plan::Line { a, b, n } => {
            for i in 0..=n {
                let t = i as f64 / n as f64;
                let w = 1.0 - t;
                f([w * a[0] + t * b[0], w * a[1] + t * b[1], w * a[2] + t * b[2]])?;
            }
            Ok(())
        }
        Plan::Rect { c, eu, ev, nu, nv } => {
            for i in 0..=nu {
                let t = i as f64 / nu as f64;
                for j in 0..=nv {
                    let w = j as f64 / nv as f64;
                    f([
                        (c[0] + t * eu[0]) + w * ev[0],
                        (c[1] + t * eu[1]) + w * ev[1],
                        (c[2] + t * eu[2]) + w * ev[2],
                    ])?;
                }
            }
            Ok(())
        }
        Plan::Disc { c, u, v, r, s, k, m } => {
            let rr = r * r;
            let at = |x: f64, y: f64| {
                [(c[0] + x * u[0]) + y * v[0], (c[1] + x * u[1]) + y * v[1], (c[2] + x * u[2]) + y * v[2]]
            };
            for j in -k..=k {
                let y = j as f64 * s;
                for i in -k..=k {
                    let x = i as f64 * s;
                    if x * x + y * y <= rr {
                        f(at(x, y))?;
                    }
                }
            }
            for i in 0..m {
                let theta = (TAU * i as f64) / m as f64;
                let cos = fid_num::f::cos(theta).map_err(|_| CoverError::NanInput)?;
                let sin = fid_num::f::sin(theta).map_err(|_| CoverError::NanInput)?;
                f(at(r * cos, r * sin))?;
            }
            Ok(())
        }
    }
}

fn index_error(e: PointIndexError) -> CoverError {
    match e {
        PointIndexError::NonFinitePoint { .. } | PointIndexError::NonFiniteQuery { .. } => CoverError::NanInput,
        e => CoverError::Index { code: e.code() },
    }
}

/// Checks that every datum target is covered by the measured `points` (module documentation).
/// `threshold` is the largest allowed coverage distance in metres, from the resolved row;
/// `spacing` is the caller's sample spacing in metres for line and area targets.
pub fn check_coverage(
    targets: &[Target],
    points: &PointBuffer,
    threshold: Option<f64>,
    spacing: f64,
) -> Result<CoverageReport, CoverError> {
    let threshold = threshold.ok_or(CoverError::ThresholdMissing)?;
    let columns = [&points.x, &points.y, &points.z];
    if !threshold.is_finite()
        || !spacing.is_finite()
        || columns.iter().any(|c| c.iter().any(|x| !x.is_finite()))
        || targets.iter().any(|t| !geometry_finite(&t.geometry))
    {
        return Err(CoverError::NanInput);
    }
    if threshold < 0.0 {
        return Err(CoverError::BadThreshold);
    }
    if spacing <= 0.0 {
        return Err(CoverError::BadSpacing);
    }
    if points.is_empty() {
        return Err(CoverError::EmptyPoints);
    }
    if targets.is_empty() {
        return Err(CoverError::NoTargets);
    }
    let mut plans = Vec::with_capacity(targets.len());
    for t in targets {
        plans.push(plan(t, spacing)?);
    }
    let index = PointIndex::build(points, 1).map_err(index_error)?;
    let mut per_target = Vec::with_capacity(targets.len());
    for (t, (p, _bound)) in targets.iter().zip(&plans) {
        let mut worst = 0.0_f64;
        let mut samples = 0_u64;
        for_each_sample(p, &mut |q| {
            samples += 1;
            let hit = index.knn(q, 1).map_err(index_error)?;
            let d2 = hit.first().ok_or(CoverError::Index { code: "inconsistent" })?.dist2;
            worst = fid_num::f::total_max(worst, d2).map_err(|_| CoverError::NanInput)?;
            Ok(())
        })?;
        let distance = fid_num::f::sqrt(worst).map_err(|_| CoverError::NanInput)?;
        per_target.push(TargetCoverage { label: t.label.clone(), distance, samples });
    }
    if let Some(c) = per_target.iter().find(|c| c.distance > threshold) {
        return Err(CoverError::TargetNotCovered { target: c.label.clone(), distance: c.distance });
    }
    Ok(CoverageReport { per_target })
}
