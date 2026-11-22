//! `fid-design sources-check --root <design dir> [--spec <spec dir>] [--json]` (spec node
//! `UX.SYSTEM.TOOL`, REQ-UX.SYSTEM.TOOL-001 .. -004). Tooling, nearest class R0; D0
//! (`BTreeMap`/`BTreeSet` only, sorted findings, no clock, no network).
//!
//! # `design-sources@1` (`<root>/sources.json`)
//!
//! ```json
//! {
//!   "format": "design-sources@1",
//!   "tools": [
//!     {
//!       "id": "penpot",
//!       "name": "Penpot",
//!       "open_source": true,
//!       "requires_paid_account": false,
//!       "export_extensions": ["penpot"]
//!     }
//!   ],
//!   "sources": [
//!     {
//!       "id": "report-summary-v1",
//!       "node": "UX.REPORT.MOCK",
//!       "tool": "penpot",
//!       "location": "report/mockups/summary/summary.penpot",
//!       "exports": [
//!         { "path": "report/mockups/summary/summary.pdf", "sha256": "sha256:<64 hex>" }
//!       ]
//!     }
//!   ]
//! }
//! ```
//!
//! # Rules
//!
//! - `tools`: non-empty array of tool entries (`id`, `name`, `open_source`,
//!   `requires_paid_account`, optional `export_extensions`). Tool IDs are unique.
//! - `sources`: array of source entries (`id`, `node`, `tool`, `location`, `exports`). Source IDs
//!   are unique.
//! - `node`: must be a valid UX spec node ID (`UX` or `UX.<SUBSYSTEM>(.<SEG>)*` where
//!   `<SUBSYSTEM>` is one of the eight UX subsystems). When `--spec <spec dir>` is supplied, the
//!   corresponding `<spec dir>/<lowercase segments>/NODE.md` must also exist as a regular file
//!   whose frontmatter `id:` equals `node`. Otherwise `SOURCE-NODE-UNKNOWN <source.id>`.
//! - `tool`: must appear in `tools`; if that tool row has `requires_paid_account = true`, then
//!   `SOURCE-PAID-TOOL <source.id>`.
//! - `location`: either a clean relative repository path or a public `https://` share URL (never
//!   fetched over the network).
//! - `exports`: non-empty list of `{ path, sha256 }` entries. Each `path` is a clean relative
//!   path below `--root` with an open-format extension (`svg`, `pdf`, `png`, or one of its tool's
//!   `export_extensions`), must exist as a regular file on disk (no symbolic links followed;
//!   missing yields `SOURCE-EXPORT-MISSING <source.id>`), and its SHA-256 digest must match
//!   `sha256` (mismatch yields `SOURCE-EXPORT-DIGEST <source.id>`).
//! - Untrusted text fails closed through [`crate::review::guard::value_defect`].
//!
//! Exit codes: 0 no findings, 1 findings, 2 unreadable input or bad usage.

use crate::finding::{findings_text, findings_value, read_text, Finding};
use crate::review::guard::{path_defect, value_defect};
use crate::review::{self, Kind};
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;

/// Report format ID.
pub const REPORT_FORMAT: &str = "fid-design-sources-check@1";
/// Format ID of `sources.json`.
pub const SOURCES_FORMAT: &str = "design-sources@1";
/// File name below `--root`.
pub const SOURCES_FILE: &str = "sources.json";

/// Finding codes (UX.SYSTEM.TOOL Design).
pub const EXPORT_MISSING: &str = "SOURCE-EXPORT-MISSING";
pub const EXPORT_DIGEST: &str = "SOURCE-EXPORT-DIGEST";
pub const PAID_TOOL: &str = "SOURCE-PAID-TOOL";
pub const NODE_UNKNOWN: &str = "SOURCE-NODE-UNKNOWN";
pub const SCHEMA: &str = "SOURCES-SCHEMA";

