//! Docs link checks and gate configuration (spec node `OUTR.DOCS.QA`).
//!
//! Two modes:
//! - Internal mode (`fid-outreach link-check --edition <dir> [--json]`, nearest class R0, D0):
//!   walks every `*.html` / `*.htm` page under `--edition` in sorted path order with the bounded
//!   HTML tokenizer (`crate::qa::a11y::html::parse`), resolves every relative `href` and `src`
//!   against the page path, and reports:
//!   - `link_escapes_edition` when `..` escapes above `--edition`;
//!   - `link_target_missing` when the resolved file does not exist inside `--edition`;
//!   - `fragment_missing` when `#fragment` does not match any element `id` in the target page.
//!
//!   No network code runs in internal mode.
//! - External mode (`fid-outreach link-check --edition <dir> --external [--out <dir>] [--json]`,
//!   nearest class X1, observed class N):
//!   collects every distinct absolute `http://` or `https://` URL across all HTML pages (sorted),
//!   sends one `HEAD` request per URL (falling back to `GET` when `HEAD` is refused) with a 10 s
//!   timeout, at most 4 concurrent requests, and a fixed generic user agent, and writes
//!   `external.json` (RFC 8785 canonical JSON) and `issue.md` (Markdown tracking-issue body for
//!   `docs-external-links`). Requests are made by the system `curl` executable, which external
//!   mode requires: when it cannot be started the run stops with `tool_unavailable` (exit 2)
//!   instead of reporting every URL as a failed link.
//!
//! An edition directory without any HTML page is `edition_unreadable` (exit 2) in both modes.
//!
//! Also provides [`DocsGatesConfig`] for reading `docs/qa/docs-gates.toml`, selecting hand-written
//! `lint_sources` (excluding generated reference pages), and checking pages for off-origin
//! resource URLs against the configured `origin`.

use crate::qa::a11y::html::{parse as parse_html, Node};
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// Largest HTML page or config file read, in bytes (8 MB).
pub const MAX_PAGE_BYTES: usize = 8 * 1024 * 1024;
/// Most files accepted under one edition root.
pub const MAX_FILES: usize = 100_000;
/// Deepest directory nesting walked under one edition root.
pub const MAX_DIR_DEPTH: usize = 32;
/// Cumulative HTML bytes read from one edition.
pub const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
/// Most distinct external URLs probed in one external run.
pub const MAX_EXTERNAL_URLS: usize = 50_000;
/// Default per-request timeout for external link checks (REQ-OUTR.DOCS.QA-004: 10 s).
pub const DEFAULT_EXTERNAL_TIMEOUT: Duration = Duration::from_secs(10);
/// Maximum concurrent external requests (REQ-OUTR.DOCS.QA-004: at most 4).
pub const MAX_CONCURRENT_REQUESTS: usize = 4;
/// Fixed generic user agent naming the project tooling generically.
pub const USER_AGENT: &str = "fid-outreach-link-check/0.1";
/// Maximum response body bytes drained on a `GET` fallback before closing the transfer.
const MAX_GET_BODY_BYTES: u64 = 1024 * 1024;
/// Marker printed to stderr by `curl --write-out` so HTTP status is parsed reliably.
const STATUS_MARK: &str = "fid-http-status:";
const LOOPBACK_HOSTS: [&str; 3] = ["127.0.0.1", "::1", "localhost"];

/// Fatal errors (exit 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkCheckError {
    EditionUnreadable {
        path: String,
        reason: String,
    },
    OutputUnwritable {
        path: String,
        reason: String,
    },
    ConfigParse {
        path: String,
        reason: String,
    },
    /// A required external tool (`curl` for `--external`) cannot be run; `path` names the tool.
    ToolUnavailable {
        path: String,
        reason: String,
    },
}

impl LinkCheckError {
    pub fn code(&self) -> &'static str {
        match self {
            LinkCheckError::EditionUnreadable { .. } => "edition_unreadable",
            LinkCheckError::OutputUnwritable { .. } => "output_unwritable",
            LinkCheckError::ConfigParse { .. } => "config_parse",
            LinkCheckError::ToolUnavailable { .. } => "tool_unavailable",
        }
    }

    fn parts(&self) -> (&str, &str) {
        match self {
            LinkCheckError::EditionUnreadable { path, reason }
            | LinkCheckError::OutputUnwritable { path, reason }
            | LinkCheckError::ConfigParse { path, reason }
            | LinkCheckError::ToolUnavailable { path, reason } => (path, reason),
        }
    }

    /// Canonical RFC 8785 JSON object for `--json` on exit 2.
    pub fn to_json(&self) -> Vec<u8> {
        let (path, detail) = self.parts();
        let mut err = BTreeMap::new();
        err.insert("code".to_string(), CValue::Str(self.code().to_string()));
        err.insert("detail".to_string(), CValue::Str(detail.to_string()));
        err.insert("path".to_string(), CValue::Str(path.to_string()));
        let mut obj = BTreeMap::new();
        obj.insert("error".to_string(), CValue::Obj(err));
        obj.insert("ok".to_string(), CValue::Bool(false));
        fid_canon::to_canonical_bytes(&CValue::Obj(obj)).unwrap_or_default()
    }
}

