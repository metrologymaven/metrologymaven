//! Text, image, header and footer report blocks (spec node `RPT.MODEL.BLOCKS.TEXT`).
//!
//! Registers payload schemas v1 for `text`, `image`, `header` and `footer`, implements
//! [`BlockKind`] and [`BlockResolver`] for each, resolves bound field runs strictly through
//! [`crate::fmt`] (never holding float types or formatting numbers outside `fmt`), sniffs
//! PNG/JPEG headers with bounded parsing and 50 MiB import cap into CAS via
//! [`fid_io_reg::env::import::BlobSink`], and exposes `report.set_text_block@1` and
//! `report.set_image_block@1` (W2, D0).

use crate::bind::error::BindError;
use crate::bind::journal::Lookup;
use crate::bind::model::{Binding, Ref, RefKind, ValueBinding, BINDING_KEY};
use crate::bind::resolve::{BlockResolver, ResolveContext, Resolved};
use crate::doc::defs::ID_PATTERN;
use crate::doc::kind::{BlockKind, PayloadError};
use crate::doc::model::parse_canonical_uuid;
use crate::doc::{
    BlockKindId, CanonicalJson, KindRegistry, Op, ReportBackend, ReportError, ReportEvent, ReportId, StoredReport,
    Text, TextFault, Untrusted, SET_IMAGE_BLOCK, SET_TEXT_BLOCK,
};
use crate::fmt::{
    self as rpt_fmt, Characteristic, FmtContext, FmtError, OutcomeText, UncertaintyText, ValueRecord, ValueText,
};
use fid_canon::CValue;
use fid_io_reg::env::import::BlobSink;
use fid_io_reg::{Principal, RootSet};
use fid_project::Uuid;
use fid_registry::{fid_command, fid_commands, CommandDef, DefError, FidSchema};
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;

/// Registered kind IDs (`text`, `image`, `header`, `footer`).
pub const TEXT_KIND_ID: &str = "text";
pub const IMAGE_KIND_ID: &str = "image";
pub const HEADER_KIND_ID: &str = "header";
pub const FOOTER_KIND_ID: &str = "footer";

/// Payload schema markers v1 (`REQ-RPT.MODEL.BLOCKS.TEXT-001`).
pub const TEXT_SCHEMA: &str = "text_block@1";
pub const IMAGE_SCHEMA: &str = "image_block@1";
pub const HEADER_SCHEMA: &str = "header_block@1";
pub const FOOTER_SCHEMA: &str = "footer_block@1";

/// Maximum runs in one `text`, `header` or `footer` block.
pub const MAX_RUNS: usize = 2_048;
/// Maximum image byte size accepted by `report.set_image_block@1` (50 MiB, REQ-004).
pub const MAX_IMAGE_BYTES: u64 = 50 * 1024 * 1024;
/// Maximum JPEG marker segments scanned when locating SOF dimensions (CONSTITUTION §8).
const MAX_JPEG_MARKERS: usize = 4_096;
/// Fixed display text when an optional field (such as a missing limit) has no value.
pub const MISSING_FIELD_TEXT: &str = "no result";
/// CLI subcommands (`fid report set-text-block`, `fid report set-image-block`).
pub const CLI_SET_TEXT: &str = "set-text-block";
pub const CLI_SET_IMAGE: &str = "set-image-block";

// ------------------------------------------------------------------ errors

/// Typed errors of `text`, `image`, `header` and `footer` blocks and their commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextBlockError {
    /// `report.invalid_block_payload`: payload violates v1 schema or bounds (REQ-001).
    InvalidBlockPayload(PayloadError),
    /// `report.image_too_large`: image exceeds [`MAX_IMAGE_BYTES`] (REQ-004).
    ImageTooLarge { bytes: u64, limit: u64 },
    /// `report.image_type_unsupported`: image bytes are neither PNG nor JPEG (REQ-004).
    ImageTypeUnsupported(&'static str),
    /// `raw_path_forbidden`: agent or script passed a raw filesystem path (REQ-006).
    RawPathForbidden,
    /// Other import-root or I/O refusal (`path_outside_root`, `root_not_found`, `io`).
    ImportRoot(&'static str),
    /// `report.text_invalid`: literal run or caption violates `Text` bounds or contains NUL.
    TextInvalid(TextFault),
    /// `report.not_found`: named report or block does not exist.
    NotFound(Uuid),
    /// `report.bad_params`: malformed command or CLI arguments.
    BadParams(&'static str),
    /// Formatting error propagated from `RPT.MODEL.FMT`.
    Format(FmtError),
    /// Document/storage error propagated from `RPT.MODEL.DOC`.
    Report(ReportError),
}

impl TextBlockError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            TextBlockError::InvalidBlockPayload(_) => "report.invalid_block_payload",
            TextBlockError::ImageTooLarge { .. } => "report.image_too_large",
            TextBlockError::ImageTypeUnsupported(_) => "report.image_type_unsupported",
            TextBlockError::RawPathForbidden => "raw_path_forbidden",
            TextBlockError::ImportRoot(c) => c,
            TextBlockError::TextInvalid(_) => "report.text_invalid",
            TextBlockError::NotFound(_) => "report.not_found",
            TextBlockError::BadParams(_) => "report.bad_params",
            TextBlockError::Format(e) => e.code(),
            TextBlockError::Report(e) => e.code(),
        }
    }

    /// Canonical `--json` / MCP error object.
    pub fn to_value(&self) -> CValue {
        match self {
            TextBlockError::Report(e) => e.to_value(),
            other => {
                let mut o = BTreeMap::new();
                o.insert("code".to_string(), CValue::Str(other.code().to_string()));
                match other {
                    TextBlockError::InvalidBlockPayload(p) => {
                        o.insert("field".to_string(), CValue::Str(p.field().to_string()));
                        o.insert("reason".to_string(), CValue::Str(p.reason().to_string()));
                    }
                    TextBlockError::ImageTooLarge { bytes, limit } => {
                        o.insert("bytes".to_string(), CValue::U64(*bytes));
                        o.insert("limit".to_string(), CValue::U64(*limit));
                    }
                    TextBlockError::ImageTypeUnsupported(reason) | TextBlockError::BadParams(reason) => {
                        o.insert("reason".to_string(), CValue::Str((*reason).to_string()));
                    }
                    TextBlockError::RawPathForbidden | TextBlockError::ImportRoot(_) => {}
                    TextBlockError::TextInvalid(f) => {
                        o.insert("reason".to_string(), CValue::Str(f.as_str().to_string()));
                    }
                    TextBlockError::NotFound(id) => {
                        o.insert("id".to_string(), CValue::Str(id.to_string()));
                    }
                    TextBlockError::Format(e) => {
                        o.insert("detail".to_string(), CValue::Str(e.to_string()));
                    }
                    TextBlockError::Report(_) => {}
                }
                CValue::Obj(o)
            }
        }
    }
}

impl fmt::Display for TextBlockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TextBlockError::InvalidBlockPayload(p) => {
                write!(f, "report.invalid_block_payload: {} ({})", p.field(), p.reason())
            }
            TextBlockError::ImageTooLarge { bytes, limit } => {
                write!(f, "report.image_too_large: {bytes} bytes exceeds limit {limit}")
            }
            TextBlockError::ImageTypeUnsupported(r) => write!(f, "report.image_type_unsupported: {r}"),
            TextBlockError::RawPathForbidden => write!(f, "raw_path_forbidden"),
            TextBlockError::ImportRoot(c) => write!(f, "{c}"),
            TextBlockError::TextInvalid(fault) => write!(f, "report.text_invalid: {}", fault.as_str()),
            TextBlockError::NotFound(id) => write!(f, "report.not_found: {id}"),
            TextBlockError::BadParams(r) => write!(f, "report.bad_params: {r}"),
            TextBlockError::Format(e) => write!(f, "{e}"),
            TextBlockError::Report(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for TextBlockError {}

impl From<FmtError> for TextBlockError {
    fn from(e: FmtError) -> Self {
        TextBlockError::Format(e)
    }
}

impl From<ReportError> for TextBlockError {
    fn from(e: ReportError) -> Self {
        TextBlockError::Report(e)
    }
}

// ------------------------------------------------------------------ closed run & image types

/// Closed set of typed bound fields permitted in a `text` block (REQ-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FieldKind {
    ResultValue,
    Nominal,
    LimitLower,
    LimitUpper,
    Decision,
    Uncertainty,
    CharacteristicName,
}

impl FieldKind {
    /// All closed variants in canonical order.
    pub const ALL: &'static [FieldKind] = &[
        FieldKind::ResultValue,
        FieldKind::Nominal,
        FieldKind::LimitLower,
        FieldKind::LimitUpper,
        FieldKind::Decision,
        FieldKind::Uncertainty,
        FieldKind::CharacteristicName,
    ];

    /// Stable wire identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            FieldKind::ResultValue => "result_value",
            FieldKind::Nominal => "nominal",
            FieldKind::LimitLower => "limit_lower",
            FieldKind::LimitUpper => "limit_upper",
            FieldKind::Decision => "decision",
            FieldKind::Uncertainty => "uncertainty",
            FieldKind::CharacteristicName => "characteristic_name",
        }
    }

    /// Parses a wire field kind name.
    pub fn parse(s: &str) -> Option<FieldKind> {
        match s {
            "result_value" => Some(FieldKind::ResultValue),
            "nominal" => Some(FieldKind::Nominal),
            "limit_lower" => Some(FieldKind::LimitLower),
            "limit_upper" => Some(FieldKind::LimitUpper),
            "decision" => Some(FieldKind::Decision),
            "uncertainty" => Some(FieldKind::Uncertainty),
            "characteristic_name" => Some(FieldKind::CharacteristicName),
            _ => None,
        }
    }
}

