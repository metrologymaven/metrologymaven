//! Query evaluation core with point and triangle predicates (spec node `GEO.SEL.QUERY.EVAL`,
//! `REQ-001`..`REQ-006`, `AC-1`..`AC-6`).
//!
//! Pure KNP-1 kernel code: no I/O, no `unsafe`, no panics, no `HashMap`/`HashSet`, no FMA,
//! transcendentals only through `fid_num::f`, fixed-order reductions, deterministic `D0`.

use crate::algebra::{intersection, invert, union};
use crate::model::{domain_count, Domain, EntityId, IndexSet, SelError, Selection, SelectionBody};
use crate::query::ast::{AttrChannel, Expr, NearRef, Predicate, SelectionQuery, TopologyId};
use crate::query::canon::canonicalize;
use core::cmp::Ordering;
use core::fmt;
use fid_geo_frame::AuthoredQuantity;
use fid_geo_types::{digest, AttributeSet, ElementKind, GeometryElement, Mesh, PointBuffer};
use fid_num::f as numf;

/// Minimum allowed normal-cone axis norm before rejecting as invalid.
const MIN_AXIS_NORM: f64 = 1e-12;

/// Typed evaluation error (`REQ-002`, `REQ-003`, `REQ-004`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    /// Predicate referenced a missing attribute channel or an out-of-range index (`REQ-002`).
    UnresolvedReference { channel: Option<&'static str>, index: Option<u64>, detail: String },
    /// Predicate does not apply to the query domain or element kind (`REQ-003`).
    DomainMismatch { predicate: &'static str, domain: Domain },
    /// A `near` or CAD predicate was encountered without a matching evaluator in [`EvalContext`] (`REQ-004`).
    UnsupportedPredicate { predicate: &'static str },
    /// Query domain does not match the element's geometry kind.
    ElementMismatch { domain: Domain, element_kind: ElementKind },
    /// Predicate parameter is degenerate or non-finite (e.g., zero-length axis or NaN bound).
    InvalidPredicate { detail: String },
    /// Underlying selection model or algebra error.
    Sel(SelError),
}

impl EvalError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            EvalError::UnresolvedReference { .. } => "unresolved_reference",
            EvalError::DomainMismatch { .. } => "domain_mismatch",
            EvalError::UnsupportedPredicate { .. } => "unsupported_predicate",
            EvalError::ElementMismatch { .. } => "element_mismatch",
            EvalError::InvalidPredicate { .. } => "invalid_predicate",
            EvalError::Sel(e) => e.code(),
        }
    }
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::UnresolvedReference { channel: Some(ch), .. } => {
                write!(f, "unresolved_reference: missing attribute channel {ch}")
            }
            EvalError::UnresolvedReference { index: Some(idx), detail, .. } => {
                write!(f, "unresolved_reference: index {idx} ({detail})")
            }
            EvalError::UnresolvedReference { detail, .. } => write!(f, "unresolved_reference: {detail}"),
            EvalError::DomainMismatch { predicate, domain } => {
                write!(f, "domain_mismatch: predicate {predicate} not valid on domain {domain:?}")
            }
            EvalError::UnsupportedPredicate { predicate } => {
                write!(f, "unsupported_predicate: no evaluator provided for {predicate}")
            }
            EvalError::ElementMismatch { domain, element_kind } => {
                write!(f, "element_mismatch: domain {domain:?} incompatible with element kind {element_kind:?}")
            }
            EvalError::InvalidPredicate { detail } => write!(f, "invalid_predicate: {detail}"),
            EvalError::Sel(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for EvalError {}

impl From<SelError> for EvalError {
    fn from(e: SelError) -> Self {
        EvalError::Sel(e)
    }
}

/// Borrowed parameters of a `near` predicate dispatched to [`NearEvaluator`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NearPred<'a> {
    pub of: &'a NearRef,
    pub distance: &'a AuthoredQuantity,
}

