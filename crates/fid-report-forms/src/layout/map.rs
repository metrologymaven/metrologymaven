//! Form field maps, `form_field` block kind & resolver, locked built-in form template
//! registration, clearance release gate and clean-room label verification
//! (`RPT.FORMS.LAYOUT.MAP`, `REQ-001`..`REQ-008`, `AC-1`..`AC-6`).
//!
//! Zero floating-point types or arithmetic (`RPT` float rule): numeric form values pass
//! exclusively through [`fid_report::fmt::ValueText`] (`RPT.MODEL.FMT`), and text values stay
//! wrapped in [`Untrusted<Text>`] (`CONSTITUTION` C6).

use crate::hdr::{cleanroom_phrase_digest, normalise_cleanroom_phrase};
use fid_canon::{digest_bytes as canon_sha256, parse_with_limits, to_canonical_bytes, CValue};
use fid_project::Uuid;
use fid_registry::{CommandDef, DefError};
use fid_report::bind::{
    find_field, BindError, Binding, BlockResolver, FieldId, FieldSource, FieldValue, Lookup, ResolveContext, Resolved,
    ValueBinding,
};
use fid_report::doc::kind::{BlockKind, PayloadError};
use fid_report::doc::{BlockId, BlockKindId, CanonicalJson, TemplateRef, Text, Untrusted};
use fid_report::fmt::ValueText;
use fid_report::tmpl::format::{Clearance, Template};
use fid_report::tmpl::ver::{BuiltinRegistry, VerError};
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Current canonical JSON schema version of [`FieldMap`].
pub const FIELD_MAP_SCHEMA_VERSION: u32 = 1;
/// Maximum byte length of a rendered standard citation (`REQ-RPT.FORMS.LAYOUT.MAP-003`).
pub const MAX_CITATION_BYTES: usize = 64;
/// Maximum entries in one [`FieldMap`].
pub const MAX_FIELD_MAP_ENTRIES: usize = 512;
/// Maximum byte length of a `FieldMap` JSON document.
pub const MAX_FIELD_MAP_BYTES: usize = 512 * 1024;
/// Maximum byte length of a clean-room phrase hash file.
pub const MAX_CLEANROOM_FILE_BYTES: usize = 1024 * 1024;
/// Maximum byte length of a label catalog key or source path.
pub const MAX_KEY_BYTES: usize = 128;

/// Block kind identifier for form field blocks (`RPT.FORMS.LAYOUT.MAP`).
pub const FORM_FIELD_KIND: &str = "form_field";
/// Registry command ID for listing built-in FAI form layouts (`R0`).
pub const SHOW_LAYOUT_FAI: &str = "report.show_layout_fai@1";
/// Registry command ID for listing built-in PPAP form layouts (`R0`).
pub const SHOW_LAYOUT_PPAP: &str = "report.show_layout_ppap@1";

const LIFECYCLE_SINCE: &str = "2026-11-22";

/// Which industry form a template and field map belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FormKind {
    FaiForm1,
    FaiForm2,
    FaiForm3,
    PpapDim,
}

impl FormKind {
    /// Canonical wire string for this form kind.
    pub fn as_str(self) -> &'static str {
        match self {
            FormKind::FaiForm1 => "fai_form_1",
            FormKind::FaiForm2 => "fai_form_2",
            FormKind::FaiForm3 => "fai_form_3",
            FormKind::PpapDim => "ppap_dim",
        }
    }

    /// Parses a wire string into a [`FormKind`].
    pub fn parse(s: &str) -> Result<FormKind, LayoutError> {
        match s {
            "fai_form_1" => Ok(FormKind::FaiForm1),
            "fai_form_2" => Ok(FormKind::FaiForm2),
            "fai_form_3" => Ok(FormKind::FaiForm3),
            "ppap_dim" => Ok(FormKind::PpapDim),
            _ => Err(LayoutError::FieldMapInvalid("unknown form kind")),
        }
    }

    /// Whether this form belongs to the AS9102 FAI family (Forms 1 to 3).
    pub fn is_fai(self) -> bool {
        matches!(self, FormKind::FaiForm1 | FormKind::FaiForm2 | FormKind::FaiForm3)
    }

    /// Whether this form belongs to the PPAP dimensional family.
    pub fn is_ppap(self) -> bool {
        matches!(self, FormKind::PpapDim)
    }
}

