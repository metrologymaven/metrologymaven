//! Licence evidence records and visibility gating for imported external datasets.
//! Spec node `UNC.RDL.EXT.LIC` (`spec/unc/rdl/ext/lic/NODE.md`).
//!
//! Decides the [`Visibility`] of an imported RDL record from its declared licence and an
//! optional [`LicenceEvidence`] record:
//! - Returns [`Visibility::Public`] **only** when `licence` is on the versioned redistribution
//!   allowlist [`ALLOWLIST_VERSION`] (`rdl-redist@1`) **and** a complete [`LicenceEvidence`]
//!   record matching `licence` is provided (REQ-UNC.RDL.EXT.LIC-001).
//! - Returns [`Visibility::ReferenceOnly`] when no evidence is given, when `licence` is not
//!   allowlisted, or when the evidence's `spdx_or_ref` does not match `licence`.
//! - Fails closed with [`LicError::EvidenceIncomplete`] naming the first missing or malformed
//!   field when an evidence record is supplied but incomplete (REQ-UNC.RDL.EXT.LIC-002).
//! - Always records [`ALLOWLIST_VERSION`] (`rdl-redist@1`) in the returned
//!   [`VisibilityDecision`] and [`RecordedEvidence`] (REQ-UNC.RDL.EXT.LIC-003).
//!
//! Determinism D0: pure table lookup over static data, no clock reads, no network, no `unsafe`,
//! no panics.

use std::collections::BTreeMap;
use std::fmt;

use fid_canon::{to_canonical_bytes, CValue};
use fid_types::Digest;

use crate::record::{is_control_or_format, licence_ok, Visibility, REDISTRIBUTABLE};

/// Version identifier of the redistribution allowlist recorded with every decision
/// (REQ-UNC.RDL.EXT.LIC-003, AC-3).
pub const ALLOWLIST_VERSION: &str = "rdl-redist@1";

/// Redistribution allowlist `rdl-redist@1`.
///
/// Matches [`REDISTRIBUTABLE`] in `fid_rdl::record` so visibility gating and
/// `fid_rdl::view::public_view` agree on every licence ID. Any addition (such as a custom
/// `LicenseRef-*` ID) requires a maintainer licence decision under the D-001 family.
pub const RDL_REDIST_V1: &[&str] = &REDISTRIBUTABLE;

/// Maximum byte length of a single evidence text field.
pub const MAX_EVIDENCE_TEXT: usize = 512;

/// Human-verified evidence for the licence terms of an external dataset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LicenceEvidence {
    /// SPDX licence ID or `LicenseRef-*` identifier observed at `evidence_url`.
    pub spdx_or_ref: String,
    /// Public URL where the licence terms were read.
    pub evidence_url: String,
    /// Calendar date (`YYYY-MM-DD`) when the terms were read.
    pub read_on: String,
    /// Name or handle of the person who confirmed the terms.
    pub confirmed_by: String,
    /// Canonical SHA-256 digest (`sha256:<64 lowercase hex>`) of the captured terms text.
    pub digest_of_terms: String,
}

/// Evidence paired with the allowlist version used to evaluate it (REQ-UNC.RDL.EXT.LIC-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedEvidence {
    pub allowlist_version: String,
    pub evidence: LicenceEvidence,
}

impl RecordedEvidence {
    /// RFC 8785 canonical JSON value.
    pub fn to_value(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("allowlist_version".to_string(), CValue::Str(self.allowlist_version.clone()));
        m.insert("confirmed_by".to_string(), CValue::Str(self.evidence.confirmed_by.clone()));
        m.insert("digest_of_terms".to_string(), CValue::Str(self.evidence.digest_of_terms.clone()));
        m.insert("evidence_url".to_string(), CValue::Str(self.evidence.evidence_url.clone()));
        m.insert("read_on".to_string(), CValue::Str(self.evidence.read_on.clone()));
        m.insert("spdx_or_ref".to_string(), CValue::Str(self.evidence.spdx_or_ref.clone()));
        CValue::Obj(m)
    }

    /// Canonical UTF-8 JSON bytes.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, LicError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| LicError::EvidenceIncomplete { field: "evidence" })
    }
}

/// Result of [`visibility_for`]: the gated visibility, the allowlist version consulted, and
/// the recorded evidence (if supplied).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibilityDecision {
    pub visibility: Visibility,
    pub allowlist_version: &'static str,
    pub recorded_evidence: Option<RecordedEvidence>,
}

