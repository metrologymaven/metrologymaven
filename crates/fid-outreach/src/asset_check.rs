//! Outreach attribution and font-origin check (spec node `OUTR.BRAND.ASSET.ATTR`).
//!
//! `fid-outreach asset-check --root <dir> --allow <file> --origin <url> [--exclude <glob>]...
//! [--json]` proves that every image, font, audio and video file under `--root` is either
//! original project work or a third-party asset listed in `ATTRIBUTION.md` under an allowed
//! licence, and that no stylesheet or page loads a font or stylesheet from another origin.
//!
//! `ATTRIBUTION.md` holds a Markdown table with the headers `path | origin | source | author |
//! licence` (the first such table counts). `path` is a root-relative path or glob (`*` and `?`
//! within one path segment, `**` for any number of segments). `origin` is `original` (project
//! work: licence `CC-BY-4.0`, author `project contributors`) or `third-party`. An empty cell or
//! `-` means "not given". The allowlist is a TOML file whose `asset_licences` array lists the
//! allowed SPDX licence IDs (`house-rules/spdx-allowlist.toml`, spec node `ECO.RULES.SPDX`);
//! its other keys are ignored.
//!
//! Findings (exit 1), sorted by code, subject and line:
//!
//! - `ATTR-MISSING <file>`: an asset file that no row matches (REQ-002);
//! - `ATTR-ORPHAN <path>`: a row that matches no file in the scanned tree (REQ-006);
//! - `ATTR-LICENCE <path> <licence>`: a third-party row whose licence is not a single SPDX ID on
//!   the allowlist, or an `original` row not licensed `CC-BY-4.0` (REQ-003, REQ-001);
//! - `ATTR-INCOMPLETE <path> <what>`: a third-party row without source or author, or an
//!   `original` row whose author is not `project contributors` (REQ-004, REQ-001);
//! - `FONT-REMOTE <file>:<line> <url>`: a CSS `@font-face` `url()`, a CSS `@import`, or an HTML
//!   `<link rel="stylesheet">` / `<link rel="preload">` `href` with an absolute URL whose origin
//!   is not `--origin` (REQ-005); also a remote `<base href>` or `<?xml-stylesheet href?>`.
//!   `.css` files are scanned, and `.html`, `.htm` and `.svg` files for those elements and the
//!   CSS in their `<style>` elements. Before deciding, CSS escapes and HTML character references
//!   are decoded and URLs are normalised as a browser does (tab, CR and LF removed, `\` read as
//!   `/`); a URL starting `//`, any non-http(s) scheme other than `data:`, and anything that
//!   cannot be decoded count as remote (fail closed);
//! - `ATTR-SYMLINK <path>`: a symbolic link in the scanned tree; links are never followed, so
//!   they are reported rather than skipped.
//!
//! Errors (exit 2): `ATTR-IO` (a file or directory cannot be read or exceeds a bound) and
//! `ATTR-PARSE` (the allowlist, the attribution table or `--origin` does not parse).
//!
//! Determinism D0: a pure function of the files read; the walk, rows and findings are sorted and
//! only ordered collections are used. Nothing is executed and nothing is fetched.

use fid_canon::CValue;
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The attribution file read at the root.
pub const ATTRIBUTION_FILE: &str = "ATTRIBUTION.md";
/// The licence of original project work.
pub const ORIGINAL_LICENCE: &str = "CC-BY-4.0";
/// The author of original project work.
pub const ORIGINAL_AUTHOR: &str = "project contributors";
/// Asset file extensions (compared case-insensitively).
pub const ASSET_EXTENSIONS: [&str; 18] = [
    "svg", "png", "jpg", "jpeg", "gif", "webp", "avif", "ico", "woff", "woff2", "ttf", "otf", "mp3", "ogg", "wav",
    "mp4", "webm", "mov",
];
/// Directory names never entered.
pub const SKIPPED_DIRS: [&str; 2] = [".git", "node_modules"];
/// Table headers, in order.
pub const HEADERS: [&str; 5] = ["path", "origin", "source", "author", "licence"];
/// Largest file read (attribution table, allowlist, CSS or HTML), in bytes.
pub const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
/// Most directory entries visited under the root.
pub const MAX_WALK_ENTRIES: usize = 100_000;
/// Deepest directory nesting visited under the root.
pub const MAX_WALK_DEPTH: usize = 32;

/// Finding codes. The order is the report order (alphabetical).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Code {
    AttrIncomplete,
    AttrLicence,
    AttrMissing,
    AttrOrphan,
    AttrSymlink,
    FontRemote,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::AttrIncomplete => "ATTR-INCOMPLETE",
            Code::AttrLicence => "ATTR-LICENCE",
            Code::AttrMissing => "ATTR-MISSING",
            Code::AttrOrphan => "ATTR-ORPHAN",
            Code::AttrSymlink => "ATTR-SYMLINK",
            Code::FontRemote => "FONT-REMOTE",
        }
    }
}

/// One finding. `subject` is a root-relative file (with `/`) or a row's path; `line` is 1-based
/// for `FONT-REMOTE` and 0 otherwise; `detail` is the licence, the missing fields or the URL.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub code: Code,
    pub subject: String,
    pub line: usize,
    pub detail: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.code.as_str(), self.subject)?;
        if self.line > 0 {
            write!(f, ":{}", self.line)?;
        }
        if !self.detail.is_empty() {
            write!(f, " {}", self.detail)?;
        }
        Ok(())
    }
}

/// Inputs that stop the check (exit 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttrError {
    /// A file or directory cannot be read, or exceeds a bound.
    Io { path: String, reason: String },
    /// The allowlist, the attribution table or the site origin does not parse.
    Parse { path: String, reason: String },
}

