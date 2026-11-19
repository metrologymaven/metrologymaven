//! `conic-audit`: the vendored conic solver's audit check (spec node `KRN.SOLV.CONIC.VEND`).
//!
//! The vendored solver lives under `vendor/fid-clarabel/` (default; `--dir` changes it):
//!
//! ```text
//! upstream/                  pristine upstream tree at the pinned commit
//! patches/NNNN-<slug>.patch  numbered unified diffs, applied in numeric order
//! crate/                     the patched crate built into the kernel
//! AUDIT.toml                 the audit record (schema 1)
//! knp1-findings.json         the KNP-1 lint report the record was audited with (`--report`)
//! ```
//!
//! The check, offline and read-only:
//!
//! * `AUDIT-DIGEST`: the tree digest of `upstream/` differs from `upstream.tree_sha256`. The
//!   digest is SHA-256 over the lines `<sha256 hex of file content>  <relative path>\n`, one per
//!   regular file, sorted by the `/`-separated relative path (byte order); written `sha256:<hex>`.
//!   This is `sha256sum` output over the sorted file list, so it can be recomputed by hand.
//!   Symbolic links and other non-regular files are refused.
//! * `AUDIT-PATCH-DRIFT`: applying the listed patches in numeric order to a copy of `upstream/`
//!   with a strict applier (exact line positions: no fuzz and no offset) fails, or the result
//!   differs from `crate/` in any file (named in the finding).
//! * `AUDIT-LICENCE`: `upstream.licence` is not an SPDX expression allowed by
//!   `house-rules/spdx-allowlist.toml` (`licences` and `exceptions`). The expression grammar is a
//!   local copy of the KNP-1 lint's (`fid-knp1-lint` is a tool crate, so it cannot be a
//!   dependency), without the legacy `/` separator.
//! * `AUDIT-LINT`: the KNP-1 report (schema 1 of `KRN.NUM.LINT.FIND`) has an unwaived finding in
//!   the vendored crate (findings whose `crate_name` is `crate/Cargo.toml`'s package name), a
//!   waived finding there whose waiver is not listed under `[[waivers]]`, or a listed waiver no
//!   waived finding uses. A waiver is named `<file>:<first line>-<last line>:<rule>`, its
//!   `knp1-waivers.toml` fields.
//! * `AUDIT-PATCH-MISSING`: with `--previous` (the record at the merge base) naming another
//!   upstream commit, an earlier patch ID that is neither in the new record nor under
//!   `[[dropped]]` with a justification.
//! * `AUDIT-SCHEMA`: a missing, unknown or malformed field of `AUDIT.toml`, a patch that names
//!   neither a KNP-1 rule ID nor `scope-reduction`, or a patch file not listed in the record.
//!
//! Every input is read with a size bound; the output is sorted and deduplicated.

use fid_canon::CValue;
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use toml::{Table, Value};

/// Default vendored solver directory.
pub const DEFAULT_DIR: &str = "vendor/fid-clarabel";
/// Default SPDX allowlist (spec node `ECO.RULES.SPDX`).
pub const DEFAULT_ALLOWLIST: &str = "house-rules/spdx-allowlist.toml";
/// The audit record's file name inside the vendored directory.
pub const AUDIT_FILE: &str = "AUDIT.toml";
/// The lint report's default file name inside the vendored directory.
pub const REPORT_FILE: &str = "knp1-findings.json";
/// Audit record schema version.
pub const SCHEMA: i64 = 1;
/// Largest file read from a tree, in bytes.
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Largest record, patch, report or allowlist, in bytes.
pub const MAX_TEXT_BYTES: u64 = 16 * 1024 * 1024;
/// Most files in one tree.
pub const MAX_FILES: usize = 100_000;
/// Deepest directory nesting in a tree.
pub const MAX_DEPTH: usize = 48;
/// The reason class a patch may name instead of a KNP-1 rule ID.
pub const SCOPE_REDUCTION: &str = "scope-reduction";

/// Usage line of `cargo xtask conic-audit`.
pub const USAGE: &str = "usage: cargo xtask conic-audit [--dir <vendor dir>] [--audit <AUDIT.toml>] \
                         [--report <knp1-findings.json>] [--allowlist <spdx-allowlist.toml>] \
                         [--previous <AUDIT.toml>]";

/// Error codes of the check, in output order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Code {
    Digest,
    Licence,
    Lint,
    PatchDrift,
    PatchMissing,
    Schema,
}

impl Code {
    /// The stable code string.
    pub fn as_str(self) -> &'static str {
        match self {
            Code::Digest => "AUDIT-DIGEST",
            Code::Licence => "AUDIT-LICENCE",
            Code::Lint => "AUDIT-LINT",
            Code::PatchDrift => "AUDIT-PATCH-DRIFT",
            Code::PatchMissing => "AUDIT-PATCH-MISSING",
            Code::Schema => "AUDIT-SCHEMA",
        }
    }
}

/// One finding: `<code> <subject>: <detail>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub code: Code,
    /// What the finding is about: a path relative to the vendored directory, a patch ID or a field.
    pub subject: String,
    pub detail: String,
}

impl Finding {
    fn new(code: Code, subject: impl Into<String>, detail: impl Into<String>) -> Finding {
        Finding { code, subject: subject.into(), detail: detail.into() }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.code.as_str(), self.subject, self.detail)
    }
}

/// Paths the check reads.
#[derive(Debug, Clone)]
pub struct Options {
    /// The vendored directory (`upstream/`, `patches/`, `crate/`).
    pub dir: PathBuf,
    /// The audit record; default `<dir>/AUDIT.toml`.
    pub audit: Option<PathBuf>,
    /// The KNP-1 lint report; default `<dir>/knp1-findings.json`.
    pub report: Option<PathBuf>,
    /// The SPDX allowlist.
    pub allowlist: PathBuf,
    /// The audit record at the merge base, for the carry-forward check.
    pub previous: Option<PathBuf>,
}

impl Options {
    /// Defaults for a vendored directory.
    pub fn new(dir: impl Into<PathBuf>) -> Options {
        Options {
            dir: dir.into(),
            audit: None,
            report: None,
            allowlist: PathBuf::from(DEFAULT_ALLOWLIST),
            previous: None,
        }
    }
}

// ------------------------------------------------------------------ audit record

/// `[upstream]` of the record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Upstream {
    pub url: String,
    pub commit: String,
    pub tree_sha256: String,
    pub licence: String,
}

