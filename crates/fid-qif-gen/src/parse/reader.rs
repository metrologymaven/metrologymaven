//! Streaming XSD 1.0 pull parser built on [`fid_xml::XmlReader`] and [`fid_io_reg::Budget`].

use crate::parse::ir::{
    Annotation, AttributeDecl, AttributeGroupDecl, AttributeItem, AttributeType, AttributeUse, ComplexContentModel,
    ComplexTypeDecl, Decl, DerivationKind, ElementDecl, ElementType, Facet, FacetKind, FileId, FormDefault, GroupDecl,
    IdentityConstraint, IdentityKind, LocalElement, MaxOccurs, Occurs, ParseError, Particle, Pos, QName, RefKind,
    SimpleTypeDecl, SimpleTypeVariety, WhiteSpaceMode, XML_NS, XS_NS,
};
use fid_io_reg::{BoundedSource, Budget, IoError, Position};
use fid_xml::{Element, EventKind, NsScope, XmlError, XmlEvent, XmlOptions, XmlReader};

/// Parsed contents of a single pinned XSD file.
#[derive(Debug, Clone)]
pub(crate) struct ParsedFile {
    pub target_namespace: Option<String>,
    pub decls: Vec<Decl>,
    /// Raw `schemaLocation` targets from `xs:include` and `xs:import`.
    pub schema_locations: Vec<String>,
}

/// Parses one XSD byte slice under `budget` using the hardened `fid-xml` pull reader.
pub(crate) fn parse_xsd_file(file: &FileId, bytes: &[u8], budget: Budget) -> Result<ParsedFile, ParseError> {
    let reader = XmlReader::with_options(BoundedSource::new(bytes, budget), XmlOptions::default());
    let mut parser = FileParser {
        file: file.clone(),
        reader,
        target_namespace: None,
        element_form_default: FormDefault::Unqualified,
        attribute_form_default: FormDefault::Unqualified,
        schema_locations: Vec::new(),
        decls: Vec::new(),
    };
    parser.parse_document()?;
    Ok(ParsedFile {
        target_namespace: parser.target_namespace,
        decls: parser.decls,
        schema_locations: parser.schema_locations,
    })
}

struct FileParser<'a> {
    file: FileId,
    reader: XmlReader<&'a [u8]>,
    target_namespace: Option<String>,
    element_form_default: FormDefault,
    attribute_form_default: FormDefault,
    schema_locations: Vec<String>,
    decls: Vec<Decl>,
}

impl<'a> FileParser<'a> {
    fn make_pos(&self, p: Position) -> Pos {
        Pos { file: self.file.clone(), line: p.line.unwrap_or(1), col: p.col.unwrap_or(1) }
    }

    fn map_xml_err(&self, err: XmlError) -> ParseError {
        let pos = self.make_pos(err.position().unwrap_or_default());
        match err {
            XmlError::ForbiddenConstruct { construct, .. } => {
                ParseError::ForbiddenXml { detail: construct.as_str().to_string(), pos }
            }
            XmlError::LimitExceeded { limit, value, .. } => {
                ParseError::LimitExceeded { detail: format!("{}={value}", limit.as_str()), pos }
            }
            XmlError::Harness(IoError::LimitExceeded { limit, value, .. }) => {
                ParseError::LimitExceeded { detail: format!("{}={value}", limit.as_str()), pos }
            }
            XmlError::Harness(io_err) => ParseError::Malformed { detail: io_err.code().to_string(), pos },
            XmlError::EncodingUnsupported { .. } => {
                ParseError::Malformed { detail: "xml_encoding_unsupported".to_string(), pos }
            }
            XmlError::Malformed { reason, .. } => ParseError::Malformed { detail: reason.to_string(), pos },
            XmlError::TruncatedFile { .. } => ParseError::Malformed { detail: "truncated_file".to_string(), pos },
        }
    }

    fn next_event(&mut self) -> Result<Option<XmlEvent>, ParseError> {
        self.reader.next_event().map_err(|e| self.map_xml_err(e))
    }

    /// Reads the next significant child event inside an element (`Start` or `End`).
    /// Ignores comments, processing instructions, and whitespace-only text.
    /// Rejects non-whitespace text where only element children are valid.
    fn next_child_event(&mut self) -> Result<ChildEvent, ParseError> {
        loop {
            let Some(ev) = self.next_event()? else {
                return Err(ParseError::Malformed {
                    detail: "unexpected_eof".to_string(),
                    pos: Pos { file: self.file.clone(), line: 1, col: 1 },
                });
            };
            let pos = self.make_pos(ev.pos);
            match ev.kind {
                EventKind::Start(el) => return Ok(ChildEvent::Start(el, pos)),
                EventKind::End { .. } => return Ok(ChildEvent::End),
                EventKind::Comment(_) | EventKind::Pi { .. } => continue,
                EventKind::Text(t) | EventKind::CData(t) => {
                    if !t.raw().trim().is_empty() {
                        return Err(ParseError::Malformed { detail: "unexpected_text_content".to_string(), pos });
                    }
                }
            }
        }
    }

    fn parse_document(&mut self) -> Result<(), ParseError> {
        let mut saw_root = false;
        while let Some(ev) = self.next_event()? {
            let pos = self.make_pos(ev.pos);
            match ev.kind {
                EventKind::Start(el) => {
                    if saw_root {
                        return Err(ParseError::Malformed { detail: "multiple_root_elements".to_string(), pos });
                    }
                    saw_root = true;
                    if !is_xs(&el, "schema") {
                        return Err(ParseError::UnsupportedConstruct { construct: display_el_name(&el), pos });
                    }
                    self.parse_schema(el, pos)?;
                }
                EventKind::Comment(_) | EventKind::Pi { .. } => continue,
                EventKind::Text(t) if t.raw().trim().is_empty() => continue,
                EventKind::End { .. } | EventKind::Text(_) | EventKind::CData(_) => {
                    return Err(ParseError::Malformed { detail: "unexpected_top_level_event".to_string(), pos });
                }
            }
        }
        if !saw_root {
            return Err(ParseError::Malformed {
                detail: "empty_schema_document".to_string(),
                pos: Pos { file: self.file.clone(), line: 1, col: 1 },
            });
        }
        Ok(())
    }