/// Closed set of page fields permitted in `header` and `footer` blocks (REQ-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PageField {
    PageNumber,
    PageCount,
    ReportId,
    ResolvedReportDigest,
}

impl PageField {
    /// All closed variants in canonical order.
    pub const ALL: &'static [PageField] =
        &[PageField::PageNumber, PageField::PageCount, PageField::ReportId, PageField::ResolvedReportDigest];

    /// Stable wire identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            PageField::PageNumber => "page_number",
            PageField::PageCount => "page_count",
            PageField::ReportId => "report_id",
            PageField::ResolvedReportDigest => "resolved_report_digest",
        }
    }

    /// Parses a wire page field name.
    pub fn parse(s: &str) -> Option<PageField> {
        match s {
            "page_number" => Some(PageField::PageNumber),
            "page_count" => Some(PageField::PageCount),
            "report_id" => Some(PageField::ReportId),
            "resolved_report_digest" => Some(PageField::ResolvedReportDigest),
            _ => None,
        }
    }
}

/// Layout-time placeholder left unresolved by `header`/`footer` resolution (REQ-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PagePlaceholder {
    PageNumber,
    PageCount,
}

impl PagePlaceholder {
    pub const fn as_str(self) -> &'static str {
        match self {
            PagePlaceholder::PageNumber => "page_number",
            PagePlaceholder::PageCount => "page_count",
        }
    }
}

/// One typed run inside a `text`, `header` or `footer` block (REQ-002, REQ-005).
///
/// Literal runs are strictly untrusted text and are never parsed for fields, templates,
/// expressions or instructions (CONSTITUTION C6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run {
    /// Verbatim untrusted literal text.
    Literal(Untrusted<Text>),
    /// Bound typed field referencing a stable entity/result ID (valid in `text` blocks).
    Field { field: FieldKind, r: Ref },
    /// Page or document metadata field (valid in `header` and `footer` blocks).
    Page(PageField),
}

impl Run {
    /// Canonical JSON representation stored in block payloads.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        match self {
            Run::Literal(t) => {
                o.insert("kind".to_string(), CValue::Str("literal".to_string()));
                o.insert("text".to_string(), t.to_value());
            }
            Run::Field { field, r } => {
                o.insert("field".to_string(), CValue::Str(field.as_str().to_string()));
                o.insert("kind".to_string(), CValue::Str("field".to_string()));
                o.insert("ref".to_string(), r.to_value());
            }
            Run::Page(pf) => {
                o.insert("kind".to_string(), CValue::Str("page".to_string()));
                o.insert("page_field".to_string(), CValue::Str(pf.as_str().to_string()));
            }
        }
        CValue::Obj(o)
    }

    /// Strict stored-payload parser.
    pub fn from_value(v: &CValue, path: &str) -> Result<Run, PayloadError> {
        let CValue::Obj(o) = v else {
            return Err(PayloadError::new(path, "not_object"));
        };
        let Some(CValue::Str(kind)) = o.get("kind") else {
            return Err(PayloadError::new(&format!("{path}/kind"), "not_string"));
        };
        match kind.as_str() {
            "literal" => {
                if o.len() != 2 || !o.contains_key("text") {
                    return Err(PayloadError::new(path, "unexpected_keys"));
                }
                let Some(tv) = o.get("text") else {
                    return Err(PayloadError::new(&format!("{path}/text"), "missing"));
                };
                let t = Untrusted::from_value(tv)
                    .map_err(|_| PayloadError::new(&format!("{path}/text"), "text_invalid"))?;
                Ok(Run::Literal(t))
            }
            "field" => {
                if o.len() != 3 || !o.contains_key("field") || !o.contains_key("ref") {
                    return Err(PayloadError::new(path, "unexpected_keys"));
                }
                let Some(CValue::Str(fs)) = o.get("field") else {
                    return Err(PayloadError::new(&format!("{path}/field"), "not_string"));
                };
                let field =
                    FieldKind::parse(fs).ok_or_else(|| PayloadError::new(&format!("{path}/field"), "unknown_field"))?;
                let Some(rv) = o.get("ref") else {
                    return Err(PayloadError::new(&format!("{path}/ref"), "missing"));
                };
                let r = parse_stored_ref(rv, &format!("{path}/ref"))?;
                Ok(Run::Field { field, r })
            }
            "page" => {
                if o.len() != 2 || !o.contains_key("page_field") {
                    return Err(PayloadError::new(path, "unexpected_keys"));
                }
                let Some(CValue::Str(ps)) = o.get("page_field") else {
                    return Err(PayloadError::new(&format!("{path}/page_field"), "not_string"));
                };
                let pf = PageField::parse(ps)
                    .ok_or_else(|| PayloadError::new(&format!("{path}/page_field"), "unknown_page_field"))?;
                Ok(Run::Page(pf))
            }
            _ => Err(PayloadError::new(&format!("{path}/kind"), "unknown_run_kind")),
        }
    }
}

/// Supported image MIME types (REQ-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageMime {
    Png,
    Jpeg,
}

impl ImageMime {
    pub const fn as_str(self) -> &'static str {
        match self {
            ImageMime::Png => "image/png",
            ImageMime::Jpeg => "image/jpeg",
        }
    }

    pub fn parse(s: &str) -> Option<ImageMime> {
        match s {
            "image/png" | "png" => Some(ImageMime::Png),
            "image/jpeg" | "jpeg" | "jpg" => Some(ImageMime::Jpeg),
            _ => None,
        }
    }
}

/// Sniffed image header information (dimensions + MIME type).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    pub mime: ImageMime,
    pub width_px: u32,
    pub height_px: u32,
}

/// Bounded content sniffing of PNG and JPEG bytes (REQ-004, CONSTITUTION §8).
///
/// Inspects magic bytes and structural headers only; never trusts file extensions and never
/// allocates proportional to untrusted header values.
pub fn sniff_image(bytes: &[u8]) -> Result<ImageInfo, TextBlockError> {
    if bytes.len() >= 24 && bytes.get(..8) == Some(b"\x89PNG\r\n\x1a\n".as_slice()) {
        return parse_png_header(bytes);
    }
    if bytes.len() >= 4 && bytes.get(..2) == Some([0xFF, 0xD8].as_slice()) {
        return parse_jpeg_header(bytes);
    }
    Err(TextBlockError::ImageTypeUnsupported("not_png_or_jpeg"))
}

fn read_be_u32(bytes: &[u8], off: usize) -> Option<u32> {
    let raw: [u8; 4] = bytes.get(off..off + 4)?.try_into().ok()?;
    Some(u32::from_be_bytes(raw))
}

fn read_be_u16(bytes: &[u8], off: usize) -> Option<u16> {
    let raw: [u8; 2] = bytes.get(off..off + 2)?.try_into().ok()?;
    Some(u16::from_be_bytes(raw))
}

fn parse_png_header(bytes: &[u8]) -> Result<ImageInfo, TextBlockError> {
    // 8-byte signature + 4-byte IHDR length (13) + 4-byte "IHDR" + 4-byte width + 4-byte height.
    let ihdr_len = read_be_u32(bytes, 8).ok_or(TextBlockError::ImageTypeUnsupported("truncated_png"))?;
    if ihdr_len != 13 || bytes.get(12..16) != Some(b"IHDR".as_slice()) {
        return Err(TextBlockError::ImageTypeUnsupported("invalid_png_ihdr"));
    }
    let width_px = read_be_u32(bytes, 16).ok_or(TextBlockError::ImageTypeUnsupported("truncated_png"))?;
    let height_px = read_be_u32(bytes, 20).ok_or(TextBlockError::ImageTypeUnsupported("truncated_png"))?;
    if width_px == 0 || height_px == 0 {
        return Err(TextBlockError::ImageTypeUnsupported("zero_png_dimensions"));
    }
    Ok(ImageInfo { mime: ImageMime::Png, width_px, height_px })
}

fn is_jpeg_sof_marker(marker: u8) -> bool {
    matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF)
}

fn parse_jpeg_header(bytes: &[u8]) -> Result<ImageInfo, TextBlockError> {
    let mut pos = 2usize;
    let mut markers_seen = 0usize;
    while pos < bytes.len() && markers_seen < MAX_JPEG_MARKERS {
        markers_seen += 1;
        // Skip fill bytes before marker prefix.
        while bytes.get(pos) == Some(&0xFF) {
            pos += 1;
        }
        let Some(&marker) = bytes.get(pos) else {
            return Err(TextBlockError::ImageTypeUnsupported("truncated_jpeg"));
        };
        pos += 1;
        // Standalone markers without payload length.
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        // EOI or SOS before finding SOF -> malformed/unsupported.
        if marker == 0xD9 || marker == 0xDA {
            return Err(TextBlockError::ImageTypeUnsupported("jpeg_missing_sof"));
        }
        let seg_len = read_be_u16(bytes, pos).ok_or(TextBlockError::ImageTypeUnsupported("truncated_jpeg"))?;
        if seg_len < 2 {
            return Err(TextBlockError::ImageTypeUnsupported("invalid_jpeg_segment"));
        }
        let seg_end = pos
            .checked_add(usize::from(seg_len))
            .ok_or(TextBlockError::ImageTypeUnsupported("invalid_jpeg_segment"))?;
        if seg_end > bytes.len() {
            return Err(TextBlockError::ImageTypeUnsupported("truncated_jpeg"));
        }
        if is_jpeg_sof_marker(marker) {
            if seg_len < 7 {
                return Err(TextBlockError::ImageTypeUnsupported("invalid_jpeg_sof"));
            }
            // Layout after 2-byte length: precision (1B), height (2B BE), width (2B BE).
            let height_px =
                u32::from(read_be_u16(bytes, pos + 3).ok_or(TextBlockError::ImageTypeUnsupported("truncated_jpeg"))?);
            let width_px =
                u32::from(read_be_u16(bytes, pos + 5).ok_or(TextBlockError::ImageTypeUnsupported("truncated_jpeg"))?);
            if width_px == 0 || height_px == 0 {
                return Err(TextBlockError::ImageTypeUnsupported("zero_jpeg_dimensions"));
            }
            return Ok(ImageInfo { mime: ImageMime::Jpeg, width_px, height_px });
        }
        pos = seg_end;
    }
    Err(TextBlockError::ImageTypeUnsupported("jpeg_sof_not_found"))
}

