//! The semantic journal of Metrology Maven (spec node `CORE.JRN`): an append-only,
//! hash-chained record of commands (CONSTITUTION §6). Modules land one spec leaf at a
//! time; see `spec/core/jrn/`.
//!
//! Feature `store` adds module `append` (`CORE.JRN.CHAIN.APPEND`), which writes through
//! `fid-store`; without it the crate is the storage-free entry model.

#[cfg(feature = "store")]
pub mod append;
pub mod entry;

#[cfg(feature = "test-hooks")]
pub use append::FaultHook;
#[cfg(feature = "store")]
pub use append::{
    utc_order, Append, AppendRequest, CrashPoint, Durability, EntryHash, Journal, JournalError, Tail, MAX_BATCH,
    MAX_BLOBS, MAX_HEADER_BYTES, MAX_QUEUE, ROLE_PAYLOAD, ROLE_REF, SYNCHRONOUS_FULL,
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
