//! Instances and per-instance entity IDs. Spec node `GEO.CAD.ASSY.MODEL`.
//!
//! An [`Instance`] is one placed occurrence of a definition [`Body`]: it names the body, the
//! GEO.FRAME frame it is placed in and its [`InstancePath`] (the occurrence IDs from the root
//! assembly). Geometry is never copied: an [`InstanceView`] hands out the definition's own
//! faces, edges and vertices together with the instance [`FrameId`], and consumers map
//! through GEO.FRAME (REQ-GEO.CAD.ASSY-005, REQ-003).
//!
//! # Pinned byte layout (REQ-001)
//!
//! - *TopologyId bytes*: the `id` field of GEO.CAD.TOPO.MODEL encoding v1, `0x00` + 16 UUID
//!   bytes for a derived ID, `0x01` + u16 LE byte length + UTF-8 for an imported label.
//! - *Path bytes*: u32 LE component count, then each component's TopologyId bytes. Each
//!   component is self-delimiting, so the encoding is injective.
//! - [`InstanceId`] = UUIDv5([`ASSY_NAMESPACE`], path bytes).
//! - [`InstanceEntityId`] = UUIDv5([`ASSY_NAMESPACE`], path bytes | `0x00` | definition
//!   entity TopologyId bytes).
//!
//! Faces, edges and vertices get instance entity IDs (REQ-GEO.CAD.ASSY-002); shells, loops
//! and coedges do not.
//!
//! # Index build check order (first failure wins)
//!
//! 1. Instance count at most [`MAX_INSTANCE_ENTITIES`] (`TooLarge`).
//! 2. Bodies in ascending body ID: a repeated ID fails `DuplicateBody`.
//! 3. Instances in ascending path: a repeated path fails `DuplicatePath`.
//! 4. Instances in ascending path: the definition must be among the bodies
//!    (`UnknownDefinition`); the running face + edge + vertex total must stay at most
//!    [`MAX_INSTANCE_ENTITIES`] (`TooLarge`).
//! 5. IDs in ascending order: a repeated instance entity ID fails `DuplicateId` (a SHA-1
//!    collision; unreachable in practice, checked so the tables stay a bijection).

use core::fmt;

use fid_geo_frame::{FrameId, FrameTree};

use crate::topo::uuid::{format_uuid, uuid_v5};
use crate::topo::{Body, Edge, EdgeIx, EntityIx, Face, FaceIx, TopologyId, Vertex, VertexIx, MAX_ENTITIES};

/// The project namespace of instance and instance entity IDs: UUIDv5 of
/// `https://github.com/metrologymaven/metrologymaven/spec/geo/cad/assy/model#instance-entity-id-v1`
/// in the RFC 9562 URL namespace, i.e. `fc8f8d57-d822-54d4-8c14-fecaeb696456` (checked by a
/// test).
pub const ASSY_NAMESPACE: [u8; 16] =
    [0xfc, 0x8f, 0x8d, 0x57, 0xd8, 0x22, 0x54, 0xd4, 0x8c, 0x14, 0xfe, 0xca, 0xeb, 0x69, 0x64, 0x56];

/// Most components in one [`InstancePath`] (assembly nesting depth).
pub const MAX_PATH_DEPTH: usize = 256;

/// Most instances, and most instance entities, in one [`InstanceIndex`].
pub const MAX_INSTANCE_ENTITIES: usize = MAX_ENTITIES;

/// The ID of a definition body ([`Body::id`]).
pub type BodyId = [u8; 16];

/// Why an assembly-model operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssyModelError {
    /// An instance path with no components.
    EmptyPath,
    /// An instance path deeper than [`MAX_PATH_DEPTH`].
    PathTooDeep {
        /// Its component count.
        depth: usize,
    },
    /// More than [`MAX_INSTANCE_ENTITIES`] instances or instance entities.
    TooLarge,
    /// Two definition bodies with one ID.
    DuplicateBody {
        /// The repeated body ID.
        body: BodyId,
    },
    /// Two instances with one path (they would share every ID).
    DuplicatePath {
        /// The repeated path.
        path: InstancePath,
    },
    /// The instance's definition body was not supplied, or a view was given another body.
    UnknownDefinition {
        /// The definition the instance names.
        body: BodyId,
    },
    /// Two instance entities derived the same ID.
    DuplicateId {
        /// The repeated ID.
        id: InstanceEntityId,
    },
    /// No such instance entity (REQ-002).
    UnknownEntity,
    /// The instance's frame is not in the supplied frame tree (REQ-004).
    UnknownFrame {
        /// The missing frame.
        frame: FrameId,
    },
}

