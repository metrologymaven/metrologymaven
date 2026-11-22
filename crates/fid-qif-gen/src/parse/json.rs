//! Deterministic RFC 8785 canonical JSON encoder for [`RawIr`] (AC-1).

use crate::parse::ir::{
    Annotation, AttributeDecl, AttributeGroupDecl, AttributeItem, AttributeType, ComplexContentModel, ComplexTypeDecl,
    Decl, ElementDecl, ElementType, Facet, FacetKind, GroupDecl, IdentityConstraint, MaxOccurs, Occurs, Particle, Pos,
    QName, RawIr, SimpleTypeDecl, SimpleTypeVariety,
};
use fid_canon::{to_canonical_bytes, CValue};
use std::collections::BTreeMap;

impl RawIr {
    /// Converts the raw IR into a canonical JSON value tree (`BTreeMap` only).
    pub fn to_cvalue(&self) -> CValue {
        let files = self.files.iter().map(|f| s(f.as_str())).collect();
        let tns: BTreeMap<String, CValue> = self
            .target_namespaces
            .iter()
            .map(|(f, ns)| (f.as_str().to_string(), ns.as_deref().map_or(CValue::Null, s)))
            .collect();
        let decls = self.decls.iter().map(decl_val).collect();
        obj([
            ("decls", CValue::Arr(decls)),
            ("files", CValue::Arr(files)),
            ("schema", s("qif-raw-ir@1")),
            ("target_namespaces", CValue::Obj(tns)),
        ])
    }

