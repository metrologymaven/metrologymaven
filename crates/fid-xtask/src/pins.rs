//! `cargo xtask pins`: the repository map and cross-repository pin check (spec node
//! `ECO.REPO.TOPO`, REQ-ECO.REPO-005).
//!
//! `repos.toml` lists the first-party repositories (`[[repo]] id, url, purpose,
//! protected_tags`, optional `status`). The check reads the workspace root `Cargo.toml`
//! and every workspace member manifest with a bounded TOML reader (no network, no
//! `cargo metadata`), and for each `git` dependency whose normalised URL is a
//! first-party repository requires an immutable selector: a full 40-hex `rev`, or a
//! `tag` on a repository whose tags are protected.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask pins [--workspace <dir>] [--repos <repos.toml>]";

/// Largest file the reader accepts.
const MAX_FILE_BYTES: u64 = 1 << 20;
/// Most manifests one run reads.
const MAX_MANIFESTS: usize = 10_000;
/// Deepest nesting of arrays and inline tables.
const MAX_DEPTH: usize = 32;

/// Dependency tables of a manifest (and of `target.<cfg>`), including Cargo's legacy spellings.
const DEP_TABLES: [&str; 5] =
    ["dependencies", "dev-dependencies", "build-dependencies", "dev_dependencies", "build_dependencies"];

/// Keys a `[[repo]]` entry may have.
const REPO_KEYS: [&str; 5] = ["id", "url", "purpose", "protected_tags", "status"];

/// Entry point for `cargo xtask pins ...` (arguments after `pins`).
pub fn cli(args: &[String]) -> ExitCode {
    let mut workspace = PathBuf::from(".");
    let mut repos: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--workspace", Some(v)) => workspace = PathBuf::from(v),
            ("--repos", Some(v)) => repos = Some(PathBuf::from(v)),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let repos = repos.unwrap_or_else(|| workspace.join("repos.toml"));
    match run(&workspace, &repos) {
        Ok(findings) if findings.is_empty() => {
            println!("pins: ok");
            ExitCode::SUCCESS
        }
        Ok(findings) => {
            for f in findings {
                println!("{f}");
            }
            ExitCode::from(1)
        }
        Err(e) => {
            println!("{e}");
            ExitCode::from(1)
        }
    }
}

/// Loads the repository map, then checks every workspace manifest. Returns sorted
/// findings; `Err` is a bad repository map (`PIN-REPOS`) or an unreadable workspace.
fn run(workspace: &Path, repos_file: &Path) -> Result<Vec<String>, String> {
    let text = read_bounded(repos_file).map_err(|e| format!("PIN-REPOS {}: {e}", repos_file.display()))?;
    let repos = parse_repos(&text).map_err(|e| format!("PIN-REPOS {}: {e}", repos_file.display()))?;
    let mut findings = BTreeSet::new();
    for rel in manifests(workspace)? {
        let shown = if rel.is_empty() { "Cargo.toml".to_string() } else { format!("{rel}/Cargo.toml") };
        let doc = read_bounded(&workspace.join(&rel).join("Cargo.toml")).and_then(|t| parse_toml(&t));
        match doc {
            Ok(doc) => check_manifest(&doc, &shown, &repos, &mut findings),
            Err(e) => {
                findings.insert(format!("PIN-MANIFEST {shown} {e}"));
            }
        }
    }
    Ok(findings.into_iter().collect())
}