/// Largest `sources.json` read (4 MiB).
pub const MAX_SOURCES_BYTES: u64 = 4 * 1024 * 1024;
/// Largest committed export file hashed (64 MiB).
pub const MAX_EXPORT_BYTES: u64 = 64 * 1024 * 1024;
/// Largest spec `NODE.md` read when `--spec` is given (1 MiB).
pub const MAX_NODE_DOC_BYTES: u64 = 1024 * 1024;
/// Deepest JSON nesting accepted.
pub const MAX_JSON_DEPTH: usize = 32;
/// Most entries in `tools`, `sources` or one source's `exports`.
pub const MAX_ITEMS: usize = 10_000;
/// Most custom export extensions declared for one tool.
pub const MAX_TOOL_EXTS: usize = 32;
/// Longest text field (`location`, `name`, etc.) in bytes.
pub const MAX_TEXT_BYTES: usize = 1024;

/// Open export formats accepted for every tool (REQ-UX.SYSTEM.TOOL-002).
pub const OPEN_EXTENSIONS: [&str; 3] = ["pdf", "png", "svg"];

/// Level-2 UX subsystems from `spec/ux/NODE.md`, sorted.
pub const UX_SUBSYSTEMS: [&str; 8] = ["COMP", "ICONS", "PATTERN", "REPORT", "RESEARCH", "SCREENS", "SYSTEM", "TOKENS"];

const TOP_KEYS: [&str; 3] = ["format", "sources", "tools"];
const SOURCE_KEYS: [&str; 5] = ["exports", "id", "location", "node", "tool"];
const EXPORT_KEYS: [&str; 2] = ["path", "sha256"];

/// One entry of the `tools` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    pub id: String,
    pub name: String,
    pub open_source: bool,
    pub requires_paid_account: bool,
    pub export_extensions: BTreeSet<String>,
}

/// Result of `fid-design sources-check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub tools: usize,
    pub sources: usize,
    pub exports_checked: usize,
    pub findings: Vec<Finding>,
}

/// Whether `s` is a valid tool ID: lowercase ASCII letter, then lowercase ASCII letters, digits
/// or `-`, length 1..=64.
pub fn valid_tool_id(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_lowercase()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Whether `s` is a valid source ID: ASCII lowercase letter or digit, then lowercase letters,
/// digits, `.`, `-` or `_`, length 1..=128.
pub fn valid_source_id(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 128
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(*c, b'.' | b'-' | b'_'))
}

/// Whether `s` is a valid lowercase file extension (without leading dot), length 1..=16.
pub fn valid_extension(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 16
        && b[0].is_ascii_lowercase()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Whether `s` is a syntactically valid UX spec node ID (`UX` or `UX.<SUBSYSTEM>(.<SEG>)*`).
pub fn valid_ux_node_id(s: &str) -> bool {
    if s.is_empty() || s.len() > 128 {
        return false;
    }
    let segs: Vec<&str> = s.split('.').collect();
    if segs.first().copied() != Some("UX") {
        return false;
    }
    if segs.len() > 8 {
        return false;
    }
    let seg_ok = |seg: &str| {
        let b = seg.as_bytes();
        !b.is_empty() && b[0].is_ascii_uppercase() && b.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    };
    if !segs.iter().all(|seg| seg_ok(seg)) {
        return false;
    }
    if let Some(sub) = segs.get(1) {
        if !UX_SUBSYSTEMS.contains(sub) {
            return false;
        }
    }
    true
}

/// Whether `s` is an accepted public location: either a public `https://` URL (non-empty host,
/// no whitespace, no credentials in the authority) or a clean relative repository path.
/// Never accesses the network.
pub fn valid_location(s: &str) -> bool {
    if s.is_empty() || s.len() > MAX_TEXT_BYTES || s != s.trim() {
        return false;
    }
    if let Some(rest) = s.strip_prefix("https://") {
        if rest.is_empty() || rest.chars().any(char::is_whitespace) {
            return false;
        }
        let authority = match rest.split(['/', '?', '#']).next() {
            Some(a) => a,
            None => return false,
        };
        !authority.is_empty() && !authority.contains('@') && !authority.starts_with(':')
    } else {
        path_defect(s).is_none()
    }
}

fn file_extension(path: &str) -> Option<String> {
    let file = path.rsplit('/').next()?;
    let (stem, ext) = file.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty()).then(|| ext.to_ascii_lowercase())
}

