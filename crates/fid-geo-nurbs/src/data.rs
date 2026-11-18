//! Validated B-spline and NURBS data. Spec node `GEO.NURBS.DATA`.
//!
//! Construction checks everything once and returns the first failure as a typed
//! [`NurbsError`]; a value that exists is valid. Inputs are stored exactly as given:
//! no knot normalisation and no weight rescaling (REQ-GEO.NURBS.DATA-006).
//!
//! Validation passes, in this order, each scanning in index order:
//! 1. degree in `1..=MAX_DEGREE` (curve; surface `u` then `v`): [`NurbsError::BadDegree`];
//! 2. knot count = control points + degree + 1, with at least degree + 1 control points
//!    (per direction): [`NurbsError::CountMismatch`];
//! 3. surfaces only: control net length = `n_u * n_v`: [`NurbsError::NetShape`];
//! 4. knots (per direction): finite and non-decreasing, then multiplicity at most degree
//!    for interior values and at most degree + 1 for the first and last value, then a
//!    non-empty domain `knots[p] < knots[n]`: [`NurbsError::BadKnots`];
//! 5. control points finite: [`NurbsError::NonFiniteCtrl`];
//! 6. weights, if present: one per control point ([`NurbsError::WeightCount`]), each finite
//!    and strictly positive ([`NurbsError::BadWeight`]).

use core::fmt;

/// Highest supported degree (REQ-GEO.NURBS.DATA-004).
pub const MAX_DEGREE: usize = 25;

/// The parameter direction an error refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// The single parameter of a curve (also used by a stand-alone [`KnotVector`]).
    Curve,
    /// The first surface parameter (rows of the control net).
    U,
    /// The second surface parameter (columns of the control net).
    V,
}

impl Dir {
    /// Stable lowercase name: `curve`, `u` or `v`.
    pub fn as_str(self) -> &'static str {
        match self {
            Dir::Curve => "curve",
            Dir::U => "u",
            Dir::V => "v",
        }
    }
}

/// Why a knot vector, curve or surface was rejected. Indices are zero-based; for a
/// surface, control-point and weight indices are flat row-major indices `i * n_v + j`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NurbsError {
    /// Degree is 0 or above [`MAX_DEGREE`].
    BadDegree { dir: Dir, degree: usize },
    /// Knot count differs from control points + degree + 1, or there are fewer than
    /// degree + 1 control points.
    CountMismatch { dir: Dir, knots: usize, ctrl: usize, degree: usize },
    /// The knot at `index` is non-finite, decreasing, exceeds the allowed multiplicity,
    /// or (`index = n`) closes an empty domain `knots[p] == knots[n]`.
    BadKnots { dir: Dir, index: usize },
    /// A surface control net does not hold `n_u * n_v` points.
    NetShape { ctrl: usize, n_u: usize, n_v: usize },
    /// A control point has a NaN or infinite coordinate.
    NonFiniteCtrl { index: usize },
    /// The weight vector does not have one weight per control point.
    WeightCount { weights: usize, ctrl: usize },
    /// A weight is non-finite or not strictly positive.
    BadWeight { index: usize },
}

impl NurbsError {
    /// Stable variant name, e.g. `BadKnots`.
    pub fn code(&self) -> &'static str {
        match self {
            NurbsError::BadDegree { .. } => "BadDegree",
            NurbsError::CountMismatch { .. } => "CountMismatch",
            NurbsError::BadKnots { .. } => "BadKnots",
            NurbsError::NetShape { .. } => "NetShape",
            NurbsError::NonFiniteCtrl { .. } => "NonFiniteCtrl",
            NurbsError::WeightCount { .. } => "WeightCount",
            NurbsError::BadWeight { .. } => "BadWeight",
        }
    }
}

impl fmt::Display for NurbsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NurbsError::BadDegree { dir, degree } => {
                write!(f, "{}: degree {degree} outside 1..={MAX_DEGREE}", dir.as_str())
            }
            NurbsError::CountMismatch { dir, knots, ctrl, degree } => {
                write!(f, "{}: {knots} knots do not fit {ctrl} control points of degree {degree}", dir.as_str())
            }
            NurbsError::BadKnots { dir, index } => write!(f, "{}: bad knot at index {index}", dir.as_str()),
            NurbsError::NetShape { ctrl, n_u, n_v } => {
                write!(f, "control net has {ctrl} points, expected {n_u} x {n_v}")
            }
            NurbsError::NonFiniteCtrl { index } => write!(f, "non-finite control point at index {index}"),
            NurbsError::WeightCount { weights, ctrl } => {
                write!(f, "{weights} weights for {ctrl} control points")
            }
            NurbsError::BadWeight { index } => write!(f, "bad weight at index {index}"),
        }
    }
}

