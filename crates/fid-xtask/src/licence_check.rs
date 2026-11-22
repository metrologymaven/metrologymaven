//! Dependency licence and asset attribution check (spec node `ECO.CI.LIC`).
//!
//! `cargo xtask licence-check --metadata <json> --allow <toml> --root <dir> [--clarify <toml>]`
//!
//! Evaluates every non-workspace dependency in a `cargo metadata --format-version 1` document
//! against an external SPDX allowlist TOML (`licences`, `exceptions`, `asset_licences`,
//! `asset_paths` — owned by `ECO.RULES.SPDX`; no licence list is embedded in code), applies
//! digest-pinned clarifications for packages without a declared licence, and verifies that
//! every file under `asset_paths` has a matching row in `<root>/ATTRIBUTION.md` and that no
//! row in `ATTRIBUTION.md` is orphaned.
//!
//! Bounded, offline, D0 deterministic: ordered collections only, bounded file reads, bounded
//! SPDX parser (max nesting depth 16, max 256 tokens), memoized linear-time glob matcher.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fid_canon::{parse as parse_json, CValue};
use sha2::{Digest as _, Sha256};

/// CLI usage string for `cargo xtask licence-check`.
pub const USAGE: &str =
    "usage: cargo xtask licence-check --metadata <json> --allow <toml> --root <dir> [--clarify <toml>]";

/// Maximum size of a single input file (`--metadata`, `--allow`, `--clarify`, `ATTRIBUTION.md`,
/// or a referenced licence file).
pub const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;

/// Maximum nesting depth of parentheses in an SPDX expression (Design).
pub const MAX_SPDX_DEPTH: usize = 16;

/// Maximum number of tokens in an SPDX expression (Design).
pub const MAX_SPDX_TOKENS: usize = 256;

/// Maximum directory nesting depth scanned under `--root`.
pub const MAX_WALK_DEPTH: usize = 32;

/// Maximum number of directory entries scanned under `--root`.
pub const MAX_WALK_ENTRIES: usize = 100_000;

/// Directories skipped while scanning `--root` for assets.
pub const SKIPPED_DIRS: [&str; 3] = [".git", "target", "node_modules"];

/// Machine-readable finding codes (`ECO.CI.LIC` Design).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FindingCode {
    AttrMissing,
    AttrOrphan,
    LicClarifyStale,
    LicDenied,
    LicException,
    LicMissing,
    LicParse,
}

impl FindingCode {
    pub fn as_str(self) -> &'static str {
        match self {
            FindingCode::AttrMissing => "ATTR-MISSING",
            FindingCode::AttrOrphan => "ATTR-ORPHAN",
            FindingCode::LicClarifyStale => "LIC-CLARIFY-STALE",
            FindingCode::LicDenied => "LIC-DENIED",
            FindingCode::LicException => "LIC-EXCEPTION",
            FindingCode::LicMissing => "LIC-MISSING",
            FindingCode::LicParse => "LIC-PARSE",
        }
    }
}

/// One sorted, byte-stable finding line (REQ-ECO.CI.LIC-005).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub code: FindingCode,
    pub subject: String,
    pub detail: String,
}

impl Finding {
    fn new(code: FindingCode, subject: impl Into<String>, detail: impl Into<String>) -> Self {
        Self { code, subject: subject.into(), detail: detail.into() }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.detail.is_empty() {
            write!(f, "{} {}", self.code.as_str(), self.subject)
        } else {
            write!(f, "{} {} {}", self.code.as_str(), self.subject, self.detail)
        }
    }
}

/// Operational input/parse errors (distinct from policy findings; exit 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputError {
    pub context: String,
    pub detail: String,
}

impl InputError {
    fn new(context: impl Into<String>, detail: impl Into<String>) -> Self {
        Self { context: context.into(), detail: detail.into() }
    }
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "licence-check: {}: {}", self.context, self.detail)
    }
}

impl std::error::Error for InputError {}

/// Parsed allowlist TOML (`ECO.RULES.SPDX` schema).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allowlist {
    pub licences: BTreeSet<String>,
    pub exceptions: BTreeSet<String>,
    pub asset_licences: BTreeSet<String>,
    pub asset_paths: Vec<String>,
}

