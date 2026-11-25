//! STEP curve and surface mapping with approximation error (spec node `IO.CAD.STEP.GEOM`).
//!
//! [`map_geometry`] turns the geometric entities of a parsed [`P21File`] into GEO types
//! (`fid-geo-nurbs`) in metres:
//!
//! - `cartesian_point` to a point; `direction`, `vector`, `axis1_placement`,
//!   `axis2_placement_3d` and `axis2_placement_2d` are read where they are used;
//! - `line`, `circle`, `ellipse` to the GEO analytic curves, `b_spline_curve_with_knots` (and
//!   the rational complex form) and `polyline` to GEO NURBS, `trimmed_curve`, `surface_curve` and
//!   `seam_curve` to records that refer to their basis by entity number, `pcurve` to a 2D curve
//!   in the GEO parameters of its basis surface;
//! - `plane`, `cylindrical_surface`, `conical_surface`, `spherical_surface` and
//!   `toroidal_surface` to the GEO analytic surfaces, `b_spline_surface_with_knots` (and the
//!   rational complex form) to GEO NURBS, `surface_of_linear_extrusion` and
//!   `surface_of_revolution` to exact rational NURBS, and `offset_surface` to an approximating
//!   NURBS whose sampled error is recorded per surface ([`Approx`]).
//!
//! # Units
//!
//! Every length is the file value times the length factor of the entity's geometric
//! representation context (`IO.CAD.STEP.HDR`), one multiplication per coordinate; every plane
//! angle (a cone's semi-angle, a conic's trim parameter) is the file value times the
//! plane-angle factor (radian when the context declares none). The context of an entity is
//! found by walking references from the items of every representation (an instance with a
//! `(name, items, context)` part whose context is a geometric representation context) without
//! entering other representations. An entity no representation reaches gets the units of the
//! file's 3D contexts when they all agree, else it is skipped with `no_context`; one reached from
//! contexts with different units is skipped with `context_conflict`. Entities reached only from
//! a 2D context are parameter-space geometry: they are mapped only through their `pcurve`.
//!
//! # Results
//!
//! Nothing stops the mapping: every entity that cannot be mapped is listed in
//! [`GeomMap::skipped`] with its entity number, its type and a reason code (REQ-005), and the
//! next entity is mapped. [`GeomMap::to_records`] writes the whole result deterministically
//! (AC-5). There is no recursion deeper than a fixed bound, no `unsafe`, no panic.

mod approx;
mod curve;
mod place;
mod read;
mod record;
mod surface;

pub use approx::{INITIAL_SPANS_PER_TURN, MAX_FIT_SPANS, MAX_REFINEMENTS, SAMPLES_PER_SPAN};

use crate::header::StepHeaderInfo;
use crate::{Instance, P21File, Param};
use fid_geo_nurbs::{AnalyticError, AnalyticSurface, Circle, Ellipse, Line, NurbsCurve, NurbsError, NurbsSurface};
use std::collections::{BTreeMap, BTreeSet};

/// Default approximation tolerance, metres (REQ-IO.CAD.STEP.GEOM-004).
pub const DEFAULT_APPROX_TOL_M: f64 = 1e-7;
/// Default limit on the sample points of one approximation level.
pub const DEFAULT_MAX_APPROX_SAMPLES: u64 = 1 << 22;
/// A placement whose normalised reference direction is further than this from orthogonal to
/// its axis (`|ref . axis|`) is reported ([`FRAME_REORTHONORMALISED`]).
pub const FRAME_DEVIATION_REPORT: f64 = 1e-9;
/// Warning code: a placement's reference direction was not orthogonal to its axis and was
/// re-orthonormalised; the value is `|ref . axis|`.
pub const FRAME_REORTHONORMALISED: &str = "frame_reorthonormalised";
/// Approximation status code of a surface above the tolerance after all refinements.
pub const APPROX_OUT_OF_TOLERANCE: &str = "approx_out_of_tolerance";

