//! Per-edition docs search index with an exact command-name table (spec node
//! `OUTR.DOCS.SEARCH.INDEX`).
//!
//! `search-index --edition <dir> --budget <file>` reads every `.html` page of one release
//! edition and writes `search-index.js` at the edition root:
//!
//! ```text
//! window.FID_SEARCH_INDEX = {"docs":[{"h":[..],"t":..,"u":..}],"names":{..},"terms":{..},"v":1};
//! ```
//!
//! - `docs`: one entry per page, in byte order of the page URL; `u` is the page path relative to
//!   the edition root (percent-encoded outside `[A-Za-z0-9._~@/-]`), `t` the `<title>` text (else
//!   the first heading, else the URL), `h` the `h1`..`h6` texts in document order.
//! - `terms`: token -> `[[doc, weight]]`, docs ascending; the weight is the highest field weight
//!   the token has in that page: 3 title, 2 heading, 1 body.
//! - `names`: exact-name table built from reference page paths
//!   `ref/<cli|mcp|python>/<command_id>@<major>.html`. For each command, `name@major` and the MCP
//!   name of that major (`name` with dots as underscores, plus `_vN` for majors 2 and above,
//!   CONSTITUTION §3) map to that major's pages; `name` and the CLI path
//!   `fid <domain> <verb-object>` (underscores as hyphens) map to the pages of every major,
//!   newest major first. Within a major the order is CLI, MCP, Python.
//!
//! The JSON is RFC 8785 canonical (sorted keys, compact, via `fid-canon`), then every `<`, `>`,
//! `&` and non-ASCII character is written as a `\uXXXX` escape, so the file is pure ASCII, cannot
//! close a surrounding `<script>` element, and decodes the same under any page charset.
//!
//! Tokenisation (the search UI must apply the same rules to queries): only ASCII `A-Z` are
//! lower-cased (no locale, no Unicode case folding, no Unicode normalisation such as NFC); a
//! token is a run of word characters, where a word character is an ASCII letter or digit, or any
//! non-ASCII character outside the fixed separator list of [`is_separator`] (code point ranges
//! written out in this file, so no Unicode property table is consulted); `.`, `_`, `@` and `-`
//! are kept when they follow a word character of the current token and the next character is a
//! word character; every other character ends the token. Tokens shorter than 2 or longer than
//! [`MAX_TOKEN_CHARS`] characters (Unicode scalar values) are dropped. No stemming. Whitespace in
//! titles and headings is collapsed with the same fixed list ([`is_space`]).
//!
//! Bounds: at most [`MAX_TOTAL_INPUT_BYTES`] of HTML are read per edition, and the build stops
//! (`over_budget`, exit 1) as soon as a lower bound of the index size exceeds the budget.
//!
//! The HTML reader is a bounded tokenizer: it skips comments, doctype and processing
//! instructions, the contents of `script` and `style`, and everything inside `nav`; every tag
//! separates words; the entities `amp lt gt quot apos nbsp` and numeric references are decoded.

use fid_canon::CValue;
use fid_types::CommandId;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{Read, Write};
use std::path::Path;

/// Name of the index file written at the edition root.
pub const INDEX_FILE: &str = "search-index.js";
/// Global the index file assigns.
pub const GLOBAL: &str = "window.FID_SEARCH_INDEX";
/// Index format version (`v`).
pub const FORMAT_VERSION: u8 = 1;
/// Largest HTML page read.
pub const MAX_PAGE_BYTES: usize = 8 * 1024 * 1024;
/// Most HTML pages in one edition.
pub const MAX_PAGES: usize = 100_000;
/// Deepest directory nesting below the edition root.
pub const MAX_DIR_DEPTH: usize = 32;
/// Largest budget file read.
pub const MAX_BUDGET_BYTES: usize = 64 * 1024;
/// Longest token kept, in characters.
pub const MAX_TOKEN_CHARS: usize = 64;
/// Longest title or heading kept, in characters (longer text is cut).
pub const MAX_LABEL_CHARS: usize = 300;
/// Most headings kept per page.
pub const MAX_HEADINGS: usize = 200;
/// Most HTML bytes read from one edition in total.
pub const MAX_TOTAL_INPUT_BYTES: u64 = 512 * 1024 * 1024;

