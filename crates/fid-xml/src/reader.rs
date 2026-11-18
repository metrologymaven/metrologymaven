//! The pull reader (REQ-IO.QIF.XML.PARSE-001 .. -006).

use crate::chars::{find, is_ws, scan_name, skip_ws, split_qname, NameErr};
use crate::error::{Construct, XmlError, XmlLimit};
use crate::input::{Encoding, Input, CHUNK};
use crate::text::{decode, DecodeErr, Mode, XmlText};
use fid_io_reg::{BoundedSource, Budget, Position};
use std::collections::BTreeSet;
use std::io::Read;
use std::ops::Range;
use std::sync::Arc;

/// The namespace bound to the `xml` prefix.
pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
/// The namespace of `xmlns` attributes; it may not be declared.
pub const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";
/// The XInclude namespace; elements in it are refused.
pub const XINCLUDE_NS: &str = "http://www.w3.org/2001/XInclude";

/// Element nesting depth limit (REQ-IO.QIF.XML-007).
pub const MAX_DEPTH: u32 = 64;
/// Attributes per element limit, namespace declarations included (REQ-IO.QIF.XML-007).
pub const MAX_ATTRIBUTES: usize = 256;
/// Name length limit in bytes.
pub const MAX_NAME_LEN: usize = 1024;
/// Text node length limit in bytes (64 MiB) [unverified: tune on corpus]. It also bounds
/// CDATA sections, comments, PI data and every single markup token.
pub const MAX_TEXT_LEN: usize = 64 * 1024 * 1024;
/// The XML declaration may not be longer than this.
const MAX_DECL_LEN: usize = 1024;

/// Markup declarations after `<!`: comment, CDATA, and the refused DTD constructs.
const BANG: &[(&[u8], Option<Construct>)] = &[
    (b"<!--", None),
    (b"<![CDATA[", None),
    (b"<!DOCTYPE", Some(Construct::Doctype)),
    (b"<!ENTITY", Some(Construct::EntityDeclaration)),
    (b"<!ELEMENT", Some(Construct::MarkupDeclaration)),
    (b"<!ATTLIST", Some(Construct::MarkupDeclaration)),
    (b"<!NOTATION", Some(Construct::MarkupDeclaration)),
];

/// One in-scope namespace binding. `prefix: None` is the default namespace.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NsBinding {
    /// The prefix, or `None` for the default namespace.
    pub prefix: Option<String>,
    /// The namespace name. Empty only in [`Element::ns_declared`], where it records an
    /// undeclaration of the default namespace (`xmlns=""`).
    pub uri: String,
}

/// The namespace bindings in scope at an element, sorted by prefix (default first),
/// without the implicit `xml` binding. Shared between elements that declare nothing.
pub type NsScope = Arc<[NsBinding]>;

/// A namespace-resolved name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QName {
    /// The prefix as written.
    pub prefix: Option<String>,
    /// The local part.
    pub local: String,
    /// The namespace name, if the name is in a namespace.
    pub ns: Option<String>,
}

/// One attribute (namespace declarations are not attributes; see
/// [`Element::ns_declared`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr {
    /// The name. Unprefixed attributes are in no namespace.
    pub name: QName,
    /// The normalised value.
    pub value: XmlText,
}

/// A start tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    /// The element name.
    pub name: QName,
    /// The attributes in document order.
    pub attrs: Vec<Attr>,
    /// The namespace declarations on this tag, in document order.
    pub ns_declared: Vec<NsBinding>,
    /// Every namespace binding in scope at this element (REQ-IO.QIF.XML.PARSE-006).
    pub ns_scope: NsScope,
    /// True for an empty-element tag (`<a/>`); an [`EventKind::End`] follows at once.
    pub empty: bool,
}

