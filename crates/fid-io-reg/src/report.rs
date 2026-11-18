//! Import and export report types (spec node `IO.REG.ENV.REPORT`).
//!
//! Every import returns an [`ImportReport`] and every export an [`ExportReport`]: in
//! `--json` output, over MCP, and as the digest-bearing part of the journal event. Format
//! readers and writers fill a [`ReportBuilder`] while they run (`count`, `warn`,
//! `drop_item`, `lossy`) and the command envelope finishes it into the immutable report.
//!
//! - Entity counts are kept by type name in a `BTreeMap`, so insertion order never shows
//!   (REQ-001, AC-4).
//! - At most [`MAX_LISTED`] warnings and dropped items are listed each, in source order;
//!   the totals stay exact (REQ-002). Capping is not an error.
//! - Text copied from the source is held as [`UntrustedText`], cut to
//!   [`MAX_DETAIL_CHARS`] Unicode scalar values with a truncation flag (REQ-004).
//! - Reports serialise to RFC 8785 canonical JSON through `fid-canon` (REQ-003). Integers
//!   of 64 bits are decimal strings (the `fid-canon` project rule); line and column are
//!   JSON numbers. Absent optional values are `null`; every key is always present.
//! - [`ImportReport::from_json`] and [`ExportReport::from_json`] read a report back
//!   strictly: unknown or missing fields, oversized lists and inconsistent totals fail.
//! - The human-readable form (`Display`) shows untrusted text and paths only through
//!   [`UntrustedText::as_display`], so control, format and bidirectional characters are
//!   escaped and every listed item stays on one line.

use crate::error::Position;
use crate::roots::RootHandle;
use crate::untrusted::UntrustedText;
use fid_canon::{CValue, CanonError};
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Most warnings, and separately most dropped items, a report lists (REQ-002).
pub const MAX_LISTED: usize = 10_000;
/// Most Unicode scalar values kept from a source text (REQ-004).
pub const MAX_DETAIL_CHARS: usize = 256;
/// Longest code, type name or format ID, in bytes.
pub const MAX_NAME_LEN: usize = 64;
/// Most distinct entity types an import report counts.
pub const MAX_ENTITY_TYPES: usize = 1_024;
/// Most distinct names an export report's `lossy` list holds.
pub const MAX_LOSSY: usize = 1_024;
/// Longest source or target location, in bytes.
pub const MAX_LOCATION_BYTES: usize = 4_096;
/// Largest report JSON [`ImportReport::from_json`] and [`ExportReport::from_json`] accept.
pub const MAX_REPORT_JSON_BYTES: usize = 48 * 1024 * 1024;
/// Warning code the builder adds once when a new entity type arrives past [`MAX_ENTITY_TYPES`].
pub const ENTITY_TYPES_CAPPED: &str = "entity_types_capped";
/// Warning code the builder adds once when a new lossy name arrives past [`MAX_LOSSY`].
pub const LOSSY_CAPPED: &str = "lossy_capped";

/// Deepest JSON nesting of a valid report.
const MAX_JSON_DEPTH: usize = 8;

/// Why a report or one of its names could not be built or read back. Messages are static
/// strings; text from the input never appears in an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportError {
    /// The JSON text itself was refused by `fid-canon`.
    Json(CanonError),
    /// A value breaks the report schema.
    Invalid {
        /// The field (schema path) at fault.
        field: &'static str,
        /// What is wrong with it.
        reason: &'static str,
    },
}

impl ReportError {
    /// The stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            ReportError::Json(_) => "report_json",
            ReportError::Invalid { .. } => "report_invalid",
        }
    }
}

impl fmt::Display for ReportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Only the code: a duplicate-key error carries text from the input.
            ReportError::Json(e) => write!(f, "report_json: {}", e.code()),
            ReportError::Invalid { field, reason } => write!(f, "report_invalid: {field}: {reason}"),
        }
    }
}

impl std::error::Error for ReportError {}

fn invalid(field: &'static str, reason: &'static str) -> ReportError {
    ReportError::Invalid { field, reason }
}

