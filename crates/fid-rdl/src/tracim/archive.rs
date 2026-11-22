//! Verbatim archive of TraCIM exchanges and reports (spec node `UNC.RDL.TRACIM.ARCH`).
//!
//! Stores every TraCIM request and response byte for byte in a content-addressed store
//! (`CORE.STO.CAS`), with a canonical `tracim-archive/v1` index entry. Protocol-marked secret
//! spans are replaced by [`REDACTION_MARKER`] before storage, and the stored digest is of the
//! redacted bytes (REQ-UNC.RDL.TRACIM.ARCH-001). Returned reports are stored unmodified so
//! their stored digest equals the digest of the received bytes (REQ-002). If the in-memory
//! order key's bytes still appear anywhere in the bytes to be stored, archiving fails closed
//! with [`ArchiveError::SecretInArchive`] (`secret_in_archive`) and stores nothing (REQ-003).
//!
//! Fetched test data is converted into validated Tier X `reference_only` [`RdlDataset`]
//! records via [`fetched_test_data_to_record`].

use std::collections::BTreeMap;
use std::fmt;

use fid_canon::{digest_bytes, to_canonical_bytes, CValue};
use fid_types::Digest;

use crate::record::{is_control_or_format, BlobRef, RdlDataset, RefOutput, Tier, Visibility, SCHEMA};
use crate::RdlError;

/// Schema identifier carried by every archive index entry.
pub const INDEX_SCHEMA: &str = "tracim-archive/v1";
/// Fixed replacement marker substituted for every protocol-marked secret span.
pub const REDACTION_MARKER: &[u8] = b"[REDACTED]";
/// Maximum accepted raw exchange payload size, in bytes (64 MiB).
pub const MAX_EXCHANGE_BYTES: usize = 64 * 1024 * 1024;
/// Maximum number of protocol-marked secret spans in a single exchange.
pub const MAX_SECRET_SPANS: usize = 1024;
/// Maximum metadata string length (timestamp or destination), in bytes.
pub const MAX_META_BYTES: usize = 512;

/// Direction / role of an archived TraCIM artefact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Direction {
    Request,
    Response,
    Report,
}

impl Direction {
    /// Stable wire representation.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Request => "request",
            Direction::Response => "response",
            Direction::Report => "report",
        }
    }

    /// Parses the exact wire string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "request" => Some(Direction::Request),
            "response" => Some(Direction::Response),
            "report" => Some(Direction::Report),
            _ => None,
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Half-open byte span `[start, end)` inside [`Exchange::bytes`] holding secret material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByteSpan {
    pub start: usize,
    pub end: usize,
}

impl ByteSpan {
    /// Constructs a half-open byte span `[start, end)`.
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// One TraCIM request, response, or returned report to archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exchange {
    pub direction: Direction,
    /// Explicit UTC timestamp (caller-provided for D0 determinism; never reads a clock).
    pub timestamp: String,
    /// Destination service / endpoint identifier.
    pub destination: String,
    /// Raw exchange or report bytes.
    pub bytes: Vec<u8>,
    /// Protocol-marked byte spans containing secrets (must be empty for [`Direction::Report`]).
    pub secret_spans: Vec<ByteSpan>,
}

/// One `tracim-archive/v1` index entry recording an archived artefact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveIndexEntry {
    pub schema: String,
    pub direction: Direction,
    pub timestamp: String,
    pub destination: String,
    pub digest: Digest,
    pub byte_len: u64,
    pub redacted_spans: usize,
}

impl ArchiveIndexEntry {
    /// Canonical JSON value representation (`tracim-archive/v1`).
    #[must_use]
    pub fn to_value(&self) -> CValue {
        let mut obj = BTreeMap::new();
        obj.insert("byte_len".to_string(), CValue::Num(self.byte_len as f64));
        obj.insert("destination".to_string(), CValue::Str(self.destination.clone()));
        obj.insert("digest".to_string(), CValue::Str(self.digest.as_str().to_string()));
        obj.insert("direction".to_string(), CValue::Str(self.direction.as_str().to_string()));
        obj.insert("redacted_spans".to_string(), CValue::Num(self.redacted_spans as f64));
        obj.insert("schema".to_string(), CValue::Str(self.schema.clone()));
        obj.insert("timestamp".to_string(), CValue::Str(self.timestamp.clone()));
        CValue::Obj(obj)
    }