impl AttrError {
    pub fn code(&self) -> &'static str {
        match self {
            AttrError::Io { .. } => "ATTR-IO",
            AttrError::Parse { .. } => "ATTR-PARSE",
        }
    }

    fn parts(&self) -> (&str, &str) {
        match self {
            AttrError::Io { path, reason } | AttrError::Parse { path, reason } => (path, reason),
        }
    }

    /// Canonical JSON for `--json` on exit 2; `Err` carries a serialisation failure.
    pub fn to_json(&self) -> Result<Vec<u8>, String> {
        let (path, reason) = self.parts();
        let mut e = BTreeMap::new();
        e.insert("code".to_string(), CValue::Str(self.code().to_string()));
        e.insert("path".to_string(), CValue::Str(path.to_string()));
        e.insert("detail".to_string(), CValue::Str(reason.to_string()));
        let mut o = BTreeMap::new();
        o.insert("error".to_string(), CValue::Obj(e));
        o.insert("ok".to_string(), CValue::Bool(false));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).map_err(|e| format!("{e:?}"))
    }
}

impl fmt::Display for AttrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (path, reason) = self.parts();
        write!(f, "{} {path}: {reason}", self.code())
    }
}

impl std::error::Error for AttrError {}

fn io_err(path: &Path, reason: impl Into<String>) -> AttrError {
    AttrError::Io { path: path.display().to_string(), reason: reason.into() }
}

fn parse_err(path: &str, reason: impl Into<String>) -> AttrError {
    AttrError::Parse { path: path.to_string(), reason: reason.into() }
}

// ---------------------------------------------------------------------------------------------
// Licence IDs and the allowlist
// ---------------------------------------------------------------------------------------------

/// Whether `id` is syntactically one SPDX licence ID (or `LicenseRef-`): ASCII letters, digits,
/// `.` and `-`, starting with a letter or digit, with an optional trailing `+`. Expressions
/// (`OR`, `AND`, `WITH`, parentheses, spaces) are not single IDs.
pub fn is_spdx_id(id: &str) -> bool {
    let body = id.strip_suffix('+').unwrap_or(id);
    let mut chars = body.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// The allowed asset licences, keyed by their ASCII-lowercased form (SPDX IDs match
/// case-insensitively), each mapping to the ID as written.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Allowlist {
    pub asset_licences: BTreeMap<String, String>,
}

impl Allowlist {
    pub fn allows(&self, licence: &str) -> bool {
        is_spdx_id(licence) && self.asset_licences.contains_key(&licence.to_ascii_lowercase())
    }
}

/// Parses the allowlist TOML: `asset_licences` must be an array of single SPDX IDs.
pub fn parse_allowlist(text: &str, path: &str) -> Result<Allowlist, AttrError> {
    let table = text.parse::<toml::Table>().map_err(|e| parse_err(path, e.message().to_string()))?;
    let Some(value) = table.get("asset_licences") else {
        return Err(parse_err(path, "missing `asset_licences`"));
    };
    let Some(items) = value.as_array() else {
        return Err(parse_err(path, "`asset_licences` must be an array"));
    };
    let mut allow = Allowlist::default();
    for item in items {
        let Some(id) = item.as_str() else {
            return Err(parse_err(path, "`asset_licences` must hold strings"));
        };
        if !is_spdx_id(id) {
            return Err(parse_err(path, format!("`{id}` is not a single SPDX licence ID")));
        }
        allow.asset_licences.insert(id.to_ascii_lowercase(), id.to_string());
    }
    Ok(allow)
}

// ---------------------------------------------------------------------------------------------
// The attribution table
// ---------------------------------------------------------------------------------------------

/// The `origin` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    Original,
    ThirdParty,
}

/// One table row. Empty strings stand for cells left empty or written `-`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Row {
    pub path: String,
    pub origin: Origin,
    pub source: String,
    pub author: String,
    pub licence: String,
    /// 1-based line in `ATTRIBUTION.md`.
    pub line: usize,
}

fn clean_cell(raw: &str) -> String {
    let t = raw.trim();
    let t = match t.strip_prefix('`').and_then(|s| s.strip_suffix('`')) {
        Some(inner) => inner.trim(),
        None => t,
    };
    if t == "-" || t == "\u{2014}" || t == "\u{2013}" {
        String::new()
    } else {
        t.to_string()
    }
}

/// The cells of a Markdown table line (`| a | b |`), or `None` if the line is not a table line.
/// `\|` is a literal pipe inside a cell; the outer pipes are optional at the end.
pub fn split_row(line: &str) -> Option<Vec<String>> {
    let inner = line.trim().strip_prefix('|')?;
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cur.push('|');
                chars.next();
            }
            '|' => {
                cells.push(clean_cell(&cur));
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        cells.push(clean_cell(&cur));
    }
    Some(cells)
}

fn is_delimiter(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells.iter().all(|c| {
            let t = c.trim();
            let t = t.strip_prefix(':').unwrap_or(t);
            let t = t.strip_suffix(':').unwrap_or(t);
            !t.is_empty() && t.chars().all(|ch| ch == '-')
        })
}

fn normalise_path(raw: &str, label: &str, line: usize) -> Result<String, AttrError> {
    let p = raw.strip_prefix("./").unwrap_or(raw);
    if p.is_empty() {
        return Err(parse_err(label, format!("line {line}: empty path")));
    }
    if p.starts_with('/') || p.contains('\\') || p.split('/').any(|s| s == ".." || s == "." || s.is_empty()) {
        return Err(parse_err(label, format!("line {line}: path `{raw}` is not a root-relative path")));
    }
    Ok(p.to_string())
}