fn read_bounded(path: &Path) -> Result<String, String> {
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > MAX_FILE_BYTES {
        return Err(format!("larger than {MAX_FILE_BYTES} bytes"));
    }
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------------------
// Repository map
// ---------------------------------------------------------------------------------------

/// The repository map, keyed by normalised URL; the value is `protected_tags`.
type Repos = BTreeMap<String, bool>;

fn parse_repos(text: &str) -> Result<Repos, String> {
    let doc = parse_toml(text)?;
    if let Some(k) = doc.keys().find(|k| k.as_str() != "repo") {
        return Err(format!("unknown top-level key {k}"));
    }
    let Some(Val::Arr(list)) = doc.get("repo") else {
        return Err("expected [[repo]] entries".into());
    };
    let mut ids = BTreeSet::new();
    let mut repos = Repos::new();
    for (i, entry) in list.iter().enumerate() {
        let n = i + 1;
        let Val::Table(t) = entry else {
            return Err(format!("repo {n}: expected a table"));
        };
        if let Some(k) = t.keys().find(|k| !REPO_KEYS.contains(&k.as_str())) {
            return Err(format!("repo {n}: unknown key {k}"));
        }
        let text_field = |k: &str| match t.get(k) {
            Some(Val::Str(s)) if !s.trim().is_empty() => Ok(s.clone()),
            _ => Err(format!("repo {n}: {k} must be a non-empty string")),
        };
        let id = text_field("id")?;
        let url = text_field("url")?;
        text_field("purpose")?;
        let protected = match t.get("protected_tags") {
            Some(Val::Atom(a)) if a == "true" => true,
            Some(Val::Atom(a)) if a == "false" => false,
            _ => return Err(format!("repo {n}: protected_tags must be true or false")),
        };
        match t.get("status") {
            None => {}
            Some(Val::Str(s)) if ["active", "archived", "planned"].contains(&s.as_str()) => {}
            _ => return Err(format!("repo {n}: status must be \"active\", \"archived\" or \"planned\"")),
        }
        if !ids.insert(id.clone()) {
            return Err(format!("repo {n}: duplicate id {id}"));
        }
        if repos.insert(normalise_url(&url), protected).is_some() {
            return Err(format!("repo {n}: duplicate url {url}"));
        }
    }
    if repos.is_empty() {
        return Err("expected [[repo]] entries".into());
    }
    Ok(repos)
}

/// Normalises a git URL for comparison: drops the scheme and any user name, lower-cases
/// the host, turns `user@host:path` into `host/path`, drops a query or fragment, and
/// strips trailing `/` and `.git`. The path keeps its case.
fn normalise_url(url: &str) -> String {
    let u = url.trim();
    let (rest, had_scheme) = match u.find("://") {
        Some(i) if i > 0 && u[..i].bytes().all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b)) => {
            (&u[i + 3..], true)
        }
        _ => (u, false),
    };
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let (mut authority, mut path) = match rest.find('/') {
        Some(i) => (rest[..i].to_string(), rest[i..].to_string()),
        None => (rest.to_string(), String::new()),
    };
    if !had_scheme {
        if let Some(i) = authority.find(':') {
            path = format!("/{}{path}", &authority[i + 1..]);
            authority.truncate(i);
        }
    }
    let host = authority.rsplit('@').next().unwrap_or("").to_ascii_lowercase();
    loop {
        let before = path.len();
        while path.ends_with('/') {
            path.pop();
        }
        if let Some(p) = path.strip_suffix(".git") {
            path = p.to_string();
        }
        if path.len() == before {
            break;
        }
    }
    format!("{host}{path}")
}

// ---------------------------------------------------------------------------------------
// Workspace manifests
// ---------------------------------------------------------------------------------------

/// Relative directories (`""` for the root) of the root manifest and every workspace member.
fn manifests(ws: &Path) -> Result<Vec<String>, String> {
    let root = ws.join("Cargo.toml");
    let text = read_bounded(&root).map_err(|e| format!("PIN-IO {}: {e}", root.display()))?;
    let doc = parse_toml(&text).map_err(|e| format!("PIN-MANIFEST Cargo.toml {e}"))?;
    let mut out = BTreeSet::from([String::new()]);
    let Some(Val::Table(w)) = doc.get("workspace") else {
        return Ok(out.into_iter().collect());
    };
    let strings = |k: &str| -> Result<Vec<String>, String> {
        match w.get(k) {
            None => Ok(Vec::new()),
            Some(Val::Arr(a)) => a
                .iter()
                .map(|v| match v {
                    Val::Str(s) => Ok(clean_rel(s)),
                    _ => Err(format!("PIN-MANIFEST Cargo.toml workspace.{k} must list strings")),
                })
                .collect(),
            Some(_) => Err(format!("PIN-MANIFEST Cargo.toml workspace.{k} must be an array")),
        }
    };
    let exclude = strings("exclude")?;
    for pattern in strings("members")? {
        for rel in expand(ws, &pattern) {
            let excluded = exclude.iter().any(|x| rel == *x || rel.starts_with(&format!("{x}/")));
            if !excluded && ws.join(&rel).join("Cargo.toml").is_file() {
                out.insert(rel);
            }
            if out.len() > MAX_MANIFESTS {
                return Err(format!("PIN-IO more than {MAX_MANIFESTS} manifests"));
            }
        }
    }
    Ok(out.into_iter().collect())
}

