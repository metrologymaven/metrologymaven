//! Schema IR normalisation, anonymous-type naming, substitution groups, derived count rules,
//! identity tables and `qif-ir.json` canonical output (spec node `IO.QIF.GEN.FLAT`).
//!
//! Turns [`RawIr`] (from `IO.QIF.GEN.PARSE`) into [`SchemaIr`] (`fid-qif-ir`) so Rust codegen,
//! JSON Schema emission and runtime XML validation all share a single normalised source.
//!
//! Pure, deterministic (`D0`), `BTreeMap`/`BTreeSet` only, zero panics.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use fid_qif_ir::{
    CountRule, CountTarget, ElementDef, IdentityTable, IrAttribute, IrAttributeUse, IrError, IrFacet, IrIdentityKind,
    IrMaxOccurs, IrOccurs, IrParticle, IrPos, IrQName, IrWhiteSpace, SchemaIr, TypeDef, TypeKind, QIF_IR_SCHEMA,
};
use fid_types::Digest;

use crate::parse::{
    parse_pinned, Annotation, AttributeDecl, AttributeGroupDecl, AttributeItem, AttributeType, AttributeUse,
    ComplexContentModel, ComplexTypeDecl, Decl, DerivationKind, ElementType, Facet, FacetKind, GroupDecl,
    IdentityConstraint, IdentityKind, MaxOccurs, Occurs, ParseError, Particle, Pos, QName, RawIr, SimpleTypeDecl,
    SimpleTypeVariety, WhiteSpaceMode, XS_NS,
};
use crate::pin::PinnedSet;

/// One unlinked `n`/`count` attribute reported by [`FlatError::CountRulesUnlinked`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnlinkedCountAttr {
    pub owner_type: IrQName,
    pub attribute: IrQName,
    pub source: IrPos,
    pub reason: String,
}

impl fmt::Display for UnlinkedCountAttr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.@{} ({}): {}", self.owner_type, self.attribute, self.source, self.reason)
    }
}

/// Typed errors returned by Schema IR normalisation (`IO.QIF.GEN.FLAT`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlatError {
    /// Two type declarations (or emitted definition names) produced the same name (`ir_name_collision`).
    NameCollision { name: String, first: IrPos, second: IrPos },
    /// A cycle was detected in base-type, group, attributeGroup, or substitution-group edges (`ir_base_cycle`).
    BaseCycle { cycle: Vec<IrQName>, pos: IrPos },
    /// One or more derived count attributes (`n`/`count`) could not be linked (`count_rules_unlinked`).
    CountRulesUnlinked { unlinked: Vec<UnlinkedCountAttr> },
    /// An invalid schema invariant prevented normalisation.
    InvalidSchema { detail: String, pos: IrPos },
    /// Upstream parse error (from [`flatten_pinned`]).
    Parse(ParseError),
    /// Canonical IR codec error.
    Ir(IrError),
}

impl FlatError {
    /// Stable error code matching `spec/io/qif/gen/flat/NODE.md`.
    pub fn code(&self) -> &'static str {
        match self {
            FlatError::NameCollision { .. } => "ir_name_collision",
            FlatError::BaseCycle { .. } => "ir_base_cycle",
            FlatError::CountRulesUnlinked { .. } => "count_rules_unlinked",
            FlatError::InvalidSchema { .. } => "ir_invalid_schema",
            FlatError::Parse(e) => e.code(),
            FlatError::Ir(_) => "qif_ir_error",
        }
    }
}