/// Caller choices.
#[derive(Debug, Clone, PartialEq)]
pub struct GeomOptions {
    /// Approximation tolerance, metres; finite and positive.
    pub approx_tol_m: f64,
    /// Limit on the sample points of one approximation level; a refinement that would exceed
    /// it is not run.
    pub max_approx_samples: u64,
    /// Parameter boxes, in the GEO parameters of the basis surface, for surfaces whose basis
    /// is unbounded in a parameter (an extrusion's `v`, the offset of a plane, cylinder or cone),
    /// by entity number of the extrusion or offset surface. `IO.CAD.STEP.BREP` passes each
    /// face's box; without one an unbounded parameter uses `[0, 1]` and the record says so.
    pub domains: BTreeMap<u64, ParamBox>,
}

impl Default for GeomOptions {
    fn default() -> Self {
        GeomOptions {
            approx_tol_m: DEFAULT_APPROX_TOL_M,
            max_approx_samples: DEFAULT_MAX_APPROX_SAMPLES,
            domains: BTreeMap::new(),
        }
    }
}

/// A parameter box `u x v`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamBox {
    /// `(lo, hi)` in `u`.
    pub u: (f64, f64),
    /// `(lo, hi)` in `v`.
    pub v: (f64, f64),
}

/// A refusal of the whole call (bad options); per-entity problems go to [`GeomMap::skipped`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GeomError {
    /// An option is out of range; `name` is the field.
    BadOption { name: &'static str },
}

impl GeomError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            GeomError::BadOption { .. } => "bad_option",
        }
    }
}

/// Which representation a trimmed or surface curve prefers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimMaster {
    /// Model space (`.CARTESIAN.`, `.CURVE_3D.`).
    Cartesian,
    /// Parameter space (`.PARAMETER.`, `.PCURVE_S1.`, `.PCURVE_S2.`).
    Parameter,
    /// `.UNSPECIFIED.`.
    Unspecified,
}

/// One end of a trimmed curve: the trimming parameter in the basis curve's GEO parameter
/// (a line's metres, a conic's radians, a B-spline's own parameter), the trimming point in
/// metres, or both.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trim {
    /// The parameter, converted.
    pub param: Option<f64>,
    /// The point, metres.
    pub point: Option<[f64; 3]>,
}

/// A `trimmed_curve`.
#[derive(Debug, Clone, PartialEq)]
pub struct TrimmedCurve {
    /// The basis curve's entity number.
    pub basis: u64,
    /// First trim.
    pub trim_1: Trim,
    /// Second trim.
    pub trim_2: Trim,
    /// `sense_agreement`.
    pub sense: bool,
    /// `master_representation`.
    pub master: TrimMaster,
}

/// A mapped curve.
#[derive(Debug, Clone, PartialEq)]
pub enum CurveRef {
    /// A `line`. GEO lines run along a unit direction, so the GEO parameter is the STEP parameter
    /// times `magnitude_m` (the vector's magnitude in metres).
    Line { line: Line, magnitude_m: f64 },
    /// A `circle`; GEO parameter = STEP parameter times the plane-angle factor.
    Circle(Circle),
    /// An `ellipse`; GEO parameter = STEP parameter times the plane-angle factor.
    Ellipse(Ellipse),
    /// A `b_spline_curve_with_knots`, rational or not; same parameter.
    Nurbs(NurbsCurve),
    /// A `polyline` as a degree-1 B-spline over `[0, points - 1]`.
    Polyline(NurbsCurve),
    /// A `trimmed_curve`.
    Trimmed(TrimmedCurve),
    /// A `surface_curve` (`seam` false) or `seam_curve` (`seam` true): its 3D curve and its one
    /// or two associated pcurves or surfaces, by entity number.
    Surface { curve_3d: u64, associated: Vec<u64>, master: TrimMaster, seam: bool },
}

