//! View-model projection and dirty-set re-projection (spec leaf `VIZ.RENDER.SCENE.PROJ`).
//!
//! The scene graph is the display-side projection of the document. Each element of the
//! document ([`ElementView`], in tree order inside a [`Document`]) is handed to the
//! [`Projector`] registered for its [`ElementKind`], which emits scene nodes into a
//! [`NodeSink`]: at most one primary node bound to the element itself and at most one derived
//! node per [`ViewOp`] (whisker, label, leader, ...). Render IDs come from the session's
//! [`IdTable`] (`VIZ.RENDER.SCENE.IDTAB`).
//!
//! # Rules pinned here
//!
//! - **Node fields** (REQ-001). A [`SceneNode`] records render ID, identity, kind, frame,
//!   f64 origin and bounds in metres, style token, display status and data digest. A
//!   projector chooses only origin, bounds and style ([`NodeDraw`]); frame and status are
//!   always the element's (status is kernel-supplied, constitution C2: no value of record is
//!   computed here), the kind follows from the identity, and the digest is fixed by the
//!   crate: a primary node shows the element's data digest, a derived node the
//!   [`derived_digest`] of the element's data digest and its view operation.
//! - **Derived digest.** `SHA-256(b"FIDSCN" || 0x01 || source digest (32 bytes) || op code)`.
//!   A derived node changes digest exactly when its source's digest changes (REQ-004).
//! - **Order** (REQ-003, REQ-005). Full projection registers nodes element by element in
//!   tree order and, within an element, in emission order. Re-projection handles the dirty
//!   elements in tree order (dirty entities no longer in the document last, ascending), and
//!   registers new nodes in that order. Change-batch lists are ascending.
//! - **Diff** (REQ-002). Re-projection re-runs the projector of each dirty element only. A
//!   node whose identity is emitted again keeps its render ID; it is listed as updated when
//!   any field differs (floats compared by bit pattern) and is left untouched otherwise. A
//!   node no longer emitted is retired (its render ID becomes a tombstone); a new identity
//!   is added under a fresh render ID. Nodes of clean elements are never visited.
//! - **Derived nodes** (REQ-004, AC-5). There is no way to insert a node except through a
//!   projector, so derived nodes cannot be loaded from storage: they live in memory only and
//!   are regenerated from their source on every projection.
//! - **Missing projector** (REQ-006). The element is skipped (no node; on re-projection its
//!   old nodes are retired) and a [`Diagnostic::NoProjector`] naming the kind is recorded.
//! - **Projector failure.** If the projector returns a [`ProjError`], the element is drawn
//!   as one primary node with style `error`, the element's origin, bounds, digest and
//!   **kernel status unchanged** (so a nonconforming element is never hidden behind the
//!   failure), [`SceneNode::proj_error`] set, and a [`Diagnostic::ProjectorFailed`] is
//!   recorded. It is never dropped silently.
//! - **Status** (C2). [`DisplayStatus`] carries the kdec outcome (including `INDETERMINATE`
//!   and `NOT_DECIDABLE(reason)`) with the claimed, declared-input and
//!   uncharacterized-input flags; it is copied, never mapped, so no fact is lost or upgraded.
//! - **Serialised form** (AC-1, AC-3). One line per node, ascending by render ID:
//!   `<rid> <identity> <kind> <frame> <ox> <oy> <oz> <minx> <miny> <minz> <maxx> <maxy> <maxz>
//!   <style> <status> <proj error> <digest>\n`, status as in [`DisplayStatus`], proj error
//!   `-` or the [`ProjError::code`], identity `e:<uuid>` or `d:<uuid>:<op>`, kind
//!   `element:<kind>` or `derived:<op>`, frame as 32 lowercase hex digits, each f64 as the 16
//!   lowercase hex digits of its IEEE 754 bits. The graph digest is the SHA-256 of that text.
//!
//! # Bounds and failure
//!
//! A document holds at most [`MAX_ELEMENTS`] elements, a registry at most [`MAX_KINDS`]
//! projectors, tokens are 1 to [`MAX_TOKEN_BYTES`] bytes. Element origins must be finite
//! (bounds are a finite [`Aabb`] by construction). Every error is a value; nothing here
//! panics (CONSTITUTION §8). Re-projection plans the whole batch before touching the graph
//! and checks the render-ID budget first, so a failed batch leaves the nodes unchanged (at
//! worst it consumes render IDs that stay unused tombstones). Determinism D0: `BTreeMap`
//! only, no clock, no randomness, no float arithmetic.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fmt::Write as _;

use fid_geo_frame::{FrameId, FrameTree};
use fid_geo_types::Aabb;
use fid_kci::kci::Sha256;
use fid_kdec::Outcome as KernelOutcome;
use fid_types::Digest;

use super::idtab::{DerivedId, EntityId, IdError, IdTable, Identity, RenderId, ViewOp, MAX_IDS_PER_ENTITY};