impl fmt::Display for FlatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FlatError::NameCollision { name, first, second } => {
                write!(f, "ir_name_collision: `{name}` at {first} and {second}")
            }
            FlatError::BaseCycle { cycle, pos } => {
                let chain: Vec<String> = cycle.iter().map(ToString::to_string).collect();
                write!(f, "ir_base_cycle: {} at {pos}", chain.join(" -> "))
            }
            FlatError::CountRulesUnlinked { unlinked } => {
                let items: Vec<String> = unlinked.iter().map(ToString::to_string).collect();
                write!(f, "count_rules_unlinked: [{}]", items.join("; "))
            }
            FlatError::InvalidSchema { detail, pos } => {
                write!(f, "ir_invalid_schema: {detail} at {pos}")
            }
            FlatError::Parse(e) => write!(f, "{e}"),
            FlatError::Ir(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FlatError {}

impl From<ParseError> for FlatError {
    fn from(e: ParseError) -> Self {
        FlatError::Parse(e)
    }
}

impl From<IrError> for FlatError {
    fn from(e: IrError) -> Self {
        FlatError::Ir(e)
    }
}

// ------------------------------------------------------------------ conversion helpers

fn conv_pos(p: &Pos) -> IrPos {
    IrPos { file: p.file.as_str().to_string(), line: p.line, col: p.col }
}

fn conv_qname(q: &QName) -> IrQName {
    IrQName { ns: q.ns.clone(), local: q.local.clone() }
}

fn xs_qname(local: &str) -> IrQName {
    IrQName { ns: Some(XS_NS.to_string()), local: local.to_string() }
}

fn conv_occurs(o: Occurs) -> IrOccurs {
    let max = match o.max {
        MaxOccurs::Bounded(n) => IrMaxOccurs::Bounded(n),
        MaxOccurs::Unbounded => IrMaxOccurs::Unbounded,
    };
    IrOccurs { min: o.min, max }
}

fn conv_attr_use(u: AttributeUse) -> IrAttributeUse {
    match u {
        AttributeUse::Optional => IrAttributeUse::Optional,
        AttributeUse::Required => IrAttributeUse::Required,
        AttributeUse::Prohibited => IrAttributeUse::Prohibited,
    }
}

fn conv_facet(f: &Facet) -> IrFacet {
    match &f.kind {
        FacetKind::Enumeration(v) => IrFacet::Enumeration(v.clone()),
        FacetKind::Pattern(v) => IrFacet::Pattern(v.clone()),
        FacetKind::Length(n) => IrFacet::Length(*n),
        FacetKind::MinLength(n) => IrFacet::MinLength(*n),
        FacetKind::MaxLength(n) => IrFacet::MaxLength(*n),
        FacetKind::MinInclusive(v) => IrFacet::MinInclusive(v.clone()),
        FacetKind::MaxInclusive(v) => IrFacet::MaxInclusive(v.clone()),
        FacetKind::MinExclusive(v) => IrFacet::MinExclusive(v.clone()),
        FacetKind::MaxExclusive(v) => IrFacet::MaxExclusive(v.clone()),
        FacetKind::TotalDigits(n) => IrFacet::TotalDigits(*n),
        FacetKind::FractionDigits(n) => IrFacet::FractionDigits(*n),
        FacetKind::WhiteSpace(w) => IrFacet::WhiteSpace(match w {
            WhiteSpaceMode::Preserve => IrWhiteSpace::Preserve,
            WhiteSpaceMode::Replace => IrWhiteSpace::Replace,
            WhiteSpaceMode::Collapse => IrWhiteSpace::Collapse,
        }),
    }
}

fn conv_id_kind(k: IdentityKind) -> IrIdentityKind {
    match k {
        IdentityKind::Key => IrIdentityKind::Key,
        IdentityKind::Keyref => IrIdentityKind::Keyref,
        IdentityKind::Unique => IrIdentityKind::Unique,
    }
}

/// Deterministic PascalCase emitted name for a normalised IR type local name (REQ-006).
pub fn to_emit_name(local: &str) -> String {
    let mut out = String::with_capacity(local.len());
    for seg in local.split(|c: char| !c.is_ascii_alphanumeric()) {
        if seg.is_empty() {
            continue;
        }
        let mut chars = seg.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.push_str(chars.as_str());
        }
    }
    if out.is_empty() || out.as_bytes()[0].is_ascii_digit() {
        format!("T{out}")
    } else {
        out
    }
}

// ------------------------------------------------------------------ internal promoted model

#[derive(Debug, Clone)]
struct AttrMeta {
    attr: IrAttribute,
    annotation: Option<String>,
    source: IrPos,
}

#[derive(Debug, Clone)]
enum PromotedContent {
    ComplexDirect {
        particle: Option<IrParticle>,
        attributes: Vec<AttrMeta>,
    },
    ComplexDerived {
        derivation: DerivationKind,
        base: IrQName,
        mixed: Option<bool>,
        particle: Option<IrParticle>,
        attributes: Vec<AttrMeta>,
    },
    SimpleContent {
        derivation: DerivationKind,
        base: IrQName,
        facets: Vec<IrFacet>,
        attributes: Vec<AttrMeta>,
    },
    SimpleRestriction {
        base: IrQName,
        facets: Vec<IrFacet>,
    },
    SimpleList {
        item_type: IrQName,
    },
    SimpleUnion {
        member_types: Vec<IrQName>,
    },
}

#[derive(Debug, Clone)]
struct PromotedType {
    name: IrQName,
    is_abstract: bool,
    mixed: bool,
    content: PromotedContent,
    source: IrPos,
}

struct Promoter<'a> {
    groups: BTreeMap<IrQName, &'a GroupDecl>,
    attr_groups: BTreeMap<IrQName, &'a AttributeGroupDecl>,
    global_attrs: BTreeMap<IrQName, &'a AttributeDecl>,
    global_attr_types: BTreeMap<IrQName, IrQName>,
    types: BTreeMap<IrQName, PromotedType>,
    type_positions: BTreeMap<IrQName, IrPos>,
    elements: BTreeMap<String, ElementDef>,
    elements_in_order: Vec<ElementDef>,
    identity: Vec<IdentityTable>,
}

impl<'a> Promoter<'a> {
    fn new(raw: &'a RawIr) -> Self {
        let mut groups = BTreeMap::new();
        let mut attr_groups = BTreeMap::new();
        let mut global_attrs = BTreeMap::new();
        for decl in &raw.decls {
            match decl {
                Decl::Group(g) => {
                    groups.insert(conv_qname(&g.name), g);
                }
                Decl::AttributeGroup(ag) => {
                    attr_groups.insert(conv_qname(&ag.name), ag);
                }
                Decl::Attribute(a) => {
                    global_attrs.insert(conv_qname(&a.name), a);
                }
                _ => {}
            }
        }
        Self {
            groups,
            attr_groups,
            global_attrs,
            global_attr_types: BTreeMap::new(),
            types: BTreeMap::new(),
            type_positions: BTreeMap::new(),
            elements: BTreeMap::new(),
            elements_in_order: Vec::new(),
            identity: Vec::new(),
        }
    }

    fn register_type(&mut self, pt: PromotedType) -> Result<(), FlatError> {
        if let Some(prev) = self.type_positions.get(&pt.name) {
            return Err(FlatError::NameCollision {
                name: pt.name.canonical_key(),
                first: prev.clone(),
                second: pt.source.clone(),
            });
        }
        self.type_positions.insert(pt.name.clone(), pt.source.clone());
        self.types.insert(pt.name.clone(), pt);
        Ok(())
    }