/// A mapped surface.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceRef {
    /// Plane, cylinder, cone, sphere or torus; same parameterisation as STEP with lengths in
    /// metres and angles in radians.
    Analytic(AnalyticSurface),
    /// A `b_spline_surface_with_knots`, rational or not; same parameters.
    Nurbs(NurbsSurface),
    /// A `surface_of_linear_extrusion`, exact. `v` is STEP's; `u` is STEP's when `param_exact`.
    /// `domain_defaulted`: no caller box, so `v` covers `[0, 1]`.
    Extrusion { nurbs: NurbsSurface, param_exact: bool, domain_defaulted: bool },
    /// A `surface_of_revolution`, exact geometry. `v` (the angle) is a rational
    /// reparameterisation, so `param_exact` is false; `u_param_exact` says whether `u` is STEP's.
    Revolution { nurbs: NurbsSurface, param_exact: bool, u_param_exact: bool },
    /// An approximating NURBS (`offset_surface`), over the basis surface's parameters; its error
    /// is in [`GeomMap::approximations`] under the same entity number.
    Approximated(NurbsSurface),
}

/// A 2D curve in the GEO parameter space of a surface.
#[derive(Debug, Clone, PartialEq)]
pub enum Curve2 {
    /// `origin + t vector`, `t` the STEP parameter.
    Line { origin: [f64; 2], vector: [f64; 2] },
    /// `center + a cos s x_dir + b sin s y_dir` with `y_dir` the left normal of `x_dir` and `s`
    /// the STEP parameter in radians (a circle has `a == b`).
    Conic { center: [f64; 2], x_dir: [f64; 2], semi_axes: [f64; 2] },
    /// A B-spline or polyline with control points `(u, v, 0)`.
    Nurbs(NurbsCurve),
}

/// A `pcurve`.
#[derive(Debug, Clone, PartialEq)]
pub struct Pcurve2 {
    /// The basis surface's entity number.
    pub surface: u64,
    /// The `definitional_representation`'s entity number.
    pub representation: u64,
    /// The 2D curve's entity number.
    pub curve_entity: u64,
    /// The 2D curve, coordinates scaled into the surface's GEO parameters.
    pub curve: Curve2,
    /// The per-coordinate factors applied (`[u, v]`).
    pub scale: [f64; 2],
    /// True when the surface's GEO parameters are an exact (affine) image of its STEP
    /// parameters, so the curve lies on the mapped surface; false for a revolution, and for an
    /// extrusion of a conic, whose NURBS parameters are rational reparameterisations.
    pub param_exact: bool,
}

/// Whether an approximation met the tolerance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApproxStatus {
    /// `max_dev_m <= tolerance_m`.
    WithinTolerance,
    /// Above the tolerance after all refinements ([`APPROX_OUT_OF_TOLERANCE`]).
    OutOfTolerance,
}

impl ApproxStatus {
    /// Stable code.
    pub fn code(self) -> &'static str {
        match self {
            ApproxStatus::WithinTolerance => "approx_within_tolerance",
            ApproxStatus::OutOfTolerance => APPROX_OUT_OF_TOLERANCE,
        }
    }
}

/// The recorded error of one approximated surface (REQ-IO.CAD.STEP.GEOM-003).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Approx {
    /// Sampled maximum of `|approximant(u, v) - exact(u, v)|`, metres: a sampled bound, not a
    /// proven one.
    pub max_dev_m: f64,
    /// The tolerance it was held to.
    pub tolerance_m: f64,
    /// Sample points measured (`grid[0] * grid[1]`).
    pub samples: u64,
    /// Sample points per direction: `32 spans + 1`.
    pub grid: [usize; 2],
    /// Knot spans per direction.
    pub spans: [usize; 2],
    /// Refinements done (0 to [`MAX_REFINEMENTS`]).
    pub refined: u32,
    /// The parameter box fitted.
    pub domain: ParamBox,
    /// True when an unbounded parameter got `[0, 1]` for want of a caller box.
    pub domain_defaulted: bool,
    /// Within or out of tolerance.
    pub status: ApproxStatus,
}

