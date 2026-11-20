//! Element registration (REQ-GEO.TYPES.REG-001 to -003): putting a geometric element into
//! the CORE element tree under a stable ID, inside the calling command's transaction.
//!
//! IDs (CONSTITUTION §5):
//! - [`IdOrigin::Imported`]: a source ID in canonical UUID form is the entity ID, unchanged
//!   (`id_collision` if taken). CORE entity IDs are UUIDs, so a source ID in any other form
//!   (such as `step:#123`) is kept verbatim on the element as its `source_id`, and the
//!   entity ID is the UUIDv5 of the project namespace over `{"source_id": <text>}`
//!   ([`imported_text_id`]): the same source ID always maps to the same entity.
//! - [`IdOrigin::New`]: a UUIDv7 from CORE's ID service, whose time and random bits are
//!   recorded observations.
//! - [`IdOrigin::Derived`]: the UUIDv5 of the project namespace over the RFC 8785 canonical
//!   bytes of `{"command": <command ID>, "inputs": [{"digest", "id"}, ...]}`, inputs sorted
//!   ascending by ID ([`derive_id`]). The canonical form puts the command ID first and the
//!   sorted inputs with their digests after it, as the leaf's Design pins.
//!
//! Registration records one `dep.edges_set@1` event (from each derivation input; none for
//! other origins) and marks the new element `current`. Replacement keeps the ID, stores the
//! new element and its facts, and runs CORE.DEP change propagation for the element, whose
//! result (the `dep.elements_marked@1` event and any recompute submission) the caller
//! journals and submits in the same commit.
//!
//! Every refusal is checked before anything changes, so on error the tree, the edge index
//! and the element states are as they were.

use crate::error::GeoCmdError;
use fid_canon::CValue;
use fid_dep::{EdgeIndex, EdgesSet, ElementState, ElementStates, Propagation, RecomputeMode, MAX_INPUTS};
use fid_geo_types::{digest, Aabb, ElementDigest, ElementKind, FrameRef, GeometryElement, TopologyFacts};
use fid_project::{IdIndex, IdService, ObservationSource};
use fid_types::{CommandId, Digest};
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};

/// A stable entity ID (CONSTITUTION §5); the same type CORE.DEP uses.
pub type EntityId = fid_dep::EntityId;

/// Longest imported source ID, in bytes.
pub const MAX_SOURCE_ID_BYTES: usize = 256;
/// Largest point or triangle count reported: 2^53, the largest integer a JSON number holds
/// exactly.
pub const MAX_COUNT: u64 = 1 << 53;

/// Where a registered element's ID comes from (CONSTITUTION §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdOrigin {
    /// Imported with this source ID, which is kept unchanged.
    Imported(String),
    /// New: a UUIDv7 from CORE.
    New,
    /// Derived by `command` from `inputs` (entity ID and content digest, any order).
    Derived { command: CommandId, inputs: Vec<(EntityId, Digest)> },
}

/// The wire name of a kind in describe results: the GEO.TYPES type name.
pub fn kind_name(kind: ElementKind) -> &'static str {
    match kind {
        ElementKind::PointSet => "PointSet",
        ElementKind::PointCloud => "PointCloud",
        ElementKind::Mesh => "Mesh",
        ElementKind::CadBodyRef => "CadBodyRef",
    }
}

/// The facts of an element, computed once at registration so a describe never rescans.
#[derive(Debug, Clone, PartialEq)]
pub struct ElementFacts {
    pub kind: ElementKind,
    /// Point count (vertex count for a mesh); `None` for a CAD body handle.
    pub points: Option<u64>,
    /// Triangle count of a mesh; `None` for other kinds.
    pub triangles: Option<u64>,
    /// `None` for an element with no points.
    pub aabb: Option<Aabb>,
    pub frame: FrameRef,
    pub digest: ElementDigest,
    /// Names of the per-point (per-vertex) attribute channels present, in field order.
    pub channels: Vec<&'static str>,
    /// Topology facts of a mesh; `None` for other kinds.
    pub topology: Option<TopologyFacts>,
}

fn count(n: u64, what: &'static str) -> Result<u64, GeoCmdError> {
    if n > MAX_COUNT {
        return Err(GeoCmdError::InputLimit(what));
    }
    Ok(n)
}

