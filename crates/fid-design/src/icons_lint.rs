//! `fid-design icons-lint --root <icons dir or svg file> [--json]` (spec node
//! `UX.ICONS.GRID`, REQ-UX.ICONS.GRID-001 .. -004). Tooling, nearest class R0; D0
//! (`BTreeMap`/`BTreeSet` only, sorted walks and findings, no clock, no network).
//!
//! # Rules enforced
//!
//! - Bounded UTF-8 XML parsing: at most 256 KiB per SVG (`MAX_SVG_BYTES`), depth at most 32,
//!   at most 4096 elements and 64 attributes per element, no DTD, no entity expansion (only the
//!   five XML built-in references `&lt;`, `&gt;`, `&amp;`, `&quot;`, `&apos;`).
//! - Allowed elements only: `svg`, `g`, `path`, `circle`, `rect`, `line`, `polyline`, `polygon`.
//!   Anything else (`text`, `image`, `script`, `style`, `defs`, `use`, `filter`, gradients, etc.)
//!   is `ICON-ELEMENT <name>`.
//! - No external references (`href`, `xlink:href`, `src`, `xmlns:xlink`, `<?xml-stylesheet?>`,
//!   `url(...)`, `http://`, `https://`, `data:`, `file://`, `javascript:`): `ICON-EXTERNAL-REF`.
//! - No literal colours in `fill` or `stroke`: only `currentColor` or `none` (plus optional
//!   `data-role="<token.name>"` naming a token role): `ICON-COLOR-LITERAL`.
//! - Allowed attributes per element only (no `style`, `class`, `transform`, `on*`, etc.);
//!   `stroke-width` must be `1` for 16 px masters and `1.5` for 24 px masters: `ICON-ATTR`.
//! - `viewBox` must be `"0 0 16 16"` for `.16.svg` masters and `"0 0 24 24"` for `.24.svg`
//!   masters (either when linting a standalone `.svg` file): `ICON-VIEWBOX`.
//! - Coordinates on `line`, `rect`, `circle`, `polyline`, `polygon` and absolute/relative `path`
//!   commands must sit on the 0.5 px grid within `1e-6`: `ICON-OFF-GRID`.
//! - Directory runs check that every icon has both `16` and `24` masters: `ICON-MASTER-MISSING`.
//!
//! Exit codes: 0 no findings, 1 findings, 2 unreadable input or bad usage.

use crate::finding::{findings_text, findings_value, read_text, Finding};
use crate::review::guard::{defects, path_defect};
use crate::review::walk;
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Report format ID.
pub const REPORT_FORMAT: &str = "fid-design-icons-lint@1";

/// Finding codes (UX.ICONS.GRID Design).
pub const ELEMENT: &str = "ICON-ELEMENT";
pub const ATTR: &str = "ICON-ATTR";
pub const COLOR_LITERAL: &str = "ICON-COLOR-LITERAL";
pub const OFF_GRID: &str = "ICON-OFF-GRID";
pub const VIEWBOX: &str = "ICON-VIEWBOX";
pub const MASTER_MISSING: &str = "ICON-MASTER-MISSING";
pub const EXTERNAL_REF: &str = "ICON-EXTERNAL-REF";

/// Largest SVG file accepted (256 KiB, UX.ICONS.GRID Build prompt).
pub const MAX_SVG_BYTES: u64 = 256 * 1024;
/// Deepest XML element nesting accepted.
pub const MAX_DEPTH: usize = 32;
/// Most XML elements in one SVG.
pub const MAX_ELEMENTS: usize = 4096;
/// Most attributes on one XML element.
pub const MAX_ATTRS: usize = 64;
/// Most path tokens parsed in one `d` attribute.
pub const MAX_PATH_TOKENS: usize = 8192;
/// Tolerance for the 0.5 px coordinate grid (UX.ICONS.GRID Design).
pub const GRID_EPS: f64 = 1e-6;

/// Allowed SVG element names (REQ-UX.ICONS.GRID-003).
pub const ALLOWED_ELEMENTS: [&str; 8] = ["circle", "g", "line", "path", "polygon", "polyline", "rect", "svg"];

const SVG_NS: &str = "http://www.w3.org/2000/svg";

/// Result of `fid-design icons-lint`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// SVG paths checked, sorted.
    pub files: Vec<String>,
    pub findings: Vec<Finding>,
}

/// Whether `v` is finite and within [`GRID_EPS`] of a multiple of `0.5`.
pub fn on_half_grid(v: f64) -> bool {
    v.is_finite() && (v - (v * 2.0).round() * 0.5).abs() <= GRID_EPS
}

/// Whether `s` is a valid token role name (`^[a-z][a-z0-9]*(\.[a-z0-9-]+)*$`, UX.SYSTEM.PKG).
pub fn valid_token_role(s: &str) -> bool {
    if s.is_empty() || s.len() > 128 {
        return false;
    }
    let mut parts = s.split('.');
    let Some(first) = parts.next() else {
        return false;
    };
    let fb = first.as_bytes();
    if fb.is_empty() || !fb[0].is_ascii_lowercase() || !fb.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        return false;
    }
    for seg in parts {
        let b = seg.as_bytes();
        if b.is_empty() || !b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-') {
            return false;
        }
    }
    true
}

