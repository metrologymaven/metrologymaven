//! Acceptance criteria of spec node IO.QIF.XML.PARSE (AC-1 .. AC-6) and the requirements
//! behind them. Expected events come from `fixtures/io/qif/xml/parse/*.events`, written
//! by the independent reference script `make_fixtures.py` (Python's expat with DTDs
//! disabled); expected errors come from its hand-labelled `expected.txt`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_reg::{BoundedSource, Budget, LimitKind, LimitRequest, Position, Principal};
use fid_xml::{
    fuzz_budget, read_slice, read_slice_with, Construct, EventKind, NsBinding, XmlError, XmlEvent, XmlLimit,
    XmlOptions, XmlReader, MAX_ATTRIBUTES, MAX_DEPTH, MAX_NAME_LEN, MAX_TEXT_LEN, XML_NS,
};
use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/qif/xml/parse").join(name)
}

fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).unwrap()
}

fn expected() -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(fixture("expected.txt")).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (k, v) = l.split_once(' ').unwrap();
            (k.to_string(), v.trim().to_string())
        })
        .collect()
}

fn num(e: &BTreeMap<String, String>, key: &str) -> u64 {
    e[key].parse().unwrap()
}

fn budget() -> Budget {
    Budget::new(Principal::Agent, &LimitRequest::default()).unwrap()
}

fn read(data: &[u8]) -> Result<Vec<XmlEvent>, XmlError> {
    read_slice(data, budget())
}

/// A source that hands out one byte per read.
struct Trickle<'a>(&'a [u8]);

impl Read for Trickle<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let data: &[u8] = self.0;
        match (data.split_first(), buf.first_mut()) {
            (Some((&b, rest)), Some(slot)) => {
                *slot = b;
                self.0 = rest;
                Ok(1)
            }
            _ => Ok(0),
        }
    }
}

/// Reads `data` one byte per source read and one byte per refill request, so every
/// token is assembled across buffer refills.
fn read_trickle(data: &[u8], b: Budget) -> Result<Vec<XmlEvent>, XmlError> {
    let options = XmlOptions::default().with_read_chunk(1);
    let mut reader = XmlReader::with_options(BoundedSource::new(Trickle(data), b), options);
    let mut out = Vec::new();
    while let Some(ev) = reader.next_event()? {
        out.push(ev);
    }
    Ok(out)
}

// ------------------------------------------------------------------ event rendering

/// The escaping of make_fixtures.py.
fn esc(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c < ' ' || c == '\u{7f}' => {
                let v = u32::from(c);
                out.push_str(&format!("\\u{{{v:x}}}"));
            }
            c => out.push(c),
        }
    }
    out
}

fn opt(s: &Option<String>) -> String {
    esc(s.as_deref().unwrap_or(""))
}

/// The event line format of make_fixtures.py.
fn render(events: &[XmlEvent]) -> String {
    let mut out = String::new();
    for ev in events {
        let line = match &ev.kind {
            EventKind::Start(e) => {
                for d in &e.ns_declared {
                    out.push_str(&format!("N\t{}\t{}\n", opt(&d.prefix), esc(&d.uri)));
                }
                let mut s = format!("S\t{}\t{{{}}}{}", opt(&e.name.prefix), opt(&e.name.ns), esc(&e.name.local));
                for a in &e.attrs {
                    let n = &a.name;
                    s.push_str(&format!(
                        "\t{}:{{{}}}{}={}",
                        opt(&n.prefix),
                        opt(&n.ns),
                        esc(&n.local),
                        esc(a.value.raw())
                    ));
                }
                s
            }
            EventKind::End { name, .. } => format!("E\t{{{}}}{}", opt(&name.ns), esc(&name.local)),
            EventKind::Text(t) => format!("T\t{}", esc(t.raw())),
            EventKind::CData(t) => format!("C\t{}", esc(t.raw())),
            EventKind::Comment(t) => format!("M\t{}", esc(t.raw())),
            EventKind::Pi { target, data } => format!("P\t{}\t{}", esc(target), esc(data.raw())),
        };
        out.push_str(&line);
        out.push('\n');
    }
    out
}

