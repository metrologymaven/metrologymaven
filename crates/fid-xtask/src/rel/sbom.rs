//! `cargo xtask rel sbom`: one SPDX 2.3 JSON document per planned artefact (spec node
//! `ECO.REL.SIGN.SBOM`).
//!
//! Inputs:
//!
//! - the package plan (JSON). The tool reads `version` (the release version), `commit_time`
//!   (`YYYY-MM-DDThh:mm:ssZ`, overridable with `--commit-time`) and, per entry of `artefacts`,
//!   `file_name`, `target` (a Rust target triple), `contents` (binary target names of workspace
//!   members), and optionally `package` (the plan's package ID), `features` and `sha256`. Other
//!   fields are ignored;
//! - the Cargo workspace: `cargo metadata --format-version 1 --locked --offline
//!   --filter-platform <target> [--features ...]` is run in it once per (target, features), and
//!   its `Cargo.lock` gives the crate checksums. Nothing touches the network;
//! - `native-components.toml`: `[[component]]` tables with `name`, `version`, `source`,
//!   `license`, `sha256` (64 lowercase hex) and `artefacts` (plan `package` IDs or artefact file
//!   names that bundle the component).
//!
//! For each artefact the crates are those reachable from the workspace members that provide its
//! `contents`, over normal and build dependency edges of the graph resolved for its target and
//! features (dev-only edges are not followed). A crate reached from the members through normal
//! edges only, and not a proc-macro crate, is compiled into the artefact (`CONTAINS`); a build
//! dependency, a proc-macro crate and everything reached only through them run at build time and
//! are recorded as `<crate> BUILD_DEPENDENCY_OF SPDXRef-Artefact`. The artefact's own SHA-256
//! comes from the file in `--artefacts <dir>` when given, else from the plan entry's `sha256`.
//!
//! A legacy Cargo licence with `/` separators (`MIT/Apache-2.0`) is read as `OR` (Cargo's
//! documented meaning): `licenseDeclared` holds the normalised expression and the package
//! `comment` keeps the original string. Path crates (no Cargo source) get the package URL
//! qualifier `repository_url=<this repository>`.
//!
//! Each document `<artefact>.spdx.json` has the artefact as root package `SPDXRef-Artefact`
//! (described by the document), one package per crate and per assigned native component, and is
//! written as JSON with sorted keys and no insignificant
//! whitespace (RFC 8785 through `fid-canon`). The namespace is a UUIDv5 of the release version
//! and the artefact name; the creation time is the commit time. Output is a pure function of the
//! plan, the lockfile, the resolved graph and the native-component file (D0). The document
//! rules are listed in `fixtures/rel/sbom/make_fixtures.py`, the independent reference.

use crate::rel::sums::{hash_file, is_valid_name};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_uuid::{uuid_v5, Uuid};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::Command;
use toml::{Table, Value};

/// Repository URL; the prefix of every `documentNamespace`.
pub const REPO: &str = "https://github.com/metrologymaven/metrologymaven";
/// RFC 9562 name space ID for URLs (the UUIDv5 namespace of `documentNamespace`).
pub const NAMESPACE_URL: Uuid = Uuid::from_u128(0x6ba7_b811_9dad_11d1_80b4_00c0_4fd4_30c8);
/// Cargo source IDs of crates.io, whose crates get the crates.io download URL.
pub const CRATES_IO_SOURCES: [&str; 2] =
    ["registry+https://github.com/rust-lang/crates.io-index", "sparse+https://index.crates.io/"];
/// Suffix of every document: `<artefact>.spdx.json`, the name family `rel sums` leaves out
/// as kind `sbom` (`crate::rel::sums::unsummed_kind`).
pub const SBOM_SUFFIX: &str = ".spdx.json";
/// SPDX value for an unknown field.
pub const NOASSERTION: &str = "NOASSERTION";
/// SPDXID of the root (artefact) package.
pub const ROOT_ID: &str = "SPDXRef-Artefact";
/// Largest plan, native-component file or lockfile read, in bytes.
pub const MAX_INPUT_BYTES: u64 = 16 << 20;
/// Largest `cargo metadata` output accepted, in bytes.
pub const MAX_METADATA_BYTES: usize = 64 << 20;
/// Deepest parenthesis nesting accepted in a licence expression.
pub const MAX_LICENCE_DEPTH: usize = 32;

/// A failure of `rel sbom`. [`fmt::Display`] gives the line the tool prints.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SbomError {
    /// `SBOM-LICENCE <name>@<version>: <reason>`: a crate or native component whose licence
    /// expression is missing or not a valid SPDX licence expression.
    Licence { component: String, reason: String },
    /// `SBOM-NATIVE-DIGEST <name>@<version>`: a native component without a valid SHA-256.
    NativeDigest(String),
    /// `SBOM-PLAN-PARSE: <reason>`: the package plan (or `--commit-time`) is not usable.
    PlanParse(String),
    /// `SBOM-NATIVE-PARSE: <reason>`: the native-component file is not usable.
    NativeParse(String),
    /// `SBOM-METADATA: <reason>`: `cargo metadata` failed or the lockfile is unreadable.
    Metadata(String),
    /// `SBOM-IO: <reason>`: a file could not be read or written.
    Io(String),
}

impl SbomError {
    /// Stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            SbomError::Licence { .. } => "SBOM-LICENCE",
            SbomError::NativeDigest(_) => "SBOM-NATIVE-DIGEST",
            SbomError::PlanParse(_) => "SBOM-PLAN-PARSE",
            SbomError::NativeParse(_) => "SBOM-NATIVE-PARSE",
            SbomError::Metadata(_) => "SBOM-METADATA",
            SbomError::Io(_) => "SBOM-IO",
        }
    }

    /// Process exit code: 1 for a finding about the content (licence, digest, I/O), 2 for an
    /// unusable input.
    pub fn exit_code(&self) -> u8 {
        match self {
            SbomError::Licence { .. } | SbomError::NativeDigest(_) | SbomError::Io(_) => 1,
            SbomError::PlanParse(_) | SbomError::NativeParse(_) | SbomError::Metadata(_) => 2,
        }
    }
}

impl fmt::Display for SbomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SbomError::Licence { component, reason } => write!(f, "{} {component}: {reason}", self.code()),
            SbomError::NativeDigest(c) => write!(f, "{} {c}", self.code()),
            SbomError::PlanParse(m) | SbomError::NativeParse(m) | SbomError::Metadata(m) | SbomError::Io(m) => {
                write!(f, "{}: {m}", self.code())
            }
        }
    }
}

impl std::error::Error for SbomError {}

// ------------------------------------------------------------------------------------------
// Licence expressions
// ------------------------------------------------------------------------------------------

/// Whether `s` is an SPDX `idstring`: one or more ASCII letters, digits, `-` or `.`.
fn is_idstring(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}

/// Whether `word` is a licence term: `<id>`, `<id>+`, `LicenseRef-<id>` or
/// `DocumentRef-<id>:LicenseRef-<id>`.
fn is_licence_term(word: &str) -> bool {
    if let Some(rest) = word.strip_prefix("DocumentRef-") {
        return match rest.split_once(':') {
            Some((doc, lic)) => is_idstring(doc) && lic.strip_prefix("LicenseRef-").is_some_and(is_idstring),
            None => false,
        };
    }
    if let Some(rest) = word.strip_prefix("LicenseRef-") {
        return is_idstring(rest);
    }
    is_idstring(word.strip_suffix('+').unwrap_or(word))
}