impl AssyModelError {
    /// Stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            AssyModelError::EmptyPath => "EmptyPath",
            AssyModelError::PathTooDeep { .. } => "PathTooDeep",
            AssyModelError::TooLarge => "TooLarge",
            AssyModelError::DuplicateBody { .. } => "DuplicateBody",
            AssyModelError::DuplicatePath { .. } => "DuplicatePath",
            AssyModelError::UnknownDefinition { .. } => "UnknownDefinition",
            AssyModelError::DuplicateId { .. } => "DuplicateId",
            AssyModelError::UnknownEntity => "UnknownEntity",
            AssyModelError::UnknownFrame { .. } => "UnknownFrame",
        }
    }
}

impl fmt::Display for AssyModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssyModelError::EmptyPath => f.write_str("instance path is empty"),
            AssyModelError::PathTooDeep { depth } => {
                write!(f, "instance path has {depth} components, above {MAX_PATH_DEPTH}")
            }
            AssyModelError::TooLarge => write!(f, "more than {MAX_INSTANCE_ENTITIES} instances or instance entities"),
            AssyModelError::DuplicateBody { body } => write!(f, "duplicate body {}", format_uuid(body)),
            AssyModelError::DuplicatePath { path } => write!(f, "duplicate instance path {path}"),
            AssyModelError::UnknownDefinition { body } => write!(f, "unknown definition body {}", format_uuid(body)),
            AssyModelError::DuplicateId { id } => write!(f, "duplicate instance entity id {id}"),
            AssyModelError::UnknownEntity => f.write_str("unknown instance entity"),
            AssyModelError::UnknownFrame { frame } => write!(f, "unknown frame {frame}"),
        }
    }
}

impl std::error::Error for AssyModelError {}

/// Appends the GEO.CAD.TOPO.MODEL encoding v1 `id` field of `id`.
fn put_topology_id(out: &mut Vec<u8>, id: &TopologyId) {
    match id {
        TopologyId::Derived(b) => {
            out.push(0x00);
            out.extend_from_slice(b);
        }
        TopologyId::Imported(l) => {
            let bytes = l.as_str().as_bytes();
            out.push(0x01);
            // A label is at most MAX_LABEL_BYTES (256) bytes, so the length fits u16.
            out.extend_from_slice(&u16::try_from(bytes.len()).unwrap_or(u16::MAX).to_le_bytes());
            out.extend_from_slice(bytes);
        }
    }
}

/// The occurrence IDs from the root assembly down to one placement: 1 to
/// [`MAX_PATH_DEPTH`] [`TopologyId`]s (imported occurrence labels or derived IDs). Ordered
/// component-wise by the [`TopologyId`] order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InstancePath(Vec<TopologyId>);

impl InstancePath {
    /// A path; `EmptyPath` or `PathTooDeep` outside `1..=MAX_PATH_DEPTH` components.
    pub fn new(components: Vec<TopologyId>) -> Result<InstancePath, AssyModelError> {
        if components.is_empty() {
            return Err(AssyModelError::EmptyPath);
        }
        if components.len() > MAX_PATH_DEPTH {
            return Err(AssyModelError::PathTooDeep { depth: components.len() });
        }
        Ok(InstancePath(components))
    }

    /// The components, root first.
    pub fn components(&self) -> &[TopologyId] {
        &self.0
    }

    /// The canonical path bytes: u32 LE component count, then each component's TopologyId
    /// bytes (encoding v1 `id` field).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + 17 * self.0.len());
        // At most MAX_PATH_DEPTH (256) components, so the count fits u32.
        out.extend_from_slice(&u32::try_from(self.0.len()).unwrap_or(u32::MAX).to_le_bytes());
        for c in &self.0 {
            put_topology_id(&mut out, c);
        }
        out
    }
}

