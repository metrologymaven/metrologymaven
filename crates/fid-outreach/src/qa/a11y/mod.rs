//! `fid-outreach qa a11y`: static accessibility gate for built pages (spec node
//! `OUTR.SITE.QA.A11Y`).
//!
//! Every `.html`/`.htm` page and `.css` stylesheet under the site root is read and checked
//! against rules A01 to A13, each mapped to a WCAG 2.2 success criterion. Contrast (A12) is
//! computed per theme of the token file; colours are resolved only through token custom
//! properties, and any literal colour is rule A13. The rule set is the automatable subset of
//! WCAG 2.2 AA for static pages, not a conformance claim.
//!
//! Token file (placeholder format until UX.SYSTEM fixes the package format):
//! `{"themes": {"light": {"text.muted": "#5c6670"}, "dark": {}}}`. A value may also be an
//! object with a `$value` string. Token `text.muted` is custom property `--text-muted`.
//!
//! Configuration: `{"suppressions": [{"page": "a.html", "rule": "A04", "reason": "..."}]}`.

pub mod css;
pub mod html;
pub mod rules;
pub mod vocab;

use css::{Rgba, Scope, StyleRule, WHITE};
use fid_canon::{parse as parse_json, to_canonical_bytes, CValue};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// Largest page, stylesheet, token or configuration file accepted, in bytes.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// Most pages and stylesheets accepted under one root.
pub const MAX_FILES: usize = 100_000;
const MAX_DIR_DEPTH: usize = 64;

/// Report schema identifier.
pub const SCHEMA: &str = "a11y-report@1";

/// Rule IDs, WCAG 2.2 success criteria and short titles.
pub const RULES: [(&str, &str, &str); 13] = [
    ("A01", "1.1.1", "image without text alternative"),
    ("A02", "1.3.1", "heading level skipped or not exactly one h1"),
    ("A03", "1.3.1, 4.1.2", "form control without a programmatic label"),
    ("A04", "1.3.1", "data table without header cells"),
    ("A05", "2.4.2", "missing, empty or shared page title"),
    ("A06", "2.4.4, 4.1.2", "link or button without an accessible name"),
    ("A07", "3.1.1", "missing or invalid lang on html"),
    ("A08", "4.1.2", "duplicate id or ARIA attribute or role not in WAI-ARIA 1.2"),
    ("A09", "2.4.1", "no main landmark or no skip link to it"),
    ("A10", "1.4.4", "viewport setting that disables zoom"),
    ("A11", "1.4.2", "autoplay media not muted or without controls"),
    ("A12", "1.4.3", "text contrast below the minimum"),
    ("A13", "1.4.3", "literal colour not resolved through a token custom property"),
];

/// WCAG 2.2 success criterion (or criteria) of a rule ID; empty for unknown IDs.
pub fn success_criterion(rule: &str) -> &'static str {
    RULES.iter().find(|(id, _, _)| *id == rule).map_or("", |(_, sc, _)| *sc)
}

/// One violation. The derived order is the report order: page, line, rule, element, detail.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Violation {
    /// Page or stylesheet path relative to the root, with `/` separators.
    pub page: String,
    pub line: u32,
    pub rule: String,
    /// Offending element (HTML) or selector (CSS).
    pub element: String,
    pub detail: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{} {} (WCAG {}) {}: {}",
            self.page,
            self.line,
            self.rule,
            success_criterion(&self.rule),
            self.element,
            self.detail
        )
    }
}

/// A configured suppression: page, rule and a non-empty reason.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Suppression {
    pub page: String,
    pub rule: String,
    pub reason: String,
}

/// Unreadable or unparsable input (exit code 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum A11yError {
    /// `A11Y-IO`: a file or directory could not be read.
    Io { file: String, message: String },
    /// `A11Y-PARSE(file)`: a page, stylesheet, token or configuration file could not be parsed.
    Parse { file: String, message: String },
}

