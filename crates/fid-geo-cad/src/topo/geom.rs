//! Curve and surface definitions held by faces, edges and coedges.
//!
//! A definition keeps the parameters exactly as given (after `-0.0` is replaced by `+0.0`)
//! next to the validated GEO.NURBS object built from them. The encoding writes the given
//! parameters, so encode, decode, encode is byte-identical even where GEO.NURBS normalises
//! internally (a [`Placement`] re-normalises its axis and reference direction).
//!
//! Construction validates through GEO.NURBS (`NurbsCurve::new`, `Placement::new`,
//! `Circle::new`, ...): a definition that exists is valid, so every stored `f64` is finite.

use core::fmt;

use fid_geo_nurbs::{
    AnalyticCurve, AnalyticError, AnalyticSurface, Circle, Cone, Cylinder, Ellipse, Line, NurbsCurve, NurbsCurveParts,
    NurbsError, NurbsSurface, NurbsSurfaceParts, Placement, Plane, Sphere, Torus,
};

/// `-0.0` becomes `+0.0`; every other value keeps its bits.
pub(crate) fn canon(x: f64) -> f64 {
    if x == 0.0 {
        0.0
    } else {
        x
    }
}

pub(crate) fn canon3(a: [f64; 3]) -> [f64; 3] {
    [canon(a[0]), canon(a[1]), canon(a[2])]
}

fn canon_vec(v: Vec<f64>) -> Vec<f64> {
    v.into_iter().map(canon).collect()
}

/// Why a curve or surface definition was refused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GeomError {
    /// GEO.NURBS.ANALYTIC refused the placement or a size.
    Analytic(AnalyticError),
    /// GEO.NURBS.DATA refused the knots, control points or weights.
    Nurbs(NurbsError),
    /// A NURBS pcurve control point at `index` has a non-zero third coordinate.
    PcurveNotPlanar { index: usize },
    /// A 2-D line has a non-finite value or a zero direction.
    BadLine2,
}

impl GeomError {
    /// Stable variant name.
    pub fn code(&self) -> &'static str {
        match self {
            GeomError::Analytic(_) => "Analytic",
            GeomError::Nurbs(_) => "Nurbs",
            GeomError::PcurveNotPlanar { .. } => "PcurveNotPlanar",
            GeomError::BadLine2 => "BadLine2",
        }
    }
}

impl fmt::Display for GeomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GeomError::Analytic(e) => write!(f, "analytic: {e}"),
            GeomError::Nurbs(e) => write!(f, "nurbs: {e}"),
            GeomError::PcurveNotPlanar { index } => write!(f, "pcurve control point {index} has a non-zero z"),
            GeomError::BadLine2 => write!(f, "2-D line has a non-finite value or a zero direction"),
        }
    }
}

impl core::error::Error for GeomError {}

impl From<AnalyticError> for GeomError {
    fn from(e: AnalyticError) -> Self {
        GeomError::Analytic(e)
    }
}

impl From<NurbsError> for GeomError {
    fn from(e: NurbsError) -> Self {
        GeomError::Nurbs(e)
    }
}

/// The given placement of an analytic shape: origin, axis `Z`, reference direction `X`
/// (the arguments of [`Placement::new`], in that order).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub origin: [f64; 3],
    pub axis: [f64; 3],
    pub ref_dir: [f64; 3],
}

impl Frame {
    fn canonical(self) -> Frame {
        Frame { origin: canon3(self.origin), axis: canon3(self.axis), ref_dir: canon3(self.ref_dir) }
    }

    fn placement(&self) -> Result<Placement, GeomError> {
        Ok(Placement::new(self.origin, self.axis, self.ref_dir)?)
    }
}

/// Given parameters of an analytic 3-D curve (constructor arguments, in order).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CurveParams {
    Line { frame: Frame },
    Circle { frame: Frame, radius: f64 },
    Ellipse { frame: Frame, semi_axis_1: f64, semi_axis_2: f64 },
}

impl CurveParams {
    fn canonical(self) -> CurveParams {
        match self {
            CurveParams::Line { frame } => CurveParams::Line { frame: frame.canonical() },
            CurveParams::Circle { frame, radius } => {
                CurveParams::Circle { frame: frame.canonical(), radius: canon(radius) }
            }
            CurveParams::Ellipse { frame, semi_axis_1, semi_axis_2 } => CurveParams::Ellipse {
                frame: frame.canonical(),
                semi_axis_1: canon(semi_axis_1),
                semi_axis_2: canon(semi_axis_2),
            },
        }
    }

