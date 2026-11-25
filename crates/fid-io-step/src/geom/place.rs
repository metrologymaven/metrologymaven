//! Points, directions, vectors and placements (ISO 10303-42 `cartesian_point`, `direction`,
//! `vector`, `axis1_placement`, `axis2_placement_2d`, `axis2_placement_3d` [unverified clause
//! numbers]). Lengths are converted with one multiplication by the context factor; directions
//! are normalised and frames re-orthonormalised in a pinned order.

use super::read::{arg, arity, list, opt_reference, real, reference, simple};
use super::{Fail, Units, FRAME_DEVIATION_REPORT};
use crate::P21File;
use fid_geo_nurbs::Placement;

/// `(a0 b0 + a1 b1) + a2 b2`.
pub(crate) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    let s = a[0] * b[0] + a[1] * b[1];
    s + a[2] * b[2]
}

/// `a x b`, component by component.
pub(crate) fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// `a + s b`, component by component (no fused multiply-add).
pub(crate) fn axpy(a: [f64; 3], s: f64, b: [f64; 3]) -> [f64; 3] {
    [a[0] + s * b[0], a[1] + s * b[1], a[2] + s * b[2]]
}

/// `a - b`.
pub(crate) fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `s a`.
pub(crate) fn scale(s: f64, a: [f64; 3]) -> [f64; 3] {
    [s * a[0], s * a[1], s * a[2]]
}

/// `|a|` through the correctly rounded `fid_num` square root.
pub(crate) fn norm(a: [f64; 3]) -> Result<f64, Fail> {
    fid_num::f::sqrt(dot(a, a)).map_err(|_| Fail::NonFinite)
}

/// `a / |a|`; a zero or non-finite vector is `malformed` at `at`.
pub(crate) fn unit(a: [f64; 3], at: u64) -> Result<[f64; 3], Fail> {
    let n = norm(a)?;
    if n.is_nan() || n <= 0.0 || n.is_infinite() {
        return Err(Fail::Malformed { at, what: "zero_direction" });
    }
    let u = [a[0] / n, a[1] / n, a[2] / n];
    if u.iter().all(|c| c.is_finite()) {
        Ok(u)
    } else {
        Err(Fail::NonFinite)
    }
}

/// The coordinates of `cartesian_point` `n`: 1 to 3 reals, unconverted.
pub(crate) fn raw_point(file: &P21File, n: u64) -> Result<Vec<f64>, Fail> {
    let p = simple(file, n, "CARTESIAN_POINT")?;
    arity(p, 2, n)?;
    let c = list(arg(p, 1, n)?, n)?;
    if c.is_empty() || c.len() > 3 {
        return Err(Fail::Malformed { at: n, what: "point_dimension" });
    }
    c.iter().map(|x| real(x, n)).collect()
}

/// A 3D `cartesian_point` in metres (each coordinate times the length factor, once).
pub(crate) fn point3(file: &P21File, n: u64, units: Units) -> Result<[f64; 3], Fail> {
    match raw_point(file, n)?.as_slice() {
        [x, y, z] => Ok([*x * units.length, *y * units.length, *z * units.length]),
        _ => Err(Fail::Malformed { at: n, what: "point_dimension" }),
    }
}

/// A 2D `cartesian_point`, unconverted.
pub(crate) fn point2(file: &P21File, n: u64) -> Result<[f64; 2], Fail> {
    match raw_point(file, n)?.as_slice() {
        [x, y] => Ok([*x, *y]),
        _ => Err(Fail::Malformed { at: n, what: "point_dimension" }),
    }
}

/// The direction ratios of `direction` `n` (2 or 3 reals), unnormalised.
pub(crate) fn raw_direction(file: &P21File, n: u64) -> Result<Vec<f64>, Fail> {
    let p = simple(file, n, "DIRECTION")?;
    arity(p, 2, n)?;
    let c = list(arg(p, 1, n)?, n)?;
    if c.len() < 2 || c.len() > 3 {
        return Err(Fail::Malformed { at: n, what: "direction_dimension" });
    }
    c.iter().map(|x| real(x, n)).collect()
}

/// A 3D `direction`, normalised.
pub(crate) fn direction3(file: &P21File, n: u64) -> Result<[f64; 3], Fail> {
    match raw_direction(file, n)?.as_slice() {
        [x, y, z] => unit([*x, *y, *z], n),
        _ => Err(Fail::Malformed { at: n, what: "direction_dimension" }),
    }
}

/// A 2D `direction`, normalised.
pub(crate) fn direction2(file: &P21File, n: u64) -> Result<[f64; 2], Fail> {
    match raw_direction(file, n)?.as_slice() {
        [x, y] => {
            let u = unit([*x, *y, 0.0], n)?;
            Ok([u[0], u[1]])
        }
        _ => Err(Fail::Malformed { at: n, what: "direction_dimension" }),
    }
}

/// `vector` `n`: `(orientation entity, magnitude in file length units)`. The magnitude must
/// be finite and strictly positive.
pub(crate) fn vector(file: &P21File, n: u64) -> Result<(u64, f64), Fail> {
    let p = simple(file, n, "VECTOR")?;
    arity(p, 3, n)?;
    let dir = reference(arg(p, 1, n)?, n)?;
    let mag = real(arg(p, 2, n)?, n)?;
    if mag > 0.0 {
        Ok((dir, mag))
    } else {
        Err(Fail::Malformed { at: n, what: "vector_magnitude" })
    }
}