impl fmt::Display for LinkCheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (path, reason) = self.parts();
        write!(f, "{} {path}: {reason}", self.code())
    }
}

impl std::error::Error for LinkCheckError {}

/// Internal finding codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InternalCode {
    FragmentMissing,
    LinkEscapesEdition,
    LinkTargetMissing,
}

impl InternalCode {
    pub fn as_str(self) -> &'static str {
        match self {
            InternalCode::FragmentMissing => "fragment_missing",
            InternalCode::LinkEscapesEdition => "link_escapes_edition",
            InternalCode::LinkTargetMissing => "link_target_missing",
        }
    }
}

/// One internal link finding. Ordered by `(page, line, code, link)`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InternalFinding {
    pub page: String,
    pub line: u32,
    pub code: InternalCode,
    pub link: String,
}

impl fmt::Display for InternalFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{} {} {}", self.page, self.line, self.code.as_str(), self.link)
    }
}

/// Result of an internal link check (R0, D0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalReport {
    pub pages: usize,
    pub findings: Vec<InternalFinding>,
}

impl InternalReport {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for f in &self.findings {
            out.push_str(&format!("{f}\n"));
        }
        out.push_str(&format!("link-check: {} page(s), {} finding(s)\n", self.pages, self.findings.len()));
        out
    }

    pub fn to_json(&self) -> Vec<u8> {
        let findings = self
            .findings
            .iter()
            .map(|f| {
                let mut o = BTreeMap::new();
                o.insert("code".to_string(), CValue::Str(f.code.as_str().to_string()));
                o.insert("line".to_string(), CValue::Num(f64::from(f.line)));
                o.insert("link".to_string(), CValue::Str(f.link.clone()));
                o.insert("page".to_string(), CValue::Str(f.page.clone()));
                CValue::Obj(o)
            })
            .collect();
        let mut o = BTreeMap::new();
        o.insert("findings".to_string(), CValue::Arr(findings));
        o.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        o.insert("pages".to_string(), CValue::Num(self.pages as f64));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default()
    }
}

/// Options for external link checking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalOptions {
    pub timeout: Duration,
    pub max_concurrent: usize,
    pub user_agent: String,
    /// The `curl` program to run (default `curl` from `PATH`).
    pub curl: PathBuf,
}

impl Default for ExternalOptions {
    fn default() -> Self {
        ExternalOptions {
            timeout: DEFAULT_EXTERNAL_TIMEOUT,
            max_concurrent: MAX_CONCURRENT_REQUESTS,
            user_agent: USER_AGENT.to_string(),
            curl: PathBuf::from("curl"),
        }
    }
}

/// Outcome of checking one distinct external URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalEntry {
    pub url: String,
    pub ok: bool,
    pub status: Option<u16>,
    pub error: Option<String>,
    /// Sorted list of edition-relative pages linking this URL.
    pub pages: Vec<String>,
}

/// Report of an external link check run (X1, observed class N).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalReport {
    pub pages: usize,
    /// Sorted by URL.
    pub results: Vec<ExternalEntry>,
}

impl ExternalReport {
    pub fn failures(&self) -> Vec<&ExternalEntry> {
        self.results.iter().filter(|r| !r.ok).collect()
    }

    pub fn exit_code(&self) -> u8 {
        u8::from(self.results.iter().any(|r| !r.ok))
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        let fails = self.failures();
        for f in &fails {
            let reason = f.error.as_deref().unwrap_or("failed");
            out.push_str(&format!("external_failed {} ({reason}) linked from {}\n", f.url, f.pages.join(", ")));
        }
        out.push_str(&format!(
            "link-check --external: {} page(s), {} url(s) checked, {} failed\n",
            self.pages,
            self.results.len(),
            fails.len()
        ));
        out
    }

    /// Canonical RFC 8785 JSON (`external.json` and `--json`).
    pub fn to_json(&self) -> Vec<u8> {
        let failures: Vec<CValue> = self
            .failures()
            .into_iter()
            .map(|r| {
                let mut o = BTreeMap::new();
                o.insert("code".to_string(), CValue::Str("external_failed".to_string()));
                o.insert("error".to_string(), r.error.as_ref().map_or(CValue::Null, |e| CValue::Str(e.clone())));
                o.insert("pages".to_string(), CValue::Arr(r.pages.iter().map(|p| CValue::Str(p.clone())).collect()));
                o.insert("status".to_string(), r.status.map_or(CValue::Null, |s| CValue::Num(f64::from(s))));
                o.insert("url".to_string(), CValue::Str(r.url.clone()));
                CValue::Obj(o)
            })
            .collect();
        let results: Vec<CValue> = self
            .results
            .iter()
            .map(|r| {
                let mut o = BTreeMap::new();
                o.insert("error".to_string(), r.error.as_ref().map_or(CValue::Null, |e| CValue::Str(e.clone())));
                o.insert("ok".to_string(), CValue::Bool(r.ok));
                o.insert("pages".to_string(), CValue::Arr(r.pages.iter().map(|p| CValue::Str(p.clone())).collect()));
                o.insert("status".to_string(), r.status.map_or(CValue::Null, |s| CValue::Num(f64::from(s))));
                o.insert("url".to_string(), CValue::Str(r.url.clone()));
                CValue::Obj(o)
            })
            .collect();
        let mut o = BTreeMap::new();
        o.insert("checked".to_string(), CValue::Num(self.results.len() as f64));
        o.insert("failed".to_string(), CValue::Num(failures.len() as f64));
        o.insert("failures".to_string(), CValue::Arr(failures));
        o.insert("ok".to_string(), CValue::Bool(self.failures().is_empty()));
        o.insert("pages".to_string(), CValue::Num(self.pages as f64));
        o.insert("results".to_string(), CValue::Arr(results));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default()
    }

