//! Classic 3-2-1 alignment and three-point frame solver (spec node `ALN.RPS.CLASSIC`,
//! `REQ-001`..`REQ-007`).
//!
//! Pure KNP-1 closed-form kernel math: SI metres and radians, no I/O, no `unsafe`, no panics,
//! no `HashMap`/`HashSet`, no FMA, `fid_num::f::{sqrt, atan2}`, fixed-order reductions, NaN is
//! an error.

use crate::report::{Disposition, Entry, EntryKind, EntryRef, ReportBuilder, ResidualReport};
use core::cmp::Ordering;
use core::fmt;
use fid_geo_frame::{Matrix3, Rigid};
use fid_num::f as numf;

/// Algorithm ID recorded on alignments created by `align.run_321@1`.
pub const RPS321_ALGORITHM_ID: &str = "align.rps.classic@1.0.0";
/// Default minimum angle (radians) between secondary direction and primary axis (`0.1 deg`, `REQ-004`).
pub const DEFAULT_PARALLEL_TOL_RAD: f64 = 0.1 * core::f64::consts::PI / 180.0;
/// Default minimum distance (metres) of tertiary point from secondary line (`1e-6 m`, `REQ-005`).
pub const DEFAULT_TERTIARY_TOL_M: f64 = 1e-6;
/// Default collinearity distance tolerance (metres) in `three_points` mode (`1e-6 m`, `REQ-006`).
pub const DEFAULT_COLLINEAR_TOL_M: f64 = 1e-6;
const MIN_DIR_NORM: f64 = 1e-12;

/// Signed coordinate-frame axis (`+X, -X, +Y, -Y, +Z, -Z`, `REQ-003`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SignedAxis {
    PosX,
    NegX,
    PosY,
    NegY,
    PosZ,
    NegZ,
}

impl SignedAxis {
    pub const fn as_str(self) -> &'static str {
        match self {
            SignedAxis::PosX => "+X",
            SignedAxis::NegX => "-X",
            SignedAxis::PosY => "+Y",
            SignedAxis::NegY => "-Y",
            SignedAxis::PosZ => "+Z",
            SignedAxis::NegZ => "-Z",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "+X" | "X" | "+x" | "x" => Some(SignedAxis::PosX),
            "-X" | "-x" => Some(SignedAxis::NegX),
            "+Y" | "Y" | "+y" | "y" => Some(SignedAxis::PosY),
            "-Y" | "-y" => Some(SignedAxis::NegY),
            "+Z" | "Z" | "+z" | "z" => Some(SignedAxis::PosZ),
            "-Z" | "-z" => Some(SignedAxis::NegZ),
            _ => None,
        }
    }

    pub const fn index(self) -> usize {
        match self {
            SignedAxis::PosX | SignedAxis::NegX => 0,
            SignedAxis::PosY | SignedAxis::NegY => 1,
            SignedAxis::PosZ | SignedAxis::NegZ => 2,
        }
    }

    pub const fn sign(self) -> f64 {
        match self {
            SignedAxis::PosX | SignedAxis::PosY | SignedAxis::PosZ => 1.0,
            SignedAxis::NegX | SignedAxis::NegY | SignedAxis::NegZ => -1.0,
        }
    }
}

/// Fitted/constructed geometric element supplied to classic 3-2-1 (`elements` mode, Design).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RpsElement {
    Plane { point: [f64; 3], normal: [f64; 3] },
    Line { point: [f64; 3], direction: [f64; 3] },
    Cylinder { point: [f64; 3], axis: [f64; 3] },
    Cone { point: [f64; 3], axis: [f64; 3] },
    Circle { center: [f64; 3], normal: [f64; 3] },
    Point { position: [f64; 3] },
    Sphere { center: [f64; 3] },
}

/// Geometric locus kind extracted from an [`RpsElement`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Locus {
    Plane { point: [f64; 3], normal: [f64; 3] },
    Line { point: [f64; 3], direction: [f64; 3] },
    Point([f64; 3]),
}

/// Direction and locus extracted from an [`RpsElement`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirectionSource {
    pub direction: Option<[f64; 3]>,
    pub locus: Locus,
}

