//! Source provenance, unit and frame capture (spec node `IO.REG.PROV`).
//!
//! Every import builds one [`SourceProvenance`]: what was read ([`SourceDigest`] from a
//! [`HashingSource`]), how its format was chosen ([`Detection`], from the `IO.REG.DETECT`
//! [`Registry`]), which reader read it ([`ReaderInfo`]), who asked ([`PrincipalKind`], the
//! journal's principal kind), the effective options ([`EffectiveOptions`]), the effective
//! length unit ([`resolve_unit`]) and the source frame ([`FrameCapture`]). The record
//! serialises to RFC 8785 canonical JSON for the import journal event (`IO.REG.ENV`).
//!
//! - Imported content is never "measured": [`Evidence`] has exactly two values,
//!   `uncharacterized` (the default) and `claimed` (the file states an origin, kept as
//!   [`UntrustedText`]). Nothing here can mark imported data as measured, attested or
//!   characterized; that is decided elsewhere from evidence, never from a file's word.
//! - Every string taken from the file or its name ([`ProvenanceBuilder::source_name`],
//!   [`ProvenanceBuilder::origin_claim`], [`ProvenanceBuilder::declare`], a declared frame
//!   label) is [`UntrustedText`]; the human-readable form shows it only through
//!   [`UntrustedText::as_display`] inside escaped quotes.
//! - Duplicate keys and keys that differ only in ASCII case are refused, in the options at
//!   every depth and in the declared metadata.
//! - Every input is bounded (sizes, counts and depth below); nothing panics.
//!
//! The pinned wire form, field by field, is in `spec/io/reg/prov/NODE.md` (Design).

mod hash;
mod unit;

/// Re-exported so callers need no direct dependency for the unit, frame and principal types.
pub use fid_geo_frame::{FrameId, LengthUnit};
pub use fid_journal::PrincipalKind;
pub use hash::{HashingSource, SourceDigest};
pub use unit::{factor_decimal, resolve_unit, to_metres, NonFiniteLength, UnitResolution, UnitSource};

use crate::detect::{DetectError, Registry};
use crate::report::{is_format_id, FormatId, MAX_NAME_LEN};
use crate::untrusted::UntrustedText;
use fid_canon::{CValue, CanonError};
use fid_geo_frame::{Frame, FrameKind, Transform};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// The `schema` value of the canonical record.
pub const SCHEMA: &str = "source_provenance@1";
/// Domain tag of the synthesized source-frame ID (followed by one zero byte and the 32
/// digest bytes, then SHA-256, first 16 bytes).
pub const SOURCE_FRAME_TAG: &[u8] = b"fid.io.reg.prov.source_frame@1";
/// Hex digits of the source digest in a synthesized frame name `source:<hex>`.
pub const SOURCE_FRAME_HEX: usize = 12;
/// Largest canonical size of the effective options, in bytes.
pub const MAX_OPTIONS_BYTES: usize = 64 * 1024;
/// Deepest nesting of the effective options (the top-level object is depth 1).
pub const MAX_OPTIONS_DEPTH: usize = 32;
/// Most declared metadata items.
pub const MAX_DECLARED: usize = 256;
/// Longest untrusted text in the record (source name, origin claim, declared value), in
/// UTF-8 bytes. Longer text is refused, never cut: the record states exactly what was read.
pub const MAX_TEXT_BYTES: usize = 4096;
/// Longest declared frame label, in UTF-8 bytes (the frame tree's name limit).
pub const MAX_FRAME_LABEL_BYTES: usize = fid_geo_frame::tree::MAX_NAME_BYTES;
/// Longest version token.
pub const MAX_VERSION_LEN: usize = 32;

/// Why a provenance record could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvError {
    /// The same key appears twice (options JSON or declared metadata).
    DuplicateKey {
        /// Where.
        field: &'static str,
    },
    /// Two keys of one object differ only in ASCII case.
    CaseDuplicateKey {
        /// Where.
        field: &'static str,
    },
    /// A value breaks its pinned form.
    Invalid {
        /// The field.
        field: &'static str,
        /// Static reason (never input text).
        reason: &'static str,
    },
    /// A bound was exceeded (size, count or depth); nothing is truncated.
    TooLarge {
        /// The field.
        field: &'static str,
        /// The bound.
        limit: u64,
    },
}

