//! The closed bus error code set. Spec node `CORE.BUS.PIPE.ERR`.
//!
//! Every bus failure is one [`BusError`]: a stable [`Code`] from a closed set of twelve,
//! a reason from that code's closed reason enum, and an untrusted [`Detail`] text.
//! It serialises to `{code, reason, detail}` (REQ-004). The codes and reasons are
//! published as `bus-errors.json` (schema version 1) in RFC 8785 canonical JSON with a
//! `sha256:` digest (REQ-002), and [`check_compatible`] fails when a later version drops
//! or renames a code or reason (REQ-003). Codes and reasons are only ever added.

use fid_canon::{digest, parse_with_limits, to_canonical_bytes, CValue, CanonError, Digest};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Schema version of `bus-errors.json`.
pub const SCHEMA_VERSION: u16 = 1;
/// Longest [`Detail`], in Unicode scalar values (characters).
pub const MAX_DETAIL_CHARS: usize = 2000;
/// Input budget when reading a published `bus-errors.json`.
pub const MAX_CATALOG_BYTES: usize = 1024 * 1024;
/// Nesting budget when reading a published `bus-errors.json`.
pub const MAX_CATALOG_DEPTH: usize = 8;

// ------------------------------------------------------------------ untrusted detail

/// A value that came from outside the trust boundary (CONSTITUTION C6). Surfaces may
/// display it but must never parse it or act on it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Untrusted<T>(T);

/// The free-text part of a [`BusError`]: untrusted, at most [`MAX_DETAIL_CHARS`] characters.
pub type Detail = Untrusted<String>;

impl Untrusted<String> {
    /// Wraps `text` as a detail, keeping at most the first [`MAX_DETAIL_CHARS`] characters.
    /// Never fails, so building an error can never itself fail.
    pub fn new(text: &str) -> Self {
        let kept = match text.char_indices().nth(MAX_DETAIL_CHARS) {
            Some((at, _)) => text.get(..at).unwrap_or(text),
            None => text,
        };
        Untrusted(kept.to_string())
    }

    /// An empty detail.
    pub fn empty() -> Self {
        Untrusted(String::new())
    }

    /// The raw untrusted text, for display only.
    pub fn as_untrusted_str(&self) -> &str {
        &self.0
    }
}

// ------------------------------------------------------------------ reasons