impl fmt::Display for InstancePath {
    /// Components joined by `/`, each shown as [`TopologyId`] shows it (labels quoted).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, c) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("/")?;
            }
            write!(f, "{c}")?;
        }
        Ok(())
    }
}

macro_rules! uuid_id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name([u8; 16]);

        impl $name {
            /// An ID from its bytes.
            pub const fn from_bytes(bytes: [u8; 16]) -> $name {
                $name(bytes)
            }

            /// The bytes.
            pub const fn to_bytes(self) -> [u8; 16] {
                self.0
            }
        }

        impl fmt::Display for $name {
            /// Hyphenated lowercase UUID.
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&format_uuid(&self.0))
            }
        }
    };
}

uuid_id!(
    /// The ID of an instance: UUIDv5([`ASSY_NAMESPACE`], path bytes). Ordered bytewise.
    InstanceId
);
uuid_id!(
    /// The ID of a face, edge or vertex of one instance: UUIDv5([`ASSY_NAMESPACE`], path
    /// bytes | `0x00` | definition TopologyId bytes). Ordered bytewise.
    InstanceEntityId
);

impl InstanceId {
    /// The ID of the instance at `path`.
    pub fn derive(path: &InstancePath) -> InstanceId {
        InstanceId(uuid_v5(&ASSY_NAMESPACE, &path.to_bytes()))
    }
}

impl InstanceEntityId {
    /// The ID of definition entity `entity` in the instance at `path` (REQ-001).
    pub fn derive(path: &InstancePath, entity: &TopologyId) -> InstanceEntityId {
        entity_id_from_path_bytes(&path.to_bytes(), entity, &mut Vec::new())
    }
}

/// UUIDv5 over `path_bytes | 0x00 | entity bytes`, reusing `buf` as scratch.
fn entity_id_from_path_bytes(path_bytes: &[u8], entity: &TopologyId, buf: &mut Vec<u8>) -> InstanceEntityId {
    buf.clear();
    buf.extend_from_slice(path_bytes);
    buf.push(0x00);
    put_topology_id(buf, entity);
    InstanceEntityId(uuid_v5(&ASSY_NAMESPACE, buf))
}

/// One placed occurrence of a definition body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    id: InstanceId,
    definition: BodyId,
    frame: FrameId,
    path: InstancePath,
}

impl Instance {
    /// The instance of body `definition` placed in `frame` at `path`; its ID is derived from
    /// the path, so it is stable across re-import.
    pub fn new(definition: BodyId, frame: FrameId, path: InstancePath) -> Instance {
        Instance { id: InstanceId::derive(&path), definition, frame, path }
    }

    /// The instance ID.
    pub fn id(&self) -> InstanceId {
        self.id
    }

    /// The definition body ID.
    pub fn definition(&self) -> &BodyId {
        &self.definition
    }

    /// The frame the instance is placed in.
    pub fn frame(&self) -> FrameId {
        self.frame
    }

    /// The assembly path.
    pub fn path(&self) -> &InstancePath {
        &self.path
    }

    /// The ID of definition entity `entity` in this instance (REQ-001).
    pub fn entity_id(&self, entity: &TopologyId) -> InstanceEntityId {
        InstanceEntityId::derive(&self.path, entity)
    }
}

/// Dense index of an instance in an [`InstanceIndex`], in ascending path order (so it does
/// not depend on the order the instances were supplied in).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct InstanceIx(u32);

impl InstanceIx {
    /// Position in ascending path order.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// The two-way mapping between instance entity IDs and (instance, definition entity), held
/// as two sorted vectors; every lookup is a binary search, O(log n) (REQ-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceIndex {
    instances: Vec<Instance>,
    forward: Vec<(InstanceEntityId, InstanceIx, EntityIx)>,
    backward: Vec<(InstanceIx, EntityIx, InstanceEntityId)>,
}