/// Parses the first table in `text` whose headers are [`HEADERS`] (case-insensitive, in order).
pub fn parse_attribution(text: &str, label: &str) -> Result<Vec<Row>, AttrError> {
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let header_at = lines.iter().position(|l| {
        split_row(l).is_some_and(|cells| {
            cells.len() == HEADERS.len() && cells.iter().zip(HEADERS).all(|(c, h)| c.eq_ignore_ascii_case(h))
        })
    });
    let Some(h) = header_at else {
        return Err(parse_err(label, "no table with headers path | origin | source | author | licence"));
    };
    let delim = lines.get(h + 1).and_then(|l| split_row(l));
    if !delim.is_some_and(|d| d.len() == HEADERS.len() && is_delimiter(&d)) {
        return Err(parse_err(label, format!("line {}: missing table delimiter row", h + 2)));
    }
    let mut rows = Vec::new();
    for (idx, raw) in lines.iter().enumerate().skip(h + 2) {
        let line = idx + 1;
        let Some(cells) = split_row(raw) else { break };
        let [path, origin, source, author, licence] = cells.as_slice() else {
            return Err(parse_err(
                label,
                format!("line {line}: expected {} cells, found {}", HEADERS.len(), cells.len()),
            ));
        };
        let origin = match origin.to_ascii_lowercase().as_str() {
            "original" => Origin::Original,
            "third-party" => Origin::ThirdParty,
            other => {
                return Err(parse_err(label, format!("line {line}: origin `{other}` is not original or third-party")))
            }
        };
        rows.push(Row {
            path: normalise_path(path, label, line)?,
            origin,
            source: source.clone(),
            author: author.clone(),
            licence: licence.clone(),
            line,
        });
    }
    Ok(rows)
}

