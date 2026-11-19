//! The format registry and content-signature detector (spec node `IO.REG.DETECT`).
//!
//! - [`FormatDescriptor`]: one registered reader or writer (ID, version range, content
//!   signatures, extensions, options schema, phase, origin, direction);
//! - [`Registry`]: descriptors keyed by `(FormatId, Direction)` in a `BTreeMap`;
//!   [`Registry::register`] fails `format_duplicate` on a second descriptor with the same key
//!   (REQ-006);
//! - [`Registry::detect`]: picks the reader from at most the first [`DETECT_PREFIX_LEN`]
//!   bytes of a file (REQ-002..005); [`read_prefix`] is the capped read that feeds it;
//! - [`list`]: the R0 commands `import.list_formats@1` and `export.list_formats@1`
//!   (REQ-001), their journal record and exact-mode replay.
//!
//! Detection never opens, resolves or follows anything: it sees a byte slice, the total
//! length and an optional file name that is only split at its last `.`. Signatures are
//! data (byte strings at offsets) or one of a closed set of named first-party
//! [`Predicate`]s; nothing a descriptor declares is executed. Every outcome is a returned
//! `Result`, and every tie is broken by a pinned rule or refused (`format_ambiguous`).

mod json;
pub mod list;

use crate::error::{IoError, Position};
use crate::report::FormatId;
use crate::roots::Direction;
use fid_canon::CValue;
use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Read};

pub use json::{MAX_REGISTRY_JSON_BYTES, MAX_REGISTRY_JSON_DEPTH};

/// The detector looks at no more than this many leading bytes of a file (REQ-002).
pub const DETECT_PREFIX_LEN: usize = 64 * 1024;
/// Most signatures one descriptor may declare.
pub const MAX_SIGNATURES: usize = 8;
/// Longest byte string of a [`Matcher::Bytes`] signature.
pub const MAX_SIGNATURE_BYTES: usize = 256;
/// Longest needle of a [`Matcher::Contains`] signature.
pub const MAX_CONTAINS_BYTES: usize = 32;
/// Widest search window of a [`Matcher::Contains`] signature.
pub const MAX_CONTAINS_WINDOW: u32 = 1024;
/// Most extensions one descriptor may declare.
pub const MAX_EXTENSIONS: usize = 16;
/// Longest extension, in bytes.
pub const MAX_EXTENSION_LEN: usize = 16;
/// Longest version token of a [`VersionRange`] bound.
pub const MAX_VERSION_LEN: usize = 32;
/// Largest canonical size of an options schema, in bytes.
pub const MAX_OPTIONS_SCHEMA_BYTES: usize = 64 * 1024;
/// Most descriptors one registry holds (both directions together).
pub const MAX_DESCRIPTORS: usize = 4096;

/// Where a descriptor comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    /// Shipped with Metrology Maven.
    FirstParty,
    /// Registered through the format-plugin interface (`IO.REG.PLUGIN`).
    Plugin,
}

impl Origin {
    /// The stable wire name: `first_party` or `plugin`.
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::FirstParty => "first_party",
            Origin::Plugin => "plugin",
        }
    }

    /// Parses a wire name.
    pub fn from_wire(s: &str) -> Option<Origin> {
        match s {
            "first_party" => Some(Origin::FirstParty),
            "plugin" => Some(Origin::Plugin),
            _ => None,
        }
    }
}

/// The `origin` parameter of the listing commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum OriginFilter {
    /// First-party descriptors only.
    FirstParty,
    /// Plugin descriptors only.
    Plugin,
    /// Every descriptor (the default).
    #[default]
    Any,
}

