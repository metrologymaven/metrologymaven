//! The reader: XML events to the element tree, then the scan descriptions.

use std::collections::BTreeSet;
use std::io::{self, Read, Seek};

use fid_io_reg::{BoundedSource, Budget, Position};
use fid_xml::{Attr, Element, EventKind, XmlOptions, XmlReader, XmlText};

use super::lex::{
    check_leaf, is_ncname, is_string_char, is_ws, parse_bool01, parse_f64, parse_i64, parse_precision, parse_u64,
    trim_ws,
};
use super::scan::{describe, ScanDesc};
use super::tree::{Bound, E57Node, E57Tree, Key, Namespace, NodeType, XmlSectionError, XmlWarning};
use super::{E57_NAMESPACE, MAX_DEPTH, MAX_ELEMENTS, MAX_TEXT_LEN};
use crate::{LogicalReader, PhysError};

/// The bytes charged to the allocation budget for every element kept in the tree: two
/// `(Key, E57Node)` slots (the slot and its share of `Vec` doubling) plus a `Key` and a
/// B-tree entry for the duplicate-key set, before the element's own strings.
pub const RETAINED_NODE_COST: u64 =
    (2 * std::mem::size_of::<(Key, E57Node)>() + std::mem::size_of::<Key>() + 64) as u64;

/// The reader's bounds. They can only be lowered from [`MAX_DEPTH`], [`MAX_ELEMENTS`] and
/// [`MAX_TEXT_LEN`] (the defaults).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XmlLimits {
    max_depth: u32,
    max_elements: u64,
    max_text_len: usize,
}

impl Default for XmlLimits {
    fn default() -> Self {
        XmlLimits { max_depth: MAX_DEPTH, max_elements: MAX_ELEMENTS, max_text_len: MAX_TEXT_LEN }
    }
}

impl XmlLimits {
    /// Lowers the depth bound.
    pub fn with_max_depth(mut self, v: u32) -> Self {
        self.max_depth = v.min(MAX_DEPTH);
        self
    }
    /// Lowers the element bound.
    pub fn with_max_elements(mut self, v: u64) -> Self {
        self.max_elements = v.min(MAX_ELEMENTS);
        self
    }
    /// Lowers the text bound.
    pub fn with_max_text_len(mut self, v: usize) -> Self {
        self.max_text_len = v.min(MAX_TEXT_LEN);
        self
    }
    /// The depth bound.
    pub fn max_depth(&self) -> u32 {
        self.max_depth
    }
    /// The element bound.
    pub fn max_elements(&self) -> u64 {
        self.max_elements
    }
    /// The text bound.
    pub fn max_text_len(&self) -> usize {
        self.max_text_len
    }
}

/// A parsed XML section. The fields are private: they hold what the reader validated.
#[derive(Debug, Clone, PartialEq)]
pub struct E57Xml {
    tree: E57Tree,
    scans: Vec<ScanDesc>,
    images2d: Option<u64>,
    elements: u64,
    extension_elements: u64,
    warnings: Vec<XmlWarning>,
}

impl E57Xml {
    /// The element tree.
    pub fn tree(&self) -> &E57Tree {
        &self.tree
    }
    /// One description per `data3D` child, in document order (REQ-001).
    pub fn scans(&self) -> &[ScanDesc] {
        &self.scans
    }
    /// The number of `images2D` children (not imported), or `None` if the root has no
    /// `images2D`.
    pub fn images2d(&self) -> Option<u64> {
        self.images2d
    }
    /// Elements read, the root included.
    pub fn element_count(&self) -> u64 {
        self.elements
    }
    /// Elements in an extension namespace (kept in the tree as untrusted metadata).
    pub fn extension_elements(&self) -> u64 {
        self.extension_elements
    }
    /// What the caller must decide on (REQ-004).
    pub fn warnings(&self) -> &[XmlWarning] {
        &self.warnings
    }
    /// The tree, giving up the rest.
    pub fn into_tree(self) -> E57Tree {
        self.tree
    }
}

fn bom(head: &[u8]) -> bool {
    head.starts_with(&[0xEF, 0xBB, 0xBF]) || head.starts_with(&[0xFE, 0xFF]) || head.starts_with(&[0xFF, 0xFE])
}

