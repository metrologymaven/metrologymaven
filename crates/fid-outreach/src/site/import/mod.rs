//! Generated-input lock file, digest check and build manifest (spec node `OUTR.SITE.GEN.IMPORT`).
//!
//! Every generated input of the website (release manifests and their parity and status reports,
//! the app list, spec-tree reports, launch-plan milestones, design tokens, brand assets) is
//! vendored into the site repository under `inputs/` and pinned in `inputs.lock.toml`:
//!
//! ```toml
//! [[input]]
//! name = "parity.json"
//! kind = "release_asset"       # release_manifest | release_asset | spec_report | app_list
//!                              # | launch_plan | tokens | brand_asset
//! release = "v0.1.0"           # optional; at least one of release and source_rev
//! source_rev = "<commit>"      # optional
//! date = "2026-10-01"          # YYYY-MM-DD (string or TOML local date)
//! path = "inputs/v0.1.0/parity.json"
//! sha256 = "sha256:<64 hex>"
//! ```
//!
//! [`verify_and_stage`] (`fid-outreach site import verify`) checks the lock against the files and
//! each release's `release-manifest.json`, then copies the inputs to `<stage>/data/<path>` and
//! writes `<stage>/build-manifest.json` as RFC 8785 canonical JSON. It never uses the network:
//! this module has no network or process code; downloading lives in [`fetch`] only.
//!
//! Findings (exit 1): `IMPORT-DIGEST <name> <expected> <actual>`, `IMPORT-RELEASE <name> <tag>`,
//! `IMPORT-UNLISTED <path>`, `IMPORT-MISSING <name>`, `IMPORT-KIND <kind>`, and for fetch
//! `IMPORT-HOST <host>` and `IMPORT-FETCH <url> <status>`. Unreadable or unparsable input is an
//! [`ImportError`] (exit 2).
//!
//! Determinism D0: the build manifest depends only on the lock file and the two arguments.

#![deny(clippy::todo, clippy::unimplemented, clippy::unreachable)]

pub mod fetch;

use fid_canon::CValue;
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Largest lock file, in bytes.
pub const MAX_LOCK_BYTES: u64 = 256 * 1024;
/// Largest `hosts.toml`, in bytes.
pub const MAX_HOSTS_BYTES: u64 = 64 * 1024;
/// Largest release manifest, in bytes.
pub const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
/// Largest fetched report, in bytes.
pub const MAX_REPORT_BYTES: u64 = 32 * 1024 * 1024;
/// Most files and folders listed under `inputs/`.
pub const MAX_TREE_ENTRIES: usize = 100_000;
/// The input kinds a lock entry may have.
pub const KINDS: [&str; 7] =
    ["release_manifest", "release_asset", "spec_report", "app_list", "launch_plan", "tokens", "brand_asset"];
/// Lock file name inside the site folder.
pub const LOCK_FILE: &str = "inputs.lock.toml";
/// Host allowlist file name inside the site folder.
pub const HOSTS_FILE: &str = "hosts.toml";
/// File name of a release manifest.
pub const RELEASE_MANIFEST: &str = "release-manifest.json";

const ENTRY_KEYS: [&str; 7] = ["date", "kind", "name", "path", "release", "sha256", "source_rev"];
const LOCK_HEADER: &str = "# Generated-input lock file (spec node OUTR.SITE.GEN.IMPORT); entries sorted by path.\n\
# Written by `fid-outreach site import fetch`; review every change in its pull request.\n";

// ------------------------------------------------------------------ findings and errors

