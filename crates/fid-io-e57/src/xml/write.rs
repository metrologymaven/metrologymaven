//! The deterministic writer and the exporter's tree builder (REQ-005).
//!
//! # Byte form
//!
//! `<?xml version="1.0" encoding="UTF-8"?>`, LF, then the root
//! `<e57Root type="Structure" xmlns="E57" xmlns:p="uri" ...>` (extension namespaces in tree
//! order), LF, the children, `</e57Root>`, LF. An element at nesting level `d` (the root's
//! children are level 1) is indented by `2 d` spaces and ends with LF. Its start tag is the
//! name (`prefix:local`, or `vectorChild` inside a `Vector`), ` type="T"`, then the present
//! attributes of [`NodeType::attributes`] in that order. A container with children is
//! `<n ...>` LF, the children, the indented `</n>`; a `CompressedVector`'s children are
//! `prototype` then `codecs`. A leaf with a value is `<n ...>value</n>`. A container without
//! children, a number without a value, an empty string and a blob are `<n .../>`. Integers
//! are written in decimal (`{}`), reals in Rust's shortest round-trip exponent form (`{:e}`,
//! for example `1e0`, `-2.5e-3`, `-0e0`), booleans as `0`/`1`. Escaping is minimal. Text
//! escapes `&`, `<`, CR (`&#xD;`) and a `>` that follows `]]`. A namespace name is quoted
//! with `"`, or with `'` when it contains `"` but not `'`; only a value holding both quote
//! characters has its `"` escaped (`&quot;`); `&`, `<`, tab, LF and CR are escaped
//! (`&#x9;` `&#xA;` `&#xD;`).
//!
//! # Checks
//!
//! Before writing, the tree is checked against the reader's rules, so whatever the writer
//! writes parses back to an equal tree. A tree the reader accepted always passes these checks,
//! except the size bounds: escaping can make a string or the root tag longer than the reader's
//! text or token bound, and then the writer refuses with [`WriteError::LimitExceeded`] instead
//! of writing bytes the reader would refuse. The checks: the root rules of the scan layer
//! (`formatName`, `versionMajor`, `data3D`, the scans; [`WriteError::Content`]); every element's own rules (ranges, finite reals, scale not
//! zero, single precision within `f32`, no control character in strings); unique keys that
//! are NCNames, with a prefix declared in [`E57Tree::namespaces`]; homogeneous vectors unless
//! heterogeneous children are allowed; a `codecs` child that is a `Vector`; namespaces with an
//! NCName prefix other than `xml` and `xmlns`, unique prefixes and namespace names, a name
//! that is not empty, not the E57 or a reserved XML namespace and has no control character;
//! and the reader's bounds (depth, element count, text and token length, attributes,
//! namespaces, name and namespace-name length).

use std::collections::BTreeSet;
use std::fmt::{self, Write as _};

use fid_xml::{XmlText, MAX_ATTRIBUTES, MAX_NAMESPACES, MAX_NAME_LEN, MAX_URI_LEN, XMLNS_NS, XML_NS};

use super::lex::{check_leaf, is_ncname, is_string_char};
use super::scan::{describe, scan_desc, ScanDesc};
use super::tree::{Bound, E57Node, E57Tree, Key, Namespace, NodeType, XmlSectionError};
use super::{E57_NAMESPACE, MAX_DEPTH, MAX_ELEMENTS, MAX_TEXT_LEN};

/// The `formatName` the builder writes \[unverified against ASTM E2807\].
pub const FORMAT_NAME: &str = "ASTM E57 3D Imaging Data File";

/// Why the writer or the builder refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// The tree breaks a reader rule (`invalid_tree`). `element` is the 0-based index of the
    /// offending element in document order (the root is 0).
    Invalid {
        /// Static reason (the reader's reason codes).
        reason: &'static str,
        /// Element index.
        element: u64,
    },
    /// The tree passes a reader bound (`limit_exceeded`).
    LimitExceeded {
        /// The bound.
        bound: Bound,
        /// Its value.
        value: u64,
    },
    /// The tree's root or scan content breaks a rule of the scan layer (`invalid_tree`), with
    /// the reader's reason and the `data3D` child if the rule is about one.
    Content {
        /// Static reason.
        reason: &'static str,
        /// The `data3D` child.
        scan: Option<usize>,
    },
    /// Scan `index` cannot be written as given (`invalid_scan`): `codecs_not_writable` for a
    /// non-zero codec count, `scan_not_representable` if reading its element back would not
    /// give the same description, or the reader's reason for the element.
    Scan {
        /// Index in the scan list.
        index: usize,
        /// Static reason.
        reason: &'static str,
    },
}

