//! The normalised item stream: XML events in, canonical items out. Both the serialiser
//! ([`super::normalise_with`]) and the comparator ([`super::compare_with`]) consume it, so
//! "equal under qif-norm" and "equal normalised bytes" cannot drift apart.
//!
//! Memory is O(depth) plus the content held back for the `drop-ws-text` rule: text and
//! processing instructions of an element are held until its first child element or its
//! end tag shows whether whitespace-only text is dropped. Held bytes are bounded by the
//! allocation limit of the budget.

use super::rules::NormRules;
use super::NormError;
use fid_io_reg::{BoundedSource, IoError, LimitKind, Position};
use fid_xml::{Element, EventKind, XmlReader, XML_NS};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::Read;

/// The XML Schema instance namespace; `xsi:type` values are QNames and are rewritten.
pub(crate) const XSI_NS: &str = "http://www.w3.org/2001/XMLSchema-instance";

/// A canonical attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CAttr {
    /// Namespace name, empty for none (the sort key with `local`).
    pub uri: String,
    /// Local name.
    pub local: String,
    /// Canonical qualified name.
    pub name: String,
    /// Canonical value.
    pub value: String,
}

/// What a canonical item is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ItemKind {
    /// A start tag with the namespace declarations it carries, sorted by prefix.
    Start { name: String, decls: Vec<(String, String)>, attrs: Vec<CAttr> },
    /// An end tag.
    End { name: String },
    /// Character data (never empty).
    Text(String),
    /// A processing instruction.
    Pi { target: String, data: String },
}

/// One canonical item with its location.
#[derive(Debug, Clone)]
pub(crate) struct Item {
    pub kind: ItemKind,
    /// Element path (the element itself for tags, the parent for text and PIs). Empty
    /// when paths are not tracked.
    pub path: String,
    /// 1-based line in the input where the item starts.
    pub line: Option<u32>,
}

struct Frame {
    name: String,
    path: String,
    simple: bool,
    has_child: bool,
    pending: Vec<Item>,
    children: BTreeMap<String, u32>,
    declared: Vec<String>,
}

pub(crate) struct Stream<'a, R> {
    reader: XmlReader<R>,
    rules: &'a NormRules,
    generated: BTreeMap<String, String>,
    next_gen: u32,
    stack: Vec<Frame>,
    queue: VecDeque<Item>,
    text: String,
    text_line: Option<u32>,
    pending_bytes: u64,
    cap: u64,
    paths: bool,
    done: bool,
}

impl<'a, R: Read> Stream<'a, R> {
    pub(crate) fn new(source: BoundedSource<R>, rules: &'a NormRules, paths: bool) -> Self {
        let cap = source.budget().limits().alloc();
        Stream {
            reader: XmlReader::new(source),
            rules,
            generated: BTreeMap::new(),
            next_gen: 0,
            stack: Vec::new(),
            queue: VecDeque::new(),
            text: String::new(),
            text_line: None,
            pending_bytes: 0,
            cap,
            paths,
            done: false,
        }
    }

    /// The allocation limit that also bounds held-back content and the output.
    pub(crate) fn cap(&self) -> u64 {
        self.cap
    }

    /// The next canonical item, or `None` after the document ends.
    pub(crate) fn next_item(&mut self) -> Result<Option<Item>, NormError> {
        loop {
            if let Some(item) = self.queue.pop_front() {
                return Ok(Some(item));
            }
            if self.done {
                return Ok(None);
            }
            let Some(ev) = self.reader.next_event()? else {
                self.flush_text();
                self.done = true;
                continue;
            };
            let line = ev.pos.line;
            match ev.kind {
                EventKind::Start(el) => self.start(&el, line),
                EventKind::End { .. } => self.end(line),
                EventKind::Text(t) | EventKind::CData(t) => self.add_text(t.raw(), line, ev.pos)?,
                EventKind::Comment(_) => {}
                EventKind::Pi { target, data } => self.pi(target, data.raw(), line, ev.pos)?,
            }
        }
    }