/// A check failure (exit 1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Finding {
    /// A listed file's SHA-256 differs from its lock entry (or, in fetch, from the manifest).
    Digest { name: String, expected: Digest, actual: Digest },
    /// A release asset is absent from its release manifest or has another digest there.
    Release { name: String, tag: String },
    /// A file under `inputs/` that no lock entry names.
    Unlisted { path: String },
    /// A listed file does not exist.
    Missing { name: String },
    /// A lock entry has a kind outside [`KINDS`].
    Kind { kind: String },
    /// The fetch URL's host is not in `hosts.toml`.
    Host { host: String },
    /// A download failed (`status` is the HTTP status or a short reason).
    Fetch { url: String, status: String },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Finding::Digest { name, expected, actual } => write!(f, "IMPORT-DIGEST {name} {expected} {actual}"),
            Finding::Release { name, tag } => write!(f, "IMPORT-RELEASE {name} {tag}"),
            Finding::Unlisted { path } => write!(f, "IMPORT-UNLISTED {path}"),
            Finding::Missing { name } => write!(f, "IMPORT-MISSING {name}"),
            Finding::Kind { kind } => write!(f, "IMPORT-KIND {kind}"),
            Finding::Host { host } => write!(f, "IMPORT-HOST {host}"),
            Finding::Fetch { url, status } => write!(f, "IMPORT-FETCH {url} {status}"),
        }
    }
}

/// A condition that stops a command (exit 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// An input cannot be read or does not parse (`INPUT-PARSE`).
    InputParse { path: String, reason: String },
    /// An output cannot be written (`IMPORT-IO`).
    Io { path: String, reason: String },
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::InputParse { path, reason } => write!(f, "INPUT-PARSE {path}: {reason}"),
            ImportError::Io { path, reason } => write!(f, "IMPORT-IO {path}: {reason}"),
        }
    }
}

fn parse_err(path: &str, reason: impl Into<String>) -> ImportError {
    ImportError::InputParse { path: path.into(), reason: reason.into() }
}

fn io_err(path: &Path, e: &std::io::Error) -> ImportError {
    ImportError::Io { path: path.display().to_string(), reason: e.to_string() }
}

// ------------------------------------------------------------------ small validators

/// True for `YYYY-MM-DD` naming a real calendar date.
pub fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        let part = s.get(r)?;
        if part.bytes().all(|c| c.is_ascii_digit()) {
            part.parse().ok()
        } else {
            None
        }
    };
    let (Some(y), Some(m), Some(d)) = (num(0..4), num(5..7), num(8..10)) else { return false };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&d)
}

/// True for a release tag: 1 to 64 of `A-Z a-z 0-9 . _ -`, not `.` or `..`.
pub fn is_tag(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s != "."
        && s != ".."
        && s.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

fn is_rev(s: &str) -> bool {
    (7..=64).contains(&s.len()) && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

/// True for a lock path: `inputs/` followed by `/`-separated plain components.
pub fn is_input_path(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("inputs/") else { return false };
    !rest.is_empty()
        && rest.split('/').all(|c| {
            !c.is_empty()
                && c != "."
                && c != ".."
                && !c.bytes().any(|b| b.is_ascii_control() || matches!(b, b'\\' | b':' | b'"'))
        })
}

/// True when a release manifest `version` belongs to `tag` (`tag` is `version` or `v<version>`).
pub fn tag_matches(tag: &str, version: &str) -> bool {
    tag == version || tag.strip_prefix('v') == Some(version)
}

/// The site-relative path joined onto `root`, component by component.
pub fn join_rel(root: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(root.to_path_buf(), |p, c| p.join(c))
}

/// The last `/`-separated component of a lock path.
pub fn file_name(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

// ------------------------------------------------------------------ hashing and reading

/// SHA-256 of a file, read in streaming mode.
pub fn hash_file(path: &Path) -> std::io::Result<Digest> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(buf.get(..n).unwrap_or_default());
    }
    Ok(Digest::from_bytes(h.finalize().into()))
}

/// SHA-256 of bytes.
pub fn hash_bytes(data: &[u8]) -> Digest {
    Digest::from_bytes(Sha256::digest(data).into())
}

/// Reads a file of at most `cap` bytes.
pub fn read_bounded(path: &Path, cap: u64) -> Result<Vec<u8>, ImportError> {
    let shown = path.display().to_string();
    let f = std::fs::File::open(path).map_err(|e| parse_err(&shown, e.to_string()))?;
    let mut data = Vec::new();
    f.take(cap + 1).read_to_end(&mut data).map_err(|e| parse_err(&shown, e.to_string()))?;
    if data.len() as u64 > cap {
        return Err(parse_err(&shown, format!("larger than {cap} bytes")));
    }
    Ok(data)
}

// ------------------------------------------------------------------ lock file

/// One `[[input]]` entry of the lock file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockEntry {
    pub name: String,
    pub kind: String,
    pub release: Option<String>,
    pub source_rev: Option<String>,
    pub date: String,
    pub path: String,
    pub sha256: Digest,
}