    /// Markdown tracking-issue body (`issue.md`) for the `docs-external-links` issue.
    pub fn to_issue_md(&self) -> String {
        let fails = self.failures();
        let mut out = String::new();
        out.push_str("# External link check report\n\n");
        if fails.is_empty() {
            out.push_str(&format!(
                "Checked {} external URL(s) across {} page(s). All links succeeded; this issue can be closed.\n",
                self.results.len(),
                self.pages
            ));
            return out;
        }
        out.push_str(&format!(
            "Checked {} external URL(s) across {} page(s); {} failed:\n\n",
            self.results.len(),
            self.pages,
            fails.len()
        ));
        out.push_str("| URL | Status / Error | Linking pages |\n");
        out.push_str("| --- | --- | --- |\n");
        for f in fails {
            let reason = match (f.status, f.error.as_deref()) {
                (Some(code), Some(err)) => {
                    if err == format!("HTTP {code}") {
                        err.to_string()
                    } else {
                        format!("HTTP {code} ({err})")
                    }
                }
                (Some(code), None) => format!("HTTP {code}"),
                (None, Some(err)) => err.to_string(),
                (None, None) => "failed".to_string(),
            };
            let pages = f.pages.iter().map(|p| format!("`{p}`")).collect::<Vec<_>>().join(", ");
            out.push_str(&format!("| `{}` | {} | {} |\n", escape_md_cell(&f.url), escape_md_cell(&reason), pages));
        }
        out
    }
}

fn escape_md_cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\r', '\n'], " ")
}

/// Writes `external.json` and `issue.md` into `out_dir`.
pub fn write_external_outputs(report: &ExternalReport, out_dir: &Path) -> Result<(), LinkCheckError> {
    let unwritable =
        |p: &Path, e: std::io::Error| LinkCheckError::OutputUnwritable { path: display_path(p), reason: e.to_string() };
    std::fs::create_dir_all(out_dir).map_err(|e| unwritable(out_dir, e))?;
    let json_path = out_dir.join("external.json");
    let mut json_bytes = report.to_json();
    json_bytes.push(b'\n');
    std::fs::write(&json_path, &json_bytes).map_err(|e| unwritable(&json_path, e))?;
    let issue_path = out_dir.join("issue.md");
    std::fs::write(&issue_path, report.to_issue_md().as_bytes()).map_err(|e| unwritable(&issue_path, e))?;
    Ok(())
}

// ------------------------------------------------------------------ edition scanning

struct ScannedPage {
    ids: BTreeSet<String>,
    /// `(line, element_name, attr_name, attr_value)`
    links: Vec<(u32, String, String, String)>,
}

struct ScannedEdition {
    all_files: BTreeSet<String>,
    pages: BTreeMap<String, ScannedPage>,
}

fn scan_edition(edition: &Path) -> Result<ScannedEdition, LinkCheckError> {
    let meta = std::fs::symlink_metadata(edition)
        .map_err(|e| LinkCheckError::EditionUnreadable { path: display_path(edition), reason: e.to_string() })?;
    if meta.file_type().is_symlink() {
        return Err(LinkCheckError::EditionUnreadable {
            path: display_path(edition),
            reason: "symbolic links are not allowed".into(),
        });
    }
    if !meta.is_dir() {
        return Err(LinkCheckError::EditionUnreadable {
            path: display_path(edition),
            reason: "not a directory".into(),
        });
    }
    let mut files = BTreeMap::new();
    let mut file_count = 0usize;
    walk_files(edition, "", 0, &mut file_count, &mut files)?;

    let mut all_files = BTreeSet::new();
    let mut pages = BTreeMap::new();
    let mut total_bytes: u64 = 0;

    for (rel, path) in files {
        all_files.insert(rel.clone());
        if is_html_path(&rel) {
            let text = read_utf8_bounded(&path, MAX_PAGE_BYTES)?;
            total_bytes = total_bytes.saturating_add(text.len() as u64);
            if total_bytes > MAX_TOTAL_BYTES {
                return Err(LinkCheckError::EditionUnreadable {
                    path: display_path(edition),
                    reason: format!("total HTML exceeds {MAX_TOTAL_BYTES} byte limit"),
                });
            }
            let doc = parse_html(&text).map_err(|msg| LinkCheckError::EditionUnreadable {
                path: display_path(&path),
                reason: format!("HTML parse error: {msg}"),
            })?;
            let mut ids = BTreeSet::new();
            let mut links = Vec::new();
            for node in &doc.nodes {
                let Node::Element(el) = node else { continue };
                if let Some(id) = el.attr("id") {
                    if !id.is_empty() {
                        ids.insert(id.to_string());
                    }
                }
                for attr in ["href", "src", "data"] {
                    if attr == "data" && el.name != "object" {
                        continue;
                    }
                    if let Some(val) = el.attr(attr) {
                        links.push((el.line, el.name.clone(), attr.to_string(), val.to_string()));
                    }
                }
            }
            pages.insert(rel, ScannedPage { ids, links });
        }
    }

    if pages.is_empty() {
        return Err(LinkCheckError::EditionUnreadable {
            path: display_path(edition),
            reason: "edition contains no HTML pages".into(),
        });
    }
    Ok(ScannedEdition { all_files, pages })
}