fn is_snake_segment(s: &str) -> bool {
    let b = s.as_bytes();
    matches!(b.first(), Some(b'a'..=b'z')) && b.iter().all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

fn is_type_name(s: &str) -> bool {
    s.split('.').all(is_snake_segment)
}

fn is_format_id(s: &str) -> bool {
    let b = s.as_bytes();
    matches!(b.first(), Some(b'a'..=b'z' | b'0'..=b'9'))
        && b.iter().all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.'))
}

macro_rules! name_type {
    ($(#[$doc:meta])* $name:ident, $field:literal, $valid:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Checks and wraps a name.
            pub fn new(s: &str) -> Result<$name, ReportError> {
                if s.is_empty() || s.len() > MAX_NAME_LEN || !$valid(s) {
                    return Err(invalid($field, "invalid name"));
                }
                Ok($name(s.to_string()))
            }

            /// The name.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                // ASCII from a closed alphabet by construction.
                f.write_str(&self.0)
            }
        }
    };
}

name_type!(
    /// A stable snake_case code: 1 to [`MAX_NAME_LEN`] bytes of `a-z`, `0-9` and `_`,
    /// starting with a letter. Used for warning codes and drop reasons.
    Code,
    "code",
    is_snake_segment
);

name_type!(
    /// An entity type, dropped-item kind or lossy field name: one or more snake_case
    /// segments joined by `.` (for example `point_set` or `mesh.normals`), at most
    /// [`MAX_NAME_LEN`] bytes.
    TypeName,
    "type_name",
    is_type_name
);

name_type!(
    /// A format ID: 1 to [`MAX_NAME_LEN`] bytes of `a-z`, `0-9`, `-`, `_` and `.`, starting
    /// with a letter or digit (for example `fid-test-points`).
    FormatId,
    "format",
    is_format_id
);

/// Source text kept in a report: at most [`MAX_DETAIL_CHARS`] Unicode scalar values of
/// [`UntrustedText`], and whether the original was longer (REQ-004).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ReportText {
    text: UntrustedText,
    truncated: bool,
}

impl ReportText {
    /// Keeps the first [`MAX_DETAIL_CHARS`] scalar values of `src`.
    pub fn new(src: &UntrustedText) -> ReportText {
        let raw = src.raw();
        match raw.char_indices().nth(MAX_DETAIL_CHARS) {
            Some((cut, _)) => {
                ReportText { text: UntrustedText::new(raw.get(..cut).unwrap_or_default()), truncated: true }
            }
            None => ReportText { text: src.clone(), truncated: false },
        }
    }

    /// The kept text.
    pub fn text(&self) -> &UntrustedText {
        &self.text
    }

    /// Whether the original text was longer than [`MAX_DETAIL_CHARS`].
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// Length of the kept text in Unicode scalar values.
    pub fn char_len(&self) -> usize {
        self.text.raw().chars().count()
    }
}

/// Where a report's source was read from or its output written to: a root handle, or a
/// path a human typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// A `root://` handle.
    Handle(RootHandle),
    /// A raw path given by a human (agents and scripts use handles only).
    HumanPath(UntrustedText),
}

impl Location {
    /// A root handle location; its wire form is at most [`MAX_LOCATION_BYTES`] bytes.
    pub fn handle(handle: RootHandle) -> Result<Location, ReportError> {
        let loc = Location::Handle(handle);
        if loc.wire().len() > MAX_LOCATION_BYTES {
            return Err(invalid("location", "too long"));
        }
        Ok(loc)
    }

    /// A raw path a human gave: non-empty, at most [`MAX_LOCATION_BYTES`] bytes, and not
    /// starting with `root://` (which would read back as a handle).
    pub fn human_path(path: &str) -> Result<Location, ReportError> {
        if path.is_empty() || path.len() > MAX_LOCATION_BYTES || path.starts_with("root://") {
            return Err(invalid("location", "invalid human path"));
        }
        Ok(Location::HumanPath(UntrustedText::new(path)))
    }

    fn wire(&self) -> String {
        match self {
            Location::Handle(h) => h.to_string(),
            Location::HumanPath(p) => p.raw().to_string(),
        }
    }

    fn from_wire(s: &str) -> Result<Location, ReportError> {
        if s.starts_with("root://") {
            let h = RootHandle::parse(s).map_err(|_| invalid("location", "invalid root handle"))?;
            if h.to_string() != s {
                return Err(invalid("location", "root handle not in normal form"));
            }
            Location::handle(h)
        } else {
            Location::human_path(s)
        }
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Root-relative path components and human paths may hold any characters.
        write!(f, "{}", UntrustedText::from_string(self.wire()).as_display())
    }
}

