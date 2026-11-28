//! The element tree, the closed error set and the warnings of the XML layer.

use fid_io_reg::{IoError, LimitKind, Position};
use fid_xml::{XmlError, XmlLimit, XmlText};
use std::fmt;
use std::io;

use crate::PhysError;

/// The name of a child element: unprefixed (in the E57 namespace) or with the prefix of an
/// extension namespace declared on the root.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key {
    /// The extension prefix, `None` for the E57 namespace.
    pub prefix: Option<String>,
    /// The local name.
    pub local: String,
}

impl Key {
    /// A key in the E57 namespace.
    pub fn new(local: &str) -> Key {
        Key { prefix: None, local: local.to_string() }
    }

    /// A key in the extension namespace bound to `prefix`.
    pub fn prefixed(prefix: &str, local: &str) -> Key {
        Key { prefix: Some(prefix.to_string()), local: local.to_string() }
    }

    /// True for an unprefixed key with local name `local`.
    pub fn is(&self, local: &str) -> bool {
        self.prefix.is_none() && self.local == local
    }

    pub(crate) fn heap_bytes(&self) -> u64 {
        (self.prefix.as_ref().map_or(0, String::len) + self.local.len()) as u64
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.prefix {
            Some(p) => write!(f, "{p}:{}", self.local),
            None => f.write_str(&self.local),
        }
    }
}

/// The `precision` of a `Float` element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Precision {
    /// `single`: IEEE-754 binary32 in the binary sections.
    Single,
    /// `double`: IEEE-754 binary64.
    Double,
}

impl Precision {
    /// The attribute value.
    pub fn as_str(self) -> &'static str {
        match self {
            Precision::Single => "single",
            Precision::Double => "double",
        }
    }
}

/// The value of an element's `type` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NodeType {
    /// Named children with unique keys.
    Structure,
    /// Ordered children named `vectorChild`.
    Vector,
    /// A binary section of records with a prototype.
    CompressedVector,
    /// A 64-bit signed integer.
    Integer,
    /// A raw integer with scale and offset.
    ScaledInteger,
    /// A real.
    Float,
    /// Text (untrusted).
    String,
    /// A binary section of bytes.
    Blob,
}

impl NodeType {
    /// Every type, in declaration order.
    pub const ALL: &'static [NodeType] = &[
        NodeType::Structure,
        NodeType::Vector,
        NodeType::CompressedVector,
        NodeType::Integer,
        NodeType::ScaledInteger,
        NodeType::Float,
        NodeType::String,
        NodeType::Blob,
    ];

    /// The attribute value.
    pub fn as_str(self) -> &'static str {
        match self {
            NodeType::Structure => "Structure",
            NodeType::Vector => "Vector",
            NodeType::CompressedVector => "CompressedVector",
            NodeType::Integer => "Integer",
            NodeType::ScaledInteger => "ScaledInteger",
            NodeType::Float => "Float",
            NodeType::String => "String",
            NodeType::Blob => "Blob",
        }
    }

    /// Parses an attribute value exactly.
    pub fn parse(s: &str) -> Option<NodeType> {
        NodeType::ALL.iter().copied().find(|t| t.as_str() == s)
    }

    /// The attributes, besides `type`, an element of this type may carry, in the writer's
    /// order.
    pub fn attributes(self) -> &'static [&'static str] {
        match self {
            NodeType::Structure | NodeType::String => &[],
            NodeType::Vector => &["allowHeterogeneousChildren"],
            NodeType::CompressedVector => &["fileOffset", "recordCount"],
            NodeType::Integer => &["minimum", "maximum"],
            NodeType::ScaledInteger => &["minimum", "maximum", "scale", "offset"],
            NodeType::Float => &["precision", "minimum", "maximum"],
            NodeType::Blob => &["fileOffset", "length"],
        }
    }
}

/// One E57 element. Attributes are kept exactly as present: `None` means the attribute (or,
/// for a number, the value text) was absent; no default is filled in.
#[derive(Debug, Clone, PartialEq)]
pub enum E57Node {
    /// Children in document order; keys are unique.
    Structure(Vec<(Key, E57Node)>),
    /// Children in document order (each written as `vectorChild`).
    Vector {
        /// `allowHeterogeneousChildren` (`0` or `1`); unless `Some(true)` every child has the
        /// same type.
        allow_heterogeneous: Option<bool>,
        /// The children.
        children: Vec<E57Node>,
    },
    /// A compressed-vector binary section.
    CompressedVector {
        /// `fileOffset`: physical offset of the section.
        file_offset: u64,
        /// `recordCount`.
        record_count: u64,
        /// The `prototype` child.
        prototype: Box<E57Node>,
        /// The `codecs` child (a `Vector`), if present.
        codecs: Option<Box<E57Node>>,
    },
    /// An integer.
    Integer {
        /// The value, if the element has value text.
        value: Option<i64>,
        /// `minimum`.
        minimum: Option<i64>,
        /// `maximum`.
        maximum: Option<i64>,
    },
    /// A scaled integer: the stored value is `raw`, the represented one `offset + scale * raw`.
    ScaledInteger {
        /// The raw value, if the element has value text.
        raw: Option<i64>,
        /// `minimum` (of the raw value).
        minimum: Option<i64>,
        /// `maximum` (of the raw value).
        maximum: Option<i64>,
        /// `scale` (finite, not zero).
        scale: Option<f64>,
        /// `offset` (finite).
        offset: Option<f64>,
    },
    /// A real.
    Float {
        /// The value, if the element has value text.
        value: Option<f64>,
        /// `precision`.
        precision: Option<Precision>,
        /// `minimum`.
        minimum: Option<f64>,
        /// `maximum`.
        maximum: Option<f64>,
    },
    /// Text, untrusted (CONSTITUTION C6).
    String(XmlText),
    /// A blob binary section.
    Blob {
        /// `fileOffset`: physical offset of the section.
        file_offset: u64,
        /// `length` in bytes.
        length: u64,
    },
}