/// Field weights.
const W_TITLE: u8 = 3;
const W_HEADING: u8 = 2;
const W_BODY: u8 = 1;

/// Errors that stop the run before an index exists (exit 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
    EditionUnreadable { path: String, reason: String },
    BudgetParse { path: String, reason: String },
    IndexUnwritable { path: String, reason: String },
    IndexSerialise { path: String, reason: String },
}

impl SearchError {
    pub fn code(&self) -> &'static str {
        match self {
            SearchError::EditionUnreadable { .. } => "edition_unreadable",
            SearchError::BudgetParse { .. } => "budget_parse",
            SearchError::IndexUnwritable { .. } => "index_unwritable",
            SearchError::IndexSerialise { .. } => "index_serialise",
        }
    }

    fn parts(&self) -> (&str, &str) {
        match self {
            SearchError::EditionUnreadable { path, reason }
            | SearchError::BudgetParse { path, reason }
            | SearchError::IndexUnwritable { path, reason }
            | SearchError::IndexSerialise { path, reason } => (path, reason),
        }
    }

    /// Canonical JSON for `--json` on exit 2.
    pub fn to_json(&self) -> Vec<u8> {
        let (path, detail) = self.parts();
        let mut e = BTreeMap::new();
        e.insert("code".to_string(), CValue::Str(self.code().to_string()));
        e.insert("detail".to_string(), CValue::Str(detail.to_string()));
        e.insert("path".to_string(), CValue::Str(path.to_string()));
        let mut o = BTreeMap::new();
        o.insert("error".to_string(), CValue::Obj(e));
        o.insert("ok".to_string(), CValue::Bool(false));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default()
    }
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (path, reason) = self.parts();
        write!(f, "{} {path}: {reason}", self.code())
    }
}

impl std::error::Error for SearchError {}

/// One indexed page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doc {
    /// Title (`t`).
    pub title: String,
    /// URL relative to the edition root (`u`).
    pub url: String,
    /// Headings in document order (`h`).
    pub headings: Vec<String>,
}

/// The index of one edition.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Index {
    pub docs: Vec<Doc>,
    /// token -> (doc index, weight), doc ascending.
    pub terms: BTreeMap<String, Vec<(usize, u8)>>,
    /// exact name -> page URLs.
    pub names: BTreeMap<String, Vec<String>>,
}

impl Index {
    fn to_value(&self) -> CValue {
        let docs = self
            .docs
            .iter()
            .map(|d| {
                let mut o = BTreeMap::new();
                o.insert("h".to_string(), CValue::Arr(d.headings.iter().map(|h| CValue::Str(h.clone())).collect()));
                o.insert("t".to_string(), CValue::Str(d.title.clone()));
                o.insert("u".to_string(), CValue::Str(d.url.clone()));
                CValue::Obj(o)
            })
            .collect();
        let terms = self
            .terms
            .iter()
            .map(|(tok, posts)| {
                let list = posts
                    .iter()
                    .map(|&(doc, w)| CValue::Arr(vec![CValue::Num(doc as f64), CValue::Num(f64::from(w))]))
                    .collect();
                (tok.clone(), CValue::Arr(list))
            })
            .collect();
        let names = self
            .names
            .iter()
            .map(|(k, urls)| (k.clone(), CValue::Arr(urls.iter().map(|u| CValue::Str(u.clone())).collect())))
            .collect();
        let mut o = BTreeMap::new();
        o.insert("docs".to_string(), CValue::Arr(docs));
        o.insert("names".to_string(), CValue::Obj(names));
        o.insert("terms".to_string(), CValue::Obj(terms));
        o.insert("v".to_string(), CValue::Num(f64::from(FORMAT_VERSION)));
        CValue::Obj(o)
    }

    /// The bytes of `search-index.js`: `window.FID_SEARCH_INDEX = <json>;\n`, pure ASCII.
    /// Fails (and nothing is written) if the canonical serialisation fails.
    pub fn to_script(&self) -> Result<Vec<u8>, String> {
        let json = fid_canon::to_canonical_bytes(&self.to_value()).map_err(|e| e.to_string())?;
        let json = String::from_utf8(json).map_err(|e| e.to_string())?;
        let mut out = String::with_capacity(json.len() + 32);
        out.push_str(GLOBAL);
        out.push_str(" = ");
        out.push_str(&js_safe(&json));
        out.push_str(";\n");
        Ok(out.into_bytes())
    }
}

