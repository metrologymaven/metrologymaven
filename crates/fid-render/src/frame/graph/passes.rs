//! Pass registration (REQ-VIZ.RENDER.FRAME.GRAPH-003).
//!
//! A pass declares a stable [`PassId`], an order key and the targets it reads and writes.
//! Passes run in ascending order key. A registration is checked against the passes already
//! registered: its inputs must be the frame's own targets ([`COLOUR`], [`DEPTH`], [`ID`],
//! cleared before the first pass) or outputs of a registered pass with a lower order key, else
//! [`GraphError::UnresolvedInput`]; so a producer is registered before its consumers. A second
//! pass with the same ID is [`GraphError::DuplicatePass`], and one with the same order key
//! [`GraphError::DuplicateOrder`] (equal keys would leave the order to registration order).
//! A rejected registration changes nothing.

use super::{FrameTargets, GraphError};
use std::fmt;

/// A pass's stable name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PassId(pub &'static str);

/// A render target a pass reads or writes, by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TargetId(pub &'static str);

/// The colour target (cleared to the frame's clear colour).
pub const COLOUR: TargetId = TargetId("colour");
/// The reversed-Z depth target (cleared to 0.0).
pub const DEPTH: TargetId = TargetId("depth");
/// The ID target (cleared to 0: no drawable).
pub const ID: TargetId = TargetId("id");
/// The targets the frame itself provides.
pub const FRAME_TARGETS: [TargetId; 3] = [COLOUR, DEPTH, ID];

impl fmt::Display for PassId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl fmt::Display for TargetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// What a pass gets to record its commands.
pub struct PassContext<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub targets: &'a FrameTargets,
}

/// A pass of the frame. `record` adds its commands to the frame's encoder; an error stops
/// the frame ([`GraphError::PassFailed`]) and nothing is submitted.
pub trait Pass {
    fn record(&mut self, ctx: &mut PassContext<'_>) -> Result<(), String>;
}

/// A pass registration.
pub struct PassDecl {
    pub id: PassId,
    pub order: u32,
    pub inputs: Vec<TargetId>,
    pub outputs: Vec<TargetId>,
    pub run: Box<dyn Pass>,
}

impl fmt::Debug for PassDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PassDecl")
            .field("id", &self.id)
            .field("order", &self.order)
            .field("inputs", &self.inputs)
            .field("outputs", &self.outputs)
            .finish_non_exhaustive()
    }
}

/// The registered passes, kept in ascending order key.
#[derive(Debug, Default)]
pub struct PassList {
    passes: Vec<PassDecl>,
}

impl PassList {
    pub fn new() -> PassList {
        PassList::default()
    }

    /// Registers `decl` (see the module documentation for the checks).
    pub fn register(&mut self, decl: PassDecl) -> Result<(), GraphError> {
        if self.passes.iter().any(|p| p.id == decl.id) {
            return Err(GraphError::DuplicatePass { pass: decl.id });
        }
        if let Some(p) = self.passes.iter().find(|p| p.order == decl.order) {
            return Err(GraphError::DuplicateOrder { order: decl.order, pass: decl.id, existing: p.id });
        }
        for input in &decl.inputs {
            let produced = FRAME_TARGETS.contains(input)
                || self.passes.iter().any(|p| p.order < decl.order && p.outputs.contains(input));
            if !produced {
                return Err(GraphError::UnresolvedInput { pass: decl.id, input: *input });
            }
        }
        let at = self.passes.iter().position(|p| p.order > decl.order).unwrap_or(self.passes.len());
        self.passes.insert(at, decl);
        Ok(())
    }

    /// The pass IDs in execution order.
    pub fn order(&self) -> Vec<PassId> {
        self.passes.iter().map(|p| p.id).collect()
    }

    pub fn len(&self) -> usize {
        self.passes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.passes.is_empty()
    }

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut PassDecl> {
        self.passes.iter_mut()
    }
}