/// Why an entity was not mapped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SkipReason {
    /// A geometric entity type this mapper does not support (REQ-005); `at` is the entity
    /// whose type is unsupported (the entity itself or one it needs).
    Unsupported { at: u64 },
    /// Wrong parameter arity or type, or an impossible value; `at` is the offending entity.
    Malformed { at: u64, what: &'static str },
    /// GEO NURBS validation refused it (REQ-002).
    NurbsInvalid { at: u64, err: NurbsError },
    /// GEO analytic construction refused it.
    GeoInvalid { at: u64, err: AnalyticError },
    /// A reference to an undefined entity.
    Dangling { to: u64 },
    /// No representation context reaches it and the file's contexts disagree on units.
    NoContext,
    /// Representation contexts with different units reach it.
    ContextConflict,
    /// An overflow or non-finite intermediate.
    NonFinite,
    /// The offset basis has a degenerate normal at a fit or sample point.
    DegenerateNormal,
    /// The approximation needs more spans or samples than allowed.
    ApproxBudget,
    /// The approximation's linear systems or evaluations failed.
    ApproxFailed,
    /// A caller parameter box is not finite and increasing.
    BadDomain,
}

impl SkipReason {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            SkipReason::Unsupported { .. } => "unsupported",
            SkipReason::Malformed { .. } => "malformed",
            SkipReason::NurbsInvalid { .. } => "nurbs_invalid",
            SkipReason::GeoInvalid { .. } => "geo_invalid",
            SkipReason::Dangling { .. } => "dangling_reference",
            SkipReason::NoContext => "no_context",
            SkipReason::ContextConflict => "context_conflict",
            SkipReason::NonFinite => "non_finite",
            SkipReason::DegenerateNormal => "offset_degenerate_normal",
            SkipReason::ApproxBudget => "approx_budget",
            SkipReason::ApproxFailed => "approx_failed",
            SkipReason::BadDomain => "bad_domain",
        }
    }
}

/// One skipped entity.
#[derive(Debug, Clone, PartialEq)]
pub struct Skipped {
    /// The entity number.
    pub entity: u64,
    /// Its type: the entity name, or the part names of a complex instance joined by `+`.
    pub type_name: String,
    /// Why.
    pub reason: SkipReason,
}

/// A non-fatal observation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeomWarning {
    /// The entity it concerns.
    pub entity: u64,
    /// Stable code ([`FRAME_REORTHONORMALISED`]).
    pub code: &'static str,
    /// The measured value.
    pub value: f64,
}

/// The mapping result (Design: `GeomMap`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GeomMap {
    /// 3D `cartesian_point`s, metres.
    pub points: BTreeMap<u64, [f64; 3]>,
    /// Curves by entity number.
    pub curves: BTreeMap<u64, CurveRef>,
    /// Surfaces by entity number.
    pub surfaces: BTreeMap<u64, SurfaceRef>,
    /// Pcurves by entity number.
    pub pcurves: BTreeMap<u64, Pcurve2>,
    /// Approximation errors by surface entity number.
    pub approximations: BTreeMap<u64, Approx>,
    /// Skipped entities, ascending entity number.
    pub skipped: Vec<Skipped>,
    /// Warnings, sorted by entity and code, without repeats.
    pub warnings: Vec<GeomWarning>,
}

/// Internal per-entity failure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Fail {
    Unsupported { at: u64 },
    Malformed { at: u64, what: &'static str },
    Nurbs { at: u64, err: NurbsError },
    Geo { at: u64, err: AnalyticError },
    Dangling { to: u64 },
    NonFinite,
    DegenerateNormal,
    ApproxBudget,
    Fit,
    BadDomain,
}