// ------------------------------------------------------------------ payloads

/// Payload of a `text`, `header`, or `footer` block (schema v1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunBlockPayload {
    /// Block kind (`text`, `header`, or `footer`).
    pub kind: RunBlockKind,
    /// Ordered list of typed runs.
    pub runs: Vec<Run>,
}

/// Distinguishes `text`, `header` and `footer` run rules (REQ-002 vs REQ-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunBlockKind {
    Text,
    Header,
    Footer,
}

impl RunBlockKind {
    pub const fn kind_id(self) -> &'static str {
        match self {
            RunBlockKind::Text => TEXT_KIND_ID,
            RunBlockKind::Header => HEADER_KIND_ID,
            RunBlockKind::Footer => FOOTER_KIND_ID,
        }
    }

    pub const fn schema(self) -> &'static str {
        match self {
            RunBlockKind::Text => TEXT_SCHEMA,
            RunBlockKind::Header => HEADER_SCHEMA,
            RunBlockKind::Footer => FOOTER_SCHEMA,
        }
    }

    pub fn parse_kind(s: &str) -> Option<RunBlockKind> {
        match s {
            TEXT_KIND_ID => Some(RunBlockKind::Text),
            HEADER_KIND_ID => Some(RunBlockKind::Header),
            FOOTER_KIND_ID => Some(RunBlockKind::Footer),
            _ => None,
        }
    }

    fn allows_run(self, run: &Run) -> bool {
        match self {
            RunBlockKind::Text => matches!(run, Run::Literal(_) | Run::Field { .. }),
            RunBlockKind::Header | RunBlockKind::Footer => matches!(run, Run::Literal(_) | Run::Page(_)),
        }
    }
}

impl RunBlockPayload {
    /// Constructs a validated run-block payload.
    pub fn new(kind: RunBlockKind, runs: Vec<Run>) -> Result<RunBlockPayload, PayloadError> {
        if runs.len() > MAX_RUNS {
            return Err(PayloadError::new("/runs", "too_many_runs"));
        }
        for (i, run) in runs.iter().enumerate() {
            if !kind.allows_run(run) {
                return Err(PayloadError::new(&format!("/runs/{i}"), "run_kind_not_allowed"));
            }
        }
        Ok(RunBlockPayload { kind, runs })
    }

    /// Empty default payload (zero runs) for `add_block`.
    pub fn empty(kind: RunBlockKind) -> RunBlockPayload {
        RunBlockPayload { kind, runs: Vec::new() }
    }

    /// Canonical JSON object for storage inside a block payload.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("runs".to_string(), CValue::Arr(self.runs.iter().map(Run::to_value).collect()));
        o.insert("schema".to_string(), CValue::Str(self.kind.schema().to_string()));
        CValue::Obj(o)
    }

    /// Canonical JSON bytes.
    pub fn to_canonical(&self) -> Result<CanonicalJson, PayloadError> {
        CanonicalJson::new(&self.to_value()).ok_or_else(|| PayloadError::new("", "payload_bounds"))
    }

    /// Validates and parses a stored `text`, `header`, or `footer` payload.
    pub fn from_value(kind: RunBlockKind, v: &CValue) -> Result<RunBlockPayload, PayloadError> {
        let CValue::Obj(o) = v else {
            return Err(PayloadError::new("", "not_object"));
        };
        let allowed_keys: &[&str] = match kind {
            RunBlockKind::Text => &["schema", "runs", BINDING_KEY, "values"],
            RunBlockKind::Header | RunBlockKind::Footer => &["schema", "runs"],
        };
        for k in o.keys() {
            if !allowed_keys.contains(&k.as_str()) {
                return Err(PayloadError::new(&format!("/{k}"), "unexpected_key"));
            }
        }
        match o.get("schema") {
            Some(CValue::Str(s)) if s == kind.schema() => {}
            _ => return Err(PayloadError::new("/schema", "wrong_schema")),
        }
        let Some(CValue::Arr(runs_raw)) = o.get("runs") else {
            return Err(PayloadError::new("/runs", "not_array"));
        };
        if runs_raw.len() > MAX_RUNS {
            return Err(PayloadError::new("/runs", "too_many_runs"));
        }
        let mut runs = Vec::with_capacity(runs_raw.len());
        for (i, rv) in runs_raw.iter().enumerate() {
            let r = Run::from_value(rv, &format!("/runs/{i}"))?;
            if !kind.allows_run(&r) {
                return Err(PayloadError::new(&format!("/runs/{i}"), "run_kind_not_allowed"));
            }
            runs.push(r);
        }
        if o.contains_key(BINDING_KEY) {
            Binding::of_payload(v).map_err(|_| PayloadError::new("/binding", "invalid_binding"))?;
        }
        if let Some(vals_v) = o.get("values") {
            let CValue::Arr(arr) = vals_v else {
                return Err(PayloadError::new("/values", "not_array"));
            };
            for (i, item) in arr.iter().enumerate() {
                let ptr = format!("/values/{i}");
                ValueBinding::from_value(item, &ptr).map_err(|_| PayloadError::new(&ptr, "invalid_value_binding"))?;
            }
        }
        Ok(RunBlockPayload { kind, runs })
    }
}

fn parse_stored_ref(v: &CValue, path: &str) -> Result<Ref, PayloadError> {
    let CValue::Obj(o) = v else {
        return Err(PayloadError::new(path, "invalid_ref"));
    };
    if o.len() != 2 {
        return Err(PayloadError::new(path, "invalid_ref"));
    }
    let (Some(CValue::Str(kind_s)), Some(CValue::Str(id_s))) = (o.get("kind"), o.get("id")) else {
        return Err(PayloadError::new(path, "invalid_ref"));
    };
    let rk = RefKind::parse(kind_s).ok_or_else(|| PayloadError::new(path, "invalid_ref"))?;
    Ref::new(rk, id_s).ok_or_else(|| PayloadError::new(path, "invalid_ref"))
}

/// Payload of an `image` block (`image_block@1`, REQ-001, REQ-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagePayload {
    pub digest: Digest,
    pub mime: ImageMime,
    pub width_px: u32,
    pub height_px: u32,
    pub caption: Option<Untrusted<Text>>,
}

fn parse_pos_u32_str(v: &CValue, field: &str) -> Result<u32, PayloadError> {
    let CValue::Str(s) = v else {
        return Err(PayloadError::new(field, "not_string"));
    };
    if s.is_empty() || s.starts_with('0') || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(PayloadError::new(field, "invalid_dimension"));
    }
    let n = s.parse::<u32>().map_err(|_| PayloadError::new(field, "invalid_dimension"))?;
    if n == 0 {
        return Err(PayloadError::new(field, "zero_dimension"));
    }
    Ok(n)
}

impl ImagePayload {
    /// Constructs a validated `image` payload.
    pub fn new(
        digest: Digest,
        mime: ImageMime,
        width_px: u32,
        height_px: u32,
        caption: Option<Untrusted<Text>>,
    ) -> Result<ImagePayload, PayloadError> {
        if width_px == 0 {
            return Err(PayloadError::new("/width_px", "zero_dimension"));
        }
        if height_px == 0 {
            return Err(PayloadError::new("/height_px", "zero_dimension"));
        }
        Ok(ImagePayload { digest, mime, width_px, height_px, caption })
    }