impl A11yError {
    /// Stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            A11yError::Io { .. } => "A11Y-IO",
            A11yError::Parse { .. } => "A11Y-PARSE",
        }
    }

    /// The file the error is about.
    pub fn file(&self) -> &str {
        match self {
            A11yError::Io { file, .. } | A11yError::Parse { file, .. } => file,
        }
    }

    fn message(&self) -> &str {
        match self {
            A11yError::Io { message, .. } | A11yError::Parse { message, .. } => message,
        }
    }
}

impl fmt::Display for A11yError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({}): {}", self.code(), self.file(), self.message())
    }
}

impl std::error::Error for A11yError {}

/// Inputs of one gate run.
#[derive(Debug, Clone)]
pub struct Options {
    pub root: PathBuf,
    pub tokens: PathBuf,
    pub config: Option<PathBuf>,
}

/// Result of a gate run.
#[derive(Debug, Clone)]
pub struct Report {
    pub pages: usize,
    pub stylesheets: usize,
    pub themes: Vec<String>,
    /// Unsuppressed violations, sorted.
    pub violations: Vec<Violation>,
    /// Violations matched by a suppression, sorted.
    pub suppressed: Vec<Violation>,
    /// Suppressions that matched at least one violation, with the match count.
    pub used: Vec<(Suppression, usize)>,
    /// Suppressions that matched nothing.
    pub unmatched: Vec<Suppression>,
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn num(v: usize) -> CValue {
    CValue::Num(v as f64)
}

fn obj(fields: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn violation_json(v: &Violation) -> CValue {
    obj(vec![
        ("page", s(&v.page)),
        ("line", CValue::Num(f64::from(v.line))),
        ("rule", s(&v.rule)),
        ("sc", s(success_criterion(&v.rule))),
        ("element", s(&v.element)),
        ("detail", s(&v.detail)),
    ])
}

fn suppression_json(x: &Suppression, matched: Option<usize>) -> CValue {
    let mut f = vec![("page", s(&x.page)), ("rule", s(&x.rule)), ("reason", s(&x.reason))];
    if let Some(m) = matched {
        f.push(("matched", num(m)));
    }
    obj(f)
}

impl Report {
    /// 0 when there is no unsuppressed violation, else 1.
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.violations.is_empty())
    }

    /// The report as canonical JSON (RFC 8785), byte-identical for identical inputs.
    pub fn to_json(&self) -> Result<Vec<u8>, String> {
        let v = obj(vec![
            ("schema", s(SCHEMA)),
            ("result", s(if self.violations.is_empty() { "pass" } else { "fail" })),
            ("pages", num(self.pages)),
            ("stylesheets", num(self.stylesheets)),
            ("themes", CValue::Arr(self.themes.iter().cloned().map(CValue::Str).collect())),
            ("violations", CValue::Arr(self.violations.iter().map(violation_json).collect())),
            ("suppressed", CValue::Arr(self.suppressed.iter().map(violation_json).collect())),
            (
                "suppressions",
                obj(vec![
                    ("used", CValue::Arr(self.used.iter().map(|(x, m)| suppression_json(x, Some(*m))).collect())),
                    ("unmatched", CValue::Arr(self.unmatched.iter().map(|x| suppression_json(x, None)).collect())),
                ]),
            ),
        ]);
        to_canonical_bytes(&v).map_err(|e| e.to_string())
    }
}

/// Report written when the run fails with exit code 2.
pub fn error_json(e: &A11yError) -> Vec<u8> {
    let v = obj(vec![
        ("schema", s(SCHEMA)),
        ("result", s("error")),
        ("error", obj(vec![("code", s(e.code())), ("file", s(e.file())), ("message", s(e.message()))])),
    ]);
    to_canonical_bytes(&v).unwrap_or_default()
}