/// Errors produced by form field-map validation, `form_field` resolution, locked registration,
/// clearance release gating and clean-room label checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutError {
    /// `report.template_locked`: built-in form template is locked against modification.
    TemplateLocked(String),
    /// `report.citation_invalid`: citation exceeds 64 bytes or contains text beyond
    /// designator + revision/edition + clause (`REQ-003`).
    CitationInvalid { detail: String },
    /// `report.field_unknown`: a `form_field` block references a field ID absent from its
    /// template's field map (`REQ-004`).
    FieldUnknown { block: BlockId, field_id: String },
    /// `report.fieldmap_invalid`: duplicate field ID in a field map (`REQ-002`, `AC-3`).
    DuplicateField { field_id: String },
    /// `report.fieldmap_invalid`: structural or ordering error in a field map.
    FieldMapInvalid(&'static str),
    /// `report.template_invalid`: built-in form template is missing required lock/clearance or
    /// failed template registration.
    TemplateInvalid(String),
    /// `report.clearance_pending`: one or more shipped form templates remain `pending` under
    /// the `release` build profile (`REQ-006`, `AC-5`).
    ClearancePending { templates: Vec<String> },
    /// `report.cleanroom_violation`: a shipped label's normalised SHA-256 matches the
    /// clean-room forbidden phrase hash list (`REQ-008`, `AC-6`).
    CleanroomViolation { label_key: String, digest: Digest },
    /// `report.formatter_failed`: formatting a numeric form field via `RPT.MODEL.FMT` failed.
    FormatterFailed(String),
    /// `report.bad_params`: invalid parameter or payload.
    BadParams(&'static str),
}

impl LayoutError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            LayoutError::TemplateLocked(_) => "report.template_locked",
            LayoutError::CitationInvalid { .. } => "report.citation_invalid",
            LayoutError::FieldUnknown { .. } => "report.field_unknown",
            LayoutError::DuplicateField { .. } | LayoutError::FieldMapInvalid(_) => "report.fieldmap_invalid",
            LayoutError::TemplateInvalid(_) => "report.template_invalid",
            LayoutError::ClearancePending { .. } => "report.clearance_pending",
            LayoutError::CleanroomViolation { .. } => "report.cleanroom_violation",
            LayoutError::FormatterFailed(_) => "report.formatter_failed",
            LayoutError::BadParams(_) => "report.bad_params",
        }
    }

    /// Canonical `--json` / MCP error payload.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("code".to_string(), CValue::Str(self.code().to_string()));
        match self {
            LayoutError::TemplateLocked(t) | LayoutError::TemplateInvalid(t) => {
                o.insert("detail".to_string(), CValue::Str(t.clone()));
            }
            LayoutError::CitationInvalid { detail } => {
                o.insert("detail".to_string(), CValue::Str(detail.clone()));
            }
            LayoutError::FieldUnknown { block, field_id } => {
                o.insert("block".to_string(), CValue::Str(block.uuid().to_string()));
                o.insert("field_id".to_string(), CValue::Str(field_id.clone()));
            }
            LayoutError::DuplicateField { field_id } => {
                o.insert("duplicate_field_id".to_string(), CValue::Str(field_id.clone()));
            }
            LayoutError::FieldMapInvalid(reason) | LayoutError::BadParams(reason) => {
                o.insert("reason".to_string(), CValue::Str((*reason).to_string()));
            }
            LayoutError::ClearancePending { templates } => {
                o.insert(
                    "templates".to_string(),
                    CValue::Arr(templates.iter().map(|s| CValue::Str(s.clone())).collect()),
                );
            }
            LayoutError::CleanroomViolation { label_key, digest } => {
                o.insert("label_key".to_string(), CValue::Str(label_key.clone()));
                o.insert("digest".to_string(), CValue::Str(digest.as_str().to_string()));
            }
            LayoutError::FormatterFailed(detail) => {
                o.insert("detail".to_string(), CValue::Str(detail.clone()));
            }
        }
        CValue::Obj(o)
    }
}

impl From<VerError> for LayoutError {
    fn from(e: VerError) -> Self {
        match e {
            VerError::Locked(id) => LayoutError::TemplateLocked(id.uuid().to_string()),
            other => LayoutError::TemplateInvalid(other.to_string()),
        }
    }
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayoutError::TemplateLocked(t) => write!(f, "{}: {t}", self.code()),
            LayoutError::CitationInvalid { detail } => write!(f, "{}: {detail}", self.code()),
            LayoutError::FieldUnknown { block, field_id } => {
                write!(f, "{}: block {} field {}", self.code(), block.uuid(), field_id)
            }
            LayoutError::DuplicateField { field_id } => {
                write!(f, "{}: duplicate field_id {}", self.code(), field_id)
            }
            LayoutError::FieldMapInvalid(r) | LayoutError::BadParams(r) => {
                write!(f, "{}: {r}", self.code())
            }
            LayoutError::TemplateInvalid(d) | LayoutError::FormatterFailed(d) => {
                write!(f, "{}: {d}", self.code())
            }
            LayoutError::ClearancePending { templates } => {
                write!(f, "{}: pending clearance for {}", self.code(), templates.join(", "))
            }
            LayoutError::CleanroomViolation { label_key, digest } => {
                write!(f, "{}: label {} matched forbidden hash {}", self.code(), label_key, digest)
            }
        }
    }
}

impl std::error::Error for LayoutError {}

// ------------------------------------------------------------------ citation

/// A strictly bounded standard citation consisting solely of a standard designator, a
/// revision or edition, and a clause/section/field number (`REQ-RPT.FORMS.LAYOUT.MAP-003`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Citation {
    pub designator: String,
    pub revision: String,
    pub clause: String,
}

impl Citation {
    /// Constructs and validates a [`Citation`].
    pub fn new(
        designator: impl Into<String>,
        revision: impl Into<String>,
        clause: impl Into<String>,
    ) -> Result<Citation, LayoutError> {
        let c = Citation { designator: designator.into(), revision: revision.into(), clause: clause.into() };
        c.validate()?;
        Ok(c)
    }

    /// Canonical compact rendering `"<designator> <revision>, <clause>"`.
    pub fn rendered(&self) -> String {
        format!("{} {}, {}", self.designator, self.revision, self.clause)
    }

    /// Validates that the citation contains only a designator, revision/edition and clause
    /// number and renders to at most [`MAX_CITATION_BYTES`] (64) bytes (`REQ-003`).
    pub fn validate(&self) -> Result<(), LayoutError> {
        let bad = |msg: &str| LayoutError::CitationInvalid { detail: msg.to_string() };
        if !is_valid_designator(&self.designator) {
            return Err(bad("invalid standard designator"));
        }
        if !is_valid_revision(&self.revision) {
            return Err(bad("invalid revision or edition token"));
        }
        if !is_valid_clause(&self.clause) {
            return Err(bad("clause contains disallowed text or prose"));
        }
        let rendered = self.rendered();
        if rendered.len() > MAX_CITATION_BYTES {
            return Err(bad("citation exceeds 64 bytes"));
        }
        Ok(())
    }

    /// Serialises to canonical [`CValue`].
    pub fn to_value(&self) -> CValue {
        obj([
            ("clause", CValue::Str(self.clause.clone())),
            ("designator", CValue::Str(self.designator.clone())),
            ("revision", CValue::Str(self.revision.clone())),
        ])
    }

    /// Parses and validates a [`Citation`] from a JSON object.
    pub fn from_value(v: &CValue) -> Result<Citation, LayoutError> {
        let CValue::Obj(o) = v else {
            return Err(LayoutError::CitationInvalid { detail: "citation must be an object".to_string() });
        };
        for k in o.keys() {
            if !matches!(k.as_str(), "designator" | "revision" | "clause") {
                return Err(LayoutError::CitationInvalid { detail: format!("unexpected citation key {k}") });
            }
        }
        let get = |k: &str| match o.get(k) {
            Some(CValue::Str(s)) => Ok(s.clone()),
            _ => Err(LayoutError::CitationInvalid { detail: format!("missing or non-string {k}") }),
        };
        Citation::new(get("designator")?, get("revision")?, get("clause")?)
    }
}

