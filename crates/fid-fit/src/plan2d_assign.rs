//! Deterministic segment assignment for compound planar profiles
//! (spec node `KRN.LSQ.PLAN2D.ASSIGN`, KNP-1, D0).
//!
//! Provides the planar profile model (`Profile2` of ordered `Seg2::Line` and `Seg2::Arc`
//! segments), 3D-to-plane projection (`PlaneFrame`), signed in-plane point-to-segment
//! distance (outside positive for counter-clockwise profiles), nearest-segment assignment
//! with lower-index tie-breaking (`assign`), per-segment minimum-points verification
//! (`check_min_points`), and fixed-order per-segment residual statistics (`SegmentStats`).
//!
//! Pure Rust, `f64`, single-threaded, no I/O, no `unsafe`, no panics, no FMA, no
//! `f64::min`/`max`/`clamp`, no `HashMap`/`HashSet`; all transcendentals and reductions go
//! through `fid_num::f` and `fid_num::red` so outputs are bit-identical on x86-64, ARM64
//! and wasm32.

use core::f64::consts::PI;
use core::fmt;
use fid_kci::kci::json::{to_canonical, Json};
use fid_kci::kci::sha256_digest;
use fid_num::{check_finite, f, red, NumError};
use fid_types::Digest;

const OP: &str = "fit.plan2d_assign";
const TWO_PI: f64 = 2.0 * PI;
const ORTHONORMAL_TOL: f64 = 1.0e-9;

// ------------------------------------------------------------------ errors

/// Typed errors of planar segment assignment.
#[derive(Debug, Clone, PartialEq)]
pub enum AssignError {
    /// Segment `segment` received fewer points than its required minimum
    /// (REQ-KRN.LSQ.PLAN2D.ASSIGN-002, AC-2).
    UnderSampled { segment: usize, required: usize, actual: usize },
    /// Malformed profile or plane frame.
    InvalidProfile { segment: Option<usize>, reason: &'static str },
    /// Numeric error from `fid-num` (NaN, infinity, domain error).
    Num(NumError),
}

impl AssignError {
    /// Stable error code.
    pub const fn code(&self) -> &'static str {
        match self {
            AssignError::UnderSampled { .. } => "under_sampled",
            AssignError::InvalidProfile { .. } => "invalid_profile",
            AssignError::Num(_) => "num_error",
        }
    }
}

impl fmt::Display for AssignError {
    fn fmt(&self, f_out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssignError::UnderSampled { segment, required, actual } => {
                write!(f_out, "UnderSampled{{segment: {segment}, required: {required}, actual: {actual}}}")
            }
            AssignError::InvalidProfile { segment: Some(seg), reason } => {
                write!(f_out, "InvalidProfile{{segment: {seg}, reason: {reason}}}")
            }
            AssignError::InvalidProfile { segment: None, reason } => {
                write!(f_out, "InvalidProfile{{reason: {reason}}}")
            }
            AssignError::Num(e) => write!(f_out, "Num({e})"),
        }
    }
}

impl std::error::Error for AssignError {}

impl From<NumError> for AssignError {
    fn from(e: NumError) -> Self {
        AssignError::Num(e)
    }
}

// ------------------------------------------------------------------ plane frame projection

/// Right-handed orthonormal plane frame `(origin, u, v, n)` for projecting 3D points into
/// 2D plane coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaneFrame {
    pub origin: [f64; 3],
    pub u: [f64; 3],
    pub v: [f64; 3],
    pub n: [f64; 3],
}