fn show(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Token colours per theme, keyed by custom property name.
#[derive(Debug, Clone, Default)]
pub struct Tokens {
    pub themes: BTreeMap<String, BTreeMap<String, Rgba>>,
    /// Every custom property name that carries a token colour in some theme.
    pub props: BTreeSet<String>,
}

/// Reads the theme names and colour values of the token file.
pub fn load_tokens(bytes: &[u8]) -> Result<Tokens, String> {
    let v = parse_json(bytes).map_err(|e| format!("invalid JSON: {e}"))?;
    let CValue::Obj(top) = v else { return Err("token file is not a JSON object".to_string()) };
    let Some(CValue::Obj(themes)) = top.get("themes") else {
        return Err("token file has no \"themes\" object".to_string());
    };
    if themes.is_empty() {
        return Err("token file defines no theme".to_string());
    }
    let mut out = Tokens::default();
    for (theme, values) in themes {
        let CValue::Obj(values) = values else { return Err(format!("theme \"{theme}\" is not an object")) };
        let mut m = BTreeMap::new();
        for (name, val) in values {
            let text = match val {
                CValue::Str(t) => Some(t.as_str()),
                CValue::Obj(o) => match o.get("$value") {
                    Some(CValue::Str(t)) => Some(t.as_str()),
                    _ => None,
                },
                _ => None,
            };
            if let Some(c) = text.and_then(css::parse_colour) {
                let prop = format!("--{}", name.replace('.', "-"));
                out.props.insert(prop.clone());
                m.insert(prop, c);
            }
        }
        out.themes.insert(theme.clone(), m);
    }
    Ok(out)
}

/// Reads the suppression list of the gate configuration.
pub fn load_config(bytes: &[u8]) -> Result<Vec<Suppression>, String> {
    let v = parse_json(bytes).map_err(|e| format!("invalid JSON: {e}"))?;
    let CValue::Obj(top) = v else { return Err("configuration is not a JSON object".to_string()) };
    let list = match top.get("suppressions") {
        None => return Ok(Vec::new()),
        Some(CValue::Arr(a)) => a,
        Some(_) => return Err("\"suppressions\" is not an array".to_string()),
    };
    let mut out = Vec::new();
    for (i, item) in list.iter().enumerate() {
        let CValue::Obj(o) = item else { return Err(format!("suppression {i} is not an object")) };
        let field = |k: &str| match o.get(k) {
            Some(CValue::Str(t)) if !t.trim().is_empty() => Ok(t.trim().to_string()),
            _ => Err(format!("suppression {i}: \"{k}\" must be a non-empty string")),
        };
        let page = field("page")?;
        let rule = field("rule")?;
        let reason = field("reason")?;
        if success_criterion(&rule).is_empty() {
            return Err(format!("suppression {i}: unknown rule \"{rule}\""));
        }
        out.push(Suppression { page, rule, reason });
    }
    Ok(out)
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, A11yError> {
    let file = show(path);
    let meta = fs::metadata(path).map_err(|e| A11yError::Io { file: file.clone(), message: e.to_string() })?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(A11yError::Parse { file, message: format!("larger than {MAX_FILE_BYTES} bytes") });
    }
    fs::read(path).map_err(|e| A11yError::Io { file, message: e.to_string() })
}

fn read_text(path: &Path, label: &str) -> Result<String, A11yError> {
    let bytes = read_bytes(path)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| A11yError::Parse { file: label.to_string(), message: "not valid UTF-8".to_string() })?;
    Ok(text.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(text))
}

#[derive(Default)]
struct Files {
    html: Vec<String>,
    css: Vec<String>,
}