    /// Serialises the raw IR to RFC 8785 canonical JSON bytes.
    pub fn to_canonical_json(&self) -> Vec<u8> {
        to_canonical_bytes(&self.to_cvalue()).unwrap_or_default()
    }
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn n(v: u64) -> CValue {
    CValue::Num(v as f64)
}

fn obj<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn pos_val(p: &Pos) -> CValue {
    obj([("col", n(u64::from(p.col))), ("file", s(p.file.as_str())), ("line", n(u64::from(p.line)))])
}

fn qname_val(q: &QName) -> CValue {
    obj([("local", s(&q.local)), ("ns", q.ns.as_deref().map_or(CValue::Null, s))])
}

fn opt_qname(q: Option<&QName>) -> CValue {
    q.map_or(CValue::Null, qname_val)
}

fn opt_str(v: Option<&str>) -> CValue {
    v.map_or(CValue::Null, s)
}

fn ann_val(a: Option<&Annotation>) -> CValue {
    a.map_or(CValue::Null, |a| obj([("pos", pos_val(&a.pos)), ("text", s(&a.text))]))
}

fn occurs_val(o: Occurs) -> CValue {
    let max = match o.max {
        MaxOccurs::Bounded(v) => n(v),
        MaxOccurs::Unbounded => s("unbounded"),
    };
    obj([("max", max), ("min", n(o.min))])
}

fn facet_val(f: &Facet) -> CValue {
    let (kind, value) = match &f.kind {
        FacetKind::Enumeration(v) => ("enumeration", s(v)),
        FacetKind::Pattern(v) => ("pattern", s(v)),
        FacetKind::Length(v) => ("length", n(*v)),
        FacetKind::MinLength(v) => ("minLength", n(*v)),
        FacetKind::MaxLength(v) => ("maxLength", n(*v)),
        FacetKind::MinInclusive(v) => ("minInclusive", s(v)),
        FacetKind::MaxInclusive(v) => ("maxInclusive", s(v)),
        FacetKind::MinExclusive(v) => ("minExclusive", s(v)),
        FacetKind::MaxExclusive(v) => ("maxExclusive", s(v)),
        FacetKind::TotalDigits(v) => ("totalDigits", n(*v)),
        FacetKind::FractionDigits(v) => ("fractionDigits", n(*v)),
        FacetKind::WhiteSpace(m) => ("whiteSpace", s(m.as_str())),
    };
    obj([
        ("annotation", ann_val(f.annotation.as_ref())),
        ("fixed", CValue::Bool(f.fixed)),
        ("kind", s(kind)),
        ("pos", pos_val(&f.pos)),
        ("value", value),
    ])
}

fn constraint_val(c: &IdentityConstraint) -> CValue {
    obj([
        ("annotation", ann_val(c.annotation.as_ref())),
        ("fields", CValue::Arr(c.fields.iter().map(|x| s(x)).collect())),
        ("kind", s(c.kind.as_str())),
        ("name", qname_val(&c.name)),
        ("pos", pos_val(&c.pos)),
        ("refer", opt_qname(c.refer.as_ref())),
        ("selector", s(&c.selector)),
    ])
}

fn attr_decl_val(a: &AttributeDecl) -> CValue {
    let ty = match &a.ty {
        AttributeType::Named(q) => obj([("kind", s("named")), ("name", qname_val(q))]),
        AttributeType::InlineSimple(st) => obj([("kind", s("inline_simple")), ("simple_type", simple_type_val(st))]),
        AttributeType::AnySimpleType => obj([("kind", s("any_simple_type"))]),
    };
    obj([
        ("annotation", ann_val(a.annotation.as_ref())),
        ("default", opt_str(a.default.as_deref())),
        ("fixed", opt_str(a.fixed.as_deref())),
        ("name", qname_val(&a.name)),
        ("pos", pos_val(&a.pos)),
        ("type", ty),
        ("use", s(a.use_kind.as_str())),
    ])
}

fn attr_item_val(item: &AttributeItem) -> CValue {
    match item {
        AttributeItem::Decl(d) => obj([("decl", attr_decl_val(d)), ("kind", s("decl"))]),
        AttributeItem::Ref { target, use_kind, default, fixed, annotation, pos } => obj([
            ("annotation", ann_val(annotation.as_ref())),
            ("default", opt_str(default.as_deref())),
            ("fixed", opt_str(fixed.as_deref())),
            ("kind", s("ref")),
            ("pos", pos_val(pos)),
            ("target", qname_val(target)),
            ("use", s(use_kind.as_str())),
        ]),
        AttributeItem::GroupRef { target, annotation, pos } => obj([
            ("annotation", ann_val(annotation.as_ref())),
            ("kind", s("group_ref")),
            ("pos", pos_val(pos)),
            ("target", qname_val(target)),
        ]),
    }
}

fn element_decl_val(e: &ElementDecl) -> CValue {
    let ty = match &e.ty {
        ElementType::Named(q) => obj([("kind", s("named")), ("name", qname_val(q))]),
        ElementType::InlineComplex(ct) => obj([("complex_type", complex_type_val(ct)), ("kind", s("inline_complex"))]),
        ElementType::InlineSimple(st) => obj([("kind", s("inline_simple")), ("simple_type", simple_type_val(st))]),
        ElementType::AnyType => obj([("kind", s("any_type"))]),
    };
    obj([
        ("abstract", CValue::Bool(e.is_abstract)),
        ("annotation", ann_val(e.annotation.as_ref())),
        ("constraints", CValue::Arr(e.constraints.iter().map(constraint_val).collect())),
        ("default", opt_str(e.default.as_deref())),
        ("fixed", opt_str(e.fixed.as_deref())),
        ("name", qname_val(&e.name)),
        ("nillable", CValue::Bool(e.nillable)),
        ("pos", pos_val(&e.pos)),
        ("substitution_group", opt_qname(e.substitution_group.as_ref())),
        ("type", ty),
    ])
}

fn particle_val(p: &Particle) -> CValue {
    match p {
        Particle::Sequence { occurs, items, annotation, pos } => obj([
            ("annotation", ann_val(annotation.as_ref())),
            ("items", CValue::Arr(items.iter().map(particle_val).collect())),
            ("kind", s("sequence")),
            ("occurs", occurs_val(*occurs)),
            ("pos", pos_val(pos)),
        ]),
        Particle::Choice { occurs, items, annotation, pos } => obj([
            ("annotation", ann_val(annotation.as_ref())),
            ("items", CValue::Arr(items.iter().map(particle_val).collect())),
            ("kind", s("choice")),
            ("occurs", occurs_val(*occurs)),
            ("pos", pos_val(pos)),
        ]),
        Particle::All { occurs, items, annotation, pos } => obj([
            ("annotation", ann_val(annotation.as_ref())),
            ("items", CValue::Arr(items.iter().map(particle_val).collect())),
            ("kind", s("all")),
            ("occurs", occurs_val(*occurs)),
            ("pos", pos_val(pos)),
        ]),
        Particle::Element(local) => {
            obj([("decl", element_decl_val(&local.decl)), ("kind", s("element")), ("occurs", occurs_val(local.occurs))])
        }
        Particle::ElementRef { target, occurs, annotation, pos } => obj([
            ("annotation", ann_val(annotation.as_ref())),
            ("kind", s("element_ref")),
            ("occurs", occurs_val(*occurs)),
            ("pos", pos_val(pos)),
            ("target", qname_val(target)),
        ]),
        Particle::GroupRef { target, occurs, annotation, pos } => obj([
            ("annotation", ann_val(annotation.as_ref())),
            ("kind", s("group_ref")),
            ("occurs", occurs_val(*occurs)),
            ("pos", pos_val(pos)),
            ("target", qname_val(target)),
        ]),
    }
}

fn complex_type_val(ct: &ComplexTypeDecl) -> CValue {
    let content = match &ct.content {
        ComplexContentModel::Direct { particle, attributes } => obj([
            ("attributes", CValue::Arr(attributes.iter().map(attr_item_val).collect())),
            ("kind", s("direct")),
            ("particle", particle.as_deref().map_or(CValue::Null, particle_val)),
        ]),
        ComplexContentModel::SimpleContent { derivation, base, facets, attributes, pos } => obj([
            ("attributes", CValue::Arr(attributes.iter().map(attr_item_val).collect())),
            ("base", qname_val(base)),
            ("derivation", s(derivation.as_str())),
            ("facets", CValue::Arr(facets.iter().map(facet_val).collect())),
            ("kind", s("simple_content")),
            ("pos", pos_val(pos)),
        ]),
        ComplexContentModel::ComplexContent { derivation, base, mixed, particle, attributes, pos } => obj([
            ("attributes", CValue::Arr(attributes.iter().map(attr_item_val).collect())),
            ("base", qname_val(base)),
            ("derivation", s(derivation.as_str())),
            ("kind", s("complex_content")),
            ("mixed", mixed.map_or(CValue::Null, CValue::Bool)),
            ("particle", particle.as_deref().map_or(CValue::Null, particle_val)),
            ("pos", pos_val(pos)),
        ]),
    };
    obj([
        ("abstract", CValue::Bool(ct.is_abstract)),
        ("annotation", ann_val(ct.annotation.as_ref())),
        ("content", content),
        ("mixed", CValue::Bool(ct.mixed)),
        ("name", opt_qname(ct.name.as_ref())),
        ("pos", pos_val(&ct.pos)),
    ])
}

fn simple_type_val(st: &SimpleTypeDecl) -> CValue {
    let variety = match &st.variety {
        SimpleTypeVariety::Restriction { base, inline_base, facets, pos } => obj([
            ("base", opt_qname(base.as_ref())),
            ("facets", CValue::Arr(facets.iter().map(facet_val).collect())),
            ("inline_base", inline_base.as_deref().map_or(CValue::Null, simple_type_val)),
            ("kind", s("restriction")),
            ("pos", pos_val(pos)),
        ]),
        SimpleTypeVariety::List { item_type, inline_item, pos } => obj([
            ("inline_item", inline_item.as_deref().map_or(CValue::Null, simple_type_val)),
            ("item_type", opt_qname(item_type.as_ref())),
            ("kind", s("list")),
            ("pos", pos_val(pos)),
        ]),
        SimpleTypeVariety::Union { member_types, inline_members, pos } => obj([
            ("inline_members", CValue::Arr(inline_members.iter().map(simple_type_val).collect())),
            ("kind", s("union")),
            ("member_types", CValue::Arr(member_types.iter().map(qname_val).collect())),
            ("pos", pos_val(pos)),
        ]),
    };
    obj([
        ("annotation", ann_val(st.annotation.as_ref())),
        ("name", opt_qname(st.name.as_ref())),
        ("pos", pos_val(&st.pos)),
        ("variety", variety),
    ])
}

fn group_val(g: &GroupDecl) -> CValue {
    obj([
        ("annotation", ann_val(g.annotation.as_ref())),
        ("name", qname_val(&g.name)),
        ("particle", particle_val(&g.particle)),
        ("pos", pos_val(&g.pos)),
    ])
}

fn attr_group_val(g: &AttributeGroupDecl) -> CValue {
    obj([
        ("annotation", ann_val(g.annotation.as_ref())),
        ("attributes", CValue::Arr(g.attributes.iter().map(attr_item_val).collect())),
        ("name", qname_val(&g.name)),
        ("pos", pos_val(&g.pos)),
    ])
}

fn decl_val(d: &Decl) -> CValue {
    match d {
        Decl::ComplexType(ct) => obj([("decl", complex_type_val(ct)), ("kind", s("complex_type"))]),
        Decl::SimpleType(st) => obj([("decl", simple_type_val(st)), ("kind", s("simple_type"))]),
        Decl::Element(e) => obj([("decl", element_decl_val(e)), ("kind", s("element"))]),
        Decl::Attribute(a) => obj([("decl", attr_decl_val(a)), ("kind", s("attribute"))]),
        Decl::Group(g) => obj([("decl", group_val(g)), ("kind", s("group"))]),
        Decl::AttributeGroup(g) => obj([("decl", attr_group_val(g)), ("kind", s("attribute_group"))]),
    }
}
