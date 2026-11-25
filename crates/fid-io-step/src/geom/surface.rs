//! Surfaces: `plane`, `cylindrical_surface`, `conical_surface`, `spherical_surface`,
//! `toroidal_surface`, `b_spline_surface_with_knots` (simple and rational complex form), and the
//! exact rational NURBS of `surface_of_linear_extrusion` and `surface_of_revolution`.

use super::curve::{degree, ellipse_nurbs, expand_knots, swept_curve};
use super::place::{axis1, axis2_3d, axpy, cross, dot, norm, point3, report_deviation, sub, vector3};
use super::read::{arg, arity, counts, get, list, part, real, reals, reference};
use super::{Fail, GeomWarning, Kind, ParamBox, SurfaceRef, Units, FRAME_REORTHONORMALISED};
use crate::{Instance, P21File, Param};
use fid_geo_nurbs::{
    AnalyticSurface, Cone, Cylinder, Dir, NurbsError, NurbsSurface, NurbsSurfaceParts, Plane, Sphere, Torus, MAX_DEGREE,
};

/// The single part's parameters of a simple instance.
fn single(inst: &Instance) -> Result<&[Param], Fail> {
    match inst.parts.as_slice() {
        [(_, p)] => Ok(p.as_slice()),
        _ => Err(Fail::Malformed { at: inst.number, what: "unexpected_entity_type" }),
    }
}

/// An analytic surface: placement in metres, radii times the length factor, the cone's
/// semi-angle times the plane-angle factor, each once.
pub(crate) fn analytic(
    file: &P21File,
    inst: &Instance,
    kind: Kind,
    units: Units,
    warnings: &mut Vec<GeomWarning>,
) -> Result<AnalyticSurface, Fail> {
    let n = inst.number;
    let p = single(inst)?;
    let want = match kind {
        Kind::Plane => 2,
        Kind::Cylinder | Kind::Sphere => 3,
        Kind::Cone | Kind::Torus => 4,
        _ => return Err(Fail::Malformed { at: n, what: "unexpected_entity_type" }),
    };
    arity(p, want, n)?;
    let place = reference(arg(p, 1, n)?, n)?;
    let frame = axis2_3d(file, place, units)?;
    if report_deviation(frame.deviation) {
        warnings.push(GeomWarning { entity: place, code: FRAME_REORTHONORMALISED, value: frame.deviation });
    }
    let pl = frame.placement;
    let len = |i: usize| -> Result<f64, Fail> { Ok(real(arg(p, i, n)?, n)? * units.length) };
    let geo = |err| Fail::Geo { at: n, err };
    Ok(match kind {
        Kind::Plane => AnalyticSurface::Plane(Plane::new(pl)),
        Kind::Cylinder => AnalyticSurface::Cylinder(Cylinder::new(pl, len(2)?).map_err(geo)?),
        Kind::Cone => {
            let angle = real(arg(p, 3, n)?, n)? * units.angle;
            AnalyticSurface::Cone(Cone::new(pl, len(2)?, angle).map_err(geo)?)
        }
        Kind::Sphere => AnalyticSurface::Sphere(Sphere::new(pl, len(2)?).map_err(geo)?),
        _ => AnalyticSurface::Torus(Torus::new(pl, len(2)?, len(3)?).map_err(geo)?),
    })
}

struct BsplineSurfaceData<'a> {
    degree_u: &'a Param,
    degree_v: &'a Param,
    ctrl: &'a Param,
    mults_u: &'a Param,
    mults_v: &'a Param,
    knots_u: &'a Param,
    knots_v: &'a Param,
    weights: Option<&'a Param>,
}

