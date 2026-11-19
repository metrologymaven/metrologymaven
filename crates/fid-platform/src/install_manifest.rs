//! Offline bundle layout, install manifest format and bundle check (spec node
//! `PLAT.DEPLOY.OFFLINE.BUNDLE`).
//!
//! Two documents, both `schema_version` 1 and RFC 8785 canonical JSON through `fid-canon`:
//!
//! - `offline-bundle.json` at the root of a bundle directory: bundle ID, release, OS,
//!   architecture and one entry `{ path, role, sha256, size }` per bundled file
//!   ([`OfflineBundle`]);
//! - `install-manifest.json`: release, OS, architecture, install kind (`per_user` or
//!   `machine`) and one entry `{ relpath, sha256, size }` per installed file, relative to the
//!   install root ([`InstallManifest`]). The doctor (`PLAT.DOCTOR.HOSTCHK.INSTALL`) and the
//!   offline updater read it on the user's machine.
//!
//! Readers are strict and bounded: at most [`MAX_BYTES`] bytes and [`MAX_ENTRIES`] entries
//! (else `TooLarge`), unknown keys refused, and the input must already be canonical JSON (so
//! one manifest has exactly one byte form). Every path is relative, `/`-separated, strictly
//! sorted by byte order, free of `.`/`..`/empty segments, backslashes, drive or stream colons,
//! control bytes and Windows device names (the per-segment rule is `fid-io-reg`'s
//! [`RelPath`]), unique also when compared ASCII-case-insensitively, and never both a file and
//! a folder of another entry; anything else is `BadPath`.
//!
//! [`check_bundle`] walks the bundle without following symbolic links, digests every file
//! before any of its bytes are used, compares the result with `offline-bundle.json`, applies
//! the package plan's content rules for the bundle's OS and architecture, parses the bundled
//! install manifests and scans the docs tree for remote resources a browser would load on its
//! own. Nothing here opens a socket or runs a program. Determinism D0: sorted walks, sorted
//! output, no clocks.

use crate::matrix::{Arch, OsFamily, Tier};
use fid_canon::{CValue, CanonError};
use fid_io_reg::RelPath;
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The only `schema_version` read or written, for both documents.
pub const SUPPORTED_SCHEMA: u32 = 1;
/// Largest accepted document (and package plan), in bytes (REQ-PLAT.DEPLOY.OFFLINE.BUNDLE-006).
pub const MAX_BYTES: usize = 32 * 1024 * 1024;
/// Most entries in one document, and most files in one walked tree (REQ-006).
pub const MAX_ENTRIES: usize = 200_000;
/// Deepest JSON nesting accepted (the formats need 3).
pub const MAX_JSON_DEPTH: usize = 8;
/// Longest accepted relative path, in bytes.
pub const MAX_PATH_BYTES: usize = 1024;
/// Deepest folder nesting walked in a bundle or install tree.
pub const MAX_WALK_DEPTH: usize = 64;
/// Largest docs file scanned for remote resources (it is held in memory once digested).
pub const MAX_SCAN_BYTES: usize = 32 * 1024 * 1024;
/// Largest file size a document can state: 2^53 - 1, the largest exact JSON integer.
pub const MAX_SIZE: u64 = (1 << 53) - 1;
/// The listing file at the root of every bundle; it is never an entry of itself.
pub const BUNDLE_FILE: &str = "offline-bundle.json";
/// Where `plat-deploy bundle-schemas` writes the `offline-bundle.json` JSON Schema.
pub const BUNDLE_SCHEMA_PATH: &str = "platform/offline-bundle.schema.json";
/// Where `plat-deploy bundle-schemas` writes the `install-manifest.json` JSON Schema.
pub const INSTALL_SCHEMA_PATH: &str = "platform/install-manifest.schema.json";
/// JSON Schema dialect of the exports.
pub const JSON_SCHEMA_DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";
/// Pattern of bundle IDs.
pub const ID_PATTERN: &str = "^[a-z0-9][a-z0-9_.-]*$";
/// Pattern of release versions.
pub const RELEASE_PATTERN: &str = "^[0-9A-Za-z][0-9A-Za-z.+-]*$";
/// Schema pattern of relative paths (the reader also applies the rules a pattern cannot say).
pub const RELPATH_PATTERN: &str = r"^[^/\\:\u0000-\u001f]+(/[^/\\:\u0000-\u001f]+)*$";
/// Schema pattern a relative path must not match: a `.` or `..` segment.
pub const DOT_SEGMENT_PATTERN: &str = r"(^|/)\.{1,2}(/|$)";

const BUNDLE_KEYS: [&str; 6] = ["schema_version", "bundle_id", "release", "os", "arch", "entries"];
const ENTRY_KEYS: [&str; 4] = ["path", "role", "sha256", "size"];
const INSTALL_KEYS: [&str; 6] = ["schema_version", "release", "os", "arch", "kind", "files"];
const FILE_KEYS: [&str; 3] = ["relpath", "sha256", "size"];
const BUFFER_BYTES: usize = 64 * 1024;

/// What a bundled file is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Installer,
    Cli,
    Docs,
    RuntimeDep,
    InstallManifest,
    Licence,
}

impl Role {
    /// Every role, in declaration order.
    pub const ALL: &'static [Role] =
        &[Role::Installer, Role::Cli, Role::Docs, Role::RuntimeDep, Role::InstallManifest, Role::Licence];

    /// The name used in `offline-bundle.json`.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Installer => "installer",
            Role::Cli => "cli",
            Role::Docs => "docs",
            Role::RuntimeDep => "runtime_dep",
            Role::InstallManifest => "install_manifest",
            Role::Licence => "licence",
        }
    }

    /// Parses the name used in `offline-bundle.json`.
    pub fn from_name(s: &str) -> Option<Role> {
        Role::ALL.iter().copied().find(|r| r.as_str() == s)
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Install kind of an install manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InstallKind {
    PerUser,
    Machine,
}

impl InstallKind {
    /// Every kind, in declaration order. A bundle carries one install manifest per kind.
    pub const ALL: &'static [InstallKind] = &[InstallKind::PerUser, InstallKind::Machine];

    /// The name used in the manifest.
    pub fn as_str(self) -> &'static str {
        match self {
            InstallKind::PerUser => "per_user",
            InstallKind::Machine => "machine",
        }
    }

    /// Parses the name used in the manifest.
    pub fn from_name(s: &str) -> Option<InstallKind> {
        InstallKind::ALL.iter().copied().find(|k| k.as_str() == s)
    }
}

impl fmt::Display for InstallKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One file of an offline bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleEntry {
    /// Relative to the bundle root, `/`-separated.
    pub path: String,
    pub role: Role,
    pub sha256: Digest,
    pub size: u64,
}

/// `offline-bundle.json`, schema 1. Entries are strictly sorted by `path` in byte order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfflineBundle {
    pub schema_version: u32,
    pub bundle_id: String,
    pub release: String,
    pub os: OsFamily,
    pub arch: Arch,
    pub entries: Vec<BundleEntry>,
}

/// One installed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Relative to the install root, `/`-separated.
    pub relpath: String,
    pub sha256: Digest,
    pub size: u64,
}

/// `install-manifest.json`, schema 1. Files are strictly sorted by `relpath` in byte order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallManifest {
    pub schema_version: u32,
    pub release: String,
    pub os: OsFamily,
    pub arch: Arch,
    pub kind: InstallKind,
    pub files: Vec<FileEntry>,
}