/// `./a//b/` -> `a/b`, with `\` read as `/`.
fn clean_rel(s: &str) -> String {
    s.replace('\\', "/").split('/').filter(|x| !x.is_empty() && *x != ".").collect::<Vec<_>>().join("/")
}

/// Expands a member pattern; `*` and `?` match within one path segment.
fn expand(ws: &Path, pattern: &str) -> Vec<String> {
    let mut cur = vec![String::new()];
    for seg in pattern.split('/').filter(|s| !s.is_empty()) {
        let mut next = BTreeSet::new();
        for base in &cur {
            if seg.contains(['*', '?']) {
                let Ok(rd) = std::fs::read_dir(ws.join(base)) else { continue };
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if !name.starts_with('.') && e.path().is_dir() && wildcard(seg.as_bytes(), name.as_bytes()) {
                        next.insert(join_rel(base, &name));
                    }
                }
            } else {
                next.insert(join_rel(base, seg));
            }
        }
        cur = next.into_iter().collect();
    }
    cur
}

fn join_rel(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else {
        format!("{base}/{name}")
    }
}

fn wildcard(p: &[u8], s: &[u8]) -> bool {
    let (mut pi, mut si) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        if pi < p.len() && (p[pi] == b'?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star = Some((pi, si));
            pi += 1;
        } else if let Some((sp, ss)) = star {
            pi = sp + 1;
            si = ss + 1;
            star = Some((sp, ss + 1));
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

/// Every `(name, spec table)` in the dependency tables of one manifest: the plain and
/// target-specific tables, `workspace.dependencies`, `patch.<source>` and `replace`.
fn dependency_specs(doc: &Table) -> Vec<(&str, &Table)> {
    let mut tables: Vec<&Table> = Vec::new();
    push_dep_tables(doc, &mut tables);
    if let Some(w) = sub(doc, "workspace") {
        tables.extend(sub(w, "dependencies"));
    }
    for k in ["target", "patch"] {
        if let Some(all) = sub(doc, k) {
            for v in all.values() {
                if let Val::Table(t) = v {
                    if k == "target" {
                        push_dep_tables(t, &mut tables);
                    } else {
                        tables.push(t);
                    }
                }
            }
        }
    }
    tables.extend(sub(doc, "replace"));
    let mut specs = Vec::new();
    for t in tables {
        for (name, v) in t {
            if let Val::Table(spec) = v {
                specs.push((name.as_str(), spec));
            }
        }
    }
    specs
}

fn sub<'t>(t: &'t Table, k: &str) -> Option<&'t Table> {
    match t.get(k) {
        Some(Val::Table(m)) => Some(m),
        _ => None,
    }
}

fn push_dep_tables<'t>(t: &'t Table, out: &mut Vec<&'t Table>) {
    for k in DEP_TABLES {
        if let Some(m) = sub(t, k) {
            out.push(m);
        }
    }
}