    /// Published anonymous type naming rule (REQ-IO.QIF.GEN.FLAT-002):
    /// `<Enclosing_0>_<Enclosing_1>_..._Anon` in the enclosing target namespace.
    fn anon_name(ns: &Option<String>, path: &[String]) -> IrQName {
        IrQName { ns: ns.clone(), local: format!("{}_Anon", path.join("_")) }
    }

    fn run(mut self, raw: &'a RawIr) -> Result<Self, FlatError> {
        // 1. Promote inline simple types on top-level global attributes first so attribute refs resolve.
        for decl in &raw.decls {
            if let Decl::Attribute(a) = decl {
                let qn = conv_qname(&a.name);
                let path = vec![qn.local.clone()];
                let ty = self.resolve_attr_type(&a.ty, &qn.ns, &path)?;
                self.global_attr_types.insert(qn, ty);
            }
        }

        // 2. Promote all top-level complex types, simple types, and global elements.
        for decl in &raw.decls {
            match decl {
                Decl::ComplexType(ct) => {
                    let Some(name) = &ct.name else {
                        return Err(FlatError::InvalidSchema {
                            detail: "top-level complexType missing name".to_string(),
                            pos: conv_pos(&ct.pos),
                        });
                    };
                    let qn = conv_qname(name);
                    let path = vec![qn.local.clone()];
                    self.promote_complex_type(qn.clone(), ct, &qn.ns, &path, &qn.canonical_key())?;
                }
                Decl::SimpleType(st) => {
                    let Some(name) = &st.name else {
                        return Err(FlatError::InvalidSchema {
                            detail: "top-level simpleType missing name".to_string(),
                            pos: conv_pos(&st.pos),
                        });
                    };
                    let qn = conv_qname(name);
                    let path = vec![qn.local.clone()];
                    self.promote_simple_type(qn.clone(), st, &qn.ns, &path)?;
                }
                Decl::Element(el) => {
                    let qn = conv_qname(&el.name);
                    let path = vec![qn.local.clone()];
                    let owner_path = qn.canonical_key();
                    self.record_identity(&owner_path, &el.constraints);
                    let ty = self.resolve_elem_type(&el.ty, &qn.ns, &path, &owner_path)?;
                    let def = ElementDef {
                        name: qn.clone(),
                        ty,
                        is_abstract: el.is_abstract,
                        instantiable: !el.is_abstract,
                        nillable: el.nillable,
                        substitution_group: el.substitution_group.as_ref().map(conv_qname),
                        default: el.default.clone(),
                        fixed: el.fixed.clone(),
                        source: conv_pos(&el.pos),
                    };
                    let key = qn.canonical_key();
                    if let Some(prev) = self.elements.get(&key) {
                        return Err(FlatError::NameCollision {
                            name: key,
                            first: prev.source.clone(),
                            second: def.source.clone(),
                        });
                    }
                    self.elements.insert(key, def.clone());
                    self.elements_in_order.push(def);
                }
                Decl::Attribute(_) | Decl::Group(_) | Decl::AttributeGroup(_) => {}
            }
        }
        Ok(self)
    }

    fn record_identity(&mut self, owner_element: &str, constraints: &[IdentityConstraint]) {
        for c in constraints {
            self.identity.push(IdentityTable {
                owner_element: owner_element.to_string(),
                kind: conv_id_kind(c.kind),
                name: conv_qname(&c.name),
                refer: c.refer.as_ref().map(conv_qname),
                selector: c.selector.clone(),
                fields: c.fields.clone(),
                source: conv_pos(&c.pos),
            });
        }
    }

    fn resolve_attr_type(
        &mut self,
        ty: &'a AttributeType,
        ns: &Option<String>,
        path: &[String],
    ) -> Result<IrQName, FlatError> {
        match ty {
            AttributeType::Named(q) => Ok(conv_qname(q)),
            AttributeType::AnySimpleType => Ok(xs_qname("anySimpleType")),
            AttributeType::InlineSimple(st) => {
                let anon = Self::anon_name(ns, path);
                self.promote_simple_type(anon.clone(), st, ns, path)?;
                Ok(anon)
            }
        }
    }

    fn resolve_elem_type(
        &mut self,
        ty: &'a ElementType,
        ns: &Option<String>,
        path: &[String],
        owner_path: &str,
    ) -> Result<IrQName, FlatError> {
        match ty {
            ElementType::Named(q) => Ok(conv_qname(q)),
            ElementType::AnyType => Ok(xs_qname("anyType")),
            ElementType::InlineComplex(ct) => {
                let anon = Self::anon_name(ns, path);
                self.promote_complex_type(anon.clone(), ct, ns, path, owner_path)?;
                Ok(anon)
            }
            ElementType::InlineSimple(st) => {
                let anon = Self::anon_name(ns, path);
                self.promote_simple_type(anon.clone(), st, ns, path)?;
                Ok(anon)
            }
        }
    }