    /// RFC 8785 canonical JSON bytes of this index entry.
    pub fn canonical_json(&self) -> Result<Vec<u8>, ArchiveError> {
        to_canonical_bytes(&self.to_value()).map_err(|e| ArchiveError::Store(format!("canonicalise index entry: {e}")))
    }

    /// SHA-256 digest of the canonical index entry.
    pub fn entry_digest(&self) -> Result<Digest, ArchiveError> {
        let bytes = self.canonical_json()?;
        Ok(Digest::from_bytes(digest_bytes(&bytes).0))
    }
}

/// Reference returned by [`archive`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveRef {
    /// SHA-256 digest of the stored bytes in the content-addressed store.
    pub digest: Digest,
    /// Appended `tracim-archive/v1` index entry.
    pub entry: ArchiveIndexEntry,
    /// SHA-256 digest of the canonical `tracim-archive/v1` index entry.
    pub entry_digest: Digest,
}

/// Typed archive errors. Error messages never contain secret key bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveError {
    /// The order key's byte sequence was found after redaction; nothing was stored (REQ-003).
    SecretInArchive,
    /// A secret span is empty or out of bounds of `exchange.bytes`.
    InvalidSpan { start: usize, end: usize, len: usize },
    /// Two protocol-marked secret spans overlap.
    OverlappingSpans,
    /// A report carries secret spans (reports must be archived unmodified, REQ-002).
    ReportHasSecretSpans,
    /// The supplied order key is empty when redacting a request/response.
    EmptyKey,
    /// Timestamp or destination is empty, oversized, or contains control characters.
    InvalidMetadata(String),
    /// Payload exceeds [`MAX_EXCHANGE_BYTES`] or span list exceeds [`MAX_SECRET_SPANS`].
    TooLarge { len: usize, max: usize },
    /// Content-addressed store I/O or integrity error.
    Store(String),
    /// RDL record validation error when constructing a Tier X `reference_only` record.
    Record(RdlError),
}

impl ArchiveError {
    /// Stable machine-readable error code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            ArchiveError::SecretInArchive => "secret_in_archive",
            ArchiveError::InvalidSpan { .. } => "invalid_span",
            ArchiveError::OverlappingSpans => "overlapping_spans",
            ArchiveError::ReportHasSecretSpans => "report_has_secret_spans",
            ArchiveError::EmptyKey => "empty_key",
            ArchiveError::InvalidMetadata(_) => "invalid_metadata",
            ArchiveError::TooLarge { .. } => "too_large",
            ArchiveError::Store(_) => "store_error",
            ArchiveError::Record(_) => "record_error",
        }
    }
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArchiveError::SecretInArchive => f.write_str("secret_in_archive: order key bytes remained after redaction"),
            ArchiveError::InvalidSpan { start, end, len } => {
                write!(f, "invalid_span: [{start}..{end}) out of bounds for {len}-byte payload")
            }
            ArchiveError::OverlappingSpans => f.write_str("overlapping_spans: secret spans must not overlap"),
            ArchiveError::ReportHasSecretSpans => {
                f.write_str("report_has_secret_spans: returned reports are stored unmodified")
            }
            ArchiveError::EmptyKey => f.write_str("empty_key: order key must not be empty"),
            ArchiveError::InvalidMetadata(msg) => write!(f, "invalid_metadata: {msg}"),
            ArchiveError::TooLarge { len, max } => write!(f, "too_large: {len} exceeds maximum {max}"),
            ArchiveError::Store(msg) => write!(f, "store_error: {msg}"),
            ArchiveError::Record(err) => write!(f, "record_error: {err}"),
        }
    }
}

