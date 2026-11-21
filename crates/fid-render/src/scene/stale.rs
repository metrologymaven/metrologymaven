//! Stale marking and the digest check (spec leaf `VIZ.RENDER.SCENE.STALE`).
//!
//! The display side of CORE's `outdated` marking (`CORE.DEP.PROP`): a result is shown stale
//! from the moment an upstream input changes until recompute delivers it again, and a stale
//! node never gets a conforming or nonconforming style.
//!
//! # Rules pinned here
//!
//! - **Marking** (REQ-001). [`SceneGraph::mark_outdated`] takes the elements CORE marks
//!   `outdated` (the `elements` of `fid_dep::ElementsMarked`, which already holds every
//!   downstream element) and, in that one change batch, sets every node bound to them and every
//!   node derived from them to [`DisplayStatus::Stale`]. `since_digest` is the data digest the
//!   node displays at that moment, `evidence` the claimed, declared-input and
//!   uncharacterized-input facts of the status it replaces (all unset for `not_evaluated` and
//!   `error`). The batch lists exactly the nodes that became stale, as `updated`. Entities
//!   without nodes keep their mark, so a node added for them later starts stale. A mark is
//!   pruned when a re-projection finds its entity no longer in the document.
//! - **Projector view.** While an element is marked, its projector is handed a view whose
//!   status is already `Stale` (since the element's digest, with its evidence facts), so a
//!   projector can never pick a style from the hidden kernel outcome.
//! - **Leaving** (REQ-002). The marks are cleared with [`SceneGraph::recomputed`], which
//!   re-projects the cleared elements together with the dirty set in one batch, or with
//!   [`SceneGraph::clear_outdated`], which must be followed by a re-projection of them. A stale node leaves
//!   `Stale` only when the projector delivers it again (re-projection of its element) **and**
//!   its element's mark is cleared; it then shows the delivered kernel status. If the delivered
//!   digest differs from `since_digest` while the mark is still set, the node stays stale (new
//!   data, `since_digest` unchanged): CORE still says the result is out of date, and a stale
//!   result must never look current. This is stricter than, and implies, REQ-002's "a new
//!   digest, or the same digest with the mark cleared" (see [`next_status`]).
//! - **Style guard** (REQ-003). [`StyleResolver`] wraps the [`StatusStyles`] that
//!   `VIZ.OVERLAY.STATUS` supplies and checks `Stale` before the kernel outcome. Its
//!   constructor refuses styles under which a stale style equals any conforming or
//!   nonconforming style, or the style of any other current result (`INDETERMINATE`, every
//!   `NOT_DECIDABLE(reason)`, `not_evaluated`; error codes are open-ended and not checked),
//!   a stale value with evidence facts looks like a stale measured one,
//!   or a conforming or nonconforming value with evidence facts looks measured (D-045). The
//!   stale styles are read once, at construction, so the stale path cannot be changed later.
//! - **Count** (REQ-004). [`SceneGraph::stale_count`] after every batch; [`MarkOutcome`] and
//!   [`RecomputeOutcome`] carry it.
//!
//! Statuses come from the kernel (constitution C2): nothing here computes or maps an outcome;
//! `Stale` only hides it. Determinism D0: `BTreeSet` only, no clock, no randomness, no float
//! arithmetic. Every error is a value; nothing panics (CONSTITUTION §8). A failed call leaves
//! the graph and its marks unchanged.

use std::collections::BTreeSet;
use std::fmt;

use fid_kdec::{NotDecidableReason, Outcome as KernelOutcome};
use fid_types::Digest;

use super::idtab::{EntityId, RenderId};
use super::proj::{
    ChangeBatch, Diagnostic, DisplayStatus, Document, KernelStatus, ProjectorRegistry, SceneError, SceneGraph,
    SceneNode, StatusCode, StyleToken, MAX_ELEMENTS,
};

/// The evidence facts of a kernel status that a display must keep: any of them means the
/// value must not look measured (D-045).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Evidence {
    /// The value was declared or derived from a specification, not evaluated.
    pub claimed: bool,
    /// The value rests on at least one declared input.
    pub declared_input: bool,
    /// The value rests on at least one uncharacterized input.
    pub uncharacterized_input: bool,
}

