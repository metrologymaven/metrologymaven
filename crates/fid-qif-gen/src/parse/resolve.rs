//! Cross-file symbol table and source-order reference checker (REQ-IO.QIF.GEN.PARSE-003).

use crate::parse::ir::{
    AttributeDecl, AttributeItem, AttributeType, ComplexContentModel, ComplexTypeDecl, Decl, ElementDecl, ElementType,
    IdentityConstraint, IdentityKind, ParseError, Particle, Pos, QName, RawIr, RefKind, SimpleTypeDecl,
    SimpleTypeVariety, XML_NS, XS_NS,
};
use std::collections::BTreeSet;

/// Built-in W3C XML Schema 1.0 type local names (`http://www.w3.org/2001/XMLSchema`).
const BUILTIN_XS_TYPES: &[&str] = &[
    "anyType",
    "anySimpleType",
    "string",
    "boolean",
    "decimal",
    "float",
    "double",
    "duration",
    "dateTime",
    "time",
    "date",
    "gYearMonth",
    "gYear",
    "gMonthDay",
    "gDay",
    "gMonth",
    "hexBinary",
    "base64Binary",
    "anyURI",
    "QName",
    "NOTATION",
    "normalizedString",
    "token",
    "language",
    "NMTOKEN",
    "NMTOKENS",
    "Name",
    "NCName",
    "ID",
    "IDREF",
    "IDREFS",
    "ENTITY",
    "ENTITIES",
    "integer",
    "nonPositiveInteger",
    "negativeInteger",
    "long",
    "int",
    "short",
    "byte",
    "nonNegativeInteger",
    "unsignedLong",
    "unsignedInt",
    "unsignedShort",
    "unsignedByte",
    "positiveInteger",
];

/// Built-in `xml:` namespace attribute local names (`http://www.w3.org/XML/1998/namespace`).
const BUILTIN_XML_ATTRS: &[&str] = &["lang", "space", "base", "id"];

struct SymbolTable {
    types: BTreeSet<QName>,
    elements: BTreeSet<QName>,
    attributes: BTreeSet<QName>,
    groups: BTreeSet<QName>,
    attribute_groups: BTreeSet<QName>,
    identity_keys: BTreeSet<QName>,
}

impl SymbolTable {
    fn from_ir(ir: &RawIr) -> Self {
        let mut syms = SymbolTable {
            types: BTreeSet::new(),
            elements: BTreeSet::new(),
            attributes: BTreeSet::new(),
            groups: BTreeSet::new(),
            attribute_groups: BTreeSet::new(),
            identity_keys: BTreeSet::new(),
        };

        for local in BUILTIN_XS_TYPES {
            syms.types.insert(QName::new(Some(XS_NS.to_string()), *local));
        }
        for local in BUILTIN_XML_ATTRS {
            syms.attributes.insert(QName::new(Some(XML_NS.to_string()), *local));
        }

        for decl in &ir.decls {
            match decl {
                Decl::ComplexType(ct) => {
                    if let Some(name) = &ct.name {
                        syms.types.insert(name.clone());
                    }
                    collect_keys_from_complex_type(ct, &mut syms.identity_keys);
                }
                Decl::SimpleType(st) => {
                    if let Some(name) = &st.name {
                        syms.types.insert(name.clone());
                    }
                }
                Decl::Element(el) => {
                    syms.elements.insert(el.name.clone());
                    collect_keys_from_element(el, &mut syms.identity_keys);
                }
                Decl::Attribute(attr) => {
                    syms.attributes.insert(attr.name.clone());
                }
                Decl::Group(grp) => {
                    syms.groups.insert(grp.name.clone());
                    collect_keys_from_particle(&grp.particle, &mut syms.identity_keys);
                }
                Decl::AttributeGroup(ag) => {
                    syms.attribute_groups.insert(ag.name.clone());
                }
            }
        }
        syms
    }