/// One `[[patches]]` entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Patch {
    pub id: String,
    /// Relative to the vendored directory: `patches/NNNN-<slug>.patch`.
    pub file: String,
    /// KNP-1 rule IDs addressed, or `scope-reduction`.
    pub rules: Vec<String>,
    pub rationale: String,
}

/// One `[[dropped]]` entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dropped {
    pub id: String,
    pub justification: String,
}

/// One `[[waivers]]` entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WaiverEntry {
    /// `<file>:<first line>-<last line>:<rule>` of a `knp1-waivers.toml` entry.
    pub waiver_ref: String,
    pub justification: String,
}

/// The audit record `AUDIT.toml`, schema 1.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditRecord {
    pub schema: i64,
    pub upstream: Upstream,
    pub patches: Vec<Patch>,
    pub dropped: Vec<Dropped>,
    pub waivers: Vec<WaiverEntry>,
    pub lint_report_sha256: String,
    pub auditor: String,
    pub date: String,
}

impl AuditRecord {
    /// IDs of the listed patches.
    pub fn patch_ids(&self) -> BTreeSet<String> {
        self.patches.iter().map(|p| p.id.clone()).collect()
    }
}

struct Fields<'a> {
    errs: &'a mut Vec<Finding>,
}

impl Fields<'_> {
    fn err(&mut self, subject: &str, detail: impl Into<String>) {
        self.errs.push(Finding::new(Code::Schema, subject, detail));
    }

    fn keys(&mut self, t: &Table, subject: &str, allowed: &[&str]) {
        for k in t.keys() {
            if !allowed.contains(&k.as_str()) {
                self.err(subject, format!("unknown field `{k}`"));
            }
        }
    }

    /// A required non-blank string.
    fn text(&mut self, t: &Table, key: &str, subject: &str) -> String {
        match t.get(key) {
            Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
            Some(Value::String(_)) => {
                self.err(subject, format!("`{key}` is empty"));
                String::new()
            }
            Some(_) => {
                self.err(subject, format!("`{key}` must be a string"));
                String::new()
            }
            None => {
                self.err(subject, format!("`{key}` is missing"));
                String::new()
            }
        }
    }

    fn tables<'t>(&mut self, t: &'t Table, key: &str) -> Vec<&'t Table> {
        match t.get(key) {
            None => Vec::new(),
            Some(Value::Array(a)) => {
                let mut out = Vec::new();
                for (k, v) in a.iter().enumerate() {
                    match v {
                        Value::Table(t) => out.push(t),
                        _ => self.err(&format!("{key}[{k}]"), "must be a table"),
                    }
                }
                out
            }
            Some(_) => {
                self.err(key, "must be an array of tables");
                Vec::new()
            }
        }
    }
}

fn is_patch_id(s: &str) -> bool {
    s.len() == 4 && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_lower_hex(s: &str) -> bool {
    s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let num = |r: std::ops::Range<usize>| {
        s.get(r).filter(|t| t.bytes().all(|c| c.is_ascii_digit())).and_then(|t| t.parse::<u32>().ok())
    };
    matches!((num(0..4), num(5..7), num(8..10)), (Some(_), Some(m), Some(d)) if (1..=12).contains(&m) && (1..=31).contains(&d))
}

/// Whether `s` is a KNP-1 rule ID (`KRN.NUM.LINT.FIND` catalogue: a fixed rule or a
/// `FLAG-`/`ALLOW-`/`DATA-` family member).
pub fn is_knp1_rule(s: &str) -> bool {
    const FIXED: [&str; 12] =
        ["FMA", "POW", "LIBM", "MINMAX", "HASH", "SIMD", "UNSAFE", "IO-FS", "IO-NET", "CLOCK", "ENV", "THREAD"];
    if s.len() > 64 {
        return false;
    }
    let Some(rest) = s.strip_prefix("KNP1-") else { return false };
    if FIXED.contains(&rest) {
        return true;
    }
    ["FLAG-", "ALLOW-", "DATA-"].iter().any(|fam| {
        rest.strip_prefix(fam).is_some_and(|suf| {
            !suf.is_empty()
                && suf
                    .split('-')
                    .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()))
        })
    })
}

