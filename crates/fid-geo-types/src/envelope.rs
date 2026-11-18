//! The element envelope: kind tags, frame reference, bounding box, CAD body handle and
//! kind guards. Spec node `GEO.TYPES.ENV`.

use crate::error::GeoTypeError;
use crate::mesh::Mesh;
use crate::points::{PointCloud, PointSet};
use core::cmp::Ordering;
use core::fmt;
use fid_types::Digest;

/// An opaque 128-bit frame ID. The frame model itself is GEO.FRAME; elements only carry
/// the reference (REQ-GEO.TYPES.ENV-002). There is deliberately no `Default`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameRef([u8; 16]);

impl FrameRef {
    /// The frame with this ID.
    pub const fn from_bytes(id: [u8; 16]) -> FrameRef {
        FrameRef(id)
    }

    /// The 16 ID bytes.
    pub const fn to_bytes(self) -> [u8; 16] {
        self.0
    }
}

/// The closed set of element kinds (REQ-GEO.TYPES-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ElementKind {
    PointSet,
    PointCloud,
    Mesh,
    CadBodyRef,
}

impl ElementKind {
    /// Every kind, in declaration order.
    pub const ALL: [ElementKind; 4] =
        [ElementKind::PointSet, ElementKind::PointCloud, ElementKind::Mesh, ElementKind::CadBodyRef];

    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            ElementKind::PointSet => "point_set",
            ElementKind::PointCloud => "point_cloud",
            ElementKind::Mesh => "mesh",
            ElementKind::CadBodyRef => "cad_body_ref",
        }
    }
}

impl fmt::Display for ElementKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// IEEE 754 totalOrder minimum (KNP-1: never `f64::min`). Ties keep `a`.
pub(crate) fn total_min(a: f64, b: f64) -> f64 {
    if b.total_cmp(&a) == Ordering::Less {
        b
    } else {
        a
    }
}

/// IEEE 754 totalOrder maximum (KNP-1: never `f64::max`). Ties keep `a`.
pub(crate) fn total_max(a: f64, b: f64) -> f64 {
    if b.total_cmp(&a) == Ordering::Greater {
        b
    } else {
        a
    }
}

fn map3(a: [f64; 3], b: [f64; 3], f: fn(f64, f64) -> f64) -> [f64; 3] {
    [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2])]
}

/// An axis-aligned bounding box in metres. Always finite with `min <= max` per axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    min: [f64; 3],
    max: [f64; 3],
}

impl Aabb {
    /// A box from its corners. Fails on a non-finite value or `min > max` on any axis.
    pub fn new(min: [f64; 3], max: [f64; 3]) -> Result<Aabb, GeoTypeError> {
        for (i, v) in min.iter().chain(max.iter()).enumerate() {
            if !v.is_finite() {
                return Err(GeoTypeError::NonFinite { index: i as u64 });
            }
        }
        for (axis, (lo, hi)) in min.iter().zip(max.iter()).enumerate() {
            if lo > hi {
                return Err(GeoTypeError::InvertedAabb { axis: axis as u8 });
            }
        }
        Ok(Aabb { min, max })
    }

    /// The box of finite points, as a fixed-order totalOrder min/max fold (KNP-1).
    /// Callers guarantee finiteness (element constructors check it).
    pub(crate) fn of_points(mut pts: impl Iterator<Item = [f64; 3]>) -> Result<Aabb, GeoTypeError> {
        let first = pts.next().ok_or(GeoTypeError::Empty)?;
        let (min, max) = pts.fold((first, first), |(lo, hi), p| (map3(lo, p, total_min), map3(hi, p, total_max)));
        Ok(Aabb { min, max })
    }

    /// The minimum corner.
    pub fn min(&self) -> [f64; 3] {
        self.min
    }

    /// The maximum corner.
    pub fn max(&self) -> [f64; 3] {
        self.max
    }

    /// The smallest box containing both boxes.
    pub fn union(&self, other: &Aabb) -> Aabb {
        Aabb { min: map3(self.min, other.min, total_min), max: map3(self.max, other.max, total_max) }
    }

    /// Whether `p` lies inside or on the boundary. NaN coordinates are never contained.
    pub fn contains(&self, p: [f64; 3]) -> bool {
        (0..3).all(|k| self.min[k] <= p[k] && p[k] <= self.max[k])
    }

    /// Whether `other` lies entirely inside this box (boundaries included).
    pub fn contains_aabb(&self, other: &Aabb) -> bool {
        self.contains(other.min) && self.contains(other.max)
    }
}

