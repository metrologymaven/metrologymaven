//! Block binding and resolution of stable IDs against a journal head. Spec node
//! `RPT.MODEL.BIND.RESOLVE`.
//!
//! Two halves of one contract:
//!
//! - [`bind_block`] (`report.bind_block@1`, W2) stores in a block only the stable IDs it shows
//!   ([`Binding`] of [`Ref`]s, under the payload key `binding`) and, for each bound
//!   ResultRecord, one `value_binding@1` ([`ValueBinding`]) per value field: record ID, record
//!   digest, field pointer and statement digest. No type here has a field that can hold a
//!   value, and both the command and resolution refuse a value anywhere outside the ID
//!   fields of a payload ([`find_value`]: a JSON number, number text with an optional unit,
//!   or a bit pattern; `report.binding_has_value`), so no value is ever copied into a block.
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
//! Out of scope: repetition and multi-stage expansion (`RPT.MODEL.BIND.EXPAND`), the
//! resolved document and its digest (`RPT.MODEL.BIND.GEN`), formatting (`RPT.MODEL.FMT`)
//! and the real kinds' payloads and resolvers (`RPT.MODEL.BLOCKS.*`, `RPT.FORMS`,
//! `RPT.BALLOON`).

pub mod command;
pub mod error;
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
pub use journal::{
    AtHead, EntityKind, EntityView, Evidence, EvidenceClass, HeadLookup, JournalReads, Journaled, Lookup, LookupFault,
    MemoryJournal, Outcome, ResultView, StatementBytes, ValueField, RECORD_VALUE_FIELD, RESULT_RECORD_SCHEMA,
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
