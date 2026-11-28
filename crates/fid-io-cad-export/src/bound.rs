//! Export plan and extents for unbounded elements. Spec node `IO.CAD.EXPORT.BOUND`.
//!
//! [`plan`] takes the selected elements in selection order, each with its frame, optional
//! fitted points and optional caller extent, and returns an [`ExportPlan`]: every element
//! moved into the target frame, with the extent used and where it came from. Both CAD writers
//! consume the same plan, so they export the same bounded geometry.
//!
//! # Extents (REQ-001 to REQ-003)
//!
//! | element | extent | parameter (in the element's own placement) |
//! |---|---|---|
//! | line | [`Extent::Line`]`(t0, t1)` | `t = (q - O) . X` |
//! | plane | [`Extent::Rect`]`(u0, u1, v0, v1)` | `u = (q - O) . X`, `v = (q - O) . Y` |
//! | cylinder, cone | [`Extent::Axial`]`(h0, h1)` | `h = (q - O) . Z`, full `2 pi` sweep |
//!
//! `O`, `X`, `Y`, `Z` are the stored (orthonormalised) placement vectors of
//! `fid_geo_nurbs::Placement`, and the projection is `Placement::to_local`: `d = q - O`
//! component-wise, then `(d0 a0 + d1 a1) + d2 a2`. Points, circles, ellipses, spheres, tori and
//! NURBS curves and surfaces are already bounded ([`Bounds::Intrinsic`]).
//!
//! - A caller extent is used as given ([`ExtentSource::Caller`]): every value finite and each
//!   interval non-empty (`lo < hi`), of the element's [`ExtentKind`].
//! - Otherwise the element's fitted points give it ([`ExtentSource::FittedPoints`], with the
//!   point count and the margin): per parameter the minimum and the maximum in IEEE 754
//!   total order (`f64::total_cmp`, first point first), in the element's own frame; then, in
//!   the target frame, `lo = s min - margin` and `hi = s max + margin` (`s` the transform's
//!   scale). The margin is in metres of the target frame, so the reported margin is exactly
//!   the widening exported.
//! - A zero-width target extent (`lo >= hi`: every point projecting to one value with no
//!   margin, or a scale that rounds both ends of an interval together) is refused as
//!   [`PlanError::DegenerateExtent`], never exported as a sliver.
//! - Neither: [`PlanError::ExtentRequired`] (REQ-003).
//!
//! Inputs that would not be used are refused, never ignored: a caller extent on a bounded
//! element ([`PlanError::ExtentNotApplicable`]), an extent of the wrong kind
//! ([`PlanError::ExtentKindMismatch`]), and fitted points on a bounded element or next to a
//! caller extent ([`PlanError::PointsNotApplicable`]). An empty point list is
//! [`PlanError::EmptyPoints`], a NaN or infinite point [`PlanError::NonFinitePoint`].
//! A cone extent whose interior holds the apex (the generator radius `r0 + h tan a` of the
//! moved cone changes sign strictly inside the target extent, `tan` from `fid-num`) would join
//! both nappes and is refused as [`PlanError::ExtentCrossesApex`]; an apex at an end of the
//! interval is accepted.
//!
//! Caller extents and fitted projections are taken in the element's own frame and multiplied
//! by the transform's scale (exactly 1 for a rigid transform, so a rigid move leaves a caller
//! extent bit for bit unchanged); the margin is then added in the target frame. Every
//! reported zero is `+0.0` (`v + 0.0`), so an extent has one representation.
//!
//! # Transform into the target frame (REQ-004)
//!
//! The transform is `FrameTree::resolve(element frame, target)` (GEO.FRAME.TREE's pinned
//! chain), resolved once per distinct frame. With XFORM's `apply_point` (`s R x + t`) and
//! `apply_dir` (`R x`):
//!
//! - a point, a placement origin and every NURBS control point: `apply_point` (NURBS weights,
//!   knots and degrees unchanged; an affine map of the control points is exact for rational
//!   B-splines);
//! - a placement's axis `Z` and reference direction `X`: `z1 = apply_dir(Z)`,
//!   `x1 = apply_dir(X)`; `z = z1 / |z1|`; `x' = x1 - (x1 . z) z`; `x = x' / |x'|` (norms are
//!   the correctly rounded `sqrt` of the pinned dot product, each component divided), then
//!   `Placement::new(origin, z, x)`, which orthonormalises once more. Renormalising first
//!   keeps a placement accepted at GEO.NURBS's `1e-12` edge valid after a rotation accepted at
//!   XFORM's `1e-12` residual, as GEO.FRAME.APPLY does for normals;
//! - radii and semi-axes: `s r` (the cone's half-angle is unchanged).
//!
//! Without the renormalisation the change of every defining parameter is the transform's own
//! rounding; the renormalisation moves a unit vector by a few ulps (about `1e-16` rad), far
//! below REQ-004's `1e-12`.
//!
//! # Errors and order
//!
//! First failure wins. Before any element: the target frame must be in the tree
//! ([`PlanError::FrameNotFound`]), the margin finite and `>= 0` ([`PlanError::BadMargin`]),
//! the selection non-empty ([`PlanError::EmptySelection`]) and at most [`MAX_ELEMENTS`]
//! long. Then per element in selection order: name ([`PlanError::BadName`]), duplicate ID
//! ([`PlanError::DuplicateElement`]), frame resolution, the input extent rules above, the
//! transform ([`PlanError::Transform`]), then the target extent (overflow, zero width, apex),
//! in the order listed in [`plan`].
//!
//! # Determinism
//!
//! D0: IEEE 754 binary64 `+ - * /` one operation at a time in the written order (Rust never
//! fuses), `sqrt` correctly rounded, `tan` only for the apex decision, `BTreeMap` only. The
//! independent oracle `fixtures/io/cad/export/bound/make_cases.py` reproduces every output bit.
//!
//! Element names are untrusted text: kept as [`UntrustedName`], never interpreted; a name
//! holding a control, format (bidi, BOM, zero-width) or other forbidden character
//! (`fid_types::text::is_forbidden_char`) is refused.