impl WriteError {
    /// The stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            WriteError::Invalid { .. } | WriteError::Content { .. } => "invalid_tree",
            WriteError::LimitExceeded { .. } => "limit_exceeded",
            WriteError::Scan { .. } => "invalid_scan",
        }
    }
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::Invalid { reason, element } => write!(f, "invalid_tree: {reason} at element {element}"),
            WriteError::Content { reason, scan: Some(i) } => write!(f, "invalid_tree: {reason} in data3D child {i}"),
            WriteError::Content { reason, scan: None } => write!(f, "invalid_tree: {reason}"),
            WriteError::LimitExceeded { bound, value } => write!(f, "limit_exceeded: bound {bound} = {value}"),
            WriteError::Scan { index, reason } => write!(f, "invalid_scan: {reason} in scan {index}"),
        }
    }
}

impl std::error::Error for WriteError {}

/// The tree the exporter writes: root children `formatName` ([`FORMAT_NAME`]), `guid`
/// (`file_guid`), `versionMajor` 1, `versionMinor` 0, `data3D` (one element per scan, see
/// below) and an empty `images2D`, with the extension namespaces `namespaces` (needed for
/// prefixed prototype fields). Each scan's element has the children `guid`, `name`,
/// `description`, `pose`, `points` (prototype fields without values, empty `codecs`),
/// `cartesianBounds`, `sphericalBounds`, `intensityLimits`, `colorLimits` in that order,
/// absent options left out, reals as `Float` with `precision="double"`. A scan whose element
/// would not read back as the same description is refused ([`WriteError::Scan`]).
pub fn scan_tree(file_guid: &XmlText, namespaces: &[Namespace], scans: &[ScanDesc]) -> Result<E57Tree, WriteError> {
    let mut children = Vec::with_capacity(scans.len());
    for (index, s) in scans.iter().enumerate() {
        if s.codecs != 0 {
            return Err(WriteError::Scan { index, reason: "codecs_not_writable" });
        }
        let node = s.to_node();
        match scan_desc(&node) {
            Ok(d) if d == *s => children.push(node),
            Ok(_) => return Err(WriteError::Scan { index, reason: "scan_not_representable" }),
            Err(reason) => return Err(WriteError::Scan { index, reason }),
        }
    }
    let int = |v| E57Node::Integer { value: Some(v), minimum: None, maximum: None };
    let root = vec![
        (Key::new("formatName"), E57Node::String(XmlText::new(FORMAT_NAME))),
        (Key::new("guid"), E57Node::String(file_guid.clone())),
        (Key::new("versionMajor"), int(1)),
        (Key::new("versionMinor"), int(0)),
        (Key::new("data3D"), E57Node::Vector { allow_heterogeneous: None, children }),
        (Key::new("images2D"), E57Node::Vector { allow_heterogeneous: None, children: Vec::new() }),
    ];
    Ok(E57Tree { namespaces: namespaces.to_vec(), root })
}

struct Writer<'a> {
    out: String,
    prefixes: BTreeSet<&'a str>,
    elements: u64,
}

fn invalid(reason: &'static str, element: u64) -> WriteError {
    WriteError::Invalid { reason, element }
}

fn limit(bound: Bound, value: u64) -> WriteError {
    WriteError::LimitExceeded { bound, value }
}

/// Minimal text escaping: `&`, `<`, CR, and `>` only where it follows `]]` (so `]]>` never
/// appears in character data).
fn escape_text(out: &mut String, s: &str) {
    let (mut p1, mut p2) = ('\0', '\0');
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' if p1 == ']' && p2 == ']' => out.push_str("&gt;"),
            '\r' => out.push_str("&#xD;"),
            _ => out.push(c),
        }
        (p2, p1) = (p1, c);
    }
}