/// Licence and completeness findings for one row (REQ-001, REQ-003, REQ-004).
pub fn check_row(row: &Row, allow: &Allowlist) -> Vec<Finding> {
    let mut out = Vec::new();
    let finding = |code, detail: String| Finding { code, subject: row.path.clone(), line: 0, detail };
    let shown = if row.licence.is_empty() { "(none)".to_string() } else { row.licence.clone() };
    match row.origin {
        Origin::Original => {
            if !row.licence.eq_ignore_ascii_case(ORIGINAL_LICENCE) {
                out.push(finding(Code::AttrLicence, shown));
            }
            if !row.author.eq_ignore_ascii_case(ORIGINAL_AUTHOR) {
                out.push(finding(Code::AttrIncomplete, format!("author must be `{ORIGINAL_AUTHOR}`")));
            }
        }
        Origin::ThirdParty => {
            if !allow.allows(&row.licence) {
                out.push(finding(Code::AttrLicence, shown));
            }
            let missing: Vec<&str> = [("source", &row.source), ("author", &row.author)]
                .iter()
                .filter(|(_, v)| v.is_empty())
                .map(|(k, _)| *k)
                .collect();
            if !missing.is_empty() {
                out.push(finding(Code::AttrIncomplete, format!("missing {}", missing.join(", "))));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Globs
// ---------------------------------------------------------------------------------------------

fn match_segment(p: &[char], s: &[char]) -> bool {
    let (mut pi, mut si) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        match p.get(pi) {
            Some('?') => {
                pi += 1;
                si += 1;
            }
            Some('*') => {
                star = Some((pi, si));
                pi += 1;
            }
            Some(c) if Some(c) == s.get(si) => {
                pi += 1;
                si += 1;
            }
            _ => match star {
                Some((sp, ss)) => {
                    pi = sp + 1;
                    si = ss + 1;
                    star = Some((sp, ss + 1));
                }
                None => return false,
            },
        }
    }
    while p.get(pi) == Some(&'*') {
        pi += 1;
    }
    pi == p.len()
}

fn match_segments(p: &[Vec<char>], s: &[Vec<char>]) -> bool {
    match p.split_first() {
        None => s.is_empty(),
        Some((first, rest)) if first.as_slice() == ['*', '*'].as_slice() => {
            (0..=s.len()).any(|i| match_segments(rest, s.get(i..).unwrap_or(&[])))
        }
        Some((first, rest)) => match s.split_first() {
            Some((seg, srest)) => match_segment(first, seg) && match_segments(rest, srest),
            None => false,
        },
    }
}

/// Whether the `/`-separated `path` matches `pattern`: `*` matches any run of characters and
/// `?` one character within a segment; a `**` segment matches zero or more whole segments
/// (consecutive `**` segments count as one).
/// Matching is case-sensitive.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let split = |t: &str| -> Vec<Vec<char>> { t.split('/').map(|s| s.chars().collect()).collect() };
    let mut pat = split(pattern);
    // Consecutive `**` segments match the same paths as one; collapsing them bounds backtracking.
    pat.dedup_by(|a, b| a.as_slice() == ['*', '*'].as_slice() && b.as_slice() == ['*', '*'].as_slice());
    match_segments(&pat, &split(path))
}

// ---------------------------------------------------------------------------------------------
// Origins and the font scan
// ---------------------------------------------------------------------------------------------

/// A URL origin: scheme, host (both lowercase) and port (scheme default when absent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlOrigin {
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
}

fn has_scheme(url: &str) -> Option<&str> {
    let colon = url.find(':')?;
    let scheme = url.get(..colon)?;
    let mut chars = scheme.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    (first_ok && chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')).then_some(scheme)
}

/// A URL as a browser's URL parser sees it before parsing: ASCII tab, CR and LF removed, leading
/// and trailing C0 controls and spaces trimmed, and `\` read as `/` (as for the special schemes
/// `http` and `https`, against which every relative URL on the site resolves).
pub fn normalise_url(raw: &str) -> String {
    let s: String = raw.chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect();
    s.trim_matches(|c: char| c <= ' ').replace('\\', "/")
}

/// The origin of a normalised `http` or `https` URL, or `None` when it cannot be decided safely:
/// another scheme, no slash after the scheme, an empty host, a host with `%`, or a bad port.
/// Like a browser, any number of slashes after the scheme is accepted and the authority ends at
/// the first `/`, `?` or `#`; userinfo up to the last `@` is dropped.
pub fn url_origin(url: &str) -> Option<UrlOrigin> {
    let scheme = has_scheme(url)?.to_ascii_lowercase();
    let default = match scheme.as_str() {
        "http" => 80,
        "https" => 443,
        _ => return None,
    };
    let rest = url.get(scheme.len() + 1..)?;
    let after = rest.trim_start_matches('/');
    if after.len() == rest.len() {
        return None;
    }
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let authority = after.get(..end)?;
    let hostport = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let (host, port) = if let Some(v6) = hostport.strip_prefix('[') {
        let (h, tail) = v6.split_once(']')?;
        (format!("[{}]", h.to_ascii_lowercase()), tail.strip_prefix(':'))
    } else {
        match hostport.rsplit_once(':') {
            Some((h, p)) => (h.to_ascii_lowercase(), Some(p)),
            None => (hostport.to_ascii_lowercase(), None),
        }
    };
    if host.is_empty() || host == "[]" || host.contains('%') {
        return None;
    }
    let port = match port {
        None | Some("") => default,
        Some(p) if p.bytes().all(|b| b.is_ascii_digit()) => p.parse::<u16>().ok()?,
        Some(_) => return None,
    };
    Some(UrlOrigin { scheme, host, port: Some(port) })
}

/// Parses `--origin`: `http://host[:port]` or `https://host[:port]`, optionally ending in `/`,
/// with no userinfo, backslash, control character or anything after the host.
pub fn parse_site_origin(url: &str) -> Result<UrlOrigin, AttrError> {
    let bad = || parse_err("--origin", format!("`{url}` is not an http(s) origin such as https://example.org"));
    let scheme_len = has_scheme(url).ok_or_else(bad)?.len();
    let tail = url.get(scheme_len + 1..).and_then(|t| t.strip_prefix("//")).ok_or_else(bad)?;
    let host = tail.strip_suffix('/').unwrap_or(tail);
    if host.is_empty() || host.chars().any(|c| c <= ' ' || matches!(c, '/' | '\\' | '@' | '?' | '#' | '%')) {
        return Err(bad());
    }
    url_origin(url).ok_or_else(bad)
}

/// Whether `url` (after [`normalise_url`]) may load from another origin than `site`. Relative
/// URLs and `data:` URLs do not. Fail closed: every URL starting `//` (also `\\`, `/\`, `\/`),
/// every scheme other than `http`, `https` and `data`, and every `http(s)` URL whose origin
/// cannot be decided counts as remote.
pub fn is_remote(url: &str, site: &UrlOrigin) -> bool {
    let u = normalise_url(url);
    if u.starts_with("//") {
        return true;
    }
    match has_scheme(&u) {
        None => false,
        Some(s) if s.eq_ignore_ascii_case("data") => false,
        Some(_) => url_origin(&u).map_or(true, |o| &o != site),
    }
}

const NAMED_ENTITIES: [(&str, char); 24] = [
    ("amp", '&'),
    ("lt", '<'),
    ("gt", '>'),
    ("quot", '"'),
    ("apos", '\''),
    ("colon", ':'),
    ("sol", '/'),
    ("bsol", '\\'),
    ("period", '.'),
    ("Tab", '\t'),
    ("NewLine", '\n'),
    ("commat", '@'),
    ("num", '#'),
    ("quest", '?'),
    ("percnt", '%'),
    ("lpar", '('),
    ("rpar", ')'),
    ("excl", '!'),
    ("equals", '='),
    ("plus", '+'),
    ("comma", ','),
    ("semi", ';'),
    ("lowbar", '_'),
    ("nbsp", '\u{a0}'),
];

/// Legacy entities a browser also decodes without `;` in an attribute value (when not followed
/// by an ASCII letter, digit or `=`), restricted to those in [`NAMED_ENTITIES`].
const LEGACY_ENTITIES: [&str; 5] = ["amp", "lt", "gt", "quot", "nbsp"];

/// Decodes character references in an HTML or XML attribute value: numeric references (with or
/// without `;`) and [`NAMED_ENTITIES`]. The flag is `false` when a named reference ending in `;`
/// is not known here, so the caller can fail closed.
pub fn decode_entities(s: &str) -> (String, bool) {
    let t: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut known = true;
    let mut i = 0usize;
    while let Some(&c) = t.get(i) {
        if c != '&' {
            out.push(c);
            i += 1;
            continue;
        }
        if t.get(i + 1) == Some(&'#') {
            let hex = matches!(t.get(i + 2), Some('x' | 'X'));
            let radix = if hex { 16 } else { 10 };
            let mut j = i + if hex { 3 } else { 2 };
            let mut value: u32 = 0;
            let mut digits = 0usize;
            while let Some(d) = t.get(j).and_then(|ch| ch.to_digit(radix)) {
                value = value.saturating_mul(radix).saturating_add(d).min(0x11_0000);
                digits += 1;
                j += 1;
            }
            if digits == 0 {
                out.push('&');
                i += 1;
                continue;
            }
            if t.get(j) == Some(&';') {
                j += 1;
            }
            out.push(if value == 0 { '\u{fffd}' } else { char::from_u32(value).unwrap_or('\u{fffd}') });
            i = j;
            continue;
        }
        let mut j = i + 1;
        while t.get(j).is_some_and(|ch| ch.is_ascii_alphanumeric()) && j - i <= 32 {
            j += 1;
        }
        let name: String = t.get(i + 1..j).map(|n| n.iter().collect()).unwrap_or_default();
        let found = NAMED_ENTITIES.iter().find(|(n, _)| *n == name).map(|(_, ch)| *ch);
        if t.get(j) == Some(&';') && !name.is_empty() {
            match found {
                Some(ch) => out.push(ch),
                None => {
                    known = false;
                    out.extend(t.get(i..=j).unwrap_or(&[]).iter());
                }
            }
            i = j + 1;
            continue;
        }
        let next_ok = !t.get(j).is_some_and(|ch| ch.is_ascii_alphanumeric() || *ch == '=');
        match found {
            Some(ch) if LEGACY_ENTITIES.contains(&name.as_str()) && next_ok => {
                out.push(ch);
                i = j;
            }
            _ => {
                out.push('&');
                i += 1;
            }
        }
    }
    (out, known)
}

fn line_at(text: &[u8], idx: usize) -> usize {
    1 + text.get(..idx).map_or(0, |p| p.iter().filter(|&&b| b == b'\n').count())
}

fn line_at_chars(text: &[char], idx: usize) -> usize {
    1 + text.get(..idx).map_or(0, |p| p.iter().filter(|&&c| c == '\n').count())
}

fn starts_with_ci(b: &[u8], at: usize, word: &[u8]) -> bool {
    b.get(at..at + word.len()).is_some_and(|s| s.eq_ignore_ascii_case(word))
}

/// `text` with CSS comments replaced by spaces (line breaks kept); strings are left alone.
fn strip_css_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                out.push(c);
                if c == '\\' {
                    if let Some(n) = chars.next() {
                        out.push(n);
                    }
                } else if c == q || c == '\n' {
                    quote = None;
                }
            }
            None if c == '\\' => {
                // An escape outside a string: keep it with the escaped character, so `\/*` does
                // not open a comment.
                out.push(c);
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            }
            None if c == '/' && chars.peek() == Some(&'*') => {
                chars.next();
                out.push_str("  ");
                let mut prev = ' ';
                for d in chars.by_ref() {
                    out.push(if d == '\n' { '\n' } else { ' ' });
                    if prev == '*' && d == '/' {
                        break;
                    }
                    prev = d;
                }
            }
            None => {
                if c == '"' || c == '\'' {
                    quote = Some(c);
                }
                out.push(c);
            }
        }
    }
    out
}