impl ProvError {
    /// The stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            ProvError::DuplicateKey { .. } => "duplicate_key",
            ProvError::CaseDuplicateKey { .. } => "case_duplicate_key",
            ProvError::Invalid { .. } => "invalid",
            ProvError::TooLarge { .. } => "too_large",
        }
    }
}

impl fmt::Display for ProvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())?;
        match self {
            ProvError::DuplicateKey { field } | ProvError::CaseDuplicateKey { field } => write!(f, ": in {field}"),
            ProvError::Invalid { field, reason } => write!(f, ": {field}: {reason}"),
            ProvError::TooLarge { field, limit } => write!(f, ": {field} above {limit}"),
        }
    }
}

impl std::error::Error for ProvError {}

fn invalid(field: &'static str, reason: &'static str) -> ProvError {
    ProvError::Invalid { field, reason }
}

fn bounded_text(field: &'static str, text: UntrustedText, max: usize) -> Result<UntrustedText, ProvError> {
    if text.len() > max {
        return Err(ProvError::TooLarge { field, limit: max as u64 });
    }
    Ok(text)
}

// ------------------------------------------------------------------ names and versions

/// A reader ID: the same alphabet as [`FormatId`] (1 to 64 bytes of `a-z`, `0-9`, `-`, `_`
/// and `.`, starting with a letter or digit).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReaderId(String);

impl ReaderId {
    /// Checks and wraps a reader ID.
    pub fn new(s: &str) -> Result<ReaderId, ProvError> {
        if s.is_empty() || s.len() > MAX_NAME_LEN || !is_format_id(s) {
            return Err(invalid("reader", "invalid reader id"));
        }
        Ok(ReaderId(s.to_string()))
    }

    /// The ID.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A version token: 1 to [`MAX_VERSION_LEN`] bytes of ASCII letters, digits, `.`, `-` and
/// `+` (the `IO.REG.DETECT` token alphabet, without the `*` wildcard). Opaque: never ordered
/// or interpreted.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VersionToken(String);

impl VersionToken {
    /// Checks and wraps a version token.
    pub fn new(s: &str) -> Result<VersionToken, ProvError> {
        let ok = !s.is_empty()
            && s.len() <= MAX_VERSION_LEN
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'));
        if !ok {
            return Err(invalid("version", "invalid version token"));
        }
        Ok(VersionToken(s.to_string()))
    }

    /// The token.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The reader that read the source, and its version.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReaderInfo {
    /// Reader ID.
    pub id: ReaderId,
    /// Reader version.
    pub version: VersionToken,
}

// ------------------------------------------------------------------ detection

/// How the format was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FormatSelection {
    /// The `IO.REG.DETECT` content signatures picked it.
    ContentSignature,
    /// The caller named it explicitly (content and extension were not examined).
    Caller,
}

impl FormatSelection {
    /// The stable wire name: `content_signature` or `caller`.
    pub fn as_str(self) -> &'static str {
        match self {
            FormatSelection::ContentSignature => "content_signature",
            FormatSelection::Caller => "caller",
        }
    }
}

/// The detector's result for one import: the format and how it was chosen.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Detection {
    format: FormatId,
    selected_by: FormatSelection,
}

impl Detection {
    /// Runs [`Registry::detect`] and records its result: [`FormatSelection::Caller`] when
    /// `named` is given, [`FormatSelection::ContentSignature`] otherwise. Detection errors
    /// pass through unchanged.
    pub fn detect(
        registry: &Registry,
        prefix: &[u8],
        total_len: u64,
        file_name: Option<&str>,
        named: Option<&FormatId>,
    ) -> Result<Detection, DetectError> {
        let format = registry.detect(prefix, total_len, file_name, named)?;
        let selected_by = if named.is_some() { FormatSelection::Caller } else { FormatSelection::ContentSignature };
        Ok(Detection { format, selected_by })
    }

    /// The format.
    pub fn format(&self) -> &FormatId {
        &self.format
    }

    /// How it was chosen.
    pub fn selected_by(&self) -> FormatSelection {
        self.selected_by
    }
}

// ------------------------------------------------------------------ options