impl RpsElement {
    pub fn extract(&self) -> Result<DirectionSource, Rps321Error> {
        match *self {
            RpsElement::Plane { point, normal } => {
                check_vec3(point)?;
                let n = unit_vec(normal)?;
                Ok(DirectionSource { direction: Some(n), locus: Locus::Plane { point, normal: n } })
            }
            RpsElement::Line { point, direction } => {
                check_vec3(point)?;
                let d = unit_vec(direction)?;
                Ok(DirectionSource { direction: Some(d), locus: Locus::Line { point, direction: d } })
            }
            RpsElement::Cylinder { point, axis } | RpsElement::Cone { point, axis } => {
                check_vec3(point)?;
                let a = unit_vec(axis)?;
                Ok(DirectionSource { direction: Some(a), locus: Locus::Line { point, direction: a } })
            }
            RpsElement::Circle { center, normal } => {
                check_vec3(center)?;
                let n = unit_vec(normal)?;
                Ok(DirectionSource { direction: Some(n), locus: Locus::Point(center) })
            }
            RpsElement::Point { position } => {
                check_vec3(position)?;
                Ok(DirectionSource { direction: None, locus: Locus::Point(position) })
            }
            RpsElement::Sphere { center } => {
                check_vec3(center)?;
                Ok(DirectionSource { direction: None, locus: Locus::Point(center) })
            }
        }
    }

    pub fn locus_point(&self) -> [f64; 3] {
        match *self {
            RpsElement::Plane { point, .. }
            | RpsElement::Line { point, .. }
            | RpsElement::Cylinder { point, .. }
            | RpsElement::Cone { point, .. } => point,
            RpsElement::Circle { center, .. } | RpsElement::Sphere { center } => center,
            RpsElement::Point { position } => position,
        }
    }
}

/// Tolerances for classic 3-2-1 degeneracy checks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rps321Tolerances {
    pub parallel_tol_rad: f64,
    pub tertiary_tol_m: f64,
    pub collinear_tol_m: f64,
}

impl Default for Rps321Tolerances {
    fn default() -> Self {
        Self {
            parallel_tol_rad: DEFAULT_PARALLEL_TOL_RAD,
            tertiary_tol_m: DEFAULT_TERTIARY_TOL_M,
            collinear_tol_m: DEFAULT_COLLINEAR_TOL_M,
        }
    }
}

/// Result of [`solve_321_elements`] or [`solve_321_three_points`].
#[derive(Debug, Clone, PartialEq)]
pub struct Rps321Solution {
    /// Rigid transform mapping measured world coordinates into the constructed part frame
    /// (`p_part = R * p_world + t`, `det(R) = +1`).
    pub transform: Rigid,
    /// Part-frame origin in world coordinates.
    pub origin_world: [f64; 3],
    /// Residual report with the 3 locus-condition distances (0 by construction).
    pub report: ResidualReport,
}

/// Errors returned by [`solve_321_elements`] and [`solve_321_three_points`].
#[derive(Debug, Clone, PartialEq)]
pub enum Rps321Error {
    InvalidAxisAssignment,
    SecondaryParallelToPrimary { angle_rad: f64 },
    TertiaryOnSecondary { distance_m: f64 },
    CollinearPoints { distance_m: f64 },
    NoDirection { role: &'static str },
    NanInput,
    SolveFailed(&'static str),
}

impl Rps321Error {
    pub const fn code(&self) -> &'static str {
        match self {
            Rps321Error::InvalidAxisAssignment => "invalid_axis_assignment",
            Rps321Error::SecondaryParallelToPrimary { .. } => "secondary_parallel_to_primary",
            Rps321Error::TertiaryOnSecondary { .. } => "tertiary_on_secondary",
            Rps321Error::CollinearPoints { .. } => "collinear_points",
            Rps321Error::NoDirection { .. } => "no_direction",
            Rps321Error::NanInput => "nan_input",
            Rps321Error::SolveFailed(_) => "solve_failed",
        }
    }
}

impl fmt::Display for Rps321Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rps321Error::InvalidAxisAssignment => f.write_str("invalid_axis_assignment"),
            Rps321Error::SecondaryParallelToPrimary { angle_rad } => {
                write!(f, "secondary_parallel_to_primary{{angle_rad={angle_rad:e}}}")
            }
            Rps321Error::TertiaryOnSecondary { distance_m } => {
                write!(f, "tertiary_on_secondary{{distance_m={distance_m:e}}}")
            }
            Rps321Error::CollinearPoints { distance_m } => {
                write!(f, "collinear_points{{distance_m={distance_m:e}}}")
            }
            Rps321Error::NoDirection { role } => write!(f, "no_direction{{{role}}}"),
            Rps321Error::NanInput => f.write_str("nan_input"),
            Rps321Error::SolveFailed(s) => write!(f, "solve_failed{{{s}}}"),
        }
    }
}

impl std::error::Error for Rps321Error {}