    /// Canonical JSON object for storage inside an `image` block payload.
    ///
    /// Dimensions are stored as canonical decimal strings so the payload holds zero JSON
    /// numbers under `RPT.MODEL.BIND.RESOLVE`'s no-value check (`find_value`).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("caption".to_string(), self.caption.as_ref().map_or(CValue::Null, Untrusted::to_value));
        o.insert("digest".to_string(), CValue::Str(self.digest.as_str().to_string()));
        o.insert("height_px".to_string(), CValue::Str(self.height_px.to_string()));
        o.insert("mime".to_string(), CValue::Str(self.mime.as_str().to_string()));
        o.insert("schema".to_string(), CValue::Str(IMAGE_SCHEMA.to_string()));
        o.insert("width_px".to_string(), CValue::Str(self.width_px.to_string()));
        CValue::Obj(o)
    }

    /// Canonical JSON bytes.
    pub fn to_canonical(&self) -> Result<CanonicalJson, PayloadError> {
        CanonicalJson::new(&self.to_value()).ok_or_else(|| PayloadError::new("", "payload_bounds"))
    }

    /// Strict parser and validator for stored `image_block@1` payloads.
    pub fn from_value(v: &CValue) -> Result<ImagePayload, PayloadError> {
        let CValue::Obj(o) = v else {
            return Err(PayloadError::new("", "not_object"));
        };
        let keys = ["schema", "digest", "mime", "width_px", "height_px", "caption"];
        if o.len() != keys.len() || o.keys().any(|k| !keys.contains(&k.as_str())) {
            return Err(PayloadError::new("", "unexpected_keys"));
        }
        match o.get("schema") {
            Some(CValue::Str(s)) if s == IMAGE_SCHEMA => {}
            _ => return Err(PayloadError::new("/schema", "wrong_schema")),
        }
        let digest = match o.get("digest") {
            Some(CValue::Str(s)) => Digest::parse(s).map_err(|_| PayloadError::new("/digest", "invalid_digest"))?,
            _ => return Err(PayloadError::new("/digest", "not_string")),
        };
        let mime = match o.get("mime") {
            Some(CValue::Str(s)) => {
                ImageMime::parse(s).ok_or_else(|| PayloadError::new("/mime", "unsupported_mime"))?
            }
            _ => return Err(PayloadError::new("/mime", "not_string")),
        };
        let width_px = parse_pos_u32_str(
            o.get("width_px").ok_or_else(|| PayloadError::new("/width_px", "missing"))?,
            "/width_px",
        )?;
        let height_px = parse_pos_u32_str(
            o.get("height_px").ok_or_else(|| PayloadError::new("/height_px", "missing"))?,
            "/height_px",
        )?;
        let caption = match o.get("caption") {
            Some(CValue::Null) => None,
            Some(other) => {
                Some(Untrusted::from_value(other).map_err(|_| PayloadError::new("/caption", "text_invalid"))?)
            }
            None => return Err(PayloadError::new("/caption", "missing")),
        };
        Ok(ImagePayload { digest, mime, width_px, height_px, caption })
    }
}

// ------------------------------------------------------------------ BlockKind implementations

fn extract_value_pointers(payload: &CanonicalJson) -> Result<Vec<String>, PayloadError> {
    let CValue::Obj(o) = payload.value() else {
        return Err(PayloadError::new("", "not_object"));
    };
    let Some(CValue::Arr(vals)) = o.get("values") else {
        return Ok(Vec::new());
    };
    Ok((0..vals.len()).map(|i| format!("/values/{i}")).collect())
}

/// Block kind `text` (payload schema v1).
#[derive(Debug, Clone, Copy, Default)]
pub struct TextBlockKind;

impl BlockKind for TextBlockKind {
    fn id(&self) -> BlockKindId {
        BlockKindId::from_literal(TEXT_KIND_ID)
    }

    fn version(&self) -> u32 {
        1
    }

    fn validate(&self, payload: &CanonicalJson) -> Result<(), PayloadError> {
        RunBlockPayload::from_value(RunBlockKind::Text, payload.value())?;
        Ok(())
    }

    fn value_fields(&self, payload: &CanonicalJson) -> Result<Vec<String>, PayloadError> {
        extract_value_pointers(payload)
    }
}

/// Block kind `image` (payload schema v1).
#[derive(Debug, Clone, Copy, Default)]
pub struct ImageBlockKind;

impl BlockKind for ImageBlockKind {
    fn id(&self) -> BlockKindId {
        BlockKindId::from_literal(IMAGE_KIND_ID)
    }

    fn version(&self) -> u32 {
        1
    }

    fn validate(&self, payload: &CanonicalJson) -> Result<(), PayloadError> {
        ImagePayload::from_value(payload.value())?;
        Ok(())
    }

    fn value_fields(&self, _payload: &CanonicalJson) -> Result<Vec<String>, PayloadError> {
        Ok(Vec::new())
    }
}

/// Block kind `header` (payload schema v1).
#[derive(Debug, Clone, Copy, Default)]
pub struct HeaderBlockKind;

impl BlockKind for HeaderBlockKind {
    fn id(&self) -> BlockKindId {
        BlockKindId::from_literal(HEADER_KIND_ID)
    }

    fn version(&self) -> u32 {
        1
    }

    fn validate(&self, payload: &CanonicalJson) -> Result<(), PayloadError> {
        RunBlockPayload::from_value(RunBlockKind::Header, payload.value())?;
        Ok(())
    }

    fn value_fields(&self, _payload: &CanonicalJson) -> Result<Vec<String>, PayloadError> {
        Ok(Vec::new())
    }
}

/// Block kind `footer` (payload schema v1).
#[derive(Debug, Clone, Copy, Default)]
pub struct FooterBlockKind;

impl BlockKind for FooterBlockKind {
    fn id(&self) -> BlockKindId {
        BlockKindId::from_literal(FOOTER_KIND_ID)
    }

    fn version(&self) -> u32 {
        1
    }

    fn validate(&self, payload: &CanonicalJson) -> Result<(), PayloadError> {
        RunBlockPayload::from_value(RunBlockKind::Footer, payload.value())?;
        Ok(())
    }

    fn value_fields(&self, _payload: &CanonicalJson) -> Result<Vec<String>, PayloadError> {
        Ok(Vec::new())
    }
}

/// Registers `text`, `image`, `header` and `footer` v1 kinds in `kinds`.
pub fn register_text_kinds(kinds: &mut KindRegistry) -> Result<(), crate::doc::KindRegistryError> {
    kinds.register(Box::new(TextBlockKind))?;
    kinds.register(Box::new(ImageBlockKind))?;
    kinds.register(Box::new(HeaderBlockKind))?;
    kinds.register(Box::new(FooterBlockKind))?;
    Ok(())
}

// ------------------------------------------------------------------ BlockResolver implementations

/// [`BlockResolver`] for `text` blocks.
#[derive(Debug, Clone, Copy, Default)]
pub struct TextBlockResolver;

impl BlockResolver for TextBlockResolver {
    fn kind(&self) -> BlockKindId {
        BlockKindId::from_literal(TEXT_KIND_ID)
    }

    fn place(
        &self,
        payload: &CanonicalJson,
        _binding: &Binding,
        values: &[ValueBinding],
    ) -> Result<CValue, PayloadError> {
        let parsed = RunBlockPayload::from_value(RunBlockKind::Text, payload.value())?;
        let CValue::Obj(mut out) = parsed.to_value() else {
            return Err(PayloadError::new("", "not_object"));
        };
        if !values.is_empty() {
            out.insert("values".to_string(), CValue::Arr(values.iter().map(ValueBinding::to_value).collect()));
        }
        Ok(CValue::Obj(out))
    }

    fn resolve(&self, view: &Resolved, _ctx: &ResolveContext<'_>, _lookup: &dyn Lookup) -> Result<CValue, BindError> {
        let pointers: Vec<CValue> = view.values().iter().map(|v| CValue::Str(v.pointer().to_string())).collect();
        let mut o = BTreeMap::new();
        o.insert("kind".to_string(), CValue::Str(TEXT_KIND_ID.to_string()));
        o.insert("value_pointers".to_string(), CValue::Arr(pointers));
        Ok(CValue::Obj(o))
    }
}

/// [`BlockResolver`] for `image` blocks (structure only; no record values).
#[derive(Debug, Clone, Copy, Default)]
pub struct ImageBlockResolver;

impl BlockResolver for ImageBlockResolver {
    fn kind(&self) -> BlockKindId {
        BlockKindId::from_literal(IMAGE_KIND_ID)
    }

    fn place(
        &self,
        payload: &CanonicalJson,
        _binding: &Binding,
        values: &[ValueBinding],
    ) -> Result<CValue, PayloadError> {
        if !values.is_empty() {
            return Err(PayloadError::new("/values", "image_accepts_no_values"));
        }
        let parsed = ImagePayload::from_value(payload.value())?;
        Ok(parsed.to_value())
    }

    fn resolve(&self, _view: &Resolved, _ctx: &ResolveContext<'_>, _lookup: &dyn Lookup) -> Result<CValue, BindError> {
        let mut o = BTreeMap::new();
        o.insert("kind".to_string(), CValue::Str(IMAGE_KIND_ID.to_string()));
        Ok(CValue::Obj(o))
    }
}

fn resolve_chrome_layout(kind: &'static str) -> CValue {
    let mut o = BTreeMap::new();
    o.insert("kind".to_string(), CValue::Str(kind.to_string()));
    o.insert("layout_time_fields".to_string(), CValue::Bool(true));
    CValue::Obj(o)
}

/// [`BlockResolver`] for `header` blocks.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeaderBlockResolver;

impl BlockResolver for HeaderBlockResolver {
    fn kind(&self) -> BlockKindId {
        BlockKindId::from_literal(HEADER_KIND_ID)
    }

    fn place(
        &self,
        payload: &CanonicalJson,
        _binding: &Binding,
        values: &[ValueBinding],
    ) -> Result<CValue, PayloadError> {
        if !values.is_empty() {
            return Err(PayloadError::new("/values", "header_accepts_no_values"));
        }
        let parsed = RunBlockPayload::from_value(RunBlockKind::Header, payload.value())?;
        Ok(parsed.to_value())
    }

    fn resolve(&self, _view: &Resolved, _ctx: &ResolveContext<'_>, _lookup: &dyn Lookup) -> Result<CValue, BindError> {
        Ok(resolve_chrome_layout(HEADER_KIND_ID))
    }
}

/// [`BlockResolver`] for `footer` blocks.
#[derive(Debug, Clone, Copy, Default)]
pub struct FooterBlockResolver;

impl BlockResolver for FooterBlockResolver {
    fn kind(&self) -> BlockKindId {
        BlockKindId::from_literal(FOOTER_KIND_ID)
    }