/// What an event is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    /// A start tag or empty-element tag.
    Start(Element),
    /// The end of an element. `element` is the byte span of the whole element, start tag
    /// to end tag, so a caller can copy the subtree verbatim (REQ-IO.QIF.XML.PARSE-006).
    End {
        /// The element name.
        name: QName,
        /// Source byte span of the element.
        element: Range<u64>,
    },
    /// Character data with references expanded and line ends normalised.
    Text(XmlText),
    /// The content of a CDATA section.
    CData(XmlText),
    /// The content of a comment.
    Comment(XmlText),
    /// A processing instruction other than the XML declaration. Reported, never run.
    Pi {
        /// The target.
        target: String,
        /// The data after the target and its white space.
        data: XmlText,
    },
}

/// One pull event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlEvent {
    /// What it is.
    pub kind: EventKind,
    /// Source byte span of the token. The `End` of an empty-element tag repeats the span
    /// and position of its tag.
    pub span: Range<u64>,
    /// Byte offset, line and column where the token starts.
    pub pos: Position,
}

/// Reader limits and the inherited namespace scope. Limits can only be lowered from
/// the defaults ([`MAX_DEPTH`], [`MAX_ATTRIBUTES`], [`MAX_NAME_LEN`], [`MAX_TEXT_LEN`]);
/// the depth limit is further lowered to the budget's depth limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlOptions {
    max_depth: u32,
    max_attributes: usize,
    max_name_len: usize,
    max_text_len: usize,
    inherited: Vec<NsBinding>,
    read_chunk: usize,
}

impl Default for XmlOptions {
    fn default() -> Self {
        XmlOptions {
            max_depth: MAX_DEPTH,
            max_attributes: MAX_ATTRIBUTES,
            max_name_len: MAX_NAME_LEN,
            max_text_len: MAX_TEXT_LEN,
            inherited: Vec::new(),
            read_chunk: CHUNK,
        }
    }
}

impl XmlOptions {
    /// Lowers the depth limit.
    pub fn with_max_depth(mut self, v: u32) -> Self {
        self.max_depth = v.min(MAX_DEPTH);
        self
    }
    /// Lowers the attributes-per-element limit.
    pub fn with_max_attributes(mut self, v: usize) -> Self {
        self.max_attributes = v.min(MAX_ATTRIBUTES);
        self
    }
    /// Lowers the name length limit.
    pub fn with_max_name_len(mut self, v: usize) -> Self {
        self.max_name_len = v.min(MAX_NAME_LEN);
        self
    }
    /// Lowers the text and token length limit.
    pub fn with_max_text_len(mut self, v: usize) -> Self {
        self.max_text_len = v.min(MAX_TEXT_LEN);
        self
    }
    /// Reads the document as if inside an element with these bindings in scope, for
    /// re-reading a subtree copied out of a larger document.
    pub fn with_inherited_namespaces(mut self, scope: &[NsBinding]) -> Self {
        self.inherited = scope.to_vec();
        self
    }
    /// Requests at most `v` bytes (1 to 8,192) from the source per read, so that tests can
    /// drive every token across buffer refills.
    pub fn with_read_chunk(mut self, v: usize) -> Self {
        self.read_chunk = v.clamp(1, CHUNK);
        self
    }
    /// The depth limit.
    pub fn max_depth(&self) -> u32 {
        self.max_depth
    }
    /// The attributes-per-element limit.
    pub fn max_attributes(&self) -> usize {
        self.max_attributes
    }
    /// The name length limit.
    pub fn max_name_len(&self) -> usize {
        self.max_name_len
    }
    /// The text and token length limit.
    pub fn max_text_len(&self) -> usize {
        self.max_text_len
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Init,
    Content,
    Done,
}

/// An open element.
struct Frame {
    raw: String,
    name: QName,
    start: u64,
    scope: NsScope,
}

/// Outcome of one lexing attempt.
enum Tok {
    /// An event is in `ready`.
    Ready,
    /// Input was consumed without an event.
    Skip,
    /// The token continues past the bytes read so far.
    More,
}

struct RawAttr {
    name: (usize, usize),
    value: (usize, usize),
}

struct RawTag {
    name: (usize, usize),
    attrs: Vec<RawAttr>,
    end: usize,
    empty: bool,
}

struct Built {
    raw: String,
    element: Element,
    scope_bytes: u64,
}

/// The hardened streaming pull reader. See the crate documentation.
pub struct XmlReader<R> {
    input: Input<R>,
    opts: XmlOptions,
    max_depth: u32,
    stack: Vec<Frame>,
    root_scope: NsScope,
    state: State,
    seen_root: bool,
    ready: Option<XmlEvent>,
    pending: Option<XmlEvent>,
    failed: Option<XmlError>,
}

impl<R: Read> XmlReader<R> {
    /// A reader with the default limits.
    pub fn new(source: BoundedSource<R>) -> Self {
        XmlReader::with_options(source, XmlOptions::default())
    }