impl PlaneFrame {
    /// Builds a validated orthonormal plane frame from `origin` and orthonormal in-plane
    /// axes `u`, `v`, setting `n = u × v`.
    pub fn new(origin: [f64; 3], u: [f64; 3], v: [f64; 3]) -> Result<Self, AssignError> {
        check_vec3("origin", origin)?;
        check_vec3("u", u)?;
        check_vec3("v", v)?;
        let nu = red::norm2(&u)?;
        let nv = red::norm2(&v)?;
        let uv = red::dot(&u, &v)?;
        if (nu - 1.0).abs() > ORTHONORMAL_TOL || (nv - 1.0).abs() > ORTHONORMAL_TOL || uv.abs() > ORTHONORMAL_TOL {
            return Err(AssignError::InvalidProfile { segment: None, reason: "frame_not_orthonormal" });
        }
        let n = [
            check_finite(OP, u[1] * v[2] - u[2] * v[1])?,
            check_finite(OP, u[2] * v[0] - u[0] * v[2])?,
            check_finite(OP, u[0] * v[1] - u[1] * v[0])?,
        ];
        Ok(Self { origin, u, v, n })
    }

    /// Projects one 3D point into `(x, y)` coordinates in this plane frame.
    pub fn project_point(&self, p: [f64; 3]) -> Result<[f64; 2], AssignError> {
        check_vec3("point", p)?;
        let d = [
            check_finite(OP, p[0] - self.origin[0])?,
            check_finite(OP, p[1] - self.origin[1])?,
            check_finite(OP, p[2] - self.origin[2])?,
        ];
        let x = check_finite(OP, red::dot(&d, &self.u)?)?;
        let y = check_finite(OP, red::dot(&d, &self.v)?)?;
        Ok([x, y])
    }

    /// Projects a slice of 3D points into 2D plane coordinates in input order.
    pub fn project_points(&self, points: &[[f64; 3]]) -> Result<Vec<[f64; 2]>, AssignError> {
        let mut out = Vec::with_capacity(points.len());
        for &p in points {
            out.push(self.project_point(p)?);
        }
        Ok(out)
    }
}

// ------------------------------------------------------------------ planar profile & segments

/// One segment of a compound planar profile (line or circular arc).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg2 {
    /// Directed straight segment from `a` to `b`.
    ///
    /// For a counter-clockwise boundary, the outward normal is the right-hand unit normal
    /// `((b.y - a.y) / L, -(b.x - a.x) / L)`.
    Line { a: [f64; 2], b: [f64; 2] },
    /// Circular arc centered at `c` with radius `r > 0`, starting angle `start` (radians),
    /// and signed angular sweep `sweep` (radians, `0 < |sweep| <= 2π`).
    ///
    /// Outside the circle (`|p - c| > r`) is positive; inside (`|p - c| < r`) is negative.
    Arc { c: [f64; 2], r: f64, start: f64, sweep: f64 },
}

impl Seg2 {
    /// Validates segment geometry (finite coordinates, non-degenerate length/radius/sweep).
    pub fn validate(&self, index: usize) -> Result<(), AssignError> {
        match *self {
            Seg2::Line { a, b } => {
                check_vec2(a)?;
                check_vec2(b)?;
                let dx = check_finite(OP, b[0] - a[0])?;
                let dy = check_finite(OP, b[1] - a[1])?;
                let len = f::hypot(dx, dy)?;
                if len <= 0.0 {
                    return Err(AssignError::InvalidProfile { segment: Some(index), reason: "zero_length_line" });
                }
                Ok(())
            }
            Seg2::Arc { c, r, start, sweep } => {
                check_vec2(c)?;
                let r = check_finite(OP, r)?;
                let _ = check_finite(OP, start)?;
                let sweep = check_finite(OP, sweep)?;
                if r <= 0.0 {
                    return Err(AssignError::InvalidProfile { segment: Some(index), reason: "non_positive_radius" });
                }
                if sweep == 0.0 || sweep.abs() > TWO_PI + 1.0e-12 {
                    return Err(AssignError::InvalidProfile { segment: Some(index), reason: "invalid_arc_sweep" });
                }
                Ok(())
            }
        }
    }

