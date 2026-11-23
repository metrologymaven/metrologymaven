//! Kernel evaluation of dimensional angles between two directed elements or an element and a
//! frame axis, with optional projection onto a named plane (spec node `GDT.DIM.ANGLE`,
//! `REQ-001`..`REQ-005`).
//!
//! Pure KNP-1 arithmetic: no FMA, transcendentals strictly via `fid_num::f::{sqrt, atan2,
//! total_min, total_max}`, fixed-order reductions, no `HashMap`/`HashSet`, no panics, no
//! `unsafe`. Output bits are deterministic across x86-64, ARM64 and `wasm32-wasip1` (D0).

use fid_num::err::NumError;
use fid_num::f as numf;
use std::fmt;

/// Minimum allowed Euclidean norm of an input or projected direction vector
/// (`REQ-GDT.DIM.ANGLE-004`).
pub const DEGENERATE_NORM_MIN: f64 = 1e-12;

/// Kernel algorithm identifier stamped onto angle ResultRecords.
pub const ANGLE_ALG_ID: &str = "eval.dim.angle@1.0.0";

/// Exact `180 / pi` factor in IEEE 754 binary64 for the single journaled radian-to-degree
/// conversion.
const RAD_TO_DEG: f64 = 180.0 / core::f64::consts::PI;

/// Angle folding mode for unprojected direction pairs (`REQ-GDT.DIM.ANGLE-002`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AngleMode {
    /// Fold `theta` to `min(theta, pi - theta)` in `[0, pi/2]`.
    Acute,
    /// Unfolded angle `theta` in `[0, pi]`.
    Full,
}

impl AngleMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            AngleMode::Acute => "acute",
            AngleMode::Full => "full",
        }
    }

    pub fn parse(s: &str) -> Option<AngleMode> {
        match s {
            "acute" => Some(AngleMode::Acute),
            "full" => Some(AngleMode::Full),
            _ => None,
        }
    }
}

/// Authored display/reporting unit for the single journaled unit conversion (`REQ-001`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AuthoredUnit {
    Rad,
    Deg,
}

impl AuthoredUnit {
    pub const fn as_str(self) -> &'static str {
        match self {
            AuthoredUnit::Rad => "rad",
            AuthoredUnit::Deg => "deg",
        }
    }

    pub fn parse(s: &str) -> Option<AuthoredUnit> {
        match s {
            "rad" | "si:rad" => Some(AuthoredUnit::Rad),
            "deg" => Some(AuthoredUnit::Deg),
            _ => None,
        }
    }
}

/// Named coordinate-frame axis usable as operand `b` (`GDT.DIM.ANGLE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FrameAxis {
    PosX,
    NegX,
    PosY,
    NegY,
    PosZ,
    NegZ,
}

impl FrameAxis {
    pub const fn as_str(self) -> &'static str {
        match self {
            FrameAxis::PosX => "+X",
            FrameAxis::NegX => "-X",
            FrameAxis::PosY => "+Y",
            FrameAxis::NegY => "-Y",
            FrameAxis::PosZ => "+Z",
            FrameAxis::NegZ => "-Z",
        }
    }

    pub fn parse(s: &str) -> Option<FrameAxis> {
        match s {
            "+X" | "X" | "+x" | "x" => Some(FrameAxis::PosX),
            "-X" | "-x" => Some(FrameAxis::NegX),
            "+Y" | "Y" | "+y" | "y" => Some(FrameAxis::PosY),
            "-Y" | "-y" => Some(FrameAxis::NegY),
            "+Z" | "Z" | "+z" | "z" => Some(FrameAxis::PosZ),
            "-Z" | "-z" => Some(FrameAxis::NegZ),
            _ => None,
        }
    }

    /// Exact unit direction vector of this frame axis.
    pub const fn unit_vec(self) -> [f64; 3] {
        match self {
            FrameAxis::PosX => [1.0, 0.0, 0.0],
            FrameAxis::NegX => [-1.0, 0.0, 0.0],
            FrameAxis::PosY => [0.0, 1.0, 0.0],
            FrameAxis::NegY => [0.0, -1.0, 0.0],
            FrameAxis::PosZ => [0.0, 0.0, 1.0],
            FrameAxis::NegZ => [0.0, 0.0, -1.0],
        }
    }
}

/// Element direction kind (directed elements provide a direction vector; non-directed
/// elements are rejected with `not_directed_element`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ElementDirectionKind {
    Line,
    Axis,
    PlaneNormal,
    ConeAxis,
    /// Non-directed element (point, sphere, etc.).
    Undirected,
}

impl ElementDirectionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            ElementDirectionKind::Line => "line",
            ElementDirectionKind::Axis => "axis",
            ElementDirectionKind::PlaneNormal => "plane_normal",
            ElementDirectionKind::ConeAxis => "cone_axis",
            ElementDirectionKind::Undirected => "undirected",
        }
    }

    pub const fn is_directed(self) -> bool {
        !matches!(self, ElementDirectionKind::Undirected)
    }
}

/// Errors of [`evaluate_angle`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AngleError {
    /// `degenerate_direction`: an input direction, plane normal, or projected direction has
    /// non-finite components or norm strictly below `1e-12` (`REQ-GDT.DIM.ANGLE-004`).
    DegenerateDirection { which: &'static str },
    /// `not_directed_element`: the referenced element has no direction vector (e.g. a point or
    /// sphere).
    NotDirectedElement { element: String },
}

impl AngleError {
    /// Stable machine-readable error code.
    pub const fn code(&self) -> &'static str {
        match self {
            AngleError::DegenerateDirection { .. } => "degenerate_direction",
            AngleError::NotDirectedElement { .. } => "not_directed_element",
        }
    }
}