fn walk(dir: &Path, rel: &str, depth: usize, files: &mut Files) -> Result<(), A11yError> {
    if depth > MAX_DIR_DEPTH {
        return Err(A11yError::Parse { file: show(dir), message: "directory nesting too deep".to_string() });
    }
    let io = |e: std::io::Error| A11yError::Io { file: show(dir), message: e.to_string() };
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir).map_err(io)? {
        let entry = entry.map_err(io)?;
        let ft = entry.file_type().map_err(io)?;
        entries.push((entry.file_name().to_string_lossy().into_owned(), ft));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, ft) in entries {
        let child = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        if ft.is_dir() {
            walk(&dir.join(&name), &child, depth + 1, files)?;
        } else if ft.is_file() {
            let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
            match ext.as_str() {
                "html" | "htm" => files.html.push(child),
                "css" => files.css.push(child),
                _ => continue,
            }
            if files.html.len() + files.css.len() > MAX_FILES {
                return Err(A11yError::Parse { file: show(dir), message: format!("more than {MAX_FILES} files") });
            }
        }
    }
    Ok(())
}

fn parse_err(file: &str, message: String) -> A11yError {
    A11yError::Parse { file: file.to_string(), message }
}

fn applies(rule: &StyleRule, theme: &str) -> bool {
    match &rule.scheme {
        None => true,
        Some(s) => s == theme,
    }
}

/// 3 for a `body` selector, 2 for `html`, 1 for `:root`, else 0.
fn root_rank(selector: &str) -> u8 {
    selector
        .split(',')
        .map(|x| match x.trim() {
            "body" => 3,
            "html" => 2,
            ":root" => 1,
            _ => 0,
        })
        .max()
        .unwrap_or(0)
}

type Sheets = Vec<(String, Vec<StyleRule>)>;

/// Rule A12 for one theme.
fn contrast_theme(theme: &str, tokens: &BTreeMap<String, Rgba>, sheets: &Sheets, out: &mut Vec<Violation>) {
    let mut aliases: BTreeMap<String, String> = BTreeMap::new();
    for (_, rules) in sheets {
        for r in rules.iter().filter(|r| applies(r, theme) && root_rank(&r.selector) > 0) {
            for d in r.decls.iter().filter(|d| d.name.starts_with("--")) {
                aliases.insert(d.name.clone(), d.value.clone());
            }
        }
    }
    let scope = Scope { tokens, aliases: &aliases };
    let mut page: Option<(u8, Rgba)> = None;
    for (_, rules) in sheets {
        for r in rules.iter().filter(|r| r.contrast && applies(r, theme)) {
            let rank = root_rank(&r.selector);
            if rank == 0 {
                continue;
            }
            if let Some(c) = scope.background(&r.decls) {
                let better = match page {
                    None => true,
                    Some((k, _)) => rank >= k,
                };
                if better {
                    page = Some((rank, c));
                }
            }
        }
    }
    let page = css::over(page.map_or(WHITE, |(_, c)| c), WHITE);
    for (path, rules) in sheets {
        for r in rules.iter().filter(|r| r.contrast && applies(r, theme)) {
            let Some(fg) = scope.foreground(&r.decls) else { continue };
            let bg = css::over(scope.background(&r.decls).unwrap_or(page), page);
            let ratio = css::contrast(css::over(fg, bg), bg);
            let large = css::large_text(&r.decls);
            let need = if large { 3.0 } else { 4.5 };
            if ratio < need {
                out.push(Violation {
                    page: path.clone(),
                    line: r.line,
                    rule: "A12".to_string(),
                    element: r.selector.clone(),
                    detail: format!(
                        "theme {theme}: contrast {}:1 below {}:1{}",
                        css::round2(ratio),
                        if large { "3" } else { "4.5" },
                        if large { " (large text)" } else { "" }
                    ),
                });
            }
        }
    }
}

fn literal_findings(page: &str, element: &str, decl: &css::Decl, tokens: &Tokens, out: &mut Vec<Violation>) {
    if decl.name.starts_with("--") && tokens.props.contains(&decl.name) {
        return;
    }
    for lit in css::literal_colours(&decl.name, &decl.value) {
        out.push(Violation {
            page: page.to_string(),
            line: decl.line,
            rule: "A13".to_string(),
            element: element.to_string(),
            detail: format!("literal colour {lit} in {}: not resolved through a token custom property", decl.name),
        });
    }
}