/// Borrowed parameters of a CAD predicate dispatched to [`CadEvaluator`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CadPred<'a> {
    FaceIds { body: EntityId, ids: &'a [TopologyId] },
    InGroup { group: EntityId },
    InLayer { layer: EntityId },
    InBody { body: EntityId },
    TangentGrow { seed: &'a Expr, angle: &'a AuthoredQuantity },
}

impl CadPred<'_> {
    pub const fn name(&self) -> &'static str {
        match self {
            CadPred::FaceIds { .. } => "face_ids",
            CadPred::InGroup { .. } => "in_group",
            CadPred::InLayer { .. } => "in_layer",
            CadPred::InBody { .. } => "in_body",
            CadPred::TangentGrow { .. } => "tangent_grow",
        }
    }
}

/// Pluggable evaluator for `near` distance predicates (`GEO.SEL.QUERY.NEAR`).
pub trait NearEvaluator {
    fn near(&self, element: &GeometryElement, domain: Domain, pred: &NearPred<'_>) -> Result<IndexSet, EvalError>;
}

/// Pluggable evaluator for CAD topology/group/layer/body/tangent predicates (`GEO.SEL.QUERY.CAD`).
pub trait CadEvaluator {
    fn eval(
        &self,
        element: &GeometryElement,
        domain: Domain,
        pred: &CadPred<'_>,
        eval_seed: &dyn Fn(&Expr) -> Result<IndexSet, EvalError>,
    ) -> Result<IndexSet, EvalError>;
}

/// Evaluation context carrying the target [`GeometryElement`] and optional pluggable evaluators.
pub struct EvalContext<'a> {
    pub element: &'a GeometryElement,
    pub near: Option<&'a dyn NearEvaluator>,
    pub cad: Option<&'a dyn CadEvaluator>,
}

impl<'a> EvalContext<'a> {
    /// Context with no distance or CAD evaluators.
    pub fn new(element: &'a GeometryElement) -> Self {
        Self { element, near: None, cad: None }
    }
}

/// Evaluates `query` on `ctx.element` and returns the canonical [`IndexSet`] (`REQ-001`..`REQ-006`).
pub fn evaluate(query: &SelectionQuery, ctx: &EvalContext<'_>) -> Result<IndexSet, EvalError> {
    let canon = canonicalize(query).map_err(|e| EvalError::InvalidPredicate { detail: e.to_string() })?;
    let count = match domain_count(ctx.element, canon.domain) {
        Ok(c) => c,
        Err(_) => {
            return Err(EvalError::ElementMismatch { domain: canon.domain, element_kind: ctx.element.kind() });
        }
    };
    eval_expr(&canon.root, canon.domain, count, ctx)
}

/// Evaluates `query` on `ctx.element` and wraps the resulting [`IndexSet`] into a [`Selection`]
/// bound to `target` and the element's canonical digest (`REQ-001`).
pub fn evaluate_selection(
    query: &SelectionQuery,
    target: EntityId,
    ctx: &EvalContext<'_>,
) -> Result<Selection, EvalError> {
    let set = evaluate(query, ctx)?;
    let elem_digest = digest(ctx.element);
    Ok(Selection::new(target, elem_digest, SelectionBody::IndexSet(set)))
}

// ------------------------------------------------------------------ explicit-stack post-order walk

enum Frame<'a> {
    Enter(&'a Expr),
    CombineAnd(usize),
    CombineOr(usize),
    CombineNot,
}

