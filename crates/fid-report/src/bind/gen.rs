//! Report generation, `ResolvedReport` contract, provenance, digest, scratch/CAS storage,
//! commands and exact replay (spec node `RPT.MODEL.BIND.GEN`).
//!
//! - [`generate`] expands a template or report at an explicit journal head, resolves every
//!   block through its registered resolver and formats every value of record strictly through
//!   [`crate::fmt`], collecting formatter IDs and non-fatal warnings sorted by `(block_id, code)`.
//! - [`ResolvedReport::digest`] is the SHA-256 of the RFC 8785 canonical JSON **excluding**
//!   the `digest` field (`REQ-RPT.MODEL.BIND.GEN-001`). Storing [`ResolvedReport::to_content_canonical_bytes`]
//!   in the content-addressed store means the stored CAS bytes hash directly to `digest` (`AC-3`).
//! - `report.generate_report@1` (R1, D0) writes nothing to the Record Journal and places the
//!   resolved document only in scratch storage keyed by `digest` (`REQ-004`).
//! - `report.save_report@1` (W2, D0) reads the resolved document from scratch
//!   (`report.resolved_not_found` if absent), stores its canonical bytes in the CAS, links them
//!   to the report and journals one W2 event holding the digest (`REQ-005`).
//! - Images and view snapshots travel by content digest only and never embed raw bytes (`REQ-006`).
//! - Zero floating-point types or formatting live in this module (CONSTITUTION C1 / RPT float rule).

use super::error::BindError;
use super::expand::{
    expand, CharacteristicItem, ExpandError, ExpandedBlock, ItemKind, ProjectModel, QueryProvider, Role,
};
use super::journal::{AtHead, HeadLookup, JournalReads, Lookup, LookupFault, Outcome as RecordOutcome};
use super::model::{Binding, JournalHead, QuantityUnit, Ref, ValueBinding};
use super::resolve::{resolve_block, BlockResolver, FieldSource, ResolveContext, Resolved, ResolverRegistry};
use crate::blocks::table::{resolve_table_cells, CellText, TablePayload, TableRowInput};
use crate::blocks::text::{
    resolve_chrome_runs, resolve_text_runs, ImagePayload, ResolvedRun, RunBlockKind, RunBlockPayload,
};
use crate::doc::defs::ID_PATTERN;
use crate::doc::kind::{BlockKind, PayloadError};
use crate::doc::model::parse_canonical_uuid;
use crate::doc::{
    Block, BlockId, BlockKindId, CanonicalJson, KindRegistry, Op, Report, ReportBackend, ReportError, ReportEvent,
    ReportId, StoredReport, TemplateRef, MAX_REPLAY_EVENTS,
};
use crate::fmt::{
    self as rpt_fmt, Catalog, Characteristic, DefaultDecimals, FmtContext, FmtError, GaugeId, InterpretationStatus,
    LimitSet, LimitUnit, LocaleError, LocaleRules, OutcomeText, SiValue, UnitOverride, ValueKind, ValueRecord,
    ValueText,
};
use crate::tmpl::format::{parse_template, serialise as serialise_template, Template, TemplateId};
use crate::tmpl::starter::{lookup_label as starter_lookup_label, resolve_starter_name};
use crate::tmpl::ver::{BuiltinRegistry, ObjectRead};
use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue};
use fid_io_res::trans::model::TranscriptionRecord;
use fid_kdec::{Catalog as RuleCatalog, Decision, EvidenceBasis, NotDecidableReason, Outcome as DecOutcome, RuleRef};
use fid_kunc::uec::RecordTable;
use fid_kunc::UncertaintyStatement;
use fid_num::fmt::{FormatterId, Unit};
use fid_num::kdec::Decimal;
use fid_project::Uuid;
use fid_registry::{fid_command, fid_commands, CommandDef, DefError, FidSchema};
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Schema version of [`ResolvedReport`] (`"1"` in canonical JSON).
pub const RESOLVED_REPORT_SCHEMA_VERSION: u32 = 1;
/// Schema tag for [`ResolvedReport`].
pub const RESOLVED_REPORT_SCHEMA: &str = "resolved_report@1";
/// Registry command ID for generation (R1, D0).
pub const GENERATE_REPORT: &str = "report.generate_report@1";
/// Registry command ID for saving a generated report into the project CAS + journal (W2, D0).
pub const SAVE_REPORT: &str = "report.save_report@1";
/// CLI subcommand for `report.generate_report@1`.
pub const CLI_GENERATE_REPORT: &str = "generate-report";
/// CLI subcommand for `report.save_report@1`.
pub const CLI_SAVE_REPORT: &str = "save-report";
/// Non-fatal warning emitted when a `view` block has no snapshot digest (`REQ-007`).
pub const WARN_VIEW_MISSING: &str = "report.view_missing";
/// Pinned locale data release identifier (`I18N.LOCALE` artifact).
pub const DEFAULT_LOCALE_RELEASE: &str = "i18n-locale@1";
/// Default kernel version recorded when no record supplies one.
pub const DEFAULT_KERNEL_VERSION: &str = "0.1.0";
/// Maximum canonical byte size of a `ResolvedReport`.
pub const MAX_RESOLVED_BYTES: usize = 64 * 1024 * 1024;
/// Maximum JSON nesting depth when parsing a stored `ResolvedReport`.
pub const MAX_RESOLVED_DEPTH: usize = 64;
/// Maximum resolved documents kept in an in-memory scratch store.
pub const MAX_SCRATCH_ENTRIES: usize = 4_096;
/// Registered kind ID for a 3D/2D saved-view snapshot block (`AC-4`).
pub const VIEW_KIND_ID: &str = "view";
/// Schema marker for a `view` block payload v1.
pub const VIEW_SCHEMA: &str = "view_block@1";
/// Schema marker for a resolved `view` block payload v1 (snapshot digest only, never image bytes).
pub const RESOLVED_VIEW_SCHEMA: &str = "resolved_view@1";

const EMBEDDED_BYTES_KEYS: [&str; 6] =
    ["bytes", "image_bytes", "snapshot_bytes", "raw_bytes", "png_bytes", "jpeg_bytes"];

// ------------------------------------------------------------------ errors

/// Typed errors of report generation, scratch lookup, saving and replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenError {
    /// `report.resolved_not_found`: scratch holds no resolved report with this digest (`REQ-005`).
    ResolvedNotFound(Digest),
    /// `report.not_found`: named report, template, or journal head does not exist.
    NotFound(String),
    /// `report.embedded_bytes_refused`: a block payload or layout tried to embed raw image/snapshot bytes (`REQ-006`).
    EmbeddedBytesRefused(String),
    /// `report.bad_params`: malformed command, CLI, locale, or unit parameters.
    BadParams(&'static str),
    /// `report.store_invalid`: canonical JSON, digest verification, or CAS integrity failed.
    StoreInvalid(&'static str),
    /// `report.limit_exceeded`: scratch or resolved document byte bound exceeded.
    LimitExceeded(&'static str),
    /// Error propagated from binding or resolution (`RPT.MODEL.BIND.RESOLVE`).
    Bind(BindError),
    /// Error propagated from template slot expansion (`RPT.MODEL.BIND.EXPAND`).
    Expand(ExpandError),
    /// Error propagated from value formatting (`RPT.MODEL.FMT`).
    Format(FmtError),
    /// Error propagated from locale rules (`I18N.LOCALE`).
    Locale(LocaleError),
    /// Error propagated from the document model (`RPT.MODEL.DOC`).
    Report(ReportError),
}

impl From<BindError> for GenError {
    fn from(e: BindError) -> GenError {
        GenError::Bind(e)
    }
}

impl From<ExpandError> for GenError {
    fn from(e: ExpandError) -> GenError {
        GenError::Expand(e)
    }
}

impl From<FmtError> for GenError {
    fn from(e: FmtError) -> GenError {
        GenError::Format(e)
    }
}

impl From<LocaleError> for GenError {
    fn from(e: LocaleError) -> GenError {
        GenError::Locale(e)
    }
}

impl From<ReportError> for GenError {
    fn from(e: ReportError) -> GenError {
        GenError::Report(e)
    }
}

impl GenError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            GenError::ResolvedNotFound(_) => "report.resolved_not_found",
            GenError::NotFound(_) => "report.not_found",
            GenError::EmbeddedBytesRefused(_) => "report.embedded_bytes_refused",
            GenError::BadParams(_) => "report.bad_params",
            GenError::StoreInvalid(_) => "report.store_invalid",
            GenError::LimitExceeded(_) => "report.limit_exceeded",
            GenError::Bind(e) => e.code(),
            GenError::Expand(e) => e.code(),
            GenError::Format(e) => e.code(),
            GenError::Locale(_) => "report.locale_invalid",
            GenError::Report(e) => e.code(),
        }
    }

    /// Canonical `--json` / MCP error object.
    pub fn to_value(&self) -> CValue {
        match self {
            GenError::Bind(e) => e.to_value(),
            GenError::Expand(e) => e.to_value(),
            GenError::Report(e) => e.to_value(),
            other => {
                let mut o = BTreeMap::new();
                o.insert("code".to_string(), CValue::Str(other.code().to_string()));
                match other {
                    GenError::ResolvedNotFound(d) => {
                        o.insert("digest".to_string(), CValue::Str(d.as_str().to_string()));
                    }
                    GenError::NotFound(id) => {
                        o.insert("id".to_string(), CValue::Str(id.clone()));
                    }
                    GenError::EmbeddedBytesRefused(field) => {
                        o.insert("field".to_string(), CValue::Str(field.clone()));
                    }
                    GenError::BadParams(r) | GenError::StoreInvalid(r) | GenError::LimitExceeded(r) => {
                        o.insert("reason".to_string(), CValue::Str((*r).to_string()));
                    }
                    GenError::Format(e) => {
                        o.insert("detail".to_string(), CValue::Str(e.to_string()));
                    }
                    GenError::Locale(e) => {
                        o.insert("detail".to_string(), CValue::Str(e.to_string()));
                    }
                    GenError::Bind(_) | GenError::Expand(_) | GenError::Report(_) => {}
                }
                CValue::Obj(o)
            }
        }
    }
}

impl fmt::Display for GenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GenError::ResolvedNotFound(d) => write!(f, "{}: {}", self.code(), d.as_str()),
            GenError::NotFound(id) => write!(f, "{}: {id}", self.code()),
            GenError::EmbeddedBytesRefused(field) => write!(f, "{}: {field}", self.code()),
            GenError::BadParams(r) | GenError::StoreInvalid(r) | GenError::LimitExceeded(r) => {
                write!(f, "{}: {r}", self.code())
            }
            GenError::Bind(e) => e.fmt(f),
            GenError::Expand(e) => e.fmt(f),
            GenError::Format(e) => e.fmt(f),
            GenError::Locale(e) => e.fmt(f),
            GenError::Report(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for GenError {}

// ------------------------------------------------------------------ pinned locale rules

/// Resolves a pinned locale rule set from the `I18N.LOCALE` release table.
pub fn default_locale_rules(locale_id: &str) -> Result<LocaleRules, GenError> {
    let rules = match locale_id {
        "en" | "en-US" => LocaleRules::new(locale_id, DEFAULT_LOCALE_RELEASE, '.', Some(','), 3, 3, "-")?,
        "de" | "de-DE" => LocaleRules::new(locale_id, DEFAULT_LOCALE_RELEASE, ',', Some('.'), 3, 3, "-")?,
        "en-IN" => LocaleRules::new("en-IN", DEFAULT_LOCALE_RELEASE, '.', Some(','), 3, 2, "-")?,
        "ar" => LocaleRules::new("ar", DEFAULT_LOCALE_RELEASE, '\u{066b}', Some('\u{066c}'), 3, 3, "\u{061c}-")?,
        "fr" | "fr-FR" => LocaleRules::new(locale_id, DEFAULT_LOCALE_RELEASE, ',', Some('\u{202f}'), 3, 3, "-")?,
        "ja" | "ja-JP" => LocaleRules::new(locale_id, DEFAULT_LOCALE_RELEASE, '.', Some(','), 3, 3, "-")?,
        _ => return Err(GenError::BadParams("unknown_locale")),
    };
    Ok(rules)
}

// ------------------------------------------------------------------ unit override tag

/// Provenance representation of the `units` override passed to `report.generate_report@1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitOverrideMode {
    /// Use each characteristic's authored display unit.
    Authored,
    /// Override length to metres (`m`) and angles to radians (`rad`).
    Si,
}

impl UnitOverrideMode {
    pub fn parse(s: &str) -> Option<UnitOverrideMode> {
        match s {
            "authored" => Some(UnitOverrideMode::Authored),
            "si" => Some(UnitOverrideMode::Si),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            UnitOverrideMode::Authored => "authored",
            UnitOverrideMode::Si => "si",
        }
    }

    pub fn to_unit_override(self) -> UnitOverride {
        match self {
            UnitOverrideMode::Authored => UnitOverride::NONE,
            UnitOverrideMode::Si => UnitOverride::new(Some(Unit::M), Some(Unit::Rad)).unwrap_or(UnitOverride::NONE),
        }
    }
}

// ------------------------------------------------------------------ ResolvedReport data model

/// Optional positioning frame on a [`GeneratedBlock`], stored as exact decimal strings (no floats).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockFrame {
    pub x: String,
    pub y: String,
    pub width: String,
    pub height: String,
}