    fn place(
        &self,
        payload: &CanonicalJson,
        _binding: &Binding,
        values: &[ValueBinding],
    ) -> Result<CValue, PayloadError> {
        if !values.is_empty() {
            return Err(PayloadError::new("/values", "footer_accepts_no_values"));
        }
        let parsed = RunBlockPayload::from_value(RunBlockKind::Footer, payload.value())?;
        Ok(parsed.to_value())
    }

    fn resolve(&self, _view: &Resolved, _ctx: &ResolveContext<'_>, _lookup: &dyn Lookup) -> Result<CValue, BindError> {
        Ok(resolve_chrome_layout(FOOTER_KIND_ID))
    }
}

/// Registers resolvers for `text`, `image`, `header` and `footer`.
pub fn register_text_resolvers(reg: &mut crate::bind::ResolverRegistry) -> Result<(), BindError> {
    reg.register(Box::new(TextBlockResolver))?;
    reg.register(Box::new(ImageBlockResolver))?;
    reg.register(Box::new(HeaderBlockResolver))?;
    reg.register(Box::new(FooterBlockResolver))?;
    Ok(())
}

// ------------------------------------------------------------------ run resolution (FMT only)

/// Bound record and characteristic data supplied for one stable [`Ref`] during text-run
/// resolution. All numeric formatting delegates strictly to [`crate::fmt`].
pub struct FieldRecordInput<'a> {
    /// Characteristic specification (nominal, limits, name, formatter kind).
    pub characteristic: &'a Characteristic,
    /// Characteristic display name (untrusted text).
    pub characteristic_name: Untrusted<Text>,
    /// Measured/evaluated result record when present for this reference.
    pub result: Option<&'a ValueRecord<'a>>,
}

/// One resolved run produced by [`resolve_text_runs`] or [`resolve_chrome_runs`].
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedRun {
    /// Literal user text, copied unchanged with untrusted marking (REQ-003, C6).
    Literal(Untrusted<Text>),
    /// Formatted numeric value from `RPT.MODEL.FMT` with its `FormatterId` (REQ-003).
    Value(ValueText),
    /// Formatted conformity decision from `RPT.MODEL.FMT`.
    Decision(OutcomeText),
    /// Formatted uncertainty statement from `RPT.MODEL.FMT` (UEC-0 = `"not evaluated"`).
    Uncertainty(UncertaintyText),
    /// Characteristic name text.
    CharacteristicName(Untrusted<Text>),
    /// Absent optional value (e.g. characteristic has no lower/upper limit or no result).
    Missing(&'static str),
    /// Layout-time placeholder left unfilled for `RPT.OUTPUT.PDF.LAYOUT` (REQ-005).
    PagePlaceholder(PagePlaceholder),
    /// Filled report UUID (`header`/`footer` `report_id` field, REQ-005).
    ReportId(ReportId),
    /// Filled resolved-report SHA-256 digest (`header`/`footer` `resolved_report_digest`, REQ-005).
    ResolvedReportDigest(Digest),
}

impl ResolvedRun {
    /// Human-readable display text (`None` for layout-time page placeholders).
    pub fn display(&self) -> Option<String> {
        match self {
            ResolvedRun::Literal(t) | ResolvedRun::CharacteristicName(t) => {
                Some(t.untrusted_value().as_str().to_string())
            }
            ResolvedRun::Value(vt) => Some(vt.display.clone()),
            ResolvedRun::Decision(ot) => Some(ot.text.clone()),
            ResolvedRun::Uncertainty(ut) => Some(ut.display().to_string()),
            ResolvedRun::Missing(s) => Some((*s).to_string()),
            ResolvedRun::PagePlaceholder(_) => None,
            ResolvedRun::ReportId(id) => Some(id.uuid().to_string()),
            ResolvedRun::ResolvedReportDigest(d) => Some(d.as_str().to_string()),
        }
    }

    /// Underlying `ValueText` (carrying `FormatterId`) when this run formatted a numeric value.
    pub fn value_text(&self) -> Option<&ValueText> {
        match self {
            ResolvedRun::Value(vt) => Some(vt),
            ResolvedRun::Uncertainty(ut) => ut.value_text(),
            _ => None,
        }
    }
}

/// Resolves a `text` block's runs against typed bound record inputs strictly through
/// [`crate::fmt`] (REQ-002, REQ-003, C6).
///
/// Literal runs are copied verbatim with their untrusted marking and are never scanned for
/// placeholders, expressions or instructions.
pub fn resolve_text_runs(
    payload: &RunBlockPayload,
    records: &BTreeMap<Ref, FieldRecordInput<'_>>,
    ctx: &FmtContext<'_>,
) -> Result<Vec<ResolvedRun>, TextBlockError> {
    if payload.kind != RunBlockKind::Text {
        return Err(TextBlockError::InvalidBlockPayload(PayloadError::new("/schema", "expected_text_block")));
    }
    let mut out = Vec::with_capacity(payload.runs.len());
    for run in &payload.runs {
        match run {
            Run::Literal(t) => out.push(ResolvedRun::Literal(t.clone())),
            Run::Field { field, r } => {
                let Some(input) = records.get(r) else {
                    out.push(ResolvedRun::Missing(MISSING_FIELD_TEXT));
                    continue;
                };
                out.push(resolve_field(*field, input, ctx)?);
            }
            Run::Page(_) => {
                return Err(TextBlockError::InvalidBlockPayload(PayloadError::new(
                    "/runs",
                    "page_field_in_text_block",
                )));
            }
        }
    }
    Ok(out)
}

fn resolve_field(
    field: FieldKind,
    input: &FieldRecordInput<'_>,
    ctx: &FmtContext<'_>,
) -> Result<ResolvedRun, TextBlockError> {
    match field {
        FieldKind::ResultValue => match input.result {
            Some(rec) => Ok(ResolvedRun::Value(rpt_fmt::fmt_value(rec, input.characteristic, ctx)?)),
            None => Ok(ResolvedRun::Missing(MISSING_FIELD_TEXT)),
        },
        FieldKind::Nominal => match input.characteristic.nominal {
            Some(_) => Ok(ResolvedRun::Value(rpt_fmt::fmt_requirement(input.characteristic, ctx)?)),
            None => Ok(ResolvedRun::Missing(MISSING_FIELD_TEXT)),
        },
        FieldKind::LimitLower => {
            match input.characteristic.limits.as_ref().and_then(|l| l.lower.map(|lo| (lo, l.unit))) {
                Some((lo, unit)) => Ok(ResolvedRun::Value(rpt_fmt::fmt_limit(input.characteristic, lo, unit, ctx)?)),
                None => Ok(ResolvedRun::Missing(MISSING_FIELD_TEXT)),
            }
        }
        FieldKind::LimitUpper => {
            match input.characteristic.limits.as_ref().and_then(|l| l.upper.map(|hi| (hi, l.unit))) {
                Some((hi, unit)) => Ok(ResolvedRun::Value(rpt_fmt::fmt_limit(input.characteristic, hi, unit, ctx)?)),
                None => Ok(ResolvedRun::Missing(MISSING_FIELD_TEXT)),
            }
        }
        FieldKind::Decision => match input.result {
            Some(rec) => Ok(ResolvedRun::Decision(rpt_fmt::fmt_outcome(rec, ctx.catalog)?)),
            None => Ok(ResolvedRun::Missing(MISSING_FIELD_TEXT)),
        },
        FieldKind::Uncertainty => match input.result {
            Some(rec) => Ok(ResolvedRun::Uncertainty(rpt_fmt::fmt_uncertainty(rec, input.characteristic, ctx)?)),
            None => Ok(ResolvedRun::Missing(MISSING_FIELD_TEXT)),
        },
        FieldKind::CharacteristicName => Ok(ResolvedRun::CharacteristicName(input.characteristic_name.clone())),
    }
}

/// Resolves a `header` or `footer` block's runs: fills `report_id` and
/// `resolved_report_digest`, copies literal runs unchanged, and leaves `page_number` and
/// `page_count` as layout placeholders (REQ-005, AC-4).
pub fn resolve_chrome_runs(
    payload: &RunBlockPayload,
    report_id: ReportId,
    resolved_report_digest: Digest,
) -> Result<Vec<ResolvedRun>, TextBlockError> {
    if !matches!(payload.kind, RunBlockKind::Header | RunBlockKind::Footer) {
        return Err(TextBlockError::InvalidBlockPayload(PayloadError::new("/schema", "expected_header_or_footer")));
    }
    let mut out = Vec::with_capacity(payload.runs.len());
    for run in &payload.runs {
        match run {
            Run::Literal(t) => out.push(ResolvedRun::Literal(t.clone())),
            Run::Page(PageField::PageNumber) => out.push(ResolvedRun::PagePlaceholder(PagePlaceholder::PageNumber)),
            Run::Page(PageField::PageCount) => out.push(ResolvedRun::PagePlaceholder(PagePlaceholder::PageCount)),
            Run::Page(PageField::ReportId) => out.push(ResolvedRun::ReportId(report_id)),
            Run::Page(PageField::ResolvedReportDigest) => {
                out.push(ResolvedRun::ResolvedReportDigest(resolved_report_digest))
            }
            Run::Field { .. } => {
                return Err(TextBlockError::InvalidBlockPayload(PayloadError::new(
                    "/runs",
                    "field_run_in_header_or_footer",
                )));
            }
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ command report.set_text_block@1

/// Execution context for [`set_text_block`].
pub struct TextCmdCtx<'a> {
    pub backend: &'a mut dyn ReportBackend,
    pub kinds: &'a KindRegistry,
}

/// One wire run parameter for `report.set_text_block@1`.
///
/// Accepts both explicit form (`kind` + `text`/`field`/`ref`/`page_field`) and compact CLI
/// shorthand (`{"text": "..."}`, `{"field": "result_value", "ref": "<id>"}`,
/// `{"page": "page_number"}`).
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct RunParam {
    #[fid(description = "Run kind: literal, field, or page (inferred from populated fields when omitted).")]
    pub kind: Option<String>,
    #[fid(description = "Untrusted literal text (literal run). Never parsed as a field or instruction.")]
    pub text: Option<String>,
    #[fid(
        description = "Typed field name (result_value, nominal, limit_lower, limit_upper, decision, uncertainty, characteristic_name)."
    )]
    pub field: Option<String>,
    #[fid(
        description = "Reference kind for field runs (result, characteristic, named_view, series, field; default result)."
    )]
    pub ref_kind: Option<String>,
    #[fid(description = "Stable target ID for field runs (UUID or field ID).")]
    pub r#ref: Option<String>,
    #[fid(description = "Header/footer page field (page_number, page_count, report_id, resolved_report_digest).")]
    pub page_field: Option<String>,
}

