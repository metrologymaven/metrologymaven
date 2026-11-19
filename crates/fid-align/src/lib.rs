//! The alignment element of Metrology Maven. Spec node `ALN.MODEL.ELEM`
//! (`spec/aln/model/elem/NODE.md`); every choice below is pinned in that node's Design.
//!
//! - [`element`]: the [`Alignment`] type, its RFC 8785 canonical form, strict decoding and
//!   its content digest (REQ-001, REQ-003).
//! - [`solver`]: the [`AlignmentSolver`] contract and the sorted [`SolverRegistry`] that
//!   recompute looks solvers up in, by exact algorithm ID and version (REQ-007).
//! - [`model`]: [`AlignmentModel`], the wrapper that turns a solver output into an element,
//!   registers its dependency edges, and recomputes alignments in place (REQ-002, REQ-004 to
//!   REQ-007).
//! - [`command`]: `align.list_alignments@1` and `align.get_alignment@1` (R0), with one handler
//!   behind the CLI and MCP forms so both return the same bytes (REQ-008).
//! - [`residuals`]: `align.get_residuals@1` (R0, spec node `ALN.QA.REPORT`), the paged read of
//!   an alignment's [`ResidualReport`] (built in the kernel crate `fid-align-solve`), and the
//!   [`ResidualStore`] that keeps reports by the ID an alignment's `report` reference names.
//!
//! Nothing here panics; every input is bounded and every refusal is a typed [`AlignError`].
//! No floating-point arithmetic happens in this crate: values are copied bit for bit.

pub mod command;
pub mod element;
mod error;
pub mod model;
pub mod residuals;
pub mod solver;

pub use command::{
    command_defs, get_alignment, invoke_cli, invoke_cli_with, invoke_mcp, invoke_mcp_with, list_alignments,
    ActivityEntry, GetParams, ListParams, Surface, CLI_GET, CLI_LIST, GET_ALIGNMENT, LIST_ALIGNMENTS, MAX_PARAMS_BYTES,
    MAX_PARAMS_DEPTH, MCP_GET, MCP_LIST,
};
pub use element::{
    Alignment, AlignmentStatus, ElementRef, InputRef, ParamSet, ParentRef, PrincipalKind, Provenance, ProvenanceState,
    ReportRef, MAX_ELEMENT_BYTES, MAX_INPUTS, MAX_PARAMS, MAX_PARAM_NAME, MAX_PRINCIPAL_BYTES,
};
pub use error::{AlignError, MAX_CODE_BYTES};
pub use model::{
    AlignmentModel, CreateRequest, Created, ElementLookup, RecomputeEntry, RecomputeItem, MAX_ALIGNMENTS,
    MAX_RECOMPUTE_NODES,
};
pub use residuals::{
    get_residuals, ResidualStore, ResidualsParams, CLI_RESIDUALS, DEFAULT_PAGE, GET_RESIDUALS, MAX_PAGE, MCP_RESIDUALS,
};
pub use solver::{AlignmentSolver, SolveStatus, SolverError, SolverOutput, SolverRegistry, MAX_SOLVERS};

/// The residual report and its builder (kernel crate `fid-align-solve`, ALN.QA.REPORT).
pub use fid_align_solve::{
    Disposition, Entry, EntryKind, EntryRef, RejectReason, ReportBuilder, ReportError, ResidualReport, ResidualStats,
};
/// The frame transform type every alignment stores (GEO.FRAME.XFORM).
pub use fid_geo_frame::Transform;
/// The algorithm ID type (`module.feature.method@major.minor.patch`), shared with the kernel.
pub use fid_kci::AlgorithmId;
