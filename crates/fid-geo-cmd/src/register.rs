//! Element registration (REQ-GEO.TYPES.REG-001 to -003): putting a geometric element into
//! the CORE element tree under a stable ID, inside the calling command's transaction.
//!
//! IDs (CONSTITUTION §5). Every UUIDv5 comes from CORE's ID service
//! (`IdService::derive_v5`: the project namespace over the RFC 8785 canonical bytes of a
//! name object), and each name carries a domain tag `kind`, so the two GEO name forms can
//! never collide with each other or with another domain's names:
//! - [`IdOrigin::Imported`]: a source ID that `Uuid::parse` accepts (any case) is imported
//!   unchanged as the entity ID, as `IdService::import` does (`id_collision` if taken). CORE
//!   entity IDs are UUIDs, so a source ID in any other form (such as `step:#123`) gets the
//!   entity ID named by `{"kind": "geo_imported_source", "source_id": <text>}`
//!   ([`imported_text_id`]): the same source ID always maps to the same entity. Either way
//!   the source text is kept verbatim as the element's `source_id`.
//! - [`IdOrigin::New`]: a UUIDv7 from CORE's ID service, whose time and random bits are
//!   recorded observations.
//! - [`IdOrigin::Derived`]: the entity ID named by `{"command", "inputs": [{"digest",
//!   "id"}, ...], "kind": "geo_derived", "output", "params"}`, inputs sorted ascending by ID
//!   ([`derive_id`]). `params` is the canonical digest of the producing command's
//!   parameters and `output` names which of its outputs this is, so one command run with
//!   other parameters, or its second output, gets another ID. Every input that is a
//!   geometric element of the tree must be given with its current digest
//!   (`digest_mismatch`). Registering an identical derivation again (same ID, kind,
//!   digest and inputs) is a no-op that returns `already: true`; a different element
//!   under the same derived ID is `id_collision`.
//!
//! Registration records one `dep.edges_set@1` event (from each derivation input; none for
//! other origins) and marks the new element `current`. Replacement keeps the ID, runs
//! CORE.DEP change propagation for the element first (all or nothing), then stores the new
//! element and its facts; the caller journals the `dep.elements_marked@1` event and submits
//! any recompute in the same commit.
//!
//! Every refusal is checked before anything changes, so on error the tree, the edge index
//! and the element states are as they were.

use crate::error::GeoCmdError;
use fid_canon::CValue;
use fid_dep::{EdgeIndex, EdgesSet, ElementState, ElementStates, Propagation, RecomputeMode, MAX_INPUTS};
use fid_geo_types::{digest, Aabb, ElementDigest, ElementKind, FrameRef, GeoTypeError, GeometryElement, TopologyFacts};
use fid_project::{IdIndex, IdService, ObservationSource, ReplaySource};
use fid_types::{CommandId, Digest};
use fid_uuid::Uuid;
use std::collections::{BTreeMap, BTreeSet};

/// A stable entity ID (CONSTITUTION §5); the same type CORE.DEP uses.
pub type EntityId = fid_dep::EntityId;

/// Longest imported source ID, in bytes.
pub const MAX_SOURCE_ID_BYTES: usize = 256;
/// Longest derivation output name, in bytes.
pub const MAX_OUTPUT_BYTES: usize = 64;
/// Largest point or triangle count reported: 2^53, the largest integer a JSON number holds
/// exactly.
pub const MAX_COUNT: u64 = 1 << 53;
/// Domain tag of a derived element's UUIDv5 name.
pub const DERIVED_TAG: &str = "geo_derived";
/// Domain tag of the UUIDv5 name of an element imported with a non-UUID source ID.
pub const IMPORTED_SOURCE_TAG: &str = "geo_imported_source";

/// How a derived element was produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Derivation {
    /// The producing command.
    pub command: CommandId,
    /// The canonical digest of the producing command's parameters.
    pub params: Digest,
    /// Which output of the command this element is: a role name or an index (1 to 64
    /// bytes, no control characters).
    pub output: String,
    /// Inputs: entity ID and content digest, in any order.
    pub inputs: Vec<(EntityId, Digest)>,
}

/// Where a registered element's ID comes from (CONSTITUTION §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdOrigin {
    /// Imported with this source ID, which is kept unchanged.
    Imported(String),
    /// New: a UUIDv7 from CORE.
    New,
    /// Derived by a command from inputs.
    Derived(Derivation),
}

/// How an element's ID was assigned, as stored on the element and reported by describe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OriginKind {
    Imported,
    New,
    Derived,
}

