//! Curves: `line`, `circle`, `ellipse`, `b_spline_curve_with_knots` (simple and rational complex
//! form), `trimmed_curve`, `polyline`, `surface_curve`, `seam_curve`, and the 2D curves of a
//! `pcurve`; plus the conversion of a swept curve to an exact NURBS curve.

use super::place::{axis2_2d, axis2_3d, direction2, dot, line_frame, point2, point3, sub, vector, vector3};
use super::read::{arg, arity, boolean, counts, enumeration, get, list, part, real, reals, reference, references};
use super::{Curve2, CurveRef, Fail, Kind, Trim, TrimMaster, TrimmedCurve, Units};
use crate::{Instance, P21File, Param};
use fid_geo_nurbs::{Circle, Dir, Ellipse, Line, NurbsCurve, NurbsCurveParts, NurbsError, MAX_DEGREE};

use core::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, PI, TAU};

/// The parameters of a B-spline curve gathered from either instance form.
struct BsplineCurveData<'a> {
    degree: &'a Param,
    ctrl: &'a Param,
    mults: &'a Param,
    knots: &'a Param,
    weights: Option<&'a Param>,
}

fn bspline_curve_data(inst: &Instance) -> Result<BsplineCurveData<'_>, Fail> {
    let n = inst.number;
    if let [(name, p)] = inst.parts.as_slice() {
        if name.as_str() != "B_SPLINE_CURVE_WITH_KNOTS" {
            return Err(Fail::Malformed { at: n, what: "unexpected_entity_type" });
        }
        arity(p, 9, n)?;
        return Ok(BsplineCurveData {
            degree: arg(p, 1, n)?,
            ctrl: arg(p, 2, n)?,
            mults: arg(p, 6, n)?,
            knots: arg(p, 7, n)?,
            weights: None,
        });
    }
    let base = part(inst, "B_SPLINE_CURVE")?.ok_or(Fail::Malformed { at: n, what: "missing_part" })?;
    let kn = part(inst, "B_SPLINE_CURVE_WITH_KNOTS")?.ok_or(Fail::Malformed { at: n, what: "missing_part" })?;
    arity(base, 5, n)?;
    arity(kn, 3, n)?;
    let weights = match part(inst, "RATIONAL_B_SPLINE_CURVE")? {
        Some(w) => {
            arity(w, 1, n)?;
            Some(arg(w, 0, n)?)
        }
        None => None,
    };
    Ok(BsplineCurveData {
        degree: arg(base, 0, n)?,
        ctrl: arg(base, 1, n)?,
        mults: arg(kn, 0, n)?,
        knots: arg(kn, 1, n)?,
        weights,
    })
}

/// The flat knot vector from distinct knots and multiplicities. The multiplicity sum is
/// checked against `ctrl + degree + 1` before anything is allocated, so a huge multiplicity
/// cannot allocate; a mismatch is the GEO `CountMismatch` error.
pub(crate) fn expand_knots(
    knots: &[f64],
    mults: &[usize],
    n_ctrl: usize,
    degree: usize,
    dir: Dir,
    at: u64,
) -> Result<Vec<f64>, Fail> {
    if knots.len() != mults.len() {
        return Err(Fail::Malformed { at, what: "knot_multiplicity_count" });
    }
    let mut total = 0usize;
    for &m in mults {
        total = total.checked_add(m).ok_or(Fail::Malformed { at, what: "integer_range" })?;
    }
    let want = n_ctrl.checked_add(degree).and_then(|x| x.checked_add(1));
    if Some(total) != want {
        return Err(Fail::Nurbs { at, err: NurbsError::CountMismatch { dir, knots: total, ctrl: n_ctrl, degree } });
    }
    let mut out = Vec::with_capacity(total);
    for (&k, &m) in knots.iter().zip(mults) {
        for _ in 0..m {
            out.push(k);
        }
    }
    Ok(out)
}

/// A degree in `1..=MAX_DEGREE` as GEO checks it; a negative integer is `malformed`.
pub(crate) fn degree(p: &Param, at: u64) -> Result<usize, Fail> {
    super::read::count(p, at)
}