impl Fail {
    fn reason(self) -> SkipReason {
        match self {
            Fail::Unsupported { at } => SkipReason::Unsupported { at },
            Fail::Malformed { at, what } => SkipReason::Malformed { at, what },
            Fail::Nurbs { at, err } => SkipReason::NurbsInvalid { at, err },
            Fail::Geo { at, err } => SkipReason::GeoInvalid { at, err },
            Fail::Dangling { to } => SkipReason::Dangling { to },
            Fail::NonFinite => SkipReason::NonFinite,
            Fail::DegenerateNormal => SkipReason::DegenerateNormal,
            Fail::ApproxBudget => SkipReason::ApproxBudget,
            Fail::Fit => SkipReason::ApproxFailed,
            Fail::BadDomain => SkipReason::BadDomain,
        }
    }
}

/// Length and plane-angle factors of a context.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Units {
    pub length: f64,
    pub angle: f64,
}

/// What an instance is, for this mapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Point,
    Line,
    Circle,
    Ellipse,
    BsplineCurve,
    Polyline,
    Trimmed,
    SurfaceCurve,
    SeamCurve,
    Pcurve,
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
    BsplineSurface,
    Extrusion,
    Revolution,
    Offset,
    /// A geometric entity type that is not supported.
    Unsupported,
    /// Not geometry (topology, products, presentation, placements and directions).
    Other,
}

/// Parts a rational or non-rational complex B-spline curve may carry.
const CURVE_PARTS: &[&str] = &[
    "BOUNDED_CURVE",
    "B_SPLINE_CURVE",
    "B_SPLINE_CURVE_WITH_KNOTS",
    "CURVE",
    "GEOMETRIC_REPRESENTATION_ITEM",
    "RATIONAL_B_SPLINE_CURVE",
    "REPRESENTATION_ITEM",
];

/// Parts a rational or non-rational complex B-spline surface may carry.
const SURFACE_PARTS: &[&str] = &[
    "BOUNDED_SURFACE",
    "B_SPLINE_SURFACE",
    "B_SPLINE_SURFACE_WITH_KNOTS",
    "GEOMETRIC_REPRESENTATION_ITEM",
    "RATIONAL_B_SPLINE_SURFACE",
    "REPRESENTATION_ITEM",
    "SURFACE",
];

/// Geometric entity names without a `_CURVE` or `_SURFACE` suffix that are reported as
/// unsupported (ISO 10303-42 [unverified: list to check against the schema]).
const OTHER_GEOMETRY: &[&str] = &[
    "BEZIER_CURVE",
    "CIRCULAR_INVOLUTE",
    "CLOTHOID",
    "CURVE_REPLICA",
    "DEGENERATE_PCURVE",
    "EVALUATED_DEGENERATE_PCURVE",
    "HYPERBOLA",
    "OFFSET_CURVE_2D",
    "OFFSET_CURVE_3D",
    "PARABOLA",
    "POINT_ON_CURVE",
    "POINT_ON_SURFACE",
    "SURFACE_REPLICA",
];

/// Names with a `_CURVE` or `_SURFACE` suffix that are topology, not geometry.
const TOPOLOGY: &[&str] = &["EDGE_CURVE", "FACE_SURFACE"];

fn is_geometric_name(name: &str) -> bool {
    if TOPOLOGY.contains(&name) {
        return false;
    }
    name.ends_with("_CURVE") || name.ends_with("_SURFACE") || OTHER_GEOMETRY.contains(&name)
}