/// Runs the gate. Returns the report, or the error that makes the run exit with code 2.
pub fn run(opts: &Options) -> Result<Report, A11yError> {
    let tokens_label = show(&opts.tokens);
    let tokens = load_tokens(&read_bytes(&opts.tokens)?)
        .map_err(|message| A11yError::Parse { file: tokens_label.clone(), message })?;
    let suppressions = match &opts.config {
        Some(p) => load_config(&read_bytes(p)?).map_err(|message| A11yError::Parse { file: show(p), message })?,
        None => Vec::new(),
    };
    let mut files = Files::default();
    walk(&opts.root, "", 0, &mut files)?;

    let mut found: Vec<Violation> = Vec::new();
    let mut sheets: Sheets = Vec::new();
    for rel in &files.css {
        let text = read_text(&opts.root.join(rel), rel)?;
        let rules = css::parse_sheet(&text, 1).map_err(|m| parse_err(rel, m))?;
        sheets.push((rel.clone(), rules));
    }
    let mut titles: BTreeMap<String, Vec<(String, u32)>> = BTreeMap::new();
    for rel in &files.html {
        let text = read_text(&opts.root.join(rel), rel)?;
        let doc = html::parse(&text).map_err(|m| parse_err(rel, m))?;
        let (findings, facts) = rules::check_page(&doc);
        for f in findings {
            found.push(Violation {
                page: rel.clone(),
                line: f.line,
                rule: f.rule.to_string(),
                element: f.element,
                detail: f.detail,
            });
        }
        if let Some((t, line)) = facts.title {
            titles.entry(t).or_default().push((rel.clone(), line));
        }
        for (block, line) in facts.style_blocks {
            let rules = css::parse_sheet(&block, line).map_err(|m| parse_err(rel, m))?;
            sheets.push((rel.clone(), rules));
        }
        for (style, line, element) in facts.style_attrs {
            for d in css::parse_inline(&style, line) {
                literal_findings(rel, &element, &d, &tokens, &mut found);
            }
        }
    }

    // A05: a title shared by two or more pages.
    for (title, pages) in &titles {
        if pages.len() < 2 {
            continue;
        }
        for (page, line) in pages {
            let others: Vec<&str> = pages.iter().filter(|(q, _)| q != page).map(|(q, _)| q.as_str()).collect();
            found.push(Violation {
                page: page.clone(),
                line: *line,
                rule: "A05".to_string(),
                element: "<title>".to_string(),
                detail: format!("title \"{title}\" shared with {}", others.join(", ")),
            });
        }
    }

    // A13: literal colours in stylesheets.
    for (page, rules) in &sheets {
        for r in rules {
            for d in &r.decls {
                literal_findings(page, &r.selector, d, &tokens, &mut found);
            }
        }
    }

    // A12: contrast per theme.
    for (theme, values) in &tokens.themes {
        contrast_theme(theme, values, &sheets, &mut found);
    }

    found.sort();
    let mut counts = vec![0usize; suppressions.len()];
    let mut violations = Vec::new();
    let mut suppressed = Vec::new();
    for v in found {
        match suppressions.iter().position(|x| x.page == v.page && x.rule == v.rule) {
            Some(k) => {
                if let Some(c) = counts.get_mut(k) {
                    *c += 1;
                }
                suppressed.push(v);
            }
            None => violations.push(v),
        }
    }
    let mut used = Vec::new();
    let mut unmatched = Vec::new();
    for (x, c) in suppressions.into_iter().zip(counts) {
        if c > 0 {
            used.push((x, c));
        } else {
            unmatched.push(x);
        }
    }
    used.sort();
    unmatched.sort();
    Ok(Report {
        pages: files.html.len(),
        stylesheets: files.css.len(),
        themes: tokens.themes.keys().cloned().collect(),
        violations,
        suppressed,
        used,
        unmatched,
    })
}