fn read_bytes(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut buf = Vec::new();
    file.take(max.saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if buf.len() as u64 > max {
        return Err(format!("{} is larger than {max} bytes", path.display()));
    }
    Ok(buf)
}

fn frontmatter_id(text: &str) -> Option<&str> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for line in lines {
        let t = line.trim();
        if t == "---" {
            return None;
        }
        if let Some(rest) = t.strip_prefix("id:") {
            let id = rest.trim();
            if !id.is_empty() {
                return Some(id);
            }
        }
    }
    None
}

fn spec_node_exists(spec_dir: &Path, node: &str) -> Result<bool, String> {
    let rel = format!("{}/NODE.md", node.split('.').map(str::to_ascii_lowercase).collect::<Vec<_>>().join("/"));
    if path_defect(&rel).is_some() {
        return Ok(false);
    }
    match review::kind(spec_dir, &rel)? {
        Kind::File => {
            let text = read_text(&spec_dir.join(&rel), MAX_NODE_DOC_BYTES)?;
            Ok(frontmatter_id(&text) == Some(node))
        }
        Kind::Missing | Kind::Dir | Kind::Other => Ok(false),
    }
}

fn schema(f: &mut Vec<Finding>, subject: impl Into<String>, detail: impl Into<String>) {
    f.push(Finding::new(SCHEMA, subject, detail));
}

fn parse_tools(arr: &[CValue], f: &mut Vec<Finding>) -> BTreeMap<String, Tool> {
    let mut tools = BTreeMap::new();
    if arr.is_empty() || arr.len() > MAX_ITEMS {
        schema(f, SOURCES_FILE, format!("`tools` must hold 1 to {MAX_ITEMS} entries"));
        return tools;
    }
    for (i, item) in arr.iter().enumerate() {
        let fallback = format!("tools[{i}]");
        let CValue::Obj(m) = item else {
            schema(f, fallback, "tool entry must be a JSON object");
            continue;
        };
        let mut ok = true;
        for k in m.keys() {
            if !matches!(k.as_str(), "id" | "name" | "open_source" | "requires_paid_account" | "export_extensions") {
                schema(f, &fallback, format!("unknown key `{k}` in tool entry"));
                ok = false;
            }
        }
        let id = match m.get("id") {
            Some(CValue::Str(s)) if valid_tool_id(s) => s.clone(),
            _ => {
                schema(f, &fallback, "`id` must be a lowercase identifier ([a-z][a-z0-9-]*)");
                ok = false;
                String::new()
            }
        };
        let subj = if id.is_empty() { fallback.as_str() } else { id.as_str() };
        let name = match m.get("name") {
            Some(CValue::Str(s)) if !s.trim().is_empty() && s.len() <= 256 && s == s.trim() => s.clone(),
            _ => {
                schema(f, subj, "`name` must be non-blank text");
                ok = false;
                String::new()
            }
        };
        let open_source = match m.get("open_source") {
            Some(CValue::Bool(b)) => *b,
            _ => {
                schema(f, subj, "`open_source` must be a boolean");
                ok = false;
                false
            }
        };
        let requires_paid_account = match m.get("requires_paid_account") {
            Some(CValue::Bool(b)) => *b,
            _ => {
                schema(f, subj, "`requires_paid_account` must be a boolean");
                ok = false;
                false
            }
        };
        let mut export_extensions = BTreeSet::new();
        if let Some(v) = m.get("export_extensions") {
            match v {
                CValue::Arr(exts) if exts.len() <= MAX_TOOL_EXTS => {
                    for e in exts {
                        match e {
                            CValue::Str(ext) if valid_extension(ext) => {
                                if !export_extensions.insert(ext.clone()) {
                                    schema(f, subj, format!("`export_extensions` repeats `{ext}`"));
                                    ok = false;
                                }
                            }
                            _ => {
                                schema(f, subj, "`export_extensions` items must be lowercase extensions without a dot");
                                ok = false;
                            }
                        }
                    }
                }
                _ => {
                    schema(f, subj, format!("`export_extensions` must be an array of at most {MAX_TOOL_EXTS} items"));
                    ok = false;
                }
            }
        }
        if ok {
            if tools.contains_key(&id) {
                schema(f, &id, "tool id is listed twice");
            } else {
                tools.insert(id.clone(), Tool { id, name, open_source, requires_paid_account, export_extensions });
            }
        }
    }
    tools
}