impl OriginFilter {
    /// The stable wire name: `first_party`, `plugin` or `any`.
    pub fn as_str(self) -> &'static str {
        match self {
            OriginFilter::FirstParty => "first_party",
            OriginFilter::Plugin => "plugin",
            OriginFilter::Any => "any",
        }
    }

    /// Parses a wire name.
    pub fn from_wire(s: &str) -> Option<OriginFilter> {
        match s {
            "first_party" => Some(OriginFilter::FirstParty),
            "plugin" => Some(OriginFilter::Plugin),
            "any" => Some(OriginFilter::Any),
            _ => None,
        }
    }

    /// Whether a descriptor of `origin` passes the filter.
    pub fn accepts(self, origin: Origin) -> bool {
        match self {
            OriginFilter::Any => true,
            OriginFilter::FirstParty => origin == Origin::FirstParty,
            OriginFilter::Plugin => origin == Origin::Plugin,
        }
    }
}

/// The wire name of a descriptor direction: `read` for [`Direction::Import`], `write` for
/// [`Direction::Export`].
pub fn direction_wire(d: Direction) -> &'static str {
    match d {
        Direction::Import => "read",
        Direction::Export => "write",
    }
}

/// Parses a descriptor direction wire name (`read` or `write`).
pub fn direction_from_wire(s: &str) -> Option<Direction> {
    match s {
        "read" => Some(Direction::Import),
        "write" => Some(Direction::Export),
        _ => None,
    }
}

/// The project phase a format ships in: `P0` to `P9`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Phase(u8);

impl Phase {
    /// Phase `P<n>`, for `n` in 0..=9.
    pub fn new(n: u8) -> Option<Phase> {
        (n <= 9).then_some(Phase(n))
    }

    /// Parses `P0` .. `P9`.
    pub fn parse(s: &str) -> Option<Phase> {
        match s.as_bytes() {
            [b'P', d @ b'0'..=b'9'] => Some(Phase(d - b'0')),
            _ => None,
        }
    }

    /// The phase number.
    pub fn number(self) -> u8 {
        self.0
    }
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "P{}", self.0)
    }
}

/// The range of format versions a reader or writer handles: two opaque version tokens
/// (`1` to [`MAX_VERSION_LEN`] bytes of ASCII letters, digits, `.`, `-` and `+`, or `*`
/// for "any"). The registry stores and lists them; it never orders or interprets them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VersionRange {
    min: String,
    max: String,
}

fn is_version_token(s: &str) -> bool {
    s == "*"
        || (!s.is_empty()
            && s.len() <= MAX_VERSION_LEN
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+')))
}

impl VersionRange {
    /// Checks both bounds.
    pub fn new(min: &str, max: &str) -> Result<VersionRange, DetectError> {
        if !is_version_token(min) || !is_version_token(max) {
            return Err(DetectError::invalid("versions", "invalid version token"));
        }
        Ok(VersionRange { min: min.to_string(), max: max.to_string() })
    }

    /// The lower bound token.
    pub fn min_version(&self) -> &str {
        &self.min
    }

    /// The upper bound token.
    pub fn max_version(&self) -> &str {
        &self.max
    }
}

/// A named, pure, first-party test over the prefix and the total file length. The set is
/// closed: plugins choose among these names and never supply code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Predicate {
    /// Binary STL size consistency: the prefix holds at least 84 bytes and the total length
    /// equals `84 + 50 * n`, where `n` is the little-endian `u32` at byte 80.
    StlBinarySize,
}

impl Predicate {
    /// Every predicate, in declaration order.
    pub const ALL: &'static [Predicate] = &[Predicate::StlBinarySize];

    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Predicate::StlBinarySize => "stl_binary_size",
        }
    }

    /// Parses a wire name.
    pub fn from_wire(s: &str) -> Option<Predicate> {
        Predicate::ALL.iter().copied().find(|p| p.as_str() == s)
    }

    /// Evaluates the predicate. Pure, bounded, never panics.
    pub fn eval(self, prefix: &[u8], total_len: u64) -> bool {
        match self {
            Predicate::StlBinarySize => match prefix.get(80..84) {
                Some(&[a, b, c, d]) => {
                    let n = u64::from(u32::from_le_bytes([a, b, c, d]));
                    // n <= 2^32 - 1, so 84 + 50 n < 2^39: no overflow.
                    total_len == 84 + 50 * n
                }
                _ => false,
            },
        }
    }
}