/// Index below [`MAX_INSTANCE_ENTITIES`] (checked first), so it fits `u32`.
fn ix32(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// The faces, edges and vertices of `body` with their IDs, in ascending ID order.
fn exposed(body: &Body) -> impl Iterator<Item = (EntityIx, &TopologyId)> {
    body.ids()
        .iter()
        .filter(|(_, ix)| matches!(ix, EntityIx::Face(_) | EntityIx::Edge(_) | EntityIx::Vertex(_)))
        .map(|(id, ix)| (*ix, id))
}

fn exposed_count(body: &Body) -> usize {
    body.faces().len() + body.edges().len() + body.vertices().len()
}

impl InstanceIndex {
    /// Indexes `instances` over their definition `bodies`, in the check order of the module
    /// documentation. The result does not depend on the order of either input.
    pub fn build(mut instances: Vec<Instance>, bodies: &[&Body]) -> Result<InstanceIndex, AssyModelError> {
        if instances.len() > MAX_INSTANCE_ENTITIES {
            return Err(AssyModelError::TooLarge);
        }
        let mut defs: Vec<&Body> = bodies.to_vec();
        defs.sort_unstable_by(|a, b| a.id().cmp(b.id()));
        for w in defs.windows(2) {
            if w[0].id() == w[1].id() {
                return Err(AssyModelError::DuplicateBody { body: *w[0].id() });
            }
        }
        instances.sort_unstable_by(|a, b| a.path.cmp(&b.path));
        for w in instances.windows(2) {
            if w[0].path == w[1].path {
                return Err(AssyModelError::DuplicatePath { path: w[0].path.clone() });
            }
        }
        let mut resolved: Vec<&Body> = Vec::with_capacity(instances.len());
        let mut total = 0usize;
        for inst in &instances {
            let body = match defs.binary_search_by(|b| b.id().cmp(&inst.definition)) {
                Ok(i) => defs[i],
                Err(_) => return Err(AssyModelError::UnknownDefinition { body: inst.definition }),
            };
            total = total.saturating_add(exposed_count(body));
            if total > MAX_INSTANCE_ENTITIES {
                return Err(AssyModelError::TooLarge);
            }
            resolved.push(body);
        }
        let mut forward = Vec::with_capacity(total);
        let mut backward = Vec::with_capacity(total);
        let mut buf = Vec::new();
        for (i, (inst, body)) in instances.iter().zip(&resolved).enumerate() {
            let ix = InstanceIx(ix32(i));
            let path_bytes = inst.path.to_bytes();
            for (ent, tid) in exposed(body) {
                let id = entity_id_from_path_bytes(&path_bytes, tid, &mut buf);
                forward.push((id, ix, ent));
                backward.push((ix, ent, id));
            }
        }
        forward.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        for w in forward.windows(2) {
            if w[0].0 == w[1].0 {
                return Err(AssyModelError::DuplicateId { id: w[0].0 });
            }
        }
        backward.sort_unstable_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
        Ok(InstanceIndex { instances, forward, backward })
    }

    /// The instances in ascending path order.
    pub fn instances(&self) -> &[Instance] {
        &self.instances
    }

    /// The instance at `ix`.
    pub fn instance(&self, ix: InstanceIx) -> Option<&Instance> {
        self.instances.get(ix.index())
    }

    /// The instance at `path`, by binary search.
    pub fn find(&self, path: &InstancePath) -> Option<InstanceIx> {
        self.instances.binary_search_by(|i| i.path.cmp(path)).ok().map(|i| InstanceIx(ix32(i)))
    }

    /// Number of instance entities.
    pub fn entity_count(&self) -> usize {
        self.forward.len()
    }

    /// Every instance entity in ascending ID order.
    pub fn entities(&self) -> impl Iterator<Item = (InstanceEntityId, InstanceIx, EntityIx)> + '_ {
        self.forward.iter().copied()
    }

    /// The (instance, definition entity) of an instance entity ID; `UnknownEntity` when the
    /// ID is not in the index. O(log n).
    pub fn resolve(&self, id: &InstanceEntityId) -> Result<(InstanceIx, EntityIx), AssyModelError> {
        match self.forward.binary_search_by(|e| e.0.cmp(id)) {
            Ok(i) => Ok((self.forward[i].1, self.forward[i].2)),
            Err(_) => Err(AssyModelError::UnknownEntity),
        }
    }

    /// The instance entity ID of definition entity `entity` in `instance`; `UnknownEntity`
    /// for an instance or entity not in the index (including shells, loops and coedges,
    /// which have no instance entity ID). O(log n).
    pub fn entity_id(&self, instance: InstanceIx, entity: EntityIx) -> Result<InstanceEntityId, AssyModelError> {
        match self.backward.binary_search_by(|e| (e.0, e.1).cmp(&(instance, entity))) {
            Ok(i) => Ok(self.backward[i].2),
            Err(_) => Err(AssyModelError::UnknownEntity),
        }
    }
}