    /// Default minimum number of points required for this segment (REQ-002):
    /// 2 for a line; 3 for an arc (or 2 when `fixed_arc_radius` is `true`).
    pub const fn default_min_points(&self, fixed_arc_radius: bool) -> usize {
        match self {
            Seg2::Line { .. } => 2,
            Seg2::Arc { .. } => {
                if fixed_arc_radius {
                    2
                } else {
                    3
                }
            }
        }
    }

    /// Signed in-plane distance from point `p` to this segment (outside positive).
    pub fn signed_distance(&self, p: [f64; 2]) -> Result<f64, AssignError> {
        check_vec2(p)?;
        match *self {
            Seg2::Line { a, b } => {
                let dx = check_finite(OP, b[0] - a[0])?;
                let dy = check_finite(OP, b[1] - a[1])?;
                let len = f::hypot(dx, dy)?;
                if len <= 0.0 {
                    return Err(AssignError::InvalidProfile { segment: None, reason: "zero_length_line" });
                }
                let tx = check_finite(OP, dx / len)?;
                let ty = check_finite(OP, dy / len)?;
                let apx = check_finite(OP, p[0] - a[0])?;
                let apy = check_finite(OP, p[1] - a[1])?;
                let proj = check_finite(OP, apx * tx + apy * ty)?;
                let s = f::total_clamp(proj, 0.0, len)?;
                let qx = check_finite(OP, a[0] + s * tx)?;
                let qy = check_finite(OP, a[1] + s * ty)?;
                let dist = f::hypot(check_finite(OP, p[0] - qx)?, check_finite(OP, p[1] - qy)?)?;
                // Outward normal for CCW profile is (ty, -tx).
                let side = check_finite(OP, apx * ty - apy * tx)?;
                if dist == 0.0 {
                    Ok(0.0)
                } else if side < 0.0 {
                    Ok(-dist)
                } else {
                    Ok(dist)
                }
            }
            Seg2::Arc { c, r, start, sweep } => {
                let rx = check_finite(OP, p[0] - c[0])?;
                let ry = check_finite(OP, p[1] - c[1])?;
                let rho = f::hypot(rx, ry)?;
                let theta = f::atan2(ry, rx)?;
                let mid = check_finite(OP, start + 0.5 * sweep)?;
                let half_sweep = 0.5 * sweep.abs();
                let delta_mid = wrap_pi(check_finite(OP, theta - mid)?)?;
                if delta_mid.abs() <= half_sweep {
                    let signed = check_finite(OP, rho - r)?;
                    Ok(if signed == 0.0 { 0.0 } else { signed })
                } else {
                    // Nearer endpoint of the arc.
                    let phi = if (sweep > 0.0 && delta_mid < 0.0) || (sweep < 0.0 && delta_mid > 0.0) {
                        start
                    } else {
                        check_finite(OP, start + sweep)?
                    };
                    let ex = check_finite(OP, c[0] + r * f::cos(phi)?)?;
                    let ey = check_finite(OP, c[1] + r * f::sin(phi)?)?;
                    let dist = f::hypot(check_finite(OP, p[0] - ex)?, check_finite(OP, p[1] - ey)?)?;
                    if dist == 0.0 {
                        Ok(0.0)
                    } else if rho < r {
                        Ok(-dist)
                    } else {
                        Ok(dist)
                    }
                }
            }
        }
    }
}

/// Ordered planar profile composed of line and circular-arc segments.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile2 {
    pub segments: Vec<Seg2>,
}

impl Profile2 {
    /// Creates and validates a [`Profile2`].
    pub fn new(segments: Vec<Seg2>) -> Result<Self, AssignError> {
        let profile = Self { segments };
        profile.validate()?;
        Ok(profile)
    }

    /// Validates that the profile is non-empty and every segment is non-degenerate and finite.
    pub fn validate(&self) -> Result<(), AssignError> {
        if self.segments.is_empty() {
            return Err(AssignError::InvalidProfile { segment: None, reason: "empty_profile" });
        }
        for (idx, seg) in self.segments.iter().enumerate() {
            seg.validate(idx)?;
        }
        Ok(())
    }

