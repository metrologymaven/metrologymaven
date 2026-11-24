//! The `instrument-profile/v1` descriptor: a content-addressed, signed wrapper around a
//! sensor-model description that lets a file import or a live device name the instrument
//! model its observations were taken with. Spec node `HAL.IDL.PROFILE`.
//!
//! HAL owns the format; `UNC.INST` owns the library of profiles and the uncertainty models
//! they reference; `TRUST` verifies signatures. This module validates structure, enforces the
//! spec-sheet cap and computes the canonical digest. It never verifies a signature and never
//! picks a profile for an import.
//!
//! # Declared, not measured (D-045)
//!
//! A profile carries what its source states. A `spec-sheet` profile is the maker's published
//! claim for a model line, not a measurement of any instrument, so its `uec_max` is capped at
//! UEC-1 ([`ProfileErrorKind::UecExceedsSource`]) and [`SourceKind::is_declared`] says so.
//! `uec_max` is a ceiling on the evidence class a result built from the profile may reach, never
//! a class the profile has reached; the device class lists capabilities the source declares, not
//! capabilities a connected device has negotiated or that anyone has characterized.
//!
//! # Wire form
//!
//! JSON through `fid-canon`, so it hashes canonically (RFC 8785):
//!
//! ```text
//! {"schema": "instrument-profile/v1", "profile_id": "<name>@<semver>",
//!  "device_class": ["<Capability>", ...], "make": "...", "model": "...",
//!  "mpe_terms": [{"standard": {"designator", "edition", "clause"}, "symbol": "...",
//!                 "value": {"unit": "m", "value": "0x..."}, "contributor": "uc:<name>@<semver>"}],
//!  "frame_chain": [{"name": "...", "kind": "sensor"}, ...],
//!  "validity": {"range": {"lower": Q, "upper": Q}, "temperature": {"lower": Q, "upper": Q}},
//!  "source": {"kind": "spec-sheet|acceptance-certificate|iemps", "digest": "sha256:...",
//!             "citation": "..."},
//!  "um_model": {"id": "im:<name>@<semver>", "digest": "sha256:..."},
//!  "uec_max": 0..3,
//!  "signature": {"keyid": "...", "sig": "<padded standard base64>"}}
//! ```
//!
//! - every field is required and none is nullable; unknown fields are refused at every level
//!   (`profile_field_unknown`);
//! - quantities are the `HAL.IDL.UNITS` wire form: an SI unit symbol and the bit-exact `"0x"` +
//!   16 hex digits of the `f64`;
//! - `device_class` is strictly ascending, so a profile has exactly one spelling;
//! - `make`, `model` and `source.citation` are [`UntrustedText`] of 1 to
//!   [`MAX_UNTRUSTED_TEXT_BYTES`] bytes, kept exactly as given and never interpreted
//!   (CONSTITUTION C6);
//! - standard references hold a designator, an edition and a clause of at most
//!   [`MAX_TOKEN_BYTES`] bytes each and no other field, so no standards text can be stored;
//! - the input is limited to [`MAX_INPUT_BYTES`] bytes and [`MAX_DEPTH`] levels of nesting.
//!
//! The digest ([`InstrumentProfile::digest`]) is SHA-256 over the canonical JSON of the profile
//! with the `signature` member removed, so re-signing a profile does not change its identity.
//!
//! # Error order
//!
//! Errors carry a stable code and the RFC 6901 JSON pointer of the first offending field.
//! "First" means: the top-level `schema` tag; then, in each object, the first unknown field in
//! sorted key order, then the declared fields in sorted key order, depth first; list elements in
//! index order after the list's own length and order rules. The semantic rules run after the
//! whole profile is structurally valid, in this order: the validity envelope (range, then
//! temperature), then the spec-sheet cap.

mod model;

pub use model::{
    ContributorId, InstrumentProfile, Limits, ModelRef, MpeTerm, ProfileFrame, ProfileId, ProfileSource,
    SignatureBlock, SourceKind, StandardRef, ValidityEnvelope,
};

pub use crate::stamp::UntrustedText;

use fid_canon::CValue;
use std::fmt;

/// The `schema` tag of this format.
pub const SCHEMA_TAG: &str = "instrument-profile/v1";
/// Largest accepted profile, in bytes.
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;
/// Deepest accepted nesting of arrays and objects.
pub const MAX_DEPTH: usize = 16;
/// Longest `make`, `model` or citation, in UTF-8 bytes (REQ-001, AC-6).
pub const MAX_UNTRUSTED_TEXT_BYTES: usize = 4096;
/// Longest standard designator, edition, clause or name, in bytes.
pub const MAX_TOKEN_BYTES: usize = 64;
/// Longest MPE term symbol, in bytes.
pub const MAX_SYMBOL_BYTES: usize = 32;
/// Most capability names in a device class.
pub const MAX_DEVICE_CLASS: usize = 16;
/// Most MPE terms in one profile.
pub const MAX_MPE_TERMS: usize = 64;
/// Most frames in a frame chain.
pub const MAX_FRAMES: usize = 16;
/// Longest signer key reference, in bytes.
pub const MAX_KEYID_BYTES: usize = 256;
/// Longest signature value, in base64 characters.
pub const MAX_SIG_CHARS: usize = 8192;
/// Highest evidence class (UEC-3).
pub const MAX_UEC: u8 = 3;
/// Highest `uec_max` of a `spec-sheet` profile (REQ-004).
pub const SPEC_SHEET_UEC_CAP: u8 = 1;