/// Parses an XML section held in memory, with the default bounds.
pub fn parse_slice(data: &[u8], budget: Budget) -> Result<E57Xml, XmlSectionError> {
    parse_slice_with(data, budget, XmlLimits::default())
}

/// Parses an XML section held in memory with lowered bounds. Positions are offsets in
/// `data`.
pub fn parse_slice_with(data: &[u8], budget: Budget, limits: XmlLimits) -> Result<E57Xml, XmlSectionError> {
    if bom(data) {
        return Err(XmlSectionError::malformed("xml_bom", Position::byte(0)));
    }
    parse_source(BoundedSource::new(data, budget), limits)
}

/// The XML section of an opened file as a byte stream; a physical-layer failure is kept
/// for the caller.
struct SectionRead<'a, R> {
    lr: &'a mut LogicalReader<R>,
    pos: u64,
    end: u64,
    err: Option<PhysError>,
}

impl<R: Read + Seek> Read for SectionRead<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.end.saturating_sub(self.pos);
        let n = usize::try_from(left).map_or(buf.len(), |l| l.min(buf.len()));
        let Some(dst) = buf.get_mut(..n) else { return Ok(0) };
        if n == 0 {
            return Ok(0);
        }
        match self.lr.read_at(self.pos, dst) {
            Ok(()) => {
                self.pos += n as u64;
                Ok(n)
            }
            Err(e) => {
                self.err = Some(e);
                Err(io::Error::other("e57 physical layer"))
            }
        }
    }
}

/// Reads and parses the XML section of an opened E57 file, streaming it page by page
/// through `reader` (each page's checksum verified), with the default bounds. Positions are
/// offsets in the section. A failing page is [`XmlSectionError::Phys`].
pub fn read_section<R: Read + Seek>(reader: &mut LogicalReader<R>, budget: Budget) -> Result<E57Xml, XmlSectionError> {
    let start = reader.xml_logical_offset();
    let len = reader.header().xml_length;
    let end = start.checked_add(len).ok_or(XmlSectionError::Phys(PhysError::TruncatedFile { at: start }))?;
    let mut head = [0u8; 3];
    let n = usize::try_from(len).map_or(3, |l| l.min(3));
    if let Some(h) = head.get_mut(..n) {
        reader.read_at(start, h).map_err(XmlSectionError::Phys)?;
    }
    if bom(&head) {
        return Err(XmlSectionError::malformed("xml_bom", Position::byte(0)));
    }
    let mut src = SectionRead { lr: reader, pos: start, end, err: None };
    let result = parse_source(BoundedSource::new(&mut src, budget), XmlLimits::default());
    match src.err.take() {
        Some(e) => Err(XmlSectionError::Phys(e)),
        None => result,
    }
}

fn parse_source<R: Read>(src: BoundedSource<R>, limits: XmlLimits) -> Result<E57Xml, XmlSectionError> {
    let opts = XmlOptions::default().with_max_depth(limits.max_depth).with_max_text_len(limits.max_text_len);
    let mut xr = XmlReader::with_options(src, opts);
    let mut st = Builder::new(limits);
    while let Some(ev) = xr.next_event().map_err(XmlSectionError::from_xml)? {
        let pos = ev.pos;
        st.event(ev.kind, pos)?;
        let b = xr.budget();
        if b.alloc_used().saturating_add(st.retained) > b.limits().alloc() {
            return Err(XmlSectionError::LimitExceeded { bound: Bound::Alloc, value: b.limits().alloc(), at: pos });
        }
    }
    let Some(tree) = st.done else {
        return Err(XmlSectionError::malformed("no_root", Position::byte(0)));
    };
    let (scans, images2d, warnings) = describe(&tree)?;
    Ok(E57Xml { tree, scans, images2d, elements: st.elements, extension_elements: st.extension_elements, warnings })
}

/// Attributes of one element, parsed.
#[derive(Default)]
struct Attrs {
    min_i: Option<i64>,
    max_i: Option<i64>,
    min_f: Option<f64>,
    max_f: Option<f64>,
    scale: Option<f64>,
    offset: Option<f64>,
    precision: Option<super::Precision>,
    file_offset: Option<u64>,
    count: Option<u64>,
    allow: Option<bool>,
}