    /// A reader with lowered limits or an inherited namespace scope.
    pub fn with_options(source: BoundedSource<R>, opts: XmlOptions) -> Self {
        let max_depth = opts.max_depth.min(source.budget().limits().depth());
        let mut scope: Vec<NsBinding> = Vec::new();
        for b in &opts.inherited {
            let reserved = matches!(b.prefix.as_deref(), Some("xml") | Some("xmlns"));
            if b.uri.is_empty() || reserved {
                continue;
            }
            scope.retain(|x| x.prefix != b.prefix);
            scope.push(b.clone());
        }
        scope.sort();
        XmlReader {
            input: Input::new(source, opts.read_chunk),
            opts,
            max_depth,
            stack: Vec::new(),
            root_scope: Arc::from(scope),
            state: State::Init,
            seen_root: false,
            ready: None,
            pending: None,
            failed: None,
        }
    }

    /// The next event; `Ok(None)` after the document ends. After an error every further
    /// call returns the same error.
    pub fn next_event(&mut self) -> Result<Option<XmlEvent>, XmlError> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        match self.step() {
            Ok(ev) => Ok(ev),
            Err(e) => {
                self.failed = Some(e.clone());
                self.state = State::Done;
                Err(e)
            }
        }
    }

    /// The number of open elements.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// The budget the reader charges.
    pub fn budget(&self) -> &Budget {
        self.input.budget()
    }

    /// Gives the source back.
    pub fn into_source(self) -> BoundedSource<R> {
        self.input.into_source()
    }

    fn step(&mut self) -> Result<Option<XmlEvent>, XmlError> {
        if let Some(ev) = self.pending.take() {
            return Ok(Some(ev));
        }
        match self.state {
            State::Done => return Ok(None),
            State::Init => {
                self.input.detect()?;
                self.prolog()?;
                self.state = State::Content;
            }
            State::Content => {}
        }
        let mut stalled = false;
        loop {
            if self.input.window().is_empty() {
                if self.input.fill(self.input.chunk())? {
                    continue;
                }
                return self.finish();
            }
            let markup = self.input.window().first() == Some(&b'<');
            let tok = if markup { self.markup()? } else { self.text()? };
            match tok {
                Tok::Ready => return Ok(self.ready.take()),
                Tok::Skip => stalled = false,
                Tok::More => {
                    let have = self.input.window().len();
                    if have > self.opts.max_text_len {
                        let limit = if markup { XmlLimit::TokenLength } else { XmlLimit::TextLength };
                        return Err(self.limit(limit, self.opts.max_text_len as u64, 0));
                    }
                    if stalled {
                        return Err(XmlError::TruncatedFile { at: self.input.pos_at(have) });
                    }
                    stalled = !self.input.fill(have.max(self.input.chunk()))?;
                }
            }
        }
    }

    fn finish(&mut self) -> Result<Option<XmlEvent>, XmlError> {
        if !self.stack.is_empty() || !self.seen_root {
            return Err(XmlError::TruncatedFile { at: self.input.pos_at(0) });
        }
        self.state = State::Done;
        Ok(None)
    }