/// Solves classic 3-2-1 alignment in `elements` mode (`REQ-001`..`REQ-005`, `REQ-007`).
pub fn solve_321_elements(
    primary: &RpsElement,
    secondary: &RpsElement,
    tertiary: &RpsElement,
    primary_axis: SignedAxis,
    secondary_axis: SignedAxis,
    tol: Rps321Tolerances,
) -> Result<Rps321Solution, Rps321Error> {
    if primary_axis.index() == secondary_axis.index() {
        return Err(Rps321Error::InvalidAxisAssignment);
    }
    if !tol.parallel_tol_rad.is_finite() || !tol.tertiary_tol_m.is_finite() {
        return Err(Rps321Error::NanInput);
    }

    let s1 = primary.extract()?;
    let s2 = secondary.extract()?;
    let _s3 = tertiary.extract()?;

    let u1 = s1.direction.ok_or(Rps321Error::NoDirection { role: "primary" })?;
    let d2 = s2.direction.ok_or(Rps321Error::NoDirection { role: "secondary" })?;

    // REQ-004: reject secondary nearly parallel (or antiparallel) to primary.
    let angle = unoriented_angle(u1, d2)?;
    if angle.total_cmp(&tol.parallel_tol_rad) == Ordering::Less {
        return Err(Rps321Error::SecondaryParallelToPrimary { angle_rad: angle });
    }

    // Gram-Schmidt: project secondary direction perpendicular to u1.
    let v2 = sub(d2, scale(u1, dot(d2, u1)));
    let u2 = unit_vec(v2)?;
    let u3 = unit_vec(cross(u1, u2))?;

    // REQ-005: if secondary has a line locus, reject tertiary point within tol of secondary line.
    let p3 = tertiary.locus_point();
    if let Locus::Line { point: p2_line, direction: d2_line } = s2.locus {
        let rel = sub(p3, p2_line);
        let perp = sub(rel, scale(d2_line, dot(rel, d2_line)));
        let dist = norm3(perp)?;
        if dist.total_cmp(&tol.tertiary_tol_m) == Ordering::Less {
            return Err(Rps321Error::TertiaryOnSecondary { distance_m: dist });
        }
    }

    // REQ-002: origin components in orthonormal basis (u1, u2, u3).
    // Along u1: primary locus; along u3 (in-plane normal to secondary): secondary locus; along u2: tertiary point.
    let p1 = primary.locus_point();
    let p2 = secondary.locus_point();
    let o1 = dot(p1, u1);
    let o3 = dot(p2, u3);
    let o2 = dot(p3, u2);
    let origin = add(add(scale(u1, o1), scale(u2, o2)), scale(u3, o3));
    let origin = [canonical_zero(origin[0]), canonical_zero(origin[1]), canonical_zero(origin[2])];

    let r = build_frame_rotation(u1, u2, primary_axis, secondary_axis)?;
    let ro = mat_vec(&r, origin);
    let t = [canonical_zero(-ro[0]), canonical_zero(-ro[1]), canonical_zero(-ro[2])];
    let transform = Rigid::new(r, t).map_err(|_| Rps321Error::SolveFailed("non_rigid_321"))?;

    // Locus residual distances (0 by construction).
    let res0 = canonical_zero((dot(origin, u1) - o1).abs());
    let res1 = canonical_zero((dot(origin, u3) - o3).abs());
    let res2 = canonical_zero((dot(origin, u2) - o2).abs());
    let report = build_report([res0, res1, res2])?;
    Ok(Rps321Solution { transform, origin_world: origin, report })
}

/// Solves classic 3-2-1 alignment in `three_points` mode (`REQ-006`, `AC-5`).
///
/// `p0` is the origin, `p0 -> p1` defines `primary_axis`, and the plane `(p0, p1, p2)`
/// defines `secondary_axis` (default `+X`, `+Y`).
pub fn solve_321_three_points(
    points: [[f64; 3]; 3],
    primary_axis: SignedAxis,
    secondary_axis: SignedAxis,
    tol: Rps321Tolerances,
) -> Result<Rps321Solution, Rps321Error> {
    if primary_axis.index() == secondary_axis.index() {
        return Err(Rps321Error::InvalidAxisAssignment);
    }
    if !tol.collinear_tol_m.is_finite() {
        return Err(Rps321Error::NanInput);
    }
    for p in points {
        check_vec3(p)?;
    }

    let [p0, p1, p2] = points;
    let d1 = sub(p1, p0);
    let len1 = norm3(d1)?;
    if len1.total_cmp(&tol.collinear_tol_m) == Ordering::Less {
        return Err(Rps321Error::CollinearPoints { distance_m: len1 });
    }
    let u1 = scale(d1, 1.0 / len1);

    let d2 = sub(p2, p0);
    let perp = sub(d2, scale(u1, dot(d2, u1)));
    let dist2 = norm3(perp)?;
    if dist2.total_cmp(&tol.collinear_tol_m) == Ordering::Less {
        return Err(Rps321Error::CollinearPoints { distance_m: dist2 });
    }
    let u2 = scale(perp, 1.0 / dist2);

    let origin = [canonical_zero(p0[0]), canonical_zero(p0[1]), canonical_zero(p0[2])];
    let r = build_frame_rotation(u1, u2, primary_axis, secondary_axis)?;
    let ro = mat_vec(&r, origin);
    let t = [canonical_zero(-ro[0]), canonical_zero(-ro[1]), canonical_zero(-ro[2])];
    let transform = Rigid::new(r, t).map_err(|_| Rps321Error::SolveFailed("non_rigid_3pts"))?;
    let report = build_report([0.0, 0.0, 0.0])?;
    Ok(Rps321Solution { transform, origin_world: origin, report })
}