fn eval_expr(root: &Expr, domain: Domain, count: Option<u64>, ctx: &EvalContext<'_>) -> Result<IndexSet, EvalError> {
    let mut stack: Vec<Frame<'_>> = Vec::new();
    let mut values: Vec<IndexSet> = Vec::new();
    stack.push(Frame::Enter(root));

    while let Some(frame) = stack.pop() {
        match frame {
            Frame::Enter(expr) => match expr {
                Expr::And(children) => {
                    if children.is_empty() {
                        values.push(all_set(domain, count)?);
                    } else {
                        stack.push(Frame::CombineAnd(children.len()));
                        // Push in reverse so children are evaluated left-to-right in canonical order.
                        for child in children.iter().rev() {
                            stack.push(Frame::Enter(child));
                        }
                    }
                }
                Expr::Or(children) => {
                    if children.is_empty() {
                        values.push(IndexSet::empty(domain));
                    } else {
                        stack.push(Frame::CombineOr(children.len()));
                        for child in children.iter().rev() {
                            stack.push(Frame::Enter(child));
                        }
                    }
                }
                Expr::Not(inner) => {
                    stack.push(Frame::CombineNot);
                    stack.push(Frame::Enter(inner));
                }
                Expr::Pred(pred) => {
                    let set = eval_predicate(pred, domain, count, ctx)?;
                    values.push(set);
                }
            },
            Frame::CombineAnd(n) => {
                let start = values
                    .len()
                    .checked_sub(n)
                    .ok_or_else(|| EvalError::InvalidPredicate { detail: "internal stack underflow".into() })?;
                let mut acc = values.remove(start);
                for _ in 1..n {
                    let next = values.remove(start);
                    acc = intersection(&acc, &next)?;
                }
                values.push(acc);
            }
            Frame::CombineOr(n) => {
                let start = values
                    .len()
                    .checked_sub(n)
                    .ok_or_else(|| EvalError::InvalidPredicate { detail: "internal stack underflow".into() })?;
                let mut acc = values.remove(start);
                for _ in 1..n {
                    let next = values.remove(start);
                    acc = union(&acc, &next)?;
                }
                values.push(acc);
            }
            Frame::CombineNot => {
                let inner = values
                    .pop()
                    .ok_or_else(|| EvalError::InvalidPredicate { detail: "internal stack underflow".into() })?;
                let dom_count = count.ok_or(EvalError::DomainMismatch { predicate: "not", domain })?;
                let comp = invert(&inner, dom_count)?;
                values.push(comp);
            }
        }
    }

    values.pop().ok_or_else(|| EvalError::InvalidPredicate { detail: "empty evaluation result".into() })
}

fn all_set(domain: Domain, count: Option<u64>) -> Result<IndexSet, EvalError> {
    match domain {
        Domain::Points | Domain::Triangles => {
            let n = count.ok_or(EvalError::DomainMismatch { predicate: "all", domain })?;
            if n == 0 {
                Ok(IndexSet::empty(domain))
            } else {
                Ok(IndexSet::from_canonical_ranges(domain, vec![(0, n)]))
            }
        }
        Domain::Faces | Domain::Edges => Err(EvalError::DomainMismatch { predicate: "all", domain }),
    }
}