/// Writes ` name=` and the quoted value: quoted with `"` unless the value contains `"` and
/// not `'`, then with `'`; only a value holding both has its quote character escaped
/// (`&quot;`). `&`, `<`, tab, LF and CR are always escaped.
fn write_attr(out: &mut String, name: &str, s: &str) {
    let q = if s.contains('"') && !s.contains('\'') { '\'' } else { '"' };
    out.push(' ');
    out.push_str(name);
    out.push('=');
    out.push(q);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' if q == '"' => out.push_str("&quot;"),
            '\t' => out.push_str("&#x9;"),
            '\n' => out.push_str("&#xA;"),
            '\r' => out.push_str("&#xD;"),
            _ => out.push(c),
        }
    }
    out.push(q);
}

/// Serialises `tree` in the fixed byte form after checking it (see the module
/// documentation).
pub fn write_tree(tree: &E57Tree) -> Result<Vec<u8>, WriteError> {
    let mut w = Writer { out: String::new(), prefixes: BTreeSet::new(), elements: 1 };
    w.out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let tag_start = w.out.len();
    w.out.push_str("<e57Root type=\"Structure\" xmlns=\"");
    w.out.push_str(E57_NAMESPACE);
    w.out.push('"');
    if tree.namespaces.len() + 2 > MAX_ATTRIBUTES {
        return Err(limit(Bound::Attributes, MAX_ATTRIBUTES as u64));
    }
    if tree.namespaces.len() + 1 > MAX_NAMESPACES {
        return Err(limit(Bound::Namespaces, MAX_NAMESPACES as u64));
    }
    let mut uris: BTreeSet<&str> = BTreeSet::new();
    for ns in &tree.namespaces {
        let p = ns.prefix.as_str();
        let u = ns.uri.raw();
        if !is_ncname(p) || p == "xml" || p == "xmlns" {
            return Err(invalid("namespace_prefix", 0));
        }
        if u.is_empty() || u == E57_NAMESPACE || u == XML_NS || u == XMLNS_NS {
            return Err(invalid("namespace_uri", 0));
        }
        if !u.chars().all(is_string_char) {
            return Err(invalid("control_character", 0));
        }
        if u.len() > MAX_URI_LEN {
            return Err(limit(Bound::UriLength, MAX_URI_LEN as u64));
        }
        if p.len() + 6 > MAX_NAME_LEN {
            return Err(limit(Bound::NameLength, MAX_NAME_LEN as u64));
        }
        if !w.prefixes.insert(p) {
            return Err(invalid("namespace_prefix_duplicate", 0));
        }
        if !uris.insert(u) {
            return Err(invalid("namespace_uri_duplicate", 0));
        }
        write_attr(&mut w.out, &format!("xmlns:{p}"), u);
    }
    w.out.push_str(">\n");
    if w.out.len() - tag_start - 1 > MAX_TEXT_LEN {
        return Err(limit(Bound::TokenLength, MAX_TEXT_LEN as u64));
    }
    w.structure(&tree.root, 1, 0)?;
    w.out.push_str("</e57Root>\n");
    match describe(tree) {
        Ok(_) => {}
        Err(XmlSectionError::Content { reason, scan }) => return Err(WriteError::Content { reason, scan }),
        Err(_) => return Err(WriteError::Content { reason: "scan_layer", scan: None }),
    }
    Ok(w.out.into_bytes())
}