/// A typed finding. `file` and `path` are relative to the bundle (or install) root, except
/// for errors about the package plan, which name the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleError {
    /// Not UTF-8, not JSON, not canonical, an unknown key, or a missing or unusable field.
    Parse { file: String, msg: String },
    /// `schema_version` is an integer other than [`SUPPORTED_SCHEMA`].
    UnsupportedSchema { file: String, found: i64 },
    /// More than [`MAX_BYTES`] bytes or [`MAX_ENTRIES`] entries (or files in a walked tree),
    /// or a docs file above [`MAX_SCAN_BYTES`].
    TooLarge { file: String },
    /// A path breaks the path rules, or a walked file is a symbolic link, not a regular file,
    /// not Unicode or too deep. `reason` is a stable code.
    BadPath { path: String, reason: &'static str },
    /// The plan requires a file of this role (named when the plan names it) and none is listed.
    MissingRole { role: Role, name: Option<String> },
    /// A listed file the plan does not call for in this role, or a second file where one is
    /// required.
    UnexpectedEntry { path: String, role: Role },
    /// The file's SHA-256 or size differs from its entry (REQ-002).
    DigestMismatch { path: String },
    /// Listed but not present.
    MissingFile { path: String },
    /// Present but not listed (REQ-002).
    Unlisted { path: String },
    /// A docs file makes a browser load `url` on its own (REQ-004).
    RemoteResource { file: String, url: String },
    /// A field disagrees with the bundle or the package plan.
    Mismatch { file: String, field: &'static str },
    /// The package plan lists no package for the bundle's OS and architecture.
    NotInPlan { os: OsFamily, arch: Arch },
    /// A Tier 1 combination of the plan has no bundle in the checked set (REQ-001).
    MissingBundle { os: OsFamily, arch: Arch },
    /// Two bundles of the checked set are for the same combination.
    DuplicateBundle { os: OsFamily, arch: Arch },
    /// A file could not be read.
    Io { path: String, msg: String },
}

impl BundleError {
    /// Stable code of the variant.
    pub fn code(&self) -> &'static str {
        match self {
            BundleError::Parse { .. } => "Parse",
            BundleError::UnsupportedSchema { .. } => "UnsupportedSchema",
            BundleError::TooLarge { .. } => "TooLarge",
            BundleError::BadPath { .. } => "BadPath",
            BundleError::MissingRole { .. } => "MissingRole",
            BundleError::UnexpectedEntry { .. } => "UnexpectedEntry",
            BundleError::DigestMismatch { .. } => "DigestMismatch",
            BundleError::MissingFile { .. } => "MissingFile",
            BundleError::Unlisted { .. } => "Unlisted",
            BundleError::RemoteResource { .. } => "RemoteResource",
            BundleError::Mismatch { .. } => "Mismatch",
            BundleError::NotInPlan { .. } => "NotInPlan",
            BundleError::MissingBundle { .. } => "MissingBundle",
            BundleError::DuplicateBundle { .. } => "DuplicateBundle",
            BundleError::Io { .. } => "Io",
        }
    }
}

/// Untrusted text is shown with Rust string escapes, so control bytes cannot reach a terminal.
impl fmt::Display for BundleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BundleError::Parse { file, msg } => write!(f, "Parse {{ file: {file:?}, msg: {msg:?} }}"),
            BundleError::UnsupportedSchema { file, found } => {
                write!(f, "UnsupportedSchema {{ file: {file:?}, found: {found} }}")
            }
            BundleError::TooLarge { file } => write!(f, "TooLarge {{ file: {file:?} }}"),
            BundleError::BadPath { path, reason } => write!(f, "BadPath {{ path: {path:?}, reason: {reason} }}"),
            BundleError::MissingRole { role, name: None } => write!(f, "MissingRole {{ role: {role} }}"),
            BundleError::MissingRole { role, name: Some(n) } => {
                write!(f, "MissingRole {{ role: {role}, name: {n:?} }}")
            }
            BundleError::UnexpectedEntry { path, role } => {
                write!(f, "UnexpectedEntry {{ path: {path:?}, role: {role} }}")
            }
            BundleError::DigestMismatch { path } => write!(f, "DigestMismatch {{ path: {path:?} }}"),
            BundleError::MissingFile { path } => write!(f, "MissingFile {{ path: {path:?} }}"),
            BundleError::Unlisted { path } => write!(f, "Unlisted {{ path: {path:?} }}"),
            BundleError::RemoteResource { file, url } => {
                write!(f, "RemoteResource {{ file: {file:?}, url: {url:?} }}")
            }
            BundleError::Mismatch { file, field } => write!(f, "Mismatch {{ file: {file:?}, field: {field} }}"),
            BundleError::NotInPlan { os, arch } => write!(f, "NotInPlan {{ os: {os}, arch: {arch} }}"),
            BundleError::MissingBundle { os, arch } => write!(f, "MissingBundle {{ os: {os}, arch: {arch} }}"),
            BundleError::DuplicateBundle { os, arch } => write!(f, "DuplicateBundle {{ os: {os}, arch: {arch} }}"),
            BundleError::Io { path, msg } => write!(f, "Io {{ path: {path:?}, msg: {msg:?} }}"),
        }
    }
}

// ------------------------------------------------------------------------------------- paths

/// Checks one relative path against the path rules (REQ-003); `Err` carries the reason code
/// (`empty`, `too_long`, `non_ascii`, `backslash`, `absolute`, `drive`, `dot_dot`,
/// `dot_segment`, `empty_segment`, `bad_name`). Names are ASCII only (maintainer decision on
/// PR #166): no Unicode normalisation or case-folding differences between file systems.
pub fn check_relpath(p: &str) -> Result<(), &'static str> {
    if p.is_empty() {
        return Err("empty");
    }
    if p.len() > MAX_PATH_BYTES {
        return Err("too_long");
    }
    if !p.is_ascii() {
        return Err("non_ascii");
    }
    if p.contains('\\') {
        return Err("backslash");
    }
    if p.starts_with('/') {
        return Err("absolute");
    }
    let b = p.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return Err("drive");
    }
    for seg in p.split('/') {
        match seg {
            ".." => return Err("dot_dot"),
            "." => return Err("dot_segment"),
            "" => return Err("empty_segment"),
            _ => {}
        }
    }
    // Per-segment rule shared with the import/export roots: no colons, control bytes,
    // dot-and-space names, trailing dots or spaces, or Windows device names.
    match RelPath::parse(p) {
        Ok(r) if r.components().len() == p.split('/').count() => Ok(()),
        _ => Err("bad_name"),
    }
}

/// Checks a whole sorted path list: each path valid, strictly increasing in byte order,
/// unique ASCII-case-insensitively, and never also a folder of another entry.
fn check_path_list<'a>(paths: impl Iterator<Item = &'a str>) -> Result<(), BundleError> {
    let mut prev: Option<&str> = None;
    let mut folded: BTreeSet<String> = BTreeSet::new();
    let mut files: BTreeSet<&str> = BTreeSet::new();
    for p in paths {
        check_relpath(p).map_err(|reason| BundleError::BadPath { path: p.to_string(), reason })?;
        if let Some(q) = prev {
            if p == q {
                return Err(BundleError::BadPath { path: p.to_string(), reason: "duplicate" });
            }
            if p < q {
                return Err(BundleError::BadPath { path: p.to_string(), reason: "not_sorted" });
            }
        }
        if !folded.insert(p.to_ascii_lowercase()) {
            return Err(BundleError::BadPath { path: p.to_string(), reason: "case_duplicate" });
        }
        // Every proper ancestor folder must not itself be a listed file. Sorted order puts
        // `a` before `a/b`, so checking ancestors of later paths is enough.
        let mut cut = p.len();
        while let Some(i) = p.get(..cut).and_then(|s| s.rfind('/')) {
            if p.get(..i).is_some_and(|a| files.contains(a)) {
                return Err(BundleError::BadPath { path: p.to_string(), reason: "file_and_dir" });
            }
            cut = i;
        }
        files.insert(p);
        prev = Some(p);
    }
    Ok(())
}

// ----------------------------------------------------------------------------------- reading

