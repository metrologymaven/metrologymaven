//! The closed device error model shared by every capability package: eleven error codes with
//! a fixed severity, retryable flag and runtime behaviour class, and the [`DeviceError`]
//! record that carries a device's own code and message as untrusted text and points to
//! operator guidance only by [`OperatorHintId`]. Spec node `HAL.IDL.BASE.ERR`
//! (REQ-HAL.IDL.BASE-003, REQ-HAL.IDL.BASE-004).
//!
//! Applying the behaviours at runtime is `HAL.ACQ.SESS.HEALTH`; the hint catalogue text is
//! owned by `VIZ` and the docs. This module only defines the types and their wire form.
//!
//! Wire form (`device-error/1`, JSON through `fid-canon`, so it is RFC 8785 canonical and
//! parse followed by write gives the same bytes):
//!
//! ```text
//! {"code": "TRANSIENT", "device_code": "-17" | null, "hint": "hal.transient.001" | null,
//!  "message": {"bytes": "...", "truncated": false}, "schema": "device-error/1"}
//! ```
//!
//! - every field is required (an absent value is `null`, never omitted) and unknown fields
//!   are refused, so there is no free-text guidance field (REQ-004);
//! - `code` is one of the eleven upper-case names of [`ErrorCode::ALL`], compared byte for
//!   byte (no case folding); any other string fails with `error_code_unknown` (REQ-001);
//! - `device_code` is the device's own numeric code as a canonical decimal `i64` string
//!   (the project's `fid-canon` rule for integers), or `null`;
//! - `message.bytes` is the device's message, at most [`MAX_MESSAGE_BYTES`] bytes of UTF-8,
//!   kept exactly as sent (not normalised, trimmed or case-folded). A longer message is cut
//!   at the last UTF-8 boundary at or below the limit and `truncated` is set, on
//!   construction and on parse alike; it is never refused (REQ-003);
//! - `hint` matches `hal\.[a-z_]+\.[0-9]{3}` and is at most [`MAX_HINT_BYTES`] bytes
//!   (REQ-004); its middle segment is not checked against `code`, so one hint may serve
//!   several codes.
//!
//! The schema tag's major version is [`ERROR_CODE_SET_VERSION`]: adding, removing or
//! renaming a code, or changing a row of the behaviour table, is a new major version.
//!
//! Parsing reports the first failure in this fixed order: the bytes as JSON (size limit
//! [`MAX_INPUT_BYTES`], depth limit [`MAX_DEPTH`], duplicate keys refused); the top level is
//! an object; an unknown key (first in sorted order); a missing key (first in sorted order);
//! `schema`; then `code`, `device_code`, `hint` and `message` in that order. Wrong shapes
//! that are not an unknown code or a bad hint ID fail with `device_error_invalid`.

use super::config::ConfigErrorCode;
use crate::units::{boolean, i64_dec, kept, obj, object, string, take, Obj};
pub use crate::stamp::UntrustedText;
use fid_canon::CValue;
use std::fmt;

/// Major version of the closed code set and of the `device-error` wire form.
pub const ERROR_CODE_SET_VERSION: u32 = 1;
/// Wire tag of a device error record.
pub const SCHEMA_TAG: &str = "device-error/1";
/// Longest device message kept, in bytes of UTF-8 (REQ-003).
pub const MAX_MESSAGE_BYTES: usize = 4096;
/// Longest operator hint ID, in bytes.
pub const MAX_HINT_BYTES: usize = 64;
/// Largest accepted device error document, in bytes. Room for a message several times the
/// kept length, so an over-long message reaches the truncation rule instead of the JSON
/// limit.
pub const MAX_INPUT_BYTES: usize = 64 * 1024;
/// Deepest accepted nesting of a device error document.
pub const MAX_DEPTH: usize = 4;

