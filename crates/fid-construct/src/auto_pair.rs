//! Nominal-actual point pairing with gates and conflict rule (spec node `KRN.CONS.AUTO.PAIR`).
//!
//! [`pair`] decides which actual points belong to which nominal element. For every element,
//! in element order, it visits the element's candidate points (or all points) in ascending
//! order and measures the orthogonal distance to the element's analytic surface and, when
//! the point carries a normal, the angle between the point normal and the nominal normal at
//! the foot point. A point passing both gates of an element is a *claim*. Claims are
//! resolved per point by the [`ConflictRule`], then every element left with fewer than
//! `min_points` points is marked [`PairStatus::Unpaired`] and its points are released to no
//! other element (REQ-KRN.CONS.AUTO.PAIR-001 .. -003).
//!
//! # Pinned rules
//!
//! - **Gates.** A claim needs `distance <= distance_gate_m` and, when the point has a
//!   normal, `angle <= normal_angle_gate_rad`, with `angle = atan2(|n_p x n_nom|,
//!   n_p . n_nom)` in `[0, pi]` (oriented: a point normal opposite to the nominal normal is
//!   at `pi`). For [`NominalPrimitive::CircleInPlane`] and [`NominalPrimitive::SlotInPlane`]
//!   the distance is the in-plane distance to the boundary curve and the out-of-plane offset
//!   must also be at most `distance_gate_m` (the out-of-plane gate reuses the distance gate).
//! - **Nominal normals.** A plane's normal is the given normal. Every other primitive has a
//!   [`NormalSense`]: `Outward` points away from the axis (cylinder, cone), the centre
//!   (sphere, circle), the circle of centres (torus) or the centre segment (slot); `Inward`
//!   is its opposite (a hole, a pocket, a concave fillet).
//! - **Undefined nominal normal.** Where the foot point has no unique normal (a point on a
//!   cylinder or cone axis, at a sphere centre, on a torus circle of centres, on a slot
//!   centre segment, or whose closest cone point is the apex), a point that carries a
//!   normal and passes the distance gate cannot be gated by angle. It is assigned to no
//!   element at all and listed in [`Pairing::normal_undefined`] (fail closed).
//! - **Conflicts.** [`ConflictRule::Nearest`]: smallest distance, then smallest angle (`0`
//!   for a point without normal), then lowest element index, by total order.
//!   [`ConflictRule::ExcludeAmbiguous`]: a point with two or more claims goes to no element
//!   and is listed in [`Pairing::ambiguous`].
//! - **Minimum points.** After conflict resolution an element with fewer than `min_points`
//!   points is `Unpaired { count }`; its points are listed in its
//!   [`ElementPairing::released`] and assigned to no element.
//! - **Accounting.** Every point is in exactly one of: an element's `indices`, an element's
//!   `released`, `ambiguous`, `normal_undefined`, or the `ungated` count (no claim at all).
//!   All index lists are ascending.
//! - **Gate floors.** `distance_gate_m` must be at least [`GATE_FLOOR_ULPS`] times
//!   `f64::EPSILON` times the largest coordinate magnitude of the points and elements (the
//!   rounding resolution of the distances), and `normal_angle_gate_rad` at least
//!   [`ANGLE_GATE_FLOOR`]; a smaller gate would let rounding decide the pairing.
//!
//! # Determinism
//!
//! D0: fixed iteration order, no threads, no hash maps, total-order comparisons, all math
//! through `fid_num` (no FMA, no `f64::min`/`max`). The result holds only integers, so its
//! canonical bytes ([`Pairing::canonical_bytes`]) are identical on x86-64, ARM64 and wasm32,
//! and do not depend on whether candidate lists are given, provided each list holds every
//! point that passes that element's gates (REQ-KRN.CONS.AUTO.PAIR-005; the caller's
//! contract, which this function cannot check without visiting every point).
//!
//! Every field of [`PairParams`] is required and part of the parameter digest
//! (REQ-KRN.CONS.AUTO.PAIR-004), carried as a [`ConstructParams`]: the distance gate is its
//! distance threshold, the angle gate its angular threshold, and `min_points` and
//! `conflict_rule` are named `count` values.

use core::cmp::Ordering;

use fid_fit::contract::{FitError, PointsView, V3};
use fid_kci::kci::sha256_digest;
use fid_num::{f as numf, red, NumError};
use fid_types::Digest;

use crate::contract::json::{self, J};
use crate::contract::{ConstructError, ConstructParams, QuantityKind};

/// The operation name in numeric errors.
pub const OP: &str = "construct.auto_pair";

/// The format tag inside [`Pairing::canonical_bytes`].
pub const PAIRING_FORMAT: &str = "construct.auto_pair.pairing@1";