impl Writer<'_> {
    fn structure(&mut self, children: &[(Key, E57Node)], level: u32, me: u64) -> Result<(), WriteError> {
        let mut seen: BTreeSet<&Key> = BTreeSet::new();
        for (key, node) in children {
            if !seen.insert(key) {
                return Err(invalid("duplicate_key", me));
            }
            self.element(key, node, level)?;
        }
        Ok(())
    }

    fn element(&mut self, key: &Key, node: &E57Node, level: u32) -> Result<(), WriteError> {
        let me = self.elements;
        self.elements += 1;
        if self.elements > MAX_ELEMENTS {
            return Err(limit(Bound::Elements, MAX_ELEMENTS));
        }
        if level + 1 > MAX_DEPTH {
            return Err(limit(Bound::Depth, u64::from(MAX_DEPTH)));
        }
        if !is_ncname(&key.local) {
            return Err(invalid("element_name", me));
        }
        if let Some(p) = &key.prefix {
            if !self.prefixes.contains(p.as_str()) {
                return Err(invalid("element_namespace", me));
            }
        }
        let name = key.to_string();
        if name.len() > MAX_NAME_LEN {
            return Err(limit(Bound::NameLength, MAX_NAME_LEN as u64));
        }
        check_leaf(node).map_err(|r| invalid(r, me))?;
        let indent = (2 * level) as usize;
        for _ in 0..indent {
            self.out.push(' ');
        }
        let _ = write!(self.out, "<{name} type=\"{}\"", node.node_type().as_str());
        let mut attr = |n: &str, v: &dyn fmt::Display| {
            let _ = write!(self.out, " {n}=\"{v}\"");
        };
        let mut value = String::new();
        let mut kids = false;
        match node {
            E57Node::Structure(ch) => kids = !ch.is_empty(),
            E57Node::Vector { allow_heterogeneous, children } => {
                if let Some(a) = allow_heterogeneous {
                    attr("allowHeterogeneousChildren", &u8::from(*a));
                }
                kids = !children.is_empty();
            }
            E57Node::CompressedVector { file_offset, record_count, .. } => {
                attr("fileOffset", file_offset);
                attr("recordCount", record_count);
                kids = true;
            }
            E57Node::Integer { value: v, minimum, maximum } => {
                opt_attr(&mut attr, "minimum", minimum);
                opt_attr(&mut attr, "maximum", maximum);
                if let Some(v) = v {
                    value = v.to_string();
                }
            }
            E57Node::ScaledInteger { raw, minimum, maximum, scale, offset } => {
                opt_attr(&mut attr, "minimum", minimum);
                opt_attr(&mut attr, "maximum", maximum);
                opt_real(&mut attr, "scale", scale);
                opt_real(&mut attr, "offset", offset);
                if let Some(v) = raw {
                    value = v.to_string();
                }
            }
            E57Node::Float { value: v, precision, minimum, maximum } => {
                if let Some(p) = precision {
                    attr("precision", &p.as_str());
                }
                opt_real(&mut attr, "minimum", minimum);
                opt_real(&mut attr, "maximum", maximum);
                if let Some(v) = v {
                    value = format!("{v:e}");
                }
            }
            E57Node::String(t) => {
                escape_text(&mut value, t.raw());
                if value.len() > MAX_TEXT_LEN {
                    return Err(limit(Bound::TextLength, MAX_TEXT_LEN as u64));
                }
            }
            E57Node::Blob { file_offset, length } => {
                attr("fileOffset", file_offset);
                attr("length", length);
            }
        }
        if kids {
            self.out.push_str(">\n");
            match node {
                E57Node::Structure(ch) => self.structure(ch, level + 1, me)?,
                E57Node::Vector { allow_heterogeneous, children } => {
                    let first = children.first().map(E57Node::node_type);
                    let child = Key::new("vectorChild");
                    for c in children {
                        if *allow_heterogeneous != Some(true) && Some(c.node_type()) != first {
                            return Err(invalid("vector_heterogeneous", me));
                        }
                        self.element(&child, c, level + 1)?;
                    }
                }
                E57Node::CompressedVector { prototype, codecs, .. } => {
                    self.element(&Key::new("prototype"), prototype, level + 1)?;
                    if let Some(c) = codecs {
                        if c.node_type() != NodeType::Vector {
                            return Err(invalid("codecs_not_vector", me));
                        }
                        self.element(&Key::new("codecs"), c, level + 1)?;
                    }
                }
                _ => {}
            }
            for _ in 0..indent {
                self.out.push(' ');
            }
            let _ = writeln!(self.out, "</{name}>");
        } else if value.is_empty() {
            self.out.push_str("/>\n");
        } else {
            let _ = writeln!(self.out, ">{value}</{name}>");
        }
        Ok(())
    }
}

fn opt_attr(attr: &mut impl FnMut(&str, &dyn fmt::Display), n: &str, v: &Option<i64>) {
    if let Some(v) = v {
        attr(n, v);
    }
}

fn opt_real(attr: &mut impl FnMut(&str, &dyn fmt::Display), n: &str, v: &Option<f64>) {
    if let Some(v) = v {
        attr(n, &format!("{v:e}"));
    }
}