/// Largest number of elements in one [`Document`] (and in one dirty set).
pub const MAX_ELEMENTS: usize = 1 << 20;
/// Largest number of projectors in one [`ProjectorRegistry`].
pub const MAX_KINDS: usize = 1024;
/// Longest element-kind or style token, in bytes.
pub const MAX_TOKEN_BYTES: usize = 64;
/// Domain tag of the derived-node digest.
pub const DERIVED_TAG: [u8; 6] = *b"FIDSCN";
/// Layout version of the derived-node digest.
pub const DERIVED_VERSION: u8 = 1;
/// The style of the node drawn for an element whose projector failed.
pub const ERROR_STYLE: &str = "error";

/// An axis-aligned f64 box in metres, finite with `min <= max` (GEO.TYPES).
pub type Aabb64 = Aabb;

/// A token is 1 to [`MAX_TOKEN_BYTES`] bytes: a lowercase ASCII letter, then lowercase ASCII
/// letters, digits, `_` or `.`.
fn is_token(s: &str) -> bool {
    let b = s.as_bytes();
    match b.split_first() {
        Some((first, rest)) => {
            b.len() <= MAX_TOKEN_BYTES
                && first.is_ascii_lowercase()
                && rest.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'.')
        }
        None => false,
    }
}

/// The kind of a document element, the key projectors are registered under (`mesh`,
/// `point_set`, `characteristic`, ...). Open-ended, so a document may hold kinds no projector
/// knows (REQ-006).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ElementKind(String);

impl ElementKind {
    /// The kind with this name, or [`SceneError::BadToken`].
    pub fn new(name: &str) -> Result<ElementKind, SceneError> {
        if is_token(name) {
            Ok(ElementKind(name.to_owned()))
        } else {
            Err(SceneError::BadToken)
        }
    }

    /// The kind of a geometry element, under its GEO.TYPES wire name.
    pub fn from_geo(kind: fid_geo_types::ElementKind) -> ElementKind {
        ElementKind(kind.as_str().to_owned())
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ElementKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A style token naming the visual style of a node; resolved to colours by SHADE.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StyleToken(String);

impl StyleToken {
    /// The token with this name, or [`SceneError::BadToken`].
    pub fn new(name: &str) -> Result<StyleToken, SceneError> {
        if is_token(name) {
            Ok(StyleToken(name.to_owned()))
        } else {
            Err(SceneError::BadToken)
        }
    }

    /// The [`ERROR_STYLE`] token.
    pub fn error() -> StyleToken {
        StyleToken(ERROR_STYLE.to_owned())
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StyleToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What the kernel decided for an element, with every fact of its evidence basis that a
/// display must not lose (`fid_kdec::Decision` and `EvidenceBasis`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KernelStatus {
    /// The outcome, including `INDETERMINATE` and `NOT_DECIDABLE(reason)`.
    pub outcome: KernelOutcome,
    /// The value was declared or derived from a specification, not evaluated.
    pub claimed: bool,
    /// The value rests on at least one declared input.
    pub declared_input: bool,
    /// The value rests on at least one uncharacterized input.
    pub uncharacterized_input: bool,
}

/// The display status of a node, supplied by the kernel through the element (C2). The
/// shape is the one pinned by `VIZ.RENDER.SCENE.STALE` minus its `Stale` variant, which that
/// leaf adds. Nothing here upgrades or drops a kernel fact.
///
/// Serialised (pinned): `kernel:<OUTCOME>:<flags>` where `<OUTCOME>` is the kdec wire name and
/// `NOT_DECIDABLE` is written `NOT_DECIDABLE(<reason>)`, and `<flags>` is three characters,
/// `c` (claimed), `d` (declared input), `u` (uncharacterized input), each `-` when unset;
/// `not_evaluated`; `error:<code>` with a token code.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DisplayStatus {
    /// A kernel decision.
    Kernel(KernelStatus),
    /// Nothing evaluated (plain geometry, or no decision yet).
    NotEvaluated,
    /// Evaluation failed in the kernel, with its error code.
    Error(StatusCode),
}

/// A kernel error code carried by [`DisplayStatus::Error`]: a token (`[a-z][a-z0-9_.]*`, at
/// most [`MAX_TOKEN_BYTES`] bytes).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StatusCode(String);

impl StatusCode {
    /// The code with this name, or [`SceneError::BadToken`].
    pub fn new(name: &str) -> Result<StatusCode, SceneError> {
        if is_token(name) {
            Ok(StatusCode(name.to_owned()))
        } else {
            Err(SceneError::BadToken)
        }
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Longest serialised [`DisplayStatus`] accepted by [`DisplayStatus::parse`].
pub const MAX_STATUS_BYTES: usize = 128;

impl DisplayStatus {
    /// Parses the serialised form (case-sensitive). Fails with [`SceneError::BadStatus`].
    pub fn parse(s: &str) -> Result<DisplayStatus, SceneError> {
        if s.len() > MAX_STATUS_BYTES {
            return Err(SceneError::BadStatus);
        }
        if s == "not_evaluated" {
            return Ok(DisplayStatus::NotEvaluated);
        }
        if let Some(code) = s.strip_prefix("error:") {
            return StatusCode::new(code).map(DisplayStatus::Error).map_err(|_| SceneError::BadStatus);
        }
        let rest = s.strip_prefix("kernel:").ok_or(SceneError::BadStatus)?;
        let (outcome, flags) = rest.rsplit_once(':').ok_or(SceneError::BadStatus)?;
        let outcome = match outcome.strip_prefix("NOT_DECIDABLE(") {
            Some(r) => {
                let reason = r.strip_suffix(')').ok_or(SceneError::BadStatus)?;
                KernelOutcome::from_wire("NOT_DECIDABLE", Some(reason))
            }
            None => KernelOutcome::from_wire(outcome, None),
        }
        .map_err(|_| SceneError::BadStatus)?;
        let flag = |i: usize, set: u8| match flags.as_bytes().get(i) {
            Some(&c) if c == set => Ok(true),
            Some(b'-') => Ok(false),
            _ => Err(SceneError::BadStatus),
        };
        if flags.len() != 3 {
            return Err(SceneError::BadStatus);
        }
        Ok(DisplayStatus::Kernel(KernelStatus {
            outcome,
            claimed: flag(0, b'c')?,
            declared_input: flag(1, b'd')?,
            uncharacterized_input: flag(2, b'u')?,
        }))
    }
}

impl fmt::Display for DisplayStatus {
    /// The serialised form.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DisplayStatus::Kernel(k) => {
                f.write_str("kernel:")?;
                match k.outcome.reason() {
                    Some(r) => write!(f, "{}({})", k.outcome.wire(), r.wire())?,
                    None => f.write_str(k.outcome.wire())?,
                }
                let c = |on: bool, ch: char| if on { ch } else { '-' };
                write!(f, ":{}{}{}", c(k.claimed, 'c'), c(k.declared_input, 'd'), c(k.uncharacterized_input, 'u'))
            }
            DisplayStatus::NotEvaluated => f.write_str("not_evaluated"),
            DisplayStatus::Error(code) => write!(f, "error:{}", code.as_str()),
        }
    }
}

/// What a node draws: an element as itself, or a view operation of it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NodeKind {
    /// The element drawn as itself.
    Element(ElementKind),
    /// A derived drawable (whisker, label, leader, ...).
    Derived(ViewOp),
}

