//! Signature meanings catalogue (spec node `TRUST.ESIG.MEANING`).
//!
//! - Built-in codes [`BUILTIN_CODES`] (`AUTHORED`, `REVIEWED`, `APPROVED`,
//!   `RELEASED`) are fixed and append-only across releases (REQ-001, REQ-005).
//! - Tenant-defined meanings come from the active governance profile document
//!   (`fid-profile`), must match `^[A-Z][A-Z0-9_]{1,31}$` and begin with `X_`,
//!   and must not collide with built-in codes or repeat (REQ-002, REQ-003).
//! - [`command`] registers `sign.list_meanings@1` (R0, D0, CLI + MCP) returning
//!   built-ins first in fixed order followed by tenant codes sorted ascending
//!   (REQ-004).

pub mod command;

pub use command::{
    cli_params, declarations, invoke_cli, invoke_mcp, list_meanings, mcp_params, EsigCall, EsigCtx, ListMeaningsParams,
    ListMeaningsResult, MeaningView, LIST_MEANINGS,
};

use fid_canon::{parse_with_limits, CValue};
use fid_profile::defs::{MAX_DESCRIPTION_CHARS, MAX_INPUT_BYTES, MAX_JSON_DEPTH, MAX_LABEL_CHARS, MAX_TENANT_MEANINGS};
use fid_profile::{MeaningCode, ProfileDoc};
use std::collections::BTreeMap;
use std::fmt;

/// Built-in signature meaning codes in canonical release order (REQ-001, REQ-005).
///
/// Append-only: existing codes must never be renamed or removed once released.
pub const BUILTIN_CODES: [&str; 4] = ["AUTHORED", "REVIEWED", "APPROVED", "RELEASED"];

/// Maximum number of localized label overrides on a single [`Meaning`].
pub const MAX_LOCALISED_LABELS: usize = 32;
/// Longest BCP-47-like language tag key, in bytes.
pub const MAX_LANG_TAG_BYTES: usize = 35;

/// Where a [`Meaning`] entry originates (REQ-TRUST.ESIG.MEANING-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    /// One of the fixed built-in codes (`builtin`).
    Builtin,
    /// A tenant-defined `X_` code from the active governance profile (`profile`).
    Profile,
}

impl Origin {
    /// Wire spelling (`builtin` or `profile`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Origin::Builtin => "builtin",
            Origin::Profile => "profile",
        }
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One signature meaning in the catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meaning {
    /// Machine-readable meaning code (`^[A-Z][A-Z0-9_]{1,31}$`).
    pub code: MeaningCode,
    /// Default English display label.
    pub label: String,
    /// One-sentence description of what signing with this meaning asserts.
    pub description: String,
    /// Origin (`builtin` or `profile`).
    pub origin: Origin,
    /// Optional localized display labels keyed by language tag (sorted by tag).
    pub labels: BTreeMap<String, String>,
}

/// Errors returned when loading or querying the signature meanings catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeaningError {
    /// A tenant meaning code collided with a built-in, broke syntax rules,
    /// lacked the `X_` prefix, appeared twice, or had invalid text (REQ-003).
    Invalid { code: String, reason: String },
    /// Invalid command parameters on CLI or MCP.
    BadParams { reason: &'static str },
    /// Bus policy class check refused the principal.
    PolicyRefused { reason: &'static str },
    /// Internal invariant error.
    Internal { reason: &'static str },
}

impl MeaningError {
    /// Stable machine-readable error code.
    pub const fn code(&self) -> &'static str {
        match self {
            MeaningError::Invalid { .. } => "meaning.invalid",
            MeaningError::BadParams { .. } => "bad_params",
            MeaningError::PolicyRefused { .. } => "policy_refused",
            MeaningError::Internal { .. } => "internal",
        }
    }

    /// The offending meaning code for `meaning.invalid`, if any.
    pub fn offending_code(&self) -> Option<&str> {
        match self {
            MeaningError::Invalid { code, .. } => Some(code.as_str()),
            _ => None,
        }
    }

    pub(crate) fn invalid(code: impl Into<String>, reason: impl Into<String>) -> Self {
        MeaningError::Invalid { code: cut_code(&code.into()), reason: reason.into() }
    }

    pub(crate) fn bad(reason: &'static str) -> Self {
        MeaningError::BadParams { reason }
    }
}

impl fmt::Display for MeaningError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MeaningError::Invalid { code, reason } => write!(f, "meaning.invalid ({code}): {reason}"),
            MeaningError::BadParams { reason } => write!(f, "bad_params: {reason}"),
            MeaningError::PolicyRefused { reason } => write!(f, "policy_refused: {reason}"),
            MeaningError::Internal { reason } => write!(f, "internal: {reason}"),
        }
    }
}

