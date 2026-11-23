//! Pure KNP-1 distance and caliper evaluation (spec node `GDT.DIM.LINEAR`, `REQ-001`..`REQ-006`).
//!
//! Pure Rust, `f64` SI metres, no I/O, no `unsafe`, no panics, no `HashMap`/`HashSet`, no FMA,
//! transcendentals only through `fid_num::f`, fixed-order reductions, total-order min/max.

use core::cmp::Ordering;
use core::fmt;
use fid_num::f as numf;
use fid_num::fmt::{self as numfmt, FormatSpec, FormatterId, Precision, Reported, Unit as FmtUnit};

/// Algorithm ID for distance checks (`GDT.DIM.LINEAR`).
pub const DISTANCE_ALGORITHM_ID: &str = "gdt.dim.distance@1.0.0";
/// Algorithm ID for caliper extent checks (`GDT.DIM.LINEAR`).
pub const CALIPER_ALGORITHM_ID: &str = "gdt.dim.caliper@1.0.0";
/// Minimum direction vector norm before returning `degenerate_element`.
pub const MIN_DIR_NORM: f64 = 1e-12;
/// Plane-plane normal angle threshold in radians above which `non_parallel_planes` is flagged (`REQ-004`).
pub const NON_PARALLEL_THRESHOLD_RAD: f64 = 1e-6;
/// Exact SI metres per international inch.
pub const METRES_PER_INCH: f64 = 0.0254;
/// Default decimal places used by `fmt.length@1` for authored-unit reporting (`REQ-005`).
pub const DEFAULT_AUTHORED_DECIMALS: u8 = 6;

/// Supported authored length units (`REQ-005`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LengthUnit {
    Metre,
    Millimetre,
    Micrometre,
    Inch,
}

impl LengthUnit {
    pub const fn as_str(self) -> &'static str {
        match self {
            LengthUnit::Metre => "m",
            LengthUnit::Millimetre => "mm",
            LengthUnit::Micrometre => "um",
            LengthUnit::Inch => "in",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "m" | "si:m" => Some(LengthUnit::Metre),
            "mm" => Some(LengthUnit::Millimetre),
            "um" | "µm" => Some(LengthUnit::Micrometre),
            "in" => Some(LengthUnit::Inch),
            _ => None,
        }
    }

    /// Corresponding [`fid_num::fmt::Unit`] for `fmt.length@1` (`REQ-005`).
    pub const fn to_fmt_unit(self) -> FmtUnit {
        match self {
            LengthUnit::Metre => FmtUnit::M,
            LengthUnit::Millimetre => FmtUnit::Mm,
            LengthUnit::Micrometre => FmtUnit::Um,
            LengthUnit::Inch => FmtUnit::In,
        }
    }

    /// Converts an SI metre value into this authored unit (`REQ-005`).
    pub fn from_si(self, metres: f64) -> f64 {
        let v = match self {
            LengthUnit::Metre => metres,
            LengthUnit::Millimetre => metres * 1_000.0,
            LengthUnit::Micrometre => metres * 1_000_000.0,
            LengthUnit::Inch => metres / METRES_PER_INCH,
        };
        canonical_zero(v)
    }

    /// Formats `metres` in this authored unit via the single kernel formatter `fmt.length@1`
    /// (CONSTITUTION §4, `REQ-005`).
    pub fn format_si(self, metres: f64, decimals: u8) -> Result<Reported, LinearError> {
        let spec = FormatSpec {
            formatter: FormatterId::LENGTH_V1,
            unit: self.to_fmt_unit(),
            precision: Precision::Decimals(decimals),
            limits: None,
        };
        numfmt::len::format(metres, &spec).map_err(|_| LinearError::NanInput)
    }
}

/// Element geometry accepted by `evaluate_distance` (Scope table).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LinearElement {
    /// Point position (circle and sphere centres count as points with `raw_point: false`; raw
    /// measured points set `raw_point: true`).
    Point { position: [f64; 3], raw_point: bool },
    /// 3D line (point on line + direction vector).
    Line { point: [f64; 3], direction: [f64; 3] },
    /// 3D plane (centroid + normal vector).
    Plane { centroid: [f64; 3], normal: [f64; 3] },
}

