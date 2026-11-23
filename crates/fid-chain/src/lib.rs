//! Verifier-side journal chain checks of Metrology Maven (spec subsystem `TRUST.CHAIN`).
//!
//! Modules:
//! - [`links`]: streaming journal chain walker, first-entry anchor checks, findings, and the
//!   `verify.check_chain@1` command composing checkpoints and ResultRecord linkage (spec node
//!   `TRUST.CHAIN.LINKS`).
//! - [`ckpt`]: checkpoint parsing, head-digest matching and signed-checkpoint verification
//!   (spec node `TRUST.CHAIN.CKPT`).
//! - [`reslink`]: ResultRecord to journal linkage (`LINKED`, `UNLINKED`, `MISLINKED`, spec
//!   node `TRUST.CHAIN.RESLINK`).
//!
//! All journal entry canonicalisation, parsing and hashing goes strictly through
//! `fid-trust-canon` (REQ-TRUST.CHAIN-007), sharing no writer code with `fid-journal` or
//! `fid-canon`.

pub mod ckpt;
pub mod links;
pub mod reslink;

pub use ckpt::{
    check_checkpoint, check_checkpoints, parse_checkpoint, Checkpoint, CheckpointInput, CheckpointReason,
    CheckpointResult, CheckpointSigResult, CheckpointStatus, CkptError, LedgerHead, SegmentHashes, CHECKPOINTS_PREFIX,
    CHECKPOINT_PAYLOAD_TYPE, FORMAT_LINEAR_V1, MAX_CHECKPOINT_BYTES, MAX_STREAM_BYTES,
};
pub use links::{
    check_chain, check_chain_report, check_package, check_reader, declarations, invoke_cli, invoke_mcp,
    walk_segment_bytes, walk_segment_reader, AnchorCheckpoint, ChainReport, ChainReportView, ChainVerdict,
    ChainWalkResult, CheckChainError, CheckChainParams, Finding, FindingKind, FindingView, MAX_CHECKPOINT_FILES,
    ZERO_DIGEST,
};
pub use reslink::{
    link_package, link_records, link_records_bytes, parse_records, parse_verified_entry_line, parse_verified_segment,
    EntryKind, LinkResult, LinkStatus, MislinkReason, PackageError, PackageMemberReader, ParsedRecord, ReslinkError,
    VerifiedEntry, VerifiedSegment, MAX_ENTRIES, MAX_RECORDS, MAX_RECORDS_BYTES, RECORDS_PATH, SEGMENT_PATH,
};