impl E57Node {
    /// The element's type.
    pub fn node_type(&self) -> NodeType {
        match self {
            E57Node::Structure(_) => NodeType::Structure,
            E57Node::Vector { .. } => NodeType::Vector,
            E57Node::CompressedVector { .. } => NodeType::CompressedVector,
            E57Node::Integer { .. } => NodeType::Integer,
            E57Node::ScaledInteger { .. } => NodeType::ScaledInteger,
            E57Node::Float { .. } => NodeType::Float,
            E57Node::String(_) => NodeType::String,
            E57Node::Blob { .. } => NodeType::Blob,
        }
    }

    /// The child with unprefixed key `local`, for a `Structure` (linear search).
    pub fn get(&self, local: &str) -> Option<&E57Node> {
        match self {
            E57Node::Structure(children) => find(children, local),
            _ => None,
        }
    }
}

/// The child with unprefixed key `local` (linear search; keys are unique).
pub(crate) fn find<'a>(children: &'a [(Key, E57Node)], local: &str) -> Option<&'a E57Node> {
    children.iter().find(|(k, _)| k.is(local)).map(|(_, n)| n)
}

/// An extension namespace declared on the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace {
    /// The prefix (an XML NCName, not `xml` or `xmlns`).
    pub prefix: String,
    /// The namespace name (untrusted, not empty, not the E57 namespace).
    pub uri: XmlText,
}

/// A whole XML section: the root `e57Root` structure's children and the extension
/// namespaces declared on it, in document order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct E57Tree {
    /// Extension namespaces (prefixed declarations on the root).
    pub namespaces: Vec<Namespace>,
    /// The root structure's children.
    pub root: Vec<(Key, E57Node)>,
}

/// The bound a [`XmlSectionError::LimitExceeded`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Bound {
    /// Element nesting depth ([`super::MAX_DEPTH`] or the budget's depth limit).
    Depth,
    /// Elements ([`super::MAX_ELEMENTS`] or the budget's element limit).
    Elements,
    /// Bytes of one text node or string value ([`super::MAX_TEXT_LEN`]).
    TextLength,
    /// Bytes of one markup token.
    TokenLength,
    /// Attributes on one element.
    Attributes,
    /// Bytes of one name.
    NameLength,
    /// Bytes of one namespace name.
    UriLength,
    /// Namespace bindings in scope.
    Namespaces,
    /// Input bytes (the budget).
    Bytes,
    /// Tracked allocation (the budget).
    Alloc,
    /// Wall time (the budget).
    Wall,
}

impl Bound {
    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Bound::Depth => "depth",
            Bound::Elements => "elements",
            Bound::TextLength => "text_length",
            Bound::TokenLength => "token_length",
            Bound::Attributes => "attributes",
            Bound::NameLength => "name_length",
            Bound::UriLength => "uri_length",
            Bound::Namespaces => "namespaces",
            Bound::Bytes => "bytes",
            Bound::Alloc => "alloc",
            Bound::Wall => "wall",
        }
    }

    fn of_xml(l: XmlLimit) -> Bound {
        match l {
            XmlLimit::Depth => Bound::Depth,
            XmlLimit::Attributes => Bound::Attributes,
            XmlLimit::NameLength => Bound::NameLength,
            XmlLimit::TextLength => Bound::TextLength,
            XmlLimit::TokenLength => Bound::TokenLength,
            XmlLimit::UriLength => Bound::UriLength,
            XmlLimit::Namespaces => Bound::Namespaces,
        }
    }

    fn of_harness(l: LimitKind) -> Bound {
        match l {
            LimitKind::Bytes => Bound::Bytes,
            LimitKind::Elements => Bound::Elements,
            LimitKind::Depth => Bound::Depth,
            LimitKind::Alloc => Bound::Alloc,
            LimitKind::Wall => Bound::Wall,
        }
    }
}