impl std::error::Error for MeaningError {}

const BUILTIN_SPECS: [(&str, &str, &str); 4] = [
    ("AUTHORED", "Authored", "The signer created or prepared the record for review."),
    ("REVIEWED", "Reviewed", "The signer checked the record for technical accuracy and completeness."),
    ("APPROVED", "Approved", "The signer formally approved the record as fit for its intended purpose."),
    ("RELEASED", "Released", "The signer authorized release of the record for operational use."),
];

/// Returns the four built-in signature meanings in fixed release order (REQ-001).
pub fn builtin_meanings() -> Vec<Meaning> {
    let mut out = Vec::with_capacity(BUILTIN_SPECS.len());
    for (code_s, label, description) in BUILTIN_SPECS {
        if let Ok(code) = MeaningCode::new(code_s) {
            out.push(Meaning {
                code,
                label: label.to_owned(),
                description: description.to_owned(),
                origin: Origin::Builtin,
                labels: BTreeMap::new(),
            });
        }
    }
    out
}

/// Validated signature meanings catalogue: fixed built-ins plus sorted tenant meanings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeaningCatalogue {
    builtins: Vec<Meaning>,
    tenants: BTreeMap<MeaningCode, Meaning>,
}

impl MeaningCatalogue {
    /// Catalogue containing only the four built-in codes.
    pub fn builtins_only() -> Self {
        Self { builtins: builtin_meanings(), tenants: BTreeMap::new() }
    }

    /// Builds the catalogue from an instantiated `fid-profile` [`ProfileDoc`]
    /// (REQ-001, REQ-002, REQ-003).
    pub fn from_profile(profile: &ProfileDoc) -> Result<Self, MeaningError> {
        let mut cat = Self::builtins_only();
        for tm in profile.params().tenant_meanings() {
            cat.add_tenant_raw(tm.code().as_str(), tm.label(), tm.description(), BTreeMap::new())?;
        }
        Ok(cat)
    }

    /// Loads the catalogue directly from profile JSON bytes — accepting either a
    /// canonical `ProfileDoc` (`{"name", "params": {"tenant_meanings": [...]}, "version"}`)
    /// or a tenant parameter file (`{"tenant_meanings": [...]}`) — so invalid tenant
    /// meaning definitions fail with `meaning.invalid` naming the offending code
    /// (REQ-002, REQ-003, AC-2, AC-3).
    pub fn from_profile_json(bytes: &[u8]) -> Result<Self, MeaningError> {
        if bytes.len() > MAX_INPUT_BYTES {
            return Err(MeaningError::invalid("<input>", "exceeds MAX_INPUT_BYTES"));
        }
        let val = parse_with_limits(bytes, MAX_INPUT_BYTES, MAX_JSON_DEPTH)
            .map_err(|e| MeaningError::invalid("<json>", format!("malformed JSON: {e}")))?;
        let CValue::Obj(root) = val else {
            return Err(MeaningError::invalid("<json>", "profile document must be a JSON object"));
        };

        let tenant_val = if let Some(params_val) = root.get("params") {
            let CValue::Obj(params_obj) = params_val else {
                return Err(MeaningError::invalid("<params>", "params must be a JSON object"));
            };
            params_obj.get("tenant_meanings")
        } else {
            root.get("tenant_meanings")
        };

        let mut cat = Self::builtins_only();
        let Some(arr_val) = tenant_val else {
            return Ok(cat);
        };
        let CValue::Arr(items) = arr_val else {
            return Err(MeaningError::invalid("<tenant_meanings>", "tenant_meanings must be an array"));
        };
        if items.len() > MAX_TENANT_MEANINGS {
            return Err(MeaningError::invalid("<tenant_meanings>", "exceeds MAX_TENANT_MEANINGS"));
        }

        for item in items {
            let CValue::Obj(entry) = item else {
                return Err(MeaningError::invalid("<entry>", "each tenant meaning must be a JSON object"));
            };
            let code_s = match entry.get("code") {
                Some(CValue::Str(s)) => s.as_str(),
                _ => return Err(MeaningError::invalid("<missing>", "tenant meaning missing string 'code'")),
            };
            let label_s = match entry.get("label") {
                Some(CValue::Str(s)) => s.as_str(),
                _ => return Err(MeaningError::invalid(code_s, "tenant meaning missing string 'label'")),
            };
            let desc_s = match entry.get("description") {
                Some(CValue::Str(s)) => s.as_str(),
                _ => return Err(MeaningError::invalid(code_s, "tenant meaning missing string 'description'")),
            };
            let mut labels = BTreeMap::new();
            if let Some(loc_val) = entry.get("labels") {
                let CValue::Obj(loc_map) = loc_val else {
                    return Err(MeaningError::invalid(code_s, "labels must be an object"));
                };
                if loc_map.len() > MAX_LOCALISED_LABELS {
                    return Err(MeaningError::invalid(code_s, "too many localised labels"));
                }
                for (lang, text_val) in loc_map {
                    if !is_lang_tag(lang) {
                        return Err(MeaningError::invalid(code_s, "invalid language tag in labels"));
                    }
                    let CValue::Str(t) = text_val else {
                        return Err(MeaningError::invalid(code_s, "localised label value must be a string"));
                    };
                    validate_text(code_s, "labels", t, MAX_LABEL_CHARS)?;
                    labels.insert(lang.clone(), t.clone());
                }
            }
            cat.add_tenant_raw(code_s, label_s, desc_s, labels)?;
        }

        Ok(cat)
    }