/// Named `count` value holding `min_points`.
pub const PARAM_MIN_POINTS: &str = "min_points";

/// Named `count` value holding the conflict rule (`0` nearest, `1` exclude_ambiguous).
pub const PARAM_CONFLICT_RULE: &str = "conflict_rule";

/// Most nominal elements one call may pair against.
pub const MAX_ELEMENTS: usize = 1 << 16;

/// Largest accepted `| ||d|| - 1 |` of a nominal direction (plane normal, axis, slot axis),
/// and largest accepted `|slot_axis . plane_normal|`.
pub const DIR_UNIT_TOL: f64 = 1e-12;

/// The distance-gate floor in units of `f64::EPSILON` times the coordinate scale.
pub const GATE_FLOOR_ULPS: f64 = 16.0;

/// The smallest accepted angle gate, radians (`16 * f64::EPSILON`).
pub const ANGLE_GATE_FLOOR: f64 = 16.0 * f64::EPSILON;

/// How a point claimed by more than one element is resolved (REQ-KRN.CONS.AUTO.PAIR-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictRule {
    /// Smallest distance, then smallest angle, then lowest element index.
    Nearest,
    /// Assigned to no element and listed as ambiguous.
    ExcludeAmbiguous,
}

impl ConflictRule {
    /// The `count` value of [`PARAM_CONFLICT_RULE`].
    pub const fn as_param(self) -> f64 {
        match self {
            ConflictRule::Nearest => 0.0,
            ConflictRule::ExcludeAmbiguous => 1.0,
        }
    }

    /// The wire string.
    pub const fn as_str(self) -> &'static str {
        match self {
            ConflictRule::Nearest => "nearest",
            ConflictRule::ExcludeAmbiguous => "exclude_ambiguous",
        }
    }

    fn from_param(v: f64) -> Result<Self, ConstructError> {
        if v == 0.0 {
            Ok(ConflictRule::Nearest)
        } else if v == 1.0 {
            Ok(ConflictRule::ExcludeAmbiguous)
        } else {
            Err(ConstructError::InvalidParams { field: PARAM_CONFLICT_RULE })
        }
    }
}

/// The pairing parameters (REQ-KRN.CONS.AUTO.PAIR-004). Every field is required; fields
/// are private so every value went through [`PairParams::new`] or
/// [`PairParams::from_construct_params`].
#[derive(Debug, Clone, PartialEq)]
pub struct PairParams {
    distance_gate_m: f64,
    normal_angle_gate_rad: f64,
    min_points: u32,
    conflict_rule: ConflictRule,
    params: ConstructParams,
}

impl PairParams {
    /// Validates the parameters. [`ConstructError::InvalidParams`] names the field:
    ///
    /// - `distance_gate_m`: finite and `> 0` (a negative value, `-0.0` and `0` are refused;
    ///   the data-dependent floor is checked by [`pair`]);
    /// - `normal_angle_gate_rad`: finite, at least [`ANGLE_GATE_FLOOR`], at most `pi`;
    /// - `min_points`: at least 1 (an element paired with no point would be an empty result
    ///   without a reason).
    pub fn new(
        distance_gate_m: f64,
        normal_angle_gate_rad: f64,
        min_points: u32,
        conflict_rule: ConflictRule,
    ) -> Result<PairParams, ConstructError> {
        if !distance_gate_m.is_finite() || distance_gate_m <= 0.0 {
            return Err(ConstructError::InvalidParams { field: "distance_gate_m" });
        }
        if !(ANGLE_GATE_FLOOR..=core::f64::consts::PI).contains(&normal_angle_gate_rad) {
            return Err(ConstructError::InvalidParams { field: "normal_angle_gate_rad" });
        }
        if min_points == 0 {
            return Err(ConstructError::InvalidParams { field: PARAM_MIN_POINTS });
        }
        let params = ConstructParams::new(normal_angle_gate_rad, distance_gate_m)?
            .with(PARAM_MIN_POINTS, QuantityKind::Count, f64::from(min_points))?
            .with(PARAM_CONFLICT_RULE, QuantityKind::Count, conflict_rule.as_param())?;
        Ok(PairParams { distance_gate_m, normal_angle_gate_rad, min_points, conflict_rule, params })
    }