impl fmt::Display for Bound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why reading the XML section failed. No variant holds text copied from the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmlSectionError {
    /// Not a well-formed, E57-conforming XML section (`malformed`). `reason` is a static
    /// code: `xml_forbidden_construct` for a DTD or entity construct (REQ-002), the XML
    /// reader's own reasons, or one of the E57 reasons listed in the module documentation.
    Malformed {
        /// Static, reader-chosen reason.
        reason: &'static str,
        /// Where.
        at: Position,
    },
    /// The tree is well formed but its content breaks a scan or root rule (`malformed`):
    /// `format_name_missing`, `format_name_wrong` (not the `String` [`super::FORMAT_NAME`]),
    /// `version_major_missing`, `version_major_unsupported` (not the `Integer` 1),
    /// `data3d_missing`, `data3d_not_vector`, `images2d_not_vector`, or a `data3D` child
    /// (`scan`, 0-based) with a missing or mistyped `guid`, `points`, pose, bounds or limits
    /// element, an empty or non-`Structure` prototype, or inverted bounds or limits.
    Content {
        /// Static reason.
        reason: &'static str,
        /// The `data3D` child, if the rule is about one.
        scan: Option<usize>,
    },
    /// A bound was passed (`limit_exceeded`, REQ-003).
    LimitExceeded {
        /// The bound.
        bound: Bound,
        /// Its configured value.
        value: u64,
        /// Where.
        at: Position,
    },
    /// The section ends inside the document (`truncated_file`).
    TruncatedFile {
        /// Where the input ended.
        at: Position,
    },
    /// The physical layer failed while reading the section (`checksum_mismatch` and the
    /// other [`PhysError`] codes).
    Phys(PhysError),
    /// The byte source failed (`io`).
    Io {
        /// The I/O error kind.
        kind: io::ErrorKind,
        /// Where.
        at: Position,
    },
}

impl XmlSectionError {
    /// The stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            XmlSectionError::Malformed { .. } | XmlSectionError::Content { .. } => "malformed",
            XmlSectionError::LimitExceeded { .. } => "limit_exceeded",
            XmlSectionError::TruncatedFile { .. } => "truncated_file",
            XmlSectionError::Phys(e) => e.code(),
            XmlSectionError::Io { .. } => "io",
        }
    }

    pub(crate) fn malformed(reason: &'static str, at: Position) -> XmlSectionError {
        XmlSectionError::Malformed { reason, at }
    }

    pub(crate) fn from_xml(e: XmlError) -> XmlSectionError {
        match e {
            XmlError::ForbiddenConstruct { at, .. } => XmlSectionError::malformed("xml_forbidden_construct", at),
            XmlError::EncodingUnsupported { at } => XmlSectionError::malformed("xml_encoding_unsupported", at),
            XmlError::LimitExceeded { limit, value, at } => {
                XmlSectionError::LimitExceeded { bound: Bound::of_xml(limit), value, at }
            }
            XmlError::Malformed { reason, at } => XmlSectionError::malformed(reason, at),
            XmlError::TruncatedFile { at } => XmlSectionError::TruncatedFile { at },
            XmlError::Harness(e) => XmlSectionError::from_io(e),
        }
    }

    fn from_io(e: IoError) -> XmlSectionError {
        match e {
            IoError::LimitExceeded { limit, value, at } => {
                XmlSectionError::LimitExceeded { bound: Bound::of_harness(limit), value, at }
            }
            IoError::Malformed { at, reason } => XmlSectionError::malformed(reason, at),
            IoError::TruncatedFile { at } => XmlSectionError::TruncatedFile { at },
            IoError::Io { kind, at } => XmlSectionError::Io { kind, at },
            other => {
                let at = other.position().unwrap_or_default();
                XmlSectionError::malformed(other.code(), at)
            }
        }
    }
}

impl fmt::Display for XmlSectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            XmlSectionError::Malformed { reason, at } => write!(f, "malformed: {reason} at {at}"),
            XmlSectionError::Content { reason, scan: Some(i) } => write!(f, "malformed: {reason} in data3D child {i}"),
            XmlSectionError::Content { reason, scan: None } => write!(f, "malformed: {reason}"),
            XmlSectionError::LimitExceeded { bound, value, at } => {
                write!(f, "limit_exceeded: bound {bound} = {value} at {at}")
            }
            XmlSectionError::TruncatedFile { at } => write!(f, "truncated_file: XML section ends at {at}"),
            XmlSectionError::Phys(e) => write!(f, "{e}"),
            XmlSectionError::Io { kind, at } => write!(f, "io: {kind} at {at}"),
        }
    }
}

impl std::error::Error for XmlSectionError {}

/// Something the reader passes on for the caller to decide.
#[derive(Debug, Clone, PartialEq)]
pub enum XmlWarning {
    /// The pose quaternion of `data3D` child `scan` has norm `norm` (computed as
    /// `sqrt(w*w + x*x + y*y + z*z)` in binary64, each operation rounded once, no FMA), which
    /// differs from 1 by more than [`super::QUATERNION_TOL`]. The quaternion is passed on
    /// unnormalised (REQ-004).
    QuaternionNotUnit {
        /// Index of the scan in `data3D`.
        scan: usize,
        /// The computed norm.
        norm: f64,
    },
    /// `data3D` has no children: the file holds no scan.
    NoScans,
}

impl XmlWarning {
    /// The stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            XmlWarning::QuaternionNotUnit { .. } => "pose_quaternion_not_unit",
            XmlWarning::NoScans => "no_scans",
        }
    }
}
