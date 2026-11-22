//! Normalised QIF 3.0 Schema IR types, validation tables and RFC 8785 canonical JSON codec
//! (spec node `IO.QIF.GEN.FLAT`).
//!
//! Shared between the build-time generator (`fid-qif-gen`) and the run-time QIF validator
//! (`fid-qif`), so emitted Rust types, JSON Schema and XML validation tables all originate
//! from one canonical `qif-ir.json` artifact with a deterministic SHA-256 digest.
//!
//! Pure, deterministic (`D0`), `BTreeMap` only, bounded decoding, zero panics.

use std::collections::BTreeMap;
use std::fmt;

use fid_canon::{digest as canon_digest, parse_with_limits, to_canonical_bytes, CValue, CanonError};
use fid_types::Digest;

/// Wire schema tag of `qif-ir.json`.
pub const QIF_IR_SCHEMA: &str = "qif-ir/v1";
/// Maximum accepted size of a `qif-ir.json` document (64 MiB).
pub const MAX_IR_BYTES: usize = 64 * 1024 * 1024;
/// Maximum JSON nesting depth accepted when decoding `qif-ir.json`.
pub const MAX_IR_DEPTH: usize = 64;

/// Source position of an IR declaration (manifest-relative file path, 1-based line and column).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IrPos {
    pub file: String,
    pub line: u32,
    pub col: u32,
}

impl fmt::Display for IrPos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.col)
    }
}

/// Expanded XML qualified name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IrQName {
    pub ns: Option<String>,
    pub local: String,
}

impl IrQName {
    pub fn new(ns: Option<String>, local: impl Into<String>) -> Self {
        Self { ns, local: local.into() }
    }

    /// Canonical map key string: `{ns}local` when namespaced, or `local` when unqualified.
    pub fn canonical_key(&self) -> String {
        match &self.ns {
            Some(ns) => format!("{{{ns}}}{}", self.local),
            None => self.local.clone(),
        }
    }

    /// Parses a [`Self::canonical_key`] string back into an [`IrQName`].
    pub fn parse_key(s: &str) -> Option<Self> {
        if let Some(rest) = s.strip_prefix('{') {
            let (ns, local) = rest.split_once('}')?;
            if local.is_empty() {
                return None;
            }
            Some(Self { ns: Some(ns.to_string()), local: local.to_string() })
        } else if !s.is_empty() {
            Some(Self { ns: None, local: s.to_string() })
        } else {
            None
        }
    }
}

impl fmt::Display for IrQName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical_key())
    }
}

/// Upper occurrence bound on a particle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IrMaxOccurs {
    Bounded(u64),
    Unbounded,
}

impl IrMaxOccurs {
    pub fn is_repeated(self) -> bool {
        match self {
            IrMaxOccurs::Unbounded => true,
            IrMaxOccurs::Bounded(n) => n > 1,
        }
    }
}

/// Occurrence bounds `(min, max)` on a particle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IrOccurs {
    pub min: u64,
    pub max: IrMaxOccurs,
}

impl IrOccurs {
    pub const ONE: Self = Self { min: 1, max: IrMaxOccurs::Bounded(1) };

    pub fn is_repeated(self) -> bool {
        self.max.is_repeated()
    }
}

/// Attribute use mode (`optional | required | prohibited`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IrAttributeUse {
    Optional,
    Required,
    Prohibited,
}

impl IrAttributeUse {
    pub fn as_str(self) -> &'static str {
        match self {
            IrAttributeUse::Optional => "optional",
            IrAttributeUse::Required => "required",
            IrAttributeUse::Prohibited => "prohibited",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "optional" => Some(IrAttributeUse::Optional),
            "required" => Some(IrAttributeUse::Required),
            "prohibited" => Some(IrAttributeUse::Prohibited),
            _ => None,
        }
    }
}

/// `xs:whiteSpace` facet mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IrWhiteSpace {
    Preserve,
    Replace,
    Collapse,
}

impl IrWhiteSpace {
    pub fn as_str(self) -> &'static str {
        match self {
            IrWhiteSpace::Preserve => "preserve",
            IrWhiteSpace::Replace => "replace",
            IrWhiteSpace::Collapse => "collapse",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "preserve" => Some(IrWhiteSpace::Preserve),
            "replace" => Some(IrWhiteSpace::Replace),
            "collapse" => Some(IrWhiteSpace::Collapse),
            _ => None,
        }
    }
}

/// Normalised constraining facet on a simple type or simple-content complex type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IrFacet {
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
    WhiteSpace(IrWhiteSpace),
}