macro_rules! reason_enum {
    ($(#[$m:meta])* $name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($variant,)+
        }

        impl $name {
            /// Every reason, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The wire string.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $s,)+
                }
            }

            /// Decodes a wire string; `None` if this build does not know it.
            pub fn from_wire(s: &str) -> Option<Self> {
                match s {
                    $($s => Some($name::$variant),)+
                    _ => None,
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

reason_enum!(
    /// Reasons for `validation` (CORE.BUS.PIPE.VALID, CORE.BUS.UNIT, CORE.BUS.PIPE.HANDLER and others).
    ValidationReason {
        Schema => "schema",
        BadReference => "bad_reference",
        UnitMismatch => "unit_mismatch",
        LiteralOutOfRange => "literal_out_of_range",
        LiteralSyntax => "literal_syntax",
        InputLimit => "input_limit",
        EffectLimit => "effect_limit",
        BadArtifact => "bad_artifact",
        BadBranchName => "bad_branch_name",
        BadStage => "bad_stage",
        Hierarchy => "hierarchy",
        Label => "label",
        Name => "name",
        UnregisteredType => "unregistered_type",
        BadToken => "bad_token",
        StaleToken => "stale_token",
    }
);

reason_enum!(
    /// Reasons for `policy` (CORE.BUS.POL and policy hooks).
    PolicyReason {
        ClassForbidden => "class_forbidden",
        HumanOnly => "human_only",
        NotJobOwner => "not_job_owner",
        NotOnMcp => "not_on_mcp",
        BadApproval => "bad_approval",
        NotRequester => "not_requester",
        RawPathRefused => "raw_path_refused",
        OutsideRoot => "outside_root",
        BadRoot => "bad_root",
        ExtensionRefused => "extension_refused",
        WouldOverwrite => "would_overwrite",
        BadHandle => "bad_handle",
        BadDestination => "bad_destination",
        NoApproval => "no_approval",
        TargetProtected => "target_protected",
        NoAssertion => "no_assertion",
        ProfileAgentWriteForbidden => "profile.agent_write_forbidden",
    }
);

reason_enum!(
    /// Reasons for `precondition` (CORE.BUS.PIPE.VALID and domain preconditions).
    PreconditionReason {
        False => "false",
        StepBoundExceeded => "step_bound_exceeded",
        CapabilityMissing => "capability_missing",
        DivergentStep => "divergent_step",
        NotSupersedable => "not_supersedable",
        NotUndoable => "not_undoable",
        NothingToRedo => "nothing_to_redo",
        ProjectLocked => "project_locked",
        ReleasedRecord => "released_record",
        StageActive => "stage_active",
    }
);

reason_enum!(
    /// Reasons for `conflict` (CORE.BUS.ROUTE, CORE.BUS.PIPE.STAGE).
    ConflictReason {
        ExpectedHeadMismatch => "expected_head_mismatch",
        HeadMoved => "head_moved",
    }
);

reason_enum!(
    /// Reasons for `not_found` (CORE.BUS.PIPE.VALID and lookups by ID).
    NotFoundReason {
        UnknownCommand => "unknown_command",
        UnknownMajor => "unknown_major",
        RemovedMajor => "removed_major",
        Branch => "branch",
        Entity => "entity",
        Job => "job",
        Request => "request",
        Stream => "stream",
    }
);

reason_enum!(
    /// Reasons for `hidden_command` (CORE.BUS.PIPE.VALID).
    HiddenCommandReason {
        HiddenMajor => "hidden_major",
    }
);

reason_enum!(
    /// Reasons for `undeclared_effect` (CORE.BUS.PIPE.HANDLER).
    UndeclaredEffectReason {
        EventType => "event_type",
    }
);

reason_enum!(
    /// Reasons for `cancelled` (CORE.JOB).
    CancelledReason {
        Requested => "requested",
        Shutdown => "shutdown",
    }
);

reason_enum!(
    /// Reasons for `unauthenticated` (CORE.BUS.PRIN).
    UnauthenticatedReason {
        NoIdentity => "no_identity",
    }
);

reason_enum!(
    /// Reasons for `idempotency_mismatch` (CORE.BUS.ROUTE).
    IdempotencyMismatchReason {
        ArgsDigestDiffers => "args_digest_differs",
    }
);

reason_enum!(
    /// Reasons for `upgrade_refused` (CORE.BUS.PRIN).
    UpgradeRefusedReason {
        Channel => "channel",
        Declined => "declined",
        Timeout => "timeout",
        UserMismatch => "user_mismatch",
    }
);

reason_enum!(
    /// Reasons for `internal`: bugs and environment faults, never caller mistakes.
    InternalReason {
        HandlerFailed => "handler_failed",
        JournalUnavailable => "journal_unavailable",
        InvariantViolated => "invariant_violated",
    }
);

// ------------------------------------------------------------------ codes and errors

macro_rules! bus_errors {
    ($($variant:ident => $code:literal, $reason:ident;)+) => {
        /// The closed set of bus error codes (REQ-001). Never renamed or reused.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Code {
            $($variant,)+
        }

        impl Code {
            /// Every code, in declaration order.
            pub const ALL: &'static [Code] = &[$(Code::$variant),+];

            /// The wire string.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Code::$variant => $code,)+
                }
            }

            /// Decodes a wire string; `None` if this build does not know it.
            pub fn from_wire(s: &str) -> Option<Code> {
                match s {
                    $($code => Some(Code::$variant),)+
                    _ => None,
                }
            }

            /// This code's reasons, in declaration order.
            pub fn reasons(self) -> Vec<&'static str> {
                match self {
                    $(Code::$variant => $reason::ALL.iter().map(|r| r.as_str()).collect(),)+
                }
            }
        }

        /// One bus failure: code, closed reason and untrusted detail.
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub enum BusError {
            $($variant($reason, Detail),)+
        }

        impl BusError {
            /// The stable code.
            pub fn code(&self) -> Code {
                match self {
                    $(BusError::$variant(..) => Code::$variant,)+
                }
            }

            /// The reason's wire string.
            pub fn reason(&self) -> &'static str {
                match self {
                    $(BusError::$variant(r, _) => r.as_str(),)+
                }
            }

            /// The untrusted detail text.
            pub fn detail(&self) -> &Detail {
                match self {
                    $(BusError::$variant(_, d) => d,)+
                }
            }

            /// Rebuilds an error from its wire parts; `None` if the code or the reason is
            /// unknown to this build or the reason does not belong to the code.
            pub fn from_wire(code: &str, reason: &str, detail: &str) -> Option<BusError> {
                match Code::from_wire(code)? {
                    $(Code::$variant => Some(BusError::$variant($reason::from_wire(reason)?, Detail::new(detail))),)+
                }
            }
        }
    };
}