fn is_valid_designator(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 20 {
        return false;
    }
    if !b[0].is_ascii_uppercase() {
        return false;
    }
    // Allow e.g. "AS9102", "SAE AS9102", "AIAG PPAP", "ISO 1101", "ASME Y14.5".
    // Maximum 2 tokens, each strictly alphanumeric / '-' / '.' / '/'.
    let tokens: Vec<&str> = s.split(' ').collect();
    if tokens.is_empty() || tokens.len() > 2 {
        return false;
    }
    tokens.iter().all(|tok| {
        !tok.is_empty()
            && tok.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || matches!(c, b'-' | b'.' | b'/'))
    })
}

fn is_valid_revision(s: &str) -> bool {
    if s.is_empty() || s.len() > 16 {
        return false;
    }
    let tokens: Vec<&str> = s.split(' ').collect();
    match tokens.as_slice() {
        [single] => is_revision_value(single),
        [prefix, val] => {
            (matches!(*prefix, "Rev" | "Rev." | "Ed" | "Ed." | "Edition") && is_revision_value(val))
                || (is_revision_value(prefix) && matches!(*val, "Ed" | "Ed." | "Edition"))
        }
        _ => false,
    }
}

fn is_revision_value(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 10 {
        return false;
    }
    // Single uppercase rev letter ("C"), numeric year ("2023"), ordinal ("4th"), or dotted numeric ("2.1").
    if b.len() == 1 && b[0].is_ascii_uppercase() {
        return true;
    }
    if b.iter().all(|c| c.is_ascii_digit() || *c == b'.' || *c == b'-') && b[0].is_ascii_digit() {
        return true;
    }
    // Ordinals like "1st", "2nd", "3rd", "4th"
    if let Some(num) = s
        .strip_suffix("st")
        .or_else(|| s.strip_suffix("nd"))
        .or_else(|| s.strip_suffix("rd"))
        .or_else(|| s.strip_suffix("th"))
    {
        return !num.is_empty() && num.bytes().all(|c| c.is_ascii_digit());
    }
    false
}

fn is_valid_clause(s: &str) -> bool {
    if s.is_empty() || s.len() > 32 {
        return false;
    }
    // Split on ", " into up to 2 sub-references (e.g. "Form 1, Field 5" or "Clause 4.2.1").
    let parts: Vec<&str> = s.split(", ").collect();
    if parts.is_empty() || parts.len() > 2 {
        return false;
    }
    parts.iter().all(|part| is_valid_clause_part(part))
}

fn is_valid_clause_part(part: &str) -> bool {
    if part.is_empty() {
        return false;
    }
    // Optional leading section sign "§" attached directly to a clause number ("§4.2.1").
    if let Some(rest) = part.strip_prefix('§') {
        return is_clause_number(rest);
    }
    let tokens: Vec<&str> = part.split(' ').collect();
    match tokens.as_slice() {
        [num] => is_clause_number(num),
        [label, num] => is_clause_label(label) && is_clause_number(num),
        _ => false,
    }
}

fn is_clause_label(s: &str) -> bool {
    matches!(
        s,
        "Clause"
            | "Section"
            | "Sec"
            | "Sec."
            | "Form"
            | "Field"
            | "Table"
            | "Annex"
            | "Appendix"
            | "Part"
            | "Block"
            | "Item"
    )
}

fn is_clause_number(s: &str) -> bool {
    if s.is_empty() || s.len() > 16 {
        return false;
    }
    // Range like "1-3" or single number/identifier like "4.2.1", "5a", "A", "A.1".
    let segments: Vec<&str> = s.split('-').collect();
    if segments.is_empty() || segments.len() > 2 {
        return false;
    }
    segments.iter().all(|seg| {
        if seg.is_empty() {
            return false;
        }
        let dot_parts: Vec<&str> = seg.split('.').collect();
        if dot_parts.is_empty() || dot_parts.len() > 5 {
            return false;
        }
        dot_parts.iter().enumerate().all(|(idx, dp)| {
            let b = dp.as_bytes();
            if b.is_empty() || b.len() > 5 {
                return false;
            }
            if idx == 0 && b.len() == 1 && b[0].is_ascii_uppercase() {
                return true;
            }
            // Digits with optional single trailing lowercase letter (e.g. "12", "5a")
            let (digits, tail) = if b.last().is_some_and(u8::is_ascii_lowercase) && b.len() > 1 {
                (&b[..b.len() - 1], true)
            } else {
                (b, false)
            };
            let _ = tail;
            !digits.is_empty() && digits.iter().all(u8::is_ascii_digit)
        })
    })
}

// ------------------------------------------------------------------ SourcePath & FieldMap

/// Data source path inside a form data provider (e.g. `fai.header.part_number`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourcePath(String);