fn eval_predicate(
    pred: &Predicate,
    domain: Domain,
    count: Option<u64>,
    ctx: &EvalContext<'_>,
) -> Result<IndexSet, EvalError> {
    match pred {
        Predicate::All => all_set(domain, count),
        Predicate::Ids(ids) => {
            if !matches!(domain, Domain::Points | Domain::Triangles) {
                return Err(EvalError::DomainMismatch { predicate: "ids", domain });
            }
            let n = count.ok_or(EvalError::DomainMismatch { predicate: "ids", domain })?;
            for &id in ids {
                if id >= n {
                    return Err(EvalError::UnresolvedReference {
                        channel: None,
                        index: Some(id),
                        detail: format!("index {id} out of range for count {n}"),
                    });
                }
            }
            Ok(IndexSet::from_indices(domain, ids, n)?)
        }
        Predicate::AttrRange { channel, min, max } => {
            if domain != Domain::Points {
                return Err(EvalError::DomainMismatch { predicate: "attr_range", domain });
            }
            let (_, attrs) = point_data(ctx.element, domain, "attr_range")?;
            eval_attr_range(attrs, *channel, *min, *max)
        }
        Predicate::QualityFlags { all_of, none_of } => {
            if domain != Domain::Points {
                return Err(EvalError::DomainMismatch { predicate: "quality_flags", domain });
            }
            let (_, attrs) = point_data(ctx.element, domain, "quality_flags")?;
            eval_quality_flags(attrs, *all_of, *none_of)
        }
        Predicate::NormalCone { axis, half_angle } => {
            let axis_unit = unit_axis(*axis)?;
            let ha = half_angle.si();
            if !ha.is_finite() {
                return Err(EvalError::InvalidPredicate { detail: "non-finite half_angle".into() });
            }
            let cos_half =
                numf::cos(ha).map_err(|_| EvalError::InvalidPredicate { detail: "half_angle cos failed".into() })?;
            match domain {
                Domain::Points => {
                    let (_, attrs) = point_data(ctx.element, domain, "normal_cone")?;
                    eval_point_normal_cone(attrs, axis_unit, cos_half)
                }
                Domain::Triangles => {
                    let mesh = mesh_data(ctx.element, domain, "normal_cone")?;
                    eval_triangle_normal_cone(mesh, axis_unit, cos_half)
                }
                Domain::Faces | Domain::Edges => Err(EvalError::DomainMismatch { predicate: "normal_cone", domain }),
            }
        }
        Predicate::InBox { min, max } => {
            let bmin = [min[0].si(), min[1].si(), min[2].si()];
            let bmax = [max[0].si(), max[1].si(), max[2].si()];
            check_vec3_finite(bmin, "in_box.min")?;
            check_vec3_finite(bmax, "in_box.max")?;
            match domain {
                Domain::Points => {
                    let (buf, _) = point_data(ctx.element, domain, "in_box")?;
                    Ok(eval_point_in_box(buf, bmin, bmax))
                }
                Domain::Triangles => {
                    let mesh = mesh_data(ctx.element, domain, "in_box")?;
                    Ok(eval_triangle_in_box(mesh, bmin, bmax))
                }
                Domain::Faces | Domain::Edges => Err(EvalError::DomainMismatch { predicate: "in_box", domain }),
            }
        }
        Predicate::InSphere { centre, radius } => {
            let c = [centre[0].si(), centre[1].si(), centre[2].si()];
            check_vec3_finite(c, "in_sphere.centre")?;
            let r = radius.si();
            if !r.is_finite() || r.total_cmp(&0.0) == Ordering::Less {
                return Err(EvalError::InvalidPredicate { detail: "invalid sphere radius".into() });
            }
            let r2 = r * r;
            match domain {
                Domain::Points => {
                    let (buf, _) = point_data(ctx.element, domain, "in_sphere")?;
                    Ok(eval_point_in_sphere(buf, c, r2))
                }
                Domain::Triangles => {
                    let mesh = mesh_data(ctx.element, domain, "in_sphere")?;
                    Ok(eval_triangle_in_sphere(mesh, c, r2))
                }
                Domain::Faces | Domain::Edges => Err(EvalError::DomainMismatch { predicate: "in_sphere", domain }),
            }
        }
        Predicate::Near { of, distance } => {
            let Some(near_eval) = ctx.near else {
                return Err(EvalError::UnsupportedPredicate { predicate: "near" });
            };
            near_eval.near(ctx.element, domain, &NearPred { of, distance })
        }
        Predicate::FaceIds { body, ids } => dispatch_cad(CadPred::FaceIds { body: *body, ids }, domain, count, ctx),
        Predicate::InGroup { group } => dispatch_cad(CadPred::InGroup { group: *group }, domain, count, ctx),
        Predicate::InLayer { layer } => dispatch_cad(CadPred::InLayer { layer: *layer }, domain, count, ctx),
        Predicate::InBody { body } => dispatch_cad(CadPred::InBody { body: *body }, domain, count, ctx),
        Predicate::TangentGrow { seed, angle } => {
            dispatch_cad(CadPred::TangentGrow { seed: seed.as_ref(), angle }, domain, count, ctx)
        }
    }
}

fn dispatch_cad(
    pred: CadPred<'_>,
    domain: Domain,
    count: Option<u64>,
    ctx: &EvalContext<'_>,
) -> Result<IndexSet, EvalError> {
    let Some(cad_eval) = ctx.cad else {
        return Err(EvalError::UnsupportedPredicate { predicate: pred.name() });
    };
    let cb = |sub: &Expr| eval_expr(sub, domain, count, ctx);
    cad_eval.eval(ctx.element, domain, &pred, &cb)
}

