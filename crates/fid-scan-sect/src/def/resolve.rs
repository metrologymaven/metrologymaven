//! Element references and their resolution (REQ-SCAN.SECT.DEF-003).
//!
//! A `FromElement` definition names a planar CAD face or a constructed plane by an
//! [`ElementRef`] and resolves it through an [`ElementResolver`], which answers with the
//! element's geometry and its current digest. [`ModelResolver`] is the resolver over the
//! kernel's own models:
//!
//! - **CAD face** ([`ElementRef::CadFace`]): the face of a `fid-geo-cad` [`Body`], found by the
//!   body ID and the face's stable [`TopologyId`]. A face whose surface is an analytic plane is
//!   mapped by GEO.CAD.NOMINAL.DIRECT [`map_face`]: the plane is the placement origin and the
//!   **out-of-material normal** (the placement axis for a face with sense `Same`, its exact
//!   negation for `Reversed`), both copied, not recomputed. Any other surface is reported as
//!   not planar with its kind (`cylinder`, `cone`, `sphere`, `torus`, or `nurbs` for a NURBS
//!   surface, even a flat one: recognising it is GEO.CAD.NOMINAL.RECOG's job). The digest is
//!   the body digest ([`body_digest`]): GEO.CAD gives a face no digest of its own, and the body
//!   digest changes whenever the face does (it also changes when another face of the body
//!   does, so a section may be recomputed when it did not need to be, never the reverse).
//! - **Constructed plane** ([`ElementRef::Constructed`]): a KRN.CONS
//!   [`ConstructedElement`] by its [`ElementId`]. A `FittedFeature::Plane` gives its point and
//!   normal as stored (sign-normalised by KRN.LSQ.TRAIT); any other feature or a value is not
//!   planar. The digest is the element's output digest ([`ConstructedElement::digest`]).
//!
//! **Frame.** Neither a B-rep body nor a constructed element names the coordinate frame its
//! values are in, so the caller states it when adding the element ([`ModelResolver::add_body`],
//! [`ModelResolver::add_constructed`]) and the resolver reports it with the plane. A definition
//! takes the element's frame and refuses an element whose frame differs from its own.
//!
//! What a resolver says is checked again by the definition (finite point, unit normal, digest
//! unchanged since recording, same frame), so a custom resolver cannot slip a bad plane through.

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;

use fid_construct::contract::{ConstructedElement, ElementId};
use fid_fit::contract::FittedFeature;
use fid_geo_cad::{body_digest, map_face, Body, DirectResult, EntityIx, NominalParams, SurfaceParams, TopologyId};
use fid_geo_frame::FrameId;
use fid_types::Digest;

/// A reference to the element a `FromElement` definition takes its plane from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ElementRef {
    /// Face `face` of the CAD body with ID `body`.
    CadFace { body: [u8; 16], face: TopologyId },
    /// A constructed element (KRN.CONS).
    Constructed(ElementId),
}

/// The geometry a resolver found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResolvedGeometry {
    /// A plane through `point` with normal `normal` (metres; the definition checks both).
    Plane { point: [f64; 3], normal: [f64; 3] },
    /// Not a plane; `kind` says what it is.
    NotPlanar { kind: &'static str },
}

/// A resolved element: its geometry, the frame its values are in and its current digest.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub geometry: ResolvedGeometry,
    pub frame: FrameId,
    pub digest: Digest,
}

/// Why a reference could not be resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveError {
    /// No such body, face or element.
    NotFound,
    /// The CAD entity exists but is not a face; `kind` is its entity kind.
    NotAFace { kind: &'static str },
    /// The source refused; `code` is its error code.
    Upstream { code: &'static str },
}

/// Resolves element references (REQ-SCAN.SECT.DEF-003).
pub trait ElementResolver {
    fn resolve(&self, element: &ElementRef) -> Result<Resolved, ResolveError>;
}

/// The resolver that knows no element: every reference is [`ResolveError::NotFound`]. For
/// definitions without a `FromElement`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoElements;

impl ElementResolver for NoElements {
    fn resolve(&self, _element: &ElementRef) -> Result<Resolved, ResolveError> {
        Err(ResolveError::NotFound)
    }
}

/// Why an element could not be added to a [`ModelResolver`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolverBuildError {
    /// A body with this ID was already added.
    DuplicateBody { id: [u8; 16] },
    /// A constructed element with this ID was already added.
    DuplicateElement { id: ElementId },
}

impl ResolverBuildError {
    /// A stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            ResolverBuildError::DuplicateBody { .. } => "duplicate_body",
            ResolverBuildError::DuplicateElement { .. } => "duplicate_element",
        }
    }
}