fn date_value(v: &toml::Value) -> Option<String> {
    let s = match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Datetime(dt) if dt.time.is_none() && dt.offset.is_none() => dt.to_string(),
        _ => return None,
    };
    is_date(&s).then_some(s)
}

/// Parses the lock file. Entries with an unknown kind become [`Finding::Kind`] and are left out;
/// anything else malformed is an [`ImportError::InputParse`]. Entries come back sorted by path.
pub fn parse_lock(text: &str, shown: &str) -> Result<(Vec<LockEntry>, Vec<Finding>), ImportError> {
    let table = text.parse::<toml::Table>().map_err(|e| parse_err(shown, e.message()))?;
    let mut entries = Vec::new();
    let mut findings = Vec::new();
    for (key, value) in &table {
        if key != "input" {
            return Err(parse_err(shown, format!("unknown top-level key `{key}`")));
        }
        let toml::Value::Array(items) = value else { return Err(parse_err(shown, "`input` is not [[input]]")) };
        for (i, item) in items.iter().enumerate() {
            let at = format!("input #{}", i + 1);
            let toml::Value::Table(t) = item else { return Err(parse_err(shown, format!("{at} is not a table"))) };
            if let Some(k) = t.keys().find(|k| !ENTRY_KEYS.contains(&k.as_str())) {
                return Err(parse_err(shown, format!("{at}: unknown key `{k}`")));
            }
            let s = |k: &str| t.get(k).and_then(toml::Value::as_str).map(str::to_string);
            let req = |k: &str| s(k).ok_or_else(|| parse_err(shown, format!("{at}: `{k}` missing or not a string")));
            let name = req("name")?;
            let kind = req("kind")?;
            let path = req("path")?;
            if name.is_empty() || name.contains(char::is_whitespace) {
                return Err(parse_err(shown, format!("{at}: bad `name`")));
            }
            if !is_input_path(&path) {
                return Err(parse_err(shown, format!("{at}: `path` must be inputs/<plain components>")));
            }
            let sha256 = Digest::parse(&req("sha256")?).map_err(|_| parse_err(shown, format!("{at}: bad `sha256`")))?;
            let date = t
                .get("date")
                .and_then(date_value)
                .ok_or_else(|| parse_err(shown, format!("{at}: `date` missing or not YYYY-MM-DD")))?;
            let release = match t.get("release") {
                None => None,
                Some(_) => Some(s("release").filter(|r| is_tag(r)).ok_or_else(|| parse_err(shown, format!("{at}: bad `release`")))?),
            };
            let source_rev = match t.get("source_rev") {
                None => None,
                Some(_) => Some(
                    s("source_rev").filter(|r| is_rev(r)).ok_or_else(|| parse_err(shown, format!("{at}: bad `source_rev`")))?,
                ),
            };
            if release.is_none() && source_rev.is_none() {
                return Err(parse_err(shown, format!("{at}: needs `release`, `source_rev` or both")));
            }
            if !KINDS.contains(&kind.as_str()) {
                findings.push(Finding::Kind { kind });
                continue;
            }
            if matches!(kind.as_str(), "release_manifest" | "release_asset") && release.is_none() {
                return Err(parse_err(shown, format!("{at}: kind {kind} needs `release`")));
            }
            entries.push(LockEntry { name, kind, release, source_rev, date, path, sha256 });
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    if let Some(w) = entries.windows(2).find(|w| w[0].path == w[1].path) {
        return Err(parse_err(shown, format!("duplicate path {}", w[0].path)));
    }
    let mut manifests = BTreeSet::new();
    for e in entries.iter().filter(|e| e.kind == "release_manifest") {
        if !manifests.insert(e.release.clone()) {
            return Err(parse_err(shown, format!("two release manifests for {}", e.release.as_deref().unwrap_or(""))));
        }
    }
    Ok((entries, findings))
}

fn toml_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Writes lock entries in the canonical lock-file layout, sorted by path.
pub fn render_lock(entries: &[LockEntry]) -> String {
    let mut sorted: Vec<&LockEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut out = String::from(LOCK_HEADER);
    for e in sorted {
        out.push_str("\n[[input]]\n");
        let mut field = |k: &str, v: &str| out.push_str(&format!("{k} = {}\n", toml_str(v)));
        field("name", &e.name);
        field("kind", &e.kind);
        if let Some(r) = &e.release {
            field("release", r);
        }
        if let Some(r) = &e.source_rev {
            field("source_rev", r);
        }
        field("date", &e.date);
        field("path", &e.path);
        field("sha256", e.sha256.as_str());
    }
    out
}

// ------------------------------------------------------------------ release manifest

/// The fields of a `release-manifest.json` (schema v1) this module reads. Other fields are left
/// untouched (the staged copy is the file itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseManifest {
    pub version: String,
    /// Release date (`YYYY-MM-DD`, or the date part of an RFC 3339 time), when present.
    pub date: Option<String>,
    /// Artefact file name to SHA-256.
    pub artefacts: BTreeMap<String, Digest>,
}

fn manifest_digest(s: &str) -> Option<Digest> {
    if s.len() == 64 {
        Digest::parse(&format!("sha256:{s}")).ok()
    } else {
        Digest::parse(s).ok()
    }
}

/// Parses a release manifest: `version`, `artefacts[].name` and `artefacts[].sha256` (either
/// `sha256:<hex>` or bare lowercase hex), and `date` when it is a date.
pub fn parse_release_manifest(bytes: &[u8], shown: &str) -> Result<ReleaseManifest, ImportError> {
    let v = fid_canon::parse_with_limits(bytes, MAX_MANIFEST_BYTES as usize, fid_canon::MAX_DEPTH)
        .map_err(|e| parse_err(shown, format!("release manifest: {}", e.code())))?;
    let CValue::Obj(top) = v else { return Err(parse_err(shown, "release manifest is not an object")) };
    let Some(CValue::Str(version)) = top.get("version") else {
        return Err(parse_err(shown, "release manifest: `version` missing"));
    };
    let date = match top.get("date") {
        Some(CValue::Str(d)) => {
            let day = d.get(..10).unwrap_or(d.as_str());
            (is_date(day) && (d.len() == 10 || d.as_bytes().get(10) == Some(&b'T'))).then(|| day.to_string())
        }
        _ => None,
    };
    let Some(CValue::Arr(items)) = top.get("artefacts") else {
        return Err(parse_err(shown, "release manifest: `artefacts` missing"));
    };
    let mut artefacts = BTreeMap::new();
    for item in items {
        let CValue::Obj(a) = item else { return Err(parse_err(shown, "release manifest: artefact not an object")) };
        let (Some(CValue::Str(name)), Some(CValue::Str(sha))) = (a.get("name"), a.get("sha256")) else {
            return Err(parse_err(shown, "release manifest: artefact without name or sha256"));
        };
        let Some(d) = manifest_digest(sha) else {
            return Err(parse_err(shown, format!("release manifest: bad sha256 for {name}")));
        };
        if artefacts.insert(name.clone(), d).is_some() {
            return Err(parse_err(shown, format!("release manifest: duplicate artefact {name}")));
        }
    }
    Ok(ReleaseManifest { version: version.clone(), date, artefacts })
}

// ------------------------------------------------------------------ verify

/// Arguments of [`verify_and_stage`].
#[derive(Debug, Clone, Copy)]
pub struct VerifyArgs<'a> {
    /// Site repository folder (holds `inputs.lock.toml` and `inputs/`).
    pub site: &'a Path,
    /// Staging folder; receives `data/<path>` and `build-manifest.json`.
    pub stage: &'a Path,
    /// Site revision recorded in the build manifest.
    pub revision: &'a str,
    /// Revision date (`YYYY-MM-DD`) recorded in the build manifest.
    pub revision_date: &'a str,
}

