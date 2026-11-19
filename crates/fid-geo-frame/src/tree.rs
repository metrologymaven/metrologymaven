//! The per-document frame tree and chain resolution. Spec node `GEO.FRAME.TREE`.
//!
//! One world frame is the root; every other frame has exactly one parent and one transform
//! to it (`to_parent` maps coordinates in the frame to coordinates in its parent). Frames
//! are stored in a `BTreeMap` keyed by [`FrameId`], so iteration order and the
//! [`FrameTree::digest`] do not depend on the platform (REQ-GEO.FRAME.TREE-005).
//!
//! Every edit validates completely before it changes anything, so a failed edit leaves the
//! tree (and its digest) exactly as it was (REQ-GEO.FRAME.TREE-002, AC-2). Every walk over
//! the tree is an explicit loop bounded by [`MAX_DEPTH`] or by the frame count; nothing
//! recurses, nothing panics.
//!
//! # Invariants
//!
//! - exactly one frame of kind world, with no parent; it cannot be reparented, re-transformed
//!   or removed (`WorldImmutable`), and no second world frame can be added (`SecondWorld`);
//! - every other frame has a parent that is in the tree (`NoParent`, `UnknownFrame`);
//! - no frame is its own ancestor (`Cycle { frames }`);
//! - IDs are unique (`DuplicateId`); [`FrameId::parse`] accepts upper- and lower-case hex and
//!   maps both to the same bytes, so a case-only variant of an ID is the same ID;
//! - names are 1 to [`MAX_NAME_BYTES`] bytes of UTF-8 without control characters and unique
//!   after ASCII lower-casing (`BadName`, `DuplicateName`);
//! - every `to_parent` passes the XFORM checks again on entry: rotation orthonormal within
//!   1e-12 (`BadTransform`) and proper, determinant +1 (`Handedness`: a reflection would
//!   join a left-handed frame to a right-handed parent);
//! - depth (edges from the world frame) at most [`MAX_DEPTH`] (`TooDeep`), frame count at
//!   most [`MAX_FRAMES`] (`TooManyFrames`).
//!
//! # Pinned resolution order (REQ-GEO.FRAME.TREE-003)
//!
//! `ancestors(f)` is the child-first list `[f, parent(f), …, world]`. For `resolve(a, b)`
//! with `a ≠ b`, the lowest common ancestor `c` is the last common entry of the two
//! root-first lists; `A` = the frames of `ancestors(a)` strictly below `c`, `B` likewise
//! for `b`. A chain is a left fold from the frame upward: `acc = T(f0)`, then
//! `acc = T(g).compose(acc)` for each next `g` (`T(g)` its `to_parent`), giving
//! `T_{c←a} = T_{c←p} ∘ … ∘ T_{p'←a}`. An empty chain is absent (never an identity that
//! would be multiplied in, which could change the sign of a zero). Then:
//!
//! - `a = b`: the exact identity [`Transform::IDENTITY`] (REQ-GEO.FRAME.TREE-006);
//! - only `A` non-empty (b is an ancestor of a): `T_{c←a}`;
//! - only `B` non-empty (a is an ancestor of b): `inverse(T_{c←b})`;
//! - both: `inverse(T_{c←b}).compose(T_{c←a})`.
//!
//! Compose and inverse are XFORM's pinned expressions; their failures come back as
//! `Xform(..)`.
//!
//! # Path round-trip bound
//!
//! XFORM's REQ-004 bounds one edge: forward and back within `ρ|x| + 48ε(|x| + |t|)`,
//! `ε = 2⁻⁵³`. For a path of `k` rigid edges between `a` and `b` (the edges of `A` and `B`),
//! with residuals `ρ_i` and translations `t_i`, [`FrameTree::path_round_trip_bound`] returns
//!
//! ```text
//! |resolve(b, a)(resolve(a, b)(x)) − x|  ≤  (Σ ρ_i)·M + 48ε·(2k + 4)·M,   M = |x| + Σ |t_i|
//! ```
//!
//! Derivation (first order in ρ and ε). Both resolves reuse the same computed chains
//! `Ã`, `B̃` (same fold, same bits), so the round trip is `Ã⁻ ∘ B̃ ∘ B̃⁻ ∘ Ã` with `X⁻` the
//! transpose inverse. Exactly, `B̃ ∘ B̃⁻ (y) = R Rᵀ y + (I − R Rᵀ) t_B`, off by at most
//! `ρ_B (|y| + |t_B|)` (`‖RRᵀ − I‖_F = ‖RᵀR − I‖_F`), and `Ã⁻ ∘ Ã` is off by `ρ_A |x|`; every
//! intermediate point and partial translation has norm at most `M`, and the residual of a
//! product is at most the sum of the residuals, so this part is at most `(Σρ_i)·M`.
//! Rounding: at most `k + 4` operations (`k − 2` chain composes, two inverses, two final
//! composes, two point applications), each perturbing the result by at most about `16ε·M`
//! in 2-norm (a compose: `3ε` per rotation entry, `9ε` in Frobenius norm, `3√3ε|t| + √3ε|t|`
//! on the translation; an application: XFORM's per-component analysis), and propagated
//! through maps of norm `1 + O(ρ)`. Budgeting `48ε·M` for each of `2k + 4` operations leaves
//! more than a factor 3 for second-order terms. The independent script
//! `tools/fixtures/geo_tree_cases.py` checks the bound in exact arithmetic on a tree built
//! from XFORM's worst-residual rotations.
//!
//! A path containing a similarity edge has no stated bound (`Ok(None)`): XFORM-004 covers
//! rigid transforms only. Note that the bound's `ρ` term can exceed REQ-GEO.FRAME.TREE-006's
//! 1e-12 m for accepted rotations (residual up to 1e-12 per edge at 100 m); the 1e-12 m
//! figure holds for the near-orthonormal transforms of `tree_7` (AC-4) and wherever this
//! bound is below it.
//!
//! # Digest layout version 1 (AC-5)
//!
//! SHA-256 (`fid-kci`) over, little-endian:
//!
//! ```text
//! "FIDFRT" | 0x01 | n_frames u64 | world id [16]
//! per frame in ascending ID-byte order:
//!     id [16] | kind u8 | has_parent u8 | parent [16] (zeros when none)
//!     | name_len u16 | name UTF-8 | xform u8 (1 rigid, 2 similarity)
//!     | R 9 × f64 row-major | t 3 × f64 | s f64 (1.0 for rigid)
//! ```
//!
//! Kind bytes: world 1, part 2, instrument 3, alignment 4, user 5. Every `f64` is written as
//! its IEEE bits with `-0.0` written as `+0.0` (frames hold no NaN or infinity). Only integer
//! and byte operations, so the digest is the same on every platform.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use fid_kci::kci::Sha256;
use fid_types::Digest;