    fn build(&self) -> Result<AnalyticCurve, GeomError> {
        Ok(match *self {
            CurveParams::Line { frame } => AnalyticCurve::Line(Line::new(frame.placement()?)),
            CurveParams::Circle { frame, radius } => AnalyticCurve::Circle(Circle::new(frame.placement()?, radius)?),
            CurveParams::Ellipse { frame, semi_axis_1, semi_axis_2 } => {
                AnalyticCurve::Ellipse(Ellipse::new(frame.placement()?, semi_axis_1, semi_axis_2)?)
            }
        })
    }
}

/// Given parameters of an analytic surface (constructor arguments, in order).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SurfaceParams {
    Plane { frame: Frame },
    Cylinder { frame: Frame, radius: f64 },
    Cone { frame: Frame, radius: f64, half_angle: f64 },
    Sphere { frame: Frame, radius: f64 },
    Torus { frame: Frame, major_radius: f64, minor_radius: f64 },
}

impl SurfaceParams {
    fn canonical(self) -> SurfaceParams {
        match self {
            SurfaceParams::Plane { frame } => SurfaceParams::Plane { frame: frame.canonical() },
            SurfaceParams::Cylinder { frame, radius } => {
                SurfaceParams::Cylinder { frame: frame.canonical(), radius: canon(radius) }
            }
            SurfaceParams::Cone { frame, radius, half_angle } => {
                SurfaceParams::Cone { frame: frame.canonical(), radius: canon(radius), half_angle: canon(half_angle) }
            }
            SurfaceParams::Sphere { frame, radius } => {
                SurfaceParams::Sphere { frame: frame.canonical(), radius: canon(radius) }
            }
            SurfaceParams::Torus { frame, major_radius, minor_radius } => SurfaceParams::Torus {
                frame: frame.canonical(),
                major_radius: canon(major_radius),
                minor_radius: canon(minor_radius),
            },
        }
    }

    fn build(&self) -> Result<AnalyticSurface, GeomError> {
        Ok(match *self {
            SurfaceParams::Plane { frame } => AnalyticSurface::Plane(Plane::new(frame.placement()?)),
            SurfaceParams::Cylinder { frame, radius } => {
                AnalyticSurface::Cylinder(Cylinder::new(frame.placement()?, radius)?)
            }
            SurfaceParams::Cone { frame, radius, half_angle } => {
                AnalyticSurface::Cone(Cone::new(frame.placement()?, radius, half_angle)?)
            }
            SurfaceParams::Sphere { frame, radius } => {
                AnalyticSurface::Sphere(Sphere::new(frame.placement()?, radius)?)
            }
            SurfaceParams::Torus { frame, major_radius, minor_radius } => {
                AnalyticSurface::Torus(Torus::new(frame.placement()?, major_radius, minor_radius)?)
            }
        })
    }
}

fn canonical_curve(c: NurbsCurve) -> Result<NurbsCurve, GeomError> {
    let p = c.into_parts();
    Ok(NurbsCurve::new(NurbsCurveParts {
        degree: p.degree,
        knots: canon_vec(p.knots),
        ctrl: p.ctrl.into_iter().map(canon3).collect(),
        weights: p.weights.map(canon_vec),
        periodic: p.periodic,
    })?)
}

fn canonical_surface(s: NurbsSurface) -> Result<NurbsSurface, GeomError> {
    let p = s.into_parts();
    Ok(NurbsSurface::new(NurbsSurfaceParts {
        degree_u: p.degree_u,
        degree_v: p.degree_v,
        knots_u: canon_vec(p.knots_u),
        knots_v: canon_vec(p.knots_v),
        n_u: p.n_u,
        n_v: p.n_v,
        ctrl: p.ctrl.into_iter().map(canon3).collect(),
        weights: p.weights.map(canon_vec),
        periodic_u: p.periodic_u,
        periodic_v: p.periodic_v,
    })?)
}

#[derive(Debug, Clone, PartialEq)]
enum CurveInner {
    Analytic { params: CurveParams, curve: AnalyticCurve },
    Nurbs(NurbsCurve),
}

/// A 3-D edge curve: an analytic line, circle or ellipse, or a NURBS curve.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveDef(CurveInner);

impl CurveDef {
    /// Validates the analytic parameters (after `-0.0` to `+0.0`).
    pub fn analytic(params: CurveParams) -> Result<CurveDef, GeomError> {
        let params = params.canonical();
        let curve = params.build()?;
        Ok(CurveDef(CurveInner::Analytic { params, curve }))
    }

    /// Wraps a validated NURBS curve (after `-0.0` to `+0.0` in knots, control points and
    /// weights).
    pub fn nurbs(curve: NurbsCurve) -> Result<CurveDef, GeomError> {
        Ok(CurveDef(CurveInner::Nurbs(canonical_curve(curve)?)))
    }