/// Whether `file` is `patches/<id>-<slug>.patch` with a slug of letters, digits, `-`, `_`, `.`.
fn is_patch_file(file: &str, id: &str) -> bool {
    let Some(name) = file.strip_prefix("patches/") else { return false };
    let Some(stem) = name.strip_suffix(".patch") else { return false };
    let Some(slug) = stem.strip_prefix(id).and_then(|s| s.strip_prefix('-')) else { return false };
    !slug.is_empty() && slug.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Parses an audit record. `Err` when the text is not TOML; otherwise the record (fields that
/// failed validation are left empty) and its `AUDIT-SCHEMA` findings (REQ-006).
pub fn parse_record(text: &str) -> Result<(AuditRecord, Vec<Finding>), Finding> {
    let table: Table = text.parse().map_err(|e: toml::de::Error| {
        Finding::new(Code::Schema, "AUDIT.toml", format!("not TOML: {}", e.to_string().trim_end().replace('\n', " ")))
    })?;
    let mut errs = Vec::new();
    let mut f = Fields { errs: &mut errs };
    let mut r = AuditRecord::default();
    f.keys(
        &table,
        "AUDIT.toml",
        &["schema", "upstream", "patches", "dropped", "waivers", "lint_report_sha256", "auditor", "date"],
    );
    match table.get("schema") {
        Some(Value::Integer(n)) if *n == SCHEMA => r.schema = *n,
        Some(_) => f.err("schema", format!("must be the integer {SCHEMA}")),
        None => f.err("schema", "is missing"),
    }
    match table.get("upstream") {
        Some(Value::Table(u)) => {
            f.keys(u, "upstream", &["url", "commit", "tree_sha256", "licence"]);
            r.upstream.url = f.text(u, "url", "upstream");
            if !r.upstream.url.is_empty()
                && (!r.upstream.url.starts_with("https://") || r.upstream.url.contains(char::is_whitespace))
            {
                f.err("upstream", "`url` must be an https:// URL");
            }
            let commit = f.text(u, "commit", "upstream");
            if !commit.is_empty() {
                if (commit.len() == 40 || commit.len() == 64) && is_lower_hex(&commit) {
                    r.upstream.commit = commit;
                } else {
                    f.err("upstream", "`commit` must be a 40- or 64-digit lowercase hex commit ID");
                }
            }
            let tree = f.text(u, "tree_sha256", "upstream");
            if !tree.is_empty() {
                if Digest::is_valid(&tree) {
                    r.upstream.tree_sha256 = tree;
                } else {
                    f.err("upstream", "`tree_sha256` must be `sha256:` and 64 lowercase hex digits");
                }
            }
            r.upstream.licence = f.text(u, "licence", "upstream");
        }
        Some(_) => f.err("upstream", "must be a table"),
        None => f.err("upstream", "is missing"),
    }
    let mut ids = BTreeSet::new();
    for p in f.tables(&table, "patches") {
        let mut patch = Patch::default();
        let id = f.text(p, "id", "patches");
        let subject = if id.is_empty() { "patches".to_string() } else { format!("patch {id}") };
        f.keys(p, &subject, &["id", "file", "rules", "rationale"]);
        if !id.is_empty() && !is_patch_id(&id) {
            f.err(&subject, "`id` must be four digits");
        } else if !id.is_empty() && !ids.insert(id.clone()) {
            f.err(&subject, "duplicate patch `id`");
        }
        patch.id = id;
        let file = f.text(p, "file", &subject);
        if !file.is_empty() && !is_patch_file(&file, &patch.id) {
            f.err(&subject, format!("`file` must be patches/{}-<slug>.patch", patch.id));
        } else {
            patch.file = file;
        }
        match p.get("rules") {
            Some(Value::Array(a)) => {
                for v in a {
                    match v.as_str() {
                        Some(s) if is_knp1_rule(s) || s == SCOPE_REDUCTION => patch.rules.push(s.to_string()),
                        _ => f.err(
                            &subject,
                            format!("`rules` entry {v:?} is neither a KNP-1 rule ID nor `{SCOPE_REDUCTION}`"),
                        ),
                    }
                }
                if a.is_empty() {
                    f.err(&subject, format!("`rules` must name at least one KNP-1 rule ID or `{SCOPE_REDUCTION}`"));
                }
            }
            Some(_) => f.err(&subject, "`rules` must be an array of strings"),
            None => f.err(&subject, format!("`rules` is missing (name a KNP-1 rule ID or `{SCOPE_REDUCTION}`)")),
        }
        patch.rationale = f.text(p, "rationale", &subject);
        r.patches.push(patch);
    }
    let mut dropped_ids = BTreeSet::new();
    for d in f.tables(&table, "dropped") {
        let id = f.text(d, "id", "dropped");
        let subject = if id.is_empty() { "dropped".to_string() } else { format!("dropped {id}") };
        f.keys(d, &subject, &["id", "justification"]);
        if !id.is_empty() && !is_patch_id(&id) {
            f.err(&subject, "`id` must be four digits");
        } else if ids.contains(&id) {
            f.err(&subject, "a patch cannot be both listed and dropped");
        } else if !id.is_empty() && !dropped_ids.insert(id.clone()) {
            f.err(&subject, "duplicate dropped `id`");
        }
        let justification = f.text(d, "justification", &subject);
        r.dropped.push(Dropped { id, justification });
    }
    let mut refs = BTreeSet::new();
    for w in f.tables(&table, "waivers") {
        let waiver_ref = f.text(w, "waiver_ref", "waivers");
        let subject = if waiver_ref.is_empty() { "waivers".to_string() } else { format!("waiver {waiver_ref}") };
        f.keys(w, &subject, &["waiver_ref", "justification"]);
        if !waiver_ref.is_empty() && !refs.insert(waiver_ref.clone()) {
            f.err(&subject, "duplicate `waiver_ref`");
        }
        let justification = f.text(w, "justification", &subject);
        r.waivers.push(WaiverEntry { waiver_ref, justification });
    }
    let report = f.text(&table, "lint_report_sha256", "AUDIT.toml");
    if !report.is_empty() && !Digest::is_valid(&report) {
        f.err("lint_report_sha256", "must be `sha256:` and 64 lowercase hex digits");
    } else {
        r.lint_report_sha256 = report;
    }
    r.auditor = f.text(&table, "auditor", "AUDIT.toml");
    match table.get("date") {
        Some(Value::String(s)) if is_date(s) => r.date = s.clone(),
        Some(Value::Datetime(d)) if is_date(&d.to_string()) => r.date = d.to_string(),
        Some(_) => f.err("date", "must be a YYYY-MM-DD date"),
        None => f.err("date", "is missing"),
    }
    Ok((r, errs))
}

// ------------------------------------------------------------------ trees and digests

fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(max + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if bytes.len() as u64 > max {
        return Err(format!("{} is larger than {max} bytes", path.display()));
    }
    Ok(bytes)
}

fn read_text(path: &Path) -> Result<String, String> {
    String::from_utf8(read_bounded(path, MAX_TEXT_BYTES)?).map_err(|_| format!("{} is not UTF-8", path.display()))
}

/// Every regular file under `root`, keyed by its `/`-separated relative path. Symbolic links,
/// other non-regular files, non-UTF-8 names and names with a newline are refused.
pub fn read_tree(root: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut out = BTreeMap::new();
    walk(root, "", 0, &mut out)?;
    Ok(out)
}

fn walk(dir: &Path, prefix: &str, depth: usize, out: &mut BTreeMap<String, Vec<u8>>) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!("{prefix}: directories nested deeper than {MAX_DEPTH}"));
    }
    let entries = std::fs::read_dir(dir).map_err(|e| format!("cannot read directory {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read directory {}: {e}", dir.display()))?;
        let name = entry.file_name().into_string().map_err(|n| format!("{prefix}/{n:?}: file name is not UTF-8"))?;
        let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        if rel.contains('\n') {
            return Err(format!("{rel:?}: file name contains a newline"));
        }
        let kind = entry.file_type().map_err(|e| format!("{rel}: {e}"))?;
        if kind.is_symlink() {
            return Err(format!("{rel}: symbolic links are not allowed"));
        } else if kind.is_dir() {
            walk(&entry.path(), &rel, depth + 1, out)?;
        } else if kind.is_file() {
            if out.len() >= MAX_FILES {
                return Err(format!("more than {MAX_FILES} files"));
            }
            let bytes = read_bounded(&entry.path(), MAX_FILE_BYTES)?;
            out.insert(rel, bytes);
        } else {
            return Err(format!("{rel}: not a regular file"));
        }
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> Digest {
    Digest::from_bytes(Sha256::digest(bytes).into())
}