#[derive(Debug, Clone)]
struct XmlElem {
    name: String,
    attrs: Vec<(String, String)>,
    parent: Option<usize>,
    /// Whether this element or an ancestor is forbidden (suppresses redundant child text errors).
    in_forbidden: bool,
}

enum XmlFailure {
    ExternalRef(String),
    Element(String, String),
    Attr(String),
}

struct XmlCursor<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> XmlCursor<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    fn rest(&self) -> &'a str {
        self.src.get(self.pos..).unwrap_or("")
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    fn peek(&self) -> Option<u8> {
        self.src.as_bytes().get(self.pos).copied()
    }

    fn starts_with(&self, s: &str) -> bool {
        self.rest().starts_with(s)
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn read_name(&mut self) -> Option<&'a str> {
        let start = self.pos;
        let bytes = self.src.as_bytes();
        while let Some(&b) = bytes.get(self.pos) {
            let ok = if self.pos == start {
                b.is_ascii_alphabetic() || b == b'_' || b == b':'
            } else {
                b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':')
            };
            if ok {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos > start {
            self.src.get(start..self.pos)
        } else {
            None
        }
    }

    fn read_attr_value(&mut self) -> Result<String, XmlFailure> {
        let quote = match self.peek() {
            Some(q @ (b'"' | b'\'')) => {
                self.pos += 1;
                q
            }
            _ => return Err(XmlFailure::Attr("attribute value must be quoted".to_string())),
        };
        let mut out = String::new();
        while !self.is_eof() {
            let b = match self.peek() {
                Some(b) => b,
                None => break,
            };
            if b == quote {
                self.pos += 1;
                return Ok(out);
            }
            if b == b'<' {
                return Err(XmlFailure::Attr("`<` is not allowed inside an attribute value".to_string()));
            }
            if b == b'&' {
                let rest = self.rest();
                if rest.starts_with("&lt;") {
                    out.push('<');
                    self.pos += 4;
                } else if rest.starts_with("&gt;") {
                    out.push('>');
                    self.pos += 4;
                } else if rest.starts_with("&amp;") {
                    out.push('&');
                    self.pos += 5;
                } else if rest.starts_with("&quot;") {
                    out.push('"');
                    self.pos += 6;
                } else if rest.starts_with("&apos;") {
                    out.push('\'');
                    self.pos += 6;
                } else {
                    return Err(XmlFailure::ExternalRef(
                        "entity reference or entity expansion is forbidden in icon SVGs".to_string(),
                    ));
                }
                continue;
            }
            let ch = match self.rest().chars().next() {
                Some(c) => c,
                None => break,
            };
            out.push(ch);
            self.pos += ch.len_utf8();
        }
        Err(XmlFailure::Attr("unterminated attribute value".to_string()))
    }
}