    /// The given analytic parameters, or `None` for a NURBS curve.
    pub fn params(&self) -> Option<&CurveParams> {
        match &self.0 {
            CurveInner::Analytic { params, .. } => Some(params),
            CurveInner::Nurbs(_) => None,
        }
    }

    /// The analytic curve, or `None` for a NURBS curve.
    pub fn as_analytic(&self) -> Option<&AnalyticCurve> {
        match &self.0 {
            CurveInner::Analytic { curve, .. } => Some(curve),
            CurveInner::Nurbs(_) => None,
        }
    }

    /// The NURBS curve, or `None` for an analytic curve.
    pub fn as_nurbs(&self) -> Option<&NurbsCurve> {
        match &self.0 {
            CurveInner::Nurbs(c) => Some(c),
            CurveInner::Analytic { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum SurfaceInner {
    Analytic { params: SurfaceParams, surface: AnalyticSurface },
    Nurbs(NurbsSurface),
}

/// A face surface: an analytic plane, cylinder, cone, sphere or torus, or a NURBS surface.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceDef(SurfaceInner);

impl SurfaceDef {
    /// Validates the analytic parameters (after `-0.0` to `+0.0`).
    pub fn analytic(params: SurfaceParams) -> Result<SurfaceDef, GeomError> {
        let params = params.canonical();
        let surface = params.build()?;
        Ok(SurfaceDef(SurfaceInner::Analytic { params, surface }))
    }

    /// Wraps a validated NURBS surface (after `-0.0` to `+0.0`).
    pub fn nurbs(surface: NurbsSurface) -> Result<SurfaceDef, GeomError> {
        Ok(SurfaceDef(SurfaceInner::Nurbs(canonical_surface(surface)?)))
    }

    /// The given analytic parameters, or `None` for a NURBS surface.
    pub fn params(&self) -> Option<&SurfaceParams> {
        match &self.0 {
            SurfaceInner::Analytic { params, .. } => Some(params),
            SurfaceInner::Nurbs(_) => None,
        }
    }

    /// The analytic surface, or `None` for a NURBS surface.
    pub fn as_analytic(&self) -> Option<&AnalyticSurface> {
        match &self.0 {
            SurfaceInner::Analytic { surface, .. } => Some(surface),
            SurfaceInner::Nurbs(_) => None,
        }
    }

    /// The NURBS surface, or `None` for an analytic surface.
    pub fn as_nurbs(&self) -> Option<&NurbsSurface> {
        match &self.0 {
            SurfaceInner::Nurbs(s) => Some(s),
            SurfaceInner::Analytic { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Curve2Inner {
    Line { origin: [f64; 2], dir: [f64; 2] },
    Nurbs(NurbsCurve),
}

/// A parameter-space curve of a coedge (pcurve): a 2-D line `origin + t * dir`, or a NURBS
/// curve whose control points all have `z = 0`.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve2(Curve2Inner);

impl Curve2 {
    /// A 2-D line; every value finite and `dir` not the zero vector.
    pub fn line(origin: [f64; 2], dir: [f64; 2]) -> Result<Curve2, GeomError> {
        let all = [origin[0], origin[1], dir[0], dir[1]];
        if !all.iter().all(|x| x.is_finite()) || (dir[0] == 0.0 && dir[1] == 0.0) {
            return Err(GeomError::BadLine2);
        }
        Ok(Curve2(Curve2Inner::Line {
            origin: [canon(origin[0]), canon(origin[1])],
            dir: [canon(dir[0]), canon(dir[1])],
        }))
    }

    /// A planar NURBS pcurve (`(u, v)` in the first two coordinates, third coordinate zero).
    pub fn nurbs(curve: NurbsCurve) -> Result<Curve2, GeomError> {
        if let Some(index) = curve.ctrl().iter().position(|c| c[2] != 0.0) {
            return Err(GeomError::PcurveNotPlanar { index });
        }
        Ok(Curve2(Curve2Inner::Nurbs(canonical_curve(curve)?)))
    }

    /// `(origin, dir)` for a line, `None` for a NURBS pcurve.
    pub fn as_line(&self) -> Option<([f64; 2], [f64; 2])> {
        match &self.0 {
            Curve2Inner::Line { origin, dir } => Some((*origin, *dir)),
            Curve2Inner::Nurbs(_) => None,
        }
    }

    /// The NURBS pcurve, or `None` for a line.
    pub fn as_nurbs(&self) -> Option<&NurbsCurve> {
        match &self.0 {
            Curve2Inner::Nurbs(c) => Some(c),
            Curve2Inner::Line { .. } => None,
        }
    }
}