impl std::error::Error for ArchiveError {}

impl From<RdlError> for ArchiveError {
    fn from(err: RdlError) -> Self {
        ArchiveError::Record(err)
    }
}

/// Content-addressed byte store abstraction (backed by `fid_store::BlobStore` with the `store`
/// feature, or [`MemoryContentStore`] on wasm32 and in unit tests).
pub trait ContentStore {
    /// Stores `bytes` content-addressed by SHA-256 and returns its digest.
    fn put(&mut self, bytes: &[u8]) -> Result<Digest, ArchiveError>;
    /// Retrieves bytes by digest, or `None` if absent.
    fn get(&self, digest: &Digest) -> Result<Option<Vec<u8>>, ArchiveError>;
}

/// Deterministic in-memory [`ContentStore`] (D0, wasm32-compatible).
#[derive(Debug, Default, Clone)]
pub struct MemoryContentStore {
    blobs: BTreeMap<Digest, Vec<u8>>,
}

impl MemoryContentStore {
    /// Creates an empty in-memory content store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of distinct blobs currently stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.blobs.len()
    }

    /// Whether the store is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blobs.is_empty()
    }
}

impl ContentStore for MemoryContentStore {
    fn put(&mut self, bytes: &[u8]) -> Result<Digest, ArchiveError> {
        let digest = Digest::from_bytes(digest_bytes(bytes).0);
        self.blobs.entry(digest).or_insert_with(|| bytes.to_vec());
        Ok(digest)
    }

    fn get(&self, digest: &Digest) -> Result<Option<Vec<u8>>, ArchiveError> {
        Ok(self.blobs.get(digest).cloned())
    }
}

/// Adapter backing [`ContentStore`] with `CORE.STO.CAS` (`fid_store::BlobStore`).
#[cfg(feature = "store")]
pub struct BlobContentStore<'a> {
    store: &'a fid_store::BlobStore,
}

#[cfg(feature = "store")]
impl<'a> BlobContentStore<'a> {
    /// Wraps an open [`fid_store::BlobStore`].
    #[must_use]
    pub fn new(store: &'a fid_store::BlobStore) -> Self {
        Self { store }
    }
}

#[cfg(feature = "store")]
impl ContentStore for BlobContentStore<'_> {
    fn put(&mut self, bytes: &[u8]) -> Result<Digest, ArchiveError> {
        let addr = self
            .store
            .put(bytes, fid_store::PutOpts { durable: true })
            .map_err(|e| ArchiveError::Store(format!("blob put failed: {e}")))?;
        Ok(addr.digest())
    }

    fn get(&self, digest: &Digest) -> Result<Option<Vec<u8>>, ArchiveError> {
        match self.store.read_object(digest, MAX_EXCHANGE_BYTES as u64) {
            Ok(buf) => Ok(Some(buf)),
            Err(fid_store::CasError::NotFound(_)) => Ok(None),
            Err(e) => Err(ArchiveError::Store(format!("blob get failed: {e}"))),
        }
    }
}

/// Append-only in-memory sequence of `tracim-archive/v1` index entries.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ArchiveIndex {
    entries: Vec<ArchiveIndexEntry>,
}

impl ArchiveIndex {
    /// Creates an empty archive index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// All recorded index entries in archive order.
    #[must_use]
    pub fn entries(&self) -> &[ArchiveIndexEntry] {
        &self.entries
    }

    /// Number of index entries recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the index is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Encodes all index entries as LF-terminated canonical JSON Lines.
    pub fn to_jsonl(&self) -> Result<Vec<u8>, ArchiveError> {
        let mut out = Vec::new();
        for entry in &self.entries {
            let line = entry.canonical_json()?;
            out.extend_from_slice(&line);
            out.push(b'\n');
        }
        Ok(out)
    }
}