/// Flattened content-model particle (all `xs:group` references expanded and all anonymous
/// types promoted to named entries in `SchemaIr::types`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IrParticle {
    Sequence {
        occurs: IrOccurs,
        items: Vec<IrParticle>,
    },
    Choice {
        occurs: IrOccurs,
        items: Vec<IrParticle>,
    },
    All {
        occurs: IrOccurs,
        items: Vec<IrParticle>,
    },
    Element {
        name: IrQName,
        ty: IrQName,
        occurs: IrOccurs,
        nillable: bool,
        default: Option<String>,
        fixed: Option<String>,
    },
    ElementRef {
        target: IrQName,
        occurs: IrOccurs,
    },
}

/// Flattened attribute declaration on a complex type (`attributeGroup` and `attribute/@ref`
/// expanded, `prohibited` restrictions removed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrAttribute {
    pub name: IrQName,
    pub ty: IrQName,
    pub use_kind: IrAttributeUse,
    pub default: Option<String>,
    pub fixed: Option<String>,
}

/// Variety of a normalised type definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypeKind {
    Complex,
    Simple,
    List,
    Union,
}

impl TypeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TypeKind::Complex => "complex",
            TypeKind::Simple => "simple",
            TypeKind::List => "list",
            TypeKind::Union => "union",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "complex" => Some(TypeKind::Complex),
            "simple" => Some(TypeKind::Simple),
            "list" => Some(TypeKind::List),
            "union" => Some(TypeKind::Union),
            _ => None,
        }
    }
}

/// Normalised type definition (REQ-IO.QIF.GEN.FLAT-001, -002).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDef {
    pub name: IrQName,
    pub kind: TypeKind,
    pub is_abstract: bool,
    pub mixed: bool,
    /// Direct base type QName, if derived.
    pub base: Option<IrQName>,
    /// Underlying simple type QName (for `Simple` or `Complex` with `simpleContent`).
    pub simple_base: Option<IrQName>,
    /// Item type QName (for `TypeKind::List`).
    pub item_type: Option<IrQName>,
    /// Member type QNames in source order (for `TypeKind::Union`).
    pub member_types: Vec<IrQName>,
    /// Flattened top-level content particles (inherited base particles precede derived additions).
    pub particles: Vec<IrParticle>,
    /// Flattened attributes (inherited attributes precede derived additions, overrides applied).
    pub attributes: Vec<IrAttribute>,
    /// Accumulated constraining facets (base facets precede derived facets).
    pub facets: Vec<IrFacet>,
    /// Declaration source location.
    pub source: IrPos,
}

/// Normalised global element declaration (REQ-IO.QIF.GEN.FLAT-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementDef {
    pub name: IrQName,
    pub ty: IrQName,
    pub is_abstract: bool,
    /// False whenever `is_abstract == true` (REQ-IO.QIF.GEN.FLAT-003).
    pub instantiable: bool,
    pub nillable: bool,
    pub substitution_group: Option<IrQName>,
    pub default: Option<String>,
    pub fixed: Option<String>,
    pub source: IrPos,
}

/// Repeated child linked to a derived count attribute (`n` / `count`) (REQ-IO.QIF.GEN.FLAT-004).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CountTarget {
    /// A single repeated element or elementRef particle.
    Element(IrQName),
    /// A single repeated `xs:choice` particle covering the listed child elements.
    Choice(Vec<IrQName>),
    /// A single repeated `xs:sequence` particle covering the listed child elements.
    Sequence(Vec<IrQName>),
}

/// Link from a derived count attribute (`n`, `count`) on `owner_type` to its repeated child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountRule {
    pub owner_type: IrQName,
    pub attribute: IrQName,
    pub target: CountTarget,
    pub source: IrPos,
}

/// Kind of identity constraint (`key | keyref | unique`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IrIdentityKind {
    Key,
    Keyref,
    Unique,
}

impl IrIdentityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            IrIdentityKind::Key => "key",
            IrIdentityKind::Keyref => "keyref",
            IrIdentityKind::Unique => "unique",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "key" => Some(IrIdentityKind::Key),
            "keyref" => Some(IrIdentityKind::Keyref),
            "unique" => Some(IrIdentityKind::Unique),
            _ => None,
        }
    }
}

/// Compiled identity constraint table entry for `IO.QIF.XML.VALID`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityTable {
    /// Canonical path of the owning element (e.g. `{ns}QIFDocument` or `{ns}Owner/{ns}Child`).
    pub owner_element: String,
    pub kind: IrIdentityKind,
    pub name: IrQName,
    pub refer: Option<IrQName>,
    pub selector: String,
    pub fields: Vec<String>,
    pub source: IrPos,
}