    fn promote_simple_type(
        &mut self,
        name: IrQName,
        st: &'a SimpleTypeDecl,
        ns: &Option<String>,
        path: &[String],
    ) -> Result<(), FlatError> {
        let content = match &st.variety {
            SimpleTypeVariety::Restriction { base, inline_base, facets, pos } => {
                let resolved_base = if let Some(b) = base {
                    conv_qname(b)
                } else if let Some(ib) = inline_base {
                    let mut p = path.to_vec();
                    p.push("Base".to_string());
                    let anon = Self::anon_name(ns, &p);
                    self.promote_simple_type(anon.clone(), ib, ns, &p)?;
                    anon
                } else {
                    return Err(FlatError::InvalidSchema {
                        detail: "simpleType restriction missing base".to_string(),
                        pos: conv_pos(pos),
                    });
                };
                PromotedContent::SimpleRestriction {
                    base: resolved_base,
                    facets: facets.iter().map(conv_facet).collect(),
                }
            }
            SimpleTypeVariety::List { item_type, inline_item, pos } => {
                let resolved_item = if let Some(it) = item_type {
                    conv_qname(it)
                } else if let Some(ii) = inline_item {
                    let mut p = path.to_vec();
                    p.push("Item".to_string());
                    let anon = Self::anon_name(ns, &p);
                    self.promote_simple_type(anon.clone(), ii, ns, &p)?;
                    anon
                } else {
                    return Err(FlatError::InvalidSchema {
                        detail: "simpleType list missing itemType".to_string(),
                        pos: conv_pos(pos),
                    });
                };
                PromotedContent::SimpleList { item_type: resolved_item }
            }
            SimpleTypeVariety::Union { member_types, inline_members, .. } => {
                let mut members: Vec<IrQName> = member_types.iter().map(conv_qname).collect();
                for (idx, im) in inline_members.iter().enumerate() {
                    let mut p = path.to_vec();
                    p.push(format!("Member{}", idx + 1));
                    let anon = Self::anon_name(ns, &p);
                    self.promote_simple_type(anon.clone(), im, ns, &p)?;
                    members.push(anon);
                }
                PromotedContent::SimpleUnion { member_types: members }
            }
        };

        self.register_type(PromotedType { name, is_abstract: false, mixed: false, content, source: conv_pos(&st.pos) })
    }

    fn promote_complex_type(
        &mut self,
        name: IrQName,
        ct: &'a ComplexTypeDecl,
        ns: &Option<String>,
        path: &[String],
        owner_path: &str,
    ) -> Result<(), FlatError> {
        let mut group_stack = Vec::new();
        let mut attr_group_stack = Vec::new();
        let content = match &ct.content {
            ComplexContentModel::Direct { particle, attributes } => {
                let part = match particle {
                    Some(p) => Some(self.expand_particle(p, ns, path, owner_path, &mut group_stack)?),
                    None => None,
                };
                let attrs = self.expand_attributes(attributes, ns, path, &mut attr_group_stack)?;
                PromotedContent::ComplexDirect { particle: part, attributes: attrs }
            }
            ComplexContentModel::ComplexContent { derivation, base, mixed, particle, attributes, .. } => {
                let part = match particle {
                    Some(p) => Some(self.expand_particle(p, ns, path, owner_path, &mut group_stack)?),
                    None => None,
                };
                let attrs = self.expand_attributes(attributes, ns, path, &mut attr_group_stack)?;
                PromotedContent::ComplexDerived {
                    derivation: *derivation,
                    base: conv_qname(base),
                    mixed: *mixed,
                    particle: part,
                    attributes: attrs,
                }
            }
            ComplexContentModel::SimpleContent { derivation, base, facets, attributes, .. } => {
                let attrs = self.expand_attributes(attributes, ns, path, &mut attr_group_stack)?;
                PromotedContent::SimpleContent {
                    derivation: *derivation,
                    base: conv_qname(base),
                    facets: facets.iter().map(conv_facet).collect(),
                    attributes: attrs,
                }
            }
        };

        self.register_type(PromotedType {
            name,
            is_abstract: ct.is_abstract,
            mixed: ct.mixed,
            content,
            source: conv_pos(&ct.pos),
        })
    }