enum Body {
    Structure { children: Vec<(Key, E57Node)>, seen: BTreeSet<Key> },
    Vector { allow: Option<bool>, children: Vec<E57Node> },
    Cv { file_offset: u64, record_count: u64, prototype: Option<Box<E57Node>>, codecs: Option<Box<E57Node>> },
    Blob { file_offset: u64, length: u64 },
    Leaf { ty: NodeType, attrs: Attrs, text: String },
}

struct Frame {
    key: Key,
    body: Body,
    pos: Position,
}

struct Builder {
    limits: XmlLimits,
    stack: Vec<Frame>,
    namespaces: Vec<Namespace>,
    prefixes: BTreeSet<String>,
    elements: u64,
    extension_elements: u64,
    retained: u64,
    done: Option<E57Tree>,
}

fn bad(reason: &'static str, at: Position) -> XmlSectionError {
    XmlSectionError::malformed(reason, at)
}

impl Builder {
    fn new(limits: XmlLimits) -> Builder {
        Builder {
            limits,
            stack: Vec::new(),
            namespaces: Vec::new(),
            prefixes: BTreeSet::new(),
            elements: 0,
            extension_elements: 0,
            retained: 0,
            done: None,
        }
    }

    fn event(&mut self, kind: EventKind, pos: Position) -> Result<(), XmlSectionError> {
        match kind {
            EventKind::Start(el) => self.start(&el, pos),
            EventKind::End { .. } => self.end(pos),
            EventKind::Text(t) | EventKind::CData(t) => self.text(&t, pos),
            EventKind::Comment(_) | EventKind::Pi { .. } => Ok(()),
        }
    }

    fn start(&mut self, el: &Element, pos: Position) -> Result<(), XmlSectionError> {
        self.elements += 1;
        if self.elements > self.limits.max_elements {
            return Err(XmlSectionError::LimitExceeded {
                bound: Bound::Elements,
                value: self.limits.max_elements,
                at: pos,
            });
        }
        if self.stack.is_empty() {
            return self.root(el, pos);
        }
        if !el.ns_declared.is_empty() {
            return Err(bad("namespace_declaration_not_on_root", pos));
        }
        // The XML reader accepts a QName whose local part is a Name; E57 keys are NCNames.
        if !is_ncname(&el.name.local) {
            return Err(bad("element_name", pos));
        }
        let key = match &el.name.prefix {
            None if el.name.ns.as_deref() == Some(E57_NAMESPACE) => Key::new(&el.name.local),
            Some(p) if self.prefixes.contains(p) => {
                self.extension_elements += 1;
                Key::prefixed(p, &el.name.local)
            }
            _ => return Err(bad("element_namespace", pos)),
        };
        let Some(parent) = self.stack.last_mut() else { return Err(bad("no_root", pos)) };
        match &mut parent.body {
            Body::Structure { seen, .. } => {
                if !seen.insert(key.clone()) {
                    return Err(bad("duplicate_key", pos));
                }
                self.retained = self.retained.saturating_add(key.heap_bytes());
            }
            Body::Vector { .. } if key.is("vectorChild") => {}
            Body::Vector { .. } => return Err(bad("vector_child_name", pos)),
            Body::Cv { .. } if key.is("prototype") || key.is("codecs") => {}
            Body::Cv { .. } => return Err(bad("compressed_vector_child", pos)),
            Body::Blob { .. } | Body::Leaf { .. } => return Err(bad("unexpected_child", pos)),
        }
        let ty = element_type(el, pos)?;
        let attrs = parse_attrs(&el.attrs, ty, pos)?;
        let body = match ty {
            NodeType::Structure => Body::Structure { children: Vec::new(), seen: BTreeSet::new() },
            NodeType::Vector => Body::Vector { allow: attrs.allow, children: Vec::new() },
            NodeType::CompressedVector => Body::Cv {
                file_offset: attrs.file_offset.ok_or(bad("attribute_missing", pos))?,
                record_count: attrs.count.ok_or(bad("attribute_missing", pos))?,
                prototype: None,
                codecs: None,
            },
            NodeType::Blob => Body::Blob {
                file_offset: attrs.file_offset.ok_or(bad("attribute_missing", pos))?,
                length: attrs.count.ok_or(bad("attribute_missing", pos))?,
            },
            _ => Body::Leaf { ty, attrs, text: String::new() },
        };
        self.stack.push(Frame { key, body, pos });
        Ok(())
    }