fn parse_err(file: &str, msg: impl Into<String>) -> BundleError {
    BundleError::Parse { file: file.to_string(), msg: msg.into() }
}

/// Parses a capped, canonical JSON object and checks `schema_version` and the key set.
fn read_top(bytes: &[u8], file: &str, keys: &[&str]) -> Result<BTreeMap<String, CValue>, BundleError> {
    if bytes.len() > MAX_BYTES {
        return Err(BundleError::TooLarge { file: file.to_string() });
    }
    let root = fid_canon::parse_with_limits(bytes, MAX_BYTES, MAX_JSON_DEPTH).map_err(|e| match e {
        CanonError::InputLimit => parse_err(file, "nesting deeper than allowed"),
        e => parse_err(file, format!("not a JSON document: {}", e.code())),
    })?;
    let CValue::Obj(top) = root else {
        return Err(parse_err(file, "the document is not an object"));
    };
    match top.get("schema_version") {
        Some(CValue::Num(n)) if *n == f64::from(SUPPORTED_SCHEMA) => {}
        Some(CValue::Num(n)) if n.fract() == 0.0 && n.abs() < 9.0e15 => {
            // Integral and below 2^53 in magnitude, so the conversion is exact.
            return Err(BundleError::UnsupportedSchema { file: file.to_string(), found: *n as i64 });
        }
        Some(_) => return Err(parse_err(file, "schema_version is not an integer")),
        None => return Err(parse_err(file, "schema_version is missing")),
    }
    for k in keys {
        if !top.contains_key(*k) {
            return Err(parse_err(file, format!("{k} is missing")));
        }
    }
    if let Some(k) = top.keys().find(|k| !keys.contains(&k.as_str())) {
        return Err(parse_err(file, format!("unknown field {k:?}")));
    }
    let canonical = fid_canon::to_canonical_bytes(&CValue::Obj(top.clone()))
        .map_err(|e| parse_err(file, format!("cannot canonicalise: {}", e.code())))?;
    if canonical != bytes {
        return Err(parse_err(file, "not RFC 8785 canonical JSON"));
    }
    Ok(top)
}

fn get<'a>(m: &'a BTreeMap<String, CValue>, key: &str, at: &str, file: &str) -> Result<&'a CValue, BundleError> {
    m.get(key).ok_or_else(|| parse_err(file, format!("{key} is missing in {at}")))
}

fn string<'a>(m: &'a BTreeMap<String, CValue>, key: &str, at: &str, file: &str) -> Result<&'a str, BundleError> {
    match get(m, key, at, file)? {
        CValue::Str(s) if !s.is_empty() => Ok(s.as_str()),
        _ => Err(parse_err(file, format!("{key} in {at} is not a non-empty string"))),
    }
}

fn matches_id(s: &str) -> bool {
    let b = s.as_bytes();
    s.len() <= 128
        && b.first().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'.' | b'-'))
}

fn matches_release(s: &str) -> bool {
    let b = s.as_bytes();
    s.len() <= 64
        && b.first().is_some_and(u8::is_ascii_alphanumeric)
        && b.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'+' | b'-'))
}

fn release(m: &BTreeMap<String, CValue>, key: &str, at: &str, file: &str) -> Result<String, BundleError> {
    let s = string(m, key, at, file)?;
    if !matches_release(s) {
        return Err(parse_err(file, format!("{key} in {at} does not match {RELEASE_PATTERN} (at most 64 bytes)")));
    }
    Ok(s.to_string())
}

fn os_arch(m: &BTreeMap<String, CValue>, at: &str, file: &str) -> Result<(OsFamily, Arch), BundleError> {
    let os = OsFamily::from_name(string(m, "os", at, file)?)
        .ok_or_else(|| parse_err(file, format!("os in {at} is not a known OS family")))?;
    let arch = Arch::from_name(string(m, "arch", at, file)?)
        .ok_or_else(|| parse_err(file, format!("arch in {at} is not a known architecture")))?;
    Ok((os, arch))
}

fn digest(m: &BTreeMap<String, CValue>, at: &str, file: &str) -> Result<Digest, BundleError> {
    Digest::parse(string(m, "sha256", at, file)?)
        .map_err(|_| parse_err(file, format!("sha256 in {at} is not sha256:<64 lowercase hex digits>")))
}

fn size(m: &BTreeMap<String, CValue>, at: &str, file: &str) -> Result<u64, BundleError> {
    match get(m, "size", at, file)? {
        // Integral and within 0..=2^53-1, so the conversion is exact.
        CValue::Num(n) if n.fract() == 0.0 && *n >= 0.0 && *n <= MAX_SIZE as f64 => Ok(*n as u64),
        _ => Err(parse_err(file, format!("size in {at} is not an integer from 0 to {MAX_SIZE}"))),
    }
}

fn list<'a>(top: &'a BTreeMap<String, CValue>, key: &str, file: &str) -> Result<&'a [CValue], BundleError> {
    match get(top, key, "the document", file)? {
        CValue::Arr(a) if a.len() > MAX_ENTRIES => Err(BundleError::TooLarge { file: file.to_string() }),
        CValue::Arr(a) => Ok(a),
        _ => Err(parse_err(file, format!("{key} is not an array"))),
    }
}

fn entry_obj<'a>(
    v: &'a CValue,
    keys: &[&str],
    at: &str,
    file: &str,
) -> Result<&'a BTreeMap<String, CValue>, BundleError> {
    let CValue::Obj(m) = v else {
        return Err(parse_err(file, format!("{at} is not an object")));
    };
    if let Some(k) = m.keys().find(|k| !keys.contains(&k.as_str())) {
        return Err(parse_err(file, format!("unknown field {k:?} in {at}")));
    }
    Ok(m)
}

/// Parses `offline-bundle.json`. `file` names the document in errors.
pub fn parse_bundle(bytes: &[u8], file: &str) -> Result<OfflineBundle, BundleError> {
    let top = read_top(bytes, file, &BUNDLE_KEYS)?;
    let at = "the document";
    let bundle_id = string(&top, "bundle_id", at, file)?;
    if !matches_id(bundle_id) {
        return Err(parse_err(file, format!("bundle_id does not match {ID_PATTERN} (at most 128 bytes)")));
    }
    let release = release(&top, "release", at, file)?;
    let (os, arch) = os_arch(&top, at, file)?;
    let mut entries = Vec::new();
    for (i, v) in list(&top, "entries", file)?.iter().enumerate() {
        let at = format!("entries[{i}]");
        let m = entry_obj(v, &ENTRY_KEYS, &at, file)?;
        let path = match get(m, "path", &at, file)? {
            CValue::Str(s) => s.clone(),
            _ => return Err(parse_err(file, format!("path in {at} is not a string"))),
        };
        let role = Role::from_name(string(m, "role", &at, file)?)
            .ok_or_else(|| parse_err(file, format!("role in {at} is not a known role")))?;
        entries.push(BundleEntry { role, sha256: digest(m, &at, file)?, size: size(m, &at, file)?, path });
    }
    check_path_list(entries.iter().map(|e| e.path.as_str()))?;
    if let Some(e) = entries.iter().find(|e| e.path == BUNDLE_FILE) {
        return Err(BundleError::BadPath { path: e.path.clone(), reason: "reserved" });
    }
    Ok(OfflineBundle { schema_version: SUPPORTED_SCHEMA, bundle_id: bundle_id.to_string(), release, os, arch, entries })
}

