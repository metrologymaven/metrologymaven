//! The reader's error set (REQ-IO.QIF.XML.PARSE-001 .. -005).

use fid_io_reg::{IoError, Position};
use std::fmt;

/// Every stable error code this crate produces itself, in declaration order of
/// [`XmlError`]. [`XmlError::Harness`] carries the codes of [`fid_io_reg::CODES`].
pub const CODES: &[&str] =
    &["xml_forbidden_construct", "xml_encoding_unsupported", "limit_exceeded", "malformed", "truncated_file"];

/// A construct the reader refuses before resolving it (REQ-IO.QIF.XML.PARSE-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Construct {
    /// A document type declaration (`<!DOCTYPE`), with or without an internal subset.
    Doctype,
    /// An entity declaration (`<!ENTITY`).
    EntityDeclaration,
    /// Another markup declaration (`<!ELEMENT`, `<!ATTLIST`, `<!NOTATION`).
    MarkupDeclaration,
    /// A reference to an entity other than the five predefined ones.
    EntityReference,
    /// An element in the XInclude namespace.
    XInclude,
}

impl Construct {
    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Construct::Doctype => "doctype",
            Construct::EntityDeclaration => "entity_declaration",
            Construct::MarkupDeclaration => "markup_declaration",
            Construct::EntityReference => "entity_reference",
            Construct::XInclude => "xinclude",
        }
    }
}

impl fmt::Display for Construct {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A structural limit of the XML reader (REQ-IO.QIF.XML.PARSE-002). The budget limits
/// (bytes, elements, allocation, wall time) come back as [`XmlError::Harness`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum XmlLimit {
    /// Element nesting depth.
    Depth,
    /// Attributes per element (namespace declarations included).
    Attributes,
    /// Bytes in one name (element, attribute, PI target).
    NameLength,
    /// Bytes in one text node, CDATA section, comment or PI data.
    TextLength,
    /// Bytes in one markup token (a start tag with its attributes, for example).
    TokenLength,
    /// Bytes in one namespace name (URI).
    UriLength,
    /// Namespace bindings in scope at one element (and so declarations on one tag).
    Namespaces,
}

impl XmlLimit {
    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            XmlLimit::Depth => "depth",
            XmlLimit::Attributes => "attributes",
            XmlLimit::NameLength => "name_length",
            XmlLimit::TextLength => "text_length",
            XmlLimit::TokenLength => "token_length",
            XmlLimit::UriLength => "uri_length",
            XmlLimit::Namespaces => "namespaces",
        }
    }
}

impl fmt::Display for XmlLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why reading stopped. Every variant carries a [`Position`] with byte offset, line and
/// column (REQ-IO.QIF.XML.PARSE-003). No variant holds text copied from the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmlError {
    /// A DTD, entity or XInclude construct (`xml_forbidden_construct`).
    ForbiddenConstruct {
        /// What was found.
        construct: Construct,
        /// Where it starts.
        at: Position,
    },
    /// The declared or detected encoding is neither UTF-8 nor BOM-marked UTF-16
    /// (`xml_encoding_unsupported`).
    EncodingUnsupported {
        /// Where the encoding was detected or declared.
        at: Position,
    },
    /// A structural limit was passed (`limit_exceeded`).
    LimitExceeded {
        /// The limit.
        limit: XmlLimit,
        /// Its configured value.
        value: u64,
        /// Where the item over the limit starts.
        at: Position,
    },
    /// The input is not well-formed namespace-aware XML 1.0 (`malformed`).
    Malformed {
        /// Static, reader-chosen reason (never document text).
        reason: &'static str,
        /// Where.
        at: Position,
    },
    /// The input ends inside a token or before the root element is closed
    /// (`truncated_file`).
    TruncatedFile {
        /// Where the input ended.
        at: Position,
    },
    /// The limit harness or the byte source stopped the read: `limit_exceeded` for the
    /// byte, element, allocation and wall-time budgets, or `io`. Line and column are those
    /// of the token being read.
    Harness(IoError),
}

impl XmlError {
    /// The stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            XmlError::ForbiddenConstruct { .. } => "xml_forbidden_construct",
            XmlError::EncodingUnsupported { .. } => "xml_encoding_unsupported",
            XmlError::LimitExceeded { .. } => "limit_exceeded",
            XmlError::Malformed { .. } => "malformed",
            XmlError::TruncatedFile { .. } => "truncated_file",
            XmlError::Harness(e) => e.code(),
        }
    }

    /// The input position.
    pub fn position(&self) -> Option<Position> {
        match self {
            XmlError::ForbiddenConstruct { at, .. }
            | XmlError::EncodingUnsupported { at }
            | XmlError::LimitExceeded { at, .. }
            | XmlError::Malformed { at, .. }
            | XmlError::TruncatedFile { at } => Some(*at),
            XmlError::Harness(e) => e.position(),
        }
    }
}

impl fmt::Display for XmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            XmlError::ForbiddenConstruct { construct, at } => {
                write!(f, "xml_forbidden_construct: {construct} at {at}")
            }
            XmlError::EncodingUnsupported { at } => write!(f, "xml_encoding_unsupported at {at}"),
            XmlError::LimitExceeded { limit, value, at } => {
                write!(f, "limit_exceeded: limit {limit} = {value} reached at {at}")
            }
            XmlError::Malformed { reason, at } => write!(f, "malformed: {reason} at {at}"),
            XmlError::TruncatedFile { at } => write!(f, "truncated_file: input ends at {at}"),
            XmlError::Harness(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for XmlError {}

impl From<IoError> for XmlError {
    fn from(e: IoError) -> Self {
        XmlError::Harness(e)
    }
}