use crate::xform::{check_rotation, dot3, orthonormality_residual, Matrix3, Transform, XformError, ROUND_TRIP_C};

/// Largest depth (edges from the world frame) a frame may have.
pub const MAX_DEPTH: usize = 256;
/// Largest number of frames in one tree, the world frame included.
pub const MAX_FRAMES: usize = 1 << 20;
/// Largest frame name, in UTF-8 bytes.
pub const MAX_NAME_BYTES: usize = 255;

/// The name of the world frame created by [`FrameTree::new`].
pub const WORLD_NAME: &str = "world";

const DIGEST_TAG: [u8; 6] = *b"FIDFRT";
const DIGEST_VERSION: u8 = 1;

/// A 16-byte frame ID. Text form: 32 hex digits, written lower-case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameId([u8; 16]);

impl FrameId {
    /// The ID of the world frame created by [`FrameTree::new`] (all zero).
    pub const WORLD: FrameId = FrameId([0; 16]);

    /// An ID from its bytes.
    pub const fn from_bytes(bytes: [u8; 16]) -> FrameId {
        FrameId(bytes)
    }

    /// The bytes.
    pub const fn to_bytes(self) -> [u8; 16] {
        self.0
    }

    /// Parses 32 hex digits. Upper- and lower-case digits give the same ID, so IDs that
    /// differ only in case are duplicates, never two frames.
    pub fn parse(s: &str) -> Result<FrameId, FrameError> {
        let b = s.as_bytes();
        if b.len() != 32 {
            return Err(FrameError::BadId);
        }
        let mut out = [0u8; 16];
        for (o, pair) in out.iter_mut().zip(b.chunks_exact(2)) {
            let hi = hex_val(pair[0]).ok_or(FrameError::BadId)?;
            let lo = hex_val(pair[1]).ok_or(FrameError::BadId)?;
            *o = (hi << 4) | lo;
        }
        Ok(FrameId(out))
    }
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

impl fmt::Display for FrameId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

/// What a frame stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FrameKind {
    /// The document's root frame.
    World,
    /// A part (workpiece) frame.
    Part,
    /// An instrument frame.
    Instrument,
    /// A frame produced by an alignment.
    Alignment,
    /// A user-defined frame.
    User,
}