fn parse_svg_xml(src: &str) -> Result<Vec<XmlElem>, XmlFailure> {
    let mut cur = XmlCursor::new(src);
    let mut elems: Vec<XmlElem> = Vec::new();
    let mut stack: Vec<(String, usize, bool)> = Vec::new();
    let mut saw_root = false;

    cur.skip_ws();
    if cur.starts_with("<?xml") && cur.rest().as_bytes().get(5).is_some_and(|b| b.is_ascii_whitespace()) {
        let Some(end) = cur.rest().find("?>") else {
            return Err(XmlFailure::Element("xml".to_string(), "unterminated XML declaration".to_string()));
        };
        cur.pos += end + 2;
    }

    while !cur.is_eof() {
        if cur.peek() != Some(b'<') {
            let start = cur.pos;
            while !cur.is_eof() && cur.peek() != Some(b'<') {
                let ch = match cur.rest().chars().next() {
                    Some(c) => c,
                    None => break,
                };
                cur.pos += ch.len_utf8();
            }
            let text = cur.src.get(start..cur.pos).unwrap_or("");
            if !text.trim().is_empty() {
                let in_forbidden = stack.last().is_some_and(|(_, _, forb)| *forb);
                if !in_forbidden {
                    return Err(XmlFailure::Element(
                        "#text".to_string(),
                        "character text content is not allowed in icon SVGs".to_string(),
                    ));
                }
            }
            continue;
        }

        if cur.starts_with("<!--") {
            let body_start = cur.pos + 4;
            let Some(rel_end) = cur.src.get(body_start..).and_then(|s| s.find("-->")) else {
                return Err(XmlFailure::Element("comment".to_string(), "unterminated XML comment".to_string()));
            };
            let body = cur.src.get(body_start..body_start + rel_end).unwrap_or("");
            if body.contains("--") {
                return Err(XmlFailure::Element(
                    "comment".to_string(),
                    "`--` is not allowed in XML comments".to_string(),
                ));
            }
            cur.pos = body_start + rel_end + 3;
            continue;
        }

        if cur.starts_with("<!") {
            let upper_prefix: String = cur.rest().chars().take(16).collect::<String>().to_ascii_uppercase();
            if upper_prefix.starts_with("<!DOCTYPE") || upper_prefix.starts_with("<!ENTITY") {
                return Err(XmlFailure::ExternalRef(
                    "DTD and entity declarations (`<!DOCTYPE` / `<!ENTITY`) are forbidden".to_string(),
                ));
            }
            return Err(XmlFailure::Element(
                "declaration".to_string(),
                "markup declarations and CDATA sections are forbidden in icon SVGs".to_string(),
            ));
        }

        if cur.starts_with("<?") {
            let rest = cur.rest();
            let lower: String = rest.chars().take(20).collect::<String>().to_ascii_lowercase();
            if lower.starts_with("<?xml-stylesheet") {
                return Err(XmlFailure::ExternalRef("`<?xml-stylesheet?>` is forbidden".to_string()));
            }
            return Err(XmlFailure::Element(
                "processing-instruction".to_string(),
                "processing instructions are forbidden in icon SVGs".to_string(),
            ));
        }

        if cur.starts_with("</") {
            cur.pos += 2;
            let Some(name) = cur.read_name() else {
                return Err(XmlFailure::Element("end-tag".to_string(), "malformed closing tag".to_string()));
            };
            cur.skip_ws();
            if cur.peek() != Some(b'>') {
                return Err(XmlFailure::Element(name.to_string(), "expected `>` in closing tag".to_string()));
            }
            cur.pos += 1;
            match stack.pop() {
                Some((open, _, _)) if open == name => {}
                Some((open, _, _)) => {
                    return Err(XmlFailure::Element(
                        name.to_string(),
                        format!("mismatched closing tag `</{name}>` for `<{open}>`"),
                    ));
                }
                None => {
                    return Err(XmlFailure::Element(name.to_string(), format!("unexpected closing tag `</{name}>`")));
                }
            }
            continue;
        }

        // Start tag `<name ...>` or `<name .../>`.
        cur.pos += 1;
        let Some(name_slice) = cur.read_name() else {
            return Err(XmlFailure::Element("start-tag".to_string(), "malformed opening tag".to_string()));
        };
        let name = name_slice.to_string();
        if stack.is_empty() {
            if saw_root {
                return Err(XmlFailure::Element(name, "multiple root elements".to_string()));
            }
            saw_root = true;
        }
        if stack.len() >= MAX_DEPTH {
            return Err(XmlFailure::Element(name, format!("element nesting exceeds {MAX_DEPTH}")));
        }
        if elems.len() >= MAX_ELEMENTS {
            return Err(XmlFailure::Element(name, format!("more than {MAX_ELEMENTS} elements")));
        }

        let mut attrs: Vec<(String, String)> = Vec::new();
        let mut seen_attrs: BTreeSet<String> = BTreeSet::new();
        let self_closing;
        loop {
            let before_ws = cur.pos;
            cur.skip_ws();
            if cur.starts_with("/>") {
                cur.pos += 2;
                self_closing = true;
                break;
            }
            if cur.peek() == Some(b'>') {
                cur.pos += 1;
                self_closing = false;
                break;
            }
            if cur.is_eof() {
                return Err(XmlFailure::Element(name, "unterminated opening tag".to_string()));
            }
            if cur.pos == before_ws {
                return Err(XmlFailure::Attr(format!("<{name}>: expected whitespace between attributes")));
            }
            if attrs.len() >= MAX_ATTRS {
                return Err(XmlFailure::Attr(format!("<{name}>: more than {MAX_ATTRS} attributes")));
            }
            let Some(aname) = cur.read_name() else {
                return Err(XmlFailure::Attr(format!("<{name}>: malformed attribute name")));
            };
            let aname = aname.to_string();
            cur.skip_ws();
            if cur.peek() != Some(b'=') {
                return Err(XmlFailure::Attr(format!("<{name}>: attribute `{aname}` missing `=`")));
            }
            cur.pos += 1;
            cur.skip_ws();
            let aval = cur.read_attr_value()?;
            if !seen_attrs.insert(aname.clone()) {
                return Err(XmlFailure::Attr(format!("<{name}>: duplicate attribute `{aname}`")));
            }
            attrs.push((aname, aval));
        }

        let parent = stack.last().map(|(_, idx, _)| *idx);
        let parent_forb = stack.last().is_some_and(|(_, _, forb)| *forb);
        let is_forb = parent_forb || !ALLOWED_ELEMENTS.contains(&name.as_str());
        let idx = elems.len();
        elems.push(XmlElem { name: name.clone(), attrs, parent, in_forbidden: is_forb });
        if !self_closing {
            stack.push((name, idx, is_forb));
        }
    }

    if let Some((open, _, _)) = stack.last() {
        return Err(XmlFailure::Element(open.clone(), format!("unclosed element `<{open}>`")));
    }
    if elems.is_empty() {
        return Err(XmlFailure::Element("svg".to_string(), "empty SVG document".to_string()));
    }
    Ok(elems)
}

fn contains_external_scheme(val: &str) -> bool {
    let low = val.to_ascii_lowercase();
    low.contains("url(")
        || low.contains("http://")
        || low.contains("https://")
        || low.contains("data:")
        || low.contains("file://")
        || low.contains("javascript:")
}

fn attr_allowed(elem: &str, attr: &str) -> bool {
    let common = matches!(
        attr,
        "fill"
            | "stroke"
            | "stroke-width"
            | "stroke-linecap"
            | "stroke-linejoin"
            | "stroke-miterlimit"
            | "fill-rule"
            | "clip-rule"
            | "fill-opacity"
            | "stroke-opacity"
            | "opacity"
            | "data-role"
            | "aria-hidden"
            | "focusable"
    );
    if common {
        return true;
    }
    match elem {
        "svg" => matches!(attr, "xmlns" | "viewBox" | "width" | "height" | "role"),
        "g" => false,
        "path" => attr == "d",
        "circle" => matches!(attr, "cx" | "cy" | "r"),
        "rect" => matches!(attr, "x" | "y" | "width" | "height" | "rx" | "ry"),
        "line" => matches!(attr, "x1" | "y1" | "x2" | "y2"),
        "polyline" | "polygon" => attr == "points",
        _ => false,
    }
}