/// Complete normalised Schema IR consumed by Rust/JSON-Schema emitters and `IO.QIF.XML.VALID`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaIr {
    pub schema_version: String,
    /// All named and promoted-anonymous types keyed by `IrQName::canonical_key()`.
    pub types: BTreeMap<String, TypeDef>,
    /// All top-level global elements keyed by `IrQName::canonical_key()`.
    pub elements: BTreeMap<String, ElementDef>,
    /// Substitution-group head canonical key -> instantiable member QNames in source order.
    pub substitution: BTreeMap<String, Vec<IrQName>>,
    /// Derived count attribute rules (`n`, `count`) sorted by `(owner_type, attribute)`.
    pub count_rules: Vec<CountRule>,
    /// Compiled identity constraint tables sorted by `(owner_element, name, source)`.
    pub identity: Vec<IdentityTable>,
    /// One-to-one map from normalised IR type canonical key -> emitted Rust/JSON-Schema name.
    pub names: BTreeMap<String, String>,
}

/// Codec error when encoding or decoding `qif-ir.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IrError {
    pub detail: String,
}

impl IrError {
    pub fn new(detail: impl Into<String>) -> Self {
        Self { detail: detail.into() }
    }
}

impl fmt::Display for IrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "qif_ir_error: {}", self.detail)
    }
}

impl std::error::Error for IrError {}

// ------------------------------------------------------------------ canonical JSON encoding

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn u64_val(n: u64) -> CValue {
    CValue::Str(n.to_string())
}

fn pos_to_value(p: &IrPos) -> CValue {
    let mut m = BTreeMap::new();
    m.insert("col".to_string(), CValue::Num(f64::from(p.col)));
    m.insert("file".to_string(), s(&p.file));
    m.insert("line".to_string(), CValue::Num(f64::from(p.line)));
    CValue::Obj(m)
}

fn qname_to_value(q: &IrQName) -> CValue {
    let mut m = BTreeMap::new();
    m.insert("local".to_string(), s(&q.local));
    if let Some(ns) = &q.ns {
        m.insert("ns".to_string(), s(ns));
    }
    CValue::Obj(m)
}

fn occurs_to_value(o: IrOccurs) -> CValue {
    let mut m = BTreeMap::new();
    m.insert(
        "max".to_string(),
        match o.max {
            IrMaxOccurs::Bounded(n) => u64_val(n),
            IrMaxOccurs::Unbounded => s("unbounded"),
        },
    );
    m.insert("min".to_string(), u64_val(o.min));
    CValue::Obj(m)
}

fn facet_to_value(f: &IrFacet) -> CValue {
    let mut m = BTreeMap::new();
    match f {
        IrFacet::Enumeration(v) => {
            m.insert("kind".to_string(), s("enumeration"));
            m.insert("value".to_string(), s(v));
        }
        IrFacet::Pattern(v) => {
            m.insert("kind".to_string(), s("pattern"));
            m.insert("value".to_string(), s(v));
        }
        IrFacet::Length(n) => {
            m.insert("kind".to_string(), s("length"));
            m.insert("value".to_string(), u64_val(*n));
        }
        IrFacet::MinLength(n) => {
            m.insert("kind".to_string(), s("minLength"));
            m.insert("value".to_string(), u64_val(*n));
        }
        IrFacet::MaxLength(n) => {
            m.insert("kind".to_string(), s("maxLength"));
            m.insert("value".to_string(), u64_val(*n));
        }
        IrFacet::MinInclusive(v) => {
            m.insert("kind".to_string(), s("minInclusive"));
            m.insert("value".to_string(), s(v));
        }
        IrFacet::MaxInclusive(v) => {
            m.insert("kind".to_string(), s("maxInclusive"));
            m.insert("value".to_string(), s(v));
        }
        IrFacet::MinExclusive(v) => {
            m.insert("kind".to_string(), s("minExclusive"));
            m.insert("value".to_string(), s(v));
        }
        IrFacet::MaxExclusive(v) => {
            m.insert("kind".to_string(), s("maxExclusive"));
            m.insert("value".to_string(), s(v));
        }
        IrFacet::TotalDigits(n) => {
            m.insert("kind".to_string(), s("totalDigits"));
            m.insert("value".to_string(), u64_val(*n));
        }
        IrFacet::FractionDigits(n) => {
            m.insert("kind".to_string(), s("fractionDigits"));
            m.insert("value".to_string(), u64_val(*n));
        }
        IrFacet::WhiteSpace(w) => {
            m.insert("kind".to_string(), s("whiteSpace"));
            m.insert("value".to_string(), s(w.as_str()));
        }
    }
    CValue::Obj(m)
}