/// A warning: a stable code, where in the source (if known), and source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// Stable snake_case code.
    pub code: Code,
    /// Source position, if known.
    pub pos: Option<Position>,
    /// Source text, possibly empty.
    pub detail: ReportText,
}

/// An item the reader could not import: its kind, source position and reason code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    /// What was dropped (an entity type or source item kind).
    pub kind: TypeName,
    /// Where in the source.
    pub pos: Position,
    /// Stable snake_case reason code.
    pub reason: Code,
    /// Source text, possibly empty.
    pub detail: ReportText,
}

/// The report of one import (REQ-IO.REG.ENV-006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    format: FormatId,
    source: Location,
    entity_counts: BTreeMap<TypeName, u64>,
    warnings: Vec<Warning>,
    warnings_total: u64,
    dropped: Vec<Dropped>,
    dropped_total: u64,
    dry_run: bool,
}

/// The report of one export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    format: FormatId,
    target: Location,
    sha256: Digest,
    bytes: u64,
    warnings: Vec<Warning>,
    warnings_total: u64,
    lossy: BTreeSet<TypeName>,
    dry_run: bool,
}

/// Collects counts, warnings, dropped items and lossy names while a reader or writer runs.
/// Every method is O(1) amortised (a listed detail costs at most [`MAX_DETAIL_CHARS`]).
#[derive(Debug, Clone, Default)]
pub struct ReportBuilder {
    counts: BTreeMap<TypeName, u64>,
    warnings: Vec<Warning>,
    warnings_total: u64,
    dropped: Vec<Dropped>,
    dropped_total: u64,
    lossy: BTreeSet<TypeName>,
    types_capped: bool,
    lossy_capped: bool,
}

impl ReportBuilder {
    /// An empty builder.
    pub fn new() -> ReportBuilder {
        ReportBuilder::default()
    }

    /// Adds `n` entities of type `ty`. A type first seen after [`MAX_ENTITY_TYPES`] types
    /// is not counted; the first such call adds an [`ENTITY_TYPES_CAPPED`] warning.
    pub fn count(&mut self, ty: &TypeName, n: u64) {
        if let Some(c) = self.counts.get_mut(ty) {
            *c = c.saturating_add(n);
        } else if self.counts.len() < MAX_ENTITY_TYPES {
            self.counts.insert(ty.clone(), n);
        } else if !self.types_capped {
            self.types_capped = true;
            self.push_warning(Code(ENTITY_TYPES_CAPPED.to_string()), None, ReportText::default());
        }
    }

    /// Adds a warning. Past [`MAX_LISTED`] warnings only the total grows.
    pub fn warn(&mut self, code: &Code, pos: Option<Position>, detail: &UntrustedText) {
        self.warnings_total = self.warnings_total.saturating_add(1);
        if self.warnings.len() < MAX_LISTED {
            self.warnings.push(Warning { code: code.clone(), pos, detail: ReportText::new(detail) });
        }
    }

    fn push_warning(&mut self, code: Code, pos: Option<Position>, detail: ReportText) {
        self.warnings_total = self.warnings_total.saturating_add(1);
        if self.warnings.len() < MAX_LISTED {
            self.warnings.push(Warning { code, pos, detail });
        }
    }

    /// Records a dropped item. Past [`MAX_LISTED`] items only the total grows.
    pub fn drop_item(&mut self, kind: &TypeName, pos: Position, reason: &Code, detail: &UntrustedText) {
        self.dropped_total = self.dropped_total.saturating_add(1);
        if self.dropped.len() < MAX_LISTED {
            self.dropped.push(Dropped {
                kind: kind.clone(),
                pos,
                reason: reason.clone(),
                detail: ReportText::new(detail),
            });
        }
    }

    /// Names a field or entity kind the export target cannot represent (REQ-005). A name
    /// first seen after [`MAX_LOSSY`] names is not listed; the first such call adds a
    /// [`LOSSY_CAPPED`] warning.
    pub fn lossy(&mut self, name: &TypeName) {
        if self.lossy.contains(name) {
            return;
        }
        if self.lossy.len() < MAX_LOSSY {
            self.lossy.insert(name.clone());
        } else if !self.lossy_capped {
            self.lossy_capped = true;
            self.push_warning(Code(LOSSY_CAPPED.to_string()), None, ReportText::default());
        }
    }

    /// Warnings added so far, including those not listed.
    pub fn warnings_total(&self) -> u64 {
        self.warnings_total
    }

