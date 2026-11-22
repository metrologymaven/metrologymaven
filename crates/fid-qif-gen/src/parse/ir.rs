//! Raw Schema IR types and error taxonomy (spec node `IO.QIF.GEN.PARSE`).

use crate::pin::PinError;
use std::collections::BTreeMap;
use std::fmt;

/// W3C XML Schema 1.0 namespace URI.
pub const XS_NS: &str = "http://www.w3.org/2001/XMLSchema";
/// Built-in `xml:` namespace URI.
pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// Verified pinned file path (manifest-relative, `/`-separated).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub String);

impl FileId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Source location of an IR node or diagnostic (1-based line and column).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pos {
    pub file: FileId,
    pub line: u32,
    pub col: u32,
}

impl fmt::Display for Pos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.col)
    }
}

/// Expanded XML qualified name (`ns` is `None` for no-namespace declarations).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QName {
    pub ns: Option<String>,
    pub local: String,
}

impl QName {
    pub fn new(ns: Option<String>, local: impl Into<String>) -> QName {
        QName { ns, local: local.into() }
    }

    pub fn xs(local: impl Into<String>) -> QName {
        QName { ns: Some(XS_NS.to_string()), local: local.into() }
    }
}

impl fmt::Display for QName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.ns {
            Some(ns) => write!(f, "{{{ns}}}{}", self.local),
            None => f.write_str(&self.local),
        }
    }
}

/// Documentation text extracted from `xs:annotation/xs:documentation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    pub text: String,
    pub pos: Pos,
}

/// Upper bound of a particle's `maxOccurs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaxOccurs {
    Bounded(u64),
    Unbounded,
}

/// Occurrence bounds on a particle (`minOccurs`, `maxOccurs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Occurs {
    pub min: u64,
    pub max: MaxOccurs,
}

impl Default for Occurs {
    fn default() -> Self {
        Occurs { min: 1, max: MaxOccurs::Bounded(1) }
    }
}

/// `xs:attribute/@use`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeUse {
    Optional,
    Required,
    Prohibited,
}

impl AttributeUse {
    pub fn as_str(self) -> &'static str {
        match self {
            AttributeUse::Optional => "optional",
            AttributeUse::Required => "required",
            AttributeUse::Prohibited => "prohibited",
        }
    }
}

/// `elementFormDefault` / `attributeFormDefault` / `@form`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormDefault {
    Qualified,
    Unqualified,
}

/// `xs:whiteSpace` facet value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhiteSpaceMode {
    Preserve,
    Replace,
    Collapse,
}

impl WhiteSpaceMode {
    pub fn as_str(self) -> &'static str {
        match self {
            WhiteSpaceMode::Preserve => "preserve",
            WhiteSpaceMode::Replace => "replace",
            WhiteSpaceMode::Collapse => "collapse",
        }
    }
}

/// Constraining facet payload on a simple type or simple content restriction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FacetKind {
    Enumeration(String),
    Pattern(String),
    Length(u64),
    MinLength(u64),
    MaxLength(u64),
    MinInclusive(String),
    MaxInclusive(String),
    MinExclusive(String),
    MaxExclusive(String),
    TotalDigits(u64),
    FractionDigits(u64),
    WhiteSpace(WhiteSpaceMode),
}

/// One facet element inside `xs:restriction`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facet {
    pub kind: FacetKind,
    pub fixed: bool,
    pub annotation: Option<Annotation>,
    pub pos: Pos,
}

/// Kind of XSD identity constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityKind {
    Key,
    Keyref,
    Unique,
}

impl IdentityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            IdentityKind::Key => "key",
            IdentityKind::Keyref => "keyref",
            IdentityKind::Unique => "unique",
        }
    }
}

/// `xs:key`, `xs:keyref`, or `xs:unique` with selector and field XPaths kept verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityConstraint {
    pub kind: IdentityKind,
    pub name: QName,
    pub refer: Option<QName>,
    pub selector: String,
    pub fields: Vec<String>,
    pub annotation: Option<Annotation>,
    pub pos: Pos,
}

/// `xs:extension` vs `xs:restriction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivationKind {
    Extension,
    Restriction,
}

impl DerivationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DerivationKind::Extension => "extension",
            DerivationKind::Restriction => "restriction",
        }
    }
}

/// Type of an attribute declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeType {
    Named(QName),
    InlineSimple(Box<SimpleTypeDecl>),
    AnySimpleType,
}