/// Escapes `<`, `>`, `&` and every non-ASCII character of canonical JSON text as `\uXXXX`
/// (UTF-16 code units, lower-case hex). These characters only occur inside JSON strings, so the
/// result parses to the same value.
pub fn js_safe(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        if c == '<' || c == '>' || c == '&' || !c.is_ascii() {
            let mut buf = [0u16; 2];
            for unit in c.encode_utf16(&mut buf) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ------------------------------------------------------------------ tokenisation

/// Non-ASCII separator characters, a fixed list of code points: U+0080..U+00BF (C1 controls,
/// Latin-1 punctuation and symbols, including U+0085 and U+00A0), U+00D7, U+00F7, U+1680 (Ogham
/// space mark), U+2000..U+206F (General Punctuation, including the other Unicode spaces and line
/// and paragraph separators), U+2E00..U+2E7F, U+3000..U+303F, U+FE10..U+FE1F, U+FE30..U+FE6F,
/// U+FF00..U+FF0F, U+FF1A..U+FF20, U+FF3B..U+FF40, U+FF5B..U+FF65 and U+FFFD. Every Unicode
/// White_Space character is in this list or is ASCII.
pub fn is_separator(c: char) -> bool {
    matches!(c as u32,
        0x80..=0xBF | 0xD7 | 0xF7 | 0x1680 | 0x2000..=0x206F | 0x2E00..=0x2E7F | 0x3000..=0x303F
        | 0xFE10..=0xFE1F | 0xFE30..=0xFE6F | 0xFF00..=0xFF0F | 0xFF1A..=0xFF20 | 0xFF3B..=0xFF40
        | 0xFF5B..=0xFF65 | 0xFFFD)
}

/// Whitespace for collapsing titles and headings: ASCII tab, line feed, vertical tab, form feed,
/// carriage return and space, and U+0085, U+00A0, U+1680, U+2000..U+200A, U+2028, U+2029,
/// U+202F, U+205F and U+3000 (the Unicode White_Space set, written out).
pub fn is_space(c: char) -> bool {
    matches!(c as u32,
        0x09..=0x0D | 0x20 | 0x85 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000)
}

fn is_word(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_alphanumeric()
    } else {
        !is_separator(c)
    }
}

fn is_connector(c: char) -> bool {
    matches!(c, '.' | '_' | '@' | '-')
}

/// Splits `text` into index tokens (see the module documentation).
pub fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut n = 0usize;
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        if is_word(c) {
            cur.push(c.to_ascii_lowercase());
            n += 1;
        } else if is_connector(c) && n > 0 && it.peek().is_some_and(|&next| is_word(next)) {
            cur.push(c);
            n += 1;
        } else {
            flush_token(&mut cur, &mut n, &mut out);
        }
    }
    flush_token(&mut cur, &mut n, &mut out);
    out
}

fn flush_token(cur: &mut String, n: &mut usize, out: &mut Vec<String>) {
    if *n >= 2 && *n <= MAX_TOKEN_CHARS {
        out.push(std::mem::take(cur));
    } else {
        cur.clear();
    }
    *n = 0;
}

// ------------------------------------------------------------------ HTML

/// Text fields of one page.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PageText {
    pub title: String,
    pub headings: Vec<String>,
    pub body: String,
}

/// Collapses whitespace runs to one space, trims, and cuts at [`MAX_LABEL_CHARS`].
fn label(raw: &str) -> String {
    let joined = raw.split(is_space).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    joined.chars().take(MAX_LABEL_CHARS).collect()
}

/// Index just past the `>` closing the tag whose name ends at `from`, honouring quoted values.
fn tag_end(s: &[u8], from: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    let mut i = from;
    while i < s.len() {
        let b = s[i];
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => quote = Some(b),
            None if b == b'>' => return Some(i + 1),
            None => {}
        }
        i += 1;
    }
    None
}