    fn root(&mut self, el: &Element, pos: Position) -> Result<(), XmlSectionError> {
        if el.name.local != "e57Root" || el.name.ns.as_deref() != Some(E57_NAMESPACE) {
            return Err(bad("not_e57_root", pos));
        }
        if el.name.prefix.is_some() {
            return Err(bad("e57_namespace_prefixed", pos));
        }
        if element_type(el, pos)? != NodeType::Structure {
            return Err(bad("root_not_structure", pos));
        }
        parse_attrs(&el.attrs, NodeType::Structure, pos)?;
        let mut uris: BTreeSet<&str> = BTreeSet::new();
        for b in &el.ns_declared {
            match &b.prefix {
                None if &*b.uri == E57_NAMESPACE => {}
                None => return Err(bad("default_namespace_not_e57", pos)),
                Some(_) if &*b.uri == E57_NAMESPACE => return Err(bad("e57_namespace_prefixed", pos)),
                Some(p) if p == "xml" => return Err(bad("namespace_reserved", pos)),
                Some(p) if !is_ncname(p) => return Err(bad("namespace_prefix", pos)),
                Some(_) if !b.uri.chars().all(is_string_char) => return Err(bad("control_character", pos)),
                Some(p) => {
                    if !uris.insert(&*b.uri) {
                        return Err(bad("namespace_uri_duplicate", pos));
                    }
                    self.prefixes.insert(p.clone());
                    self.namespaces.push(Namespace { prefix: p.clone(), uri: XmlText::new(&b.uri) });
                }
            }
        }
        let body = Body::Structure { children: Vec::new(), seen: BTreeSet::new() };
        self.stack.push(Frame { key: Key::new("e57Root"), body, pos });
        Ok(())
    }

    fn text(&mut self, t: &XmlText, pos: Position) -> Result<(), XmlSectionError> {
        let max = self.limits.max_text_len;
        match self.stack.last_mut() {
            Some(Frame { body: Body::Leaf { text, .. }, .. }) => {
                if text.len().saturating_add(t.len()) > max {
                    return Err(XmlSectionError::LimitExceeded {
                        bound: Bound::TextLength,
                        value: max as u64,
                        at: pos,
                    });
                }
                text.push_str(t.raw());
                Ok(())
            }
            _ if t.raw().chars().all(is_ws) => Ok(()),
            _ => Err(bad("unexpected_text", pos)),
        }
    }

    fn end(&mut self, pos: Position) -> Result<(), XmlSectionError> {
        let Some(frame) = self.stack.pop() else { return Err(bad("no_root", pos)) };
        let at = frame.pos;
        let node = finish(frame.body, at)?;
        check_leaf(&node).map_err(|r| bad(r, at))?;
        let heap = match &node {
            E57Node::String(t) => t.len() as u64,
            _ => 0,
        };
        self.retained = self.retained.saturating_add(RETAINED_NODE_COST + frame.key.heap_bytes() + heap);
        let Some(parent) = self.stack.last_mut() else {
            let E57Node::Structure(root) = node else { return Err(bad("root_not_structure", at)) };
            self.done = Some(E57Tree { namespaces: std::mem::take(&mut self.namespaces), root });
            return Ok(());
        };
        match &mut parent.body {
            Body::Structure { children, .. } => children.push((frame.key, node)),
            Body::Vector { allow, children } => {
                let same = children.first().is_none_or(|c| c.node_type() == node.node_type());
                if *allow != Some(true) && !same {
                    return Err(bad("vector_heterogeneous", at));
                }
                children.push(node);
            }
            Body::Cv { prototype, codecs, .. } => {
                let slot = if frame.key.is("prototype") { prototype } else { codecs };
                if slot.is_some() {
                    return Err(bad("compressed_vector_child", at));
                }
                if frame.key.is("codecs") && node.node_type() != NodeType::Vector {
                    return Err(bad("codecs_not_vector", at));
                }
                *slot = Some(Box::new(node));
            }
            Body::Blob { .. } | Body::Leaf { .. } => return Err(bad("unexpected_child", at)),
        }
        Ok(())
    }
}