const OPERATORS: [&str; 3] = ["AND", "OR", "WITH"];

struct ExprParser<'a> {
    tokens: &'a [String],
    pos: usize,
    depth: usize,
}

impl<'a> ExprParser<'a> {
    fn peek(&self) -> Option<&'a str> {
        self.tokens.get(self.pos).map(String::as_str)
    }

    fn next(&mut self) -> Option<&'a str> {
        let t = self.tokens.get(self.pos).map(String::as_str);
        self.pos += 1;
        t
    }

    fn or_expr(&mut self) -> Result<(), String> {
        self.and_expr()?;
        while self.peek() == Some("OR") {
            self.pos += 1;
            self.and_expr()?;
        }
        Ok(())
    }

    fn and_expr(&mut self) -> Result<(), String> {
        self.term()?;
        while self.peek() == Some("AND") {
            self.pos += 1;
            self.term()?;
        }
        Ok(())
    }

    fn term(&mut self) -> Result<(), String> {
        let before = self.tokens.get(self.pos.wrapping_sub(1)).cloned();
        match self.next() {
            None => Err(match before {
                Some(op) => format!("expression ends after {op}"),
                None => "empty licence expression".to_string(),
            }),
            Some("(") => {
                self.depth += 1;
                if self.depth > MAX_LICENCE_DEPTH {
                    return Err(format!("nesting deeper than {MAX_LICENCE_DEPTH}"));
                }
                self.or_expr()?;
                if self.next() != Some(")") {
                    return Err("unbalanced parentheses".to_string());
                }
                self.depth -= 1;
                Ok(())
            }
            Some(t) if t == ")" || OPERATORS.contains(&t) => Err(format!("{t} where a licence was expected")),
            Some(t) if !is_licence_term(t) => Err(format!("{t:?} is not a licence identifier")),
            Some(_) => {
                if self.peek() == Some("WITH") {
                    self.pos += 1;
                    match self.next() {
                        Some(e) if is_idstring(e) && !OPERATORS.contains(&e) => {}
                        Some(e) => return Err(format!("{e:?} is not a licence exception identifier")),
                        None => return Err("expression ends after WITH".to_string()),
                    }
                }
                Ok(())
            }
        }
    }
}

/// Checks that `expr` is an SPDX licence expression (SPDX 2.3 Annex D grammar): licence terms
/// joined by `AND` and `OR` (case-sensitive), `<term> WITH <exception>`, and parentheses.
/// Identifiers are checked for form, not against the SPDX licence list (the allowlist is
/// `ECO.CI.LIC`'s job). `Err` gives the reason.
pub fn check_licence_expression(expr: &str) -> Result<(), String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut word = String::new();
    for c in expr.chars() {
        if c == '(' || c == ')' || c.is_whitespace() {
            if !word.is_empty() {
                tokens.push(std::mem::take(&mut word));
            }
            if !c.is_whitespace() {
                tokens.push(c.to_string());
            }
        } else {
            word.push(c);
        }
    }
    if !word.is_empty() {
        tokens.push(word);
    }
    let mut p = ExprParser { tokens: &tokens, pos: 0, depth: 0 };
    p.or_expr()?;
    match p.peek() {
        None => Ok(()),
        Some(t) => Err(format!("unexpected {t:?}")),
    }
}

// ------------------------------------------------------------------------------------------
// Small encoders
// ------------------------------------------------------------------------------------------

/// Percent-encodes every byte outside `[A-Za-z0-9._~-]` (uppercase hex), for package URLs.
pub fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~' | b'-') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `s` with every character outside `[A-Za-z0-9.-]` replaced by `-` (an SPDXID fragment).
pub fn spdx_id_part(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '-' }).collect()
}

/// SPDX `downloadLocation` of a crate: the crates.io download URL for a crates.io source,
/// `git+<url>@<commit>` for a git source, `NOASSERTION` for a path crate or another registry.
pub fn download_location(source: Option<&str>, name: &str, version: &str) -> String {
    match source {
        Some(s) if CRATES_IO_SOURCES.contains(&s) => {
            format!("https://crates.io/api/v1/crates/{name}/{version}/download")
        }
        Some(s) if s.starts_with("git+") => {
            let (url_query, commit) = s.split_once('#').unwrap_or((s, ""));
            let url = url_query.split_once('?').map_or(url_query, |(u, _)| u);
            if commit.is_empty() {
                url.to_string()
            } else {
                format!("{url}@{commit}")
            }
        }
        _ => NOASSERTION.to_string(),
    }
}

/// Whether `s` is a UTC time `YYYY-MM-DDThh:mm:ssZ` naming a real calendar date.
pub fn is_valid_time(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 20 {
        return false;
    }
    let shape = b.iter().enumerate().all(|(i, &c)| match i {
        4 | 7 => c == b'-',
        10 => c == b'T',
        13 | 16 => c == b':',
        19 => c == b'Z',
        _ => c.is_ascii_digit(),
    });
    if !shape {
        return false;
    }
    let num = |r: std::ops::Range<usize>| s.get(r).and_then(|x| x.parse::<u32>().ok()).unwrap_or(u32::MAX);
    let (y, mo, d, h, mi, se) = (num(0..4), num(5..7), num(8..10), num(11..13), num(14..16), num(17..19));
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match mo {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&d) && h < 24 && mi < 60 && se < 60
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

// ------------------------------------------------------------------------------------------
// Inputs
// ------------------------------------------------------------------------------------------

/// One planned artefact (the fields this tool reads).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanArtefact {
    pub file_name: String,
    pub package: Option<String>,
    pub target: String,
    pub contents: Vec<String>,
    pub features: Vec<String>,
    pub sha256: Option<String>,
}

/// The package plan fields this tool reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub version: String,
    pub commit_time: Option<String>,
    pub artefacts: Vec<PlanArtefact>,
}

fn read_bounded(path: &Path, what: fn(String) -> SbomError) -> Result<Vec<u8>, SbomError> {
    let io = |e: std::io::Error| SbomError::Io(format!("{}: {e}", path.display()));
    let file = File::open(path).map_err(io)?;
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_BYTES + 1).read_to_end(&mut bytes).map_err(io)?;
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err(what(format!("{} is larger than {MAX_INPUT_BYTES} bytes", path.display())));
    }
    Ok(bytes)
}

fn str_list(v: Option<&CValue>, at: &str) -> Result<Vec<String>, SbomError> {
    match v {
        None => Ok(Vec::new()),
        Some(CValue::Arr(a)) => a
            .iter()
            .map(|x| match x {
                CValue::Str(s) => Ok(s.clone()),
                _ => Err(SbomError::PlanParse(format!("{at} must hold strings"))),
            })
            .collect(),
        Some(_) => Err(SbomError::PlanParse(format!("{at} must be an array of strings"))),
    }
}

fn opt_str(v: Option<&CValue>, at: &str) -> Result<Option<String>, SbomError> {
    match v {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.clone())),
        Some(_) => Err(SbomError::PlanParse(format!("{at} must be a string"))),
    }
}

fn is_target(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

fn is_feature(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'+' | b'.' | b'/'))
}