impl fmt::Display for NodeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeKind::Element(k) => write!(f, "element:{k}"),
            NodeKind::Derived(op) => write!(f, "derived:{}", op.name()),
        }
    }
}

/// One scene node (REQ-001).
#[derive(Debug, Clone, PartialEq)]
pub struct SceneNode {
    /// Session-local render ID.
    pub id: RenderId,
    /// The entity or derived identity it is bound to.
    pub identity: Identity,
    /// What it draws.
    pub kind: NodeKind,
    /// The frame its origin and bounds are expressed in (GEO.FRAME).
    pub frame: FrameId,
    /// Origin in metres.
    pub origin_m: [f64; 3],
    /// Bounds in metres.
    pub bounds_m: Aabb64,
    /// Style token.
    pub style: StyleToken,
    /// Kernel-supplied display status.
    pub status: DisplayStatus,
    /// Why the projector failed, for the error node drawn in its place; `None` otherwise.
    pub proj_error: Option<ProjError>,
    /// SHA-256 digest of the data the node displays.
    pub data_digest: Digest,
}

impl SceneNode {
    /// Whether the node is derived (in memory only, never persisted).
    pub const fn is_derived(&self) -> bool {
        matches!(self.identity, Identity::Derived(_))
    }

    /// Equal field by field, floats by bit pattern ("byte-identical").
    fn same_as(&self, p: &Planned) -> bool {
        self.identity == p.identity
            && self.kind == p.kind
            && self.frame == p.frame
            && bits3(self.origin_m) == bits3(p.origin_m)
            && bits3(self.bounds_m.min()) == bits3(p.bounds_m.min())
            && bits3(self.bounds_m.max()) == bits3(p.bounds_m.max())
            && self.style == p.style
            && self.status == p.status
            && self.proj_error == p.proj_error
            && self.data_digest == p.data_digest
    }
}

fn bits3(v: [f64; 3]) -> [u64; 3] {
    [v[0].to_bits(), v[1].to_bits(), v[2].to_bits()]
}

impl fmt::Display for SceneNode {
    /// The serialised line, without the newline.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ", self.id)?;
        match self.identity {
            Identity::Entity(e) => write!(f, "e:{e}")?,
            Identity::Derived(d) => write!(f, "d:{}:{}", d.source, d.op.name())?,
        }
        write!(f, " {} {}", self.kind, self.frame)?;
        for v in self.origin_m.iter().chain(self.bounds_m.min().iter()).chain(self.bounds_m.max().iter()) {
            write!(f, " {:016x}", v.to_bits())?;
        }
        write!(f, " {} {} ", self.style, self.status)?;
        match self.proj_error {
            Some(e) => f.write_str(e.code())?,
            None => f.write_str("-")?,
        }
        write!(f, " {}", self.data_digest.as_str())
    }
}