impl ElementFacts {
    /// Computes the facts of `element`: one digest pass and one bounding-box pass. Refuses
    /// counts above [`MAX_COUNT`] (`input_limit`).
    pub fn of(element: &GeometryElement) -> Result<ElementFacts, GeoCmdError> {
        let (points, triangles, channels, topology) = match element {
            GeometryElement::PointSet(p) => (Some(p.len()), None, p.attributes().channel_names(), None),
            GeometryElement::PointCloud(p) => (Some(p.len()), None, p.attributes().channel_names(), None),
            GeometryElement::Mesh(m) => {
                (Some(m.vertex_count()), Some(m.triangle_count()), m.vertex_attrs().channel_names(), Some(m.facts()))
            }
            GeometryElement::CadBody(_) => (None, None, Vec::new(), None),
        };
        let points = points.map(|n| count(n, "points")).transpose()?;
        let triangles = triangles.map(|n| count(n, "triangles")).transpose()?;
        Ok(ElementFacts {
            kind: element.kind(),
            points,
            triangles,
            aabb: element.aabb().ok(),
            frame: element.frame(),
            digest: digest(element),
            channels,
            topology,
        })
    }
}

/// A geometric element in the tree, with its cached facts and imported source ID.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoEntry {
    pub element: GeometryElement,
    pub facts: ElementFacts,
    /// The imported source ID, verbatim; `None` for new and derived elements.
    pub source_id: Option<String>,
}

/// One entry of the CORE element tree, as this crate sees it.
#[derive(Debug, Clone, PartialEq)]
pub enum TreeEntry {
    Geometry(Box<GeoEntry>),
    /// Any non-geometric element (a feature, a characteristic, ...), by its kind name.
    Other {
        kind: String,
    },
}

/// The CORE element tree inside the caller's transaction (CORE.PRJ.TREE; the leaf assumes
/// `insert`, `replace` and `get`). Implementations refuse `insert` of an ID already
/// present and `replace` of an absent one with [`GeoCmdError::IdCollision`] and
/// [`GeoCmdError::NotFound`].
pub trait ElementTree {
    fn get(&self, id: &EntityId) -> Option<&TreeEntry>;
    fn insert(&mut self, id: EntityId, entry: TreeEntry) -> Result<(), GeoCmdError>;
    /// Replaces the entry and returns the old one.
    fn replace(&mut self, id: &EntityId, entry: TreeEntry) -> Result<TreeEntry, GeoCmdError>;
}

impl ElementTree for BTreeMap<EntityId, TreeEntry> {
    fn get(&self, id: &EntityId) -> Option<&TreeEntry> {
        BTreeMap::get(self, id)
    }

    fn insert(&mut self, id: EntityId, entry: TreeEntry) -> Result<(), GeoCmdError> {
        if self.contains_key(&id) {
            return Err(GeoCmdError::IdCollision(id));
        }
        BTreeMap::insert(self, id, entry);
        Ok(())
    }

    fn replace(&mut self, id: &EntityId, entry: TreeEntry) -> Result<TreeEntry, GeoCmdError> {
        match self.get_mut(id) {
            Some(slot) => Ok(std::mem::replace(slot, entry)),
            None => Err(GeoCmdError::NotFound(*id)),
        }
    }
}

/// The parts of the caller's transaction registration works in.
pub struct GeoTx<'a> {
    pub tree: &'a mut dyn ElementTree,
    pub edges: &'a mut EdgeIndex,
    pub states: &'a mut ElementStates,
    /// The project's recompute mode (CORE.DEP.PROP).
    pub mode: RecomputeMode,
    /// The project namespace UUID of UUIDv5 derivation (CORE.PRJ.CONT.ID).
    pub namespace: Uuid,
    /// The recorded clock and CSPRNG of UUIDv7s (CORE.PRJ.CONT.ID).
    pub source: &'a mut dyn ObservationSource,
}

/// The IDs already in the project: the tree and the dependency graph.
struct Taken<'a> {
    tree: &'a dyn ElementTree,
    edges: &'a EdgeIndex,
}

impl IdIndex for Taken<'_> {
    fn contains_id(&self, id: &Uuid) -> bool {
        self.tree.get(id).is_some() || self.edges.contains(id)
    }
}

/// A registered element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registered {
    pub id: EntityId,
    pub digest: ElementDigest,
    /// The `dep.edges_set@1` event to journal in the same commit.
    pub edges: EdgesSet,
}