impl FrameKind {
    /// The digest byte (layout version 1).
    pub const fn byte(self) -> u8 {
        match self {
            FrameKind::World => 1,
            FrameKind::Part => 2,
            FrameKind::Instrument => 3,
            FrameKind::Alignment => 4,
            FrameKind::User => 5,
        }
    }
}

/// A coordinate frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// Stable ID.
    pub id: FrameId,
    /// Display name, unique in the tree ignoring ASCII case.
    pub name: String,
    /// Kind.
    pub kind: FrameKind,
    /// The parent frame; `None` only for the world frame.
    pub parent: Option<FrameId>,
    /// Maps coordinates in this frame to coordinates in the parent (identity for world).
    pub to_parent: Transform,
}

/// Why a frame-tree operation failed. A failed edit leaves the tree unchanged.
#[derive(Debug, Clone, PartialEq)]
pub enum FrameError {
    /// The frame is not in the tree (also a dangling parent reference).
    UnknownFrame {
        /// The missing ID.
        id: FrameId,
    },
    /// A frame with this ID already exists.
    DuplicateId {
        /// The ID.
        id: FrameId,
    },
    /// A frame whose name equals this one ignoring ASCII case already exists.
    DuplicateName {
        /// The rejected name.
        name: String,
        /// The frame holding it.
        existing: FrameId,
    },
    /// The text is not 32 hex digits.
    BadId,
    /// The name is empty, longer than [`MAX_NAME_BYTES`] or holds a control character.
    BadName,
    /// A non-world frame has no parent.
    NoParent {
        /// The frame.
        id: FrameId,
    },
    /// The edit would make `frames[0]` its own ancestor: `frames` is the cycle that would form,
    /// starting at the frame, then its would-be parent and that frame's ancestors up to the
    /// frame (excluded). From [`FrameTree::from_frames`], the cycle starts at its first
    /// frame reached when walking parents from each frame in ascending ID order.
    Cycle {
        /// The cycle.
        frames: Vec<FrameId>,
    },
    /// The frame still has child frames.
    HasChildren {
        /// The frame.
        id: FrameId,
    },
    /// The world frame cannot be reparented, re-transformed or removed.
    WorldImmutable,
    /// A second frame of kind world.
    SecondWorld {
        /// The rejected frame.
        id: FrameId,
    },
    /// A frame list for [`FrameTree::from_frames`] does not hold exactly one world frame.
    WorldCount {
        /// Number of world frames found.
        found: usize,
    },
    /// The world frame has a parent or a transform other than the exact identity.
    BadWorld,
    /// The transform's rotation is improper (determinant not +1): it would join frames of
    /// opposite handedness.
    Handedness {
        /// The frame.
        id: FrameId,
        /// The determinant.
        det: f64,
    },
    /// The transform fails XFORM's validation (not orthonormal, non-finite).
    BadTransform {
        /// The frame.
        id: FrameId,
        /// The XFORM error.
        error: XformError,
    },
    /// The frame would be deeper than [`MAX_DEPTH`].
    TooDeep {
        /// The frame.
        id: FrameId,
    },
    /// The tree would hold more than [`MAX_FRAMES`] frames.
    TooManyFrames,
    /// A scratch allocation failed.
    Alloc,
    /// Composition or inversion failed during resolution.
    Xform(XformError),
    /// An internal invariant does not hold (unreachable through the public API).
    Corrupt,
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameError::UnknownFrame { id } => write!(f, "unknown_frame {id}"),
            FrameError::DuplicateId { id } => write!(f, "duplicate_id {id}"),
            FrameError::DuplicateName { name, existing } => write!(f, "duplicate_name {name:?} (held by {existing})"),
            FrameError::BadId => f.write_str("bad_id"),
            FrameError::BadName => f.write_str("bad_name"),
            FrameError::NoParent { id } => write!(f, "no_parent {id}"),
            FrameError::Cycle { frames } => {
                f.write_str("cycle [")?;
                for (i, p) in frames.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{p}")?;
                }
                f.write_str("]")
            }
            FrameError::HasChildren { id } => write!(f, "has_children {id}"),
            FrameError::WorldImmutable => f.write_str("world_immutable"),
            FrameError::SecondWorld { id } => write!(f, "second_world {id}"),
            FrameError::WorldCount { found } => write!(f, "world_count {found}"),
            FrameError::BadWorld => f.write_str("bad_world"),
            FrameError::Handedness { id, det } => write!(f, "handedness {id} (det {det})"),
            FrameError::BadTransform { id, error } => write!(f, "bad_transform {id}: {error}"),
            FrameError::TooDeep { id } => write!(f, "too_deep {id}"),
            FrameError::TooManyFrames => f.write_str("too_many_frames"),
            FrameError::Alloc => f.write_str("alloc"),
            FrameError::Xform(e) => write!(f, "xform: {e}"),
            FrameError::Corrupt => f.write_str("corrupt"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<XformError> for FrameError {
    fn from(e: XformError) -> Self {
        FrameError::Xform(e)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Node {
    frame: Frame,
    children: BTreeSet<FrameId>,
}

/// The frame tree of one document.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameTree {
    frames: BTreeMap<FrameId, Node>,
    /// ASCII-lower-cased name to ID.
    names: BTreeMap<String, FrameId>,
    world: FrameId,
}

impl Default for FrameTree {
    fn default() -> Self {
        Self::new()
    }
}

fn check_name(name: &str) -> Result<String, FrameError> {
    if name.is_empty() || name.len() > MAX_NAME_BYTES || name.chars().any(char::is_control) {
        return Err(FrameError::BadName);
    }
    Ok(name.to_ascii_lowercase())
}

fn check_transform(id: FrameId, t: &Transform) -> Result<(), FrameError> {
    check_frame_rotation(id, &t.rotation())
}

fn check_frame_rotation(id: FrameId, r: &Matrix3) -> Result<(), FrameError> {
    match check_rotation(r) {
        Ok(()) => Ok(()),
        Err(XformError::Improper { det }) => Err(FrameError::Handedness { id, det }),
        Err(error) => Err(FrameError::BadTransform { id, error }),
    }
}

fn scratch<T>(n: usize) -> Result<Vec<T>, FrameError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| FrameError::Alloc)?;
    Ok(v)
}