impl SourcePath {
    /// Validates and constructs a [`SourcePath`].
    pub fn new(s: &str) -> Result<SourcePath, LayoutError> {
        if !is_valid_dotted_key(s) {
            return Err(LayoutError::FieldMapInvalid("invalid source path"));
        }
        Ok(SourcePath(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_valid_dotted_key(s: &str) -> bool {
    if s.is_empty() || s.len() > MAX_KEY_BYTES {
        return false;
    }
    s.split('.').all(|seg| {
        let b = seg.as_bytes();
        !b.is_empty()
            && b[0].is_ascii_lowercase()
            && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
    })
}

/// One mapped field in a [`FieldMap`] (`REQ-RPT.FORMS.LAYOUT.MAP-002`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldEntry {
    pub field_id: FieldId,
    pub label_key: String,
    pub source: SourcePath,
    pub citation: Option<Citation>,
}

impl FieldEntry {
    /// Validates this entry's label key, source path and optional citation.
    pub fn validate(&self) -> Result<(), LayoutError> {
        if !is_valid_dotted_key(&self.label_key) {
            return Err(LayoutError::FieldMapInvalid("invalid label_key"));
        }
        let _ = SourcePath::new(self.source.as_str())?;
        if let Some(c) = &self.citation {
            c.validate()?;
        }
        Ok(())
    }

    /// Serialises to canonical [`CValue`].
    pub fn to_value(&self) -> CValue {
        let citation = match &self.citation {
            None => CValue::Null,
            Some(c) => c.to_value(),
        };
        obj([
            ("citation", citation),
            ("field_id", CValue::Str(self.field_id.as_str().to_string())),
            ("label_key", CValue::Str(self.label_key.clone())),
            ("source", CValue::Str(self.source.as_str().to_string())),
        ])
    }

    /// Parses one [`FieldEntry`] from JSON.
    pub fn from_value(v: &CValue) -> Result<FieldEntry, LayoutError> {
        let CValue::Obj(o) = v else {
            return Err(LayoutError::FieldMapInvalid("field entry must be an object"));
        };
        for k in o.keys() {
            if !matches!(k.as_str(), "field_id" | "label_key" | "source" | "citation") {
                return Err(LayoutError::FieldMapInvalid("unknown key in field entry"));
            }
        }
        let field_id_str = req_str(o, "field_id")?;
        let field_id = FieldId::new(field_id_str).ok_or(LayoutError::FieldMapInvalid("invalid field_id"))?;
        let label_key = req_str(o, "label_key")?.to_string();
        let source = SourcePath::new(req_str(o, "source")?)?;
        let citation = match o.get("citation") {
            None | Some(CValue::Null) => None,
            Some(cv) => Some(Citation::from_value(cv)?),
        };
        let entry = FieldEntry { field_id, label_key, source, citation };
        entry.validate()?;
        Ok(entry)
    }
}

/// Versioned, canonical-JSON field map published alongside every built-in form template
/// (`REQ-RPT.FORMS.LAYOUT.MAP-002`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldMap {
    pub schema_version: u32,
    pub form: FormKind,
    pub fields: Vec<FieldEntry>,
}

impl FieldMap {
    /// Validates schema version, bounds, uniqueness, ascending sort order by `field_id`, and
    /// every entry's label key, source path and citation (`REQ-002`, `REQ-003`, `AC-3`).
    pub fn validate(&self) -> Result<(), LayoutError> {
        if self.schema_version != FIELD_MAP_SCHEMA_VERSION {
            return Err(LayoutError::FieldMapInvalid("unsupported field map schema_version"));
        }
        if self.fields.len() > MAX_FIELD_MAP_ENTRIES {
            return Err(LayoutError::FieldMapInvalid("field map exceeds maximum entries"));
        }
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut prev: Option<&str> = None;
        for entry in &self.fields {
            let id = entry.field_id.as_str();
            if !seen.insert(id) {
                return Err(LayoutError::DuplicateField { field_id: id.to_string() });
            }
            if let Some(p) = prev {
                if id < p {
                    return Err(LayoutError::FieldMapInvalid("fields must be sorted ascending by field_id"));
                }
            }
            entry.validate()?;
            prev = Some(id);
        }
        Ok(())
    }

    /// Looks up a field entry by [`FieldId`].
    pub fn get(&self, id: &FieldId) -> Option<&FieldEntry> {
        self.fields.binary_search_by(|e| e.field_id.as_str().cmp(id.as_str())).ok().and_then(|idx| self.fields.get(idx))
    }

    /// Serialises to canonical [`CValue`] (decimal string `schema_version`, zero floats).
    pub fn to_value(&self) -> CValue {
        obj([
            ("fields", CValue::Arr(self.fields.iter().map(FieldEntry::to_value).collect())),
            ("form", CValue::Str(self.form.as_str().to_string())),
            ("schema_version", CValue::Str(self.schema_version.to_string())),
        ])
    }

    /// Serialises to RFC 8785 canonical JSON bytes after validation.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, LayoutError> {
        self.validate()?;
        to_canonical_bytes(&self.to_value()).map_err(|_| LayoutError::FieldMapInvalid("canonical JSON encoding failed"))
    }

    /// Computes the SHA-256 digest of the canonical JSON bytes of this field map.
    pub fn digest(&self) -> Result<Digest, LayoutError> {
        let bytes = self.to_canonical_bytes()?;
        Ok(sha256_bytes(&bytes))
    }

    /// Parses and validates a [`FieldMap`] from JSON bytes.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<FieldMap, LayoutError> {
        if bytes.len() > MAX_FIELD_MAP_BYTES {
            return Err(LayoutError::FieldMapInvalid("field map JSON exceeds byte limit"));
        }
        let val = parse_with_limits(bytes, MAX_FIELD_MAP_BYTES, 16)
            .map_err(|_| LayoutError::FieldMapInvalid("malformed JSON"))?;
        Self::from_value(&val)
    }

    /// Parses and validates a [`FieldMap`] from a [`CValue`].
    pub fn from_value(v: &CValue) -> Result<FieldMap, LayoutError> {
        let CValue::Obj(o) = v else {
            return Err(LayoutError::FieldMapInvalid("field map must be an object"));
        };
        for k in o.keys() {
            if !matches!(k.as_str(), "schema_version" | "form" | "fields") {
                return Err(LayoutError::FieldMapInvalid("unknown key in field map"));
            }
        }
        let schema_version = match o.get("schema_version") {
            Some(CValue::Str(s)) => s.parse::<u32>().map_err(|_| LayoutError::FieldMapInvalid("bad schema_version"))?,
            Some(CValue::U64(n)) => {
                u32::try_from(*n).map_err(|_| LayoutError::FieldMapInvalid("bad schema_version"))?
            }
            _ => return Err(LayoutError::FieldMapInvalid("missing schema_version")),
        };
        let form = FormKind::parse(req_str(o, "form")?)?;
        let Some(CValue::Arr(raw_fields)) = o.get("fields") else {
            return Err(LayoutError::FieldMapInvalid("fields must be an array"));
        };
        if raw_fields.len() > MAX_FIELD_MAP_ENTRIES {
            return Err(LayoutError::FieldMapInvalid("field map exceeds maximum entries"));
        }
        let mut fields = Vec::with_capacity(raw_fields.len());
        for rf in raw_fields {
            fields.push(FieldEntry::from_value(rf)?);
        }
        let fm = FieldMap { schema_version, form, fields };
        fm.validate()?;
        Ok(fm)
    }
}

// ------------------------------------------------------------------ form_field block kind & resolver

/// Decoded payload of a `form_field` block (`{ "field_id": <FieldId>, "show_label": <bool> }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormFieldPayload {
    pub field_id: FieldId,
    pub show_label: bool,
}