    fn add_tenant_raw(
        &mut self,
        code_raw: &str,
        label: &str,
        description: &str,
        labels: BTreeMap<String, String>,
    ) -> Result<(), MeaningError> {
        if BUILTIN_CODES.contains(&code_raw) {
            return Err(MeaningError::invalid(code_raw, "collides with a built-in meaning code"));
        }
        if !is_meaning_code_syntax(code_raw) {
            return Err(MeaningError::invalid(code_raw, "code does not match ^[A-Z][A-Z0-9_]{1,31}$"));
        }
        if !code_raw.starts_with("X_") {
            return Err(MeaningError::invalid(code_raw, "tenant meaning code must start with X_"));
        }
        let code = MeaningCode::new(code_raw)
            .map_err(|_| MeaningError::invalid(code_raw, "code does not match ^[A-Z][A-Z0-9_]{1,31}$"))?;
        if self.tenants.contains_key(&code) {
            return Err(MeaningError::invalid(code_raw, "duplicate tenant meaning code"));
        }
        validate_text(code_raw, "label", label, MAX_LABEL_CHARS)?;
        validate_text(code_raw, "description", description, MAX_DESCRIPTION_CHARS)?;
        self.tenants.insert(
            code.clone(),
            Meaning {
                code,
                label: label.to_owned(),
                description: description.to_owned(),
                origin: Origin::Profile,
                labels,
            },
        );
        Ok(())
    }

    /// Returns every meaning: built-ins first in fixed order, then tenant meanings
    /// sorted ascending by code (REQ-TRUST.ESIG.MEANING-004).
    pub fn list(&self) -> Vec<&Meaning> {
        let mut out = Vec::with_capacity(self.builtins.len() + self.tenants.len());
        out.extend(self.builtins.iter());
        out.extend(self.tenants.values());
        out
    }

    /// Looks up a meaning by exact code.
    pub fn get(&self, code: &str) -> Option<&Meaning> {
        if let Some(m) = self.builtins.iter().find(|m| m.code.as_str() == code) {
            return Some(m);
        }
        self.tenants.values().find(|m| m.code.as_str() == code)
    }
}

/// Checks `^[A-Z][A-Z0-9_]{1,31}$` (REQ-TRUST.ESIG.MEANING-002).
pub fn is_meaning_code_syntax(s: &str) -> bool {
    let b = s.as_bytes();
    (2..=32).contains(&b.len())
        && b[0].is_ascii_uppercase()
        && b[1..].iter().all(|&c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
}

fn cut_code(s: &str) -> String {
    s.chars().take(64).collect()
}

fn is_lang_tag(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_LANG_TAG_BYTES
        && !s.starts_with('-')
        && !s.ends_with('-')
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn is_forbidden_text_char(c: char) -> bool {
    let u = u32::from(c);
    u < 0x20 || (0x7f..=0x9f).contains(&u) || (0x202a..=0x202e).contains(&u) || (0x2066..=0x2069).contains(&u)
}

fn validate_text(code: &str, field: &'static str, s: &str, max_chars: usize) -> Result<(), MeaningError> {
    let n = s.chars().count();
    if n == 0 {
        return Err(MeaningError::invalid(code, format!("{field} must not be empty")));
    }
    if n > max_chars {
        return Err(MeaningError::invalid(code, format!("{field} exceeds {max_chars} characters")));
    }
    if s.chars().any(is_forbidden_text_char) {
        return Err(MeaningError::invalid(code, format!("{field} contains a control or bidi character")));
    }
    Ok(())
}
