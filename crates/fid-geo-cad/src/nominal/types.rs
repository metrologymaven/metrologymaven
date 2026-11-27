//! Nominal element types (spec node `GEO.CAD.NOMINAL.DIRECT`, Scope).
//!
//! A [`NominalElement`] is built only inside this crate (by [`super::direct`] today, by the
//! NURBS recognition leaf later), so an element that says [`Method::Direct`] always carries
//! parameters copied from an analytic CAD definition and `max_deviation_m == 0`; it is never a
//! fit (REQ-GEO.CAD.NOMINAL-001).

use crate::topo::{Frame, TopologyId};

/// The kind of a nominal element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NominalKind {
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
    Line,
    Circle,
    Point,
}

impl NominalKind {
    /// Stable lowercase name: `plane`, `cylinder`, `cone`, `sphere`, `torus`, `line`, `circle`,
    /// `point`.
    pub fn as_str(self) -> &'static str {
        match self {
            NominalKind::Plane => "plane",
            NominalKind::Cylinder => "cylinder",
            NominalKind::Cone => "cone",
            NominalKind::Sphere => "sphere",
            NominalKind::Torus => "torus",
            NominalKind::Line => "line",
            NominalKind::Circle => "circle",
            NominalKind::Point => "point",
        }
    }
}

/// Placement and size of a nominal element, in the body's frame and metres (angles in
/// radians). The placement is the CAD definition's given [`Frame`] (origin, axis, reference
/// direction) and the sizes its given values, bit for bit (after the topology model's `-0.0` to
/// `+0.0`); the shapes and parameterisations are those of GEO.NURBS.ANALYTIC (a line runs along
/// the frame's reference direction, a circle lies in the plane normal to the frame's axis).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NominalParams {
    Plane {
        frame: Frame,
    },
    Cylinder {
        frame: Frame,
        radius: f64,
    },
    /// `radius` is the radius at the placement origin, `half_angle` in `(0, pi/2)`.
    Cone {
        frame: Frame,
        radius: f64,
        half_angle: f64,
    },
    Sphere {
        frame: Frame,
        radius: f64,
    },
    Torus {
        frame: Frame,
        major_radius: f64,
        minor_radius: f64,
    },
    Line {
        frame: Frame,
    },
    Circle {
        frame: Frame,
        radius: f64,
    },
    Point {
        point: [f64; 3],
    },
}

impl NominalParams {
    /// The element kind these parameters describe.
    pub fn kind(&self) -> NominalKind {
        match self {
            NominalParams::Plane { .. } => NominalKind::Plane,
            NominalParams::Cylinder { .. } => NominalKind::Cylinder,
            NominalParams::Cone { .. } => NominalKind::Cone,
            NominalParams::Sphere { .. } => NominalKind::Sphere,
            NominalParams::Torus { .. } => NominalKind::Torus,
            NominalParams::Line { .. } => NominalKind::Line,
            NominalParams::Circle { .. } => NominalKind::Circle,
            NominalParams::Point { .. } => NominalKind::Point,
        }
    }

    /// The placement, or `None` for a point.
    pub fn frame(&self) -> Option<&Frame> {
        match self {
            NominalParams::Plane { frame }
            | NominalParams::Cylinder { frame, .. }
            | NominalParams::Cone { frame, .. }
            | NominalParams::Sphere { frame, .. }
            | NominalParams::Torus { frame, .. }
            | NominalParams::Line { frame }
            | NominalParams::Circle { frame, .. } => Some(frame),
            NominalParams::Point { .. } => None,
        }
    }
}

/// The part of the element's surface or curve that the CAD entity covers, in the element's
/// own placement (local coordinates `x = (q - O) . X`, `y = (q - O) . Y`, `z = (q - O) . Z`
/// of the orthonormalised frame).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Extent {
    /// Cylinder and cone faces: the range of `z` (metres along the axis). Line edges: the
    /// edge's parameter range, metres along the line from its origin.
    Axial { h_min: f64, h_max: f64 },
    /// Circle edges: the edge's parameter range in radians (`a0 < a1`, as recorded).
    Angular { a0: f64, a1: f64 },
    /// Plane faces: the box `[u0, u1] x [v0, v1]` of the in-plane coordinates `x`, `y`.
    Planar { u: [f64; 2], v: [f64; 2] },
    /// A plane, cylinder or cone face without a boundary loop: the whole, unbounded surface.
    Unbounded,
    /// No extent is defined for this kind (sphere and torus faces, points): the element's
    /// sources bound it. Never an approximation of a bounded region.
    NotDefined,
}