fn bspline_surface_data(inst: &Instance) -> Result<BsplineSurfaceData<'_>, Fail> {
    let n = inst.number;
    if let [(name, p)] = inst.parts.as_slice() {
        if name.as_str() != "B_SPLINE_SURFACE_WITH_KNOTS" {
            return Err(Fail::Malformed { at: n, what: "unexpected_entity_type" });
        }
        arity(p, 13, n)?;
        return Ok(BsplineSurfaceData {
            degree_u: arg(p, 1, n)?,
            degree_v: arg(p, 2, n)?,
            ctrl: arg(p, 3, n)?,
            mults_u: arg(p, 8, n)?,
            mults_v: arg(p, 9, n)?,
            knots_u: arg(p, 10, n)?,
            knots_v: arg(p, 11, n)?,
            weights: None,
        });
    }
    let base = part(inst, "B_SPLINE_SURFACE")?.ok_or(Fail::Malformed { at: n, what: "missing_part" })?;
    let kn = part(inst, "B_SPLINE_SURFACE_WITH_KNOTS")?.ok_or(Fail::Malformed { at: n, what: "missing_part" })?;
    arity(base, 7, n)?;
    arity(kn, 5, n)?;
    let weights = match part(inst, "RATIONAL_B_SPLINE_SURFACE")? {
        Some(w) => {
            arity(w, 1, n)?;
            Some(arg(w, 0, n)?)
        }
        None => None,
    };
    Ok(BsplineSurfaceData {
        degree_u: arg(base, 0, n)?,
        degree_v: arg(base, 1, n)?,
        ctrl: arg(base, 2, n)?,
        mults_u: arg(kn, 0, n)?,
        mults_v: arg(kn, 1, n)?,
        knots_u: arg(kn, 2, n)?,
        knots_v: arg(kn, 3, n)?,
        weights,
    })
}

/// A rectangular list of lists: `(rows, columns)`, refusing ragged rows.
fn grid(p: &Param, at: u64) -> Result<(Vec<&[Param]>, usize), Fail> {
    let rows: Vec<&[Param]> = list(p, at)?.iter().map(|r| list(r, at)).collect::<Result<_, _>>()?;
    let cols = rows.first().map_or(0, |r| r.len());
    if rows.iter().any(|r| r.len() != cols) {
        return Err(Fail::Malformed { at, what: "ragged_net" });
    }
    Ok((rows, cols))
}

/// A B-spline surface (simple or rational complex form), validated by GEO. The STEP net
/// `control_points_list[i][j]` (`i` along `u`) is stored row-major at `i * n_v + j`, as GEO does.
pub(crate) fn bspline_surface(file: &P21File, inst: &Instance, units: Units) -> Result<NurbsSurface, Fail> {
    let n = inst.number;
    let d = bspline_surface_data(inst)?;
    let degree_u = degree(d.degree_u, n)?;
    let degree_v = degree(d.degree_v, n)?;
    for (dir, deg) in [(Dir::U, degree_u), (Dir::V, degree_v)] {
        if deg == 0 || deg > MAX_DEGREE {
            return Err(Fail::Nurbs { at: n, err: NurbsError::BadDegree { dir, degree: deg } });
        }
    }
    let (rows, n_v) = grid(d.ctrl, n)?;
    let n_u = rows.len();
    let mut ctrl = Vec::with_capacity(n_u.saturating_mul(n_v));
    for row in &rows {
        for r in row.iter() {
            ctrl.push(point3(file, reference(r, n)?, units)?);
        }
    }
    let knots_u = expand_knots(&reals(d.knots_u, n)?, &counts(d.mults_u, n)?, n_u, degree_u, Dir::U, n)?;
    let knots_v = expand_knots(&reals(d.knots_v, n)?, &counts(d.mults_v, n)?, n_v, degree_v, Dir::V, n)?;
    let weights = match d.weights {
        Some(w) => {
            let (wrows, wcols) = grid(w, n)?;
            if wrows.len() != n_u || wcols != n_v {
                return Err(Fail::Nurbs {
                    at: n,
                    err: NurbsError::WeightCount { weights: wrows.len().saturating_mul(wcols), ctrl: ctrl.len() },
                });
            }
            let mut flat = Vec::with_capacity(ctrl.len());
            for row in wrows {
                for x in row {
                    flat.push(real(x, n)?);
                }
            }
            Some(flat)
        }
        None => None,
    };
    NurbsSurface::new(NurbsSurfaceParts {
        degree_u,
        degree_v,
        knots_u,
        knots_v,
        n_u,
        n_v,
        ctrl,
        weights,
        periodic_u: false,
        periodic_v: false,
    })
    .map_err(|err| Fail::Nurbs { at: n, err })
}