impl FormFieldPayload {
    /// Parses a `form_field` block payload from a [`CValue`] object.
    pub fn from_value(v: &CValue) -> Result<FormFieldPayload, PayloadError> {
        let CValue::Obj(o) = v else {
            return Err(PayloadError::new("", "expected_object"));
        };
        for k in o.keys() {
            if !matches!(k.as_str(), "field_id" | "show_label" | "binding") {
                return Err(PayloadError::new(&format!("/{k}"), "unknown_field"));
            }
        }
        let field_id = match o.get("field_id") {
            Some(CValue::Str(s)) => {
                FieldId::new(s).ok_or_else(|| PayloadError::new("/field_id", "invalid_field_id"))?
            }
            Some(_) => return Err(PayloadError::new("/field_id", "expected_string")),
            None => return Err(PayloadError::new("/field_id", "missing")),
        };
        let show_label = match o.get("show_label") {
            Some(CValue::Bool(b)) => *b,
            Some(_) => return Err(PayloadError::new("/show_label", "expected_bool")),
            None => return Err(PayloadError::new("/show_label", "missing")),
        };
        Ok(FormFieldPayload { field_id, show_label })
    }

    /// Serialises to canonical [`CValue`].
    pub fn to_value(&self) -> CValue {
        obj([
            ("field_id", CValue::Str(self.field_id.as_str().to_string())),
            ("show_label", CValue::Bool(self.show_label)),
        ])
    }
}

/// Block kind `form_field` v1 (`RPT.FORMS.LAYOUT.MAP`).
#[derive(Debug, Clone)]
pub struct FormFieldKind {
    kind: BlockKindId,
}

impl FormFieldKind {
    /// Constructs the `form_field` block kind.
    pub fn new() -> Result<FormFieldKind, LayoutError> {
        let kind = BlockKindId::new(FORM_FIELD_KIND).ok_or(LayoutError::BadParams("invalid form_field kind id"))?;
        Ok(FormFieldKind { kind })
    }
}

impl BlockKind for FormFieldKind {
    fn id(&self) -> BlockKindId {
        self.kind.clone()
    }

    fn version(&self) -> u32 {
        1
    }

    fn validate(&self, payload: &CanonicalJson) -> Result<(), PayloadError> {
        let _ = FormFieldPayload::from_value(payload.value())?;
        Ok(())
    }

    fn value_fields(&self, _payload: &CanonicalJson) -> Result<Vec<String>, PayloadError> {
        // Numeric values arrive through FieldSource::Result + RPT.MODEL.FMT rather than
        // literal JSON values in the form_field payload.
        Ok(Vec::new())
    }
}

/// `BlockResolver` implementation for `form_field` blocks integrating with
/// `fid_report::bind::ResolverRegistry`.
///
/// Validates that every bound `Ref::Field(id)` exists in the template's [`FieldMap`] and
/// returns structure-only layout metadata (never raw numeric values).
#[derive(Debug, Clone)]
pub struct FormFieldResolver {
    kind: BlockKindId,
    field_map: FieldMap,
}

impl FormFieldResolver {
    /// Creates a `form_field` resolver bound to a validated [`FieldMap`].
    pub fn new(field_map: FieldMap) -> Result<FormFieldResolver, LayoutError> {
        field_map.validate()?;
        let kind = BlockKindId::new(FORM_FIELD_KIND).ok_or(LayoutError::BadParams("invalid form_field kind id"))?;
        Ok(FormFieldResolver { kind, field_map })
    }

    /// The template field map enforced by this resolver.
    pub fn field_map(&self) -> &FieldMap {
        &self.field_map
    }
}

impl BlockResolver for FormFieldResolver {
    fn kind(&self) -> BlockKindId {
        self.kind.clone()
    }

    fn place(
        &self,
        payload: &CanonicalJson,
        _binding: &Binding,
        _values: &[ValueBinding],
    ) -> Result<CValue, PayloadError> {
        let parsed = FormFieldPayload::from_value(payload.value())?;
        if self.field_map.get(&parsed.field_id).is_none() {
            return Err(PayloadError::new("/field_id", "field_unknown"));
        }
        Ok(parsed.to_value())
    }

    fn resolve(&self, view: &Resolved, ctx: &ResolveContext<'_>, _lookup: &dyn Lookup) -> Result<CValue, BindError> {
        let mut field_ref = None;
        for r in view.refs() {
            if let fid_report::bind::ResolvedRef::Field { id, .. } = r {
                field_ref = Some(id);
                break;
            }
        }
        let Some(field_id) = field_ref else {
            return Err(BindError::BadParams("form_field block has no field reference"));
        };
        let Some(entry) = self.field_map.get(field_id) else {
            return Err(BindError::NotFound(format!(
                "report.field_unknown: block {} field {}",
                view.block().uuid(),
                field_id.as_str()
            )));
        };
        let value_kind = match find_field(&ctx.sources, field_id) {
            Some(FieldValue::Text(_)) => "text",
            Some(FieldValue::Result(_)) => "result",
            None => "missing",
        };
        Ok(obj([
            ("field_id", CValue::Str(entry.field_id.as_str().to_string())),
            ("label_key", CValue::Str(entry.label_key.clone())),
            ("source", CValue::Str(entry.source.as_str().to_string())),
            ("value_kind", CValue::Str(value_kind.to_string())),
        ]))
    }
}

/// Formats numeric `FieldValue::Result(Uuid)` references exclusively through `RPT.MODEL.FMT`
/// (`REQ-RPT.FORMS.LAYOUT.MAP-005`, `AC-4`).
pub trait FormNumericFormatter {
    /// Formats the result record `record_id` using `fid_report::fmt::fmt_value` (or
    /// `fmt_requirement`), returning its typed [`ValueText`] carrying the `FormatterId`.
    fn format_result(&self, record_id: &Uuid) -> Result<ValueText, LayoutError>;
}

/// Resolved value of a `form_field` block (`REQ-005`, `AC-4`).
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedFormFieldValue {
    /// Text field: always typed as [`Untrusted<Text>`] (`CONSTITUTION` C6).
    Text(Untrusted<Text>),
    /// Numeric field: always formatted through `RPT.MODEL.FMT` carrying its [`ValueText::formatter`].
    Formatted(ValueText),
    /// Field is declared in the field map but absent in the active [`FieldSource`]s (blank).
    Missing,
}