use core::cmp::Ordering;
use core::fmt;
use std::collections::BTreeMap;

use fid_geo_frame::{FrameError, FrameId, FrameTree, Transform};
use fid_geo_nurbs::{
    AnalyticError, Circle, Cone, Cylinder, Ellipse, Line, NurbsCurve, NurbsError, NurbsSurface, Placement, Plane,
    Sphere, Torus,
};

/// An element's ID: the project-tree UUID (the same type as `fid_dep::EntityId`).
pub type ElementId = fid_uuid::Uuid;

/// Largest number of elements in one plan.
pub const MAX_ELEMENTS: usize = 1 << 20;

/// Largest element name, in UTF-8 bytes.
pub const MAX_NAME_BYTES: usize = 255;

// ---------------------------------------------------------------- geometry

/// A geometric element to export, in its own frame (input) or in the target frame (plan).
#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    /// A point (metres).
    Point([f64; 3]),
    /// An unbounded line `O + t X`.
    Line(Line),
    /// An unbounded plane `O + u X + v Y`.
    Plane(Plane),
    /// A cylinder, unbounded along its axis.
    Cylinder(Cylinder),
    /// A cone (both nappes), unbounded along its axis.
    Cone(Cone),
    /// A full circle.
    Circle(Circle),
    /// A full ellipse.
    Ellipse(Ellipse),
    /// A sphere.
    Sphere(Sphere),
    /// A torus.
    Torus(Torus),
    /// A B-spline or NURBS curve over its own domain.
    NurbsCurve(NurbsCurve),
    /// A B-spline or NURBS surface over its own domain.
    NurbsSurface(NurbsSurface),
}

/// The kind of a [`Geometry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GeomKind {
    /// [`Geometry::Point`].
    Point,
    /// [`Geometry::Line`].
    Line,
    /// [`Geometry::Plane`].
    Plane,
    /// [`Geometry::Cylinder`].
    Cylinder,
    /// [`Geometry::Cone`].
    Cone,
    /// [`Geometry::Circle`].
    Circle,
    /// [`Geometry::Ellipse`].
    Ellipse,
    /// [`Geometry::Sphere`].
    Sphere,
    /// [`Geometry::Torus`].
    Torus,
    /// [`Geometry::NurbsCurve`].
    NurbsCurve,
    /// [`Geometry::NurbsSurface`].
    NurbsSurface,
}

impl GeomKind {
    /// Stable lowercase name, e.g. `cylinder`, `nurbs_curve`.
    pub fn as_str(self) -> &'static str {
        match self {
            GeomKind::Point => "point",
            GeomKind::Line => "line",
            GeomKind::Plane => "plane",
            GeomKind::Cylinder => "cylinder",
            GeomKind::Cone => "cone",
            GeomKind::Circle => "circle",
            GeomKind::Ellipse => "ellipse",
            GeomKind::Sphere => "sphere",
            GeomKind::Torus => "torus",
            GeomKind::NurbsCurve => "nurbs_curve",
            GeomKind::NurbsSurface => "nurbs_surface",
        }
    }

    /// The extent an unbounded kind needs, or `None` for a bounded kind.
    pub fn extent_kind(self) -> Option<ExtentKind> {
        match self {
            GeomKind::Line => Some(ExtentKind::Line),
            GeomKind::Plane => Some(ExtentKind::Rect),
            GeomKind::Cylinder | GeomKind::Cone => Some(ExtentKind::Axial),
            _ => None,
        }
    }
}

impl Geometry {
    /// The kind.
    pub fn kind(&self) -> GeomKind {
        match self {
            Geometry::Point(_) => GeomKind::Point,
            Geometry::Line(_) => GeomKind::Line,
            Geometry::Plane(_) => GeomKind::Plane,
            Geometry::Cylinder(_) => GeomKind::Cylinder,
            Geometry::Cone(_) => GeomKind::Cone,
            Geometry::Circle(_) => GeomKind::Circle,
            Geometry::Ellipse(_) => GeomKind::Ellipse,
            Geometry::Sphere(_) => GeomKind::Sphere,
            Geometry::Torus(_) => GeomKind::Torus,
            Geometry::NurbsCurve(_) => GeomKind::NurbsCurve,
            Geometry::NurbsSurface(_) => GeomKind::NurbsSurface,
        }
    }