impl Allowlist {
    /// Parses an allowlist TOML document. No licences are hardcoded.
    pub fn parse(text: &str) -> Result<Self, InputError> {
        let tbl: toml::Table = toml::from_str(text).map_err(|e| InputError::new("allowlist", e.to_string()))?;
        let licences = string_array(&tbl, "licences", "allowlist")?.into_iter().collect();
        let exceptions = string_array(&tbl, "exceptions", "allowlist")?.into_iter().collect();
        let asset_licences = string_array(&tbl, "asset_licences", "allowlist")?.into_iter().collect();
        let asset_paths = string_array(&tbl, "asset_paths", "allowlist")?;
        Ok(Self { licences, exceptions, asset_licences, asset_paths })
    }

    fn allows_licence(&self, id: &str, plus: bool) -> bool {
        if plus {
            let with_plus = format!("{id}+");
            if self.licences.contains(&with_plus) {
                return true;
            }
        }
        self.licences.contains(id)
    }

    fn allows_exception(&self, id: &str) -> bool {
        self.exceptions.contains(id)
    }
}

/// One `[[clarify]]` entry pinning a package's licence by SHA-256 of its licence file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clarification {
    pub package: String,
    pub version: String,
    pub expression: String,
    pub licence_file_sha256: String,
}

/// Parsed clarification map keyed by `(package, version)`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Clarifications {
    pub entries: BTreeMap<(String, String), Clarification>,
}

impl Clarifications {
    pub fn parse(text: &str) -> Result<Self, InputError> {
        let tbl: toml::Table = toml::from_str(text).map_err(|e| InputError::new("clarify", e.to_string()))?;
        let mut entries = BTreeMap::new();
        let Some(val) = tbl.get("clarify") else {
            return Ok(Self { entries });
        };
        let Some(arr) = val.as_array() else {
            return Err(InputError::new("clarify", "`clarify` must be an array of tables"));
        };
        for (i, item) in arr.iter().enumerate() {
            let Some(t) = item.as_table() else {
                return Err(InputError::new("clarify", format!("clarify[{i}] must be a table")));
            };
            let package = req_str(t, "package", i)?;
            let version = req_str(t, "version", i)?;
            let expression = req_str(t, "expression", i)?;
            let raw_digest = req_str(t, "licence_file_sha256", i)?;
            let digest = normalize_sha256(&raw_digest).ok_or_else(|| {
                InputError::new("clarify", format!("clarify[{i}].licence_file_sha256 is not a valid sha256 digest"))
            })?;
            let key = (package.clone(), version.clone());
            if entries
                .insert(key, Clarification { package, version, expression, licence_file_sha256: digest })
                .is_some()
            {
                return Err(InputError::new("clarify", format!("duplicate clarification entry at clarify[{i}]")));
            }
        }
        Ok(Self { entries })
    }
}

fn string_array(tbl: &toml::Table, key: &str, ctx: &str) -> Result<Vec<String>, InputError> {
    let Some(val) = tbl.get(key) else {
        return Err(InputError::new(ctx, format!("missing required key `{key}`")));
    };
    let Some(arr) = val.as_array() else {
        return Err(InputError::new(ctx, format!("`{key}` must be an array of strings")));
    };
    let mut out = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let Some(s) = item.as_str() else {
            return Err(InputError::new(ctx, format!("`{key}[{i}]` must be a string")));
        };
        out.push(s.to_string());
    }
    Ok(out)
}

fn req_str(tbl: &toml::Table, key: &str, idx: usize) -> Result<String, InputError> {
    match tbl.get(key).and_then(toml::Value::as_str) {
        Some(s) if !s.is_empty() => Ok(s.to_string()),
        _ => Err(InputError::new("clarify", format!("clarify[{idx}] missing non-empty string `{key}`"))),
    }
}

fn normalize_sha256(raw: &str) -> Option<String> {
    let hex = raw.strip_prefix("sha256:").unwrap_or(raw);
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("sha256:{}", hex.to_ascii_lowercase()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(7 + 64);
    out.push_str("sha256:");
    for b in digest {
        let hi = b >> 4;
        let lo = b & 0x0f;
        out.push(hex_nibble(hi));
        out.push(hex_nibble(lo));
    }
    out
}

fn hex_nibble(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + (n - 10)) as char,
        _ => '0',
    }
}

// ============================================================================
// Bounded SPDX expression parser & evaluator
// ============================================================================

