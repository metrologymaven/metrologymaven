//! The semantic journal of Metrology Maven (spec node `CORE.JRN`): an append-only,
//! hash-chained record of commands (CONSTITUTION §6). Modules land one spec leaf at a
//! time; see `spec/core/jrn/`.

pub mod entry;

pub use entry::{
    build, commitment, parse_digest, session_start, validate_header_value, verify_commitment, verify_header_value,
    AiAssistance, CommandDraft, CommandHeader, CommitError, ConfirmMethod, DraftBody, Entry, EntryDraft, EntryError,
    EntryHeader, EntryKind, EntryPayload, EntryTime, Field, HeadMove, HeaderBody, Manifests, OsSalts, OutputDigest,
    PrincipalCommit, PrincipalDraft, PrincipalKind, RecordedSalts, Salt, SaltSource, SessionInfo, TaggedDigest,
    UpgradeDraft, UpgradeHeader, R1_RESULT_TAG, SALT_BYTES, SCHEMA_VERSION, TOMBSTONE,
};
