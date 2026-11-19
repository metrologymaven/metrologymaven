//! Moving elements between frames, and the frame-mismatch guard. Spec node `GEO.FRAME.APPLY`.
//!
//! [`transform_element`] maps a [`PointSet`], [`PointCloud`] or [`Mesh`] through a
//! [`Transform`] into a new element in the target frame; the input is never modified
//! (REQ-GEO.FRAME.APPLY-001). [`require_same_frame`] is the one guard every binary geometric
//! operation calls before combining two elements: different frames are a typed
//! [`GeoError::FrameMismatch`] and are never converted (REQ-GEO.FRAME.APPLY-003,
//! REQ-GEO.FRAME-008).
//!
//! # Pinned choices (value-affecting; repeated in the leaf's `NODE.md` Design)
//!
//! - Points (vertices) are mapped one at a time in index order by
//!   [`Transform::apply_point`]: `y[i] = s * dot3(R[i], x) + t[i]` with
//!   `dot3(a, b) = (a0 * b0 + a1 * b1) + a2 * b2` (GEO.FRAME.XFORM). Scan origins are points
//!   and are mapped the same way, in their stored (ascending scanner-ID) order.
//! - Normals are mapped by [`Transform::apply_dir`], `m[i] = dot3(R[i], n)`: the rotation
//!   only, for rigid and similarity transforms alike. The normal transform of `x ↦ s R x + t`
//!   is `(s R)⁻ᵀ = R / s`, which has the direction of `R`; [`Transform`] has no non-uniform
//!   scale or shear, so no other inverse-transpose case exists. Each rotated normal is then
//!   renormalised: `len = fid_num::f::sqrt(dot3(m, m))`, `n'[i] = m[i] / len` (a division per
//!   component, not a multiplication by `1 / len`). The output is unit within 1e-12
//!   (REQ-GEO.FRAME.APPLY-002) even when an input normal sits at GEO.TYPES' ±1e-9 acceptance
//!   edge, so a valid element never fails to transform because rotation rounding pushed a
//!   normal past that edge. A non-finite or zero length is `BadNormal { index }`
//!   (unreachable for validated input).
//! - Identity: under [`Transform::IDENTITY`] (translation +0.0) points and scan origins are
//!   bit-exact except that −0.0 becomes +0.0 (`x + 0.0`); normals become `n / ‖n‖`, so they
//!   are bit-exact only when the computed `‖n‖` is exactly 1.
//! - Every other channel (intensity, colour, scanner ID, timestamp, quality), the grid shape,
//!   the triangles, the per-triangle attributes and the mesh index width are copied
//!   unchanged. The new element is built through the validating constructor of its kind
//!   (`PointSet::new`, `PointCloud::new`, `Mesh::with_options`), so it satisfies every
//!   invariant of GEO.TYPES; mesh topology facts are recomputed and equal the input's,
//!   because they depend on the triangles only.
//! - The target frame is not compared with the source frame: re-expressing an element in its
//!   own frame (a correction transform) is allowed. Which transform links the two frames is
//!   the caller's business (GEO.FRAME.TREE resolves chains; GEO.FRAME.CMD journals the
//!   result). [`transform_element_with_provenance`] returns the source frame, target frame
//!   and transform alongside the element for that record.
//! - Errors, first failure wins, in this order: a [`CadBodyRef`](fid_geo_types::CadBodyRef) is
//!   [`GeoError::Unsupported`] before anything is computed; then points in index order (a
//!   non-finite result, possible only on overflow, is `NonFinite { index }`), then normals
//!   (`BadNormal { index }`), then scan origins (`ScanOrigin { scanner_id }`), then the
//!   element constructor's own checks, all as [`GeoError::Element`]. Output columns are
//!   reserved fallibly (`AllocationFailed`); copied attribute columns and per-triangle
//!   attributes use `try_reserve_exact` then `extend_from_slice`, and the triangle list is
//!   reserved the same way before it is filled, so no copy sized by the input allocates
//!   infallibly.
//!
//! KNP-1: no fused multiply-add, no `f64::min`/`f64::max`, no hash containers; the only
//! non-arithmetic function is the correctly rounded `fid_num::f::sqrt`; the arithmetic is the pinned GEO.FRAME.XFORM arithmetic, so
//! results are bit-identical on x86-64, ARM64 and wasm32. Nothing here panics.