/// Complete resolved representation of one `form_field` block.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedFormField {
    pub block_id: BlockId,
    pub field_id: FieldId,
    pub label_key: String,
    pub show_label: bool,
    pub source: SourcePath,
    pub citation: Option<Citation>,
    pub value: ResolvedFormFieldValue,
}

/// Resolves a single `form_field` block against `field_map`, `sources` and `formatter`
/// (`REQ-004`, `REQ-005`, `AC-3`, `AC-4`).
///
/// Fails with `LayoutError::FieldUnknown` (`report.field_unknown`, naming `block_id`) if
/// `payload.field_id` is not in `field_map`.
pub fn resolve_form_field_block(
    block_id: BlockId,
    payload: &FormFieldPayload,
    field_map: &FieldMap,
    sources: &[&dyn FieldSource],
    formatter: &dyn FormNumericFormatter,
) -> Result<ResolvedFormField, LayoutError> {
    let Some(entry) = field_map.get(&payload.field_id) else {
        return Err(LayoutError::FieldUnknown { block: block_id, field_id: payload.field_id.as_str().to_string() });
    };
    let value = match find_field(sources, &payload.field_id) {
        None => ResolvedFormFieldValue::Missing,
        Some(FieldValue::Text(t)) => ResolvedFormFieldValue::Text(t),
        Some(FieldValue::Result(record_id)) => ResolvedFormFieldValue::Formatted(formatter.format_result(&record_id)?),
    };
    Ok(ResolvedFormField {
        block_id,
        field_id: entry.field_id.clone(),
        label_key: entry.label_key.clone(),
        show_label: payload.show_label,
        source: entry.source.clone(),
        citation: entry.citation.clone(),
        value,
    })
}

/// Resolves every `form_field` block in `template` against `field_map`, `sources` and
/// `formatter` (`REQ-004`, `REQ-005`, `AC-3`, `AC-4`).
pub fn resolve_template_fields(
    template: &Template,
    field_map: &FieldMap,
    sources: &[&dyn FieldSource],
    formatter: &dyn FormNumericFormatter,
) -> Result<Vec<ResolvedFormField>, LayoutError> {
    field_map.validate()?;
    let mut out = Vec::new();
    for page in template.pages() {
        for block in page.blocks() {
            if block.kind().as_str() != FORM_FIELD_KIND {
                continue;
            }
            let payload = FormFieldPayload::from_value(block.payload().value())
                .map_err(|e| LayoutError::TemplateInvalid(e.to_string()))?;
            out.push(resolve_form_field_block(block.id(), &payload, field_map, sources, formatter)?);
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ built-in locked registry & clearance

/// Clearance status of a built-in industry form template (`REQ-RPT.FORMS.LAYOUT.MAP-006`, D-015).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormClearance {
    /// Counsel review not yet recorded; allowed in default builds with a warning, refused in
    /// `release` profile packaging tests (`AC-5`).
    Pending,
    /// Counsel review recorded under `review_ref`.
    Cleared { review_ref: Untrusted<Text> },
}

impl FormClearance {
    fn from_template(c: &Clearance) -> FormClearance {
        match c {
            Clearance::Pending => FormClearance::Pending,
            Clearance::Cleared { review_ref } => FormClearance::Cleared { review_ref: review_ref.clone() },
        }
    }

    /// Canonical [`CValue`] representation.
    pub fn to_value(&self) -> CValue {
        match self {
            FormClearance::Pending => obj([("state", CValue::Str("pending".to_string()))]),
            FormClearance::Cleared { review_ref } => obj([
                ("review_ref", CValue::Str(review_ref.untrusted_value().as_str().to_string())),
                ("state", CValue::Str("cleared".to_string())),
            ]),
        }
    }
}

/// Registration payload for one built-in locked form template.
#[derive(Debug, Clone)]
pub struct FormTemplateEntry {
    pub form: FormKind,
    pub logical_name: String,
    pub template: Template,
    pub version: u32,
    pub field_map: FieldMap,
    /// English source strings for every label key used by this template (for clean-room checks).
    pub labels: BTreeMap<String, String>,
}

/// Published metadata returned by `report.show_layout_fai@1` and `report.show_layout_ppap@1`
/// (`REQ-RPT.FORMS.LAYOUT.MAP-007`, `AC-1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormLayoutInfo {
    pub form: FormKind,
    pub logical_name: String,
    pub template: TemplateRef,
    pub locked: bool,
    pub field_map: FieldMap,
    pub field_map_digest: Digest,
    pub clearance: FormClearance,
}

impl FormLayoutInfo {
    /// Serialises to canonical [`CValue`].
    pub fn to_value(&self) -> CValue {
        obj([
            ("clearance", self.clearance.to_value()),
            ("field_map", self.field_map.to_value()),
            ("field_map_digest", CValue::Str(self.field_map_digest.as_str().to_string())),
            ("form", CValue::Str(self.form.as_str().to_string())),
            ("locked", CValue::Bool(self.locked)),
            ("logical_name", CValue::Str(self.logical_name.clone())),
            (
                "template",
                obj([
                    ("builtin", CValue::Bool(self.template.builtin)),
                    ("digest", CValue::Str(self.template.digest.as_str().to_string())),
                    ("id", CValue::Str(self.template.id.to_string())),
                    ("version", CValue::Str(self.template.version.to_string())),
                ]),
            ),
        ])
    }
}

/// Registry of built-in locked industry form templates and their field maps
/// (`REQ-001`, `REQ-002`, `REQ-004`, `REQ-006`, `REQ-007`).
#[derive(Debug, Clone, Default)]
pub struct FormLayoutRegistry {
    entries: BTreeMap<FormKind, FormLayoutInfo>,
    labels: BTreeMap<String, String>,
}

impl FormLayoutRegistry {
    /// Empty registry.
    pub fn new() -> FormLayoutRegistry {
        FormLayoutRegistry::default()
    }

    /// Registers one built-in form template as locked in `builtins`, validating its lock flag,
    /// clearance state, field map, and every `form_field` block reference (`REQ-001`..`REQ-006`).
    pub fn register(
        &mut self,
        entry: FormTemplateEntry,
        builtins: &mut BuiltinRegistry,
    ) -> Result<FormLayoutInfo, LayoutError> {
        if entry.logical_name.is_empty() || entry.logical_name.len() > MAX_KEY_BYTES {
            return Err(LayoutError::TemplateInvalid("invalid logical_name".to_string()));
        }
        if !entry.template.locked() {
            return Err(LayoutError::TemplateInvalid(format!(
                "built-in form template {} must have locked = true",
                entry.logical_name
            )));
        }
        let Some(raw_clearance) = entry.template.clearance() else {
            return Err(LayoutError::TemplateInvalid(format!(
                "built-in form template {} must declare a clearance state",
                entry.logical_name
            )));
        };
        let clearance = FormClearance::from_template(raw_clearance);
        if entry.field_map.form != entry.form {
            return Err(LayoutError::FieldMapInvalid("field map form kind mismatches entry"));
        }
        entry.field_map.validate()?;

        // Every form_field block in the template must reference a declared field map entry (REQ-004).
        for page in entry.template.pages() {
            for block in page.blocks() {
                if block.kind().as_str() == FORM_FIELD_KIND {
                    let payload = FormFieldPayload::from_value(block.payload().value())
                        .map_err(|e| LayoutError::TemplateInvalid(e.to_string()))?;
                    if entry.field_map.get(&payload.field_id).is_none() {
                        return Err(LayoutError::FieldUnknown {
                            block: block.id(),
                            field_id: payload.field_id.as_str().to_string(),
                        });
                    }
                }
            }
        }

        let field_map_digest = entry.field_map.digest()?;
        let template_ref = builtins.register(entry.template, entry.version)?;
        let info = FormLayoutInfo {
            form: entry.form,
            logical_name: entry.logical_name,
            template: template_ref,
            locked: true,
            field_map: entry.field_map,
            field_map_digest,
            clearance,
        };
        if self.entries.contains_key(&entry.form) {
            return Err(LayoutError::TemplateInvalid(format!(
                "duplicate registration for form {}",
                entry.form.as_str()
            )));
        }
        for (k, v) in entry.labels {
            self.labels.insert(k, v);
        }
        self.entries.insert(entry.form, info.clone());
        Ok(info)
    }

    /// All registered layouts, sorted by [`FormKind`].
    pub fn all(&self) -> Vec<FormLayoutInfo> {
        self.entries.values().cloned().collect()
    }

    /// All registered label `(key, text)` pairs for clean-room CI verification (`REQ-008`).
    pub fn labels(&self) -> &BTreeMap<String, String> {
        &self.labels
    }

    /// `report.show_layout_fai@1`: built-in FAI templates (forms 1, 2 and 3) in form order
    /// (`REQ-007`, `AC-1`).
    pub fn show_layout_fai(&self) -> Vec<FormLayoutInfo> {
        self.entries.values().filter(|i| i.form.is_fai()).cloned().collect()
    }

    /// `report.show_layout_ppap@1`: built-in PPAP templates in form order (`REQ-007`, `AC-1`).
    pub fn show_layout_ppap(&self) -> Vec<FormLayoutInfo> {
        self.entries.values().filter(|i| i.form.is_ppap()).cloned().collect()
    }

    /// JSON result payload for `report.show_layout_fai@1`.
    pub fn show_layout_fai_json(&self) -> CValue {
        obj([("layouts", CValue::Arr(self.show_layout_fai().iter().map(FormLayoutInfo::to_value).collect()))])
    }

    /// JSON result payload for `report.show_layout_ppap@1`.
    pub fn show_layout_ppap_json(&self) -> CValue {
        obj([("layouts", CValue::Arr(self.show_layout_ppap().iter().map(FormLayoutInfo::to_value).collect()))])
    }
}

// ------------------------------------------------------------------ clearance release gate (REQ-006, AC-5)

/// Build profile governing whether `pending` counsel clearance is a warning or a hard failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildProfile {
    /// Development / default profile: `pending` templates pass with explicit warnings.
    Default,
    /// Release packaging profile: any `pending` template fails (`REQ-006`, D-015 condition).
    Release,
}