    fn check_type(&self, q: &QName, pos: &Pos) -> Result<(), ParseError> {
        if self.types.contains(q) {
            Ok(())
        } else {
            Err(ParseError::UnresolvedRef { kind: RefKind::Type, name: q.to_string(), pos: pos.clone() })
        }
    }

    fn check_element(&self, q: &QName, pos: &Pos) -> Result<(), ParseError> {
        if self.elements.contains(q) {
            Ok(())
        } else {
            Err(ParseError::UnresolvedRef { kind: RefKind::Element, name: q.to_string(), pos: pos.clone() })
        }
    }

    fn check_attribute(&self, q: &QName, pos: &Pos) -> Result<(), ParseError> {
        if self.attributes.contains(q) {
            Ok(())
        } else {
            Err(ParseError::UnresolvedRef { kind: RefKind::Attribute, name: q.to_string(), pos: pos.clone() })
        }
    }

    fn check_group(&self, q: &QName, pos: &Pos) -> Result<(), ParseError> {
        if self.groups.contains(q) {
            Ok(())
        } else {
            Err(ParseError::UnresolvedRef { kind: RefKind::Group, name: q.to_string(), pos: pos.clone() })
        }
    }

    fn check_attribute_group(&self, q: &QName, pos: &Pos) -> Result<(), ParseError> {
        if self.attribute_groups.contains(q) {
            Ok(())
        } else {
            Err(ParseError::UnresolvedRef { kind: RefKind::AttributeGroup, name: q.to_string(), pos: pos.clone() })
        }
    }

    fn check_identity_key(&self, q: &QName, pos: &Pos) -> Result<(), ParseError> {
        if self.identity_keys.contains(q) {
            Ok(())
        } else {
            Err(ParseError::UnresolvedRef { kind: RefKind::IdentityKey, name: q.to_string(), pos: pos.clone() })
        }
    }
}

fn collect_keys_from_element(el: &ElementDecl, out: &mut BTreeSet<QName>) {
    for c in &el.constraints {
        if matches!(c.kind, IdentityKind::Key | IdentityKind::Unique) {
            out.insert(c.name.clone());
        }
    }
    if let ElementType::InlineComplex(ct) = &el.ty {
        collect_keys_from_complex_type(ct, out);
    }
}

fn collect_keys_from_complex_type(ct: &ComplexTypeDecl, out: &mut BTreeSet<QName>) {
    match &ct.content {
        ComplexContentModel::Direct { particle: Some(p), .. }
        | ComplexContentModel::ComplexContent { particle: Some(p), .. } => {
            collect_keys_from_particle(p, out);
        }
        ComplexContentModel::Direct { particle: None, .. }
        | ComplexContentModel::ComplexContent { particle: None, .. }
        | ComplexContentModel::SimpleContent { .. } => {}
    }
}

fn collect_keys_from_particle(p: &Particle, out: &mut BTreeSet<QName>) {
    match p {
        Particle::Sequence { items, .. } | Particle::Choice { items, .. } | Particle::All { items, .. } => {
            for item in items {
                collect_keys_from_particle(item, out);
            }
        }
        Particle::Element(local) => collect_keys_from_element(&local.decl, out),
        Particle::ElementRef { .. } | Particle::GroupRef { .. } => {}
    }
}

/// Checks every reference in `ir` in deterministic source order, returning the first
/// unresolved reference with its source position (REQ-IO.QIF.GEN.PARSE-003).
pub(crate) fn check_references(ir: &RawIr) -> Result<(), ParseError> {
    let syms = SymbolTable::from_ir(ir);
    for decl in &ir.decls {
        match decl {
            Decl::ComplexType(ct) => check_complex_type(&syms, ct)?,
            Decl::SimpleType(st) => check_simple_type(&syms, st)?,
            Decl::Element(el) => check_element(&syms, el)?,
            Decl::Attribute(attr) => check_attribute_decl(&syms, attr)?,
            Decl::Group(grp) => check_particle(&syms, &grp.particle)?,
            Decl::AttributeGroup(ag) => check_attribute_items(&syms, &ag.attributes)?,
        }
    }
    Ok(())
}