impl core::error::Error for NurbsError {}

fn check_degree(dir: Dir, degree: usize) -> Result<(), NurbsError> {
    if degree == 0 || degree > MAX_DEGREE {
        Err(NurbsError::BadDegree { dir, degree })
    } else {
        Ok(())
    }
}

fn check_count(dir: Dir, knots: usize, ctrl: usize, degree: usize) -> Result<(), NurbsError> {
    // degree <= MAX_DEGREE here, so degree + 1 cannot overflow; ctrl + degree + 1 may.
    let fits = ctrl.checked_add(degree + 1) == Some(knots);
    if ctrl < degree + 1 || !fits {
        Err(NurbsError::CountMismatch { dir, knots, ctrl, degree })
    } else {
        Ok(())
    }
}

/// Knot checks (pass 4). The count has already been checked, so `knots.len() >= 2p + 2`.
fn check_knots(dir: Dir, knots: &[f64], degree: usize) -> Result<(), NurbsError> {
    let mut prev: Option<f64> = None;
    for (index, &k) in knots.iter().enumerate() {
        if !k.is_finite() {
            return Err(NurbsError::BadKnots { dir, index });
        }
        if let Some(p) = prev {
            if k < p {
                return Err(NurbsError::BadKnots { dir, index });
            }
        }
        prev = Some(k);
    }
    let (first, last) = match (knots.first(), knots.last()) {
        (Some(&a), Some(&b)) => (a, b),
        _ => return Err(NurbsError::BadKnots { dir, index: 0 }),
    };
    let mut run = 0usize;
    let mut prev: Option<f64> = None;
    for (index, &k) in knots.iter().enumerate() {
        run = if prev == Some(k) { run + 1 } else { 1 };
        prev = Some(k);
        let limit = if k == first || k == last { degree + 1 } else { degree };
        if run > limit {
            return Err(NurbsError::BadKnots { dir, index });
        }
    }
    let n = knots.len() - degree - 1;
    match (knots.get(degree), knots.get(n)) {
        (Some(&a), Some(&b)) if a < b => Ok(()),
        _ => Err(NurbsError::BadKnots { dir, index: n }),
    }
}

fn check_ctrl(ctrl: &[[f64; 3]]) -> Result<(), NurbsError> {
    for (index, c) in ctrl.iter().enumerate() {
        if !(c[0].is_finite() && c[1].is_finite() && c[2].is_finite()) {
            return Err(NurbsError::NonFiniteCtrl { index });
        }
    }
    Ok(())
}

fn check_weights(weights: Option<&[f64]>, ctrl: usize) -> Result<(), NurbsError> {
    let Some(w) = weights else { return Ok(()) };
    if w.len() != ctrl {
        return Err(NurbsError::WeightCount { weights: w.len(), ctrl });
    }
    for (index, &x) in w.iter().enumerate() {
        if !(x.is_finite() && x > 0.0) {
            return Err(NurbsError::BadWeight { index });
        }
    }
    Ok(())
}

fn rational(weights: Option<&[f64]>) -> bool {
    match weights {
        Some([first, rest @ ..]) => rest.iter().any(|w| w != first),
        _ => false,
    }
}

/// A validated knot vector for a given degree: finite, non-decreasing, interior
/// multiplicity at most the degree, end multiplicity at most degree + 1, non-empty domain.
#[derive(Debug, Clone, PartialEq)]
pub struct KnotVector {
    knots: Vec<f64>,
    degree: usize,
}

impl KnotVector {
    /// Validates `knots` for `degree` (errors name [`Dir::Curve`]). The knots are stored
    /// unchanged; the implied number of control points is `knots.len() - degree - 1`.
    pub fn new(knots: Vec<f64>, degree: usize) -> Result<Self, NurbsError> {
        Self::validated(Dir::Curve, knots, degree)
    }