/// Index of the first `</name` (ASCII case-insensitive) at or after `from`.
fn find_end_tag(s: &[u8], from: usize, name: &[u8]) -> Option<usize> {
    let mut i = from;
    while i + 2 + name.len() <= s.len() {
        if s[i] == b'<' && s[i + 1] == b'/' && s[i + 2..i + 2 + name.len()].eq_ignore_ascii_case(name) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Decodes one entity starting at `&` in `s[at..]`; returns the text and the bytes consumed.
fn entity(s: &str, at: usize) -> Option<(String, usize)> {
    let rest = s.get(at + 1..)?;
    let semi = rest.bytes().take(12).position(|b| b == b';')?;
    let name = rest.get(..semi)?;
    let text = match name {
        "amp" => "&".to_string(),
        "lt" => "<".to_string(),
        "gt" => ">".to_string(),
        "quot" => "\"".to_string(),
        "apos" => "'".to_string(),
        "nbsp" => " ".to_string(),
        _ => {
            let num = name.strip_prefix('#')?;
            let code = match num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
                Some(hex) if !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()) => {
                    u32::from_str_radix(hex, 16).ok()?
                }
                Some(_) => return None,
                None if !num.is_empty() && num.bytes().all(|b| b.is_ascii_digit()) => num.parse::<u32>().ok()?,
                None => return None,
            };
            let c = if code == 0 { '\u{fffd}' } else { char::from_u32(code).unwrap_or('\u{fffd}') };
            c.to_string()
        }
    };
    Some((text, semi + 2))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sink {
    Title,
    Heading,
    Body,
}

/// Reads the title, headings and body text of one HTML page.
pub fn read_html(src: &str) -> PageText {
    let b = src.as_bytes();
    let mut page = PageText::default();
    let mut title_raw = String::new();
    let mut title_done = false;
    let mut in_title = false;
    let mut heading: Option<String> = None;
    let mut nav_depth = 0usize;
    let mut i = 0usize;
    let mut text_start = 0usize;

    // Appends src[text_start..end] (entity-decoded) to the current sink.
    let push_text = |page: &mut PageText,
                     title_raw: &mut String,
                     heading: &mut Option<String>,
                     sink: Option<Sink>,
                     start: usize,
                     end: usize| {
        let Some(sink) = sink else { return };
        let Some(raw) = src.get(start..end) else { return };
        let target: &mut String = match sink {
            Sink::Title => title_raw,
            Sink::Heading => match heading.as_mut() {
                Some(h) => h,
                None => return,
            },
            Sink::Body => &mut page.body,
        };
        let mut j = 0usize;
        let rb = raw.as_bytes();
        let mut seg = 0usize;
        while j < rb.len() {
            if rb[j] == b'&' {
                if let Some((t, used)) = entity(raw, j) {
                    target.push_str(raw.get(seg..j).unwrap_or(""));
                    target.push_str(&t);
                    j += used;
                    seg = j;
                    continue;
                }
            }
            j += 1;
        }
        target.push_str(raw.get(seg..).unwrap_or(""));
    };
    let sink = |in_title: bool, heading: &Option<String>, nav_depth: usize, title_done: bool| -> Option<Sink> {
        if nav_depth > 0 {
            None
        } else if in_title {
            if title_done {
                None
            } else {
                Some(Sink::Title)
            }
        } else if heading.is_some() {
            Some(Sink::Heading)
        } else {
            Some(Sink::Body)
        }
    };

    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &b[i + 1..];
        let current = sink(in_title, &heading, nav_depth, title_done);
        if rest.starts_with(b"!--") {
            push_text(&mut page, &mut title_raw, &mut heading, current, text_start, i);
            let after = i + 4;
            i = match src.get(after..).and_then(|r| r.find("-->")) {
                Some(p) => after + p + 3,
                None => b.len(),
            };
            text_start = i;
            continue;
        }
        if rest.first().is_some_and(|&c| c == b'!' || c == b'?') {
            push_text(&mut page, &mut title_raw, &mut heading, current, text_start, i);
            i = tag_end(b, i + 1).unwrap_or(b.len());
            text_start = i;
            continue;
        }
        let closing = rest.first() == Some(&b'/');
        let name_from = i + 1 + usize::from(closing);
        let name_len = b[name_from.min(b.len())..].iter().take_while(|c| c.is_ascii_alphanumeric()).count();
        if name_len == 0 || !b[name_from].is_ascii_alphabetic() {
            // A literal `<`: part of the text.
            i += 1;
            continue;
        }
        push_text(&mut page, &mut title_raw, &mut heading, current, text_start, i);
        let name = src.get(name_from..name_from + name_len).unwrap_or("").to_ascii_lowercase();
        let mut next = tag_end(b, name_from + name_len).unwrap_or(b.len());
        let is_heading = matches!(name.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
        match (closing, name.as_str()) {
            (false, "script" | "style") => {
                // Raw text: skip to the matching end tag and past it.
                next = match find_end_tag(b, next, name.as_bytes()) {
                    Some(p) => tag_end(b, p + 2 + name.len()).unwrap_or(b.len()),
                    None => b.len(),
                };
            }
            (false, "nav") => nav_depth += 1,
            (true, "nav") => nav_depth = nav_depth.saturating_sub(1),
            (false, "title") if !title_done => in_title = true,
            (true, "title") if in_title => {
                in_title = false;
                title_done = true;
            }
            (false, _) if is_heading && heading.is_none() && nav_depth == 0 && !in_title => {
                heading = Some(String::new());
            }
            (true, _) if is_heading => {
                if let Some(h) = heading.take() {
                    let h = label(&h);
                    if !h.is_empty() && page.headings.len() < MAX_HEADINGS {
                        page.headings.push(h);
                    }
                }
            }
            _ => {}
        }
        // Every tag separates words.
        if let Some(s) = sink(in_title, &heading, nav_depth, title_done) {
            match s {
                Sink::Title => title_raw.push(' '),
                Sink::Heading => {
                    if let Some(h) = heading.as_mut() {
                        h.push(' ');
                    }
                }
                Sink::Body => page.body.push(' '),
            }
        }
        i = next;
        text_start = i;
    }
    let current = sink(in_title, &heading, nav_depth, title_done);
    push_text(&mut page, &mut title_raw, &mut heading, current, text_start, b.len());
    if let Some(h) = heading.take() {
        let h = label(&h);
        if !h.is_empty() && page.headings.len() < MAX_HEADINGS {
            page.headings.push(h);
        }
    }
    page.title = label(&title_raw);
    page
}

// ------------------------------------------------------------------ edition walk

fn read_bounded(path: &Path, max: usize) -> Result<Vec<u8>, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    f.take(max as u64 + 1).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    if buf.len() > max {
        return Err(format!("larger than {max} bytes"));
    }
    Ok(buf)
}