    fn expand_particle(
        &mut self,
        p: &'a Particle,
        ns: &Option<String>,
        path: &[String],
        owner_path: &str,
        group_stack: &mut Vec<IrQName>,
    ) -> Result<IrParticle, FlatError> {
        match p {
            Particle::Sequence { occurs, items, .. } => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.expand_particle(item, ns, path, owner_path, group_stack)?);
                }
                Ok(IrParticle::Sequence { occurs: conv_occurs(*occurs), items: out })
            }
            Particle::Choice { occurs, items, .. } => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.expand_particle(item, ns, path, owner_path, group_stack)?);
                }
                Ok(IrParticle::Choice { occurs: conv_occurs(*occurs), items: out })
            }
            Particle::All { occurs, items, .. } => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.expand_particle(item, ns, path, owner_path, group_stack)?);
                }
                Ok(IrParticle::All { occurs: conv_occurs(*occurs), items: out })
            }
            Particle::Element(local) => {
                let el = &local.decl;
                let qn = conv_qname(&el.name);
                let mut next_path = path.to_vec();
                next_path.push(qn.local.clone());
                let next_owner = format!("{owner_path}/{}", qn.canonical_key());
                self.record_identity(&next_owner, &el.constraints);
                let ty = self.resolve_elem_type(&el.ty, ns, &next_path, &next_owner)?;
                Ok(IrParticle::Element {
                    name: qn,
                    ty,
                    occurs: conv_occurs(local.occurs),
                    nillable: el.nillable,
                    default: el.default.clone(),
                    fixed: el.fixed.clone(),
                })
            }
            Particle::ElementRef { target, occurs, .. } => {
                Ok(IrParticle::ElementRef { target: conv_qname(target), occurs: conv_occurs(*occurs) })
            }
            Particle::GroupRef { target, occurs, pos, .. } => {
                let tq = conv_qname(target);
                if let Some(idx) = group_stack.iter().position(|g| g == &tq) {
                    let mut cycle = group_stack[idx..].to_vec();
                    cycle.push(tq);
                    return Err(FlatError::BaseCycle { cycle, pos: conv_pos(pos) });
                }
                let Some(group) = self.groups.get(&tq).copied() else {
                    return Err(FlatError::InvalidSchema {
                        detail: format!("unresolved group `{tq}`"),
                        pos: conv_pos(pos),
                    });
                };
                group_stack.push(tq);
                let expanded = self.expand_particle(&group.particle, ns, path, owner_path, group_stack)?;
                group_stack.pop();
                let occ = conv_occurs(*occurs);
                if occ == IrOccurs::ONE {
                    Ok(expanded)
                } else {
                    Ok(IrParticle::Sequence { occurs: occ, items: vec![expanded] })
                }
            }
        }
    }

    fn expand_attributes(
        &mut self,
        items: &'a [AttributeItem],
        ns: &Option<String>,
        path: &[String],
        ag_stack: &mut Vec<IrQName>,
    ) -> Result<Vec<AttrMeta>, FlatError> {
        let mut out = Vec::new();
        for item in items {
            match item {
                AttributeItem::Decl(a) => {
                    let qn = conv_qname(&a.name);
                    let mut next_path = path.to_vec();
                    next_path.push(qn.local.clone());
                    let ty = self.resolve_attr_type(&a.ty, ns, &next_path)?;
                    out.push(AttrMeta {
                        attr: IrAttribute {
                            name: qn,
                            ty,
                            use_kind: conv_attr_use(a.use_kind),
                            default: a.default.clone(),
                            fixed: a.fixed.clone(),
                        },
                        annotation: a.annotation.as_ref().map(|an| an.text.clone()),
                        source: conv_pos(&a.pos),
                    });
                }
                AttributeItem::Ref { target, use_kind, default, fixed, annotation, pos } => {
                    let tq = conv_qname(target);
                    let Some(global) = self.global_attrs.get(&tq).copied() else {
                        return Err(FlatError::InvalidSchema {
                            detail: format!("unresolved attribute ref `{tq}`"),
                            pos: conv_pos(pos),
                        });
                    };
                    let ty = self.global_attr_types.get(&tq).cloned().unwrap_or_else(|| xs_qname("anySimpleType"));
                    let eff_ann =
                        annotation.as_ref().or(global.annotation.as_ref()).map(|a: &Annotation| a.text.clone());
                    out.push(AttrMeta {
                        attr: IrAttribute {
                            name: tq,
                            ty,
                            use_kind: conv_attr_use(*use_kind),
                            default: default.clone().or_else(|| global.default.clone()),
                            fixed: fixed.clone().or_else(|| global.fixed.clone()),
                        },
                        annotation: eff_ann,
                        source: conv_pos(pos),
                    });
                }
                AttributeItem::GroupRef { target, pos, .. } => {
                    let tq = conv_qname(target);
                    if let Some(idx) = ag_stack.iter().position(|g| g == &tq) {
                        let mut cycle = ag_stack[idx..].to_vec();
                        cycle.push(tq);
                        return Err(FlatError::BaseCycle { cycle, pos: conv_pos(pos) });
                    }
                    let Some(ag) = self.attr_groups.get(&tq).copied() else {
                        return Err(FlatError::InvalidSchema {
                            detail: format!("unresolved attributeGroup `{tq}`"),
                            pos: conv_pos(pos),
                        });
                    };
                    ag_stack.push(tq);
                    let sub = self.expand_attributes(&ag.attributes, ns, path, ag_stack)?;
                    ag_stack.pop();
                    out.extend(sub);
                }
            }
        }
        Ok(out)
    }
}

// ------------------------------------------------------------------ topological sort & flattening

