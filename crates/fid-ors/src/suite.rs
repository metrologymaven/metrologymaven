//! The suite manifest `ors-suite/v1`, suite loading and the command errors.
//!
//! A suite is a manifest plus its case documents. Loading checks only what makes the input
//! readable: bounded JSON with no duplicate keys, a well-formed manifest, and exactly one
//! case document per listed case ID. Everything a case says is checked later, as findings
//! ([`crate::check_suite`]). A refusal here is the command error `suite_invalid`.

use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue};
use fid_gps::dmt::SemVer;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// The `schema` value of a case document.
pub const CASE_SCHEMA: &str = "ors-case/v1";
/// The `schema` value of a suite manifest.
pub const SUITE_SCHEMA: &str = "ors-suite/v1";
/// Longest free text, in Unicode scalar values (REQ-004).
pub const MAX_TEXT_CHARS: usize = 280;
/// Most cases in one suite.
pub const MAX_CASES: usize = 5000;
/// Most changelog entries in one manifest.
pub const MAX_CHANGELOG: usize = 1000;
/// Largest case document: the 4 MiB dataset bound plus 64 KiB for the ORS fields.
pub const MAX_CASE_BYTES: usize = 4 * 1024 * 1024 + 64 * 1024;
/// Deepest case document: the dataset's 32 levels inside the case object.
pub const MAX_CASE_DEPTH: usize = 33;
/// Largest manifest.
pub const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;
/// Deepest manifest.
pub const MAX_MANIFEST_DEPTH: usize = 4;
/// Largest bundle (`{manifest, cases}` in one document, as the fixtures use).
pub const MAX_BUNDLE_BYTES: usize = 64 * 1024 * 1024;
/// The licence of a manifest that names none.
pub const DEFAULT_LICENCE: &str = "CC-BY-4.0";
/// Longest case ID or suite name, in bytes.
pub const MAX_ID: usize = 96;
/// Longest manifest licence string, in bytes. A longer one, or one holding control or format
/// characters, is unreadable input; any other string loads and an unopen one is the finding
/// `licence_not_open` (REQ-006).
pub const MAX_LICENCE: usize = 128;

/// Every error of the commands. Fields are schema names and indexes only; reasons are
/// static. No input text appears in an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrsError {
    /// The suite cannot be read (malformed manifest, missing or extra case document).
    SuiteInvalid {
        /// Where.
        field: String,
        /// Static reason.
        reason: &'static str,
    },
    /// No suite with this handle.
    SuiteNotFound {
        /// `suite` or `previous`.
        field: &'static str,
    },
    /// Malformed CLI arguments or MCP parameters.
    BadParams {
        /// Where.
        field: String,
        /// Static reason.
        reason: &'static str,
    },
    /// A suite with this handle is already held with other content, or is already installed
    /// (staging), so the handle would no longer name one suite.
    HandleConflict {
        /// `suite`.
        field: &'static str,
        /// Static reason.
        reason: &'static str,
    },
    /// `previous` is not the release the version rule compares against (the highest installed
    /// release of the same suite below the checked version).
    PreviousMismatch,
    /// A value this crate built could not be encoded (never expected).
    Internal {
        /// Static reason.
        reason: &'static str,
    },
}

impl OrsError {
    /// The stable code.
    pub fn code(&self) -> &'static str {
        match self {
            OrsError::SuiteInvalid { .. } => "suite_invalid",
            OrsError::SuiteNotFound { .. } => "suite_not_found",
            OrsError::BadParams { .. } => "bad_params",
            OrsError::HandleConflict { .. } => "handle_conflict",
            OrsError::PreviousMismatch => "previous_mismatch",
            OrsError::Internal { .. } => "internal_error",
        }
    }

    /// The field path.
    pub fn field(&self) -> &str {
        match self {
            OrsError::SuiteInvalid { field, .. } | OrsError::BadParams { field, .. } => field,
            OrsError::SuiteNotFound { field } | OrsError::HandleConflict { field, .. } => field,
            OrsError::PreviousMismatch => "previous",
            OrsError::Internal { .. } => "result",
        }
    }

    fn reason(&self) -> &'static str {
        match self {
            OrsError::SuiteInvalid { reason, .. } | OrsError::BadParams { reason, .. } => reason,
            OrsError::SuiteNotFound { .. } => "no suite with this handle",
            OrsError::HandleConflict { reason, .. } => reason,
            OrsError::PreviousMismatch => "not the highest installed release below this version",
            OrsError::Internal { reason } => reason,
        }
    }
}