    /// Dropped items added so far, including those not listed.
    pub fn dropped_total(&self) -> u64 {
        self.dropped_total
    }

    /// Finishes an import report. Lossy names are an export notion and are not kept.
    pub fn finish_import(self, format: FormatId, source: Location, dry_run: bool) -> ImportReport {
        ImportReport {
            format,
            source,
            entity_counts: self.counts,
            warnings: self.warnings,
            warnings_total: self.warnings_total,
            dropped: self.dropped,
            dropped_total: self.dropped_total,
            dry_run,
        }
    }

    /// Finishes an export report. Entity counts are not kept; the kind of every listed
    /// dropped item is added to `lossy`, since the target could not represent it.
    pub fn finish_export(
        mut self,
        format: FormatId,
        target: Location,
        sha256: Digest,
        bytes: u64,
        dry_run: bool,
    ) -> ExportReport {
        let kinds: Vec<TypeName> = self.dropped.iter().map(|d| d.kind.clone()).collect();
        for k in &kinds {
            self.lossy(k);
        }
        ExportReport {
            format,
            target,
            sha256,
            bytes,
            warnings: self.warnings,
            warnings_total: self.warnings_total,
            lossy: self.lossy,
            dry_run,
        }
    }
}

// ---------------------------------------------------------------------------------------
// Canonical JSON

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn obj<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn opt_u32(v: Option<u32>) -> CValue {
    v.map_or(CValue::Null, |n| CValue::Num(f64::from(n)))
}

fn pos_value(p: &Position) -> CValue {
    obj([("byte", CValue::U64(p.byte)), ("col", opt_u32(p.col)), ("line", opt_u32(p.line))])
}

fn text_value(t: &ReportText) -> CValue {
    obj([("text", s(t.text.raw())), ("truncated", CValue::Bool(t.truncated))])
}

fn warnings_value(ws: &[Warning]) -> CValue {
    CValue::Arr(
        ws.iter()
            .map(|w| {
                obj([
                    ("code", s(w.code.as_str())),
                    ("detail", text_value(&w.detail)),
                    ("pos", w.pos.as_ref().map_or(CValue::Null, pos_value)),
                ])
            })
            .collect(),
    )
}

fn finish(v: &CValue) -> Result<Vec<u8>, ReportError> {
    fid_canon::to_canonical_bytes(v).map_err(ReportError::Json)
}

fn digest_of(v: &CValue) -> Result<Digest, ReportError> {
    finish(v).map(|b| Digest::from_bytes(fid_canon::digest_bytes(&b).0))
}