/// The tree digest (REQ-001): SHA-256 over `<content sha256 hex>  <path>\n`, sorted by path.
pub fn tree_digest(files: &BTreeMap<String, Vec<u8>>) -> Digest {
    let mut outer = Sha256::new();
    for (path, bytes) in files {
        outer.update(sha256(bytes).hex().as_bytes());
        outer.update(b"  ");
        outer.update(path.as_bytes());
        outer.update(b"\n");
    }
    Digest::from_bytes(outer.finalize().into())
}

// ------------------------------------------------------------------ strict patch applier

struct Hunk {
    old_start: usize,
    old_len: usize,
    /// (tag, line bytes including its newline unless marked `\ No newline at end of file`).
    body: Vec<(u8, Vec<u8>)>,
}

enum Target {
    Modify(String),
    Create(String),
    Delete(String),
}

fn safe_rel_path(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && !p.contains('\\')
        && !p.contains(':')
        && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
}

/// The path of a `---`/`+++` header (`None` for `/dev/null`), with its `a/`/`b/` prefix removed.
fn header_path(rest: &[u8], prefix: &str, line: usize) -> Result<Option<String>, String> {
    let end = rest.iter().position(|&b| b == b'\t').unwrap_or(rest.len());
    let raw = std::str::from_utf8(&rest[..end]).map_err(|_| format!("line {line}: path is not UTF-8"))?;
    if raw == "/dev/null" {
        return Ok(None);
    }
    match raw.strip_prefix(prefix) {
        Some(p) if safe_rel_path(p) => Ok(Some(p.to_string())),
        _ => Err(format!("line {line}: path {raw:?} must be {prefix}<relative path>")),
    }
}

fn parse_range(s: &str) -> Option<(usize, usize)> {
    let num = |t: &str| {
        if !t.is_empty() && t.len() <= 9 && t.bytes().all(|b| b.is_ascii_digit()) {
            t.parse::<usize>().ok()
        } else {
            None
        }
    };
    match s.split_once(',') {
        Some((a, b)) => Some((num(a)?, num(b)?)),
        None => Some((num(s)?, 1)),
    }
}

/// `@@ -A[,B] +C[,D] @@[ section]` to ((A, B), (C, D)).
fn parse_hunk_header(l: &[u8], line: usize) -> Result<((usize, usize), (usize, usize)), String> {
    let bad = || format!("line {line}: malformed hunk header");
    let text = std::str::from_utf8(l).map_err(|_| bad())?;
    let rest = text.strip_prefix("@@ -").ok_or_else(bad)?;
    let (ranges, _) = rest.split_once(" @@").ok_or_else(bad)?;
    let (old, new) = ranges.split_once(" +").ok_or_else(bad)?;
    Ok((parse_range(old).ok_or_else(bad)?, parse_range(new).ok_or_else(bad)?))
}

/// Applies one unified diff to `files` in place (REQ-002). Strict: every context and removed
/// line must match the old file exactly at the header's line number (no fuzz, no offset);
/// files are created from `/dev/null` and deleted to `/dev/null`. Text before a `---` header
/// (mail headers, `diff --git`, `index`) is ignored; binary, rename, copy and mode patches are
/// refused. Returns the number of file sections applied. On error `files` may be partly changed.
pub fn apply_patch(files: &mut BTreeMap<String, Vec<u8>>, patch: &[u8]) -> Result<usize, String> {
    const UNSUPPORTED: [&[u8]; 8] = [
        b"GIT binary patch",
        b"Binary files ",
        b"rename from ",
        b"rename to ",
        b"copy from ",
        b"copy to ",
        b"old mode ",
        b"new mode ",
    ];
    let mut lines: Vec<&[u8]> = patch.split(|&b| b == b'\n').collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let mut sections = 0;
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        if !l.starts_with(b"--- ") {
            if UNSUPPORTED.iter().any(|u| l.starts_with(u)) {
                return Err(format!("line {}: binary, rename, copy and mode changes are not supported", i + 1));
            }
            if l.starts_with(b"@@ ") {
                return Err(format!("line {}: hunk outside a file section", i + 1));
            }
            i += 1;
            continue;
        }
        let old = header_path(&l[4..], "a/", i + 1)?;
        let plus = lines
            .get(i + 1)
            .filter(|n| n.starts_with(b"+++ "))
            .ok_or_else(|| format!("line {}: `---` without `+++`", i + 1))?;
        let new = header_path(&plus[4..], "b/", i + 2)?;
        let target = match (old, new) {
            (Some(o), Some(n)) if o == n => Target::Modify(o),
            (Some(o), Some(n)) => return Err(format!("line {}: renames are not supported ({o} -> {n})", i + 1)),
            (None, Some(n)) => Target::Create(n),
            (Some(o), None) => Target::Delete(o),
            (None, None) => return Err(format!("line {}: both sides are /dev/null", i + 1)),
        };
        i += 2;
        let mut hunks = Vec::new();
        while i < lines.len() && lines[i].starts_with(b"@@ ") {
            let ((old_start, old_len), (_, new_len)) = parse_hunk_header(lines[i], i + 1)?;
            let header_line = i + 1;
            i += 1;
            let (mut seen_old, mut seen_new) = (0usize, 0usize);
            let mut body: Vec<(u8, Vec<u8>)> = Vec::new();
            while seen_old < old_len || seen_new < new_len {
                let Some(l) = lines.get(i) else {
                    return Err(format!("line {header_line}: hunk ends before its header's line counts"));
                };
                let Some((&tag, text)) = l.split_first() else {
                    return Err(format!("line {}: empty line in a hunk (context lines start with a space)", i + 1));
                };
                match tag {
                    b' ' => {
                        seen_old += 1;
                        seen_new += 1;
                    }
                    b'-' => seen_old += 1,
                    b'+' => seen_new += 1,
                    _ => return Err(format!("line {}: hunk line must start with ' ', '-' or '+'", i + 1)),
                }
                if seen_old > old_len || seen_new > new_len {
                    return Err(format!("line {header_line}: hunk is longer than its header says"));
                }
                let mut content = text.to_vec();
                content.push(b'\n');
                body.push((tag, content));
                i += 1;
                if lines.get(i).is_some_and(|n| n.starts_with(b"\\ ")) {
                    if let Some((_, c)) = body.last_mut() {
                        c.pop();
                    }
                    i += 1;
                }
            }
            hunks.push(Hunk { old_start, old_len, body });
        }
        if hunks.is_empty() {
            return Err(format!("line {}: file section without hunks", i));
        }
        let (path, old_bytes) = match &target {
            Target::Modify(p) | Target::Delete(p) => {
                (p.clone(), files.get(p).cloned().ok_or_else(|| format!("{p}: patched file does not exist"))?)
            }
            Target::Create(p) if files.contains_key(p) => return Err(format!("{p}: created file already exists")),
            Target::Create(p) => (p.clone(), Vec::new()),
        };
        let old_lines: Vec<&[u8]> = old_bytes.split_inclusive(|&b| b == b'\n').collect();
        let mut out = Vec::with_capacity(old_bytes.len());
        let mut cursor = 0usize;
        for (k, h) in hunks.iter().enumerate() {
            let start = if h.old_len == 0 { h.old_start } else { h.old_start.saturating_sub(1) };
            if (h.old_len > 0 && h.old_start == 0) || start < cursor || start > old_lines.len() {
                return Err(format!("{path}: hunk {} is out of order or out of range", k + 1));
            }
            for l in &old_lines[cursor..start] {
                out.extend_from_slice(l);
            }
            let mut pos = start;
            for (tag, content) in &h.body {
                if *tag != b'+' {
                    if old_lines.get(pos).copied() != Some(content.as_slice()) {
                        return Err(format!("{path}: hunk {} does not match line {}", k + 1, pos + 1));
                    }
                    pos += 1;
                }
                if *tag != b'-' {
                    out.extend_from_slice(content);
                }
            }
            cursor = pos;
        }
        for l in &old_lines[cursor..] {
            out.extend_from_slice(l);
        }
        match target {
            Target::Delete(_) if !out.is_empty() => return Err(format!("{path}: deletion leaves content behind")),
            Target::Delete(_) => {
                files.remove(&path);
            }
            Target::Modify(_) | Target::Create(_) => {
                files.insert(path, out);
            }
        }
        sections += 1;
    }
    if sections == 0 {
        return Err("no file changes".to_string());
    }
    Ok(sections)
}