impl OriginKind {
    /// The wire name: `imported`, `new` or `derived`.
    pub fn as_str(self) -> &'static str {
        match self {
            OriginKind::Imported => "imported",
            OriginKind::New => "new",
            OriginKind::Derived => "derived",
        }
    }
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
    /// Whether the box is the one a CAD body handle carries from import (claimed), not one
    /// computed from coordinates.
    pub aabb_claimed: bool,
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
    /// counts above [`MAX_COUNT`] (`input_limit`). Only an element with no points has no
    /// box; any other bounding-box error is returned (`geometry`).
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
        let aabb = match element.aabb() {
            Ok(b) => Some(b),
            Err(GeoTypeError::Empty) => None,
            Err(e) => return Err(GeoCmdError::Geometry(e)),
        };
        Ok(ElementFacts {
            kind: element.kind(),
            points,
            triangles,
            aabb,
            aabb_claimed: matches!(element, GeometryElement::CadBody(_)),
            frame: element.frame(),
            digest: digest(element),
            channels,
            topology,
        })
    }
}

/// A geometric element in the tree, with its cached facts, ID origin and imported source ID.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoEntry {
    pub element: GeometryElement,
    pub facts: ElementFacts,
    pub origin: OriginKind,
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
    /// The `dep.edges_set@1` event to journal in the same commit; `None` when `already`.
    pub edges: Option<EdgesSet>,
    /// The identical derivation was already registered; nothing changed.
    pub already: bool,
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

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn name_obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// The UUIDv5 of `name` in `namespace`, through CORE's ID service (the one place IDs are
/// made). Deriving draws no observation, so an empty replay source stands in.
fn v5(namespace: Uuid, name: &CValue) -> Result<Uuid, GeoCmdError> {
    let mut none = ReplaySource::new(Vec::new());
    let taken: BTreeSet<Uuid> = BTreeSet::new();
    IdService::new(namespace, &mut none, &taken).derive_v5(name).map_err(GeoCmdError::Id)
}

/// Text checks shared by source IDs and output names: non-empty, at most `max` bytes, no
/// control characters.
fn check_label(s: &str, max: usize) -> Result<(), &'static str> {
    if s.is_empty() {
        return Err("empty");
    }
    if s.len() > max {
        return Err("too long");
    }
    if s.chars().any(char::is_control) {
        return Err("control character");
    }
    Ok(())
}

fn check_source_id(text: &str) -> Result<(), GeoCmdError> {
    check_label(text, MAX_SOURCE_ID_BYTES).map_err(GeoCmdError::BadSourceId)
}

