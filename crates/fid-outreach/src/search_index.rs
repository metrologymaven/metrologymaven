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
//! Tokenisation: ASCII letters are lower-cased (no locale or Unicode case folding); a token is a
//! run of word characters (ASCII letters and digits, and every non-ASCII character that is not
//! whitespace and not in a fixed list of punctuation blocks), where `.`, `_`, `@` and `-` are kept
//! when they sit between a word character and a following word character; tokens shorter than 2
//! or longer than [`MAX_TOKEN_CHARS`] characters are dropped. No stemming.
//!
//! The HTML reader is a bounded tokenizer: it skips comments, doctype and processing
//! instructions, the contents of `script` and `style`, and everything inside `nav`; every tag
//! separates words; the entities `amp lt gt quot apos nbsp` and numeric references are decoded.

use fid_canon::CValue;
use fid_types::CommandId;
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
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
}

impl SearchError {
    pub fn code(&self) -> &'static str {
        match self {
            SearchError::EditionUnreadable { .. } => "edition_unreadable",
            SearchError::BudgetParse { .. } => "budget_parse",
            SearchError::IndexUnwritable { .. } => "index_unwritable",
        }
    }

    fn parts(&self) -> (&str, &str) {
        match self {
            SearchError::EditionUnreadable { path, reason }
            | SearchError::BudgetParse { path, reason }
            | SearchError::IndexUnwritable { path, reason } => (path, reason),
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
    pub fn to_script(&self) -> Vec<u8> {
        // Every number is a small integer, so serialisation cannot fail; an empty result would
        // still be caught by the caller's parse test rather than by a panic here.
        let json = fid_canon::to_canonical_bytes(&self.to_value()).unwrap_or_default();
        let json = String::from_utf8(json).unwrap_or_default();
        let mut out = String::with_capacity(json.len() + 32);
        out.push_str(GLOBAL);
        out.push_str(" = ");
        out.push_str(&js_safe(&json));
        out.push_str(";\n");
        out.into_bytes()
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

/// Non-ASCII separator characters: Latin-1 punctuation and symbols, multiplication and division
/// signs, General Punctuation, Supplemental Punctuation, CJK symbols and punctuation, vertical
/// and small form variants, fullwidth ASCII punctuation, and the replacement character.
fn is_unicode_separator(c: char) -> bool {
    matches!(c as u32,
        0x80..=0xBF | 0xD7 | 0xF7 | 0x2000..=0x206F | 0x2E00..=0x2E7F | 0x3000..=0x303F
        | 0xFE10..=0xFE1F | 0xFE30..=0xFE6F | 0xFF00..=0xFF0F | 0xFF1A..=0xFF20 | 0xFF3B..=0xFF40
        | 0xFF5B..=0xFF65 | 0xFFFD)
}

fn is_word(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_alphanumeric()
    } else {
        !c.is_whitespace() && !is_unicode_separator(c)
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
    let joined = raw.split_whitespace().collect::<Vec<_>>().join(" ");
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

/// Builds the exact-name table from the reference pages among `urls` (relative paths).
pub fn names_table(rels: &[String]) -> BTreeMap<String, Vec<String>> {
    // name -> major -> (id text, surface rank -> url)
    type Majors = BTreeMap<u32, (String, BTreeMap<u8, String>)>;
    let mut cmds: BTreeMap<String, Majors> = BTreeMap::new();
    for rel in rels {
        let Some((id, rank)) = ref_page(rel) else { continue };
        let entry = cmds
            .entry(id.name().to_string())
            .or_default()
            .entry(id.major())
            .or_insert_with(|| (id.as_str().to_string(), BTreeMap::new()));
        // Two spellings of one major (`x@1`, `x@01`): the first in byte order wins per surface.
        entry.1.entry(rank).or_insert_with(|| url_of(rel));
    }
    let mut names: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, majors) in &cmds {
        let mut all = Vec::new();
        for (major, (id_text, pages)) in majors.iter().rev() {
            let urls: Vec<String> = pages.values().cloned().collect();
            names.entry(id_text.clone()).or_default().extend(urls.iter().cloned());
            names.entry(format!("{name}@{major}")).or_default().extend(urls.iter().cloned());
            names.entry(mcp_name(name, *major)).or_default().extend(urls.iter().cloned());
            all.extend(urls);
        }
        names.entry(name.clone()).or_default().extend(all.iter().cloned());
        names.entry(cli_path(name)).or_default().extend(all);
    }
    for urls in names.values_mut() {
        let mut seen = std::collections::BTreeSet::new();
        urls.retain(|u| seen.insert(u.clone()));
    }
    names
}

/// Builds the index of the edition at `edition`.
pub fn build(edition: &Path) -> Result<Index, SearchError> {
    let meta = std::fs::metadata(edition).map_err(|e| unreadable(edition, e.to_string()))?;
    if !meta.is_dir() {
        return Err(unreadable(edition, "not a directory"));
    }
    let mut rels = Vec::new();
    walk(edition, "", 0, &mut rels)?;
    rels.sort();
    let mut index = Index::default();
    for (n, rel) in rels.iter().enumerate() {
        let path = edition.join(rel);
        let bytes = read_bounded(&path, MAX_PAGE_BYTES).map_err(|e| unreadable(&path, e))?;
        let text = String::from_utf8_lossy(&bytes);
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let page = read_html(text);
        let url = url_of(rel);
        let title = if !page.title.is_empty() {
            page.title.clone()
        } else if let Some(h) = page.headings.first() {
            h.clone()
        } else {
            url.clone()
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
        add(&title, W_TITLE);
        for (tok, w) in weights {
            index.terms.entry(tok).or_default().push((n, w));
        }
        index.docs.push(Doc { title, url, headings: page.headings });
    }
    index.names = names_table(&rels);
    Ok(index)
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
    /// Size of the index file in bytes.
    pub bytes: u64,
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
                "over_budget {INDEX_FILE}: {} bytes exceeds the budget of {} bytes; nothing written\n",
                self.bytes, self.max_bytes
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
/// the file would exceed the budget's `max_bytes`, in which case nothing is written.
pub fn run(edition: &Path, budget: &Path) -> Result<Outcome, SearchError> {
    let max_bytes = read_budget(budget)?;
    let index = build(edition)?;
    let script = index.to_script();
    let mut outcome = Outcome {
        bytes: script.len() as u64,
        max_bytes,
        docs: index.docs.len(),
        terms: index.terms.len(),
        names: index.names.len(),
        written: false,
    };
    if !outcome.over_budget() {
        let path = edition.join(INDEX_FILE);
        std::fs::write(&path, &script)
            .map_err(|e| SearchError::IndexUnwritable { path: path.display().to_string(), reason: e.to_string() })?;
        outcome.written = true;
    }
    Ok(outcome)
}