/// Point coordinates of a control-point reference, as a 3-vector (2D points get `z = 0` and
/// their two coordinates scaled by `s2`; 3D points are converted to metres).
#[derive(Clone, Copy)]
pub(crate) enum Space {
    Model(Units),
    Param([f64; 2]),
}

fn ctrl_point(file: &P21File, r: u64, space: Space) -> Result<[f64; 3], Fail> {
    match space {
        Space::Model(u) => point3(file, r, u),
        Space::Param(s) => {
            let p = point2(file, r)?;
            Ok([p[0] * s[0], p[1] * s[1], 0.0])
        }
    }
}

/// A B-spline curve (simple or rational complex form), validated by GEO.
pub(crate) fn bspline_curve(file: &P21File, inst: &Instance, space: Space) -> Result<NurbsCurve, Fail> {
    let n = inst.number;
    let d = bspline_curve_data(inst)?;
    let degree = degree(d.degree, n)?;
    if degree == 0 || degree > MAX_DEGREE {
        return Err(Fail::Nurbs { at: n, err: NurbsError::BadDegree { dir: Dir::U, degree } });
    }
    let refs = references(d.ctrl, n)?;
    let mut ctrl = Vec::with_capacity(refs.len());
    for r in refs {
        ctrl.push(ctrl_point(file, r, space)?);
    }
    let knots = expand_knots(&reals(d.knots, n)?, &counts(d.mults, n)?, ctrl.len(), degree, Dir::U, n)?;
    let weights = match d.weights {
        Some(w) => Some(reals(w, n)?),
        None => None,
    };
    NurbsCurve::new(NurbsCurveParts { degree, knots, ctrl, weights, periodic: false })
        .map_err(|err| Fail::Nurbs { at: n, err })
}

/// A `polyline` as a degree-1 B-spline with knots `0, 0, 1, ..., m - 2, m - 1, m - 1` for `m`
/// points (parameter range `[0, m - 1]` [unverified: ISO 10303-42 polyline parameterisation]).
pub(crate) fn polyline(file: &P21File, n: u64, p: &[Param], space: Space) -> Result<NurbsCurve, Fail> {
    arity(p, 2, n)?;
    let refs = references(arg(p, 1, n)?, n)?;
    if refs.len() < 2 {
        return Err(Fail::Malformed { at: n, what: "polyline_points" });
    }
    let mut ctrl = Vec::with_capacity(refs.len());
    for r in refs {
        ctrl.push(ctrl_point(file, r, space)?);
    }
    let last = (ctrl.len() - 1) as f64;
    let mut knots = Vec::with_capacity(ctrl.len() + 2);
    knots.push(0.0);
    for i in 0..ctrl.len() {
        knots.push(i as f64);
    }
    knots.push(last);
    NurbsCurve::new(NurbsCurveParts { degree: 1, knots, ctrl, weights: None, periodic: false })
        .map_err(|err| Fail::Nurbs { at: n, err })
}