    fn check_held(&self, at: Position) -> Result<(), NormError> {
        if self.pending_bytes.saturating_add(self.text.len() as u64) > self.cap {
            return Err(NormError::Harness(IoError::LimitExceeded { limit: LimitKind::Alloc, value: self.cap, at }));
        }
        Ok(())
    }

    fn add_text(&mut self, text: &str, line: Option<u32>, at: Position) -> Result<(), NormError> {
        if text.is_empty() {
            return Ok(());
        }
        if self.text.is_empty() {
            self.text_line = line;
        }
        self.text.push_str(text);
        self.check_held(at)
    }

    /// Ends the current text run (text merged across comments and CDATA sections).
    fn flush_text(&mut self) {
        if self.text.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.text);
        let line = self.text_line.take();
        // Outside the root element only whitespace can occur; it is not part of the
        // canonical form.
        let Some(top) = self.stack.last_mut() else { return };
        if top.has_child {
            if let Some(t) = finish_text(text, true, top.simple) {
                self.queue.push_back(Item { kind: ItemKind::Text(t), path: top.path.clone(), line });
            }
        } else {
            self.pending_bytes += text.len() as u64;
            top.pending.push(Item { kind: ItemKind::Text(text), path: top.path.clone(), line });
        }
    }

    /// Releases what the parent held back once it is known to have a child element.
    fn parent_gets_child(&mut self) {
        let Some(parent) = self.stack.last_mut() else { return };
        if parent.has_child {
            return;
        }
        parent.has_child = true;
        let simple = parent.simple;
        for Item { kind, path, line } in std::mem::take(&mut parent.pending) {
            self.pending_bytes = self.pending_bytes.saturating_sub(held_len(&kind));
            let kind = match kind {
                ItemKind::Text(t) => match finish_text(t, true, simple) {
                    Some(t) => ItemKind::Text(t),
                    None => continue,
                },
                other => other,
            };
            self.queue.push_back(Item { kind, path, line });
        }
    }

    fn prefix_for(&mut self, uri: &str) -> String {
        if uri == XML_NS {
            return "xml".to_string();
        }
        if let Some(p) = self.rules.fixed_prefix(uri) {
            return p.to_string();
        }
        if let Some(p) = self.generated.get(uri) {
            return p.clone();
        }
        self.next_gen += 1;
        let p = format!("n{}", self.next_gen);
        self.generated.insert(uri.to_string(), p.clone());
        p
    }

    fn start(&mut self, el: &Element, line: Option<u32>) {
        self.flush_text();
        self.parent_gets_child();

        // Namespaces this tag uses, in sorted order, so that generated prefixes are
        // numbered independently of attribute order and of the prefixes in the input.
        let elem_uri = el.name.ns.as_deref().unwrap_or("").to_string();
        let mut xsi_types: Vec<Option<(String, String)>> = Vec::with_capacity(el.attrs.len());
        let mut used: BTreeSet<String> = BTreeSet::new();
        if !elem_uri.is_empty() {
            used.insert(elem_uri.clone());
        }
        for a in &el.attrs {
            let uri = a.name.ns.as_deref().unwrap_or("");
            if !uri.is_empty() {
                used.insert(uri.to_string());
            }
            let resolved =
                (uri == XSI_NS && a.name.local == "type").then(|| resolve_qname(el, a.value.raw())).flatten();
            if let Some((u, _)) = &resolved {
                if !u.is_empty() {
                    used.insert(u.clone());
                }
            }
            xsi_types.push(resolved);
        }
        let mut prefixes: BTreeMap<String, String> = BTreeMap::new();
        for uri in &used {
            let p = self.prefix_for(uri);
            prefixes.insert(uri.clone(), p);
        }
        let qualify = |uri: &str, local: &str| -> String {
            match prefixes.get(uri) {
                Some(p) if !uri.is_empty() => format!("{p}:{local}"),
                _ => local.to_string(),
            }
        };

        let name = qualify(&elem_uri, &el.name.local);
        let mut attrs: Vec<CAttr> = Vec::with_capacity(el.attrs.len());
        for (a, resolved) in el.attrs.iter().zip(xsi_types) {
            let uri = a.name.ns.as_deref().unwrap_or("").to_string();
            let value = match resolved {
                Some((u, local)) => qualify(&u, &local),
                None if uri == XSI_NS && a.name.local == "type" => collapse(a.value.raw()),
                None if self.rules.is_simple_attribute(&uri, &a.name.local) => collapse(a.value.raw()),
                None => a.value.raw().to_string(),
            };
            let name = qualify(&uri, &a.name.local);
            attrs.push(CAttr { uri, local: a.name.local.clone(), name, value });
        }
        attrs.sort_by(|x, y| (&x.uri, &x.local).cmp(&(&y.uri, &y.local)));

        let mut declared = Vec::new();
        let mut decls = Vec::new();
        for uri in used {
            if uri == XML_NS || self.stack.iter().any(|f| f.declared.contains(&uri)) {
                continue;
            }
            if let Some(p) = prefixes.get(&uri) {
                decls.push((p.clone(), uri.clone()));
            }
            declared.push(uri);
        }
        decls.sort();

        let path = if self.paths {
            let n = match self.stack.last_mut() {
                Some(parent) => {
                    let n = parent.children.entry(name.clone()).or_insert(0);
                    *n += 1;
                    *n
                }
                None => 1,
            };
            let parent = self.stack.last().map_or("", |f| f.path.as_str());
            format!("{parent}/{name}[{n}]")
        } else {
            String::new()
        };
        let simple = self.rules.is_simple_element(&elem_uri, &el.name.local);
        self.queue.push_back(Item {
            kind: ItemKind::Start { name: name.clone(), decls, attrs },
            path: path.clone(),
            line,
        });
        self.stack.push(Frame {
            name,
            path,
            simple,
            has_child: false,
            pending: Vec::new(),
            children: BTreeMap::new(),
            declared,
        });
    }

    fn end(&mut self, line: Option<u32>) {
        self.flush_text();
        let Some(frame) = self.stack.pop() else { return };
        for Item { kind, path, line } in frame.pending {
            self.pending_bytes = self.pending_bytes.saturating_sub(held_len(&kind));
            let kind = match kind {
                ItemKind::Text(t) => match finish_text(t, false, frame.simple) {
                    Some(t) => ItemKind::Text(t),
                    None => continue,
                },
                other => other,
            };
            self.queue.push_back(Item { kind, path, line });
        }
        self.queue.push_back(Item { kind: ItemKind::End { name: frame.name }, path: frame.path, line });
    }

    fn pi(&mut self, target: String, data: &str, line: Option<u32>, at: Position) -> Result<(), NormError> {
        self.flush_text();
        let kind = ItemKind::Pi { target, data: data.to_string() };
        match self.stack.last_mut() {
            None => self.queue.push_back(Item { kind, path: String::new(), line }),
            Some(top) if top.has_child => self.queue.push_back(Item { kind, path: top.path.clone(), line }),
            Some(top) => {
                self.pending_bytes += held_len(&kind);
                top.pending.push(Item { kind, path: top.path.clone(), line });
                self.check_held(at)?;
            }
        }
        Ok(())
    }
}