fn check_manifest(doc: &Table, file: &str, repos: &Repos, findings: &mut BTreeSet<String>) {
    for (name, spec) in dependency_specs(doc) {
        let Some(Val::Str(git)) = spec.get("git") else { continue };
        let Some(&protected_tags) = repos.get(&normalise_url(git)) else { continue };
        let code = if spec.contains_key("branch") {
            Some("PIN-MOVING")
        } else if let Some(rev) = spec.get("rev") {
            let full = matches!(rev, Val::Str(r) if r.len() == 40 && r.bytes().all(|b| b.is_ascii_hexdigit()));
            (!full).then_some("PIN-REV-SHORT")
        } else if spec.contains_key("tag") {
            (!protected_tags).then_some("PIN-TAG-UNPROTECTED")
        } else {
            Some("PIN-MOVING")
        };
        if let Some(code) = code {
            findings.insert(format!("{code} {file} {name}"));
        }
    }
}

// ---------------------------------------------------------------------------------------
// Bounded TOML reader (the subset Cargo manifests and repos.toml use)
// ---------------------------------------------------------------------------------------

type Table = BTreeMap<String, Val>;

#[derive(Debug, Clone, PartialEq)]
enum Val {
    Str(String),
    Arr(Vec<Val>),
    Table(Table),
    /// A number, boolean or date, as written.
    Atom(String),
}

/// Parses a TOML document into nested tables. Supports tables, arrays of tables, dotted
/// keys, all four string forms, arrays and inline tables (both may span lines);
/// numbers, booleans and dates are kept as text. Duplicate keys are an error.
fn parse_toml(text: &str) -> Result<Table, String> {
    let mut p = Parser { s: text.as_bytes(), pos: 0, line: 1 };
    p.document().map_err(|e| format!("line {}: {e}", p.line))
}

struct Parser<'a> {
    s: &'a [u8],
    pos: usize,
    line: usize,
}

fn table_at<'m>(mut t: &'m mut Table, path: &[String]) -> Result<&'m mut Table, String> {
    for seg in path {
        let v = t.entry(seg.clone()).or_insert_with(|| Val::Table(Table::new()));
        t = match v {
            Val::Table(m) => m,
            Val::Arr(a) => match a.last_mut() {
                Some(Val::Table(m)) => m,
                _ => return Err(format!("key {seg} is not a table")),
            },
            _ => return Err(format!("key {seg} is not a table")),
        };
    }
    Ok(t)
}