fn element_type(el: &Element, pos: Position) -> Result<NodeType, XmlSectionError> {
    let attr = el.attrs.iter().find(|a| a.name.prefix.is_none() && a.name.local == "type");
    let Some(a) = attr else { return Err(bad("type_missing", pos)) };
    NodeType::parse(a.value.raw()).ok_or(bad("type_unknown", pos))
}

fn parse_attrs(attrs: &[Attr], ty: NodeType, pos: Position) -> Result<Attrs, XmlSectionError> {
    let mut out = Attrs::default();
    for a in attrs {
        let name = a.name.local.as_str();
        if a.name.prefix.is_some() || a.name.ns.is_some() {
            return Err(bad("attribute_not_applicable", pos));
        }
        if name == "type" {
            continue;
        }
        if !ty.attributes().contains(&name) {
            return Err(bad("attribute_not_applicable", pos));
        }
        let v = trim_ws(a.value.raw());
        let int = matches!(ty, NodeType::Integer | NodeType::ScaledInteger);
        let e = |r| bad(r, pos);
        match name {
            "minimum" if int => out.min_i = Some(parse_i64(v).map_err(e)?),
            "maximum" if int => out.max_i = Some(parse_i64(v).map_err(e)?),
            "minimum" => out.min_f = Some(parse_f64(v).map_err(e)?),
            "maximum" => out.max_f = Some(parse_f64(v).map_err(e)?),
            "scale" => out.scale = Some(parse_f64(v).map_err(e)?),
            "offset" => out.offset = Some(parse_f64(v).map_err(e)?),
            "precision" => out.precision = Some(parse_precision(v).map_err(e)?),
            "fileOffset" => out.file_offset = Some(parse_u64(v).map_err(e)?),
            "recordCount" | "length" => out.count = Some(parse_u64(v).map_err(e)?),
            "allowHeterogeneousChildren" => out.allow = Some(parse_bool01(v).map_err(e)?),
            _ => return Err(bad("attribute_not_applicable", pos)),
        }
    }
    Ok(out)
}

fn opt<T>(s: &str, f: fn(&str) -> Result<T, &'static str>, at: Position) -> Result<Option<T>, XmlSectionError> {
    let s = trim_ws(s);
    if s.is_empty() {
        Ok(None)
    } else {
        f(s).map(Some).map_err(|r| bad(r, at))
    }
}

fn finish(body: Body, at: Position) -> Result<E57Node, XmlSectionError> {
    Ok(match body {
        Body::Structure { children, .. } => E57Node::Structure(children),
        Body::Vector { allow, children } => E57Node::Vector { allow_heterogeneous: allow, children },
        Body::Cv { file_offset, record_count, prototype, codecs } => E57Node::CompressedVector {
            file_offset,
            record_count,
            prototype: prototype.ok_or(bad("prototype_missing", at))?,
            codecs,
        },
        Body::Blob { file_offset, length } => E57Node::Blob { file_offset, length },
        Body::Leaf { ty, attrs: a, text } => match ty {
            NodeType::Integer => {
                E57Node::Integer { value: opt(&text, parse_i64, at)?, minimum: a.min_i, maximum: a.max_i }
            }
            NodeType::ScaledInteger => E57Node::ScaledInteger {
                raw: opt(&text, parse_i64, at)?,
                minimum: a.min_i,
                maximum: a.max_i,
                scale: a.scale,
                offset: a.offset,
            },
            NodeType::Float => E57Node::Float {
                value: opt(&text, parse_f64, at)?,
                precision: a.precision,
                minimum: a.min_f,
                maximum: a.max_f,
            },
            NodeType::String => E57Node::String(XmlText::new(&text)),
            NodeType::Structure | NodeType::Vector | NodeType::CompressedVector | NodeType::Blob => {
                return Err(bad("type_unknown", at));
            }
        },
    })
}