/// The digest a derived node displays: `SHA-256(b"FIDSCN" || 0x01 || source || op code)`.
pub fn derived_digest(source: &Digest, op: ViewOp) -> Digest {
    let mut h = Sha256::new();
    h.update(&DERIVED_TAG);
    h.update(&[DERIVED_VERSION]);
    h.update(&source.to_bytes());
    h.update(&[op.code()]);
    Digest::from_bytes(h.finish())
}

/// The document state of one element, as projectors see it.
#[derive(Debug, Clone, PartialEq)]
pub struct ElementView {
    /// The element's entity ID.
    pub entity: EntityId,
    /// Its kind; selects the projector.
    pub kind: ElementKind,
    /// The frame its geometry is expressed in.
    pub frame: FrameId,
    /// Origin in metres; must be finite.
    pub origin_m: [f64; 3],
    /// Bounds in metres.
    pub bounds_m: Aabb64,
    /// Kernel-supplied status.
    pub status: DisplayStatus,
    /// Digest of the element's data (GEO.TYPES.DIGEST for geometry).
    pub data_digest: Digest,
}

/// The document state to project: elements in tree order (CORE.PRJ.TREE walk), with an
/// entity index.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Document {
    elements: Vec<ElementView>,
    index: BTreeMap<EntityId, usize>,
}

impl Document {
    /// A document from elements in tree order. Fails with [`SceneError::TooManyElements`],
    /// [`SceneError::DuplicateEntity`] or [`SceneError::NonFiniteOrigin`].
    pub fn new(elements: Vec<ElementView>) -> Result<Document, SceneError> {
        if elements.len() > MAX_ELEMENTS {
            return Err(SceneError::TooManyElements);
        }
        let mut index = BTreeMap::new();
        for (i, el) in elements.iter().enumerate() {
            if !el.origin_m.iter().all(|v| v.is_finite()) {
                return Err(SceneError::NonFiniteOrigin(el.entity));
            }
            if index.insert(el.entity, i).is_some() {
                return Err(SceneError::DuplicateEntity(el.entity));
            }
        }
        Ok(Document { elements, index })
    }

    /// The elements in tree order.
    pub fn elements(&self) -> &[ElementView] {
        &self.elements
    }

    /// The element with this entity ID.
    pub fn get(&self, entity: EntityId) -> Option<&ElementView> {
        self.index.get(&entity).and_then(|&i| self.elements.get(i))
    }

    /// Checks that every element's frame exists in `tree`, else
    /// [`SceneError::UnknownFrame`] for the first one (tree order) that does not.
    pub fn check_frames(&self, tree: &FrameTree) -> Result<(), SceneError> {
        match self.elements.iter().find(|el| tree.get(el.frame).is_none()) {
            Some(el) => Err(SceneError::UnknownFrame { entity: el.entity, frame: el.frame }),
            None => Ok(()),
        }
    }
}

/// What a projector chooses for one node.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeDraw {
    /// Origin in metres; must be finite.
    pub origin_m: [f64; 3],
    /// Bounds in metres.
    pub bounds_m: Aabb64,
    /// Style token.
    pub style: StyleToken,
}

/// A node planned by a projector, before it has a render ID.
#[derive(Debug, Clone, PartialEq)]
struct Planned {
    identity: Identity,
    kind: NodeKind,
    frame: FrameId,
    origin_m: [f64; 3],
    bounds_m: Aabb64,
    style: StyleToken,
    status: DisplayStatus,
    proj_error: Option<ProjError>,
    data_digest: Digest,
}

impl Planned {
    fn into_node(self, id: RenderId) -> SceneNode {
        SceneNode {
            id,
            identity: self.identity,
            kind: self.kind,
            frame: self.frame,
            origin_m: self.origin_m,
            bounds_m: self.bounds_m,
            style: self.style,
            status: self.status,
            proj_error: self.proj_error,
            data_digest: self.data_digest,
        }
    }
}

/// Where a projector emits the nodes of one element: one primary node and one derived node
/// per view operation at most, so never more than [`MAX_IDS_PER_ENTITY`].
#[derive(Debug)]
pub struct NodeSink<'a> {
    el: &'a ElementView,
    nodes: Vec<Planned>,
}