/// An `xs:attribute` declaration (top-level or local).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeDecl {
    pub name: QName,
    pub ty: AttributeType,
    pub use_kind: AttributeUse,
    pub default: Option<String>,
    pub fixed: Option<String>,
    pub annotation: Option<Annotation>,
    pub pos: Pos,
}

/// One attribute item inside a complex type, derivation, or attribute group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeItem {
    Decl(AttributeDecl),
    Ref {
        target: QName,
        use_kind: AttributeUse,
        default: Option<String>,
        fixed: Option<String>,
        annotation: Option<Annotation>,
        pos: Pos,
    },
    GroupRef {
        target: QName,
        annotation: Option<Annotation>,
        pos: Pos,
    },
}

/// Type of an element declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementType {
    Named(QName),
    InlineComplex(Box<ComplexTypeDecl>),
    InlineSimple(Box<SimpleTypeDecl>),
    AnyType,
}

/// An `xs:element` declaration (top-level or local).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementDecl {
    pub name: QName,
    pub ty: ElementType,
    pub substitution_group: Option<QName>,
    pub is_abstract: bool,
    pub nillable: bool,
    pub default: Option<String>,
    pub fixed: Option<String>,
    pub constraints: Vec<IdentityConstraint>,
    pub annotation: Option<Annotation>,
    pub pos: Pos,
}

/// A local `xs:element` declaration inside a particle (carries occurrence bounds).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalElement {
    pub occurs: Occurs,
    pub decl: ElementDecl,
}

/// Content model particle tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Particle {
    Sequence { occurs: Occurs, items: Vec<Particle>, annotation: Option<Annotation>, pos: Pos },
    Choice { occurs: Occurs, items: Vec<Particle>, annotation: Option<Annotation>, pos: Pos },
    All { occurs: Occurs, items: Vec<Particle>, annotation: Option<Annotation>, pos: Pos },
    Element(Box<LocalElement>),
    ElementRef { target: QName, occurs: Occurs, annotation: Option<Annotation>, pos: Pos },
    GroupRef { target: QName, occurs: Occurs, annotation: Option<Annotation>, pos: Pos },
}

/// Content of a `xs:complexType`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComplexContentModel {
    /// Shorthand content directly under `xs:complexType` (optional particle + attributes).
    Direct { particle: Option<Box<Particle>>, attributes: Vec<AttributeItem> },
    /// `xs:simpleContent` (`xs:extension` or `xs:restriction`).
    SimpleContent {
        derivation: DerivationKind,
        base: QName,
        facets: Vec<Facet>,
        attributes: Vec<AttributeItem>,
        pos: Pos,
    },
    /// `xs:complexContent` (`xs:extension` or `xs:restriction`).
    ComplexContent {
        derivation: DerivationKind,
        base: QName,
        mixed: Option<bool>,
        particle: Option<Box<Particle>>,
        attributes: Vec<AttributeItem>,
        pos: Pos,
    },
}

/// An `xs:complexType` declaration (named at top-level or anonymous inline).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComplexTypeDecl {
    pub name: Option<QName>,
    pub is_abstract: bool,
    pub mixed: bool,
    pub content: ComplexContentModel,
    pub annotation: Option<Annotation>,
    pub pos: Pos,
}

/// Variety of an `xs:simpleType`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimpleTypeVariety {
    Restriction { base: Option<QName>, inline_base: Option<Box<SimpleTypeDecl>>, facets: Vec<Facet>, pos: Pos },
    List { item_type: Option<QName>, inline_item: Option<Box<SimpleTypeDecl>>, pos: Pos },
    Union { member_types: Vec<QName>, inline_members: Vec<SimpleTypeDecl>, pos: Pos },
}

/// An `xs:simpleType` declaration (named at top-level or anonymous inline).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleTypeDecl {
    pub name: Option<QName>,
    pub variety: SimpleTypeVariety,
    pub annotation: Option<Annotation>,
    pub pos: Pos,
}

/// Top-level `xs:group` definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupDecl {
    pub name: QName,
    pub particle: Particle,
    pub annotation: Option<Annotation>,
    pub pos: Pos,
}

/// Top-level `xs:attributeGroup` definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeGroupDecl {
    pub name: QName,
    pub attributes: Vec<AttributeItem>,
    pub annotation: Option<Annotation>,
    pub pos: Pos,
}

/// One top-level declaration in a schema file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decl {
    ComplexType(ComplexTypeDecl),
    SimpleType(SimpleTypeDecl),
    Element(ElementDecl),
    Attribute(AttributeDecl),
    Group(GroupDecl),
    AttributeGroup(AttributeGroupDecl),
}