/// Parses the package plan.
pub fn parse_plan(bytes: &[u8]) -> Result<Plan, SbomError> {
    let value = parse_with_limits(bytes, bytes.len(), 16).map_err(|e| SbomError::PlanParse(format!("plan: {e}")))?;
    let CValue::Obj(top) = value else {
        return Err(SbomError::PlanParse("plan: not an object".into()));
    };
    let version = match top.get("version") {
        Some(CValue::Str(s)) if !s.is_empty() && !s.contains(['/', ' ']) && !s.chars().any(char::is_control) => {
            s.clone()
        }
        _ => return Err(SbomError::PlanParse("plan: `version` must be a non-empty string without / or spaces".into())),
    };
    let commit_time = opt_str(top.get("commit_time"), "plan: `commit_time`")?;
    let Some(CValue::Arr(arts)) = top.get("artefacts") else {
        return Err(SbomError::PlanParse("plan: `artefacts` must be an array".into()));
    };
    let mut seen = BTreeSet::new();
    let mut artefacts = Vec::with_capacity(arts.len());
    for (i, a) in arts.iter().enumerate() {
        let at = format!("plan: artefacts[{i}]");
        let CValue::Obj(a) = a else {
            return Err(SbomError::PlanParse(format!("{at} is not an object")));
        };
        let file_name = match a.get("file_name") {
            Some(CValue::Str(s)) if is_valid_name(s) => s.clone(),
            _ => return Err(SbomError::PlanParse(format!("{at}.file_name must be a file name of [A-Za-z0-9._+-]"))),
        };
        if !seen.insert(file_name.clone()) {
            return Err(SbomError::PlanParse(format!("plan: artefact {file_name} is listed twice")));
        }
        let at = format!("plan: artefact {file_name}");
        let target = match a.get("target") {
            Some(CValue::Str(s)) if is_target(s) => s.clone(),
            _ => return Err(SbomError::PlanParse(format!("{at}: `target` must be a target triple"))),
        };
        let contents = str_list(a.get("contents"), &format!("{at}: `contents`"))?;
        if contents.is_empty() {
            return Err(SbomError::PlanParse(format!("{at}: `contents` names no binary")));
        }
        let features = str_list(a.get("features"), &format!("{at}: `features`"))?;
        if let Some(f) = features.iter().find(|f| !is_feature(f)) {
            return Err(SbomError::PlanParse(format!("{at}: bad feature {f:?}")));
        }
        let sha256 = opt_str(a.get("sha256"), &format!("{at}: `sha256`"))?;
        if sha256.as_deref().is_some_and(|s| !is_hex64(s)) {
            return Err(SbomError::PlanParse(format!("{at}: `sha256` must be 64 lowercase hex digits")));
        }
        let package = opt_str(a.get("package"), &format!("{at}: `package`"))?;
        artefacts.push(PlanArtefact { file_name, package, target, contents, features, sha256 });
    }
    Ok(Plan { version, commit_time, artefacts })
}

/// One entry of `native-components.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Native {
    pub name: String,
    pub version: String,
    pub source: String,
    pub license: String,
    pub sha256: String,
    pub artefacts: Vec<String>,
}

const NATIVE_KEYS: [&str; 6] = ["name", "version", "source", "license", "sha256", "artefacts"];

/// Parses `native-components.toml`. A malformed file is one `SBOM-NATIVE-PARSE`; otherwise
/// every component with a bad digest (`SBOM-NATIVE-DIGEST`) or licence (`SBOM-LICENCE`) is
/// reported.
pub fn parse_native(text: &str) -> Result<Vec<Native>, Vec<SbomError>> {
    let perr = |m: String| vec![SbomError::NativeParse(m)];
    let table: Table = text
        .parse::<Table>()
        .map_err(|e| perr(format!("native components: {}", e.to_string().trim_end().replace('\n', " "))))?;
    if let Some(k) = table.keys().find(|k| k.as_str() != "component") {
        return Err(perr(format!("native components: unknown key {k:?}")));
    }
    let items = match table.get("component") {
        None => return Ok(Vec::new()),
        Some(Value::Array(a)) => a,
        Some(_) => return Err(perr("native components: `component` must be an array of tables".into())),
    };
    let mut out: Vec<Native> = Vec::with_capacity(items.len());
    let mut findings = Vec::new();
    let mut seen = BTreeSet::new();
    for (i, item) in items.iter().enumerate() {
        let at = format!("native components: component[{i}]");
        let Value::Table(t) = item else {
            return Err(perr(format!("{at} is not a table")));
        };
        if let Some(k) = t.keys().find(|k| !NATIVE_KEYS.contains(&k.as_str())) {
            return Err(perr(format!("{at}: unknown key {k:?}")));
        }
        let text_field = |k: &str| match t.get(k) {
            Some(Value::String(s)) if !s.trim().is_empty() && !s.chars().any(char::is_control) => Ok(s.clone()),
            _ => Err(perr(format!("{at}: `{k}` must be a non-empty single-line string"))),
        };
        let (name, version, source) = (text_field("name")?, text_field("version")?, text_field("source")?);
        let artefacts = match t.get("artefacts") {
            Some(Value::Array(a)) if !a.is_empty() => a
                .iter()
                .map(|x| match x {
                    Value::String(s) if !s.is_empty() => Ok(s.clone()),
                    _ => Err(perr(format!("{at}: `artefacts` must hold non-empty strings"))),
                })
                .collect::<Result<Vec<_>, _>>()?,
            _ => return Err(perr(format!("{at}: `artefacts` must be a non-empty array"))),
        };
        let id = format!("{name}@{version}");
        if !seen.insert(id.clone()) {
            return Err(perr(format!("native components: {id} is listed twice")));
        }
        let sha256 = match t.get("sha256") {
            Some(Value::String(s)) if is_hex64(s) => s.clone(),
            _ => {
                findings.push(SbomError::NativeDigest(id.clone()));
                String::new()
            }
        };
        let license = match t.get("license") {
            Some(Value::String(s)) => s.clone(),
            None => String::new(),
            Some(_) => return Err(perr(format!("{at}: `license` must be a string"))),
        };
        if let Err(reason) = licence_check(&license) {
            findings.push(SbomError::Licence { component: id, reason });
        }
        out.push(Native { name, version, source, license, sha256, artefacts });
    }
    if findings.is_empty() {
        Ok(out)
    } else {
        Err(findings)
    }
}

/// Cargo's legacy `/` licence separator read as `OR`: `MIT/Apache-2.0` becomes
/// `MIT OR Apache-2.0`, and a part holding spaces is parenthesised. Returns the expression and,
/// when it was rewritten, the original string.
pub fn normalise_cargo_licence(licence: &str) -> (String, Option<String>) {
    if !licence.contains('/') {
        return (licence.to_string(), None);
    }
    let parts: Vec<String> = licence
        .split('/')
        .map(str::trim)
        .map(|p| if p.contains(char::is_whitespace) { format!("({p})") } else { p.to_string() })
        .collect();
    (parts.join(" OR "), Some(licence.to_string()))
}

fn licence_check(licence: &str) -> Result<(), String> {
    if licence.trim().is_empty() {
        return Err("no licence expression".to_string());
    }
    check_licence_expression(licence)
}