fn held_len(kind: &ItemKind) -> u64 {
    match kind {
        ItemKind::Text(t) => t.len() as u64,
        ItemKind::Pi { target, data } => (target.len() + data.len()) as u64,
        _ => 0,
    }
}

/// Applies `drop-ws-text` (when the parent has child elements) and `xsd-collapse` (when
/// the parent is simple). `None` when nothing is left.
fn finish_text(text: String, has_child: bool, simple: bool) -> Option<String> {
    if has_child && is_ws_only(&text) {
        return None;
    }
    let text = if simple { collapse(&text) } else { text };
    (!text.is_empty()).then_some(text)
}

fn is_xml_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

/// True if `s` holds only XML whitespace (space, tab, LF, CR); true for empty text.
pub(crate) fn is_ws_only(s: &str) -> bool {
    s.chars().all(is_xml_ws)
}

/// XSD `collapse`: tab, LF and CR become spaces, runs of spaces become one, and leading
/// and trailing spaces are removed.
pub(crate) fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for part in s.split(is_xml_ws).filter(|p| !p.is_empty()) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(part);
    }
    out
}

/// Resolves an `xsi:type` QName against the element's in-scope namespaces. Returns
/// `(namespace name or "", local)`, or `None` when the value is not a QName or its prefix
/// is not bound (the value is then only collapsed).
fn resolve_qname(el: &Element, value: &str) -> Option<(String, String)> {
    let v = collapse(value);
    let (prefix, local) = match v.split_once(':') {
        Some((p, l)) => (Some(p), l),
        None => (None, v.as_str()),
    };
    if local.is_empty() || local.contains(':') || prefix == Some("") || v.contains(' ') {
        return None;
    }
    if prefix == Some("xml") {
        return Some((XML_NS.to_string(), local.to_string()));
    }
    match el.ns_scope.iter().find(|b| b.prefix.as_deref() == prefix) {
        Some(b) => Some((b.uri.to_string(), local.to_string())),
        None if prefix.is_none() => Some((String::new(), local.to_string())),
        None => None,
    }
}

