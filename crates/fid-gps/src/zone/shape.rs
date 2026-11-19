//! Tolerance-zone shapes and their fixed-pose usage. Spec node `GDT.ZONE.ZTYPE`.
//!
//! Each [`ZoneShape`], given a [`Pose`] and the toleranced points, answers two pure questions:
//! how large the zone must be, centred where the pose puts it, to contain every point (its
//! *usage*), and which point attains that size (the *driving* point). The pose optimizer
//! (`GDT.ZONE.POSE`) varies the pose; the tolerance families choose the shape. Nothing here
//! searches or iterates: every function is a closed form evaluated once per point.
//!
//! # Shapes and their size convention
//!
//! Sizes are full zone widths or diameters in SI metres, never half-widths or radii, and
//! never negative. With `o` the pose origin, `n` the pose direction and `r = p - o`:
//!
//! - [`ZoneShape::TwoPlanes`]: two parallel planes with unit normal `n`, symmetric about the
//!   plane through `o`. Size = the distance between the planes = `2 max |n·r|`.
//! - [`ZoneShape::TwoLines`]: two parallel lines lying in a plane with unit normal `plane`,
//!   symmetric about the line through `o`; `n` is the in-plane direction across the lines and
//!   must lie in the plane. Size = `2 max |n·r|`; the out-of-plane component of a point plays
//!   no part (the zone is a 2D zone in that plane).
//! - [`ZoneShape::Cylinder`]: a cylinder with axis through `o` along `n`. Size = diameter =
//!   `2 max |r − (n·r) n|`.
//! - [`ZoneShape::Circle`]: a circle centred at `o` in the plane with unit normal `plane`
//!   (a 2D zone, for instance a position tolerance of a point in a section). Size =
//!   diameter = `2 max |r − (m·r) m|` with `m = plane`; the pose direction is not used.
//! - [`ZoneShape::Sphere`]: a sphere centred at `o`. Size = diameter = `2 max |r|`; the pose
//!   direction is not used.
//! - [`ZoneShape::Offset`]: a uniform offset of a nominal surface or line. The caller passes
//!   each toleranced point's signed deviation `d_i` from the nominal and the zone's
//!   disposition shift `c` (the signed offset of the zone's mid-surface from the nominal: 0
//!   for a zone symmetric about the nominal), both under the caller's one sign convention
//!   (this module needs only that they agree). Size = total width = `2 max |d_i − c|`. The
//!   pose is not used by the arithmetic (the deviations were taken at it) but is still
//!   validated.
//!
//! # Pinned expressions (REQ-GDT.ZONE.ZTYPE-005, D0)
//!
//! With `dot3(a, b) = (a0 * b0 + a1 * b1) + a2 * b2` (the order of `fid-geo-frame`) and
//! `r = [p0 - o0, p1 - o1, p2 - o2]`, the per-point value `w_i` is:
//!
//! - two planes, two lines: `abs(dot3(n, r))`;
//! - cylinder (`a = n`) and circle (`a = plane`): `h = dot3(a, r)`,
//!   `q = [r0 - h * a0, r1 - h * a1, r2 - h * a2]`, `w = sqrt(dot3(q, q))`;
//! - sphere: `w = sqrt(dot3(r, r))`;
//! - offset: `w = abs(d_i - c)`.
//!
//! `sqrt` is `fid_num::f::sqrt` (IEEE 754 correctly rounded). Rust never contracts `a * b + c`
//! into a fused multiply-add and no `f64::max`/`f64::min` is used, so every platform,
//! including wasm32, produces the same bits. The maximum runs in index order from `w_0`; a
//! later value replaces the current one only if it is strictly greater, so on ties the lowest
//! index drives (REQ-GDT.ZONE.ZTYPE-001). NaN never reaches the comparison (it is rejected
//! first), so `>` is a total order there. Size = `2.0 * max`, exact in binary floating point.
//!
//! The expressions take the pose direction and plane normal as given (no renormalisation);
//! they are accepted only within [`UNIT_TOL`] of unit length.
//!
//! # Accuracy
//!
//! The result is the pinned expression's bits, not the correctly rounded exact value. Let
//! `ε = 2^-53`, `S` be the exact value of the usage formula on the given f64 inputs (the
//! direction and plane normal as given), and `R = max_i |p_i − o|` (Euclidean). Then for the
//! geometric shapes
//!
//! `|size − S| ≤ 32 ε R + 2^-535 m`,
//!
//! and for the offset shape `|size − S| ≤ 2 ε max_i |d_i − c|`. Derivation, per point (the
//! maximum and the exact doubling add nothing): each component of `r` carries one rounding
//! (`ε |r|` in 2-norm); `dot3` of two vectors errs by at most `γ3 |a| |b|` with
//! `γ3 = 3ε / (1 − 3ε)`. Planes and lines: `(ε + γ3(1 + ε)) |n| |r| < 5 ε |r|`. Round shapes:
//! `|q̂ − q| ≤ (ε + 4ε + ε + ε) |r|` (the rounded `r`, the error of `h` times `|a|`, the
//! product `h·a_k` and the subtraction), then `dot3(q̂, q̂)` and the square root add a
//! relative `1.5 ε + ε`, below `10 ε |r|` in all, with `|a| ≤ 1 + 1e-12` absorbed. The bound
//! takes `c = 16` per point (`32` on the size) for margin. The `2^-535` term covers
//! underflow: below about `1.5e-154 m` (`2^-511`) a squared component is subnormal and loses
//! relative precision, and below about `1.5e-162 m` it flushes to zero; each square then errs
//! by at most `2^-1075` absolutely, so a distance errs by at most `sqrt(3 · 2^-1075) < 2^-536`.
//! A size can therefore read 0 for a point that is not exactly on the zone reference (a
//! point `1e-163 m` from a sphere centre gives size 0).
//!
//! The error is absolute, relative to `R`, not to the size: a point far from the origin but
//! close to the reference (a near-axis point 1 km along a cylinder axis, a point 1e6 m from
//! the origin near the datum plane) may be off by many ulps of its small distance. Callers
//! that need small sizes at large coordinates should put the pose origin near the points.
//! Above about `1.3e154 m` a squared component overflows and the call returns `overflow`
//! (typed error, never a wrong finite size). Bit-exact agreement with the exact value holds
//! only when every intermediate is exact (for instance small dyadic coordinates with axis
//! directions, the `bits` class of the analytic fixture).
//!
//! # Validation order (pinned)
//!
//! Each call checks, in this order, and returns the first failure:
//!
//! 1. input kind: [`ZoneShape::Offset`] takes [`ZoneInput::Deviations`], every other shape
//!    [`ZoneInput::Points`] (`input_kind_mismatch`);
//! 2. one deviation per point (`deviation_count_mismatch`);
//! 3. at least one point (`empty_points`), at most [`MAX_POINTS`] (`too_many_points`);
//! 4. any NaN in the pose, the shape parameters, the `contains` size, the points or the
//!    deviations (`nan_input`);
//! 5. any infinity in the same values (`non_finite_input`);
//! 6. the pose direction, then the plane normal (two lines, circle), within [`UNIT_TOL`] of
//!    unit length: `|sqrt(dot3(d, d)) - 1| <= UNIT_TOL` (`non_unit_direction`);
//! 7. two lines only: `|dot3(n, plane)| <= IN_PLANE_TOL` (`direction_not_in_plane`);
//! 8. while evaluating: a per-point value, or the doubled maximum, that is not finite
//!    (`overflow`; possible only for coordinates near the f64 range, about 1e154 m for the
//!    round shapes).