fn base_of(pt: &PromotedType) -> Option<&IrQName> {
    match &pt.content {
        PromotedContent::ComplexDerived { base, .. }
        | PromotedContent::SimpleContent { base, .. }
        | PromotedContent::SimpleRestriction { base, .. } => Some(base),
        PromotedContent::ComplexDirect { .. }
        | PromotedContent::SimpleList { .. }
        | PromotedContent::SimpleUnion { .. } => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Done,
}

fn topo_sort_types(types: &BTreeMap<IrQName, PromotedType>) -> Result<Vec<IrQName>, FlatError> {
    let mut state: BTreeMap<IrQName, VisitState> = BTreeMap::new();
    let mut stack: Vec<IrQName> = Vec::new();
    let mut order: Vec<IrQName> = Vec::with_capacity(types.len());

    fn dfs(
        cur: &IrQName,
        types: &BTreeMap<IrQName, PromotedType>,
        state: &mut BTreeMap<IrQName, VisitState>,
        stack: &mut Vec<IrQName>,
        order: &mut Vec<IrQName>,
    ) -> Result<(), FlatError> {
        if state.get(cur) == Some(&VisitState::Done) {
            return Ok(());
        }
        state.insert(cur.clone(), VisitState::Visiting);
        stack.push(cur.clone());

        if let Some(pt) = types.get(cur) {
            if let Some(base) = base_of(pt) {
                if types.contains_key(base) {
                    match state.get(base) {
                        Some(VisitState::Visiting) => {
                            let idx = stack.iter().position(|n| n == base).unwrap_or(0);
                            let mut cycle = stack[idx..].to_vec();
                            cycle.push(base.clone());
                            return Err(FlatError::BaseCycle { cycle, pos: pt.source.clone() });
                        }
                        Some(VisitState::Done) => {}
                        None => dfs(base, types, state, stack, order)?,
                    }
                }
            }
        }

        stack.pop();
        state.insert(cur.clone(), VisitState::Done);
        order.push(cur.clone());
        Ok(())
    }

    for name in types.keys() {
        dfs(name, types, &mut state, &mut stack, &mut order)?;
    }
    Ok(order)
}

/// Unwraps a default-occurrence (`min = 1, max = 1`) `Sequence` into its top-level items so
/// flattened extension chains concatenate cleanly (`[C..., B..., A...]`).
fn flatten_particle_list(p: Option<IrParticle>) -> Vec<IrParticle> {
    match p {
        None => Vec::new(),
        Some(IrParticle::Sequence { occurs, items }) if occurs == IrOccurs::ONE => items,
        Some(other) => vec![other],
    }
}

/// Merges base attributes with derived attributes: overrides match by QName in place,
/// new attributes append in source order, and `Prohibited` removes the attribute on restriction.
fn merge_attributes(base: &[AttrMeta], derived: &[AttrMeta], drop_prohibited: bool) -> Vec<AttrMeta> {
    let mut out = base.to_vec();
    for d in derived {
        if let Some(existing) = out.iter_mut().find(|a| a.attr.name == d.attr.name) {
            *existing = d.clone();
        } else {
            out.push(d.clone());
        }
    }
    if drop_prohibited {
        out.retain(|a| a.attr.use_kind != IrAttributeUse::Prohibited);
    }
    out
}

// ------------------------------------------------------------------ count rule derivation (REQ-004)

/// Extracts an explicit count target from `xs:annotation/xs:documentation`, if present
/// (supports `count-of:<name>` or `counts:<name>`, case-insensitive).
fn annotation_count_target(ann: Option<&str>) -> Option<String> {
    let text = ann?;
    let lower = text.to_ascii_lowercase();
    for prefix in ["count-of:", "counts:"] {
        if let Some(idx) = lower.find(prefix) {
            let start = idx + prefix.len();
            let rest = text[start..].trim_start();
            let tok: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':' | '{' | '}' | '/' | '.'))
                .collect();
            if !tok.is_empty() {
                return Some(tok);
            }
        }
    }
    None
}

fn is_count_attr_name(name: &IrQName) -> bool {
    name.local == "n" || name.local == "count"
}

fn collect_direct_element_names(items: &[IrParticle], out: &mut Vec<IrQName>) {
    for p in items {
        match p {
            IrParticle::Element { name, .. } => out.push(name.clone()),
            IrParticle::ElementRef { target, .. } => out.push(target.clone()),
            IrParticle::Sequence { items: sub, .. }
            | IrParticle::Choice { items: sub, .. }
            | IrParticle::All { items: sub, .. } => collect_direct_element_names(sub, out),
        }
    }
}

fn collect_repeated_candidates(particles: &[IrParticle], out: &mut Vec<CountTarget>) {
    for p in particles {
        match p {
            IrParticle::Element { name, occurs, .. } => {
                if occurs.is_repeated() {
                    out.push(CountTarget::Element(name.clone()));
                }
            }
            IrParticle::ElementRef { target, occurs } => {
                if occurs.is_repeated() {
                    out.push(CountTarget::Element(target.clone()));
                }
            }
            IrParticle::Choice { occurs, items } => {
                if occurs.is_repeated() {
                    let mut members = Vec::new();
                    collect_direct_element_names(items, &mut members);
                    if members.len() == 1 {
                        if let Some(only) = members.into_iter().next() {
                            out.push(CountTarget::Element(only));
                        }
                    } else if !members.is_empty() {
                        out.push(CountTarget::Choice(members));
                    }
                } else {
                    collect_repeated_candidates(items, out);
                }
            }
            IrParticle::Sequence { occurs, items } => {
                if occurs.is_repeated() {
                    let mut members = Vec::new();
                    collect_direct_element_names(items, &mut members);
                    if members.len() == 1 {
                        if let Some(only) = members.into_iter().next() {
                            out.push(CountTarget::Element(only));
                        }
                    } else if !members.is_empty() {
                        out.push(CountTarget::Sequence(members));
                    }
                } else {
                    collect_repeated_candidates(items, out);
                }
            }
            IrParticle::All { items, .. } => collect_repeated_candidates(items, out),
        }
    }
}

fn derive_count_rules(
    types: &BTreeMap<String, TypeDef>,
    attr_metas: &BTreeMap<String, Vec<AttrMeta>>,
) -> Result<Vec<CountRule>, FlatError> {
    let mut rules = Vec::new();
    let mut unlinked = Vec::new();

    for (key, td) in types {
        if td.kind != TypeKind::Complex {
            continue;
        }
        let Some(metas) = attr_metas.get(key) else { continue };
        let mut candidates = Vec::new();
        collect_repeated_candidates(&td.particles, &mut candidates);

        for am in metas {
            let explicit = annotation_count_target(am.annotation.as_deref());
            let by_name = is_count_attr_name(&am.attr.name);
            if explicit.is_none() && !by_name {
                continue;
            }

            if let Some(target_hint) = explicit {
                let matched: Vec<&CountTarget> = candidates
                    .iter()
                    .filter(|c| match c {
                        CountTarget::Element(q) => q.local == target_hint || q.canonical_key() == target_hint,
                        _ => false,
                    })
                    .collect();
                if matched.len() == 1 {
                    rules.push(CountRule {
                        owner_type: td.name.clone(),
                        attribute: am.attr.name.clone(),
                        target: matched[0].clone(),
                        source: am.source.clone(),
                    });
                } else {
                    unlinked.push(UnlinkedCountAttr {
                        owner_type: td.name.clone(),
                        attribute: am.attr.name.clone(),
                        source: am.source.clone(),
                        reason: format!("annotation target `{target_hint}` did not match a unique repeated child"),
                    });
                }
            } else if candidates.len() == 1 {
                rules.push(CountRule {
                    owner_type: td.name.clone(),
                    attribute: am.attr.name.clone(),
                    target: candidates[0].clone(),
                    source: am.source.clone(),
                });
            } else {
                let reason = if candidates.is_empty() {
                    "no repeated child particle in complex type".to_string()
                } else {
                    format!("ambiguous: {} repeated child particles", candidates.len())
                };
                unlinked.push(UnlinkedCountAttr {
                    owner_type: td.name.clone(),
                    attribute: am.attr.name.clone(),
                    source: am.source.clone(),
                    reason,
                });
            }
        }
    }

    if !unlinked.is_empty() {
        return Err(FlatError::CountRulesUnlinked { unlinked });
    }
    rules.sort_by(|a, b| {
        a.owner_type
            .canonical_key()
            .cmp(&b.owner_type.canonical_key())
            .then_with(|| a.attribute.canonical_key().cmp(&b.attribute.canonical_key()))
    });
    Ok(rules)
}