const FIELDS: [&str; 5] = ["code", "device_code", "hint", "message", "schema"];
const MESSAGE_FIELDS: [&str; 2] = ["bytes", "truncated"];

// ------------------------------------------------------------------ codes

/// How bad an error is. Ordered `Info < Warning < Error < Critical`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Info,
    Warning,
    Error,
    Critical,
}

impl Severity {
    /// The wire name: `info`, `warning`, `error` or `critical`.
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
            Severity::Critical => "critical",
        }
    }
}

/// The one normative runtime behaviour class of a code. The runtime applies it
/// (`HAL.ACQ.SESS.HEALTH`); this type only names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Behaviour {
    /// Retry the operation under the runtime's retry policy.
    Retry,
    /// The session is interrupted; streams stop and may be resumed after reconnecting.
    Interrupt,
    /// Every stream of the device stops; the session stays open.
    StopStreams,
    /// The operation fails; the session and other operations are unaffected.
    Fail,
    /// The device is put into `FAULT`.
    Fault,
}

impl Behaviour {
    /// Stable name: `retry`, `interrupt`, `stop_streams`, `fail` or `fault`.
    pub fn as_str(self) -> &'static str {
        match self {
            Behaviour::Retry => "retry",
            Behaviour::Interrupt => "interrupt",
            Behaviour::StopStreams => "stop_streams",
            Behaviour::Fail => "fail",
            Behaviour::Fault => "fault",
        }
    }
}

/// The closed set of device error codes (REQ-001). Declaration order is the order of the
/// table in the leaf's Design, and it is the order of [`Ord`] and of [`ErrorCode::ALL`].
///
/// Deliberately not `#[non_exhaustive]`: an exhaustive `match` on this type anywhere in the
/// workspace stops compiling when a code is added, which is what forces a new table row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ErrorCode {
    Transient,
    CommLost,
    ConfigInvalid,
    OutOfRange,
    NotQualified,
    CalibrationExpired,
    Limit,
    SafetyStopObserved,
    VendorLicense,
    Unsupported,
    Internal,
}

impl ErrorCode {
    /// Every code, in table order.
    pub const ALL: [ErrorCode; 11] = [
        ErrorCode::Transient,
        ErrorCode::CommLost,
        ErrorCode::ConfigInvalid,
        ErrorCode::OutOfRange,
        ErrorCode::NotQualified,
        ErrorCode::CalibrationExpired,
        ErrorCode::Limit,
        ErrorCode::SafetyStopObserved,
        ErrorCode::VendorLicense,
        ErrorCode::Unsupported,
        ErrorCode::Internal,
    ];