/// Archives one TraCIM request, response, or report into `store` and appends a
/// `tracim-archive/v1` entry to `index` (REQ-UNC.RDL.TRACIM.ARCH-001..003).
///
/// If `key` bytes remain anywhere in the payload after redacting `exchange.secret_spans`,
/// fails closed with [`ArchiveError::SecretInArchive`] without writing to `store` or `index`.
pub fn archive<S: ContentStore>(
    store: &mut S,
    index: &mut ArchiveIndex,
    exchange: &Exchange,
    key: &[u8],
) -> Result<ArchiveRef, ArchiveError> {
    validate_meta(&exchange.timestamp, "timestamp")?;
    validate_meta(&exchange.destination, "destination")?;
    if exchange.bytes.len() > MAX_EXCHANGE_BYTES {
        return Err(ArchiveError::TooLarge { len: exchange.bytes.len(), max: MAX_EXCHANGE_BYTES });
    }
    if exchange.secret_spans.len() > MAX_SECRET_SPANS {
        return Err(ArchiveError::TooLarge { len: exchange.secret_spans.len(), max: MAX_SECRET_SPANS });
    }

    let (to_store, redacted_spans) = match exchange.direction {
        Direction::Report => {
            if !exchange.secret_spans.is_empty() {
                return Err(ArchiveError::ReportHasSecretSpans);
            }
            (exchange.bytes.clone(), 0usize)
        }
        Direction::Request | Direction::Response => {
            if key.is_empty() {
                return Err(ArchiveError::EmptyKey);
            }
            let redacted = redact_spans(&exchange.bytes, &exchange.secret_spans)?;
            (redacted, exchange.secret_spans.len())
        }
    };

    // REQ-003: sentinel scan — never store if the in-memory order key appears anywhere.
    if !key.is_empty() && contains_subslice(&to_store, key) {
        return Err(ArchiveError::SecretInArchive);
    }

    let expected_digest = Digest::from_bytes(digest_bytes(&to_store).0);
    let stored_digest = store.put(&to_store)?;
    if stored_digest != expected_digest {
        return Err(ArchiveError::Store(format!(
            "store returned digest {stored_digest} != expected {expected_digest}"
        )));
    }

    let entry = ArchiveIndexEntry {
        schema: INDEX_SCHEMA.to_string(),
        direction: exchange.direction,
        timestamp: exchange.timestamp.clone(),
        destination: exchange.destination.clone(),
        digest: stored_digest,
        byte_len: to_store.len() as u64,
        redacted_spans,
    };
    let entry_digest = entry.entry_digest()?;
    index.entries.push(entry.clone());

    Ok(ArchiveRef { digest: stored_digest, entry, entry_digest })
}

/// Parameters for converting archived TraCIM test data into a Tier X `reference_only`
/// [`RdlDataset`] record (`UNC.RDL.FORMAT`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedDataRecordParams {
    /// Dataset ID of the form `rdl:X/<aim>/<case>/<nnnn>`.
    pub id: String,
    /// Kernel aim (`module.feature.method`).
    pub aim: String,
    /// Input blob references (typically referencing archived CAS blob digests).
    pub inputs: Vec<BlobRef>,
    /// Reference outputs (in SI units).
    pub outputs: Vec<RefOutput>,
    /// Precision digits of the reference values.
    pub precision_digits: u32,
    /// Provenance source string (e.g. TraCIM test-suite citation + archive reference).
    pub source: String,
    /// Optional provenance method note.
    pub method: Option<String>,
    /// Non-redistributable licence tag (e.g. `PTB-TraCIM-Reference-Only`).
    pub licence: String,
}