// ------------------------------------------------------------------ substitution groups (REQ-003)

fn build_substitution_groups(elements_in_order: &[ElementDef]) -> Result<BTreeMap<String, Vec<IrQName>>, FlatError> {
    let by_name: BTreeMap<IrQName, &ElementDef> = elements_in_order.iter().map(|e| (e.name.clone(), e)).collect();
    let mut subst: BTreeMap<String, Vec<IrQName>> = BTreeMap::new();

    // Ensure every abstract element and every referenced head appears in the map.
    for e in elements_in_order {
        if e.is_abstract {
            subst.entry(e.name.canonical_key()).or_default();
        }
        if let Some(head) = &e.substitution_group {
            subst.entry(head.canonical_key()).or_default();
        }
    }

    // For every element in source order, walk its substitutionGroup chain up to all ancestor heads.
    for e in elements_in_order {
        let mut seen = BTreeSet::new();
        seen.insert(e.name.clone());
        let mut cur = e.substitution_group.clone();
        let mut chain = vec![e.name.clone()];
        while let Some(head) = cur {
            if !seen.insert(head.clone()) {
                chain.push(head);
                return Err(FlatError::BaseCycle { cycle: chain, pos: e.source.clone() });
            }
            chain.push(head.clone());
            // Only non-abstract (instantiable) elements are offered as substitution choices (REQ-003).
            if e.instantiable {
                let members = subst.entry(head.canonical_key()).or_default();
                if !members.contains(&e.name) {
                    members.push(e.name.clone());
                }
            }
            cur = by_name.get(&head).and_then(|h| h.substitution_group.clone());
        }
    }

    Ok(subst)
}

// ------------------------------------------------------------------ public entry points