    fn validated(dir: Dir, knots: Vec<f64>, degree: usize) -> Result<Self, NurbsError> {
        check_degree(dir, degree)?;
        let ctrl = knots.len().saturating_sub(degree + 1);
        check_count(dir, knots.len(), ctrl, degree)?;
        check_knots(dir, &knots, degree)?;
        Ok(KnotVector { knots, degree })
    }

    /// The knots, exactly as given.
    pub fn as_slice(&self) -> &[f64] {
        &self.knots
    }

    /// The degree the vector was validated for.
    pub fn degree(&self) -> usize {
        self.degree
    }

    /// Number of control points this vector fits: `len - degree - 1`.
    pub fn n_ctrl(&self) -> usize {
        self.knots.len() - self.degree - 1
    }

    /// Parameter domain `[knots[p], knots[n]]` (REQ-GEO.NURBS.DATA-005).
    pub fn domain(&self) -> (f64, f64) {
        let n = self.n_ctrl();
        // Both indices are in range by construction (len >= 2p + 2).
        let a = self.knots.get(self.degree).copied().unwrap_or(0.0);
        let b = self.knots.get(n).copied().unwrap_or(0.0);
        (a, b)
    }

    /// Distinct knot values with their multiplicities, in order.
    pub fn multiplicities(&self) -> Vec<(f64, usize)> {
        let mut out: Vec<(f64, usize)> = Vec::new();
        for &k in &self.knots {
            match out.last_mut() {
                Some((v, m)) if *v == k => *m += 1,
                _ => out.push((k, 1)),
            }
        }
        out
    }

    /// Returns the knots, exactly as given.
    pub fn into_vec(self) -> Vec<f64> {
        self.knots
    }
}

/// Raw input (and output of [`NurbsCurve::into_parts`]) of a curve.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsCurveParts {
    pub degree: usize,
    pub knots: Vec<f64>,
    pub ctrl: Vec<[f64; 3]>,
    pub weights: Option<Vec<f64>>,
    pub periodic: bool,
}

/// A validated B-spline (no weights) or NURBS curve.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsCurve {
    knots: KnotVector,
    ctrl: Vec<[f64; 3]>,
    weights: Option<Vec<f64>>,
    periodic: bool,
}

impl NurbsCurve {
    /// Validates and stores `parts` unchanged; the first failure is returned.
    pub fn new(parts: NurbsCurveParts) -> Result<Self, NurbsError> {
        let NurbsCurveParts { degree, knots, ctrl, weights, periodic } = parts;
        let dir = Dir::Curve;
        check_degree(dir, degree)?;
        check_count(dir, knots.len(), ctrl.len(), degree)?;
        let knots = KnotVector::validated(dir, knots, degree)?;
        check_ctrl(&ctrl)?;
        check_weights(weights.as_deref(), ctrl.len())?;
        Ok(NurbsCurve { knots, ctrl, weights, periodic })
    }

    pub fn degree(&self) -> usize {
        self.knots.degree
    }

    pub fn knots(&self) -> &KnotVector {
        &self.knots
    }

    pub fn ctrl(&self) -> &[[f64; 3]] {
        &self.ctrl
    }

    pub fn weights(&self) -> Option<&[f64]> {
        self.weights.as_deref()
    }

    pub fn periodic(&self) -> bool {
        self.periodic
    }

    /// Parameter domain `[knots[p], knots[n]]` (REQ-GEO.NURBS.DATA-005).
    pub fn domain(&self) -> (f64, f64) {
        self.knots.domain()
    }

    /// True when weights are present and not all equal (REQ-GEO.NURBS.DATA-005).
    pub fn is_rational(&self) -> bool {
        rational(self.weights.as_deref())
    }

    /// Returns the data exactly as it was given.
    pub fn into_parts(self) -> NurbsCurveParts {
        NurbsCurveParts {
            degree: self.knots.degree,
            knots: self.knots.knots,
            ctrl: self.ctrl,
            weights: self.weights,
            periodic: self.periodic,
        }
    }
}