    /// Reads the parameters from a [`ConstructParams`] (as a construction command passes
    /// them): the distance threshold is the distance gate, the angular threshold the angle
    /// gate, and the named `count` values [`PARAM_MIN_POINTS`] and [`PARAM_CONFLICT_RULE`]
    /// are required ([`ConstructError::MissingParam`]; no defaults). Any other named value is
    /// refused ([`ConstructError::InvalidParams`] `values`), so the digest covers only what
    /// the pairing reads. Then as [`PairParams::new`].
    pub fn from_construct_params(p: &ConstructParams) -> Result<PairParams, ConstructError> {
        let min = p.get(PARAM_MIN_POINTS, QuantityKind::Count)?;
        let rule = ConflictRule::from_param(p.get(PARAM_CONFLICT_RULE, QuantityKind::Count)?)?;
        if p.values().count() != 2 {
            return Err(ConstructError::InvalidParams { field: "values" });
        }
        // A `count` is a non-negative integer (ConstructParams::with); bound it to u32.
        if !(1.0..=f64::from(u32::MAX)).contains(&min) {
            return Err(ConstructError::InvalidParams { field: PARAM_MIN_POINTS });
        }
        // In range and integral, so the conversion is exact.
        let min_points = min as u32;
        PairParams::new(p.distance_threshold(), p.angular_threshold(), min_points, rule)
    }

    /// The distance gate, metres.
    pub fn distance_gate_m(&self) -> f64 {
        self.distance_gate_m
    }

    /// The normal-angle gate, radians.
    pub fn normal_angle_gate_rad(&self) -> f64 {
        self.normal_angle_gate_rad
    }

    /// The minimum number of points an element must keep.
    pub fn min_points(&self) -> u32 {
        self.min_points
    }

    /// The conflict rule.
    pub fn conflict_rule(&self) -> ConflictRule {
        self.conflict_rule
    }

    /// The parameters as a [`ConstructParams`] (what the digest covers).
    pub fn construct_params(&self) -> &ConstructParams {
        &self.params
    }

    /// The parameter digest (`ConstructParams::digest`).
    pub fn digest(&self) -> Digest {
        self.params.digest()
    }
}

/// Which way the nominal normal of a curved primitive points (module documentation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalSense {
    /// Away from the axis, centre, circle of centres or centre segment.
    Outward,
    /// Towards it.
    Inward,
}

impl NormalSense {
    fn sign(self) -> f64 {
        match self {
            NormalSense::Outward => 1.0,
            NormalSense::Inward => -1.0,
        }
    }
}

/// A nominal element as an analytic primitive, SI metres and radians. Directions must be
/// unit length within [`DIR_UNIT_TOL`]; surfaces are unbounded (a plane, an infinite
/// cylinder, one nappe of a cone), so callers bound the work with candidate lists.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NominalPrimitive {
    /// The plane through `origin` with unit `normal` (the nominal normal).
    Plane { origin: V3, normal: V3 },
    /// The cylinder of `radius > 0` around the line through `axis_point` along unit `axis`.
    Cylinder { axis_point: V3, axis: V3, radius: f64, sense: NormalSense },
    /// The nappe from `apex` opening along unit `axis`, `half_angle_rad` in `(0, pi/2)`.
    Cone { apex: V3, axis: V3, half_angle_rad: f64, sense: NormalSense },
    /// The sphere of `radius > 0` around `centre`.
    Sphere { centre: V3, radius: f64, sense: NormalSense },
    /// The ring torus around `centre` with unit `axis`, `0 < minor_radius < major_radius`.
    Torus { centre: V3, axis: V3, major_radius: f64, minor_radius: f64, sense: NormalSense },
    /// The circle of `radius > 0` around `centre` in the plane with unit `plane_normal`.
    CircleInPlane { centre: V3, plane_normal: V3, radius: f64, sense: NormalSense },
    /// The slot (stadium) boundary in the plane with unit `plane_normal`: the points at
    /// `radius > 0` from the centre segment `centre +- half_length * slot_axis`, with unit
    /// `slot_axis` perpendicular to `plane_normal` and `half_length >= 0`.
    SlotInPlane { centre: V3, plane_normal: V3, slot_axis: V3, half_length: f64, radius: f64, sense: NormalSense },
}

impl NominalPrimitive {
    /// The kind as a wire string.
    pub const fn kind_str(&self) -> &'static str {
        match self {
            NominalPrimitive::Plane { .. } => "plane",
            NominalPrimitive::Cylinder { .. } => "cylinder",
            NominalPrimitive::Cone { .. } => "cone",
            NominalPrimitive::Sphere { .. } => "sphere",
            NominalPrimitive::Torus { .. } => "torus",
            NominalPrimitive::CircleInPlane { .. } => "circle_in_plane",
            NominalPrimitive::SlotInPlane { .. } => "slot_in_plane",
        }
    }
}

/// Whether an element kept its points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairStatus {
    /// At least `min_points` points after conflict resolution.
    Paired,
    /// Fewer than `min_points`; `count` is how many it had after conflict resolution.
    Unpaired { count: u32 },
}

/// The points of one element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementPairing {
    indices: Vec<u32>,
    released: Vec<u32>,
    count: u32,
    status: PairStatus,
}