impl RunParam {
    /// Convenience constructor for a literal untrusted text run.
    pub fn literal(text: impl Into<String>) -> RunParam {
        RunParam {
            kind: Some("literal".to_string()),
            text: Some(text.into()),
            field: None,
            ref_kind: None,
            r#ref: None,
            page_field: None,
        }
    }

    /// Convenience constructor for a typed field run referencing a result UUID.
    pub fn result_field(field: FieldKind, result_id: Uuid) -> RunParam {
        RunParam {
            kind: Some("field".to_string()),
            text: None,
            field: Some(field.as_str().to_string()),
            ref_kind: Some("result".to_string()),
            r#ref: Some(result_id.to_string()),
            page_field: None,
        }
    }

    /// Convenience constructor for a header/footer page field run.
    pub fn page(pf: PageField) -> RunParam {
        RunParam {
            kind: Some("page".to_string()),
            text: None,
            field: None,
            ref_kind: None,
            r#ref: None,
            page_field: Some(pf.as_str().to_string()),
        }
    }

    fn from_cvalue(v: &CValue, idx: usize) -> Result<RunParam, TextBlockError> {
        let path = format!("/runs/{idx}");
        let CValue::Obj(o) = v else {
            return Err(TextBlockError::InvalidBlockPayload(PayloadError::new(&path, "not_object")));
        };
        let allowed = ["kind", "text", "field", "ref_kind", "ref", "page_field", "page"];
        if o.keys().any(|k| !allowed.contains(&k.as_str())) {
            return Err(TextBlockError::InvalidBlockPayload(PayloadError::new(&path, "unexpected_keys")));
        }
        let opt_str = |k: &str| -> Result<Option<String>, TextBlockError> {
            match o.get(k) {
                None | Some(CValue::Null) => Ok(None),
                Some(CValue::Str(s)) => Ok(Some(s.clone())),
                Some(_) => {
                    Err(TextBlockError::InvalidBlockPayload(PayloadError::new(&format!("{path}/{k}"), "not_string")))
                }
            }
        };
        let kind = opt_str("kind")?;
        let text = opt_str("text")?;
        let field = opt_str("field")?;
        let ref_kind = opt_str("ref_kind")?;
        let r_str = opt_str("ref")?;
        let page_field = match (opt_str("page_field")?, opt_str("page")?) {
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
            (Some(_), Some(_)) => {
                return Err(TextBlockError::InvalidBlockPayload(PayloadError::new(&path, "duplicate_page_field")));
            }
        };
        Ok(RunParam { kind, text, field, ref_kind, r#ref: r_str, page_field })
    }

    fn to_run(&self, idx: usize) -> Result<Run, TextBlockError> {
        let path = format!("/runs/{idx}");
        let effective_kind = match self.kind.as_deref() {
            Some(k) => k,
            None => {
                if self.text.is_some() && self.field.is_none() && self.page_field.is_none() {
                    "literal"
                } else if self.field.is_some() && self.text.is_none() && self.page_field.is_none() {
                    "field"
                } else if self.page_field.is_some() && self.text.is_none() && self.field.is_none() {
                    "page"
                } else {
                    return Err(TextBlockError::InvalidBlockPayload(PayloadError::new(&path, "ambiguous_run")));
                }
            }
        };
        match effective_kind {
            "literal" => {
                let Some(s) = &self.text else {
                    return Err(TextBlockError::InvalidBlockPayload(PayloadError::new(
                        &format!("{path}/text"),
                        "missing",
                    )));
                };
                if self.field.is_some() || self.r#ref.is_some() || self.page_field.is_some() {
                    return Err(TextBlockError::InvalidBlockPayload(PayloadError::new(&path, "unexpected_keys")));
                }
                let t = parse_text(s)?;
                Ok(Run::Literal(Untrusted::new(t)))
            }
            "field" => {
                if self.text.is_some() || self.page_field.is_some() {
                    return Err(TextBlockError::InvalidBlockPayload(PayloadError::new(&path, "unexpected_keys")));
                }
                let Some(fs) = &self.field else {
                    return Err(TextBlockError::InvalidBlockPayload(PayloadError::new(
                        &format!("{path}/field"),
                        "missing",
                    )));
                };
                let field = FieldKind::parse(fs).ok_or_else(|| {
                    TextBlockError::InvalidBlockPayload(PayloadError::new(&format!("{path}/field"), "unknown_field"))
                })?;
                let Some(rs) = &self.r#ref else {
                    return Err(TextBlockError::InvalidBlockPayload(PayloadError::new(
                        &format!("{path}/ref"),
                        "missing",
                    )));
                };
                let rk = self.ref_kind.as_deref().unwrap_or("result");
                let r = parse_ref_param(rk, rs, &format!("{path}/ref"))?;
                Ok(Run::Field { field, r })
            }
            "page" => {
                if self.text.is_some() || self.field.is_some() || self.r#ref.is_some() {
                    return Err(TextBlockError::InvalidBlockPayload(PayloadError::new(&path, "unexpected_keys")));
                }
                let Some(ps) = &self.page_field else {
                    return Err(TextBlockError::InvalidBlockPayload(PayloadError::new(
                        &format!("{path}/page_field"),
                        "missing",
                    )));
                };
                let pf = PageField::parse(ps).ok_or_else(|| {
                    TextBlockError::InvalidBlockPayload(PayloadError::new(
                        &format!("{path}/page_field"),
                        "unknown_page_field",
                    ))
                })?;
                Ok(Run::Page(pf))
            }
            _ => {
                Err(TextBlockError::InvalidBlockPayload(PayloadError::new(&format!("{path}/kind"), "unknown_run_kind")))
            }
        }
    }
}

fn parse_text(s: &str) -> Result<Text, TextBlockError> {
    Text::new(s).map_err(|e| match e {
        ReportError::TextInvalid(f) => TextBlockError::TextInvalid(f),
        other => TextBlockError::Report(other),
    })
}

fn parse_ref_param(kind: &str, id: &str, path: &str) -> Result<Ref, TextBlockError> {
    let rk = RefKind::parse(kind)
        .ok_or_else(|| TextBlockError::InvalidBlockPayload(PayloadError::new(path, "invalid_ref")))?;
    Ref::new(rk, id).ok_or_else(|| TextBlockError::InvalidBlockPayload(PayloadError::new(path, "invalid_ref")))
}

/// Parameters of `report.set_text_block@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct SetTextBlockParams {
    #[fid(description = "Report ID (UUIDv7).", pattern = ID_PATTERN)]
    pub report: String,
    #[fid(description = "Block ID (UUIDv7) of a text, header, or footer block.", pattern = ID_PATTERN)]
    pub block: String,
    #[fid(description = "Ordered list of literal, typed field, or page runs.", max_items = MAX_RUNS)]
    pub runs: Vec<RunParam>,
}

impl SetTextBlockParams {
    /// Strict `--json` / MCP parameter parser.
    pub fn from_value(v: &CValue) -> Result<SetTextBlockParams, TextBlockError> {
        let CValue::Obj(o) = v else {
            return Err(TextBlockError::BadParams("params is not an object"));
        };
        if o.keys().any(|k| !matches!(k.as_str(), "report" | "block" | "runs")) {
            return Err(TextBlockError::BadParams("unknown parameter"));
        }
        let get_str = |k: &str| match o.get(k) {
            Some(CValue::Str(s)) => Ok(s.clone()),
            _ => Err(TextBlockError::BadParams("report and block must be uuid strings")),
        };
        let report = get_str("report")?;
        let block = get_str("block")?;
        let Some(CValue::Arr(raw_runs)) = o.get("runs") else {
            return Err(TextBlockError::InvalidBlockPayload(PayloadError::new("/runs", "not_array")));
        };
        if raw_runs.len() > MAX_RUNS {
            return Err(TextBlockError::InvalidBlockPayload(PayloadError::new("/runs", "too_many_runs")));
        }
        let mut runs = Vec::with_capacity(raw_runs.len());
        for (i, item) in raw_runs.iter().enumerate() {
            runs.push(RunParam::from_cvalue(item, i)?);
        }
        Ok(SetTextBlockParams { report, block, runs })
    }
}