/// Raw input (and output of [`NurbsSurface::into_parts`]) of a surface. The control net
/// and weights are row-major: point `(i, j)` is at `i * n_v + j`, `i < n_u`, `j < n_v`.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsSurfaceParts {
    pub degree_u: usize,
    pub degree_v: usize,
    pub knots_u: Vec<f64>,
    pub knots_v: Vec<f64>,
    pub n_u: usize,
    pub n_v: usize,
    pub ctrl: Vec<[f64; 3]>,
    pub weights: Option<Vec<f64>>,
    pub periodic_u: bool,
    pub periodic_v: bool,
}

/// A validated B-spline (no weights) or NURBS tensor-product surface.
#[derive(Debug, Clone, PartialEq)]
pub struct NurbsSurface {
    knots_u: KnotVector,
    knots_v: KnotVector,
    n_u: usize,
    n_v: usize,
    ctrl: Vec<[f64; 3]>,
    weights: Option<Vec<f64>>,
    periodic_u: bool,
    periodic_v: bool,
}

impl NurbsSurface {
    /// Validates and stores `parts` unchanged; the first failure is returned.
    pub fn new(parts: NurbsSurfaceParts) -> Result<Self, NurbsError> {
        let NurbsSurfaceParts { degree_u, degree_v, knots_u, knots_v, n_u, n_v, ctrl, weights, periodic_u, periodic_v } =
            parts;
        check_degree(Dir::U, degree_u)?;
        check_degree(Dir::V, degree_v)?;
        check_count(Dir::U, knots_u.len(), n_u, degree_u)?;
        check_count(Dir::V, knots_v.len(), n_v, degree_v)?;
        if n_u.checked_mul(n_v) != Some(ctrl.len()) {
            return Err(NurbsError::NetShape { ctrl: ctrl.len(), n_u, n_v });
        }
        let knots_u = KnotVector::validated(Dir::U, knots_u, degree_u)?;
        let knots_v = KnotVector::validated(Dir::V, knots_v, degree_v)?;
        check_ctrl(&ctrl)?;
        check_weights(weights.as_deref(), ctrl.len())?;
        Ok(NurbsSurface { knots_u, knots_v, n_u, n_v, ctrl, weights, periodic_u, periodic_v })
    }

    pub fn degree_u(&self) -> usize {
        self.knots_u.degree
    }

    pub fn degree_v(&self) -> usize {
        self.knots_v.degree
    }

    pub fn knots_u(&self) -> &KnotVector {
        &self.knots_u
    }

    pub fn knots_v(&self) -> &KnotVector {
        &self.knots_v
    }

    /// Control points per direction `(n_u, n_v)`.
    pub fn shape(&self) -> (usize, usize) {
        (self.n_u, self.n_v)
    }

    /// Row-major control net.
    pub fn ctrl(&self) -> &[[f64; 3]] {
        &self.ctrl
    }

    /// Control point `(i, j)`, or `None` out of range.
    pub fn ctrl_at(&self, i: usize, j: usize) -> Option<&[f64; 3]> {
        if i < self.n_u && j < self.n_v {
            self.ctrl.get(i * self.n_v + j)
        } else {
            None
        }
    }

    pub fn weights(&self) -> Option<&[f64]> {
        self.weights.as_deref()
    }

    pub fn periodic_u(&self) -> bool {
        self.periodic_u
    }

    pub fn periodic_v(&self) -> bool {
        self.periodic_v
    }

    /// Parameter domains `([u0, u1], [v0, v1])` (REQ-GEO.NURBS.DATA-005).
    pub fn domain(&self) -> ((f64, f64), (f64, f64)) {
        (self.knots_u.domain(), self.knots_v.domain())
    }

    /// True when weights are present and not all equal (REQ-GEO.NURBS.DATA-005).
    pub fn is_rational(&self) -> bool {
        rational(self.weights.as_deref())
    }

    /// Returns the data exactly as it was given.
    pub fn into_parts(self) -> NurbsSurfaceParts {
        NurbsSurfaceParts {
            degree_u: self.knots_u.degree,
            degree_v: self.knots_v.degree,
            knots_u: self.knots_u.knots,
            knots_v: self.knots_v.knots,
            n_u: self.n_u,
            n_v: self.n_v,
            ctrl: self.ctrl,
            weights: self.weights,
            periodic_u: self.periodic_u,
            periodic_v: self.periodic_v,
        }
    }
}