/// Classifies an instance.
pub(crate) fn classify(inst: &Instance) -> Kind {
    if let [(name, _)] = inst.parts.as_slice() {
        return match name.as_str() {
            "CARTESIAN_POINT" => Kind::Point,
            "LINE" => Kind::Line,
            "CIRCLE" => Kind::Circle,
            "ELLIPSE" => Kind::Ellipse,
            "B_SPLINE_CURVE_WITH_KNOTS" => Kind::BsplineCurve,
            "POLYLINE" => Kind::Polyline,
            "TRIMMED_CURVE" => Kind::Trimmed,
            "SURFACE_CURVE" => Kind::SurfaceCurve,
            "SEAM_CURVE" => Kind::SeamCurve,
            "PCURVE" => Kind::Pcurve,
            "PLANE" => Kind::Plane,
            "CYLINDRICAL_SURFACE" => Kind::Cylinder,
            "CONICAL_SURFACE" => Kind::Cone,
            "SPHERICAL_SURFACE" => Kind::Sphere,
            "TOROIDAL_SURFACE" => Kind::Torus,
            "B_SPLINE_SURFACE_WITH_KNOTS" => Kind::BsplineSurface,
            "SURFACE_OF_LINEAR_EXTRUSION" => Kind::Extrusion,
            "SURFACE_OF_REVOLUTION" => Kind::Revolution,
            "OFFSET_SURFACE" => Kind::Offset,
            other if is_geometric_name(other) => Kind::Unsupported,
            _ => Kind::Other,
        };
    }
    let has = |n: &str| inst.parts.iter().any(|(p, _)| p.as_str() == n);
    let only = |allowed: &[&str]| inst.parts.iter().all(|(p, _)| allowed.contains(&p.as_str()));
    if has("B_SPLINE_CURVE_WITH_KNOTS") && has("B_SPLINE_CURVE") && only(CURVE_PARTS) {
        return Kind::BsplineCurve;
    }
    if has("B_SPLINE_SURFACE_WITH_KNOTS") && has("B_SPLINE_SURFACE") && only(SURFACE_PARTS) {
        return Kind::BsplineSurface;
    }
    let geometric = inst.parts.iter().any(|(p, _)| {
        let p = p.as_str();
        is_geometric_name(p) || p == "CURVE" || p == "SURFACE"
    });
    if geometric {
        Kind::Unsupported
    } else {
        Kind::Other
    }
}

/// The type name of an instance for a skip record.
fn type_name(inst: &Instance) -> String {
    let names: Vec<&str> = inst.parts.iter().map(|(p, _)| p.as_str()).collect();
    names.join("+")
}

// ------------------------------------------------------------------ contexts

/// The context assignment of an entity.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Assign {
    /// Units and dimension of the (first) context that reaches it.
    One { units: Units, dim: u8 },
    /// Contexts with different units or dimensions reach it.
    Conflict,
}

/// `(name, items, context)` of a representation part, if `params` has that shape with
/// `context` among `contexts`.
fn representation<'a>(
    params: &'a [Param],
    contexts: &BTreeMap<u64, (Option<Units>, u8)>,
) -> Option<(&'a [Param], u64)> {
    match params {
        [_, Param::List(items), Param::Ref(c)] if contexts.contains_key(c) => Some((items, *c)),
        _ => None,
    }
}

/// Every geometric representation context's units (`None` when unitless) and dimension.
fn context_table(file: &P21File, header: &StepHeaderInfo) -> BTreeMap<u64, (Option<Units>, u8)> {
    let mut out = BTreeMap::new();
    for (&n, inst) in &file.data {
        let Some((_, p)) = inst.parts.iter().find(|(id, _)| id.as_str() == "GEOMETRIC_REPRESENTATION_CONTEXT") else {
            continue;
        };
        let dim = match p.last() {
            Some(Param::Int(2)) => 2,
            _ => 3,
        };
        let units = header
            .contexts
            .get(&n)
            .map(|c| Units { length: c.length_factor_m, angle: c.angle.map_or(1.0, |a| a.factor_rad()) });
        out.insert(n, (units, dim));
    }
    out
}