/// Parsed SPDX licence expression AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpdxExpr {
    Simple { id: String, plus: bool, exception: Option<String> },
    And(Box<SpdxExpr>, Box<SpdxExpr>),
    Or(Box<SpdxExpr>, Box<SpdxExpr>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    LParen,
    RParen,
    Plus,
    And,
    Or,
    With,
    Id(String),
}

/// Tokenises an SPDX expression with a hard cap of [`MAX_SPDX_TOKENS`] tokens.
fn tokenize_spdx(input: &str) -> Result<Vec<Tok>, String> {
    let bytes = input.as_bytes();
    let mut i = 0usize;
    let mut toks = Vec::new();
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if toks.len() >= MAX_SPDX_TOKENS {
            return Err(format!("SPDX expression exceeds {MAX_SPDX_TOKENS} tokens"));
        }
        match b {
            b'(' => {
                toks.push(Tok::LParen);
                i += 1;
            }
            b')' => {
                toks.push(Tok::RParen);
                i += 1;
            }
            b'+' => {
                toks.push(Tok::Plus);
                i += 1;
            }
            c if is_id_char(c) => {
                let start = i;
                while i < bytes.len() && is_id_char(bytes[i]) {
                    i += 1;
                }
                let word = &input[start..i];
                let tok = match word {
                    "AND" => Tok::And,
                    "OR" => Tok::Or,
                    "WITH" => Tok::With,
                    _ => Tok::Id(word.to_string()),
                };
                toks.push(tok);
            }
            _ => {
                return Err(format!("unexpected character {:?} at byte {i}", b as char));
            }
        }
    }
    if toks.is_empty() {
        return Err("empty SPDX expression".to_string());
    }
    Ok(toks)
}

fn is_id_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':')
}

struct SpdxParser {
    toks: Vec<Tok>,
    pos: usize,
}