    // ------------------------------------------------------------------ errors

    fn malformed(&self, reason: &'static str, rel: usize) -> XmlError {
        XmlError::Malformed { reason, at: self.input.pos_at(rel) }
    }

    fn limit(&self, limit: XmlLimit, value: u64, rel: usize) -> XmlError {
        XmlError::LimitExceeded { limit, value, at: self.input.pos_at(rel) }
    }

    fn name_err(&self, e: NameErr) -> XmlError {
        match e {
            NameErr::Bad(k) => self.malformed("bad_name", k),
            NameErr::TooLong(k) => self.limit(XmlLimit::NameLength, self.opts.max_name_len as u64, k),
        }
    }

    fn decode_err(&self, base: usize, e: (usize, DecodeErr)) -> XmlError {
        let (k, kind) = e;
        match kind {
            DecodeErr::Forbidden => {
                XmlError::ForbiddenConstruct { construct: Construct::EntityReference, at: self.input.pos_at(base + k) }
            }
            DecodeErr::Malformed(reason) => self.malformed(reason, base + k),
        }
    }

    fn utf8(&self, bytes: &[u8], rel: usize) -> Result<String, XmlError> {
        std::str::from_utf8(bytes).map(str::to_owned).map_err(|_| self.malformed("invalid_utf8", rel))
    }

    fn name(&self, w: &[u8], i: usize) -> Result<usize, XmlError> {
        scan_name(w, i, self.opts.max_name_len).map_err(|e| self.name_err(e))
    }

    fn text_limit(&self, len: usize) -> Result<(), XmlError> {
        if len > self.opts.max_text_len {
            return Err(self.limit(XmlLimit::TextLength, self.opts.max_text_len as u64, 0));
        }
        Ok(())
    }

    /// Consumes `len` bytes as one event.
    fn emit(&mut self, len: usize, kind: EventKind) -> Tok {
        let pos = self.input.pos_at(0);
        self.input.consume(len);
        self.ready = Some(XmlEvent { kind, span: pos.byte..self.input.tracker.src, pos });
        Tok::Ready
    }

    // ------------------------------------------------------------------ prolog

    /// The optional XML declaration: checked, never reported.
    fn prolog(&mut self) -> Result<(), XmlError> {
        while self.input.window().len() < 6 {
            if !self.input.fill(self.input.chunk())? {
                break;
            }
        }
        let w = self.input.window();
        if !(w.starts_with(b"<?xml") && w.get(5).is_some_and(|&b| is_ws(b))) {
            return Ok(());
        }
        let end = loop {
            if let Some(e) = find(self.input.window(), 5, b"?>") {
                break e;
            }
            let have = self.input.window().len();
            if have > MAX_DECL_LEN {
                return Err(self.malformed("bad_xml_declaration", 0));
            }
            if !self.input.fill(self.input.chunk())? {
                return Err(XmlError::TruncatedFile { at: self.input.pos_at(have) });
            }
        };
        let decl = self.input.window().get(..end).unwrap_or(&[]);
        let encoding = parse_decl(decl).map_err(|k| self.malformed("bad_xml_declaration", k))?;
        if let Some((a, b)) = encoding {
            let name = decl.get(a..b).unwrap_or(&[]);
            let ok = match self.input.encoding() {
                Encoding::Utf8 => name.eq_ignore_ascii_case(b"UTF-8"),
                Encoding::Utf16Le => name.eq_ignore_ascii_case(b"UTF-16") || name.eq_ignore_ascii_case(b"UTF-16LE"),
                Encoding::Utf16Be => name.eq_ignore_ascii_case(b"UTF-16") || name.eq_ignore_ascii_case(b"UTF-16BE"),
            };
            if !ok {
                return Err(XmlError::EncodingUnsupported { at: self.input.pos_at(a) });
            }
        }
        self.input.consume(end + 2);
        Ok(())
    }