    /// The placement whose axes parameterise the extent of an unbounded kind.
    fn extent_placement(&self) -> Option<&Placement> {
        match self {
            Geometry::Line(g) => Some(g.placement()),
            Geometry::Plane(g) => Some(g.placement()),
            Geometry::Cylinder(g) => Some(g.placement()),
            Geometry::Cone(g) => Some(g.placement()),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------- extents

/// Which extent an unbounded element takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExtentKind {
    /// A parameter interval along a line.
    Line,
    /// A rectangle in a plane's own axes.
    Rect,
    /// An axial interval of a cylinder or cone (full sweep).
    Axial,
}

impl ExtentKind {
    /// Stable name: `line`, `rect`, `axial`.
    pub fn as_str(self) -> &'static str {
        match self {
            ExtentKind::Line => "line",
            ExtentKind::Rect => "rect",
            ExtentKind::Axial => "axial",
        }
    }

    /// The parameter names, in the order of the extent's intervals.
    fn axes(self) -> &'static [&'static str] {
        match self {
            ExtentKind::Line => &["t"],
            ExtentKind::Rect => &["u", "v"],
            ExtentKind::Axial => &["h"],
        }
    }
}

/// A finite extent in metres, in the element's placement parameters (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Extent {
    /// `t0 <= t <= t1` along a line.
    Line(f64, f64),
    /// `u0 <= u <= u1`, `v0 <= v <= v1` in a plane's own axes.
    Rect(f64, f64, f64, f64),
    /// `h0 <= h <= h1` along a cylinder's or cone's axis.
    Axial(f64, f64),
}

impl Extent {
    /// The kind.
    pub fn kind(&self) -> ExtentKind {
        match self {
            Extent::Line(..) => ExtentKind::Line,
            Extent::Rect(..) => ExtentKind::Rect,
            Extent::Axial(..) => ExtentKind::Axial,
        }
    }

    /// The intervals `(lo, hi)` in axis order (one, or two for a rectangle).
    pub fn intervals(&self) -> Vec<(f64, f64)> {
        match *self {
            Extent::Line(a, b) | Extent::Axial(a, b) => vec![(a, b)],
            Extent::Rect(u0, u1, v0, v1) => vec![(u0, u1), (v0, v1)],
        }
    }

    fn from_intervals(kind: ExtentKind, iv: &[(f64, f64)]) -> Option<Extent> {
        match (kind, iv) {
            (ExtentKind::Line, [(a, b)]) => Some(Extent::Line(*a, *b)),
            (ExtentKind::Axial, [(a, b)]) => Some(Extent::Axial(*a, *b)),
            (ExtentKind::Rect, [(u0, u1), (v0, v1)]) => Some(Extent::Rect(*u0, *u1, *v0, *v1)),
            _ => None,
        }
    }
}

/// Where an extent came from (the report entry of REQ-001 and REQ-002).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExtentSource {
    /// Given by the caller, used as given.
    Caller,
    /// The min/max projection of the element's fitted points, widened by the margin.
    FittedPoints {
        /// How many points were projected.
        points: u64,
        /// The margin added on each side, in metres of the target frame (exactly the
        /// widening exported).
        margin_m: f64,
    },
}

impl ExtentSource {
    /// Stable code: `caller` or `fitted_points`.
    pub fn code(&self) -> &'static str {
        match self {
            ExtentSource::Caller => "caller",
            ExtentSource::FittedPoints { .. } => "fitted_points",
        }
    }
}

/// How a planned element is bounded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Bounds {
    /// The element is bounded by its own definition (point, circle, ellipse, sphere, torus,
    /// NURBS curve or surface).
    Intrinsic,
    /// An unbounded element with the extent used, in the target frame.
    Extent {
        /// The extent.
        extent: Extent,
        /// Where it came from.
        source: ExtentSource,
    },
}

// ---------------------------------------------------------------- names

/// Why an element name was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameFault {
    /// The name is empty.
    Empty,
    /// The name is longer than [`MAX_NAME_BYTES`].
    TooLong {
        /// Its length in bytes.
        bytes: usize,
    },
    /// The name holds a forbidden code point (control, format such as bidi or the BOM,
    /// separator, noncharacter or tag).
    ForbiddenChar {
        /// Byte offset of the first one.
        offset: usize,
    },
}

impl NameFault {
    /// Stable code: `empty`, `too_long`, `forbidden_char`.
    pub fn code(&self) -> &'static str {
        match self {
            NameFault::Empty => "empty",
            NameFault::TooLong { .. } => "too_long",
            NameFault::ForbiddenChar { .. } => "forbidden_char",
        }
    }
}

/// An element name: untrusted text the writers copy into entity names or labels. Never
/// interpreted; validated at construction (non-empty, at most [`MAX_NAME_BYTES`], no
/// forbidden code point).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UntrustedName(Box<str>);

