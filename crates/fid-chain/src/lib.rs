//! Verifier-side journal chain checks of Metrology Maven (spec subsystem `TRUST.CHAIN`).
//!
//! Implemented so far:
//! - [`ckpt`]: checkpoint parsing, head-digest matching and signed-checkpoint verification
//!   (spec node `TRUST.CHAIN.CKPT`).
//! - [`reslink`]: ResultRecord to journal linkage (`LINKED`, `UNLINKED`, `MISLINKED`, spec
//!   node `TRUST.CHAIN.RESLINK`).
//!
//! This crate is CORE-free: it parses and canonicalises checkpoints and records through
//! `fid-trust-canon` and verifies DSSE checkpoint signatures through `fid-sign-core::troot`,
//! sharing no writer code with `fid-journal` or `fid-canon`.

pub mod ckpt;
pub mod reslink;

pub use ckpt::{
    check_checkpoint, check_checkpoints, parse_checkpoint, Checkpoint, CheckpointInput, CheckpointReason,
    CheckpointResult, CheckpointSigResult, CheckpointStatus, CkptError, LedgerHead, SegmentHashes, CHECKPOINTS_PREFIX,
    CHECKPOINT_PAYLOAD_TYPE, FORMAT_LINEAR_V1, MAX_CHECKPOINT_BYTES, MAX_STREAM_BYTES,
};
pub use reslink::{
    link_package, link_records, link_records_bytes, parse_records, parse_verified_entry_line, parse_verified_segment,
    EntryKind, LinkResult, LinkStatus, MislinkReason, PackageError, PackageMemberReader, ParsedRecord, ReslinkError,
    VerifiedEntry, VerifiedSegment, MAX_ENTRIES, MAX_RECORDS, MAX_RECORDS_BYTES, RECORDS_PATH, SEGMENT_PATH,
};