/// A 3D curve entity `n` of kind `kind` mapped to its GEO type.
pub(crate) fn curve(file: &P21File, inst: &Instance, kind: Kind, units: Units) -> Result<CurveRef, Fail> {
    let n = inst.number;
    let single = || -> Result<&[Param], Fail> {
        match inst.parts.as_slice() {
            [(_, p)] => Ok(p.as_slice()),
            _ => Err(Fail::Malformed { at: n, what: "unexpected_entity_type" }),
        }
    };
    match kind {
        Kind::Line => {
            let p = single()?;
            arity(p, 3, n)?;
            let origin = point3(file, reference(arg(p, 1, n)?, n)?, units)?;
            let (dir, mag) = vector(file, reference(arg(p, 2, n)?, n)?)?;
            let x = super::place::direction3(file, dir)?;
            let line = Line::new(line_frame(origin, x, n)?);
            Ok(CurveRef::Line { line, magnitude_m: mag * units.length })
        }
        Kind::Circle => {
            let p = single()?;
            arity(p, 3, n)?;
            let frame = axis2_3d(file, reference(arg(p, 1, n)?, n)?, units)?;
            let r = real(arg(p, 2, n)?, n)? * units.length;
            let c = Circle::new(frame.placement, r).map_err(|err| Fail::Geo { at: n, err })?;
            Ok(CurveRef::Circle(c))
        }
        Kind::Ellipse => {
            let p = single()?;
            arity(p, 4, n)?;
            let frame = axis2_3d(file, reference(arg(p, 1, n)?, n)?, units)?;
            let a = real(arg(p, 2, n)?, n)? * units.length;
            let b = real(arg(p, 3, n)?, n)? * units.length;
            let e = Ellipse::new(frame.placement, a, b).map_err(|err| Fail::Geo { at: n, err })?;
            Ok(CurveRef::Ellipse(e))
        }
        Kind::BsplineCurve => Ok(CurveRef::Nurbs(bspline_curve(file, inst, Space::Model(units))?)),
        Kind::Polyline => Ok(CurveRef::Polyline(polyline(file, n, single()?, Space::Model(units))?)),
        Kind::Trimmed => Ok(CurveRef::Trimmed(trimmed(file, n, single()?, units)?)),
        Kind::SurfaceCurve | Kind::SeamCurve => {
            let p = single()?;
            arity(p, 4, n)?;
            let curve_3d = reference(arg(p, 1, n)?, n)?;
            let associated = references(arg(p, 2, n)?, n)?;
            if associated.is_empty() || associated.len() > 2 {
                return Err(Fail::Malformed { at: n, what: "associated_geometry" });
            }
            let master = trim_master(enumeration(arg(p, 3, n)?, n)?, n)?;
            Ok(CurveRef::Surface { curve_3d, associated, master, seam: kind == Kind::SeamCurve })
        }
        _ => Err(Fail::Malformed { at: n, what: "unexpected_entity_type" }),
    }
}

/// `master_representation`: `.CURVE_3D.`/`.CARTESIAN.` map to [`TrimMaster::Cartesian`] (the
/// model-space representation), `.PCURVE_S1.`/`.PCURVE_S2.`/`.PARAMETER.` to
/// [`TrimMaster::Parameter`], `.UNSPECIFIED.` to [`TrimMaster::Unspecified`].
fn trim_master(e: &str, at: u64) -> Result<TrimMaster, Fail> {
    match e {
        "CARTESIAN" | "CURVE_3D" => Ok(TrimMaster::Cartesian),
        "PARAMETER" | "PCURVE_S1" | "PCURVE_S2" => Ok(TrimMaster::Parameter),
        "UNSPECIFIED" => Ok(TrimMaster::Unspecified),
        _ => Err(Fail::Malformed { at, what: "master_representation" }),
    }
}

/// Factor from a curve's STEP parameter to its GEO parameter: the line's vector magnitude in
/// metres (GEO lines use a unit direction), the plane-angle factor for circles and ellipses
/// [unverified: conic parameters are in the context's plane-angle unit], 1 for B-splines and
/// polylines. Follows `trimmed_curve` and `surface_curve` bases up to [`MAX_BASIS_DEPTH`].
pub(crate) fn param_factor(file: &P21File, n: u64, units: Units) -> Result<f64, Fail> {
    let mut at = n;
    for _ in 0..MAX_BASIS_DEPTH {
        let inst = get(file, at)?;
        match super::classify(inst) {
            Kind::Line => {
                let p = &inst.parts[0].1;
                arity(p, 3, at)?;
                let (_, mag) = vector(file, reference(arg(p, 2, at)?, at)?)?;
                return Ok(mag * units.length);
            }
            Kind::Circle | Kind::Ellipse => return Ok(units.angle),
            Kind::BsplineCurve | Kind::Polyline => return Ok(1.0),
            Kind::Trimmed | Kind::SurfaceCurve | Kind::SeamCurve => {
                let p = &inst.parts[0].1;
                at = reference(arg(p, 1, at)?, at)?;
            }
            _ => return Err(Fail::Unsupported { at }),
        }
    }
    Err(Fail::Unsupported { at })
}

/// Bases followed from a trimmed or surface curve to the curve that carries the parameter.
pub(crate) const MAX_BASIS_DEPTH: usize = 8;