fn particle_to_value(p: &IrParticle) -> CValue {
    let mut m = BTreeMap::new();
    match p {
        IrParticle::Sequence { occurs, items } => {
            m.insert("items".to_string(), CValue::Arr(items.iter().map(particle_to_value).collect()));
            m.insert("kind".to_string(), s("sequence"));
            m.insert("occurs".to_string(), occurs_to_value(*occurs));
        }
        IrParticle::Choice { occurs, items } => {
            m.insert("items".to_string(), CValue::Arr(items.iter().map(particle_to_value).collect()));
            m.insert("kind".to_string(), s("choice"));
            m.insert("occurs".to_string(), occurs_to_value(*occurs));
        }
        IrParticle::All { occurs, items } => {
            m.insert("items".to_string(), CValue::Arr(items.iter().map(particle_to_value).collect()));
            m.insert("kind".to_string(), s("all"));
            m.insert("occurs".to_string(), occurs_to_value(*occurs));
        }
        IrParticle::Element { name, ty, occurs, nillable, default, fixed } => {
            if let Some(d) = default {
                m.insert("default".to_string(), s(d));
            }
            if let Some(fx) = fixed {
                m.insert("fixed".to_string(), s(fx));
            }
            m.insert("kind".to_string(), s("element"));
            m.insert("name".to_string(), qname_to_value(name));
            m.insert("nillable".to_string(), CValue::Bool(*nillable));
            m.insert("occurs".to_string(), occurs_to_value(*occurs));
            m.insert("type".to_string(), qname_to_value(ty));
        }
        IrParticle::ElementRef { target, occurs } => {
            m.insert("kind".to_string(), s("element_ref"));
            m.insert("occurs".to_string(), occurs_to_value(*occurs));
            m.insert("target".to_string(), qname_to_value(target));
        }
    }
    CValue::Obj(m)
}

fn attr_to_value(a: &IrAttribute) -> CValue {
    let mut m = BTreeMap::new();
    if let Some(d) = &a.default {
        m.insert("default".to_string(), s(d));
    }
    if let Some(fx) = &a.fixed {
        m.insert("fixed".to_string(), s(fx));
    }
    m.insert("name".to_string(), qname_to_value(&a.name));
    m.insert("type".to_string(), qname_to_value(&a.ty));
    m.insert("use".to_string(), s(a.use_kind.as_str()));
    CValue::Obj(m)
}

fn typedef_to_value(t: &TypeDef) -> CValue {
    let mut m = BTreeMap::new();
    m.insert("abstract".to_string(), CValue::Bool(t.is_abstract));
    m.insert("attributes".to_string(), CValue::Arr(t.attributes.iter().map(attr_to_value).collect()));
    if let Some(b) = &t.base {
        m.insert("base".to_string(), qname_to_value(b));
    }
    m.insert("facets".to_string(), CValue::Arr(t.facets.iter().map(facet_to_value).collect()));
    if let Some(it) = &t.item_type {
        m.insert("item_type".to_string(), qname_to_value(it));
    }
    m.insert("kind".to_string(), s(t.kind.as_str()));
    if !t.member_types.is_empty() {
        m.insert("member_types".to_string(), CValue::Arr(t.member_types.iter().map(qname_to_value).collect()));
    }
    m.insert("mixed".to_string(), CValue::Bool(t.mixed));
    m.insert("name".to_string(), qname_to_value(&t.name));
    m.insert("particles".to_string(), CValue::Arr(t.particles.iter().map(particle_to_value).collect()));
    if let Some(sb) = &t.simple_base {
        m.insert("simple_base".to_string(), qname_to_value(sb));
    }
    m.insert("source".to_string(), pos_to_value(&t.source));
    CValue::Obj(m)
}

fn elemdef_to_value(e: &ElementDef) -> CValue {
    let mut m = BTreeMap::new();
    m.insert("abstract".to_string(), CValue::Bool(e.is_abstract));
    if let Some(d) = &e.default {
        m.insert("default".to_string(), s(d));
    }
    if let Some(fx) = &e.fixed {
        m.insert("fixed".to_string(), s(fx));
    }
    m.insert("instantiable".to_string(), CValue::Bool(e.instantiable));
    m.insert("name".to_string(), qname_to_value(&e.name));
    m.insert("nillable".to_string(), CValue::Bool(e.nillable));
    m.insert("source".to_string(), pos_to_value(&e.source));
    if let Some(sg) = &e.substitution_group {
        m.insert("substitution_group".to_string(), qname_to_value(sg));
    }
    m.insert("type".to_string(), qname_to_value(&e.ty));
    CValue::Obj(m)
}