/// What a signature tests.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Matcher {
    /// These exact bytes start at the signature offset.
    Bytes(Vec<u8>),
    /// The needle occurs somewhere in `prefix[offset .. offset + window]` (clipped to the
    /// prefix).
    Contains {
        /// The bytes searched for.
        needle: Vec<u8>,
        /// The search window, in bytes from the offset.
        window: u32,
    },
    /// A named first-party predicate; the signature offset must be 0.
    Predicate(Predicate),
}

/// One content signature of a format.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Signature {
    offset: u32,
    matcher: Matcher,
    priority: u8,
}

impl Signature {
    /// Checks and builds a signature. Everything it tests must lie within the first
    /// [`DETECT_PREFIX_LEN`] bytes.
    pub fn new(offset: u32, matcher: Matcher, priority: u8) -> Result<Signature, DetectError> {
        let off = u64::from(offset);
        let cap = DETECT_PREFIX_LEN as u64;
        match &matcher {
            Matcher::Bytes(b) => {
                if b.is_empty() || b.len() > MAX_SIGNATURE_BYTES {
                    return Err(DetectError::invalid("signatures", "byte signature length out of range"));
                }
                if off + b.len() as u64 > cap {
                    return Err(DetectError::invalid("signatures", "signature beyond the detection prefix"));
                }
            }
            Matcher::Contains { needle, window } => {
                if needle.is_empty() || needle.len() > MAX_CONTAINS_BYTES {
                    return Err(DetectError::invalid("signatures", "contains needle length out of range"));
                }
                if *window == 0 || *window > MAX_CONTAINS_WINDOW || (*window as usize) < needle.len() {
                    return Err(DetectError::invalid("signatures", "contains window out of range"));
                }
                if off + u64::from(*window) > cap {
                    return Err(DetectError::invalid("signatures", "signature beyond the detection prefix"));
                }
            }
            Matcher::Predicate(_) => {
                if offset != 0 {
                    return Err(DetectError::invalid("signatures", "predicate signatures take offset 0"));
                }
            }
        }
        Ok(Signature { offset, matcher, priority })
    }

    /// Byte offset from the start of the file.
    pub fn offset(&self) -> u32 {
        self.offset
    }

    /// What is tested.
    pub fn matcher(&self) -> &Matcher {
        &self.matcher
    }

    /// Declared priority; higher wins (REQ-003).
    pub fn priority(&self) -> u8 {
        self.priority
    }

    /// Whether the signature matches. `prefix` is at most [`DETECT_PREFIX_LEN`] bytes.
    pub fn matches(&self, prefix: &[u8], total_len: u64) -> bool {
        let off = self.offset as usize;
        match &self.matcher {
            Matcher::Bytes(b) => off.checked_add(b.len()).and_then(|end| prefix.get(off..end)) == Some(b.as_slice()),
            Matcher::Contains { needle, window } => {
                let Some(rest) = prefix.get(off..) else { return false };
                let hay = rest.get(..*window as usize).unwrap_or(rest);
                hay.windows(needle.len()).any(|w| w == needle.as_slice())
            }
            Matcher::Predicate(p) => p.eval(prefix, total_len),
        }
    }
}

/// One registered reader or writer.
#[derive(Debug, Clone, PartialEq)]
pub struct FormatDescriptor {
    id: FormatId,
    versions: VersionRange,
    signatures: Vec<Signature>,
    extensions: Vec<String>,
    options_schema: CValue,
    phase: Phase,
    origin: Origin,
    direction: Direction,
}