fn point_data<'a>(
    element: &'a GeometryElement,
    domain: Domain,
    predicate: &'static str,
) -> Result<(&'a PointBuffer, &'a AttributeSet), EvalError> {
    match element {
        GeometryElement::PointSet(p) => Ok((p.buffer(), p.attributes())),
        GeometryElement::PointCloud(p) => Ok((p.buffer(), p.attributes())),
        GeometryElement::Mesh(m) => Ok((m.vertices(), m.vertex_attrs())),
        GeometryElement::CadBody(_) => Err(EvalError::DomainMismatch { predicate, domain }),
    }
}

fn mesh_data<'a>(element: &'a GeometryElement, domain: Domain, predicate: &'static str) -> Result<&'a Mesh, EvalError> {
    match element {
        GeometryElement::Mesh(m) => Ok(m),
        _ => Err(EvalError::DomainMismatch { predicate, domain }),
    }
}

// ------------------------------------------------------------------ range coalescing scan helper

struct RangeCollector {
    ranges: Vec<(u64, u64)>,
}

impl RangeCollector {
    fn new() -> Self {
        Self { ranges: Vec::new() }
    }

    #[inline]
    fn push(&mut self, idx: u64) {
        if let Some(last) = self.ranges.last_mut() {
            if last.1 == idx {
                last.1 = idx + 1;
                return;
            }
        }
        self.ranges.push((idx, idx + 1));
    }

    fn finish(self, domain: Domain) -> IndexSet {
        IndexSet::from_canonical_ranges(domain, self.ranges)
    }
}

// ------------------------------------------------------------------ point predicates

fn eval_attr_range(
    attrs: &AttributeSet,
    channel: AttrChannel,
    min: Option<f64>,
    max: Option<f64>,
) -> Result<IndexSet, EvalError> {
    if let Some(lo) = min {
        if !lo.is_finite() {
            return Err(EvalError::InvalidPredicate { detail: "non-finite attr_range.min".into() });
        }
    }
    if let Some(hi) = max {
        if !hi.is_finite() {
            return Err(EvalError::InvalidPredicate { detail: "non-finite attr_range.max".into() });
        }
    }
    let mut out = RangeCollector::new();
    match channel {
        AttrChannel::Intensity => {
            let Some(vals) = attrs.intensity.as_deref() else {
                return Err(EvalError::UnresolvedReference {
                    channel: Some(channel.as_str()),
                    index: None,
                    detail: "element lacks intensity channel".into(),
                });
            };
            for (i, &v) in vals.iter().enumerate() {
                let vf = f64::from(v);
                if in_opt_range(vf, min, max) {
                    out.push(u64::try_from(i).unwrap_or(u64::MAX));
                }
            }
        }
        AttrChannel::ScannerId => {
            let Some(vals) = attrs.scanner_id.as_deref() else {
                return Err(EvalError::UnresolvedReference {
                    channel: Some(channel.as_str()),
                    index: None,
                    detail: "element lacks scanner_id channel".into(),
                });
            };
            for (i, &v) in vals.iter().enumerate() {
                let vf = f64::from(v);
                if in_opt_range(vf, min, max) {
                    out.push(u64::try_from(i).unwrap_or(u64::MAX));
                }
            }
        }
        AttrChannel::TimestampNs => {
            let Some(vals) = attrs.timestamp_ns.as_deref() else {
                return Err(EvalError::UnresolvedReference {
                    channel: Some(channel.as_str()),
                    index: None,
                    detail: "element lacks timestamp_ns channel".into(),
                });
            };
            for (i, &v) in vals.iter().enumerate() {
                let vf = v as f64;
                if in_opt_range(vf, min, max) {
                    out.push(u64::try_from(i).unwrap_or(u64::MAX));
                }
            }
        }
    }
    Ok(out.finish(Domain::Points))
}

#[inline]
fn in_opt_range(val: f64, min: Option<f64>, max: Option<f64>) -> bool {
    if !val.is_finite() {
        return false;
    }
    if let Some(lo) = min {
        if val.total_cmp(&lo) == Ordering::Less {
            return false;
        }
    }
    if let Some(hi) = max {
        if val.total_cmp(&hi) == Ordering::Greater {
            return false;
        }
    }
    true
}