/// The effective options of one import, after defaults: a JSON object, at most
/// [`MAX_OPTIONS_BYTES`] canonical bytes and [`MAX_OPTIONS_DEPTH`] deep, with no two keys
/// in any object that are equal or differ only in ASCII case.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveOptions(CValue);

impl EffectiveOptions {
    /// An empty options object.
    pub fn empty() -> EffectiveOptions {
        EffectiveOptions(CValue::Obj(BTreeMap::new()))
    }

    /// Checks a value.
    pub fn new(value: CValue) -> Result<EffectiveOptions, ProvError> {
        if !matches!(value, CValue::Obj(_)) {
            return Err(invalid("options", "options must be an object"));
        }
        check_options_tree(&value)?;
        match fid_canon::to_canonical_bytes(&value) {
            Ok(b) if b.len() <= MAX_OPTIONS_BYTES => Ok(EffectiveOptions(value)),
            Ok(_) => Err(ProvError::TooLarge { field: "options", limit: MAX_OPTIONS_BYTES as u64 }),
            Err(_) => Err(invalid("options", "options are not canonical JSON")),
        }
    }

    /// Parses JSON text (at most [`MAX_OPTIONS_BYTES`] bytes). A key repeated in one object
    /// is `duplicate_key`; keys differing only in ASCII case are `case_duplicate_key`.
    pub fn from_json(json: &[u8]) -> Result<EffectiveOptions, ProvError> {
        if json.len() > MAX_OPTIONS_BYTES {
            return Err(ProvError::TooLarge { field: "options", limit: MAX_OPTIONS_BYTES as u64 });
        }
        // fid-canon's own depth cap is above ours; new() applies MAX_OPTIONS_DEPTH.
        match fid_canon::parse_with_limits(json, MAX_OPTIONS_BYTES, fid_canon::MAX_DEPTH) {
            Ok(v) => EffectiveOptions::new(v),
            Err(CanonError::DuplicateKey(_)) => Err(ProvError::DuplicateKey { field: "options" }),
            Err(CanonError::InputLimit) => {
                Err(ProvError::TooLarge { field: "options", limit: MAX_OPTIONS_DEPTH as u64 })
            }
            Err(_) => Err(invalid("options", "options are not JSON")),
        }
    }

    /// The options value.
    pub fn value(&self) -> &CValue {
        &self.0
    }
}

/// Walks the tree iteratively: depth bound, and no case-only duplicate keys in any object.
fn check_options_tree(root: &CValue) -> Result<(), ProvError> {
    let mut stack: Vec<(&CValue, usize)> = vec![(root, 1)];
    while let Some((v, depth)) = stack.pop() {
        let children: Box<dyn Iterator<Item = &CValue>> = match v {
            CValue::Obj(map) => {
                let mut folded = BTreeSet::new();
                for k in map.keys() {
                    if !folded.insert(k.to_ascii_lowercase()) {
                        return Err(ProvError::CaseDuplicateKey { field: "options" });
                    }
                }
                Box::new(map.values())
            }
            CValue::Arr(items) => Box::new(items.iter()),
            _ => continue,
        };
        if depth > MAX_OPTIONS_DEPTH {
            return Err(ProvError::TooLarge { field: "options", limit: MAX_OPTIONS_DEPTH as u64 });
        }
        for c in children {
            stack.push((c, depth + 1));
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ frames and evidence

/// The frame the file's coordinates are expressed in (REQ-IO.REG.PROV-005).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameCapture {
    /// The format carries a frame, already resolved to an ID by the reader or the envelope;
    /// its label, if any, is file text.
    Declared {
        /// The frame ID.
        id: FrameId,
        /// The file's label for it.
        label: Option<UntrustedText>,
    },
    /// The format carries no frame: a new frame `source:<first 12 hex of the digest>` at the
    /// identity transform of the document's world frame ([`SourceProvenance::source_frame`]).
    Synthesized,
}

/// The evidence class of imported content. There is no "measured" value: an import alone
/// never makes data measured, whatever the file says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Evidence {
    /// Nothing is known about where the content came from (the default).
    Uncharacterized,
    /// The file states an origin (a device, a program, "measured on ..."); the statement is
    /// untrusted text and is not checked.
    Claimed,
}

impl Evidence {
    /// The stable wire name: `uncharacterized` or `claimed`.
    pub fn as_str(self) -> &'static str {
        match self {
            Evidence::Uncharacterized => "uncharacterized",
            Evidence::Claimed => "claimed",
        }
    }
}