impl Evidence {
    /// No fact set: a measured value.
    pub const MEASURED: Evidence = Evidence { claimed: false, declared_input: false, uncharacterized_input: false };

    /// All eight combinations, [`Evidence::MEASURED`] first.
    pub const ALL: [Evidence; 8] = [
        Evidence::from_index(0),
        Evidence::from_index(1),
        Evidence::from_index(2),
        Evidence::from_index(3),
        Evidence::from_index(4),
        Evidence::from_index(5),
        Evidence::from_index(6),
        Evidence::from_index(7),
    ];

    const fn from_index(i: usize) -> Evidence {
        Evidence { claimed: i & 4 != 0, declared_input: i & 2 != 0, uncharacterized_input: i & 1 != 0 }
    }

    const fn index(self) -> usize {
        ((self.claimed as usize) << 2) | ((self.declared_input as usize) << 1) | (self.uncharacterized_input as usize)
    }

    /// Whether any fact is set.
    pub const fn any(self) -> bool {
        self.claimed || self.declared_input || self.uncharacterized_input
    }

    /// The facts of a kernel status.
    pub const fn of_kernel(k: &KernelStatus) -> Evidence {
        Evidence {
            claimed: k.claimed,
            declared_input: k.declared_input,
            uncharacterized_input: k.uncharacterized_input,
        }
    }

    /// The facts a status carries: a kernel status's flags, a stale status's kept flags, none
    /// for `not_evaluated` and `error`.
    pub fn of(status: &DisplayStatus) -> Evidence {
        match status {
            DisplayStatus::Kernel(k) => Evidence::of_kernel(k),
            DisplayStatus::Stale { evidence, .. } => *evidence,
            DisplayStatus::NotEvaluated | DisplayStatus::Error(_) => Evidence::MEASURED,
        }
    }
}

/// The status a node gets when its element's projector delivers it (REQ-002), a pure function
/// of the delivered status and digest, the node as it is now (`None` for a new node) and
/// whether CORE still marks the element `outdated`:
///
/// | marked | current node | result |
/// |---|---|---|
/// | no | any | `delivered` (leaves `Stale`; same or new digest) |
/// | yes | `Stale { since }` | `Stale { since, evidence of delivered }` |
/// | yes | not stale | `Stale { its current digest, evidence of delivered }` |
/// | yes | none (new node) | `Stale { delivered digest, evidence of delivered }` |
pub fn next_status(
    delivered: &DisplayStatus,
    delivered_digest: Digest,
    current: Option<&SceneNode>,
    marked: bool,
) -> DisplayStatus {
    if !marked {
        return delivered.clone();
    }
    let since_digest = match current {
        Some(SceneNode { status: DisplayStatus::Stale { since_digest, .. }, .. }) => *since_digest,
        Some(node) => node.data_digest,
        None => delivered_digest,
    };
    DisplayStatus::Stale { since_digest, evidence: Evidence::of(delivered) }
}

/// The status a node gets when CORE marks its element `outdated` (REQ-001): unchanged if it is
/// already stale, else `Stale` since the digest it displays, keeping its evidence facts.
pub fn marked_status(node: &SceneNode) -> DisplayStatus {
    match node.status {
        DisplayStatus::Stale { .. } => node.status.clone(),
        _ => DisplayStatus::Stale { since_digest: node.data_digest, evidence: Evidence::of(&node.status) },
    }
}

/// The result of [`SceneGraph::mark_outdated`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MarkOutcome {
    /// The nodes that became stale, as `updated` (ascending); nothing is added or retired.
    pub batch: ChangeBatch,
    /// Stale nodes after the batch (REQ-004).
    pub stale_count: usize,
}

/// The result of [`SceneGraph::recomputed`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecomputeOutcome {
    /// What changed, as for [`SceneGraph::reproject`].
    pub batch: ChangeBatch,
    /// Skipped and failed elements, in processing order.
    pub diagnostics: Vec<Diagnostic>,
    /// Stale nodes after the batch (REQ-004).
    pub stale_count: usize,
}