// ------------------------------------------------------------------ SPDX expressions

/// Bounded SPDX licence-expression evaluator, a local copy of the KNP-1 lint's grammar
/// (`WITH` > `AND` > `OR`, upper-case operators, parentheses), without the legacy `/`.
pub mod spdx {
    use std::collections::BTreeSet;

    const MAX_EXPR_BYTES: usize = 1024;
    const MAX_TOKENS: usize = 256;
    const MAX_DEPTH: usize = 32;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Tok {
        Id(String),
        And,
        Or,
        With,
        Open,
        Close,
    }

    enum Expr {
        Licence(String),
        With(String, String),
        And(Box<Expr>, Box<Expr>),
        Or(Box<Expr>, Box<Expr>),
    }

    fn lex(text: &str) -> Result<Vec<Tok>, String> {
        if text.len() > MAX_EXPR_BYTES {
            return Err(format!("expression longer than {MAX_EXPR_BYTES} bytes"));
        }
        let is_id = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+' | b':');
        let b = text.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            let c = b[i];
            let tok = match c {
                b' ' | b'\t' => {
                    i += 1;
                    continue;
                }
                b'(' => {
                    i += 1;
                    Tok::Open
                }
                b')' => {
                    i += 1;
                    Tok::Close
                }
                _ if is_id(c) => {
                    let start = i;
                    while i < b.len() && is_id(b[i]) {
                        i += 1;
                    }
                    match text.get(start..i).unwrap_or_default() {
                        "AND" => Tok::And,
                        "OR" => Tok::Or,
                        "WITH" => Tok::With,
                        w => Tok::Id(w.to_string()),
                    }
                }
                _ => return Err(format!("unexpected character at byte {i}")),
            };
            out.push(tok);
            if out.len() > MAX_TOKENS {
                return Err(format!("more than {MAX_TOKENS} tokens"));
            }
        }
        Ok(out)
    }

    struct Parser {
        toks: Vec<Tok>,
        pos: usize,
    }

    impl Parser {
        fn bump(&mut self) -> Option<Tok> {
            let t = self.toks.get(self.pos).cloned();
            self.pos += 1;
            t
        }

        fn or(&mut self, depth: usize) -> Result<Expr, String> {
            let mut left = self.and(depth)?;
            while self.toks.get(self.pos) == Some(&Tok::Or) {
                self.pos += 1;
                left = Expr::Or(Box::new(left), Box::new(self.and(depth)?));
            }
            Ok(left)
        }

        fn and(&mut self, depth: usize) -> Result<Expr, String> {
            let mut left = self.with(depth)?;
            while self.toks.get(self.pos) == Some(&Tok::And) {
                self.pos += 1;
                left = Expr::And(Box::new(left), Box::new(self.with(depth)?));
            }
            Ok(left)
        }

        fn with(&mut self, depth: usize) -> Result<Expr, String> {
            let atom = self.atom(depth)?;
            if self.toks.get(self.pos) != Some(&Tok::With) {
                return Ok(atom);
            }
            self.pos += 1;
            let Expr::Licence(id) = atom else {
                return Err("WITH must follow a single licence identifier".to_string());
            };
            match self.bump() {
                Some(Tok::Id(exc)) => Ok(Expr::With(id, exc)),
                _ => Err("WITH must be followed by an exception identifier".to_string()),
            }
        }

        fn atom(&mut self, depth: usize) -> Result<Expr, String> {
            match self.bump() {
                Some(Tok::Id(id)) => Ok(Expr::Licence(id)),
                Some(Tok::Open) if depth >= MAX_DEPTH => Err(format!("parentheses nested deeper than {MAX_DEPTH}")),
                Some(Tok::Open) => {
                    let inner = self.or(depth + 1)?;
                    match self.bump() {
                        Some(Tok::Close) => Ok(inner),
                        _ => Err("missing `)`".to_string()),
                    }
                }
                Some(t) => Err(format!("unexpected token {t:?}")),
                None => Err("unexpected end of expression".to_string()),
            }
        }
    }

    fn allowed(e: &Expr, licences: &BTreeSet<String>, exceptions: &BTreeSet<String>) -> bool {
        match e {
            Expr::Licence(id) => licences.contains(id),
            Expr::With(id, exc) => licences.contains(id) && exceptions.contains(exc),
            Expr::And(a, b) => allowed(a, licences, exceptions) && allowed(b, licences, exceptions),
            Expr::Or(a, b) => allowed(a, licences, exceptions) || allowed(b, licences, exceptions),
        }
    }

    /// Parses `text` and evaluates it: `OR` needs one allowed side, `AND` both, `WITH` an allowed
    /// licence and exception. `Err` carries the parse error.
    pub fn evaluate(text: &str, licences: &BTreeSet<String>, exceptions: &BTreeSet<String>) -> Result<bool, String> {
        let toks = lex(text)?;
        if toks.is_empty() {
            return Err("empty expression".to_string());
        }
        let mut p = Parser { toks, pos: 0 };
        let e = p.or(0)?;
        if p.pos != p.toks.len() {
            return Err(format!("unexpected token after position {}", p.pos));
        }
        Ok(allowed(&e, licences, exceptions))
    }
}