    // ------------------------------------------------------------------ tokens

    fn markup(&mut self) -> Result<Tok, XmlError> {
        let Some(&second) = self.input.window().get(1) else {
            return Ok(Tok::More);
        };
        match second {
            b'?' => self.pi(),
            b'!' => self.bang(),
            b'/' => self.end_tag(),
            _ => self.start_tag(),
        }
    }

    fn bang(&mut self) -> Result<Tok, XmlError> {
        let w = self.input.window();
        let mut partial = false;
        for &(kw, construct) in BANG {
            if w.starts_with(kw) {
                return match construct {
                    Some(construct) => Err(XmlError::ForbiddenConstruct { construct, at: self.input.pos_at(0) }),
                    None if kw.len() == 4 => self.comment(),
                    None => self.cdata(),
                };
            }
            partial |= kw.starts_with(w);
        }
        if partial {
            return Ok(Tok::More);
        }
        Err(self.malformed("bad_markup_declaration", 0))
    }

    fn comment(&mut self) -> Result<Tok, XmlError> {
        let w = self.input.window();
        let Some(end) = find(w, 4, b"-->") else {
            return Ok(Tok::More);
        };
        let body = w.get(4..end).unwrap_or(&[]);
        if let Some(k) = find(body, 0, b"--") {
            return Err(self.malformed("double_hyphen_in_comment", 4 + k));
        }
        if body.last() == Some(&b'-') {
            return Err(self.malformed("double_hyphen_in_comment", 3 + body.len()));
        }
        self.text_limit(body.len())?;
        let text = decode(body, Mode::Plain).map_err(|e| self.decode_err(4, e))?;
        Ok(self.emit(end + 3, EventKind::Comment(XmlText::from_string(text))))
    }

    fn cdata(&mut self) -> Result<Tok, XmlError> {
        if self.stack.is_empty() {
            return Err(self.malformed("cdata_outside_root", 0));
        }
        let w = self.input.window();
        let Some(end) = find(w, 9, b"]]>") else {
            return Ok(Tok::More);
        };
        let body = w.get(9..end).unwrap_or(&[]);
        self.text_limit(body.len())?;
        let text = decode(body, Mode::Plain).map_err(|e| self.decode_err(9, e))?;
        Ok(self.emit(end + 3, EventKind::CData(XmlText::from_string(text))))
    }

    fn pi(&mut self) -> Result<Tok, XmlError> {
        let w = self.input.window();
        let name_end = self.name(w, 2)?;
        if name_end >= w.len() {
            return Ok(Tok::More);
        }
        let Some(end) = find(w, name_end, b"?>") else {
            return Ok(Tok::More);
        };
        let target = w.get(2..name_end).unwrap_or(&[]);
        if target.eq_ignore_ascii_case(b"xml") {
            return Err(self.malformed("reserved_pi_target", 0));
        }
        let data_start = if end == name_end {
            end
        } else if w.get(name_end).is_some_and(|&b| is_ws(b)) {
            skip_ws(w, name_end).min(end)
        } else {
            return Err(self.malformed("bad_processing_instruction", name_end));
        };
        let data = w.get(data_start..end).unwrap_or(&[]);
        self.text_limit(data.len())?;
        let target = self.utf8(target, 2)?;
        let data = decode(data, Mode::Plain).map_err(|e| self.decode_err(data_start, e))?;
        Ok(self.emit(end + 2, EventKind::Pi { target, data: XmlText::from_string(data) }))
    }