fn count_rule_to_value(r: &CountRule) -> CValue {
    let mut m = BTreeMap::new();
    m.insert("attribute".to_string(), qname_to_value(&r.attribute));
    m.insert("owner_type".to_string(), qname_to_value(&r.owner_type));
    m.insert("source".to_string(), pos_to_value(&r.source));
    let mut t = BTreeMap::new();
    match &r.target {
        CountTarget::Element(q) => {
            t.insert("element".to_string(), qname_to_value(q));
            t.insert("kind".to_string(), s("element"));
        }
        CountTarget::Choice(qs) => {
            t.insert("kind".to_string(), s("choice"));
            t.insert("members".to_string(), CValue::Arr(qs.iter().map(qname_to_value).collect()));
        }
        CountTarget::Sequence(qs) => {
            t.insert("kind".to_string(), s("sequence"));
            t.insert("members".to_string(), CValue::Arr(qs.iter().map(qname_to_value).collect()));
        }
    }
    m.insert("target".to_string(), CValue::Obj(t));
    CValue::Obj(m)
}

fn identity_to_value(id: &IdentityTable) -> CValue {
    let mut m = BTreeMap::new();
    m.insert("fields".to_string(), CValue::Arr(id.fields.iter().map(|x| s(x)).collect()));
    m.insert("kind".to_string(), s(id.kind.as_str()));
    m.insert("name".to_string(), qname_to_value(&id.name));
    m.insert("owner_element".to_string(), s(&id.owner_element));
    if let Some(r) = &id.refer {
        m.insert("refer".to_string(), qname_to_value(r));
    }
    m.insert("selector".to_string(), s(&id.selector));
    m.insert("source".to_string(), pos_to_value(&id.source));
    CValue::Obj(m)
}

impl SchemaIr {
    /// Converts the normalised IR into canonical JSON (`CValue`).
    pub fn to_value(&self) -> CValue {
        let mut root = BTreeMap::new();
        root.insert("count_rules".to_string(), CValue::Arr(self.count_rules.iter().map(count_rule_to_value).collect()));
        let elems: BTreeMap<String, CValue> =
            self.elements.iter().map(|(k, v)| (k.clone(), elemdef_to_value(v))).collect();
        root.insert("elements".to_string(), CValue::Obj(elems));
        root.insert("identity".to_string(), CValue::Arr(self.identity.iter().map(identity_to_value).collect()));
        let names: BTreeMap<String, CValue> = self.names.iter().map(|(k, v)| (k.clone(), s(v))).collect();
        root.insert("names".to_string(), CValue::Obj(names));
        root.insert("schema_version".to_string(), s(&self.schema_version));
        let subst: BTreeMap<String, CValue> = self
            .substitution
            .iter()
            .map(|(k, v)| (k.clone(), CValue::Arr(v.iter().map(qname_to_value).collect())))
            .collect();
        root.insert("substitution".to_string(), CValue::Obj(subst));
        let types: BTreeMap<String, CValue> =
            self.types.iter().map(|(k, v)| (k.clone(), typedef_to_value(v))).collect();
        root.insert("types".to_string(), CValue::Obj(types));
        CValue::Obj(root)
    }

