//! The doctor report (spec node `PLAT.DOCTOR.REPORT`).
//!
//! A [`DoctorReport`] holds the snapshot ([`EnvSnapshot`], `PLAT.DOCTOR.PROBE`) and the
//! assessment ([`Assessment`], `PLAT.DOCTOR.LOOKUP`) with the schema version, the tool version,
//! the build phase and whether it was redacted. It has two views of one value:
//!
//! - [`DoctorReport::to_json`]: RFC 8785 canonical JSON, the shape described by
//!   `platform/doctor-report.schema.json` ([`SCHEMA_PATH`]);
//! - [`DoctorReport::to_text`]: the human-readable text, rendered by walking the same JSON value
//!   tree, so every JSON leaf value appears in it (REQ-PLAT.DOCTOR.REPORT-002) and the two views
//!   cannot disagree.
//!
//! [`Redactor`] replaces the user name, host name, home directory and absolute paths outside
//! the install directory (REQ-PLAT.DOCTOR.REPORT-003); [`DoctorReport::for_sharing`] applies it
//! unless the caller passes `include_identifying` (REQ-PLAT.DOCTOR.REPORT-004).
//! [`DoctorReport::parse`] reads a report back with a 1 MiB cap, a schema-version check and a
//! strict validation (REQ-PLAT.DOCTOR.REPORT-005); every string is rendered as data with
//! control, bidi and other format characters escaped (REQ-PLAT.DOCTOR.REPORT-006, C6).
//!
//! Determinism D0: no clock, no hash-ordered maps, no I/O except [`Identity::from_os`].

mod parse;
mod redact;
mod text;

pub use redact::{Identity, Redactor, PLACEHOLDER_HOME, PLACEHOLDER_HOST, PLACEHOLDER_PATH, PLACEHOLDER_USER};
pub use text::{escape_text, is_escaped_char, quote_text};

use crate::lookup::Assessment;
use crate::probe::{EnvSnapshot, MAX_DETAIL_CHARS};
use fid_canon::CValue;
use fid_platform_core::matrix::Phase;
use std::collections::BTreeMap;
use std::fmt;

/// Report schema version (`schema_version` of the report JSON).
pub const SCHEMA_VERSION: u32 = 1;
/// Largest report accepted by [`DoctorReport::parse`] (1 MiB, REQ-PLAT.DOCTOR.REPORT-005).
pub const MAX_REPORT_BYTES: usize = 1024 * 1024;
/// Longest accepted `tool_version`, in Unicode scalar values.
pub const MAX_TOOL_VERSION_CHARS: usize = 64;
/// Most profile verdicts accepted when parsing a report.
pub const MAX_PROFILES: usize = 64;
/// Longest accepted profile name when parsing a report, in Unicode scalar values.
pub const MAX_PROFILE_NAME_CHARS: usize = 64;
/// Repository path of the report JSON Schema.
pub const SCHEMA_PATH: &str = "platform/doctor-report.schema.json";
/// The keys of the report JSON object, in canonical order.
pub const REPORT_KEYS: [&str; 6] = ["assessment", "phase", "redacted", "schema_version", "snapshot", "tool_version"];

/// A doctor report: one value with a JSON and a text view.
#[derive(Debug, Clone, PartialEq)]
pub struct DoctorReport {
    pub schema_version: u32,
    /// Version of the tool that produced the report (supplied by the caller, e.g. the `fid`
    /// binary's version).
    pub tool_version: String,
    /// Build phase the assessment was made at.
    pub phase: Phase,
    /// Whether the redaction pass ran on this report.
    pub redacted: bool,
    pub snapshot: EnvSnapshot,
    /// The assessment, which also carries the overall outcome and its exit code.
    pub assessment: Assessment,
}

/// Typed errors of building, redacting, serialising and parsing a report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportError {
    /// The input is larger than [`MAX_REPORT_BYTES`].
    TooLarge { len: usize, max: usize },
    /// The report's `schema_version` is an integer other than [`SCHEMA_VERSION`].
    UnsupportedSchema { found: String },
    /// The report does not match the schema; `pointer` is a JSON Pointer to the offending value.
    SchemaInvalid { pointer: String, detail: String },
    /// The input is not valid JSON (`code` is the `fid-canon` error code).
    Parse { code: &'static str },
    /// The tool version is empty, too long or holds control or format characters.
    InvalidToolVersion,
    /// The install directory given to the redactor is not an absolute, non-root directory
    /// outside the home directory.
    InvalidInstallDir { detail: String },
    /// An identifying string (user, host or home) could not be determined, so redaction could
    /// not be guaranteed (fail closed).
    IdentityUnavailable { item: &'static str },
    /// A redacted string no longer fits its field's bound (e.g. `arch` over 64 bytes).
    RedactionOverflow { pointer: String },
    /// A hand-built report holds a non-finite number, which JSON cannot carry.
    NonFiniteNumber,
}

impl ReportError {
    /// Stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            ReportError::TooLarge { .. } => "too_large",
            ReportError::UnsupportedSchema { .. } => "unsupported_schema",
            ReportError::SchemaInvalid { .. } => "schema_invalid",
            ReportError::Parse { .. } => "parse",
            ReportError::InvalidToolVersion => "invalid_tool_version",
            ReportError::InvalidInstallDir { .. } => "invalid_install_dir",
            ReportError::IdentityUnavailable { .. } => "identity_unavailable",
            ReportError::RedactionOverflow { .. } => "redaction_overflow",
            ReportError::NonFiniteNumber => "non_finite_number",
        }
    }
}