/// Resolves CAD faces of the added bodies and the added constructed elements (module
/// documentation). Lookups are `BTreeMap`s (no `HashMap`, KNP-1).
#[derive(Debug, Clone, Default)]
pub struct ModelResolver<'a> {
    bodies: BTreeMap<[u8; 16], (&'a Body, FrameId)>,
    constructed: BTreeMap<ElementId, (&'a ConstructedElement, FrameId)>,
}

impl<'a> ModelResolver<'a> {
    pub fn new() -> ModelResolver<'a> {
        ModelResolver::default()
    }

    /// Adds a body whose coordinates are in `frame`; two bodies with the same ID are refused
    /// (a reference would be ambiguous).
    pub fn add_body(&mut self, body: &'a Body, frame: FrameId) -> Result<(), ResolverBuildError> {
        let id = *body.id();
        match self.bodies.entry(id) {
            Entry::Occupied(_) => Err(ResolverBuildError::DuplicateBody { id }),
            Entry::Vacant(v) => {
                v.insert((body, frame));
                Ok(())
            }
        }
    }

    /// Adds a constructed element whose values are in `frame`; a repeated ID is refused.
    pub fn add_constructed(
        &mut self,
        id: ElementId,
        element: &'a ConstructedElement,
        frame: FrameId,
    ) -> Result<(), ResolverBuildError> {
        match self.constructed.entry(id) {
            Entry::Occupied(_) => Err(ResolverBuildError::DuplicateElement { id }),
            Entry::Vacant(v) => {
                v.insert((element, frame));
                Ok(())
            }
        }
    }

    fn cad_face(&self, body: &[u8; 16], face: &TopologyId) -> Result<Resolved, ResolveError> {
        let &(b, frame) = self.bodies.get(body).ok_or(ResolveError::NotFound)?;
        let ix = match b.lookup(face) {
            Some(EntityIx::Face(ix)) => ix,
            Some(other) => return Err(ResolveError::NotAFace { kind: other.kind().as_str() }),
            None => return Err(ResolveError::NotFound),
        };
        let f = b.face(ix).ok_or(ResolveError::NotFound)?;
        let geometry = match f.surface().params() {
            Some(SurfaceParams::Plane { .. }) => match map_face(b, ix) {
                Ok(DirectResult::Element(e)) => match (e.params(), e.plane_normal_out_of_material()) {
                    (NominalParams::Plane { frame }, Some(normal)) => {
                        ResolvedGeometry::Plane { point: frame.origin, normal }
                    }
                    _ => return Err(ResolveError::Upstream { code: "plane_face_without_plane_element" }),
                },
                Ok(DirectResult::NotAnalytic) => ResolvedGeometry::NotPlanar { kind: "nurbs" },
                Ok(DirectResult::UnsupportedKind { kind }) => ResolvedGeometry::NotPlanar { kind: kind.as_str() },
                Err(e) => return Err(ResolveError::Upstream { code: e.code() }),
            },
            Some(SurfaceParams::Cylinder { .. }) => ResolvedGeometry::NotPlanar { kind: "cylinder" },
            Some(SurfaceParams::Cone { .. }) => ResolvedGeometry::NotPlanar { kind: "cone" },
            Some(SurfaceParams::Sphere { .. }) => ResolvedGeometry::NotPlanar { kind: "sphere" },
            Some(SurfaceParams::Torus { .. }) => ResolvedGeometry::NotPlanar { kind: "torus" },
            None => ResolvedGeometry::NotPlanar { kind: "nurbs" },
        };
        Ok(Resolved { geometry, frame, digest: body_digest(b) })
    }

    fn constructed(&self, id: &ElementId) -> Result<Resolved, ResolveError> {
        let &(e, frame) = self.constructed.get(id).ok_or(ResolveError::NotFound)?;
        let geometry = match e.as_feature() {
            Some(FittedFeature::Plane { p, n }) => ResolvedGeometry::Plane { point: *p, normal: *n },
            Some(other) => ResolvedGeometry::NotPlanar { kind: other.primitive().slug() },
            None => ResolvedGeometry::NotPlanar { kind: "value" },
        };
        Ok(Resolved { geometry, frame, digest: e.digest() })
    }
}

impl ElementResolver for ModelResolver<'_> {
    fn resolve(&self, element: &ElementRef) -> Result<Resolved, ResolveError> {
        match element {
            ElementRef::CadFace { body, face } => self.cad_face(body, face),
            ElementRef::Constructed(id) => self.constructed(id),
        }
    }
}