    fn parse_schema(&mut self, el: Element, pos: Pos) -> Result<(), ParseError> {
        check_allowed_attrs(
            &el,
            &pos,
            &["targetNamespace", "elementFormDefault", "attributeFormDefault", "version", "id"],
            true,
        )?;
        if let Some(tns) = attr_raw(&el, "targetNamespace") {
            let trimmed = tns.trim();
            if trimmed.is_empty() {
                return Err(ParseError::Malformed { detail: "empty_target_namespace".to_string(), pos });
            }
            self.target_namespace = Some(trimmed.to_string());
        }
        if let Some(efd) = attr_raw(&el, "elementFormDefault") {
            self.element_form_default = parse_form(efd, &pos)?;
        }
        if let Some(afd) = attr_raw(&el, "attributeFormDefault") {
            self.attribute_form_default = parse_form(afd, &pos)?;
        }

        loop {
            match self.next_child_event()? {
                ChildEvent::End => break,
                ChildEvent::Start(child, cpos) => {
                    if !is_xs_ns(&child) {
                        return Err(ParseError::UnsupportedConstruct { construct: display_el_name(&child), pos: cpos });
                    }
                    match child.name.local.as_str() {
                        "annotation" => {
                            let _ = self.parse_annotation(child, cpos)?;
                        }
                        "include" => self.parse_include(child, cpos)?,
                        "import" => self.parse_import(child, cpos)?,
                        "complexType" => {
                            let ct = self.parse_complex_type(child, cpos, true)?;
                            self.decls.push(Decl::ComplexType(ct));
                        }
                        "simpleType" => {
                            let st = self.parse_simple_type(child, cpos, true)?;
                            self.decls.push(Decl::SimpleType(st));
                        }
                        "element" => {
                            let elem = self.parse_top_level_element(child, cpos)?;
                            self.decls.push(Decl::Element(elem));
                        }
                        "attribute" => {
                            let attr = self.parse_top_level_attribute(child, cpos)?;
                            self.decls.push(Decl::Attribute(attr));
                        }
                        "group" => {
                            let grp = self.parse_top_level_group(child, cpos)?;
                            self.decls.push(Decl::Group(grp));
                        }
                        "attributeGroup" => {
                            let ag = self.parse_top_level_attribute_group(child, cpos)?;
                            self.decls.push(Decl::AttributeGroup(ag));
                        }
                        _ => {
                            return Err(ParseError::UnsupportedConstruct {
                                construct: format!("xs:{}", child.name.local),
                                pos: cpos,
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn parse_include(&mut self, el: Element, pos: Pos) -> Result<(), ParseError> {
        check_allowed_attrs(&el, &pos, &["schemaLocation", "id"], false)?;
        let loc = require_attr(&el, "schemaLocation", &pos)?.trim().to_string();
        if loc.is_empty() {
            return Err(ParseError::Malformed { detail: "empty_schema_location".to_string(), pos });
        }
        self.schema_locations.push(loc);
        self.consume_only_optional_annotation()?;
        Ok(())
    }

    fn parse_import(&mut self, el: Element, pos: Pos) -> Result<(), ParseError> {
        check_allowed_attrs(&el, &pos, &["namespace", "schemaLocation", "id"], false)?;
        if let Some(loc) = attr_raw(&el, "schemaLocation") {
            let trimmed = loc.trim();
            if trimmed.is_empty() {
                return Err(ParseError::Malformed { detail: "empty_schema_location".to_string(), pos });
            }
            self.schema_locations.push(trimmed.to_string());
        }
        self.consume_only_optional_annotation()?;
        Ok(())
    }

    fn consume_only_optional_annotation(&mut self) -> Result<Option<Annotation>, ParseError> {
        let mut ann = None;
        loop {
            match self.next_child_event()? {
                ChildEvent::End => break,
                ChildEvent::Start(child, cpos) => {
                    if is_xs(&child, "annotation") && ann.is_none() {
                        ann = self.parse_annotation(child, cpos)?;
                    } else {
                        return Err(ParseError::UnsupportedConstruct { construct: display_el_name(&child), pos: cpos });
                    }
                }
            }
        }
        Ok(ann)
    }

    fn parse_annotation(&mut self, el: Element, pos: Pos) -> Result<Option<Annotation>, ParseError> {
        check_allowed_attrs(&el, &pos, &["id"], false)?;
        let mut docs: Vec<String> = Vec::new();
        let mut first_doc_pos: Option<Pos> = None;
        loop {
            match self.next_child_event()? {
                ChildEvent::End => break,
                ChildEvent::Start(child, cpos) => {
                    if is_xs(&child, "documentation") {
                        check_allowed_attrs(&child, &cpos, &["source"], true)?;
                        let text = self.collect_mixed_text()?;
                        if first_doc_pos.is_none() {
                            first_doc_pos = Some(cpos);
                        }
                        docs.push(text.trim().to_string());
                    } else if is_xs(&child, "appinfo") {
                        check_allowed_attrs(&child, &cpos, &["source"], false)?;
                        let _ = self.collect_mixed_text()?;
                    } else {
                        return Err(ParseError::UnsupportedConstruct { construct: display_el_name(&child), pos: cpos });
                    }
                }
            }
        }
        if docs.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Annotation { text: docs.join("\n"), pos: first_doc_pos.unwrap_or(pos) }))
        }
    }

    /// Consumes arbitrary mixed content (nested elements + text) inside `xs:documentation`
    /// or `xs:appinfo` until the matching `End` event, returning concatenated text.
    fn collect_mixed_text(&mut self) -> Result<String, ParseError> {
        let mut depth: usize = 0;
        let mut out = String::new();
        loop {
            let Some(ev) = self.next_event()? else {
                return Err(ParseError::Malformed {
                    detail: "unexpected_eof".to_string(),
                    pos: Pos { file: self.file.clone(), line: 1, col: 1 },
                });
            };
            match ev.kind {
                EventKind::Start(_) => depth += 1,
                EventKind::End { .. } => {
                    if depth == 0 {
                        return Ok(out);
                    }
                    depth -= 1;
                }
                EventKind::Text(t) | EventKind::CData(t) => out.push_str(t.raw()),
                EventKind::Comment(_) | EventKind::Pi { .. } => {}
            }
        }
    }

    // ------------------------------------------------------------------ complexType

    fn parse_complex_type(&mut self, el: Element, pos: Pos, top_level: bool) -> Result<ComplexTypeDecl, ParseError> {
        if top_level {
            check_allowed_attrs(&el, &pos, &["name", "abstract", "mixed", "id"], false)?;
        } else {
            check_allowed_attrs(&el, &pos, &["mixed", "id"], false)?;
        }
        let name = if top_level {
            let local = require_ncname(&el, "name", &pos)?;
            Some(QName::new(self.target_namespace.clone(), local))
        } else {
            None
        };
        let is_abstract = parse_opt_bool(&el, "abstract", &pos)?.unwrap_or(false);
        let mixed = parse_opt_bool(&el, "mixed", &pos)?.unwrap_or(false);

        let mut annotation: Option<Annotation> = None;
        let mut content: Option<ComplexContentModel> = None;
        let mut direct_particle: Option<Box<Particle>> = None;
        let mut direct_attrs: Vec<AttributeItem> = Vec::new();
        let mut saw_direct_items = false;

        loop {
            match self.next_child_event()? {
                ChildEvent::End => break,
                ChildEvent::Start(child, cpos) => {
                    if !is_xs_ns(&child) {
                        return Err(ParseError::UnsupportedConstruct { construct: display_el_name(&child), pos: cpos });
                    }
                    match child.name.local.as_str() {
                        "annotation" => {
                            if annotation.is_some() || content.is_some() || saw_direct_items {
                                return Err(ParseError::Malformed {
                                    detail: "misplaced_annotation".to_string(),
                                    pos: cpos,
                                });
                            }
                            annotation = self.parse_annotation(child, cpos)?;
                        }
                        "simpleContent" => {
                            if content.is_some() || saw_direct_items {
                                return Err(ParseError::Malformed {
                                    detail: "duplicate_complex_type_content".to_string(),
                                    pos: cpos,
                                });
                            }
                            content = Some(self.parse_simple_content(child, cpos)?);
                        }
                        "complexContent" => {
                            if content.is_some() || saw_direct_items {
                                return Err(ParseError::Malformed {
                                    detail: "duplicate_complex_type_content".to_string(),
                                    pos: cpos,
                                });
                            }
                            content = Some(self.parse_complex_content(child, cpos)?);
                        }
                        "sequence" | "choice" | "all" | "group" => {
                            if content.is_some() || direct_particle.is_some() || !direct_attrs.is_empty() {
                                return Err(ParseError::Malformed {
                                    detail: "misplaced_particle_in_complex_type".to_string(),
                                    pos: cpos,
                                });
                            }
                            saw_direct_items = true;
                            direct_particle = Some(Box::new(self.parse_particle(child, cpos, false)?));
                        }
                        "attribute" => {
                            if content.is_some() {
                                return Err(ParseError::Malformed {
                                    detail: "attribute_outside_derivation".to_string(),
                                    pos: cpos,
                                });
                            }
                            saw_direct_items = true;
                            direct_attrs.push(self.parse_local_attribute(child, cpos)?);
                        }
                        "attributeGroup" => {
                            if content.is_some() {
                                return Err(ParseError::Malformed {
                                    detail: "attribute_group_outside_derivation".to_string(),
                                    pos: cpos,
                                });
                            }
                            saw_direct_items = true;
                            direct_attrs.push(self.parse_attribute_group_ref(child, cpos)?);
                        }
                        _ => {
                            return Err(ParseError::UnsupportedConstruct {
                                construct: format!("xs:{}", child.name.local),
                                pos: cpos,
                            });
                        }
                    }
                }
            }
        }

        let model =
            content.unwrap_or(ComplexContentModel::Direct { particle: direct_particle, attributes: direct_attrs });

        Ok(ComplexTypeDecl { name, is_abstract, mixed, content: model, annotation, pos })
    }

    fn parse_simple_content(&mut self, el: Element, pos: Pos) -> Result<ComplexContentModel, ParseError> {
        check_allowed_attrs(&el, &pos, &["id"], false)?;
        let mut derived: Option<ComplexContentModel> = None;
        let mut saw_ann = false;
        loop {
            match self.next_child_event()? {
                ChildEvent::End => break,
                ChildEvent::Start(child, cpos) => {
                    if !is_xs_ns(&child) {
                        return Err(ParseError::UnsupportedConstruct { construct: display_el_name(&child), pos: cpos });
                    }
                    match child.name.local.as_str() {
                        "annotation" if !saw_ann && derived.is_none() => {
                            saw_ann = true;
                            let _ = self.parse_annotation(child, cpos)?;
                        }
                        "extension" if derived.is_none() => {
                            derived =
                                Some(self.parse_simple_content_derivation(child, cpos, DerivationKind::Extension)?);
                        }
                        "restriction" if derived.is_none() => {
                            derived =
                                Some(self.parse_simple_content_derivation(child, cpos, DerivationKind::Restriction)?);
                        }
                        _ => {
                            return Err(ParseError::UnsupportedConstruct {
                                construct: format!("xs:{}", child.name.local),
                                pos: cpos,
                            });
                        }
                    }
                }
            }
        }
        derived.ok_or(ParseError::Malformed { detail: "empty_simple_content".to_string(), pos })
    }

    fn parse_simple_content_derivation(
        &mut self,
        el: Element,
        pos: Pos,
        derivation: DerivationKind,
    ) -> Result<ComplexContentModel, ParseError> {
        check_allowed_attrs(&el, &pos, &["base", "id"], false)?;
        let base_raw = require_attr(&el, "base", &pos)?;
        let base = resolve_qname(base_raw, &el.ns_scope, &pos)?;
        let mut facets = Vec::new();
        let mut attributes = Vec::new();
        let mut saw_ann = false;

        loop {
            match self.next_child_event()? {
                ChildEvent::End => break,
                ChildEvent::Start(child, cpos) => {
                    if !is_xs_ns(&child) {
                        return Err(ParseError::UnsupportedConstruct { construct: display_el_name(&child), pos: cpos });
                    }
                    match child.name.local.as_str() {
                        "annotation" if !saw_ann && facets.is_empty() && attributes.is_empty() => {
                            saw_ann = true;
                            let _ = self.parse_annotation(child, cpos)?;
                        }
                        "attribute" => attributes.push(self.parse_local_attribute(child, cpos)?),
                        "attributeGroup" => attributes.push(self.parse_attribute_group_ref(child, cpos)?),
                        other if derivation == DerivationKind::Restriction && is_facet_name(other) => {
                            if !attributes.is_empty() {
                                return Err(ParseError::Malformed {
                                    detail: "facet_after_attribute".to_string(),
                                    pos: cpos,
                                });
                            }
                            facets.push(self.parse_facet(child, cpos)?);
                        }
                        _ => {
                            return Err(ParseError::UnsupportedConstruct {
                                construct: format!("xs:{}", child.name.local),
                                pos: cpos,
                            });
                        }
                    }
                }
            }
        }

        Ok(ComplexContentModel::SimpleContent { derivation, base, facets, attributes, pos })
    }

    fn parse_complex_content(&mut self, el: Element, pos: Pos) -> Result<ComplexContentModel, ParseError> {
        check_allowed_attrs(&el, &pos, &["mixed", "id"], false)?;
        let mixed = parse_opt_bool(&el, "mixed", &pos)?;
        let mut derived: Option<ComplexContentModel> = None;
        let mut saw_ann = false;

        loop {
            match self.next_child_event()? {
                ChildEvent::End => break,
                ChildEvent::Start(child, cpos) => {
                    if !is_xs_ns(&child) {
                        return Err(ParseError::UnsupportedConstruct { construct: display_el_name(&child), pos: cpos });
                    }
                    match child.name.local.as_str() {
                        "annotation" if !saw_ann && derived.is_none() => {
                            saw_ann = true;
                            let _ = self.parse_annotation(child, cpos)?;
                        }
                        "extension" if derived.is_none() => {
                            derived = Some(self.parse_complex_content_derivation(
                                child,
                                cpos,
                                DerivationKind::Extension,
                                mixed,
                            )?);
                        }
                        "restriction" if derived.is_none() => {
                            derived = Some(self.parse_complex_content_derivation(
                                child,
                                cpos,
                                DerivationKind::Restriction,
                                mixed,
                            )?);
                        }
                        _ => {
                            return Err(ParseError::UnsupportedConstruct {
                                construct: format!("xs:{}", child.name.local),
                                pos: cpos,
                            });
                        }
                    }
                }
            }
        }

        derived.ok_or(ParseError::Malformed { detail: "empty_complex_content".to_string(), pos })
    }

    fn parse_complex_content_derivation(
        &mut self,
        el: Element,
        pos: Pos,
        derivation: DerivationKind,
        mixed: Option<bool>,
    ) -> Result<ComplexContentModel, ParseError> {
        check_allowed_attrs(&el, &pos, &["base", "id"], false)?;
        let base_raw = require_attr(&el, "base", &pos)?;
        let base = resolve_qname(base_raw, &el.ns_scope, &pos)?;
        let mut particle: Option<Box<Particle>> = None;
        let mut attributes: Vec<AttributeItem> = Vec::new();
        let mut saw_ann = false;

        loop {
            match self.next_child_event()? {
                ChildEvent::End => break,
                ChildEvent::Start(child, cpos) => {
                    if !is_xs_ns(&child) {
                        return Err(ParseError::UnsupportedConstruct { construct: display_el_name(&child), pos: cpos });
                    }
                    match child.name.local.as_str() {
                        "annotation" if !saw_ann && particle.is_none() && attributes.is_empty() => {
                            saw_ann = true;
                            let _ = self.parse_annotation(child, cpos)?;
                        }
                        "sequence" | "choice" | "all" | "group" => {
                            if particle.is_some() || !attributes.is_empty() {
                                return Err(ParseError::Malformed {
                                    detail: "misplaced_particle_in_derivation".to_string(),
                                    pos: cpos,
                                });
                            }
                            particle = Some(Box::new(self.parse_particle(child, cpos, false)?));
                        }
                        "attribute" => attributes.push(self.parse_local_attribute(child, cpos)?),
                        "attributeGroup" => attributes.push(self.parse_attribute_group_ref(child, cpos)?),
                        _ => {
                            return Err(ParseError::UnsupportedConstruct {
                                construct: format!("xs:{}", child.name.local),
                                pos: cpos,
                            });
                        }
                    }
                }
            }
        }

        Ok(ComplexContentModel::ComplexContent { derivation, base, mixed, particle, attributes, pos })
    }

    // ------------------------------------------------------------------ simpleType

    fn parse_simple_type(&mut self, el: Element, pos: Pos, top_level: bool) -> Result<SimpleTypeDecl, ParseError> {
        if top_level {
            check_allowed_attrs(&el, &pos, &["name", "id"], false)?;
        } else {
            check_allowed_attrs(&el, &pos, &["id"], false)?;
        }
        let name = if top_level {
            let local = require_ncname(&el, "name", &pos)?;
            Some(QName::new(self.target_namespace.clone(), local))
        } else {
            None
        };
        let mut annotation: Option<Annotation> = None;
        let mut variety: Option<SimpleTypeVariety> = None;

        loop {
            match self.next_child_event()? {
                ChildEvent::End => break,
                ChildEvent::Start(child, cpos) => {
                    if !is_xs_ns(&child) {
                        return Err(ParseError::UnsupportedConstruct { construct: display_el_name(&child), pos: cpos });
                    }
                    match child.name.local.as_str() {
                        "annotation" if annotation.is_none() && variety.is_none() => {
                            annotation = self.parse_annotation(child, cpos)?;
                        }
                        "restriction" if variety.is_none() => {
                            variety = Some(self.parse_simple_restriction(child, cpos)?);
                        }
                        "list" if variety.is_none() => {
                            variety = Some(self.parse_simple_list(child, cpos)?);
                        }
                        "union" if variety.is_none() => {
                            variety = Some(self.parse_simple_union(child, cpos)?);
                        }
                        _ => {
                            return Err(ParseError::UnsupportedConstruct {
                                construct: format!("xs:{}", child.name.local),
                                pos: cpos,
                            });
                        }
                    }
                }
            }
        }

        let variety = variety.ok_or_else(|| ParseError::Malformed {
            detail: "simple_type_missing_variety".to_string(),
            pos: pos.clone(),
        })?;

        Ok(SimpleTypeDecl { name, variety, annotation, pos })
    }

    fn parse_simple_restriction(&mut self, el: Element, pos: Pos) -> Result<SimpleTypeVariety, ParseError> {
        check_allowed_attrs(&el, &pos, &["base", "id"], false)?;
        let base = match attr_raw(&el, "base") {
            Some(raw) => Some(resolve_qname(raw, &el.ns_scope, &pos)?),
            None => None,
        };
        let mut inline_base: Option<Box<SimpleTypeDecl>> = None;
        let mut facets = Vec::new();
        let mut saw_ann = false;

        loop {
            match self.next_child_event()? {
                ChildEvent::End => break,
                ChildEvent::Start(child, cpos) => {
                    if !is_xs_ns(&child) {
                        return Err(ParseError::UnsupportedConstruct { construct: display_el_name(&child), pos: cpos });
                    }
                    match child.name.local.as_str() {
                        "annotation" if !saw_ann && inline_base.is_none() && facets.is_empty() => {
                            saw_ann = true;
                            let _ = self.parse_annotation(child, cpos)?;
                        }
                        "simpleType" if base.is_none() && inline_base.is_none() && facets.is_empty() => {
                            inline_base = Some(Box::new(self.parse_simple_type(child, cpos, false)?));
                        }
                        other if is_facet_name(other) => {
                            facets.push(self.parse_facet(child, cpos)?);
                        }
                        _ => {
                            return Err(ParseError::UnsupportedConstruct {
                                construct: format!("xs:{}", child.name.local),
                                pos: cpos,
                            });
                        }
                    }
                }
            }
        }

        if base.is_none() && inline_base.is_none() {
            return Err(ParseError::Malformed { detail: "restriction_missing_base".to_string(), pos });
        }
        Ok(SimpleTypeVariety::Restriction { base, inline_base, facets, pos })
    }

    fn parse_simple_list(&mut self, el: Element, pos: Pos) -> Result<SimpleTypeVariety, ParseError> {
        check_allowed_attrs(&el, &pos, &["itemType", "id"], false)?;
        let item_type = match attr_raw(&el, "itemType") {
            Some(raw) => Some(resolve_qname(raw, &el.ns_scope, &pos)?),
            None => None,
        };
        let mut inline_item: Option<Box<SimpleTypeDecl>> = None;
        let mut saw_ann = false;

        loop {
            match self.next_child_event()? {
                ChildEvent::End => break,
                ChildEvent::Start(child, cpos) => {
                    if !is_xs_ns(&child) {
                        return Err(ParseError::UnsupportedConstruct { construct: display_el_name(&child), pos: cpos });
                    }
                    match child.name.local.as_str() {
                        "annotation" if !saw_ann && inline_item.is_none() => {
                            saw_ann = true;
                            let _ = self.parse_annotation(child, cpos)?;
                        }
                        "simpleType" if item_type.is_none() && inline_item.is_none() => {
                            inline_item = Some(Box::new(self.parse_simple_type(child, cpos, false)?));
                        }
                        _ => {
                            return Err(ParseError::UnsupportedConstruct {
                                construct: format!("xs:{}", child.name.local),
                                pos: cpos,
                            });
                        }
                    }
                }
            }
        }

        if item_type.is_none() && inline_item.is_none() {
            return Err(ParseError::Malformed { detail: "list_missing_item_type".to_string(), pos });
        }
        Ok(SimpleTypeVariety::List { item_type, inline_item, pos })
    }

    fn parse_simple_union(&mut self, el: Element, pos: Pos) -> Result<SimpleTypeVariety, ParseError> {
        check_allowed_attrs(&el, &pos, &["memberTypes", "id"], false)?;
        let mut member_types = Vec::new();
        if let Some(raw) = attr_raw(&el, "memberTypes") {
            for token in raw.split_whitespace() {
                member_types.push(resolve_qname(token, &el.ns_scope, &pos)?);
            }
        }
        let mut inline_members = Vec::new();
        let mut saw_ann = false;

        loop {
            match self.next_child_event()? {
                ChildEvent::End => break,
                ChildEvent::Start(child, cpos) => {
                    if !is_xs_ns(&child) {
                        return Err(ParseError::UnsupportedConstruct { construct: display_el_name(&child), pos: cpos });
                    }
                    match child.name.local.as_str() {
                        "annotation" if !saw_ann && inline_members.is_empty() => {
                            saw_ann = true;
                            let _ = self.parse_annotation(child, cpos)?;
                        }
                        "simpleType" => {
                            inline_members.push(self.parse_simple_type(child, cpos, false)?);
                        }
                        _ => {
                            return Err(ParseError::UnsupportedConstruct {
                                construct: format!("xs:{}", child.name.local),
                                pos: cpos,
                            });
                        }
                    }
                }
            }
        }

        if member_types.is_empty() && inline_members.is_empty() {
            return Err(ParseError::Malformed { detail: "union_missing_members".to_string(), pos });
        }
        Ok(SimpleTypeVariety::Union { member_types, inline_members, pos })
    }

    fn parse_facet(&mut self, el: Element, pos: Pos) -> Result<Facet, ParseError> {
        let allows_fixed = !matches!(el.name.local.as_str(), "enumeration" | "pattern");
        if allows_fixed {
            check_allowed_attrs(&el, &pos, &["value", "fixed", "id"], false)?;
        } else {
            check_allowed_attrs(&el, &pos, &["value", "id"], false)?;
        }
        let value = require_attr(&el, "value", &pos)?;
        let fixed = if allows_fixed { parse_opt_bool(&el, "fixed", &pos)?.unwrap_or(false) } else { false };
        let kind = match el.name.local.as_str() {
            "enumeration" => FacetKind::Enumeration(value.to_string()),
            "pattern" => FacetKind::Pattern(value.to_string()),
            "length" => FacetKind::Length(parse_u64(value, &pos, "invalid_facet_u64")?),
            "minLength" => FacetKind::MinLength(parse_u64(value, &pos, "invalid_facet_u64")?),
            "maxLength" => FacetKind::MaxLength(parse_u64(value, &pos, "invalid_facet_u64")?),
            "minInclusive" => FacetKind::MinInclusive(value.to_string()),
            "maxInclusive" => FacetKind::MaxInclusive(value.to_string()),
            "minExclusive" => FacetKind::MinExclusive(value.to_string()),
            "maxExclusive" => FacetKind::MaxExclusive(value.to_string()),
            "totalDigits" => {
                let n = parse_u64(value, &pos, "invalid_facet_u64")?;
                if n == 0 {
                    return Err(ParseError::Malformed { detail: "total_digits_must_be_positive".to_string(), pos });
                }
                FacetKind::TotalDigits(n)
            }
            "fractionDigits" => FacetKind::FractionDigits(parse_u64(value, &pos, "invalid_facet_u64")?),
            "whiteSpace" => {
                let mode = match value.trim() {
                    "preserve" => WhiteSpaceMode::Preserve,
                    "replace" => WhiteSpaceMode::Replace,
                    "collapse" => WhiteSpaceMode::Collapse,
                    _ => return Err(ParseError::Malformed { detail: "invalid_whitespace_facet".to_string(), pos }),
                };
                FacetKind::WhiteSpace(mode)
            }
            other => {
                return Err(ParseError::UnsupportedConstruct { construct: format!("xs:{other}"), pos });
            }
        };
        let annotation = self.consume_only_optional_annotation()?;
        Ok(Facet { kind, fixed, annotation, pos })
    }

    // ------------------------------------------------------------------ elements

    fn parse_top_level_element(&mut self, el: Element, pos: Pos) -> Result<ElementDecl, ParseError> {
        check_allowed_attrs(
            &el,
            &pos,
            &["name", "type", "substitutionGroup", "abstract", "nillable", "default", "fixed", "id"],
            false,
        )?;
        let local = require_ncname(&el, "name", &pos)?;
        let name = QName::new(self.target_namespace.clone(), local);
        let substitution_group = match attr_raw(&el, "substitutionGroup") {
            Some(raw) => Some(resolve_qname(raw, &el.ns_scope, &pos)?),
            None => None,
        };
        let is_abstract = parse_opt_bool(&el, "abstract", &pos)?.unwrap_or(false);
        self.parse_element_body(el, pos, name, substitution_group, is_abstract)
    }

    fn parse_local_element_particle(&mut self, el: Element, pos: Pos) -> Result<Particle, ParseError> {
        let occurs = parse_occurs(&el, &pos)?;
        if let Some(ref_raw) = attr_raw(&el, "ref") {
            check_allowed_attrs(&el, &pos, &["ref", "minOccurs", "maxOccurs", "id"], false)?;
            let target = resolve_qname(ref_raw, &el.ns_scope, &pos)?;
            let annotation = self.consume_only_optional_annotation()?;
            return Ok(Particle::ElementRef { target, occurs, annotation, pos });
        }

        check_allowed_attrs(
            &el,
            &pos,
            &["name", "type", "minOccurs", "maxOccurs", "nillable", "default", "fixed", "form", "id"],
            false,
        )?;
        let local = require_ncname(&el, "name", &pos)?;
        let form = match attr_raw(&el, "form") {
            Some(raw) => parse_form(raw, &pos)?,
            None => self.element_form_default,
        };
        let ns = match form {
            FormDefault::Qualified => self.target_namespace.clone(),
            FormDefault::Unqualified => None,
        };
        let name = QName::new(ns, local);
        let decl = self.parse_element_body(el, pos, name, None, false)?;
        Ok(Particle::Element(Box::new(LocalElement { occurs, decl })))
    }

    fn parse_element_body(
        &mut self,
        el: Element,
        pos: Pos,
        name: QName,
        substitution_group: Option<QName>,
        is_abstract: bool,
    ) -> Result<ElementDecl, ParseError> {
        let nillable = parse_opt_bool(&el, "nillable", &pos)?.unwrap_or(false);
        let default = attr_raw(&el, "default").map(str::to_string);
        let fixed = attr_raw(&el, "fixed").map(str::to_string);
        if default.is_some() && fixed.is_some() {
            return Err(ParseError::Malformed { detail: "element_both_default_and_fixed".to_string(), pos });
        }
        let type_attr = match attr_raw(&el, "type") {
            Some(raw) => Some(resolve_qname(raw, &el.ns_scope, &pos)?),
            None => None,
        };

        let mut annotation: Option<Annotation> = None;
        let mut inline_type: Option<ElementType> = None;
        let mut constraints: Vec<IdentityConstraint> = Vec::new();

        loop {
            match self.next_child_event()? {
                ChildEvent::End => break,
                ChildEvent::Start(child, cpos) => {
                    if !is_xs_ns(&child) {
                        return Err(ParseError::UnsupportedConstruct { construct: display_el_name(&child), pos: cpos });
                    }
                    match child.name.local.as_str() {
                        "annotation" if annotation.is_none() && inline_type.is_none() && constraints.is_empty() => {
                            annotation = self.parse_annotation(child, cpos)?;
                        }
                        "complexType" if type_attr.is_none() && inline_type.is_none() && constraints.is_empty() => {
                            let ct = self.parse_complex_type(child, cpos, false)?;
                            inline_type = Some(ElementType::InlineComplex(Box::new(ct)));
                        }
                        "simpleType" if type_attr.is_none() && inline_type.is_none() && constraints.is_empty() => {
                            let st = self.parse_simple_type(child, cpos, false)?;
                            inline_type = Some(ElementType::InlineSimple(Box::new(st)));
                        }
                        "key" => constraints.push(self.parse_identity_constraint(child, cpos, IdentityKind::Key)?),
                        "keyref" => {
                            constraints.push(self.parse_identity_constraint(child, cpos, IdentityKind::Keyref)?)
                        }
                        "unique" => {
                            constraints.push(self.parse_identity_constraint(child, cpos, IdentityKind::Unique)?)
                        }
                        _ => {
                            return Err(ParseError::UnsupportedConstruct {
                                construct: format!("xs:{}", child.name.local),
                                pos: cpos,
                            });
                        }
                    }
                }
            }
        }

        let ty = match (type_attr, inline_type) {
            (Some(q), None) => ElementType::Named(q),
            (None, Some(it)) => it,
            (None, None) => ElementType::AnyType,
            (Some(_), Some(_)) => {
                return Err(ParseError::Malformed { detail: "element_both_type_attr_and_inline".to_string(), pos });
            }
        };

        Ok(ElementDecl {
            name,
            ty,
            substitution_group,
            is_abstract,
            nillable,
            default,
            fixed,
            constraints,
            annotation,
            pos,
        })
    }

    fn parse_identity_constraint(
        &mut self,
        el: Element,
        pos: Pos,
        kind: IdentityKind,
    ) -> Result<IdentityConstraint, ParseError> {
        if kind == IdentityKind::Keyref {
            check_allowed_attrs(&el, &pos, &["name", "refer", "id"], false)?;
        } else {
            check_allowed_attrs(&el, &pos, &["name", "id"], false)?;
        }
        let local = require_ncname(&el, "name", &pos)?;
        let name = QName::new(self.target_namespace.clone(), local);
        let refer = if kind == IdentityKind::Keyref {
            let raw = require_attr(&el, "refer", &pos)?;
            Some(resolve_qname(raw, &el.ns_scope, &pos)?)
        } else {
            None
        };

        let mut annotation: Option<Annotation> = None;
        let mut selector: Option<String> = None;
        let mut fields: Vec<String> = Vec::new();

        loop {
            match self.next_child_event()? {
                ChildEvent::End => break,
                ChildEvent::Start(child, cpos) => {
                    if !is_xs_ns(&child) {
                        return Err(ParseError::UnsupportedConstruct { construct: display_el_name(&child), pos: cpos });
                    }
                    match child.name.local.as_str() {
                        "annotation" if annotation.is_none() && selector.is_none() && fields.is_empty() => {
                            annotation = self.parse_annotation(child, cpos)?;
                        }
                        "selector" if selector.is_none() && fields.is_empty() => {
                            check_allowed_attrs(&child, &cpos, &["xpath", "id"], false)?;
                            let xp = require_attr(&child, "xpath", &cpos)?.to_string();
                            let _ = self.consume_only_optional_annotation()?;
                            selector = Some(xp);
                        }
                        "field" if selector.is_some() => {
                            check_allowed_attrs(&child, &cpos, &["xpath", "id"], false)?;
                            let xp = require_attr(&child, "xpath", &cpos)?.to_string();
                            let _ = self.consume_only_optional_annotation()?;
                            fields.push(xp);
                        }
                        _ => {
                            return Err(ParseError::UnsupportedConstruct {
                                construct: format!("xs:{}", child.name.local),
                                pos: cpos,
                            });
                        }
                    }
                }
            }
        }

        let selector = selector.ok_or_else(|| ParseError::Malformed {
            detail: "identity_constraint_missing_selector".to_string(),
            pos: pos.clone(),
        })?;
        if fields.is_empty() {
            return Err(ParseError::Malformed { detail: "identity_constraint_missing_field".to_string(), pos });
        }

        Ok(IdentityConstraint { kind, name, refer, selector, fields, annotation, pos })
    }

    // ------------------------------------------------------------------ attributes

    fn parse_top_level_attribute(&mut self, el: Element, pos: Pos) -> Result<AttributeDecl, ParseError> {
        check_allowed_attrs(&el, &pos, &["name", "type", "default", "fixed", "id"], false)?;
        let local = require_ncname(&el, "name", &pos)?;
        let name = QName::new(self.target_namespace.clone(), local);
        self.parse_attribute_body(el, pos, name, AttributeUse::Optional)
    }

    fn parse_local_attribute(&mut self, el: Element, pos: Pos) -> Result<AttributeItem, ParseError> {
        let use_kind = parse_opt_attribute_use(&el, &pos)?.unwrap_or(AttributeUse::Optional);
        let default = attr_raw(&el, "default").map(str::to_string);
        let fixed = attr_raw(&el, "fixed").map(str::to_string);
        if default.is_some() && fixed.is_some() {
            return Err(ParseError::Malformed { detail: "attribute_both_default_and_fixed".to_string(), pos });
        }
        if let Some(ref_raw) = attr_raw(&el, "ref") {
            check_allowed_attrs(&el, &pos, &["ref", "use", "default", "fixed", "id"], false)?;
            let target = resolve_qname(ref_raw, &el.ns_scope, &pos)?;
            let annotation = self.consume_only_optional_annotation()?;
            return Ok(AttributeItem::Ref { target, use_kind, default, fixed, annotation, pos });
        }

        check_allowed_attrs(&el, &pos, &["name", "type", "use", "default", "fixed", "form", "id"], false)?;
        let local = require_ncname(&el, "name", &pos)?;
        let form = match attr_raw(&el, "form") {
            Some(raw) => parse_form(raw, &pos)?,
            None => self.attribute_form_default,
        };
        let ns = match form {
            FormDefault::Qualified => self.target_namespace.clone(),
            FormDefault::Unqualified => None,
        };
        let name = QName::new(ns, local);
        let decl = self.parse_attribute_body(el, pos, name, use_kind)?;
        Ok(AttributeItem::Decl(decl))
    }

    fn parse_attribute_body(
        &mut self,
        el: Element,
        pos: Pos,
        name: QName,
        use_kind: AttributeUse,
    ) -> Result<AttributeDecl, ParseError> {
        let default = attr_raw(&el, "default").map(str::to_string);
        let fixed = attr_raw(&el, "fixed").map(str::to_string);
        if default.is_some() && fixed.is_some() {
            return Err(ParseError::Malformed { detail: "attribute_both_default_and_fixed".to_string(), pos });
        }
        let type_attr = match attr_raw(&el, "type") {
            Some(raw) => Some(resolve_qname(raw, &el.ns_scope, &pos)?),
            None => None,
        };

        let mut annotation: Option<Annotation> = None;
        let mut inline_type: Option<AttributeType> = None;

        loop {
            match self.next_child_event()? {
                ChildEvent::End => break,
                ChildEvent::Start(child, cpos) => {
                    if !is_xs_ns(&child) {
                        return Err(ParseError::UnsupportedConstruct { construct: display_el_name(&child), pos: cpos });
                    }
                    match child.name.local.as_str() {
                        "annotation" if annotation.is_none() && inline_type.is_none() => {
                            annotation = self.parse_annotation(child, cpos)?;
                        }
                        "simpleType" if type_attr.is_none() && inline_type.is_none() => {
                            let st = self.parse_simple_type(child, cpos, false)?;
                            inline_type = Some(AttributeType::InlineSimple(Box::new(st)));
                        }
                        _ => {
                            return Err(ParseError::UnsupportedConstruct {
                                construct: format!("xs:{}", child.name.local),
                                pos: cpos,
                            });
                        }
                    }
                }
            }
        }

        let ty = match (type_attr, inline_type) {
            (Some(q), None) => AttributeType::Named(q),
            (None, Some(it)) => it,
            (None, None) => AttributeType::AnySimpleType,
            (Some(_), Some(_)) => {
                return Err(ParseError::Malformed { detail: "attribute_both_type_attr_and_inline".to_string(), pos });
            }
        };

        Ok(AttributeDecl { name, ty, use_kind, default, fixed, annotation, pos })
    }

    // ------------------------------------------------------------------ groups & particles

    fn parse_top_level_group(&mut self, el: Element, pos: Pos) -> Result<GroupDecl, ParseError> {
        check_allowed_attrs(&el, &pos, &["name", "id"], false)?;
        let local = require_ncname(&el, "name", &pos)?;
        let name = QName::new(self.target_namespace.clone(), local);
        let mut annotation: Option<Annotation> = None;
        let mut particle: Option<Particle> = None;

        loop {
            match self.next_child_event()? {
                ChildEvent::End => break,
                ChildEvent::Start(child, cpos) => {
                    if !is_xs_ns(&child) {
                        return Err(ParseError::UnsupportedConstruct { construct: display_el_name(&child), pos: cpos });
                    }
                    match child.name.local.as_str() {
                        "annotation" if annotation.is_none() && particle.is_none() => {
                            annotation = self.parse_annotation(child, cpos)?;
                        }
                        "sequence" | "choice" | "all" if particle.is_none() => {
                            particle = Some(self.parse_particle(child, cpos, true)?);
                        }
                        _ => {
                            return Err(ParseError::UnsupportedConstruct {
                                construct: format!("xs:{}", child.name.local),
                                pos: cpos,
                            });
                        }
                    }
                }
            }
        }

        let particle = particle.ok_or_else(|| ParseError::Malformed {
            detail: "group_missing_compositor".to_string(),
            pos: pos.clone(),
        })?;
        Ok(GroupDecl { name, particle, annotation, pos })
    }

    fn parse_top_level_attribute_group(&mut self, el: Element, pos: Pos) -> Result<AttributeGroupDecl, ParseError> {
        check_allowed_attrs(&el, &pos, &["name", "id"], false)?;
        let local = require_ncname(&el, "name", &pos)?;
        let name = QName::new(self.target_namespace.clone(), local);
        let mut annotation: Option<Annotation> = None;
        let mut attributes = Vec::new();

        loop {
            match self.next_child_event()? {
                ChildEvent::End => break,
                ChildEvent::Start(child, cpos) => {
                    if !is_xs_ns(&child) {
                        return Err(ParseError::UnsupportedConstruct { construct: display_el_name(&child), pos: cpos });
                    }
                    match child.name.local.as_str() {
                        "annotation" if annotation.is_none() && attributes.is_empty() => {
                            annotation = self.parse_annotation(child, cpos)?;
                        }
                        "attribute" => attributes.push(self.parse_local_attribute(child, cpos)?),
                        "attributeGroup" => attributes.push(self.parse_attribute_group_ref(child, cpos)?),
                        _ => {
                            return Err(ParseError::UnsupportedConstruct {
                                construct: format!("xs:{}", child.name.local),
                                pos: cpos,
                            });
                        }
                    }
                }
            }
        }

        Ok(AttributeGroupDecl { name, attributes, annotation, pos })
    }

    fn parse_attribute_group_ref(&mut self, el: Element, pos: Pos) -> Result<AttributeItem, ParseError> {
        check_allowed_attrs(&el, &pos, &["ref", "id"], false)?;
        let ref_raw = require_attr(&el, "ref", &pos)?;
        let target = resolve_qname(ref_raw, &el.ns_scope, &pos)?;
        let annotation = self.consume_only_optional_annotation()?;
        Ok(AttributeItem::GroupRef { target, annotation, pos })
    }

    fn parse_particle(&mut self, el: Element, pos: Pos, in_group_def: bool) -> Result<Particle, ParseError> {
        match el.name.local.as_str() {
            "sequence" => self.parse_compositor(el, pos, CompositorKind::Sequence, in_group_def),
            "choice" => self.parse_compositor(el, pos, CompositorKind::Choice, in_group_def),
            "all" => self.parse_compositor(el, pos, CompositorKind::All, in_group_def),
            "element" => self.parse_local_element_particle(el, pos),
            "group" => {
                check_allowed_attrs(&el, &pos, &["ref", "minOccurs", "maxOccurs", "id"], false)?;
                let ref_raw = require_attr(&el, "ref", &pos)?;
                let target = resolve_qname(ref_raw, &el.ns_scope, &pos)?;
                let occurs = parse_occurs(&el, &pos)?;
                let annotation = self.consume_only_optional_annotation()?;
                Ok(Particle::GroupRef { target, occurs, annotation, pos })
            }
            other => Err(ParseError::UnsupportedConstruct { construct: format!("xs:{other}"), pos }),
        }
    }

    fn parse_compositor(
        &mut self,
        el: Element,
        pos: Pos,
        kind: CompositorKind,
        in_group_def: bool,
    ) -> Result<Particle, ParseError> {
        if in_group_def {
            check_allowed_attrs(&el, &pos, &["id"], false)?;
        } else {
            check_allowed_attrs(&el, &pos, &["minOccurs", "maxOccurs", "id"], false)?;
        }
        let occurs = if in_group_def { Occurs::default() } else { parse_occurs(&el, &pos)? };
        if kind == CompositorKind::All && (occurs.min > 1 || occurs.max != MaxOccurs::Bounded(1)) {
            return Err(ParseError::Malformed { detail: "all_invalid_occurs".to_string(), pos });
        }

        let mut annotation: Option<Annotation> = None;
        let mut items: Vec<Particle> = Vec::new();

        loop {
            match self.next_child_event()? {
                ChildEvent::End => break,
                ChildEvent::Start(child, cpos) => {
                    if !is_xs_ns(&child) {
                        return Err(ParseError::UnsupportedConstruct { construct: display_el_name(&child), pos: cpos });
                    }
                    match child.name.local.as_str() {
                        "annotation" if annotation.is_none() && items.is_empty() => {
                            annotation = self.parse_annotation(child, cpos)?;
                        }
                        "element" => {
                            let item = self.parse_local_element_particle(child, cpos)?;
                            items.push(item);
                        }
                        "sequence" | "choice" | "group" if kind != CompositorKind::All => {
                            items.push(self.parse_particle(child, cpos, false)?);
                        }
                        _ => {
                            return Err(ParseError::UnsupportedConstruct {
                                construct: format!("xs:{}", child.name.local),
                                pos: cpos,
                            });
                        }
                    }
                }
            }
        }

        Ok(match kind {
            CompositorKind::Sequence => Particle::Sequence { occurs, items, annotation, pos },
            CompositorKind::Choice => Particle::Choice { occurs, items, annotation, pos },
            CompositorKind::All => Particle::All { occurs, items, annotation, pos },
        })
    }
}

enum ChildEvent {
    Start(Element, Pos),
    End,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CompositorKind {
    Sequence,
    Choice,
    All,
}

// ------------------------------------------------------------------ helpers

fn is_xs_ns(el: &Element) -> bool {
    el.name.ns.as_deref() == Some(XS_NS)
}

fn is_xs(el: &Element, local: &str) -> bool {
    is_xs_ns(el) && el.name.local == local
}

fn display_el_name(el: &Element) -> String {
    match el.name.ns.as_deref() {
        Some(XS_NS) => format!("xs:{}", el.name.local),
        Some(ns) => format!("{{{ns}}}{}", el.name.local),
        None => el.name.local.clone(),
    }
}

fn is_facet_name(local: &str) -> bool {
    matches!(
        local,
        "enumeration"
            | "pattern"
            | "length"
            | "minLength"
            | "maxLength"
            | "minInclusive"
            | "maxInclusive"
            | "minExclusive"
            | "maxExclusive"
            | "totalDigits"
            | "fractionDigits"
            | "whiteSpace"
    )
}

fn attr_raw<'a>(el: &'a Element, local: &str) -> Option<&'a str> {
    el.attrs.iter().find(|a| a.name.ns.is_none() && a.name.local == local).map(|a| a.value.raw())
}

fn require_attr<'a>(el: &'a Element, local: &str, pos: &Pos) -> Result<&'a str, ParseError> {
    attr_raw(el, local).ok_or_else(|| ParseError::Malformed {
        detail: format!("missing_required_attribute_{local}"),
        pos: pos.clone(),
    })
}

fn require_ncname(el: &Element, local: &str, pos: &Pos) -> Result<String, ParseError> {
    let raw = require_attr(el, local, pos)?.trim();
    if raw.is_empty() || raw.contains(':') || raw.chars().any(char::is_whitespace) {
        return Err(ParseError::Malformed { detail: format!("invalid_ncname_{local}"), pos: pos.clone() });
    }
    Ok(raw.to_string())
}

fn check_allowed_attrs(el: &Element, pos: &Pos, allowed: &[&str], allow_xml_lang: bool) -> Result<(), ParseError> {
    for a in &el.attrs {
        if a.name.ns.is_none() {
            if !allowed.contains(&a.name.local.as_str()) {
                return Err(ParseError::UnsupportedConstruct {
                    construct: format!("xs:{}/@{}", el.name.local, a.name.local),
                    pos: pos.clone(),
                });
            }
        } else if allow_xml_lang && a.name.ns.as_deref() == Some(XML_NS) && a.name.local == "lang" {
            continue;
        } else {
            let attr_name = match a.name.ns.as_deref() {
                Some(ns) => format!("{{{ns}}}{}", a.name.local),
                None => a.name.local.clone(),
            };
            return Err(ParseError::UnsupportedConstruct {
                construct: format!("xs:{}/@{attr_name}", el.name.local),
                pos: pos.clone(),
            });
        }
    }
    Ok(())
}

fn parse_form(raw: &str, pos: &Pos) -> Result<FormDefault, ParseError> {
    match raw.trim() {
        "qualified" => Ok(FormDefault::Qualified),
        "unqualified" => Ok(FormDefault::Unqualified),
        _ => Err(ParseError::Malformed { detail: "invalid_form_value".to_string(), pos: pos.clone() }),
    }
}

fn parse_opt_bool(el: &Element, local: &str, pos: &Pos) -> Result<Option<bool>, ParseError> {
    let Some(raw) = attr_raw(el, local) else {
        return Ok(None);
    };
    match raw.trim() {
        "true" | "1" => Ok(Some(true)),
        "false" | "0" => Ok(Some(false)),
        _ => Err(ParseError::Malformed { detail: format!("invalid_bool_{local}"), pos: pos.clone() }),
    }
}

fn parse_u64(raw: &str, pos: &Pos, reason: &str) -> Result<u64, ParseError> {
    raw.trim().parse::<u64>().map_err(|_| ParseError::Malformed { detail: reason.to_string(), pos: pos.clone() })
}

fn parse_occurs(el: &Element, pos: &Pos) -> Result<Occurs, ParseError> {
    let min = match attr_raw(el, "minOccurs") {
        Some(raw) => parse_u64(raw, pos, "invalid_min_occurs")?,
        None => 1,
    };
    let max = match attr_raw(el, "maxOccurs") {
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed == "unbounded" {
                MaxOccurs::Unbounded
            } else {
                MaxOccurs::Bounded(parse_u64(trimmed, pos, "invalid_max_occurs")?)
            }
        }
        None => MaxOccurs::Bounded(1),
    };
    if let MaxOccurs::Bounded(m) = max {
        if min > m {
            return Err(ParseError::Malformed {
                detail: "min_occurs_exceeds_max_occurs".to_string(),
                pos: pos.clone(),
            });
        }
    }
    Ok(Occurs { min, max })
}

fn parse_opt_attribute_use(el: &Element, pos: &Pos) -> Result<Option<AttributeUse>, ParseError> {
    let Some(raw) = attr_raw(el, "use") else {
        return Ok(None);
    };
    match raw.trim() {
        "optional" => Ok(Some(AttributeUse::Optional)),
        "required" => Ok(Some(AttributeUse::Required)),
        "prohibited" => Ok(Some(AttributeUse::Prohibited)),
        _ => Err(ParseError::Malformed { detail: "invalid_attribute_use".to_string(), pos: pos.clone() }),
    }
}

fn resolve_qname(raw: &str, scope: &NsScope, pos: &Pos) -> Result<QName, ParseError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ParseError::Malformed { detail: "empty_qname".to_string(), pos: pos.clone() });
    }
    if let Some((prefix, local)) = trimmed.split_once(':') {
        if prefix.is_empty() || local.is_empty() || local.contains(':') {
            return Err(ParseError::Malformed { detail: "invalid_qname".to_string(), pos: pos.clone() });
        }
        if prefix == "xml" {
            return Ok(QName::new(Some(XML_NS.to_string()), local));
        }
        for binding in scope.iter() {
            if binding.prefix.as_deref() == Some(prefix) {
                return Ok(QName::new(Some(binding.uri.to_string()), local));
            }
        }
        Err(ParseError::UnresolvedRef { kind: RefKind::NamespacePrefix, name: trimmed.to_string(), pos: pos.clone() })
    } else {
        let default_ns =
            scope.iter().find(|b| b.prefix.is_none()).filter(|b| !b.uri.is_empty()).map(|b| b.uri.to_string());
        Ok(QName::new(default_ns, trimmed))
    }
}