/// Runs the internal link and fragment check (R0, D0, no network).
pub fn check_internal(edition: &Path) -> Result<InternalReport, LinkCheckError> {
    let scanned = scan_edition(edition)?;
    let mut findings = Vec::new();

    for (page_rel, page) in &scanned.pages {
        for (line, _el, attr, raw) in &page.links {
            if attr != "href" && attr != "src" {
                continue;
            }
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed == "#" || trimmed.starts_with("//") {
                continue;
            }
            if has_url_scheme(trimmed) {
                continue;
            }

            let (before_frag, frag) = match trimmed.split_once('#') {
                Some((p, f)) => (p, Some(f)),
                None => (trimmed, None),
            };
            let path_part = before_frag.split_once('?').map_or(before_frag, |(p, _)| p);

            let resolved = if path_part.is_empty() {
                Some(page_rel.clone())
            } else if let Some(rooted) = path_part.strip_prefix('/') {
                resolve_segments(&[], rooted)
            } else {
                let base_dirs: Vec<&str> = match page_rel.rsplit_once('/') {
                    Some((dir, _)) if !dir.is_empty() => dir.split('/').collect(),
                    _ => Vec::new(),
                };
                resolve_segments(&base_dirs, path_part)
            };

            let Some(mut target_rel) = resolved else {
                findings.push(InternalFinding {
                    page: page_rel.clone(),
                    line: *line,
                    code: InternalCode::LinkEscapesEdition,
                    link: trimmed.to_string(),
                });
                continue;
            };

            // If a directory link ends in `/`, check `<dir>/index.html`.
            if path_part.ends_with('/') && !scanned.all_files.contains(&target_rel) {
                target_rel =
                    if target_rel.is_empty() { "index.html".to_string() } else { format!("{target_rel}/index.html") };
            }

            let found_target = if scanned.all_files.contains(&target_rel) {
                Some(target_rel)
            } else {
                let decoded = percent_decode(&target_rel);
                if scanned.all_files.contains(&decoded) {
                    Some(decoded)
                } else {
                    None
                }
            };

            let Some(target_key) = found_target else {
                findings.push(InternalFinding {
                    page: page_rel.clone(),
                    line: *line,
                    code: InternalCode::LinkTargetMissing,
                    link: trimmed.to_string(),
                });
                continue;
            };

            if let Some(frag_raw) = frag {
                if !frag_raw.is_empty() {
                    let decoded_frag = percent_decode(frag_raw);
                    let has_id = scanned
                        .pages
                        .get(&target_key)
                        .is_some_and(|p| p.ids.contains(frag_raw) || p.ids.contains(&decoded_frag));
                    if !has_id {
                        findings.push(InternalFinding {
                            page: page_rel.clone(),
                            line: *line,
                            code: InternalCode::FragmentMissing,
                            link: trimmed.to_string(),
                        });
                    }
                }
            }
        }
    }

    findings.sort();
    Ok(InternalReport { pages: scanned.pages.len(), findings })
}

/// Runs the external HTTP(S) link check (X1, observed class N).
pub fn check_external(edition: &Path, options: &ExternalOptions) -> Result<ExternalReport, LinkCheckError> {
    let scanned = scan_edition(edition)?;
    require_curl(options)?;
    let mut url_pages: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for (page_rel, page) in &scanned.pages {
        for (_line, _el, attr, raw) in &page.links {
            if attr != "href" && attr != "src" {
                continue;
            }
            let trimmed = raw.trim();
            if is_http_url(trimmed) {
                // Strip fragment before probing (fragments are client-side only).
                let req_url = trimmed.split_once('#').map_or(trimmed, |(u, _)| u).to_string();
                if !url_pages.contains_key(&req_url) && url_pages.len() >= MAX_EXTERNAL_URLS {
                    return Err(LinkCheckError::EditionUnreadable {
                        path: display_path(edition),
                        reason: format!("more than {MAX_EXTERNAL_URLS} distinct external URLs"),
                    });
                }
                url_pages.entry(req_url).or_default().insert(page_rel.clone());
            }
        }
    }

    let items: Vec<(String, Vec<String>)> =
        url_pages.into_iter().map(|(url, pages)| (url, pages.into_iter().collect())).collect();

    let workers = options.max_concurrent.clamp(1, MAX_CONCURRENT_REQUESTS).min(items.len().max(1));
    let next_idx = AtomicUsize::new(0);
    let results_map: Mutex<BTreeMap<usize, ExternalEntry>> = Mutex::new(BTreeMap::new());

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let idx = next_idx.fetch_add(1, Ordering::Relaxed);
                let Some((url, pages)) = items.get(idx) else { break };
                let probe = probe_external_url(url, options);
                let entry = ExternalEntry {
                    url: url.clone(),
                    ok: probe.ok,
                    status: probe.status,
                    error: probe.error,
                    pages: pages.clone(),
                };
                if let Ok(mut guard) = results_map.lock() {
                    guard.insert(idx, entry);
                }
            });
        }
    });

    let results: Vec<ExternalEntry> = match results_map.into_inner() {
        Ok(map) => map.into_values().collect(),
        Err(poisoned) => poisoned.into_inner().into_values().collect(),
    };
    // curl vanishing mid-run is a tool error too, never a list of failed links.
    if results.iter().any(|r| r.error.as_deref() == Some(CURL_UNAVAILABLE)) {
        return Err(LinkCheckError::ToolUnavailable {
            path: display_path(&options.curl),
            reason: "curl could not be started during the run".into(),
        });
    }

    Ok(ExternalReport { pages: scanned.pages.len(), results })
}

