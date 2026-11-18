//! Shared result envelope and closed error-code set. Spec node `AUTO.SURF.ENV`.
//!
//! Every CLI (`--json`) and MCP result is one [`Envelope`], serialised as RFC 8785
//! canonical JSON through `fid-canon`, so people, agents and the verifier read the
//! same bytes. The design names serde; this crate uses `fid-canon`'s typed value
//! model instead so that encoding is canonical by construction.

use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use std::collections::BTreeMap;
use std::fmt;

/// Current envelope version. Removing or renaming an outcome or error code bumps it.
pub const ENVELOPE_VERSION: u16 = 1;
/// Decoder input budget (bytes).
pub const MAX_ENVELOPE_BYTES: usize = 1024 * 1024;
/// Decoder nesting budget.
pub const MAX_ENVELOPE_DEPTH: usize = 64;

macro_rules! string_enum {
    ($(#[$m:meta])* $name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub enum $name {
            $($variant,)+
            /// A value this build does not know; the raw string is kept (REQ-AUTO.SURF.ENV-006).
            Unknown(String),
        }

        impl $name {
            /// Every known value, in declaration order.
            pub const KNOWN: &'static [$name] = &[$($name::$variant),+];

            /// The wire string.
            pub fn as_str(&self) -> &str {
                match self {
                    $($name::$variant => $s,)+
                    $name::Unknown(s) => s,
                }
            }

            /// Decodes a wire string; never fails.
            pub fn from_wire(s: &str) -> Self {
                match s {
                    $($s => $name::$variant,)+
                    other => $name::Unknown(other.to_string()),
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

string_enum!(
    /// What happened to the command (REQ-AUTO.SURF.ENV-002).
    Outcome {
        Committed => "committed",
        Computed => "computed",
        DryRun => "dry_run",
        RequestFiled => "request_filed",
        Accepted => "accepted",
        Failed => "failed",
        Cancelled => "cancelled",
        NotRun => "not_run",
        NoReply => "no_reply",
    }
);

string_enum!(
    /// The closed error-code set. Adding a code is additive.
    ErrorCode {
        UsageError => "usage_error",
        ValidationError => "validation_error",
        UnitError => "unit_error",
        PolicyRefused => "policy_refused",
        PreconditionFailed => "precondition_failed",
        Conflict => "conflict",
        NotFound => "not_found",
        VersionMismatch => "version_mismatch",
        RawPathRefused => "raw_path_refused",
        RootEscape => "root_escape",
        DryRunUnsupported => "dry_run_unsupported",
        Timeout => "timeout",
        Cancelled => "cancelled",
        InternalError => "internal_error",
    }
);

/// The error part of a `failed` envelope.
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
    pub details: Option<CValue>,
}

/// One command result.
#[derive(Debug, Clone, PartialEq)]
pub struct Envelope {
    pub envelope_version: u16,
    /// Registry command ID with major version, e.g. `align.run_321@1`.
    pub command_id: String,
    pub outcome: Outcome,
    pub error: Option<ErrorBody>,
    /// `sha256:<hex>` of the journal entry; present exactly when `committed`.
    pub journal_entry_hash: Option<String>,
    pub job_id: Option<String>,
    pub request_id: Option<String>,
    pub result: Option<CValue>,
}

/// Why an envelope is not valid or could not be decoded. Codes are stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvelopeError {
    MissingJournalEntryHash,
    UnexpectedJournalEntryHash,
    MissingError,
    UnexpectedError,
    MissingJobId,
    MissingRequestId,
    UnknownOutcome(String),
    UnknownErrorCode(String),
    BadCommandId(String),
    BadJournalEntryHash,
    SchemaVersionUnsupported(u64),
    /// A field is missing, has the wrong JSON type, or is not part of envelope v1.
    Shape(String),
    /// The bytes are not acceptable JSON (`fid-canon` error code).
    Json(&'static str),
}

impl EnvelopeError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            EnvelopeError::MissingJournalEntryHash => "missing_journal_entry_hash",
            EnvelopeError::UnexpectedJournalEntryHash => "unexpected_journal_entry_hash",
            EnvelopeError::MissingError => "missing_error",
            EnvelopeError::UnexpectedError => "unexpected_error",
            EnvelopeError::MissingJobId => "missing_job_id",
            EnvelopeError::MissingRequestId => "missing_request_id",
            EnvelopeError::UnknownOutcome(_) => "unknown_outcome",
            EnvelopeError::UnknownErrorCode(_) => "unknown_error_code",
            EnvelopeError::BadCommandId(_) => "bad_command_id",
            EnvelopeError::BadJournalEntryHash => "bad_journal_entry_hash",
            EnvelopeError::SchemaVersionUnsupported(_) => "schema_version_unsupported",
            EnvelopeError::Shape(_) => "bad_shape",
            EnvelopeError::Json(c) => *c,
        }
    }
}

impl fmt::Display for EnvelopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EnvelopeError::UnknownOutcome(s)
            | EnvelopeError::UnknownErrorCode(s)
            | EnvelopeError::BadCommandId(s)
            | EnvelopeError::Shape(s) => write!(f, "{}: {s}", self.code()),
            EnvelopeError::SchemaVersionUnsupported(v) => write!(f, "{}: {v}", self.code()),
            _ => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for EnvelopeError {}

impl Envelope {
    /// A `computed` result (read-only command, nothing journaled).
    pub fn computed(command_id: &str, result: CValue) -> Self {
        Envelope { result: Some(result), ..Self::bare(command_id, Outcome::Computed) }
    }

    /// A `failed` result.
    pub fn failed(command_id: &str, code: ErrorCode, message: &str, details: Option<CValue>) -> Self {
        Envelope {
            error: Some(ErrorBody { code, message: message.to_string(), details }),
            ..Self::bare(command_id, Outcome::Failed)
        }
    }

    /// An envelope with only the required fields.
    pub fn bare(command_id: &str, outcome: Outcome) -> Self {
        Envelope {
            envelope_version: ENVELOPE_VERSION,
            command_id: command_id.to_string(),
            outcome,
            error: None,
            journal_entry_hash: None,
            job_id: None,
            request_id: None,
            result: None,
        }
    }

    /// Checks every envelope invariant; every encoder calls this.
    pub fn validate(&self) -> Result<(), EnvelopeError> {
        if self.envelope_version != ENVELOPE_VERSION {
            return Err(EnvelopeError::SchemaVersionUnsupported(u64::from(self.envelope_version)));
        }
        if !valid_command_id(&self.command_id) {
            return Err(EnvelopeError::BadCommandId(self.command_id.clone()));
        }
        if let Outcome::Unknown(s) = &self.outcome {
            return Err(EnvelopeError::UnknownOutcome(s.clone()));
        }
        let committed = self.outcome == Outcome::Committed;
        match (&self.journal_entry_hash, committed) {
            (None, true) => return Err(EnvelopeError::MissingJournalEntryHash),
            (Some(_), false) => return Err(EnvelopeError::UnexpectedJournalEntryHash),
            (Some(h), true) if !valid_hash(h) => return Err(EnvelopeError::BadJournalEntryHash),
            _ => {}
        }
        let failed = self.outcome == Outcome::Failed;
        match (&self.error, failed) {
            (None, true) => return Err(EnvelopeError::MissingError),
            (Some(_), false) => return Err(EnvelopeError::UnexpectedError),
            (Some(ErrorBody { code: ErrorCode::Unknown(c), .. }), true) => {
                return Err(EnvelopeError::UnknownErrorCode(c.clone()))
            }
            _ => {}
        }
        if self.outcome == Outcome::Accepted && !matches!(self.job_id.as_deref(), Some(j) if !j.is_empty()) {
            return Err(EnvelopeError::MissingJobId);
        }
        if self.outcome == Outcome::RequestFiled && !matches!(self.request_id.as_deref(), Some(r) if !r.is_empty()) {
            return Err(EnvelopeError::MissingRequestId);
        }
        Ok(())
    }

    /// The typed JSON value of this envelope (no validation).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("envelope_version".to_string(), CValue::Num(f64::from(self.envelope_version)));
        o.insert("command_id".to_string(), CValue::Str(self.command_id.clone()));
        o.insert("outcome".to_string(), CValue::Str(self.outcome.as_str().to_string()));
        if let Some(e) = &self.error {
            let mut eo = BTreeMap::new();
            eo.insert("code".to_string(), CValue::Str(e.code.as_str().to_string()));
            eo.insert("message".to_string(), CValue::Str(e.message.clone()));
            if let Some(d) = &e.details {
                eo.insert("details".to_string(), d.clone());
            }
            o.insert("error".to_string(), CValue::Obj(eo));
        }
        for (k, v) in [
            ("journal_entry_hash", &self.journal_entry_hash),
            ("job_id", &self.job_id),
            ("request_id", &self.request_id),
        ] {
            if let Some(v) = v {
                o.insert(k.to_string(), CValue::Str(v.clone()));
            }
        }
        if let Some(r) = &self.result {
            o.insert("result".to_string(), r.clone());
        }
        CValue::Obj(o)
    }

    /// Validates, then encodes as RFC 8785 canonical JSON (REQ-AUTO.SURF.ENV-005).
    pub fn encode(&self) -> Result<Vec<u8>, EnvelopeError> {
        self.validate()?;
        to_canonical_bytes(&self.to_value()).map_err(|e| EnvelopeError::Json(e.code()))
    }

    /// Decodes bytes into an envelope without enforcing the outcome invariants
    /// (call [`Envelope::validate`] for that). Unknown outcome and error-code strings
    /// decode to `Unknown` and never fail (REQ-AUTO.SURF.ENV-006).
    pub fn decode(bytes: &[u8]) -> Result<Envelope, EnvelopeError> {
        let v = parse_with_limits(bytes, MAX_ENVELOPE_BYTES, MAX_ENVELOPE_DEPTH)
            .map_err(|e| EnvelopeError::Json(e.code()))?;
        let CValue::Obj(mut o) = v else {
            return Err(EnvelopeError::Shape("envelope must be a JSON object".into()));
        };
        let version = match o.remove("envelope_version") {
            Some(CValue::Num(n)) if n.fract() == 0.0 && (0.0..=65535.0).contains(&n) => n as u16,
            Some(CValue::Num(n)) if n.fract() == 0.0 && n > 65535.0 && n < 1e15 => {
                return Err(EnvelopeError::SchemaVersionUnsupported(n as u64))
            }
            _ => return Err(EnvelopeError::Shape("envelope_version must be a small integer".into())),
        };
        if version != ENVELOPE_VERSION {
            return Err(EnvelopeError::SchemaVersionUnsupported(u64::from(version)));
        }
        let command_id = take_str(&mut o, "command_id")?.ok_or_else(|| missing("command_id"))?;
        let outcome = Outcome::from_wire(&take_str(&mut o, "outcome")?.ok_or_else(|| missing("outcome"))?);
        let error = match o.remove("error") {
            None => None,
            Some(CValue::Obj(mut eo)) => {
                let code = ErrorCode::from_wire(&take_str(&mut eo, "code")?.ok_or_else(|| missing("error.code"))?);
                let message = take_str(&mut eo, "message")?.ok_or_else(|| missing("error.message"))?;
                let details = eo.remove("details");
                if let Some(k) = eo.keys().next() {
                    return Err(EnvelopeError::Shape(format!("unknown field error.{k}")));
                }
                Some(ErrorBody { code, message, details })
            }
            Some(_) => return Err(EnvelopeError::Shape("error must be an object".into())),
        };
        let journal_entry_hash = take_str(&mut o, "journal_entry_hash")?;
        let job_id = take_str(&mut o, "job_id")?;
        let request_id = take_str(&mut o, "request_id")?;
        let result = o.remove("result");
        if let Some(k) = o.keys().next() {
            return Err(EnvelopeError::Shape(format!("unknown field {k}")));
        }
        Ok(Envelope {
            envelope_version: version,
            command_id,
            outcome,
            error,
            journal_entry_hash,
            job_id,
            request_id,
            result,
        })
    }
}