impl UntrustedName {
    /// Validates a name.
    pub fn new(text: &str) -> Result<UntrustedName, NameFault> {
        if text.is_empty() {
            return Err(NameFault::Empty);
        }
        if text.len() > MAX_NAME_BYTES {
            return Err(NameFault::TooLong { bytes: text.len() });
        }
        if let Some((offset, _)) = text.char_indices().find(|(_, c)| fid_types::text::is_forbidden_char(*c)) {
            return Err(NameFault::ForbiddenChar { offset });
        }
        Ok(UntrustedName(Box::from(text)))
    }

    /// The raw untrusted text, for a writer's entity name. Data, never an instruction.
    pub fn as_untrusted_str(&self) -> &str {
        &self.0
    }

    /// Length in bytes (UTF-8); never zero.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Always false: an empty name is refused at construction.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

// ---------------------------------------------------------------- input and plan

/// One selected element.
#[derive(Debug, Clone, Copy)]
pub struct ElementInput<'a> {
    /// The element's ID.
    pub element: ElementId,
    /// The element's name (untrusted).
    pub name: &'a str,
    /// The frame the geometry and the points are expressed in.
    pub frame: FrameId,
    /// The geometry, in `frame`.
    pub geom: &'a Geometry,
    /// The element's fitted points in `frame` (metres), or `None` (for example a constructed
    /// element). Only for an unbounded element without a caller extent.
    pub points: Option<&'a [[f64; 3]]>,
    /// The caller's extent (metres, in the element's own frame), or `None`. Only for an
    /// unbounded element.
    pub extent: Option<Extent>,
}

/// One planned element, in the target frame.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanItem {
    element: ElementId,
    name: UntrustedName,
    source_frame: FrameId,
    geom: Geometry,
    bounds: Bounds,
}

impl PlanItem {
    /// The element's ID.
    pub fn element(&self) -> ElementId {
        self.element
    }

    /// The element's name.
    pub fn name(&self) -> &UntrustedName {
        &self.name
    }

    /// The frame the element was given in.
    pub fn source_frame(&self) -> FrameId {
        self.source_frame
    }

    /// The kind.
    pub fn kind(&self) -> GeomKind {
        self.geom.kind()
    }

    /// The geometry in the target frame.
    pub fn geom(&self) -> &Geometry {
        &self.geom
    }

    /// How it is bounded.
    pub fn bounds(&self) -> Bounds {
        self.bounds
    }

    /// The extent used, or `None` for an intrinsically bounded element.
    pub fn extent(&self) -> Option<Extent> {
        match self.bounds {
            Bounds::Intrinsic => None,
            Bounds::Extent { extent, .. } => Some(extent),
        }
    }

    /// Where the extent came from, or `None` for an intrinsically bounded element.
    pub fn extent_source(&self) -> Option<ExtentSource> {
        match self.bounds {
            Bounds::Intrinsic => None,
            Bounds::Extent { source, .. } => Some(source),
        }
    }
}

/// The export plan: every selected element, in selection order, in one target frame.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportPlan {
    frame: FrameId,
    margin_m: f64,
    items: Vec<PlanItem>,
}

impl ExportPlan {
    /// The target frame.
    pub fn frame(&self) -> FrameId {
        self.frame
    }

    /// The margin applied to fitted-point extents (metres).
    pub fn margin_m(&self) -> f64 {
        self.margin_m
    }

    /// The planned elements, in selection order.
    pub fn items(&self) -> &[PlanItem] {
        &self.items
    }
}

// ---------------------------------------------------------------- errors

/// Why fitted points were not applicable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointsUnused {
    /// The element is bounded and takes no extent.
    Bounded,
    /// The caller gave an extent, which wins.
    CallerExtent,
}

/// What is wrong with an extent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtentFault {
    /// A value is NaN or infinite, or the computation overflowed.
    NonFinite,
    /// An interval is empty or reversed (`lo >= hi`) on this axis.
    Empty {
        /// `t`, `u`, `v` or `h`.
        axis: &'static str,
    },
}

/// Why an element could not be moved into the target frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransformFault {
    /// A mapped coordinate, direction or size overflowed (or a direction vanished).
    NonFinite,
    /// The mapped shape was refused by its constructor.
    Shape(AnalyticError),
    /// The mapped NURBS was refused by its constructor.
    Nurbs(NurbsError),
}