/// Parses `install-manifest.json` (REQ-003, REQ-005, REQ-006). `file` names the document in
/// errors.
pub fn parse_install_manifest(bytes: &[u8], file: &str) -> Result<InstallManifest, BundleError> {
    let top = read_top(bytes, file, &INSTALL_KEYS)?;
    let at = "the document";
    let release = release(&top, "release", at, file)?;
    let (os, arch) = os_arch(&top, at, file)?;
    let kind = InstallKind::from_name(string(&top, "kind", at, file)?)
        .ok_or_else(|| parse_err(file, "kind is not per_user or machine"))?;
    let mut files = Vec::new();
    for (i, v) in list(&top, "files", file)?.iter().enumerate() {
        let at = format!("files[{i}]");
        let m = entry_obj(v, &FILE_KEYS, &at, file)?;
        let relpath = match get(m, "relpath", &at, file)? {
            CValue::Str(s) => s.clone(),
            _ => return Err(parse_err(file, format!("relpath in {at} is not a string"))),
        };
        files.push(FileEntry { sha256: digest(m, &at, file)?, size: size(m, &at, file)?, relpath });
    }
    check_path_list(files.iter().map(|f| f.relpath.as_str()))?;
    Ok(InstallManifest { schema_version: SUPPORTED_SCHEMA, release, os, arch, kind, files })
}

// ----------------------------------------------------------------------------------- writing

fn s(v: &str) -> CValue {
    CValue::Str(v.to_string())
}

fn num(v: f64) -> CValue {
    CValue::Num(v)
}

fn object<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// A size as a JSON number. Callers check `v <= MAX_SIZE` first ([`check_sizes`]), so the
/// conversion is exact.
fn size_value(v: u64) -> CValue {
    num(v as f64)
}

/// Sizes above [`MAX_SIZE`] cannot be written exactly as JSON numbers: an error, never a clamp.
fn check_sizes<'a>(file: &str, sizes: impl Iterator<Item = (&'a str, u64)>) -> Result<(), BundleError> {
    for (path, n) in sizes {
        if n > MAX_SIZE {
            return Err(parse_err(file, format!("size of {path:?} is above {MAX_SIZE}")));
        }
    }
    Ok(())
}

impl OfflineBundle {
    /// The document as a JSON value.
    pub fn to_value(&self) -> CValue {
        let entries = self
            .entries
            .iter()
            .map(|e| {
                object([
                    ("path", s(&e.path)),
                    ("role", s(e.role.as_str())),
                    ("sha256", s(e.sha256.as_str())),
                    ("size", size_value(e.size)),
                ])
            })
            .collect();
        object([
            ("schema_version", num(f64::from(self.schema_version))),
            ("bundle_id", s(&self.bundle_id)),
            ("release", s(&self.release)),
            ("os", s(self.os.as_str())),
            ("arch", s(self.arch.as_str())),
            ("entries", CValue::Arr(entries)),
        ])
    }

    /// RFC 8785 canonical bytes, checked with [`parse_bundle`] before they are returned.
    pub fn to_json(&self) -> Result<Vec<u8>, BundleError> {
        check_sizes(BUNDLE_FILE, self.entries.iter().map(|e| (e.path.as_str(), e.size)))?;
        let bytes = fid_canon::to_canonical_bytes(&self.to_value())
            .map_err(|e| parse_err(BUNDLE_FILE, format!("cannot serialise: {}", e.code())))?;
        parse_bundle(&bytes, BUNDLE_FILE)?;
        Ok(bytes)
    }
}

impl InstallManifest {
    /// The document as a JSON value.
    pub fn to_value(&self) -> CValue {
        let files = self
            .files
            .iter()
            .map(|f| {
                object([("relpath", s(&f.relpath)), ("sha256", s(f.sha256.as_str())), ("size", size_value(f.size))])
            })
            .collect();
        object([
            ("schema_version", num(f64::from(self.schema_version))),
            ("release", s(&self.release)),
            ("os", s(self.os.as_str())),
            ("arch", s(self.arch.as_str())),
            ("kind", s(self.kind.as_str())),
            ("files", CValue::Arr(files)),
        ])
    }

    /// RFC 8785 canonical bytes (no trailing newline), checked with [`parse_install_manifest`]
    /// before they are returned: the same manifest always gives the same bytes (D0).
    pub fn to_json(&self) -> Result<Vec<u8>, BundleError> {
        let file = "install-manifest.json";
        check_sizes(file, self.files.iter().map(|f| (f.relpath.as_str(), f.size)))?;
        let bytes = fid_canon::to_canonical_bytes(&self.to_value())
            .map_err(|e| parse_err(file, format!("cannot serialise: {}", e.code())))?;
        parse_install_manifest(&bytes, file)?;
        Ok(bytes)
    }
}

// ------------------------------------------------------------------------------ walk and hash

/// Lists every regular file below `root` as (relative `/` path, full path), sorted by the
/// relative path in byte order. Symbolic links (and Windows junctions) are reported, never
/// followed; so is anything that is neither a regular file nor a folder.
pub fn walk(root: &Path) -> Result<Vec<(String, PathBuf)>, Vec<BundleError>> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    let mut errors = Vec::new();
    let mut stack: Vec<(PathBuf, String, usize)> = vec![(root.to_path_buf(), String::new(), 0)];
    while let Some((dir, prefix, depth)) = stack.pop() {
        let shown = if prefix.is_empty() { ".".to_string() } else { prefix.clone() };
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) => {
                errors.push(BundleError::Io { path: shown, msg: e.to_string() });
                continue;
            }
        };
        for item in rd {
            let item = match item {
                Ok(i) => i,
                Err(e) => {
                    errors.push(BundleError::Io { path: shown.clone(), msg: e.to_string() });
                    continue;
                }
            };
            let name = item.file_name();
            let Some(name) = name.to_str() else {
                let lossy = format!("{prefix}{}", item.file_name().to_string_lossy());
                errors.push(BundleError::BadPath { path: lossy, reason: "not_unicode" });
                continue;
            };
            let rel = format!("{prefix}{name}");
            if !name.is_ascii() {
                errors.push(BundleError::BadPath { path: rel, reason: "non_ascii" });
                continue;
            }
            let ft = match item.file_type() {
                Ok(t) => t,
                Err(e) => {
                    errors.push(BundleError::Io { path: rel, msg: e.to_string() });
                    continue;
                }
            };
            if ft.is_symlink() {
                errors.push(BundleError::BadPath { path: rel, reason: "symlink" });
            } else if ft.is_dir() {
                if depth + 1 >= MAX_WALK_DEPTH {
                    errors.push(BundleError::BadPath { path: rel, reason: "too_deep" });
                } else {
                    stack.push((item.path(), format!("{rel}/"), depth + 1));
                }
            } else if ft.is_file() {
                if out.len() >= MAX_ENTRIES {
                    errors.push(BundleError::TooLarge { file: shown });
                    return Err(errors);
                }
                out.push((rel, item.path()));
            } else {
                errors.push(BundleError::BadPath { path: rel, reason: "not_regular" });
            }
        }
    }
    out.sort();
    errors.sort_by_key(|e| e.to_string());
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