impl fmt::Display for OrsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", self.code(), self.field(), self.reason())
    }
}

impl std::error::Error for OrsError {}

pub(crate) fn invalid(field: impl Into<String>, reason: &'static str) -> OrsError {
    OrsError::SuiteInvalid { field: field.into(), reason }
}

pub(crate) fn bad_params(field: impl Into<String>, reason: &'static str) -> OrsError {
    OrsError::BadParams { field: field.into(), reason }
}

// ------------------------------------------------------------------ grammars

fn lower_alnum(c: u8) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}

/// A case ID: `ors:<suite>/<name>`, suite `[a-z0-9][a-z0-9_-]*`, name `[a-z0-9][a-z0-9_.-]*`,
/// at most 96 bytes (within the DMT's `ors_cases` grammar).
pub fn case_id_ok(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("ors:") else { return false };
    let Some((suite, name)) = rest.split_once('/') else { return false };
    let part = |p: &str, extra: &[u8]| {
        let b = p.as_bytes();
        b.first().is_some_and(|&c| lower_alnum(c)) && b.iter().all(|&c| lower_alnum(c) || extra.contains(&c))
    };
    s.len() <= MAX_ID && part(suite, b"_-") && part(name, b"_.-")
}

/// A suite name: `ors-[a-z0-9][a-z0-9-]*`, at most 64 bytes.
pub fn suite_name_ok(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("ors-") else { return false };
    let b = rest.as_bytes();
    s.len() <= 64 && b.first().is_some_and(|&c| lower_alnum(c)) && b.iter().all(|&c| lower_alnum(c) || c == b'-')
}

/// Splits a handle `ors-<suite>@<semver>`.
pub fn parse_handle(s: &str) -> Option<(&str, SemVer)> {
    let (name, v) = s.rsplit_once('@')?;
    if !suite_name_ok(name) {
        return None;
    }
    Some((name, SemVer::parse(v)?))
}

// ------------------------------------------------------------------ manifest

/// One changelog entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangelogEntry {
    /// The suite version the entry belongs to.
    pub version: SemVer,
    /// The cases whose expected result or tolerance changed.
    pub cases: Vec<String>,
    /// Why (free text, at most [`MAX_TEXT_CHARS`] or a `text_too_long` finding).
    pub reason: String,
}

/// A suite manifest `ors-suite/v1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuiteManifest {
    /// The suite name, `ors-<suite>`.
    pub name: String,
    /// The suite version.
    pub version: SemVer,
    /// The suite licence as declared ([`DEFAULT_LICENCE`] when absent). It need not be an SPDX
    /// ID: anything outside the open list is the finding `licence_not_open`.
    pub licence: String,
    /// The digest of the DMT table the suite pins.
    pub dmt_digest: Digest,
    /// The case IDs, strictly ascending.
    pub cases: Vec<String>,
    /// The changelog, in file order.
    pub changelog: Vec<ChangelogEntry>,
}

fn obj<'a>(v: &'a CValue, field: &str) -> Result<&'a BTreeMap<String, CValue>, OrsError> {
    match v {
        CValue::Obj(o) => Ok(o),
        _ => Err(invalid(field, "not an object")),
    }
}

fn string<'a>(o: &'a BTreeMap<String, CValue>, key: &str, field: &str) -> Result<&'a str, OrsError> {
    match o.get(key) {
        Some(CValue::Str(s)) => Ok(s),
        Some(_) => Err(invalid(field, "not a string")),
        None => Err(invalid(field, "required")),
    }
}

fn ids(v: Option<&CValue>, field: &str, max: usize) -> Result<Vec<String>, OrsError> {
    let Some(CValue::Arr(a)) = v else { return Err(invalid(field, "not an array")) };
    if a.len() > max {
        return Err(invalid(field, "too many entries"));
    }
    let mut out: Vec<String> = Vec::with_capacity(a.len());
    for (i, x) in a.iter().enumerate() {
        match x {
            CValue::Str(s) if case_id_ok(s) => {
                if out.last().is_some_and(|p| p.as_str() >= s.as_str()) {
                    return Err(invalid(format!("{field}.{i}"), "case IDs not strictly ascending"));
                }
                out.push(s.clone());
            }
            _ => return Err(invalid(format!("{field}.{i}"), "not a case ID")),
        }
    }
    Ok(out)
}