fn eval_quality_flags(attrs: &AttributeSet, all_of: Option<u16>, none_of: Option<u16>) -> Result<IndexSet, EvalError> {
    let Some(vals) = attrs.quality.as_deref() else {
        return Err(EvalError::UnresolvedReference {
            channel: Some("quality"),
            index: None,
            detail: "element lacks quality channel".into(),
        });
    };
    let all_mask = all_of.unwrap_or(0);
    let none_mask = none_of.unwrap_or(0);
    let mut out = RangeCollector::new();
    for (i, &q) in vals.iter().enumerate() {
        if (q & all_mask) == all_mask && (q & none_mask) == 0 {
            out.push(u64::try_from(i).unwrap_or(u64::MAX));
        }
    }
    Ok(out.finish(Domain::Points))
}

fn eval_point_normal_cone(attrs: &AttributeSet, axis: [f64; 3], cos_half: f64) -> Result<IndexSet, EvalError> {
    let Some([nx, ny, nz]) = attrs.normal.as_ref() else {
        return Err(EvalError::UnresolvedReference {
            channel: Some("normal"),
            index: None,
            detail: "element lacks normal channel".into(),
        });
    };
    let n = nx.len();
    let mut out = RangeCollector::new();
    for i in 0..n {
        let vx = nx[i];
        let vy = ny[i];
        let vz = nz[i];
        let d = ((vx * axis[0]) + (vy * axis[1])) + (vz * axis[2]);
        if d.is_finite() && d.total_cmp(&cos_half) != Ordering::Less {
            out.push(u64::try_from(i).unwrap_or(u64::MAX));
        }
    }
    Ok(out.finish(Domain::Points))
}

fn eval_point_in_box(buf: &PointBuffer, bmin: [f64; 3], bmax: [f64; 3]) -> IndexSet {
    let n = buf.x.len();
    let mut out = RangeCollector::new();
    for i in 0..n {
        let p = [buf.x[i], buf.y[i], buf.z[i]];
        if point_in_box(p, bmin, bmax) {
            out.push(u64::try_from(i).unwrap_or(u64::MAX));
        }
    }
    out.finish(Domain::Points)
}

fn eval_point_in_sphere(buf: &PointBuffer, centre: [f64; 3], r2: f64) -> IndexSet {
    let n = buf.x.len();
    let mut out = RangeCollector::new();
    for i in 0..n {
        let p = [buf.x[i], buf.y[i], buf.z[i]];
        if point_in_sphere(p, centre, r2) {
            out.push(u64::try_from(i).unwrap_or(u64::MAX));
        }
    }
    out.finish(Domain::Points)
}

// ------------------------------------------------------------------ triangle predicates

fn eval_triangle_normal_cone(mesh: &Mesh, axis: [f64; 3], cos_half: f64) -> Result<IndexSet, EvalError> {
    let mut out = RangeCollector::new();
    for (ti, [i0, i1, i2]) in mesh.triangles().enumerate() {
        let (Some(v0), Some(v1), Some(v2)) = (mesh.vertex(i0), mesh.vertex(i1), mesh.vertex(i2)) else {
            continue;
        };
        let e1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
        let e2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
        let n =
            [(e1[1] * e2[2]) - (e1[2] * e2[1]), (e1[2] * e2[0]) - (e1[0] * e2[2]), (e1[0] * e2[1]) - (e1[1] * e2[0])];
        let n2 = ((n[0] * n[0]) + (n[1] * n[1])) + (n[2] * n[2]);
        if n2.total_cmp(&0.0) != Ordering::Greater || !n2.is_finite() {
            continue;
        }
        let Ok(n_len) = numf::sqrt(n2) else { continue };
        if n_len.total_cmp(&0.0) != Ordering::Greater {
            continue;
        }
        let inv = 1.0 / n_len;
        let nu = [n[0] * inv, n[1] * inv, n[2] * inv];
        let d = ((nu[0] * axis[0]) + (nu[1] * axis[1])) + (nu[2] * axis[2]);
        if d.is_finite() && d.total_cmp(&cos_half) != Ordering::Less {
            out.push(u64::try_from(ti).unwrap_or(u64::MAX));
        }
    }
    Ok(out.finish(Domain::Triangles))
}