/// Result of `report.set_text_block@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct SetTextBlockResult {
    #[fid(description = "Updated block ID.", pattern = ID_PATTERN)]
    pub block_id: String,
    #[fid(description = "Number of runs stored in the block.")]
    pub run_count: u32,
    #[fid(description = "SHA-256 digest of the stored report after the command.")]
    pub report_digest: String,
}

impl SetTextBlockResult {
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("block_id".to_string(), CValue::Str(self.block_id.clone()));
        o.insert("report_digest".to_string(), CValue::Str(self.report_digest.clone()));
        o.insert("run_count".to_string(), CValue::U64(u64::from(self.run_count)));
        CValue::Obj(o)
    }
}

#[fid_command(
    id = "report.set_text_block",
    major = 1,
    title = "Set text, header or footer block",
    description = "Replaces a text, header or footer block's typed runs and journals one event.",
    side_effect = W2,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = None,
    fs_effect = None,
    undo = HeadMove,
    preconditions = [],
    effects = [SET_TEXT_BLOCK],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-22"),
    web = Available,
)]
pub fn set_text_block(
    ctx: &mut TextCmdCtx<'_>,
    params: SetTextBlockParams,
) -> Result<SetTextBlockResult, TextBlockError> {
    let report =
        parse_canonical_uuid(&params.report).ok_or(TextBlockError::BadParams("report is not a canonical uuid"))?;
    let block =
        parse_canonical_uuid(&params.block).ok_or(TextBlockError::BadParams("block is not a canonical uuid"))?;
    if params.runs.len() > MAX_RUNS {
        return Err(TextBlockError::InvalidBlockPayload(PayloadError::new("/runs", "too_many_runs")));
    }

    let mut runs = Vec::with_capacity(params.runs.len());
    for (i, rp) in params.runs.iter().enumerate() {
        runs.push(rp.to_run(i)?);
    }

    let rid = ReportId::new(report).ok_or(TextBlockError::NotFound(report))?;
    let stored = ctx.backend.load(&rid)?.ok_or(TextBlockError::NotFound(report))?;
    let current = stored.open(&rid, ctx.kinds)?;
    let existing = current.block(&block).ok_or(TextBlockError::NotFound(block))?;
    let Some(block_kind) = RunBlockKind::parse_kind(existing.kind().as_str()) else {
        return Err(TextBlockError::InvalidBlockPayload(PayloadError::new("/kind", "not_text_block")));
    };
    let bid = existing.id();

    let new_payload = RunBlockPayload::new(block_kind, runs).map_err(TextBlockError::InvalidBlockPayload)?;

    // Preserve any existing binding / placed value bindings on `text` blocks.
    let CValue::Obj(mut merged) = new_payload.to_value() else {
        return Err(TextBlockError::InvalidBlockPayload(PayloadError::new("", "not_object")));
    };
    if block_kind == RunBlockKind::Text {
        if let CValue::Obj(prev) = existing.payload().value() {
            if let Some(binding) = prev.get(BINDING_KEY) {
                merged.insert(BINDING_KEY.to_string(), binding.clone());
            }
            if let Some(vals) = prev.get("values") {
                merged.insert("values".to_string(), vals.clone());
            }
        }
    }
    let payload = CanonicalJson::new(&CValue::Obj(merged))
        .ok_or_else(|| TextBlockError::InvalidBlockPayload(PayloadError::new("", "payload_bounds")))?;

    let op = Op::SetTextBlock { report: rid, block: bid, payload };
    let next = op.apply(Some(current), ctx.kinds)?;
    let stored_next = StoredReport::of(&next)?;
    let digest = stored_next.digest;
    ctx.backend.commit(&rid, stored_next, ReportEvent::new(op, digest))?;

    Ok(SetTextBlockResult {
        block_id: bid.uuid().to_string(),
        run_count: u32::try_from(new_payload.runs.len()).unwrap_or(u32::MAX),
        report_digest: digest.as_str().to_string(),
    })
}

// ------------------------------------------------------------------ command report.set_image_block@1

/// Execution context for [`set_image_block`].
pub struct ImageCmdCtx<'a> {
    pub backend: &'a mut dyn ReportBackend,
    pub kinds: &'a KindRegistry,
    pub roots: &'a RootSet,
    pub principal: Principal,
    pub blobs: &'a mut dyn BlobSink,
}

/// Parameters of `report.set_image_block@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct SetImageBlockParams {
    #[fid(description = "Report ID (UUIDv7).", pattern = ID_PATTERN)]
    pub report: String,
    #[fid(description = "Image block ID (UUIDv7).", pattern = ID_PATTERN)]
    pub block: String,
    #[fid(
        description = "Image source: an import-root handle (root://<id>/<rel>), a human raw path, or an existing sha256: digest."
    )]
    pub source: String,
    #[fid(description = "Optional untrusted caption text shown with the image.")]
    pub caption: Option<String>,
    #[fid(description = "Required only when source is a pre-stored sha256: digest: image/png or image/jpeg.")]
    pub mime: Option<String>,
    #[fid(description = "Required only when source is a pre-stored sha256: digest: pixel width (> 0).")]
    pub width_px: Option<u32>,
    #[fid(description = "Required only when source is a pre-stored sha256: digest: pixel height (> 0).")]
    pub height_px: Option<u32>,
}

impl SetImageBlockParams {
    /// Convenience constructor for importing an image from a root handle or human path.
    pub fn from_source(
        report: ReportId,
        block: crate::doc::BlockId,
        source: impl Into<String>,
        caption: Option<String>,
    ) -> Self {
        SetImageBlockParams {
            report: report.uuid().to_string(),
            block: block.uuid().to_string(),
            source: source.into(),
            caption,
            mime: None,
            width_px: None,
            height_px: None,
        }
    }

    /// Strict `--json` / MCP parameter parser.
    pub fn from_value(v: &CValue) -> Result<SetImageBlockParams, TextBlockError> {
        let CValue::Obj(o) = v else {
            return Err(TextBlockError::BadParams("params is not an object"));
        };
        let allowed = ["report", "block", "source", "caption", "mime", "width_px", "height_px"];
        if o.keys().any(|k| !allowed.contains(&k.as_str())) {
            return Err(TextBlockError::BadParams("unknown parameter"));
        }
        let req_str = |k: &str| match o.get(k) {
            Some(CValue::Str(s)) => Ok(s.clone()),
            _ => Err(TextBlockError::BadParams("report, block and source must be strings")),
        };
        let opt_str = |k: &str| match o.get(k) {
            None | Some(CValue::Null) => Ok(None),
            Some(CValue::Str(s)) => Ok(Some(s.clone())),
            Some(_) => Err(TextBlockError::BadParams("optional text field must be string or null")),
        };
        let opt_u32 = |k: &str| match o.get(k) {
            None | Some(CValue::Null) => Ok(None),
            Some(CValue::U64(n)) => {
                u32::try_from(*n).map(Some).map_err(|_| TextBlockError::BadParams("dimension overflow"))
            }
            Some(CValue::I64(n)) => {
                u32::try_from(*n).map(Some).map_err(|_| TextBlockError::BadParams("dimension negative"))
            }
            Some(_) => Err(TextBlockError::BadParams("dimension must be integer")),
        };
        Ok(SetImageBlockParams {
            report: req_str("report")?,
            block: req_str("block")?,
            source: req_str("source")?,
            caption: opt_str("caption")?,
            mime: opt_str("mime")?,
            width_px: opt_u32("width_px")?,
            height_px: opt_u32("height_px")?,
        })
    }
}

/// Result of `report.set_image_block@1`.
#[derive(FidSchema, Debug, Clone, PartialEq, Eq)]
pub struct SetImageBlockResult {
    #[fid(description = "Updated image block ID.", pattern = ID_PATTERN)]
    pub block_id: String,
    #[fid(description = "Content-addressed SHA-256 digest of the stored image bytes.")]
    pub digest: String,
    #[fid(description = "Sniffed image MIME type (image/png or image/jpeg).")]
    pub mime: String,
    #[fid(description = "Image width in pixels.")]
    pub width_px: u32,
    #[fid(description = "Image height in pixels.")]
    pub height_px: u32,
    #[fid(description = "SHA-256 digest of the stored report after the command.")]
    pub report_digest: String,
}

impl SetImageBlockResult {
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("block_id".to_string(), CValue::Str(self.block_id.clone()));
        o.insert("digest".to_string(), CValue::Str(self.digest.clone()));
        o.insert("height_px".to_string(), CValue::U64(u64::from(self.height_px)));
        o.insert("mime".to_string(), CValue::Str(self.mime.clone()));
        o.insert("report_digest".to_string(), CValue::Str(self.report_digest.clone()));
        o.insert("width_px".to_string(), CValue::U64(u64::from(self.width_px)));
        CValue::Obj(o)
    }
}