/// Opens a regular file without following a link that replaced it after the walk.
///
/// Windows: `FILE_FLAG_OPEN_REPARSE_POINT` opens a symbolic link or junction itself, never its
/// target, and the handle's type is checked. Unix: the path is checked with `lstat` before
/// opening and the opened file must have the same device and inode, so a link swapped in
/// between is refused rather than followed. Either way the handle must be a regular file.
fn open_no_follow(path: &Path, rel: &str) -> Result<File, BundleError> {
    let io = |e: std::io::Error| BundleError::Io { path: rel.to_string(), msg: e.to_string() };
    let bad = |reason: &'static str| BundleError::BadPath { path: rel.to_string(), reason };
    #[cfg(unix)]
    let before = {
        let m = std::fs::symlink_metadata(path).map_err(io)?;
        if m.file_type().is_symlink() {
            return Err(bad("symlink"));
        }
        m
    };
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT
        opts.custom_flags(0x0020_0000);
    }
    let file = opts.open(path).map_err(io)?;
    let after = file.metadata().map_err(io)?;
    if after.file_type().is_symlink() {
        return Err(bad("symlink"));
    }
    if !after.is_file() {
        return Err(bad("not_regular"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (before.dev(), before.ino()) != (after.dev(), after.ino()) {
            return Err(bad("changed"));
        }
    }
    Ok(file)
}

/// SHA-256 and length of a file, streamed. With `keep`, the bytes are also returned (up to
/// [`MAX_SCAN_BYTES`]; a larger file is `TooLarge`), so a caller uses exactly the bytes it
/// digested and never reopens the file.
fn hash_file(path: &Path, rel: &str, keep: bool) -> Result<(Digest, u64, Option<Vec<u8>>), BundleError> {
    let io = |e: std::io::Error| BundleError::Io { path: rel.to_string(), msg: e.to_string() };
    let mut file = open_no_follow(path, rel)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; BUFFER_BYTES];
    let mut size: u64 = 0;
    let mut kept: Option<Vec<u8>> = if keep { Some(Vec::new()) } else { None };
    loop {
        let n = match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(io(e)),
        };
        let chunk = buf.get(..n).unwrap_or_default();
        hasher.update(chunk);
        size = size.saturating_add(n as u64);
        if let Some(k) = kept.as_mut() {
            if k.len() + chunk.len() > MAX_SCAN_BYTES {
                return Err(BundleError::TooLarge { file: rel.to_string() });
            }
            k.extend_from_slice(chunk);
        }
    }
    Ok((Digest::from_bytes(hasher.finalize().into()), size, kept))
}

/// Builds the install manifest of the tree at `root` (`xtask plat-deploy
/// make-install-manifest`): every regular file, sorted, digested. The same tree always gives
/// the same manifest and, through [`InstallManifest::to_json`], the same bytes (AC-6).
pub fn make_install_manifest(
    root: &Path,
    kind: InstallKind,
    release: &str,
    os: OsFamily,
    arch: Arch,
) -> Result<InstallManifest, Vec<BundleError>> {
    if !matches_release(release) {
        return Err(vec![parse_err("--release", format!("does not match {RELEASE_PATTERN}"))]);
    }
    let listed = walk(root)?;
    check_path_list(listed.iter().map(|(r, _)| r.as_str())).map_err(|e| vec![e])?;
    let mut files = Vec::with_capacity(listed.len());
    let mut errors = Vec::new();
    for (rel, full) in listed {
        match hash_file(&full, &rel, false) {
            Ok((sha256, size, _)) if size <= MAX_SIZE => files.push(FileEntry { relpath: rel, sha256, size }),
            Ok(_) => errors.push(BundleError::TooLarge { file: rel }),
            Err(e) => errors.push(e),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(InstallManifest { schema_version: SUPPORTED_SCHEMA, release: release.to_string(), os, arch, kind, files })
}

// ------------------------------------------------------------------------------- package plan

/// Package kind in the package plan (`ECO.REL.PKG.PLAN`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageKind {
    App,
    Cli,
    Verify,
}

/// One package of the plan (the fields this check reads).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanPackage {
    pub package: String,
    pub kind: PackageKind,
    pub os: OsFamily,
    pub arch: Arch,
    /// The combination's support tier, as the plan states it.
    pub tier: Tier,
    /// The artefact file name; required for `app` packages.
    pub file_name: Option<String>,
}

/// A runtime dependency the plan names for one OS and architecture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDep {
    pub os: OsFamily,
    pub arch: Arch,
    pub file_name: String,
}

/// The package plan fields this check reads. Other fields belong to `ECO.REL.PKG.PLAN` and
/// are ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagePlan {
    pub version: String,
    pub packages: Vec<PlanPackage>,
    pub runtime_deps: Vec<RuntimeDep>,
}

fn plain_file_name(s: &str) -> bool {
    !s.contains('/') && check_relpath(s).is_ok()
}

/// Parses the package plan: `version`, `packages[] { package, kind, os, arch, tier, file_name }`
/// and the optional `runtime_deps[] { os, arch, file_name }`. Bounded like the manifests;
/// unknown fields are ignored because the plan's schema is owned by `ECO.REL.PKG.PLAN`.
pub fn parse_plan(bytes: &[u8], file: &str) -> Result<PackagePlan, BundleError> {
    if bytes.len() > MAX_BYTES {
        return Err(BundleError::TooLarge { file: file.to_string() });
    }
    let root = fid_canon::parse_with_limits(bytes, MAX_BYTES, MAX_JSON_DEPTH)
        .map_err(|e| parse_err(file, format!("not a JSON document: {}", e.code())))?;
    let CValue::Obj(top) = root else {
        return Err(parse_err(file, "the plan is not an object"));
    };
    let at = "the plan";
    let version = release(&top, "version", at, file)?;
    let mut packages = Vec::new();
    for (i, v) in list(&top, "packages", file)?.iter().enumerate() {
        let at = format!("packages[{i}]");
        let CValue::Obj(m) = v else {
            return Err(parse_err(file, format!("{at} is not an object")));
        };
        let kind = match string(m, "kind", &at, file)? {
            "app" => PackageKind::App,
            "cli" => PackageKind::Cli,
            "verify" => PackageKind::Verify,
            _ => return Err(parse_err(file, format!("kind in {at} is not app, cli or verify"))),
        };
        let (os, arch) = os_arch(m, &at, file)?;
        let tier = Tier::from_name(string(m, "tier", &at, file)?)
            .ok_or_else(|| parse_err(file, format!("tier in {at} is not tier1, tier2 or tier3")))?;
        let file_name = match m.get("file_name") {
            None => None,
            Some(CValue::Str(n)) if plain_file_name(n) => Some(n.clone()),
            Some(_) => return Err(parse_err(file, format!("file_name in {at} is not a plain file name"))),
        };
        if kind == PackageKind::App && file_name.is_none() {
            return Err(parse_err(file, format!("{at} is an app package without file_name")));
        }
        let package = string(m, "package", &at, file)?.to_string();
        packages.push(PlanPackage { package, kind, os, arch, tier, file_name });
    }
    let mut runtime_deps = Vec::new();
    if top.contains_key("runtime_deps") {
        for (i, v) in list(&top, "runtime_deps", file)?.iter().enumerate() {
            let at = format!("runtime_deps[{i}]");
            let CValue::Obj(m) = v else {
                return Err(parse_err(file, format!("{at} is not an object")));
            };
            let (os, arch) = os_arch(m, &at, file)?;
            let name = string(m, "file_name", &at, file)?;
            if !plain_file_name(name) {
                return Err(parse_err(file, format!("file_name in {at} is not a plain file name")));
            }
            runtime_deps.push(RuntimeDep { os, arch, file_name: name.to_string() });
        }
    }
    Ok(PackagePlan { version, packages, runtime_deps })
}

// ------------------------------------------------------------------------------ bundle check