/// Error string a probe records when `curl` cannot be spawned.
const CURL_UNAVAILABLE: &str = "curl_unavailable";

/// External mode requires `curl`: `curl -q --version` must start and exit 0.
fn require_curl(options: &ExternalOptions) -> Result<(), LinkCheckError> {
    let unavailable = |reason: String| LinkCheckError::ToolUnavailable { path: display_path(&options.curl), reason };
    let status = Command::new(&options.curl)
        .args(["-q", "--version"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| unavailable(format!("curl is required for --external and cannot be run: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(unavailable("curl is required for --external; `curl --version` failed".into()))
    }
}

struct ProbeOutcome {
    ok: bool,
    status: Option<u16>,
    error: Option<String>,
}

fn probe_external_url(url: &str, options: &ExternalOptions) -> ProbeOutcome {
    if url.len() > 2048 || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return ProbeOutcome { ok: false, status: None, error: Some("invalid_url".to_string()) };
    }
    let loopback = is_loopback_url(url);
    let head = run_curl(url, true, loopback, options);
    match head {
        CurlResult::Http(code) if (200..400).contains(&code) => {
            ProbeOutcome { ok: true, status: Some(code), error: None }
        }
        CurlResult::Http(400 | 403 | 405 | 501) => {
            // Fall back to GET when HEAD is refused.
            match run_curl(url, false, loopback, options) {
                CurlResult::Http(g) if (200..400).contains(&g) => {
                    ProbeOutcome { ok: true, status: Some(g), error: None }
                }
                CurlResult::Http(g) => ProbeOutcome { ok: false, status: Some(g), error: Some(format!("HTTP {g}")) },
                CurlResult::Timeout => ProbeOutcome { ok: false, status: None, error: Some("timeout".to_string()) },
                CurlResult::Error(msg) => ProbeOutcome { ok: false, status: None, error: Some(msg) },
            }
        }
        CurlResult::Http(code) => ProbeOutcome { ok: false, status: Some(code), error: Some(format!("HTTP {code}")) },
        CurlResult::Timeout => ProbeOutcome { ok: false, status: None, error: Some("timeout".to_string()) },
        CurlResult::Error(msg) => {
            // If server rejected HEAD by closing connection cleanly (empty reply), retry GET once.
            if msg == "empty_reply" {
                match run_curl(url, false, loopback, options) {
                    CurlResult::Http(g) if (200..400).contains(&g) => {
                        return ProbeOutcome { ok: true, status: Some(g), error: None };
                    }
                    CurlResult::Http(g) => {
                        return ProbeOutcome { ok: false, status: Some(g), error: Some(format!("HTTP {g}")) };
                    }
                    CurlResult::Timeout => {
                        return ProbeOutcome { ok: false, status: None, error: Some("timeout".to_string()) };
                    }
                    CurlResult::Error(gmsg) => {
                        return ProbeOutcome { ok: false, status: None, error: Some(gmsg) };
                    }
                }
            }
            ProbeOutcome { ok: false, status: None, error: Some(msg) }
        }
    }
}

enum CurlResult {
    Http(u16),
    Timeout,
    Error(String),
}

fn run_curl(url: &str, head_only: bool, loopback: bool, options: &ExternalOptions) -> CurlResult {
    let timeout_secs = options.timeout.as_secs().max(1).to_string();
    let mut cmd = Command::new(&options.curl);
    // `-q` must come first so curl ignores any ~/.curlrc.
    cmd.args(["-q", "--globoff", "--silent", "--show-error", "--location", "--max-redirs", "5"])
        .args(["--proto", "=http,https", "--proto-redir", "=http,https"])
        .args(["--connect-timeout", &timeout_secs, "--max-time", &timeout_secs])
        .args(["--user-agent", &options.user_agent])
        .args(["--write-out", "%{stderr}\nfid-http-status:%{http_code}\n", "--output", "-"]);
    if head_only {
        cmd.arg("--head");
    } else {
        cmd.args(["--request", "GET"]);
    }
    if loopback {
        cmd.args(["--noproxy", "*"]);
    }
    let Ok(mut child) =
        cmd.args(["--", url]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
    else {
        return CurlResult::Error(CURL_UNAVAILABLE.to_string());
    };

    // Drain stdout up to MAX_GET_BODY_BYTES so large pages never stall or blow memory.
    if let Some(mut stdout) = child.stdout.take() {
        let mut buf = [0u8; 8192];
        let mut total: u64 = 0;
        loop {
            match stdout.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    total = total.saturating_add(n as u64);
                    if total >= MAX_GET_BODY_BYTES {
                        break;
                    }
                }
            }
        }
    }

    let Ok(out) = child.wait_with_output() else {
        return CurlResult::Error("curl_wait_failed".to_string());
    };
    let stderr = String::from_utf8_lossy(&out.stderr);
    let status_code = stderr
        .lines()
        .rev()
        .find_map(|l| l.strip_prefix(STATUS_MARK))
        .and_then(|s| s.trim().parse::<u16>().ok())
        .unwrap_or(0);

    if out.status.code() == Some(28) {
        return CurlResult::Timeout;
    }
    if status_code >= 100 {
        return CurlResult::Http(status_code);
    }
    match out.status.code() {
        Some(6) => CurlResult::Error("dns_failed".to_string()),
        Some(7) => CurlResult::Error("connection_refused".to_string()),
        Some(35 | 51 | 60) => CurlResult::Error("tls_error".to_string()),
        Some(52) => CurlResult::Error("empty_reply".to_string()),
        Some(c) => CurlResult::Error(format!("curl_exit_{c}")),
        None => CurlResult::Error("request_failed".to_string()),
    }
}

// ------------------------------------------------------------------ docs-gates.toml & off-origin check

/// Parsed `docs/qa/docs-gates.toml` configuration for the reusable `OUTR.SITE.QA` gates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocsGatesConfig {
    pub origin: String,
    pub root: String,
    pub pages: Vec<String>,
    pub a11y_level: String,
    pub size_budget_kb: u64,
    pub lint_sources: Vec<String>,
}