    /// Default minimum point counts per segment (2 for lines, 3 for arcs unless `fixed_arc_radius`).
    pub fn default_min_points(&self, fixed_arc_radius: bool) -> Vec<usize> {
        self.segments.iter().map(|s| s.default_min_points(fixed_arc_radius)).collect()
    }
}

// ------------------------------------------------------------------ assignment & statistics

/// Fixed-order signed residual statistics for one profile segment (REQ-KRN.LSQ.PLAN2D.ASSIGN-003).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SegmentStats {
    pub segment: usize,
    pub count: usize,
    pub rms: Option<f64>,
    pub max: Option<f64>,
    pub min: Option<f64>,
}

/// Result of [`assign`]: segment index and signed residual per point, plus per-segment statistics.
#[derive(Debug, Clone, PartialEq)]
pub struct Assignment {
    /// Assigned segment index for each input point, in input order.
    pub segment_of: Vec<usize>,
    /// Signed in-plane residual (outside positive) for each input point, in input order.
    pub residuals: Vec<f64>,
    /// Per-segment signed residual statistics in segment index order.
    pub stats: Vec<SegmentStats>,
}

impl Assignment {
    /// Canonical RFC 8785 JSON bytes with every f64 encoded as 16-hex-digit IEEE-754 bits
    /// for cross-target bit-identity verification (REQ-004, AC-3).
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, AssignError> {
        let segs: Vec<Json> = self.segment_of.iter().map(|i| Json::Str(i.to_string())).collect();
        let res_hex: Vec<Json> = self.residuals.iter().map(|r| Json::Str(hex_bits(*r))).collect();
        let stats_json: Vec<Json> = self
            .stats
            .iter()
            .map(|st| {
                let mut fields = Vec::with_capacity(5);
                fields.push(("count".to_owned(), Json::Str(st.count.to_string())));
                if let Some(mx) = st.max {
                    fields.push(("max_hex".to_owned(), Json::Str(hex_bits(mx))));
                }
                if let Some(mn) = st.min {
                    fields.push(("min_hex".to_owned(), Json::Str(hex_bits(mn))));
                }
                if let Some(rms) = st.rms {
                    fields.push(("rms_hex".to_owned(), Json::Str(hex_bits(rms))));
                }
                fields.push(("segment".to_owned(), Json::Str(st.segment.to_string())));
                Json::Obj(fields)
            })
            .collect();
        let root = Json::Obj(vec![
            ("residuals_hex".to_owned(), Json::Arr(res_hex)),
            ("segment_of".to_owned(), Json::Arr(segs)),
            ("stats".to_owned(), Json::Arr(stats_json)),
        ]);
        to_canonical(&root).map_err(|_| AssignError::InvalidProfile { segment: None, reason: "canonical_json" })
    }

    /// SHA-256 digest of [`Self::to_canonical_bytes`].
    pub fn digest(&self) -> Result<Digest, AssignError> {
        let bytes = self.to_canonical_bytes()?;
        Ok(sha256_digest(&bytes))
    }
}