/// Top-level declaration counts for verification against independent scripts (AC-6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeclCounts {
    pub complex_types: usize,
    pub simple_types: usize,
    pub elements: usize,
    pub attributes: usize,
    pub groups: usize,
    pub attribute_groups: usize,
    pub total: usize,
}

/// Raw Schema IR produced by [`crate::parse::parse_pinned`].
///
/// Files appear in sorted manifest order; declarations appear in exact source order within each
/// file (REQ-IO.QIF.GEN.PARSE-006).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawIr {
    pub files: Vec<FileId>,
    pub target_namespaces: BTreeMap<FileId, Option<String>>,
    pub decls: Vec<Decl>,
}

impl RawIr {
    /// Counts top-level declarations by kind.
    pub fn top_level_counts(&self) -> DeclCounts {
        let mut c = DeclCounts::default();
        for d in &self.decls {
            match d {
                Decl::ComplexType(_) => c.complex_types += 1,
                Decl::SimpleType(_) => c.simple_types += 1,
                Decl::Element(_) => c.elements += 1,
                Decl::Attribute(_) => c.attributes += 1,
                Decl::Group(_) => c.groups += 1,
                Decl::AttributeGroup(_) => c.attribute_groups += 1,
            }
            c.total += 1;
        }
        c
    }
}

// ------------------------------------------------------------------ errors

/// Category of an unresolved reference (`xsd_unresolved_ref`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Type,
    Element,
    Attribute,
    Group,
    AttributeGroup,
    IdentityKey,
    NamespacePrefix,
}

impl RefKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RefKind::Type => "type",
            RefKind::Element => "element",
            RefKind::Attribute => "attribute",
            RefKind::Group => "group",
            RefKind::AttributeGroup => "attributeGroup",
            RefKind::IdentityKey => "keyref",
            RefKind::NamespacePrefix => "prefix",
        }
    }
}

/// Errors returned by the XSD parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Unsupported XSD element or attribute (`xsd_construct_unsupported`).
    UnsupportedConstruct { construct: String, pos: Pos },
    /// Reference to an undeclared type, element, attribute, group or key (`xsd_unresolved_ref`).
    UnresolvedRef { kind: RefKind, name: String, pos: Pos },
    /// Forbidden XML construct such as DOCTYPE, entity declaration or XInclude (`xml_forbidden_construct`).
    ForbiddenXml { detail: String, pos: Pos },
    /// A reader or harness limit was reached (`limit_exceeded`).
    LimitExceeded { detail: String, pos: Pos },
    /// Malformed XML or invalid XSD structure (`malformed`).
    Malformed { detail: String, pos: Pos },
    /// Pinned-set error such as an unpinned `include`/`import` target.
    Pin(PinError),
}

impl ParseError {
    /// Stable snake_case error code.
    pub fn code(&self) -> &'static str {
        match self {
            ParseError::UnsupportedConstruct { .. } => "xsd_construct_unsupported",
            ParseError::UnresolvedRef { .. } => "xsd_unresolved_ref",
            ParseError::ForbiddenXml { .. } => "xml_forbidden_construct",
            ParseError::LimitExceeded { .. } => "limit_exceeded",
            ParseError::Malformed { .. } => "malformed",
            ParseError::Pin(e) => e.code(),
        }
    }

    /// Source position associated with this error, if any.
    pub fn pos(&self) -> Option<&Pos> {
        match self {
            ParseError::UnsupportedConstruct { pos, .. }
            | ParseError::UnresolvedRef { pos, .. }
            | ParseError::ForbiddenXml { pos, .. }
            | ParseError::LimitExceeded { pos, .. }
            | ParseError::Malformed { pos, .. } => Some(pos),
            ParseError::Pin(_) => None,
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnsupportedConstruct { construct, pos } => {
                write!(f, "xsd_construct_unsupported: `{construct}` at {pos}")
            }
            ParseError::UnresolvedRef { kind, name, pos } => {
                write!(f, "xsd_unresolved_ref: {} `{name}` at {pos}", kind.as_str())
            }
            ParseError::ForbiddenXml { detail, pos } => write!(f, "xml_forbidden_construct: {detail} at {pos}"),
            ParseError::LimitExceeded { detail, pos } => write!(f, "limit_exceeded: {detail} at {pos}"),
            ParseError::Malformed { detail, pos } => write!(f, "malformed: {detail} at {pos}"),
            ParseError::Pin(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ParseError {}

impl From<PinError> for ParseError {
    fn from(e: PinError) -> Self {
        ParseError::Pin(e)
    }
}