fn base_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The plan's content rules for the bundle's OS and architecture (REQ-001): one `installer`
/// per app package (matched by file name), exactly one `cli`, a docs tree with an HTML entry
/// point (`index.html`, `index.htm` or `index.xhtml`, any folder) and every named
/// `runtime_dep`; nothing else in those roles. Install manifests are counted per
/// kind by [`check_bundle`] once they are parsed.
pub fn check_roles(bundle: &OfflineBundle, plan: &PackagePlan) -> Vec<BundleError> {
    let mut errors = Vec::new();
    let (os, arch) = (bundle.os, bundle.arch);
    let pkgs: Vec<&PlanPackage> = plan.packages.iter().filter(|p| p.os == os && p.arch == arch).collect();
    if pkgs.is_empty() {
        errors.push(BundleError::NotInPlan { os, arch });
        return errors;
    }
    if bundle.release != plan.version {
        errors.push(BundleError::Mismatch { file: BUNDLE_FILE.to_string(), field: "release" });
    }
    let of_role = |role: Role| bundle.entries.iter().filter(move |e| e.role == role);

    // Installers: one per app package, by file name.
    let mut wanted: BTreeMap<&str, usize> = BTreeMap::new();
    for p in pkgs.iter().filter(|p| p.kind == PackageKind::App) {
        if let Some(n) = &p.file_name {
            wanted.insert(n.as_str(), 0);
        }
    }
    named_role(of_role(Role::Installer), Role::Installer, &mut wanted, &mut errors);

    // Runtime dependencies: every one the plan names, by file name.
    let mut deps: BTreeMap<&str, usize> =
        plan.runtime_deps.iter().filter(|d| d.os == os && d.arch == arch).map(|d| (d.file_name.as_str(), 0)).collect();
    named_role(of_role(Role::RuntimeDep), Role::RuntimeDep, &mut deps, &mut errors);

    // Exactly one CLI entry.
    let mut cli = of_role(Role::Cli);
    if cli.next().is_none() {
        errors.push(BundleError::MissingRole { role: Role::Cli, name: None });
    }
    for extra in cli {
        errors.push(BundleError::UnexpectedEntry { path: extra.path.clone(), role: Role::Cli });
    }

    // The docs tree: at least one file, and an HTML entry point.
    if of_role(Role::Docs).next().is_none() {
        errors.push(BundleError::MissingRole { role: Role::Docs, name: None });
    } else if !of_role(Role::Docs).any(|e| is_entry_point(&e.path)) {
        errors.push(BundleError::MissingRole { role: Role::Docs, name: Some("index.html".to_string()) });
    }
    errors
}

fn is_entry_point(path: &str) -> bool {
    let name = base_name(path).to_ascii_lowercase();
    matches!(name.as_str(), "index.html" | "index.htm" | "index.xhtml")
}

fn named_role<'a>(
    entries: impl Iterator<Item = &'a BundleEntry>,
    role: Role,
    wanted: &mut BTreeMap<&str, usize>,
    errors: &mut Vec<BundleError>,
) {
    for e in entries {
        match wanted.get_mut(base_name(&e.path)) {
            Some(n) if *n == 0 => *n = 1,
            _ => errors.push(BundleError::UnexpectedEntry { path: e.path.clone(), role }),
        }
    }
    for (name, n) in wanted.iter() {
        if *n == 0 {
            errors.push(BundleError::MissingRole { role, name: Some((*name).to_string()) });
        }
    }
}

/// Docs files scanned for remote resources, by extension (ASCII case-insensitive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocsKind {
    Html,
    Svg,
    Css,
    Js,
}

impl DocsKind {
    /// The scan kind of a docs path, or `None` for files that cannot load anything.
    pub fn of(path: &str) -> Option<DocsKind> {
        let ext = base_name(path).rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
        match ext.as_str() {
            "html" | "htm" | "xhtml" => Some(DocsKind::Html),
            "svg" => Some(DocsKind::Svg),
            "css" => Some(DocsKind::Css),
            "js" | "mjs" => Some(DocsKind::Js),
            _ => None,
        }
    }
}

/// Checks a bundle directory against its `offline-bundle.json` and the package plan.
/// Returns every finding, in a fixed order; empty means the bundle passes (AC-1).
///
/// Order of work: parse the listing; walk the tree (no link following); digest every file,
/// keeping the bytes of install manifests and scannable docs files; compare (missing,
/// unlisted, digest or size mismatch); apply the plan's role rules; then parse only the
/// install manifests and scan only the docs files whose digests matched.
pub fn check_bundle(dir: &Path, plan: &PackagePlan) -> Vec<BundleError> {
    let bundle = match read_listing(dir) {
        Ok(b) => b,
        Err(e) => return vec![e],
    };
    let mut errors = Vec::new();
    let on_disk: BTreeMap<String, PathBuf> = match walk(dir) {
        Ok(files) => files.into_iter().collect(),
        Err(e) => {
            errors.extend(e);
            return errors;
        }
    };

    // Present but unlisted.
    let listed: BTreeSet<&str> = bundle.entries.iter().map(|e| e.path.as_str()).collect();
    for rel in on_disk.keys() {
        if rel != BUNDLE_FILE && !listed.contains(rel.as_str()) {
            errors.push(BundleError::Unlisted { path: rel.clone() });
        }
    }

    // Listed: present, same digest and size. Keep verified bytes of the files used later.
    let mut verified: Vec<(&BundleEntry, Vec<u8>)> = Vec::new();
    for e in &bundle.entries {
        let Some(full) = on_disk.get(&e.path) else {
            errors.push(BundleError::MissingFile { path: e.path.clone() });
            continue;
        };
        let keep = match e.role {
            Role::InstallManifest => true,
            Role::Docs => DocsKind::of(&e.path).is_some(),
            _ => false,
        };
        match hash_file(full, &e.path, keep) {
            Ok((d, n, kept)) if d == e.sha256 && n == e.size => {
                if let Some(k) = kept {
                    verified.push((e, k));
                }
            }
            Ok(_) => errors.push(BundleError::DigestMismatch { path: e.path.clone() }),
            Err(err) => errors.push(err),
        }
    }

    errors.extend(check_roles(&bundle, plan));

    // Install manifests: one per kind, matching the bundle.
    let mut kinds: BTreeMap<InstallKind, usize> = BTreeMap::new();
    for (e, data) in verified.iter().filter(|(e, _)| e.role == Role::InstallManifest) {
        match parse_install_manifest(data, &e.path) {
            Ok(m) => {
                for (field, same) in [
                    ("release", m.release == bundle.release),
                    ("os", m.os == bundle.os),
                    ("arch", m.arch == bundle.arch),
                ] {
                    if !same {
                        errors.push(BundleError::Mismatch { file: e.path.clone(), field });
                    }
                }
                let n = kinds.entry(m.kind).or_insert(0);
                *n += 1;
                if *n > 1 {
                    errors.push(BundleError::UnexpectedEntry { path: e.path.clone(), role: Role::InstallManifest });
                }
            }
            Err(err) => errors.push(err),
        }
    }
    for kind in InstallKind::ALL {
        if !kinds.contains_key(kind) {
            errors
                .push(BundleError::MissingRole { role: Role::InstallManifest, name: Some(kind.as_str().to_string()) });
        }
    }

    // Docs: no auto-loaded remote resource (REQ-004).
    for (e, data) in verified.iter().filter(|(e, _)| e.role == Role::Docs) {
        for url in scan_docs_file(&e.path, data) {
            errors.push(BundleError::RemoteResource { file: e.path.clone(), url });
        }
    }
    errors
}

/// A finding of [`check_bundles`]: `bundle` names the bundle directory, or is `None` for a
/// finding about the set (coverage).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetFinding {
    pub bundle: Option<PathBuf>,
    pub error: BundleError,
}

impl fmt::Display for SetFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.bundle {
            Some(b) => write!(f, "{}: {}", b.display(), self.error),
            None => write!(f, "{}", self.error),
        }
    }
}

/// Reads and parses a bundle's `offline-bundle.json` (capped at [`MAX_BYTES`]).
pub fn read_listing(dir: &Path) -> Result<OfflineBundle, BundleError> {
    let mut bytes = Vec::new();
    let read = open_no_follow(&dir.join(BUNDLE_FILE), BUNDLE_FILE).and_then(|f| {
        f.take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| BundleError::Io { path: BUNDLE_FILE.to_string(), msg: e.to_string() })
    });
    read?;
    parse_bundle(&bytes, BUNDLE_FILE)
}