impl LinearElement {
    pub const fn kind_str(self) -> &'static str {
        match self {
            LinearElement::Point { .. } => "point",
            LinearElement::Line { .. } => "line",
            LinearElement::Plane { .. } => "plane",
        }
    }
}

/// Distance measurand applied to the reference segment (`REQ-002`, Design).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DistanceMeasurand {
    /// 3D Euclidean length of the reference segment.
    ThreeD,
    /// Euclidean length of the reference segment projected orthogonally onto the plane with normal.
    Projected { plane_normal: [f64; 3] },
    /// Signed component of the reference segment along the unit axis direction of the named frame.
    Component { axis_unit: [f64; 3] },
}

/// Directed reference segment from element A's reference point to element B's reference point (metres).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReferenceSegment {
    pub start: [f64; 3],
    pub end: [f64; 3],
}

/// Diagnostic flag attached to a distance result (`REQ-004`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DistanceFlag {
    /// Plane normals differ by more than `1e-6` rad (`REQ-004`, `AC-5`).
    NonParallelPlanes { angle_rad: f64 },
}

/// Result of [`evaluate_distance`].
#[derive(Debug, Clone, PartialEq)]
pub struct DistanceValue {
    pub value_si: f64,
    pub value_authored: f64,
    pub authored_unit: LengthUnit,
    /// Exact decimal conversion from SI metres through `fmt.length@1` (CONSTITUTION §4).
    pub reported: Reported,
    pub reference_segment: ReferenceSegment,
    pub flags: Vec<DistanceFlag>,
    /// `true` iff both inputs were raw measured points (`evaluation_basis = POINTS`, else `FEATURES`).
    pub from_raw_points: bool,
}

impl DistanceValue {
    pub const fn algorithm(&self) -> &'static str {
        DISTANCE_ALGORITHM_ID
    }
}

/// Result of [`evaluate_caliper`].
#[derive(Debug, Clone, PartialEq)]
pub struct CaliperValue {
    pub value_si: f64,
    pub value_authored: f64,
    pub authored_unit: LengthUnit,
    /// Exact decimal conversion from SI metres through `fmt.length@1` (CONSTITUTION §4).
    pub reported: Reported,
    pub min_index: u64,
    pub max_index: u64,
    pub min_proj_si: f64,
    pub max_proj_si: f64,
}

impl CaliperValue {
    pub const fn algorithm(&self) -> &'static str {
        CALIPER_ALGORITHM_ID
    }
}

/// Errors returned by [`evaluate_distance`] and [`evaluate_caliper`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinearError {
    UnsupportedPair { a: &'static str, b: &'static str },
    DegenerateElement { detail: &'static str },
    EmptyPointSet,
    NanInput,
}

impl LinearError {
    pub const fn code(&self) -> &'static str {
        match self {
            LinearError::UnsupportedPair { .. } => "unsupported_pair",
            LinearError::DegenerateElement { .. } => "degenerate_element",
            LinearError::EmptyPointSet => "empty_point_set",
            LinearError::NanInput => "nan_input",
        }
    }
}

impl fmt::Display for LinearError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LinearError::UnsupportedPair { a, b } => write!(f, "unsupported_pair: {a}-{b}"),
            LinearError::DegenerateElement { detail } => write!(f, "degenerate_element: {detail}"),
            LinearError::EmptyPointSet => write!(f, "empty_point_set"),
            LinearError::NanInput => write!(f, "nan_input"),
        }
    }
}

impl std::error::Error for LinearError {}