    /// The wire name, e.g. `SAFETY_STOP_OBSERVED`.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Transient => "TRANSIENT",
            ErrorCode::CommLost => "COMM_LOST",
            ErrorCode::ConfigInvalid => "CONFIG_INVALID",
            ErrorCode::OutOfRange => "OUT_OF_RANGE",
            ErrorCode::NotQualified => "NOT_QUALIFIED",
            ErrorCode::CalibrationExpired => "CALIBRATION_EXPIRED",
            ErrorCode::Limit => "LIMIT",
            ErrorCode::SafetyStopObserved => "SAFETY_STOP_OBSERVED",
            ErrorCode::VendorLicense => "VENDOR_LICENSE",
            ErrorCode::Unsupported => "UNSUPPORTED",
            ErrorCode::Internal => "INTERNAL",
        }
    }

    /// The code of a wire name, compared byte for byte; anything else fails with
    /// `error_code_unknown` (REQ-001).
    pub fn parse(s: &str) -> Result<ErrorCode, DeviceErrorParseError> {
        ErrorCode::ALL
            .into_iter()
            .find(|c| c.as_str() == s)
            .ok_or_else(|| DeviceErrorParseError::new(ParseErrorKind::CodeUnknown, "code", s))
    }

    /// The severity of the Design table (REQ-002).
    pub fn severity(self) -> Severity {
        match self {
            ErrorCode::Transient => Severity::Warning,
            ErrorCode::CommLost => Severity::Error,
            ErrorCode::ConfigInvalid => Severity::Error,
            ErrorCode::OutOfRange => Severity::Error,
            ErrorCode::NotQualified => Severity::Error,
            ErrorCode::CalibrationExpired => Severity::Warning,
            ErrorCode::Limit => Severity::Error,
            ErrorCode::SafetyStopObserved => Severity::Critical,
            ErrorCode::VendorLicense => Severity::Error,
            ErrorCode::Unsupported => Severity::Error,
            ErrorCode::Internal => Severity::Critical,
        }
    }

    /// The retryable flag of the Design table (REQ-002). Only `TRANSIENT` is retryable.
    #[allow(clippy::match_like_matches_macro)] // Exhaustive on purpose: a new code needs a row.
    pub fn retryable(self) -> bool {
        match self {
            ErrorCode::Transient => true,
            ErrorCode::CommLost
            | ErrorCode::ConfigInvalid
            | ErrorCode::OutOfRange
            | ErrorCode::NotQualified
            | ErrorCode::CalibrationExpired
            | ErrorCode::Limit
            | ErrorCode::SafetyStopObserved
            | ErrorCode::VendorLicense
            | ErrorCode::Unsupported
            | ErrorCode::Internal => false,
        }
    }

    /// The runtime behaviour class of the Design table (REQ-002).
    pub fn behaviour(self) -> Behaviour {
        match self {
            ErrorCode::Transient => Behaviour::Retry,
            ErrorCode::CommLost => Behaviour::Interrupt,
            ErrorCode::ConfigInvalid => Behaviour::Fail,
            ErrorCode::OutOfRange => Behaviour::Fail,
            ErrorCode::NotQualified => Behaviour::Fail,
            ErrorCode::CalibrationExpired => Behaviour::Fail,
            ErrorCode::Limit => Behaviour::StopStreams,
            ErrorCode::SafetyStopObserved => Behaviour::StopStreams,
            ErrorCode::VendorLicense => Behaviour::Fail,
            ErrorCode::Unsupported => Behaviour::Fail,
            ErrorCode::Internal => Behaviour::Fault,
        }
    }

    /// What the code means, in the project's own words.
    pub fn semantics(self) -> &'static str {
        match self {
            ErrorCode::Transient => {
                "A short-lived condition (busy bus, timeout, dropped packet) that the same request \
                 may not meet again."
            }
            ErrorCode::CommLost => "The link to the device is gone; nothing more can be exchanged until it reconnects.",
            ErrorCode::ConfigInvalid => {
                "The requested configuration breaks a typing rule of the device's declared schema."
            }
            ErrorCode::OutOfRange => "A well-typed value lies outside the range the device declares for it.",
            ErrorCode::NotQualified => {
                "The device, probe or tool has no valid qualification for the requested operation."
            }
            ErrorCode::CalibrationExpired => {
                "The calibration the operation relies on is past its due date; results would not be \
                 traceable."
            }
            ErrorCode::Limit => {
                "The device reached a travel, force, temperature or other limit and stopped the motion \
                 or measurement."
            }
            ErrorCode::SafetyStopObserved => {
                "The device reports that a safety stop is active. Software observes it only; the reset \
                 happens at the machine (REQ-HAL.IDL.BASE-005)."
            }
            ErrorCode::VendorLicense => "The device or its vendor software refuses the operation for licensing reasons.",
            ErrorCode::Unsupported => "The device does not offer the requested operation or option.",
            ErrorCode::Internal => {
                "A fault inside the driver or device that none of the other codes describes; the device \
                 cannot be trusted until it is reset."
            }
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<ConfigErrorCode> for ErrorCode {
    fn from(c: ConfigErrorCode) -> ErrorCode {
        match c {
            ConfigErrorCode::ConfigInvalid => ErrorCode::ConfigInvalid,
            ConfigErrorCode::OutOfRange => ErrorCode::OutOfRange,
        }
    }
}

// ------------------------------------------------------------------ parse errors

/// Why a device error document or one of its parts was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ParseErrorKind {
    /// A `code` string outside the closed set (REQ-001): `error_code_unknown`.
    CodeUnknown,
    /// A hint ID that does not match `hal\.[a-z_]+\.[0-9]{3}` or is too long (REQ-004):
    /// `hint_id_invalid`.
    HintIdInvalid,
    /// Anything else: bytes `fid-canon` refuses, a wrong JSON type, an unknown or missing
    /// field, a wrong schema tag, a non-canonical `device_code`. `device_error_invalid`;
    /// not named by the leaf.
    Invalid,
}