fn read_and_store_image(
    roots: &RootSet,
    principal: Principal,
    source: &str,
    blobs: &mut dyn BlobSink,
) -> Result<(Digest, ImageInfo), TextBlockError> {
    let mut opened = roots.resolve_import(principal, source).map_err(|e| match e {
        fid_io_reg::IoError::RawPathForbidden => TextBlockError::RawPathForbidden,
        other => TextBlockError::ImportRoot(other.code()),
    })?;

    // Check file metadata size before allocating (REQ-004, CONSTITUTION §8).
    let meta = opened.file.metadata().map_err(|_| TextBlockError::ImportRoot("io"))?;
    if meta.len() > MAX_IMAGE_BYTES {
        return Err(TextBlockError::ImageTooLarge { bytes: meta.len(), limit: MAX_IMAGE_BYTES });
    }

    // Bounded read capped at MAX_IMAGE_BYTES + 1 in case file grew concurrently.
    let mut bytes = Vec::new();
    let cap_read = MAX_IMAGE_BYTES.saturating_add(1);
    let mut limited = (&mut opened.file).take(cap_read);
    let mut scratch = vec![0u8; 64 * 1024];
    loop {
        let k = match limited.read(&mut scratch) {
            Ok(0) => break,
            Ok(k) => k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(TextBlockError::ImportRoot("io")),
        };
        let next_len =
            u64::try_from(bytes.len()).unwrap_or(u64::MAX).saturating_add(u64::try_from(k).unwrap_or(u64::MAX));
        if next_len > MAX_IMAGE_BYTES {
            return Err(TextBlockError::ImageTooLarge { bytes: next_len, limit: MAX_IMAGE_BYTES });
        }
        bytes.try_reserve(k).map_err(|_| TextBlockError::ImportRoot("limit_exceeded"))?;
        bytes.extend_from_slice(&scratch[..k]);
    }

    let info = sniff_image(&bytes)?;
    let digest = blobs.put(&bytes).map_err(|e| TextBlockError::ImportRoot(e.code()))?;
    Ok((digest, info))
}

#[fid_command(
    id = "report.set_image_block",
    major = 1,
    title = "Set image block",
    description = "Reads a PNG or JPEG image from an import-root handle (at most 50 MiB), stores its bytes in CAS by digest, updates the image block and journals one event.",
    side_effect = W2,
    mcp_projection = Exposed,
    determinism = D0,
    idempotency = None,
    fs_effect = ReadRoot,
    undo = HeadMove,
    preconditions = [],
    effects = [SET_IMAGE_BLOCK],
    algorithms = [],
    exposure = [Cli, Mcp],
    lifecycle(state = "active", since = "2026-11-22"),
    web = Available,
)]
pub fn set_image_block(
    ctx: &mut ImageCmdCtx<'_>,
    params: SetImageBlockParams,
) -> Result<SetImageBlockResult, TextBlockError> {
    let report =
        parse_canonical_uuid(&params.report).ok_or(TextBlockError::BadParams("report is not a canonical uuid"))?;
    let block =
        parse_canonical_uuid(&params.block).ok_or(TextBlockError::BadParams("block is not a canonical uuid"))?;
    let caption = match &params.caption {
        Some(c) => Some(Untrusted::new(parse_text(c)?)),
        None => None,
    };

    // Resolve and validate the report/block first so errors on unknown IDs never touch roots.
    let rid = ReportId::new(report).ok_or(TextBlockError::NotFound(report))?;
    let stored = ctx.backend.load(&rid)?.ok_or(TextBlockError::NotFound(report))?;
    let current = stored.open(&rid, ctx.kinds)?;
    let existing = current.block(&block).ok_or(TextBlockError::NotFound(block))?;
    if existing.kind().as_str() != IMAGE_KIND_ID {
        return Err(TextBlockError::InvalidBlockPayload(PayloadError::new("/kind", "not_image_block")));
    }
    let bid = existing.id();

    let (digest, info) = if params.source.starts_with("sha256:") {
        let d = Digest::parse(&params.source).map_err(|_| TextBlockError::BadParams("invalid sha256 digest"))?;
        let mime = params
            .mime
            .as_deref()
            .and_then(ImageMime::parse)
            .ok_or(TextBlockError::ImageTypeUnsupported("missing_or_unsupported_mime"))?;
        let w = params.width_px.filter(|&n| n > 0).ok_or(TextBlockError::BadParams("missing width_px"))?;
        let h = params.height_px.filter(|&n| n > 0).ok_or(TextBlockError::BadParams("missing height_px"))?;
        (d, ImageInfo { mime, width_px: w, height_px: h })
    } else {
        read_and_store_image(ctx.roots, ctx.principal, &params.source, ctx.blobs)?
    };

    let image_payload = ImagePayload::new(digest, info.mime, info.width_px, info.height_px, caption)
        .map_err(TextBlockError::InvalidBlockPayload)?;
    let payload = image_payload.to_canonical().map_err(TextBlockError::InvalidBlockPayload)?;

    let op = Op::SetImageBlock { report: rid, block: bid, payload };
    let next = op.apply(Some(current), ctx.kinds)?;
    let stored_next = StoredReport::of(&next)?;
    let report_digest = stored_next.digest;
    ctx.backend.commit(&rid, stored_next, ReportEvent::new(op, report_digest))?;

    Ok(SetImageBlockResult {
        block_id: bid.uuid().to_string(),
        digest: digest.as_str().to_string(),
        mime: info.mime.as_str().to_string(),
        width_px: info.width_px,
        height_px: info.height_px,
        report_digest: report_digest.as_str().to_string(),
    })
}

fid_commands![set_text_block, set_image_block];

/// Registry command definitions exported by `RPT.MODEL.BLOCKS.TEXT`.
pub fn command_defs() -> Result<Vec<CommandDef>, DefError> {
    Ok(declarations().into_iter().map(|d| d.def).collect())
}

/// Executes `report.set_text_block@1` from canonical JSON parameters (`--json` / MCP).
pub fn execute_set_text_block(ctx: &mut TextCmdCtx<'_>, params: &CValue) -> Result<CValue, TextBlockError> {
    let parsed = SetTextBlockParams::from_value(params)?;
    let res = set_text_block(ctx, parsed)?;
    Ok(res.to_value())
}

/// Executes `report.set_image_block@1` from canonical JSON parameters (`--json` / MCP).
pub fn execute_set_image_block(ctx: &mut ImageCmdCtx<'_>, params: &CValue) -> Result<CValue, TextBlockError> {
    let parsed = SetImageBlockParams::from_value(params)?;
    let res = set_image_block(ctx, parsed)?;
    Ok(res.to_value())
}

/// CLI entry point for `fid report set-text-block --report <id> --block <id> --runs <json> --json`.
pub fn invoke_cli_set_text(ctx: &mut TextCmdCtx<'_>, args: &[&str]) -> Result<CValue, TextBlockError> {
    let mut report: Option<String> = None;
    let mut block: Option<String> = None;
    let mut runs_json: Option<String> = None;
    let mut json_flag = false;

    let mut i = 0usize;
    while i < args.len() {
        let Some(&flag) = args.get(i) else {
            break;
        };
        match flag {
            "--json" => {
                json_flag = true;
                i += 1;
            }
            "--report" => {
                report = args.get(i + 1).map(|s| (*s).to_string());
                i += 2;
            }
            "--block" => {
                block = args.get(i + 1).map(|s| (*s).to_string());
                i += 2;
            }
            "--runs" => {
                runs_json = args.get(i + 1).map(|s| (*s).to_string());
                i += 2;
            }
            _ => return Err(TextBlockError::BadParams("unknown cli flag")),
        }
    }
    if !json_flag {
        return Err(TextBlockError::BadParams("--json flag required"));
    }
    let Some(report) = report else {
        return Err(TextBlockError::BadParams("missing --report"));
    };
    let Some(block) = block else {
        return Err(TextBlockError::BadParams("missing --block"));
    };
    let Some(runs_str) = runs_json else {
        return Err(TextBlockError::BadParams("missing --runs"));
    };
    let runs_val = fid_canon::parse_with_limits(runs_str.as_bytes(), 1024 * 1024, 16)
        .map_err(|_| TextBlockError::BadParams("invalid --runs json"))?;
    let mut o = BTreeMap::new();
    o.insert("block".to_string(), CValue::Str(block));
    o.insert("report".to_string(), CValue::Str(report));
    o.insert("runs".to_string(), runs_val);
    execute_set_text_block(ctx, &CValue::Obj(o))
}

/// CLI entry point for `fid report set-image-block --report <id> --block <id> --source <handle> [--caption <text>] --json`.
pub fn invoke_cli_set_image(ctx: &mut ImageCmdCtx<'_>, args: &[&str]) -> Result<CValue, TextBlockError> {
    let mut report: Option<String> = None;
    let mut block: Option<String> = None;
    let mut source: Option<String> = None;
    let mut caption: Option<String> = None;
    let mut json_flag = false;

    let mut i = 0usize;
    while i < args.len() {
        let Some(&flag) = args.get(i) else {
            break;
        };
        match flag {
            "--json" => {
                json_flag = true;
                i += 1;
            }
            "--report" => {
                report = args.get(i + 1).map(|s| (*s).to_string());
                i += 2;
            }
            "--block" => {
                block = args.get(i + 1).map(|s| (*s).to_string());
                i += 2;
            }
            "--source" => {
                source = args.get(i + 1).map(|s| (*s).to_string());
                i += 2;
            }
            "--caption" => {
                caption = args.get(i + 1).map(|s| (*s).to_string());
                i += 2;
            }
            _ => return Err(TextBlockError::BadParams("unknown cli flag")),
        }
    }
    if !json_flag {
        return Err(TextBlockError::BadParams("--json flag required"));
    }
    let Some(report) = report else {
        return Err(TextBlockError::BadParams("missing --report"));
    };
    let Some(block) = block else {
        return Err(TextBlockError::BadParams("missing --block"));
    };
    let Some(source) = source else {
        return Err(TextBlockError::BadParams("missing --source"));
    };
    let mut o = BTreeMap::new();
    o.insert("block".to_string(), CValue::Str(block));
    o.insert("report".to_string(), CValue::Str(report));
    o.insert("source".to_string(), CValue::Str(source));
    if let Some(c) = caption {
        o.insert("caption".to_string(), CValue::Str(c));
    }
    execute_set_image_block(ctx, &CValue::Obj(o))
}