fn check_complex_type(syms: &SymbolTable, ct: &ComplexTypeDecl) -> Result<(), ParseError> {
    match &ct.content {
        ComplexContentModel::Direct { particle, attributes } => {
            if let Some(p) = particle {
                check_particle(syms, p)?;
            }
            check_attribute_items(syms, attributes)?;
        }
        ComplexContentModel::SimpleContent { base, attributes, pos, .. } => {
            syms.check_type(base, pos)?;
            check_attribute_items(syms, attributes)?;
        }
        ComplexContentModel::ComplexContent { base, particle, attributes, pos, .. } => {
            syms.check_type(base, pos)?;
            if let Some(p) = particle {
                check_particle(syms, p)?;
            }
            check_attribute_items(syms, attributes)?;
        }
    }
    Ok(())
}

fn check_simple_type(syms: &SymbolTable, st: &SimpleTypeDecl) -> Result<(), ParseError> {
    match &st.variety {
        SimpleTypeVariety::Restriction { base, inline_base, pos, .. } => {
            if let Some(q) = base {
                syms.check_type(q, pos)?;
            }
            if let Some(ib) = inline_base {
                check_simple_type(syms, ib)?;
            }
        }
        SimpleTypeVariety::List { item_type, inline_item, pos } => {
            if let Some(q) = item_type {
                syms.check_type(q, pos)?;
            }
            if let Some(ii) = inline_item {
                check_simple_type(syms, ii)?;
            }
        }
        SimpleTypeVariety::Union { member_types, inline_members, pos } => {
            for q in member_types {
                syms.check_type(q, pos)?;
            }
            for im in inline_members {
                check_simple_type(syms, im)?;
            }
        }
    }
    Ok(())
}

fn check_element(syms: &SymbolTable, el: &ElementDecl) -> Result<(), ParseError> {
    if let Some(sg) = &el.substitution_group {
        syms.check_element(sg, &el.pos)?;
    }
    match &el.ty {
        ElementType::Named(q) => syms.check_type(q, &el.pos)?,
        ElementType::InlineComplex(ct) => check_complex_type(syms, ct)?,
        ElementType::InlineSimple(st) => check_simple_type(syms, st)?,
        ElementType::AnyType => {}
    }
    for c in &el.constraints {
        check_identity_constraint(syms, c)?;
    }
    Ok(())
}

fn check_identity_constraint(syms: &SymbolTable, c: &IdentityConstraint) -> Result<(), ParseError> {
    if let Some(refer) = &c.refer {
        syms.check_identity_key(refer, &c.pos)?;
    }
    Ok(())
}

fn check_attribute_decl(syms: &SymbolTable, attr: &AttributeDecl) -> Result<(), ParseError> {
    match &attr.ty {
        AttributeType::Named(q) => syms.check_type(q, &attr.pos),
        AttributeType::InlineSimple(st) => check_simple_type(syms, st),
        AttributeType::AnySimpleType => Ok(()),
    }
}

fn check_attribute_items(syms: &SymbolTable, items: &[AttributeItem]) -> Result<(), ParseError> {
    for item in items {
        match item {
            AttributeItem::Decl(d) => check_attribute_decl(syms, d)?,
            AttributeItem::Ref { target, pos, .. } => syms.check_attribute(target, pos)?,
            AttributeItem::GroupRef { target, pos, .. } => syms.check_attribute_group(target, pos)?,
        }
    }
    Ok(())
}

fn check_particle(syms: &SymbolTable, p: &Particle) -> Result<(), ParseError> {
    match p {
        Particle::Sequence { items, .. } | Particle::Choice { items, .. } | Particle::All { items, .. } => {
            for item in items {
                check_particle(syms, item)?;
            }
            Ok(())
        }
        Particle::Element(local) => check_element(syms, &local.decl),
        Particle::ElementRef { target, pos, .. } => syms.check_element(target, pos),
        Particle::GroupRef { target, pos, .. } => syms.check_group(target, pos),
    }
}