/// `trimmed_curve`: the basis by reference, each trim as a GEO parameter and/or a point in metres.
fn trimmed(file: &P21File, n: u64, p: &[Param], units: Units) -> Result<TrimmedCurve, Fail> {
    arity(p, 6, n)?;
    let basis = reference(arg(p, 1, n)?, n)?;
    let factor = param_factor(file, basis, units)?;
    let trim_1 = trim(file, n, arg(p, 2, n)?, factor, units)?;
    let trim_2 = trim(file, n, arg(p, 3, n)?, factor, units)?;
    let sense = boolean(arg(p, 4, n)?, n)?;
    let master = trim_master(enumeration(arg(p, 5, n)?, n)?, n)?;
    Ok(TrimmedCurve { basis, trim_1, trim_2, sense, master })
}

fn trim(file: &P21File, n: u64, p: &Param, factor: f64, units: Units) -> Result<Trim, Fail> {
    let items = list(p, n)?;
    if items.is_empty() || items.len() > 2 {
        return Err(Fail::Malformed { at: n, what: "trim_select" });
    }
    let mut out = Trim { param: None, point: None };
    for it in items {
        match it {
            Param::Ref(r) => {
                if out.point.replace(point3(file, *r, units)?).is_some() {
                    return Err(Fail::Malformed { at: n, what: "trim_select" });
                }
            }
            Param::Typed(name, _) if name.as_str() == "PARAMETER_VALUE" => {
                if out.param.replace(real(it, n)? * factor).is_some() {
                    return Err(Fail::Malformed { at: n, what: "trim_select" });
                }
            }
            _ => return Err(Fail::Malformed { at: n, what: "trim_select" }),
        }
    }
    Ok(out)
}

/// A 2D curve of a pcurve, its coordinates scaled per component by `s` (the basis surface's
/// parameter factors).
pub(crate) fn curve2(file: &P21File, n: u64, s: [f64; 2]) -> Result<Curve2, Fail> {
    let inst = get(file, n)?;
    let kind = super::classify(inst);
    let single = || -> Result<&[Param], Fail> {
        match inst.parts.as_slice() {
            [(_, p)] => Ok(p.as_slice()),
            _ => Err(Fail::Malformed { at: n, what: "unexpected_entity_type" }),
        }
    };
    match kind {
        Kind::Line => {
            let p = single()?;
            arity(p, 3, n)?;
            let o = point2(file, reference(arg(p, 1, n)?, n)?)?;
            let (dir, mag) = vector(file, reference(arg(p, 2, n)?, n)?)?;
            let d = direction2(file, dir)?;
            Ok(Curve2::Line { origin: [o[0] * s[0], o[1] * s[1]], vector: [(d[0] * mag) * s[0], (d[1] * mag) * s[1]] })
        }
        Kind::Circle | Kind::Ellipse => {
            let p = single()?;
            let (a, b) = if kind == Kind::Circle {
                arity(p, 3, n)?;
                let r = real(arg(p, 2, n)?, n)?;
                (r, r)
            } else {
                arity(p, 4, n)?;
                (real(arg(p, 2, n)?, n)?, real(arg(p, 3, n)?, n)?)
            };
            if s[0].to_bits() != s[1].to_bits() {
                return Err(Fail::Unsupported { at: n });
            }
            if a <= 0.0 || b <= 0.0 {
                return Err(Fail::Malformed { at: n, what: "conic_size" });
            }
            let (o, x) = axis2_2d(file, reference(arg(p, 1, n)?, n)?)?;
            Ok(Curve2::Conic { center: [o[0] * s[0], o[1] * s[1]], x_dir: x, semi_axes: [a * s[0], b * s[0]] })
        }
        Kind::BsplineCurve => Ok(Curve2::Nurbs(bspline_curve(file, inst, Space::Param(s))?)),
        Kind::Polyline => Ok(Curve2::Nurbs(polyline(file, n, single()?, Space::Param(s))?)),
        _ => Err(Fail::Unsupported { at: n }),
    }
}