impl DocsGatesConfig {
    /// Parses `docs-gates.toml` strictly, rejecting missing or mistyped required keys.
    pub fn parse(text: &str, path_label: &str) -> Result<Self, LinkCheckError> {
        let err = |reason: String| LinkCheckError::ConfigParse { path: path_label.to_string(), reason };
        let val: toml::Value = toml::from_str(text).map_err(|e| err(e.to_string()))?;
        let tbl = val.as_table().ok_or_else(|| err("top level must be a table".into()))?;
        let allowed = ["origin", "root", "pages", "a11y_level", "size_budget_kb", "lint_sources"];
        for k in tbl.keys() {
            if !allowed.contains(&k.as_str()) {
                return Err(err(format!("unknown key `{k}`")));
            }
        }
        let req_str = |key: &str| -> Result<String, LinkCheckError> {
            tbl.get(key)
                .and_then(toml::Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .ok_or_else(|| err(format!("missing non-empty string `{key}`")))
        };
        let req_str_arr = |key: &str| -> Result<Vec<String>, LinkCheckError> {
            let arr = tbl
                .get(key)
                .and_then(toml::Value::as_array)
                .ok_or_else(|| err(format!("missing string array `{key}`")))?;
            let mut out = Vec::with_capacity(arr.len());
            for (i, item) in arr.iter().enumerate() {
                let s = item
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| err(format!("{key}[{i}] must be a non-empty string")))?;
                out.push(s.to_string());
            }
            Ok(out)
        };
        let origin = req_str("origin")?;
        if parse_origin(&origin).is_none() {
            return Err(err(format!("`origin` must be an http(s) origin, got {origin:?}")));
        }
        let root = req_str("root")?;
        let pages = req_str_arr("pages")?;
        let a11y_level = req_str("a11y_level")?;
        let size_budget_kb_i64 = tbl
            .get("size_budget_kb")
            .and_then(toml::Value::as_integer)
            .ok_or_else(|| err("missing integer `size_budget_kb`".into()))?;
        if size_budget_kb_i64 <= 0 {
            return Err(err("`size_budget_kb` must be positive".into()));
        }
        let size_budget_kb = size_budget_kb_i64 as u64;
        let lint_sources = req_str_arr("lint_sources")?;
        Ok(DocsGatesConfig { origin, root, pages, a11y_level, size_budget_kb, lint_sources })
    }

    /// Loads `docs-gates.toml` from disk with bounded reads.
    pub fn load(path: &Path) -> Result<Self, LinkCheckError> {
        let text = read_utf8_bounded(path, MAX_PAGE_BYTES)?;
        Self::parse(&text, &display_path(path))
    }
}

/// Returns every file under `root` (sorted edition-relative paths) matched by `config.lint_sources`.
/// Reference pages (`ref/...`) are excluded unless explicitly matched by a `lint_sources` glob.
pub fn select_lint_sources(root: &Path, config: &DocsGatesConfig) -> Result<Vec<PathBuf>, LinkCheckError> {
    let mut files = BTreeMap::new();
    let mut count = 0usize;
    walk_files(root, "", 0, &mut count, &mut files)?;
    let mut out = Vec::new();
    for (rel, path) in files {
        if config.lint_sources.iter().any(|pat| glob_matches(pat, &rel)) {
            out.push(path);
        }
    }
    Ok(out)
}

/// One off-origin resource request detected statically in an edition page.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OffOriginFinding {
    pub page: String,
    pub line: u32,
    pub element: String,
    pub url: String,
}