impl SuiteManifest {
    /// Reads a manifest value; any departure is `suite_invalid` naming the field.
    pub fn from_value(v: &CValue) -> Result<SuiteManifest, OrsError> {
        let o = obj(v, "manifest")?;
        if o.keys().any(|k| {
            !matches!(k.as_str(), "changelog" | "cases" | "dmt_digest" | "licence" | "name" | "schema" | "version")
        }) {
            return Err(invalid("manifest", "unknown field"));
        }
        if string(o, "schema", "schema")? != SUITE_SCHEMA {
            return Err(invalid("schema", "not ors-suite/v1"));
        }
        let name = string(o, "name", "name")?;
        if !suite_name_ok(name) {
            return Err(invalid("name", "not ors-<suite>"));
        }
        let version = SemVer::parse(string(o, "version", "version")?).ok_or_else(|| invalid("version", "not x.y.z"))?;
        let licence = match o.get("licence") {
            None => DEFAULT_LICENCE.to_string(),
            Some(CValue::Str(s)) if s.len() <= MAX_LICENCE && !s.chars().any(fid_rdl::is_control_or_format) => {
                s.clone()
            }
            Some(CValue::Str(_)) => return Err(invalid("licence", "too long or holds control characters")),
            Some(_) => return Err(invalid("licence", "not a string")),
        };
        let dmt_digest =
            Digest::parse(string(o, "dmt_digest", "dmt_digest")?).map_err(|_| invalid("dmt_digest", "not a digest"))?;
        let cases = ids(o.get("cases"), "cases", MAX_CASES)?;
        if cases.is_empty() {
            return Err(invalid("cases", "empty"));
        }
        let Some(CValue::Arr(log)) = o.get("changelog") else { return Err(invalid("changelog", "not an array")) };
        if log.len() > MAX_CHANGELOG {
            return Err(invalid("changelog", "too many entries"));
        }
        let mut changelog = Vec::with_capacity(log.len());
        for (i, e) in log.iter().enumerate() {
            let f = format!("changelog.{i}");
            let eo = obj(e, &f)?;
            if eo.keys().any(|k| !matches!(k.as_str(), "cases" | "reason" | "version")) {
                return Err(invalid(f, "unknown field"));
            }
            let version = SemVer::parse(string(eo, "version", &format!("{f}.version"))?)
                .ok_or_else(|| invalid(format!("{f}.version"), "not x.y.z"))?;
            let reason = string(eo, "reason", &format!("{f}.reason"))?;
            // Long reasons are a finding; only an absurd one is unreadable input.
            if reason.len() > MAX_CASE_BYTES {
                return Err(invalid(format!("{f}.reason"), "too large"));
            }
            let mut cs = ids(eo.get("cases"), &format!("{f}.cases"), MAX_CASES)?;
            cs.dedup();
            changelog.push(ChangelogEntry { version, cases: cs, reason: reason.to_string() });
        }
        Ok(SuiteManifest { name: name.to_string(), version, licence, dmt_digest, cases, changelog })
    }

    /// The handle `ors-<suite>@<semver>`.
    pub fn handle(&self) -> String {
        format!("{}@{}", self.name, self.version)
    }
}

// ------------------------------------------------------------------ suite

/// A readable suite: its manifest and one raw case document per listed case ID.
#[derive(Debug, Clone, PartialEq)]
pub struct Suite {
    manifest: SuiteManifest,
    cases: BTreeMap<String, CValue>,
    digest: Digest,
}

fn case_key(v: &CValue, field: &str) -> Result<String, OrsError> {
    match v {
        CValue::Obj(o) => match o.get("case") {
            Some(CValue::Str(s)) if case_id_ok(s) => Ok(s.clone()),
            _ => Err(invalid(format!("{field}.case"), "not a case ID")),
        },
        _ => Err(invalid(field, "not an object")),
    }
}