impl<'a> NodeSink<'a> {
    fn new(el: &'a ElementView) -> Result<NodeSink<'a>, SceneError> {
        let mut nodes = Vec::new();
        nodes.try_reserve_exact(MAX_IDS_PER_ENTITY).map_err(|_| SceneError::AllocationFailed)?;
        Ok(NodeSink { el, nodes })
    }

    /// The element being projected.
    pub fn element(&self) -> &ElementView {
        self.el
    }

    /// Emits the element's own node. Fails with [`ProjError::NonFiniteOrigin`] or
    /// [`ProjError::DuplicatePrimary`].
    pub fn primary(&mut self, draw: NodeDraw) -> Result<(), ProjError> {
        let identity = Identity::Entity(self.el.entity);
        if self.nodes.iter().any(|n| n.identity == identity) {
            return Err(ProjError::DuplicatePrimary);
        }
        self.push(identity, NodeKind::Element(self.el.kind.clone()), self.el.data_digest, draw)
    }

    /// Emits a derived node. Fails with [`ProjError::NonFiniteOrigin`] or
    /// [`ProjError::DuplicateDerived`].
    pub fn derived(&mut self, op: ViewOp, draw: NodeDraw) -> Result<(), ProjError> {
        let identity = Identity::Derived(DerivedId { source: self.el.entity, op });
        if self.nodes.iter().any(|n| n.identity == identity) {
            return Err(ProjError::DuplicateDerived(op));
        }
        self.push(identity, NodeKind::Derived(op), derived_digest(&self.el.data_digest, op), draw)
    }

    fn push(&mut self, identity: Identity, kind: NodeKind, data_digest: Digest, d: NodeDraw) -> Result<(), ProjError> {
        if !d.origin_m.iter().all(|v| v.is_finite()) {
            return Err(ProjError::NonFiniteOrigin);
        }
        // Identities are unique per element, so at most MAX_IDS_PER_ENTITY nodes arrive and
        // the reserved capacity is never exceeded; guard it anyway (fail closed).
        if self.nodes.len() >= MAX_IDS_PER_ENTITY {
            return Err(ProjError::TooManyNodes);
        }
        self.nodes.push(Planned {
            identity,
            kind,
            frame: self.el.frame,
            origin_m: d.origin_m,
            bounds_m: d.bounds_m,
            style: d.style,
            status: self.el.status.clone(),
            proj_error: None,
            data_digest,
        });
        Ok(())
    }
}

/// Projects elements of one kind into scene nodes.
pub trait Projector {
    /// The element kind this projector handles.
    fn kind(&self) -> ElementKind;
    /// Emits the nodes of `el` into `out`.
    fn project(&self, el: &ElementView, out: &mut NodeSink<'_>) -> Result<(), ProjError>;
}

/// The projectors, one per element kind.
#[derive(Default)]
pub struct ProjectorRegistry {
    by_kind: BTreeMap<ElementKind, Box<dyn Projector>>,
}

impl fmt::Debug for ProjectorRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.by_kind.keys()).finish()
    }
}

impl ProjectorRegistry {
    /// An empty registry.
    pub fn new() -> ProjectorRegistry {
        ProjectorRegistry::default()
    }

    /// Registers a projector under its kind. Fails with [`SceneError::DuplicateProjector`]
    /// or [`SceneError::TooManyKinds`].
    pub fn register(&mut self, p: Box<dyn Projector>) -> Result<(), SceneError> {
        let kind = p.kind();
        if self.by_kind.contains_key(&kind) {
            return Err(SceneError::DuplicateProjector(kind));
        }
        if self.by_kind.len() >= MAX_KINDS {
            return Err(SceneError::TooManyKinds);
        }
        self.by_kind.insert(kind, p);
        Ok(())
    }

    /// The registered kinds, ascending.
    pub fn kinds(&self) -> impl Iterator<Item = &ElementKind> {
        self.by_kind.keys()
    }
}

/// Why a projector could not project one element. The element is then drawn as an error
/// node; projection continues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjError {
    /// A node origin was NaN or infinite.
    NonFiniteOrigin,
    /// The primary node was emitted twice.
    DuplicatePrimary,
    /// A derived node of this operation was emitted twice.
    DuplicateDerived(ViewOp),
    /// More nodes than one element may have (guards the per-entity bound).
    TooManyNodes,
    /// The projector's own failure.
    Failed {
        /// A short stable reason.
        reason: &'static str,
    },
}

impl ProjError {
    /// The stable error code.
    pub const fn code(&self) -> &'static str {
        match self {
            ProjError::NonFiniteOrigin => "NonFiniteOrigin",
            ProjError::DuplicatePrimary => "DuplicatePrimary",
            ProjError::DuplicateDerived(_) => "DuplicateDerived",
            ProjError::TooManyNodes => "TooManyNodes",
            ProjError::Failed { .. } => "ProjectorFailed",
        }
    }
}

impl fmt::Display for ProjError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjError::DuplicateDerived(op) => write!(f, "{}: {}", self.code(), op.name()),
            ProjError::Failed { reason } => write!(f, "{}: {reason}", self.code()),
            _ => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for ProjError {}

/// A non-fatal finding of a projection run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diagnostic {
    /// No projector is registered for the element's kind; it was skipped (REQ-006).
    NoProjector {
        /// The element.
        entity: EntityId,
        /// Its kind.
        kind: ElementKind,
    },
    /// The projector failed; the element is drawn as an error node.
    ProjectorFailed {
        /// The element.
        entity: EntityId,
        /// Its kind.
        kind: ElementKind,
        /// The failure.
        error: ProjError,
    },
}