/// Why planning failed. Nothing is planned on any error.
#[derive(Debug, Clone, PartialEq)]
pub enum PlanError {
    /// No element was selected.
    EmptySelection,
    /// More than [`MAX_ELEMENTS`] elements.
    TooManyElements {
        /// The count given.
        count: usize,
    },
    /// The margin is NaN, infinite or negative.
    BadMargin {
        /// The margin given.
        value: f64,
    },
    /// The target frame or an element's frame is not in the tree.
    FrameNotFound {
        /// The element whose frame is missing, or `None` for the target frame.
        element: Option<ElementId>,
        /// The missing frame.
        frame: FrameId,
    },
    /// The transform from an element's frame to the target could not be composed.
    FrameResolution {
        /// The element's frame.
        frame: FrameId,
        /// The frame tree's error.
        error: FrameError,
    },
    /// An element name was refused.
    BadName {
        /// The element.
        element: ElementId,
        /// Why.
        fault: NameFault,
    },
    /// The same element was selected twice.
    DuplicateElement {
        /// The element.
        element: ElementId,
        /// Its first selection index.
        first: usize,
        /// The repeated index.
        index: usize,
    },
    /// An unbounded element has neither a caller extent nor fitted points (REQ-003).
    ExtentRequired {
        /// The element.
        element: ElementId,
    },
    /// A caller extent was given for an intrinsically bounded element.
    ExtentNotApplicable {
        /// The element.
        element: ElementId,
        /// Its kind.
        kind: GeomKind,
    },
    /// A caller extent of the wrong kind.
    ExtentKindMismatch {
        /// The element.
        element: ElementId,
        /// The kind the element takes.
        expected: ExtentKind,
        /// The kind given.
        found: ExtentKind,
    },
    /// Fitted points were given but would not be used.
    PointsNotApplicable {
        /// The element.
        element: ElementId,
        /// Why they would not be used.
        reason: PointsUnused,
    },
    /// The fitted point list is empty.
    EmptyPoints {
        /// The element.
        element: ElementId,
    },
    /// A fitted point is NaN or infinite.
    NonFinitePoint {
        /// The element.
        element: ElementId,
        /// The point's index.
        index: usize,
    },
    /// A caller extent is invalid, or a fitted extent overflowed.
    BadExtent {
        /// The element.
        element: ElementId,
        /// What is wrong.
        fault: ExtentFault,
    },
    /// Every fitted point projects to one value on an axis and the margin is zero.
    DegenerateExtent {
        /// The element.
        element: ElementId,
        /// `t`, `u`, `v` or `h`.
        axis: &'static str,
    },
    /// A cone extent holds the apex strictly inside.
    ExtentCrossesApex {
        /// The element.
        element: ElementId,
    },
    /// The element could not be moved into the target frame.
    Transform {
        /// The element.
        element: ElementId,
        /// Why.
        fault: TransformFault,
    },
    /// The plan could not be allocated.
    AllocationFailed,
}

impl PlanError {
    /// Stable snake-case code, e.g. `extent_required`, `frame_not_found`.
    pub fn code(&self) -> &'static str {
        match self {
            PlanError::EmptySelection => "empty_selection",
            PlanError::TooManyElements { .. } => "too_many_elements",
            PlanError::BadMargin { .. } => "bad_margin",
            PlanError::FrameNotFound { .. } => "frame_not_found",
            PlanError::FrameResolution { .. } => "frame_resolution_failed",
            PlanError::BadName { .. } => "bad_name",
            PlanError::DuplicateElement { .. } => "duplicate_element",
            PlanError::ExtentRequired { .. } => "extent_required",
            PlanError::ExtentNotApplicable { .. } => "extent_not_applicable",
            PlanError::ExtentKindMismatch { .. } => "extent_kind_mismatch",
            PlanError::PointsNotApplicable { .. } => "points_not_applicable",
            PlanError::EmptyPoints { .. } => "empty_points",
            PlanError::NonFinitePoint { .. } => "non_finite_point",
            PlanError::BadExtent { .. } => "bad_extent",
            PlanError::DegenerateExtent { .. } => "degenerate_extent",
            PlanError::ExtentCrossesApex { .. } => "extent_crosses_apex",
            PlanError::Transform { .. } => "transform_failed",
            PlanError::AllocationFailed => "allocation_failed",
        }
    }

    /// The element the error names, if any.
    pub fn element(&self) -> Option<ElementId> {
        match self {
            PlanError::BadName { element, .. }
            | PlanError::DuplicateElement { element, .. }
            | PlanError::ExtentRequired { element }
            | PlanError::ExtentNotApplicable { element, .. }
            | PlanError::ExtentKindMismatch { element, .. }
            | PlanError::PointsNotApplicable { element, .. }
            | PlanError::EmptyPoints { element }
            | PlanError::NonFinitePoint { element, .. }
            | PlanError::BadExtent { element, .. }
            | PlanError::DegenerateExtent { element, .. }
            | PlanError::ExtentCrossesApex { element }
            | PlanError::Transform { element, .. } => Some(*element),
            PlanError::FrameNotFound { element, .. } => *element,
            _ => None,
        }
    }
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlanError::EmptySelection => write!(f, "no element selected"),
            PlanError::TooManyElements { count } => write!(f, "{count} elements, more than {MAX_ELEMENTS}"),
            PlanError::BadMargin { value } => write!(f, "margin {value:e} is not finite and >= 0"),
            PlanError::FrameNotFound { element: None, frame } => write!(f, "target frame {frame} not found"),
            PlanError::FrameNotFound { element: Some(element), frame } => {
                write!(f, "element {element}: frame {frame} not found")
            }
            PlanError::FrameResolution { frame, error } => write!(f, "frame {frame} not resolved: {error}"),
            PlanError::BadName { element, fault } => write!(f, "element {element}: name refused ({})", fault.code()),
            PlanError::DuplicateElement { element, first, index } => {
                write!(f, "element {element} selected at {first} and again at {index}")
            }
            PlanError::ExtentRequired { element } => {
                write!(f, "element {element} is unbounded and has neither an extent nor fitted points")
            }
            PlanError::ExtentNotApplicable { element, kind } => {
                write!(f, "element {element} is a bounded {} and takes no extent", kind.as_str())
            }
            PlanError::ExtentKindMismatch { element, expected, found } => {
                write!(f, "element {element} takes a {} extent, not {}", expected.as_str(), found.as_str())
            }
            PlanError::PointsNotApplicable { element, reason } => match reason {
                PointsUnused::Bounded => write!(f, "element {element} is bounded; its points would not be used"),
                PointsUnused::CallerExtent => {
                    write!(f, "element {element} has a caller extent; its points would not be used")
                }
            },
            PlanError::EmptyPoints { element } => write!(f, "element {element}: empty fitted point list"),
            PlanError::NonFinitePoint { element, index } => {
                write!(f, "element {element}: fitted point {index} is not finite")
            }
            PlanError::BadExtent { element, fault } => match fault {
                ExtentFault::NonFinite => write!(f, "element {element}: extent not finite"),
                ExtentFault::Empty { axis } => write!(f, "element {element}: extent empty on {axis}"),
            },
            PlanError::DegenerateExtent { element, axis } => {
                write!(f, "element {element}: fitted points have zero width on {axis}")
            }
            PlanError::ExtentCrossesApex { element } => write!(f, "element {element}: cone extent crosses the apex"),
            PlanError::Transform { element, fault } => match fault {
                TransformFault::NonFinite => write!(f, "element {element}: transform overflowed"),
                TransformFault::Shape(e) => write!(f, "element {element}: transformed shape refused: {e}"),
                TransformFault::Nurbs(e) => write!(f, "element {element}: transformed NURBS refused: {}", e.code()),
            },
            PlanError::AllocationFailed => write!(f, "allocation failed"),
        }
    }
}