fn is_meta_key(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_NAME_LEN
        && s.split('.').all(|seg| {
            let b = seg.as_bytes();
            matches!(b.first(), Some(b'a'..=b'z')) && b.iter().all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'_'))
        })
}

/// The ID of the synthesized source frame for a source digest: the first 16 bytes of
/// SHA-256([`SOURCE_FRAME_TAG`] || 0x00 || the 32 digest bytes).
pub fn source_frame_id(sha256: &Digest) -> FrameId {
    let mut h = Sha256::new();
    h.update(SOURCE_FRAME_TAG);
    h.update([0u8]);
    h.update(sha256.to_bytes());
    let full: [u8; 32] = h.finalize().into();
    let mut id = [0u8; 16];
    for (o, b) in id.iter_mut().zip(full.iter()) {
        *o = *b;
    }
    FrameId::from_bytes(id)
}

/// The name of the synthesized source frame: `source:` and the first
/// [`SOURCE_FRAME_HEX`] hex digits of the digest.
pub fn source_frame_name(sha256: &Digest) -> String {
    let hex = sha256.hex();
    format!("source:{}", hex.get(..SOURCE_FRAME_HEX).unwrap_or(hex))
}

// ------------------------------------------------------------------ the record

/// The provenance record of one successful import. Built only through
/// [`ProvenanceBuilder`], so every field has passed its check.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceProvenance {
    source: SourceDigest,
    source_name: Option<UntrustedText>,
    detection: Detection,
    format_version: Option<VersionToken>,
    reader: ReaderInfo,
    principal: PrincipalKind,
    options: EffectiveOptions,
    unit: UnitResolution,
    frame: FrameCapture,
    origin_claim: Option<UntrustedText>,
    declared: BTreeMap<String, UntrustedText>,
}

/// Collects the parts of a [`SourceProvenance`].
#[derive(Debug, Clone)]
pub struct ProvenanceBuilder {
    rec: SourceProvenance,
}

impl SourceProvenance {
    /// Starts a record with every required part. The frame defaults to
    /// [`FrameCapture::Synthesized`] and the evidence to `uncharacterized`.
    pub fn builder(
        source: SourceDigest,
        detection: Detection,
        reader: ReaderInfo,
        principal: PrincipalKind,
        options: EffectiveOptions,
        unit: UnitResolution,
    ) -> ProvenanceBuilder {
        ProvenanceBuilder {
            rec: SourceProvenance {
                source,
                source_name: None,
                detection,
                format_version: None,
                reader,
                principal,
                options,
                unit,
                frame: FrameCapture::Synthesized,
                origin_claim: None,
                declared: BTreeMap::new(),
            },
        }
    }

    /// Digest and size of the whole source.
    pub fn source(&self) -> SourceDigest {
        self.source
    }

    /// The source file name, as untrusted text.
    pub fn source_name(&self) -> Option<&UntrustedText> {
        self.source_name.as_ref()
    }

    /// The detector's result.
    pub fn detection(&self) -> &Detection {
        &self.detection
    }

    /// The format version the reader identified, if any.
    pub fn format_version(&self) -> Option<&VersionToken> {
        self.format_version.as_ref()
    }

    /// The reader.
    pub fn reader(&self) -> &ReaderInfo {
        &self.reader
    }

    /// The principal kind of the import command.
    pub fn principal(&self) -> PrincipalKind {
        self.principal
    }

    /// The effective options.
    pub fn options(&self) -> &EffectiveOptions {
        &self.options
    }

    /// The effective unit.
    pub fn unit(&self) -> &UnitResolution {
        &self.unit
    }

    /// The source frame as captured.
    pub fn frame(&self) -> &FrameCapture {
        &self.frame
    }

    /// The source frame ID: the declared one, or [`source_frame_id`] of the digest.
    pub fn frame_id(&self) -> FrameId {
        match &self.frame {
            FrameCapture::Declared { id, .. } => *id,
            FrameCapture::Synthesized => source_frame_id(&self.source.sha256()),
        }
    }