impl fmt::Display for AngleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AngleError::DegenerateDirection { which } => write!(f, "degenerate_direction{{{which}}}"),
            AngleError::NotDirectedElement { element } => write!(f, "not_directed_element{{{element}}}"),
        }
    }
}

impl std::error::Error for AngleError {}

/// Inputs to [`evaluate_angle`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AngleInput {
    /// Direction vector of element `a`.
    pub dir_a: [f64; 3],
    /// Direction vector of element `b` or frame axis.
    pub dir_b: [f64; 3],
    /// Folding mode (`acute` or `full`, applied to unprojected angles).
    pub mode: AngleMode,
    /// Optional projection plane normal; when `Some`, both directions are projected onto the
    /// plane orthogonal to this normal and the signed angle in `(-pi, pi]` about the unit
    /// normal is returned (`REQ-003`).
    pub plane_normal: Option<[f64; 3]>,
    /// Authored reporting unit (`rad` or `deg`, single conversion from radians).
    pub authored_unit: AuthoredUnit,
}

/// Deterministic kernel result of [`evaluate_angle`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AngleValue {
    /// Internal angle in radians (`[0, pi/2]` in acute mode, `[0, pi]` in full mode, or
    /// `(-pi, pi]` when projected onto a plane). Never `-0.0` or subnormal.
    pub value_rad: f64,
    /// Value in the authored unit (`rad` or `deg`) via a single conversion.
    pub authored_value: f64,
    /// Authored unit.
    pub authored_unit: AuthoredUnit,
    /// Requested angle mode.
    pub mode: AngleMode,
    /// Whether projection onto a named plane was performed.
    pub projected: bool,
}

impl AngleValue {
    /// Kernel algorithm ID (`eval.dim.angle@1.0.0`).
    pub const fn algorithm(&self) -> &'static str {
        ANGLE_ALG_ID
    }
}

/// Evaluates the angle between `input.dir_a` and `input.dir_b` (`REQ-GDT.DIM.ANGLE-001`..`005`).
pub fn evaluate_angle(input: &AngleInput) -> Result<AngleValue, AngleError> {
    let ua = unit_vec(input.dir_a, "a")?;
    let ub = unit_vec(input.dir_b, "b")?;

    let (value_rad, projected) = match input.plane_normal {
        Some(raw_n) => {
            let n = unit_vec(raw_n, "plane")?;
            let pa = project_onto_plane(ua, n);
            let pb = project_onto_plane(ub, n);
            let qa = unit_vec(pa, "a")?;
            let qb = unit_vec(pb, "b")?;
            let c = cross(qa, qb);
            let sin_t = dot(c, n);
            let cos_t = dot(qa, qb);
            let mut theta = map_num(numf::atan2(sin_t, cos_t), "projected")?;
            if theta <= -core::f64::consts::PI {
                theta = core::f64::consts::PI;
            }
            (canonical_zero(theta), true)
        }
        None => {
            let c = cross(ua, ub);
            let c_norm = map_num(numf::sqrt(dot(c, c)), "cross")?;
            let d = dot(ua, ub);
            let raw = map_num(numf::atan2(c_norm, d), "angle")?;
            let folded = match input.mode {
                AngleMode::Full => raw,
                AngleMode::Acute => {
                    let comp = core::f64::consts::PI - raw;
                    let comp_clamped = map_num(numf::total_max(0.0, comp), "acute")?;
                    map_num(numf::total_min(raw, comp_clamped), "acute")?
                }
            };
            (canonical_zero(folded), false)
        }
    };

    let authored_value = match input.authored_unit {
        AuthoredUnit::Rad => value_rad,
        AuthoredUnit::Deg => canonical_zero(value_rad * RAD_TO_DEG),
    };

    Ok(AngleValue { value_rad, authored_value, authored_unit: input.authored_unit, mode: input.mode, projected })
}

fn map_num(r: Result<f64, NumError>, which: &'static str) -> Result<f64, AngleError> {
    match r {
        Ok(v) if v.is_finite() => Ok(v),
        _ => Err(AngleError::DegenerateDirection { which }),
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    // Fixed-order reduction (KNP-1): never reordered, no FMA.
    ((a[0] * b[0]) + (a[1] * b[1])) + (a[2] * b[2])
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [(a[1] * b[2]) - (a[2] * b[1]), (a[2] * b[0]) - (a[0] * b[2]), (a[0] * b[1]) - (a[1] * b[0])]
}

fn project_onto_plane(v: [f64; 3], n: [f64; 3]) -> [f64; 3] {
    let d = dot(v, n);
    [v[0] - (d * n[0]), v[1] - (d * n[1]), v[2] - (d * n[2])]
}

fn unit_vec(v: [f64; 3], which: &'static str) -> Result<[f64; 3], AngleError> {
    if !v[0].is_finite() || !v[1].is_finite() || !v[2].is_finite() {
        return Err(AngleError::DegenerateDirection { which });
    }
    let norm2 = dot(v, v);
    if !norm2.is_finite() {
        return Err(AngleError::DegenerateDirection { which });
    }
    let norm = map_num(numf::sqrt(norm2), which)?;
    if norm < DEGENERATE_NORM_MIN {
        return Err(AngleError::DegenerateDirection { which });
    }
    let inv = 1.0 / norm;
    let out = [v[0] * inv, v[1] * inv, v[2] * inv];
    if !out[0].is_finite() || !out[1].is_finite() || !out[2].is_finite() {
        return Err(AngleError::DegenerateDirection { which });
    }
    Ok(out)
}

fn canonical_zero(v: f64) -> f64 {
    if v == 0.0 || v.abs() < f64::MIN_POSITIVE {
        0.0
    } else {
        v
    }
}