impl ParseErrorKind {
    /// Stable machine-readable code.
    pub fn code(self) -> &'static str {
        match self {
            ParseErrorKind::CodeUnknown => "error_code_unknown",
            ParseErrorKind::HintIdInvalid => "hint_id_invalid",
            ParseErrorKind::Invalid => "device_error_invalid",
        }
    }
}

/// A refused device error document: the kind, the top-level field it concerns (empty for
/// the document as a whole) and up to 64 bytes of the offending text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceErrorParseError {
    pub kind: ParseErrorKind,
    pub field: String,
    pub value: String,
}

impl DeviceErrorParseError {
    fn new(kind: ParseErrorKind, field: &str, value: &str) -> DeviceErrorParseError {
        DeviceErrorParseError { kind, field: kept(field), value: kept(value) }
    }

    fn invalid(field: &str) -> DeviceErrorParseError {
        DeviceErrorParseError::new(ParseErrorKind::Invalid, field, "")
    }

    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        self.kind.code()
    }
}

impl fmt::Display for DeviceErrorParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at field {:?}", self.code(), self.field)
    }
}

impl std::error::Error for DeviceErrorParseError {}

// ------------------------------------------------------------------ hint ID

/// A reference into the project-owned operator hint catalogue: `hal.<segment>.<nnn>`, where
/// the segment is one or more of `a-z` and `_` and `nnn` is exactly three ASCII digits, at
/// most [`MAX_HINT_BYTES`] bytes in all (REQ-004). The catalogue text is not here.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OperatorHintId(String);

impl OperatorHintId {
    /// Checks and wraps a hint ID; fails with `hint_id_invalid`.
    pub fn parse(s: &str) -> Result<OperatorHintId, DeviceErrorParseError> {
        if valid_hint(s) {
            Ok(OperatorHintId(s.to_string()))
        } else {
            Err(DeviceErrorParseError::new(ParseErrorKind::HintIdInvalid, "hint", s))
        }
    }

    /// The ID text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OperatorHintId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// `hal\.[a-z_]+\.[0-9]{3}`, anchored at both ends, over bytes (so no Unicode digit or
/// letter slips through), and at most [`MAX_HINT_BYTES`] bytes.
fn valid_hint(s: &str) -> bool {
    if s.len() > MAX_HINT_BYTES {
        return false;
    }
    let Some(rest) = s.strip_prefix("hal.") else {
        return false;
    };
    let Some((segment, digits)) = rest.split_once('.') else {
        return false;
    };
    !segment.is_empty()
        && segment.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
        && digits.len() == 3
        && digits.bytes().all(|b| b.is_ascii_digit())
}

// ------------------------------------------------------------------ device text

/// A device-supplied string bounded to [`MAX_MESSAGE_BYTES`] bytes, with a marker telling
/// whether the device sent more (REQ-003). The text itself is an [`UntrustedText`]: data,
/// never instructions, shown to a person only through [`UntrustedText::as_display`].
///
/// Wire form: `{"bytes": <string>, "truncated": <bool>}`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct DeviceText {
    text: UntrustedText,
    truncated: bool,
}