impl ImportReport {
    /// The format ID.
    pub fn format(&self) -> &FormatId {
        &self.format
    }
    /// Where the source was read from.
    pub fn source(&self) -> &Location {
        &self.source
    }
    /// Entity counts by type name, in type-name order.
    pub fn entity_counts(&self) -> &BTreeMap<TypeName, u64> {
        &self.entity_counts
    }
    /// The listed warnings (at most [`MAX_LISTED`], in source order).
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }
    /// All warnings added.
    pub fn warnings_total(&self) -> u64 {
        self.warnings_total
    }
    /// The listed dropped items (at most [`MAX_LISTED`], in source order).
    pub fn dropped(&self) -> &[Dropped] {
        &self.dropped
    }
    /// All dropped items added.
    pub fn dropped_total(&self) -> u64 {
        self.dropped_total
    }
    /// Whether this was a dry run.
    pub fn dry_run(&self) -> bool {
        self.dry_run
    }

    /// The JSON value model of the report.
    pub fn to_cvalue(&self) -> CValue {
        let counts = self.entity_counts.iter().map(|(k, n)| (k.as_str().to_string(), CValue::U64(*n))).collect();
        let dropped = self
            .dropped
            .iter()
            .map(|d| {
                obj([
                    ("detail", text_value(&d.detail)),
                    ("kind", s(d.kind.as_str())),
                    ("pos", pos_value(&d.pos)),
                    ("reason", s(d.reason.as_str())),
                ])
            })
            .collect();
        obj([
            ("dropped", CValue::Arr(dropped)),
            ("dropped_total", CValue::U64(self.dropped_total)),
            ("dry_run", CValue::Bool(self.dry_run)),
            ("entity_counts", CValue::Obj(counts)),
            ("format", s(self.format.as_str())),
            ("source", s(&self.source.wire())),
            ("warnings", warnings_value(&self.warnings)),
            ("warnings_total", CValue::U64(self.warnings_total)),
        ])
    }

    /// RFC 8785 canonical bytes (REQ-003).
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ReportError> {
        finish(&self.to_cvalue())
    }

    /// SHA-256 of the canonical bytes.
    pub fn digest(&self) -> Result<Digest, ReportError> {
        digest_of(&self.to_cvalue())
    }

    /// Reads a report back from JSON, strictly.
    pub fn from_json(json: &[u8]) -> Result<ImportReport, ReportError> {
        let v = parse_json(json)?;
        let m = fields(&v, "import_report", &IMPORT_KEYS)?;
        let warnings_total = u64_of(get(m, "warnings_total")?, "warnings_total")?;
        let dropped_total = u64_of(get(m, "dropped_total")?, "dropped_total")?;
        let warnings = warnings_of(get(m, "warnings")?, warnings_total)?;
        let dropped_items = listed(get(m, "dropped")?, "dropped", dropped_total)?;
        let mut dropped = Vec::with_capacity(dropped_items.len());
        for d in dropped_items {
            let dm = fields(d, "dropped", &["detail", "kind", "pos", "reason"])?;
            dropped.push(Dropped {
                kind: TypeName::new(str_of(get(dm, "kind")?, "dropped.kind")?)?,
                pos: pos_of(get(dm, "pos")?)?,
                reason: Code::new(str_of(get(dm, "reason")?, "dropped.reason")?)?,
                detail: text_of(get(dm, "detail")?)?,
            });
        }
        let CValue::Obj(cm) = get(m, "entity_counts")? else {
            return Err(invalid("entity_counts", "not an object"));
        };
        if cm.len() > MAX_ENTITY_TYPES {
            return Err(invalid("entity_counts", "too many types"));
        }
        let mut entity_counts = BTreeMap::new();
        for (k, n) in cm {
            entity_counts.insert(TypeName::new(k)?, u64_of(n, "entity_counts")?);
        }
        Ok(ImportReport {
            format: FormatId::new(str_of(get(m, "format")?, "format")?)?,
            source: Location::from_wire(str_of(get(m, "source")?, "source")?)?,
            entity_counts,
            warnings,
            warnings_total,
            dropped,
            dropped_total,
            dry_run: bool_of(get(m, "dry_run")?, "dry_run")?,
        })
    }
}

impl ExportReport {
    /// The format ID.
    pub fn format(&self) -> &FormatId {
        &self.format
    }
    /// Where the output was (or, in a dry run, would be) written.
    pub fn target(&self) -> &Location {
        &self.target
    }
    /// SHA-256 of the output bytes.
    pub fn sha256(&self) -> &Digest {
        &self.sha256
    }
    /// Output size in bytes.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    /// The listed warnings (at most [`MAX_LISTED`], in order of addition).
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }
    /// All warnings added.
    pub fn warnings_total(&self) -> u64 {
        self.warnings_total
    }
    /// Fields and entity kinds the target format could not represent, in name order.
    pub fn lossy(&self) -> &BTreeSet<TypeName> {
        &self.lossy
    }
    /// Whether this was a dry run.
    pub fn dry_run(&self) -> bool {
        self.dry_run
    }

    /// The JSON value model of the report.
    pub fn to_cvalue(&self) -> CValue {
        obj([
            ("bytes", CValue::U64(self.bytes)),
            ("dry_run", CValue::Bool(self.dry_run)),
            ("format", s(self.format.as_str())),
            ("lossy", CValue::Arr(self.lossy.iter().map(|n| s(n.as_str())).collect())),
            ("sha256", s(self.sha256.as_str())),
            ("target", s(&self.target.wire())),
            ("warnings", warnings_value(&self.warnings)),
            ("warnings_total", CValue::U64(self.warnings_total)),
        ])
    }

    /// RFC 8785 canonical bytes.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ReportError> {
        finish(&self.to_cvalue())
    }

    /// SHA-256 of the canonical bytes.
    pub fn digest(&self) -> Result<Digest, ReportError> {
        digest_of(&self.to_cvalue())
    }

    /// Reads a report back from JSON, strictly. `lossy` must be sorted and free of repeats.
    pub fn from_json(json: &[u8]) -> Result<ExportReport, ReportError> {
        let v = parse_json(json)?;
        let m = fields(&v, "export_report", &EXPORT_KEYS)?;
        let warnings_total = u64_of(get(m, "warnings_total")?, "warnings_total")?;
        let warnings = warnings_of(get(m, "warnings")?, warnings_total)?;
        let CValue::Arr(items) = get(m, "lossy")? else {
            return Err(invalid("lossy", "not an array"));
        };
        if items.len() > MAX_LOSSY {
            return Err(invalid("lossy", "too many names"));
        }
        let mut lossy = BTreeSet::new();
        for item in items {
            let name = TypeName::new(str_of(item, "lossy")?)?;
            if lossy.last().is_some_and(|last| *last >= name) {
                return Err(invalid("lossy", "not sorted or repeated"));
            }
            lossy.insert(name);
        }
        Ok(ExportReport {
            format: FormatId::new(str_of(get(m, "format")?, "format")?)?,
            target: Location::from_wire(str_of(get(m, "target")?, "target")?)?,
            sha256: Digest::parse(str_of(get(m, "sha256")?, "sha256")?)
                .map_err(|_| invalid("sha256", "invalid digest"))?,
            bytes: u64_of(get(m, "bytes")?, "bytes")?,
            warnings,
            warnings_total,
            lossy,
            dry_run: bool_of(get(m, "dry_run")?, "dry_run")?,
        })
    }
}