/// Normalises a [`RawIr`] (from `IO.QIF.GEN.PARSE`) into the canonical [`SchemaIr`] (`fid-qif-ir`).
///
/// Satisfies `REQ-IO.QIF.GEN.FLAT-001`..`REQ-IO.QIF.GEN.FLAT-006`.
pub fn flatten(raw: &RawIr) -> Result<SchemaIr, FlatError> {
    let promoted = Promoter::new(raw).run(raw)?;
    let topo = topo_sort_types(&promoted.types)?;

    let mut flat_types: BTreeMap<String, TypeDef> = BTreeMap::new();
    let mut flat_attr_metas: BTreeMap<String, Vec<AttrMeta>> = BTreeMap::new();

    for qn in &topo {
        let Some(pt) = promoted.types.get(qn) else { continue };
        let key = qn.canonical_key();

        let (td, metas) = match &pt.content {
            PromotedContent::ComplexDirect { particle, attributes } => {
                let cleaned = merge_attributes(&[], attributes, true);
                let attrs = cleaned.iter().map(|m| m.attr.clone()).collect();
                (
                    TypeDef {
                        name: pt.name.clone(),
                        kind: TypeKind::Complex,
                        is_abstract: pt.is_abstract,
                        mixed: pt.mixed,
                        base: None,
                        simple_base: None,
                        item_type: None,
                        member_types: Vec::new(),
                        particles: flatten_particle_list(particle.clone()),
                        attributes: attrs,
                        facets: Vec::new(),
                        source: pt.source.clone(),
                    },
                    cleaned,
                )
            }
            PromotedContent::ComplexDerived { derivation, base, mixed, particle, attributes } => {
                let base_key = base.canonical_key();
                let base_td = flat_types.get(&base_key);
                let base_metas = flat_attr_metas.get(&base_key).map(Vec::as_slice).unwrap_or(&[]);

                let derived_parts = flatten_particle_list(particle.clone());
                let particles = match derivation {
                    DerivationKind::Extension => {
                        let mut combined = base_td.map(|b| b.particles.clone()).unwrap_or_default();
                        combined.extend(derived_parts);
                        combined
                    }
                    DerivationKind::Restriction => derived_parts,
                };
                let drop_prohibited = *derivation == DerivationKind::Restriction;
                let cleaned = merge_attributes(base_metas, attributes, drop_prohibited);
                let attrs = cleaned.iter().map(|m| m.attr.clone()).collect();
                let eff_mixed = mixed.unwrap_or_else(|| pt.mixed || base_td.is_some_and(|b| b.mixed));

                (
                    TypeDef {
                        name: pt.name.clone(),
                        kind: TypeKind::Complex,
                        is_abstract: pt.is_abstract,
                        mixed: eff_mixed,
                        base: Some(base.clone()),
                        simple_base: base_td.and_then(|b| b.simple_base.clone()),
                        item_type: None,
                        member_types: Vec::new(),
                        particles,
                        attributes: attrs,
                        facets: base_td.map(|b| b.facets.clone()).unwrap_or_default(),
                        source: pt.source.clone(),
                    },
                    cleaned,
                )
            }
            PromotedContent::SimpleContent { derivation, base, facets, attributes } => {
                let base_key = base.canonical_key();
                let base_td = flat_types.get(&base_key);
                let base_metas = flat_attr_metas.get(&base_key).map(Vec::as_slice).unwrap_or(&[]);

                let simple_base = base_td.and_then(|b| b.simple_base.clone()).or_else(|| Some(base.clone()));
                let mut combined_facets = base_td.map(|b| b.facets.clone()).unwrap_or_default();
                combined_facets.extend(facets.iter().cloned());

                let drop_prohibited = *derivation == DerivationKind::Restriction;
                let cleaned = merge_attributes(base_metas, attributes, drop_prohibited);
                let attrs = cleaned.iter().map(|m| m.attr.clone()).collect();

                (
                    TypeDef {
                        name: pt.name.clone(),
                        kind: TypeKind::Complex,
                        is_abstract: pt.is_abstract,
                        mixed: false,
                        base: Some(base.clone()),
                        simple_base,
                        item_type: None,
                        member_types: Vec::new(),
                        particles: Vec::new(),
                        attributes: attrs,
                        facets: combined_facets,
                        source: pt.source.clone(),
                    },
                    cleaned,
                )
            }
            PromotedContent::SimpleRestriction { base, facets } => {
                let base_key = base.canonical_key();
                let base_td = flat_types.get(&base_key);
                let simple_base = base_td.and_then(|b| b.simple_base.clone()).or_else(|| Some(base.clone()));
                let mut combined_facets = base_td.map(|b| b.facets.clone()).unwrap_or_default();
                combined_facets.extend(facets.iter().cloned());

                (
                    TypeDef {
                        name: pt.name.clone(),
                        kind: TypeKind::Simple,
                        is_abstract: false,
                        mixed: false,
                        base: Some(base.clone()),
                        simple_base,
                        item_type: None,
                        member_types: Vec::new(),
                        particles: Vec::new(),
                        attributes: Vec::new(),
                        facets: combined_facets,
                        source: pt.source.clone(),
                    },
                    Vec::new(),
                )
            }
            PromotedContent::SimpleList { item_type } => (
                TypeDef {
                    name: pt.name.clone(),
                    kind: TypeKind::List,
                    is_abstract: false,
                    mixed: false,
                    base: None,
                    simple_base: None,
                    item_type: Some(item_type.clone()),
                    member_types: Vec::new(),
                    particles: Vec::new(),
                    attributes: Vec::new(),
                    facets: Vec::new(),
                    source: pt.source.clone(),
                },
                Vec::new(),
            ),
            PromotedContent::SimpleUnion { member_types } => (
                TypeDef {
                    name: pt.name.clone(),
                    kind: TypeKind::Union,
                    is_abstract: false,
                    mixed: false,
                    base: None,
                    simple_base: None,
                    item_type: None,
                    member_types: member_types.clone(),
                    particles: Vec::new(),
                    attributes: Vec::new(),
                    facets: Vec::new(),
                    source: pt.source.clone(),
                },
                Vec::new(),
            ),
        };

        flat_types.insert(key.clone(), td);
        flat_attr_metas.insert(key, metas);
    }

    let substitution = build_substitution_groups(&promoted.elements_in_order)?;
    let count_rules = derive_count_rules(&flat_types, &flat_attr_metas)?;

    let mut identity = promoted.identity;
    identity.sort_by(|a, b| {
        a.owner_element.cmp(&b.owner_element).then_with(|| a.name.cmp(&b.name)).then_with(|| a.source.cmp(&b.source))
    });

    // Build bijective IR-name -> emitted-name table (REQ-IO.QIF.GEN.FLAT-006).
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    let mut emitted_pos: BTreeMap<String, IrPos> = BTreeMap::new();
    for (ir_key, td) in &flat_types {
        let emit = to_emit_name(&td.name.local);
        if let Some(prev) = emitted_pos.get(&emit) {
            return Err(FlatError::NameCollision { name: emit, first: prev.clone(), second: td.source.clone() });
        }
        emitted_pos.insert(emit.clone(), td.source.clone());
        names.insert(ir_key.clone(), emit);
    }

    Ok(SchemaIr {
        schema_version: QIF_IR_SCHEMA.to_string(),
        types: flat_types,
        elements: promoted.elements,
        substitution,
        count_rules,
        identity,
        names,
    })
}

/// Parses a verified [`PinnedSet`] and flattens it into a normalised [`SchemaIr`].
pub fn flatten_pinned(pinned: &PinnedSet) -> Result<SchemaIr, FlatError> {
    let raw = parse_pinned(pinned)?;
    flatten(&raw)
}

/// Serialises a normalised [`SchemaIr`] to RFC 8785 canonical JSON (`qif-ir.json`) and its SHA-256 digest.
pub fn encode_qif_ir(ir: &SchemaIr) -> Result<(Vec<u8>, Digest), FlatError> {
    let bytes = ir.to_canonical_json()?;
    let digest = ir.digest()?;
    Ok((bytes, digest))
}