    /// The frame to add to the document when the format carried none: named
    /// `source:<12 hex>`, kind [`FrameKind::User`], child of the world frame
    /// ([`FrameId::WORLD`]) at the identity transform. `None` for a declared frame.
    pub fn source_frame(&self) -> Option<Frame> {
        match self.frame {
            FrameCapture::Declared { .. } => None,
            FrameCapture::Synthesized => Some(Frame {
                id: self.frame_id(),
                name: source_frame_name(&self.source.sha256()),
                kind: FrameKind::User,
                parent: Some(FrameId::WORLD),
                to_parent: Transform::IDENTITY,
            }),
        }
    }

    /// `claimed` when the file stated an origin, `uncharacterized` otherwise.
    pub fn evidence(&self) -> Evidence {
        if self.origin_claim.is_some() {
            Evidence::Claimed
        } else {
            Evidence::Uncharacterized
        }
    }

    /// The file's own statement of origin, as untrusted text.
    pub fn origin_claim(&self) -> Option<&UntrustedText> {
        self.origin_claim.as_ref()
    }

    /// Metadata the file declared (header text and the like), keyed by reader-chosen keys.
    pub fn declared(&self) -> &BTreeMap<String, UntrustedText> {
        &self.declared
    }

    /// The canonical JSON value (pinned in `spec/io/reg/prov/NODE.md` Design). Every key is
    /// always present; absent values are `null`; the byte size is a decimal string.
    pub fn to_cvalue(&self) -> CValue {
        let s = |v: &str| CValue::Str(v.to_string());
        let opt_text = |t: Option<&UntrustedText>| t.map_or(CValue::Null, |t| s(t.raw()));
        let (frame_name, frame_origin) = match &self.frame {
            FrameCapture::Declared { label, .. } => (opt_text(label.as_ref()), "declared"),
            FrameCapture::Synthesized => (s(&source_frame_name(&self.source.sha256())), "synthesized"),
        };
        let declared = self.declared.iter().map(|(k, v)| (k.clone(), s(v.raw()))).collect();
        let fields = [
            ("schema", s(SCHEMA)),
            ("sha256", s(self.source.sha256().as_str())),
            ("bytes", CValue::U64(self.source.bytes())),
            ("source_name", opt_text(self.source_name.as_ref())),
            ("format", s(self.detection.format.as_str())),
            ("format_version", self.format_version.as_ref().map_or(CValue::Null, |v| s(v.as_str()))),
            ("format_selected_by", s(self.detection.selected_by.as_str())),
            ("reader", s(self.reader.id.as_str())),
            ("reader_version", s(self.reader.version.as_str())),
            ("principal", s(self.principal.as_str())),
            ("options", self.options.0.clone()),
            ("unit", s(self.unit.unit().symbol())),
            ("unit_factor", s(self.unit.factor_decimal())),
            ("unit_source", s(self.unit.source().as_str())),
            ("unit_overridden", CValue::Bool(self.unit.overridden())),
            ("frame", s(&self.frame_id().to_string())),
            ("frame_name", frame_name),
            ("frame_origin", s(frame_origin)),
            ("evidence", s(self.evidence().as_str())),
            ("origin_claim", opt_text(self.origin_claim.as_ref())),
            ("declared", CValue::Obj(declared)),
        ];
        CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    /// RFC 8785 canonical bytes of [`SourceProvenance::to_cvalue`].
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ProvError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue()).map_err(|_| invalid("provenance", "not canonical JSON"))
    }

    /// SHA-256 of the canonical bytes.
    pub fn digest(&self) -> Result<Digest, ProvError> {
        let bytes = self.canonical_bytes()?;
        Ok(Digest::from_bytes(Sha256::digest(&bytes).into()))
    }
}

impl ProvenanceBuilder {
    /// The source file name (untrusted; at most [`MAX_TEXT_BYTES`]).
    pub fn source_name(&mut self, name: UntrustedText) -> Result<&mut Self, ProvError> {
        self.rec.source_name = Some(bounded_text("source_name", name, MAX_TEXT_BYTES)?);
        Ok(self)
    }

    /// The format version the reader identified.
    pub fn format_version(&mut self, version: VersionToken) -> &mut Self {
        self.rec.format_version = Some(version);
        self
    }