/// Converts fetched TraCIM test data into a validated Tier X `reference_only` [`RdlDataset`].
pub fn fetched_test_data_to_record(params: &FetchedDataRecordParams) -> Result<RdlDataset, ArchiveError> {
    let inputs: Vec<CValue> = params
        .inputs
        .iter()
        .map(|b| {
            let mut m = BTreeMap::new();
            m.insert("digest".to_string(), CValue::Str(b.digest.as_str().to_string()));
            m.insert("role".to_string(), CValue::Str(b.role.clone()));
            m.insert("unit".to_string(), CValue::Str(b.unit.as_str().to_string()));
            CValue::Obj(m)
        })
        .collect();

    let outputs: Vec<CValue> = params
        .outputs
        .iter()
        .map(|o| {
            let mut m = BTreeMap::new();
            m.insert("kind".to_string(), CValue::Str(o.kind.as_str().to_string()));
            m.insert("name".to_string(), CValue::Str(o.name.clone()));
            m.insert("test_uncertainty_hex".to_string(), CValue::Str(o.test_uncertainty_hex.to_wire()));
            if let Some(dec) = &o.value_dec {
                m.insert("value_dec".to_string(), CValue::Str(dec.clone()));
            }
            m.insert("value_hex".to_string(), CValue::Str(o.value_hex.to_wire()));
            CValue::Obj(m)
        })
        .collect();

    let mut reference = BTreeMap::new();
    reference.insert("outputs".to_string(), CValue::Arr(outputs));
    reference.insert("precision_digits".to_string(), CValue::Num(f64::from(params.precision_digits)));

    let mut provenance = BTreeMap::new();
    provenance.insert("source".to_string(), CValue::Str(params.source.clone()));
    if let Some(method) = &params.method {
        provenance.insert("method".to_string(), CValue::Str(method.clone()));
    }

    let mut root = BTreeMap::new();
    root.insert("aim".to_string(), CValue::Str(params.aim.clone()));
    root.insert("cross_check".to_string(), CValue::Arr(Vec::new()));
    root.insert("id".to_string(), CValue::Str(params.id.clone()));
    root.insert("inputs".to_string(), CValue::Arr(inputs));
    root.insert("licence".to_string(), CValue::Str(params.licence.clone()));
    root.insert("provenance".to_string(), CValue::Obj(provenance));
    root.insert("reference".to_string(), CValue::Obj(reference));
    root.insert("schema".to_string(), CValue::Str(SCHEMA.to_string()));
    root.insert("tier".to_string(), CValue::Str(Tier::X.as_str().to_string()));
    root.insert("visibility".to_string(), CValue::Str(Visibility::ReferenceOnly.as_str().to_string()));

    Ok(RdlDataset::from_value(&CValue::Obj(root))?)
}

fn validate_meta(s: &str, field: &str) -> Result<(), ArchiveError> {
    if s.is_empty() || s.len() > MAX_META_BYTES {
        return Err(ArchiveError::InvalidMetadata(format!("{field} must be 1..={MAX_META_BYTES} bytes")));
    }
    if s.chars().any(is_control_or_format) {
        return Err(ArchiveError::InvalidMetadata(format!("{field} contains control or format characters")));
    }
    Ok(())
}

fn redact_spans(bytes: &[u8], spans: &[ByteSpan]) -> Result<Vec<u8>, ArchiveError> {
    if spans.is_empty() {
        return Ok(bytes.to_vec());
    }
    let mut sorted = spans.to_vec();
    sorted.sort();
    let len = bytes.len();
    let mut prev_end = 0usize;
    for (idx, span) in sorted.iter().enumerate() {
        if span.start >= span.end || span.end > len {
            return Err(ArchiveError::InvalidSpan { start: span.start, end: span.end, len });
        }
        if idx > 0 && span.start < prev_end {
            return Err(ArchiveError::OverlappingSpans);
        }
        prev_end = span.end;
    }

    let mut out = Vec::with_capacity(len);
    let mut cursor = 0usize;
    for span in &sorted {
        if let Some(chunk) = bytes.get(cursor..span.start) {
            out.extend_from_slice(chunk);
        }
        out.extend_from_slice(REDACTION_MARKER);
        cursor = span.end;
    }
    if let Some(tail) = bytes.get(cursor..) {
        out.extend_from_slice(tail);
    }
    Ok(out)
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}