use core::fmt;

use fid_geo_frame::Rigid;

/// A point or vector in SI metres (or a dimensionless direction).
pub type P3 = [f64; 3];

/// Largest accepted `|‖d‖ − 1|` for the pose direction and a plane normal
/// (REQ-GDT.ZONE.ZTYPE-004).
pub const UNIT_TOL: f64 = 1e-12;

/// Largest accepted `|n · plane|` for [`ZoneShape::TwoLines`] (the line-crossing direction
/// must lie in the plane of the lines).
pub const IN_PLANE_TOL: f64 = 1e-12;

/// Most points one call accepts: the driving index is a `u32`.
pub const MAX_POINTS: usize = u32::MAX as usize;

/// Where a zone sits: its origin (centre point, mid-plane point or axis point) and its
/// direction (plane normal, line-crossing direction or axis), in the part frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    /// The zone origin in SI metres.
    pub origin: P3,
    /// The zone direction; must be unit within [`UNIT_TOL`].
    pub direction: P3,
}

impl Pose {
    /// A pose from its parts (validated when used).
    pub fn new(origin: P3, direction: P3) -> Pose {
        Pose { origin, direction }
    }

    /// The pose of a zone whose local frame maps to the part frame by `t`: the origin is the
    /// image of the local origin (`t`'s translation) and the direction the image of the local
    /// z axis (the third column of `t`'s rotation), both copied without arithmetic.
    pub fn from_rigid(t: &Rigid) -> Pose {
        let r = t.r();
        Pose { origin: t.t(), direction: [r[0][2], r[1][2], r[2][2]] }
    }
}