fn is_css_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n')
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-' || !c.is_ascii()
}

fn skip_css_ws(t: &[char], mut i: usize) -> usize {
    while t.get(i).is_some_and(|&c| is_css_ws(c)) {
        i += 1;
    }
    i
}

/// Decodes the CSS escape whose backslash is at `i` (CSS Syntax 4.3.7): up to six hex digits
/// and one optional whitespace, or the next character. `None` is an escaped line break (a line
/// continuation inside a string).
fn css_escape(t: &[char], i: usize) -> (Option<char>, usize) {
    let mut j = i + 1;
    match t.get(j) {
        None => (Some('\u{fffd}'), j),
        Some('\n') => (None, j + 1),
        Some(c) if c.is_ascii_hexdigit() => {
            let mut value: u32 = 0;
            let mut n = 0;
            while let Some(d) = t.get(j).and_then(|h| h.to_digit(16)) {
                if n == 6 {
                    break;
                }
                value = value * 16 + d;
                n += 1;
                j += 1;
            }
            if t.get(j).is_some_and(|&w| is_css_ws(w)) {
                j += 1;
            }
            (Some(if value == 0 { '\u{fffd}' } else { char::from_u32(value).unwrap_or('\u{fffd}') }), j)
        }
        Some(&c) => (Some(c), j + 1),
    }
}

/// An identifier starting at `i`, escapes decoded; returns it and the index after it.
fn read_ident(t: &[char], mut i: usize) -> (String, usize) {
    let mut out = String::new();
    while let Some(&c) = t.get(i) {
        if c == '\\' {
            if t.get(i + 1) == Some(&'\n') {
                break;
            }
            let (ch, next) = css_escape(t, i);
            out.extend(ch);
            i = next;
        } else if is_ident_char(c) {
            out.push(c);
            i += 1;
        } else {
            break;
        }
    }
    (out, i)
}

/// A string whose quote is at `i`, escapes decoded: its value, the index of its first character
/// and the index after it.
fn read_css_string(t: &[char], i: usize) -> (String, usize, usize) {
    let q = t.get(i).copied().unwrap_or('"');
    let start = i + 1;
    let mut j = start;
    let mut out = String::new();
    while let Some(&c) = t.get(j) {
        if c == '\\' {
            let (ch, next) = css_escape(t, j);
            out.extend(ch);
            j = next;
        } else if c == q {
            return (out, start, j + 1);
        } else if c == '\n' {
            return (out, start, j);
        } else {
            out.push(c);
            j += 1;
        }
    }
    (out, start, j)
}

/// The argument of a `url(` whose `(` is just before `i`: value, start index, index after `)`.
fn read_url_arg(t: &[char], i: usize) -> (String, usize, usize) {
    let i = skip_css_ws(t, i);
    if matches!(t.get(i), Some('"' | '\'')) {
        let (v, at, next) = read_css_string(t, i);
        let close = (next..t.len()).find(|&k| t.get(k) == Some(&')')).map_or(t.len(), |k| k + 1);
        return (v, at, close);
    }
    let mut out = String::new();
    let mut j = i;
    while let Some(&c) = t.get(j) {
        match c {
            ')' => return (out, i, j + 1),
            '\\' => {
                let (ch, next) = css_escape(t, j);
                out.extend(ch);
                j = next;
            }
            c if is_css_ws(c) => {
                let k = skip_css_ws(t, j);
                return (out, i, if t.get(k) == Some(&')') { k + 1 } else { k });
            }
            c => {
                out.push(c);
                j += 1;
            }
        }
    }
    (out, i, j)
}