/// Active build profile inferred from `debug_assertions`.
pub fn current_build_profile() -> BuildProfile {
    if cfg!(debug_assertions) {
        BuildProfile::Default
    } else {
        BuildProfile::Release
    }
}

/// Result of [`check_release_clearance`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClearanceCheckReport {
    pub profile: BuildProfile,
    pub pending_templates: Vec<String>,
    pub warnings: Vec<String>,
}

/// Enforces the D-015 clearance release gate (`REQ-RPT.FORMS.LAYOUT.MAP-006`, `AC-5`).
///
/// Under [`BuildProfile::Release`], returns `Err(LayoutError::ClearancePending)` naming every
/// pending template. Under [`BuildProfile::Default`], returns `Ok` with one warning per
/// pending template.
pub fn check_release_clearance(
    layouts: &[FormLayoutInfo],
    profile: BuildProfile,
) -> Result<ClearanceCheckReport, LayoutError> {
    let mut pending_templates = Vec::new();
    let mut warnings = Vec::new();
    for layout in layouts {
        if matches!(layout.clearance, FormClearance::Pending) {
            pending_templates.push(layout.logical_name.clone());
            warnings.push(format!(
                "form template {} ({}) has clearance=pending (D-015 counsel review required before release)",
                layout.logical_name, layout.template.id
            ));
        }
    }
    if profile == BuildProfile::Release && !pending_templates.is_empty() {
        return Err(LayoutError::ClearancePending { templates: pending_templates });
    }
    Ok(ClearanceCheckReport { profile, pending_templates, warnings })
}

// ------------------------------------------------------------------ clean-room label check (REQ-008, AC-6)

/// Set of SHA-256 digests of normalised external phrases forbidden in shipped form labels
/// (`REQ-RPT.FORMS.LAYOUT.MAP-008`, `AC-6`). Contains hashes only, never standards text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanroomPhraseSet {
    forbidden: BTreeSet<Digest>,
}