/// A tolerance-zone shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZoneShape {
    /// Two parallel planes, normal = the pose direction.
    TwoPlanes,
    /// Two parallel lines in the plane with unit normal `plane`; the pose direction crosses
    /// the lines within that plane.
    TwoLines {
        /// Unit normal of the plane of the lines.
        plane: P3,
    },
    /// A cylinder, axis through the pose origin along the pose direction.
    Cylinder,
    /// A circle centred at the pose origin in the plane with unit normal `plane`.
    Circle {
        /// Unit normal of the plane of the circle.
        plane: P3,
    },
    /// A sphere centred at the pose origin.
    Sphere,
    /// A uniform offset of a nominal surface or line, mid-surface shifted by
    /// `disposition_shift` (metres, signed like the deviations).
    Offset {
        /// Signed offset of the zone's mid-surface from the nominal, in metres.
        disposition_shift: f64,
    },
}

/// The toleranced input of one call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZoneInput<'a> {
    /// Toleranced points (every shape except [`ZoneShape::Offset`]).
    Points(&'a [P3]),
    /// Toleranced points with their signed deviations from the nominal, one per point
    /// ([`ZoneShape::Offset`]); the driving index refers to both.
    Deviations {
        /// The toleranced points.
        points: &'a [P3],
        /// Their signed deviations from the nominal, in metres.
        deviations: &'a [f64],
    },
}

/// The fixed-pose usage of a zone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Usage {
    /// Smallest zone size (full width or diameter) containing every point, in metres.
    pub size_m: f64,
    /// Index of the point that attains it (lowest on ties).
    pub driving_index: u32,
}

/// Why a usage or containment could not be computed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZoneError {
    /// The input kind does not fit the shape (deviations for a geometric shape, or points
    /// without deviations for an offset zone).
    InputKindMismatch,
    /// The number of deviations differs from the number of points.
    DeviationCountMismatch {
        /// Number of points.
        points: usize,
        /// Number of deviations.
        deviations: usize,
    },
    /// No toleranced point.
    EmptyPoints,
    /// More than [`MAX_POINTS`] points.
    TooManyPoints {
        /// Number of points given.
        count: usize,
    },
    /// An input value is NaN.
    NanInput,
    /// An input value is infinite.
    NonFiniteInput,
    /// The pose direction or a plane normal is not unit within [`UNIT_TOL`].
    NonUnitDirection {
        /// Its length (`+∞` if the squared length overflows).
        norm: f64,
    },
    /// The two-lines crossing direction is not in the plane of the lines.
    DirectionNotInPlane {
        /// `n · plane`.
        dot: f64,
    },
    /// A per-point value or the size overflowed.
    Overflow {
        /// The point being evaluated, or the driving point when the doubling overflowed.
        index: u32,
    },
}

impl ZoneError {
    /// The stable snake_case error code.
    pub fn code(&self) -> &'static str {
        match self {
            ZoneError::InputKindMismatch => "input_kind_mismatch",
            ZoneError::DeviationCountMismatch { .. } => "deviation_count_mismatch",
            ZoneError::EmptyPoints => "empty_points",
            ZoneError::TooManyPoints { .. } => "too_many_points",
            ZoneError::NanInput => "nan_input",
            ZoneError::NonFiniteInput => "non_finite_input",
            ZoneError::NonUnitDirection { .. } => "non_unit_direction",
            ZoneError::DirectionNotInPlane { .. } => "direction_not_in_plane",
            ZoneError::Overflow { .. } => "overflow",
        }
    }
}

impl fmt::Display for ZoneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ZoneError::DeviationCountMismatch { points, deviations } => {
                write!(f, "deviation_count_mismatch ({points} points, {deviations} deviations)")
            }
            ZoneError::TooManyPoints { count } => write!(f, "too_many_points ({count})"),
            ZoneError::NonUnitDirection { norm } => write!(f, "non_unit_direction (norm {norm})"),
            ZoneError::DirectionNotInPlane { dot } => write!(f, "direction_not_in_plane (dot {dot:e})"),
            ZoneError::Overflow { index } => write!(f, "overflow (point {index})"),
            other => f.write_str(other.code()),
        }
    }
}