fn is_extension(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_EXTENSION_LEN && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

impl FormatDescriptor {
    /// Checks and builds a descriptor. Extensions are lowercase ASCII letters and digits
    /// without the dot; they are stored sorted, and a repeated extension is refused.
    /// Signatures keep their declared order. The options schema must be a JSON object of
    /// at most [`MAX_OPTIONS_SCHEMA_BYTES`] canonical bytes.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: FormatId,
        versions: VersionRange,
        signatures: Vec<Signature>,
        extensions: Vec<String>,
        options_schema: CValue,
        phase: Phase,
        origin: Origin,
        direction: Direction,
    ) -> Result<FormatDescriptor, DetectError> {
        if signatures.len() > MAX_SIGNATURES {
            return Err(DetectError::invalid("signatures", "too many signatures"));
        }
        if extensions.len() > MAX_EXTENSIONS {
            return Err(DetectError::invalid("extensions", "too many extensions"));
        }
        let mut extensions = extensions;
        if !extensions.iter().all(|e| is_extension(e)) {
            return Err(DetectError::invalid("extensions", "invalid extension"));
        }
        extensions.sort();
        if extensions.windows(2).any(|w| w.first() == w.get(1)) {
            return Err(DetectError::invalid("extensions", "repeated extension"));
        }
        if !matches!(options_schema, CValue::Obj(_)) {
            return Err(DetectError::invalid("options_schema", "options schema must be an object"));
        }
        match fid_canon::to_canonical_bytes(&options_schema) {
            Ok(b) if b.len() <= MAX_OPTIONS_SCHEMA_BYTES => {}
            Ok(_) => return Err(DetectError::invalid("options_schema", "options schema too large")),
            Err(_) => return Err(DetectError::invalid("options_schema", "options schema is not canonical JSON")),
        }
        Ok(FormatDescriptor { id, versions, signatures, extensions, options_schema, phase, origin, direction })
    }

    /// The format ID.
    pub fn id(&self) -> &FormatId {
        &self.id
    }

    /// The version range.
    pub fn versions(&self) -> &VersionRange {
        &self.versions
    }

    /// The content signatures, in declared order.
    pub fn signatures(&self) -> &[Signature] {
        &self.signatures
    }

    /// The extensions, sorted, without the dot.
    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }

    /// The options JSON Schema.
    pub fn options_schema(&self) -> &CValue {
        &self.options_schema
    }

    /// The phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// First-party or plugin.
    pub fn origin(&self) -> Origin {
        self.origin
    }

    /// Read ([`Direction::Import`]) or write ([`Direction::Export`]).
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// The highest priority among the matching signatures, if any matches.
    pub fn best_match(&self, prefix: &[u8], total_len: u64) -> Option<u8> {
        self.signatures.iter().filter(|s| s.matches(prefix, total_len)).map(Signature::priority).max()
    }
}

/// Why registration or detection failed. [`DetectError::code`] is always one of the
/// `IO.REG.LIMITS` taxonomy codes ([`crate::CODES`]); [`DetectError::to_io_error`] maps it
/// onto [`IoError`] (whose format variants carry no payload).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectError {
    /// No signature matches and no format was named (REQ-004).
    Unknown,
    /// The caller named a format that is not registered for reading.
    NotRegistered {
        /// The named format.
        named: FormatId,
    },
    /// Two or more formats match at the same highest priority (REQ-003).
    Ambiguous {
        /// The tied priority.
        priority: u8,
        /// The tied formats, sorted by ID.
        candidates: Vec<FormatId>,
    },
    /// The file extension maps to registered formats that do not include the detected one
    /// (REQ-005).
    Mismatch {
        /// The format the content signature selected.
        detected: FormatId,
        /// The extension (lowercase, without the dot; always a registered extension).
        extension: String,
        /// The formats registered for that extension, sorted by ID.
        extension_formats: Vec<FormatId>,
    },
    /// The same format ID and direction is registered twice (REQ-006).
    Duplicate {
        /// The format ID.
        id: FormatId,
        /// The direction.
        direction: Direction,
    },
    /// A descriptor, a registry document or the detection call itself is invalid.
    Invalid {
        /// The offending field.
        field: &'static str,
        /// Static reason (never input text).
        reason: &'static str,
    },
    /// Reading the prefix failed.
    Io {
        /// The I/O error kind.
        kind: io::ErrorKind,
        /// Bytes read before the failure.
        at: u64,
    },
}