impl fmt::Display for ReportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReportError::TooLarge { len, max } => write!(f, "too_large: {len} bytes exceeds {max}"),
            ReportError::UnsupportedSchema { found } => {
                write!(f, "unsupported_schema: schema_version {found} (expected {SCHEMA_VERSION})")
            }
            ReportError::SchemaInvalid { pointer, detail } => write!(f, "schema_invalid at {pointer:?}: {detail}"),
            ReportError::Parse { code } => write!(f, "parse: {code}"),
            ReportError::InvalidToolVersion => f.write_str("invalid_tool_version"),
            ReportError::InvalidInstallDir { detail } => write!(f, "invalid_install_dir: {detail}"),
            ReportError::IdentityUnavailable { item } => write!(f, "identity_unavailable: {item}"),
            ReportError::RedactionOverflow { pointer } => write!(f, "redaction_overflow at {pointer:?}"),
            ReportError::NonFiniteNumber => f.write_str("non_finite_number"),
        }
    }
}

impl std::error::Error for ReportError {}

/// Whether `s` is an acceptable tool version: 1 to [`MAX_TOOL_VERSION_CHARS`] characters, not
/// blank, with no control or format character.
pub(crate) fn valid_tool_version(s: &str) -> bool {
    let n = s.chars().count();
    (1..=MAX_TOOL_VERSION_CHARS).contains(&n) && !s.trim().is_empty() && !s.chars().any(is_escaped_char)
}

impl DoctorReport {
    /// A new, unredacted report (`redacted: false`).
    pub fn new(
        tool_version: &str,
        phase: Phase,
        snapshot: EnvSnapshot,
        assessment: Assessment,
    ) -> Result<Self, ReportError> {
        if !valid_tool_version(tool_version) {
            return Err(ReportError::InvalidToolVersion);
        }
        Ok(DoctorReport {
            schema_version: SCHEMA_VERSION,
            tool_version: tool_version.to_string(),
            phase,
            redacted: false,
            snapshot,
            assessment,
        })
    }

    /// The report as a canonical JSON value tree (`BTreeMap`-ordered keys).
    pub fn to_cvalue(&self) -> CValue {
        CValue::Obj(BTreeMap::from([
            ("assessment".to_string(), self.assessment.to_cvalue()),
            ("phase".to_string(), CValue::Str(self.phase.as_str().to_string())),
            ("redacted".to_string(), CValue::Bool(self.redacted)),
            ("schema_version".to_string(), CValue::Num(f64::from(self.schema_version))),
            ("snapshot".to_string(), self.snapshot.to_cvalue()),
            ("tool_version".to_string(), CValue::Str(self.tool_version.clone())),
        ]))
    }

    /// RFC 8785 canonical JSON bytes of the report.
    pub fn to_json(&self) -> Result<Vec<u8>, ReportError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue()).map_err(|_| ReportError::NonFiniteNumber)
    }

    /// The human-readable text view: sections Platform, Hardware, GPU, Verdicts and Unknowns,
    /// rendered from the same value tree as [`DoctorReport::to_json`]. Every string is quoted
    /// and escaped ([`quote_text`]).
    pub fn to_text(&self) -> Result<String, ReportError> {
        text::render(&self.to_cvalue())
    }

    /// Parses and validates a report (REQ-PLAT.DOCTOR.REPORT-005): at most
    /// [`MAX_REPORT_BYTES`], `schema_version` 1, and the full report schema, including the
    /// nested snapshot and assessment and the consistency of verdicts, outcome and exit code.
    pub fn parse(bytes: &[u8]) -> Result<Self, ReportError> {
        parse::parse(bytes)
    }

    /// The redacted copy of this report (`redacted: true`): every probed free-text string and
    /// the tool version pass through `redactor`. Closed-vocabulary values (statuses, reasons,
    /// verdicts, outcome, fields, tiers, phase) and the profile names of the installed profile
    /// file are configuration, not machine data, and are kept. An unknown item's `detail` that
    /// redaction made longer than 200 characters is cut to 199 and ends with `…`.
    pub fn redact(&self, redactor: &Redactor) -> Result<Self, ReportError> {
        let mut out = self.clone();
        out.tool_version = redactor.redact_text(&self.tool_version);
        if !valid_tool_version(&out.tool_version) {
            return Err(ReportError::RedactionOverflow { pointer: "/tool_version".to_string() });
        }
        redact::redact_snapshot(&mut out.snapshot, redactor)?;
        redact::redact_probed_detail(&mut out.assessment.tier, redactor);
        out.redacted = true;
        Ok(out)
    }

    /// The report as it may be shared: redacted by default; unchanged (and `redacted` left as
    /// it is, `false` for a new report) when `include_identifying` is set.
    pub fn for_sharing(&self, redactor: &Redactor, include_identifying: bool) -> Result<Self, ReportError> {
        if include_identifying {
            Ok(self.clone())
        } else {
            self.redact(redactor)
        }
    }
}

/// Cuts `s` to at most [`MAX_DETAIL_CHARS`] characters, ending with `…` when it was cut.
pub(crate) fn fit_detail(s: String) -> String {
    if s.chars().count() <= MAX_DETAIL_CHARS {
        return s;
    }
    let mut out: String = s.chars().take(MAX_DETAIL_CHARS - 1).collect();
    out.push('\u{2026}');
    out
}