fn world_frame(id: FrameId, name: String) -> Frame {
    Frame { id, name, kind: FrameKind::World, parent: None, to_parent: Transform::IDENTITY }
}

impl FrameTree {
    /// A tree holding only the world frame ([`FrameId::WORLD`], named [`WORLD_NAME`])
    /// (REQ-GEO.FRAME.TREE-001).
    pub fn new() -> FrameTree {
        let world = FrameId::WORLD;
        let mut frames = BTreeMap::new();
        frames.insert(world, Node { frame: world_frame(world, WORLD_NAME.to_owned()), children: BTreeSet::new() });
        let mut names = BTreeMap::new();
        names.insert(WORLD_NAME.to_owned(), world);
        FrameTree { frames, names, world }
    }

    /// Builds a tree from a list of frames in any order (for example when loading a
    /// document), checking every invariant. Errors, in order: list longer than
    /// [`MAX_FRAMES`]; per frame in list order `BadName`, `DuplicateId`, `DuplicateName`;
    /// `WorldCount`; `BadWorld`; per frame in ID order `NoParent`, `Cycle` (a parent
    /// equal to the frame), `UnknownFrame` (dangling parent), `BadTransform`/`Handedness`;
    /// then, walking each frame in ID order up to the world, `Cycle` or `TooDeep`.
    pub fn from_frames(list: Vec<Frame>) -> Result<FrameTree, FrameError> {
        if list.len() > MAX_FRAMES {
            return Err(FrameError::TooManyFrames);
        }
        let mut frames: BTreeMap<FrameId, Node> = BTreeMap::new();
        let mut names: BTreeMap<String, FrameId> = BTreeMap::new();
        let mut worlds = 0usize;
        let mut world = FrameId::WORLD;
        for frame in list {
            let key = check_name(&frame.name)?;
            if frames.contains_key(&frame.id) {
                return Err(FrameError::DuplicateId { id: frame.id });
            }
            if let Some(existing) = names.get(&key) {
                return Err(FrameError::DuplicateName { name: frame.name, existing: *existing });
            }
            if frame.kind == FrameKind::World {
                worlds += 1;
                world = frame.id;
            }
            names.insert(key, frame.id);
            frames.insert(frame.id, Node { frame, children: BTreeSet::new() });
        }
        if worlds != 1 {
            return Err(FrameError::WorldCount { found: worlds });
        }
        let w = frames.get(&world).ok_or(FrameError::Corrupt)?;
        if w.frame.parent.is_some() || w.frame.to_parent != Transform::IDENTITY {
            return Err(FrameError::BadWorld);
        }
        let mut edges: Vec<(FrameId, FrameId)> = scratch(frames.len())?;
        for (id, node) in &frames {
            if *id == world {
                continue;
            }
            let parent = node.frame.parent.ok_or(FrameError::NoParent { id: *id })?;
            if parent == *id {
                return Err(FrameError::Cycle { frames: vec![*id] });
            }
            if !frames.contains_key(&parent) {
                return Err(FrameError::UnknownFrame { id: parent });
            }
            check_transform(*id, &node.frame.to_parent)?;
            edges.push((parent, *id));
        }
        for (parent, child) in edges {
            frames.get_mut(&parent).ok_or(FrameError::Corrupt)?.children.insert(child);
        }
        let tree = FrameTree { frames, names, world };
        tree.check_acyclic_and_depth()?;
        Ok(tree)
    }