fn parse_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let v: f64 = t.parse().ok()?;
    v.is_finite().then_some(v)
}

fn parse_size_attr(s: &str) -> Option<f64> {
    let t = s.trim();
    let num = t.strip_suffix("px").unwrap_or(t);
    parse_number(num)
}

fn parse_viewbox(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() != 4 {
        return None;
    }
    let x = parse_number(parts[0])?;
    let y = parse_number(parts[1])?;
    let w = parse_number(parts[2])?;
    let h = parse_number(parts[3])?;
    if x == 0.0 && y == 0.0 && w == 16.0 && h == 16.0 {
        Some(16)
    } else if x == 0.0 && y == 0.0 && w == 24.0 && h == 24.0 {
        Some(24)
    } else {
        None
    }
}

fn parse_points(s: &str) -> Result<Vec<(f64, f64)>, String> {
    let mut nums = Vec::new();
    let mut cur = PathCursor::new(s);
    cur.skip_ws();
    while !cur.is_eof() {
        if nums.len() >= MAX_PATH_TOKENS {
            return Err(format!("`points` exceeds {MAX_PATH_TOKENS} numbers"));
        }
        let Some(v) = cur.read_number() else {
            return Err("invalid number in `points`".to_string());
        };
        nums.push(v);
        cur.skip_comma_ws();
    }
    if nums.is_empty() || nums.len() % 2 != 0 {
        return Err("`points` must hold a non-empty even list of coordinates".to_string());
    }
    let mut out = Vec::with_capacity(nums.len() / 2);
    let mut i = 0;
    while i < nums.len() {
        out.push((nums[i], nums[i + 1]));
        i += 2;
    }
    Ok(out)
}

struct PathCursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> PathCursor<'a> {
    fn new(s: &'a str) -> Self {
        Self { bytes: s.as_bytes(), pos: 0 }
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn skip_comma_ws(&mut self) {
        self.skip_ws();
        if self.peek() == Some(b',') {
            self.pos += 1;
            self.skip_ws();
        }
    }

    fn starts_number(&self) -> bool {
        matches!(self.peek(), Some(b'+' | b'-' | b'.' | b'0'..=b'9'))
    }

    fn read_flag(&mut self) -> Option<u8> {
        self.skip_comma_ws();
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                Some(0)
            }
            Some(b'1') => {
                self.pos += 1;
                Some(1)
            }
            _ => None,
        }
    }

    fn read_number(&mut self) -> Option<f64> {
        self.skip_comma_ws();
        let start = self.pos;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        let mut digits = 0usize;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
            digits += 1;
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            self.pos = start;
            return None;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let exp_start = self.pos;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            let mut exp_digits = 0usize;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
                exp_digits += 1;
            }
            if exp_digits == 0 {
                self.pos = exp_start;
            }
        }
        let slice = std::str::from_utf8(self.bytes.get(start..self.pos)?).ok()?;
        let v: f64 = slice.parse().ok()?;
        v.is_finite().then_some(v)
    }
}

