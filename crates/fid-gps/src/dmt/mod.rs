//! The Dialect Mapping Table schema (spec node `GDT.RES.SCHEMA`, D-009).
//!
//! - [`ident`]: the row key grammar `dmt:<dialect>/<characteristic>/<modifiers|none>@<v>`,
//!   dialects, versions, algorithm IDs and the closed ISO 17450 operator vocabulary.
//! - [`row`]: [`DmtRow`], [`ClauseRef`], [`OperatorChain`], [`Operator`], [`Param`],
//!   [`RowStatus`], [`ReviewState`], their strict parser and canonical form.
//! - [`table`]: the versioned container [`DmtTable`] (`fid.dmt-table/1`), its bounded loader
//!   and the table digest.
//! - [`profile_keys`]: the defaults-profile key vocabulary read by `GDT.CHAR.DPROF`.
//! - [`DMT_TABLE_JSON_SCHEMA`]: the exported JSON Schema 2020-12 of a table file.
//!
//! Digests (row, chain, table) are SHA-256 over RFC 8785 canonical JSON. No JSON number is
//! accepted anywhere in a table; floating-point parameters are IEEE-754 hex strings, so
//! parsing and hashing involve no floating-point arithmetic (D0). The schema holds structure
//! only, never interpretation: rows are authored by experts against it.

pub mod ident;
pub mod profile_keys;
pub mod row;
pub mod table;

pub use ident::{AlgId, Dialect, DmtKey, OperatorKind, SemVer, MAX_SEMVER_PART};
pub use profile_keys::{profile_vocabulary, ProfileKeyRule, NAMED_PROFILE_KEYS, PROFILE_KEYS_SCHEMA};
pub use row::{ClauseRef, DmtRow, Operator, OperatorChain, Param, ParamKind, ReviewState, Reviewers, RowStatus};
pub use table::{DmtTable, TABLE_SCHEMA};

use std::fmt;

/// The JSON Schema (draft 2020-12) of a `fid.dmt-table/1` file. It checks structure, field
/// sets and grammars; the loader is stricter (sorted sets, key agreement, unique keys).
pub const DMT_TABLE_JSON_SCHEMA: &str = include_str!("../../schema/dmt-table-v1.schema.json");

/// Longest text echoed back inside an error, in bytes.
const MAX_ECHO: usize = 160;

fn echo(s: &str) -> String {
    let mut end = s.len().min(MAX_ECHO);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

/// Errors of the DMT schema. [`DmtError::code`] is the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DmtError {
    /// `bad_dmt_key{key}`: the text is not a canonical row key (REQ-005).
    BadDmtKey {
        /// The text (truncated).
        key: String,
    },
    /// `duplicate_dmt_key{key}`: two rows share a key (REQ-006).
    DuplicateDmtKey {
        /// The key.
        key: String,
    },
    /// `unknown_operator_kind{kind}`: not in the closed ISO 17450 vocabulary.
    UnknownOperatorKind {
        /// The text (truncated).
        kind: String,
    },
    /// `bad_algorithm_id{alg}`: not `<module>.<feature>.<method>@<x.y.z>`.
    BadAlgorithmId {
        /// The text (truncated).
        alg: String,
    },
    /// `field_too_long{field}`: a string over its field's limit (free text: 280 characters).
    FieldTooLong {
        /// The field name.
        field: &'static str,
    },
    /// `malformed_table{reason}`: anything else that breaks the format or a limit.
    Malformed {
        /// Static reason.
        reason: &'static str,
    },
}

impl DmtError {
    /// The stable code.
    pub fn code(&self) -> &'static str {
        match self {
            DmtError::BadDmtKey { .. } => "bad_dmt_key",
            DmtError::DuplicateDmtKey { .. } => "duplicate_dmt_key",
            DmtError::UnknownOperatorKind { .. } => "unknown_operator_kind",
            DmtError::BadAlgorithmId { .. } => "bad_algorithm_id",
            DmtError::FieldTooLong { .. } => "field_too_long",
            DmtError::Malformed { .. } => "malformed_table",
        }
    }

    /// The static reason or field name, where there is one.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            DmtError::FieldTooLong { field: r } | DmtError::Malformed { reason: r } => Some(r),
            _ => None,
        }
    }

    pub(crate) fn malformed(reason: &'static str) -> DmtError {
        DmtError::Malformed { reason }
    }

    pub(crate) fn too_long(field: &'static str) -> DmtError {
        DmtError::FieldTooLong { field }
    }

    pub(crate) fn bad_key(s: &str) -> DmtError {
        DmtError::BadDmtKey { key: echo(s) }
    }

    pub(crate) fn duplicate_key(s: &str) -> DmtError {
        DmtError::DuplicateDmtKey { key: echo(s) }
    }

    pub(crate) fn unknown_kind(s: &str) -> DmtError {
        DmtError::UnknownOperatorKind { kind: echo(s) }
    }

    pub(crate) fn bad_alg(s: &str) -> DmtError {
        DmtError::BadAlgorithmId { alg: echo(s) }
    }
}

impl fmt::Display for DmtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DmtError::BadDmtKey { key: s }
            | DmtError::DuplicateDmtKey { key: s }
            | DmtError::UnknownOperatorKind { kind: s }
            | DmtError::BadAlgorithmId { alg: s } => write!(f, "{}{{{}}}", self.code(), s.escape_debug()),
            other => match other.reason() {
                Some(r) => write!(f, "{}{{{r}}}", other.code()),
                None => f.write_str(other.code()),
            },
        }
    }
}

impl std::error::Error for DmtError {}