fn missing(field: &str) -> EnvelopeError {
    EnvelopeError::Shape(format!("missing field {field}"))
}

fn take_str(o: &mut BTreeMap<String, CValue>, k: &str) -> Result<Option<String>, EnvelopeError> {
    match o.remove(k) {
        None => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s)),
        Some(_) => Err(EnvelopeError::Shape(format!("{k} must be a string"))),
    }
}

/// `name@major`, name in `[a-z0-9_.]`, starting and ending with an alphanumeric.
pub fn valid_command_id(s: &str) -> bool {
    let Some((name, major)) = s.rsplit_once('@') else { return false };
    let name_ok = !name.is_empty()
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
        && name.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && name.bytes().last().is_some_and(|b| b.is_ascii_alphanumeric());
    let major_ok = !major.is_empty() && major.len() <= 5 && major.bytes().all(|b| b.is_ascii_digit());
    name_ok && major_ok
}

fn valid_hash(s: &str) -> bool {
    s.strip_prefix("sha256:")
        .is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

// ------------------------------------------------------------------ schemas

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|x| CValue::Str((*x).to_string())).collect())
}

fn when(outcome: &str, field: &str, forbid_otherwise: bool) -> CValue {
    let cond = obj([("properties", obj([("outcome", obj([("const", s(outcome))]))]))]);
    let then = obj([("required", strs(&[field]))]);
    if forbid_otherwise {
        obj([("if", cond), ("then", then), ("else", obj([("not", obj([("required", strs(&[field]))]))]))])
    } else {
        obj([("if", cond), ("then", then)])
    }
}