/// Parses an SVG `d` path attribute and collects every anchor coordinate `(x, y)` resolved
/// from absolute and relative commands (`M/m`, `L/l`, `H/h`, `V/v`, `C/c`, `S/s`, `Q/q`, `T/t`,
/// `A/a`, `Z/z`).
pub fn parse_path_anchors(d: &str) -> Result<Vec<(f64, f64)>, String> {
    let mut cur = PathCursor::new(d);
    let mut anchors: Vec<(f64, f64)> = Vec::new();
    let mut cx = 0.0_f64;
    let mut cy = 0.0_f64;
    let mut sx = 0.0_f64;
    let mut sy = 0.0_f64;
    let mut cmd: Option<u8> = None;
    let mut tokens = 0usize;

    cur.skip_ws();
    if cur.is_eof() {
        return Err("path `d` is empty".to_string());
    }

    while !cur.is_eof() {
        tokens += 1;
        if tokens > MAX_PATH_TOKENS {
            return Err(format!("path `d` exceeds {MAX_PATH_TOKENS} segments"));
        }
        let b = match cur.peek() {
            Some(b) => b,
            None => break,
        };
        let active = if b.is_ascii_alphabetic() {
            cur.pos += 1;
            if cmd.is_none() && !matches!(b, b'M' | b'm') {
                return Err(format!("path `d` must begin with `M` or `m`, found `{}`", char::from(b)));
            }
            cmd = Some(b);
            b
        } else if let Some(prev) = cmd {
            // Implicit repeated command (`M` repeats as `L`, `m` repeats as `l`).
            match prev {
                b'M' => b'L',
                b'm' => b'l',
                b'Z' | b'z' => return Err("unexpected coordinate after `Z`/`z`".to_string()),
                other => other,
            }
        } else {
            return Err("path `d` must begin with a command letter".to_string());
        };

        match active {
            b'M' | b'm' => {
                let x = cur.read_number().ok_or_else(|| "missing x in M/m".to_string())?;
                let y = cur.read_number().ok_or_else(|| "missing y in M/m".to_string())?;
                if active == b'M' {
                    cx = x;
                    cy = y;
                } else {
                    cx += x;
                    cy += y;
                }
                sx = cx;
                sy = cy;
                anchors.push((cx, cy));
            }
            b'L' | b'l' | b'T' | b't' => {
                let x = cur.read_number().ok_or_else(|| "missing x in L/l/T/t".to_string())?;
                let y = cur.read_number().ok_or_else(|| "missing y in L/l/T/t".to_string())?;
                if active.is_ascii_uppercase() {
                    cx = x;
                    cy = y;
                } else {
                    cx += x;
                    cy += y;
                }
                anchors.push((cx, cy));
            }
            b'H' | b'h' => {
                let x = cur.read_number().ok_or_else(|| "missing x in H/h".to_string())?;
                if active == b'H' {
                    cx = x;
                } else {
                    cx += x;
                }
                anchors.push((cx, cy));
            }
            b'V' | b'v' => {
                let y = cur.read_number().ok_or_else(|| "missing y in V/v".to_string())?;
                if active == b'V' {
                    cy = y;
                } else {
                    cy += y;
                }
                anchors.push((cx, cy));
            }
            b'C' | b'c' => {
                let _x1 = cur.read_number().ok_or_else(|| "missing x1 in C/c".to_string())?;
                let _y1 = cur.read_number().ok_or_else(|| "missing y1 in C/c".to_string())?;
                let _x2 = cur.read_number().ok_or_else(|| "missing x2 in C/c".to_string())?;
                let _y2 = cur.read_number().ok_or_else(|| "missing y2 in C/c".to_string())?;
                let x = cur.read_number().ok_or_else(|| "missing x in C/c".to_string())?;
                let y = cur.read_number().ok_or_else(|| "missing y in C/c".to_string())?;
                if active == b'C' {
                    cx = x;
                    cy = y;
                } else {
                    cx += x;
                    cy += y;
                }
                anchors.push((cx, cy));
            }
            b'S' | b's' | b'Q' | b'q' => {
                let _x1 = cur.read_number().ok_or_else(|| "missing x1 in S/s/Q/q".to_string())?;
                let _y1 = cur.read_number().ok_or_else(|| "missing y1 in S/s/Q/q".to_string())?;
                let x = cur.read_number().ok_or_else(|| "missing x in S/s/Q/q".to_string())?;
                let y = cur.read_number().ok_or_else(|| "missing y in S/s/Q/q".to_string())?;
                if active.is_ascii_uppercase() {
                    cx = x;
                    cy = y;
                } else {
                    cx += x;
                    cy += y;
                }
                anchors.push((cx, cy));
            }
            b'A' | b'a' => {
                let _rx = cur.read_number().ok_or_else(|| "missing rx in A/a".to_string())?;
                let _ry = cur.read_number().ok_or_else(|| "missing ry in A/a".to_string())?;
                let _rot = cur.read_number().ok_or_else(|| "missing rotation in A/a".to_string())?;
                let _large = cur.read_flag().ok_or_else(|| "invalid large-arc flag in A/a".to_string())?;
                let _sweep = cur.read_flag().ok_or_else(|| "invalid sweep flag in A/a".to_string())?;
                let x = cur.read_number().ok_or_else(|| "missing x in A/a".to_string())?;
                let y = cur.read_number().ok_or_else(|| "missing y in A/a".to_string())?;
                if active == b'A' {
                    cx = x;
                    cy = y;
                } else {
                    cx += x;
                    cy += y;
                }
                anchors.push((cx, cy));
            }
            b'Z' | b'z' => {
                cx = sx;
                cy = sy;
            }
            other => return Err(format!("unknown path command `{}`", char::from(other))),
        }

        cur.skip_ws();
        if cur.peek() == Some(b',') {
            cur.pos += 1;
            cur.skip_ws();
            if !cur.starts_number() {
                return Err("dangling comma in path `d`".to_string());
            }
        }
    }

    Ok(anchors)
}

fn master_from_name(rel: &str) -> (String, Option<u32>) {
    if let Some(stem) = rel.strip_suffix(".16.svg") {
        (stem.to_string(), Some(16))
    } else if let Some(stem) = rel.strip_suffix(".24.svg") {
        (stem.to_string(), Some(24))
    } else if let Some(stem) = rel.strip_suffix(".svg") {
        (stem.to_string(), None)
    } else {
        (rel.to_string(), None)
    }
}

fn check_grid_coord(file: &str, what: &str, v: f64, f: &mut Vec<Finding>) {
    if !on_half_grid(v) {
        f.push(Finding::new(OFF_GRID, file, format!("{what} coordinate {v} is not on the 0.5 px grid")));
    }
}