    /// Walks every frame up to the world with a step bound, so a cycle or an over-deep
    /// chain is found without recursion. Frames already known to reach the world stop the
    /// walk early, so the total work is linear in the frame count.
    fn check_acyclic_and_depth(&self) -> Result<(), FrameError> {
        // depth of each frame known to reach the world.
        let mut depth: BTreeMap<FrameId, usize> = BTreeMap::new();
        depth.insert(self.world, 0);
        let mut stack: Vec<FrameId> = scratch(MAX_DEPTH + 1)?;
        for start in self.frames.keys() {
            stack.clear();
            let mut cur = *start;
            let base = loop {
                if let Some(d) = depth.get(&cur) {
                    break *d;
                }
                if let Some(pos) = stack.iter().position(|f| *f == cur) {
                    // `cur` repeats: the cycle is stack[pos..], reported from `cur` downward
                    // in the would-be order frame, parent, grandparent, ...
                    let mut frames = scratch(stack.len() - pos)?;
                    frames.extend_from_slice(&stack[pos..]);
                    return Err(FrameError::Cycle { frames });
                }
                if stack.len() > MAX_DEPTH {
                    return Err(FrameError::TooDeep { id: *start });
                }
                stack.push(cur);
                cur = self.parent_of(cur)?;
            };
            for (i, f) in stack.iter().rev().enumerate() {
                let d = base + i + 1;
                if d > MAX_DEPTH {
                    return Err(FrameError::TooDeep { id: *f });
                }
                depth.insert(*f, d);
            }
        }
        Ok(())
    }

    fn node(&self, id: FrameId) -> Result<&Node, FrameError> {
        self.frames.get(&id).ok_or(FrameError::UnknownFrame { id })
    }

    fn parent_of(&self, id: FrameId) -> Result<FrameId, FrameError> {
        self.node(id)?.frame.parent.ok_or(FrameError::Corrupt)
    }