impl BlockFrame {
    pub fn to_value(&self) -> CValue {
        obj([
            ("height", CValue::Str(self.height.clone())),
            ("width", CValue::Str(self.width.clone())),
            ("x", CValue::Str(self.x.clone())),
            ("y", CValue::Str(self.y.clone())),
        ])
    }

    pub fn from_value(v: &CValue) -> Result<BlockFrame, GenError> {
        let o = exact_obj(v, &["height", "width", "x", "y"])?;
        Ok(BlockFrame {
            x: req_str(o, "x")?.to_string(),
            y: req_str(o, "y")?.to_string(),
            width: req_str(o, "width")?.to_string(),
            height: req_str(o, "height")?.to_string(),
        })
    }
}

/// One kernel formatter used during generation (`(FormatterId, Version)`), ordered canonically.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FormatterVersion {
    /// Formatter identifier including major tag (e.g. `fmt.length@1`).
    pub id: String,
    /// Major version string (e.g. `"1"`).
    pub version: String,
}

impl FormatterVersion {
    pub fn of(fid: FormatterId) -> FormatterVersion {
        let wire = fid.to_string();
        let version = wire.split_once('@').map_or("1", |(_, v)| v).to_string();
        FormatterVersion { id: wire, version }
    }

    pub fn to_value(&self) -> CValue {
        obj([("id", CValue::Str(self.id.clone())), ("version", CValue::Str(self.version.clone()))])
    }

    pub fn from_value(v: &CValue) -> Result<FormatterVersion, GenError> {
        let o = exact_obj(v, &["id", "version"])?;
        Ok(FormatterVersion { id: req_str(o, "id")?.to_string(), version: req_str(o, "version")?.to_string() })
    }
}

/// Non-fatal warning collected during generation, sorted by `(block_id, code, detail)` (`REQ-007`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct GenWarning {
    pub block_id: Uuid,
    pub code: String,
    pub detail: Option<String>,
}

impl GenWarning {
    pub fn to_value(&self) -> CValue {
        obj([
            ("block_id", CValue::Str(self.block_id.to_string())),
            ("code", CValue::Str(self.code.clone())),
            ("detail", self.detail.as_ref().map_or(CValue::Null, |s| CValue::Str(s.clone()))),
        ])
    }

    pub fn from_value(v: &CValue) -> Result<GenWarning, GenError> {
        let o = exact_obj(v, &["block_id", "code", "detail"])?;
        let block_id =
            parse_canonical_uuid(req_str(o, "block_id")?).ok_or(GenError::StoreInvalid("warning block_id"))?;
        let code = req_str(o, "code")?.to_string();
        let detail = opt_str(o, "detail")?;
        Ok(GenWarning { block_id, code, detail })
    }
}

/// Provenance header recorded inside every [`ResolvedReport`] (`REQ-003`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub head: JournalHead,
    pub template: Option<TemplateRef>,
    pub kernel_version: String,
    pub formatters: Vec<FormatterVersion>,
    pub locale: String,
    pub locale_release: String,
    pub units: Option<UnitOverrideMode>,
}

impl Provenance {
    pub fn to_value(&self) -> CValue {
        let tmpl = match &self.template {
            None => CValue::Null,
            Some(t) => obj([
                ("builtin", CValue::Bool(t.builtin)),
                ("digest", CValue::Str(t.digest.as_str().to_string())),
                ("id", CValue::Str(t.id.to_string())),
                ("version", CValue::Str(t.version.to_string())),
            ]),
        };
        obj([
            ("formatters", CValue::Arr(self.formatters.iter().map(FormatterVersion::to_value).collect())),
            ("head", CValue::Str(self.head.digest().as_str().to_string())),
            ("kernel_version", CValue::Str(self.kernel_version.clone())),
            ("locale", CValue::Str(self.locale.clone())),
            ("locale_release", CValue::Str(self.locale_release.clone())),
            ("template", tmpl),
            ("units", self.units.map_or(CValue::Null, |u| CValue::Str(u.as_str().to_string()))),
        ])
    }

    pub fn from_value(v: &CValue) -> Result<Provenance, GenError> {
        let o =
            exact_obj(v, &["formatters", "head", "kernel_version", "locale", "locale_release", "template", "units"])?;
        let head = JournalHead::parse(req_str(o, "head")?).ok_or(GenError::StoreInvalid("provenance head"))?;
        let kernel_version = req_str(o, "kernel_version")?.to_string();
        let locale = req_str(o, "locale")?.to_string();
        let locale_release = req_str(o, "locale_release")?.to_string();
        let units = match opt_str(o, "units")? {
            None => None,
            Some(s) => Some(UnitOverrideMode::parse(&s).ok_or(GenError::StoreInvalid("provenance units"))?),
        };
        let template = match o.get("template") {
            Some(CValue::Null) => None,
            Some(tv) => {
                let to = exact_obj(tv, &["builtin", "digest", "id", "version"])?;
                let builtin = match to.get("builtin") {
                    Some(CValue::Bool(b)) => *b,
                    _ => return Err(GenError::StoreInvalid("template builtin")),
                };
                let digest =
                    Digest::parse(req_str(to, "digest")?).map_err(|_| GenError::StoreInvalid("template digest"))?;
                let id = parse_canonical_uuid(req_str(to, "id")?).ok_or(GenError::StoreInvalid("template id"))?;
                let version =
                    req_str(to, "version")?.parse::<u32>().map_err(|_| GenError::StoreInvalid("template version"))?;
                Some(TemplateRef { id, version, digest, builtin })
            }
            None => return Err(GenError::StoreInvalid("template missing")),
        };
        let Some(CValue::Arr(fmts)) = o.get("formatters") else {
            return Err(GenError::StoreInvalid("formatters not array"));
        };
        let mut formatters = Vec::with_capacity(fmts.len());
        for f in fmts {
            formatters.push(FormatterVersion::from_value(f)?);
        }
        Ok(Provenance { head, template, kernel_version, formatters, locale, locale_release, units })
    }
}

/// One resolved block envelope in a [`ResolvedPage`] (`Design: ResolvedBlock`).
#[derive(Debug, Clone, PartialEq)]
pub struct GeneratedBlock {
    pub id: Uuid,
    pub kind: BlockKindId,
    pub kind_version: u32,
    pub frame: Option<BlockFrame>,
    pub payload: CValue,
}

impl GeneratedBlock {
    pub fn to_value(&self) -> CValue {
        obj([
            ("frame", self.frame.as_ref().map_or(CValue::Null, BlockFrame::to_value)),
            ("id", CValue::Str(self.id.to_string())),
            ("kind", CValue::Str(self.kind.as_str().to_string())),
            ("kind_version", CValue::Str(self.kind_version.to_string())),
            ("payload", self.payload.clone()),
        ])
    }

    pub fn from_value(v: &CValue) -> Result<GeneratedBlock, GenError> {
        let o = exact_obj(v, &["frame", "id", "kind", "kind_version", "payload"])?;
        let id = parse_canonical_uuid(req_str(o, "id")?).ok_or(GenError::StoreInvalid("block id"))?;
        let kind = BlockKindId::new(req_str(o, "kind")?).ok_or(GenError::StoreInvalid("block kind"))?;
        let kind_version =
            req_str(o, "kind_version")?.parse::<u32>().map_err(|_| GenError::StoreInvalid("block kind_version"))?;
        let frame = match o.get("frame") {
            Some(CValue::Null) => None,
            Some(fv) => Some(BlockFrame::from_value(fv)?),
            None => return Err(GenError::StoreInvalid("block frame missing")),
        };
        let payload = o.get("payload").ok_or(GenError::StoreInvalid("block payload missing"))?.clone();
        reject_embedded_bytes(&payload)?;
        Ok(GeneratedBlock { id, kind, kind_version, frame, payload })
    }
}

/// One resolved page in a [`ResolvedReport`].
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPage {
    pub id: Uuid,
    pub blocks: Vec<GeneratedBlock>,
}

impl ResolvedPage {
    pub fn to_value(&self) -> CValue {
        obj([
            ("blocks", CValue::Arr(self.blocks.iter().map(GeneratedBlock::to_value).collect())),
            ("id", CValue::Str(self.id.to_string())),
        ])
    }

    pub fn from_value(v: &CValue) -> Result<ResolvedPage, GenError> {
        let o = exact_obj(v, &["blocks", "id"])?;
        let id = parse_canonical_uuid(req_str(o, "id")?).ok_or(GenError::StoreInvalid("page id"))?;
        let Some(CValue::Arr(bs)) = o.get("blocks") else {
            return Err(GenError::StoreInvalid("page blocks not array"));
        };
        let mut blocks = Vec::with_capacity(bs.len());
        for b in bs {
            blocks.push(GeneratedBlock::from_value(b)?);
        }
        Ok(ResolvedPage { id, blocks })
    }
}

/// The resolved document contract consumed by output, preview and forms (`RPT.MODEL.BIND.GEN`).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedReport {
    pub schema_version: u32,
    pub provenance: Provenance,
    pub pages: Vec<ResolvedPage>,
    pub warnings: Vec<GenWarning>,
    /// SHA-256 of the RFC 8785 canonical JSON of `{pages, provenance, schema_version, warnings}`
    /// (excluding `digest`, `REQ-RPT.MODEL.BIND.GEN-001`).
    pub digest: Digest,
}

impl ResolvedReport {
    /// Builds a [`ResolvedReport`], sorting formatters and warnings deterministically and
    /// computing `digest` from its canonical JSON excluding `digest` (`REQ-001`).
    pub fn finalize(
        mut provenance: Provenance,
        pages: Vec<ResolvedPage>,
        mut warnings: Vec<GenWarning>,
    ) -> Result<ResolvedReport, GenError> {
        for p in &pages {
            for b in &p.blocks {
                reject_embedded_bytes(&b.payload)?;
            }
        }
        provenance.formatters.sort();
        provenance.formatters.dedup();
        warnings.sort();
        warnings.dedup();
        let placeholder = ResolvedReport {
            schema_version: RESOLVED_REPORT_SCHEMA_VERSION,
            provenance,
            pages,
            warnings,
            digest: Digest::from_bytes([0u8; 32]),
        };
        let content_bytes = placeholder.to_content_canonical_bytes()?;
        let digest = Digest::from_bytes(digest_bytes(&content_bytes).0);
        Ok(ResolvedReport { digest, ..placeholder })
    }

    /// Canonical JSON value WITHOUT the `digest` field (the exact pre-image of `self.digest`).
    pub fn to_content_value(&self) -> CValue {
        obj([
            ("pages", CValue::Arr(self.pages.iter().map(ResolvedPage::to_value).collect())),
            ("provenance", self.provenance.to_value()),
            ("schema", CValue::Str(RESOLVED_REPORT_SCHEMA.to_string())),
            ("schema_version", CValue::Str(self.schema_version.to_string())),
            ("warnings", CValue::Arr(self.warnings.iter().map(GenWarning::to_value).collect())),
        ])
    }

    /// RFC 8785 canonical bytes WITHOUT the `digest` field.
    ///
    /// Stored in the CAS by `report.save_report@1` so that `sha256(cas_bytes) == self.digest`
    /// (`REQ-001`, `REQ-005`, `AC-3`).
    pub fn to_content_canonical_bytes(&self) -> Result<Vec<u8>, GenError> {
        let bytes = to_canonical_bytes(&self.to_content_value())
            .map_err(|_| GenError::StoreInvalid("resolved content not canonical"))?;
        if bytes.len() > MAX_RESOLVED_BYTES {
            return Err(GenError::LimitExceeded("resolved_report_too_large"));
        }
        Ok(bytes)
    }