use core::fmt;

use fid_geo_types::{
    AttributeSet, ElementKind, FrameRef, GeoTypeError, GeometryElement, IndexWidth, Mesh, MeshOptions, PointBuffer,
    PointCloud, PointSet, TriangleAttrs,
};

use crate::xform::{dot3, Transform};

/// The shared error of the GEO frame operations: the frame-mismatch guard, refusals and the
/// element errors of rebuilding a transformed element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeoError {
    /// Two elements (or frame references) that must share a frame do not
    /// (REQ-GEO.FRAME.APPLY-003, REQ-GEO.FRAME-008). Nothing was converted.
    FrameMismatch {
        /// The frame of the first (left) operand.
        left: FrameRef,
        /// The frame of the second (right) operand.
        right: FrameRef,
    },
    /// The operation does not apply to this element kind (a CAD body handle is placed by
    /// GEO.CAD.ASSY, not transformed here; REQ-GEO.FRAME.APPLY-004).
    Unsupported {
        /// The refused kind.
        kind: ElementKind,
    },
    /// Building the transformed element failed (an overflowing coordinate, a normal or scan
    /// origin that no longer validates, or a failed allocation).
    Element(GeoTypeError),
}

impl GeoError {
    /// A stable machine-readable code; element errors keep their GEO.TYPES code.
    pub fn code(&self) -> &'static str {
        match self {
            GeoError::FrameMismatch { .. } => "frame_mismatch",
            GeoError::Unsupported { .. } => "unsupported",
            GeoError::Element(e) => e.code(),
        }
    }
}

fn hex16(f: &mut fmt::Formatter<'_>, id: FrameRef) -> fmt::Result {
    for b in id.to_bytes() {
        write!(f, "{b:02x}")?;
    }
    Ok(())
}

impl fmt::Display for GeoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GeoError::FrameMismatch { left, right } => {
                f.write_str("frame mismatch: left is in frame ")?;
                hex16(f, *left)?;
                f.write_str(", right is in frame ")?;
                hex16(f, *right)
            }
            GeoError::Unsupported { kind } => write!(f, "operation not supported for element kind {kind}"),
            GeoError::Element(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for GeoError {}

impl From<GeoTypeError> for GeoError {
    fn from(e: GeoTypeError) -> Self {
        GeoError::Element(e)
    }
}

/// Where a transformed element came from: the record GEO.FRAME.CMD journals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformProvenance {
    /// The frame of the input element.
    pub source: FrameRef,
    /// The frame of the output element.
    pub target: FrameRef,
    /// The transform applied (source coordinates to target coordinates).
    pub transform: Transform,
}

/// A transformed element with its provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct Transformed {
    /// The new element, in `provenance.target`.
    pub element: GeometryElement,
    /// The source frame, target frame and transform used.
    pub provenance: TransformProvenance,
}

/// The frame-mismatch guard on two frame references: `Ok(left)` if they are equal, else
/// `FrameMismatch { left, right }`. For operations whose operands expose a frame but are not
/// [`GeometryElement`]s.
pub fn require_same_frame_ref(left: FrameRef, right: FrameRef) -> Result<FrameRef, GeoError> {
    if left == right {
        Ok(left)
    } else {
        Err(GeoError::FrameMismatch { left, right })
    }
}