/// `surface_of_linear_extrusion`: `S(u, v) = C(u) + v V` as a NURBS of degree 1 in `v` over
/// `[v0, v1]` (the caller's box, else `[0, 1]`), control rows `P_i + v0 V`, `P_i + v1 V` with the
/// curve's weights. Exact; the `v` parameter is STEP's, the `u` parameter is STEP's when the
/// swept curve's is ([`super::curve::SweptCurve::param_exact`]).
pub(crate) fn extrusion(
    file: &P21File,
    inst: &Instance,
    units: Units,
    hint: Option<&ParamBox>,
) -> Result<SurfaceRef, Fail> {
    let n = inst.number;
    let p = single(inst)?;
    arity(p, 3, n)?;
    let swept = swept_curve(file, reference(arg(p, 1, n)?, n)?, units)?;
    let v = vector3(file, reference(arg(p, 2, n)?, n)?, units)?;
    let (v0, v1) = hint.map_or((0.0, 1.0), |b| b.v);
    let parts = swept.curve.into_parts();
    let mut ctrl = Vec::with_capacity(parts.ctrl.len().saturating_mul(2));
    for &c in &parts.ctrl {
        ctrl.push(axpy(c, v0, v));
        ctrl.push(axpy(c, v1, v));
    }
    let weights = parts.weights.map(|w| w.iter().flat_map(|&x| [x, x]).collect());
    let nurbs = NurbsSurface::new(NurbsSurfaceParts {
        degree_u: parts.degree,
        degree_v: 1,
        knots_u: parts.knots,
        knots_v: vec![v0, v0, v1, v1],
        n_u: parts.ctrl.len(),
        n_v: 2,
        ctrl,
        weights,
        periodic_u: false,
        periodic_v: false,
    })
    .map_err(|err| Fail::Nurbs { at: n, err })?;
    Ok(SurfaceRef::Extrusion { nurbs, param_exact: swept.param_exact, domain_defaulted: hint.is_none() })
}