impl DeviceText {
    /// Wraps a device string, cutting it at the last UTF-8 boundary at or below
    /// [`MAX_MESSAGE_BYTES`] if it is longer, and marking it truncated then. Never fails.
    pub fn new(text: &str) -> DeviceText {
        let (kept, cut) = truncate(text);
        DeviceText { text: UntrustedText::new(kept), truncated: cut }
    }

    /// The text as kept (at most [`MAX_MESSAGE_BYTES`] bytes).
    pub fn text(&self) -> &UntrustedText {
        &self.text
    }

    /// Whether the device sent more than was kept.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// The wire value.
    pub fn to_value(&self) -> CValue {
        obj([("bytes", CValue::Str(self.text.raw().to_string())), ("truncated", CValue::Bool(self.truncated))])
    }

    /// Reads the wire value. A `bytes` string longer than the limit is cut and marked, not
    /// refused; a `truncated: true` on a short string is kept as sent.
    pub fn from_value(v: CValue) -> Result<DeviceText, DeviceErrorParseError> {
        let bad = |_| DeviceErrorParseError::invalid("message");
        let mut o = object(v, "message", &MESSAGE_FIELDS).map_err(bad)?;
        let bytes = string("bytes", take(&mut o, "bytes").map_err(bad)?).map_err(bad)?;
        let marked = boolean("truncated", take(&mut o, "truncated").map_err(bad)?).map_err(bad)?;
        let mut t = DeviceText::new(&bytes);
        t.truncated |= marked;
        Ok(t)
    }
}

/// The longest prefix of `s` of at most [`MAX_MESSAGE_BYTES`] bytes that ends on a char
/// boundary, and whether anything was cut.
fn truncate(s: &str) -> (&str, bool) {
    if s.len() <= MAX_MESSAGE_BYTES {
        return (s, false);
    }
    let mut end = MAX_MESSAGE_BYTES;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    (s.get(..end).unwrap_or(""), true)
}

// ------------------------------------------------------------------ device error

/// One error reported by a device, driver or simulator (REQ-HAL.IDL.BASE-003, -004).
///
/// There is deliberately no free-text guidance field: guidance is the catalogue entry
/// `hint` points to. Ordering (derived) is by `code` in table order, then `device_code`
/// (`None` first), then `message` (bytes, then the marker), then `hint` (`None` first).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceError {
    pub code: ErrorCode,
    /// The device's own numeric code, if it has one.
    pub device_code: Option<i64>,
    /// The device's own message, untrusted and bounded.
    pub message: DeviceText,
    /// Operator guidance, by catalogue ID only.
    pub hint: Option<OperatorHintId>,
}

impl DeviceError {
    /// A device error; `message` is bounded by [`DeviceText::new`].
    pub fn new(code: ErrorCode, device_code: Option<i64>, message: &str, hint: Option<OperatorHintId>) -> DeviceError {
        DeviceError { code, device_code, message: DeviceText::new(message), hint }
    }

    /// The severity of [`DeviceError::code`].
    pub fn severity(&self) -> Severity {
        self.code.severity()
    }

    /// The retryable flag of [`DeviceError::code`].
    pub fn retryable(&self) -> bool {
        self.code.retryable()
    }

    /// The behaviour class of [`DeviceError::code`].
    pub fn behaviour(&self) -> Behaviour {
        self.code.behaviour()
    }

    /// The `device-error/1` wire value.
    pub fn to_value(&self) -> CValue {
        obj([
            ("code", CValue::Str(self.code.as_str().to_string())),
            ("device_code", self.device_code.map_or(CValue::Null, CValue::I64)),
            ("hint", self.hint.as_ref().map_or(CValue::Null, |h| CValue::Str(h.0.clone()))),
            ("message", self.message.to_value()),
            ("schema", CValue::Str(SCHEMA_TAG.to_string())),
        ])
    }