    /// The source frame (default [`FrameCapture::Synthesized`]). A declared label is at most
    /// [`MAX_FRAME_LABEL_BYTES`].
    pub fn frame(&mut self, frame: FrameCapture) -> Result<&mut Self, ProvError> {
        if let FrameCapture::Declared { label: Some(l), .. } = &frame {
            if l.len() > MAX_FRAME_LABEL_BYTES {
                return Err(ProvError::TooLarge { field: "frame_name", limit: MAX_FRAME_LABEL_BYTES as u64 });
            }
        }
        self.rec.frame = frame;
        Ok(self)
    }

    /// The file's statement of where its content came from; makes the evidence `claimed`.
    /// A second call is refused (`duplicate_key`).
    pub fn origin_claim(&mut self, claim: UntrustedText) -> Result<&mut Self, ProvError> {
        if self.rec.origin_claim.is_some() {
            return Err(ProvError::DuplicateKey { field: "origin_claim" });
        }
        self.rec.origin_claim = Some(bounded_text("origin_claim", claim, MAX_TEXT_BYTES)?);
        Ok(self)
    }

    /// One item of declared file metadata. `key` is chosen by the reader: one or more
    /// lowercase snake_case segments joined by `.`, at most 64 bytes (so keys never differ
    /// only in case). A repeated key is `duplicate_key`; at most [`MAX_DECLARED`] items.
    pub fn declare(&mut self, key: &str, value: UntrustedText) -> Result<&mut Self, ProvError> {
        if !is_meta_key(key) {
            return Err(invalid("declared", "invalid key"));
        }
        if self.rec.declared.contains_key(key) {
            return Err(ProvError::DuplicateKey { field: "declared" });
        }
        if self.rec.declared.len() >= MAX_DECLARED {
            return Err(ProvError::TooLarge { field: "declared", limit: MAX_DECLARED as u64 });
        }
        let value = bounded_text("declared", value, MAX_TEXT_BYTES)?;
        self.rec.declared.insert(key.to_string(), value);
        Ok(self)
    }

    /// The finished record.
    pub fn build(&self) -> SourceProvenance {
        self.rec.clone()
    }
}

/// Writes untrusted text in double quotes: escaped by [`UntrustedText::as_display`], and `"`
/// written as `\"`, so the text can neither break the line nor close its quotes.
struct Quoted<'a>(&'a UntrustedText);

impl fmt::Display for Quoted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let shown = self.0.as_display().to_string();
        write!(f, "\"{}\"", shown.replace('"', "\\\""))
    }
}

impl fmt::Display for SourceProvenance {
    /// One line per field, for people. File-derived text only appears quoted and escaped.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "source: {} ({} bytes)", self.source.sha256().as_str(), self.source.bytes())?;
        if let Some(n) = &self.source_name {
            writeln!(f, "name: {}", Quoted(n))?;
        }
        write!(f, "format: {} ({})", self.detection.format, self.detection.selected_by.as_str())?;
        if let Some(v) = &self.format_version {
            write!(f, " version {}", v.as_str())?;
        }
        writeln!(f)?;
        writeln!(f, "reader: {} {}", self.reader.id.as_str(), self.reader.version.as_str())?;
        writeln!(f, "principal: {}", self.principal.as_str())?;
        writeln!(
            f,
            "unit: {} (factor {}, {})",
            self.unit.unit().symbol(),
            self.unit.factor_decimal(),
            self.unit.source().as_str()
        )?;
        match &self.frame {
            FrameCapture::Declared { id, label: Some(l) } => writeln!(f, "frame: {id} declared {}", Quoted(l))?,
            FrameCapture::Declared { id, label: None } => writeln!(f, "frame: {id} declared")?,
            FrameCapture::Synthesized => {
                writeln!(f, "frame: {} {} (synthesized)", self.frame_id(), source_frame_name(&self.source.sha256()))?
            }
        }
        match &self.origin_claim {
            Some(c) => writeln!(f, "evidence: claimed {}", Quoted(c))?,
            None => writeln!(f, "evidence: uncharacterized")?,
        }
        for (k, v) in &self.declared {
            writeln!(f, "declared {k}: {}", Quoted(v))?;
        }
        Ok(())
    }
}