/// Lints one SVG document held in memory. Returns the detected master size (`16` or `24`) when
/// the root `viewBox` is valid.
pub fn lint_svg_str(file: &str, text: &str, expected_master: Option<u32>, f: &mut Vec<Finding>) -> Option<u32> {
    let elems = match parse_svg_xml(text) {
        Ok(e) => e,
        Err(XmlFailure::ExternalRef(msg)) => {
            f.push(Finding::new(EXTERNAL_REF, file, msg));
            return None;
        }
        Err(XmlFailure::Element(tag, msg)) => {
            f.push(Finding::new(ELEMENT, tag, format!("{file}: {msg}")));
            return None;
        }
        Err(XmlFailure::Attr(msg)) => {
            f.push(Finding::new(ATTR, file, msg));
            return None;
        }
    };

    let root = &elems[0];
    if root.name != "svg" {
        f.push(Finding::new(ELEMENT, &root.name, format!("{file}: root element must be <svg>")));
    }

    // Determine viewBox / master size from root <svg>.
    let mut detected_master: Option<u32> = None;
    let mut saw_viewbox = false;
    for (k, v) in &root.attrs {
        if k == "viewBox" {
            saw_viewbox = true;
            match parse_viewbox(v) {
                Some(sz) => {
                    detected_master = Some(sz);
                    if let Some(exp) = expected_master {
                        if sz != exp {
                            f.push(Finding::new(
                                VIEWBOX,
                                file,
                                format!("viewBox {v:?} does not match expected {exp} px master (\"0 0 {exp} {exp}\")"),
                            ));
                        }
                    }
                }
                None => {
                    f.push(Finding::new(
                        VIEWBOX,
                        file,
                        format!("viewBox {v:?} must be \"0 0 16 16\" or \"0 0 24 24\""),
                    ));
                }
            }
        }
    }
    if !saw_viewbox {
        f.push(Finding::new(VIEWBOX, file, "root <svg> is missing required `viewBox`"));
    }

    let active_master = expected_master.or(detected_master);
    let expected_stroke_width = match active_master {
        Some(16) => Some(1.0_f64),
        Some(24) => Some(1.5_f64),
        _ => None,
    };

    // Track inherited stroke and stroke-width down the element tree.
    let mut eff_stroke: Vec<Option<String>> = vec![None; elems.len()];
    let mut eff_stroke_width: Vec<Option<f64>> = vec![None; elems.len()];

    for (idx, el) in elems.iter().enumerate() {
        let mut cur_stroke = el.parent.and_then(|p| eff_stroke[p].clone());
        let mut cur_sw = el.parent.and_then(|p| eff_stroke_width[p]);

        if !ALLOWED_ELEMENTS.contains(&el.name.as_str()) {
            f.push(Finding::new(
                ELEMENT,
                &el.name,
                format!("{file}: element <{}> is not allowed in the icon SVG profile", el.name),
            ));
        }

        let mut attr_map: BTreeMap<&str, &str> = BTreeMap::new();
        for (k, v) in &el.attrs {
            if !defects(k).is_empty() || !defects(v).is_empty() {
                f.push(Finding::new(ATTR, file, format!("<{}>: refused untrusted text in attribute `{k}`", el.name)));
            }
            let k_low = k.to_ascii_lowercase();
            if matches!(k_low.as_str(), "href" | "xlink:href" | "src" | "xmlns:xlink") {
                f.push(Finding::new(
                    EXTERNAL_REF,
                    file,
                    format!("<{}>: external reference attribute `{k}` is forbidden", el.name),
                ));
                continue;
            }
            if k == "xmlns" {
                if v != SVG_NS {
                    f.push(Finding::new(
                        EXTERNAL_REF,
                        file,
                        format!("<{}>: unexpected XML namespace {v:?}; only {SVG_NS:?} is allowed", el.name),
                    ));
                }
                attr_map.insert(k.as_str(), v.as_str());
                continue;
            }
            if contains_external_scheme(v) {
                f.push(Finding::new(
                    EXTERNAL_REF,
                    file,
                    format!("<{}>: attribute `{k}` contains an external or URL reference", el.name),
                ));
                continue;
            }
            if k_low.starts_with("on") || matches!(k_low.as_str(), "style" | "class" | "transform") {
                f.push(Finding::new(ATTR, file, format!("<{}>: forbidden attribute `{k}`", el.name)));
                continue;
            }
            if el.in_forbidden {
                continue;
            }
            if !attr_allowed(&el.name, k) {
                f.push(Finding::new(
                    ATTR,
                    file,
                    format!("<{}>: attribute `{k}` is not allowed in the icon SVG profile", el.name),
                ));
                continue;
            }
            attr_map.insert(k.as_str(), v.as_str());

            match k.as_str() {
                "fill" | "stroke" => {
                    if v != "none" && v != "currentColor" {
                        f.push(Finding::new(
                            COLOR_LITERAL,
                            file,
                            format!(
                                "<{}>: `{k}` uses literal colour {v:?}; use `currentColor` or `none` (with optional `data-role`)",
                                el.name
                            ),
                        ));
                    }
                    if k == "stroke" {
                        cur_stroke = Some(v.clone());
                    }
                }
                "data-role" => {
                    if v.starts_with('#')
                        || v.to_ascii_lowercase().starts_with("rgb(")
                        || v.to_ascii_lowercase().starts_with("hsl(")
                    {
                        f.push(Finding::new(
                            COLOR_LITERAL,
                            file,
                            format!("<{}>: `data-role` must name a token role, not literal colour {v:?}", el.name),
                        ));
                    } else if !valid_token_role(v) {
                        f.push(Finding::new(
                            ATTR,
                            file,
                            format!("<{}>: `data-role` {v:?} is not a valid token role name", el.name),
                        ));
                    }
                }
                "stroke-width" => match parse_size_attr(v) {
                    Some(sw) => {
                        cur_sw = Some(sw);
                        if let Some(exp) = expected_stroke_width {
                            if (sw - exp).abs() > GRID_EPS {
                                f.push(Finding::new(
                                    ATTR,
                                    file,
                                    format!("<{}>: `stroke-width` {v:?} must be {exp} for this master size", el.name),
                                ));
                            }
                        }
                    }
                    None => f.push(Finding::new(
                        ATTR,
                        file,
                        format!("<{}>: `stroke-width` {v:?} is not a finite number", el.name),
                    )),
                },
                "width" | "height" if el.name == "svg" => match (parse_size_attr(v), active_master) {
                    (Some(n), Some(exp)) if (n - f64::from(exp)).abs() <= GRID_EPS => {}
                    _ => f.push(Finding::new(VIEWBOX, file, format!("<svg>: `{k}` {v:?} does not match master size"))),
                },
                _ => {}
            }
        }

        eff_stroke[idx] = cur_stroke.clone();
        eff_stroke_width[idx] = cur_sw;

        if el.in_forbidden {
            continue;
        }

        let is_shape = matches!(el.name.as_str(), "path" | "circle" | "rect" | "line" | "polyline" | "polygon");
        if is_shape && cur_stroke.as_deref() == Some("currentColor") && cur_sw.is_none() {
            if let Some(exp) = expected_stroke_width {
                if (1.0 - exp).abs() > GRID_EPS {
                    f.push(Finding::new(
                        ATTR,
                        file,
                        format!("<{}>: stroked element needs `stroke-width=\"{exp}\"` for this master", el.name),
                    ));
                }
            }
        }

        // Geometry & 0.5 px grid checks.
        match el.name.as_str() {
            "line" => {
                for key in ["x1", "y1", "x2", "y2"] {
                    let val = attr_map.get(key).copied().unwrap_or("0");
                    match parse_number(val) {
                        Some(n) => check_grid_coord(file, &format!("<line {key}>"), n, f),
                        None => f.push(Finding::new(ATTR, file, format!("<line>: invalid `{key}` {val:?}"))),
                    }
                }
            }
            "circle" => {
                for key in ["cx", "cy"] {
                    let val = attr_map.get(key).copied().unwrap_or("0");
                    match parse_number(val) {
                        Some(n) => check_grid_coord(file, &format!("<circle {key}>"), n, f),
                        None => f.push(Finding::new(ATTR, file, format!("<circle>: invalid `{key}` {val:?}"))),
                    }
                }
                match attr_map.get("r").copied().and_then(parse_number) {
                    Some(r) if r >= 0.0 => check_grid_coord(file, "<circle r>", r, f),
                    _ => f.push(Finding::new(ATTR, file, "<circle>: missing or invalid `r`")),
                }
            }
            "rect" => {
                let xv = attr_map.get("x").copied().unwrap_or("0");
                let x = match parse_number(xv) {
                    Some(n) => {
                        check_grid_coord(file, "<rect x>", n, f);
                        Some(n)
                    }
                    None => {
                        f.push(Finding::new(ATTR, file, format!("<rect>: invalid `x` {xv:?}")));
                        None
                    }
                };
                let yv = attr_map.get("y").copied().unwrap_or("0");
                let y = match parse_number(yv) {
                    Some(n) => {
                        check_grid_coord(file, "<rect y>", n, f);
                        Some(n)
                    }
                    None => {
                        f.push(Finding::new(ATTR, file, format!("<rect>: invalid `y` {yv:?}")));
                        None
                    }
                };
                let w = match attr_map.get("width").copied().and_then(parse_number) {
                    Some(n) if n >= 0.0 => {
                        check_grid_coord(file, "<rect width>", n, f);
                        Some(n)
                    }
                    _ => {
                        f.push(Finding::new(ATTR, file, "<rect>: missing or invalid `width`"));
                        None
                    }
                };
                let h = match attr_map.get("height").copied().and_then(parse_number) {
                    Some(n) if n >= 0.0 => {
                        check_grid_coord(file, "<rect height>", n, f);
                        Some(n)
                    }
                    _ => {
                        f.push(Finding::new(ATTR, file, "<rect>: missing or invalid `height`"));
                        None
                    }
                };
                if let (Some(x), Some(w)) = (x, w) {
                    check_grid_coord(file, "<rect x+width>", x + w, f);
                }
                if let (Some(y), Some(h)) = (y, h) {
                    check_grid_coord(file, "<rect y+height>", y + h, f);
                }
                for key in ["rx", "ry"] {
                    if let Some(v) = attr_map.get(key).copied() {
                        match parse_number(v) {
                            Some(n) if n >= 0.0 => check_grid_coord(file, &format!("<rect {key}>"), n, f),
                            _ => f.push(Finding::new(ATTR, file, format!("<rect>: invalid `{key}` {v:?}"))),
                        }
                    }
                }
            }
            "polyline" | "polygon" => match attr_map.get("points").copied() {
                Some(pts) => match parse_points(pts) {
                    Ok(pairs) => {
                        for (x, y) in pairs {
                            check_grid_coord(file, &format!("<{} x>", el.name), x, f);
                            check_grid_coord(file, &format!("<{} y>", el.name), y, f);
                        }
                    }
                    Err(e) => f.push(Finding::new(ATTR, file, format!("<{}>: {e}", el.name))),
                },
                None => f.push(Finding::new(ATTR, file, format!("<{}>: missing `points`", el.name))),
            },
            "path" => match attr_map.get("d").copied() {
                Some(d) => match parse_path_anchors(d) {
                    Ok(anchors) => {
                        for (x, y) in anchors {
                            check_grid_coord(file, "<path x>", x, f);
                            check_grid_coord(file, "<path y>", y, f);
                        }
                    }
                    Err(e) => f.push(Finding::new(ATTR, file, format!("<path>: {e}"))),
                },
                None => f.push(Finding::new(ATTR, file, "<path>: missing `d`")),
            },
            _ => {}
        }
    }

    detected_master
}