/// The frame-mismatch guard every binary geometric operation calls: the shared frame of `a`
/// and `b`, or `FrameMismatch { left: a.frame(), right: b.frame() }`. Never converts
/// (REQ-GEO.FRAME.APPLY-003). Any kinds, CAD body handles included, may be compared.
pub fn require_same_frame(a: &GeometryElement, b: &GeometryElement) -> Result<FrameRef, GeoError> {
    require_same_frame_ref(a.frame(), b.frame())
}

/// `e` mapped through `t` into frame `target`, as a new element (REQ-GEO.FRAME.APPLY-001 and
/// -002); `Unsupported { kind: CadBodyRef }` for a CAD body handle (REQ-GEO.FRAME.APPLY-004).
/// See the [module documentation](self) for the pinned arithmetic and error order.
pub fn transform_element(e: &GeometryElement, t: &Transform, target: FrameRef) -> Result<GeometryElement, GeoError> {
    match e {
        GeometryElement::PointSet(p) => {
            let (buf, attrs) = transform_points(p.buffer(), p.attributes(), t)?;
            Ok(GeometryElement::PointSet(PointSet::new(target, buf, attrs)?))
        }
        GeometryElement::PointCloud(p) => {
            let (buf, attrs) = transform_points(p.buffer(), p.attributes(), t)?;
            Ok(GeometryElement::PointCloud(PointCloud::new(target, buf, attrs)?))
        }
        GeometryElement::Mesh(m) => Ok(GeometryElement::Mesh(transform_mesh(m, t, target)?)),
        GeometryElement::CadBody(_) => Err(GeoError::Unsupported { kind: ElementKind::CadBodyRef }),
    }
}

/// [`transform_element`] with the [`TransformProvenance`] of the result.
pub fn transform_element_with_provenance(
    e: &GeometryElement,
    t: &Transform,
    target: FrameRef,
) -> Result<Transformed, GeoError> {
    let element = transform_element(e, t, target)?;
    Ok(Transformed { element, provenance: TransformProvenance { source: e.frame(), target, transform: *t } })
}

/// `usize` to `u64` (lossless on every supported target; saturates otherwise).
fn index_of(i: usize) -> u64 {
    u64::try_from(i).unwrap_or(u64::MAX)
}

/// Three empty columns with room for `n` values each, or `AllocationFailed`.
fn columns(n: usize) -> Result<[Vec<f64>; 3], GeoError> {
    let bytes = index_of(n).saturating_mul(3 * 8);
    let mut cols = [Vec::new(), Vec::new(), Vec::new()];
    for c in &mut cols {
        c.try_reserve_exact(n).map_err(|_| GeoTypeError::AllocationFailed { needed: bytes })?;
    }
    Ok(cols)
}

/// `m / ‖m‖` with `‖m‖ = sqrt(dot3(m, m))` through `fid-num`, divided per component; `None`
/// if the length is zero or not finite, or a result is not finite.
fn unit(m: &[f64; 3]) -> Option<[f64; 3]> {
    let len = fid_num::f::sqrt(dot3(m[0], m[1], m[2], m[0], m[1], m[2])).ok()?;
    if !(len.is_finite() && len > 0.0) {
        return None;
    }
    let n = [m[0] / len, m[1] / len, m[2] / len];
    if n.iter().all(|v| v.is_finite()) {
        Some(n)
    } else {
        None
    }
}

/// A fallible copy of a slice (`try_reserve_exact` then `extend_from_slice`).
fn copy_slice<T: Copy>(v: &[T]) -> Result<Vec<T>, GeoError> {
    let mut out = Vec::new();
    out.try_reserve_exact(v.len()).map_err(|_| GeoTypeError::AllocationFailed {
        needed: index_of(v.len()).saturating_mul(index_of(core::mem::size_of::<T>())),
    })?;
    out.extend_from_slice(v);
    Ok(out)
}

/// [`copy_slice`] of an optional channel.
fn copy_opt<T: Copy>(v: Option<&[T]>) -> Result<Option<Vec<T>>, GeoError> {
    match v {
        Some(v) => Ok(Some(copy_slice(v)?)),
        None => Ok(None),
    }
}