impl Diagnostic {
    /// The stable diagnostic code.
    pub const fn code(&self) -> &'static str {
        match self {
            Diagnostic::NoProjector { .. } => "NoProjector",
            Diagnostic::ProjectorFailed { .. } => "ProjectorFailed",
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Diagnostic::NoProjector { entity, kind } => write!(f, "NoProjector: kind {kind} (entity {entity})"),
            Diagnostic::ProjectorFailed { entity, kind, error } => {
                write!(f, "ProjectorFailed: kind {kind} (entity {entity}): {error}")
            }
        }
    }
}

/// A fatal error: the graph is left unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneError {
    /// A kind or style token is not 1..=64 bytes of `[a-z][a-z0-9_.]*`.
    BadToken,
    /// A serialised display status does not parse.
    BadStatus,
    /// The document or dirty set has more than [`MAX_ELEMENTS`] entries.
    TooManyElements,
    /// Two elements share an entity ID.
    DuplicateEntity(EntityId),
    /// An element origin is NaN or infinite.
    NonFiniteOrigin(EntityId),
    /// An element names a frame the frame tree does not hold.
    UnknownFrame {
        /// The element.
        entity: EntityId,
        /// Its frame.
        frame: FrameId,
    },
    /// A projector for this kind is already registered.
    DuplicateProjector(ElementKind),
    /// The registry holds [`MAX_KINDS`] projectors.
    TooManyKinds,
    /// The render-ID table refused (for example `IdSpaceExhausted`).
    Id(IdError),
    /// Memory could not be reserved.
    AllocationFailed,
}

impl SceneError {
    /// The stable error code.
    pub const fn code(&self) -> &'static str {
        match self {
            SceneError::BadToken => "BadToken",
            SceneError::BadStatus => "BadStatus",
            SceneError::TooManyElements => "TooManyElements",
            SceneError::DuplicateEntity(_) => "DuplicateEntity",
            SceneError::NonFiniteOrigin(_) => "NonFiniteOrigin",
            SceneError::UnknownFrame { .. } => "UnknownFrame",
            SceneError::DuplicateProjector(_) => "DuplicateProjector",
            SceneError::TooManyKinds => "TooManyKinds",
            SceneError::Id(e) => e.code(),
            SceneError::AllocationFailed => "AllocationFailed",
        }
    }
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SceneError::DuplicateEntity(e) | SceneError::NonFiniteOrigin(e) => write!(f, "{}: {e}", self.code()),
            SceneError::UnknownFrame { entity, frame } => write!(f, "{}: {entity} names {frame}", self.code()),
            SceneError::DuplicateProjector(k) => write!(f, "{}: {k}", self.code()),
            SceneError::Id(e) => write!(f, "{e}"),
            _ => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for SceneError {}

impl From<IdError> for SceneError {
    fn from(e: IdError) -> SceneError {
        SceneError::Id(e)
    }
}

/// The render IDs one projection run added, updated and retired, each ascending (REQ-003).
/// FRAME, LOD, SHADE and GLYPH consume it as the scene's change stream.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChangeBatch {
    /// Newly registered nodes.
    pub added: Vec<RenderId>,
    /// Nodes that kept their render ID but changed.
    pub updated: Vec<RenderId>,
    /// Nodes whose render ID is now a tombstone.
    pub retired: Vec<RenderId>,
}

impl ChangeBatch {
    /// Whether nothing changed.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.updated.is_empty() && self.retired.is_empty()
    }
}

/// The result of a projection run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outcome {
    /// What changed.
    pub batch: ChangeBatch,
    /// Skipped and failed elements, in processing order.
    pub diagnostics: Vec<Diagnostic>,
}

/// The scene graph of one session: nodes ordered by render ID, plus the session's ID table.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SceneGraph {
    ids: IdTable,
    nodes: BTreeMap<RenderId, SceneNode>,
}

impl SceneGraph {
    /// An empty graph whose ID table may allocate up to `RenderId::MAX` IDs.
    pub fn new() -> SceneGraph {
        SceneGraph::default()
    }

    /// An empty graph whose ID table allocates at most `limit` IDs.
    pub fn with_id_limit(limit: u32) -> SceneGraph {
        SceneGraph { ids: IdTable::with_limit(limit), nodes: BTreeMap::new() }
    }

    /// Full projection of `doc` into a new graph (tree order).
    pub fn project(registry: &ProjectorRegistry, doc: &Document) -> Result<(SceneGraph, Outcome), SceneError> {
        let mut g = SceneGraph::new();
        let out = g.project_into(registry, doc)?;
        Ok((g, out))
    }