/// An opaque handle to a CAD body: body ID, the digest of the source blob it was imported
/// from, and the bounding box recorded at import time. No topology (that is GEO.CAD).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CadBodyRef {
    frame: FrameRef,
    body: [u8; 16],
    source_digest: Digest,
    aabb: Aabb,
}

impl CadBodyRef {
    /// A handle in `frame`.
    pub fn new(frame: FrameRef, body: [u8; 16], source_digest: Digest, aabb: Aabb) -> CadBodyRef {
        CadBodyRef { frame, body, source_digest, aabb }
    }

    /// The frame the body is expressed in.
    pub fn frame(&self) -> FrameRef {
        self.frame
    }

    /// The body ID.
    pub fn body(&self) -> [u8; 16] {
        self.body
    }

    /// The digest of the source blob.
    pub fn source_digest(&self) -> Digest {
        self.source_digest
    }

    /// The box stored at import time (a CAD body always has one).
    pub fn aabb(&self) -> Aabb {
        self.aabb
    }
}

/// The envelope every geometric element sits in (REQ-GEO.TYPES-001, -002).
#[derive(Debug, Clone, PartialEq)]
pub enum GeometryElement {
    PointSet(PointSet),
    PointCloud(PointCloud),
    Mesh(Mesh),
    CadBody(CadBodyRef),
}

fn wrong(expected: ElementKind, found: ElementKind) -> GeoTypeError {
    GeoTypeError::WrongKind { expected, found }
}

impl GeometryElement {
    /// The kind of the wrapped element (REQ-GEO.TYPES.ENV-003).
    pub fn kind(&self) -> ElementKind {
        match self {
            GeometryElement::PointSet(_) => ElementKind::PointSet,
            GeometryElement::PointCloud(_) => ElementKind::PointCloud,
            GeometryElement::Mesh(_) => ElementKind::Mesh,
            GeometryElement::CadBody(_) => ElementKind::CadBodyRef,
        }
    }

    /// The frame of the wrapped element.
    pub fn frame(&self) -> FrameRef {
        match self {
            GeometryElement::PointSet(e) => e.frame(),
            GeometryElement::PointCloud(e) => e.frame(),
            GeometryElement::Mesh(e) => e.frame(),
            GeometryElement::CadBody(e) => e.frame(),
        }
    }

    /// The bounding box; [`GeoTypeError::Empty`] for an element with no points
    /// (REQ-GEO.TYPES.ENV-005). A CAD body returns its stored box.
    pub fn aabb(&self) -> Result<Aabb, GeoTypeError> {
        match self {
            GeometryElement::PointSet(e) => e.aabb(),
            GeometryElement::PointCloud(e) => e.aabb(),
            GeometryElement::Mesh(e) => e.aabb(),
            GeometryElement::CadBody(e) => Ok(e.aabb()),
        }
    }

    /// The kind guard: `Ok` if the element is of kind `expected`, else `WrongKind`.
    pub fn require(&self, expected: ElementKind) -> Result<(), GeoTypeError> {
        let found = self.kind();
        if found == expected {
            Ok(())
        } else {
            Err(wrong(expected, found))
        }
    }

    /// The wrapped point set, or `WrongKind { expected: PointSet, .. }`.
    pub fn as_point_set(&self) -> Result<&PointSet, GeoTypeError> {
        match self {
            GeometryElement::PointSet(e) => Ok(e),
            other => Err(wrong(ElementKind::PointSet, other.kind())),
        }
    }

    /// The wrapped point cloud, or `WrongKind { expected: PointCloud, .. }`.
    pub fn as_point_cloud(&self) -> Result<&PointCloud, GeoTypeError> {
        match self {
            GeometryElement::PointCloud(e) => Ok(e),
            other => Err(wrong(ElementKind::PointCloud, other.kind())),
        }
    }

    /// The wrapped mesh, or `WrongKind { expected: Mesh, .. }`.
    pub fn as_mesh(&self) -> Result<&Mesh, GeoTypeError> {
        match self {
            GeometryElement::Mesh(e) => Ok(e),
            other => Err(wrong(ElementKind::Mesh, other.kind())),
        }
    }

    /// The wrapped CAD body handle, or `WrongKind { expected: CadBodyRef, .. }`.
    pub fn as_cad_body(&self) -> Result<&CadBodyRef, GeoTypeError> {
        match self {
            GeometryElement::CadBody(e) => Ok(e),
            other => Err(wrong(ElementKind::CadBodyRef, other.kind())),
        }
    }
}