/// Maps three columns point by point; `on_error(i)` is the error for a non-finite result at
/// index `i`.
fn map_columns(
    src: [&[f64]; 3],
    f: impl Fn(&[f64; 3]) -> Option<[f64; 3]>,
    on_error: impl Fn(u64) -> GeoTypeError,
) -> Result<[Vec<f64>; 3], GeoError> {
    let [sx, sy, sz] = src;
    let [mut x, mut y, mut z] = columns(sx.len())?;
    for (i, ((&a, &b), &c)) in sx.iter().zip(sy).zip(sz).enumerate() {
        let q = f(&[a, b, c]).ok_or_else(|| on_error(index_of(i)))?;
        x.push(q[0]);
        y.push(q[1]);
        z.push(q[2]);
    }
    Ok([x, y, z])
}

/// The transformed coordinates and attributes of a point set, cloud or mesh vertex buffer.
fn transform_points(
    buf: &PointBuffer,
    attrs: &AttributeSet,
    t: &Transform,
) -> Result<(PointBuffer, AttributeSet), GeoError> {
    let [x, y, z] = map_columns(
        [buf.x.as_slice(), buf.y.as_slice(), buf.z.as_slice()],
        |p| t.apply_point(p).ok(),
        |index| GeoTypeError::NonFinite { index },
    )?;
    let normal = match &attrs.normal {
        Some([nx, ny, nz]) => Some(map_columns(
            [nx.as_slice(), ny.as_slice(), nz.as_slice()],
            |n| t.apply_dir(n).ok().and_then(|m| unit(&m)),
            |index| GeoTypeError::BadNormal { index },
        )?),
        None => None,
    };
    let mut scan_origins = Vec::new();
    scan_origins.try_reserve_exact(attrs.scan_origins.len()).map_err(|_| GeoTypeError::AllocationFailed {
        needed: index_of(attrs.scan_origins.len()).saturating_mul(32),
    })?;
    for &(scanner_id, o) in &attrs.scan_origins {
        let q = t.apply_point(&o).map_err(|_| GeoTypeError::ScanOrigin { scanner_id })?;
        scan_origins.push((scanner_id, q));
    }
    let out = AttributeSet {
        normal,
        intensity: copy_opt(attrs.intensity.as_deref())?,
        colour: copy_opt(attrs.colour.as_deref())?,
        scanner_id: copy_opt(attrs.scanner_id.as_deref())?,
        timestamp_ns: copy_opt(attrs.timestamp_ns.as_deref())?,
        quality: copy_opt(attrs.quality.as_deref())?,
        scan_origins,
        grid: attrs.grid,
    };
    Ok((PointBuffer::new(x, y, z), out))
}

fn transform_mesh(m: &Mesh, t: &Transform, target: FrameRef) -> Result<Mesh, GeoError> {
    let (buf, attrs) = transform_points(m.vertices(), m.vertex_attrs(), t)?;
    let count = usize::try_from(m.triangle_count())
        .map_err(|_| GeoTypeError::AllocationFailed { needed: m.triangle_count().saturating_mul(24) })?;
    let mut tris = Vec::new();
    tris.try_reserve_exact(count)
        .map_err(|_| GeoTypeError::AllocationFailed { needed: m.triangle_count().saturating_mul(24) })?;
    tris.extend(m.triangles());
    let options = MeshOptions {
        triangle_attrs: TriangleAttrs {
            scanner_id: copy_opt(m.triangle_attrs().scanner_id.as_deref())?,
            quality: copy_opt(m.triangle_attrs().quality.as_deref())?,
        },
        force_u64_indexes: m.index_width() == IndexWidth::U64,
        work_budget_bytes: None,
    };
    Ok(Mesh::with_options(target, buf, attrs, tris, options)?)
}