    /// Serialises to RFC 8785 canonical JSON bytes (`qif-ir.json`; REQ-IO.QIF.GEN.FLAT-005).
    pub fn to_canonical_json(&self) -> Result<Vec<u8>, IrError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| IrError::new("failed to encode SchemaIr as canonical JSON"))
    }

    /// Computes the SHA-256 digest of [`Self::to_canonical_json`] (REQ-IO.QIF.GEN.FLAT-005).
    pub fn digest(&self) -> Result<Digest, IrError> {
        let d = canon_digest(&self.to_value()).map_err(|_| IrError::new("failed to digest SchemaIr"))?;
        Ok(Digest::from_bytes(d.0))
    }

    /// Parses a `qif-ir.json` document with bounded byte size ([`MAX_IR_BYTES`]) and nesting
    /// depth ([`MAX_IR_DEPTH`]).
    pub fn from_json(bytes: &[u8]) -> Result<Self, IrError> {
        if bytes.len() > MAX_IR_BYTES {
            return Err(IrError::new(format!("qif-ir.json exceeds {MAX_IR_BYTES} bytes")));
        }
        let val = parse_with_limits(bytes, MAX_IR_BYTES, MAX_IR_DEPTH).map_err(|e| match e {
            CanonError::InputLimit => IrError::new("qif-ir.json exceeds byte or depth limit"),
            CanonError::DuplicateKey(k) => IrError::new(format!("duplicate key: {k}")),
            _ => IrError::new("malformed qif-ir.json"),
        })?;
        Self::from_value(&val)
    }

    /// Decodes a [`SchemaIr`] from a parsed [`CValue`].
    pub fn from_value(v: &CValue) -> Result<Self, IrError> {
        let root = obj(v, "root")?;
        let schema_version = req_str(root, "schema_version")?.to_string();
        if schema_version != QIF_IR_SCHEMA {
            return Err(IrError::new(format!("unsupported schema_version: {schema_version}")));
        }

        let types_obj = obj(req(root, "types")?, "types")?;
        let mut types = BTreeMap::new();
        for (k, tv) in types_obj {
            types.insert(k.clone(), typedef_from_value(tv)?);
        }

        let elems_obj = obj(req(root, "elements")?, "elements")?;
        let mut elements = BTreeMap::new();
        for (k, ev) in elems_obj {
            elements.insert(k.clone(), elemdef_from_value(ev)?);
        }

        let subst_obj = obj(req(root, "substitution")?, "substitution")?;
        let mut substitution = BTreeMap::new();
        for (k, sv) in subst_obj {
            let items = arr(sv, "substitution entry")?;
            let mut members = Vec::with_capacity(items.len());
            for item in items {
                members.push(qname_from_value(item)?);
            }
            substitution.insert(k.clone(), members);
        }

        let rules_arr = arr(req(root, "count_rules")?, "count_rules")?;
        let mut count_rules = Vec::with_capacity(rules_arr.len());
        for rv in rules_arr {
            count_rules.push(count_rule_from_value(rv)?);
        }

        let id_arr = arr(req(root, "identity")?, "identity")?;
        let mut identity = Vec::with_capacity(id_arr.len());
        for iv in id_arr {
            identity.push(identity_from_value(iv)?);
        }

        let names_obj = obj(req(root, "names")?, "names")?;
        let mut names = BTreeMap::new();
        for (k, nv) in names_obj {
            let CValue::Str(emitted) = nv else {
                return Err(IrError::new("names values must be strings"));
            };
            names.insert(k.clone(), emitted.clone());
        }

        Ok(Self { schema_version, types, elements, substitution, count_rules, identity, names })
    }
}

// ------------------------------------------------------------------ decoding helpers

fn obj<'a>(v: &'a CValue, ctx: &str) -> Result<&'a BTreeMap<String, CValue>, IrError> {
    match v {
        CValue::Obj(m) => Ok(m),
        _ => Err(IrError::new(format!("{ctx}: expected object"))),
    }
}

fn arr<'a>(v: &'a CValue, ctx: &str) -> Result<&'a [CValue], IrError> {
    match v {
        CValue::Arr(a) => Ok(a),
        _ => Err(IrError::new(format!("{ctx}: expected array"))),
    }
}

fn req<'a>(m: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a CValue, IrError> {
    m.get(k).ok_or_else(|| IrError::new(format!("missing required key: {k}")))
}

fn req_str<'a>(m: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, IrError> {
    match req(m, k)? {
        CValue::Str(s) => Ok(s),
        _ => Err(IrError::new(format!("{k}: expected string"))),
    }
}

fn opt_str(m: &BTreeMap<String, CValue>, k: &str) -> Result<Option<String>, IrError> {
    match m.get(k) {
        None => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.clone())),
        Some(_) => Err(IrError::new(format!("{k}: expected string"))),
    }
}

fn req_bool(m: &BTreeMap<String, CValue>, k: &str) -> Result<bool, IrError> {
    match req(m, k)? {
        CValue::Bool(b) => Ok(*b),
        _ => Err(IrError::new(format!("{k}: expected boolean"))),
    }
}

fn req_u32(m: &BTreeMap<String, CValue>, k: &str) -> Result<u32, IrError> {
    match req(m, k)? {
        CValue::Num(n) if *n >= 0.0 && *n <= f64::from(u32::MAX) && n.fract() == 0.0 => Ok(*n as u32),
        _ => Err(IrError::new(format!("{k}: expected u32"))),
    }
}

fn parse_u64_str(s: &str, ctx: &str) -> Result<u64, IrError> {
    s.parse::<u64>().map_err(|_| IrError::new(format!("{ctx}: invalid u64: {s}")))
}