/// Assigns each 2D point to the segment of `profile` with the smallest absolute distance,
/// breaking ties strictly by the lower segment index, and computes fixed-order per-segment
/// signed residual statistics via `fid_num::red` (REQ-KRN.LSQ.PLAN2D.ASSIGN-001, -003, -004).
pub fn assign(points: &[[f64; 2]], profile: &Profile2) -> Result<Assignment, AssignError> {
    profile.validate()?;
    let n_seg = profile.segments.len();
    let n_pts = points.len();

    let mut segment_of = Vec::with_capacity(n_pts);
    let mut residuals = Vec::with_capacity(n_pts);
    let mut per_seg: Vec<Vec<f64>> = vec![Vec::new(); n_seg];

    for &p in points {
        check_vec2(p)?;
        let mut best_seg = 0usize;
        let mut best_signed = profile.segments[0].signed_distance(p)?;
        let mut best_abs = best_signed.abs();

        for (seg_idx, seg) in profile.segments.iter().enumerate().skip(1) {
            let sd = seg.signed_distance(p)?;
            let ad = sd.abs();
            // Strict `<` ensures ties go to the lower segment index (REQ-001).
            if ad < best_abs {
                best_seg = seg_idx;
                best_signed = sd;
                best_abs = ad;
            }
        }
        segment_of.push(best_seg);
        residuals.push(best_signed);
        per_seg[best_seg].push(best_signed);
    }

    let mut stats = Vec::with_capacity(n_seg);
    for (seg_idx, seg_res) in per_seg.iter().enumerate() {
        let count = seg_res.len();
        if count == 0 {
            stats.push(SegmentStats { segment: seg_idx, count: 0, rms: None, max: None, min: None });
            continue;
        }
        let sqrt_n = f::sqrt(count as f64)?;
        let rms = check_finite(OP, red::norm2(seg_res)? / sqrt_n)?;
        let mut max_v = seg_res[0];
        let mut min_v = seg_res[0];
        for &r in seg_res.iter().skip(1) {
            max_v = f::total_max(max_v, r)?;
            min_v = f::total_min(min_v, r)?;
        }
        stats.push(SegmentStats { segment: seg_idx, count, rms: Some(rms), max: Some(max_v), min: Some(min_v) });
    }

    Ok(Assignment { segment_of, residuals, stats })
}

/// Checks that every segment received at least `min_per_segment[i]` points, returning
/// `AssignError::UnderSampled { segment, required, actual }` for the first segment in index
/// order that falls short (REQ-KRN.LSQ.PLAN2D.ASSIGN-002, AC-2).
pub fn check_min_points(assignment: &Assignment, min_per_segment: &[usize]) -> Result<(), AssignError> {
    if assignment.stats.len() != min_per_segment.len() {
        return Err(AssignError::InvalidProfile { segment: None, reason: "min_per_segment_length_mismatch" });
    }
    for (idx, (&req, st)) in min_per_segment.iter().zip(assignment.stats.iter()).enumerate() {
        if st.count < req {
            return Err(AssignError::UnderSampled { segment: idx, required: req, actual: st.count });
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ helpers

fn check_vec2(v: [f64; 2]) -> Result<(), AssignError> {
    let _ = check_finite(OP, v[0])?;
    let _ = check_finite(OP, v[1])?;
    Ok(())
}

fn check_vec3(label: &'static str, v: [f64; 3]) -> Result<(), AssignError> {
    let _ = check_finite(label, v[0])?;
    let _ = check_finite(label, v[1])?;
    let _ = check_finite(label, v[2])?;
    Ok(())
}

/// Wraps angle `a` into `(-PI, PI]` using pure IEEE-754 arithmetic (no libm `round`, no FMA).
fn wrap_pi(a: f64) -> Result<f64, AssignError> {
    let a = check_finite(OP, a)?;
    let q = check_finite(OP, a / TWO_PI)?;
    let mut r = if q.abs() < 4_503_599_627_370_496.0 {
        let k = if q >= 0.0 { ((q + 0.5) as i64) as f64 } else { ((q - 0.5) as i64) as f64 };
        check_finite(OP, a - k * TWO_PI)?
    } else {
        a
    };
    for _ in 0..8 {
        if r <= -PI {
            r = check_finite(OP, r + TWO_PI)?;
        } else if r > PI {
            r = check_finite(OP, r - TWO_PI)?;
        } else {
            break;
        }
    }
    Ok(r)
}

fn hex_bits(x: f64) -> String {
    format!("0x{:016x}", x.to_bits())
}