/// Scans every HTML page under `edition` for resource URLs (`<script src>`, `<img src>`,
/// `<link href>` for stylesheet/preload/icon, `<iframe src>`, `<audio src>`, `<video src>`,
/// `<source src>`, `<embed src>`, `<object data>`) whose origin differs from `allowed_origin`.
pub fn check_off_origin(edition: &Path, allowed_origin: &str) -> Result<Vec<OffOriginFinding>, LinkCheckError> {
    let Some(allowed) = parse_origin(allowed_origin) else {
        return Err(LinkCheckError::ConfigParse {
            path: allowed_origin.to_string(),
            reason: "invalid allowed origin".into(),
        });
    };
    let scanned = scan_edition(edition)?;
    let mut findings = Vec::new();
    for (page_rel, page) in &scanned.pages {
        for (line, el, attr, raw) in &page.links {
            let is_resource = matches!(
                (el.as_str(), attr.as_str()),
                ("script" | "img" | "iframe" | "audio" | "video" | "source" | "embed", "src")
                    | ("object", "data")
                    | ("link", "href")
            );
            if !is_resource {
                continue;
            }
            let trimmed = raw.trim();
            if trimmed.starts_with("//") {
                findings.push(OffOriginFinding {
                    page: page_rel.clone(),
                    line: *line,
                    element: el.clone(),
                    url: trimmed.to_string(),
                });
                continue;
            }
            if is_http_url(trimmed) {
                match parse_origin(trimmed) {
                    Some(orig) if orig == allowed => {}
                    _ => findings.push(OffOriginFinding {
                        page: page_rel.clone(),
                        line: *line,
                        element: el.clone(),
                        url: trimmed.to_string(),
                    }),
                }
            }
        }
    }
    findings.sort();
    Ok(findings)
}

// ------------------------------------------------------------------ helpers

fn display_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn is_html_path(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    lower.ends_with(".html") || lower.ends_with(".htm")
}

fn has_url_scheme(s: &str) -> bool {
    let Some((scheme, _)) = s.split_once(':') else { return false };
    !scheme.is_empty()
        && !scheme.contains('/')
        && scheme.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && scheme.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
}

fn is_http_url(s: &str) -> bool {
    let lower_prefix = s.get(..8).unwrap_or(s).to_ascii_lowercase();
    lower_prefix.starts_with("http://") || lower_prefix.starts_with("https://")
}

/// Returns `(scheme_lower, host_lower, effective_port)` for an `http://` or `https://` URL.
fn parse_origin(url: &str) -> Option<(String, String, u16)> {
    let (scheme, default_port, rest) = if let Some(r) = url
        .strip_prefix("https://")
        .or_else(|| url.get(..8).filter(|p| p.eq_ignore_ascii_case("https://")).and_then(|_| url.get(8..)))
    {
        ("https", 443u16, r)
    } else {
        let r = url
            .strip_prefix("http://")
            .or_else(|| url.get(..7).filter(|p| p.eq_ignore_ascii_case("http://")).and_then(|_| url.get(7..)))?;
        ("http", 80u16, r)
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (h, after) = v6.split_once(']')?;
        let p = if after.is_empty() { default_port } else { after.strip_prefix(':')?.parse::<u16>().ok()? };
        (h.to_ascii_lowercase(), p)
    } else if let Some((h, p_str)) = authority.split_once(':') {
        (h.to_ascii_lowercase(), p_str.parse::<u16>().ok()?)
    } else {
        (authority.to_ascii_lowercase(), default_port)
    };
    if host.is_empty() {
        return None;
    }
    Some((scheme.to_string(), host, port))
}

fn is_loopback_url(url: &str) -> bool {
    parse_origin(url).is_some_and(|(_, host, _)| LOOPBACK_HOSTS.contains(&host.as_str()))
}