    /// The world frame's ID.
    pub fn world(&self) -> FrameId {
        self.world
    }

    /// Number of frames, the world frame included.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Always false: the world frame is always present.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// The frame with this ID.
    pub fn get(&self, id: FrameId) -> Option<&Frame> {
        self.frames.get(&id).map(|n| &n.frame)
    }

    /// The frame whose name equals `name` ignoring ASCII case.
    pub fn find_by_name(&self, name: &str) -> Option<FrameId> {
        self.names.get(&name.to_ascii_lowercase()).copied()
    }

    /// All frames in ascending ID order.
    pub fn frames(&self) -> impl Iterator<Item = &Frame> {
        self.frames.values().map(|n| &n.frame)
    }

    /// The child IDs of a frame in ascending order.
    pub fn children(&self, id: FrameId) -> Result<impl Iterator<Item = FrameId> + '_, FrameError> {
        Ok(self.node(id)?.children.iter().copied())
    }

    /// `[id, parent(id), …, world]`, child first. Iterative and bounded by [`MAX_DEPTH`].
    pub fn ancestors(&self, id: FrameId) -> Result<Vec<FrameId>, FrameError> {
        self.node(id)?;
        let mut out = scratch(MAX_DEPTH + 1)?;
        let mut cur = id;
        loop {
            out.push(cur);
            if cur == self.world {
                return Ok(out);
            }
            if out.len() > MAX_DEPTH {
                return Err(FrameError::Corrupt);
            }
            cur = self.parent_of(cur)?;
        }
    }

    /// Edges from the world frame to `id`.
    pub fn depth(&self, id: FrameId) -> Result<usize, FrameError> {
        Ok(self.ancestors(id)?.len() - 1)
    }

    /// Edges on the longest downward path from `id` (0 for a leaf). Iterative.
    fn height(&self, id: FrameId) -> Result<usize, FrameError> {
        let mut best = 0usize;
        let mut stack: Vec<(FrameId, usize)> = scratch(16)?;
        stack.push((id, 0));
        let mut visited = 0usize;
        while let Some((f, h)) = stack.pop() {
            visited += 1;
            if visited > self.frames.len() || h > MAX_DEPTH {
                return Err(FrameError::Corrupt);
            }
            if h > best {
                best = h;
            }
            for c in &self.node(f)?.children {
                stack.try_reserve(1).map_err(|_| FrameError::Alloc)?;
                stack.push((*c, h + 1));
            }
        }
        Ok(best)
    }

    /// Adds a frame under an existing parent. Errors, in order: `BadName`, `TooManyFrames`,
    /// `SecondWorld`, `DuplicateId`, `NoParent`, `Cycle` (parent equal to the frame),
    /// `UnknownFrame` (dangling parent), `DuplicateName`, `BadTransform`/`Handedness`,
    /// `TooDeep`.
    pub fn insert(&mut self, frame: Frame) -> Result<(), FrameError> {
        let key = check_name(&frame.name)?;
        if self.frames.len() >= MAX_FRAMES {
            return Err(FrameError::TooManyFrames);
        }
        let id = frame.id;
        if frame.kind == FrameKind::World {
            return Err(FrameError::SecondWorld { id });
        }
        if self.frames.contains_key(&id) {
            return Err(FrameError::DuplicateId { id });
        }
        let parent = frame.parent.ok_or(FrameError::NoParent { id })?;
        if parent == id {
            return Err(FrameError::Cycle { frames: vec![id] });
        }
        self.node(parent)?;
        if let Some(existing) = self.names.get(&key) {
            return Err(FrameError::DuplicateName { name: frame.name, existing: *existing });
        }
        check_transform(id, &frame.to_parent)?;
        if self.depth(parent)? + 1 > MAX_DEPTH {
            return Err(FrameError::TooDeep { id });
        }
        self.frames.get_mut(&parent).ok_or(FrameError::Corrupt)?.children.insert(id);
        self.names.insert(key, id);
        self.frames.insert(id, Node { frame, children: BTreeSet::new() });
        Ok(())
    }

    /// Moves a frame (with its subtree) under a new parent with a new transform to it.
    /// Errors, in order: `WorldImmutable`, `UnknownFrame` (frame), `Cycle` (new parent equal
    /// to the frame, or a descendant of it), `UnknownFrame` (new parent),
    /// `BadTransform`/`Handedness`, `TooDeep` (some frame of the subtree).
    pub fn reparent(&mut self, id: FrameId, new_parent: FrameId, to_parent: Transform) -> Result<(), FrameError> {
        if id == self.world {
            return Err(FrameError::WorldImmutable);
        }
        let old_parent = self.parent_of(id)?;
        if new_parent == id {
            return Err(FrameError::Cycle { frames: vec![id] });
        }
        let chain = self.ancestors(new_parent)?;
        if let Some(pos) = chain.iter().position(|f| *f == id) {
            let mut frames = scratch(pos + 1)?;
            frames.push(id);
            frames.extend_from_slice(&chain[..pos]);
            return Err(FrameError::Cycle { frames });
        }
        check_transform(id, &to_parent)?;
        if (chain.len() - 1) + 1 + self.height(id)? > MAX_DEPTH {
            return Err(FrameError::TooDeep { id });
        }
        self.frames.get_mut(&old_parent).ok_or(FrameError::Corrupt)?.children.remove(&id);
        self.frames.get_mut(&new_parent).ok_or(FrameError::Corrupt)?.children.insert(id);
        let node = self.frames.get_mut(&id).ok_or(FrameError::Corrupt)?;
        node.frame.parent = Some(new_parent);
        node.frame.to_parent = to_parent;
        Ok(())
    }

    /// Replaces a frame's transform to its parent. Errors, in order: `WorldImmutable`,
    /// `UnknownFrame`, `BadTransform`/`Handedness`.
    pub fn set_transform(&mut self, id: FrameId, to_parent: Transform) -> Result<(), FrameError> {
        if id == self.world {
            return Err(FrameError::WorldImmutable);
        }
        self.node(id)?;
        check_transform(id, &to_parent)?;
        let node = self.frames.get_mut(&id).ok_or(FrameError::Corrupt)?;
        node.frame.to_parent = to_parent;
        Ok(())
    }

    /// Removes a frame without children (REQ-GEO.FRAME.TREE-004) and returns it. Errors, in
    /// order: `WorldImmutable`, `UnknownFrame`, `HasChildren`.
    pub fn remove(&mut self, id: FrameId) -> Result<Frame, FrameError> {
        if id == self.world {
            return Err(FrameError::WorldImmutable);
        }
        let node = self.node(id)?;
        if !node.children.is_empty() {
            return Err(FrameError::HasChildren { id });
        }
        let parent = node.frame.parent.ok_or(FrameError::Corrupt)?;
        let key = node.frame.name.to_ascii_lowercase();
        self.frames.get_mut(&parent).ok_or(FrameError::Corrupt)?.children.remove(&id);
        self.names.remove(&key);
        let node = self.frames.remove(&id).ok_or(FrameError::Corrupt)?;
        Ok(node.frame)
    }

    /// The frames strictly below the lowest common ancestor on each side, child first.
    fn split(&self, a: FrameId, b: FrameId) -> Result<(Vec<FrameId>, Vec<FrameId>), FrameError> {
        let mut la = self.ancestors(a)?;
        let mut lb = self.ancestors(b)?;
        while let (Some(x), Some(y)) = (la.last(), lb.last()) {
            if x != y {
                break;
            }
            la.pop();
            lb.pop();
        }
        Ok((la, lb))
    }

    fn chain(&self, frames: &[FrameId]) -> Result<Option<Transform>, FrameError> {
        let Some((first, rest)) = frames.split_first() else {
            return Ok(None);
        };
        let mut acc = self.node(*first)?.frame.to_parent;
        for g in rest {
            acc = self.node(*g)?.frame.to_parent.compose(&acc)?;
        }
        Ok(Some(acc))
    }

    /// The transform mapping coordinates in frame `a` to coordinates in frame `b`, composed
    /// through the lowest common ancestor in the pinned order (module docs;
    /// REQ-GEO.FRAME.TREE-003). `resolve(a, a)` is the exact identity.
    pub fn resolve(&self, a: FrameId, b: FrameId) -> Result<Transform, FrameError> {
        self.node(a)?;
        self.node(b)?;
        if a == b {
            return Ok(Transform::IDENTITY);
        }
        let (ea, eb) = self.split(a, b)?;
        let ta = self.chain(&ea)?;
        let tb = self.chain(&eb)?;
        match (ta, tb) {
            (Some(ta), None) => Ok(ta),
            (None, Some(tb)) => Ok(tb.inverse()?),
            (Some(ta), Some(tb)) => Ok(tb.inverse()?.compose(&ta)?),
            (None, None) => Err(FrameError::Corrupt),
        }
    }

    /// The bound on `|resolve(b, a)(resolve(a, b)(x)) − x|` stated in the module docs, or
    /// `None` when the path holds a similarity edge. `Some(0.0)` for `a = b`.
    pub fn path_round_trip_bound(&self, a: FrameId, b: FrameId, x: &[f64; 3]) -> Result<Option<f64>, FrameError> {
        self.node(a)?;
        self.node(b)?;
        if a == b {
            return Ok(Some(0.0));
        }
        let (ea, eb) = self.split(a, b)?;
        let mut rho = 0.0;
        let mut tsum = 0.0;
        for f in ea.iter().chain(&eb) {
            let t = self.node(*f)?.frame.to_parent;
            if !t.is_rigid() {
                return Ok(None);
            }
            rho += orthonormality_residual(&t.rotation())?;
            let tr = t.translation();
            tsum += fid_num::f::sqrt(dot3(tr[0], tr[1], tr[2], tr[0], tr[1], tr[2])).map_err(XformError::from)?;
        }
        let nx = fid_num::f::sqrt(dot3(x[0], x[1], x[2], x[0], x[1], x[2])).map_err(XformError::from)?;
        let m = nx + tsum;
        // k <= 2 * MAX_DEPTH, exact in f64.
        let ops = (2 * (ea.len() + eb.len()) + 4) as f64;
        Ok(Some(rho * m + (ROUND_TRIP_C * (f64::EPSILON * 0.5) * ops) * m))
    }

    /// The SHA-256 digest of layout version 1 (module docs; AC-5).
    pub fn digest(&self) -> Digest {
        let mut h = Sha256::new();
        h.update(&DIGEST_TAG);
        h.update(&[DIGEST_VERSION]);
        h.update(&(self.frames.len() as u64).to_le_bytes());
        h.update(&self.world.0);
        for (id, node) in &self.frames {
            let f = &node.frame;
            h.update(&id.0);
            h.update(&[f.kind.byte()]);
            match f.parent {
                Some(p) => {
                    h.update(&[1]);
                    h.update(&p.0);
                }
                None => {
                    h.update(&[0]);
                    h.update(&[0u8; 16]);
                }
            }
            // Names are at most MAX_NAME_BYTES (255) bytes.
            h.update(&(f.name.len() as u16).to_le_bytes());
            h.update(f.name.as_bytes());
            h.update(&[if f.to_parent.is_rigid() { 1 } else { 2 }]);
            let r = f.to_parent.rotation();
            let t = f.to_parent.translation();
            for v in r.iter().flatten().chain(t.iter()).chain(core::iter::once(&f.to_parent.scale())) {
                let bits = if *v == 0.0 { 0u64 } else { v.to_bits() };
                h.update(&bits.to_le_bytes());
            }
        }
        Digest::from_bytes(h.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Transform` cannot hold a reflection (XFORM refuses it on construction), so the
    /// handedness mapping is checked on the rotation check directly.
    #[test]
    fn reflection_is_a_handedness_error() {
        let id = FrameId::from_bytes([7; 16]);
        let mirror_x = [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        assert_eq!(check_frame_rotation(id, &mirror_x), Err(FrameError::Handedness { id, det: -1.0 }));
        let skew = [[1.0, 1e-9, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        assert!(matches!(
            check_frame_rotation(id, &skew),
            Err(FrameError::BadTransform { error: XformError::NotOrthonormal { .. }, .. })
        ));
        assert_eq!(check_frame_rotation(id, &crate::xform::IDENTITY), Ok(()));
    }
}