    /// RFC 8785 canonical bytes of the wire value (REQ-005).
    pub fn to_canonical_json(&self) -> Result<Vec<u8>, DeviceErrorParseError> {
        fid_canon::to_canonical_bytes(&self.to_value()).map_err(|_| DeviceErrorParseError::invalid(""))
    }

    /// Parses a `device-error/1` document (see the module docs for the order of checks).
    pub fn from_json(input: &[u8]) -> Result<DeviceError, DeviceErrorParseError> {
        let v = fid_canon::parse_with_limits(input, MAX_INPUT_BYTES, MAX_DEPTH)
            .map_err(|_| DeviceErrorParseError::invalid(""))?;
        DeviceError::from_value(v)
    }

    /// [`DeviceError::from_json`] for an already parsed value.
    pub fn from_value(v: CValue) -> Result<DeviceError, DeviceErrorParseError> {
        let mut o: Obj = match v {
            CValue::Obj(o) => o,
            _ => return Err(DeviceErrorParseError::invalid("")),
        };
        if let Some(k) = o.keys().find(|k| !FIELDS.contains(&k.as_str())) {
            return Err(DeviceErrorParseError::new(ParseErrorKind::Invalid, k, ""));
        }
        if let Some(k) = FIELDS.iter().find(|k| !o.contains_key(**k)) {
            return Err(DeviceErrorParseError::invalid(k));
        }
        let field = |o: &mut Obj, k: &'static str| take(o, k).map_err(|_| DeviceErrorParseError::invalid(k));

        match field(&mut o, "schema")? {
            CValue::Str(s) if s == SCHEMA_TAG => {}
            _ => return Err(DeviceErrorParseError::invalid("schema")),
        }
        let code = match field(&mut o, "code")? {
            CValue::Str(s) => ErrorCode::parse(&s)?,
            _ => return Err(DeviceErrorParseError::invalid("code")),
        };
        let device_code = match field(&mut o, "device_code")? {
            CValue::Null => None,
            v @ CValue::Str(_) => {
                Some(i64_dec("device_code", v).map_err(|_| DeviceErrorParseError::invalid("device_code"))?)
            }
            _ => return Err(DeviceErrorParseError::invalid("device_code")),
        };
        let hint = match field(&mut o, "hint")? {
            CValue::Null => None,
            CValue::Str(s) => Some(OperatorHintId::parse(&s)?),
            _ => return Err(DeviceErrorParseError::invalid("hint")),
        };
        let message = DeviceText::from_value(field(&mut o, "message")?)?;
        Ok(DeviceError { code, device_code, message, hint })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip_their_names() {
        for c in ErrorCode::ALL {
            assert_eq!(ErrorCode::parse(c.as_str()), Ok(c));
        }
        let mut sorted = ErrorCode::ALL;
        sorted.sort();
        assert_eq!(sorted, ErrorCode::ALL);
    }

    #[test]
    fn truncation_never_splits_a_char() {
        let s = "€".repeat(2000); // 6000 bytes; 4096 is not a boundary (4096 = 3 * 1365 + 1)
        let (k, cut) = truncate(&s);
        assert!(cut);
        assert_eq!(k.len(), 4095);
        let s = "a".repeat(MAX_MESSAGE_BYTES);
        assert_eq!(truncate(&s), (s.as_str(), false));
    }

    #[test]
    fn hint_grammar() {
        assert!(valid_hint("hal.comm_lost.001"));
        assert!(valid_hint("hal._.999"));
        for bad in ["hal..001", "hal.a.01", "hal.a.0001", "Hal.a.001", "hal.a1.001", "hal.a.00١", "hal.a.b.001"] {
            assert!(!valid_hint(bad), "{bad}");
        }
        let long = format!("hal.{}.001", "a".repeat(MAX_HINT_BYTES));
        assert!(!valid_hint(&long));
    }
}