/// An instance seen through its definition: the definition's own geometry plus the instance
/// [`FrameId`]. It offers no method that returns transformed copies of geometry (REQ-003);
/// coordinates are in the instance frame, and mapping them is GEO.FRAME's job.
#[derive(Debug, Clone, Copy)]
pub struct InstanceView<'a> {
    instance: &'a Instance,
    definition: &'a Body,
}

impl<'a> InstanceView<'a> {
    /// A view of `instance` over `definition`. Errors, in order: `UnknownDefinition` when
    /// `definition` is not the instance's definition body; `UnknownFrame` when the instance
    /// frame is not in `tree` (REQ-004).
    pub fn new(
        instance: &'a Instance,
        definition: &'a Body,
        tree: &FrameTree,
    ) -> Result<InstanceView<'a>, AssyModelError> {
        if definition.id() != &instance.definition {
            return Err(AssyModelError::UnknownDefinition { body: instance.definition });
        }
        if tree.get(instance.frame).is_none() {
            return Err(AssyModelError::UnknownFrame { frame: instance.frame });
        }
        Ok(InstanceView { instance, definition })
    }

    /// The instance.
    pub fn instance(&self) -> &'a Instance {
        self.instance
    }

    /// The instance frame: the definition's coordinates are in this frame.
    pub fn frame(&self) -> FrameId {
        self.instance.frame
    }

    /// The definition body itself (shared, never copied).
    pub fn definition(&self) -> &'a Body {
        self.definition
    }

    /// A definition face.
    pub fn face(&self, ix: FaceIx) -> Option<&'a Face> {
        self.definition.face(ix)
    }

    /// A definition edge.
    pub fn edge(&self, ix: EdgeIx) -> Option<&'a Edge> {
        self.definition.edge(ix)
    }

    /// A definition vertex.
    pub fn vertex(&self, ix: VertexIx) -> Option<&'a Vertex> {
        self.definition.vertex(ix)
    }

    /// The ID of definition entity `entity` in this instance (REQ-001).
    pub fn entity_id(&self, entity: &TopologyId) -> InstanceEntityId {
        self.instance.entity_id(entity)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::topo::Label;

    #[test]
    fn namespace_is_derived() {
        let url = [0x6b, 0xa7, 0xb8, 0x11, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8];
        let ns = uuid_v5(
            &url,
            b"https://github.com/metrologymaven/metrologymaven/spec/geo/cad/assy/model#instance-entity-id-v1",
        );
        assert_eq!(format_uuid(&ns), format_uuid(&ASSY_NAMESPACE));
    }

    #[test]
    fn path_bytes_layout() {
        let p =
            InstancePath::new(vec![TopologyId::Imported(Label::new("#5").unwrap()), TopologyId::Derived([0xab; 16])])
                .unwrap();
        let mut want = vec![2, 0, 0, 0, 0x01, 2, 0, b'#', b'5', 0x00];
        want.extend_from_slice(&[0xab; 16]);
        assert_eq!(p.to_bytes(), want);
        assert_eq!(p.to_string(), "\"#5\"/abababab-abab-abab-abab-abababababab");
    }

    #[test]
    fn path_bounds() {
        assert_eq!(InstancePath::new(vec![]), Err(AssyModelError::EmptyPath));
        let one = TopologyId::Derived([1; 16]);
        assert!(InstancePath::new(vec![one.clone(); MAX_PATH_DEPTH]).is_ok());
        assert_eq!(
            InstancePath::new(vec![one; MAX_PATH_DEPTH + 1]),
            Err(AssyModelError::PathTooDeep { depth: MAX_PATH_DEPTH + 1 })
        );
    }
}
