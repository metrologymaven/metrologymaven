//! Batched balloon and characteristic proposal command (`RPT.BALLOON.AUTO.PROPOSE`).
//!
//! - `report.propose_balloons@1` (`W1`, D0): runs `auto_text` + `auto_parse`, optionally merges
//!   validated untrusted OCR candidates, computes border drawing zones and collision-free balloon
//!   label offsets, and writes a [`BalloonProposalBatch`] onto a draft/agent branch without
//!   mutating the working document.
//! - `adopt_proposal_batch` (`H2` -> `W2`): human-only atomic batch adoption calling
//!   `report.add_char_balloon@1` per accepted item while preserving sheet bounding-box provenance.

pub mod engine;
pub mod model;

pub use engine::{command_def, replay_propose_exact, ProposalBranchStore, PROPOSE_BALLOONS};
pub use model::{
    AdoptBatchResult, AdoptDecision, AdoptedProvenance, BalloonProposal, BalloonProposalBatch, Diagnostic, OcrTextRun,
    ProposalOrigin, ProposeBalloonsParams, ProposeError, ZoneGridSpec, DIAG_RASTER_NEEDS_OCR, MAX_OCR_CANDIDATES,
};