/// A replaced element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replaced {
    pub id: EntityId,
    pub digest_before: ElementDigest,
    pub digest_after: ElementDigest,
    /// What CORE.DEP did: the dependents marked `outdated`, the event to journal and the
    /// recompute to submit.
    pub propagation: Propagation,
}

fn id_value(id: &EntityId) -> CValue {
    CValue::Str(id.to_string())
}

/// The canonical UUID text form, or `None` (an uppercase or braced spelling is not
/// canonical, so it stays a plain source ID).
fn canonical_uuid(text: &str) -> Option<Uuid> {
    let id = Uuid::parse(text).ok()?;
    (id.to_string() == text).then_some(id)
}

fn check_source_id(text: &str) -> Result<(), GeoCmdError> {
    if text.is_empty() {
        return Err(GeoCmdError::BadSourceId("empty"));
    }
    if text.len() > MAX_SOURCE_ID_BYTES {
        return Err(GeoCmdError::BadSourceId("longer than 256 bytes"));
    }
    if text.chars().any(char::is_control) {
        return Err(GeoCmdError::BadSourceId("control character"));
    }
    Ok(())
}

/// The entity ID of an element imported with the non-UUID source ID `text`: UUIDv5 of
/// `namespace` over the canonical bytes of `{"source_id": text}`.
pub fn imported_text_id(namespace: Uuid, text: &str) -> Result<EntityId, GeoCmdError> {
    check_source_id(text)?;
    let name = CValue::Obj(BTreeMap::from([("source_id".to_string(), CValue::Str(text.to_string()))]));
    let bytes = fid_canon::to_canonical_bytes(&name).map_err(|_| GeoCmdError::BadSourceId("no canonical form"))?;
    Ok(fid_uuid::uuid_v5(&namespace, &bytes))
}

/// The inputs sorted ascending by ID; a repeated ID is `duplicate_input`.
fn sorted_inputs(inputs: &[(EntityId, Digest)]) -> Result<Vec<(EntityId, Digest)>, GeoCmdError> {
    if inputs.len() > MAX_INPUTS {
        return Err(GeoCmdError::InputLimit("inputs"));
    }
    let mut v = inputs.to_vec();
    v.sort_by_key(|(id, _)| *id);
    for pair in v.windows(2) {
        if let [a, b] = pair {
            if a.0 == b.0 {
                return Err(GeoCmdError::DuplicateInput(a.0));
            }
        }
    }
    Ok(v)
}

fn derive_name(command: &CommandId, inputs: &[(EntityId, Digest)]) -> CValue {
    let items = inputs
        .iter()
        .map(|(id, d)| {
            CValue::Obj(BTreeMap::from([
                ("digest".to_string(), CValue::Str(d.as_str().to_string())),
                ("id".to_string(), id_value(id)),
            ]))
        })
        .collect();
    CValue::Obj(BTreeMap::from([
        ("command".to_string(), CValue::Str(command.as_str().to_string())),
        ("inputs".to_string(), CValue::Arr(items)),
    ]))
}

/// The UUIDv5 of a derived element: `namespace` over the canonical bytes of
/// `{"command": command, "inputs": [{"digest", "id"}, ...]}`, inputs sorted ascending by ID
/// (so the order they are given in does not matter). Refuses a repeated input
/// (`duplicate_input`) and more than `fid_dep::MAX_INPUTS` inputs (`input_limit`).
pub fn derive_id(namespace: Uuid, command: &CommandId, inputs: &[(EntityId, Digest)]) -> Result<EntityId, GeoCmdError> {
    let sorted = sorted_inputs(inputs)?;
    let bytes = fid_canon::to_canonical_bytes(&derive_name(command, &sorted))
        .map_err(|_| GeoCmdError::Internal("derivation name has no canonical form"))?;
    Ok(fid_uuid::uuid_v5(&namespace, &bytes))
}