/// `definitional_representation` `n`: its single item.
pub(crate) fn definitional_item(file: &P21File, n: u64) -> Result<u64, Fail> {
    let inst = get(file, n)?;
    let p = match inst.parts.as_slice() {
        [(name, p)] if name.as_str() == "DEFINITIONAL_REPRESENTATION" => p,
        _ => part(inst, "REPRESENTATION")?.ok_or(Fail::Malformed { at: n, what: "unexpected_entity_type" })?,
    };
    arity(p, 3, n)?;
    match references(arg(p, 1, n)?, n)?.as_slice() {
        [item] => Ok(*item),
        _ => Err(Fail::Malformed { at: n, what: "definitional_items" }),
    }
}

// ------------------------------------------------------------- exact NURBS of swept curves

/// The 9-point rational quadratic full ellipse `C + a cos X + b sin Y` (a circle when `a == b`)
/// over the knots `[0, 0, 0, pi/2, pi/2, pi, pi, 3pi/2, 3pi/2, 2pi, 2pi, 2pi]`: four quarter
/// arcs, corner weights `1/sqrt(2)`. Exact geometry; the NURBS parameter equals the angle only at
/// the quarter points.
pub(crate) fn ellipse_nurbs(
    c: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    a: f64,
    b: f64,
) -> (Vec<f64>, Vec<[f64; 3]>, Vec<f64>) {
    let ax = super::place::scale(a, x);
    let by = super::place::scale(b, y);
    let corner = |sx: f64, sy: f64| {
        [c[0] + sx * ax[0] + sy * by[0], c[1] + sx * ax[1] + sy * by[1], c[2] + sx * ax[2] + sy * by[2]]
    };
    let signs: [(f64, f64); 9] = [
        (1.0, 0.0),
        (1.0, 1.0),
        (0.0, 1.0),
        (-1.0, 1.0),
        (-1.0, 0.0),
        (-1.0, -1.0),
        (0.0, -1.0),
        (1.0, -1.0),
        (1.0, 0.0),
    ];
    let ctrl: Vec<[f64; 3]> = signs.iter().map(|&(sx, sy)| corner(sx, sy)).collect();
    let weights: Vec<f64> = (0..9).map(|i| if i % 2 == 1 { FRAC_1_SQRT_2 } else { 1.0 }).collect();
    let knots = vec![0.0, 0.0, 0.0, FRAC_PI_2, FRAC_PI_2, PI, PI, PI + FRAC_PI_2, PI + FRAC_PI_2, TAU, TAU, TAU];
    (knots, ctrl, weights)
}

/// A swept curve as an exact NURBS curve, with whether its parameter equals the STEP parameter.
pub(crate) struct SweptCurve {
    pub curve: NurbsCurve,
    pub param_exact: bool,
}

fn reversed(c: NurbsCurve, at: u64) -> Result<NurbsCurve, Fail> {
    let mut parts = c.into_parts();
    let (lo, hi) = match (parts.knots.first(), parts.knots.last()) {
        (Some(&lo), Some(&hi)) => (lo, hi),
        _ => return Err(Fail::Malformed { at, what: "knots" }),
    };
    parts.knots = parts.knots.iter().rev().map(|&k| (lo + hi) - k).collect();
    parts.ctrl.reverse();
    if let Some(w) = parts.weights.as_mut() {
        w.reverse();
    }
    NurbsCurve::new(parts).map_err(|err| Fail::Nurbs { at, err })
}