impl SceneGraph {
    /// Marks the listed elements `outdated` (REQ-001): every node bound to them or derived from
    /// them becomes [`DisplayStatus::Stale`] in this one batch. Duplicates count once; entities
    /// without nodes keep the mark. Fails with [`SceneError::TooManyElements`] if the list or
    /// the resulting mark set exceeds [`MAX_ELEMENTS`], leaving the graph unchanged.
    pub fn mark_outdated(&mut self, marked: &[EntityId]) -> Result<MarkOutcome, SceneError> {
        if marked.len() > MAX_ELEMENTS {
            return Err(SceneError::TooManyElements);
        }
        let set: BTreeSet<EntityId> = marked.iter().copied().collect();
        let fresh = set.iter().filter(|e| !self.outdated.contains(e)).count();
        if self.outdated.len().saturating_add(fresh) > MAX_ELEMENTS {
            return Err(SceneError::TooManyElements);
        }
        // Plan: the nodes that change.
        let mut updated: Vec<RenderId> = Vec::new();
        for &e in &set {
            for &rid in self.ids.render_ids(e) {
                if self.nodes.get(&rid).is_some_and(|n| !n.status.is_stale()) {
                    updated.try_reserve(1).map_err(|_| SceneError::AllocationFailed)?;
                    updated.push(rid);
                }
            }
        }
        // Commit: nothing below can fail.
        for &rid in &updated {
            if let Some(n) = self.nodes.get_mut(&rid) {
                n.status = marked_status(n);
            }
        }
        self.outdated.extend(set);
        updated.sort_unstable();
        Ok(MarkOutcome { batch: ChangeBatch { updated, ..ChangeBatch::default() }, stale_count: self.stale_count() })
    }

    /// Clears CORE's `outdated` mark of the listed elements. Their nodes stay stale until the
    /// projector delivers them again, so **every call must be followed by a
    /// [`SceneGraph::reproject`] whose dirty set includes the cleared elements**; otherwise a
    /// node with an unchanged digest stays stale with no mark left. Prefer
    /// [`SceneGraph::recomputed`], which clears and re-projects them in one batch. Fails with
    /// [`SceneError::TooManyElements`] for more than [`MAX_ELEMENTS`] entries.
    pub fn clear_outdated(&mut self, cleared: &[EntityId]) -> Result<(), SceneError> {
        if cleared.len() > MAX_ELEMENTS {
            return Err(SceneError::TooManyElements);
        }
        for e in cleared {
            self.outdated.remove(e);
        }
        Ok(())
    }

    /// Recompute finished: clears the marks of `cleared`, then re-projects `dirty ∪ cleared`
    /// against `doc`, in one change batch (REQ-002). Every cleared element is re-delivered, so
    /// none of its nodes can stay stale without a mark, even when CORE leaves it out of the
    /// dirty set because its digest did not change. A delivered node whose element is no longer marked
    /// leaves `Stale` and shows its kernel status; a node of an element still marked stays
    /// stale. On error the graph and its marks are unchanged.
    pub fn recomputed(
        &mut self,
        registry: &ProjectorRegistry,
        doc: &Document,
        dirty: &[EntityId],
        cleared: &[EntityId],
    ) -> Result<RecomputeOutcome, SceneError> {
        if cleared.len() > MAX_ELEMENTS || dirty.len().saturating_add(cleared.len()) > MAX_ELEMENTS {
            return Err(SceneError::TooManyElements);
        }
        let mut deliver: Vec<EntityId> = Vec::new();
        deliver.try_reserve_exact(dirty.len() + cleared.len()).map_err(|_| SceneError::AllocationFailed)?;
        deliver.extend_from_slice(dirty);
        deliver.extend_from_slice(cleared);
        let mut removed: Vec<EntityId> = Vec::new();
        removed.try_reserve_exact(cleared.len()).map_err(|_| SceneError::AllocationFailed)?;
        for e in cleared {
            if self.outdated.remove(e) {
                removed.push(*e);
            }
        }
        match self.reproject(registry, doc, &deliver) {
            Ok(out) => {
                Ok(RecomputeOutcome { batch: out.batch, diagnostics: out.diagnostics, stale_count: self.stale_count() })
            }
            Err(e) => {
                self.outdated.extend(removed);
                Err(e)
            }
        }
    }