/// `(name, version, source)` to checksum, from `Cargo.lock` (format versions 2 to 4: the
/// per-package `checksum` field).
pub fn lock_checksums(text: &str) -> Result<BTreeMap<(String, String, String), String>, SbomError> {
    let table: Table = text
        .parse::<Table>()
        .map_err(|e| SbomError::Metadata(format!("Cargo.lock: {}", e.to_string().trim_end().replace('\n', " "))))?;
    let mut out = BTreeMap::new();
    let Some(Value::Array(pkgs)) = table.get("package") else {
        return Ok(out);
    };
    for p in pkgs {
        let Value::Table(p) = p else {
            return Err(SbomError::Metadata("Cargo.lock: a [[package]] is not a table".into()));
        };
        let get = |k: &str| match p.get(k) {
            Some(Value::String(s)) => Some(s.clone()),
            _ => None,
        };
        if let (Some(n), Some(v), Some(s), Some(c)) = (get("name"), get("version"), get("source"), get("checksum")) {
            out.insert((n, v, s), c);
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------
// The resolved crate graph
// ------------------------------------------------------------------------------------------

/// One package of the `cargo metadata` output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaPackage {
    pub name: String,
    pub version: String,
    pub source: Option<String>,
    pub license: Option<String>,
    /// Names of its `bin` targets.
    pub bins: Vec<String>,
    /// Whether it has a `proc-macro` target (it runs in the compiler, not in the artefact).
    pub proc_macro: bool,
    pub member: bool,
}

/// A dependency edge that is not dev-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    /// Package ID depended on.
    pub to: String,
    /// A normal dependency (kind null) for this target.
    pub normal: bool,
    /// A build dependency (kind `build`) for this target.
    pub build: bool,
}

/// The graph resolved for one target and feature set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resolved {
    /// Package ID to package.
    pub packages: BTreeMap<String, MetaPackage>,
    /// Package ID to its normal and build edges, sorted by target ID (dev-only edges dropped).
    pub edges: BTreeMap<String, Vec<Edge>>,
}

/// How a component relates to the artefact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    /// Compiled into the artefact: `SPDXRef-Artefact CONTAINS <component>`.
    Contained,
    /// Used only to build it (build dependency, proc-macro, or reached only through those):
    /// `<component> BUILD_DEPENDENCY_OF SPDXRef-Artefact`.
    Build,
}

fn md(m: &str) -> SbomError {
    SbomError::Metadata(m.to_string())
}

fn obj<'a>(v: Option<&'a CValue>, what: &str) -> Result<&'a BTreeMap<String, CValue>, SbomError> {
    match v {
        Some(CValue::Obj(o)) => Ok(o),
        _ => Err(md(&format!("cargo metadata: {what} is not an object"))),
    }
}

fn arr<'a>(v: Option<&'a CValue>, what: &str) -> Result<&'a [CValue], SbomError> {
    match v {
        Some(CValue::Arr(a)) => Ok(a),
        _ => Err(md(&format!("cargo metadata: {what} is not an array"))),
    }
}

fn string(v: Option<&CValue>, what: &str) -> Result<String, SbomError> {
    match v {
        Some(CValue::Str(s)) => Ok(s.clone()),
        _ => Err(md(&format!("cargo metadata: {what} is not a string"))),
    }
}

fn nullable(v: Option<&CValue>, what: &str) -> Result<Option<String>, SbomError> {
    match v {
        None | Some(CValue::Null) => Ok(None),
        Some(CValue::Str(s)) => Ok(Some(s.clone())),
        _ => Err(md(&format!("cargo metadata: {what} is not a string or null"))),
    }
}

/// Builds the graph from `cargo metadata --format-version 1` output (with `resolve`).
pub fn parse_metadata(json: &[u8]) -> Result<Resolved, SbomError> {
    let root = parse_with_limits(json, MAX_METADATA_BYTES, 64).map_err(|e| md(&format!("cargo metadata: {e}")))?;
    let root = obj(Some(&root), "the output")?;
    let members: BTreeSet<String> = arr(root.get("workspace_members"), "workspace_members")?
        .iter()
        .map(|m| string(Some(m), "a workspace member"))
        .collect::<Result<_, _>>()?;
    let mut g = Resolved::default();
    for p in arr(root.get("packages"), "packages")? {
        let p = obj(Some(p), "a package")?;
        let id = string(p.get("id"), "package id")?;
        let mut bins = Vec::new();
        let mut proc_macro = false;
        for t in arr(p.get("targets"), "targets")? {
            let t = obj(Some(t), "a target")?;
            let kinds = arr(t.get("kind"), "target kind")?;
            let has = |want: &str| kinds.iter().any(|k| matches!(k, CValue::Str(s) if s == want));
            if has("bin") {
                bins.push(string(t.get("name"), "target name")?);
            }
            proc_macro |= has("proc-macro");
        }
        let pkg = MetaPackage {
            name: string(p.get("name"), "package name")?,
            version: string(p.get("version"), "package version")?,
            source: nullable(p.get("source"), "package source")?,
            license: nullable(p.get("license"), "package license")?,
            bins,
            proc_macro,
            member: members.contains(&id),
        };
        g.packages.insert(id, pkg);
    }
    let resolve = obj(root.get("resolve"), "resolve")?;
    for n in arr(resolve.get("nodes"), "resolve.nodes")? {
        let n = obj(Some(n), "a resolve node")?;
        let id = string(n.get("id"), "node id")?;
        let mut to: Vec<Edge> = Vec::new();
        for d in arr(n.get("deps"), "node deps")? {
            let d = obj(Some(d), "a node dep")?;
            let kinds = arr(d.get("dep_kinds"), "dep_kinds")?;
            // Kind null is a normal dependency, "build" a build dependency; dev-only edges are
            // neither compiled into nor used to build the artefact. No kinds (old Cargo): normal.
            let (mut normal, mut build) = (kinds.is_empty(), false);
            for k in kinds {
                match obj(Some(k), "a dep kind")?.get("kind") {
                    None | Some(CValue::Null) => normal = true,
                    Some(CValue::Str(s)) if s == "build" => build = true,
                    Some(CValue::Str(s)) if s == "dev" => {}
                    Some(other) => return Err(md(&format!("cargo metadata: unknown dep kind {other:?}"))),
                }
            }
            if normal || build {
                to.push(Edge { to: string(d.get("pkg"), "dep pkg")?, normal, build });
            }
        }
        to.sort_by(|a, b| a.to.cmp(&b.to));
        g.edges.insert(id, to);
    }
    for (from, tos) in &g.edges {
        let mut ids = std::iter::once(from).chain(tos.iter().map(|e| &e.to));
        if let Some(bad) = ids.find(|id| !g.packages.contains_key(*id)) {
            return Err(md(&format!("cargo metadata: unknown package {bad}")));
        }
    }
    Ok(g)
}

/// Runs `cargo metadata --format-version 1 --locked --offline --filter-platform <target>` in
/// `workspace` (so the workspace's `.cargo/config.toml` applies), with the features if any.
pub fn run_metadata(workspace: &Path, target: &str, features: &[String]) -> Result<Resolved, SbomError> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(workspace).args([
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--offline",
        "--filter-platform",
        target,
        "--manifest-path",
        "Cargo.toml",
    ]);
    if !features.is_empty() {
        cmd.arg("--features").arg(features.join(","));
    }
    let out = cmd.output().map_err(|e| md(&format!("cannot run cargo in {}: {e}", workspace.display())))?;
    if !out.status.success() {
        return Err(md(&format!(
            "cargo metadata failed for {target}: {}",
            String::from_utf8_lossy(&out.stderr).trim().replace('\n', " | ")
        )));
    }
    if out.stdout.len() > MAX_METADATA_BYTES {
        return Err(md(&format!("cargo metadata output is larger than {MAX_METADATA_BYTES} bytes")));
    }
    parse_metadata(&out.stdout)
}