// ---------------------------------------------------------------------------------------
// Strict reading

const IMPORT_KEYS: [&str; 8] =
    ["dropped", "dropped_total", "dry_run", "entity_counts", "format", "source", "warnings", "warnings_total"];
const EXPORT_KEYS: [&str; 8] =
    ["bytes", "dry_run", "format", "lossy", "sha256", "target", "warnings", "warnings_total"];

fn parse_json(json: &[u8]) -> Result<CValue, ReportError> {
    fid_canon::parse_with_limits(json, MAX_REPORT_JSON_BYTES, MAX_JSON_DEPTH).map_err(ReportError::Json)
}

/// The object `v` with exactly the keys `keys`.
fn fields<'a>(v: &'a CValue, field: &'static str, keys: &[&str]) -> Result<&'a BTreeMap<String, CValue>, ReportError> {
    let CValue::Obj(m) = v else {
        return Err(invalid(field, "not an object"));
    };
    if m.len() != keys.len() || !keys.iter().all(|k| m.contains_key(*k)) {
        return Err(invalid(field, "unknown or missing field"));
    }
    Ok(m)
}

fn get<'a>(m: &'a BTreeMap<String, CValue>, key: &'static str) -> Result<&'a CValue, ReportError> {
    m.get(key).ok_or_else(|| invalid(key, "missing"))
}

fn str_of<'a>(v: &'a CValue, field: &'static str) -> Result<&'a str, ReportError> {
    match v {
        CValue::Str(s) => Ok(s),
        _ => Err(invalid(field, "not a string")),
    }
}

fn bool_of(v: &CValue, field: &'static str) -> Result<bool, ReportError> {
    match v {
        CValue::Bool(b) => Ok(*b),
        _ => Err(invalid(field, "not a boolean")),
    }
}

/// A 64-bit count: a string of decimal digits without leading zeros.
fn u64_of(v: &CValue, field: &'static str) -> Result<u64, ReportError> {
    let s = str_of(v, field)?;
    let b = s.as_bytes();
    let ok = !b.is_empty() && b.len() <= 20 && b.iter().all(u8::is_ascii_digit) && (b.len() == 1 || b[0] != b'0');
    if !ok {
        return Err(invalid(field, "not a decimal u64 string"));
    }
    s.parse::<u64>().map_err(|_| invalid(field, "not a decimal u64 string"))
}

/// A 1-based line or column: `null` or a JSON integer from 1 to `u32::MAX`.
fn line_col_of(v: &CValue, field: &'static str) -> Result<Option<u32>, ReportError> {
    match v {
        CValue::Null => Ok(None),
        CValue::Num(f) if f.fract() == 0.0 && (1.0..=f64::from(u32::MAX)).contains(f) => {
            u32::try_from(*f as u64).map(Some).map_err(|_| invalid(field, "out of range"))
        }
        _ => Err(invalid(field, "not null or an integer from 1")),
    }
}

fn pos_of(v: &CValue) -> Result<Position, ReportError> {
    let m = fields(v, "pos", &["byte", "col", "line"])?;
    Ok(Position {
        byte: u64_of(get(m, "byte")?, "pos.byte")?,
        line: line_col_of(get(m, "line")?, "pos.line")?,
        col: line_col_of(get(m, "col")?, "pos.col")?,
    })
}