/// Runs `fid-design icons-lint` against `<root>` (either an icons directory or a single `.svg`
/// file). `Err` is an unreadable input or bad path (exit 2).
pub fn check(root: &Path) -> Result<Report, String> {
    let md = std::fs::symlink_metadata(root).map_err(|e| format!("cannot read --root {}: {e}", root.display()))?;
    if md.file_type().is_symlink() {
        return Err(format!("--root {} is a symbolic link; not followed", root.display()));
    }

    let mut f = Vec::new();
    let mut files = Vec::new();

    if md.is_file() {
        let name = root
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("--root {} has a non-UTF-8 file name", root.display()))?
            .to_string();
        let text = read_text(root, MAX_SVG_BYTES)?;
        let (_, expected_master) = master_from_name(&name);
        lint_svg_str(&name, &text, expected_master, &mut f);
        files.push(name);
    } else if md.is_dir() {
        let mut wf = Vec::new();
        let entries = walk::walk(root, "", &mut wf)?;
        f.extend(wf.into_iter().map(|w| Finding::new(ATTR, w.subject, w.detail)));

        let mut masters: BTreeMap<String, (bool, bool)> = BTreeMap::new();
        for e in entries {
            if e.is_dir || !e.rel.to_ascii_lowercase().ends_with(".svg") {
                continue;
            }
            if let Some(d) = path_defect(&e.rel) {
                f.push(Finding::new(ATTR, &e.rel, format!("icon path refused: {d}")));
                continue;
            }
            let text = read_text(&e.path, MAX_SVG_BYTES)?;
            let (stem, expected_master) = master_from_name(&e.rel);
            let detected = lint_svg_str(&e.rel, &text, expected_master, &mut f);
            let slot = masters.entry(stem).or_insert((false, false));
            match expected_master.or(detected) {
                Some(16) => slot.0 = true,
                Some(24) => slot.1 = true,
                _ => {}
            }
            files.push(e.rel);
        }

        for (stem, (has_16, has_24)) in masters {
            if !has_16 {
                f.push(Finding::new(MASTER_MISSING, "16", format!("{stem}: missing 16 px master ({stem}.16.svg)")));
            }
            if !has_24 {
                f.push(Finding::new(MASTER_MISSING, "24", format!("{stem}: missing 24 px master ({stem}.24.svg)")));
            }
        }
    } else {
        return Err(format!("--root {} is not a regular file or directory", root.display()));
    }

    files.sort();
    f.sort();
    f.dedup();
    Ok(Report { files, findings: f })
}