/// The packages of an artefact: everything reachable from the members whose binary targets are
/// named in `contents`, each with its [`Scope`]. A package reached from those members through
/// normal edges, without passing a proc-macro crate, is [`Scope::Contained`]; everything else
/// reached (build dependencies, proc-macro crates, and their own dependencies) is
/// [`Scope::Build`]. Sorted by package ID.
pub fn artefact_crates<'g>(
    g: &'g Resolved,
    artefact: &PlanArtefact,
) -> Result<Vec<(&'g MetaPackage, Scope)>, SbomError> {
    let mut queue: VecDeque<&str> = VecDeque::new();
    for bin in &artefact.contents {
        let owners: Vec<&'g str> = g
            .packages
            .iter()
            .filter(|(_, p)| p.member && p.bins.iter().any(|b| b == bin))
            .map(|(id, _)| id.as_str())
            .collect();
        match owners.as_slice() {
            [id] => queue.push_back(*id),
            [] => {
                return Err(SbomError::PlanParse(format!(
                    "plan: artefact {}: {bin:?} is not a binary of a workspace member",
                    artefact.file_name
                )))
            }
            _ => {
                return Err(SbomError::PlanParse(format!(
                    "plan: artefact {}: binary {bin:?} is provided by more than one member",
                    artefact.file_name
                )))
            }
        }
    }
    let roots: BTreeSet<&str> = queue.iter().copied().collect();
    // Pass 1: compiled into the artefact.
    let mut contained: BTreeSet<&str> = BTreeSet::new();
    let mut build_seeds: VecDeque<&str> = VecDeque::new();
    while let Some(id) = queue.pop_front() {
        if contained.contains(id) {
            continue;
        }
        if !roots.contains(id) && g.packages.get(id).is_some_and(|p| p.proc_macro) {
            build_seeds.push_back(id);
            continue;
        }
        contained.insert(id);
        for e in g.edges.get(id).map(Vec::as_slice).unwrap_or_default() {
            if e.normal {
                queue.push_back(&e.to);
            }
            if e.build {
                build_seeds.push_back(&e.to);
            }
        }
    }
    // Pass 2: used to build it; everything reached from a build-time seed.
    let mut build: BTreeSet<&str> = BTreeSet::new();
    while let Some(id) = build_seeds.pop_front() {
        if contained.contains(id) || !build.insert(id) {
            continue;
        }
        for e in g.edges.get(id).map(Vec::as_slice).unwrap_or_default() {
            build_seeds.push_back(&e.to);
        }
    }
    let mut out: Vec<(&'g str, Scope)> = contained.into_iter().map(|id| (id, Scope::Contained)).collect();
    out.extend(build.into_iter().map(|id| (id, Scope::Build)));
    out.sort();
    Ok(out.into_iter().filter_map(|(id, s)| g.packages.get(id).map(|p| (p, s))).collect())
}

// ------------------------------------------------------------------------------------------
// The document
// ------------------------------------------------------------------------------------------

/// One component package of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    /// `SPDXRef-Crate-` or `SPDXRef-Native-`.
    pub id_prefix: &'static str,
    pub name: String,
    pub version: String,
    pub licence: String,
    pub purl: String,
    pub download: String,
    /// SHA-256, 64 lowercase hex, when known.
    pub checksum: Option<String>,
    pub scope: Scope,
    /// SPDX package `comment` (the original Cargo licence string when it was normalised).
    pub comment: Option<String>,
}

impl Component {
    /// A crate from the resolved graph, with its (normalised) licence expression, lockfile
    /// checksum if there is one, scope and the original licence string if it was normalised.
    pub fn from_crate(
        p: &MetaPackage,
        licence: &str,
        checksum: Option<String>,
        scope: Scope,
        original_licence: Option<&str>,
    ) -> Component {
        let mut purl = format!("pkg:cargo/{}@{}", pct_encode(&p.name), pct_encode(&p.version));
        if p.source.is_none() {
            purl.push_str("?repository_url=");
            purl.push_str(REPO);
        }
        Component {
            id_prefix: "SPDXRef-Crate-",
            name: p.name.clone(),
            version: p.version.clone(),
            licence: licence.to_string(),
            purl,
            download: download_location(p.source.as_deref(), &p.name, &p.version),
            checksum,
            scope,
            comment: original_licence
                .map(|o| format!("Cargo licence field {o:?}: the legacy `/` separator is read as OR")),
        }
    }

    /// A native component.
    pub fn from_native(n: &Native) -> Component {
        Component {
            id_prefix: "SPDXRef-Native-",
            name: n.name.clone(),
            version: n.version.clone(),
            licence: n.license.clone(),
            purl: format!("pkg:generic/{}@{}", pct_encode(&n.name), pct_encode(&n.version)),
            download: n.source.clone(),
            checksum: Some(n.sha256.clone()),
            scope: Scope::Contained,
            comment: None,
        }
    }

    fn sort_key(&self) -> (&str, &str, &str, &str) {
        (&self.name, &self.version, &self.purl, &self.download)
    }
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn object<const N: usize>(fields: [(&str, CValue); N]) -> CValue {
    CValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn sha256_list(hex: &str) -> CValue {
    CValue::Arr(vec![object([("algorithm", s("SHA256")), ("checksumValue", s(hex))])])
}

/// The `documentNamespace` of an artefact's document.
pub fn document_namespace(release: &str, artefact: &str) -> String {
    let u = uuid_v5(&NAMESPACE_URL, format!("{REPO}/spdx/{release}/{artefact}").as_bytes());
    format!("{REPO}/spdx/{artefact}-{u}")
}

/// The bytes of one SPDX document. `components` may come in any order.
pub fn document(
    artefact: &str,
    release: &str,
    created: &str,
    artefact_sha256: &str,
    mut components: Vec<Component>,
) -> Result<Vec<u8>, SbomError> {
    components.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    components.dedup();
    let root = object([
        ("SPDXID", s(ROOT_ID)),
        ("name", s(artefact)),
        ("versionInfo", s(release)),
        ("downloadLocation", s(NOASSERTION)),
        ("filesAnalyzed", CValue::Bool(false)),
        ("licenseConcluded", s(NOASSERTION)),
        ("licenseDeclared", s(NOASSERTION)),
        ("copyrightText", s(NOASSERTION)),
        ("checksums", sha256_list(artefact_sha256)),
    ]);
    let mut packages = vec![root];
    let mut relationships = vec![object([
        ("spdxElementId", s("SPDXRef-DOCUMENT")),
        ("relationshipType", s("DESCRIBES")),
        ("relatedSpdxElement", s(ROOT_ID)),
    ])];
    let mut used: BTreeSet<String> = BTreeSet::new();
    for c in &components {
        let base = format!("{}{}-{}", c.id_prefix, spdx_id_part(&c.name), spdx_id_part(&c.version));
        let mut id = base.clone();
        let mut n = 1u32;
        while used.contains(&id) {
            n += 1;
            id = format!("{base}-{n}");
        }
        used.insert(id.clone());
        let mut p: BTreeMap<String, CValue> = BTreeMap::new();
        p.insert("SPDXID".into(), s(&id));
        p.insert("name".into(), s(&c.name));
        p.insert("versionInfo".into(), s(&c.version));
        p.insert("downloadLocation".into(), s(&c.download));
        p.insert("filesAnalyzed".into(), CValue::Bool(false));
        p.insert("licenseConcluded".into(), s(NOASSERTION));
        p.insert("licenseDeclared".into(), s(&c.licence));
        p.insert("copyrightText".into(), s(NOASSERTION));
        p.insert(
            "externalRefs".into(),
            CValue::Arr(vec![object([
                ("referenceCategory", s("PACKAGE-MANAGER")),
                ("referenceType", s("purl")),
                ("referenceLocator", s(&c.purl)),
            ])]),
        );
        if let Some(sum) = &c.checksum {
            p.insert("checksums".into(), sha256_list(sum));
        }
        if let Some(comment) = &c.comment {
            p.insert("comment".into(), s(comment));
        }
        packages.push(CValue::Obj(p));
        relationships.push(match c.scope {
            Scope::Contained => object([
                ("spdxElementId", s(ROOT_ID)),
                ("relationshipType", s("CONTAINS")),
                ("relatedSpdxElement", s(&id)),
            ]),
            Scope::Build => object([
                ("spdxElementId", s(&id)),
                ("relationshipType", s("BUILD_DEPENDENCY_OF")),
                ("relatedSpdxElement", s(ROOT_ID)),
            ]),
        });
    }
    let doc = object([
        ("spdxVersion", s("SPDX-2.3")),
        ("dataLicense", s("CC0-1.0")),
        ("SPDXID", s("SPDXRef-DOCUMENT")),
        ("name", s(artefact)),
        ("documentNamespace", s(&document_namespace(release, artefact))),
        (
            "creationInfo",
            object([
                ("created", s(created)),
                ("creators", CValue::Arr(vec![s(&format!("Tool: fid-xtask-{}", env!("CARGO_PKG_VERSION")))])),
            ]),
        ),
        ("packages", CValue::Arr(packages)),
        ("relationships", CValue::Arr(relationships)),
    ]);
    to_canonical_bytes(&doc).map_err(|e| SbomError::Io(format!("{artefact}{SBOM_SUFFIX}: {e}")))
}

// ------------------------------------------------------------------------------------------
// The command
// ------------------------------------------------------------------------------------------

/// Arguments of `rel sbom`.
#[derive(Debug, Clone, Copy)]
pub struct Options<'a> {
    pub plan: &'a Path,
    pub workspace: &'a Path,
    pub native: &'a Path,
    /// Directory holding the artefact files; when absent, the plan's `sha256` is used.
    pub artefacts: Option<&'a Path>,
    /// Overrides the plan's `commit_time`.
    pub commit_time: Option<&'a str>,
}

/// One generated document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sbom {
    /// `<artefact>.spdx.json`.
    pub file_name: String,
    pub bytes: Vec<u8>,
    /// Number of component packages (the root not counted).
    pub components: usize,
}