/// Evaluates distance between elements `a` and `b` under `measurand` and converts to `unit`
/// (`REQ-001`..`REQ-006`).
pub fn evaluate_distance(
    a: LinearElement,
    b: LinearElement,
    measurand: DistanceMeasurand,
    unit: LengthUnit,
) -> Result<DistanceValue, LinearError> {
    let mut flags = Vec::new();
    let (seg, from_raw_points) = match (a, b) {
        (
            LinearElement::Point { position: p1, raw_point: r1 },
            LinearElement::Point { position: p2, raw_point: r2 },
        ) => {
            check_vec3(p1)?;
            check_vec3(p2)?;
            (ReferenceSegment { start: p1, end: p2 }, r1 && r2)
        }
        (LinearElement::Point { position: p, .. }, LinearElement::Line { point: lp, direction: ld }) => {
            check_vec3(p)?;
            check_vec3(lp)?;
            let u = unit_vec(ld, "line direction is degenerate")?;
            let q = foot_on_line(p, lp, u);
            (ReferenceSegment { start: p, end: q }, false)
        }
        (LinearElement::Line { point: lp, direction: ld }, LinearElement::Point { position: p, .. }) => {
            check_vec3(p)?;
            check_vec3(lp)?;
            let u = unit_vec(ld, "line direction is degenerate")?;
            let q = foot_on_line(p, lp, u);
            (ReferenceSegment { start: q, end: p }, false)
        }
        (LinearElement::Point { position: p, .. }, LinearElement::Plane { centroid: pc, normal: pn }) => {
            check_vec3(p)?;
            check_vec3(pc)?;
            let n = unit_vec(pn, "plane normal is degenerate")?;
            let q = foot_on_plane(p, pc, n);
            (ReferenceSegment { start: p, end: q }, false)
        }
        (LinearElement::Plane { centroid: pc, normal: pn }, LinearElement::Point { position: p, .. }) => {
            check_vec3(p)?;
            check_vec3(pc)?;
            let n = unit_vec(pn, "plane normal is degenerate")?;
            let q = foot_on_plane(p, pc, n);
            (ReferenceSegment { start: q, end: p }, false)
        }
        (LinearElement::Line { point: p1, direction: d1 }, LinearElement::Line { point: p2, direction: d2 }) => {
            check_vec3(p1)?;
            check_vec3(p2)?;
            let u1 = unit_vec(d1, "first line direction is degenerate")?;
            let u2 = unit_vec(d2, "second line direction is degenerate")?;
            let (q1, q2) = line_line_closest(p1, u1, p2, u2);
            (ReferenceSegment { start: q1, end: q2 }, false)
        }
        (
            LinearElement::Plane { centroid: c1, normal: n1_raw },
            LinearElement::Plane { centroid: c2, normal: n2_raw },
        ) => {
            check_vec3(c1)?;
            check_vec3(c2)?;
            let n1 = unit_vec(n1_raw, "first plane normal is degenerate")?;
            let n2 = unit_vec(n2_raw, "second plane normal is degenerate")?;
            let angle = unoriented_angle(n1, n2)?;
            if angle.total_cmp(&NON_PARALLEL_THRESHOLD_RAD) == Ordering::Greater {
                flags.push(DistanceFlag::NonParallelPlanes { angle_rad: angle });
            }
            let q2 = foot_on_plane(c1, c2, n2);
            (ReferenceSegment { start: c1, end: q2 }, false)
        }
        (x, y) => return Err(LinearError::UnsupportedPair { a: x.kind_str(), b: y.kind_str() }),
    };

    check_vec3(seg.start)?;
    check_vec3(seg.end)?;
    let v = sub(seg.end, seg.start);
    let value_si = match measurand {
        DistanceMeasurand::ThreeD => norm3(v)?,
        DistanceMeasurand::Projected { plane_normal } => {
            let np = unit_vec(plane_normal, "projection plane normal is degenerate")?;
            let vp = sub(v, scale(np, dot(v, np)));
            norm3(vp)?
        }
        DistanceMeasurand::Component { axis_unit } => {
            let ax = unit_vec(axis_unit, "component axis is degenerate")?;
            dot(v, ax)
        }
    };
    if !value_si.is_finite() {
        return Err(LinearError::NanInput);
    }
    let value_si = canonical_zero(value_si);
    let value_authored = unit.from_si(value_si);
    let reported = unit.format_si(value_si, DEFAULT_AUTHORED_DECIMALS)?;
    Ok(DistanceValue {
        value_si,
        value_authored,
        authored_unit: unit,
        reported,
        reference_segment: seg,
        flags,
        from_raw_points,
    })
}