/// What went wrong; [`ProfileErrorKind::code`] gives the stable wire code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProfileErrorKind {
    /// A required field is absent (REQ-001).
    FieldMissing,
    /// A `spec-sheet` profile with `uec_max` above UEC-1 (REQ-004).
    UecExceedsSource,
    /// A validity envelope whose lower limit exceeds its upper limit, whose limits disagree in
    /// unit, or whose temperature is not in kelvin or below 0 K (REQ-005).
    EnvelopeInvalid,
    /// A quantity unit outside the `HAL.IDL.UNITS` set (REQ-002, AC-4).
    UnitUnsupported,
    /// A NaN or infinite quantity value. Not named by the leaf; the `HAL.IDL.UNITS` code.
    QuantityNotFinite,
    /// Untrusted text over [`MAX_UNTRUSTED_TEXT_BYTES`] bytes (REQ-001, AC-6).
    TextTooLong,
    /// A field the format does not have (`deny_unknown_fields`). Not named by the leaf.
    FieldUnknown,
    /// A value of the wrong JSON type, outside its pattern, enum or bound. Not named by the
    /// leaf.
    ValueInvalid,
    /// A `schema` of the form `instrument-profile/v<n>` with `n` other than 1. Not named by
    /// the leaf; the code `HAL.IDL.STAMP` uses for the same case.
    SchemaVersionUnsupported,
    /// Input that is not JSON `fid-canon` accepts (syntax, invalid UTF-8, duplicate key, size
    /// or depth limit). Not named by the leaf.
    JsonInvalid,
}

impl ProfileErrorKind {
    /// Every kind, in declaration order.
    pub const ALL: [ProfileErrorKind; 10] = [
        ProfileErrorKind::FieldMissing,
        ProfileErrorKind::UecExceedsSource,
        ProfileErrorKind::EnvelopeInvalid,
        ProfileErrorKind::UnitUnsupported,
        ProfileErrorKind::QuantityNotFinite,
        ProfileErrorKind::TextTooLong,
        ProfileErrorKind::FieldUnknown,
        ProfileErrorKind::ValueInvalid,
        ProfileErrorKind::SchemaVersionUnsupported,
        ProfileErrorKind::JsonInvalid,
    ];

    /// Stable machine-readable code.
    pub fn code(self) -> &'static str {
        match self {
            ProfileErrorKind::FieldMissing => "profile_field_missing",
            ProfileErrorKind::UecExceedsSource => "profile_uec_exceeds_source",
            ProfileErrorKind::EnvelopeInvalid => "profile_envelope_invalid",
            ProfileErrorKind::UnitUnsupported => "unit_unsupported",
            ProfileErrorKind::QuantityNotFinite => "quantity_not_finite",
            ProfileErrorKind::TextTooLong => "untrusted_text_too_long",
            ProfileErrorKind::FieldUnknown => "profile_field_unknown",
            ProfileErrorKind::ValueInvalid => "profile_value_invalid",
            ProfileErrorKind::SchemaVersionUnsupported => "schema_version_unsupported",
            ProfileErrorKind::JsonInvalid => "profile_json_invalid",
        }
    }
}

/// A validation failure: a typed kind and the JSON pointer (RFC 6901) of the offending field.
/// The pointer of the whole profile is the empty string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProfileError {
    pub kind: ProfileErrorKind,
    pub pointer: String,
}

impl ProfileError {
    pub(crate) fn new(kind: ProfileErrorKind, pointer: &str) -> ProfileError {
        ProfileError { kind, pointer: pointer.to_string() }
    }

    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        self.kind.code()
    }
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {:?}", self.code(), self.pointer)
    }
}

impl std::error::Error for ProfileError {}

/// Parses and validates one `instrument-profile/v1` descriptor. Fails closed with the first
/// offending field.
pub fn validate(input: &[u8]) -> Result<InstrumentProfile, ProfileError> {
    let value = fid_canon::parse_with_limits(input, MAX_INPUT_BYTES, MAX_DEPTH)
        .map_err(|_| ProfileError::new(ProfileErrorKind::JsonInvalid, ""))?;
    validate_value(value)
}

/// [`validate`] for an already parsed value.
pub fn validate_value(value: CValue) -> Result<InstrumentProfile, ProfileError> {
    let profile = InstrumentProfile::read(value)?;
    profile.semantic()?;
    Ok(profile)
}

/// Validates `input` and returns its descriptor digest (REQ-006).
pub fn digest_json(input: &[u8]) -> Result<fid_types::Digest, ProfileError> {
    validate(input)?.digest()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable_and_distinct() {
        let codes: Vec<&str> = ProfileErrorKind::ALL.iter().map(|k| k.code()).collect();
        assert_eq!(
            codes,
            [
                "profile_field_missing",
                "profile_uec_exceeds_source",
                "profile_envelope_invalid",
                "unit_unsupported",
                "quantity_not_finite",
                "untrusted_text_too_long",
                "profile_field_unknown",
                "profile_value_invalid",
                "schema_version_unsupported",
                "profile_json_invalid",
            ]
        );
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), codes.len());
    }

    #[test]
    fn oversized_and_malformed_input_is_refused() {
        let big = vec![b' '; MAX_INPUT_BYTES + 1];
        assert_eq!(validate(&big).map_err(|e| e.code()).err(), Some("profile_json_invalid"));
        assert_eq!(validate(b"{\"a\":1,\"a\":2}").map_err(|e| e.code()).err(), Some("profile_json_invalid"));
        let deep = "[".repeat(MAX_DEPTH + 1) + &"]".repeat(MAX_DEPTH + 1);
        assert_eq!(validate(deep.as_bytes()).map_err(|e| e.code()).err(), Some("profile_json_invalid"));
    }
}