fn string_set(t: &Table, key: &str) -> Result<BTreeSet<String>, String> {
    match t.get(key) {
        Some(Value::Array(a)) => a
            .iter()
            .map(|v| v.as_str().map(str::to_string).ok_or_else(|| format!("`{key}` must list strings")))
            .collect(),
        _ => Err(format!("`{key}` is missing or not an array")),
    }
}

/// `AUDIT-LICENCE` (REQ-003).
fn check_licence(record: &AuditRecord, allowlist: &Path, out: &mut BTreeSet<Finding>) {
    let licence = &record.upstream.licence;
    if licence.is_empty() {
        return; // already an AUDIT-SCHEMA finding
    }
    let lists = read_text(allowlist).and_then(|text| {
        let t: Table = text
            .parse()
            .map_err(|e: toml::de::Error| format!("{}: {}", allowlist.display(), e.to_string().trim_end()))?;
        Ok((string_set(&t, "licences")?, string_set(&t, "exceptions")?))
    });
    match lists {
        Err(e) => {
            out.insert(Finding::new(Code::Licence, "upstream.licence", format!("cannot read the allowlist: {e}")));
        }
        Ok((licences, exceptions)) => match spdx::evaluate(licence, &licences, &exceptions) {
            Ok(true) => {}
            Ok(false) => {
                out.insert(Finding::new(
                    Code::Licence,
                    "upstream.licence",
                    format!("{licence:?} is not allowed by the SPDX allowlist"),
                ));
            }
            Err(e) => {
                out.insert(Finding::new(
                    Code::Licence,
                    "upstream.licence",
                    format!("{licence:?} is not an SPDX expression: {e}"),
                ));
            }
        },
    }
}

// ------------------------------------------------------------------ lint report

fn obj<'a>(v: &'a CValue, what: &str) -> Result<&'a BTreeMap<String, CValue>, String> {
    match v {
        CValue::Obj(o) => Ok(o),
        _ => Err(format!("{what} must be an object")),
    }
}

fn arr<'a>(o: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a [CValue], String> {
    match o.get(key) {
        Some(CValue::Arr(a)) => Ok(a),
        _ => Err(format!("`{key}` must be an array")),
    }
}

fn s<'a>(o: &'a BTreeMap<String, CValue>, key: &str) -> Result<&'a str, String> {
    match o.get(key) {
        Some(CValue::Str(v)) => Ok(v),
        _ => Err(format!("`{key}` must be a string")),
    }
}

fn pos(v: Option<&CValue>, key: &str) -> Result<u64, String> {
    match v {
        Some(CValue::Num(n)) if (1.0..=4_294_967_295.0).contains(n) && n.trunc() == *n => Ok(*n as u64),
        _ => Err(format!("`{key}` must be an integer from 1 to 4294967295")),
    }
}

/// The vendored crate's package name from `crate/Cargo.toml`.
fn crate_name(dir: &Path) -> Result<String, String> {
    let text = read_text(&dir.join("crate").join("Cargo.toml"))?;
    let t: Table = text.parse().map_err(|e: toml::de::Error| format!("not TOML: {}", e.to_string().trim_end()))?;
    t.get("package")
        .and_then(Value::as_table)
        .and_then(|p| p.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "has no [package] name".to_string())
}

/// `AUDIT-LINT` (REQ-004).
fn check_lint(record: &AuditRecord, dir: &Path, report: &Path, out: &mut BTreeSet<Finding>) {
    let name = match crate_name(dir) {
        Ok(n) => n,
        Err(e) => {
            out.insert(Finding::new(Code::Lint, "crate/Cargo.toml", e));
            return;
        }
    };
    let parsed =
        read_bounded(report, MAX_TEXT_BYTES).and_then(|b| fid_canon::parse(&b).map_err(|e| format!("not JSON: {e}")));
    let result = parsed.and_then(|v| -> Result<Vec<Finding>, String> {
        let r = obj(&v, "the report")?;
        if r.get("schema") != Some(&CValue::Num(1.0)) || s(r, "tool")? != "fid-knp1-lint" {
            return Err("not a fid-knp1-lint report of schema 1".to_string());
        }
        let mut found = Vec::new();
        for f in arr(r, "findings")? {
            let f = obj(f, "a finding")?;
            if s(f, "crate_name")? == name {
                let at = format!(
                    "crate/{}:{}:{}",
                    s(f, "file")?,
                    pos(f.get("line"), "line")?,
                    pos(f.get("column"), "column")?
                );
                found.push(Finding::new(Code::Lint, at, format!("unwaived {} finding", s(f, "rule")?)));
            }
        }
        let listed: BTreeMap<&str, &str> =
            record.waivers.iter().map(|w| (w.waiver_ref.as_str(), w.justification.trim())).collect();
        let mut used = BTreeSet::new();
        for w in arr(r, "waived")? {
            let w = obj(w, "a waived entry")?;
            let f = obj(w.get("finding").ok_or("`finding` is missing")?, "`finding`")?;
            let wv = obj(w.get("waiver").ok_or("`waiver` is missing")?, "`waiver`")?;
            if s(f, "crate_name")? != name {
                continue;
            }
            let lines = match wv.get("lines") {
                Some(CValue::Arr(a)) if a.len() == 2 => (pos(a.first(), "lines")?, pos(a.get(1), "lines")?),
                _ => return Err("`lines` must be two line numbers".to_string()),
            };
            let waiver_ref = format!("{}:{}-{}:{}", s(wv, "file")?, lines.0, lines.1, s(wv, "rule")?);
            let at =
                format!("crate/{}:{}:{}", s(f, "file")?, pos(f.get("line"), "line")?, pos(f.get("column"), "column")?);
            match listed.get(waiver_ref.as_str()) {
                Some(j) if !j.is_empty() => {
                    used.insert(waiver_ref);
                }
                _ => found.push(Finding::new(
                    Code::Lint,
                    at,
                    format!(
                        "waived {} finding: waiver {waiver_ref} is not listed with a justification in AUDIT.toml",
                        s(f, "rule")?
                    ),
                )),
            }
        }
        for w in &record.waivers {
            if !w.waiver_ref.is_empty() && !used.contains(&w.waiver_ref) {
                found.push(Finding::new(
                    Code::Lint,
                    format!("waiver {}", w.waiver_ref),
                    "listed in AUDIT.toml but no waived finding of the vendored crate in the report uses it",
                ));
            }
        }
        Ok(found)
    });
    match result {
        Ok(found) => out.extend(found),
        Err(e) => {
            out.insert(Finding::new(Code::Lint, "report", format!("{}: {e}", report.display())));
        }
    }
}

