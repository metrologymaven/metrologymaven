//! The semantic journal of Metrology Maven (spec node `CORE.JRN`): an append-only,
//! hash-chained record of commands (CONSTITUTION §6). Modules land one spec leaf at a
//! time; see `spec/core/jrn/`.
//!
//! Feature `store` adds module `append` (`CORE.JRN.CHAIN.APPEND`), which writes through
//! `fid-store`, and the head cache and checkpoint table of module `checkpoint`
//! (`CORE.JRN.CHAIN.CKPT`); without it the crate is the storage-free entry model plus the
//! checkpoint record and its verifier.
//!
//! Module `branch` (`CORE.JRN.BR.NAME`) is the branch table: name rules, the branch events and
//! their projection, and the planners of `journal.create_branch@1`, `journal.list_branches@1`,
//! `journal.protect_branch@1` and `journal.set_branch_coediting@1`; with feature `store` it
//! also declares those commands. The bus handlers are in `fid-journal-cmd`.
//!
//! Module `verify` (`CORE.JRN.CHAIN.VERIFY`) verifies the hash chain, blobs and checkpoints of
//! stored rows and lists entry headers; with feature `store` it reads them from the project
//! database and declares `journal.verify_chain@1` and `journal.list_entries@1`.
//!
//! Module `diff` (`CORE.JRN.BR.DIFF`) diffs two branches entity by entity from verified entry
//! headers (`journal.diff_branch@1`); with feature `store` it reads them from the project
//! database and declares the command. The bus handler is in `fid-journal-cmd`.
//!
//! Module `promote` (`CORE.JRN.BR.PROMO`) is the person/agent gate: the planner and the
//! origin-bound fold of `journal.promote_branch@1`, `journal.promote_protected_branch@1` and
//! `journal.request_promote_branch@1`, the `Rebaser` and `AssertionCheck` traits, and with
//! feature `store` the three definitions. The bus handlers are in `fid-journal-cmd`.
//!
//! Module `supersede` (`CORE.JRN.UNDO.SUPER`) is the supersession of released records: the
//! planner and the origin-bound fold of `journal.supersede_record@1` (the released-record and
//! `superseded_by` indexes), and with feature `store` its definition. The bus handler is in
//! `fid-journal-cmd`.
//!
//! Module `stream` (`CORE.JRN.STREAM.ALOG`) routes completed calls to their stream and writes
//! the P0 Activity Log; with feature `store` it also declares the Activity Log settings.

#[cfg(feature = "store")]
pub mod append;
pub mod branch;
pub mod checkpoint;
pub mod diff;
pub mod entry;
pub mod promote;
pub mod stream;
pub mod supersede;
pub mod verify;

#[cfg(feature = "store")]
pub use checkpoint::{BranchHead, HeadsRebuilt, StoredCheckpoint};
pub use checkpoint::{Checkpoint, CheckpointError, Coverage, LedgerHead, Link, Verified};
pub use verify::{
    check_heads, list_rows, plan_list, resolve_start, verify_rows, Anchor, BlobSource, BlobState, Checked,
    CheckpointRow, Failure, FailureKind, ListPage, ListPlan, ListQuery, ListedEntry, ReadError, Start, StoredEntry,
    TrustedHead, VerifyOptions, VerifyReport,
};
#[cfg(feature = "store")]
pub use verify::{command_defs, list_store, verify_store};

#[cfg(feature = "test-hooks")]
pub use append::FaultHook;
#[cfg(feature = "store")]
pub use append::{
    utc_instant, utc_order, Append, AppendRequest, Clock, CrashPoint, Durability, EntryHash, Journal, JournalError,
    SystemClock, Tail, MAX_AHEAD_SECS, MAX_BATCH, MAX_BATCH_BYTES, MAX_BLOBS, MAX_HEADER_BYTES, MAX_QUEUE,
    ROLE_PAYLOAD, ROLE_REF, SYNCHRONOUS_FULL,
};

pub use entry::{
    build, commitment, parse_digest, session_start, validate_header_value, verify_commitment, verify_header_value,
    AiAssistance, CommandDraft, CommandHeader, CommitError, ConfirmMethod, DraftBody, Entry, EntryDraft, EntryError,
    EntryHeader, EntryKind, EntryPayload, EntryTime, Field, HeadMove, HeaderBody, Manifests, ObjectShape, OsSalts,
    OutputDigest, PrincipalCommit, PrincipalDraft, PrincipalKind, RecordedSalts, Salt, SaltSource, SessionInfo,
    TaggedDigest, UpgradeDraft, UpgradeHeader, ALGORITHM_PATTERN, BODY_KEYS, BRANCH_PATTERN, COMMAND_ID_PATTERN,
    DIGEST_PATTERN, FIRST_SEQ, GENESIS_PREV, HEADER_SHAPE, IDENT_PATTERN, R1_RESULT_TAG, SALT_BYTES, SCHEMA_VERSION,
    SEQ_PATTERN, SHAPES, TOMBSTONE, U64_PATTERN, UTC_PATTERN,
};