    /// Whether CORE currently marks this element `outdated`.
    pub fn is_outdated(&self, entity: EntityId) -> bool {
        self.outdated.contains(&entity)
    }

    /// The elements currently marked `outdated`, ascending.
    pub fn outdated(&self) -> impl Iterator<Item = &EntityId> {
        self.outdated.iter()
    }

    /// The number of stale nodes (REQ-004).
    pub fn stale_count(&self) -> usize {
        self.nodes.values().filter(|n| n.status.is_stale()).count()
    }
}

/// The status styles `VIZ.OVERLAY.STATUS` supplies, as style tokens SHADE resolves (its glyph,
/// pattern and text carry the state, never colour alone). Implementations must be pure.
pub trait StatusStyles {
    /// The style of a kernel decision, with its evidence facts.
    fn kernel(&self, status: &KernelStatus) -> StyleToken;
    /// The stale style, for a value with these evidence facts.
    fn stale(&self, evidence: Evidence) -> StyleToken;
    /// The style of a node with nothing evaluated.
    fn not_evaluated(&self) -> StyleToken;
    /// The style of a kernel evaluation error.
    fn error(&self, code: &StatusCode) -> StyleToken;
}

/// Why [`StyleResolver::new`] refused a set of status styles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleError {
    /// The stale style for these facts equals a conforming or nonconforming style (REQ-003).
    StaleLooksDecided {
        /// The facts of the stale value.
        evidence: Evidence,
        /// The outcome whose style it equals.
        outcome: KernelOutcome,
    },
    /// The stale style for these facts equals the style of a current result that is not
    /// conforming or nonconforming (`INDETERMINATE`, any `NOT_DECIDABLE(reason)`, or
    /// `not_evaluated`): a stale node must look stale, never like any current result.
    StaleLooksCurrent {
        /// The facts of the stale value.
        evidence: Evidence,
        /// The status it looks like: `kernel:<OUTCOME>` or `not_evaluated`.
        status: String,
    },
    /// A stale value with evidence facts gets the style of a stale measured value (D-045).
    StaleLooksMeasured {
        /// The facts of the stale value.
        evidence: Evidence,
    },
    /// A conforming or nonconforming value with evidence facts gets a measured style (D-045).
    ClaimedLooksMeasured {
        /// The outcome.
        outcome: KernelOutcome,
        /// The facts.
        evidence: Evidence,
    },
}

impl StyleError {
    /// The stable error code.
    pub const fn code(&self) -> &'static str {
        match self {
            StyleError::StaleLooksDecided { .. } => "StaleLooksDecided",
            StyleError::StaleLooksCurrent { .. } => "StaleLooksCurrent",
            StyleError::StaleLooksMeasured { .. } => "StaleLooksMeasured",
            StyleError::ClaimedLooksMeasured { .. } => "ClaimedLooksMeasured",
        }
    }
}

impl fmt::Display for StyleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StyleError::StaleLooksDecided { evidence, outcome } => {
                write!(f, "{}: stale {evidence:?} styled as {}", self.code(), outcome.wire())
            }
            StyleError::StaleLooksCurrent { evidence, status } => {
                write!(f, "{}: stale {evidence:?} styled as {status}", self.code())
            }
            StyleError::StaleLooksMeasured { evidence } => write!(f, "{}: stale {evidence:?}", self.code()),
            StyleError::ClaimedLooksMeasured { outcome, evidence } => {
                write!(f, "{}: {} {evidence:?}", self.code(), outcome.wire())
            }
        }
    }
}

impl std::error::Error for StyleError {}

/// The decided outcomes whose styles a stale node must never get (REQ-003).
const DECIDED: [KernelOutcome; 2] = [KernelOutcome::Conforms, KernelOutcome::Nonconforms];