bus_errors! {
    Validation => "validation", ValidationReason;
    Policy => "policy", PolicyReason;
    Precondition => "precondition", PreconditionReason;
    Conflict => "conflict", ConflictReason;
    NotFound => "not_found", NotFoundReason;
    HiddenCommand => "hidden_command", HiddenCommandReason;
    UndeclaredEffect => "undeclared_effect", UndeclaredEffectReason;
    Cancelled => "cancelled", CancelledReason;
    Unauthenticated => "unauthenticated", UnauthenticatedReason;
    IdempotencyMismatch => "idempotency_mismatch", IdempotencyMismatchReason;
    UpgradeRefused => "upgrade_refused", UpgradeRefusedReason;
    Internal => "internal", InternalReason;
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl BusError {
    /// The typed JSON value `{code, reason, detail}` (REQ-004).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("code".to_string(), CValue::Str(self.code().as_str().to_string()));
        o.insert("reason".to_string(), CValue::Str(self.reason().to_string()));
        o.insert("detail".to_string(), CValue::Str(self.detail().as_untrusted_str().to_string()));
        CValue::Obj(o)
    }

    /// RFC 8785 canonical JSON of [`BusError::to_value`].
    pub fn to_json(&self) -> Result<Vec<u8>, CanonError> {
        to_canonical_bytes(&self.to_value())
    }
}

impl fmt::Display for BusError {
    /// `code/reason`; the untrusted detail is left out on purpose.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.code(), self.reason())
    }
}

impl std::error::Error for BusError {}

// ------------------------------------------------------------------ published catalog

/// Codes and their reasons, as published: code -> set of reasons.
pub type Catalog = BTreeMap<String, BTreeSet<String>>;

/// The catalog this build defines.
pub fn current_catalog() -> Catalog {
    Code::ALL.iter().map(|c| (c.as_str().to_string(), c.reasons().into_iter().map(str::to_string).collect())).collect()
}

/// `{"codes": {code: [reasons, sorted]}, "schema_version": 1}` for a catalog.
fn catalog_to_value(catalog: &Catalog) -> CValue {
    let codes = catalog
        .iter()
        .map(|(c, rs)| (c.clone(), CValue::Arr(rs.iter().map(|r| CValue::Str(r.clone())).collect())))
        .collect();
    let mut o = BTreeMap::new();
    o.insert("codes".to_string(), CValue::Obj(codes));
    o.insert("schema_version".to_string(), CValue::Num(f64::from(SCHEMA_VERSION)));
    CValue::Obj(o)
}

/// The typed value of `bus-errors.json` for this build.
pub fn catalog_value() -> CValue {
    catalog_to_value(&current_catalog())
}

/// The canonical bytes of `bus-errors.json` (REQ-002), without a trailing newline.
pub fn bus_errors_json() -> Result<Vec<u8>, CanonError> {
    to_canonical_bytes(&catalog_value())
}

/// The `sha256:` digest of [`bus_errors_json`] (REQ-002).
pub fn bus_errors_digest() -> Result<Digest, CanonError> {
    digest(&catalog_value())
}

/// Why a published `bus-errors.json` could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    /// The bytes are not acceptable JSON (`fid-canon` error code).
    Json(&'static str),
    /// `schema_version` is not 1.
    SchemaVersionUnsupported,
    /// Wrong shape: missing or extra field, wrong JSON type, or a duplicate reason.
    Shape(String),
}

impl CatalogError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            CatalogError::Json(c) => c,
            CatalogError::SchemaVersionUnsupported => "schema_version_unsupported",
            CatalogError::Shape(_) => "bad_shape",
        }
    }
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogError::Shape(s) => write!(f, "bad_shape: {s}"),
            other => f.write_str(other.code()),
        }
    }
}