/// JSON Schema (2020-12) for envelope v1; checked in as `schema/envelope-v1.json`.
pub fn envelope_schema() -> CValue {
    let outcomes: Vec<&str> = Outcome::KNOWN.iter().map(Outcome::as_str).collect();
    obj([
        ("$schema", s("https://json-schema.org/draft/2020-12/schema")),
        ("$id", s("https://metrologymaven.io/schema/envelope-v1.json")),
        ("title", s("Metrology Maven result envelope, version 1")),
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["command_id", "envelope_version", "outcome"])),
        (
            "properties",
            obj([
                ("envelope_version", obj([("const", CValue::Num(f64::from(ENVELOPE_VERSION)))])),
                ("command_id", obj([("type", s("string")), ("pattern", s("^[a-z0-9][a-z0-9_.]*[a-z0-9]@[0-9]{1,5}$"))])),
                ("outcome", obj([("enum", strs(&outcomes))])),
                (
                    "error",
                    obj([
                        ("type", s("object")),
                        ("additionalProperties", CValue::Bool(false)),
                        ("required", strs(&["code", "message"])),
                        (
                            "properties",
                            obj([
                                ("code", obj([("type", s("string"))])),
                                ("message", obj([("type", s("string"))])),
                                ("details", obj([])),
                            ]),
                        ),
                    ]),
                ),
                ("journal_entry_hash", obj([("type", s("string")), ("pattern", s("^sha256:[0-9a-f]{64}$"))])),
                ("job_id", obj([("type", s("string")), ("minLength", CValue::Num(1.0))])),
                ("request_id", obj([("type", s("string")), ("minLength", CValue::Num(1.0))])),
                ("result", obj([])),
            ]),
        ),
        (
            "allOf",
            CValue::Arr(vec![
                when("committed", "journal_entry_hash", true),
                when("failed", "error", true),
                when("accepted", "job_id", false),
                when("request_filed", "request_id", false),
            ]),
        ),
    ])
}

/// Every known error code exactly once, in byte order; checked in as `schema/error-codes.json`.
pub fn error_codes() -> CValue {
    let mut codes: Vec<&str> = ErrorCode::KNOWN.iter().map(ErrorCode::as_str).collect();
    codes.sort_unstable();
    obj([("envelope_version", CValue::Num(f64::from(ENVELOPE_VERSION))), ("error_codes", strs(&codes))])
}