/// Walks references from every representation's items, without entering other
/// representations or contexts, and assigns each reached entity its context's units.
fn assign_contexts(file: &P21File, ctx: &BTreeMap<u64, (Option<Units>, u8)>) -> BTreeMap<u64, Assign> {
    let mut reps: BTreeMap<u64, (Vec<u64>, u64)> = BTreeMap::new();
    for (&n, inst) in &file.data {
        for (_, params) in &inst.parts {
            if let Some((items, c)) = representation(params, ctx) {
                let refs: Vec<u64> =
                    items.iter().filter_map(|p| if let Param::Ref(r) = p { Some(*r) } else { None }).collect();
                reps.insert(n, (refs, c));
                break;
            }
        }
    }
    let stops: BTreeSet<u64> = reps.keys().copied().chain(ctx.keys().copied()).collect();
    let mut out: BTreeMap<u64, Assign> = BTreeMap::new();
    let mut stack: Vec<u64> = Vec::new();
    let mut params: Vec<&Param> = Vec::new();
    for (refs, c) in reps.values() {
        let Some(&(Some(units), dim)) = ctx.get(c) else { continue };
        let here = Assign::One { units, dim };
        stack.extend(refs.iter().rev());
        while let Some(e) = stack.pop() {
            if stops.contains(&e) {
                continue;
            }
            // Each entity is entered at most twice: when first assigned, and when it turns into
            // a conflict (so everything below it becomes a conflict too).
            match out.get(&e) {
                Some(a) if *a == here || *a == Assign::Conflict => continue,
                Some(_) => {
                    out.insert(e, Assign::Conflict);
                }
                None => {
                    out.insert(e, here);
                }
            }
            let Some(inst) = file.get(e) else { continue };
            for (_, ps) in &inst.parts {
                params.extend(ps.iter().rev());
                while let Some(p) = params.pop() {
                    match p {
                        Param::Ref(r) => stack.push(*r),
                        Param::List(items) => params.extend(items.iter().rev()),
                        Param::Typed(_, inner) => params.push(inner),
                        _ => {}
                    }
                }
            }
        }
    }
    out
}

// ------------------------------------------------------------------ mapping

/// Maps every geometric entity of `file` (REQ-IO.CAD.STEP.GEOM-001..005). `header` is the
/// resolved header of the same file (`resolve_header`).
pub fn map_geometry(file: &P21File, header: &StepHeaderInfo, opts: &GeomOptions) -> Result<GeomMap, GeomError> {
    if !opts.approx_tol_m.is_finite() || opts.approx_tol_m <= 0.0 {
        return Err(GeomError::BadOption { name: "approx_tol_m" });
    }
    let ctx = context_table(file, header);
    let assigned = assign_contexts(file, &ctx);
    // Units for entities no representation reaches: those of all 3D contexts, if they agree.
    let mut fallback: Option<Units> = None;
    let mut agree = true;
    for (units, dim) in ctx.values() {
        if let (Some(u), 3) = (units, *dim) {
            match fallback {
                None => fallback = Some(*u),
                Some(f) if f == *u => {}
                Some(_) => agree = false,
            }
        }
    }
    let fallback = if agree { fallback } else { None };

    let mut out = GeomMap::default();
    for (&n, inst) in &file.data {
        let kind = classify(inst);
        if kind == Kind::Other {
            continue;
        }
        let units = match assigned.get(&n) {
            Some(Assign::One { dim: 2, .. }) => continue,
            Some(Assign::One { units, .. }) => Ok(*units),
            Some(Assign::Conflict) => Err(SkipReason::ContextConflict),
            None => fallback.ok_or(SkipReason::NoContext),
        };
        let result = match units {
            Ok(units) => map_one(file, inst, kind, units, opts, &mut out).map_err(Fail::reason),
            Err(r) => Err(r),
        };
        if let Err(reason) = result {
            out.skipped.push(Skipped { entity: n, type_name: type_name(inst), reason });
        }
    }
    out.warnings.sort_by(|a, b| (a.entity, a.code).cmp(&(b.entity, b.code)));
    out.warnings.dedup_by(|a, b| a.entity == b.entity && a.code == b.code);
    Ok(out)
}