fn build_frame_rotation(
    u1: [f64; 3],
    u2: [f64; 3],
    primary_axis: SignedAxis,
    secondary_axis: SignedAxis,
) -> Result<Matrix3, Rps321Error> {
    let i1 = primary_axis.index();
    let i2 = secondary_axis.index();
    let i3 = 3 - i1 - i2;

    let mut axes = [[0.0_f64; 3]; 3];
    axes[i1] = scale(u1, primary_axis.sign());
    axes[i2] = scale(u2, secondary_axis.sign());
    axes[i3] = match i3 {
        0 => cross(axes[1], axes[2]),
        1 => cross(axes[2], axes[0]),
        _ => cross(axes[0], axes[1]),
    };
    axes[i3] = unit_vec(axes[i3])?;
    Ok([
        [canonical_zero(axes[0][0]), canonical_zero(axes[0][1]), canonical_zero(axes[0][2])],
        [canonical_zero(axes[1][0]), canonical_zero(axes[1][1]), canonical_zero(axes[1][2])],
        [canonical_zero(axes[2][0]), canonical_zero(axes[2][1]), canonical_zero(axes[2][2])],
    ])
}

fn build_report(res: [f64; 3]) -> Result<ResidualReport, Rps321Error> {
    let mut rb = ReportBuilder::new();
    for (i, &r) in res.iter().enumerate() {
        rb.push(
            Entry {
                reference: EntryRef { input: u32::try_from(i).unwrap_or(u32::MAX), item: 0 },
                kind: EntryKind::Feature,
                residual_m: r,
                vector_m: None,
                weight: None,
            },
            Disposition::Used,
        )
        .map_err(|_| Rps321Error::SolveFailed("report_push"))?;
    }
    rb.finish().map_err(|_| Rps321Error::SolveFailed("report_finish"))
}

// ------------------------------------------------------------------ KNP-1 helpers

fn check_vec3(v: [f64; 3]) -> Result<(), Rps321Error> {
    if v[0].is_finite() && v[1].is_finite() && v[2].is_finite() {
        Ok(())
    } else {
        Err(Rps321Error::NanInput)
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] * b[0]) + (a[1] * b[1])) + (a[2] * b[2])
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [(a[1] * b[2]) - (a[2] * b[1]), (a[2] * b[0]) - (a[0] * b[2]), (a[0] * b[1]) - (a[1] * b[0])]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(v: [f64; 3], s: f64) -> [f64; 3] {
    [v[0] * s, v[1] * s, v[2] * s]
}

fn mat_vec(a: &Matrix3, v: [f64; 3]) -> [f64; 3] {
    [dot(a[0], v), dot(a[1], v), dot(a[2], v)]
}

fn norm3(v: [f64; 3]) -> Result<f64, Rps321Error> {
    check_vec3(v)?;
    let n2 = dot(v, v);
    if !n2.is_finite() {
        return Err(Rps321Error::NanInput);
    }
    numf::sqrt(n2).map_err(|_| Rps321Error::NanInput)
}

fn unit_vec(v: [f64; 3]) -> Result<[f64; 3], Rps321Error> {
    let n = norm3(v)?;
    if n.total_cmp(&MIN_DIR_NORM) == Ordering::Less {
        return Err(Rps321Error::NoDirection { role: "degenerate" });
    }
    let out = scale(v, 1.0 / n);
    check_vec3(out)?;
    Ok(out)
}

fn unoriented_angle(u: [f64; 3], v: [f64; 3]) -> Result<f64, Rps321Error> {
    let sin_a = norm3(cross(u, v))?;
    let cos_a = dot(u, v).abs();
    numf::atan2(sin_a, cos_a).map_err(|_| Rps321Error::NanInput)
}

fn canonical_zero(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}