impl DetectError {
    fn invalid(field: &'static str, reason: &'static str) -> DetectError {
        DetectError::Invalid { field, reason }
    }

    /// The taxonomy code.
    pub fn code(&self) -> &'static str {
        self.to_io_error().code()
    }

    /// The matching [`IoError`] (payload dropped).
    pub fn to_io_error(&self) -> IoError {
        match self {
            DetectError::Unknown | DetectError::NotRegistered { .. } => IoError::FormatUnknown,
            DetectError::Ambiguous { .. } => IoError::FormatAmbiguous,
            DetectError::Mismatch { .. } => IoError::FormatMismatch,
            DetectError::Duplicate { .. } => IoError::FormatDuplicate,
            DetectError::Invalid { reason, .. } => IoError::Malformed { at: Position::byte(0), reason },
            DetectError::Io { kind, at } => IoError::Io { kind: *kind, at: Position::byte(*at) },
        }
    }
}

impl From<DetectError> for IoError {
    fn from(e: DetectError) -> IoError {
        e.to_io_error()
    }
}

fn join(ids: &[FormatId]) -> String {
    ids.iter().map(FormatId::as_str).collect::<Vec<_>>().join(", ")
}

impl fmt::Display for DetectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())?;
        match self {
            DetectError::Unknown => f.write_str(": no registered signature matches"),
            DetectError::NotRegistered { named } => write!(f, ": {named} is not a registered reader"),
            DetectError::Ambiguous { priority, candidates } => {
                write!(f, ": {} match at priority {priority}", join(candidates))
            }
            DetectError::Mismatch { detected, extension, extension_formats } => {
                write!(f, ": content is {detected}, extension .{extension} maps to {}", join(extension_formats))
            }
            DetectError::Duplicate { id, direction } => {
                write!(f, ": {id} ({}) is already registered", direction_wire(*direction))
            }
            DetectError::Invalid { field, reason } => write!(f, ": {field}: {reason}"),
            DetectError::Io { kind, at } => write!(f, ": {kind} after {at} bytes"),
        }
    }
}

impl std::error::Error for DetectError {}

