//! Primitives, methods, algorithm IDs and fitted features (REQ-KRN.LSQ.TRAIT-007, -008).

use fid_kci::AlgorithmId;
use fid_num::{check_finite_slice, red};

use super::error::{alloc, FitError};

/// A 2D vector or point `[x, y]`.
pub type V2 = [f64; 2];
/// A 3D vector or point `[x, y, z]`.
pub type V3 = [f64; 3];

macro_rules! slug_enum {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($(#[$vm])* $variant,)+
        }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The ID segment (`[a-z][a-z0-9]*`).
            pub const fn slug(self) -> &'static str {
                match self {
                    $($name::$variant => $s,)+
                }
            }

            /// Parses a slug exactly.
            pub fn parse(s: &str) -> Option<Self> {
                match s {
                    $($s => Some($name::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

slug_enum!(
    /// The geometric primitive a fit produces.
    Primitive {
        Point => "point",
        Line2 => "line2",
        Line3 => "line3",
        Plane => "plane",
        Circle2 => "circle2",
        Circle3 => "circle3",
        Sphere => "sphere",
        Cylinder => "cylinder",
        Cone => "cone",
        Torus => "torus",
        Slot => "slot",
        Rectangle => "rectangle",
        Ellipse => "ellipse",
        Paraboloid => "paraboloid",
        Polyline => "polyline",
        Spline => "spline",
    }
);

slug_enum!(
    /// The fitting objective.
    Method {
        /// Orthogonal-distance least squares (Gaussian), `KRN.LSQ`.
        LeastSquares => "ls",
        /// Minimum zone (Chebyshev), `KRN.MZ`.
        MinimumZone => "mz",
        /// Maximum inscribed, `KRN.ENV`.
        MaximumInscribed => "mi",
        /// Minimum circumscribed, `KRN.ENV`.
        MinimumCircumscribed => "mc",
        /// Tangential (one-sided, material side), `KRN.ENV`.
        Tangential => "tan",
    }
);

/// What the degeneracy checks require of the data for a primitive (pinned table, see
/// [`Primitive::requirement`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Requirement {
    /// Fewest points (the number of free shape parameters).
    pub min_points: usize,
    /// Numerical rank the centred data must reach: 0 (any), 1 (not all coincident),
    /// 2 (not collinear) or 3 (not coplanar).
    pub rank: usize,
    /// Whether the angular coverage check applies (circle-type data).
    pub coverage: bool,
}

impl Primitive {
    /// The pinned requirement table:
    ///
    /// | primitive | min points | rank | coverage |
    /// |---|---|---|---|
    /// | point | 1 | 0 | no |
    /// | line2, line3, polyline, spline | 2 | 1 | no |
    /// | plane | 3 | 2 | no |
    /// | circle2, circle3 | 3 | 2 | yes |
    /// | slot, rectangle, ellipse | 5 | 2 | no |
    /// | sphere | 4 | 3 | no |
    /// | cylinder | 5 | 3 | no |
    /// | cone, paraboloid | 6 | 3 | no |
    /// | torus | 7 | 3 | no |
    pub const fn requirement(self) -> Requirement {
        let (min_points, rank, coverage) = match self {
            Primitive::Point => (1, 0, false),
            Primitive::Line2 | Primitive::Line3 | Primitive::Polyline | Primitive::Spline => (2, 1, false),
            Primitive::Plane => (3, 2, false),
            Primitive::Circle2 | Primitive::Circle3 => (3, 2, true),
            Primitive::Slot | Primitive::Rectangle | Primitive::Ellipse => (5, 2, false),
            Primitive::Sphere => (4, 3, false),
            Primitive::Cylinder => (5, 3, false),
            Primitive::Cone | Primitive::Paraboloid => (6, 3, false),
            Primitive::Torus => (7, 3, false),
        };
        Requirement { min_points, rank, coverage }
    }
}

impl Method {
    /// Whether the method is defined for the primitive (pinned table): least squares for
    /// every primitive; minimum zone for every primitive except `point`; maximum inscribed for
    /// `circle2`, `sphere` and `cylinder` (`mic_tr::MIC_DEFINED`, spec node `KRN.ENV.MIC.TR`;
    /// a 3D circle is projected to `circle2` by the caller); minimum circumscribed for
    /// `circle2`, `circle3`, `sphere` and `cylinder`;
    /// tangential for `line2`, `plane` and `cylinder` (the primitives of
    /// `tan_rock::DEFINED_PAIRS`, spec node `KRN.ENV.TAN.ROCK`; a 3D line has no material side).
    pub const fn defined_for(self, p: Primitive) -> bool {
        match self {
            Method::LeastSquares => true,
            Method::MinimumZone => !matches!(p, Primitive::Point),
            Method::MaximumInscribed => matches!(p, Primitive::Circle2 | Primitive::Sphere | Primitive::Cylinder),
            Method::MinimumCircumscribed => {
                matches!(p, Primitive::Circle2 | Primitive::Circle3 | Primitive::Sphere | Primitive::Cylinder)
            }
            Method::Tangential => matches!(p, Primitive::Line2 | Primitive::Plane | Primitive::Cylinder),
        }
    }
}

/// The algorithm ID `fit.<primitive>.<method>@<major>.<minor>.<patch>` (REQ-KRN.LSQ.TRAIT-008,
/// format of `KRN.MAN.CARD`). [`FitError::MethodUndefined`] when the method is not defined
/// for the primitive.
pub fn algorithm_id(primitive: Primitive, method: Method, version: (u32, u32, u32)) -> Result<AlgorithmId, FitError> {
    if !method.defined_for(primitive) {
        return Err(FitError::MethodUndefined { primitive, method });
    }
    Ok(AlgorithmId {
        module: "fit".to_string(),
        feature: primitive.slug().to_string(),
        method: method.slug().to_string(),
        version,
    })
}

/// A fitted feature in SI metres and radians. Directions are unit vectors; see
/// [`FittedFeature::canonical`] for the normalisation rules (REQ-KRN.LSQ.TRAIT-007). Points on
/// axes (`Line2`, `Line3`, `Cylinder`) are the foot of the perpendicular from the centroid of
/// the fitted data ([`foot_of_perpendicular`]).
#[derive(Debug, Clone, PartialEq)]
pub enum FittedFeature {
    Point {
        c: V3,
    },
    Line2 {
        p: V2,
        d: V2,
    },
    Line3 {
        p: V3,
        d: V3,
    },
    Plane {
        p: V3,
        n: V3,
    },
    Circle2 {
        c: V2,
        r: f64,
    },
    Circle3 {
        c: V3,
        n: V3,
        r: f64,
    },
    Sphere {
        c: V3,
        r: f64,
    },
    Cylinder {
        p: V3,
        d: V3,
        r: f64,
    },
    /// `d` points from the apex into the cone (it carries orientation and is not
    /// sign-normalised); `half_angle` in radians.
    Cone {
        apex: V3,
        d: V3,
        half_angle: f64,
    },
    Torus {
        c: V3,
        n: V3,
        major_r: f64,
        minor_r: f64,
    },
    /// Centre, plane normal, long-axis direction, centre-to-centre length, width.
    Slot {
        c: V3,
        n: V3,
        d: V3,
        length: f64,
        width: f64,
    },
    /// Centre, plane normal, long-side direction, length, width.
    Rectangle {
        c: V3,
        n: V3,
        d: V3,
        length: f64,
        width: f64,
    },
    /// Centre, plane normal, major-axis direction, semi-axes `a >= b`.
    Ellipse {
        c: V3,
        n: V3,
        d: V3,
        a: f64,
        b: f64,
    },
    /// `d` points from the apex into the paraboloid (orientation, not sign-normalised);
    /// `focal` is the focal length.
    Paraboloid {
        apex: V3,
        d: V3,
        focal: f64,
    },
    Polyline {
        vertices: Vec<V3>,
    },
    Spline {
        degree: u32,
        knots: Vec<f64>,
        control: Vec<V3>,
    },
}

impl FittedFeature {
    /// The primitive of this feature.
    pub const fn primitive(&self) -> Primitive {
        match self {
            FittedFeature::Point { .. } => Primitive::Point,
            FittedFeature::Line2 { .. } => Primitive::Line2,
            FittedFeature::Line3 { .. } => Primitive::Line3,
            FittedFeature::Plane { .. } => Primitive::Plane,
            FittedFeature::Circle2 { .. } => Primitive::Circle2,
            FittedFeature::Circle3 { .. } => Primitive::Circle3,
            FittedFeature::Sphere { .. } => Primitive::Sphere,
            FittedFeature::Cylinder { .. } => Primitive::Cylinder,
            FittedFeature::Cone { .. } => Primitive::Cone,
            FittedFeature::Torus { .. } => Primitive::Torus,
            FittedFeature::Slot { .. } => Primitive::Slot,
            FittedFeature::Rectangle { .. } => Primitive::Rectangle,
            FittedFeature::Ellipse { .. } => Primitive::Ellipse,
            FittedFeature::Paraboloid { .. } => Primitive::Paraboloid,
            FittedFeature::Polyline { .. } => Primitive::Polyline,
            FittedFeature::Spline { .. } => Primitive::Spline,
        }
    }

    /// A copy with every direction normalised so that equal geometry gives equal parameters
    /// (REQ-KRN.LSQ.TRAIT-007): line directions, plane and circle normals, cylinder axes,
    /// torus normals and the planar shapes' normals and long-axis directions go through
    /// [`normalize_direction`] (unit length, sign-normalised); the cone and paraboloid axes,
    /// whose sign is part of the geometry, through [`unit_direction`] (unit length only).
    /// Every value must be finite ([`FitError::Num`] otherwise). Out-of-range values are
    /// [`FitError::InvalidParams`] naming the field (`feature.<name>`): a radius `r`, torus
    /// `major_r` or `minor_r`, ellipse `b`, rectangle `width` or paraboloid `focal` that is not
    /// strictly positive; a negative slot `length` or `width` (`-0.0` becomes `+0.0`); an
    /// ellipse with `a < b` or a rectangle with `length < width`; a cone `half_angle` outside the open interval `(0, pi/2)`; a spline with
    /// `degree == 0`, fewer than `degree + 1` control points, or a knot count other than
    /// `control + degree + 1` (checked arithmetic, so an overflowing degree is refused). Values are copied unchanged otherwise.
    pub fn canonical(&self) -> Result<FittedFeature, FitError> {
        use FittedFeature as F;
        let nd = normalize_direction;
        let ud = unit_direction;
        Ok(match self {
            F::Point { c } => F::Point { c: fin3(c)? },
            F::Line2 { p, d } => F::Line2 { p: fin2(p)?, d: normalize_direction2(*d)? },
            F::Line3 { p, d } => F::Line3 { p: fin3(p)?, d: nd(*d)? },
            F::Plane { p, n } => F::Plane { p: fin3(p)?, n: nd(*n)? },
            F::Circle2 { c, r } => F::Circle2 { c: fin2(c)?, r: pos("feature.r", *r)? },
            F::Circle3 { c, n, r } => F::Circle3 { c: fin3(c)?, n: nd(*n)?, r: pos("feature.r", *r)? },
            F::Sphere { c, r } => F::Sphere { c: fin3(c)?, r: pos("feature.r", *r)? },
            F::Cylinder { p, d, r } => F::Cylinder { p: fin3(p)?, d: nd(*d)?, r: pos("feature.r", *r)? },
            F::Cone { apex, d, half_angle } => {
                let h = fin(*half_angle)?;
                if !(h > 0.0 && h < core::f64::consts::FRAC_PI_2) {
                    return Err(FitError::InvalidParams { field: "feature.half_angle" });
                }
                F::Cone { apex: fin3(apex)?, d: ud(*d)?, half_angle: h }
            }
            F::Torus { c, n, major_r, minor_r } => F::Torus {
                c: fin3(c)?,
                n: nd(*n)?,
                major_r: pos("feature.major_r", *major_r)?,
                minor_r: pos("feature.minor_r", *minor_r)?,
            },
            F::Slot { c, n, d, length, width } => F::Slot {
                c: fin3(c)?,
                n: nd(*n)?,
                d: nd(*d)?,
                length: nonneg("feature.length", *length)?,
                width: nonneg("feature.width", *width)?,
            },
            F::Rectangle { c, n, d, length, width } => {
                let width = pos("feature.width", *width)?;
                let length = fin(*length)?;
                if length < width {
                    return Err(FitError::InvalidParams { field: "feature.length" });
                }
                F::Rectangle { c: fin3(c)?, n: nd(*n)?, d: nd(*d)?, length, width }
            }
            F::Ellipse { c, n, d, a, b } => {
                let b = pos("feature.b", *b)?;
                let a = fin(*a)?;
                if a < b {
                    return Err(FitError::InvalidParams { field: "feature.a" });
                }
                F::Ellipse { c: fin3(c)?, n: nd(*n)?, d: nd(*d)?, a, b }
            }
            F::Paraboloid { apex, d, focal } => {
                F::Paraboloid { apex: fin3(apex)?, d: ud(*d)?, focal: pos("feature.focal", *focal)? }
            }
            F::Polyline { vertices } => F::Polyline { vertices: copy_points(vertices)? },
            F::Spline { degree, knots, control } => {
                check_finite_slice("fit.feature", knots)?;
                spline_counts(*degree, knots.len(), control.len())?;
                let mut k = alloc("fit.feature", knots.len())?;
                k.extend_from_slice(knots);
                F::Spline { degree: *degree, knots: k, control: copy_points(control)? }
            }
        })
    }
}

fn fin(x: f64) -> Result<f64, FitError> {
    Ok(fid_num::check_finite("fit.feature", x)?)
}

/// Finite and strictly positive, else [`FitError::InvalidParams`] naming `field`.
fn pos(field: &'static str, x: f64) -> Result<f64, FitError> {
    let x = fin(x)?;
    if x > 0.0 {
        Ok(x)
    } else {
        Err(FitError::InvalidParams { field })
    }
}

/// Finite and not negative, else [`FitError::InvalidParams`]. Returns `x + 0.0`, so `-0.0`
/// becomes `+0.0` and both zeros encode identically.
fn nonneg(field: &'static str, x: f64) -> Result<f64, FitError> {
    let x = fin(x)?;
    if x >= 0.0 {
        Ok(x + 0.0)
    } else {
        Err(FitError::InvalidParams { field })
    }
}

/// Counts of a B-spline of degree `p`: `p >= 1`, at least `p + 1` control points `m`, and
/// exactly `m + p + 1` knots. (Knot order is the fitting method's concern.)
fn spline_counts(degree: u32, knots: usize, control: usize) -> Result<(), FitError> {
    let bad = |field| Err(FitError::InvalidParams { field });
    if degree == 0 {
        return bad("feature.degree");
    }
    // Checked arithmetic: a degree near `u32::MAX` must not overflow (on 32-bit targets
    // `usize` is 32 bits), it is refused.
    let Some(order) = usize::try_from(degree).ok().and_then(|p| p.checked_add(1)) else {
        return bad("feature.degree");
    };
    if control < order {
        return bad("feature.control");
    }
    match control.checked_add(order) {
        Some(k) if k == knots => Ok(()),
        _ => bad("feature.knots"),
    }
}

fn fin2(v: &V2) -> Result<V2, FitError> {
    check_finite_slice("fit.feature", v)?;
    Ok(*v)
}

fn fin3(v: &V3) -> Result<V3, FitError> {
    check_finite_slice("fit.feature", v)?;
    Ok(*v)
}

fn copy_points(src: &[V3]) -> Result<Vec<V3>, FitError> {
    let mut out = alloc("fit.feature", src.len())?;
    for v in src {
        out.push(fin3(v)?);
    }
    Ok(out)
}

/// Divides by `red::norm2`; [`FitError::InvalidDirection`] when the norm is below `2^-1022`
/// (zero included), [`FitError::Num`] for a non-finite component or an overflowing norm.
fn unit<const K: usize>(d: [f64; K]) -> Result<[f64; K], FitError> {
    check_finite_slice("fit.direction", &d)?;
    let len = red::norm2(&d)?;
    if len < f64::MIN_POSITIVE {
        return Err(FitError::InvalidDirection);
    }
    let mut u = d;
    for x in u.iter_mut() {
        *x /= len;
    }
    Ok(u)
}

/// The sign rule: if the first component that is not `±0.0` is negative, every component is
/// negated; then every component `x` becomes `x + 0.0`, which turns `-0.0` into `+0.0` and
/// leaves every other value unchanged.
fn sign_normalize<const K: usize>(mut u: [f64; K]) -> [f64; K] {
    if let Some(first) = u.iter().copied().find(|x| *x != 0.0) {
        if first < 0.0 {
            for x in u.iter_mut() {
                *x = -*x;
            }
        }
    }
    for x in u.iter_mut() {
        *x += 0.0;
    }
    u
}

/// Unit length, sign-normalised 3D direction (REQ-KRN.LSQ.TRAIT-007): `d / red::norm2(d)`
/// (one division per component), then the first component that is not `±0.0` is made
/// positive by negating all components, then `-0.0` components become `+0.0`. So `(0, 0, -1)`
/// and `(0, 0, 1)` both give `(+0.0, +0.0, 1.0)` bit for bit.
pub fn normalize_direction(d: V3) -> Result<V3, FitError> {
    Ok(sign_normalize(unit(d)?))
}

/// The 2D form of [`normalize_direction`].
pub fn normalize_direction2(d: V2) -> Result<V2, FitError> {
    Ok(sign_normalize(unit(d)?))
}

/// Unit length only (for axes whose sign is part of the geometry): `d / red::norm2(d)`, then
/// `-0.0` components become `+0.0`.
pub fn unit_direction(d: V3) -> Result<V3, FitError> {
    let mut u = unit(d)?;
    for x in u.iter_mut() {
        *x += 0.0;
    }
    Ok(u)
}

/// The foot of the perpendicular from `centroid` onto the line through `p` with direction
/// `d` (normalised with [`normalize_direction`] first): `p + t * u` with
/// `t = red::dot(centroid - p, u)`, each component one product and one sum (no FMA).
pub fn foot_of_perpendicular(centroid: V3, p: V3, d: V3) -> Result<V3, FitError> {
    check_finite_slice("fit.foot", &centroid)?;
    check_finite_slice("fit.foot", &p)?;
    let u = normalize_direction(d)?;
    let w = [centroid[0] - p[0], centroid[1] - p[1], centroid[2] - p[2]];
    let t = red::dot(&w, &u)?;
    let out = [p[0] + t * u[0], p[1] + t * u[1], p[2] + t * u[2]];
    check_finite_slice("fit.foot", &out)?;
    Ok(out)
}