fn unreadable(path: &Path, reason: impl Into<String>) -> SearchError {
    SearchError::EditionUnreadable { path: path.display().to_string(), reason: reason.into() }
}

/// Relative paths (`/`-separated) of every regular `.html` file below `dir`. Symbolic links are
/// not followed.
fn walk(dir: &Path, rel: &str, depth: usize, out: &mut Vec<String>) -> Result<(), SearchError> {
    if depth > MAX_DIR_DEPTH {
        return Err(unreadable(dir, "directory nesting too deep"));
    }
    let io = |e: std::io::Error| unreadable(dir, e.to_string());
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(io)? {
        let entry = entry.map_err(io)?;
        let ft = entry.file_type().map_err(io)?;
        let name = entry.file_name().into_string().map_err(|_| unreadable(dir, "file name is not valid UTF-8"))?;
        entries.push((name, ft));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, ft) in entries {
        let child = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        if ft.is_dir() {
            walk(&dir.join(&name), &child, depth + 1, out)?;
        } else if ft.is_file() && name.ends_with(".html") {
            out.push(child);
            if out.len() > MAX_PAGES {
                return Err(unreadable(dir, format!("more than {MAX_PAGES} pages")));
            }
        }
    }
    Ok(())
}

/// Percent-encodes every byte outside `[A-Za-z0-9._~@/-]`.
pub fn url_of(rel: &str) -> String {
    let mut out = String::with_capacity(rel.len());
    for &b in rel.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~' | b'@' | b'/' | b'-') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Rank of the MCP surface.
const MCP_RANK: u8 = 1;

/// Marker the reference renderer (`ref_pages`) puts on a surface page of a command that is not
/// available on that surface.
const UNAVAILABLE_MARKER: &str = "class=\"unavailable\"";

/// Surface order of reference pages: CLI, MCP, Python.
fn surface_rank(s: &str) -> Option<u8> {
    match s {
        "cli" => Some(0),
        "mcp" => Some(1),
        "python" => Some(2),
        _ => None,
    }
}

/// `(command id, surface rank)` of a reference page path `ref/<surface>/<command_id>.html`.
fn ref_page(rel: &str) -> Option<(CommandId, u8)> {
    let mut parts = rel.split('/');
    let (Some("ref"), Some(surface), Some(file), None) = (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let rank = surface_rank(surface)?;
    let id = CommandId::parse(file.strip_suffix(".html")?).ok()?;
    Some((id, rank))
}

/// CLI path of a command name: `fid <domain> <verb-object>`, underscores in the verb-object as
/// hyphens; a name without a dot is a single-verb command, `fid <verb>`.
pub fn cli_path(name: &str) -> String {
    match name.split_once('.') {
        Some((domain, rest)) => format!("fid {domain} {}", rest.replace('_', "-")),
        None => format!("fid {}", name.replace('_', "-")),
    }
}

/// MCP tool name of a command major: dots as underscores, `_vN` for majors 2 and above.
pub fn mcp_name(name: &str, major: u32) -> String {
    let base = name.replace('.', "_");
    if major >= 2 {
        format!("{base}_v{major}")
    } else {
        base
    }
}

/// Builds the exact-name table from the reference pages among `rels` (relative paths).
/// `mcp_unavailable` holds the MCP pages that say the command is not available on MCP; no MCP
/// tool name key is made for those command majors.
pub fn names_table(rels: &[String], mcp_unavailable: &BTreeSet<String>) -> BTreeMap<String, Vec<String>> {
    // name -> major -> (id text, surface rank -> url)
    // name -> major -> (id text, surface rank -> url, MCP marked unavailable)
    type Majors = BTreeMap<u32, (String, BTreeMap<u8, String>, bool)>;
    let mut cmds: BTreeMap<String, Majors> = BTreeMap::new();
    for rel in rels {
        let Some((id, rank)) = ref_page(rel) else { continue };
        let entry = cmds
            .entry(id.name().to_string())
            .or_default()
            .entry(id.major())
            .or_insert_with(|| (id.as_str().to_string(), BTreeMap::new(), false));
        // Two spellings of one major (`x@1`, `x@01`): the first in byte order wins per surface.
        if !entry.1.contains_key(&rank) {
            entry.1.insert(rank, url_of(rel));
            if rank == MCP_RANK && mcp_unavailable.contains(rel) {
                entry.2 = true;
            }
        }
    }
    let mut names: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, majors) in &cmds {
        let mut all = Vec::new();
        for (major, (id_text, pages, no_mcp)) in majors.iter().rev() {
            let urls: Vec<String> = pages.values().cloned().collect();
            names.entry(id_text.clone()).or_default().extend(urls.iter().cloned());
            names.entry(format!("{name}@{major}")).or_default().extend(urls.iter().cloned());
            if !*no_mcp {
                names.entry(mcp_name(name, *major)).or_default().extend(urls.iter().cloned());
            }
            all.extend(urls);
        }
        names.entry(name.clone()).or_default().extend(all.iter().cloned());
        names.entry(cli_path(name)).or_default().extend(all);
    }
    for urls in names.values_mut() {
        let mut seen = BTreeSet::new();
        urls.retain(|u| seen.insert(u.clone()));
    }
    names
}

/// Result of [`build`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Built {
    Index(Index),
    /// Stopped early: the index would be at least this many bytes, over the budget.
    OverBudget { at_least: u64 },
}