impl std::error::Error for ZoneError {}

fn dot3(a: &P3, b: &P3) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn sub(p: &P3, o: &P3) -> P3 {
    [p[0] - o[0], p[1] - o[1], p[2] - o[2]]
}

/// Correctly rounded square root of a sum of squares; `+∞` if the sum overflowed.
fn root(s: f64) -> f64 {
    if s.is_finite() {
        // Unreachable fallback: `s` is a finite sum of squares, hence `+0` or positive (never
        // NaN, never negative, never `-0`), and `fid_num::f::sqrt` fails only on NaN, infinite
        // or negative arguments. `+∞` keeps the function total without a panic path.
        fid_num::f::sqrt(s).unwrap_or(f64::INFINITY)
    } else {
        f64::INFINITY
    }
}

/// Distance of `r` from the line through the origin along `a`: `|r − (a·r) a|`.
fn off_axis(a: &P3, r: &P3) -> f64 {
    let h = dot3(a, r);
    let q = [r[0] - h * a[0], r[1] - h * a[1], r[2] - h * a[2]];
    root(dot3(&q, &q))
}

/// NaN anywhere wins over an infinity anywhere (validation steps 4 and 5).
#[derive(Default)]
struct Scan {
    nan: bool,
    inf: bool,
}

impl Scan {
    fn one(&mut self, x: f64) {
        self.nan |= x.is_nan();
        self.inf |= x.is_infinite();
    }

    fn vec(&mut self, v: &P3) {
        for x in v {
            self.one(*x);
        }
    }

    fn result(&self) -> Result<(), ZoneError> {
        if self.nan {
            Err(ZoneError::NanInput)
        } else if self.inf {
            Err(ZoneError::NonFiniteInput)
        } else {
            Ok(())
        }
    }
}

fn check_count(count: usize) -> Result<(), ZoneError> {
    if count == 0 {
        Err(ZoneError::EmptyPoints)
    } else if count > MAX_POINTS {
        Err(ZoneError::TooManyPoints { count })
    } else {
        Ok(())
    }
}

/// The length `sqrt(dot3(d, d))` (`+∞` on overflow) if it is not within [`UNIT_TOL`] of 1.
fn check_unit(d: &P3) -> Result<(), ZoneError> {
    let norm = root(dot3(d, d));
    if (norm - 1.0).abs() > UNIT_TOL {
        Err(ZoneError::NonUnitDirection { norm })
    } else {
        Ok(())
    }
}

fn index_u32(i: usize) -> Result<u32, ZoneError> {
    u32::try_from(i).map_err(|_| ZoneError::TooManyPoints { count: i })
}

/// The maximum of `w(0), …, w(n − 1)` in index order (strictly greater replaces), doubled.
fn max_doubled(n: usize, w: impl Fn(usize) -> f64) -> Result<Usage, ZoneError> {
    let mut best = 0.0;
    let mut best_i = 0usize;
    for i in 0..n {
        let wi = w(i);
        if !wi.is_finite() {
            return Err(ZoneError::Overflow { index: index_u32(i)? });
        }
        if i == 0 || wi > best {
            best = wi;
            best_i = i;
        }
    }
    let driving_index = index_u32(best_i)?;
    let size_m = 2.0 * best;
    if !size_m.is_finite() {
        return Err(ZoneError::Overflow { index: driving_index });
    }
    Ok(Usage { size_m, driving_index })
}

