//! Installation and operational qualification records, status computation,
//! `validate.show_record@1` (`QUAL.IQOQ.RECORD`) and golden-journal registration
//! `validate.register_golden@1` (`QUAL.IQOQ.GOLDEN`).
//!
//! - [`record`]: Every IQ (`validate.run_iq@1`) and OQ (`validate.run_oq@1`) run commits
//!   exactly one journal event whose body is a [`record::QualificationRecord`] (schema
//!   `qual-record/1`) and whose event result digest equals the SHA-256 of that record's
//!   RFC 8785 canonical JSON. [`record::command_defs`] registers `validate.show_record@1`
//!   (R0, D0, exposed on CLI and MCP).
//! - [`golden`]: Registers committed branch entry ranges as golden journals for upgrade OQ
//!   via `validate.register_golden@1` (W2, D0, `human_only: true`, MCP `request_only`),
//!   computing `expected_digest` as SHA-256 of the canonical JSON array of `ResultRecord`
//!   digests and supporting superseding retirement and [`golden::GoldenJournal::active_goldens`].

pub mod golden;
pub mod record;

pub use golden::{
    build_register_golden_entry, compute_expected_digest, deterministic_uuid_v7, register_golden, uuid_v7_from_parts,
    Golden, GoldenError, GoldenJournal, GoldenJournalEvent, ProjectEntry, RegisterGoldenCall, Untrusted,
    CLI_REGISTER_GOLDEN, ERR_GOLDEN_EMPTY, ERR_GOLDEN_UNKNOWN, ERR_POLICY_REFUSED, GOLDEN_SCHEMA, MCP_REGISTER_GOLDEN,
    REGISTER_GOLDEN_COMMAND, REGISTER_GOLDEN_ID, REGISTER_GOLDEN_MAJOR,
};
pub use record::{
    build_journal_entry, command_defs, execute, extract_from_entry, invoke_cli, invoke_mcp, run, ActivityEntry,
    CheckResult, CliOutput, GoldenExceedance, GoldenOutcome, GoldenResult, JournalEvent, JournalMeta, QualError,
    QualificationEvent, QualificationRecord, QualificationStatus, RecordJournal, RecordKind, ShowRecordCall,
    StatusCode, Surface, Verdict, CLI_SHOW_RECORD, ERR_RECORD_NOT_FOUND, MCP_SHOW_RECORD, RECORD_SCHEMA,
    RUN_IQ_COMMAND, RUN_IQ_ID, RUN_IQ_MAJOR, RUN_OQ_COMMAND, RUN_OQ_ID, RUN_OQ_MAJOR, SHOW_RECORD_COMMAND,
    SHOW_RECORD_ID,
};

/// Returns every registry command definition exported by `fid-qual`
/// (`validate.show_record@1` and `validate.register_golden@1`).
pub fn all_command_defs() -> Result<Vec<fid_registry::CommandDef>, fid_registry::DefError> {
    let mut defs = record::command_defs()?;
    defs.extend(golden::command_defs()?);
    Ok(defs)
}
