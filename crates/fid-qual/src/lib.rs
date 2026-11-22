//! Installation and operational qualification records, status computation and the
//! `validate.show_record@1` command (spec node `QUAL.IQOQ.RECORD`).
//!
//! Every IQ (`validate.run_iq@1`) and OQ (`validate.run_oq@1`) run commits exactly one
//! journal event whose body is a [`record::QualificationRecord`] (schema `qual-record/1`)
//! and whose event result digest equals the SHA-256 of that record's RFC 8785 canonical
//! JSON. [`record::command_defs`] registers `validate.show_record@1` (R0, D0, exposed on
//! CLI and MCP), which reads the Record Journal without writing to it and logs calls to the
//! Activity Log only (D-031).

pub mod record;

pub use record::{
    build_journal_entry, command_defs, execute, extract_from_entry, invoke_cli, invoke_mcp, run, ActivityEntry,
    CheckResult, CliOutput, GoldenExceedance, GoldenOutcome, GoldenResult, JournalEvent, JournalMeta, QualError,
    QualificationEvent, QualificationRecord, QualificationStatus, RecordJournal, RecordKind, ShowRecordCall,
    StatusCode, Surface, Verdict, CLI_SHOW_RECORD, ERR_RECORD_NOT_FOUND, MCP_SHOW_RECORD, RECORD_SCHEMA,
    RUN_IQ_COMMAND, RUN_IQ_ID, RUN_IQ_MAJOR, RUN_OQ_COMMAND, RUN_OQ_ID, RUN_OQ_MAJOR, SHOW_RECORD_COMMAND,
    SHOW_RECORD_ID,
};