struct CheckCtx<'a> {
    root: &'a Path,
    spec: Option<&'a Path>,
    tools: &'a BTreeMap<String, Tool>,
    seen_ids: BTreeSet<String>,
    exports_checked: usize,
    findings: &'a mut Vec<Finding>,
}

impl CheckCtx<'_> {
    fn check_source(&mut self, i: usize, item: &CValue) -> Result<(), String> {
        let root = self.root;
        let spec = self.spec;
        let tools = self.tools;
        let seen_ids = &mut self.seen_ids;
        let exports_checked = &mut self.exports_checked;
        let f = &mut *self.findings;
        let fallback = format!("sources[{i}]");
        let CValue::Obj(m) = item else {
            schema(f, fallback, "source entry must be a JSON object");
            return Ok(());
        };
        let id_str = match m.get("id") {
            Some(CValue::Str(s)) if valid_source_id(s) => {
                if !seen_ids.insert(s.clone()) {
                    schema(f, s.as_str(), "source id is listed twice");
                }
                Some(s.as_str())
            }
            _ => {
                schema(f, &fallback, "`id` must be a lowercase identifier ([a-z0-9][a-z0-9._-]*)");
                None
            }
        };
        let subj = id_str.unwrap_or(fallback.as_str());

        let keys: Vec<&str> = m.keys().map(String::as_str).collect();
        if keys != SOURCE_KEYS {
            schema(f, subj, format!("source entry must have exactly the keys {}", SOURCE_KEYS.join(", ")));
        }

        match m.get("node") {
            Some(CValue::Str(node)) if !node.trim().is_empty() && node.len() <= 128 && node == node.trim() => {
                let known = if !valid_ux_node_id(node) {
                    false
                } else if let Some(spec_dir) = spec {
                    spec_node_exists(spec_dir, node)?
                } else {
                    true
                };
                if !known {
                    f.push(Finding::new(NODE_UNKNOWN, subj, format!("unknown UX spec node `{node}`")));
                }
            }
            _ => schema(f, subj, "`node` must be a non-blank spec node ID"),
        }

        let resolved_tool = match m.get("tool") {
            Some(CValue::Str(t)) if !t.trim().is_empty() && t == t.trim() => match tools.get(t) {
                Some(tool) => {
                    if tool.requires_paid_account {
                        f.push(Finding::new(PAID_TOOL, subj, format!("tool `{t}` requires a paid account")));
                    }
                    Some(tool)
                }
                None => {
                    schema(f, subj, format!("tool `{t}` is not listed in `tools`"));
                    None
                }
            },
            _ => {
                schema(f, subj, "`tool` must name a tool ID from `tools`");
                None
            }
        };

        match m.get("location") {
            Some(CValue::Str(loc)) if valid_location(loc) => {}
            _ => schema(f, subj, "`location` must be a clean relative path or a public https:// URL"),
        }

        match m.get("exports") {
            Some(CValue::Arr(exps)) if exps.is_empty() => {
                f.push(Finding::new(EXPORT_MISSING, subj, "source has no committed exports"));
            }
            Some(CValue::Arr(exps)) if exps.len() > MAX_ITEMS => {
                schema(f, subj, format!("`exports` holds more than {MAX_ITEMS} entries"));
            }
            Some(CValue::Arr(exps)) => {
                let mut seen_paths = BTreeSet::new();
                for exp in exps {
                    let CValue::Obj(e) = exp else {
                        schema(f, subj, "each export must be a JSON object");
                        continue;
                    };
                    let ekeys: Vec<&str> = e.keys().map(String::as_str).collect();
                    if ekeys != EXPORT_KEYS {
                        schema(f, subj, format!("export must have exactly the keys {}", EXPORT_KEYS.join(", ")));
                    }
                    let path = match e.get("path") {
                        Some(CValue::Str(p)) => match path_defect(p) {
                            Some(d) => {
                                schema(f, subj, format!("export path refused: {d}"));
                                None
                            }
                            None => {
                                if !seen_paths.insert(p.clone()) {
                                    schema(f, subj, format!("export `{p}` is listed twice"));
                                }
                                Some(p.as_str())
                            }
                        },
                        _ => {
                            schema(f, subj, "export `path` must be a relative path string");
                            None
                        }
                    };
                    let want_digest = match e.get("sha256") {
                        Some(CValue::Str(d)) if fid_types::Digest::is_valid(d) => Some(d.as_str()),
                        _ => {
                            schema(f, subj, "export `sha256` must be `sha256:<64 lowercase hex>`");
                            None
                        }
                    };
                    let Some(path) = path else {
                        continue;
                    };
                    let ext_ok = match file_extension(path) {
                        Some(ext) => {
                            OPEN_EXTENSIONS.contains(&ext.as_str())
                                || resolved_tool.is_some_and(|t| t.export_extensions.contains(&ext))
                        }
                        None => false,
                    };
                    if !ext_ok {
                        schema(
                            f,
                            subj,
                            format!("export `{path}` is not an open format (svg, pdf, png, or declared tool export)"),
                        );
                    }
                    match review::kind(root, path)? {
                        Kind::Missing => {
                            f.push(Finding::new(
                                EXPORT_MISSING,
                                subj,
                                format!("committed export `{path}` does not exist"),
                            ));
                        }
                        Kind::Dir | Kind::Other => {
                            schema(f, subj, format!("export `{path}` is not a regular file"));
                        }
                        Kind::File => {
                            let bytes = read_bytes(&root.join(path), MAX_EXPORT_BYTES)?;
                            *exports_checked += 1;
                            let have = fid_types::Digest::from_bytes(fid_canon::digest_bytes(&bytes).0);
                            if let Some(want) = want_digest {
                                if have.as_str() != want {
                                    f.push(Finding::new(
                                        EXPORT_DIGEST,
                                        subj,
                                        format!("export `{path}` digest differs (expected {want}, got {have})"),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            _ => schema(f, subj, "`exports` must be an array of {path, sha256}"),
        }
        Ok(())
    }
}

/// Runs `fid-design sources-check` against `<root>/sources.json`.
/// `Err` is an unreadable input or bad directory (exit 2).
pub fn check(root: &Path, spec: Option<&Path>) -> Result<Report, String> {
    let root_md = std::fs::metadata(root).map_err(|e| format!("cannot read --root {}: {e}", root.display()))?;
    if !root_md.is_dir() {
        return Err(format!("--root {} is not a directory", root.display()));
    }
    if let Some(spec_dir) = spec {
        let md = std::fs::metadata(spec_dir).map_err(|e| format!("cannot read --spec {}: {e}", spec_dir.display()))?;
        if !md.is_dir() {
            return Err(format!("--spec {} is not a directory", spec_dir.display()));
        }
    }
    let text = match review::kind(root, SOURCES_FILE)? {
        Kind::File => read_text(&root.join(SOURCES_FILE), MAX_SOURCES_BYTES)?,
        Kind::Missing => return Err(format!("cannot read {}: file does not exist", root.join(SOURCES_FILE).display())),
        Kind::Dir | Kind::Other => return Err(format!("{} is not a regular file", root.join(SOURCES_FILE).display())),
    };

    let mut f = Vec::new();
    let val = match fid_canon::parse_with_limits(text.as_bytes(), MAX_SOURCES_BYTES as usize, MAX_JSON_DEPTH) {
        Ok(v) => v,
        Err(e) => {
            schema(&mut f, SOURCES_FILE, format!("not valid JSON: {}", e.code()));
            return Ok(Report { tools: 0, sources: 0, exports_checked: 0, findings: f });
        }
    };
    if let Some(d) = value_defect(&val) {
        schema(&mut f, SOURCES_FILE, format!("text refused at {d}"));
        return Ok(Report { tools: 0, sources: 0, exports_checked: 0, findings: f });
    }
    let CValue::Obj(m) = &val else {
        schema(&mut f, SOURCES_FILE, format!("{SOURCES_FORMAT} must be a JSON object"));
        return Ok(Report { tools: 0, sources: 0, exports_checked: 0, findings: f });
    };
    let keys: Vec<&str> = m.keys().map(String::as_str).collect();
    if keys != TOP_KEYS {
        schema(&mut f, SOURCES_FILE, format!("{SOURCES_FORMAT} must have exactly the keys {}", TOP_KEYS.join(", ")));
    }
    match m.get("format") {
        Some(CValue::Str(fmt)) if fmt == SOURCES_FORMAT => {}
        _ => schema(&mut f, SOURCES_FILE, format!("`format` must be \"{SOURCES_FORMAT}\"")),
    }
    let tools = match m.get("tools") {
        Some(CValue::Arr(arr)) => parse_tools(arr, &mut f),
        _ => {
            schema(&mut f, SOURCES_FILE, "`tools` must be an array");
            BTreeMap::new()
        }
    };
    let mut exports_checked = 0usize;
    let sources_count = match m.get("sources") {
        Some(CValue::Arr(arr)) if arr.len() > MAX_ITEMS => {
            schema(&mut f, SOURCES_FILE, format!("`sources` holds more than {MAX_ITEMS} entries"));
            arr.len()
        }
        Some(CValue::Arr(arr)) => {
            let mut ctx =
                CheckCtx { root, spec, tools: &tools, seen_ids: BTreeSet::new(), exports_checked: 0, findings: &mut f };
            for (i, item) in arr.iter().enumerate() {
                ctx.check_source(i, item)?;
            }
            exports_checked = ctx.exports_checked;
            arr.len()
        }
        _ => {
            schema(&mut f, SOURCES_FILE, "`sources` must be an array");
            0
        }
    };
    f.sort();
    f.dedup();
    Ok(Report { tools: tools.len(), sources: sources_count, exports_checked, findings: f })
}

impl Report {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("format".to_string(), CValue::Str(REPORT_FORMAT.to_string()));
        m.insert("tools".to_string(), CValue::Num(self.tools as f64));
        m.insert("sources".to_string(), CValue::Num(self.sources as f64));
        m.insert("exports_checked".to_string(), CValue::Num(self.exports_checked as f64));
        m.insert("findings".to_string(), findings_value(&self.findings));
        m.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let mut s = format!(
            "fid-design sources-check: {} tool(s), {} source(s), {} export(s) checked\n",
            self.tools, self.sources, self.exports_checked
        );
        findings_text(&mut s, &self.findings);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validators() {
        assert!(valid_tool_id("penpot"));
        assert!(valid_tool_id("text-svg"));
        assert!(!valid_tool_id(""));
        assert!(!valid_tool_id("Penpot"));
        assert!(!valid_tool_id("1tool"));
        assert!(!valid_tool_id("a_b"));

        assert!(valid_source_id("screen-inspect-v1"));
        assert!(valid_source_id("report.summary_01"));
        assert!(!valid_source_id(""));
        assert!(!valid_source_id("Screen"));
        assert!(!valid_source_id("-bad"));

        assert!(valid_extension("penpot"));
        assert!(valid_extension("svg"));
        assert!(!valid_extension(".svg"));
        assert!(!valid_extension("SVG"));
        assert!(!valid_extension(""));

        assert!(valid_ux_node_id("UX"));
        assert!(valid_ux_node_id("UX.SYSTEM.TOOL"));
        assert!(valid_ux_node_id("UX.SCREENS.INSPECT.MAIN"));
        assert!(!valid_ux_node_id("UX.BOGUS"));
        assert!(!valid_ux_node_id("UX.BOGUS.LEAF"));
        assert!(!valid_ux_node_id("CORE.BUS"));
        assert!(!valid_ux_node_id("ux.system.tool"));
        assert!(!valid_ux_node_id(""));

        assert!(valid_location("sources/screen.penpot"));
        assert!(valid_location("https://design.penpot.app/#/view/example"));
        assert!(!valid_location("http://example.com/a"));
        assert!(!valid_location("https://"));
        assert!(!valid_location("https://user:pass@example.com/a"));
        assert!(!valid_location("https://example.com/a b"));
        assert!(!valid_location("../escape.svg"));
        assert!(!valid_location("/abs/path.svg"));
        assert!(!valid_location(""));
    }

    #[test]
    fn frontmatter_id_parsing() {
        assert_eq!(frontmatter_id("---\nid: UX.SYSTEM.TOOL\n---\n# Title\n"), Some("UX.SYSTEM.TOOL"));
        assert_eq!(frontmatter_id("no frontmatter\nid: UX\n"), None);
        assert_eq!(frontmatter_id("---\ntitle: X\n---\nid: UX\n"), None);
    }
}