/// The index after the block of the at-rule whose prelude starts at `i`: after the `}` that
/// closes its first `{` (strings skipped, nested braces counted), or after a `;` that ends the
/// rule first.
fn at_rule_end(t: &[char], mut i: usize) -> usize {
    let mut depth = 0usize;
    while let Some(&c) = t.get(i) {
        match c {
            '"' | '\'' => {
                i = read_css_string(t, i).2;
                continue;
            }
            '\\' => {
                i = css_escape(t, i).1;
                continue;
            }
            '{' => depth += 1,
            '}' if depth <= 1 => return i + 1,
            '}' => depth -= 1,
            ';' if depth == 0 => return i + 1,
            _ => {}
        }
        i += 1;
    }
    t.len()
}

/// `url()` values in `t[from..to]`, with function names matched after escape decoding.
fn urls_in(t: &[char], from: usize, to: usize, urls: &mut Vec<(String, usize)>) {
    let mut k = from;
    while k < to {
        match t.get(k) {
            Some('"' | '\'') => k = read_css_string(t, k).2.max(k + 1),
            Some(&c) if c == '\\' || is_ident_char(c) => {
                let (name, next) = read_ident(t, k);
                if name.eq_ignore_ascii_case("url") && t.get(next) == Some(&'(') {
                    let (v, at, after) = read_url_arg(t, next + 1);
                    urls.push((v, at));
                    k = after.max(next + 1);
                } else {
                    k = next.max(k + 1);
                }
            }
            _ => k += 1,
        }
    }
}

/// `FONT-REMOTE` findings for CSS text: `@import` targets and `url()` values inside
/// `@font-face` blocks, after CSS escapes are decoded (so `@\66ont-face`, `u\72l(` and
/// `https\3a //` are seen as written by a browser). An `@import` whose target is neither a
/// string nor `url()` is reported (fail closed). `first_line` is the line number of the first
/// line of `text`.
pub fn scan_css(file: &str, text: &str, first_line: usize, site: &UrlOrigin) -> Vec<Finding> {
    let pre = text.replace("\r\n", "\n").replace(['\r', '\u{c}'], "\n");
    let t: Vec<char> = strip_css_comments(&pre).chars().collect();
    let mut urls: Vec<(String, usize)> = Vec::new();
    let mut opaque: Vec<usize> = Vec::new();
    let mut i = 0usize;
    while let Some(&c) = t.get(i) {
        match c {
            '"' | '\'' => i = read_css_string(&t, i).2.max(i + 1),
            '\\' => i = css_escape(&t, i).1,
            '@' => {
                let (name, j) = read_ident(&t, i + 1);
                if name.eq_ignore_ascii_case("import") {
                    let k = skip_css_ws(&t, j);
                    match t.get(k) {
                        Some('"' | '\'') => {
                            let (v, at, next) = read_css_string(&t, k);
                            urls.push((v, at));
                            i = next;
                        }
                        _ => {
                            let (f, after) = read_ident(&t, k);
                            if f.eq_ignore_ascii_case("url") && t.get(after) == Some(&'(') {
                                let (v, at, next) = read_url_arg(&t, after + 1);
                                urls.push((v, at));
                                i = next;
                            } else {
                                opaque.push(k);
                                i = after.max(k + 1);
                            }
                        }
                    }
                } else if name.eq_ignore_ascii_case("font-face") {
                    let end = at_rule_end(&t, j);
                    urls_in(&t, j, end, &mut urls);
                    i = end.max(j + 1);
                } else {
                    i = j.max(i + 1);
                }
            }
            _ => i += 1,
        }
    }
    let finding = |at: usize, detail: String| Finding {
        code: Code::FontRemote,
        subject: file.to_string(),
        line: first_line + line_at_chars(&t, at) - 1,
        detail,
    };
    let mut out: Vec<Finding> =
        urls.into_iter().filter(|(u, _)| is_remote(u, site)).map(|(u, at)| finding(at, normalise_url(&u))).collect();
    out.extend(opaque.into_iter().map(|at| finding(at, "unrecognised @import target".to_string())));
    out
}

/// `text` with HTML comments replaced by spaces (line breaks kept).
fn strip_html_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        out.push_str(rest.get(..start).unwrap_or(""));
        let after = rest.get(start..).unwrap_or("");
        let end = after.get(4..).and_then(|a| a.find("-->")).map_or(after.len(), |p| p + 7);
        let comment = after.get(..end).unwrap_or(after);
        out.extend(comment.chars().map(|c| if c == '\n' { '\n' } else { ' ' }));
        rest = after.get(end..).unwrap_or("");
    }
    out.push_str(rest);
    out
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while b.get(i).is_some_and(|c| c.is_ascii_whitespace()) {
        i += 1;
    }
    i
}

fn find_ci(b: &[u8], from: usize, word: &[u8]) -> Option<usize> {
    (from..b.len()).find(|&i| starts_with_ci(b, i, word))
}

/// One attribute value: entity-decoded text, whether every reference was understood, and the
/// byte offset of its first character.
#[derive(Debug, Clone)]
struct AttrValue {
    text: String,
    known: bool,
    at: usize,
}