impl CleanroomPhraseSet {
    /// Builds a phrase set from precomputed SHA-256 digests.
    pub fn from_digests(digests: impl IntoIterator<Item = Digest>) -> CleanroomPhraseSet {
        CleanroomPhraseSet { forbidden: digests.into_iter().collect() }
    }

    /// Parses `fixtures/rpt/forms/cleanroom_phrases.json` (`format == "cleanroom-phrases@1"`).
    pub fn from_json_bytes(bytes: &[u8]) -> Result<CleanroomPhraseSet, LayoutError> {
        if bytes.len() > MAX_CLEANROOM_FILE_BYTES {
            return Err(LayoutError::BadParams("cleanroom phrase list too large"));
        }
        let val = parse_with_limits(bytes, MAX_CLEANROOM_FILE_BYTES, 8)
            .map_err(|_| LayoutError::BadParams("malformed cleanroom phrase JSON"))?;
        let CValue::Obj(o) = val else {
            return Err(LayoutError::BadParams("cleanroom phrase file must be an object"));
        };
        if req_str(&o, "format")? != "cleanroom-phrases@1" {
            return Err(LayoutError::BadParams("unsupported cleanroom phrase format"));
        }
        let Some(CValue::Arr(list)) = o.get("forbidden_digests") else {
            return Err(LayoutError::BadParams("missing forbidden_digests array"));
        };
        let mut forbidden = BTreeSet::new();
        for item in list {
            let CValue::Str(s) = item else {
                return Err(LayoutError::BadParams("forbidden digest must be a string"));
            };
            let d = Digest::parse(s).map_err(|_| LayoutError::BadParams("invalid sha256 digest in cleanroom list"))?;
            forbidden.insert(d);
        }
        Ok(CleanroomPhraseSet { forbidden })
    }

    /// Number of forbidden digests in this set.
    pub fn len(&self) -> usize {
        self.forbidden.len()
    }

    /// Whether the forbidden digest set is empty.
    pub fn is_empty(&self) -> bool {
        self.forbidden.is_empty()
    }

    /// Whether `digest` is forbidden.
    pub fn contains(&self, digest: &Digest) -> bool {
        self.forbidden.contains(digest)
    }
}

/// Checks every `(label_key, label_text)` pair against `forbidden` (`REQ-008`, `AC-6`).
///
/// Fails with [`LayoutError::CleanroomViolation`] naming the offending `label_key` if any
/// normalised label hashes to an entry of `forbidden`.
pub fn check_cleanroom_labels<'a>(
    labels: impl IntoIterator<Item = (&'a str, &'a str)>,
    forbidden: &CleanroomPhraseSet,
) -> Result<(), LayoutError> {
    for (label_key, label_text) in labels {
        let _norm = normalise_cleanroom_phrase(label_text);
        let digest = cleanroom_phrase_digest(label_text);
        if forbidden.contains(&digest) {
            return Err(LayoutError::CleanroomViolation { label_key: label_key.to_string(), digest });
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ CommandDefs (REQ-007)

/// Returns the two validated [`CommandDef`]s of `RPT.FORMS.LAYOUT.MAP`:
/// `report.show_layout_fai@1` and `report.show_layout_ppap@1`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    let text = |s: &str| CValue::Str(s.to_string());
    let strs = |items: &[&str]| CValue::Arr(items.iter().map(|s| text(s)).collect());
    let empty_obj = || {
        obj([
            ("additionalProperties", CValue::Bool(false)),
            ("properties", obj([])),
            ("required", CValue::Arr(Vec::new())),
            ("type", text("object")),
        ])
    };
    let layouts_result = || {
        obj([
            ("additionalProperties", CValue::Bool(false)),
            (
                "properties",
                obj([(
                    "layouts",
                    obj([
                        (
                            "description",
                            text(
                                "Built-in locked form templates with sorted field maps, digests and clearance states.",
                            ),
                        ),
                        ("type", text("array")),
                    ]),
                )]),
            ),
            ("required", strs(&["layouts"])),
            ("type", text("object")),
        ])
    };
    let make = |id: &str, title: &str, desc: &str| -> Result<CommandDef, DefError> {
        let val = obj([
            ("algorithms", CValue::Arr(Vec::new())),
            ("cli_bare", CValue::Bool(false)),
            ("description", text(desc)),
            ("determinism", text("D0")),
            ("effects", CValue::Arr(Vec::new())),
            ("exposure", strs(&["cli", "mcp"])),
            ("fs_effect", text("none")),
            ("human_only", CValue::Bool(false)),
            ("id", text(id)),
            ("idempotency", text("natural")),
            ("lifecycle", obj([("since", text(LIFECYCLE_SINCE)), ("state", text("active"))])),
            ("major", json_u32(1)),
            ("mcp_projection", text("exposed")),
            ("network", text("none")),
            ("params_schema", empty_obj()),
            ("preconditions", CValue::Arr(Vec::new())),
            ("result_schema", layouts_result()),
            ("side_effect", text("R0")),
            ("title", text(title)),
            ("undo", text("not_undoable")),
            ("web", text("available")),
        ]);
        CommandDef::from_value(val)
    };
    Ok(vec![
        make(
            "report.show_layout_fai",
            "Show built-in FAI form layouts",
            "Return built-in locked AS9102 FAI form templates (forms 1 to 3), their sorted field maps, digests and clearance states.",
        )?,
        make(
            "report.show_layout_ppap",
            "Show built-in PPAP form layouts",
            "Return built-in locked PPAP dimensional form template, its sorted field map, digest and clearance state.",
        )?,
    ])
}

// ------------------------------------------------------------------ helpers

fn obj<const N: usize>(entries: [(&str, CValue); N]) -> CValue {
    let mut m = BTreeMap::new();
    for (k, v) in entries {
        m.insert(k.to_string(), v);
    }
    CValue::Obj(m)
}

fn req_str<'a>(o: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a str, LayoutError> {
    match o.get(key) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(LayoutError::FieldMapInvalid("missing or non-string field")),
    }
}

fn sha256_bytes(bytes: &[u8]) -> Digest {
    Digest::from_bytes(canon_sha256(bytes).0)
}

fn json_u32(n: u32) -> CValue {
    let s = n.to_string();
    parse_with_limits(s.as_bytes(), 16, 4).unwrap_or(CValue::U64(u64::from(n)))
}