/// Registers `element` in the tree under the ID `origin` gives (REQ-001, REQ-002).
///
/// Order of checks: facts (`input_limit`); the ID (`bad_source_id`, `duplicate_input`,
/// ID-service errors; a new element draws its UUIDv7 here, and a refusal before this step
/// draws nothing); the ID must not be in the tree or the dependency graph
/// (`id_collision`); every derivation input must be in the dependency graph
/// (`dangling_input`). Then the incoming edges are set, the element is inserted and marked
/// `current`.
pub fn register_element(
    tx: &mut GeoTx<'_>,
    element: GeometryElement,
    origin: IdOrigin,
) -> Result<Registered, GeoCmdError> {
    let facts = ElementFacts::of(&element)?;
    let (id, source_id, inputs) = {
        let taken = Taken { tree: &*tx.tree, edges: &*tx.edges };
        let mut ids = IdService::new(tx.namespace, &mut *tx.source, &taken);
        match origin {
            IdOrigin::Imported(text) => {
                check_source_id(&text)?;
                let id = match canonical_uuid(&text) {
                    Some(u) => ids.import_uuid(u).map_err(|e| match e {
                        fid_project::IdError::IdCollision(u) => GeoCmdError::IdCollision(u),
                        other => GeoCmdError::Id(other),
                    })?,
                    None => imported_text_id(tx.namespace, &text)?,
                };
                (id, Some(text), BTreeSet::new())
            }
            IdOrigin::New => (ids.new_v7().map_err(GeoCmdError::Id)?, None, BTreeSet::new()),
            IdOrigin::Derived { command, inputs } => {
                let id = derive_id(tx.namespace, &command, &inputs)?;
                (id, None, inputs.iter().map(|(i, _)| *i).collect::<BTreeSet<EntityId>>())
            }
        }
    };
    if id == Uuid::NIL || id == Uuid::MAX {
        return Err(GeoCmdError::Id(fid_project::IdError::BadId));
    }
    if tx.tree.get(&id).is_some() || tx.edges.contains(&id) || inputs.contains(&id) {
        return Err(GeoCmdError::IdCollision(id));
    }
    for input in &inputs {
        if !tx.edges.contains(input) {
            return Err(GeoCmdError::Edge(fid_dep::EdgeError::DanglingInput { element: id, input: *input }));
        }
    }
    // A fresh element has no dependents, so its edges cannot close a cycle; with the
    // checks above, applying them cannot fail.
    let set = EdgesSet::from_set(id, &inputs).map_err(GeoCmdError::Edge)?;
    tx.edges.apply(std::slice::from_ref(&set)).map_err(GeoCmdError::Edge)?;
    let digest = facts.digest;
    tx.tree
        .insert(id, TreeEntry::Geometry(Box::new(GeoEntry { element, facts, source_id })))
        .map_err(|_| GeoCmdError::Internal("tree refused an insert of an absent id"))?;
    tx.states.insert(id, ElementState::Current);
    Ok(Registered { id, digest, edges: set })
}

/// Replaces the element `id` with `element`, keeping its ID and source ID, storing the new
/// facts and digest, and running CORE.DEP change propagation in the same transaction
/// (REQ-003).
///
/// Refuses an unknown ID (`not_found`), a non-geometric element (`not_geometry`), a change
/// of kind (`kind_mismatch`), and anything propagation refuses (planned before any change).
pub fn replace_element(tx: &mut GeoTx<'_>, id: &EntityId, element: GeometryElement) -> Result<Replaced, GeoCmdError> {
    let old = match tx.tree.get(id) {
        None => return Err(GeoCmdError::NotFound(*id)),
        Some(TreeEntry::Other { .. }) => return Err(GeoCmdError::NotGeometry(*id)),
        Some(TreeEntry::Geometry(g)) => g,
    };
    if old.facts.kind != element.kind() {
        return Err(GeoCmdError::KindMismatch { expected: old.facts.kind, found: element.kind() });
    }
    let digest_before = old.facts.digest;
    let source_id = old.source_id.clone();
    let facts = ElementFacts::of(&element)?;
    let digest_after = facts.digest;
    let changed = [*id];
    fid_dep::plan(&*tx.edges, &*tx.states, &changed, &[]).map_err(GeoCmdError::Prop)?;
    tx.tree
        .replace(id, TreeEntry::Geometry(Box::new(GeoEntry { element, facts, source_id })))
        .map_err(|_| GeoCmdError::Internal("tree refused a replace of a present id"))?;
    let propagation = fid_dep::propagate(&*tx.edges, tx.states, tx.mode, &changed, &[]).map_err(GeoCmdError::Prop)?;
    Ok(Replaced { id: *id, digest_before, digest_after, propagation })
}