/// Writes canonical items as bytes (the `c14n11` serialisation).
#[derive(Default)]
pub(crate) struct Writer {
    pub out: Vec<u8>,
    depth: usize,
    root_done: bool,
}

impl Writer {
    pub(crate) fn write(&mut self, kind: &ItemKind) {
        match kind {
            ItemKind::Start { .. } => {
                render_start(&mut self.out, kind, true);
                self.depth += 1;
            }
            ItemKind::End { name } => {
                self.out.extend_from_slice(b"</");
                self.out.extend_from_slice(name.as_bytes());
                self.out.push(b'>');
                self.depth = self.depth.saturating_sub(1);
                if self.depth == 0 {
                    self.root_done = true;
                }
            }
            ItemKind::Text(t) => escape_text(&mut self.out, t),
            ItemKind::Pi { .. } => {
                if self.depth == 0 && self.root_done {
                    self.out.push(b'\n');
                }
                render_pi(&mut self.out, kind);
                if self.depth == 0 && !self.root_done {
                    self.out.push(b'\n');
                }
            }
        }
    }
}

/// `<name xmlns:p="uri" ... a="v" ...>`; declarations only when `decls` is set.
pub(crate) fn render_start(out: &mut Vec<u8>, kind: &ItemKind, with_decls: bool) {
    let ItemKind::Start { name, decls, attrs } = kind else { return };
    out.push(b'<');
    out.extend_from_slice(name.as_bytes());
    if with_decls {
        for (p, uri) in decls {
            out.extend_from_slice(b" xmlns:");
            out.extend_from_slice(p.as_bytes());
            out.extend_from_slice(b"=\"");
            escape_attr(out, uri);
            out.push(b'"');
        }
    }
    for a in attrs {
        out.push(b' ');
        out.extend_from_slice(a.name.as_bytes());
        out.extend_from_slice(b"=\"");
        escape_attr(out, &a.value);
        out.push(b'"');
    }
    out.push(b'>');
}

pub(crate) fn render_pi(out: &mut Vec<u8>, kind: &ItemKind) {
    let ItemKind::Pi { target, data } = kind else { return };
    out.extend_from_slice(b"<?");
    out.extend_from_slice(target.as_bytes());
    if !data.is_empty() {
        out.push(b' ');
        out.extend_from_slice(data.as_bytes());
    }
    out.extend_from_slice(b"?>");
}

fn escape_text(out: &mut Vec<u8>, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.extend_from_slice(b"&amp;"),
            '<' => out.extend_from_slice(b"&lt;"),
            '>' => out.extend_from_slice(b"&gt;"),
            '\r' => out.extend_from_slice(b"&#xD;"),
            _ => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
}

fn escape_attr(out: &mut Vec<u8>, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.extend_from_slice(b"&amp;"),
            '<' => out.extend_from_slice(b"&lt;"),
            '"' => out.extend_from_slice(b"&quot;"),
            '\t' => out.extend_from_slice(b"&#x9;"),
            '\n' => out.extend_from_slice(b"&#xA;"),
            '\r' => out.extend_from_slice(b"&#xD;"),
            _ => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
}