/// Bytes every index file has besides its contents: the assignment, the four top-level keys and
/// their brackets.
fn fixed_bytes() -> u64 {
    (GLOBAL.len() + " = ".len() + r#"{"docs":[],"names":{},"terms":{},"v":1};"#.len() + 1) as u64
}

/// Builds the index of the edition at `edition`, reading at most `max_input` HTML bytes in total,
/// and stops with [`Built::OverBudget`] as soon as a lower bound of the index size exceeds
/// `max_bytes`. The lower bound counts the unescaped bytes of every title, URL, heading and term
/// plus 5 bytes (`[d,w]`) per posting; escaping and separators only add to it.
pub fn build(edition: &Path, max_bytes: u64, max_input: u64) -> Result<Built, SearchError> {
    let meta = std::fs::metadata(edition).map_err(|e| unreadable(edition, e.to_string()))?;
    if !meta.is_dir() {
        return Err(unreadable(edition, "not a directory"));
    }
    let mut rels = Vec::new();
    walk(edition, "", 0, &mut rels)?;
    rels.sort();
    let mut index = Index::default();
    let mut mcp_unavailable = BTreeSet::new();
    let mut input = 0u64;
    let mut estimate = fixed_bytes();
    for (n, rel) in rels.iter().enumerate() {
        let path = edition.join(rel);
        let bytes = read_bounded(&path, MAX_PAGE_BYTES).map_err(|e| unreadable(&path, e))?;
        input += bytes.len() as u64;
        if input > max_input {
            return Err(unreadable(edition, format!("edition holds more than {max_input} bytes of HTML")));
        }
        let text = String::from_utf8_lossy(&bytes);
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        if ref_page(rel).is_some_and(|(_, rank)| rank == MCP_RANK) && text.contains(UNAVAILABLE_MARKER) {
            mcp_unavailable.insert(rel.clone());
        }
        let page = read_html(text);
        let url = url_of(rel);
        // A page without title or heading is listed under its URL, but the URL is not indexed.
        let (title, title_terms) = if !page.title.is_empty() {
            (page.title.clone(), true)
        } else if let Some(h) = page.headings.first() {
            (h.clone(), true)
        } else {
            (url.clone(), false)
        };
        let mut weights: BTreeMap<String, u8> = BTreeMap::new();
        let mut add = |text: &str, w: u8| {
            for tok in tokenize(text) {
                let e = weights.entry(tok).or_insert(w);
                if *e < w {
                    *e = w;
                }
            }
        };
        add(&page.body, W_BODY);
        for h in &page.headings {
            add(h, W_HEADING);
        }
        if title_terms {
            add(&title, W_TITLE);
        }
        estimate += (title.len() + url.len() + page.headings.iter().map(String::len).sum::<usize>()) as u64;
        for (tok, w) in weights {
            let key_len = tok.len() as u64;
            let posts = index.terms.entry(tok).or_default();
            if posts.is_empty() {
                estimate += key_len;
            }
            posts.push((n, w));
            estimate += 5;
        }
        index.docs.push(Doc { title, url, headings: page.headings });
        if estimate > max_bytes {
            return Ok(Built::OverBudget { at_least: estimate });
        }
    }
    index.names = names_table(&rels, &mcp_unavailable);
    Ok(Built::Index(index))
}

/// Reads `max_bytes` (a positive integer) from the budget TOML file.
pub fn read_budget(path: &Path) -> Result<u64, SearchError> {
    let bad = |reason: String| SearchError::BudgetParse { path: path.display().to_string(), reason };
    let bytes = read_bounded(path, MAX_BUDGET_BYTES).map_err(&bad)?;
    let text = String::from_utf8(bytes).map_err(|_| bad("not valid UTF-8".to_string()))?;
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| bad(e.message().to_string()))?;
    match table.get("max_bytes") {
        Some(toml::Value::Integer(n)) if *n > 0 => Ok(*n as u64),
        Some(_) => Err(bad("max_bytes must be a positive integer".to_string())),
        None => Err(bad("missing max_bytes".to_string())),
    }
}