impl ZoneShape {
    /// The stable snake_case name of the shape.
    pub fn name(&self) -> &'static str {
        match self {
            ZoneShape::TwoPlanes => "two_planes",
            ZoneShape::TwoLines { .. } => "two_lines",
            ZoneShape::Cylinder => "cylinder",
            ZoneShape::Circle { .. } => "circle",
            ZoneShape::Sphere => "sphere",
            ZoneShape::Offset { .. } => "offset",
        }
    }

    /// The smallest zone size, centred as the shape defines at `pose`, that contains every
    /// toleranced point, and the point that attains it (lowest index on ties)
    /// (REQ-GDT.ZONE.ZTYPE-001, -002).
    pub fn usage(&self, pose: &Pose, input: ZoneInput<'_>) -> Result<Usage, ZoneError> {
        self.validate(pose, input, None)?;
        self.evaluate(pose, input)
    }

    /// True exactly when `usage(pose, input).size_m <= size`, an exact f64 comparison with no
    /// tolerance (REQ-GDT.ZONE.ZTYPE-003). `size` must not be NaN or infinite; a negative
    /// size contains nothing.
    pub fn contains(&self, pose: &Pose, size: f64, input: ZoneInput<'_>) -> Result<bool, ZoneError> {
        self.validate(pose, input, Some(size))?;
        Ok(self.evaluate(pose, input)?.size_m <= size)
    }

    fn validate(&self, pose: &Pose, input: ZoneInput<'_>, size: Option<f64>) -> Result<(), ZoneError> {
        let is_offset = matches!(self, ZoneShape::Offset { .. });
        let (points, deviations) = match input {
            ZoneInput::Points(points) if !is_offset => (points, None),
            ZoneInput::Deviations { points, deviations } if is_offset => (points, Some(deviations)),
            _ => return Err(ZoneError::InputKindMismatch),
        };
        if let Some(d) = deviations {
            if d.len() != points.len() {
                return Err(ZoneError::DeviationCountMismatch { points: points.len(), deviations: d.len() });
            }
        }
        check_count(points.len())?;

        let mut scan = Scan::default();
        scan.vec(&pose.origin);
        scan.vec(&pose.direction);
        match self {
            ZoneShape::TwoLines { plane } | ZoneShape::Circle { plane } => scan.vec(plane),
            ZoneShape::Offset { disposition_shift } => scan.one(*disposition_shift),
            ZoneShape::TwoPlanes | ZoneShape::Cylinder | ZoneShape::Sphere => {}
        }
        if let Some(s) = size {
            scan.one(s);
        }
        for p in points {
            scan.vec(p);
        }
        for d in deviations.unwrap_or(&[]) {
            scan.one(*d);
        }
        scan.result()?;

        check_unit(&pose.direction)?;
        match self {
            ZoneShape::TwoLines { plane } => {
                check_unit(plane)?;
                let dot = dot3(&pose.direction, plane);
                if dot.abs() > IN_PLANE_TOL {
                    return Err(ZoneError::DirectionNotInPlane { dot });
                }
            }
            ZoneShape::Circle { plane } => check_unit(plane)?,
            _ => {}
        }
        Ok(())
    }

    /// Steps 8 of the validation order on validated input.
    fn evaluate(&self, pose: &Pose, input: ZoneInput<'_>) -> Result<Usage, ZoneError> {
        let o = &pose.origin;
        let n = &pose.direction;
        match (self, input) {
            (ZoneShape::Offset { disposition_shift }, ZoneInput::Deviations { deviations, .. }) => {
                let c = *disposition_shift;
                max_doubled(deviations.len(), |i| (deviations[i] - c).abs())
            }
            (ZoneShape::TwoPlanes | ZoneShape::TwoLines { .. }, ZoneInput::Points(pts)) => {
                max_doubled(pts.len(), |i| dot3(n, &sub(&pts[i], o)).abs())
            }
            (ZoneShape::Cylinder, ZoneInput::Points(pts)) => max_doubled(pts.len(), |i| off_axis(n, &sub(&pts[i], o))),
            (ZoneShape::Circle { plane }, ZoneInput::Points(pts)) => {
                max_doubled(pts.len(), |i| off_axis(plane, &sub(&pts[i], o)))
            }
            (ZoneShape::Sphere, ZoneInput::Points(pts)) => max_doubled(pts.len(), |i| {
                let r = sub(&pts[i], o);
                root(dot3(&r, &r))
            }),
            _ => Err(ZoneError::InputKindMismatch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_limits() {
        assert_eq!(check_count(0), Err(ZoneError::EmptyPoints));
        assert_eq!(check_count(1), Ok(()));
        assert_eq!(check_count(MAX_POINTS), Ok(()));
        if let Some(over) = MAX_POINTS.checked_add(1) {
            assert_eq!(check_count(over), Err(ZoneError::TooManyPoints { count: over }));
        }
    }

    #[test]
    fn overflow_index_is_reported() {
        let r = max_doubled(3, |i| if i == 2 { f64::INFINITY } else { 1.0 });
        assert_eq!(r, Err(ZoneError::Overflow { index: 2 }));
        let r = max_doubled(2, |i| if i == 1 { f64::MAX } else { 1.0 });
        assert_eq!(r, Err(ZoneError::Overflow { index: 1 }));
    }

    #[test]
    fn non_unit_overflow_reports_infinite_norm() {
        assert_eq!(check_unit(&[1e200, 0.0, 0.0]), Err(ZoneError::NonUnitDirection { norm: f64::INFINITY }));
    }
}