fn insert(t: &mut Table, current: &[String], key: &[String], v: Val) -> Result<(), String> {
    let Some((last, parents)) = key.split_last() else {
        return Err("empty key".into());
    };
    let path: Vec<String> = current.iter().chain(parents).cloned().collect();
    let t = table_at(t, &path)?;
    if t.contains_key(last) {
        return Err(format!("duplicate key {last}"));
    }
    t.insert(last.clone(), v);
    Ok(())
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    fn starts(&self, lit: &[u8]) -> bool {
        self.s.get(self.pos..).is_some_and(|r| r.starts_with(lit))
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
        }
        Some(b)
    }

    fn expect(&mut self, b: u8) -> Result<(), String> {
        if self.bump() == Some(b) {
            Ok(())
        } else {
            Err(format!("expected '{}'", b as char))
        }
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.pos += 1;
        }
    }

    fn skip_comment(&mut self) {
        if self.peek() == Some(b'#') {
            while !matches!(self.peek(), None | Some(b'\n')) {
                self.pos += 1;
            }
        }
    }

    /// Whitespace, comments and newlines.
    fn skip_blank(&mut self) {
        loop {
            self.skip_ws();
            self.skip_comment();
            match self.peek() {
                Some(b'\n' | b'\r') => {
                    self.bump();
                }
                _ => return,
            }
        }
    }

    fn end_of_line(&mut self) -> Result<(), String> {
        self.skip_ws();
        self.skip_comment();
        if self.starts(b"\r\n") {
            self.pos += 1;
        }
        match self.peek() {
            None => Ok(()),
            Some(b'\n') => {
                self.bump();
                Ok(())
            }
            Some(_) => Err("expected end of line".into()),
        }
    }

    fn document(&mut self) -> Result<Table, String> {
        let mut root = Table::new();
        let mut current: Vec<String> = Vec::new();
        loop {
            self.skip_blank();
            match self.peek() {
                None => return Ok(root),
                Some(b'[') => {
                    let array = self.starts(b"[[");
                    self.pos += if array { 2 } else { 1 };
                    self.skip_ws();
                    let key = self.key()?;
                    self.skip_ws();
                    self.expect(b']')?;
                    if array {
                        self.expect(b']')?;
                        let Some((last, parents)) = key.split_last() else {
                            return Err("empty key".into());
                        };
                        let parent = table_at(&mut root, parents)?;
                        match parent.entry(last.clone()).or_insert_with(|| Val::Arr(Vec::new())) {
                            Val::Arr(a) => a.push(Val::Table(Table::new())),
                            _ => return Err(format!("key {last} is not an array of tables")),
                        }
                    } else {
                        table_at(&mut root, &key)?;
                    }
                    current = key;
                }
                Some(_) => {
                    let key = self.key()?;
                    self.skip_ws();
                    self.expect(b'=')?;
                    self.skip_ws();
                    let v = self.value(0)?;
                    insert(&mut root, &current, &key, v)?;
                }
            }
            self.end_of_line()?;
        }
    }

    /// A dotted key.
    fn key(&mut self) -> Result<Vec<String>, String> {
        let mut parts = vec![self.simple_key()?];
        loop {
            self.skip_ws();
            if self.peek() != Some(b'.') {
                return Ok(parts);
            }
            self.pos += 1;
            self.skip_ws();
            parts.push(self.simple_key()?);
        }
    }

    fn simple_key(&mut self) -> Result<String, String> {
        match self.peek() {
            Some(b'"') => {
                self.pos += 1;
                self.basic_string(false)
            }
            Some(b'\'') => {
                self.pos += 1;
                self.literal_string(false)
            }
            _ => {
                let start = self.pos;
                while self.peek().is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
                    self.pos += 1;
                }
                if self.pos == start {
                    return Err("expected a key".into());
                }
                Ok(String::from_utf8_lossy(&self.s[start..self.pos]).into_owned())
            }
        }
    }

    fn value(&mut self, depth: usize) -> Result<Val, String> {
        if depth > MAX_DEPTH {
            return Err("nested too deeply".into());
        }
        match self.peek() {
            Some(b'"') if self.starts(b"\"\"\"") => {
                self.pos += 3;
                self.skip_first_newline();
                self.basic_string(true).map(Val::Str)
            }
            Some(b'"') => {
                self.pos += 1;
                self.basic_string(false).map(Val::Str)
            }
            Some(b'\'') if self.starts(b"'''") => {
                self.pos += 3;
                self.skip_first_newline();
                self.literal_string(true).map(Val::Str)
            }
            Some(b'\'') => {
                self.pos += 1;
                self.literal_string(false).map(Val::Str)
            }
            Some(b'[') => {
                self.pos += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_blank();
                    if self.peek() == Some(b']') {
                        self.pos += 1;
                        return Ok(Val::Arr(items));
                    }
                    items.push(self.value(depth + 1)?);
                    self.skip_blank();
                    match self.bump() {
                        Some(b',') => {}
                        Some(b']') => return Ok(Val::Arr(items)),
                        _ => return Err("expected ',' or ']' in array".into()),
                    }
                }
            }
            Some(b'{') => {
                self.pos += 1;
                let mut t = Table::new();
                self.skip_blank();
                if self.peek() == Some(b'}') {
                    self.pos += 1;
                    return Ok(Val::Table(t));
                }
                loop {
                    self.skip_blank();
                    let key = self.key()?;
                    self.skip_ws();
                    self.expect(b'=')?;
                    self.skip_ws();
                    let v = self.value(depth + 1)?;
                    insert(&mut t, &[], &key, v)?;
                    self.skip_blank();
                    match self.bump() {
                        Some(b',') => {}
                        Some(b'}') => return Ok(Val::Table(t)),
                        _ => return Err("expected ',' or '}' in inline table".into()),
                    }
                }
            }
            _ => {
                let start = self.pos;
                while !matches!(self.peek(), None | Some(b' ' | b'\t' | b'\r' | b'\n' | b',' | b']' | b'}' | b'#')) {
                    self.pos += 1;
                }
                if self.pos == start {
                    return Err("expected a value".into());
                }
                Ok(Val::Atom(String::from_utf8_lossy(&self.s[start..self.pos]).into_owned()))
            }
        }
    }

    fn skip_first_newline(&mut self) {
        if self.starts(b"\r\n") {
            self.pos += 1;
        }
        if self.peek() == Some(b'\n') {
            self.bump();
        }
    }

    /// The closing delimiter of a multi-line string may carry up to two extra quotes.
    fn close_multi(&mut self, q: u8, out: &mut Vec<u8>) -> bool {
        let triple = [q, q, q];
        if !self.starts(&triple) {
            return false;
        }
        let mut n = 3;
        while n < 5 && self.s.get(self.pos + n) == Some(&q) {
            n += 1;
        }
        for _ in 3..n {
            out.push(q);
        }
        self.pos += n;
        true
    }

    /// The rest of a basic string, after its opening quote(s).
    fn basic_string(&mut self, multi: bool) -> Result<String, String> {
        let mut out = Vec::new();
        loop {
            if multi && self.close_multi(b'"', &mut out) {
                break;
            }
            match self.bump() {
                None => return Err("unterminated string".into()),
                Some(b'"') if !multi => break,
                Some(b'\n') if !multi => return Err("newline in string".into()),
                Some(b'\\') => {
                    let esc = self.bump().ok_or("unterminated string")?;
                    let c = match esc {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'n' => '\n',
                        b't' => '\t',
                        b'r' => '\r',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'e' => '\u{1b}',
                        b'u' | b'U' => {
                            let n = if esc == b'u' { 4 } else { 8 };
                            let hex = self.s.get(self.pos..self.pos + n).ok_or("short unicode escape")?;
                            let hex = std::str::from_utf8(hex).map_err(|_| "bad unicode escape")?;
                            let code = u32::from_str_radix(hex, 16).map_err(|_| "bad unicode escape")?;
                            self.pos += n;
                            char::from_u32(code).ok_or("bad unicode escape")?
                        }
                        b' ' | b'\t' | b'\r' | b'\n' if multi => {
                            // Line-ending backslash: drop the newline and leading whitespace.
                            while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
                                self.bump();
                            }
                            continue;
                        }
                        _ => return Err("bad escape".into()),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
                Some(b) => out.push(b),
            }
        }
        String::from_utf8(out).map_err(|_| "string is not UTF-8".into())
    }

    /// The rest of a literal string, after its opening quote(s).
    fn literal_string(&mut self, multi: bool) -> Result<String, String> {
        let mut out = Vec::new();
        loop {
            if multi && self.close_multi(b'\'', &mut out) {
                break;
            }
            match self.bump() {
                None => return Err("unterminated string".into()),
                Some(b'\'') if !multi => break,
                Some(b'\n') if !multi => return Err("newline in string".into()),
                Some(b) => out.push(b),
            }
        }
        String::from_utf8(out).map_err(|_| "string is not UTF-8".into())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(x: &str) -> Val {
        Val::Str(x.into())
    }

    #[test]
    fn reader_handles_manifest_shapes() {
        let doc = parse_toml(concat!(
            "# comment\n",
            "[package]\n",
            "name = \"a\" # trailing\n",
            "version.workspace = true\n",
            "\n",
            "[dependencies]\n",
            "x = { git = 'https://h/x', rev = \"0123\", features = [\n  \"f1\", # c\n  \"f2\",\n] }\n",
            "\n",
            "[target.'cfg(windows)'.dependencies.y]\n",
            "git = \"\"\"\nhttps://h/y\"\"\"\n",
            "branch = '''main'''\n",
            "\n",
            "[[bin]]\n",
            "name = \"one\"\n",
            "[[bin]]\n",
            "name = \"t\\u00e9st\"\n",
        ))
        .unwrap();
        let Some(Val::Table(pkg)) = doc.get("package") else { panic!() };
        assert_eq!(pkg.get("name"), Some(&s("a")));
        assert_eq!(
            pkg.get("version"),
            Some(&Val::Table(Table::from([("workspace".into(), Val::Atom("true".into()))])))
        );
        let Some(Val::Table(deps)) = doc.get("dependencies") else { panic!() };
        let Some(Val::Table(x)) = deps.get("x") else { panic!() };
        assert_eq!(x.get("git"), Some(&s("https://h/x")));
        assert_eq!(x.get("features"), Some(&Val::Arr(vec![s("f1"), s("f2")])));
        let specs = dependency_specs(&doc);
        let names: Vec<&str> = specs.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ["x", "y"]);
        assert_eq!(specs[1].1.get("git"), Some(&s("https://h/y")));
        let Some(Val::Arr(bins)) = doc.get("bin") else { panic!() };
        assert_eq!(bins.len(), 2);
        assert_eq!(bins[1], Val::Table(Table::from([("name".into(), s("t\u{e9}st"))])));
    }

    #[test]
    fn reader_rejects_bad_input() {
        for bad in ["a = 1\na = 2\n", "a = \"open\n", "[x\n", "a = [1 2]\n", "a = 1 b = 2\n", "= 1\n"] {
            assert!(parse_toml(bad).is_err(), "{bad:?}");
        }
        let deep = format!("a = {}{}\n", "[".repeat(40), "]".repeat(40));
        assert!(parse_toml(&deep).is_err());
    }

    /// REQ-ECO.REPO.TOPO-004, hand-labelled.
    #[test]
    fn urls_normalise() {
        let base = "example.org/fid/libs";
        for u in [
            "https://example.org/fid/libs.git",
            "HTTPS://Example.org/fid/libs",
            "https://example.org/fid/libs/",
            "http://EXAMPLE.ORG/fid/libs.git/",
            "ssh://git@example.org/fid/libs.git",
            "git@example.org:fid/libs.git",
            "git+https://example.org/fid/libs?x=1",
        ] {
            assert_eq!(normalise_url(u), base, "{u}");
        }
        assert_ne!(normalise_url("https://example.org/fid/Libs"), base);
        assert_ne!(normalise_url("https://example.org/fid/libs2"), base);
    }

    #[test]
    fn wildcards() {
        assert!(wildcard(b"*", b"fid-num"));
        assert!(wildcard(b"fid-*", b"fid-num"));
        assert!(wildcard(b"f?d-*m", b"fid-num"));
        assert!(!wildcard(b"fid-*", b"xtask"));
        assert!(!wildcard(b"a*b", b"a"));
    }

    #[test]
    fn bad_repo_maps_are_refused() {
        let ok = "[[repo]]\nid = \"a\"\nurl = \"https://h/a\"\npurpose = \"p\"\nprotected_tags = false\n";
        assert!(parse_repos(ok).is_ok());
        for (text, why) in [
            ("", "empty"),
            ("[repo]\nid = \"a\"\n", "not an array of tables"),
            (&ok.replace("protected_tags = false", "protected_tags = \"no\"")[..], "flag not boolean"),
            (&ok.replace("purpose = \"p\"\n", "")[..], "missing purpose"),
            (&ok.replace("id = \"a\"", "id = \"a\"\nowner = \"x\"")[..], "unknown key"),
            (&format!("{ok}status = \"gone\"\n")[..], "bad status"),
            (
                &format!("{ok}{}", ok.replace("id = \"a\"", "id = \"b\"").replace("https://h/a", "HTTPS://H/a.git"))[..],
                "dup url",
            ),
            (&format!("{ok}{}", ok.replace("https://h/a", "https://h/b"))[..], "dup id"),
            (&format!("x = 1\n{ok}")[..], "unknown top-level key"),
        ] {
            assert!(parse_repos(text).is_err(), "{why}");
        }
    }
}