/// The Tier 1 (os, arch) combinations of the plan, sorted.
pub fn tier1_combinations(plan: &PackagePlan) -> BTreeSet<(OsFamily, Arch)> {
    plan.packages.iter().filter(|p| p.tier == Tier::Tier1).map(|p| (p.os, p.arch)).collect()
}

/// Checks a release's set of bundles (REQ-001): every bundle with [`check_bundle`], then one
/// bundle per Tier 1 combination of the plan (`MissingBundle`) and at most one per combination
/// (`DuplicateBundle`). Bundles for Tier 2 combinations are checked but not required.
pub fn check_bundles(dirs: &[PathBuf], plan: &PackagePlan) -> Vec<SetFinding> {
    let mut out = Vec::new();
    let mut seen: BTreeSet<(OsFamily, Arch)> = BTreeSet::new();
    for dir in dirs {
        for error in check_bundle(dir, plan) {
            out.push(SetFinding { bundle: Some(dir.clone()), error });
        }
        if let Ok(b) = read_listing(dir) {
            if !seen.insert((b.os, b.arch)) {
                out.push(SetFinding { bundle: None, error: BundleError::DuplicateBundle { os: b.os, arch: b.arch } });
            }
        }
    }
    for (os, arch) in tier1_combinations(plan) {
        if !seen.contains(&(os, arch)) {
            out.push(SetFinding { bundle: None, error: BundleError::MissingBundle { os, arch } });
        }
    }
    out
}

// -------------------------------------------------------------------------------- docs scan

/// The remote URLs a browser would load on its own from one docs file, in order of first
/// appearance, without duplicates. HTML and SVG: `src`, `href`, `xlink:href`, `poster` and
/// `data` of `link`, `script`, `img`, `iframe`, `frame`, `embed`, `source`, `video`, `audio`,
/// `track`, `object`, `input`, `base`, `image`, `use` and `feimage`; `srcset` and `imagesrcset`
/// candidates; the URL of `<meta http-equiv=refresh>`; CSS in `<style>` elements and `style`
/// attributes; JS in `<script>` elements. CSS: `url(...)`, `@import` and the quoted strings of
/// `image-set(...)`, outside comments. JS: any string literal that starts with `http://`,
/// `https://` or `//host` (conservative: JS can load from any string). HTML comments are
/// skipped. Files of other kinds give nothing.
pub fn scan_docs_file(path: &str, bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    match DocsKind::of(path) {
        Some(DocsKind::Html | DocsKind::Svg) => scan_html(&text, &mut out),
        Some(DocsKind::Css) => scan_css(&text, &mut out),
        Some(DocsKind::Js) => scan_js(&text, &mut out),
        None => {}
    }
    let mut seen = BTreeSet::new();
    out.retain(|u| seen.insert(u.clone()));
    out
}

/// Whether a URL as written would be fetched from the network: `http:`, `https:` or
/// protocol-relative `//`, after the clean-up browsers apply (outer whitespace and control
/// bytes trimmed, tabs and newlines removed, `\` read as `/`).
pub fn is_remote(url: &str) -> bool {
    let cleaned: String = url
        .trim_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .map(|c| if c == '\\' { '/' } else { c.to_ascii_lowercase() })
        .collect();
    cleaned.starts_with("http:") || cleaned.starts_with("https:") || cleaned.starts_with("//")
}