/// `surface_of_revolution`: the swept curve rotated about the axis by the angle `v`, as the
/// tensor product of the curve with the 9-point rational quadratic circle (`v` knots
/// `0 .. 2 pi` in quarter turns). Each control point `P_i` is split into its foot `O_i` on the
/// axis and radius vector `R_i = P_i - O_i`; the circle of radius `|R_i|` about `O_i` in the
/// frame `X = R_i / |R_i|`, `Y = D x X` gives row `i`, with weights `w_i c_j`. A control point on
/// the axis gives a row of nine copies. Exact geometry; `v` is the angle only at quarter turns.
pub(crate) fn revolution(file: &P21File, inst: &Instance, units: Units) -> Result<SurfaceRef, Fail> {
    let n = inst.number;
    let p = single(inst)?;
    arity(p, 3, n)?;
    let swept = swept_curve(file, reference(arg(p, 1, n)?, n)?, units)?;
    let (a, d) = axis1(file, reference(arg(p, 2, n)?, n)?, units)?;
    let parts = swept.curve.into_parts();
    let mut ctrl = Vec::with_capacity(parts.ctrl.len().saturating_mul(9));
    let mut weights = Vec::with_capacity(ctrl.capacity());
    let mut knots_v = Vec::new();
    for (i, &pi) in parts.ctrl.iter().enumerate() {
        let wi = parts.weights.as_ref().and_then(|w| w.get(i).copied()).unwrap_or(1.0);
        let h = dot(sub(pi, a), d);
        let o = axpy(a, h, d);
        let r_vec = sub(pi, o);
        let r = norm(r_vec)?;
        let (kv, row, cw) = if r > 0.0 {
            let x = [r_vec[0] / r, r_vec[1] / r, r_vec[2] / r];
            let y = cross(d, x);
            ellipse_nurbs(o, x, y, r, r)
        } else {
            let (kv, _, cw) = ellipse_nurbs(o, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.0, 0.0);
            (kv, vec![pi; 9], cw)
        };
        knots_v = kv;
        ctrl.extend(row);
        weights.extend(cw.iter().map(|&c| wi * c));
    }
    let nurbs = NurbsSurface::new(NurbsSurfaceParts {
        degree_u: parts.degree,
        degree_v: 2,
        knots_u: parts.knots,
        knots_v,
        n_u: parts.ctrl.len(),
        n_v: 9,
        ctrl,
        weights: Some(weights),
        periodic_u: false,
        periodic_v: false,
    })
    .map_err(|err| Fail::Nurbs { at: n, err })?;
    Ok(SurfaceRef::Revolution { nurbs, param_exact: false, u_param_exact: swept.param_exact })
}

/// The kind of the curve that carries the parameter of curve `n` (following trimmed and
/// surface curves).
pub(crate) fn underlying_kind(file: &P21File, n: u64) -> Result<Kind, Fail> {
    let mut at = n;
    for _ in 0..super::curve::MAX_BASIS_DEPTH {
        let inst = get(file, at)?;
        let k = super::classify(inst);
        match k {
            Kind::Trimmed | Kind::SurfaceCurve | Kind::SeamCurve => at = reference(arg(single(inst)?, 1, at)?, at)?,
            other => return Ok(other),
        }
    }
    Err(Fail::Unsupported { at })
}

/// Factors from a surface's STEP parameters to the GEO parameters of its mapped surface, and
/// whether that map is exact (affine). Plane `(L, L)`; cylinder and cone `(A, L)`; sphere and
/// torus `(A, A)`; B-spline `(1, 1)`; extrusion `(c, 1)` and revolution `(c, A)` with `c = A` for
/// a conic swept curve, else 1; offset: its basis surface's. `L` and `A` are the length and
/// plane-angle factors.
pub(crate) fn param_scales(file: &P21File, n: u64, units: Units) -> Result<([f64; 2], bool), Fail> {
    let (l, a) = (units.length, units.angle);
    let mut at = n;
    for _ in 0..2 {
        let inst = get(file, at)?;
        let k = super::classify(inst);
        return match k {
            Kind::Plane => Ok(([l, l], true)),
            Kind::Cylinder | Kind::Cone => Ok(([a, l], true)),
            Kind::Sphere | Kind::Torus => Ok(([a, a], true)),
            Kind::BsplineSurface => Ok(([1.0, 1.0], true)),
            Kind::Extrusion | Kind::Revolution => {
                let p = single(inst)?;
                arity(p, 3, at)?;
                let c_n = reference(arg(p, 1, at)?, at)?;
                let conic = matches!(underlying_kind(file, c_n)?, Kind::Circle | Kind::Ellipse);
                let c = if conic { a } else { 1.0 };
                if k == Kind::Extrusion {
                    Ok(([c, 1.0], !conic && swept_curve(file, c_n, units)?.param_exact))
                } else {
                    Ok(([c, a], false))
                }
            }
            Kind::Offset => {
                let p = single(inst)?;
                arity(p, 4, at)?;
                at = reference(arg(p, 1, at)?, at)?;
                continue;
            }
            _ => Err(Fail::Unsupported { at }),
        };
    }
    Err(Fail::Unsupported { at })
}