    /// Full canonical JSON value WITH the `"digest"` field (wire / CLI / golden form).
    pub fn to_value(&self) -> CValue {
        let mut m = match self.to_content_value() {
            CValue::Obj(o) => o,
            _ => BTreeMap::new(),
        };
        m.insert("digest".to_string(), CValue::Str(self.digest.as_str().to_string()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical bytes WITH `"digest"`.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, GenError> {
        let bytes = to_canonical_bytes(&self.to_value())
            .map_err(|_| GenError::StoreInvalid("resolved envelope not canonical"))?;
        if bytes.len() > MAX_RESOLVED_BYTES {
            return Err(GenError::LimitExceeded("resolved_report_too_large"));
        }
        Ok(bytes)
    }

    /// Reads and verifies a [`ResolvedReport`] from CAS canonical content bytes (excluding `digest`),
    /// recomputing `digest = sha256(bytes)`.
    pub fn from_cas_bytes(bytes: &[u8]) -> Result<ResolvedReport, GenError> {
        if bytes.len() > MAX_RESOLVED_BYTES {
            return Err(GenError::LimitExceeded("resolved_report_too_large"));
        }
        let v = parse_with_limits(bytes, MAX_RESOLVED_BYTES, MAX_RESOLVED_DEPTH)
            .map_err(|_| GenError::StoreInvalid("resolved json invalid"))?;
        if to_canonical_bytes(&v).map_err(|_| GenError::StoreInvalid("resolved canonicalize"))? != bytes {
            return Err(GenError::StoreInvalid("resolved bytes not canonical"));
        }
        let o = exact_obj(&v, &["pages", "provenance", "schema", "schema_version", "warnings"])?;
        if req_str(o, "schema")? != RESOLVED_REPORT_SCHEMA {
            return Err(GenError::StoreInvalid("resolved schema"));
        }
        let schema_version = req_str(o, "schema_version")?
            .parse::<u32>()
            .map_err(|_| GenError::StoreInvalid("resolved schema_version"))?;
        if schema_version != RESOLVED_REPORT_SCHEMA_VERSION {
            return Err(GenError::StoreInvalid("resolved unsupported schema_version"));
        }
        let provenance =
            Provenance::from_value(o.get("provenance").ok_or(GenError::StoreInvalid("provenance missing"))?)?;
        let Some(CValue::Arr(ps)) = o.get("pages") else {
            return Err(GenError::StoreInvalid("pages not array"));
        };
        let mut pages = Vec::with_capacity(ps.len());
        for p in ps {
            pages.push(ResolvedPage::from_value(p)?);
        }
        let Some(CValue::Arr(ws)) = o.get("warnings") else {
            return Err(GenError::StoreInvalid("warnings not array"));
        };
        let mut warnings = Vec::with_capacity(ws.len());
        for w in ws {
            warnings.push(GenWarning::from_value(w)?);
        }
        let digest = Digest::from_bytes(digest_bytes(bytes).0);
        Ok(ResolvedReport { schema_version, provenance, pages, warnings, digest })
    }

    /// Reads and verifies a full wire [`ResolvedReport`] envelope (including `"digest"`), verifying
    /// that `digest` matches the SHA-256 of the content without `"digest"`.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<ResolvedReport, GenError> {
        if bytes.len() > MAX_RESOLVED_BYTES {
            return Err(GenError::LimitExceeded("resolved_report_too_large"));
        }
        let v = parse_with_limits(bytes, MAX_RESOLVED_BYTES, MAX_RESOLVED_DEPTH)
            .map_err(|_| GenError::StoreInvalid("resolved json invalid"))?;
        if to_canonical_bytes(&v).map_err(|_| GenError::StoreInvalid("resolved canonicalize"))? != bytes {
            return Err(GenError::StoreInvalid("resolved bytes not canonical"));
        }
        let o = exact_obj(&v, &["digest", "pages", "provenance", "schema", "schema_version", "warnings"])?;
        let expected = Digest::parse(req_str(o, "digest")?).map_err(|_| GenError::StoreInvalid("resolved digest"))?;
        let mut without = o.clone();
        without.remove("digest");
        let content_bytes = to_canonical_bytes(&CValue::Obj(without))
            .map_err(|_| GenError::StoreInvalid("resolved content canonicalize"))?;
        let parsed = ResolvedReport::from_cas_bytes(&content_bytes)?;
        if parsed.digest != expected {
            return Err(GenError::StoreInvalid("resolved digest mismatch"));
        }
        Ok(parsed)
    }
}

/// Walks a `CValue` iteratively to guarantee no embedded image/snapshot raw byte keys (`REQ-006`).
pub fn reject_embedded_bytes(v: &CValue) -> Result<(), GenError> {
    let mut stack: Vec<(String, &CValue)> = vec![(String::new(), v)];
    while let Some((path, cur)) = stack.pop() {
        match cur {
            CValue::Obj(map) => {
                for (k, child) in map {
                    let next = format!("{path}/{k}");
                    if EMBEDDED_BYTES_KEYS.contains(&k.as_str()) {
                        return Err(GenError::EmbeddedBytesRefused(next));
                    }
                    stack.push((next, child));
                }
            }
            CValue::Arr(arr) => {
                for (i, child) in arr.iter().enumerate() {
                    stack.push((format!("{path}/{i}"), child));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ view block kind (AC-4)

/// `view` block kind (v1) referencing a saved `named_view` by ID and resolving to a snapshot
/// content digest only — never embedding image bytes (`REQ-006`, `AC-4`).
#[derive(Debug, Clone, Copy, Default)]
pub struct ViewBlockKind;

impl BlockKind for ViewBlockKind {
    fn id(&self) -> BlockKindId {
        BlockKindId::from_literal(VIEW_KIND_ID)
    }

    fn version(&self) -> u32 {
        1
    }

    fn validate(&self, payload: &CanonicalJson) -> Result<(), PayloadError> {
        let CValue::Obj(o) = payload.value() else {
            return Err(PayloadError::new("", "not_object"));
        };
        if o.get("schema") != Some(&CValue::Str(VIEW_SCHEMA.to_string())) {
            return Err(PayloadError::new("/schema", "bad_schema"));
        }
        for k in o.keys() {
            if !matches!(k.as_str(), "schema" | "view_id" | "binding" | "caption") {
                return Err(PayloadError::new(&format!("/{k}"), "unknown_field"));
            }
        }
        match o.get("view_id") {
            Some(CValue::Str(s)) if parse_canonical_uuid(s).is_some() => {}
            _ => return Err(PayloadError::new("/view_id", "invalid_uuid")),
        }
        if let Some(cap) = o.get("caption") {
            if !matches!(cap, CValue::Null | CValue::Str(_)) {
                return Err(PayloadError::new("/caption", "not_string"));
            }
        }
        Ok(())
    }

    fn value_fields(&self, _payload: &CanonicalJson) -> Result<Vec<String>, PayloadError> {
        Ok(Vec::new())
    }
}

/// [`BlockResolver`] for `view` blocks (`AC-4`).
#[derive(Debug, Clone, Copy, Default)]
pub struct ViewBlockResolver;

impl BlockResolver for ViewBlockResolver {
    fn kind(&self) -> BlockKindId {
        BlockKindId::from_literal(VIEW_KIND_ID)
    }

    fn place(
        &self,
        payload: &CanonicalJson,
        binding: &Binding,
        _values: &[ValueBinding],
    ) -> Result<CValue, PayloadError> {
        let CValue::Obj(mut o) = payload.value().clone() else {
            return Err(PayloadError::new("", "not_object"));
        };
        if let Some(Ref::NamedView(vid)) = binding.refs().iter().find(|r| matches!(r, Ref::NamedView(_))) {
            o.insert("view_id".to_string(), CValue::Str(vid.to_string()));
        }
        o.remove("binding");
        Ok(CValue::Obj(o))
    }

    fn resolve(&self, view: &Resolved, _ctx: &ResolveContext<'_>, _lookup: &dyn Lookup) -> Result<CValue, BindError> {
        let refs: Vec<CValue> = view
            .refs()
            .iter()
            .filter_map(|r| match r {
                super::resolve::ResolvedRef::NamedView(ev) => Some(obj([
                    ("digest", CValue::Str(ev.digest().as_str().to_string())),
                    ("id", CValue::Str(ev.id().to_string())),
                ])),
                _ => None,
            })
            .collect();
        Ok(obj([("kind", CValue::Str(VIEW_KIND_ID.to_string())), ("views", CValue::Arr(refs))]))
    }
}

/// Builds a canonical `view` block payload v1 referencing `view_id` (`AC-4`).
pub fn view_block_payload(view_id: Uuid, caption: Option<&str>) -> Result<CanonicalJson, GenError> {
    let mut o = BTreeMap::new();
    o.insert("binding".to_string(), Binding::new(vec![Ref::NamedView(view_id)])?.to_value());
    o.insert("caption".to_string(), caption.map_or(CValue::Null, |s| CValue::Str(s.to_string())));
    o.insert("schema".to_string(), CValue::Str(VIEW_SCHEMA.to_string()));
    o.insert("view_id".to_string(), CValue::Str(view_id.to_string()));
    CanonicalJson::new(&CValue::Obj(o)).ok_or(GenError::StoreInvalid("view payload canonical"))
}

// ------------------------------------------------------------------ metadata seam

/// Extra per-record and per-view metadata consulted during generation (transcriptions for
/// `RESULTS_ONLY`, gauges for `DIRECT_READING`, snapshot digests for `view` blocks).
pub trait GenMetadata {
    /// Transcription record for a `RESULTS_ONLY` ResultRecord, if available.
    fn transcription(&self, _record_id: &Uuid) -> Option<&TranscriptionRecord> {
        None
    }

    /// Gauge ID for a `DIRECT_READING` ResultRecord, if available.
    fn gauge(&self, _record_id: &Uuid) -> Option<GaugeId> {
        None
    }

    /// Content-addressed snapshot digest for a `named_view` entity, if a snapshot exists (`AC-4`).
    fn view_snapshot(&self, _view_id: &Uuid) -> Option<Digest> {
        None
    }

    /// Optional overrides for a characteristic's formatting metadata (unit, decimals, limits).
    fn characteristic_override(&self, _char_id: &Uuid) -> Option<Characteristic> {
        None
    }

    /// Optional project title or report ID string for `text`/`header`/`footer` page fields.
    fn project_title(&self) -> Option<&str> {
        None
    }
}

/// In-memory metadata table for tests and CLI execution.
#[derive(Debug, Clone, Default)]
pub struct MemoryGenMetadata {
    pub transcriptions: BTreeMap<Uuid, TranscriptionRecord>,
    pub default_transcription: Option<TranscriptionRecord>,
    pub gauges: BTreeMap<Uuid, GaugeId>,
    pub view_snapshots: BTreeMap<Uuid, Digest>,
    pub characteristics: BTreeMap<Uuid, Characteristic>,
    pub project_title: Option<String>,
}

impl GenMetadata for MemoryGenMetadata {
    fn transcription(&self, record_id: &Uuid) -> Option<&TranscriptionRecord> {
        self.transcriptions.get(record_id).or(self.default_transcription.as_ref())
    }

    fn gauge(&self, record_id: &Uuid) -> Option<GaugeId> {
        self.gauges.get(record_id).cloned()
    }

    fn view_snapshot(&self, view_id: &Uuid) -> Option<Digest> {
        self.view_snapshots.get(view_id).copied()
    }

    fn characteristic_override(&self, char_id: &Uuid) -> Option<Characteristic> {
        self.characteristics.get(char_id).cloned()
    }

    fn project_title(&self) -> Option<&str> {
        self.project_title.as_deref()
    }
}

// ------------------------------------------------------------------ generation inputs

/// What [`generate`] generates from: an authored report (optionally tied to a template version)
/// or a template version directly.
pub enum GenSource<'a> {
    /// An authored report document, with its referenced template when `report.template_ref()` is set.
    Report { report: &'a Report, template: Option<&'a Template> },
    /// A template directly, with its pinned [`TemplateRef`].
    Template { template: &'a Template, template_ref: TemplateRef },
}

/// Inputs to [`generate`].
pub struct GenContext<'a> {
    pub project: Uuid,
    pub head: JournalHead,
    pub stages: Vec<Uuid>,
    pub runs: Vec<Uuid>,
    pub sources: Vec<&'a dyn FieldSource>,
    pub locale: LocaleRules,
    pub units: Option<UnitOverrideMode>,
    pub default_decimals: DefaultDecimals,
    pub catalog: &'a Catalog,
    pub kernel_version: String,
    pub metadata: &'a dyn GenMetadata,
    pub query_provider: Option<&'a dyn QueryProvider>,
}

// ------------------------------------------------------------------ journal parsing helpers (zero floats!)

struct ParsedCharacteristic {
    name: String,
    spec: Characteristic,
}

fn parse_bits_hex(s: &str) -> Option<u64> {
    let hex = s.strip_prefix("0x")?;
    if hex.len() != 16 || !hex.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
        return None;
    }
    u64::from_str_radix(hex, 16).ok()
}

fn value_kind_and_default_unit(q_unit: QuantityUnit) -> (ValueKind, Unit) {
    match q_unit {
        QuantityUnit::Metre => (ValueKind::Length, Unit::Mm),
        QuantityUnit::Radian => (ValueKind::Angle, Unit::Deg),
        QuantityUnit::One => (ValueKind::Scalar, Unit::One),
    }
}

fn parse_display_unit(s: &str) -> Option<Unit> {
    match s {
        "m" => Some(Unit::M),
        "mm" => Some(Unit::Mm),
        "um" | "µm" => Some(Unit::Um),
        "in" => Some(Unit::In),
        "rad" => Some(Unit::Rad),
        "deg" => Some(Unit::Deg),
        "dms" => Some(Unit::Dms),
        "1" => Some(Unit::One),
        _ => None,
    }
}

fn load_characteristic(
    cid: Uuid,
    seq: u64,
    journal: &dyn JournalReads,
    lookup: &HeadLookup<'_>,
    metadata: &dyn GenMetadata,
    block_id: BlockId,
) -> Result<ParsedCharacteristic, GenError> {
    // Validate ownership, kind and head visibility via HeadLookup first.
    let _ev = lookup.characteristic(&cid).map_err(|f| map_lookup_fault(block_id, &cid.to_string(), f))?;
    if let Some(ov) = metadata.characteristic_override(&cid) {
        return Ok(ParsedCharacteristic { name: ov.id.clone(), spec: ov });
    }
    let AtHead::Committed(j) = journal.entity(&cid, seq) else {
        return Err(GenError::NotFound(cid.to_string()));
    };
    let v = parse_with_limits(&j.bytes, 1024 * 1024, 32).map_err(|_| GenError::StoreInvalid("characteristic json"))?;
    let CValue::Obj(o) = &v else {
        return Err(GenError::StoreInvalid("characteristic not object"));
    };
    let name = match o.get("name") {
        Some(CValue::Str(s)) => s.clone(),
        _ => cid.to_string(),
    };
    let (kind, mut unit, nominal) = match o.get("nominal") {
        Some(CValue::Obj(nom)) => {
            let qu = match nom.get("unit") {
                Some(CValue::Str(u)) => QuantityUnit::parse(u).unwrap_or(QuantityUnit::Metre),
                _ => QuantityUnit::Metre,
            };
            let (vk, du) = value_kind_and_default_unit(qu);
            let sv = match nom.get("value") {
                Some(CValue::Str(bits)) => parse_bits_hex(bits).map(SiValue::from_bits),
                _ => None,
            };
            (Some(vk), du, sv)
        }
        _ => (Some(ValueKind::Length), Unit::Mm, None),
    };
    if let Some(CValue::Str(u)) = o.get("display_unit") {
        if let Some(parsed) = parse_display_unit(u) {
            unit = parsed;
        }
    }
    let decimals = match o.get("decimals") {
        Some(CValue::Str(s)) => s.parse::<u8>().ok(),
        Some(CValue::U64(n)) => u8::try_from(*n).ok(),
        _ => None,
    };
    let limits = match o.get("limits") {
        Some(CValue::Obj(lim)) => {
            let lower = match lim.get("lower") {
                Some(CValue::Str(s)) => Decimal::parse(s).ok(),
                _ => None,
            };
            let upper = match lim.get("upper") {
                Some(CValue::Str(s)) => Decimal::parse(s).ok(),
                _ => None,
            };
            let lunit = match lim.get("unit") {
                Some(CValue::Str(s)) if s == "deg" => LimitUnit::Deg,
                _ => LimitUnit::Si,
            };
            if lower.is_some() || upper.is_some() {
                Some(LimitSet { lower, upper, unit: lunit })
            } else {
                None
            }
        }
        _ => None,
    };
    Ok(ParsedCharacteristic {
        name: name.clone(),
        spec: Characteristic { id: name, kind, unit, decimals, limits, nominal },
    })
}

struct LoadedResult {
    id: Uuid,
    digest: Digest,
    value: SiValue,
    unit: QuantityUnit,
    basis: fid_gdt::chars::record::EvaluationBasis,
    decision: Decision,
    statement: UncertaintyStatement,
    interpretation: InterpretationStatus,
    kernel_version: Option<String>,
}

fn load_result(
    rid: Uuid,
    seq: u64,
    journal: &dyn JournalReads,
    lookup: &HeadLookup<'_>,
    rule_catalog: &RuleCatalog,
    block_id: BlockId,
) -> Result<LoadedResult, GenError> {
    let rv = lookup.result(&rid).map_err(|f| map_lookup_fault(block_id, &rid.to_string(), f))?;
    let vf = rv.values().first().ok_or(GenError::StoreInvalid("result has no value field"))?;
    let empty_table = RecordTable::default();
    let src_records = journal.source_records().unwrap_or(&empty_table);
    let statement = match vf.statement() {
        Some(sd) => {
            let sb = lookup.statement(&sd).map_err(|f| map_lookup_fault(block_id, &rid.to_string(), f))?;
            UncertaintyStatement::parse_canonical_verified(sb.as_bytes(), src_records)
                .map_err(|_| GenError::StoreInvalid("statement verify"))?
        }
        None => {
            // UEC-0: construct canonical UEC-0 statement bytes without touching floats.
            return Err(GenError::StoreInvalid("result_missing_statement"));
        }
    };
    let ev = rv.evidence();
    let rule = match ev.rule() {
        Some(r) => {
            let rr = RuleRef::parse(r).map_err(|_| GenError::StoreInvalid("decision rule ref"))?;
            Some(rule_catalog.resolve(&rr).map_err(|_| GenError::StoreInvalid("decision rule unknown"))?)
        }
        None => None,
    };
    let outcome = match ev.outcome() {
        RecordOutcome::Conforms => DecOutcome::Conforms,
        RecordOutcome::Nonconforms => DecOutcome::Nonconforms,
        RecordOutcome::Indeterminate => DecOutcome::Indeterminate,
        RecordOutcome::NotDecidable => {
            let reason = if rule.is_none() {
                NotDecidableReason::RuleMissing
            } else {
                match ev.reason() {
                    Some("label_only") => NotDecidableReason::LabelOnly,
                    Some("u_missing" | "unc_no_decision") => NotDecidableReason::UMissing,
                    Some("pdf_unavailable") => NotDecidableReason::PdfUnavailable,
                    _ => NotDecidableReason::RuleMissing,
                }
            };
            DecOutcome::NotDecidable(reason)
        }
    };
    let basis = if ev.decision_source() == "unc" || ev.rule().is_some() {
        Some(EvidenceBasis::of(&statement).map_err(|_| GenError::StoreInvalid("decision evidence basis"))?)
    } else {
        None
    };
    let decision =
        Decision::new(rule.as_ref(), outcome, basis).map_err(|_| GenError::StoreInvalid("decision construct"))?;
    let interpretation = InterpretationStatus::of_result(&rv);

    // Optional kernel_version extracted from the stored canonical record bytes.
    let kernel_version = match journal.entity(&rid, seq) {
        AtHead::Committed(j) => parse_with_limits(&j.bytes, 1024 * 1024, 32).ok().and_then(|v| match v {
            CValue::Obj(o) => match o.get("kernel_version") {
                Some(CValue::Str(s)) => Some(s.clone()),
                _ => None,
            },
            _ => None,
        }),
        _ => None,
    };

    Ok(LoadedResult {
        id: rid,
        digest: rv.digest(),
        value: vf.quantity().value(),
        unit: vf.quantity().unit(),
        basis: ev.basis(),
        decision,
        statement,
        interpretation,
        kernel_version,
    })
}

fn value_text_to_value(
    vt: &ValueText,
    block_id: Uuid,
    formatters: &mut BTreeSet<FormatterVersion>,
    warnings: &mut Vec<GenWarning>,
) -> CValue {
    formatters.insert(FormatterVersion::of(vt.formatter()));
    for w in vt.warnings() {
        warnings.push(GenWarning { block_id, code: w.code.to_string(), detail: Some(w.record.clone()) });
    }
    let shown = vt.text();
    let markers: Vec<CValue> = shown
        .markers
        .iter()
        .map(|m| obj([("key", CValue::Str(m.marker.key().to_string())), ("text", CValue::Str(m.text.clone()))]))
        .collect();
    let labels: Vec<CValue> = shown.labels.iter().map(|l| CValue::Str(l.text.clone())).collect();
    obj([
        ("canonical", CValue::Str(shown.canonical.to_string())),
        ("display", CValue::Str(shown.display.to_string())),
        ("formatter", CValue::Str(vt.formatter().to_string())),
        ("labels", CValue::Arr(labels)),
        ("markers", CValue::Arr(markers)),
        ("side", CValue::Str(vt.side().name().to_string())),
        ("unit", CValue::Str(shown.unit.symbol().to_string())),
    ])
}

fn outcome_text_to_value(ot: &OutcomeText) -> CValue {
    let shown = ot.text();
    let labels: Vec<CValue> = shown.labels.iter().map(|l| CValue::Str(l.text.clone())).collect();
    obj([
        ("labels", CValue::Arr(labels)),
        ("rule", shown.rule_text.map_or(CValue::Null, |r| CValue::Str(r.to_string()))),
        ("text", CValue::Str(shown.text.to_string())),
    ])
}

fn cell_text_to_value(
    cell: &CellText,
    block_id: Uuid,
    formatters: &mut BTreeSet<FormatterVersion>,
    warnings: &mut Vec<GenWarning>,
) -> CValue {
    match cell {
        CellText::Text(s) => obj([("kind", CValue::Str("text".to_string())), ("text", CValue::Str(s.clone()))]),
        CellText::Missing => obj([
            ("kind", CValue::Str("missing".to_string())),
            ("text", CValue::Str(crate::blocks::table::MISSING_TEXT.to_string())),
        ]),
        CellText::Value(vt) => obj([
            ("kind", CValue::Str("value".to_string())),
            ("value", value_text_to_value(vt, block_id, formatters, warnings)),
        ]),
        CellText::Outcome(ot) => {
            obj([("kind", CValue::Str("outcome".to_string())), ("outcome", outcome_text_to_value(ot))])
        }
        CellText::Uncertainty(u) => match u {
            rpt_fmt::UncertaintyText::NotEvaluated(s) => {
                obj([("kind", CValue::Str("uncertainty_not_evaluated".to_string())), ("text", CValue::Str(s.clone()))])
            }
            rpt_fmt::UncertaintyText::ClassOnly(s) => {
                obj([("kind", CValue::Str("uncertainty_class".to_string())), ("text", CValue::Str(s.clone()))])
            }
            rpt_fmt::UncertaintyText::Expanded { value, class_label } => obj([
                ("class", CValue::Str(class_label.clone())),
                ("kind", CValue::Str("uncertainty".to_string())),
                ("value", value_text_to_value(value, block_id, formatters, warnings)),
            ]),
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_view_block(
    block_id: Uuid,
    source_block: BlockId,
    payload: &CValue,
    seq: u64,
    journal: &dyn JournalReads,
    lookup: &HeadLookup<'_>,
    metadata: &dyn GenMetadata,
    warnings: &mut Vec<GenWarning>,
) -> Result<CValue, GenError> {
    reject_embedded_bytes(payload)?;
    let CValue::Obj(o) = payload else {
        return Err(GenError::StoreInvalid("view payload not object"));
    };
    let vid_str = match o.get("view_id") {
        Some(CValue::Str(s)) => s.as_str(),
        _ => return Err(GenError::StoreInvalid("view_id missing")),
    };
    let vid = parse_canonical_uuid(vid_str).ok_or(GenError::StoreInvalid("view_id invalid"))?;
    let caption = match o.get("caption") {
        Some(CValue::Str(s)) => Some(s.clone()),
        _ => None,
    };

    // Look up named_view at head. If absent -> report.view_missing warning.
    let (name, view_digest, snapshot_digest) = match lookup.named_view(&vid) {
        Ok(ev) => {
            let mut v_name = String::new();
            let mut snap = metadata.view_snapshot(&vid);
            if let AtHead::Committed(j) = journal.entity(&vid, seq) {
                if let Ok(CValue::Obj(vo)) = parse_with_limits(&j.bytes, 1024 * 1024, 32) {
                    reject_embedded_bytes(&CValue::Obj(vo.clone()))?;
                    if let Some(CValue::Str(n)) = vo.get("name") {
                        v_name = n.clone();
                    }
                    if snap.is_none() {
                        if let Some(CValue::Str(sd)) = vo.get("snapshot_digest") {
                            snap = Digest::parse(sd).ok();
                        }
                    }
                }
            }
            (v_name, Some(ev.digest()), snap)
        }
        Err(LookupFault::NotFound) => (String::new(), None, None),
        Err(other) => return Err(map_lookup_fault(source_block, vid_str, other)),
    };

    if snapshot_digest.is_none() {
        warnings.push(GenWarning { block_id, code: WARN_VIEW_MISSING.to_string(), detail: Some(vid.to_string()) });
    }

    Ok(obj([
        ("caption", caption.map_or(CValue::Null, CValue::Str)),
        ("name", CValue::Str(name)),
        ("schema", CValue::Str(RESOLVED_VIEW_SCHEMA.to_string())),
        ("snapshot_digest", snapshot_digest.map_or(CValue::Null, |d| CValue::Str(d.as_str().to_string()))),
        ("view_digest", view_digest.map_or(CValue::Null, |d| CValue::Str(d.as_str().to_string()))),
        ("view_id", CValue::Str(vid.to_string())),
    ]))
}

fn resolve_starter_or_text_payload(
    block_id: Uuid,
    payload: &CValue,
    fmt_ctx: &FmtContext<'_>,
    _metadata: &dyn GenMetadata,
    report_id: Option<ReportId>,
    formatters: &mut BTreeSet<FormatterVersion>,
    warnings: &mut Vec<GenWarning>,
) -> Result<CValue, GenError> {
    reject_embedded_bytes(payload)?;
    let CValue::Obj(o) = payload else {
        return Err(GenError::StoreInvalid("text payload not object"));
    };
    // Starter template heading/text shorthand: {"text": "rpt.starter.*"}
    if o.get("schema").is_none() {
        if let Some(CValue::Str(key)) = o.get("text") {
            let display = starter_lookup_label(key).map(str::to_string).unwrap_or_else(|| key.clone());
            return Ok(obj([
                ("key", CValue::Str(key.clone())),
                ("schema", CValue::Str("resolved_text@1".to_string())),
                ("text", CValue::Str(display)),
            ]));
        }
    }
    // Full `text_block@1` / `header_block@1` / `footer_block@1` payload
    let run_kind = match o.get("schema") {
        Some(CValue::Str(s)) if s == crate::blocks::text::HEADER_SCHEMA => RunBlockKind::Header,
        Some(CValue::Str(s)) if s == crate::blocks::text::FOOTER_SCHEMA => RunBlockKind::Footer,
        _ => RunBlockKind::Text,
    };
    let tp = RunBlockPayload::from_value(run_kind, payload)
        .map_err(|e| GenError::Report(ReportError::InvalidBlockPayload(e)))?;
    let empty_records = BTreeMap::new();
    let runs = if run_kind == RunBlockKind::Text {
        resolve_text_runs(&tp, &empty_records, fmt_ctx)
            .map_err(|e| GenError::Report(ReportError::InvalidBlockPayload(PayloadError::new("/runs", e.code()))))?
    } else if let Some(rid) = report_id {
        resolve_chrome_runs(&tp, rid, Digest::from_bytes([0u8; 32]))
            .map_err(|e| GenError::Report(ReportError::InvalidBlockPayload(PayloadError::new("/runs", e.code()))))?
    } else {
        Vec::new()
    };
    let runs_val: Vec<CValue> = runs
        .iter()
        .map(|r| match r {
            ResolvedRun::Literal(u) => obj([
                ("kind", CValue::Str("literal".to_string())),
                ("text", CValue::Str(u.untrusted_value().as_str().to_string())),
            ]),
            ResolvedRun::Value(value) => obj([
                ("kind", CValue::Str("value".to_string())),
                ("value", value_text_to_value(value, block_id, formatters, warnings)),
            ]),
            ResolvedRun::Decision(outcome) => {
                obj([("kind", CValue::Str("outcome".to_string())), ("outcome", outcome_text_to_value(outcome))])
            }
            ResolvedRun::Uncertainty(u) => {
                cell_text_to_value(&CellText::Uncertainty(u.clone()), block_id, formatters, warnings)
            }
            ResolvedRun::CharacteristicName(name) => obj([
                ("kind", CValue::Str("characteristic_name".to_string())),
                ("text", CValue::Str(name.untrusted_value().as_str().to_string())),
            ]),
            ResolvedRun::Missing(msg) => {
                obj([("kind", CValue::Str("missing".to_string())), ("text", CValue::Str((*msg).to_string()))])
            }
            ResolvedRun::PagePlaceholder(pf) => obj([
                ("field", CValue::Str(pf.as_str().to_string())),
                ("kind", CValue::Str("page_placeholder".to_string())),
            ]),
            ResolvedRun::ReportId(rid) => obj([
                ("field", CValue::Str("report_id".to_string())),
                ("kind", CValue::Str("page_text".to_string())),
                ("text", CValue::Str(rid.uuid().to_string())),
            ]),
            ResolvedRun::ResolvedReportDigest(d) => obj([
                ("field", CValue::Str("resolved_report_digest".to_string())),
                ("kind", CValue::Str("page_text".to_string())),
                ("text", CValue::Str(d.as_str().to_string())),
            ]),
        })
        .collect();
    Ok(obj([("runs", CValue::Arr(runs_val)), ("schema", CValue::Str("resolved_text@1".to_string()))]))
}

fn resolve_image_payload(payload: &CValue) -> Result<CValue, GenError> {
    reject_embedded_bytes(payload)?;
    let img = ImagePayload::from_value(payload).map_err(|e| GenError::Report(ReportError::InvalidBlockPayload(e)))?;
    Ok(obj([
        (
            "caption",
            img.caption.as_ref().map_or(CValue::Null, |c| CValue::Str(c.untrusted_value().as_str().to_string())),
        ),
        ("cas_digest", CValue::Str(img.digest.as_str().to_string())),
        ("height_px", CValue::Str(img.height_px.to_string())),
        ("media_type", CValue::Str(img.mime.as_str().to_string())),
        ("schema", CValue::Str("resolved_image@1".to_string())),
        ("width_px", CValue::Str(img.width_px.to_string())),
    ]))
}

// ------------------------------------------------------------------ core generate()

/// Generates a deterministic [`ResolvedReport`] from a [`GenSource`] at `ctx.head`
/// (`REQ-RPT.MODEL.BIND.GEN-001`..`008`).
///
/// Pure and read-only: writes nothing to the Record Journal (`REQ-004`).
pub fn generate(
    source: GenSource<'_>,
    ctx: &GenContext<'_>,
    model: &dyn ProjectModel,
    journal: &dyn JournalReads,
    kinds: &KindRegistry,
    resolvers: &ResolverRegistry,
) -> Result<ResolvedReport, GenError> {
    let seq = journal.head_seq(&ctx.head).ok_or_else(|| GenError::NotFound(ctx.head.digest().as_str().to_string()))?;
    let lookup = HeadLookup::new(journal, ctx.project, &ctx.head)?;
    let rule_catalog = RuleCatalog::builtin().map_err(|_| GenError::StoreInvalid("rule catalog"))?;
    let fmt_ctx = FmtContext {
        locale: ctx.locale.clone(),
        units: ctx.units.map_or(UnitOverride::NONE, UnitOverrideMode::to_unit_override),
        default_decimals: ctx.default_decimals.clone(),
        catalog: ctx.catalog,
    };
    let resolve_ctx = ResolveContext {
        project: ctx.project,
        head: ctx.head,
        stages: ctx.stages.clone(),
        runs: ctx.runs.clone(),
        sources: ctx.sources.clone(),
    };

    let mut formatters: BTreeSet<FormatterVersion> = BTreeSet::new();
    let mut warnings: Vec<GenWarning> = Vec::new();
    let mut observed_kernel_version: Option<String> = None;

    let (template_ref, pages) = match source {
        GenSource::Template { template, template_ref } => {
            let tbytes =
                serialise_template(template).map_err(|_| GenError::StoreInvalid("template canonical"))?.into_vec();
            let actual = Digest::from_bytes(digest_bytes(&tbytes).0);
            if actual != template_ref.digest {
                return Err(GenError::StoreInvalid("template digest mismatch"));
            }
            let pages = generate_from_template(
                template,
                None,
                &resolve_ctx,
                &fmt_ctx,
                ctx,
                model,
                journal,
                seq,
                &lookup,
                &rule_catalog,
                kinds,
                resolvers,
                &mut formatters,
                &mut warnings,
                &mut observed_kernel_version,
            )?;
            (Some(template_ref), pages)
        }
        GenSource::Report { report, template } => {
            if let Some(tref) = report.template_ref() {
                if let Some(tmpl) = template {
                    let tbytes =
                        serialise_template(tmpl).map_err(|_| GenError::StoreInvalid("template canonical"))?.into_vec();
                    let actual = Digest::from_bytes(digest_bytes(&tbytes).0);
                    if actual != tref.digest {
                        return Err(GenError::StoreInvalid("template digest mismatch"));
                    }
                    let pages = generate_from_template(
                        tmpl,
                        Some(report.id()),
                        &resolve_ctx,
                        &fmt_ctx,
                        ctx,
                        model,
                        journal,
                        seq,
                        &lookup,
                        &rule_catalog,
                        kinds,
                        resolvers,
                        &mut formatters,
                        &mut warnings,
                        &mut observed_kernel_version,
                    )?;
                    (Some(tref), pages)
                } else {
                    let pages = generate_from_report_pages(
                        report,
                        &resolve_ctx,
                        &fmt_ctx,
                        ctx,
                        journal,
                        seq,
                        &lookup,
                        &rule_catalog,
                        kinds,
                        resolvers,
                        &mut formatters,
                        &mut warnings,
                        &mut observed_kernel_version,
                    )?;
                    (Some(tref), pages)
                }
            } else {
                let pages = generate_from_report_pages(
                    report,
                    &resolve_ctx,
                    &fmt_ctx,
                    ctx,
                    journal,
                    seq,
                    &lookup,
                    &rule_catalog,
                    kinds,
                    resolvers,
                    &mut formatters,
                    &mut warnings,
                    &mut observed_kernel_version,
                )?;
                (None, pages)
            }
        }
    };

    let kernel_version = if !ctx.kernel_version.is_empty() {
        ctx.kernel_version.clone()
    } else {
        observed_kernel_version.unwrap_or_else(|| DEFAULT_KERNEL_VERSION.to_string())
    };
    let provenance = Provenance {
        head: ctx.head,
        template: template_ref,
        kernel_version,
        formatters: formatters.into_iter().collect(),
        locale: ctx.locale.id().to_string(),
        locale_release: ctx.locale.release().to_string(),
        units: ctx.units,
    };
    ResolvedReport::finalize(provenance, pages, warnings)
}

#[allow(clippy::too_many_arguments)]
fn generate_from_report_pages(
    report: &Report,
    resolve_ctx: &ResolveContext<'_>,
    fmt_ctx: &FmtContext<'_>,
    ctx: &GenContext<'_>,
    journal: &dyn JournalReads,
    seq: u64,
    lookup: &HeadLookup<'_>,
    rule_catalog: &RuleCatalog,
    kinds: &KindRegistry,
    resolvers: &ResolverRegistry,
    formatters: &mut BTreeSet<FormatterVersion>,
    warnings: &mut Vec<GenWarning>,
    observed_kernel_version: &mut Option<String>,
) -> Result<Vec<ResolvedPage>, GenError> {
    let mut out_pages = Vec::with_capacity(report.pages().len());
    for page in report.pages() {
        let mut out_blocks = Vec::with_capacity(page.blocks().len());
        for block in page.blocks() {
            let resolved_payload = resolve_authored_block(
                block,
                Some(report.id()),
                resolve_ctx,
                fmt_ctx,
                ctx,
                journal,
                seq,
                lookup,
                rule_catalog,
                kinds,
                resolvers,
                formatters,
                warnings,
                observed_kernel_version,
            )?;
            out_blocks.push(GeneratedBlock {
                id: block.id().uuid(),
                kind: block.kind().clone(),
                kind_version: block.kind_version(),
                frame: None,
                payload: resolved_payload,
            });
        }
        out_pages.push(ResolvedPage { id: page.id().uuid(), blocks: out_blocks });
    }
    Ok(out_pages)
}

#[allow(clippy::too_many_arguments)]
fn resolve_authored_block(
    block: &Block,
    report_id: Option<ReportId>,
    resolve_ctx: &ResolveContext<'_>,
    fmt_ctx: &FmtContext<'_>,
    ctx: &GenContext<'_>,
    journal: &dyn JournalReads,
    seq: u64,
    lookup: &HeadLookup<'_>,
    rule_catalog: &RuleCatalog,
    kinds: &KindRegistry,
    resolvers: &ResolverRegistry,
    formatters: &mut BTreeSet<FormatterVersion>,
    warnings: &mut Vec<GenWarning>,
    observed_kernel_version: &mut Option<String>,
) -> Result<CValue, GenError> {
    reject_embedded_bytes(block.payload().value())?;
    match block.kind().as_str() {
        VIEW_KIND_ID => {
            // Also run registered resolver if present (verifies binding at head).
            if resolvers.get(block.kind()).is_ok() && kinds.get(block.kind()).is_some() {
                let _ = resolve_block(block, resolve_ctx, kinds, resolvers, journal)?;
            }
            resolve_view_block(
                block.id().uuid(),
                block.id(),
                block.payload().value(),
                seq,
                journal,
                lookup,
                ctx.metadata,
                warnings,
            )
        }
        "image" => {
            if resolvers.get(block.kind()).is_ok() && kinds.get(block.kind()).is_some() {
                let _ = resolve_block(block, resolve_ctx, kinds, resolvers, journal)?;
            }
            resolve_image_payload(block.payload().value())
        }
        "text" | "header" | "footer" => {
            if resolvers.get(block.kind()).is_ok() && kinds.get(block.kind()).is_some() {
                let _ = resolve_block(block, resolve_ctx, kinds, resolvers, journal)?;
            }
            resolve_starter_or_text_payload(
                block.id().uuid(),
                block.payload().value(),
                fmt_ctx,
                ctx.metadata,
                report_id,
                formatters,
                warnings,
            )
        }
        "table" => {
            let rb = resolve_block(block, resolve_ctx, kinds, resolvers, journal)?;
            if let Ok(tp) = TablePayload::from_value(block.payload().value()) {
                let mut rows_out = Vec::new();
                for r in rb.view().refs() {
                    if let super::resolve::ResolvedRef::Result { id: rid, .. } = r {
                        let lr = load_result(*rid, seq, journal, lookup, rule_catalog, block.id())?;
                        if observed_kernel_version.is_none() {
                            *observed_kernel_version = lr.kernel_version.clone();
                        }
                        let (vk, du) = value_kind_and_default_unit(lr.unit);
                        let ch = Characteristic {
                            id: rid.to_string(),
                            kind: Some(vk),
                            unit: du,
                            decimals: None,
                            limits: None,
                            nominal: None,
                        };
                        let vr = ValueRecord {
                            id: lr.id.to_string(),
                            value: lr.value,
                            basis: lr.basis,
                            decision: &lr.decision,
                            statement: &lr.statement,
                            acceptance: None,
                            transcription: if lr.basis == fid_gdt::chars::record::EvaluationBasis::ResultsOnly {
                                ctx.metadata.transcription(&lr.id)
                            } else {
                                None
                            },
                            gauge: if lr.basis == fid_gdt::chars::record::EvaluationBasis::DirectReading {
                                ctx.metadata.gauge(&lr.id)
                            } else {
                                None
                            },
                            record_digest: lr.digest,
                            interpretation: lr.interpretation,
                        };
                        let tri = TableRowInput {
                            characteristic_id: &ch.id,
                            characteristic_name: &ch.id,
                            characteristic_type: "",
                            characteristic: &ch,
                            tags: &[],
                            stage: None,
                            run: None,
                            result: Some(&vr),
                        };
                        let tc = resolve_table_cells(&tp, &[tri], fmt_ctx).map_err(|e| match e {
                            crate::blocks::table::TableError::Format(fe) => GenError::Format(fe),
                            crate::blocks::table::TableError::Report(re) => GenError::Report(re),
                            _ => GenError::StoreInvalid("table resolve"),
                        })?;
                        for row_cells in tc.rows {
                            rows_out.push(CValue::Arr(
                                row_cells
                                    .iter()
                                    .map(|c| cell_text_to_value(c, block.id().uuid(), formatters, warnings))
                                    .collect(),
                            ));
                        }
                    }
                }
                let header: Vec<CValue> =
                    tp.columns.iter().map(|c| CValue::Str(c.effective_header().to_string())).collect();
                Ok(obj([
                    ("header", CValue::Arr(header)),
                    ("rows", CValue::Arr(rows_out)),
                    ("schema", CValue::Str("resolved_table@1".to_string())),
                ]))
            } else {
                Ok(rb.layout().clone())
            }
        }
        _ => {
            let rb = resolve_block(block, resolve_ctx, kinds, resolvers, journal)?;
            reject_embedded_bytes(rb.layout())?;
            Ok(rb.layout().clone())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn generate_from_template(
    template: &Template,
    report_id: Option<ReportId>,
    resolve_ctx: &ResolveContext<'_>,
    fmt_ctx: &FmtContext<'_>,
    ctx: &GenContext<'_>,
    model: &dyn ProjectModel,
    journal: &dyn JournalReads,
    seq: u64,
    lookup: &HeadLookup<'_>,
    rule_catalog: &RuleCatalog,
    kinds: &KindRegistry,
    resolvers: &ResolverRegistry,
    formatters: &mut BTreeSet<FormatterVersion>,
    warnings: &mut Vec<GenWarning>,
    observed_kernel_version: &mut Option<String>,
) -> Result<Vec<ResolvedPage>, GenError> {
    let expanded = expand(template, resolve_ctx, model, journal, ctx.query_provider)?;
    // Map template block ID -> (kind, kind_version, payload value) for fast O(log N) lookup.
    let mut tmpl_blocks: BTreeMap<BlockId, (&BlockKindId, u32, &CValue)> = BTreeMap::new();
    for p in &template.pages {
        for b in &p.blocks {
            tmpl_blocks.insert(b.id, (&b.kind, b.kind_version, b.payload.value()));
        }
    }
    // Index ProjectModel characteristics for fast O(log N) lookup on 500x3 performance fixtures.
    let char_items: BTreeMap<Uuid, CharacteristicItem> =
        model.characteristics(&ctx.head)?.into_iter().map(|c| (c.id, c)).collect();

    // Group expanded blocks by expanded page UUID preserving first-seen page order.
    let mut page_order: Vec<Uuid> = Vec::new();
    let mut page_buckets: BTreeMap<Uuid, Vec<usize>> = BTreeMap::new();
    for (idx, eb) in expanded.iter().enumerate() {
        let bucket = page_buckets.entry(eb.page).or_insert_with(|| {
            page_order.push(eb.page);
            Vec::new()
        });
        bucket.push(idx);
    }

    let mut pages = Vec::with_capacity(page_order.len());
    for pid in page_order {
        let indices = page_buckets.get(&pid).map(Vec::as_slice).unwrap_or(&[]);
        let mut out_blocks = Vec::new();
        let mut pos = 0usize;
        while pos < indices.len() {
            let eb = &expanded[indices[pos]];
            let (kind, kind_version, src_payload) = tmpl_blocks
                .get(&eb.source_block)
                .copied()
                .ok_or(GenError::StoreInvalid("expanded source block missing"))?;
            match eb.role {
                Role::Copy => {
                    let payload = match kind.as_str() {
                        "text" | "header" | "footer" => resolve_starter_or_text_payload(
                            eb.id,
                            src_payload,
                            fmt_ctx,
                            ctx.metadata,
                            report_id,
                            formatters,
                            warnings,
                        )?,
                        "image" => resolve_image_payload(src_payload)?,
                        VIEW_KIND_ID => resolve_view_block(
                            eb.id,
                            eb.source_block,
                            src_payload,
                            seq,
                            journal,
                            lookup,
                            ctx.metadata,
                            warnings,
                        )?,
                        _ => {
                            if resolvers.get(kind).is_ok() && kinds.get(kind).is_some() {
                                let cj = CanonicalJson::new(src_payload)
                                    .ok_or(GenError::StoreInvalid("template block payload canonical"))?;
                                let tmp_block = Block::new(eb.source_block, kind.clone(), kind_version, cj);
                                let rb = resolve_block(&tmp_block, resolve_ctx, kinds, resolvers, journal)?;
                                rb.layout().clone()
                            } else {
                                src_payload.clone()
                            }
                        }
                    };
                    out_blocks.push(GeneratedBlock {
                        id: eb.id,
                        kind: kind.clone(),
                        kind_version,
                        frame: None,
                        payload,
                    });
                    pos += 1;
                }
                Role::ColumnGroup => {
                    let cols: Vec<CValue> = eb
                        .columns
                        .iter()
                        .map(|c| {
                            obj([("id", CValue::Str(c.id.clone())), ("kind", CValue::Str(c.kind.as_str().to_string()))])
                        })
                        .collect();
                    let payload = obj([
                        ("columns", CValue::Arr(cols)),
                        ("schema", CValue::Str("resolved_column_group@1".to_string())),
                    ]);
                    out_blocks.push(GeneratedBlock {
                        id: eb.id,
                        kind: kind.clone(),
                        kind_version,
                        frame: None,
                        payload,
                    });
                    pos += 1;
                }
                Role::Table => {
                    // Collect all immediately following Role::Row entries belonging to this source_block.
                    let table_block_id = eb.id;
                    let source_block = eb.source_block;
                    let stage_cols = &eb.columns;
                    let mut row_indices = Vec::new();
                    pos += 1;
                    while pos < indices.len() {
                        let next_eb = &expanded[indices[pos]];
                        if next_eb.role == Role::Row && next_eb.source_block == eb.source_block {
                            row_indices.push(indices[pos]);
                            pos += 1;
                        } else {
                            break;
                        }
                    }
                    let table_payload = resolve_expanded_table(
                        table_block_id,
                        source_block,
                        src_payload,
                        stage_cols,
                        &row_indices,
                        &expanded,
                        &char_items,
                        fmt_ctx,
                        ctx,
                        journal,
                        seq,
                        lookup,
                        rule_catalog,
                        formatters,
                        warnings,
                        observed_kernel_version,
                    )?;
                    out_blocks.push(GeneratedBlock {
                        id: table_block_id,
                        kind: kind.clone(),
                        kind_version,
                        frame: None,
                        payload: table_payload,
                    });
                }
                Role::Row => {
                    // Orphan row (should not occur after Role::Table); skip cleanly.
                    pos += 1;
                }
            }
        }
        pages.push(ResolvedPage { id: pid, blocks: out_blocks });
    }
    Ok(pages)
}

#[allow(clippy::too_many_arguments)]
fn resolve_expanded_table(
    table_block_id: Uuid,
    source_block: BlockId,
    src_payload: &CValue,
    stage_cols: &[super::expand::ExpandItem],
    row_indices: &[usize],
    expanded: &[ExpandedBlock],
    char_items: &BTreeMap<Uuid, CharacteristicItem>,
    fmt_ctx: &FmtContext<'_>,
    ctx: &GenContext<'_>,
    journal: &dyn JournalReads,
    seq: u64,
    lookup: &HeadLookup<'_>,
    rule_catalog: &RuleCatalog,
    formatters: &mut BTreeSet<FormatterVersion>,
    warnings: &mut Vec<GenWarning>,
    observed_kernel_version: &mut Option<String>,
) -> Result<CValue, GenError> {
    // Extract column descriptors from template payload if present (starter templates declare `columns`).
    let header_cols: Vec<CValue> = match src_payload {
        CValue::Obj(o) => match o.get("columns") {
            Some(CValue::Arr(cols)) => cols
                .iter()
                .filter_map(|c| match c {
                    CValue::Obj(co) => {
                        let key = match co.get("key").or_else(|| co.get("source")) {
                            Some(CValue::Str(s)) => s.clone(),
                            _ => return None,
                        };
                        let title = match co.get("title_key") {
                            Some(CValue::Str(tk)) => {
                                starter_lookup_label(tk).map(str::to_string).unwrap_or_else(|| tk.clone())
                            }
                            _ => match co.get("header") {
                                Some(CValue::Str(h)) => h.clone(),
                                _ => key.clone(),
                            },
                        };
                        Some(obj([("key", CValue::Str(key)), ("title", CValue::Str(title))]))
                    }
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };

    let stage_ids: Vec<CValue> = stage_cols.iter().map(|s| CValue::Str(s.id.clone())).collect();
    let mut rows_out = Vec::with_capacity(row_indices.len());

    for &ridx in row_indices {
        let reb = &expanded[ridx];
        let Some(item) = &reb.item else {
            continue;
        };
        if item.kind != ItemKind::Characteristic {
            rows_out.push(obj([
                ("item_id", CValue::Str(item.id.clone())),
                ("item_kind", CValue::Str(item.kind.as_str().to_string())),
                ("row_id", CValue::Str(reb.id.to_string())),
            ]));
            continue;
        }
        let cid = parse_canonical_uuid(&item.id).ok_or(GenError::StoreInvalid("row characteristic id"))?;
        let parsed_ch = load_characteristic(cid, seq, journal, lookup, ctx.metadata, source_block)?;
        let item_meta = char_items.get(&cid);
        let ch_name = item_meta.map_or(parsed_ch.name.as_str(), |m| m.name.as_str());
        let nominal_val = match parsed_ch.spec.nominal {
            Some(_) => {
                let vt = rpt_fmt::fmt_requirement(&parsed_ch.spec, fmt_ctx)?;
                value_text_to_value(&vt, table_block_id, formatters, warnings)
            }
            None => CValue::Null,
        };

        let mut stage_cells = Vec::with_capacity(reb.cells.len());
        for (col_idx, cell_rid) in reb.cells.iter().enumerate() {
            let stage_id = stage_cols.get(col_idx).map(|s| s.id.as_str()).unwrap_or("");
            let cell_val = match cell_rid {
                None => obj([
                    ("decision", CValue::Null),
                    ("deviation", CValue::Null),
                    ("measured", CValue::Null),
                    ("record_id", CValue::Null),
                    ("stage_id", CValue::Str(stage_id.to_string())),
                    ("status", CValue::Str(crate::blocks::table::MISSING_TEXT.to_string())),
                ]),
                Some(rid) => {
                    let lr = load_result(*rid, seq, journal, lookup, rule_catalog, source_block)?;
                    if observed_kernel_version.is_none() {
                        *observed_kernel_version = lr.kernel_version.clone();
                    }
                    let vr = ValueRecord {
                        id: lr.id.to_string(),
                        value: lr.value,
                        basis: lr.basis,
                        decision: &lr.decision,
                        statement: &lr.statement,
                        acceptance: None,
                        transcription: if lr.basis == fid_gdt::chars::record::EvaluationBasis::ResultsOnly {
                            ctx.metadata.transcription(&lr.id)
                        } else {
                            None
                        },
                        gauge: if lr.basis == fid_gdt::chars::record::EvaluationBasis::DirectReading {
                            ctx.metadata.gauge(&lr.id)
                        } else {
                            None
                        },
                        record_digest: lr.digest,
                        interpretation: lr.interpretation,
                    };
                    let measured_vt = rpt_fmt::fmt_value(&vr, &parsed_ch.spec, fmt_ctx)?;
                    let measured_json = value_text_to_value(&measured_vt, table_block_id, formatters, warnings);
                    let dev_json = if parsed_ch.spec.nominal.is_some() {
                        let dvt = rpt_fmt::fmt_deviation(&vr, &parsed_ch.spec, fmt_ctx)?;
                        value_text_to_value(&dvt, table_block_id, formatters, warnings)
                    } else {
                        CValue::Null
                    };
                    let dec_ot = rpt_fmt::fmt_outcome(&vr, fmt_ctx.catalog)?;
                    let dec_json = outcome_text_to_value(&dec_ot);
                    let unc = rpt_fmt::fmt_uncertainty(&vr, &parsed_ch.spec, fmt_ctx)?;
                    let unc_json =
                        cell_text_to_value(&CellText::Uncertainty(unc), table_block_id, formatters, warnings);
                    obj([
                        ("decision", dec_json),
                        ("deviation", dev_json),
                        ("measured", measured_json),
                        ("record_digest", CValue::Str(lr.digest.as_str().to_string())),
                        ("record_id", CValue::Str(lr.id.to_string())),
                        ("stage_id", CValue::Str(stage_id.to_string())),
                        ("uncertainty", unc_json),
                    ])
                }
            };
            stage_cells.push(cell_val);
        }

        rows_out.push(obj([
            ("cells", CValue::Arr(stage_cells)),
            ("characteristic_id", CValue::Str(cid.to_string())),
            ("name", CValue::Str(ch_name.to_string())),
            ("nominal", nominal_val),
            ("row_id", CValue::Str(reb.id.to_string())),
        ]));
    }

    Ok(obj([
        ("columns", CValue::Arr(header_cols)),
        ("rows", CValue::Arr(rows_out)),
        ("schema", CValue::Str("resolved_table@1".to_string())),
        ("stages", CValue::Arr(stage_ids)),
    ]))
}

// ------------------------------------------------------------------ scratch & CAS stores

/// Scratch store holding generated [`ResolvedReport`]s keyed by content digest (`REQ-004`).
pub trait ScratchStore {
    fn put(&mut self, resolved: &ResolvedReport) -> Result<(), GenError>;
    fn get(&self, digest: &Digest) -> Result<Option<ResolvedReport>, GenError>;
}

/// In-memory scratch store for R1 `report.generate_report@1` outputs.
#[derive(Debug, Clone, Default)]
pub struct MemoryScratch {
    entries: BTreeMap<Digest, Vec<u8>>,
}

impl MemoryScratch {
    pub fn new() -> MemoryScratch {
        MemoryScratch::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl ScratchStore for MemoryScratch {
    fn put(&mut self, resolved: &ResolvedReport) -> Result<(), GenError> {
        let bytes = resolved.to_content_canonical_bytes()?;
        let digest = Digest::from_bytes(digest_bytes(&bytes).0);
        if digest != resolved.digest {
            return Err(GenError::StoreInvalid("scratch digest mismatch"));
        }
        if !self.entries.contains_key(&digest) && self.entries.len() >= MAX_SCRATCH_ENTRIES {
            return Err(GenError::LimitExceeded("scratch_full"));
        }
        self.entries.insert(digest, bytes);
        Ok(())
    }

    fn get(&self, digest: &Digest) -> Result<Option<ResolvedReport>, GenError> {
        let Some(bytes) = self.entries.get(digest) else {
            return Ok(None);
        };
        if Digest::from_bytes(digest_bytes(bytes).0) != *digest {
            return Err(GenError::StoreInvalid("scratch corrupted"));
        }
        Ok(Some(ResolvedReport::from_cas_bytes(bytes)?))
    }
}

/// One W2 journal event emitted by `report.save_report@1` (`REQ-005`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveReportEvent {
    pub project: Uuid,
    pub report: ReportId,
    pub resolved_digest: Digest,
    pub head: JournalHead,
}

impl SaveReportEvent {
    pub fn to_value(&self) -> CValue {
        obj([
            ("head", CValue::Str(self.head.digest().as_str().to_string())),
            ("project", CValue::Str(self.project.to_string())),
            ("report", CValue::Str(self.report.uuid().to_string())),
            ("resolved_digest", CValue::Str(self.resolved_digest.as_str().to_string())),
            ("type", CValue::Str(SAVE_REPORT.to_string())),
        ])
    }

    pub fn from_value(v: &CValue) -> Result<SaveReportEvent, GenError> {
        let o = exact_obj(v, &["head", "project", "report", "resolved_digest", "type"])?;
        if req_str(o, "type")? != SAVE_REPORT {
            return Err(GenError::StoreInvalid("not save_report event"));
        }
        let project =
            parse_canonical_uuid(req_str(o, "project")?).ok_or(GenError::StoreInvalid("save_report project"))?;
        let report_u = parse_canonical_uuid(req_str(o, "report")?)
            .and_then(ReportId::new)
            .ok_or(GenError::StoreInvalid("save_report report"))?;
        let resolved_digest = Digest::parse(req_str(o, "resolved_digest")?)
            .map_err(|_| GenError::StoreInvalid("save_report resolved_digest"))?;
        let head = JournalHead::parse(req_str(o, "head")?).ok_or(GenError::StoreInvalid("save_report head"))?;
        Ok(SaveReportEvent { project, report: report_u, resolved_digest, head })
    }
}

/// Receipt returned by `report.save_report@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveReceipt {
    pub report: ReportId,
    pub resolved_digest: Digest,
    pub bytes: u64,
}

impl SaveReceipt {
    pub fn to_value(&self) -> CValue {
        obj([
            ("bytes", CValue::Str(self.bytes.to_string())),
            ("report", CValue::Str(self.report.uuid().to_string())),
            ("resolved_digest", CValue::Str(self.resolved_digest.as_str().to_string())),
        ])
    }
}

/// Content-addressed storage, report link index and W2 journal seam for `report.save_report@1`.
pub trait ResolvedStore: ObjectRead {
    /// Stores `canonical_content_bytes` (whose SHA-256 MUST equal `event.resolved_digest`) in the
    /// CAS, links `event.resolved_digest` to `event.report`, and appends `event` atomically.
    fn commit_saved(&mut self, canonical_content_bytes: Vec<u8>, event: SaveReportEvent) -> Result<(), GenError>;
}

/// In-memory CAS + report link + W2 save journal for tests and CLI execution.
#[derive(Debug, Clone, Default)]
pub struct MemoryResolvedStore {
    objects: BTreeMap<Digest, Vec<u8>>,
    links: BTreeMap<ReportId, Vec<Digest>>,
    journal: Vec<SaveReportEvent>,
}

impl MemoryResolvedStore {
    pub fn new() -> MemoryResolvedStore {
        MemoryResolvedStore::default()
    }

    pub fn cas_bytes(&self, digest: &Digest) -> Option<&[u8]> {
        self.objects.get(digest).map(Vec::as_slice)
    }

    pub fn linked(&self, report: &ReportId) -> &[Digest] {
        self.links.get(report).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn journal(&self) -> &[SaveReportEvent] {
        &self.journal
    }

    pub fn journal_values(&self) -> Vec<CValue> {
        self.journal.iter().map(SaveReportEvent::to_value).collect()
    }
}

impl ObjectRead for MemoryResolvedStore {
    fn object(&self, digest: &Digest) -> Result<Option<Vec<u8>>, ReportError> {
        Ok(self.objects.get(digest).cloned())
    }
}

impl ResolvedStore for MemoryResolvedStore {
    fn commit_saved(&mut self, canonical_content_bytes: Vec<u8>, event: SaveReportEvent) -> Result<(), GenError> {
        let actual = Digest::from_bytes(digest_bytes(&canonical_content_bytes).0);
        if actual != event.resolved_digest {
            return Err(GenError::StoreInvalid("cas digest mismatch"));
        }
        self.objects.entry(actual).or_insert(canonical_content_bytes);
        self.links.entry(event.report).or_default().push(actual);
        self.journal.push(event);
        Ok(())
    }
}

// ------------------------------------------------------------------ commands

/// Parameters of `report.generate_report@1` (R1, D0).
#[derive(FidSchema, Debug, Clone, PartialEq, Eq, Default)]
pub struct GenerateReportParams {
    #[fid(description = "Optional authored report ID (UUIDv7).", pattern = ID_PATTERN)]
    pub report: Option<String>,
    #[fid(description = "Optional template ID (UUIDv7) or built-in starter name (e.g. tmpl:multistage-table).")]
    pub template: Option<String>,
    #[fid(description = "Optional explicit journal head digest (sha256:...); defaults to current head.")]
    pub head: Option<String>,
    #[fid(description = "Optional BCP 47 locale ID (defaults to 'en').")]
    pub locale: Option<String>,
    #[fid(description = "Optional unit override ('authored' or 'si').")]
    pub units: Option<String>,
    #[fid(description = "Optional ordered list of stage IDs (UUIDs).")]
    pub stages: Vec<String>,
    #[fid(description = "Optional ordered list of run IDs (UUIDs).")]
    pub runs: Vec<String>,
}

/// Parameters of `report.save_report@1` (W2, D0).
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct SaveReportParams {
    #[fid(description = "Target report ID (UUIDv7).", pattern = ID_PATTERN)]
    pub report: String,
    #[fid(description = "Content digest (sha256:...) of the resolved report in scratch storage.")]
    pub resolved: String,
}

/// Wire output of `report.generate_report@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct GenerateReportOutput {
    #[fid(description = "SHA-256 content digest of the generated ResolvedReport.")]
    pub digest: String,
    #[fid(description = "Explicit journal head hash recorded in provenance.")]
    pub head: String,
    #[fid(description = "Locale ID recorded in provenance.")]
    pub locale: String,
    #[fid(description = "Locale data release recorded in provenance.")]
    pub locale_release: String,
    #[fid(description = "Number of pages in the resolved document.")]
    pub page_count: u32,
    #[fid(description = "Number of non-fatal warnings in the resolved document.")]
    pub warning_count: u32,
}

/// Wire output of `report.save_report@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct SaveReportOutput {
    #[fid(description = "Report ID the resolved document was linked to.")]
    pub report: String,
    #[fid(description = "SHA-256 content digest stored in the CAS and journaled.")]
    pub resolved_digest: String,
    #[fid(description = "Canonical decimal byte length of the resolved document stored in the CAS.")]
    pub bytes: String,
}

#[fid_command(
    id = "report.generate_report",
    major = 1,
    title = "Generate resolved report",
    description = "Expands and resolves a report or template at an explicit journal head into scratch storage without writing to the Record Journal.",
    side_effect = R1,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = None,
    fs_effect = None,
    undo = NotUndoable,
    preconditions = [],
    effects = [],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-22"),
    web = Available,
)]
fn generate_report_decl(
    env: &mut GenerateCmdEnv<'_>,
    params: GenerateReportParams,
) -> Result<GenerateReportOutput, GenError> {
    let resolved = generate_report(&params, env)?;
    Ok(GenerateReportOutput {
        digest: resolved.digest.as_str().to_string(),
        head: resolved.provenance.head.digest().as_str().to_string(),
        locale: resolved.provenance.locale,
        locale_release: resolved.provenance.locale_release,
        page_count: u32::try_from(resolved.pages.len()).unwrap_or(u32::MAX),
        warning_count: u32::try_from(resolved.warnings.len()).unwrap_or(u32::MAX),
    })
}

#[fid_command(
    id = "report.save_report",
    major = 1,
    title = "Save resolved report",
    description = "Stores a scratch resolved report in the content-addressed store, links it to the report and journals one W2 event.",
    side_effect = W2,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = None,
    fs_effect = None,
    undo = HeadMove,
    preconditions = [],
    effects = [SAVE_REPORT],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-22"),
    web = Available,
)]
fn save_report_decl(env: &mut SaveCmdEnv<'_>, params: SaveReportParams) -> Result<SaveReportOutput, GenError> {
    let receipt = save_report(&params, env)?;
    Ok(SaveReportOutput {
        report: receipt.report.uuid().to_string(),
        resolved_digest: receipt.resolved_digest.as_str().to_string(),
        bytes: receipt.bytes.to_string(),
    })
}

fid_commands![generate_report_decl, save_report_decl];

/// Registry definitions for `report.generate_report@1` and `report.save_report@1`.
pub fn gen_command_defs() -> Result<Vec<CommandDef>, DefError> {
    Ok(declarations().into_iter().map(|d| d.def).collect())
}

/// Environment for executing `report.generate_report@1` (R1, never mutates the Record Journal).
pub struct GenerateCmdEnv<'a> {
    pub backend: &'a dyn ReportBackend,
    pub builtins: &'a BuiltinRegistry,
    pub kinds: &'a KindRegistry,
    pub resolvers: &'a ResolverRegistry,
    pub model: &'a dyn ProjectModel,
    pub journal: &'a dyn JournalReads,
    pub catalog: &'a Catalog,
    pub default_decimals: DefaultDecimals,
    pub kernel_version: String,
    pub metadata: &'a dyn GenMetadata,
    pub query_provider: Option<&'a dyn QueryProvider>,
    pub field_sources: Vec<&'a dyn FieldSource>,
    pub custom_locales: BTreeMap<String, LocaleRules>,
    pub scratch: &'a mut dyn ScratchStore,
}

/// Executes `report.generate_report@1` (R1): expands and resolves into scratch storage keyed
/// by content digest, writing nothing to the Record Journal (`REQ-004`).
pub fn generate_report(
    params: &GenerateReportParams,
    env: &mut GenerateCmdEnv<'_>,
) -> Result<ResolvedReport, GenError> {
    let head = match &params.head {
        Some(h) => JournalHead::parse(h).ok_or(GenError::BadParams("invalid_head"))?,
        None => env.journal.current_head().ok_or_else(|| GenError::NotFound("current_head".to_string()))?,
    };
    let locale_id = params.locale.as_deref().unwrap_or("en");
    let locale = match env.custom_locales.get(locale_id) {
        Some(r) => r.clone(),
        None => default_locale_rules(locale_id)?,
    };
    let units = match &params.units {
        None => None,
        Some(u) => Some(UnitOverrideMode::parse(u).ok_or(GenError::BadParams("invalid_units"))?),
    };
    let mut stages = Vec::with_capacity(params.stages.len());
    for s in &params.stages {
        stages.push(parse_canonical_uuid(s).ok_or(GenError::BadParams("invalid_stage_id"))?);
    }
    let mut runs = Vec::with_capacity(params.runs.len());
    for r in &params.runs {
        runs.push(parse_canonical_uuid(r).ok_or(GenError::BadParams("invalid_run_id"))?);
    }

    let gctx = GenContext {
        project: env.backend.project(),
        head,
        stages,
        runs,
        sources: env.field_sources.clone(),
        locale,
        units,
        default_decimals: env.default_decimals.clone(),
        catalog: env.catalog,
        kernel_version: env.kernel_version.clone(),
        metadata: env.metadata,
        query_provider: env.query_provider,
    };

    let resolved = match (&params.report, &params.template) {
        (Some(_), Some(_)) | (None, None) => return Err(GenError::BadParams("specify_report_or_template")),
        (Some(rep_str), None) => {
            let rid = parse_canonical_uuid(rep_str)
                .and_then(ReportId::new)
                .ok_or(GenError::BadParams("invalid_report_id"))?;
            let stored = env.backend.load(&rid)?.ok_or_else(|| GenError::NotFound(rep_str.clone()))?;
            let report = stored.open(&rid, env.kinds)?;
            let parsed_tmpl = report
                .template_ref()
                .and_then(|tref| {
                    let tid = TemplateId::new(tref.id)?;
                    env.builtins.get(&tid, Some(tref.version))
                })
                .map(|b| parse_template(&b.bytes).map_err(|_| GenError::StoreInvalid("builtin template invalid")))
                .transpose()?;
            generate(
                GenSource::Report { report: &report, template: parsed_tmpl.as_ref() },
                &gctx,
                env.model,
                env.journal,
                env.kinds,
                env.resolvers,
            )?
        }
        (None, Some(tmpl_str)) => {
            let (tref, tmpl) = resolve_template_arg(tmpl_str, env.builtins)?;
            generate(
                GenSource::Template { template: &tmpl, template_ref: tref },
                &gctx,
                env.model,
                env.journal,
                env.kinds,
                env.resolvers,
            )?
        }
    };

    env.scratch.put(&resolved)?;
    Ok(resolved)
}

fn resolve_template_arg(arg: &str, builtins: &BuiltinRegistry) -> Result<(TemplateRef, Template), GenError> {
    if let Some(tid) = resolve_starter_name(arg) {
        if let Some(b) = builtins.get(&tid, Some(crate::tmpl::starter::STARTER_VERSION)) {
            let tmpl = parse_template(&b.bytes).map_err(|_| GenError::StoreInvalid("builtin starter invalid"))?;
            return Ok((b.r, tmpl));
        }
    }
    let (id_part, ver_part) = match arg.split_once('@') {
        Some((i, v)) => (i, Some(v)),
        None => (arg, None),
    };
    let id_clean = id_part.strip_prefix("builtin:").unwrap_or(id_part);
    let tid =
        parse_canonical_uuid(id_clean).and_then(TemplateId::new).ok_or_else(|| GenError::NotFound(arg.to_string()))?;
    let ver = match ver_part {
        Some(v) => Some(v.parse::<u32>().map_err(|_| GenError::BadParams("invalid_template_version"))?),
        None => None,
    };
    let b = builtins.get(&tid, ver).ok_or_else(|| GenError::NotFound(arg.to_string()))?;
    let tmpl = parse_template(&b.bytes).map_err(|_| GenError::StoreInvalid("builtin template invalid"))?;
    Ok((b.r, tmpl))
}

/// Environment for executing `report.save_report@1` (W2).
pub struct SaveCmdEnv<'a> {
    pub backend: &'a dyn ReportBackend,
    pub scratch: &'a dyn ScratchStore,
    pub store: &'a mut dyn ResolvedStore,
}

/// Executes `report.save_report@1` (W2): persists a scratch resolved report into the CAS,
/// links it to `params.report` and journals one W2 event (`REQ-005`, `AC-3`).
pub fn save_report(params: &SaveReportParams, env: &mut SaveCmdEnv<'_>) -> Result<SaveReceipt, GenError> {
    let rid =
        parse_canonical_uuid(&params.report).and_then(ReportId::new).ok_or(GenError::BadParams("invalid_report_id"))?;
    let digest = Digest::parse(&params.resolved).map_err(|_| GenError::BadParams("invalid_resolved_digest"))?;
    if env.backend.load(&rid)?.is_none() {
        return Err(GenError::NotFound(params.report.clone()));
    }
    let resolved = env.scratch.get(&digest)?.ok_or(GenError::ResolvedNotFound(digest))?;
    let cas_bytes = resolved.to_content_canonical_bytes()?;
    let actual = Digest::from_bytes(digest_bytes(&cas_bytes).0);
    if actual != digest {
        return Err(GenError::StoreInvalid("resolved digest mismatch"));
    }
    let bytes_len = u64::try_from(cas_bytes.len()).unwrap_or(u64::MAX);
    let event = SaveReportEvent {
        project: env.backend.project(),
        report: rid,
        resolved_digest: digest,
        head: resolved.provenance.head,
    };
    env.store.commit_saved(cas_bytes, event)?;
    Ok(SaveReceipt { report: rid, resolved_digest: digest, bytes: bytes_len })
}

// ------------------------------------------------------------------ CLI & --json dispatch

/// Parses CLI arguments for `fid report generate-report` (after `generate-report`) and executes it.
pub fn invoke_generate_cli(args: &[&str], env: &mut GenerateCmdEnv<'_>) -> Result<(ResolvedReport, bool), GenError> {
    let mut params = GenerateReportParams::default();
    let mut json_flag = false;
    let mut i = 0usize;
    while i < args.len() {
        match args[i] {
            "--json" => {
                json_flag = true;
                i += 1;
            }
            "--report" => {
                let val = args.get(i + 1).ok_or(GenError::BadParams("missing_report_arg"))?;
                params.report = Some((*val).to_string());
                i += 2;
            }
            "--template" => {
                let val = args.get(i + 1).ok_or(GenError::BadParams("missing_template_arg"))?;
                params.template = Some((*val).to_string());
                i += 2;
            }
            "--head" => {
                let val = args.get(i + 1).ok_or(GenError::BadParams("missing_head_arg"))?;
                params.head = Some((*val).to_string());
                i += 2;
            }
            "--locale" => {
                let val = args.get(i + 1).ok_or(GenError::BadParams("missing_locale_arg"))?;
                params.locale = Some((*val).to_string());
                i += 2;
            }
            "--units" => {
                let val = args.get(i + 1).ok_or(GenError::BadParams("missing_units_arg"))?;
                params.units = Some((*val).to_string());
                i += 2;
            }
            "--stage" => {
                let val = args.get(i + 1).ok_or(GenError::BadParams("missing_stage_arg"))?;
                params.stages.push((*val).to_string());
                i += 2;
            }
            "--run" => {
                let val = args.get(i + 1).ok_or(GenError::BadParams("missing_run_arg"))?;
                params.runs.push((*val).to_string());
                i += 2;
            }
            _ => return Err(GenError::BadParams("unknown_cli_flag")),
        }
    }
    let resolved = generate_report(&params, env)?;
    Ok((resolved, json_flag))
}

/// Parses CLI arguments for `fid report save-report` (after `save-report`) and executes it.
pub fn invoke_save_cli(args: &[&str], env: &mut SaveCmdEnv<'_>) -> Result<(SaveReceipt, bool), GenError> {
    let mut report: Option<String> = None;
    let mut resolved: Option<String> = None;
    let mut json_flag = false;
    let mut i = 0usize;
    while i < args.len() {
        match args[i] {
            "--json" => {
                json_flag = true;
                i += 1;
            }
            "--report" => {
                let val = args.get(i + 1).ok_or(GenError::BadParams("missing_report_arg"))?;
                report = Some((*val).to_string());
                i += 2;
            }
            "--resolved" => {
                let val = args.get(i + 1).ok_or(GenError::BadParams("missing_resolved_arg"))?;
                resolved = Some((*val).to_string());
                i += 2;
            }
            _ => return Err(GenError::BadParams("unknown_cli_flag")),
        }
    }
    let params = SaveReportParams {
        report: report.ok_or(GenError::BadParams("missing_report_arg"))?,
        resolved: resolved.ok_or(GenError::BadParams("missing_resolved_arg"))?,
    };
    let receipt = save_report(&params, env)?;
    Ok((receipt, json_flag))
}

// ------------------------------------------------------------------ exact replay (AC-7)

/// Outcome of exact replay over a journal holding report events and `report.save_report@1` events.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveReplayRun {
    pub outcome: ReplayOutcome,
    /// Index of the first non-identical event, if any.
    pub first_mismatch: Option<usize>,
    /// Replayed reports with their stored digests.
    pub reports: BTreeMap<ReportId, (Report, Digest)>,
    /// Replayed `(report, resolved_digest)` links verified against the CAS.
    pub saved: Vec<(ReportId, Digest)>,
}

/// Replays a project journal containing report events (`RPT.MODEL.DOC`) and
/// `report.save_report@1` events in exact mode (`AC-7`).
///
/// Every `report.save_report@1` event is verified against the content-addressed store: the
/// stored bytes must hash to `event.resolved_digest`, parse back canonically as a valid
/// [`ResolvedReport`], and carry `provenance.head == event.head`.
pub fn replay_save_report_exact(
    events: &[CValue],
    store: &dyn ObjectRead,
    kinds: &KindRegistry,
    project: Uuid,
) -> SaveReplayRun {
    let mut reports: BTreeMap<ReportId, (Report, Digest)> = BTreeMap::new();
    let mut saved: Vec<(ReportId, Digest)> = Vec::new();

    for (i, v) in events.iter().enumerate() {
        if i >= MAX_REPLAY_EVENTS {
            return SaveReplayRun { outcome: ReplayOutcome::NotReplayable, first_mismatch: Some(i), reports, saved };
        }
        let ty = match v {
            CValue::Obj(o) => match o.get("type") {
                Some(CValue::Str(s)) => s.as_str(),
                _ => "",
            },
            _ => "",
        };
        if ty == SAVE_REPORT {
            let Ok(ev) = SaveReportEvent::from_value(v) else {
                return SaveReplayRun {
                    outcome: ReplayOutcome::NotReplayable,
                    first_mismatch: Some(i),
                    reports,
                    saved,
                };
            };
            if ev.project != project || !reports.contains_key(&ev.report) {
                return SaveReplayRun { outcome: ReplayOutcome::Divergent, first_mismatch: Some(i), reports, saved };
            }
            let Ok(Some(bytes)) = store.object(&ev.resolved_digest) else {
                return SaveReplayRun { outcome: ReplayOutcome::Divergent, first_mismatch: Some(i), reports, saved };
            };
            let actual = Digest::from_bytes(digest_bytes(&bytes).0);
            if actual != ev.resolved_digest {
                return SaveReplayRun { outcome: ReplayOutcome::Divergent, first_mismatch: Some(i), reports, saved };
            }
            let Ok(parsed) = ResolvedReport::from_cas_bytes(&bytes) else {
                return SaveReplayRun { outcome: ReplayOutcome::Divergent, first_mismatch: Some(i), reports, saved };
            };
            if parsed.digest != ev.resolved_digest || parsed.provenance.head != ev.head {
                return SaveReplayRun { outcome: ReplayOutcome::Divergent, first_mismatch: Some(i), reports, saved };
            }
            saved.push((ev.report, parsed.digest));
        } else {
            let Ok(ev) = ReportEvent::from_value(v) else {
                return SaveReplayRun {
                    outcome: ReplayOutcome::NotReplayable,
                    first_mismatch: Some(i),
                    reports,
                    saved,
                };
            };
            if let Op::AddBlock { kind, kind_version, .. } = ev.op() {
                if kinds.get(kind).map(|(v, _)| v) != Some(*kind_version) {
                    return SaveReplayRun {
                        outcome: ReplayOutcome::NotReplayable,
                        first_mismatch: Some(i),
                        reports,
                        saved,
                    };
                }
            }
            if let Op::CreateReport { project: p, .. } = ev.op() {
                if *p != project {
                    return SaveReplayRun {
                        outcome: ReplayOutcome::Divergent,
                        first_mismatch: Some(i),
                        reports,
                        saved,
                    };
                }
            }
            let id = ev.report();
            let state = reports.get(&id).map(|(r, _)| r.clone());
            let replayed = ev.op().apply(state, kinds).and_then(|r| StoredReport::of(&r).map(|s| (r, s.digest)));
            match replayed {
                Ok((r, d)) if d == ev.report_digest() => {
                    reports.insert(id, (r, d));
                }
                _ => {
                    return SaveReplayRun {
                        outcome: ReplayOutcome::Divergent,
                        first_mismatch: Some(i),
                        reports,
                        saved,
                    };
                }
            }
        }
    }

    SaveReplayRun { outcome: ReplayOutcome::Identical, first_mismatch: None, reports, saved }
}

// ------------------------------------------------------------------ private helpers

fn map_lookup_fault(block: BlockId, id: &str, f: LookupFault) -> GenError {
    match f {
        LookupFault::NotFound => GenError::Bind(BindError::NotFound(id.to_string())),
        LookupFault::Unjournaled => GenError::Bind(BindError::UnjournaledRecord { block, record: id.to_string() }),
        LookupFault::CrossProject(project) => GenError::Bind(BindError::CrossProjectBinding { block, project }),
        LookupFault::Invalid(r) => GenError::Bind(BindError::StoreInvalid(r)),
    }
}

fn obj<const N: usize>(entries: [(&str, CValue); N]) -> CValue {
    let mut m = BTreeMap::new();
    for (k, v) in entries {
        m.insert(k.to_string(), v);
    }
    CValue::Obj(m)
}

fn exact_obj<'a>(v: &'a CValue, keys: &[&str]) -> Result<&'a BTreeMap<String, CValue>, GenError> {
    let CValue::Obj(o) = v else {
        return Err(GenError::StoreInvalid("not object"));
    };
    if o.len() != keys.len() || keys.iter().any(|k| !o.contains_key(*k)) {
        return Err(GenError::StoreInvalid("unexpected keys"));
    }
    Ok(o)
}

fn req_str<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, GenError> {
    match o.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(GenError::StoreInvalid("expected string")),
    }
}

fn opt_str(o: &BTreeMap<String, CValue>, k: &str) -> Result<Option<String>, GenError> {
    match o.get(k) {
        Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.clone())),
        _ => Err(GenError::StoreInvalid("expected optional string")),
    }
}