fn pos_from_value(v: &CValue) -> Result<IrPos, IrError> {
    let m = obj(v, "source")?;
    Ok(IrPos { file: req_str(m, "file")?.to_string(), line: req_u32(m, "line")?, col: req_u32(m, "col")? })
}

fn qname_from_value(v: &CValue) -> Result<IrQName, IrError> {
    let m = obj(v, "qname")?;
    Ok(IrQName { ns: opt_str(m, "ns")?, local: req_str(m, "local")?.to_string() })
}

fn occurs_from_value(v: &CValue) -> Result<IrOccurs, IrError> {
    let m = obj(v, "occurs")?;
    let min = parse_u64_str(req_str(m, "min")?, "occurs.min")?;
    let max_s = req_str(m, "max")?;
    let max = if max_s == "unbounded" {
        IrMaxOccurs::Unbounded
    } else {
        IrMaxOccurs::Bounded(parse_u64_str(max_s, "occurs.max")?)
    };
    Ok(IrOccurs { min, max })
}

fn facet_from_value(v: &CValue) -> Result<IrFacet, IrError> {
    let m = obj(v, "facet")?;
    let kind = req_str(m, "kind")?;
    let val = req_str(m, "value")?;
    match kind {
        "enumeration" => Ok(IrFacet::Enumeration(val.to_string())),
        "pattern" => Ok(IrFacet::Pattern(val.to_string())),
        "length" => Ok(IrFacet::Length(parse_u64_str(val, "facet.length")?)),
        "minLength" => Ok(IrFacet::MinLength(parse_u64_str(val, "facet.minLength")?)),
        "maxLength" => Ok(IrFacet::MaxLength(parse_u64_str(val, "facet.maxLength")?)),
        "minInclusive" => Ok(IrFacet::MinInclusive(val.to_string())),
        "maxInclusive" => Ok(IrFacet::MaxInclusive(val.to_string())),
        "minExclusive" => Ok(IrFacet::MinExclusive(val.to_string())),
        "maxExclusive" => Ok(IrFacet::MaxExclusive(val.to_string())),
        "totalDigits" => Ok(IrFacet::TotalDigits(parse_u64_str(val, "facet.totalDigits")?)),
        "fractionDigits" => Ok(IrFacet::FractionDigits(parse_u64_str(val, "facet.fractionDigits")?)),
        "whiteSpace" => IrWhiteSpace::parse(val)
            .map(IrFacet::WhiteSpace)
            .ok_or_else(|| IrError::new(format!("invalid whiteSpace facet: {val}"))),
        _ => Err(IrError::new(format!("unknown facet kind: {kind}"))),
    }
}

fn particle_from_value(v: &CValue) -> Result<IrParticle, IrError> {
    let m = obj(v, "particle")?;
    let kind = req_str(m, "kind")?;
    let occurs = occurs_from_value(req(m, "occurs")?)?;
    match kind {
        "sequence" | "choice" | "all" => {
            let raw_items = arr(req(m, "items")?, "particle.items")?;
            let mut items = Vec::with_capacity(raw_items.len());
            for item in raw_items {
                items.push(particle_from_value(item)?);
            }
            match kind {
                "sequence" => Ok(IrParticle::Sequence { occurs, items }),
                "choice" => Ok(IrParticle::Choice { occurs, items }),
                _ => Ok(IrParticle::All { occurs, items }),
            }
        }
        "element" => Ok(IrParticle::Element {
            name: qname_from_value(req(m, "name")?)?,
            ty: qname_from_value(req(m, "type")?)?,
            occurs,
            nillable: req_bool(m, "nillable")?,
            default: opt_str(m, "default")?,
            fixed: opt_str(m, "fixed")?,
        }),
        "element_ref" => Ok(IrParticle::ElementRef { target: qname_from_value(req(m, "target")?)?, occurs }),
        _ => Err(IrError::new(format!("unknown particle kind: {kind}"))),
    }
}

fn attr_from_value(v: &CValue) -> Result<IrAttribute, IrError> {
    let m = obj(v, "attribute")?;
    let use_str = req_str(m, "use")?;
    let use_kind =
        IrAttributeUse::parse(use_str).ok_or_else(|| IrError::new(format!("invalid attribute use: {use_str}")))?;
    Ok(IrAttribute {
        name: qname_from_value(req(m, "name")?)?,
        ty: qname_from_value(req(m, "type")?)?,
        use_kind,
        default: opt_str(m, "default")?,
        fixed: opt_str(m, "fixed")?,
    })
}