/// Attributes of a tag whose name ends just before `i`: lowercase name to its value (the first
/// occurrence wins, as in a browser), and the offset just past the closing `>`.
fn read_attributes(b: &[u8], mut i: usize) -> (BTreeMap<String, AttrValue>, usize) {
    let mut attrs = BTreeMap::new();
    loop {
        i = skip_ws(b, i);
        match b.get(i) {
            None => return (attrs, i),
            Some(b'>') => return (attrs, i + 1),
            Some(b'/') => {
                i += 1;
                continue;
            }
            Some(_) => {}
        }
        let name_start = i;
        while b.get(i).is_some_and(|&c| !c.is_ascii_whitespace() && c != b'=' && c != b'>' && c != b'/') {
            i += 1;
        }
        let name = String::from_utf8_lossy(b.get(name_start..i).unwrap_or(&[])).to_ascii_lowercase();
        if name.is_empty() {
            i += 1;
            continue;
        }
        let j = skip_ws(b, i);
        if b.get(j) != Some(&b'=') {
            attrs.entry(name).or_insert(AttrValue { text: String::new(), known: true, at: i });
            continue;
        }
        let v = skip_ws(b, j + 1);
        let (value, at, next) = match b.get(v) {
            Some(&q @ (b'"' | b'\'')) => {
                let end = b.get(v + 1..).and_then(|s| s.iter().position(|&c| c == q)).map_or(b.len(), |p| v + 1 + p);
                (b.get(v + 1..end).unwrap_or(&[]), v + 1, end + 1)
            }
            _ => {
                let end = b
                    .get(v..)
                    .and_then(|s| s.iter().position(|&c| c.is_ascii_whitespace() || c == b'>'))
                    .map_or(b.len(), |p| v + p);
                (b.get(v..end).unwrap_or(&[]), v, end)
            }
        };
        let (text, known) = decode_entities(&String::from_utf8_lossy(value));
        attrs.entry(name).or_insert(AttrValue { text, known, at });
        i = next;
    }
}

/// `FONT-REMOTE` findings for HTML, XHTML or SVG text: `<link>` elements whose `rel` includes
/// `stylesheet` or `preload`, `<base href>`, `<?xml-stylesheet href?>`, and the CSS inside
/// `<style>` elements. Attribute values are entity-decoded first; a value with a reference that
/// cannot be decoded here counts as remote.
pub fn scan_html(file: &str, text: &str, site: &UrlOrigin) -> Vec<Finding> {
    let clean = strip_html_comments(text);
    let b = clean.as_bytes();
    let mut out = Vec::new();
    let flag = |href: Option<&AttrValue>, out: &mut Vec<Finding>| {
        if let Some(v) = href {
            if !v.known || is_remote(&v.text, site) {
                out.push(Finding {
                    code: Code::FontRemote,
                    subject: file.to_string(),
                    line: line_at(b, v.at),
                    detail: normalise_url(&v.text),
                });
            }
        }
    };
    let mut i = 0usize;
    while let Some(lt) = (i..b.len()).find(|&k| b.get(k) == Some(&b'<')) {
        let tag_end_ok =
            |n: usize| b.get(lt + n).map_or(true, |&c| c.is_ascii_whitespace() || c == b'>' || c == b'/' || c == b'?');
        if starts_with_ci(b, lt + 1, b"link") && tag_end_ok(5) {
            let (attrs, next) = read_attributes(b, lt + 5);
            let rel = attrs.get("rel").map(|v| v.text.to_ascii_lowercase()).unwrap_or_default();
            if rel.split_ascii_whitespace().any(|r| r == "stylesheet" || r == "preload") {
                flag(attrs.get("href"), &mut out);
            }
            i = next.max(lt + 1);
        } else if starts_with_ci(b, lt + 1, b"base") && tag_end_ok(5) {
            let (attrs, next) = read_attributes(b, lt + 5);
            flag(attrs.get("href"), &mut out);
            i = next.max(lt + 1);
        } else if starts_with_ci(b, lt + 1, b"?xml-stylesheet") && tag_end_ok(16) {
            let (attrs, next) = read_attributes(b, lt + 16);
            flag(attrs.get("href"), &mut out);
            i = next.max(lt + 1);
        } else if starts_with_ci(b, lt + 1, b"style") && tag_end_ok(6) {
            let (_, content_start) = read_attributes(b, lt + 6);
            let content_end = find_ci(b, content_start, b"</style").unwrap_or(b.len());
            let css = String::from_utf8_lossy(b.get(content_start..content_end).unwrap_or(&[])).into_owned();
            out.extend(scan_css(file, &css, line_at(b, content_start), site));
            i = content_end.max(lt + 1);
        } else {
            i = lt + 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// The walk and the run
// ---------------------------------------------------------------------------------------------

fn extension(name: &str) -> Option<String> {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())
}

/// Whether a file name has an asset extension.
pub fn is_asset(name: &str) -> bool {
    extension(name).is_some_and(|e| ASSET_EXTENSIONS.contains(&e.as_str()))
}

fn is_css(name: &str) -> bool {
    extension(name).is_some_and(|e| e == "css")
}

fn is_markup(name: &str) -> bool {
    extension(name).is_some_and(|e| e == "html" || e == "htm" || e == "svg")
}

/// What [`walk`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tree {
    /// Sorted root-relative paths (with `/`) and full paths of the regular files.
    pub files: Vec<(String, PathBuf)>,
    /// Sorted root-relative paths of symbolic links (never followed).
    pub links: Vec<String>,
}

/// Every regular file and symbolic link under `root`, skipping [`SKIPPED_DIRS`] and anything
/// matched by an `excludes` glob. Symbolic links are listed, not followed; any other kind of
/// entry (socket, device, ...) is an error.
pub fn walk(root: &Path, excludes: &[String]) -> Result<Tree, AttrError> {
    let mut found = Vec::new();
    let mut links = Vec::new();
    let mut visited = 0usize;
    let mut stack: Vec<(PathBuf, String, usize)> = vec![(root.to_path_buf(), String::new(), 0)];
    while let Some((dir, prefix, depth)) = stack.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|e| io_err(&dir, e.to_string()))? {
            let entry = entry.map_err(|e| io_err(&dir, e.to_string()))?;
            visited += 1;
            if visited > MAX_WALK_ENTRIES {
                return Err(io_err(root, format!("more than {MAX_WALK_ENTRIES} entries")));
            }
            let ft = entry.file_type().map_err(|e| io_err(&entry.path(), e.to_string()))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            if excludes.iter().any(|g| glob_match(g, &rel)) {
                continue;
            }
            if ft.is_symlink() {
                links.push(rel);
            } else if ft.is_dir() {
                if SKIPPED_DIRS.contains(&name.as_str()) {
                    continue;
                }
                if depth >= MAX_WALK_DEPTH {
                    return Err(io_err(&entry.path(), format!("nested deeper than {MAX_WALK_DEPTH}")));
                }
                stack.push((entry.path(), rel, depth + 1));
            } else if ft.is_file() {
                found.push((rel, entry.path()));
            } else {
                return Err(io_err(&entry.path(), "neither a file, a directory nor a symbolic link"));
            }
        }
    }
    found.sort();
    links.sort();
    Ok(Tree { files: found, links })
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, AttrError> {
    let file = std::fs::File::open(path).map_err(|e| io_err(path, e.to_string()))?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|e| io_err(path, e.to_string()))?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(io_err(path, format!("larger than {MAX_FILE_BYTES} bytes")));
    }
    Ok(bytes)
}