/// The entity ID of an element imported with the non-UUID source ID `text`: UUIDv5 of
/// `namespace` over `{"kind": "geo_imported_source", "source_id": text}`.
pub fn imported_text_id(namespace: Uuid, text: &str) -> Result<EntityId, GeoCmdError> {
    check_source_id(text)?;
    v5(namespace, &name_obj([("kind", self::text(IMPORTED_SOURCE_TAG)), ("source_id", self::text(text))]))
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

/// The UUIDv5 name of a derivation, with its inputs sorted (checked) ascending by ID.
fn derive_name(d: &Derivation) -> Result<CValue, GeoCmdError> {
    check_label(&d.output, MAX_OUTPUT_BYTES).map_err(GeoCmdError::BadOutput)?;
    let inputs = sorted_inputs(&d.inputs)?
        .iter()
        .map(|(id, dg)| name_obj([("digest", text(dg.as_str())), ("id", text(&id.to_string()))]))
        .collect();
    Ok(name_obj([
        ("command", text(d.command.as_str())),
        ("inputs", CValue::Arr(inputs)),
        ("kind", text(DERIVED_TAG)),
        ("output", text(&d.output)),
        ("params", text(d.params.as_str())),
    ]))
}

/// The UUIDv5 of a derived element: `namespace` over `{"command", "inputs": [{"digest",
/// "id"}, ...], "kind": "geo_derived", "output", "params"}`, inputs sorted ascending by ID
/// (so the order they are given in does not matter). Refuses a repeated input
/// (`duplicate_input`), more than `fid_dep::MAX_INPUTS` inputs (`input_limit`) and an
/// empty, over-long or control-character output name (`bad_output`).
pub fn derive_id(namespace: Uuid, derivation: &Derivation) -> Result<EntityId, GeoCmdError> {
    v5(namespace, &derive_name(derivation)?)
}

fn current_digest(tree: &dyn ElementTree, id: &EntityId) -> Option<Digest> {
    match tree.get(id) {
        Some(TreeEntry::Geometry(g)) => Some(g.facts.digest.to_digest()),
        _ => None,
    }
}

/// Registers `element` in the tree under the ID `origin` gives (REQ-001, REQ-002).
///
/// Order of checks: facts (`input_limit`, `geometry`); the ID (`bad_source_id`,
/// `bad_output`, `duplicate_input`, `digest_mismatch` for a geometric input given with a
/// stale digest, ID-service errors; a new element draws its UUIDv7 here, and a refusal
/// before this step draws nothing); an identical derivation already registered returns
/// `already: true`; otherwise the ID must not be in the tree or the dependency graph
/// (`id_collision`), and every derivation input must be in the dependency graph
/// (`not_found` / `dangling_input`). Then the incoming edges are set, the element is
/// inserted and marked `current`.
pub fn register_element(
    tx: &mut GeoTx<'_>,
    element: GeometryElement,
    origin: IdOrigin,
) -> Result<Registered, GeoCmdError> {
    let facts = ElementFacts::of(&element)?;
    let (id, kind, source_id, inputs) = {
        let taken = Taken { tree: &*tx.tree, edges: &*tx.edges };
        let mut ids = IdService::new(tx.namespace, &mut *tx.source, &taken);
        match origin {
            IdOrigin::Imported(text) => {
                check_source_id(&text)?;
                let id = match Uuid::parse(&text) {
                    Ok(u) => ids.import_uuid(u).map_err(|e| match e {
                        fid_project::IdError::IdCollision(u) => GeoCmdError::IdCollision(u),
                        other => GeoCmdError::Id(other),
                    })?,
                    Err(_) => imported_text_id(tx.namespace, &text)?,
                };
                (id, OriginKind::Imported, Some(text), BTreeSet::new())
            }
            IdOrigin::New => (ids.new_v7().map_err(GeoCmdError::Id)?, OriginKind::New, None, BTreeSet::new()),
            IdOrigin::Derived(d) => {
                let name = derive_name(&d)?;
                for (input, given) in &d.inputs {
                    if current_digest(&*tx.tree, input).is_some_and(|have| have != *given) {
                        return Err(GeoCmdError::DigestMismatch(*input));
                    }
                }
                let id = ids.derive_v5(&name).map_err(GeoCmdError::Id)?;
                (id, OriginKind::Derived, None, d.inputs.iter().map(|(i, _)| *i).collect::<BTreeSet<EntityId>>())
            }
        }
    };
    if id == Uuid::NIL || id == Uuid::MAX {
        return Err(GeoCmdError::Id(fid_project::IdError::BadId));
    }
    if let Some(TreeEntry::Geometry(g)) = tx.tree.get(&id) {
        let same = kind == OriginKind::Derived
            && g.origin == OriginKind::Derived
            && g.facts.kind == facts.kind
            && g.facts.digest == facts.digest
            && tx.edges.inputs_of(&id).map_or(inputs.is_empty(), |have| *have == inputs);
        if same {
            return Ok(Registered { id, digest: facts.digest, edges: None, already: true });
        }
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
        .insert(id, TreeEntry::Geometry(Box::new(GeoEntry { element, facts, origin: kind, source_id })))
        .map_err(|_| GeoCmdError::Internal("tree refused an insert of an absent id"))?;
    tx.states.insert(id, ElementState::Current);
    Ok(Registered { id, digest, edges: Some(set), already: false })
}

/// Replaces the element `id` with `element`, keeping its ID, origin and source ID, and
/// storing the new facts and digest, with CORE.DEP change propagation in the same
/// transaction (REQ-003).
///
/// `recomputed` lists the dependents this same commit recomputed against the new element
/// (CORE.DEP.PROP); each one that propagation does not find stale is marked `current`.
///
/// Refuses an unknown ID (`not_found`), a non-geometric element (`not_geometry`), a change
/// of kind (`kind_mismatch`) and anything propagation refuses. Propagation runs (all or
/// nothing) before the tree is written, so the new element is never stored while its
/// dependents still read `current`.
pub fn replace_element(
    tx: &mut GeoTx<'_>,
    id: &EntityId,
    element: GeometryElement,
    recomputed: &[EntityId],
) -> Result<Replaced, GeoCmdError> {
    let old = match tx.tree.get(id) {
        None => return Err(GeoCmdError::NotFound(*id)),
        Some(TreeEntry::Other { .. }) => return Err(GeoCmdError::NotGeometry(*id)),
        Some(TreeEntry::Geometry(g)) => g,
    };
    if old.facts.kind != element.kind() {
        return Err(GeoCmdError::KindMismatch { expected: old.facts.kind, found: element.kind() });
    }
    let digest_before = old.facts.digest;
    let (origin, source_id) = (old.origin, old.source_id.clone());
    let facts = ElementFacts::of(&element)?;
    let digest_after = facts.digest;
    let propagation =
        fid_dep::propagate(&*tx.edges, tx.states, tx.mode, &[*id], recomputed).map_err(GeoCmdError::Prop)?;
    for r in recomputed {
        if !propagation.marking.pending().contains(r) {
            tx.states.insert(*r, ElementState::Current);
        }
    }
    // The ID was found above; a refusal here breaks the tree's contract. The dependents
    // are then already `outdated`, which is the safe side.
    tx.tree
        .replace(id, TreeEntry::Geometry(Box::new(GeoEntry { element, facts, origin, source_id })))
        .map_err(|_| GeoCmdError::Internal("tree refused a replace of a present id"))?;
    Ok(Replaced { id: *id, digest_before, digest_after, propagation })
}