/// Evaluates caliper extent `max(p·d) - min(p·d)` along unit direction `direction`, returning
/// the lowest point indices attaining the minimum and maximum on ties (`REQ-003`, `AC-4`).
pub fn evaluate_caliper(
    points: &[[f64; 3]],
    direction: [f64; 3],
    unit: LengthUnit,
) -> Result<CaliperValue, LinearError> {
    if points.is_empty() {
        return Err(LinearError::EmptyPointSet);
    }
    let d = unit_vec(direction, "caliper direction is degenerate")?;
    check_vec3(points[0])?;
    let p0 = dot(points[0], d);
    if !p0.is_finite() {
        return Err(LinearError::NanInput);
    }
    let mut min_val = p0;
    let mut max_val = p0;
    let mut min_idx = 0u64;
    let mut max_idx = 0u64;

    for (i, &pt) in points.iter().enumerate().skip(1) {
        check_vec3(pt)?;
        let proj = dot(pt, d);
        if !proj.is_finite() {
            return Err(LinearError::NanInput);
        }
        // Strict inequality keeps the lowest index on ties (REQ-003, AC-4).
        if proj.total_cmp(&min_val) == Ordering::Less {
            min_val = proj;
            min_idx = u64::try_from(i).unwrap_or(u64::MAX);
        }
        if proj.total_cmp(&max_val) == Ordering::Greater {
            max_val = proj;
            max_idx = u64::try_from(i).unwrap_or(u64::MAX);
        }
    }

    let value_si = canonical_zero(max_val - min_val);
    if !value_si.is_finite() {
        return Err(LinearError::NanInput);
    }
    let value_authored = unit.from_si(value_si);
    let reported = unit.format_si(value_si, DEFAULT_AUTHORED_DECIMALS)?;
    Ok(CaliperValue {
        value_si,
        value_authored,
        authored_unit: unit,
        reported,
        min_index: min_idx,
        max_index: max_idx,
        min_proj_si: canonical_zero(min_val),
        max_proj_si: canonical_zero(max_val),
    })
}

// ------------------------------------------------------------------ KNP-1 math helpers

fn check_vec3(v: [f64; 3]) -> Result<(), LinearError> {
    if v[0].is_finite() && v[1].is_finite() && v[2].is_finite() {
        Ok(())
    } else {
        Err(LinearError::NanInput)
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

fn norm3(v: [f64; 3]) -> Result<f64, LinearError> {
    check_vec3(v)?;
    let n2 = dot(v, v);
    if !n2.is_finite() {
        return Err(LinearError::NanInput);
    }
    numf::sqrt(n2).map_err(|_| LinearError::NanInput)
}

fn unit_vec(v: [f64; 3], detail: &'static str) -> Result<[f64; 3], LinearError> {
    let n = norm3(v)?;
    if n.total_cmp(&MIN_DIR_NORM) == Ordering::Less {
        return Err(LinearError::DegenerateElement { detail });
    }
    let inv = 1.0 / n;
    let out = scale(v, inv);
    check_vec3(out)?;
    Ok(out)
}

fn foot_on_line(p: [f64; 3], lp: [f64; 3], u: [f64; 3]) -> [f64; 3] {
    let t = dot(sub(p, lp), u);
    add(lp, scale(u, t))
}

fn foot_on_plane(p: [f64; 3], pc: [f64; 3], n: [f64; 3]) -> [f64; 3] {
    let h = dot(sub(p, pc), n);
    sub(p, scale(n, h))
}

fn line_line_closest(p1: [f64; 3], u1: [f64; 3], p2: [f64; 3], u2: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let w0 = sub(p1, p2);
    let b = dot(u1, u2);
    let d = dot(u1, w0);
    let e = dot(u2, w0);
    let denom = 1.0 - (b * b);
    if denom.total_cmp(&1e-24) != Ordering::Greater {
        // Parallel lines: perpendicular from p1 onto line 2.
        (p1, foot_on_line(p1, p2, u2))
    } else {
        let s = ((b * e) - d) / denom;
        let t = (e - (b * d)) / denom;
        (add(p1, scale(u1, s)), add(p2, scale(u2, t)))
    }
}

fn unoriented_angle(u: [f64; 3], v: [f64; 3]) -> Result<f64, LinearError> {
    let c = cross(u, v);
    let sin_a = norm3(c)?;
    let cos_a = dot(u, v).abs();
    numf::atan2(sin_a, cos_a).map_err(|_| LinearError::NanInput)
}

fn canonical_zero(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}