fn assigned(n: &Native, a: &PlanArtefact) -> bool {
    n.artefacts.iter().any(|x| *x == a.file_name || a.package.as_deref() == Some(x.as_str()))
}

/// Computes every document in memory. Findings come back all at once, sorted.
pub fn generate(opts: &Options<'_>) -> Result<(String, Vec<Sbom>), Vec<SbomError>> {
    let one = |e: SbomError| vec![e];
    let plan = parse_plan(&read_bounded(opts.plan, SbomError::PlanParse).map_err(one)?).map_err(one)?;
    let created = match opts.commit_time.map(str::to_string).or_else(|| plan.commit_time.clone()) {
        Some(t) if is_valid_time(&t) => t,
        Some(t) => return Err(one(SbomError::PlanParse(format!("commit time {t:?} is not YYYY-MM-DDThh:mm:ssZ")))),
        None => return Err(one(SbomError::PlanParse("no commit time: set `commit_time` or --commit-time".into()))),
    };
    let native_bytes = read_bounded(opts.native, SbomError::NativeParse).map_err(one)?;
    let native_text = String::from_utf8(native_bytes)
        .map_err(|_| one(SbomError::NativeParse(format!("{} is not UTF-8", opts.native.display()))))?;
    let natives = parse_native(&native_text)?;
    for n in &natives {
        for x in &n.artefacts {
            if !plan.artefacts.iter().any(|a| *x == a.file_name || a.package.as_deref() == Some(x.as_str())) {
                return Err(one(SbomError::NativeParse(format!(
                    "native components: {}@{} is assigned to {x:?}, which is no artefact or package of the plan",
                    n.name, n.version
                ))));
            }
        }
    }
    let lock_path = opts.workspace.join("Cargo.lock");
    let lock_bytes = read_bounded(&lock_path, SbomError::Metadata).map_err(one)?;
    let lock_text = String::from_utf8(lock_bytes).map_err(|_| one(md("Cargo.lock is not UTF-8")))?;
    let checksums = lock_checksums(&lock_text).map_err(one)?;

    let mut graphs: BTreeMap<(String, Vec<String>), Resolved> = BTreeMap::new();
    let mut findings: BTreeSet<SbomError> = BTreeSet::new();
    let mut planned: Vec<(&PlanArtefact, String, Vec<Component>)> = Vec::new();
    for a in &plan.artefacts {
        let digest = match (opts.artefacts, &a.sha256) {
            (Some(dir), _) => {
                hash_file(&dir.join(&a.file_name)).map_err(|e| one(SbomError::Io(e.to_string())))?.0.hex().to_string()
            }
            (None, Some(h)) => h.clone(),
            (None, None) => {
                return Err(one(SbomError::PlanParse(format!(
                    "plan: artefact {} has no `sha256` and no --artefacts directory was given",
                    a.file_name
                ))))
            }
        };
        let key = (a.target.clone(), a.features.clone());
        if !graphs.contains_key(&key) {
            let g = run_metadata(opts.workspace, &a.target, &a.features).map_err(one)?;
            graphs.insert(key.clone(), g);
        }
        let Some(g) = graphs.get(&key) else {
            return Err(one(md("internal: graph cache")));
        };
        let mut comps = Vec::new();
        for (p, scope) in artefact_crates(g, a).map_err(one)? {
            let (licence, original) = normalise_cargo_licence(p.license.as_deref().unwrap_or_default());
            if let Err(reason) = licence_check(&licence) {
                findings.insert(SbomError::Licence { component: format!("{}@{}", p.name, p.version), reason });
                continue;
            }
            let sum = p
                .source
                .as_ref()
                .and_then(|src| checksums.get(&(p.name.clone(), p.version.clone(), src.clone())))
                .filter(|c| is_hex64(c))
                .cloned();
            comps.push(Component::from_crate(p, &licence, sum, scope, original.as_deref()));
        }
        comps.extend(natives.iter().filter(|n| assigned(n, a)).map(Component::from_native));
        planned.push((a, digest, comps));
    }
    if !findings.is_empty() {
        return Err(findings.into_iter().collect());
    }
    let mut out = Vec::with_capacity(planned.len());
    for (a, digest, comps) in planned {
        let components = comps.len();
        let bytes = document(&a.file_name, &plan.version, &created, &digest, comps).map_err(one)?;
        out.push(Sbom { file_name: format!("{}{SBOM_SUFFIX}", a.file_name), bytes, components });
    }
    out.sort_by(|x, y| x.file_name.cmp(&y.file_name));
    Ok((plan.version, out))
}