/// Result of one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Size of the index file in bytes, or a lower bound of it when `complete` is false.
    pub bytes: u64,
    /// Whether `bytes` is the exact size (false when the build stopped early over budget).
    pub complete: bool,
    /// The budget's `max_bytes`.
    pub max_bytes: u64,
    pub docs: usize,
    pub terms: usize,
    pub names: usize,
    /// Whether the file was written (false when over budget).
    pub written: bool,
}

impl Outcome {
    pub fn over_budget(&self) -> bool {
        self.bytes > self.max_bytes
    }

    pub fn exit_code(&self) -> u8 {
        u8::from(self.over_budget())
    }

    /// Text report: one `over_budget` finding line, or a summary line.
    pub fn render(&self) -> String {
        if self.over_budget() {
            format!(
                "over_budget {INDEX_FILE}: {}{} bytes exceeds the budget of {} bytes; nothing written\n",
                if self.complete { "" } else { "at least " },
                self.bytes,
                self.max_bytes
            )
        } else {
            format!(
                "search-index: wrote {INDEX_FILE} ({} bytes of {} budget; {} page(s), {} term(s), {} name(s))\n",
                self.bytes, self.max_bytes, self.docs, self.terms, self.names
            )
        }
    }

    /// Canonical JSON report.
    pub fn to_json(&self) -> Vec<u8> {
        let num = |n: u64| CValue::Num(n as f64);
        let mut findings = Vec::new();
        if self.over_budget() {
            let mut f = BTreeMap::new();
            f.insert("bytes".to_string(), num(self.bytes));
            f.insert("code".to_string(), CValue::Str("over_budget".to_string()));
            f.insert("max_bytes".to_string(), num(self.max_bytes));
            findings.push(CValue::Obj(f));
        }
        let mut o = BTreeMap::new();
        o.insert("bytes".to_string(), num(self.bytes));
        o.insert("complete".to_string(), CValue::Bool(self.complete));
        o.insert("docs".to_string(), num(self.docs as u64));
        o.insert("findings".to_string(), CValue::Arr(findings));
        o.insert("max_bytes".to_string(), num(self.max_bytes));
        o.insert("names".to_string(), num(self.names as u64));
        o.insert("ok".to_string(), CValue::Bool(!self.over_budget()));
        o.insert("terms".to_string(), num(self.terms as u64));
        o.insert("written".to_string(), CValue::Bool(self.written));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default()
    }
}