    fn end_tag(&mut self) -> Result<Tok, XmlError> {
        let w = self.input.window();
        let name_end = self.name(w, 2)?;
        if name_end >= w.len() {
            return Ok(Tok::More);
        }
        let k = skip_ws(w, name_end);
        match w.get(k) {
            None => return Ok(Tok::More),
            Some(&b'>') => {}
            Some(_) => return Err(self.malformed("bad_end_tag", k)),
        }
        let raw = w.get(2..name_end).unwrap_or(&[]);
        match self.stack.last() {
            None => return Err(self.malformed("unexpected_end_tag", 0)),
            Some(f) if f.raw.as_bytes() != raw => return Err(self.malformed("mismatched_end_tag", 0)),
            Some(_) => {}
        }
        let Some(frame) = self.stack.pop() else {
            return Err(self.malformed("unexpected_end_tag", 0));
        };
        let pos = self.input.pos_at(0);
        self.input.consume(k + 1);
        let end = self.input.tracker.src;
        let kind = EventKind::End { name: frame.name, element: frame.start..end };
        self.ready = Some(XmlEvent { kind, span: pos.byte..end, pos });
        Ok(Tok::Ready)
    }

    fn start_tag(&mut self) -> Result<Tok, XmlError> {
        if self.seen_root && self.stack.is_empty() {
            return Err(self.malformed("content_after_root", 0));
        }
        if self.stack.len() >= self.max_depth as usize {
            return Err(self.limit(XmlLimit::Depth, u64::from(self.max_depth), 0));
        }
        let Some(tag) = self.scan_start_tag()? else {
            return Ok(Tok::More);
        };
        let pos = self.input.pos_at(0);
        let built = self.build_element(&tag)?;
        if built.element.name.ns.as_deref() == Some(XINCLUDE_NS) {
            return Err(XmlError::ForbiddenConstruct { construct: Construct::XInclude, at: pos });
        }
        self.input.budget_mut().charge_elements(1, pos)?;
        if built.scope_bytes > 0 {
            self.input.budget_mut().charge_alloc(built.scope_bytes, pos)?;
        }
        self.input.consume(tag.end);
        let span = pos.byte..self.input.tracker.src;
        let element = built.element;
        if element.empty {
            let kind = EventKind::End { name: element.name.clone(), element: span.clone() };
            self.pending = Some(XmlEvent { kind, span: span.clone(), pos });
        } else {
            self.stack.push(Frame {
                raw: built.raw,
                name: element.name.clone(),
                start: pos.byte,
                scope: element.ns_scope.clone(),
            });
        }
        self.seen_root = true;
        self.ready = Some(XmlEvent { kind: EventKind::Start(element), span, pos });
        Ok(Tok::Ready)
    }

    /// Finds the extent of a start tag; `None` if it continues past the bytes read.
    fn scan_start_tag(&self) -> Result<Option<RawTag>, XmlError> {
        let w = self.input.window();
        let name_end = self.name(w, 1)?;
        if name_end >= w.len() {
            return Ok(None);
        }
        let mut attrs: Vec<RawAttr> = Vec::new();
        let mut i = name_end;
        loop {
            let k = skip_ws(w, i);
            match w.get(k) {
                None => return Ok(None),
                Some(&b'>') => return Ok(Some(RawTag { name: (1, name_end), attrs, end: k + 1, empty: false })),
                Some(&b'/') => {
                    return match w.get(k + 1) {
                        None => Ok(None),
                        Some(&b'>') => Ok(Some(RawTag { name: (1, name_end), attrs, end: k + 2, empty: true })),
                        Some(_) => Err(self.malformed("bad_start_tag", k + 1)),
                    };
                }
                Some(_) => {}
            }
            if k == i {
                return Err(self.malformed("missing_whitespace", k));
            }
            if attrs.len() >= self.opts.max_attributes {
                return Err(self.limit(XmlLimit::Attributes, self.opts.max_attributes as u64, k));
            }
            let an_end = self.name(w, k)?;
            if an_end >= w.len() {
                return Ok(None);
            }
            let eq = skip_ws(w, an_end);
            match w.get(eq) {
                None => return Ok(None),
                Some(&b'=') => {}
                Some(_) => return Err(self.malformed("expected_equals", eq)),
            }
            let q = skip_ws(w, eq + 1);
            let quote = match w.get(q) {
                None => return Ok(None),
                Some(&c) if c == b'"' || c == b'\'' => c,
                Some(_) => return Err(self.malformed("expected_quote", q)),
            };
            let vs = q + 1;
            let Some(len) = w.get(vs..).and_then(|r| r.iter().position(|&c| c == quote)) else {
                return Ok(None);
            };
            attrs.push(RawAttr { name: (k, an_end), value: (vs, vs + len) });
            i = vs + len + 1;
        }
    }