fn read_utf8(path: &Path) -> Result<String, AttrError> {
    String::from_utf8(read_bytes(path)?).map_err(|_| io_err(path, "not UTF-8"))
}

/// Command options.
#[derive(Debug, Clone)]
pub struct Options {
    pub root: PathBuf,
    pub allow: PathBuf,
    pub origin: String,
    pub excludes: Vec<String>,
}

/// The result of a run that read all its inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Root-relative asset files checked against the table.
    pub assets: Vec<String>,
    /// Root-relative CSS, HTML and SVG files scanned for font and stylesheet origins.
    pub styles: Vec<String>,
    /// Number of table rows.
    pub rows: usize,
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn render(&self) -> String {
        let mut s = String::new();
        for f in &self.findings {
            s.push_str(&f.to_string());
            s.push('\n');
        }
        s.push_str(&format!(
            "asset-check: {} asset file(s), {} style or page file(s), {} row(s), {} finding(s)\n",
            self.assets.len(),
            self.styles.len(),
            self.rows,
            self.findings.len()
        ));
        s
    }

    /// Canonical JSON report (RFC 8785 via `fid-canon`); `Err` carries a serialisation failure.
    pub fn to_json(&self) -> Result<Vec<u8>, String> {
        let strs = |v: &[String]| CValue::Arr(v.iter().map(|s| CValue::Str(s.clone())).collect());
        let findings = self
            .findings
            .iter()
            .map(|f| {
                let mut o = BTreeMap::new();
                o.insert("code".to_string(), CValue::Str(f.code.as_str().to_string()));
                o.insert("subject".to_string(), CValue::Str(f.subject.clone()));
                o.insert("line".to_string(), CValue::U64(f.line as u64));
                o.insert("detail".to_string(), CValue::Str(f.detail.clone()));
                CValue::Obj(o)
            })
            .collect();
        let mut o = BTreeMap::new();
        o.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        o.insert("assets".to_string(), strs(&self.assets));
        o.insert("styles".to_string(), strs(&self.styles));
        o.insert("rows".to_string(), CValue::U64(self.rows as u64));
        o.insert("findings".to_string(), CValue::Arr(findings));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).map_err(|e| format!("{e:?}"))
    }
}

/// Runs the check.
pub fn run(opts: &Options) -> Result<Report, AttrError> {
    let allow_label = opts.allow.display().to_string();
    let allow = parse_allowlist(&read_utf8(&opts.allow)?, &allow_label)?;
    let site = parse_site_origin(&opts.origin)?;
    let tree = walk(&opts.root, &opts.excludes)?;
    let files = &tree.files;
    let attribution = opts.root.join(ATTRIBUTION_FILE);
    let rows = parse_attribution(&read_utf8(&attribution)?, ATTRIBUTION_FILE)?;

    let mut findings: Vec<Finding> = tree
        .links
        .iter()
        .map(|l| Finding { code: Code::AttrSymlink, subject: l.clone(), line: 0, detail: String::new() })
        .collect();
    let mut assets = Vec::new();
    let mut styles = Vec::new();
    for (rel, path) in files {
        let name = rel.rsplit('/').next().unwrap_or(rel.as_str());
        if is_asset(name) {
            if !rows.iter().any(|r| glob_match(&r.path, rel)) {
                findings.push(Finding {
                    code: Code::AttrMissing,
                    subject: rel.clone(),
                    line: 0,
                    detail: String::new(),
                });
            }
            assets.push(rel.clone());
        }
        if is_css(name) || is_markup(name) {
            let text = String::from_utf8_lossy(&read_bytes(path)?).into_owned();
            if is_css(name) {
                findings.extend(scan_css(rel, &text, 1, &site));
            } else {
                findings.extend(scan_html(rel, &text, &site));
            }
            styles.push(rel.clone());
        }
    }
    for row in &rows {
        if !files.iter().any(|(rel, _)| glob_match(&row.path, rel)) {
            findings.push(Finding {
                code: Code::AttrOrphan,
                subject: row.path.clone(),
                line: 0,
                detail: String::new(),
            });
        }
        findings.extend(check_row(row, &allow));
    }
    findings.sort();
    findings.dedup();
    Ok(Report { assets, styles, rows: rows.len(), findings })
}