impl ElementPairing {
    /// The assigned point indices, ascending (empty when unpaired).
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// The point indices the element won but released because it is unpaired, ascending
    /// (empty when paired).
    pub fn released(&self) -> &[u32] {
        &self.released
    }

    /// How many points the element held after conflict resolution (`indices` when paired,
    /// `released` when unpaired).
    pub fn count(&self) -> u32 {
        self.count
    }

    /// The status.
    pub fn status(&self) -> PairStatus {
        self.status
    }
}

/// The result of [`pair`] (module documentation, "Accounting").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pairing {
    points: u32,
    params_digest: Digest,
    per_element: Vec<ElementPairing>,
    ambiguous: Vec<u32>,
    normal_undefined: Vec<u32>,
    ungated: u32,
}

impl Pairing {
    /// Number of input points.
    pub fn points(&self) -> u32 {
        self.points
    }

    /// The digest of the parameters the pairing ran under.
    pub fn params_digest(&self) -> &Digest {
        &self.params_digest
    }

    /// One entry per nominal element, in element order.
    pub fn per_element(&self) -> &[ElementPairing] {
        &self.per_element
    }

    /// Points claimed by two or more elements under `exclude_ambiguous`, ascending.
    pub fn ambiguous(&self) -> &[u32] {
        &self.ambiguous
    }

    /// Points that passed an element's distance gate where its nominal normal is undefined,
    /// ascending; assigned to no element.
    pub fn normal_undefined(&self) -> &[u32] {
        &self.normal_undefined
    }

    /// Points no element claimed.
    pub fn ungated(&self) -> u32 {
        self.ungated
    }

    /// The canonical bytes: RFC 8785 JSON, integers as decimal strings, keys sorted:
    ///
    /// ```text
    /// {"ambiguous":[..],"elements":[{"count":"<n>","indices":[..],"released":[..],
    ///  "status":"paired"|"unpaired"},..],"format":"construct.auto_pair.pairing@1",
    ///  "normal_undefined":[..],"params_digest":"sha256:<hex>","points":"<n>",
    ///  "ungated":"<n>"}
    /// ```
    ///
    /// (shown wrapped; the bytes have no whitespace). `count` is the number of points the
    /// element held after conflict resolution (`indices` when paired, `released` when not).
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let elements = self
            .per_element
            .iter()
            .map(|e| {
                let status = match e.status {
                    PairStatus::Paired => "paired",
                    PairStatus::Unpaired { .. } => "unpaired",
                };
                J::O(vec![
                    ("count", int(e.count)),
                    ("indices", ints(&e.indices)),
                    ("released", ints(&e.released)),
                    ("status", json::s(status)),
                ])
            })
            .collect();
        json::write(&J::O(vec![
            ("ambiguous", ints(&self.ambiguous)),
            ("elements", J::A(elements)),
            ("format", json::s(PAIRING_FORMAT)),
            ("normal_undefined", ints(&self.normal_undefined)),
            ("params_digest", json::s(self.params_digest.as_str())),
            ("points", int(self.points)),
            ("ungated", int(self.ungated)),
        ]))
    }

    /// SHA-256 of [`Pairing::canonical_bytes`].
    pub fn digest(&self) -> Digest {
        sha256_digest(&self.canonical_bytes())
    }
}

fn int(x: u32) -> J {
    J::S(x.to_string())
}

fn ints(xs: &[u32]) -> J {
    J::A(xs.iter().map(|&x| int(x)).collect())
}

/// Sets the point index of a numeric error raised while measuring point `i`.
fn at_point(e: ConstructError, i: usize) -> ConstructError {
    match e {
        ConstructError::Num(NumError::NotANumber { op, index: None }) => {
            ConstructError::Num(NumError::NotANumber { op, index: Some(i) })
        }
        ConstructError::Num(NumError::Infinite { op, index: None }) => {
            ConstructError::Num(NumError::Infinite { op, index: Some(i) })
        }
        other => other,
    }
}

// ------------------------------------------------------------------ vector helpers (KNP-1)