fn find(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || from >= hay.len() {
        return None;
    }
    hay.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

fn push_if_remote(url: &str, out: &mut Vec<String>) {
    if is_remote(url) {
        out.push(url.trim_matches(|c: char| c <= ' ').to_string());
    }
}

const LOADING_TAGS: [&str; 16] = [
    "link", "script", "img", "iframe", "frame", "embed", "source", "video", "audio", "track", "object", "input",
    "base", "image", "use", "feimage",
];
const LOADING_ATTRS: [&str; 5] = ["src", "href", "xlink:href", "poster", "data"];
const SRCSET_ATTRS: [&str; 2] = ["srcset", "imagesrcset"];

/// The URL of a refresh `content` value such as `0; url=https://x`, if any.
fn refresh_url(content: &str) -> Option<&str> {
    let (_, rest) = content.split_once([';', ','])?;
    let rest = rest.trim_start();
    let rest = match rest.get(..3) {
        Some(u) if u.eq_ignore_ascii_case("url") => rest.get(3..)?.trim_start().strip_prefix('=')?.trim_start(),
        _ => rest,
    };
    Some(rest.trim_matches(|c: char| c == '"' || c == '\'' || c <= ' '))
}

fn scan_html(text: &str, out: &mut Vec<String>) {
    let lower = text.to_ascii_lowercase();
    let lb = lower.as_bytes();
    let len = lb.len();
    let mut i = 0;
    while let Some(lt) = find(lb, i, b"<") {
        i = lt + 1;
        if lb.get(i..).is_some_and(|r| r.starts_with(b"!--")) {
            i = find(lb, i + 3, b"-->").map_or(len, |e| e + 3);
            continue;
        }
        let name_start = i;
        while i < len && lb[i].is_ascii_alphanumeric() {
            i += 1;
        }
        let name = lower.get(name_start..i).unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let mut attrs: Vec<(&str, &str)> = Vec::new();
        loop {
            while i < len && (lb[i].is_ascii_whitespace() || lb[i] == b'/') {
                i += 1;
            }
            if i >= len {
                break;
            }
            if lb[i] == b'>' {
                i += 1;
                break;
            }
            let an = i;
            while i < len && !lb[i].is_ascii_whitespace() && !matches!(lb[i], b'=' | b'>' | b'/') {
                i += 1;
            }
            if i == an {
                // A stray `=`: skip it so the loop always advances.
                i += 1;
                continue;
            }
            let attr = lower.get(an..i).unwrap_or("");
            while i < len && lb[i].is_ascii_whitespace() {
                i += 1;
            }
            let mut value = "";
            if i < len && lb[i] == b'=' {
                i += 1;
                while i < len && lb[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < len && (lb[i] == b'"' || lb[i] == b'\'') {
                    let q = lb[i];
                    let end = find(lb, i + 1, &[q]).unwrap_or(len);
                    value = text.get(i + 1..end).unwrap_or("");
                    i = (end + 1).min(len);
                } else {
                    let vs = i;
                    while i < len && !lb[i].is_ascii_whitespace() && lb[i] != b'>' {
                        i += 1;
                    }
                    value = text.get(vs..i).unwrap_or("");
                }
            }
            attrs.push((attr, value));
        }
        for (attr, value) in &attrs {
            if *attr == "style" {
                scan_css(value, out);
            }
        }
        if LOADING_TAGS.contains(&name) {
            for (attr, value) in &attrs {
                if LOADING_ATTRS.contains(attr) {
                    push_if_remote(value, out);
                } else if SRCSET_ATTRS.contains(attr) {
                    for candidate in value.split(',') {
                        push_if_remote(candidate.split_whitespace().next().unwrap_or(""), out);
                    }
                }
            }
        }
        if name == "meta" && attrs.iter().any(|(a, v)| *a == "http-equiv" && v.trim().eq_ignore_ascii_case("refresh")) {
            for (attr, value) in &attrs {
                if *attr == "content" {
                    push_if_remote(refresh_url(value).unwrap_or(""), out);
                }
            }
        }
        if name == "style" || name == "script" {
            let close = if name == "style" { b"</style".as_slice() } else { b"</script".as_slice() };
            let end = find(lb, i, close).unwrap_or(len);
            let body = text.get(i..end).unwrap_or("");
            if name == "style" {
                scan_css(body, out);
            } else {
                scan_js(body, out);
            }
            i = end;
        }
    }
}

fn skip_ws(lb: &[u8], mut i: usize) -> usize {
    while i < lb.len() && lb[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// A quoted CSS string starting at `i`: (content, index after the closing quote).
fn quoted<'a>(text: &'a str, lb: &[u8], i: usize) -> Option<(&'a str, usize)> {
    let q = *lb.get(i)?;
    if q != b'"' && q != b'\'' {
        return None;
    }
    let end = find(lb, i + 1, &[q]).unwrap_or(lb.len());
    Some((text.get(i + 1..end).unwrap_or(""), (end + 1).min(lb.len())))
}

fn scan_css(text: &str, out: &mut Vec<String>) {
    let lower = text.to_ascii_lowercase();
    let lb = lower.as_bytes();
    let len = lb.len();
    let mut i = 0;
    while i < len {
        let rest = &lb[i..];
        if rest.starts_with(b"/*") {
            i = find(lb, i + 2, b"*/").map_or(len, |e| e + 2);
        } else if rest.starts_with(b"url(") {
            let j = skip_ws(lb, i + 4);
            if let Some((v, after)) = quoted(text, lb, j) {
                push_if_remote(v, out);
                i = after;
            } else {
                let end = find(lb, j, b")").unwrap_or(len);
                push_if_remote(text.get(j..end).unwrap_or(""), out);
                i = end;
            }
        } else if rest.starts_with(b"image-set(") {
            // Bare strings in image-set() are image URLs. Collect them, then continue inside so
            // `url(...)` candidates are handled by the branch above (duplicates are removed).
            let mut j = i + 10;
            let mut depth = 1usize;
            while j < len && depth > 0 {
                match lb[j] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    b'"' | b'\'' => {
                        if let Some((v, after)) = quoted(text, lb, j) {
                            push_if_remote(v, out);
                            j = after;
                            continue;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            i += 10;
        } else if rest.starts_with(b"@import") {
            let j = skip_ws(lb, i + 7);
            match quoted(text, lb, j) {
                Some((v, after)) => {
                    push_if_remote(v, out);
                    i = after;
                }
                // `@import url(...)` is handled by the `url(` branch.
                None => i = j.max(i + 1),
            }
        } else {
            i += 1;
        }
    }
}

fn scan_js(text: &str, out: &mut Vec<String>) {
    let lb = text.as_bytes();
    let len = lb.len();
    let mut i = 0;
    while i < len {
        let q = lb[i];
        if q == b'"' || q == b'\'' || q == b'`' {
            let start = i + 1;
            let head: String = lb
                .get(start..(start + 8).min(len))
                .unwrap_or_default()
                .iter()
                .map(|b| char::from(b.to_ascii_lowercase()))
                .collect();
            let protocol_relative =
                head.starts_with("//") && lb.get(start + 2).is_some_and(|c| c.is_ascii_alphanumeric());
            if head.starts_with("http://") || head.starts_with("https://") || protocol_relative {
                let mut end = start;
                while end < len && lb[end] != q && lb[end] != b'\n' {
                    end += 1;
                }
                push_if_remote(text.get(start..end).unwrap_or(""), out);
                i = end;
            }
        }
        i += 1;
    }
}

// ------------------------------------------------------------------------- JSON Schema export

fn required(keys: &[&str]) -> CValue {
    CValue::Arr(keys.iter().map(|k| s(k)).collect())
}

fn enum_of(values: impl Iterator<Item = &'static str>) -> CValue {
    object([("enum", CValue::Arr(values.map(s).collect()))])
}

fn closed(keys: &[&str], props: Vec<(&str, CValue)>) -> Vec<(String, CValue)> {
    vec![
        ("type".to_string(), s("object")),
        ("additionalProperties".to_string(), CValue::Bool(false)),
        ("required".to_string(), required(keys)),
        ("properties".to_string(), CValue::Obj(props.into_iter().map(|(k, v)| (k.to_string(), v)).collect())),
    ]
}

fn closed_value(keys: &[&str], props: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(closed(keys, props).into_iter().collect())
}

fn path_schema() -> CValue {
    object([
        ("type", s("string")),
        ("minLength", num(1.0)),
        ("maxLength", num(MAX_PATH_BYTES as f64)),
        ("pattern", s(RELPATH_PATTERN)),
        ("not", object([("pattern", s(DOT_SEGMENT_PATTERN))])),
    ])
}

fn digest_schema() -> CValue {
    object([("type", s("string")), ("pattern", s(Digest::PATTERN))])
}

fn size_schema() -> CValue {
    object([("type", s("integer")), ("minimum", num(0.0)), ("maximum", num(MAX_SIZE as f64))])
}

fn release_schema() -> CValue {
    object([("type", s("string")), ("maxLength", num(64.0)), ("pattern", s(RELEASE_PATTERN))])
}

fn top_schema(title: &str, keys: &[&str], props: Vec<(&str, CValue)>) -> CValue {
    let mut top = vec![("$schema".to_string(), s(JSON_SCHEMA_DIALECT)), ("title".to_string(), s(title))];
    top.extend(closed(keys, props));
    CValue::Obj(top.into_iter().collect())
}

fn entries_schema(items: CValue) -> CValue {
    object([("type", s("array")), ("maxItems", num(MAX_ENTRIES as f64)), ("items", items)])
}

/// JSON Schema (draft 2020-12) of `offline-bundle.json`, from the same tables the reader
/// uses (REQ-005). Sorting, uniqueness, device names and trailing dots are checked by
/// [`parse_bundle`] only.
pub fn bundle_json_schema() -> CValue {
    let entry = closed_value(
        &ENTRY_KEYS,
        vec![
            ("path", path_schema()),
            ("role", enum_of(Role::ALL.iter().map(|r| r.as_str()))),
            ("sha256", digest_schema()),
            ("size", size_schema()),
        ],
    );
    top_schema(
        "Metrology Maven offline bundle, schema_version 1 (PLAT.DEPLOY.OFFLINE.BUNDLE)",
        &BUNDLE_KEYS,
        vec![
            ("schema_version", object([("const", num(f64::from(SUPPORTED_SCHEMA)))])),
            ("bundle_id", object([("type", s("string")), ("maxLength", num(128.0)), ("pattern", s(ID_PATTERN))])),
            ("release", release_schema()),
            ("os", enum_of(OsFamily::ALL.iter().map(|o| o.as_str()))),
            ("arch", enum_of(Arch::ALL.iter().map(|a| a.as_str()))),
            ("entries", entries_schema(entry)),
        ],
    )
}

/// JSON Schema (draft 2020-12) of `install-manifest.json` (REQ-005). Sorting, uniqueness,
/// device names and trailing dots are checked by [`parse_install_manifest`] only.
pub fn install_json_schema() -> CValue {
    let file = closed_value(
        &FILE_KEYS,
        vec![("relpath", path_schema()), ("sha256", digest_schema()), ("size", size_schema())],
    );
    top_schema(
        "Metrology Maven install manifest, schema_version 1 (PLAT.DEPLOY.OFFLINE.BUNDLE)",
        &INSTALL_KEYS,
        vec![
            ("schema_version", object([("const", num(f64::from(SUPPORTED_SCHEMA)))])),
            ("release", release_schema()),
            ("os", enum_of(OsFamily::ALL.iter().map(|o| o.as_str()))),
            ("arch", enum_of(Arch::ALL.iter().map(|a| a.as_str()))),
            ("kind", enum_of(InstallKind::ALL.iter().map(|k| k.as_str()))),
            ("files", entries_schema(file)),
        ],
    )
}

/// The exported schema files as (repository path, canonical JSON plus a newline).
pub fn json_schema_files() -> Result<Vec<(&'static str, Vec<u8>)>, CanonError> {
    let mut out = Vec::new();
    for (path, value) in [(BUNDLE_SCHEMA_PATH, bundle_json_schema()), (INSTALL_SCHEMA_PATH, install_json_schema())] {
        let mut bytes = fid_canon::to_canonical_bytes(&value)?;
        bytes.push(b'\n');
        out.push((path, bytes));
    }
    Ok(out)
}