/// `search-index`: builds the index of `edition` and writes `search-index.js` at its root, unless
/// the file would exceed the budget's `max_bytes`, in which case nothing is written. The file is
/// written to a new temporary file in the edition and renamed over the target; a symbolic link or
/// a non-file at the target is refused.
pub fn run(edition: &Path, budget: &Path) -> Result<Outcome, SearchError> {
    let max_bytes = read_budget(budget)?;
    let index = match build(edition, max_bytes, MAX_TOTAL_INPUT_BYTES)? {
        Built::Index(index) => index,
        Built::OverBudget { at_least } => {
            return Ok(Outcome {
                bytes: at_least,
                complete: false,
                max_bytes,
                docs: 0,
                terms: 0,
                names: 0,
                written: false,
            })
        }
    };
    let script = index
        .to_script()
        .map_err(|reason| SearchError::IndexSerialise { path: edition.display().to_string(), reason })?;
    let mut outcome = Outcome {
        bytes: script.len() as u64,
        complete: true,
        max_bytes,
        docs: index.docs.len(),
        terms: index.terms.len(),
        names: index.names.len(),
        written: false,
    };
    if !outcome.over_budget() {
        write_atomic(&edition.join(INDEX_FILE), &script)?;
        outcome.written = true;
    }
    Ok(outcome)
}

/// Writes `bytes` to a fresh temporary file next to `path`, then renames it over `path`.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), SearchError> {
    let fail = |p: &Path, reason: String| SearchError::IndexUnwritable { path: p.display().to_string(), reason };
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => {
            return Err(fail(path, "refusing to replace a symbolic link".to_string()));
        }
        Ok(m) if !m.is_file() => return Err(fail(path, "exists and is not a regular file".to_string())),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(fail(path, e.to_string())),
    }
    let tmp = path.with_file_name(format!("{INDEX_FILE}.tmp-{}", std::process::id()));
    let result = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .and_then(|mut f| f.write_all(bytes).and_then(|()| f.sync_all()))
        .and_then(|()| std::fs::rename(&tmp, path));
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(fail(path, e.to_string()));
    }
    Ok(())
}