fn sub3(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale3(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn add_scaled(a: V3, b: V3, s: f64) -> V3 {
    [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s]
}

fn dot3(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn norm3(a: V3) -> Result<f64, ConstructError> {
    Ok(red::norm2(&a)?)
}

fn abs_gt(m: &mut f64, x: f64) {
    let a = x.abs();
    if a > *m {
        *m = a;
    }
}

// ------------------------------------------------------------------ prepared elements

/// A validated element in evaluation form.
#[derive(Debug, Clone, Copy)]
enum Prep {
    Plane { o: V3, n: V3 },
    Cyl { o: V3, a: V3, r: f64, s: f64 },
    Cone { apex: V3, a: V3, sa: f64, ca: f64, s: f64 },
    Sphere { c: V3, r: f64, s: f64 },
    Torus { c: V3, a: V3, big: f64, small: f64, s: f64 },
    Circle { c: V3, n: V3, r: f64, s: f64 },
    Slot { c: V3, n: V3, u: V3, v: V3, half: f64, r: f64, s: f64 },
}

/// The outcome of measuring one point against one element.
enum Foot {
    /// Outside the distance (or out-of-plane) gate.
    Far,
    /// Within the gates; the nominal normal at the foot, or `None` where it is undefined.
    Near { dist: f64, normal: Option<V3> },
}

fn finite(e: usize, xs: &[f64]) -> Result<(), ConstructError> {
    for &x in xs {
        if x.is_nan() {
            return Err(NumError::NotANumber { op: OP, index: Some(e) }.into());
        }
        if x.is_infinite() {
            return Err(NumError::Infinite { op: OP, index: Some(e) }.into());
        }
    }
    Ok(())
}

fn unit_dir(e: usize, d: V3) -> Result<V3, ConstructError> {
    let len = norm3(d)?;
    let dev = len - 1.0;
    if dev.abs() > DIR_UNIT_TOL {
        return Err(ConstructError::InvalidFeature { index: Some(e), code: "invalid_direction" });
    }
    Ok(d)
}

fn positive(e: usize, x: f64) -> Result<f64, ConstructError> {
    if x > 0.0 {
        Ok(x)
    } else {
        Err(ConstructError::InvalidFeature { index: Some(e), code: "invalid_params" })
    }
}

/// Validates element `e` and returns its evaluation form and its coordinate scale (the
/// largest magnitude among its coordinates, plus its radii and half-length).
fn prepare(e: usize, el: &NominalPrimitive) -> Result<(Prep, f64), ConstructError> {
    let bad = ConstructError::InvalidFeature { index: Some(e), code: "invalid_params" };
    let mut m = 0.0_f64;
    let prep = match *el {
        NominalPrimitive::Plane { origin, normal } => {
            finite(e, &origin)?;
            finite(e, &normal)?;
            origin.iter().for_each(|&x| abs_gt(&mut m, x));
            Prep::Plane { o: origin, n: unit_dir(e, normal)? }
        }
        NominalPrimitive::Cylinder { axis_point, axis, radius, sense } => {
            finite(e, &axis_point)?;
            finite(e, &axis)?;
            finite(e, &[radius])?;
            axis_point.iter().for_each(|&x| abs_gt(&mut m, x));
            m += positive(e, radius)?;
            Prep::Cyl { o: axis_point, a: unit_dir(e, axis)?, r: radius, s: sense.sign() }
        }
        NominalPrimitive::Cone { apex, axis, half_angle_rad, sense } => {
            finite(e, &apex)?;
            finite(e, &axis)?;
            finite(e, &[half_angle_rad])?;
            apex.iter().for_each(|&x| abs_gt(&mut m, x));
            if half_angle_rad <= 0.0 || half_angle_rad >= core::f64::consts::FRAC_PI_2 {
                return Err(bad);
            }
            let sa = numf::sin(half_angle_rad)?;
            let ca = numf::cos(half_angle_rad)?;
            Prep::Cone { apex, a: unit_dir(e, axis)?, sa, ca, s: sense.sign() }
        }
        NominalPrimitive::Sphere { centre, radius, sense } => {
            finite(e, &centre)?;
            finite(e, &[radius])?;
            centre.iter().for_each(|&x| abs_gt(&mut m, x));
            m += positive(e, radius)?;
            Prep::Sphere { c: centre, r: radius, s: sense.sign() }
        }
        NominalPrimitive::Torus { centre, axis, major_radius, minor_radius, sense } => {
            finite(e, &centre)?;
            finite(e, &axis)?;
            finite(e, &[major_radius, minor_radius])?;
            centre.iter().for_each(|&x| abs_gt(&mut m, x));
            if minor_radius <= 0.0 || minor_radius >= major_radius {
                return Err(bad);
            }
            m += major_radius + minor_radius;
            Prep::Torus { c: centre, a: unit_dir(e, axis)?, big: major_radius, small: minor_radius, s: sense.sign() }
        }
        NominalPrimitive::CircleInPlane { centre, plane_normal, radius, sense } => {
            finite(e, &centre)?;
            finite(e, &plane_normal)?;
            finite(e, &[radius])?;
            centre.iter().for_each(|&x| abs_gt(&mut m, x));
            m += positive(e, radius)?;
            Prep::Circle { c: centre, n: unit_dir(e, plane_normal)?, r: radius, s: sense.sign() }
        }
        NominalPrimitive::SlotInPlane { centre, plane_normal, slot_axis, half_length, radius, sense } => {
            finite(e, &centre)?;
            finite(e, &plane_normal)?;
            finite(e, &slot_axis)?;
            finite(e, &[half_length, radius])?;
            centre.iter().for_each(|&x| abs_gt(&mut m, x));
            if half_length.is_sign_negative() {
                return Err(bad);
            }
            m += half_length + positive(e, radius)?;
            let n = unit_dir(e, plane_normal)?;
            let u = unit_dir(e, slot_axis)?;
            if dot3(n, u).abs() > DIR_UNIT_TOL {
                return Err(ConstructError::InvalidFeature { index: Some(e), code: "invalid_direction" });
            }
            Prep::Slot { c: centre, n, u, v: cross3(n, u), half: half_length + 0.0, r: radius, s: sense.sign() }
        }
    };
    if !m.is_finite() {
        return Err(NumError::Infinite { op: OP, index: Some(e) }.into());
    }
    Ok((prep, m))
}

/// `w / len` scaled by `s`, or `None` when `len` is zero.
fn dir(w: V3, len: f64, s: f64) -> Option<V3> {
    if len == 0.0 {
        None
    } else {
        Some(scale3(w, s / len))
    }
}

impl Prep {
    /// Measures `p` against the element under the distance gate `gate`.
    fn foot(&self, p: V3, gate: f64) -> Result<Foot, ConstructError> {
        let (dist, normal) = match *self {
            Prep::Plane { o, n } => (dot3(sub3(p, o), n).abs(), Some(n)),
            Prep::Cyl { o, a, r, s } => {
                let v = sub3(p, o);
                let w = add_scaled(v, a, -dot3(v, a));
                let rho = norm3(w)?;
                ((rho - r).abs(), dir(w, rho, s))
            }
            Prep::Cone { apex, a, sa, ca, s } => {
                let v = sub3(p, apex);
                let t = dot3(v, a);
                let w = add_scaled(v, a, -t);
                let rho = norm3(w)?;
                // Foot parameter along the generator in the meridian half-plane of `p`.
                let along = t * ca + rho * sa;
                if along < 0.0 {
                    // The closest point of the nappe is the apex: no unique normal.
                    (norm3(v)?, None)
                } else {
                    let signed = rho * ca - t * sa;
                    let normal = if rho == 0.0 {
                        None
                    } else {
                        let e = scale3(w, 1.0 / rho);
                        Some(scale3([e[0] * ca - a[0] * sa, e[1] * ca - a[1] * sa, e[2] * ca - a[2] * sa], s))
                    };
                    (signed.abs(), normal)
                }
            }
            Prep::Sphere { c, r, s } => {
                let v = sub3(p, c);
                let d = norm3(v)?;
                ((d - r).abs(), dir(v, d, s))
            }
            Prep::Torus { c, a, big, small, s } => {
                let v = sub3(p, c);
                let t = dot3(v, a);
                let w = add_scaled(v, a, -t);
                let rho = norm3(w)?;
                if rho == 0.0 {
                    // Every point of the circle of centres is equally close.
                    let dc = numf::hypot(big, t)?;
                    ((dc - small).abs(), None)
                } else {
                    let q = sub3(v, scale3(w, big / rho));
                    let dq = norm3(q)?;
                    ((dq - small).abs(), dir(q, dq, s))
                }
            }
            Prep::Circle { c, n, r, s } => {
                let v = sub3(p, c);
                let h = dot3(v, n);
                if !h.is_finite() {
                    return Err(NumError::Infinite { op: OP, index: None }.into());
                }
                if h.abs() > gate {
                    return Ok(Foot::Far);
                }
                let w = add_scaled(v, n, -h);
                let d = norm3(w)?;
                ((d - r).abs(), dir(w, d, s))
            }
            Prep::Slot { c, n, u, v, half, r, s } => {
                let q = sub3(p, c);
                let h = dot3(q, n);
                if !h.is_finite() {
                    return Err(NumError::Infinite { op: OP, index: None }.into());
                }
                if h.abs() > gate {
                    return Ok(Foot::Far);
                }
                let x = dot3(q, u);
                let y = dot3(q, v);
                let xc = numf::total_clamp(x, -half, half)?;
                let dx = x - xc;
                let w = add_scaled(scale3(u, dx), v, y);
                let d = numf::hypot(dx, y)?;
                ((d - r).abs(), dir(w, d, s))
            }
        };
        if dist.is_nan() {
            return Err(NumError::NotANumber { op: OP, index: None }.into());
        }
        if dist.is_infinite() {
            return Err(NumError::Infinite { op: OP, index: None }.into());
        }
        if dist > gate {
            return Ok(Foot::Far);
        }
        Ok(Foot::Near { dist: dist + 0.0, normal })
    }
}

// ------------------------------------------------------------------ pairing

/// The best claim on a point so far.
#[derive(Debug, Clone, Copy)]
struct Best {
    dist: f64,
    angle: f64,
    elem: u32,
}

/// The claim state of one point.
#[derive(Debug, Clone, Copy, Default)]
struct PointClaims {
    best: Option<Best>,
    claims: u32,
    undefined: bool,
}

/// Whether `b` beats `a` under the `nearest` order (distance, angle; a tie keeps `a`, which
/// came from a lower element index).
fn beats(b: &Best, a: &Best) -> Result<bool, ConstructError> {
    Ok(match numf::total_cmp(b.dist, a.dist)? {
        Ordering::Less => true,
        Ordering::Greater => false,
        Ordering::Equal => numf::total_cmp(b.angle, a.angle)? == Ordering::Less,
    })
}

fn candidates_ok(candidates: &[Vec<u32>], n_elems: usize, n: usize) -> Result<(), ConstructError> {
    let bad = ConstructError::InvalidParams { field: "candidates" };
    if candidates.len() != n_elems {
        return Err(bad);
    }
    for list in candidates {
        let mut prev: Option<u32> = None;
        for &i in list {
            if usize::try_from(i).map_or(true, |i| i >= n) || prev.is_some_and(|p| i <= p) {
                return Err(bad);
            }
            prev = Some(i);
        }
    }
    Ok(())
}

/// Pairs the actual `points` with the nominal `elems` under `p` (module documentation).
///
/// `normals_required`: refuse a view without normals ([`ConstructError::InvalidParams`]
/// `normals`); with normals present the angle gate applies whatever this flag says.
/// `candidates`: one strictly ascending list of point indices per element, each index in
/// range, bounding which points each element visits.
///
/// Errors (check order): `InvalidParams` `normals`; `InvalidParams` `elems` (none, or more
/// than [`MAX_ELEMENTS`]); per element in order, `Num` (`NotANumber`/`Infinite`, index =
/// element) for a non-finite value and `InvalidFeature` (index = element; `invalid_direction`
/// or `invalid_params`) for a malformed primitive; `InvalidParams` `candidates`;
/// `InvalidParams` `distance_gate_m` below the data-dependent floor; `InvalidParams`
/// `points` when the per-point table cannot be allocated; `Num` when a measured distance
/// or angle is not finite (coordinates near the `f64` range). No partial pairing is
/// returned with an error.
pub fn pair(
    points: &PointsView<'_>,
    normals_required: bool,
    elems: &[NominalPrimitive],
    candidates: Option<&[Vec<u32>]>,
    p: &PairParams,
) -> Result<Pairing, ConstructError> {
    if normals_required && points.normals().is_none() {
        return Err(ConstructError::InvalidParams { field: "normals" });
    }
    if elems.is_empty() || elems.len() > MAX_ELEMENTS {
        return Err(ConstructError::InvalidParams { field: "elems" });
    }
    let n = points.len();
    let n32 = u32::try_from(n).map_err(|_| ConstructError::InvalidParams { field: "points" })?;
    let mut scale = 0.0_f64;
    let mut preps = Vec::new();
    preps.try_reserve_exact(elems.len()).map_err(|_| ConstructError::InvalidParams { field: "elems" })?;
    for (e, el) in elems.iter().enumerate() {
        let (prep, m) = prepare(e, el)?;
        if m > scale {
            scale = m;
        }
        preps.push(prep);
    }
    if let Some(c) = candidates {
        candidates_ok(c, elems.len(), n)?;
    }
    for &x in points.xyz() {
        abs_gt(&mut scale, x);
    }
    let gate = p.distance_gate_m();
    let gate_angle = p.normal_angle_gate_rad();
    if gate < GATE_FLOOR_ULPS * f64::EPSILON * scale {
        return Err(ConstructError::InvalidParams { field: "distance_gate_m" });
    }

    let mut table: Vec<PointClaims> = Vec::new();
    table.try_reserve_exact(n).map_err(|_| ConstructError::InvalidParams { field: "points" })?;
    table.resize(n, PointClaims::default());

    for (e, prep) in preps.iter().enumerate() {
        let elem = u32::try_from(e).map_err(|_| ConstructError::InvalidParams { field: "elems" })?;
        let mut visit = |i: usize| -> Result<(), ConstructError> {
            let (Some(pt), Some(slot)) = (points.point(i), table.get_mut(i)) else {
                return Err(ConstructError::InvalidParams { field: "candidates" });
            };
            let Foot::Near { dist, normal } = prep.foot(pt, gate).map_err(|err| at_point(err, i))? else {
                return Ok(());
            };
            let angle = match (points.normal(i), normal) {
                (None, _) => 0.0,
                (Some(_), None) => {
                    slot.undefined = true;
                    return Ok(());
                }
                (Some(pn), Some(nn)) => {
                    let sn = norm3(cross3(pn, nn)).map_err(|err| at_point(err, i))?;
                    let a = numf::atan2(sn, dot3(pn, nn)).map_err(|err| at_point(err.into(), i))?;
                    if a > gate_angle {
                        return Ok(());
                    }
                    a + 0.0
                }
            };
            let claim = Best { dist, angle, elem };
            slot.claims = slot.claims.saturating_add(1);
            let better = match &slot.best {
                None => true,
                Some(cur) => beats(&claim, cur)?,
            };
            if better {
                slot.best = Some(claim);
            }
            Ok(())
        };
        match candidates.and_then(|c| c.get(e)) {
            Some(list) => {
                for &i in list {
                    let i = usize::try_from(i).map_err(|_| ConstructError::InvalidParams { field: "candidates" })?;
                    visit(i)?;
                }
            }
            None => {
                for i in 0..n {
                    visit(i)?;
                }
            }
        }
    }

    // Conflict resolution in point order.
    let mut won: Vec<Vec<u32>> = vec![Vec::new(); elems.len()];
    let mut ambiguous = Vec::new();
    let mut normal_undefined = Vec::new();
    let mut ungated = 0_u32;
    for (i, slot) in table.iter().enumerate() {
        let idx = u32::try_from(i).map_err(|_| ConstructError::InvalidParams { field: "points" })?;
        if slot.undefined {
            normal_undefined.push(idx);
            continue;
        }
        let Some(best) = slot.best else {
            ungated = ungated.checked_add(1).ok_or(ConstructError::InvalidParams { field: "points" })?;
            continue;
        };
        if slot.claims >= 2 && p.conflict_rule() == ConflictRule::ExcludeAmbiguous {
            ambiguous.push(idx);
            continue;
        }
        let Some(list) = usize::try_from(best.elem).ok().and_then(|e| won.get_mut(e)) else {
            return Err(ConstructError::InvalidParams { field: "elems" });
        };
        list.push(idx);
    }

    let mut per_element = Vec::new();
    per_element.try_reserve_exact(won.len()).map_err(|_| ConstructError::InvalidParams { field: "elems" })?;
    for list in won {
        let count = u32::try_from(list.len()).map_err(|_| ConstructError::InvalidParams { field: "points" })?;
        per_element.push(if count < p.min_points() {
            ElementPairing { indices: Vec::new(), released: list, count, status: PairStatus::Unpaired { count } }
        } else {
            ElementPairing { indices: list, released: Vec::new(), count, status: PairStatus::Paired }
        });
    }

    Ok(Pairing { points: n32, params_digest: p.digest(), per_element, ambiguous, normal_undefined, ungated })
}

/// [`pair`] on raw buffers (`xyz` triples and optional unit normals, SI metres). A NaN or
/// infinite coordinate is [`ConstructError::Num`] (`NotANumber`/`Infinite`) with the point
/// index, op [`OP`] (and op `construct.auto_pair.normals` for a normal); any other buffer
/// defect `fid_fit::contract::PointsView` refuses is [`ConstructError::InvalidParams`]
/// `points` (length, size, subnormal coordinate) or `normals` (length, non-unit normal).
pub fn pair_buffers(
    xyz: &[f64],
    normals: Option<&[f64]>,
    normals_required: bool,
    elems: &[NominalPrimitive],
    candidates: Option<&[Vec<u32>]>,
    p: &PairParams,
) -> Result<Pairing, ConstructError> {
    const OP_NORMALS: &str = "construct.auto_pair.normals";
    let view = PointsView::new(xyz, normals).map_err(|e| {
        let nan_at = |buf: Option<&[f64]>, index: usize, op: &'static str| {
            let nan = buf
                .and_then(|b| b.get(index.saturating_mul(3)..index.saturating_mul(3).saturating_add(3)))
                .is_some_and(|c| c.iter().any(|x| x.is_nan()));
            if nan {
                ConstructError::Num(NumError::NotANumber { op, index: Some(index) })
            } else {
                ConstructError::Num(NumError::Infinite { op, index: Some(index) })
            }
        };
        match e {
            FitError::NonFinite { index } => nan_at(Some(xyz), index, OP),
            FitError::NonFiniteNormal { index } => nan_at(normals, index, OP_NORMALS),
            FitError::LengthMismatch { .. } | FitError::NonUnitNormal { .. } => {
                ConstructError::InvalidParams { field: "normals" }
            }
            _ => ConstructError::InvalidParams { field: "points" },
        }
    })?;
    pair(&view, normals_required, elems, candidates, p)
}