impl Report {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("format".to_string(), CValue::Str(REPORT_FORMAT.to_string()));
        m.insert("files".to_string(), CValue::Arr(self.files.iter().map(|p| CValue::Str(p.clone())).collect()));
        m.insert("findings".to_string(), findings_value(&self.findings));
        m.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let mut s = format!("fid-design icons-lint: {} file(s)\n", self.files.len());
        findings_text(&mut s, &self.findings);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_grid_and_roles() {
        assert!(on_half_grid(0.0));
        assert!(on_half_grid(0.5));
        assert!(on_half_grid(12.5));
        assert!(on_half_grid(3.5 + 5e-7));
        assert!(!on_half_grid(3.3));
        assert!(!on_half_grid(3.5 + 2e-6));
        assert!(!on_half_grid(f64::NAN));
        assert!(!on_half_grid(f64::INFINITY));

        assert!(valid_token_role("sem.icon.primary"));
        assert!(valid_token_role("sem.status.out-of-tolerance"));
        assert!(!valid_token_role(""));
        assert!(!valid_token_role("#ff0000"));
        assert!(!valid_token_role("Sem.Icon"));
        assert!(!valid_token_role("sem..icon"));
    }

    #[test]
    fn path_parser_absolute_relative_and_compact_arcs() {
        let pts = parse_path_anchors("M2.5 3.5l4-1.5h2v3.5a2 2 0 012 2Z").ok();
        assert_eq!(pts, Some(vec![(2.5, 3.5), (6.5, 2.0), (8.5, 2.0), (8.5, 5.5), (10.5, 7.5)]));
        let curves = parse_path_anchors("M1 1C1.1 2.2 3.3 4.4 5 5.5s1 1 2 2q.5.5 1 1t1 1").ok();
        assert_eq!(curves, Some(vec![(1.0, 1.0), (5.0, 5.5), (7.0, 7.5), (8.0, 8.5), (9.0, 9.5)]));
        assert!(parse_path_anchors("L1 1").is_err());
        assert!(parse_path_anchors("").is_err());
        assert!(parse_path_anchors("M1 1 A2 2 0 2 0 4 4").is_err());
    }
}
