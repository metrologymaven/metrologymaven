//! Single-precision export rounding report (spec node `IO.MESH.MBLD`, REQ-IO.MESH.MBLD-004).

use fid_geo_types::{Mesh, PointBuffer, PointSet};

/// Maximum absolute coordinate error introduced by rounding `f64` vertex coordinates to
/// IEEE-754 binary32 (`f32`), computed in `f64` and expressed in metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrecisionReport {
    /// Maximum `|f64::from(c as f32) - c|` across all vertex coordinates `(x, y, z)`, in metres.
    /// Zero for an empty mesh or when every coordinate is exactly representable in `f32`.
    pub max_abs_m: f64,
}

/// Computes the single-precision rounding report for a [`Mesh`] (REQ-IO.MESH.MBLD-004).
pub fn f32_rounding_report(mesh: &Mesh) -> PrecisionReport {
    f32_rounding_report_buffer(mesh.vertices())
}

/// Computes the single-precision rounding report for a [`PointSet`].
pub fn f32_rounding_report_point_set(points: &PointSet) -> PrecisionReport {
    f32_rounding_report_buffer(points.buffer())
}

/// Computes the single-precision rounding report for a [`PointBuffer`] in metres.
///
/// Traverses coordinates in fixed vertex-then-axis order `(x, y, z)` so the outcome is
/// `D0` deterministic on every platform.
pub fn f32_rounding_report_buffer(buf: &PointBuffer) -> PrecisionReport {
    let mut max_abs = 0.0_f64;
    for ((&x, &y), &z) in buf.x.iter().zip(&buf.y).zip(&buf.z) {
        for c in [x, y, z] {
            let rounded = f64::from(c as f32);
            let err = (rounded - c).abs();
            if err > max_abs {
                max_abs = err;
            }
        }
    }
    PrecisionReport { max_abs_m: max_abs }
}