/// The exact NURBS of swept curve `n`: B-splines and polylines as they are; circles and
/// ellipses as the full 9-point rational quadratic; a `trimmed_curve` of a line as the segment
/// between its trims (the line's own parameter kept); a `trimmed_curve` of anything else as the
/// untrimmed basis (a superset of the trimmed curve, so the surface still contains the face),
/// reversed when `sense_agreement` is false. An untrimmed line is unbounded and refused.
pub(crate) fn swept_curve(file: &P21File, n: u64, units: Units) -> Result<SweptCurve, Fail> {
    let inst = get(file, n)?;
    match super::classify(inst) {
        Kind::BsplineCurve => {
            Ok(SweptCurve { curve: bspline_curve(file, inst, Space::Model(units))?, param_exact: true })
        }
        Kind::Polyline => {
            let p = &inst.parts[0].1;
            Ok(SweptCurve { curve: polyline(file, n, p, Space::Model(units))?, param_exact: true })
        }
        Kind::Circle | Kind::Ellipse => {
            let (pl, a, b) = match curve(file, inst, super::classify(inst), units)? {
                CurveRef::Circle(c) => (*c.placement(), c.radius(), c.radius()),
                CurveRef::Ellipse(e) => {
                    let (a, b) = e.semi_axes();
                    (*e.placement(), a, b)
                }
                _ => return Err(Fail::Malformed { at: n, what: "unexpected_entity_type" }),
            };
            let (knots, ctrl, weights) = ellipse_nurbs(pl.origin(), pl.ref_dir(), pl.y_dir(), a, b);
            let curve =
                NurbsCurve::new(NurbsCurveParts { degree: 2, knots, ctrl, weights: Some(weights), periodic: false })
                    .map_err(|err| Fail::Nurbs { at: n, err })?;
            Ok(SweptCurve { curve, param_exact: false })
        }
        Kind::Trimmed => {
            let p = &inst.parts[0].1;
            arity(p, 6, n)?;
            let basis_n = reference(arg(p, 1, n)?, n)?;
            let basis = get(file, basis_n)?;
            if super::classify(basis) == Kind::Line {
                // Trims in the line's own STEP parameter (factor 1).
                let t1 = trim(file, n, arg(p, 2, n)?, 1.0, units)?;
                let t2 = trim(file, n, arg(p, 3, n)?, 1.0, units)?;
                return line_segment(file, basis, &t1, &t2, units);
            }
            let t = trimmed(file, n, p, units)?;
            if matches!(super::classify(basis), Kind::Trimmed) {
                return Err(Fail::Unsupported { at: t.basis });
            }
            let inner = swept_curve(file, t.basis, units)?;
            if t.sense {
                Ok(inner)
            } else {
                Ok(SweptCurve { curve: reversed(inner.curve, n)?, param_exact: false })
            }
        }
        Kind::Line => Err(Fail::Malformed { at: n, what: "unbounded_swept_curve" }),
        _ => Err(Fail::Unsupported { at: n }),
    }
}

/// The segment of a line between two trims, as a degree-1 curve over the STEP parameters
/// `[min(u1, u2), max(u1, u2)]`, running from trim 1 to trim 2 (a line is open, so the order of
/// the trims gives the direction). A trim given only as a point is projected onto the line. The
/// parameter equals the line's own when `u1 < u2`.
fn line_segment(file: &P21File, line: &Instance, t1: &Trim, t2: &Trim, units: Units) -> Result<SweptCurve, Fail> {
    let n = line.number;
    let p = &line.parts[0].1;
    arity(p, 3, n)?;
    let origin = point3(file, reference(arg(p, 1, n)?, n)?, units)?;
    let v = vector3(file, reference(arg(p, 2, n)?, n)?, units)?;
    let vv = dot(v, v);
    let to_u = |tr: &Trim| -> Result<f64, Fail> {
        match (tr.param, tr.point) {
            (Some(u), _) => Ok(u),
            (None, Some(q)) => Ok(dot(sub(q, origin), v) / vv),
            (None, None) => Err(Fail::Malformed { at: n, what: "trim_select" }),
        }
    };
    let (u1, u2) = (to_u(t1)?, to_u(t2)?);
    let forward = u1 < u2;
    let (lo, hi) = if forward { (u1, u2) } else { (u2, u1) };
    if lo.is_nan() || hi.is_nan() || lo >= hi {
        return Err(Fail::Malformed { at: n, what: "empty_trim" });
    }
    let at = |u: f64| super::place::axpy(origin, u, v);
    let mut ctrl = vec![at(lo), at(hi)];
    if !forward {
        ctrl.reverse();
    }
    let curve = NurbsCurve::new(NurbsCurveParts {
        degree: 1,
        knots: vec![lo, lo, hi, hi],
        ctrl,
        weights: None,
        periodic: false,
    })
    .map_err(|err| Fail::Nurbs { at: n, err })?;
    Ok(SweptCurve { curve, param_exact: forward })
}