    /// Decodes a complete start tag and resolves its namespaces.
    fn build_element(&self, tag: &RawTag) -> Result<Built, XmlError> {
        let w = self.input.window();
        let slice = move |r: (usize, usize)| w.get(r.0..r.1).unwrap_or(&[]);
        let raw_name = self.utf8(slice(tag.name), tag.name.0)?;

        // Attributes as written: unique raw names, decoded values.
        let mut seen = BTreeSet::new();
        let mut decls: Vec<NsBinding> = Vec::new();
        let mut others: Vec<(Option<String>, String, usize, String)> = Vec::new();
        for a in &tag.attrs {
            let at = a.name.0;
            let name = self.utf8(slice(a.name), at)?;
            if !seen.insert(name.clone()) {
                return Err(self.malformed("duplicate_attribute", at));
            }
            let value = decode(slice(a.value), Mode::Attr).map_err(|e| self.decode_err(a.value.0, e))?;
            let (prefix, local) = split_qname(&name).ok_or_else(|| self.malformed("bad_qname", at))?;
            if prefix.is_none() && local == "xmlns" {
                if value == XML_NS || value == XMLNS_NS {
                    return Err(self.malformed("bad_namespace_declaration", at));
                }
                decls.push(NsBinding { prefix: None, uri: value });
            } else if prefix == Some("xmlns") {
                let bad = local == "xmlns" || value.is_empty() || value == XMLNS_NS || ((local == "xml") != (value == XML_NS));
                if bad {
                    return Err(self.malformed("bad_namespace_declaration", at));
                }
                decls.push(NsBinding { prefix: Some(local.to_owned()), uri: value });
            } else {
                others.push((prefix.map(str::to_owned), local.to_owned(), at, value));
            }
        }

        // The scope: the parent's, with this tag's declarations applied.
        let parent = self.stack.last().map_or_else(|| self.root_scope.clone(), |f| f.scope.clone());
        let mut scope_bytes = 0u64;
        let scope: NsScope = if decls.is_empty() {
            parent
        } else {
            let mut v: Vec<NsBinding> =
                parent.iter().filter(|b| !decls.iter().any(|d| d.prefix == b.prefix)).cloned().collect();
            v.extend(decls.iter().filter(|d| !d.uri.is_empty() && d.prefix.as_deref() != Some("xml")).cloned());
            v.sort();
            scope_bytes = v.iter().map(|b| (b.uri.len() + b.prefix.as_ref().map_or(0, String::len) + 64) as u64).sum();
            Arc::from(v)
        };
        let resolve = |prefix: Option<&str>, attr: bool| -> Option<Option<String>> {
            match prefix {
                None if attr => Some(None),
                None => Some(scope.iter().find(|b| b.prefix.is_none()).map(|b| b.uri.clone())),
                Some("xml") => Some(Some(XML_NS.to_owned())),
                Some("xmlns") => None,
                Some(p) => scope.iter().find(|b| b.prefix.as_deref() == Some(p)).map(|b| Some(b.uri.clone())),
            }
        };

        let (prefix, local) = split_qname(&raw_name).ok_or_else(|| self.malformed("bad_qname", tag.name.0))?;
        let ns = resolve(prefix, false).ok_or_else(|| self.malformed("unbound_prefix", tag.name.0))?;
        let name = QName { prefix: prefix.map(str::to_owned), local: local.to_owned(), ns };

        let mut attrs = Vec::with_capacity(others.len());
        let mut expanded = BTreeSet::new();
        for (prefix, local, at, value) in others {
            let ns = resolve(prefix.as_deref(), true).ok_or_else(|| self.malformed("unbound_prefix", at))?;
            if let Some(uri) = &ns {
                if !expanded.insert((uri.clone(), local.clone())) {
                    return Err(self.malformed("duplicate_attribute", at));
                }
            }
            attrs.push(Attr { name: QName { prefix, local, ns }, value: XmlText::from_string(value) });
        }
        let element = Element { name, attrs, ns_declared: decls, ns_scope: scope, empty: tag.empty };
        Ok(Built { raw: raw_name, element, scope_bytes })
    }