fn typedef_from_value(v: &CValue) -> Result<TypeDef, IrError> {
    let m = obj(v, "typedef")?;
    let kind_str = req_str(m, "kind")?;
    let kind = TypeKind::parse(kind_str).ok_or_else(|| IrError::new(format!("invalid type kind: {kind_str}")))?;
    let base = match m.get("base") {
        None => None,
        Some(b) => Some(qname_from_value(b)?),
    };
    let simple_base = match m.get("simple_base") {
        None => None,
        Some(b) => Some(qname_from_value(b)?),
    };
    let item_type = match m.get("item_type") {
        None => None,
        Some(b) => Some(qname_from_value(b)?),
    };
    let mut member_types = Vec::new();
    if let Some(mt) = m.get("member_types") {
        for item in arr(mt, "member_types")? {
            member_types.push(qname_from_value(item)?);
        }
    }
    let mut particles = Vec::new();
    for item in arr(req(m, "particles")?, "particles")? {
        particles.push(particle_from_value(item)?);
    }
    let mut attributes = Vec::new();
    for item in arr(req(m, "attributes")?, "attributes")? {
        attributes.push(attr_from_value(item)?);
    }
    let mut facets = Vec::new();
    for item in arr(req(m, "facets")?, "facets")? {
        facets.push(facet_from_value(item)?);
    }
    Ok(TypeDef {
        name: qname_from_value(req(m, "name")?)?,
        kind,
        is_abstract: req_bool(m, "abstract")?,
        mixed: req_bool(m, "mixed")?,
        base,
        simple_base,
        item_type,
        member_types,
        particles,
        attributes,
        facets,
        source: pos_from_value(req(m, "source")?)?,
    })
}

fn elemdef_from_value(v: &CValue) -> Result<ElementDef, IrError> {
    let m = obj(v, "elemdef")?;
    let substitution_group = match m.get("substitution_group") {
        None => None,
        Some(sg) => Some(qname_from_value(sg)?),
    };
    Ok(ElementDef {
        name: qname_from_value(req(m, "name")?)?,
        ty: qname_from_value(req(m, "type")?)?,
        is_abstract: req_bool(m, "abstract")?,
        instantiable: req_bool(m, "instantiable")?,
        nillable: req_bool(m, "nillable")?,
        substitution_group,
        default: opt_str(m, "default")?,
        fixed: opt_str(m, "fixed")?,
        source: pos_from_value(req(m, "source")?)?,
    })
}

fn count_rule_from_value(v: &CValue) -> Result<CountRule, IrError> {
    let m = obj(v, "count_rule")?;
    let tm = obj(req(m, "target")?, "count_rule.target")?;
    let target = match req_str(tm, "kind")? {
        "element" => CountTarget::Element(qname_from_value(req(tm, "element")?)?),
        "choice" => {
            let mut qs = Vec::new();
            for item in arr(req(tm, "members")?, "count_rule.target.members")? {
                qs.push(qname_from_value(item)?);
            }
            CountTarget::Choice(qs)
        }
        "sequence" => {
            let mut qs = Vec::new();
            for item in arr(req(tm, "members")?, "count_rule.target.members")? {
                qs.push(qname_from_value(item)?);
            }
            CountTarget::Sequence(qs)
        }
        other => return Err(IrError::new(format!("unknown count target kind: {other}"))),
    };
    Ok(CountRule {
        owner_type: qname_from_value(req(m, "owner_type")?)?,
        attribute: qname_from_value(req(m, "attribute")?)?,
        target,
        source: pos_from_value(req(m, "source")?)?,
    })
}

fn identity_from_value(v: &CValue) -> Result<IdentityTable, IrError> {
    let m = obj(v, "identity")?;
    let kind_str = req_str(m, "kind")?;
    let kind =
        IrIdentityKind::parse(kind_str).ok_or_else(|| IrError::new(format!("invalid identity kind: {kind_str}")))?;
    let refer = match m.get("refer") {
        None => None,
        Some(r) => Some(qname_from_value(r)?),
    };
    let mut fields = Vec::new();
    for fv in arr(req(m, "fields")?, "identity.fields")? {
        let CValue::Str(fs) = fv else {
            return Err(IrError::new("identity.fields must be strings"));
        };
        fields.push(fs.clone());
    }
    Ok(IdentityTable {
        owner_element: req_str(m, "owner_element")?.to_string(),
        kind,
        name: qname_from_value(req(m, "name")?)?,
        refer,
        selector: req_str(m, "selector")?.to_string(),
        fields,
        source: pos_from_value(req(m, "source")?)?,
    })
}