impl std::error::Error for PlanError {}

// ---------------------------------------------------------------- planning

/// Builds the export plan into frame `target`. `margin_m` (metres of the target frame,
/// finite, `>= 0`; `0` by default) widens fitted-point extents on each side.
///
/// Per element, in selection order, the first failure wins: [`PlanError::BadName`];
/// [`PlanError::DuplicateElement`]; [`PlanError::FrameNotFound`] or
/// [`PlanError::FrameResolution`]; for a bounded element [`PlanError::ExtentNotApplicable`]
/// then [`PlanError::PointsNotApplicable`]; for an unbounded one with a caller extent
/// [`PlanError::ExtentKindMismatch`], [`PlanError::PointsNotApplicable`],
/// [`PlanError::BadExtent`]; without one [`PlanError::ExtentRequired`],
/// [`PlanError::EmptyPoints`], [`PlanError::NonFinitePoint`] (first index),
/// [`PlanError::BadExtent`] (projection overflow); then [`PlanError::Transform`]; then the
/// target extent: [`PlanError::BadExtent`] (overflow), [`PlanError::DegenerateExtent`]
/// (first axis), and for a cone [`PlanError::ExtentCrossesApex`].
pub fn plan(
    tree: &FrameTree,
    target: FrameId,
    elements: &[ElementInput<'_>],
    margin_m: f64,
) -> Result<ExportPlan, PlanError> {
    if tree.get(target).is_none() {
        return Err(PlanError::FrameNotFound { element: None, frame: target });
    }
    if !margin_m.is_finite() || margin_m < 0.0 {
        return Err(PlanError::BadMargin { value: margin_m });
    }
    let margin_m = margin_m + 0.0;
    if elements.is_empty() {
        return Err(PlanError::EmptySelection);
    }
    if elements.len() > MAX_ELEMENTS {
        return Err(PlanError::TooManyElements { count: elements.len() });
    }
    let mut items: Vec<PlanItem> = Vec::new();
    items.try_reserve_exact(elements.len()).map_err(|_| PlanError::AllocationFailed)?;
    let mut seen: BTreeMap<ElementId, usize> = BTreeMap::new();
    let mut transforms: BTreeMap<FrameId, Transform> = BTreeMap::new();
    for (index, e) in elements.iter().enumerate() {
        let element = e.element;
        let name = UntrustedName::new(e.name).map_err(|fault| PlanError::BadName { element, fault })?;
        if let Some(first) = seen.get(&element) {
            return Err(PlanError::DuplicateElement { element, first: *first, index });
        }
        seen.insert(element, index);
        let t = match transforms.get(&e.frame) {
            Some(t) => *t,
            None => {
                let t = resolve(tree, element, e.frame, target)?;
                transforms.insert(e.frame, t);
                t
            }
        };
        let raw = source_bounds(e)?;
        let geom = map_geometry(e.geom, &t).map_err(|fault| PlanError::Transform { element, fault })?;
        let bounds = target_bounds(element, raw, t.scale(), margin_m, &geom)?;
        items.push(PlanItem { element, name, source_frame: e.frame, geom, bounds });
    }
    Ok(ExportPlan { frame: target, margin_m, items })
}

fn resolve(tree: &FrameTree, element: ElementId, from: FrameId, target: FrameId) -> Result<Transform, PlanError> {
    tree.resolve(from, target).map_err(|error| match error {
        FrameError::UnknownFrame { id } => PlanError::FrameNotFound { element: Some(element), frame: id },
        error => PlanError::FrameResolution { frame: from, error },
    })
}

/// `-0.0` to `+0.0`; every other value unchanged.
fn pos_zero(v: f64) -> f64 {
    v + 0.0
}

/// The bounds read in the element's own frame, before the scale and the margin.
enum Raw {
    /// A bounded element.
    Intrinsic,
    /// A validated caller extent: kind and `(lo, hi)` per axis.
    Caller(ExtentKind, Vec<(f64, f64)>),
    /// The total-order `(min, max)` projection per axis and the point count.
    Fitted(ExtentKind, Vec<(f64, f64)>, u64),
}

/// The input extent rules, in the element's own frame.
fn source_bounds(e: &ElementInput<'_>) -> Result<Raw, PlanError> {
    let element = e.element;
    let kind = e.geom.kind();
    let (Some(ek), Some(pl)) = (kind.extent_kind(), e.geom.extent_placement()) else {
        if e.extent.is_some() {
            return Err(PlanError::ExtentNotApplicable { element, kind });
        }
        if e.points.is_some() {
            return Err(PlanError::PointsNotApplicable { element, reason: PointsUnused::Bounded });
        }
        return Ok(Raw::Intrinsic);
    };
    let bad = |fault| PlanError::BadExtent { element, fault };
    if let Some(x) = e.extent {
        if x.kind() != ek {
            return Err(PlanError::ExtentKindMismatch { element, expected: ek, found: x.kind() });
        }
        if e.points.is_some() {
            return Err(PlanError::PointsNotApplicable { element, reason: PointsUnused::CallerExtent });
        }
        let iv = x.intervals();
        if iv.iter().any(|(a, b)| !a.is_finite() || !b.is_finite()) {
            return Err(bad(ExtentFault::NonFinite));
        }
        if let Some(axis) = iv.iter().zip(ek.axes()).find(|((a, b), _)| a >= b).map(|(_, axis)| *axis) {
            return Err(bad(ExtentFault::Empty { axis }));
        }
        return Ok(Raw::Caller(ek, iv));
    }
    let points = e.points.ok_or(PlanError::ExtentRequired { element })?;
    if points.is_empty() {
        return Err(PlanError::EmptyPoints { element });
    }
    if let Some(index) = points.iter().position(|q| q.iter().any(|c| !c.is_finite())) {
        return Err(PlanError::NonFinitePoint { element, index });
    }
    let iv = projections(pl, ek, points).ok_or_else(|| bad(ExtentFault::NonFinite))?;
    Ok(Raw::Fitted(ek, iv, points.len() as u64))
}

/// Min/max (total order) of the projections on each axis of `ek`; `None` on overflow.
fn projections(pl: &Placement, ek: ExtentKind, points: &[[f64; 3]]) -> Option<Vec<(f64, f64)>> {
    // Local coordinate index of each axis: t and u are x, v is y, h is z.
    let comps: &[usize] = match ek {
        ExtentKind::Line => &[0],
        ExtentKind::Rect => &[0, 1],
        ExtentKind::Axial => &[2],
    };
    let mut acc: Vec<Option<(f64, f64)>> = vec![None; comps.len()];
    for q in points {
        let l = pl.to_local(*q).ok()?;
        for (slot, c) in acc.iter_mut().zip(comps) {
            let v = l.get(*c).copied()?;
            *slot = Some(match *slot {
                None => (v, v),
                Some((lo, hi)) => (
                    if v.total_cmp(&lo) == Ordering::Less { v } else { lo },
                    if v.total_cmp(&hi) == Ordering::Greater { v } else { hi },
                ),
            });
        }
    }
    acc.into_iter().collect()
}

/// The extent in the target frame: `s v` for a caller extent, `s min - margin` and
/// `s max + margin` for fitted points; refused when not finite, of zero width, or (for the
/// moved cone) holding the apex strictly inside.
fn target_bounds(element: ElementId, raw: Raw, s: f64, margin_m: f64, geom: &Geometry) -> Result<Bounds, PlanError> {
    let (ek, iv, source, widen) = match raw {
        Raw::Intrinsic => return Ok(Bounds::Intrinsic),
        Raw::Caller(ek, iv) => (ek, iv, ExtentSource::Caller, None),
        Raw::Fitted(ek, iv, points) => (ek, iv, ExtentSource::FittedPoints { points, margin_m }, Some(margin_m)),
    };
    let bad = || PlanError::BadExtent { element, fault: ExtentFault::NonFinite };
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(iv.len());
    for ((a, b), axis) in iv.iter().zip(ek.axes()) {
        let (lo, hi) = match widen {
            None => (s * a, s * b),
            Some(m) => (s * a - m, s * b + m),
        };
        if !lo.is_finite() || !hi.is_finite() {
            return Err(bad());
        }
        if lo >= hi {
            return Err(PlanError::DegenerateExtent { element, axis });
        }
        out.push((pos_zero(lo), pos_zero(hi)));
    }
    let extent = Extent::from_intervals(ek, &out).ok_or_else(bad)?;
    if let Geometry::Cone(c) = geom {
        match check_apex(c, &extent) {
            Ok(()) => {}
            Err(Apex::Crosses) => return Err(PlanError::ExtentCrossesApex { element }),
            Err(Apex::Overflow) => return Err(bad()),
        }
    }
    Ok(Bounds::Extent { extent, source })
}

enum Apex {
    Crosses,
    Overflow,
}

/// Refuses an axial extent whose interior holds the cone apex: `r0 + h tan a` changes sign
/// strictly inside `[h0, h1]`.
fn check_apex(c: &Cone, extent: &Extent) -> Result<(), Apex> {
    let Extent::Axial(h0, h1) = *extent else {
        return Ok(());
    };
    let ta = fid_num::f::tan(c.half_angle()).map_err(|_| Apex::Overflow)?;
    let r0 = c.radius();
    let at0 = r0 + h0 * ta;
    let at1 = r0 + h1 * ta;
    if !at0.is_finite() || !at1.is_finite() {
        return Err(Apex::Overflow);
    }
    if at0 < 0.0 && at1 > 0.0 {
        return Err(Apex::Crosses);
    }
    Ok(())
}

// ---------------------------------------------------------------- transform

/// `(a0 b0 + a1 b1) + a2 b2`.
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

fn unit(v: [f64; 3]) -> Result<[f64; 3], TransformFault> {
    let n = fid_num::f::sqrt(dot(v, v)).map_err(|_| TransformFault::NonFinite)?;
    if !n.is_finite() || n <= 0.0 {
        return Err(TransformFault::NonFinite);
    }
    let u = [v[0] / n, v[1] / n, v[2] / n];
    if u.iter().any(|c| !c.is_finite()) {
        return Err(TransformFault::NonFinite);
    }
    Ok(u)
}

fn point(t: &Transform, p: &[f64; 3]) -> Result<[f64; 3], TransformFault> {
    t.apply_point(p).map_err(|_| TransformFault::NonFinite)
}

fn map_placement(pl: &Placement, t: &Transform) -> Result<Placement, TransformFault> {
    let origin = point(t, &pl.origin())?;
    let z1 = t.apply_dir(&pl.axis()).map_err(|_| TransformFault::NonFinite)?;
    let x1 = t.apply_dir(&pl.ref_dir()).map_err(|_| TransformFault::NonFinite)?;
    let z = unit(z1)?;
    let p = dot(x1, z);
    let x = unit([x1[0] - p * z[0], x1[1] - p * z[1], x1[2] - p * z[2]])?;
    Placement::new(origin, z, x).map_err(TransformFault::Shape)
}

fn size(s: f64, r: f64) -> Result<f64, TransformFault> {
    let v = s * r;
    if v.is_finite() {
        Ok(v)
    } else {
        Err(TransformFault::NonFinite)
    }
}

fn map_geometry(g: &Geometry, t: &Transform) -> Result<Geometry, TransformFault> {
    let s = t.scale();
    let shape = TransformFault::Shape;
    Ok(match g {
        Geometry::Point(p) => Geometry::Point(point(t, p)?),
        Geometry::Line(l) => Geometry::Line(Line::new(map_placement(l.placement(), t)?)),
        Geometry::Plane(p) => Geometry::Plane(Plane::new(map_placement(p.placement(), t)?)),
        Geometry::Cylinder(c) => {
            Geometry::Cylinder(Cylinder::new(map_placement(c.placement(), t)?, size(s, c.radius())?).map_err(shape)?)
        }
        Geometry::Cone(c) => Geometry::Cone(
            Cone::new(map_placement(c.placement(), t)?, size(s, c.radius())?, c.half_angle()).map_err(shape)?,
        ),
        Geometry::Circle(c) => {
            Geometry::Circle(Circle::new(map_placement(c.placement(), t)?, size(s, c.radius())?).map_err(shape)?)
        }
        Geometry::Ellipse(e) => {
            let (a, b) = e.semi_axes();
            Geometry::Ellipse(Ellipse::new(map_placement(e.placement(), t)?, size(s, a)?, size(s, b)?).map_err(shape)?)
        }
        Geometry::Sphere(c) => {
            Geometry::Sphere(Sphere::new(map_placement(c.placement(), t)?, size(s, c.radius())?).map_err(shape)?)
        }
        Geometry::Torus(c) => {
            let (major, minor) = c.radii();
            Geometry::Torus(
                Torus::new(map_placement(c.placement(), t)?, size(s, major)?, size(s, minor)?).map_err(shape)?,
            )
        }
        Geometry::NurbsCurve(c) => {
            let mut parts = c.clone().into_parts();
            for p in parts.ctrl.iter_mut() {
                *p = point(t, p)?;
            }
            Geometry::NurbsCurve(NurbsCurve::new(parts).map_err(TransformFault::Nurbs)?)
        }
        Geometry::NurbsSurface(c) => {
            let mut parts = c.clone().into_parts();
            for p in parts.ctrl.iter_mut() {
                *p = point(t, p)?;
            }
            Geometry::NurbsSurface(NurbsSurface::new(parts).map_err(TransformFault::Nurbs)?)
        }
    })
}