/// Lists every non-folder entry under `dir` as a site-relative path (`inputs/...`), sorted.
fn list_inputs(site: &Path) -> Result<Vec<String>, ImportError> {
    let root = site.join("inputs");
    let mut out = Vec::new();
    if !root.is_dir() {
        return Ok(out);
    }
    let mut stack = vec![(root, String::from("inputs"))];
    let mut seen = 0usize;
    while let Some((dir, rel)) = stack.pop() {
        let rd = std::fs::read_dir(&dir).map_err(|e| parse_err(&dir.display().to_string(), e.to_string()))?;
        for entry in rd {
            let entry = entry.map_err(|e| parse_err(&dir.display().to_string(), e.to_string()))?;
            seen += 1;
            if seen > MAX_TREE_ENTRIES {
                return Err(parse_err("inputs", format!("more than {MAX_TREE_ENTRIES} entries")));
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let child = format!("{rel}/{name}");
            let ft = entry.file_type().map_err(|e| parse_err(&child, e.to_string()))?;
            if ft.is_dir() {
                stack.push((entry.path(), child));
            } else {
                out.push(child);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Checks the lock file against the files under `<site>/inputs/` and each release manifest.
/// Returns the sorted findings; nothing is written. Makes no network request.
pub fn verify(site: &Path) -> Result<(Vec<LockEntry>, Vec<Finding>), ImportError> {
    let lock_path = site.join(LOCK_FILE);
    let bytes = read_bounded(&lock_path, MAX_LOCK_BYTES)?;
    let shown = lock_path.display().to_string();
    let text = String::from_utf8(bytes).map_err(|_| parse_err(&shown, "not UTF-8"))?;
    let (entries, mut findings) = parse_lock(&text, &shown)?;

    // Files against the lock.
    let mut intact = BTreeSet::new();
    for e in &entries {
        let p = join_rel(site, &e.path);
        if !p.is_file() {
            findings.push(Finding::Missing { name: e.name.clone() });
            continue;
        }
        let actual = hash_file(&p).map_err(|err| parse_err(&e.path, err.to_string()))?;
        if actual == e.sha256 {
            intact.insert(e.path.clone());
        } else {
            findings.push(Finding::Digest { name: e.name.clone(), expected: e.sha256, actual });
        }
    }

    // Files nobody listed.
    let listed: BTreeSet<&str> = entries.iter().map(|e| e.path.as_str()).collect();
    for path in list_inputs(site)? {
        if !listed.contains(path.as_str()) {
            findings.push(Finding::Unlisted { path });
        }
    }

    // Release assets against their release manifest (lock digest against manifest digest).
    let mut manifests: BTreeMap<&str, Option<ReleaseManifest>> = BTreeMap::new();
    for m in entries.iter().filter(|e| e.kind == "release_manifest") {
        let tag = m.release.as_deref().unwrap_or("");
        // A missing or altered manifest is already reported; it is not used for cross-checks.
        let parsed = if intact.contains(&m.path) {
            let p = join_rel(site, &m.path);
            let rm = parse_release_manifest(&read_bounded(&p, MAX_MANIFEST_BYTES)?, &m.path)?;
            if !tag_matches(tag, &rm.version) {
                findings.push(Finding::Release { name: m.name.clone(), tag: tag.into() });
            }
            Some(rm)
        } else {
            None
        };
        manifests.insert(tag, parsed);
    }
    for a in entries.iter().filter(|e| e.kind == "release_asset") {
        let tag = a.release.as_deref().unwrap_or("");
        match manifests.get(tag) {
            None => findings.push(Finding::Release { name: a.name.clone(), tag: tag.into() }),
            Some(None) => {}
            Some(Some(rm)) => {
                if rm.artefacts.get(file_name(&a.path)) != Some(&a.sha256) {
                    findings.push(Finding::Release { name: a.name.clone(), tag: tag.into() });
                }
            }
        }
    }
    findings.sort();
    findings.dedup();
    Ok((entries, findings))
}

/// The build manifest of verified entries, as RFC 8785 canonical JSON.
pub fn build_manifest(entries: &[LockEntry], revision: &str, revision_date: &str) -> Result<Vec<u8>, ImportError> {
    let mut sorted: Vec<&LockEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let inputs = sorted
        .into_iter()
        .map(|e| {
            let mut o = BTreeMap::new();
            o.insert("name".to_string(), CValue::Str(e.name.clone()));
            o.insert("path".to_string(), CValue::Str(e.path.clone()));
            o.insert("kind".to_string(), CValue::Str(e.kind.clone()));
            if let Some(r) = &e.release {
                o.insert("release".to_string(), CValue::Str(r.clone()));
            }
            if let Some(r) = &e.source_rev {
                o.insert("source_rev".to_string(), CValue::Str(r.clone()));
            }
            o.insert("date".to_string(), CValue::Str(e.date.clone()));
            o.insert("sha256".to_string(), CValue::Str(e.sha256.as_str().to_string()));
            CValue::Obj(o)
        })
        .collect();
    let mut top = BTreeMap::new();
    top.insert("site_revision".to_string(), CValue::Str(revision.into()));
    top.insert("revision_date".to_string(), CValue::Str(revision_date.into()));
    top.insert("inputs".to_string(), CValue::Arr(inputs));
    fid_canon::to_canonical_bytes(&CValue::Obj(top))
        .map_err(|e| ImportError::Io { path: "build-manifest.json".into(), reason: e.code().into() })
}

/// Copies `src` to `dst` while hashing; returns the digest of the bytes written.
fn copy_hashing(src: &Path, dst: &Path) -> Result<Digest, ImportError> {
    use std::io::Write;
    let mut r = std::fs::File::open(src).map_err(|e| parse_err(&src.display().to_string(), e.to_string()))?;
    let mut w = std::fs::File::create(dst).map_err(|e| io_err(dst, &e))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = r.read(&mut buf).map_err(|e| parse_err(&src.display().to_string(), e.to_string()))?;
        if n == 0 {
            break;
        }
        let chunk = buf.get(..n).unwrap_or_default();
        h.update(chunk);
        w.write_all(chunk).map_err(|e| io_err(dst, &e))?;
    }
    w.flush().map_err(|e| io_err(dst, &e))?;
    Ok(Digest::from_bytes(h.finalize().into()))
}

/// Verifies the site's inputs and, when there is no finding, stages them: `<stage>/data/<path>`
/// (any earlier `<stage>/data` is replaced) and `<stage>/build-manifest.json`. With findings,
/// nothing is written and the sorted findings are returned. Makes no network request.
pub fn verify_and_stage(args: &VerifyArgs<'_>) -> Result<Vec<Finding>, ImportError> {
    if args.revision.is_empty() || args.revision.len() > 128 || !args.revision.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(parse_err("--revision", "must be 1 to 128 visible ASCII characters"));
    }
    if !is_date(args.revision_date) {
        return Err(parse_err("--revision-date", "must be YYYY-MM-DD"));
    }
    let (entries, findings) = verify(args.site)?;
    if !findings.is_empty() {
        return Ok(findings);
    }
    let manifest = build_manifest(&entries, args.revision, args.revision_date)?;
    let data = args.stage.join("data");
    if data.exists() {
        std::fs::remove_dir_all(&data).map_err(|e| io_err(&data, &e))?;
    }
    let mut late = Vec::new();
    for e in &entries {
        let dst = join_rel(&data, &e.path);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|err| io_err(parent, &err))?;
        }
        let actual = copy_hashing(&join_rel(args.site, &e.path), &dst)?;
        if actual != e.sha256 {
            // The file changed between the check and the copy.
            late.push(Finding::Digest { name: e.name.clone(), expected: e.sha256, actual });
        }
    }
    if !late.is_empty() {
        // Do not leave a half-verified stage behind.
        let _ = std::fs::remove_dir_all(&data);
        return Ok(late);
    }
    let out = args.stage.join("build-manifest.json");
    std::fs::write(&out, manifest).map_err(|e| io_err(&out, &e))?;
    Ok(Vec::new())
}