fn eval_triangle_in_box(mesh: &Mesh, bmin: [f64; 3], bmax: [f64; 3]) -> IndexSet {
    let mut out = RangeCollector::new();
    for (ti, [i0, i1, i2]) in mesh.triangles().enumerate() {
        let (Some(v0), Some(v1), Some(v2)) = (mesh.vertex(i0), mesh.vertex(i1), mesh.vertex(i2)) else {
            continue;
        };
        let c = tri_centroid(v0, v1, v2);
        if point_in_box(c, bmin, bmax) {
            out.push(u64::try_from(ti).unwrap_or(u64::MAX));
        }
    }
    out.finish(Domain::Triangles)
}

fn eval_triangle_in_sphere(mesh: &Mesh, centre: [f64; 3], r2: f64) -> IndexSet {
    let mut out = RangeCollector::new();
    for (ti, [i0, i1, i2]) in mesh.triangles().enumerate() {
        let (Some(v0), Some(v1), Some(v2)) = (mesh.vertex(i0), mesh.vertex(i1), mesh.vertex(i2)) else {
            continue;
        };
        let c = tri_centroid(v0, v1, v2);
        if point_in_sphere(c, centre, r2) {
            out.push(u64::try_from(ti).unwrap_or(u64::MAX));
        }
    }
    out.finish(Domain::Triangles)
}

// ------------------------------------------------------------------ pure KNP-1 geometric primitives

#[inline]
fn tri_centroid(v0: [f64; 3], v1: [f64; 3], v2: [f64; 3]) -> [f64; 3] {
    [((v0[0] + v1[0]) + v2[0]) / 3.0, ((v0[1] + v1[1]) + v2[1]) / 3.0, ((v0[2] + v1[2]) + v2[2]) / 3.0]
}

#[inline]
fn point_in_box(p: [f64; 3], bmin: [f64; 3], bmax: [f64; 3]) -> bool {
    p[0].total_cmp(&bmin[0]) != Ordering::Less
        && p[0].total_cmp(&bmax[0]) != Ordering::Greater
        && p[1].total_cmp(&bmin[1]) != Ordering::Less
        && p[1].total_cmp(&bmax[1]) != Ordering::Greater
        && p[2].total_cmp(&bmin[2]) != Ordering::Less
        && p[2].total_cmp(&bmax[2]) != Ordering::Greater
}

#[inline]
fn point_in_sphere(p: [f64; 3], centre: [f64; 3], r2: f64) -> bool {
    let dx = p[0] - centre[0];
    let dy = p[1] - centre[1];
    let dz = p[2] - centre[2];
    let d2 = ((dx * dx) + (dy * dy)) + (dz * dz);
    d2.is_finite() && d2.total_cmp(&r2) != Ordering::Greater
}

fn check_vec3_finite(v: [f64; 3], name: &str) -> Result<(), EvalError> {
    if v[0].is_finite() && v[1].is_finite() && v[2].is_finite() {
        Ok(())
    } else {
        Err(EvalError::InvalidPredicate { detail: format!("non-finite vector {name}") })
    }
}

fn unit_axis(axis: [f64; 3]) -> Result<[f64; 3], EvalError> {
    check_vec3_finite(axis, "normal_cone.axis")?;
    let n2 = ((axis[0] * axis[0]) + (axis[1] * axis[1])) + (axis[2] * axis[2]);
    let norm = numf::sqrt(n2).map_err(|_| EvalError::InvalidPredicate { detail: "axis norm sqrt failed".into() })?;
    if norm.total_cmp(&MIN_AXIS_NORM) == Ordering::Less {
        return Err(EvalError::InvalidPredicate { detail: "zero-length normal_cone axis".into() });
    }
    let inv = 1.0 / norm;
    Ok([axis[0] * inv, axis[1] * inv, axis[2] * inv])
}