const OK_FIXTURES: &[&str] = &["basic", "basic_bom", "ns", "utf16", "utf16be", "utf16_twin", "deep64", "attrs256"];

/// Every well-formed fixture gives exactly expat's events, read in one go and read one
/// byte at a time.
#[test]
fn events_match_the_expat_reference() {
    for name in OK_FIXTURES {
        let data = bytes(&format!("{name}.xml"));
        let want = std::fs::read_to_string(fixture(&format!("{name}.events"))).unwrap();
        let events = read(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(render(&events), want, "{name}");
        let trickled = read_trickle(&data, budget()).unwrap();
        assert_eq!(trickled, events, "{name} read one byte at a time");
    }
}

// ------------------------------------------------------------------ AC-1, REQ-001

/// AC-1: external entity, DOCTYPE, billion laughs and XInclude each fail
/// `xml_forbidden_construct` at the construct; so do an undeclared entity reference (in
/// text and in an attribute), a bare entity declaration and an XInclude default namespace.
#[test]
fn ac1_forbidden_constructs_fail_at_the_construct() {
    let e = expected();
    let names = [
        "xxe",
        "doctype",
        "billion_laughs",
        "xinclude",
        "xinclude_default",
        "entity_ref",
        "entity_ref_attr",
        "entity_decl",
    ];
    for name in names {
        let err = read(&bytes(&format!("{name}.xml"))).unwrap_err();
        assert_eq!(err.code(), e[&format!("{name}.code")], "{name}");
        let at = Position::line_col(
            num(&e, &format!("{name}.byte")),
            num(&e, &format!("{name}.line")) as u32,
            num(&e, &format!("{name}.col")) as u32,
        );
        match err {
            XmlError::ForbiddenConstruct { construct, at: got } => {
                assert_eq!(construct.as_str(), e[&format!("{name}.construct")], "{name}");
                assert_eq!(got, at, "{name}");
            }
            other => panic!("{name}: unexpected {other:?}"),
        }
    }
}

/// REQ-001: the refusal happens before anything after the construct is read: the events
/// before it are delivered, and the reader stops there.
#[test]
fn req001_events_before_the_construct_are_delivered() {
    let data = bytes("xinclude.xml");
    let mut r = XmlReader::new(BoundedSource::new(&data[..], budget()));
    let mut starts = Vec::new();
    let err = loop {
        match r.next_event() {
            Ok(Some(ev)) => {
                if let EventKind::Start(el) = ev.kind {
                    starts.push(el.name.local);
                }
            }
            Ok(None) => panic!("xinclude.xml must fail"),
            Err(e) => break e,
        }
    };
    assert_eq!(starts, ["plan", "step"]);
    assert!(matches!(err, XmlError::ForbiddenConstruct { construct: Construct::XInclude, .. }));
    // The error is sticky.
    assert_eq!(r.next_event().unwrap_err(), err);
}

/// Markup declarations other than ENTITY are refused too, and so is a DOCTYPE after the
/// root element.
#[test]
fn req001_other_markup_declarations() {
    let cases: &[(&[u8], Construct)] = &[
        (b"<!ELEMENT r ANY>\n<r/>", Construct::MarkupDeclaration),
        (b"<!ATTLIST r a CDATA #IMPLIED>\n<r/>", Construct::MarkupDeclaration),
        (b"<!NOTATION n SYSTEM \"x\">\n<r/>", Construct::MarkupDeclaration),
        (b"<r/>\n<!DOCTYPE r>", Construct::Doctype),
        (b"<r><!DOCTYPE r></r>", Construct::Doctype),
    ];
    for &(doc, construct) in cases {
        match read(doc).unwrap_err() {
            XmlError::ForbiddenConstruct { construct: c, .. } => assert_eq!(c, construct),
            other => panic!("unexpected {other:?}"),
        }
    }
}

/// Numeric character references and the five predefined entities are allowed.
#[test]
fn req001_character_references_are_allowed() {
    let events = read(b"<r a='&#x3C;&#60;&lt;'>&#x10FFFF;&#9;&amp;&gt;&quot;&apos;</r>").unwrap();
    match &events[0].kind {
        EventKind::Start(e) => assert_eq!(e.attrs[0].value.raw(), "<<<"),
        other => panic!("unexpected {other:?}"),
    }
    match &events[1].kind {
        EventKind::Text(t) => assert_eq!(t.raw(), "\u{10FFFF}\t&>\"'"),
        other => panic!("unexpected {other:?}"),
    }
}

// ------------------------------------------------------------------ AC-2, AC-3, REQ-002

/// AC-2: deep65.xml fails `limit_exceeded` (depth 64) at the 65th start tag; deep64.xml
/// (in `events_match_the_expat_reference`) parses.
#[test]
fn ac2_depth_65_fails() {
    let e = expected();
    let err = read(&bytes("deep65.xml")).unwrap_err();
    assert_eq!(err.code(), e["deep65.code"]);
    assert_eq!(e["deep65.limit"], XmlLimit::Depth.as_str());
    assert_eq!(
        err,
        XmlError::LimitExceeded {
            limit: XmlLimit::Depth,
            value: num(&e, "deep65.value"),
            at: Position::line_col(num(&e, "deep65.byte"), num(&e, "deep65.line") as u32, num(&e, "deep65.col") as u32),
        }
    );
    assert_eq!(u64::from(MAX_DEPTH), num(&e, "deep65.value"));
}

/// The depth limit also follows a lowered harness depth limit.
#[test]
fn ac2_depth_follows_the_budget() {
    let b = Budget::new(Principal::Agent, &LimitRequest::default().depth(3)).unwrap();
    let err = read_slice(b"<a><b><c><d/></c></b></a>", b).unwrap_err();
    assert_eq!(err, XmlError::LimitExceeded { limit: XmlLimit::Depth, value: 3, at: Position::line_col(9, 1, 10) });
}

/// AC-3: an element with 257 attributes fails `limit_exceeded` (attributes 256) at the
/// 257th attribute; 256 attributes parse.
#[test]
fn ac3_attributes_257_fails() {
    let e = expected();
    let err = read(&bytes("attrs257.xml")).unwrap_err();
    assert_eq!(err.code(), e["attrs257.code"]);
    assert_eq!(e["attrs257.limit"], XmlLimit::Attributes.as_str());
    assert_eq!(
        err,
        XmlError::LimitExceeded {
            limit: XmlLimit::Attributes,
            value: num(&e, "attrs257.value"),
            at: Position::line_col(
                num(&e, "attrs257.byte"),
                num(&e, "attrs257.line") as u32,
                num(&e, "attrs257.col") as u32
            ),
        }
    );
    assert_eq!(MAX_ATTRIBUTES as u64, num(&e, "attrs257.value"));
    match &read(&bytes("attrs256.xml")).unwrap()[0].kind {
        EventKind::Start(el) => assert_eq!(el.attrs.len(), 256),
        other => panic!("unexpected {other:?}"),
    }
}

/// Namespace declarations count as attributes.
#[test]
fn ac3_namespace_declarations_count() {
    let mut doc = b"<r".to_vec();
    for i in 0..257 {
        doc.extend_from_slice(format!(" xmlns:p{i}=\"urn:{i}\"").as_bytes());
    }
    doc.extend_from_slice(b"/>");
    let err = read(&doc).unwrap_err();
    assert!(matches!(err, XmlError::LimitExceeded { limit: XmlLimit::Attributes, value: 256, .. }), "{err:?}");
}

/// Names longer than 1,024 bytes fail as soon as byte 1,025 is seen; 1,024 is fine.
#[test]
fn req002_name_length() {
    let ok = format!("<{0}/>", "n".repeat(MAX_NAME_LEN));
    assert_eq!(read(ok.as_bytes()).unwrap().len(), 2);
    let long = format!("<{0}", "n".repeat(MAX_NAME_LEN + 1));
    let err = read_trickle(long.as_bytes(), budget()).unwrap_err();
    assert_eq!(
        err,
        XmlError::LimitExceeded { limit: XmlLimit::NameLength, value: 1024, at: Position::line_col(1, 1, 2) }
    );
    let attr = format!("<r {0}='1'/>", "a".repeat(MAX_NAME_LEN + 1));
    assert!(matches!(read(attr.as_bytes()).unwrap_err(), XmlError::LimitExceeded { limit: XmlLimit::NameLength, .. }));
}

/// Text nodes and tokens are bounded (lowered limits; the defaults are 64 MiB).
#[test]
fn req002_text_and_token_length() {
    assert_eq!(MAX_TEXT_LEN, 64 * 1024 * 1024);
    let opts = || XmlOptions::default().with_max_text_len(16);
    let ok = read_slice_with(b"<r>0123456789abcdef</r>", budget(), opts()).unwrap();
    assert_eq!(ok.len(), 3);
    let err = read_slice_with(b"<r>0123456789abcdefg</r>", budget(), opts()).unwrap_err();
    assert_eq!(
        err,
        XmlError::LimitExceeded { limit: XmlLimit::TextLength, value: 16, at: Position::line_col(3, 1, 4) }
    );
    // Unterminated text grows the buffer only up to the limit.
    let unterminated = format!("<r>{}", "x".repeat(100));
    let err = read_slice_with(unterminated.as_bytes(), budget(), opts().with_read_chunk(4)).unwrap_err();
    assert!(matches!(err, XmlError::LimitExceeded { limit: XmlLimit::TextLength, .. }), "{err:?}");
    let comment = read_slice_with(b"<r><!-- 0123456789abcdefg --></r>", budget(), opts()).unwrap_err();
    assert!(matches!(comment, XmlError::LimitExceeded { limit: XmlLimit::TextLength, .. }), "{comment:?}");
    let tag = format!("<r a='{}'", "v".repeat(100));
    let err = read_slice_with(tag.as_bytes(), budget(), opts().with_read_chunk(4)).unwrap_err();
    assert!(matches!(err, XmlError::LimitExceeded { limit: XmlLimit::TokenLength, .. }), "{err:?}");
}

/// The harness budget applies: element and byte limits come back as `limit_exceeded`
/// from the harness, with line and column.
#[test]
fn req002_harness_budget() {
    let doc = b"<r><a/><a/><a/></r>";
    let b = Budget::new(Principal::Script, &LimitRequest::default().elements(3)).unwrap();
    match read_slice(doc, b).unwrap_err() {
        XmlError::Harness(fid_io_reg::IoError::LimitExceeded { limit: LimitKind::Elements, value: 3, at }) => {
            assert_eq!(at, Position::line_col(11, 1, 12));
        }
        other => panic!("unexpected {other:?}"),
    }
    let b = Budget::new(Principal::Script, &LimitRequest::default().bytes(8)).unwrap();
    let err = read_slice(doc, b).unwrap_err();
    assert_eq!(err.code(), "limit_exceeded");
    assert!(err.position().unwrap().line.is_some());
}

// ------------------------------------------------------------------ AC-4, REQ-004

fn without_offsets(events: &[XmlEvent]) -> Vec<(EventKind, Option<u32>, Option<u32>)> {
    events
        .iter()
        .map(|ev| {
            let mut kind = ev.kind.clone();
            if let EventKind::End { element, .. } = &mut kind {
                *element = 0..0;
            }
            (kind, ev.pos.line, ev.pos.col)
        })
        .collect()
}

/// AC-4: utf16.xml (and its big-endian sibling) give the events of the UTF-8 twin, with
/// the same lines and columns; byte spans are offsets in the UTF-16 source.
#[test]
fn ac4_utf16_equals_its_utf8_twin() {
    let twin = read(&bytes("utf16_twin.xml")).unwrap();
    for name in ["utf16.xml", "utf16be.xml"] {
        let data = bytes(name);
        let events = read(&data).unwrap();
        assert_eq!(without_offsets(&events), without_offsets(&twin), "{name}");
        // The first start tag's span is the UTF-16 bytes of `<messung einheit="mm">`.
        let le = name == "utf16.xml";
        let tag: Vec<u8> = "<messung einheit=\"mm\">"
            .encode_utf16()
            .flat_map(|u| if le { u.to_le_bytes() } else { u.to_be_bytes() })
            .collect();
        let at = data.windows(tag.len()).position(|w| w == tag).unwrap() as u64;
        assert_eq!(events[0].span, at..at + tag.len() as u64, "{name}");
        // The non-BMP character (a surrogate pair) arrives as one char.
        assert!(events.iter().any(|ev| matches!(&ev.kind, EventKind::Text(t) if t.raw().starts_with('\u{1D11E}'))));
    }
}

/// AC-4: latin1.xml declaring ISO-8859-1 fails `xml_encoding_unsupported`, and so do
/// UTF-16 without BOM, a BOM that contradicts the declaration, and UCS-4.
#[test]
fn ac4_other_encodings_fail() {
    let e = expected();
    for name in ["latin1", "utf16_nobom", "utf16_decl_utf8", "utf8_decl_utf16", "utf32"] {
        let err = read(&bytes(&format!("{name}.xml"))).unwrap_err();
        assert_eq!(err.code(), e[&format!("{name}.code")], "{name}");
        assert!(matches!(err, XmlError::EncodingUnsupported { .. }), "{name}");
    }
    // The position is that of the declared name.
    let err = read(&bytes("latin1.xml")).unwrap_err();
    assert_eq!(err, XmlError::EncodingUnsupported { at: Position::line_col(30, 1, 31) });
}

// ------------------------------------------------------------------ AC-6, REQ-006

/// AC-6: the span of a nested element of ns.xml, copied verbatim and re-read with its
/// reported in-scope namespaces, yields the same events.
#[test]
fn ac6_subtree_copy_round_trips() {
    let data = bytes("ns.xml");
    let events = read(&data).unwrap();
    let first =
        events.iter().position(|ev| matches!(&ev.kind, EventKind::Start(e) if e.name.local == "block")).unwrap();
    let EventKind::Start(block) = &events[first].kind else { unreachable!() };
    let scope = block.ns_scope.to_vec();
    assert_eq!(
        scope,
        [
            NsBinding { prefix: None, uri: "urn:example:default".into() },
            NsBinding { prefix: Some("ext".into()), uri: "urn:example:ext".into() },
            NsBinding { prefix: Some("q".into()), uri: "urn:example:q".into() },
        ]
    );
    let last =
        events.iter().position(|ev| matches!(&ev.kind, EventKind::End { name, .. } if name.local == "block")).unwrap();
    let EventKind::End { element, .. } = &events[last].kind else { unreachable!() };
    assert_eq!(element.start, events[first].span.start);
    assert_eq!(element.end, events[last].span.end);
    let copy = &data[element.start as usize..element.end as usize];
    assert!(copy.starts_with(b"<ext:block ") && copy.ends_with(b"</ext:block>"));

    let options = XmlOptions::default().with_inherited_namespaces(&scope);
    let again = read_slice_with(copy, budget(), options).unwrap();
    let original = &events[first..=last];
    assert_eq!(again.len(), original.len());
    for (a, b) in again.iter().zip(original) {
        assert_eq!(a.kind_without_element_span(), b.kind_without_element_span());
        assert_eq!(a.span.start + element.start, b.span.start);
        assert_eq!(a.span.end + element.start, b.span.end);
    }
    // Inside the copy the default namespace is undeclared and q is rebound, as before.
    let leaf = again.iter().find_map(|ev| match &ev.kind {
        EventKind::Start(e) if e.name.local == "leaf" => Some(e.clone()),
        _ => None,
    });
    let leaf = leaf.unwrap();
    assert_eq!(leaf.name.ns.as_deref(), Some("urn:example:q2"));
    assert_eq!(leaf.attrs[0].name.ns.as_deref(), Some("urn:example:q2"));
}

trait KindOnly {
    fn kind_without_element_span(&self) -> EventKind;
}

impl KindOnly for XmlEvent {
    fn kind_without_element_span(&self) -> EventKind {
        let mut kind = self.kind.clone();
        if let EventKind::End { element, .. } = &mut kind {
            *element = 0..0;
        }
        kind
    }
}

/// REQ-006: every start event exposes its in-scope declarations; `xml:` resolves to the
/// XML namespace without a declaration.
#[test]
fn req006_namespace_scope() {
    let events = read(&bytes("ns.xml")).unwrap();
    let EventKind::Start(doc) = &events[0].kind else { panic!("first event is the root") };
    assert_eq!(doc.ns_declared.len(), 2);
    assert_eq!(doc.attrs[0].name.ns.as_deref(), Some(XML_NS));
    let part = events
        .iter()
        .find_map(|ev| match &ev.kind {
            EventKind::Start(e) if e.name.local == "part" => Some(e.clone()),
            _ => None,
        })
        .unwrap();
    // xmlns="" removes the default binding from the scope.
    assert!(part.ns_scope.iter().all(|b| b.prefix.is_some()));
    assert_eq!(part.ns_declared, [NsBinding { prefix: None, uri: String::new() }]);
}

// ------------------------------------------------------------------ REQ-003, REQ-005

/// Malformed input fails with the hand-labelled code; every error carries byte offset,
/// line and column (REQ-003).
#[test]
fn malformed_inputs_fail_with_positions() {
    let e = expected();
    let mut seen = 0;
    for (key, code) in &e {
        let Some(name) = key.strip_prefix("malformed.").and_then(|k| k.strip_suffix(".code")) else { continue };
        let data = bytes(&format!("malformed/{name}.xml"));
        let err = read(&data).unwrap_err();
        assert_eq!(err.code(), code, "{name}: {err}");
        let at = err.position().unwrap();
        assert!(at.line.is_some() && at.col.is_some(), "{name}");
        assert_eq!(read_trickle(&data, budget()).unwrap_err(), err, "{name} read one byte at a time");
        seen += 1;
    }
    assert!(seen >= 30);
}

/// REQ-003: every event carries a position consistent with its span.
#[test]
fn req003_event_positions() {
    let data = bytes("basic.xml");
    for ev in read(&data).unwrap() {
        assert!(ev.pos.line.is_some() && ev.pos.col.is_some());
        assert_eq!(ev.pos.byte, ev.span.start);
        assert!(ev.span.end > ev.span.start);
    }
    // CRLF counts as one line end (the root tag's `multi` value spans lines 4 and 5): the
    // <item n="2"> start tag is on line 7, column 3.
    let events = read(&data).unwrap();
    let item2 = events
        .iter()
        .find(|ev| matches!(&ev.kind, EventKind::Start(e) if e.attrs.first().is_some_and(|a| a.value.raw() == "2")))
        .unwrap();
    assert_eq!((item2.pos.line, item2.pos.col), (Some(7), Some(3)));
    let at = data.windows(12).position(|w| w == b"<item n=\"2\">").unwrap() as u64;
    assert_eq!(item2.span, at..at + 12);
}

/// The reader streams: a 1.9 MB document is read with a buffer of a few chunks, not a
/// copy of the document.
#[test]
fn streaming_keeps_the_buffer_small() {
    let mut doc = b"<r>\n".to_vec();
    for i in 0..100_000 {
        doc.extend_from_slice(format!("<i n=\"{i}\">x</i>\n").as_bytes());
    }
    doc.extend_from_slice(b"</r>\n");
    assert!(doc.len() > 1_900_000);
    let mut r = XmlReader::new(BoundedSource::new(&doc[..], budget()));
    let mut count = 0u64;
    while let Some(ev) = r.next_event().unwrap() {
        if matches!(ev.kind, EventKind::Start(_)) {
            count += 1;
        }
    }
    assert_eq!(count, 100_001);
    assert_eq!(r.budget().elements_used(), 100_001);
    assert!(r.budget().alloc_used() < 128 * 1024, "{}", r.budget().alloc_used());
}

fn next_rand(state: &mut u64) -> u64 {
    // xorshift64
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

const TOKENS: &[&[u8]] = &[
    b"<",
    b">",
    b"&",
    b"<!--",
    b"-->",
    b"]]>",
    b"<![CDATA[",
    b"&#x0;",
    b"&#1114111;",
    b"\xff",
    b"\x00",
    b"\xd8\x00",
    b"<a>",
    b"</a>",
    b"<b/>",
    b" xmlns:p=\"\"",
    b" xmlns=\"urn:x\"",
    b"<!DOCTYPE",
    b"<!ENTITY",
    b"\"",
    b"'",
    b"?>",
    b"<?",
    b"\r",
    b"&lt;",
    b"&e;",
    b"\xf0\x9d\x84\x9e",
    b"p:",
];

fn mutate(state: &mut u64, data: &[u8]) -> Vec<u8> {
    let mut v = data.to_vec();
    let ops = 1 + next_rand(state) % 4;
    for _ in 0..ops {
        let r = next_rand(state);
        let at = if v.is_empty() { 0 } else { (r >> 16) as usize % (v.len() + 1) };
        match r % 6 {
            0 if at < v.len() => v[at] = (r >> 40) as u8,
            1 => v.truncate(at),
            2 | 3 => {
                let t = TOKENS[(r >> 8) as usize % TOKENS.len()];
                let tail = v.split_off(at);
                v.extend_from_slice(t);
                v.extend_from_slice(&tail);
            }
            4 if at < v.len() => {
                let end = (at + 1 + (r >> 48) as usize % 64).min(v.len());
                let tail = v.split_off(at);
                let dup = tail[..end - at].to_vec();
                v.extend_from_slice(&dup);
                v.extend_from_slice(&tail);
            }
            _ if at < v.len() => v[at] ^= 1 << ((r >> 32) % 8),
            _ => {}
        }
    }
    v
}

/// AC-5 (in-tree part; the cargo-fuzz target is `fuzz/fuzz_targets/xml_reader.rs`): on
/// thousands of mutated fixtures the reader returns events or a typed error with a
/// position, never panics, terminates, and gives the same outcome however the input is
/// chunked.
#[test]
fn ac5_mutated_inputs_never_panic() {
    let mut seeds: Vec<Vec<u8>> = Vec::new();
    for entry in std::fs::read_dir(fixture("")).unwrap().chain(std::fs::read_dir(fixture("malformed")).unwrap()) {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|x| x == "xml") {
            seeds.push(std::fs::read(path).unwrap());
        }
    }
    assert!(seeds.len() > 50);
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut outcomes = BTreeMap::new();
    for seed in &seeds {
        for _ in 0..60 {
            let input = mutate(&mut state, seed);
            let whole = read_slice(&input, fuzz_budget().unwrap());
            let trickled = read_trickle(&input, fuzz_budget().unwrap());
            assert_eq!(whole, trickled, "chunking changed the outcome of {input:?}");
            if let Err(e) = &whole {
                let at = e.position().unwrap();
                assert!(at.line.is_some() && at.col.is_some(), "{e:?}");
            }
            *outcomes.entry(whole.map_or_else(|e| e.code(), |_| "ok")).or_insert(0u32) += 1;
        }
    }
    // The mutations reach the main outcomes.
    for code in ["ok", "malformed", "truncated_file", "xml_forbidden_construct"] {
        assert!(outcomes.contains_key(code), "{code} not reached: {outcomes:?}");
    }
}