impl std::error::Error for CatalogError {}

/// Reads a published `bus-errors.json` (schema version 1), e.g. a previous version.
pub fn parse_catalog(bytes: &[u8]) -> Result<Catalog, CatalogError> {
    let v = parse_with_limits(bytes, MAX_CATALOG_BYTES, MAX_CATALOG_DEPTH).map_err(|e| CatalogError::Json(e.code()))?;
    let CValue::Obj(mut o) = v else {
        return Err(CatalogError::Shape("bus-errors.json must be an object".into()));
    };
    match o.remove("schema_version") {
        Some(CValue::Num(n)) if n == f64::from(SCHEMA_VERSION) => {}
        Some(CValue::Num(_)) => return Err(CatalogError::SchemaVersionUnsupported),
        _ => return Err(CatalogError::Shape("schema_version must be a number".into())),
    }
    let Some(CValue::Obj(codes)) = o.remove("codes") else {
        return Err(CatalogError::Shape("codes must be an object".into()));
    };
    if let Some(k) = o.keys().next() {
        return Err(CatalogError::Shape(format!("unknown field {k}")));
    }
    let mut out = Catalog::new();
    for (code, reasons) in codes {
        let CValue::Arr(items) = reasons else {
            return Err(CatalogError::Shape(format!("reasons of {code} must be an array")));
        };
        let mut set = BTreeSet::new();
        for item in items {
            let CValue::Str(r) = item else {
                return Err(CatalogError::Shape(format!("reasons of {code} must be strings")));
            };
            if !set.insert(r.clone()) {
                return Err(CatalogError::Shape(format!("duplicate reason {code}/{r}")));
            }
        }
        out.insert(code, set);
    }
    Ok(out)
}

/// A code or reason present in a previous version but missing now (REQ-003).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Removal {
    Code(String),
    Reason { code: String, reason: String },
}

impl fmt::Display for Removal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Removal::Code(c) => write!(f, "code `{c}` was removed or renamed"),
            Removal::Reason { code, reason } => write!(f, "reason `{reason}` of code `{code}` was removed or renamed"),
        }
    }
}

/// Checks that `current` keeps every code and reason of `previous` (REQ-003). Additions
/// are allowed. On failure, returns every removal, codes before reasons, in byte order.
pub fn check_compatible(previous: &Catalog, current: &Catalog) -> Result<(), Vec<Removal>> {
    let mut removed = Vec::new();
    for (code, reasons) in previous {
        match current.get(code) {
            None => removed.push(Removal::Code(code.clone())),
            Some(now) => removed
                .extend(reasons.difference(now).map(|r| Removal::Reason { code: code.clone(), reason: r.clone() })),
        }
    }
    removed.sort();
    if removed.is_empty() {
        Ok(())
    } else {
        Err(removed)
    }
}

// ------------------------------------------------------------------ schema

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn strs<'a>(items: impl IntoIterator<Item = &'a str>) -> CValue {
    CValue::Arr(items.into_iter().map(s).collect())
}

/// JSON Schema (2020-12) for one serialised [`BusError`]; checked in as
/// `schema/bus-error.schema.json`.
pub fn bus_error_schema() -> CValue {
    let catalog = current_catalog();
    let per_code = catalog
        .iter()
        .map(|(code, reasons)| {
            obj([(
                "properties",
                obj([
                    ("code", obj([("const", s(code))])),
                    ("reason", obj([("enum", strs(reasons.iter().map(String::as_str)))])),
                ]),
            )])
        })
        .collect();
    obj([
        ("$schema", s("https://json-schema.org/draft/2020-12/schema")),
        ("$id", s("https://metrologymaven.io/schema/bus-error.schema.json")),
        ("title", s("Metrology Maven bus error, schema version 1")),
        ("type", s("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(["code", "detail", "reason"])),
        (
            "properties",
            obj([
                ("code", obj([("enum", strs(catalog.keys().map(String::as_str)))])),
                ("reason", obj([("type", s("string"))])),
                ("detail", obj([("type", s("string")), ("maxLength", CValue::Num(MAX_DETAIL_CHARS as f64))])),
            ]),
        ),
        ("oneOf", CValue::Arr(per_code)),
    ])
}