/// The extension of the last component of `file_name`, lowercased, if it is one the
/// registry could hold: the text after the last `.`, where the component has a non-empty
/// stem and the extension is 1 to [`MAX_EXTENSION_LEN`] ASCII letters or digits. Both `/`
/// and `\` separate components. The name is only split, never resolved.
pub fn extension_of(file_name: &str) -> Option<String> {
    let last = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let (stem, ext) = last.rsplit_once('.')?;
    if stem.is_empty()
        || ext.is_empty()
        || ext.len() > MAX_EXTENSION_LEN
        || !ext.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

/// Reads at most [`DETECT_PREFIX_LEN`] bytes from `r` (fewer only at end of input). The
/// reader is never asked for more than that many bytes in total.
pub fn read_prefix<R: Read>(r: R) -> Result<Vec<u8>, DetectError> {
    let mut buf = Vec::with_capacity(DETECT_PREFIX_LEN);
    let mut limited = r.take(DETECT_PREFIX_LEN as u64);
    match limited.read_to_end(&mut buf) {
        Ok(_) => Ok(buf),
        Err(e) => Err(DetectError::Io { kind: e.kind(), at: buf.len() as u64 }),
    }
}

/// The in-memory format registry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Registry {
    formats: BTreeMap<(FormatId, Direction), FormatDescriptor>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Registry {
        Registry::default()
    }

    /// Adds a descriptor. Fails `format_duplicate` when its ID and direction are already
    /// registered (REQ-006); the registry is then unchanged.
    pub fn register(&mut self, desc: FormatDescriptor) -> Result<(), DetectError> {
        let key = (desc.id.clone(), desc.direction);
        if self.formats.contains_key(&key) {
            return Err(DetectError::Duplicate { id: key.0, direction: key.1 });
        }
        if self.formats.len() >= MAX_DESCRIPTORS {
            return Err(DetectError::invalid("registry", "registry is full"));
        }
        self.formats.insert(key, desc);
        Ok(())
    }

    /// Number of descriptors (both directions).
    pub fn len(&self) -> usize {
        self.formats.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.formats.is_empty()
    }

    /// The descriptor for an ID and direction.
    pub fn get(&self, id: &FormatId, direction: Direction) -> Option<&FormatDescriptor> {
        self.formats.get(&(id.clone(), direction))
    }

    /// Every descriptor of one direction that passes `filter`, sorted by format ID.
    pub fn list(&self, direction: Direction, filter: OriginFilter) -> Vec<&FormatDescriptor> {
        self.formats.values().filter(|d| d.direction == direction && filter.accepts(d.origin)).collect()
    }

    /// Picks the reader for a file (REQ-002..005).
    ///
    /// - `named`: when given, it is returned if registered for reading (`format_unknown`
    ///   otherwise); content and extension are then not examined.
    /// - Otherwise only the first [`DETECT_PREFIX_LEN`] bytes of `prefix` are used;
    ///   `total_len` is the whole file's length (for size predicates) and may not be smaller
    ///   than `prefix`. Each read descriptor's match priority is the highest priority of its
    ///   matching signatures; descriptors are sorted by (priority descending, ID ascending).
    ///   None: `format_unknown`. Several at the top priority: `format_ambiguous` listing
    ///   them. One: it is the detected format.
    /// - Finally, if `file_name` has an extension that some read descriptors declare and
    ///   the detected format is not among them: `format_mismatch`.
    pub fn detect(
        &self,
        prefix: &[u8],
        total_len: u64,
        file_name: Option<&str>,
        named: Option<&FormatId>,
    ) -> Result<FormatId, DetectError> {
        if let Some(named) = named {
            return match self.get(named, Direction::Import) {
                Some(d) => Ok(d.id.clone()),
                None => Err(DetectError::NotRegistered { named: named.clone() }),
            };
        }
        if prefix.len() as u64 > total_len {
            return Err(DetectError::invalid("total_len", "total length is smaller than the prefix"));
        }
        let prefix = prefix.get(..DETECT_PREFIX_LEN).unwrap_or(prefix);
        let readers = || self.formats.values().filter(|d| d.direction == Direction::Import);

        let mut hits: Vec<(u8, &FormatId)> =
            readers().filter_map(|d| d.best_match(prefix, total_len).map(|p| (p, &d.id))).collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        let Some(&(top, first)) = hits.first() else {
            return Err(DetectError::Unknown);
        };
        let tied: Vec<FormatId> = hits.iter().take_while(|h| h.0 == top).map(|h| h.1.clone()).collect();
        if tied.len() > 1 {
            return Err(DetectError::Ambiguous { priority: top, candidates: tied });
        }

        if let Some(ext) = file_name.and_then(extension_of) {
            let by_ext: Vec<FormatId> =
                readers().filter(|d| d.extensions.contains(&ext)).map(|d| d.id.clone()).collect();
            if !by_ext.is_empty() && !by_ext.contains(first) {
                return Err(DetectError::Mismatch {
                    detected: first.clone(),
                    extension: ext,
                    extension_formats: by_ext,
                });
            }
        }
        Ok(first.clone())
    }

    /// [`read_prefix`] then [`Registry::detect`].
    pub fn detect_reader<R: Read>(
        &self,
        r: R,
        total_len: u64,
        file_name: Option<&str>,
        named: Option<&FormatId>,
    ) -> Result<FormatId, DetectError> {
        if let Some(named) = named {
            return self.detect(&[], total_len, file_name, Some(named));
        }
        let prefix = read_prefix(r)?;
        self.detect(&prefix, total_len, file_name, None)
    }
}