impl Suite {
    /// A suite from a manifest value and case values: every listed ID has exactly one case
    /// document and every document is listed.
    pub fn from_values(manifest_value: &CValue, cases: &[CValue]) -> Result<Suite, OrsError> {
        let manifest = SuiteManifest::from_value(manifest_value)?;
        if cases.len() != manifest.cases.len() {
            return Err(invalid("cases", "case documents do not match the manifest"));
        }
        let mut map = BTreeMap::new();
        for (i, c) in cases.iter().enumerate() {
            let key = case_key(c, &format!("cases.{i}"))?;
            if manifest.cases.binary_search(&key).is_err() {
                return Err(invalid(format!("cases.{i}.case"), "not listed in the manifest"));
            }
            if map.insert(key, c.clone()).is_some() {
                return Err(invalid(format!("cases.{i}.case"), "duplicate case document"));
            }
        }
        let digest = suite_digest(manifest_value, &manifest, &map)?;
        Ok(Suite { manifest, cases: map, digest })
    }

    /// A suite from manifest and case file bytes (bounded, duplicate keys refused).
    pub fn from_json(manifest: &[u8], cases: &[&[u8]]) -> Result<Suite, OrsError> {
        let m = parse_with_limits(manifest, MAX_MANIFEST_BYTES, MAX_MANIFEST_DEPTH)
            .map_err(|_| invalid("manifest", "not acceptable JSON"))?;
        if cases.len() > MAX_CASES {
            return Err(invalid("cases", "too many case documents"));
        }
        let mut vs = Vec::with_capacity(cases.len());
        for (i, c) in cases.iter().enumerate() {
            vs.push(
                parse_with_limits(c, MAX_CASE_BYTES, MAX_CASE_DEPTH)
                    .map_err(|_| invalid(format!("cases.{i}"), "not acceptable JSON"))?,
            );
        }
        Suite::from_values(&m, &vs)
    }

    /// A suite from one bundle document `{manifest, cases: [...]}`.
    pub fn from_bundle(bytes: &[u8]) -> Result<Suite, OrsError> {
        let v = parse_with_limits(bytes, MAX_BUNDLE_BYTES, MAX_CASE_DEPTH + 2)
            .map_err(|_| invalid("bundle", "not acceptable JSON"))?;
        let o = obj(&v, "bundle")?;
        if o.keys().any(|k| k != "manifest" && k != "cases") {
            return Err(invalid("bundle", "unknown field"));
        }
        let m = o.get("manifest").ok_or_else(|| invalid("manifest", "required"))?;
        let Some(CValue::Arr(cs)) = o.get("cases") else { return Err(invalid("cases", "not an array")) };
        if cs.len() > MAX_CASES {
            return Err(invalid("cases", "too many case documents"));
        }
        Suite::from_values(m, cs)
    }

    /// The manifest.
    pub fn manifest(&self) -> &SuiteManifest {
        &self.manifest
    }

    /// The raw case document with this ID.
    pub fn case(&self, id: &str) -> Option<&CValue> {
        self.cases.get(id)
    }

    /// The handle.
    pub fn handle(&self) -> String {
        self.manifest.handle()
    }

    /// The suite digest: SHA-256 of the canonical (RFC 8785) bytes of
    /// `{"cases": [case documents in manifest order], "manifest": <manifest as given>}`.
    /// Two suites under one handle with any difference in content have different digests.
    pub fn digest(&self) -> Digest {
        self.digest
    }
}

fn suite_digest(
    manifest_value: &CValue,
    manifest: &SuiteManifest,
    cases: &BTreeMap<String, CValue>,
) -> Result<Digest, OrsError> {
    let mut ordered = Vec::with_capacity(manifest.cases.len());
    for id in &manifest.cases {
        ordered.push(cases.get(id).cloned().ok_or_else(|| invalid("cases", "case document missing"))?);
    }
    let mut m = BTreeMap::new();
    m.insert("cases".to_string(), CValue::Arr(ordered));
    m.insert("manifest".to_string(), manifest_value.clone());
    let bytes = to_canonical_bytes(&CValue::Obj(m)).map_err(|_| invalid("manifest", "no canonical form"))?;
    Ok(Digest::from_bytes(digest_bytes(&bytes).0))
}