    /// Projects every element of `doc` into this graph in tree order, as if all were dirty.
    /// Nodes of entities that are not in `doc` are left alone, so call it on an empty graph
    /// (or use [`SceneGraph::reproject`] with their IDs to retire them).
    pub fn project_into(&mut self, registry: &ProjectorRegistry, doc: &Document) -> Result<Outcome, SceneError> {
        let mut order: Vec<(EntityId, Option<&ElementView>)> = Vec::new();
        order.try_reserve_exact(doc.elements.len()).map_err(|_| SceneError::AllocationFailed)?;
        order.extend(doc.elements.iter().map(|e| (e.entity, Some(e))));
        self.apply(registry, &order)
    }

    /// Re-projects the elements of a dirty set (`CORE.DEP.PROP`) against the new document
    /// state; every other node stays byte-identical (REQ-002). Duplicate entries count
    /// once; entities neither in the document nor in the graph are ignored.
    pub fn reproject(
        &mut self,
        registry: &ProjectorRegistry,
        doc: &Document,
        dirty: &[EntityId],
    ) -> Result<Outcome, SceneError> {
        if dirty.len() > MAX_ELEMENTS {
            return Err(SceneError::TooManyElements);
        }
        let set: BTreeSet<EntityId> = dirty.iter().copied().collect();
        let mut present: Vec<(usize, EntityId)> = Vec::new();
        let mut gone: Vec<EntityId> = Vec::new();
        present.try_reserve_exact(set.len()).map_err(|_| SceneError::AllocationFailed)?;
        for e in set {
            match doc.index.get(&e) {
                Some(&pos) => present.push((pos, e)),
                None if !self.ids.render_ids(e).is_empty() => {
                    gone.try_reserve(1).map_err(|_| SceneError::AllocationFailed)?;
                    gone.push(e);
                }
                None => {}
            }
        }
        present.sort_unstable();
        let mut order: Vec<(EntityId, Option<&ElementView>)> = Vec::new();
        order.try_reserve_exact(present.len() + gone.len()).map_err(|_| SceneError::AllocationFailed)?;
        order.extend(present.iter().map(|&(_, e)| (e, doc.get(e))));
        order.extend(gone.iter().map(|&e| (e, None)));
        self.apply(registry, &order)
    }

    /// Plans every listed element, then applies the plan.
    fn apply(
        &mut self,
        registry: &ProjectorRegistry,
        order: &[(EntityId, Option<&ElementView>)],
    ) -> Result<Outcome, SceneError> {
        // 1. Plan (no mutation).
        let mut diagnostics = Vec::new();
        let mut plans: Vec<(EntityId, Vec<Planned>)> = Vec::new();
        plans.try_reserve_exact(order.len()).map_err(|_| SceneError::AllocationFailed)?;
        for &(entity, el) in order {
            let planned = match el {
                Some(el) => plan_element(registry, el, &mut diagnostics)?,
                None => Vec::new(),
            };
            plans.push((entity, planned));
        }

        // 2. Classify against the current graph.
        let mut batch = ChangeBatch::default();
        let mut to_add: Vec<Planned> = Vec::new();
        let mut to_update: Vec<(RenderId, Planned)> = Vec::new();
        for (entity, planned) in plans {
            let old: Vec<RenderId> = self.ids.render_ids(entity).to_vec();
            for &rid in &old {
                let keep = match self.nodes.get(&rid) {
                    Some(node) => planned.iter().any(|p| p.identity == node.identity),
                    None => false,
                };
                if !keep {
                    batch.retired.try_reserve(1).map_err(|_| SceneError::AllocationFailed)?;
                    batch.retired.push(rid);
                }
            }
            for p in planned {
                match self.ids.render_id_of(p.identity) {
                    Some(rid) => {
                        if !self.nodes.get(&rid).is_some_and(|n| n.same_as(&p)) {
                            to_update.try_reserve(1).map_err(|_| SceneError::AllocationFailed)?;
                            to_update.push((rid, p));
                        }
                    }
                    None => {
                        to_add.try_reserve(1).map_err(|_| SceneError::AllocationFailed)?;
                        to_add.push(p);
                    }
                }
            }
        }
        let budget = u64::from(self.ids.limit()).saturating_sub(u64::from(self.ids.allocated()));
        if u64::try_from(to_add.len()).unwrap_or(u64::MAX) > budget {
            return Err(SceneError::Id(IdError::IdSpaceExhausted));
        }
        batch.added.try_reserve_exact(to_add.len()).map_err(|_| SceneError::AllocationFailed)?;
        batch.updated.try_reserve_exact(to_update.len()).map_err(|_| SceneError::AllocationFailed)?;

        // 3. Register new identities first; on failure undo them so no node changes.
        for p in &to_add {
            match self.ids.register(Some(p.identity)) {
                Ok(rid) => batch.added.push(rid),
                Err(e) => {
                    for &rid in &batch.added {
                        // Just registered and live, so retiring cannot fail.
                        let _ = self.ids.retire(rid);
                    }
                    return Err(SceneError::Id(e));
                }
            }
        }

        // 4. Commit: nothing below can fail.
        for &rid in &batch.retired {
            let _ = self.ids.retire(rid);
            self.nodes.remove(&rid);
        }
        for (p, &rid) in to_add.into_iter().zip(batch.added.iter()) {
            self.nodes.insert(rid, p.into_node(rid));
        }
        for (rid, p) in to_update {
            batch.updated.push(rid);
            self.nodes.insert(rid, p.into_node(rid));
        }
        batch.added.sort_unstable();
        batch.updated.sort_unstable();
        batch.retired.sort_unstable();
        Ok(Outcome { batch, diagnostics })
    }