/// The style resolver: consults [`DisplayStatus`] before the kernel outcome, so a stale node
/// always gets the stale style and never a conforming or nonconforming one (REQ-003).
#[derive(Debug, Clone)]
pub struct StyleResolver<S> {
    styles: S,
    stale: [StyleToken; 8],
}

impl<S: StatusStyles> StyleResolver<S> {
    /// Wraps `styles` after checking them (module docs, "Style guard").
    pub fn new(styles: S) -> Result<StyleResolver<S>, StyleError> {
        let kernel = |outcome: KernelOutcome, e: Evidence| {
            styles.kernel(&KernelStatus {
                outcome,
                claimed: e.claimed,
                declared_input: e.declared_input,
                uncharacterized_input: e.uncharacterized_input,
            })
        };
        let stale = Evidence::ALL.map(|e| styles.stale(e));
        let measured: Vec<(KernelOutcome, StyleToken)> =
            DECIDED.iter().map(|&o| (o, kernel(o, Evidence::MEASURED))).collect();
        for &o in &DECIDED {
            for e in Evidence::ALL {
                let t = kernel(o, e);
                if let Some(s) = Evidence::ALL.into_iter().find(|s| stale[s.index()] == t) {
                    return Err(StyleError::StaleLooksDecided { evidence: s, outcome: o });
                }
                if e.any() && measured.iter().any(|(_, m)| *m == t) {
                    return Err(StyleError::ClaimedLooksMeasured { outcome: o, evidence: e });
                }
            }
        }
        // A stale node must not look like any other current result either.
        let mut current: Vec<KernelOutcome> = vec![KernelOutcome::Indeterminate];
        current.extend(NotDecidableReason::ALL.iter().map(|&r| KernelOutcome::NotDecidable(r)));
        for o in current {
            for e in Evidence::ALL {
                let t = kernel(o, e);
                if let Some(s) = Evidence::ALL.into_iter().find(|s| stale[s.index()] == t) {
                    let status = match o.reason() {
                        Some(r) => format!("kernel:{}({})", o.wire(), r.wire()),
                        None => format!("kernel:{}", o.wire()),
                    };
                    return Err(StyleError::StaleLooksCurrent { evidence: s, status });
                }
            }
        }
        let not_evaluated = styles.not_evaluated();
        if let Some(s) = Evidence::ALL.into_iter().find(|s| stale[s.index()] == not_evaluated) {
            return Err(StyleError::StaleLooksCurrent { evidence: s, status: "not_evaluated".to_string() });
        }
        if let Some(e) = Evidence::ALL.into_iter().find(|e| e.any() && stale[e.index()] == stale[0]) {
            return Err(StyleError::StaleLooksMeasured { evidence: e });
        }
        Ok(StyleResolver { styles, stale })
    }

    /// The style of a status: `Stale` first (the stale style read at construction), then the
    /// kernel outcome, `not_evaluated` and `error`.
    pub fn resolve(&self, status: &DisplayStatus) -> StyleToken {
        match status {
            DisplayStatus::Stale { evidence, .. } => self.stale[evidence.index()].clone(),
            DisplayStatus::Kernel(k) => self.styles.kernel(k),
            DisplayStatus::NotEvaluated => self.styles.not_evaluated(),
            DisplayStatus::Error(code) => self.styles.error(code),
        }
    }

    /// The style of a node's status.
    pub fn resolve_node(&self, node: &SceneNode) -> StyleToken {
        self.resolve(&node.status)
    }

    /// The wrapped styles.
    pub fn styles(&self) -> &S {
        &self.styles
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_table() {
        assert_eq!(Evidence::ALL[0], Evidence::MEASURED);
        for (i, e) in Evidence::ALL.iter().enumerate() {
            assert_eq!(e.index(), i);
            assert_eq!(e.any(), i != 0);
        }
        let set: BTreeSet<Evidence> = Evidence::ALL.into_iter().collect();
        assert_eq!(set.len(), 8);
    }
}
