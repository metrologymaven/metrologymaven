//! Verifier-side journal chain checks of Metrology Maven (spec subsystem `TRUST.CHAIN`).
//!
//! Implemented so far:
//! - [`ckpt`]: checkpoint parsing, head-digest matching and signed-checkpoint verification
//!   (spec node `TRUST.CHAIN.CKPT`).
//!
//! This crate is CORE-free: it parses and canonicalises checkpoints through `fid-trust-canon`
//! and verifies DSSE checkpoint signatures through `fid-sign-core::troot`, sharing no writer
//! code with `fid-journal` or `fid-canon`.

pub mod ckpt;

pub use ckpt::{
    check_checkpoint, check_checkpoints, parse_checkpoint, Checkpoint, CheckpointInput, CheckpointReason,
    CheckpointResult, CheckpointSigResult, CheckpointStatus, CkptError, LedgerHead, SegmentHashes, CHECKPOINTS_PREFIX,
    CHECKPOINT_PAYLOAD_TYPE, FORMAT_LINEAR_V1, MAX_CHECKPOINT_BYTES, MAX_STREAM_BYTES,
};