    /// The node with this render ID.
    pub fn get(&self, id: RenderId) -> Option<&SceneNode> {
        self.nodes.get(&id)
    }

    /// The nodes, ascending by render ID.
    pub fn nodes(&self) -> impl Iterator<Item = &SceneNode> {
        self.nodes.values()
    }

    /// Number of nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the graph has no node.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The session's render-ID table (both lookups, for VIZ.PICK).
    pub fn ids(&self) -> &IdTable {
        &self.ids
    }

    /// The live render IDs bound to an entity or derived from it, ascending.
    pub fn render_ids(&self, entity: EntityId) -> &[RenderId] {
        self.ids.render_ids(entity)
    }

    /// The serialised node list (module docs), one line per node.
    pub fn serialise(&self) -> Result<String, SceneError> {
        let mut out = String::new();
        let mut line = String::new();
        for node in self.nodes.values() {
            line.clear();
            write_line(&mut line, node)?;
            out.try_reserve(line.len()).map_err(|_| SceneError::AllocationFailed)?;
            out.push_str(&line);
        }
        Ok(out)
    }

    /// The SHA-256 digest of [`SceneGraph::serialise`], computed without building the text.
    pub fn digest(&self) -> Result<Digest, SceneError> {
        let mut h = Sha256::new();
        let mut line = String::new();
        for node in self.nodes.values() {
            line.clear();
            write_line(&mut line, node)?;
            h.update(line.as_bytes());
        }
        Ok(Digest::from_bytes(h.finish()))
    }
}

/// Writes one node line with its newline. A line is under 1 KiB (tokens are bounded).
fn write_line(line: &mut String, node: &SceneNode) -> Result<(), SceneError> {
    line.try_reserve(512).map_err(|_| SceneError::AllocationFailed)?;
    writeln!(line, "{node}").map_err(|_| SceneError::AllocationFailed)
}

/// Runs the projector of one element; a missing projector or a failure becomes a diagnostic.
fn plan_element(
    registry: &ProjectorRegistry,
    el: &ElementView,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<Planned>, SceneError> {
    let Some(projector) = registry.by_kind.get(&el.kind) else {
        diagnostics.try_reserve(1).map_err(|_| SceneError::AllocationFailed)?;
        diagnostics.push(Diagnostic::NoProjector { entity: el.entity, kind: el.kind.clone() });
        return Ok(Vec::new());
    };
    let mut sink = NodeSink::new(el)?;
    match projector.project(el, &mut sink) {
        Ok(()) => Ok(sink.nodes),
        Err(error) => {
            diagnostics.try_reserve(1).map_err(|_| SceneError::AllocationFailed)?;
            diagnostics.push(Diagnostic::ProjectorFailed { entity: el.entity, kind: el.kind.clone(), error });
            let mut nodes = Vec::new();
            nodes.try_reserve_exact(1).map_err(|_| SceneError::AllocationFailed)?;
            nodes.push(Planned {
                identity: Identity::Entity(el.entity),
                kind: NodeKind::Element(el.kind.clone()),
                frame: el.frame,
                origin_m: el.origin_m,
                bounds_m: el.bounds_m,
                style: StyleToken::error(),
                status: el.status.clone(),
                proj_error: Some(error),
                data_digest: el.data_digest,
            });
            Ok(nodes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens() {
        assert!(is_token("geom.point_set"));
        assert!(is_token("a"));
        assert!(!is_token(""));
        assert!(!is_token("Mesh"));
        assert!(!is_token("1mesh"));
        assert!(!is_token("mesh label"));
        assert!(is_token(&"a".repeat(MAX_TOKEN_BYTES)));
        assert!(!is_token(&"a".repeat(MAX_TOKEN_BYTES + 1)));
    }

    #[test]
    fn status_text_is_strict() {
        for bad in [
            "",
            "none",
            "kernel:CONFORMS",
            "kernel:CONFORMS:---x",
            "kernel:CONFORMS:dcu",
            "kernel:conforms:---",
            "kernel:NOT_DECIDABLE:---",
            "kernel:NOT_DECIDABLE(nope):---",
            "kernel:INDETERMINATE(u_missing):---",
            "error:",
            "error:Bad",
            "Not_evaluated",
        ] {
            assert_eq!(DisplayStatus::parse(bad), Err(SceneError::BadStatus), "{bad}");
        }
        assert_eq!(DisplayStatus::parse(&"x".repeat(MAX_STATUS_BYTES + 1)), Err(SceneError::BadStatus));
    }
}