/// Error returned when a supplied [`LicenceEvidence`] record is incomplete or malformed
/// (REQ-UNC.RDL.EXT.LIC-002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LicError {
    /// Required evidence field missing, empty or malformed (`licence_evidence_incomplete`).
    EvidenceIncomplete { field: &'static str },
}

impl LicError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            LicError::EvidenceIncomplete { .. } => "licence_evidence_incomplete",
        }
    }

    /// The incomplete or malformed field name.
    pub fn field(&self) -> &'static str {
        match self {
            LicError::EvidenceIncomplete { field } => field,
        }
    }
}

impl fmt::Display for LicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code(), self.field())
    }
}

impl std::error::Error for LicError {}

/// Returns true iff `licence` is in [`RDL_REDIST_V1`] (`rdl-redist@1`, exact case-sensitive match).
pub fn is_allowlisted(licence: &str) -> bool {
    RDL_REDIST_V1.contains(&licence)
}

/// Validates every required field of `ev` in fixed order; fails with
/// [`LicError::EvidenceIncomplete`] naming the first missing or malformed field.
pub fn validate_evidence(ev: &LicenceEvidence) -> Result<(), LicError> {
    if !valid_text(&ev.spdx_or_ref) || !licence_ok(&ev.spdx_or_ref) {
        return Err(LicError::EvidenceIncomplete { field: "spdx_or_ref" });
    }
    if !valid_url(&ev.evidence_url) {
        return Err(LicError::EvidenceIncomplete { field: "evidence_url" });
    }
    if !valid_iso_date(&ev.read_on) {
        return Err(LicError::EvidenceIncomplete { field: "read_on" });
    }
    if !valid_text(&ev.confirmed_by) {
        return Err(LicError::EvidenceIncomplete { field: "confirmed_by" });
    }
    if Digest::parse(&ev.digest_of_terms).is_err() {
        return Err(LicError::EvidenceIncomplete { field: "digest_of_terms" });
    }
    Ok(())
}

/// Decides the visibility of an imported record from its declared `licence` and optional
/// `evidence` (REQ-UNC.RDL.EXT.LIC-001..003).
///
/// - If `evidence` is `Some(ev)`, validates completeness first; incomplete evidence returns
///   `Err(LicError::EvidenceIncomplete { field })`.
/// - Returns [`Visibility::Public`] only when `licence` is in `rdl-redist@1`, `evidence` is
///   complete, and `ev.spdx_or_ref == licence`.
/// - Returns [`Visibility::ReferenceOnly`] in all other non-error cases.
/// - Records `allowlist_version: "rdl-redist@1"` on every returned [`VisibilityDecision`].
pub fn visibility_for(licence: &str, evidence: Option<&LicenceEvidence>) -> Result<VisibilityDecision, LicError> {
    match evidence {
        None => Ok(VisibilityDecision {
            visibility: Visibility::ReferenceOnly,
            allowlist_version: ALLOWLIST_VERSION,
            recorded_evidence: None,
        }),
        Some(ev) => {
            validate_evidence(ev)?;
            let visibility = if is_allowlisted(licence) && ev.spdx_or_ref == licence {
                Visibility::Public
            } else {
                Visibility::ReferenceOnly
            };
            Ok(VisibilityDecision {
                visibility,
                allowlist_version: ALLOWLIST_VERSION,
                recorded_evidence: Some(RecordedEvidence {
                    allowlist_version: ALLOWLIST_VERSION.to_string(),
                    evidence: ev.clone(),
                }),
            })
        }
    }
}

fn valid_text(s: &str) -> bool {
    let trimmed = s.trim();
    !trimmed.is_empty() && s.len() <= MAX_EVIDENCE_TEXT && !s.chars().any(is_control_or_format)
}

fn valid_url(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://")) else {
        return false;
    };
    !rest.is_empty() && s.len() <= MAX_EVIDENCE_TEXT && s.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

/// Validates calendar date `YYYY-MM-DD` (proleptic Gregorian).
fn valid_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let parse_u32 = |slice: &str| -> Option<u32> {
        if !slice.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        slice.parse().ok()
    };
    let (Some(y), Some(m), Some(d)) = (parse_u32(&s[0..4]), parse_u32(&s[5..7]), parse_u32(&s[8..10])) else {
        return false;
    };
    if !(1..=12).contains(&m) || d == 0 {
        return false;
    }
    let max_day = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => return false,
    };
    d <= max_day
}