fn text_of(v: &CValue) -> Result<ReportText, ReportError> {
    let m = fields(v, "detail", &["text", "truncated"])?;
    let text = str_of(get(m, "text")?, "detail.text")?;
    let truncated = bool_of(get(m, "truncated")?, "detail.truncated")?;
    let n = text.chars().count();
    if n > MAX_DETAIL_CHARS {
        return Err(invalid("detail.text", "longer than 256 characters"));
    }
    if truncated && n != MAX_DETAIL_CHARS {
        return Err(invalid("detail.truncated", "truncated text must keep 256 characters"));
    }
    Ok(ReportText { text: UntrustedText::new(text), truncated })
}

/// A listed array of `min(total, MAX_LISTED)` items.
fn listed<'a>(v: &'a CValue, field: &'static str, total: u64) -> Result<&'a [CValue], ReportError> {
    let CValue::Arr(items) = v else {
        return Err(invalid(field, "not an array"));
    };
    let want = usize::try_from(total).map_or(MAX_LISTED, |t| t.min(MAX_LISTED));
    if items.len() != want {
        return Err(invalid(field, "listed items do not match the total"));
    }
    Ok(items)
}

fn warnings_of(v: &CValue, total: u64) -> Result<Vec<Warning>, ReportError> {
    let items = listed(v, "warnings", total)?;
    let mut out = Vec::with_capacity(items.len());
    for w in items {
        let wm = fields(w, "warning", &["code", "detail", "pos"])?;
        let pos = match get(wm, "pos")? {
            CValue::Null => None,
            p => Some(pos_of(p)?),
        };
        out.push(Warning {
            code: Code::new(str_of(get(wm, "code")?, "warning.code")?)?,
            pos,
            detail: text_of(get(wm, "detail")?)?,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------
// Human-readable form: untrusted text only through `as_display`.

fn write_pos(f: &mut fmt::Formatter<'_>, pos: Option<&Position>) -> fmt::Result {
    match pos {
        Some(p) => write!(f, "{p}"),
        None => f.write_str("unknown position"),
    }
}

fn write_detail(f: &mut fmt::Formatter<'_>, d: &ReportText) -> fmt::Result {
    write!(f, "\"{}\"", d.text.as_display())?;
    if d.truncated {
        f.write_str(" (truncated)")?;
    }
    Ok(())
}

fn write_warnings(f: &mut fmt::Formatter<'_>, ws: &[Warning], total: u64) -> fmt::Result {
    writeln!(f, "warnings: {total} ({} listed)", ws.len())?;
    for w in ws {
        write!(f, "  {} at ", w.code)?;
        write_pos(f, w.pos.as_ref())?;
        f.write_str(": ")?;
        write_detail(f, &w.detail)?;
        writeln!(f)?;
    }
    Ok(())
}

impl fmt::Display for ImportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let dry = if self.dry_run { " (dry run)" } else { "" };
        writeln!(f, "import {} from {}{dry}", self.format, self.source)?;
        f.write_str("entities:")?;
        if self.entity_counts.is_empty() {
            f.write_str(" none")?;
        }
        for (ty, n) in &self.entity_counts {
            write!(f, " {ty}={n}")?;
        }
        writeln!(f)?;
        write_warnings(f, &self.warnings, self.warnings_total)?;
        writeln!(f, "dropped: {} ({} listed)", self.dropped_total, self.dropped.len())?;
        for d in &self.dropped {
            write!(f, "  {} at ", d.kind)?;
            write_pos(f, Some(&d.pos))?;
            write!(f, ": {}: ", d.reason)?;
            write_detail(f, &d.detail)?;
            writeln!(f)?;
        }
        Ok(())
    }
}

impl fmt::Display for ExportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let dry = if self.dry_run { " (dry run)" } else { "" };
        writeln!(f, "export {} to {}{dry}", self.format, self.target)?;
        writeln!(f, "output: {} bytes, {}", self.bytes, self.sha256)?;
        f.write_str("lossy:")?;
        if self.lossy.is_empty() {
            f.write_str(" none")?;
        }
        for n in &self.lossy {
            write!(f, " {n}")?;
        }
        writeln!(f)?;
        write_warnings(f, &self.warnings, self.warnings_total)
    }
}
