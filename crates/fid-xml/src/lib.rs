//! The hardened streaming XML pull reader of Metrology Maven (spec node
//! `IO.QIF.XML.PARSE`), shared by every IO component that reads XML: QIF documents, XSDs
//! and workbook parts.
//!
//! [`XmlReader`] reads namespace-aware XML 1.0 from a [`fid_io_reg::BoundedSource`] and
//! hands out one [`XmlEvent`] at a time. It is written for hostile input:
//!
//! - No DTD processing at all. A DOCTYPE, an entity or other markup declaration, a
//!   reference to any entity but the five predefined ones, and an element in the
//!   XInclude namespace each fail with `xml_forbidden_construct` at the construct, before
//!   anything is resolved. Nothing is ever fetched, so there is no external-entity (XXE)
//!   or entity-expansion attack surface. Numeric character references are allowed.
//! - Bounded: element depth 64, 256 attributes per element, names of 1,024 bytes, text
//!   nodes and tokens of 64 MiB (`limit_exceeded`), plus the byte, element, allocation
//!   and wall-time budget of the harness. Growth of the reader's buffer is charged to the
//!   allocation budget before it happens.
//! - Streaming: the source is read in 8 KiB chunks and only the token being read is
//!   buffered; the reader keeps no copy of the document.
//! - Strict: not well-formed input fails with `malformed` or `truncated_file`; every
//!   event and error carries a byte offset, a 1-based line and a 1-based column.
//! - Encodings: UTF-8 (with or without BOM) and UTF-16 with BOM; anything else, declared
//!   or detected, fails with `xml_encoding_unsupported`.
//! - Never panics and never loops without consuming input: every outcome is a returned
//!   `Result`, and after an error the reader keeps returning that error.
//!
//! Positions: byte offsets and spans are offsets in the source bytes (so a span can be
//! copied verbatim from the original file); columns count bytes of the decoded UTF-8
//! text from the line start, so a UTF-16 document reports the same lines and columns as
//! its UTF-8 twin. The byte order mark is not counted in columns.
//!
//! Text, attribute values, comments and PI data are [`XmlText`] (untrusted, CONSTITUTION
//! C6). The XML declaration is checked but not reported.

mod chars;
mod error;
mod input;
mod reader;
mod text;

pub use error::{Construct, XmlError, XmlLimit, CODES};
pub use reader::{
    Attr, Element, EventKind, NsBinding, NsScope, QName, XmlEvent, XmlOptions, XmlReader, MAX_ATTRIBUTES, MAX_DEPTH,
    MAX_NAME_LEN, MAX_TEXT_LEN, XINCLUDE_NS, XMLNS_NS, XML_NS,
};
pub use text::XmlText;

use fid_io_reg::{BoundedSource, Budget, IoError, LimitRequest, Principal};

/// Reads every event of `data` under `budget` with the default limits.
pub fn read_slice(data: &[u8], budget: Budget) -> Result<Vec<XmlEvent>, XmlError> {
    read_slice_with(data, budget, XmlOptions::default())
}

/// Reads every event of `data` under `budget` with `options`.
pub fn read_slice_with(data: &[u8], budget: Budget, options: XmlOptions) -> Result<Vec<XmlEvent>, XmlError> {
    let mut reader = XmlReader::with_options(BoundedSource::new(data, budget), options);
    let mut out = Vec::new();
    while let Some(ev) = reader.next_event()? {
        out.push(ev);
    }
    Ok(out)
}

/// The budget the fuzz target and the in-tree mutation test use: lowered from the
/// defaults by a script principal so one input stays fast (1 MiB, 65,536 elements,
/// 16 MiB tracked allocation).
pub fn fuzz_budget() -> Result<Budget, IoError> {
    let request = LimitRequest::default().bytes(1 << 20).elements(65_536).alloc(16 << 20);
    Budget::new(Principal::Script, &request)
}