impl SpdxParser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        if self.pos < self.toks.len() {
            let t = self.toks[self.pos].clone();
            self.pos += 1;
            Some(t)
        } else {
            None
        }
    }

    fn parse_or(&mut self, depth: usize) -> Result<SpdxExpr, String> {
        let mut lhs = self.parse_and(depth)?;
        while matches!(self.peek(), Some(Tok::Or)) {
            self.next();
            let rhs = self.parse_and(depth)?;
            lhs = SpdxExpr::Or(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_and(&mut self, depth: usize) -> Result<SpdxExpr, String> {
        let mut lhs = self.parse_atom(depth)?;
        while matches!(self.peek(), Some(Tok::And)) {
            self.next();
            let rhs = self.parse_atom(depth)?;
            lhs = SpdxExpr::And(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_atom(&mut self, depth: usize) -> Result<SpdxExpr, String> {
        match self.next() {
            Some(Tok::LParen) => {
                let next_depth = depth + 1;
                if next_depth > MAX_SPDX_DEPTH {
                    return Err(format!("SPDX nesting depth exceeds {MAX_SPDX_DEPTH}"));
                }
                let inner = self.parse_or(next_depth)?;
                if !matches!(self.next(), Some(Tok::RParen)) {
                    return Err("unclosed `(` in SPDX expression".to_string());
                }
                Ok(inner)
            }
            Some(Tok::Id(id)) => {
                let plus = if matches!(self.peek(), Some(Tok::Plus)) {
                    self.next();
                    true
                } else {
                    false
                };
                let exception = if matches!(self.peek(), Some(Tok::With)) {
                    self.next();
                    match self.next() {
                        Some(Tok::Id(exc)) => Some(exc),
                        _ => return Err("expected exception ID after `WITH`".to_string()),
                    }
                } else {
                    None
                };
                Ok(SpdxExpr::Simple { id, plus, exception })
            }
            Some(other) => Err(format!("unexpected token `{other:?}` in SPDX expression")),
            None => Err("unexpected end of SPDX expression".to_string()),
        }
    }
}

/// Parses an SPDX expression under the depth (<= 16) and token (<= 256) bounds.
pub fn parse_spdx(input: &str) -> Result<SpdxExpr, String> {
    let toks = tokenize_spdx(input)?;
    let mut parser = SpdxParser { toks, pos: 0 };
    let expr = parser.parse_or(0)?;
    if let Some(extra) = parser.peek() {
        return Err(format!("unexpected trailing token `{extra:?}`"));
    }
    Ok(expr)
}

/// Result of evaluating an SPDX expression against an [`Allowlist`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalOutcome {
    Allowed,
    DeniedLicence,
    DeniedException(String),
}

/// Evaluates an SPDX expression against `allow`.
///
/// - `Simple { id, plus, None }`: `Allowed` iff `id` (or `id+`) is in `allow.licences`.
/// - `Simple { id, plus, Some(exc) }`: `DeniedException` if `exc` is not in `allow.exceptions`,
///   else `Allowed` iff `id` is allowed.
/// - `And(a, b)`: both must be `Allowed`; if either fails with `DeniedException`, propagates it.
/// - `Or(a, b)`: either `Allowed` satisfies the expression; otherwise prefers `DeniedException`
///   if any branch failed on an exception.
pub fn eval_spdx(expr: &SpdxExpr, allow: &Allowlist) -> EvalOutcome {
    match expr {
        SpdxExpr::Simple { id, plus, exception } => {
            if let Some(exc) = exception {
                if !allow.allows_exception(exc) {
                    return EvalOutcome::DeniedException(exc.clone());
                }
            }
            if allow.allows_licence(id, *plus) {
                EvalOutcome::Allowed
            } else {
                EvalOutcome::DeniedLicence
            }
        }
        SpdxExpr::And(a, b) => match (eval_spdx(a, allow), eval_spdx(b, allow)) {
            (EvalOutcome::Allowed, EvalOutcome::Allowed) => EvalOutcome::Allowed,
            (EvalOutcome::DeniedException(e), _) | (_, EvalOutcome::DeniedException(e)) => {
                EvalOutcome::DeniedException(e)
            }
            _ => EvalOutcome::DeniedLicence,
        },
        SpdxExpr::Or(a, b) => match (eval_spdx(a, allow), eval_spdx(b, allow)) {
            (EvalOutcome::Allowed, _) | (_, EvalOutcome::Allowed) => EvalOutcome::Allowed,
            (EvalOutcome::DeniedException(e), _) | (_, EvalOutcome::DeniedException(e)) => {
                EvalOutcome::DeniedException(e)
            }
            _ => EvalOutcome::DeniedLicence,
        },
    }
}

// ============================================================================
// Metadata parsing & dependency licence evaluation
// ============================================================================

#[derive(Debug, Clone)]
struct MetaPackage {
    id: Option<String>,
    name: String,
    version: String,
    license: Option<String>,
    license_file: Option<String>,
    source: Option<String>,
    manifest_path: Option<String>,
}

struct MetadataDoc {
    workspace_members: BTreeSet<String>,
    has_workspace_members_field: bool,
    packages: Vec<MetaPackage>,
}

fn parse_metadata(bytes: &[u8]) -> Result<MetadataDoc, InputError> {
    let val = parse_json(bytes).map_err(|e| InputError::new("metadata", format!("json: {e}")))?;
    let CValue::Obj(root) = val else {
        return Err(InputError::new("metadata", "root must be a JSON object"));
    };
    let mut workspace_members = BTreeSet::new();
    let has_workspace_members_field = root.contains_key("workspace_members");
    if let Some(wm) = root.get("workspace_members") {
        let CValue::Arr(arr) = wm else {
            return Err(InputError::new("metadata", "`workspace_members` must be an array"));
        };
        for (i, item) in arr.iter().enumerate() {
            let CValue::Str(id) = item else {
                return Err(InputError::new("metadata", format!("`workspace_members[{i}]` must be a string")));
            };
            workspace_members.insert(id.clone());
        }
    }
    let Some(CValue::Arr(pkgs)) = root.get("packages") else {
        return Err(InputError::new("metadata", "missing `packages` array"));
    };
    let mut packages = Vec::with_capacity(pkgs.len());
    for (i, p) in pkgs.iter().enumerate() {
        let CValue::Obj(o) = p else {
            return Err(InputError::new("metadata", format!("`packages[{i}]` must be an object")));
        };
        let Some(CValue::Str(name)) = o.get("name") else {
            return Err(InputError::new("metadata", format!("`packages[{i}].name` must be a string")));
        };
        let Some(CValue::Str(version)) = o.get("version") else {
            return Err(InputError::new("metadata", format!("`packages[{i}].version` must be a string")));
        };
        let id = opt_str(o, "id", i)?;
        let license = opt_str(o, "license", i)?;
        let license_file = opt_str(o, "license_file", i)?;
        let source = opt_str(o, "source", i)?;
        let manifest_path = opt_str(o, "manifest_path", i)?;
        packages.push(MetaPackage {
            id,
            name: name.clone(),
            version: version.clone(),
            license,
            license_file,
            source,
            manifest_path,
        });
    }
    Ok(MetadataDoc { workspace_members, has_workspace_members_field, packages })
}

fn opt_str(o: &BTreeMap<String, CValue>, key: &str, idx: usize) -> Result<Option<String>, InputError> {
    match o.get(key) {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.clone())),
        _ => Err(InputError::new("metadata", format!("`packages[{idx}].{key}` must be a string or null"))),
    }
}

fn is_workspace_member(pkg: &MetaPackage, doc: &MetadataDoc) -> bool {
    if doc.has_workspace_members_field {
        if let Some(id) = &pkg.id {
            return doc.workspace_members.contains(id);
        }
        return false;
    }
    pkg.source.is_none()
}

fn resolve_licence_file(pkg: &MetaPackage, lf: &str, root: &Path, metadata_dir: &Path) -> PathBuf {
    let p = Path::new(lf);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    if let Some(mp) = &pkg.manifest_path {
        if let Some(parent) = Path::new(mp).parent() {
            let candidate = parent.join(p);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    let under_meta = metadata_dir.join(p);
    if under_meta.is_file() {
        return under_meta;
    }
    root.join(p)
}

fn check_packages(
    doc: &MetadataDoc,
    allow: &Allowlist,
    clarifications: &Clarifications,
    root: &Path,
    metadata_dir: &Path,
    findings: &mut BTreeSet<Finding>,
) {
    for pkg in &doc.packages {
        if is_workspace_member(pkg, doc) {
            continue;
        }
        let subject = format!("{} {}", pkg.name, pkg.version);
        let declared = pkg.license.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let expr_str: String = match declared {
            Some(expr) => expr.to_string(),
            None => {
                let key = (pkg.name.clone(), pkg.version.clone());
                let Some(clar) = clarifications.entries.get(&key) else {
                    findings.insert(Finding::new(FindingCode::LicMissing, subject, ""));
                    continue;
                };
                let Some(lf) = pkg.license_file.as_deref().filter(|s| !s.is_empty()) else {
                    findings.insert(Finding::new(
                        FindingCode::LicClarifyStale,
                        subject,
                        "package declares no license_file",
                    ));
                    continue;
                };
                let resolved = resolve_licence_file(pkg, lf, root, metadata_dir);
                let Ok(bytes) = read_bounded(&resolved, MAX_INPUT_BYTES) else {
                    findings.insert(Finding::new(
                        FindingCode::LicClarifyStale,
                        subject,
                        format!("cannot read license_file {lf}"),
                    ));
                    continue;
                };
                let actual = sha256_hex(&bytes);
                if actual != clar.licence_file_sha256 {
                    findings.insert(Finding::new(
                        FindingCode::LicClarifyStale,
                        subject,
                        format!("expected {} got {actual}", clar.licence_file_sha256),
                    ));
                    continue;
                }
                clar.expression.clone()
            }
        };

        match parse_spdx(&expr_str) {
            Ok(ast) => match eval_spdx(&ast, allow) {
                EvalOutcome::Allowed => {}
                EvalOutcome::DeniedLicence => {
                    findings.insert(Finding::new(FindingCode::LicDenied, subject, expr_str));
                }
                EvalOutcome::DeniedException(_) => {
                    findings.insert(Finding::new(FindingCode::LicException, subject, expr_str));
                }
            },
            Err(detail) => {
                findings.insert(Finding::new(FindingCode::LicParse, subject, detail));
            }
        }
    }
}

// ============================================================================
// Asset attribution check (ATTRIBUTION.md vs asset_paths)
// ============================================================================

/// Memoized glob matcher supporting `*`, `?` (within one path segment) and `**` (zero or more
/// path segments). Path separators are `/`.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let p_segs: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let t_segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let pn = p_segs.len();
    let tn = t_segs.len();
    // dp[i][j] = p_segs[i..] matches t_segs[j..]
    let mut dp = vec![vec![false; tn + 1]; pn + 1];
    dp[pn][tn] = true;
    for i in (0..pn).rev() {
        if p_segs[i] == "**" {
            dp[i][tn] = dp[i + 1][tn];
        }
    }
    for i in (0..pn).rev() {
        for j in (0..tn).rev() {
            dp[i][j] = if p_segs[i] == "**" {
                dp[i + 1][j] || dp[i][j + 1]
            } else {
                seg_match(p_segs[i], t_segs[j]) && dp[i + 1][j + 1]
            };
        }
    }
    dp[0][0]
}

fn seg_match(pat: &str, text: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let pn = p.len();
    let tn = t.len();
    let mut dp = vec![vec![false; tn + 1]; pn + 1];
    dp[pn][tn] = true;
    for i in (0..pn).rev() {
        if p[i] == '*' {
            dp[i][tn] = dp[i + 1][tn];
        }
    }
    for i in (0..pn).rev() {
        for j in (0..tn).rev() {
            dp[i][j] = match p[i] {
                '*' => dp[i + 1][j] || dp[i][j + 1],
                '?' => dp[i + 1][j + 1],
                c => c == t[j] && dp[i + 1][j + 1],
            };
        }
    }
    dp[0][0]
}

/// Parses the first column of every Markdown table in `ATTRIBUTION.md` as repo-relative paths
/// or glob patterns.
pub fn parse_attribution_paths(markdown: &str) -> Vec<String> {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 1 < lines.len() {
        if is_table_row(lines[i]) && is_separator_row(lines[i + 1]) {
            i += 2;
            while i < lines.len() && is_table_row(lines[i]) {
                if let Some(first_cell) = first_table_cell(lines[i]) {
                    let cleaned = first_cell.trim().trim_matches('`').trim();
                    if !cleaned.is_empty() && cleaned != "-" {
                        out.push(cleaned.replace('\\', "/"));
                    }
                }
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    out
}

fn is_table_row(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.ends_with('|') && t.len() >= 2
}

fn is_separator_row(line: &str) -> bool {
    if !is_table_row(line) {
        return false;
    }
    let t = line.trim();
    let inner = &t[1..t.len() - 1];
    let mut saw_cell = false;
    for cell in inner.split('|') {
        let c = cell.trim();
        if c.is_empty() {
            return false;
        }
        let stripped = c.trim_start_matches(':').trim_end_matches(':');
        if stripped.is_empty() || !stripped.bytes().all(|b| b == b'-') {
            return false;
        }
        saw_cell = true;
    }
    saw_cell
}

fn first_table_cell(line: &str) -> Option<&str> {
    let t = line.trim();
    let inner = t.strip_prefix('|')?.strip_suffix('|')?;
    inner.split('|').next()
}

fn collect_files(root: &Path) -> Result<Vec<String>, InputError> {
    let mut out = Vec::new();
    let mut visited = 0usize;
    let mut stack: Vec<(PathBuf, String, usize)> = vec![(root.to_path_buf(), String::new(), 0)];
    while let Some((dir, prefix, depth)) = stack.pop() {
        if depth > MAX_WALK_DEPTH {
            return Err(InputError::new(
                dir.display().to_string(),
                format!("directory depth exceeds {MAX_WALK_DEPTH}"),
            ));
        }
        let mut entries = Vec::new();
        for e in fs::read_dir(&dir).map_err(|e| InputError::new(dir.display().to_string(), e.to_string()))? {
            let e = e.map_err(|e| InputError::new(dir.display().to_string(), e.to_string()))?;
            let name = e.file_name().to_string_lossy().into_owned();
            entries.push((name, e.path()));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, path) in entries {
            visited += 1;
            if visited > MAX_WALK_ENTRIES {
                return Err(InputError::new(
                    root.display().to_string(),
                    format!("tree exceeds {MAX_WALK_ENTRIES} entries"),
                ));
            }
            let meta =
                fs::symlink_metadata(&path).map_err(|e| InputError::new(path.display().to_string(), e.to_string()))?;
            if meta.file_type().is_symlink() {
                continue;
            }
            let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            if meta.is_dir() {
                if SKIPPED_DIRS.iter().any(|skip| *skip == name) {
                    continue;
                }
                stack.push((path, rel, depth + 1));
            } else {
                out.push(rel);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn check_attribution(root: &Path, allow: &Allowlist, findings: &mut BTreeSet<Finding>) -> Result<(), InputError> {
    let all_files = collect_files(root)?;
    let attr_path = root.join("ATTRIBUTION.md");
    let attr_rows = if attr_path.is_file() {
        let bytes = read_bounded(&attr_path, MAX_INPUT_BYTES)?;
        let text = String::from_utf8(bytes)
            .map_err(|e| InputError::new(attr_path.display().to_string(), format!("not UTF-8: {e}")))?;
        parse_attribution_paths(&text)
    } else {
        Vec::new()
    };

    // Every file under configured `asset_paths` must match at least one row in ATTRIBUTION.md.
    for file in &all_files {
        if file == "ATTRIBUTION.md" {
            continue;
        }
        if allow.asset_paths.iter().any(|pat| glob_match(pat, file)) {
            let covered = attr_rows.iter().any(|row| glob_match(row, file));
            if !covered {
                findings.insert(Finding::new(FindingCode::AttrMissing, file.clone(), ""));
            }
        }
    }

    // Every row in ATTRIBUTION.md must match at least one existing file under `--root`.
    for row in &attr_rows {
        let matched = all_files.iter().any(|file| glob_match(row, file));
        if !matched {
            findings.insert(Finding::new(FindingCode::AttrOrphan, row.clone(), ""));
        }
    }

    Ok(())
}

// ============================================================================
// Public driver & CLI
// ============================================================================

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, InputError> {
    let f = fs::File::open(path).map_err(|e| InputError::new(path.display().to_string(), e.to_string()))?;
    let mut bytes = Vec::new();
    let cap = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    f.take(cap).read_to_end(&mut bytes).map_err(|e| InputError::new(path.display().to_string(), e.to_string()))?;
    if bytes.len() > limit {
        return Err(InputError::new(path.display().to_string(), format!("file exceeds {limit} bytes")));
    }
    Ok(bytes)
}

/// Runs the full offline licence and asset attribution check (REQ-ECO.CI.LIC-001..005).
///
/// Returns sorted, de-duplicated findings. `Err` is reserved for unreadable or malformed
/// input files (`--metadata`, `--allow`, `--clarify`, `--root`).
pub fn run(
    metadata_path: &Path,
    allow_path: &Path,
    root: &Path,
    clarify_path: Option<&Path>,
) -> Result<Vec<Finding>, InputError> {
    let meta_bytes = read_bounded(metadata_path, MAX_INPUT_BYTES)?;
    let doc = parse_metadata(&meta_bytes)?;

    let allow_bytes = read_bounded(allow_path, MAX_INPUT_BYTES)?;
    let allow_text = String::from_utf8(allow_bytes)
        .map_err(|e| InputError::new(allow_path.display().to_string(), format!("not UTF-8: {e}")))?;
    let allow = Allowlist::parse(&allow_text)?;

    let clarifications = match clarify_path {
        Some(p) => {
            let bytes = read_bounded(p, MAX_INPUT_BYTES)?;
            let text = String::from_utf8(bytes)
                .map_err(|e| InputError::new(p.display().to_string(), format!("not UTF-8: {e}")))?;
            Clarifications::parse(&text)?
        }
        None => Clarifications::default(),
    };

    if !root.is_dir() {
        return Err(InputError::new(root.display().to_string(), "root is not a directory"));
    }

    let metadata_dir = metadata_path.parent().unwrap_or(root);
    let mut findings = BTreeSet::new();
    check_packages(&doc, &allow, &clarifications, root, metadata_dir, &mut findings);
    check_attribution(root, &allow, &mut findings)?;

    Ok(findings.into_iter().collect())
}

/// CLI entry point for `cargo xtask licence-check`.
///
/// Exit codes: `0` pass; `1` findings present; `2` usage or input error.
pub fn cli(args: &[String]) -> ExitCode {
    let (mut metadata, mut allow, mut root, mut clarify) = (None, None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--metadata", Some(v)) if metadata.is_none() => metadata = Some(PathBuf::from(v)),
            ("--allow", Some(v)) if allow.is_none() => allow = Some(PathBuf::from(v)),
            ("--root", Some(v)) if root.is_none() => root = Some(PathBuf::from(v)),
            ("--clarify", Some(v)) if clarify.is_none() => clarify = Some(PathBuf::from(v)),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(metadata), Some(allow), Some(root)) = (metadata, allow, root) else {
        eprintln!("licence-check: --metadata, --allow and --root are required\n{USAGE}");
        return ExitCode::from(2);
    };
    match run(&metadata, &allow, &root, clarify.as_deref()) {
        Ok(findings) if findings.is_empty() => {
            println!("licence-check: ok");
            ExitCode::SUCCESS
        }
        Ok(findings) => {
            for f in &findings {
                println!("{f}");
            }
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