/// A 3D `vector` in metres: unit orientation times (magnitude times the length factor).
pub(crate) fn vector3(file: &P21File, n: u64, units: Units) -> Result<[f64; 3], Fail> {
    let (dir, mag) = vector(file, n)?;
    let d = direction3(file, dir)?;
    Ok(scale(mag * units.length, d))
}

/// A frame with its re-orthonormalisation deviation.
pub(crate) struct Frame {
    pub placement: Placement,
    /// `|ref . axis|` of the normalised inputs: how far from orthogonal the file's reference
    /// direction was (0 for an orthogonal or defaulted one).
    pub deviation: f64,
}

/// The default reference direction of ISO 10303-42 `first_proj_axis` for a missing
/// `ref_direction` [unverified]: `(1, 0, 0)` unless the axis is `(+-1, 0, 0)`, then `(0, 1, 0)`.
fn default_ref(z: [f64; 3]) -> [f64; 3] {
    if z[1] == 0.0 && z[2] == 0.0 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    }
}

/// Fixed-order Gram-Schmidt: `Z = axis / |axis|`, `x' = v - (v . Z) Z`, `X = x' / |x'|`, where
/// `v` is the normalised reference direction. Returns `(Z, X, |v . Z|)`.
pub(crate) fn orthonormal(axis: [f64; 3], v: [f64; 3], at: u64) -> Result<([f64; 3], [f64; 3], f64), Fail> {
    let z = unit(axis, at)?;
    let v = unit(v, at)?;
    let p = dot(v, z);
    let xp = axpy(v, -p, z);
    let n = norm(xp)?;
    if n.is_nan() || n <= 0.0 {
        return Err(Fail::Malformed { at, what: "parallel_ref_direction" });
    }
    let x = [xp[0] / n, xp[1] / n, xp[2] / n];
    Ok((z, x, p.abs()))
}

/// `axis2_placement_3d` `n` in metres.
pub(crate) fn axis2_3d(file: &P21File, n: u64, units: Units) -> Result<Frame, Fail> {
    let p = simple(file, n, "AXIS2_PLACEMENT_3D")?;
    arity(p, 4, n)?;
    let origin = point3(file, reference(arg(p, 1, n)?, n)?, units)?;
    let z = match opt_reference(arg(p, 2, n)?, n)? {
        Some(d) => direction3(file, d)?,
        None => [0.0, 0.0, 1.0],
    };
    let v = match opt_reference(arg(p, 3, n)?, n)? {
        Some(d) => direction3(file, d)?,
        None => default_ref(z),
    };
    let (z, x, deviation) = orthonormal(z, v, n)?;
    let placement = Placement::new(origin, z, x).map_err(|e| Fail::Geo { at: n, err: e })?;
    Ok(Frame { placement, deviation })
}

/// `axis1_placement` `n` in metres: `(location, unit axis)`; a missing axis is `(0, 0, 1)`.
pub(crate) fn axis1(file: &P21File, n: u64, units: Units) -> Result<([f64; 3], [f64; 3]), Fail> {
    let p = simple(file, n, "AXIS1_PLACEMENT")?;
    arity(p, 3, n)?;
    let origin = point3(file, reference(arg(p, 1, n)?, n)?, units)?;
    let z = match opt_reference(arg(p, 2, n)?, n)? {
        Some(d) => direction3(file, d)?,
        None => [0.0, 0.0, 1.0],
    };
    Ok((origin, z))
}

/// `axis2_placement_2d` `n`, unconverted: `(location, unit reference direction)`; a missing
/// reference direction is `(1, 0)`.
pub(crate) fn axis2_2d(file: &P21File, n: u64) -> Result<([f64; 2], [f64; 2]), Fail> {
    let p = simple(file, n, "AXIS2_PLACEMENT_2D")?;
    arity(p, 3, n)?;
    let origin = point2(file, reference(arg(p, 1, n)?, n)?)?;
    let x = match opt_reference(arg(p, 2, n)?, n)? {
        Some(d) => direction2(file, d)?,
        None => [1.0, 0.0],
    };
    Ok((origin, x))
}

/// A placement whose reference direction is `x` (unit), for a line: the axis is the unit
/// vector `x x e` normalised, with `e` the coordinate axis of the smallest `|x_i|` (first on
/// ties), so the choice is fixed.
pub(crate) fn line_frame(origin: [f64; 3], x: [f64; 3], at: u64) -> Result<Placement, Fail> {
    let a = [x[0].abs(), x[1].abs(), x[2].abs()];
    let e = if a[0] <= a[1] && a[0] <= a[2] {
        [1.0, 0.0, 0.0]
    } else if a[1] <= a[2] {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let z = unit(cross(x, e), at)?;
    Placement::new(origin, z, x).map_err(|err| Fail::Geo { at, err })
}

/// True when a frame deviation is large enough to report.
pub(crate) fn report_deviation(deviation: f64) -> bool {
    deviation > FRAME_DEVIATION_REPORT
}
