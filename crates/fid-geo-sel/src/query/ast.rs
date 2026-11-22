//! Typed AST, limits and errors for semantic selection queries (spec node
//! `GEO.SEL.QUERY.GRAMMAR`).

use crate::model::{Domain, EntityId};
use core::fmt;
use fid_geo_frame::{AuthoredQuantity, UnitsError};
use fid_types::Digest;

/// The only query schema version accepted by this parser.
pub const QUERY_VERSION: u64 = 1;
/// Maximum raw JSON input size in bytes (64 KiB, REQ-GEO.SEL.QUERY.GRAMMAR-003).
pub const MAX_QUERY_BYTES: usize = 64 * 1024;
/// Maximum JSON container and AST nesting depth (REQ-GEO.SEL.QUERY.GRAMMAR-003).
pub const MAX_QUERY_DEPTH: usize = 32;
/// Maximum number of `Expr` AST nodes in one query (REQ-GEO.SEL.QUERY.GRAMMAR-003).
pub const MAX_AST_NODES: usize = 1_024;
/// Maximum number of IDs in one `ids` or `face_ids` list (REQ-GEO.SEL.QUERY.GRAMMAR-003).
pub const MAX_IDS_PER_LIST: usize = 10_000;
/// Maximum UTF-8 byte length of a [`TopologyId`] (REQ-GEO.SEL.QUERY.GRAMMAR-002).
pub const MAX_TOPOLOGY_ID_BYTES: usize = 256;

/// SHA-256 digest of a canonicalised [`SelectionQuery`] (`sha256:<64 lowercase hex>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QueryDigest(pub Digest);

impl QueryDigest {
    pub const fn to_digest(self) -> Digest {
        self.0
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn to_bytes(self) -> [u8; 32] {
        self.0.to_bytes()
    }
}

impl fmt::Display for QueryDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

/// Errors produced by query parsing, validation and canonicalisation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    /// Input does not conform to closed schema v1 (REQ-001).
    BadQuery { pointer: String, reason: String },
    /// Input exceeded byte, depth, AST node or ID-list limits (REQ-003).
    QueryLimit { reason: &'static str },
    /// Authored length or angle failed `GEO.FRAME.UNITS` validation.
    BadQuantity { pointer: String, error: UnitsError },
}

impl QueryError {
    pub const fn code(&self) -> &'static str {
        match self {
            QueryError::BadQuery { .. } => "bad_query",
            QueryError::QueryLimit { .. } => "query_limit",
            QueryError::BadQuantity { .. } => "bad_quantity",
        }
    }

    pub fn pointer(&self) -> Option<&str> {
        match self {
            QueryError::BadQuery { pointer, .. } | QueryError::BadQuantity { pointer, .. } => Some(pointer),
            QueryError::QueryLimit { .. } => None,
        }
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueryError::BadQuery { pointer, reason } => write!(f, "bad_query at {pointer:?}: {reason}"),
            QueryError::QueryLimit { reason } => write!(f, "query_limit: {reason}"),
            QueryError::BadQuantity { pointer, error } => write!(f, "bad_quantity at {pointer:?}: {error}"),
        }
    }
}

impl std::error::Error for QueryError {}

/// An opaque CAD topology label (1..=256 UTF-8 bytes), compared strictly bytewise and never
/// evaluated, templated or interpreted (REQ-002, C6).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TopologyId(String);

impl TopologyId {
    pub fn new(raw: String) -> Option<TopologyId> {
        if raw.is_empty() || raw.len() > MAX_TOPOLOGY_ID_BYTES {
            return None;
        }
        Some(TopologyId(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Closed attribute channel set for `attr_range`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AttrChannel {
    Intensity,
    ScannerId,
    TimestampNs,
}

impl AttrChannel {
    pub const fn as_str(self) -> &'static str {
        match self {
            AttrChannel::Intensity => "intensity",
            AttrChannel::ScannerId => "scanner_id",
            AttrChannel::TimestampNs => "timestamp_ns",
        }
    }

    pub fn parse(s: &str) -> Option<AttrChannel> {
        match s {
            "intensity" => Some(AttrChannel::Intensity),
            "scanner_id" => Some(AttrChannel::ScannerId),
            "timestamp_ns" => Some(AttrChannel::TimestampNs),
            _ => None,
        }
    }
}

/// Target reference of a `near` predicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NearRef {
    Element(EntityId),
    Selection(EntityId),
    Face { body: EntityId, id: TopologyId },
}

/// Closed v1 predicate set.
#[derive(Debug, Clone, PartialEq)]
pub enum Predicate {
    All,
    Ids(Vec<u64>),
    AttrRange { channel: AttrChannel, min: Option<f64>, max: Option<f64> },
    QualityFlags { all_of: Option<u16>, none_of: Option<u16> },
    NormalCone { axis: [f64; 3], half_angle: AuthoredQuantity },
    InBox { min: [AuthoredQuantity; 3], max: [AuthoredQuantity; 3] },
    InSphere { centre: [AuthoredQuantity; 3], radius: AuthoredQuantity },
    Near { of: NearRef, distance: AuthoredQuantity },
    FaceIds { body: EntityId, ids: Vec<TopologyId> },
    InGroup { group: EntityId },
    InLayer { layer: EntityId },
    InBody { body: EntityId },
    TangentGrow { seed: Box<Expr>, angle: AuthoredQuantity },
}

/// Boolean / predicate expression tree.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Expr {
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Not(Box<Expr>),
    Pred(Predicate),
}

/// A parsed semantic selection query (schema v1).
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionQuery {
    pub domain: Domain,
    pub root: Expr,
}

/// Parses a canonical lowercase hyphenated UUID (`8-4-4-4-12`), rejecting nil and max UUIDs.
pub fn parse_entity_id(s: &str) -> Option<EntityId> {
    let b = s.as_bytes();
    if b.len() != 36 {
        return None;
    }
    if b.get(8) != Some(&b'-') || b.get(13) != Some(&b'-') || b.get(18) != Some(&b'-') || b.get(23) != Some(&b'-') {
        return None;
    }
    let mut out = [0u8; 16];
    let mut oi = 0usize;
    let mut i = 0usize;
    while i < 36 {
        if i == 8 || i == 13 || i == 18 || i == 23 {
            i += 1;
            continue;
        }
        let hi = lower_hex(*b.get(i)?)?;
        let lo = lower_hex(*b.get(i + 1)?)?;
        if let Some(slot) = out.get_mut(oi) {
            *slot = (hi << 4) | lo;
        }
        oi += 1;
        i += 2;
    }
    if out == [0u8; 16] || out == [0xffu8; 16] {
        return None;
    }
    Some(EntityId::from_bytes(out))
}

/// Formats an [`EntityId`] as canonical lowercase hyphenated UUID (`8-4-4-4-12`).
pub fn format_entity_id(id: EntityId) -> String {
    let b = id.to_bytes();
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

fn lower_hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}