/// `rel sbom`: generates every document, then writes them into `out` (created if absent).
/// Nothing is written unless every artefact succeeds; each document is written to a temporary
/// file in `out` first and renamed into place once all are written. If a rename fails, the
/// documents already renamed are removed again, so a failed run leaves no partial set.
pub fn run(opts: &Options<'_>, out: &Path) -> Result<(String, Vec<Sbom>), Vec<SbomError>> {
    let (release, docs) = generate(opts)?;
    let io = |p: &Path, e: std::io::Error| vec![SbomError::Io(format!("{}: {e}", p.display()))];
    std::fs::create_dir_all(out).map_err(|e| io(out, e))?;
    let tmp = |d: &Sbom| out.join(format!(".{}.tmp", d.file_name));
    let cleanup = || {
        for d in &docs {
            let _ = std::fs::remove_file(tmp(d));
        }
    };
    for d in &docs {
        if let Err(e) = std::fs::write(tmp(d), &d.bytes) {
            cleanup();
            return Err(io(&tmp(d), e));
        }
    }
    let mut renamed: Vec<std::path::PathBuf> = Vec::with_capacity(docs.len());
    for d in &docs {
        let dest = out.join(&d.file_name);
        if let Err(e) = std::fs::rename(tmp(d), &dest) {
            for r in &renamed {
                let _ = std::fs::remove_file(r);
            }
            cleanup();
            return Err(io(&dest, e));
        }
        renamed.push(dest);
    }
    Ok((release, docs))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn licence_expressions() {
        for ok in [
            "MIT",
            "MIT OR Apache-2.0",
            "(MIT OR Apache-2.0) AND Unicode-3.0",
            "GPL-2.0-or-later WITH Classpath-exception-2.0",
            "GPL-2.0+",
            "LicenseRef-Proprietary",
            "DocumentRef-spdx-tool-1.2:LicenseRef-MIT-Style-2",
            "Apache-2.0 WITH LLVM-exception OR MIT",
            "((MIT))",
        ] {
            assert_eq!(check_licence_expression(ok), Ok(()), "{ok}");
        }
        for bad in [
            "",
            "   ",
            "GPL-2.0 OR",
            "MIT/Apache-2.0",
            "MIT and Apache-2.0",
            "MIT OR OR Apache-2.0",
            "(MIT",
            "MIT)",
            "(MIT) WITH Classpath-exception-2.0",
            "MIT WITH",
            "LicenseRef-",
            "DocumentRef-x:MIT",
            "AND",
            "MIT Apache-2.0",
        ] {
            assert!(check_licence_expression(bad).is_err(), "{bad:?} accepted");
        }
        assert_eq!(check_licence_expression("GPL-2.0 OR"), Err("expression ends after OR".to_string()));
        let deep = format!("{}MIT{}", "(".repeat(40), ")".repeat(40));
        assert!(check_licence_expression(&deep).is_err());
    }

    #[test]
    fn encoders() {
        assert_eq!(pct_encode("Fixture Sans"), "Fixture%20Sans");
        assert_eq!(pct_encode("1.0.0+build.5"), "1.0.0%2Bbuild.5");
        assert_eq!(pct_encode("ä"), "%C3%A4");
        assert_eq!(spdx_id_part("serde_json 1.0+x"), "serde-json-1.0-x");
    }

    #[test]
    fn download_locations() {
        assert_eq!(
            download_location(Some(CRATES_IO_SOURCES[0]), "serde", "1.0.0"),
            "https://crates.io/api/v1/crates/serde/1.0.0/download"
        );
        assert_eq!(
            download_location(Some("sparse+https://index.crates.io/"), "a", "0.1.0"),
            "https://crates.io/api/v1/crates/a/0.1.0/download"
        );
        assert_eq!(
            download_location(Some("git+https://github.com/o/r?rev=abc#0123abcd"), "a", "0.1.0"),
            "git+https://github.com/o/r@0123abcd"
        );
        assert_eq!(download_location(Some("git+https://github.com/o/r"), "a", "0.1.0"), "git+https://github.com/o/r");
        assert_eq!(download_location(None, "a", "0.1.0"), NOASSERTION);
        assert_eq!(download_location(Some("registry+https://example.org/index"), "a", "0.1.0"), NOASSERTION);
    }

    #[test]
    fn times() {
        assert!(is_valid_time("2026-11-17T09:30:00Z"));
        assert!(is_valid_time("2024-02-29T23:59:59Z"));
        assert!(!is_valid_time("2026-02-29T00:00:00Z"));
        assert!(!is_valid_time("2026-11-17T24:00:00Z"));
        assert!(!is_valid_time("2026-11-17T09:30:00+00:00"));
        assert!(!is_valid_time("2026-13-01T00:00:00Z"));
        assert!(!is_valid_time("2026-11-17 09:30:00Z"));
    }

    #[test]
    fn namespace_is_uuid_v5() {
        // Python: uuid.uuid5(uuid.NAMESPACE_URL, "https://www.example.org/").
        let u = uuid_v5(&NAMESPACE_URL, b"https://www.example.org/");
        assert_eq!(u.to_string(), "84f5ec25-93a7-5271-bfbb-77d0e0a15d7b");
        assert!(document_namespace("1.4.0", "a.zip").starts_with(&format!("{REPO}/spdx/a.zip-")));
    }

    #[test]
    fn native_file() {
        let good = "[[component]]\nname = \"F\"\nversion = \"1\"\nsource = \"https://x\"\nlicense = \"OFL-1.1\"\n\
                    sha256 = \"0000000000000000000000000000000000000000000000000000000000000000\"\nartefacts = [\"fid\"]\n";
        assert_eq!(parse_native(good).map(|v| v.len()), Ok(1));
        assert_eq!(parse_native(""), Ok(Vec::new()));
        let bad_digest = good.replace("0000000000000000000000000000000000000000000000000000000000000000", "abc");
        assert_eq!(parse_native(&bad_digest), Err(vec![SbomError::NativeDigest("F@1".into())]));
        let bad_licence = good.replace("OFL-1.1", "OFL-1.1 AND");
        assert!(matches!(parse_native(&bad_licence).as_ref().map_err(Vec::as_slice), Err([SbomError::Licence { .. }])));
        let unknown = format!("{good}extra = 1\n");
        assert!(matches!(parse_native(&unknown).as_ref().map_err(Vec::as_slice), Err([SbomError::NativeParse(_)])));
    }

    #[test]
    fn plan_errors() {
        assert!(parse_plan(b"[]").is_err());
        let ok = br#"{"version":"1.0.0","artefacts":[{"file_name":"a.zip","target":"x86_64-pc-windows-msvc","contents":["fid"]}]}"#;
        assert_eq!(parse_plan(ok).map(|p| p.artefacts.len()), Ok(1));
        let twice = br#"{"version":"1","artefacts":[{"file_name":"a","target":"t","contents":["x"]},{"file_name":"a","target":"t","contents":["x"]}]}"#;
        assert!(matches!(parse_plan(twice), Err(SbomError::PlanParse(_))));
        let bad_name = br#"{"version":"1","artefacts":[{"file_name":"a b","target":"t","contents":["x"]}]}"#;
        assert!(matches!(parse_plan(bad_name), Err(SbomError::PlanParse(_))));
    }

    /// Hand-written `cargo metadata` output (labelled by hand): `app` (workspace member, binary)
    /// depends on `n` (normal), `d` (dev only), `b` (build), `pm` (proc-macro), `w` (normal, only
    /// for `cfg(windows)`: present because the graph was filtered for a Windows target) and `g`
    /// (git). `b` depends on `bn` and on `n`; `pm` depends on `pmd`.
    const METADATA: &str = r#"{
      "workspace_members": ["app"],
      "packages": [
        {"id":"app","name":"app","version":"0.1.0","source":null,"license":"MIT",
         "targets":[{"kind":["bin"],"name":"app"}]},
        {"id":"n","name":"n","version":"1.0.0","source":"registry+https://github.com/rust-lang/crates.io-index",
         "license":"MIT/Apache-2.0","targets":[{"kind":["lib"],"name":"n"}]},
        {"id":"d","name":"d","version":"1.0.0","source":"registry+https://github.com/rust-lang/crates.io-index",
         "license":"MIT","targets":[{"kind":["lib"],"name":"d"}]},
        {"id":"b","name":"b","version":"1.0.0","source":"registry+https://github.com/rust-lang/crates.io-index",
         "license":"MIT","targets":[{"kind":["lib"],"name":"b"}]},
        {"id":"bn","name":"bn","version":"1.0.0","source":"registry+https://github.com/rust-lang/crates.io-index",
         "license":"MIT","targets":[{"kind":["lib"],"name":"bn"}]},
        {"id":"pm","name":"pm","version":"1.0.0","source":"registry+https://github.com/rust-lang/crates.io-index",
         "license":"MIT","targets":[{"kind":["proc-macro"],"name":"pm"}]},
        {"id":"pmd","name":"pmd","version":"1.0.0","source":"registry+https://github.com/rust-lang/crates.io-index",
         "license":"MIT","targets":[{"kind":["lib"],"name":"pmd"}]},
        {"id":"w","name":"w","version":"1.0.0","source":"registry+https://github.com/rust-lang/crates.io-index",
         "license":"MIT","targets":[{"kind":["lib"],"name":"w"}]},
        {"id":"g","name":"g","version":"0.3.0","source":"git+https://github.com/o/r?rev=abc#0123abcd",
         "license":"Apache-2.0","targets":[{"kind":["lib"],"name":"g"}]}
      ],
      "resolve": {"nodes": [
        {"id":"app","deps":[
          {"pkg":"n","dep_kinds":[{"kind":null,"target":null}]},
          {"pkg":"d","dep_kinds":[{"kind":"dev","target":null}]},
          {"pkg":"b","dep_kinds":[{"kind":"build","target":null}]},
          {"pkg":"pm","dep_kinds":[{"kind":null,"target":null}]},
          {"pkg":"w","dep_kinds":[{"kind":null,"target":"cfg(windows)"}]},
          {"pkg":"g","dep_kinds":[{"kind":null,"target":null}]}]},
        {"id":"b","deps":[
          {"pkg":"bn","dep_kinds":[{"kind":null,"target":null}]},
          {"pkg":"n","dep_kinds":[{"kind":null,"target":null}]}]},
        {"id":"pm","deps":[{"pkg":"pmd","dep_kinds":[{"kind":null,"target":null}]}]},
        {"id":"n","deps":[]}, {"id":"d","deps":[]}, {"id":"bn","deps":[]},
        {"id":"pmd","deps":[]}, {"id":"w","deps":[]}, {"id":"g","deps":[]}
      ]}
    }"#;

    fn app_artefact() -> PlanArtefact {
        PlanArtefact {
            file_name: "app.zip".into(),
            package: None,
            target: "x86_64-pc-windows-msvc".into(),
            contents: vec!["app".into()],
            features: Vec::new(),
            sha256: None,
        }
    }

    #[test]
    fn metadata_edges_and_scopes() {
        let g = parse_metadata(METADATA.as_bytes()).unwrap();
        let app: Vec<(&str, bool, bool)> = g.edges["app"].iter().map(|e| (e.to.as_str(), e.normal, e.build)).collect();
        // The dev-only edge to `d` is dropped; `b` is a build edge; `w` (target-specific) is normal.
        assert_eq!(
            app,
            [("b", false, true), ("g", true, false), ("n", true, false), ("pm", true, false), ("w", true, false)]
        );
        assert!(g.packages["pm"].proc_macro && !g.packages["n"].proc_macro);
        let scopes: Vec<(String, Scope)> =
            artefact_crates(&g, &app_artefact()).unwrap().into_iter().map(|(p, s)| (p.name.clone(), s)).collect();
        let want = [
            ("app", Scope::Contained),
            ("b", Scope::Build),
            ("bn", Scope::Build),
            ("g", Scope::Contained),
            ("n", Scope::Contained),
            ("pm", Scope::Build),
            ("pmd", Scope::Build),
            ("w", Scope::Contained),
        ];
        assert_eq!(scopes, want.map(|(n, s)| (n.to_string(), s)));
    }

    #[test]
    fn metadata_component_fields() {
        let g = parse_metadata(METADATA.as_bytes()).unwrap();
        let git = Component::from_crate(&g.packages["g"], "Apache-2.0", None, Scope::Contained, None);
        assert_eq!(git.download, "git+https://github.com/o/r@0123abcd");
        assert_eq!(git.purl, "pkg:cargo/g@0.3.0");
        let app = Component::from_crate(&g.packages["app"], "MIT", None, Scope::Contained, None);
        assert_eq!(app.purl, format!("pkg:cargo/app@0.1.0?repository_url={REPO}"));
        assert_eq!(app.download, NOASSERTION);
        let bad = METADATA.replace(r#""kind":"dev""#, r#""kind":"weird""#);
        assert!(matches!(parse_metadata(bad.as_bytes()), Err(SbomError::Metadata(_))));
        let unknown = METADATA.replace(r#"{"pkg":"g","#, r#"{"pkg":"zz","#);
        assert!(matches!(parse_metadata(unknown.as_bytes()), Err(SbomError::Metadata(_))));
    }

    #[test]
    fn build_dependency_relationship() {
        let g = parse_metadata(METADATA.as_bytes()).unwrap();
        let comps: Vec<Component> = artefact_crates(&g, &app_artefact())
            .unwrap()
            .into_iter()
            .map(|(p, s)| Component::from_crate(p, "MIT", None, s, None))
            .collect();
        let doc = document("app.zip", "1.0.0", "2026-11-17T09:30:00Z", &"0".repeat(64), comps).unwrap();
        let text = String::from_utf8(doc).unwrap();
        assert!(text.contains(r#"{"relatedSpdxElement":"SPDXRef-Artefact","relationshipType":"BUILD_DEPENDENCY_OF","spdxElementId":"SPDXRef-Crate-pm-1.0.0"}"#));
        assert!(text.contains(r#"{"relatedSpdxElement":"SPDXRef-Crate-n-1.0.0","relationshipType":"CONTAINS","spdxElementId":"SPDXRef-Artefact"}"#));
        assert!(!text.contains("SPDXRef-Crate-d-"));
    }

    #[test]
    fn legacy_slash_licences() {
        assert_eq!(normalise_cargo_licence("MIT"), ("MIT".to_string(), None));
        assert_eq!(
            normalise_cargo_licence("MIT/Apache-2.0"),
            ("MIT OR Apache-2.0".to_string(), Some("MIT/Apache-2.0".to_string()))
        );
        let (n, _) = normalise_cargo_licence("MIT / Apache-2.0 WITH LLVM-exception");
        assert_eq!(n, "MIT OR (Apache-2.0 WITH LLVM-exception)");
        assert_eq!(check_licence_expression(&n), Ok(()));
        assert!(check_licence_expression(&normalise_cargo_licence("MIT/").0).is_err());
        let c = Component::from_crate(
            &parse_metadata(METADATA.as_bytes()).unwrap().packages["n"],
            "MIT OR Apache-2.0",
            None,
            Scope::Contained,
            Some("MIT/Apache-2.0"),
        );
        assert_eq!(
            c.comment.as_deref(),
            Some("Cargo licence field \"MIT/Apache-2.0\": the legacy `/` separator is read as OR")
        );
    }
}