/// Which side of a face the material is on (REQ-003), from the face sense and the surface's
/// natural normal `Su x Sv`. That normal points to the outer side of every analytic surface:
/// `+Z` of a plane's placement, away from the axis of a cylinder or cone (on both nappes),
/// away from the centre of a sphere and away from the tube circle of a torus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Material {
    /// The out-of-material normal points to the outer side: a shaft, a boss, a ball, a convex
    /// round; for a plane, the placement axis `+Z` points out of the material.
    Outside,
    /// The out-of-material normal points to the inner side: a hole, a pocket, a socket, a
    /// concave fillet; for a plane, `+Z` points into the material.
    Inside,
}

/// How an element was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Method {
    /// Copied from an analytic CAD definition (this leaf): deviation zero by construction.
    Direct,
    /// Recognised from NURBS within a tolerance (`GEO.CAD.NOMINAL.RECOG`, not built here).
    Recognised,
}

/// A typed nominal element derived from one or more CAD entities.
#[derive(Debug, Clone, PartialEq)]
pub struct NominalElement {
    params: NominalParams,
    extent: Extent,
    material: Option<Material>,
    sources: Vec<TopologyId>,
    method: Method,
    max_deviation_m: f64,
}

impl NominalElement {
    /// A directly mapped element: `method = Direct`, `max_deviation_m = 0`.
    pub(crate) fn direct(
        params: NominalParams,
        extent: Extent,
        material: Option<Material>,
        source: TopologyId,
    ) -> NominalElement {
        NominalElement { params, extent, material, sources: vec![source], method: Method::Direct, max_deviation_m: 0.0 }
    }

    /// The element kind.
    pub fn kind(&self) -> NominalKind {
        self.params.kind()
    }

    /// Placement and sizes.
    pub fn params(&self) -> &NominalParams {
        &self.params
    }

    /// The covered part of the element.
    pub fn extent(&self) -> &Extent {
        &self.extent
    }

    /// Material side of a face element; `None` for elements from edges and vertices.
    pub fn material(&self) -> Option<Material> {
        self.material
    }

    /// Stable topology IDs of the CAD entities the element comes from.
    pub fn sources(&self) -> &[TopologyId] {
        &self.sources
    }

    pub fn method(&self) -> Method {
        self.method
    }

    /// Largest distance between the element and its source geometry; `0` for `Direct`.
    pub fn max_deviation_m(&self) -> f64 {
        self.max_deviation_m
    }

    /// For a plane face element, the unit normal pointing out of the material: the given
    /// placement axis for [`Material::Outside`], its exact negation (computed as `0 - x`, so a
    /// zero component stays `+0.0`) for [`Material::Inside`].
    /// `None` for every other element.
    pub fn plane_normal_out_of_material(&self) -> Option<[f64; 3]> {
        match (&self.params, self.material) {
            (NominalParams::Plane { frame }, Some(Material::Outside)) => Some(frame.axis),
            (NominalParams::Plane { frame }, Some(Material::Inside)) => {
                Some([0.0 - frame.axis[0], 0.0 - frame.axis[1], 0.0 - frame.axis[2]])
            }
            _ => None,
        }
    }
}

/// An analytic CAD kind that has no nominal element type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnsupportedKind {
    Ellipse,
}

impl UnsupportedKind {
    /// Stable lowercase name.
    pub fn as_str(self) -> &'static str {
        match self {
            UnsupportedKind::Ellipse => "ellipse",
        }
    }
}

/// The outcome of mapping one face, edge or vertex (REQ-004: never an approximate element).
#[derive(Debug, Clone, PartialEq)]
pub enum DirectResult {
    Element(NominalElement),
    /// The geometry is NURBS (recognition is `GEO.CAD.NOMINAL.RECOG`).
    NotAnalytic,
    /// The geometry is analytic but of a kind with no nominal type.
    UnsupportedKind {
        kind: UnsupportedKind,
    },
}