fn resolve_segments(base_dirs: &[&str], rel: &str) -> Option<String> {
    let mut stack: Vec<&str> = base_dirs.to_vec();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                stack.pop()?;
            }
            other => stack.push(other),
        }
    }
    Some(stack.join("/"))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let (Some(&h1), Some(&h2)) = (bytes.get(i + 1), bytes.get(i + 2)) {
                if let (Some(a), Some(b)) = (hex_val(h1), hex_val(h2)) {
                    out.push((a << 4) | b);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Segment-aware glob matcher supporting `*`, `?` (within one segment) and `**` (zero or more segments).
pub fn glob_matches(pattern: &str, path: &str) -> bool {
    let pat_segs: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let path_segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    glob_segs(&pat_segs, &path_segs)
}

/// Segment-level two-pointer matcher: `**` plays the role `*` plays in [`seg_matches`], so the
/// work is O(pattern segments x path segments) even with many `**` (no exponential backtracking).
fn glob_segs(pat: &[&str], path: &[&str]) -> bool {
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star_pi: Option<usize> = None;
    let mut star_ti: usize = 0;
    while ti < path.len() {
        if pi < pat.len() && pat[pi] == "**" {
            star_pi = Some(pi);
            star_ti = ti;
            pi += 1;
        } else if pi < pat.len() && seg_matches(pat[pi].as_bytes(), path[ti].as_bytes()) {
            pi += 1;
            ti += 1;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < pat.len() && pat[pi] == "**" {
        pi += 1;
    }
    pi == pat.len()
}

fn seg_matches(pat: &[u8], text: &[u8]) -> bool {
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star_pi: Option<usize> = None;
    let mut star_ti: usize = 0;
    while ti < text.len() {
        if pi < pat.len() && (pat[pi] == b'?' || pat[pi] == text[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pat.len() && pat[pi] == b'*' {
            star_pi = Some(pi);
            star_ti = ti;
            pi += 1;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < pat.len() && pat[pi] == b'*' {
        pi += 1;
    }
    pi == pat.len()
}

fn walk_files(
    dir: &Path,
    prefix: &str,
    depth: usize,
    file_count: &mut usize,
    out: &mut BTreeMap<String, PathBuf>,
) -> Result<(), LinkCheckError> {
    if depth > MAX_DIR_DEPTH {
        return Err(LinkCheckError::EditionUnreadable {
            path: display_path(dir),
            reason: format!("directory nesting exceeds depth {MAX_DIR_DEPTH}"),
        });
    }
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(dir)
        .map_err(|e| LinkCheckError::EditionUnreadable { path: display_path(dir), reason: e.to_string() })?
    {
        let entry =
            entry.map_err(|e| LinkCheckError::EditionUnreadable { path: display_path(dir), reason: e.to_string() })?;
        entries.push((entry.file_name().to_string_lossy().into_owned(), entry.path()));
    }
    entries.sort();
    for (name, path) in entries {
        let meta = std::fs::symlink_metadata(&path)
            .map_err(|e| LinkCheckError::EditionUnreadable { path: display_path(&path), reason: e.to_string() })?;
        if meta.file_type().is_symlink() {
            return Err(LinkCheckError::EditionUnreadable {
                path: display_path(&path),
                reason: "symbolic links are not allowed".into(),
            });
        }
        let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        if meta.is_dir() {
            walk_files(&path, &rel, depth + 1, file_count, out)?;
        } else if meta.is_file() {
            *file_count = file_count.saturating_add(1);
            if *file_count > MAX_FILES {
                return Err(LinkCheckError::EditionUnreadable {
                    path: display_path(&path),
                    reason: format!("more than {MAX_FILES} files"),
                });
            }
            out.insert(rel, path);
        }
    }
    Ok(())
}

fn read_utf8_bounded(path: &Path, max_bytes: usize) -> Result<String, LinkCheckError> {
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| LinkCheckError::EditionUnreadable { path: display_path(path), reason: e.to_string() })?;
    if meta.file_type().is_symlink() {
        return Err(LinkCheckError::EditionUnreadable {
            path: display_path(path),
            reason: "symbolic links are not allowed".into(),
        });
    }
    if !meta.is_file() {
        return Err(LinkCheckError::EditionUnreadable {
            path: display_path(path),
            reason: "not a regular file".into(),
        });
    }
    if meta.len() > max_bytes as u64 {
        return Err(LinkCheckError::EditionUnreadable {
            path: display_path(path),
            reason: format!("file exceeds {max_bytes} byte limit"),
        });
    }
    let file = std::fs::File::open(path)
        .map_err(|e| LinkCheckError::EditionUnreadable { path: display_path(path), reason: e.to_string() })?;
    let mut buf = Vec::new();
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|e| LinkCheckError::EditionUnreadable { path: display_path(path), reason: e.to_string() })?;
    if buf.len() > max_bytes {
        return Err(LinkCheckError::EditionUnreadable {
            path: display_path(path),
            reason: format!("file exceeds {max_bytes} byte limit"),
        });
    }
    String::from_utf8(buf).map_err(|e| LinkCheckError::EditionUnreadable {
        path: display_path(path),
        reason: format!("invalid UTF-8: {e}"),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn glob_matches_hand_written_and_excludes_ref() {
        assert!(glob_matches("book/**/*.md", "book/intro.md"));
        assert!(glob_matches("book/**/*.md", "book/concepts/units.md"));
        assert!(!glob_matches("book/**/*.md", "ref/cli/demo.read_item@1.html"));
        assert!(!glob_matches("book/**/*.md", "book/intro.html"));
        assert!(glob_matches("**/x/**/y.md", "a/x/b/c/y.md"));
        assert!(glob_matches("**", "a/b"));
        assert!(!glob_matches("a/**/b", "a/c"));
    }

    /// Repeated `**` must not backtrack exponentially (the old recursive matcher needed about
    /// C(80, 40) steps for this non-match).
    #[test]
    fn glob_with_repeated_double_star_is_polynomial() {
        let pattern = vec!["**"; 40].join("/") + "/z";
        let path = vec!["a"; 40].join("/");
        assert!(!glob_matches(&pattern, &path));
        assert!(glob_matches(&pattern, &(path.clone() + "/z")));
    }

    #[test]
    fn origin_parsing_handles_default_ports() {
        assert_eq!(
            parse_origin("https://docs.metrologymaven.io"),
            parse_origin("https://docs.metrologymaven.io:443/x")
        );
        assert_ne!(parse_origin("https://docs.metrologymaven.io"), parse_origin("https://tracker.example/js"));
        assert!(is_loopback_url("http://127.0.0.1:8080/ok"));
        assert!(!is_loopback_url("https://example.org/ok"));
    }
}