fn map_one(
    file: &P21File,
    inst: &Instance,
    kind: Kind,
    units: Units,
    opts: &GeomOptions,
    out: &mut GeomMap,
) -> Result<(), Fail> {
    let n = inst.number;
    match kind {
        Kind::Point => {
            // 2D points outside a 2D context are parameter-space data of a pcurve: not mapped.
            let raw = place::raw_point(file, n)?;
            if raw.len() == 3 {
                out.points.insert(n, place::point3(file, n, units)?);
            }
            Ok(())
        }
        Kind::Line
        | Kind::Circle
        | Kind::Ellipse
        | Kind::BsplineCurve
        | Kind::Polyline
        | Kind::Trimmed
        | Kind::SurfaceCurve
        | Kind::SeamCurve => {
            let c = curve::curve(file, inst, kind, units)?;
            out.curves.insert(n, c);
            Ok(())
        }
        Kind::Pcurve => {
            let pc = pcurve(file, inst, units)?;
            out.pcurves.insert(n, pc);
            Ok(())
        }
        Kind::Offset => {
            let p = match inst.parts.as_slice() {
                [(_, p)] => p,
                _ => return Err(Fail::Malformed { at: n, what: "unexpected_entity_type" }),
            };
            read::arity(p, 4, n)?;
            let base_n = read::reference(read::arg(p, 1, n)?, n)?;
            let d = read::real(read::arg(p, 2, n)?, n)? * units.length;
            let base_inst = read::get(file, base_n)?;
            let base_kind = classify(base_inst);
            if base_kind == Kind::Offset {
                return Err(Fail::Unsupported { at: base_n });
            }
            let base = map_surface(file, base_inst, base_kind, units, opts, &mut out.warnings)?;
            let (nurbs, approx) = approx::offset(&base, d, opts, opts.domains.get(&n))?;
            out.surfaces.insert(n, SurfaceRef::Approximated(nurbs));
            out.approximations.insert(n, approx);
            Ok(())
        }
        Kind::Unsupported => Err(Fail::Unsupported { at: n }),
        Kind::Other => Ok(()),
        _ => {
            let s = map_surface(file, inst, kind, units, opts, &mut out.warnings)?;
            out.surfaces.insert(n, s);
            Ok(())
        }
    }
}

fn map_surface(
    file: &P21File,
    inst: &Instance,
    kind: Kind,
    units: Units,
    opts: &GeomOptions,
    warnings: &mut Vec<GeomWarning>,
) -> Result<SurfaceRef, Fail> {
    match kind {
        Kind::Plane | Kind::Cylinder | Kind::Cone | Kind::Sphere | Kind::Torus => {
            Ok(SurfaceRef::Analytic(surface::analytic(file, inst, kind, units, warnings)?))
        }
        Kind::BsplineSurface => Ok(SurfaceRef::Nurbs(surface::bspline_surface(file, inst, units)?)),
        Kind::Extrusion => surface::extrusion(file, inst, units, opts.domains.get(&inst.number)),
        Kind::Revolution => surface::revolution(file, inst, units),
        _ => Err(Fail::Unsupported { at: inst.number }),
    }
}

fn pcurve(file: &P21File, inst: &Instance, units: Units) -> Result<Pcurve2, Fail> {
    let n = inst.number;
    let p = match inst.parts.as_slice() {
        [(_, p)] => p,
        _ => return Err(Fail::Malformed { at: n, what: "unexpected_entity_type" }),
    };
    read::arity(p, 3, n)?;
    let surface = read::reference(read::arg(p, 1, n)?, n)?;
    let representation = read::reference(read::arg(p, 2, n)?, n)?;
    let (scale, param_exact) = surface::param_scales(file, surface, units)?;
    let curve_entity = curve::definitional_item(file, representation)?;
    let curve = curve::curve2(file, curve_entity, scale)?;
    Ok(Pcurve2 { surface, representation, curve_entity, curve, scale, param_exact })
}

impl GeomMap {
    /// The whole result as deterministic text records (AC-5): one line per item in map order,
    /// every real as the 16 hex digits of its IEEE 754 bit pattern.
    pub fn to_records(&self) -> Vec<u8> {
        record::write(self).into_bytes()
    }
}