    fn text(&mut self) -> Result<Tok, XmlError> {
        let w = self.input.window();
        let len = match w.iter().position(|&b| b == b'<') {
            Some(n) => n,
            None if self.input.at_end() => w.len(),
            None => return Ok(Tok::More),
        };
        let raw = w.get(..len).unwrap_or(&[]);
        if self.stack.is_empty() {
            if let Some(k) = raw.iter().position(|&b| !is_ws(b)) {
                return Err(self.malformed("text_outside_root", k));
            }
            self.input.consume(len);
            return Ok(Tok::Skip);
        }
        if len > self.opts.max_text_len {
            return Err(self.limit(XmlLimit::TextLength, self.opts.max_text_len as u64, 0));
        }
        let text = decode(raw, Mode::Text).map_err(|e| self.decode_err(0, e))?;
        Ok(self.emit(len, EventKind::Text(XmlText::from_string(text))))
    }
}

/// Parses `<?xml` VersionInfo EncodingDecl? SDDecl? S? (the bytes before `?>`).
/// Returns the byte range of the encoding name, or the index of the first error.
fn parse_decl(d: &[u8]) -> Result<Option<(usize, usize)>, usize> {
    let (mut i, (vs, ve)) = pseudo_attr(d, 5, b"version")?.ok_or(5usize)?;
    let version = d.get(vs..ve).unwrap_or(&[]);
    let digits = version.strip_prefix(b"1.").unwrap_or(&[]);
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return Err(vs);
    }
    let mut encoding = None;
    if let Some((next, (es, ee))) = pseudo_attr(d, i, b"encoding")? {
        let e = d.get(es..ee).unwrap_or(&[]);
        let valid = e.first().is_some_and(u8::is_ascii_alphabetic)
            && e.iter().all(|&b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
        if !valid {
            return Err(es);
        }
        encoding = Some((es, ee));
        i = next;
    }
    if let Some((next, (ss, se))) = pseudo_attr(d, i, b"standalone")? {
        let s = d.get(ss..se).unwrap_or(&[]);
        if s != b"yes" && s != b"no" {
            return Err(ss);
        }
        i = next;
    }
    let j = skip_ws(d, i);
    if j != d.len() {
        return Err(j);
    }
    Ok(encoding)
}

/// One pseudo-attribute `S name S? = S? "value"` of the XML declaration starting at
/// `i`: the index after it and the value's range, `None` if `name` is not next.
fn pseudo_attr(d: &[u8], i: usize, name: &[u8]) -> Result<Option<(usize, (usize, usize))>, usize> {
    let j = skip_ws(d, i);
    if j == i || !d.get(j..).is_some_and(|r| r.starts_with(name)) {
        return Ok(None);
    }
    let eq = skip_ws(d, j + name.len());
    if d.get(eq) != Some(&b'=') {
        return Err(eq);
    }
    let q = skip_ws(d, eq + 1);
    let quote = match d.get(q) {
        Some(&c) if c == b'"' || c == b'\'' => c,
        _ => return Err(q),
    };
    let vs = q + 1;
    let len = d.get(vs..).and_then(|r| r.iter().position(|&b| b == quote)).ok_or(vs)?;
    Ok(Some((vs + len + 1, (vs, vs + len))))
}