// ------------------------------------------------------------------ the check

/// `AUDIT-DIGEST` and `AUDIT-PATCH-DRIFT` (REQ-001, REQ-002), plus patch files not in the record.
fn check_tree(record: &AuditRecord, dir: &Path, out: &mut BTreeSet<Finding>) {
    let listed: BTreeSet<&str> = record.patches.iter().map(|p| p.file.as_str()).collect();
    let mut on_disk = BTreeSet::new();
    match std::fs::read_dir(dir.join("patches")) {
        Ok(entries) => {
            for e in entries.flatten() {
                let name = format!("patches/{}", e.file_name().to_string_lossy());
                if !listed.contains(name.as_str()) {
                    out.insert(Finding::new(Code::Schema, name.clone(), "patch file is not listed in AUDIT.toml"));
                }
                on_disk.insert(name);
            }
        }
        Err(_) if record.patches.is_empty() => {}
        Err(e) => {
            out.insert(Finding::new(Code::PatchDrift, "patches/", format!("cannot read: {e}")));
        }
    }
    let upstream = match read_tree(&dir.join("upstream")) {
        Ok(t) => t,
        Err(e) => {
            out.insert(Finding::new(Code::Digest, "upstream/", e));
            return;
        }
    };
    let digest = tree_digest(&upstream);
    if !record.upstream.tree_sha256.is_empty() && digest.as_str() != record.upstream.tree_sha256 {
        out.insert(Finding::new(
            Code::Digest,
            "upstream/",
            format!("tree digest is {digest}, AUDIT.toml records {}", record.upstream.tree_sha256),
        ));
    }
    let mut patches: Vec<&Patch> = record.patches.iter().collect();
    patches.sort_by(|a, b| a.id.cmp(&b.id));
    let mut tree = upstream;
    for p in patches {
        if p.file.is_empty() {
            return; // already an AUDIT-SCHEMA finding; the result cannot be compared
        }
        if !on_disk.contains(&p.file) {
            out.insert(Finding::new(Code::Schema, format!("patch {}", p.id), format!("{} does not exist", p.file)));
            return;
        }
        let applied = read_bounded(&dir.join(&p.file), MAX_TEXT_BYTES).and_then(|bytes| apply_patch(&mut tree, &bytes));
        if let Err(e) = applied {
            out.insert(Finding::new(Code::PatchDrift, p.file.clone(), format!("does not apply: {e}")));
            return;
        }
    }
    let krate = match read_tree(&dir.join("crate")) {
        Ok(t) => t,
        Err(e) => {
            out.insert(Finding::new(Code::PatchDrift, "crate/", e));
            return;
        }
    };
    let paths: BTreeSet<&String> = tree.keys().chain(krate.keys()).collect();
    for p in paths {
        let detail = match (tree.get(p), krate.get(p)) {
            (Some(a), Some(b)) if a == b => continue,
            (Some(_), Some(_)) => "differs from upstream/ plus the patches",
            (Some(_), None) => "missing from crate/ but produced by upstream/ plus the patches",
            _ => "not produced by upstream/ plus the patches",
        };
        out.insert(Finding::new(Code::PatchDrift, format!("crate/{p}"), detail));
    }
}

/// `AUDIT-PATCH-MISSING` (REQ-005).
fn check_previous(record: &AuditRecord, previous: &Path, out: &mut BTreeSet<Finding>) {
    let prev = match read_text(previous)
        .map_err(|e| Finding::new(Code::Schema, "--previous", e))
        .and_then(|t| parse_record(&t))
    {
        Ok((prev, errs)) => {
            for e in errs {
                out.insert(Finding::new(Code::Schema, format!("--previous {}", e.subject), e.detail));
            }
            prev
        }
        Err(f) => {
            out.insert(Finding::new(Code::Schema, format!("--previous {}", f.subject), f.detail));
            return;
        }
    };
    if prev.upstream.commit.is_empty()
        || record.upstream.commit.is_empty()
        || prev.upstream.commit == record.upstream.commit
    {
        return;
    }
    let present = record.patch_ids();
    let dropped: BTreeSet<&str> =
        record.dropped.iter().filter(|d| !d.justification.trim().is_empty()).map(|d| d.id.as_str()).collect();
    for id in prev.patch_ids() {
        if !present.contains(&id) && !dropped.contains(id.as_str()) {
            out.insert(Finding::new(
                Code::PatchMissing,
                id,
                "patch of the previous audit record is neither present nor listed under [[dropped]] with a justification",
            ));
        }
    }
}

/// Runs every check; returns the findings sorted by code, subject and detail (empty: pass).
pub fn audit(opts: &Options) -> Vec<Finding> {
    let mut out = BTreeSet::new();
    let audit_path = opts.audit.clone().unwrap_or_else(|| opts.dir.join(AUDIT_FILE));
    let parsed =
        read_text(&audit_path).map_err(|e| Finding::new(Code::Schema, "AUDIT.toml", e)).and_then(|t| parse_record(&t));
    let record = match parsed {
        Ok((record, errs)) => {
            out.extend(errs);
            record
        }
        Err(f) => return vec![f],
    };
    check_tree(&record, &opts.dir, &mut out);
    check_licence(&record, &opts.allowlist, &mut out);
    let report = opts.report.clone().unwrap_or_else(|| opts.dir.join(REPORT_FILE));
    check_lint(&record, &opts.dir, &report, &mut out);
    if let Some(prev) = &opts.previous {
        check_previous(&record, prev, &mut out);
    }
    out.into_iter().collect()
}

