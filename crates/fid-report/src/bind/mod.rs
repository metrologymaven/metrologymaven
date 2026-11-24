//! Block binding and resolution of stable IDs against a journal head. Spec node
//! `RPT.MODEL.BIND.RESOLVE`.
//!
//! Two halves of one contract:
//!
//! - [`bind_block`] (`report.bind_block@1`, W2) stores in a block only the stable IDs it shows
//!   ([`Binding`] of [`Ref`]s, under the payload key `binding`) and, for each bound
//!   ResultRecord, one `value_binding@1` ([`ValueBinding`]) per value field: record ID, record
//!   digest, field pointer and statement digest. No type here has a field that can hold a
//!   value, and both the command and resolution refuse a value anywhere in a payload but its
//!   top-level `binding` ([`find_value`]: a JSON number, a bit pattern, and in a bound
//!   block number text with an optional unit; `report.binding_has_value`), so no value is ever copied into a block.
//! - [`resolve_block`] looks every stored ID up as of the explicit [`JournalHead`] of a
//!   [`ResolveContext`] (never "latest"), refuses IDs not journaled by that head
//!   (`report.unjournaled_record`) or owned by another project
//!   (`report.cross_project_binding`, naming only the block and that project), recomputes
//!   both digests of every value binding (`report.binding_digest_mismatch`,
//!   `report.binding_field_not_value`), and hands the kind's [`BlockResolver`] a typed
//!   [`Resolved`] view whose values are [`Quantity`]s (a [`crate::fmt::SiValue`] and a unit)
//!   with the record's [`Evidence`] (basis, decision, claim facts), never floats or text.
//!   The view is built by the engine only; a resolver returns a layout.
//!
//! Block kinds plug in through [`BlockResolver`] and a [`ResolverRegistry`]
//! (`report.no_resolver` for a kind without one); form data through [`FieldSource`]; the
//! project store through [`JournalReads`] ([`MemoryJournal`] in memory), wrapped by the typed
//! [`Lookup`] at one head ([`HeadLookup`]).
//!
//! [`expand`] (module [`expand`](mod@expand), `RPT.MODEL.BIND.EXPAND`) turns a template's slots
//! into expanded blocks with plain bindings at a journal head.
//!
//! Out of scope: the resolved document and its digest (`RPT.MODEL.BIND.GEN`), formatting (`RPT.MODEL.FMT`)
//! and the real kinds' payloads and resolvers (`RPT.MODEL.BLOCKS.*`, `RPT.FORMS`,
//! `RPT.BALLOON`).

pub mod command;
pub mod error;
pub mod expand;
pub mod gen;
pub mod journal;
pub mod model;
pub mod resolve;
#[cfg(feature = "test-kinds")]
pub mod stub;

pub use command::{
    bind_block, declarations, execute_bind, invoke_cli, is_agent_branch, BindBlockParams, BindBlockResult, BindCtx,
    Caller, Principal, RefParam, CLI_BIND, CLI_GROUP, MCP_BIND, REF_ID_PATTERN,
};
pub use error::BindError;
pub use expand::{
    expand, CharacteristicItem, ExpandError, ExpandItem, ExpandedBlock, ItemKind, MemoryModel, ProjectModel,
    QueryProvider, ResultItem, Role, RunItem, StageItem, EXPANSION_SCHEMA, MAX_CELLS, MAX_COLUMNS, MAX_PAGES, MAX_ROWS,
};
pub use gen::{
    default_locale_rules, gen_command_defs, generate, generate_report, invoke_generate_cli, invoke_save_cli,
    reject_embedded_bytes, replay_save_report_exact, save_report, view_block_payload, BlockFrame, FormatterVersion,
    GenContext, GenError, GenMetadata, GenSource, GenWarning, GenerateCmdEnv, GenerateReportOutput,
    GenerateReportParams, GeneratedBlock, MemoryGenMetadata, MemoryResolvedStore, MemoryScratch, Provenance,
    ResolvedPage, ResolvedReport, ResolvedStore, SaveCmdEnv, SaveReceipt, SaveReplayRun, SaveReportEvent,
    SaveReportOutput, SaveReportParams, ScratchStore, UnitOverrideMode, ViewBlockKind, ViewBlockResolver,
    CLI_GENERATE_REPORT, CLI_SAVE_REPORT, DEFAULT_KERNEL_VERSION, DEFAULT_LOCALE_RELEASE, GENERATE_REPORT,
    RESOLVED_REPORT_SCHEMA, RESOLVED_REPORT_SCHEMA_VERSION, RESOLVED_VIEW_SCHEMA, SAVE_REPORT, VIEW_KIND_ID,
    VIEW_SCHEMA, WARN_VIEW_MISSING,
};
pub use journal::{
    AtHead, EntityKind, EntityView, Evidence, EvidenceClass, HeadLookup, JournalReads, Journaled, Lookup, LookupFault,
    MemoryJournal, Outcome, ResultView, StatementBytes, ValueField, RECORD_VALUE_FIELD, RESULT_RECORD_SCHEMA,
    RESULT_RECORD_SCHEMA_V2,
};
pub use model::{
    Binding, FieldId, JournalHead, Quantity, QuantityUnit, Ref, RefKind, ValueBinding, BINDING_KEY, BINDING_SCHEMA,
    MAX_REFS,
};
pub use resolve::{
    find_field, find_value, is_bit_pattern, is_number_text, resolve_block, BlockResolver, FieldSource, FieldValue,
    ResolveContext, Resolved, ResolvedBlock, ResolvedRef, ResolvedValue, ResolverRegistry, RowSource,
};

/// The design's name for resolution errors; one error type serves binding and resolution.
pub type ResolveError = BindError;