/// `cargo xtask conic-audit [--dir] [--audit] [--report] [--allowlist] [--previous]`. Prints the
/// findings, one per line; exit 0 when there are none, 1 when there are, 2 on bad usage.
pub fn cli(args: &[String]) -> ExitCode {
    let mut opts = Options::new(DEFAULT_DIR);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--dir", Some(v)) => opts.dir = PathBuf::from(v),
            ("--audit", Some(v)) => opts.audit = Some(PathBuf::from(v)),
            ("--report", Some(v)) => opts.report = Some(PathBuf::from(v)),
            ("--allowlist", Some(v)) => opts.allowlist = PathBuf::from(v),
            ("--previous", Some(v)) => opts.previous = Some(PathBuf::from(v)),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let findings = audit(&opts);
    if findings.is_empty() {
        println!("conic-audit: ok");
        return ExitCode::SUCCESS;
    }
    for f in &findings {
        println!("{f}");
    }
    ExitCode::from(1)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn tree(files: &[(&str, &str)]) -> BTreeMap<String, Vec<u8>> {
        files.iter().map(|(p, c)| (p.to_string(), c.as_bytes().to_vec())).collect()
    }

    #[test]
    fn digest_of_empty_tree_is_sha256_of_nothing() {
        assert_eq!(
            tree_digest(&BTreeMap::new()).as_str(),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn digest_is_sha256sum_of_the_sorted_list() {
        // sha256("a\n") (hand-checked with `printf 'a\n' | sha256sum`), so the list is the one
        // line `sha256sum x` prints.
        let t = tree(&[("x", "a\n")]);
        let line = "87428fc522803d31065e7bce3cf03fe475096631e5e07bbd7a0fde60c4cf25c7  x\n";
        assert_eq!(tree_digest(&t), sha256(line.as_bytes()));
    }

    #[test]
    fn modify_create_delete_and_no_newline() {
        let mut t = tree(&[("a.txt", "one\ntwo\nthree\n"), ("gone.txt", "bye\n")]);
        let patch = "intro text\n\
                     --- a/a.txt\n+++ b/a.txt\n@@ -2,2 +2,2 @@\n two\n-three\n+THREE\n\\ No newline at end of file\n\
                     --- /dev/null\n+++ b/new/n.txt\n@@ -0,0 +1,2 @@\n+n1\n+n2\n\
                     --- a/gone.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-bye\n";
        assert_eq!(apply_patch(&mut t, patch.as_bytes()), Ok(3));
        assert_eq!(t, tree(&[("a.txt", "one\ntwo\nTHREE"), ("new/n.txt", "n1\nn2\n")]));
    }

    #[test]
    fn insertion_hunk_with_zero_old_lines() {
        let mut t = tree(&[("f", "a\nb\n")]);
        apply_patch(&mut t, b"--- a/f\n+++ b/f\n@@ -1,0 +2 @@\n+x\n").unwrap();
        assert_eq!(t, tree(&[("f", "a\nx\nb\n")]));
    }

    #[test]
    fn no_offset_and_no_fuzz() {
        let base = tree(&[("f", "a\nb\nc\nd\n")]);
        // Right content, wrong line number: refused (no offset).
        let mut t = base.clone();
        let e = apply_patch(&mut t, b"--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n b\n-c\n+C\n").unwrap_err();
        assert!(e.contains("does not match line 1"), "{e}");
        // One context line differs: refused (no fuzz).
        let mut t = base.clone();
        assert!(apply_patch(&mut t, b"--- a/f\n+++ b/f\n@@ -2,2 +2,2 @@\n B\n-c\n+C\n").is_err());
        // Exact: applies.
        let mut t = base;
        apply_patch(&mut t, b"--- a/f\n+++ b/f\n@@ -2,2 +2,2 @@\n b\n-c\n+C\n").unwrap();
        assert_eq!(t, tree(&[("f", "a\nb\nC\nd\n")]));
    }

    #[test]
    fn malformed_patches_are_refused() {
        let base = tree(&[("f", "a\n")]);
        let bad: [&[u8]; 9] = [
            b"",
            b"just text\n",
            b"--- a/f\n+++ b/f\n",
            b"--- a/f\n+++ b/f\n@@ -1 +1 @@\n-a\n",
            b"--- a/f\n+++ b/f\n@@ -1 +1 @@\n\n",
            b"--- a/../f\n+++ b/../f\n@@ -1 +1 @@\n-a\n+b\n",
            b"--- a/f\n+++ b/g\n@@ -1 +1 @@\n-a\n+b\n",
            b"diff --git a/f b/f\nGIT binary patch\n",
            b"--- a/missing\n+++ b/missing\n@@ -1 +1 @@\n-a\n+b\n",
        ];
        for p in bad {
            let mut t = base.clone();
            assert!(apply_patch(&mut t, p).is_err(), "{}", String::from_utf8_lossy(p));
        }
    }

    #[test]
    fn rule_ids() {
        for ok in ["KNP1-POW", "KNP1-IO-FS", "KNP1-FLAG-TARGET-CPU", "KNP1-ALLOW-SPDX", "KNP1-DATA-X1"] {
            assert!(is_knp1_rule(ok), "{ok}");
        }
        for bad in
            ["KNP1-pow", "KNP1-", "KNP1-FLAG-", "KNP1-FLAG-a", "KNP1-FLAG--X", "POW", "KNP1-NOPE", SCOPE_REDUCTION]
        {
            assert!(!is_knp1_rule(bad), "{bad}");
        }
    }

    #[test]
    fn spdx_expressions() {
        let lic: BTreeSet<String> = ["MIT", "Apache-2.0"].iter().map(|s| s.to_string()).collect();
        let exc: BTreeSet<String> = ["LLVM-exception"].iter().map(|s| s.to_string()).collect();
        let ev = |t: &str| spdx::evaluate(t, &lic, &exc);
        assert_eq!(ev("MIT"), Ok(true));
        assert_eq!(ev("Apache-2.0 WITH LLVM-exception"), Ok(true));
        assert_eq!(ev("GPL-3.0-only OR (MIT AND Apache-2.0)"), Ok(true));
        assert_eq!(ev("MIT AND GPL-3.0-only"), Ok(false));
        assert_eq!(ev("Apache-2.0 WITH Classpath-exception-2.0"), Ok(false));
        for bad in ["", "MIT/Apache-2.0", "(MIT", "MIT OR", "mit or MIT ;"] {
            assert!(ev(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn dates() {
        assert!(is_date("2026-11-18"));
        assert!(!is_date("2026-13-01") && !is_date("2026-1-01") && !is_date("26-11-18x"));
    }
}
