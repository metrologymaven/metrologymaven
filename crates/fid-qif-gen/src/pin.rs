//! Pinned XSD manifest and digest check (spec node IO.QIF.GEN.PIN).
//!
//! The manifest `qif-xsd.manifest.toml` sits at the root of the schema directory and pins
//! every schema file by relative path, byte size, SHA-256 and role (REQ-IO.QIF.GEN.PIN-001):
//!
//! ```toml
//! manifest_version = 1
//! licence_path = "LICENSE.txt"            # upstream licence file, also checked
//! licence_sha256 = "sha256:<64 hex>"
//!
//! [source]
//! url = "https://example.org/upstream.git"
//! rev = "<tag or commit>"
//!
//! [[file]]
//! path = "lib/units.xsd"                  # '/'-separated, relative to the directory
//! bytes = 237
//! sha256 = "sha256:<64 hex>"
//! role = "xsd"                            # "xsd" (.xsd) or "xslt" (.xsl, .xslt)
//! ```
//!
//! [`PinnedSet::verify`] lists the directory without following symbolic links, fails
//! `xsd_unpinned_file` for any unlisted `.xsd`/`.xsl`/`.xslt` file, then reads each listed
//! file once and fails `xsd_digest_mismatch` if it is missing or its size or digest differs.
//! The verified bytes are then reachable only through [`PinnedSet::resolve`] (and
//! [`PinnedSet::resolve_relative`] for `xs:include`/`xs:import` locations), which never
//! touch the file system. No network is ever used (REQ-IO.QIF.GEN.PIN-005).
//!
//! Determinism D0: the same directory gives the same set or the same first error, because
//! listing, checks and errors all follow sorted path order.

use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// File name of the manifest at the root of the schema directory.
pub const MANIFEST_FILE: &str = "qif-xsd.manifest.toml";
/// The only manifest format version this crate reads.
pub const MANIFEST_VERSION: i64 = 1;
/// Largest manifest accepted, in bytes.
pub const MAX_MANIFEST_BYTES: u64 = 1 << 20;
/// Most `[[file]]` rows accepted.
pub const MAX_FILES: usize = 4096;
/// Largest pinned file accepted, in bytes.
pub const MAX_FILE_BYTES: u64 = 64 << 20;
/// Deepest subdirectory nesting listed below the schema directory.
pub const MAX_DEPTH: usize = 16;
/// Most directory entries listed in total.
pub const MAX_ENTRIES: usize = 65_536;
/// Longest relative path or schema location accepted, in bytes.
pub const MAX_PATH_BYTES: usize = 512;
const MAX_TEXT: usize = 1024;
/// Longest hostile input kept in an error message, in bytes.
const KEEP: usize = 256;

const TOP_KEYS: [&str; 5] = ["manifest_version", "licence_path", "licence_sha256", "source", "file"];
const SOURCE_KEYS: [&str; 2] = ["url", "rev"];
const FILE_KEYS: [&str; 4] = ["path", "bytes", "sha256", "role"];

/// Role of a pinned file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    /// An XML Schema file (`.xsd`).
    Xsd,
    /// A normative XSLT file (`.xsl` or `.xslt`).
    Xslt,
}

impl Role {
    /// Wire string, `xsd` or `xslt`.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Xsd => "xsd",
            Role::Xslt => "xslt",
        }
    }

    /// Parses the wire string.
    pub fn parse(s: &str) -> Option<Role> {
        match s {
            "xsd" => Some(Role::Xsd),
            "xslt" => Some(Role::Xslt),
            _ => None,
        }
    }

    /// The role a file name implies by its extension (ASCII case-insensitive), if it is a
    /// schema file at all. These are the names that must be pinned (REQ-IO.QIF.GEN.PIN-003).
    pub fn for_name(name: &str) -> Option<Role> {
        let last = name.rsplit('/').next().unwrap_or(name);
        let (_, ext) = last.rsplit_once('.')?;
        if ext.eq_ignore_ascii_case("xsd") {
            Some(Role::Xsd)
        } else if ext.eq_ignore_ascii_case("xsl") || ext.eq_ignore_ascii_case("xslt") {
            Some(Role::Xslt)
        } else {
            None
        }
    }
}

/// Upstream source of the pinned files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    /// Repository URL (recorded only; never fetched).
    pub url: String,
    /// Tag or commit.
    pub rev: String,
}

/// One manifest row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinnedFile {
    /// `/`-separated path relative to the schema directory.
    pub path: String,
    /// Exact size in bytes.
    pub bytes: u64,
    /// SHA-256 of the file's bytes.
    pub sha256: Digest,
    /// Role, which must agree with the file extension.
    pub role: Role,
}

/// A parsed and validated `qif-xsd.manifest.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    /// Upstream source reference.
    pub source: Source,
    /// Path of the upstream licence file, relative to the schema directory.
    pub licence_path: String,
    /// SHA-256 of the upstream licence file.
    pub licence_sha256: Digest,
    /// Rows, sorted by path, unique even ignoring ASCII case.
    pub files: Vec<PinnedFile>,
}

/// Details of an `xsd_digest_mismatch`: the file and both digests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mismatch {
    /// The pinned file (or the licence file).
    pub path: String,
    /// Digest from the manifest.
    pub expected: Digest,
    /// Size from the manifest (`None` for the licence file, which pins only a digest).
    pub expected_bytes: Option<u64>,
    /// Digest found; `None` when the file is missing, not a regular file, or too large to hash.
    pub actual: Option<Digest>,
    /// Size found; `None` when the file is missing or not a regular file.
    pub actual_bytes: Option<u64>,
}

/// Errors, each with a stable code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PinError {
    /// A pinned file (or the licence file) is missing, or its size or digest differs.
    DigestMismatch(Box<Mismatch>),
    /// The directory holds a schema file the manifest does not list.
    UnpinnedFile { path: String },
    /// A requested schema location is not a manifest row.
    UnpinnedReference { location: String },
    /// The manifest is malformed, out of bounds or inconsistent.
    ManifestInvalid { detail: String },
    /// The schema directory could not be listed or read.
    DirUnreadable { path: String, detail: String },
}

impl PinError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            PinError::DigestMismatch { .. } => "xsd_digest_mismatch",
            PinError::UnpinnedFile { .. } => "xsd_unpinned_file",
            PinError::UnpinnedReference { .. } => "xsd_unpinned_reference",
            PinError::ManifestInvalid { .. } => "manifest_invalid",
            PinError::DirUnreadable { .. } => "schema_dir_unreadable",
        }
    }

    /// The file or location the error names, if any.
    pub fn path(&self) -> Option<&str> {
        match self {
            PinError::DigestMismatch(m) => Some(&m.path),
            PinError::UnpinnedFile { path } | PinError::DirUnreadable { path, .. } => Some(path),
            PinError::UnpinnedReference { location } => Some(location),
            PinError::ManifestInvalid { .. } => None,
        }
    }
}

impl fmt::Display for PinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PinError::DigestMismatch(m) => {
                write!(f, "{}: {:?}: expected {}", self.code(), m.path, m.expected)?;
                if let Some(n) = m.expected_bytes {
                    write!(f, " ({n} bytes)")?;
                }
                match (m.actual, m.actual_bytes) {
                    (Some(d), Some(n)) => write!(f, ", found {d} ({n} bytes)"),
                    (Some(d), None) => write!(f, ", found {d}"),
                    (None, Some(n)) => write!(f, ", found {n} bytes (not hashed)"),
                    (None, None) => write!(f, ", found no regular file"),
                }
            }
            PinError::UnpinnedFile { path } => write!(f, "{}: {path:?} is not in the manifest", self.code()),
            PinError::UnpinnedReference { location } => {
                write!(f, "{}: {location:?} is not a pinned file", self.code())
            }
            PinError::ManifestInvalid { detail } => write!(f, "{}: {detail}", self.code()),
            PinError::DirUnreadable { path, detail } => write!(f, "{}: {path:?}: {detail}", self.code()),
        }
    }
}

impl std::error::Error for PinError {}

/// Copies at most [`KEEP`] bytes of `s`, cut at a char boundary.
fn kept(s: &str) -> String {
    if s.len() <= KEEP {
        return s.to_string();
    }
    let mut end = KEEP;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

fn invalid(detail: impl fmt::Display) -> PinError {
    PinError::ManifestInvalid { detail: detail.to_string() }
}

fn unreadable(path: &str, detail: impl fmt::Display) -> PinError {
    PinError::DirUnreadable { path: kept(path), detail: detail.to_string() }
}

fn unpinned_ref(location: &str) -> PinError {
    PinError::UnpinnedReference { location: kept(location) }
}

/// Portable path characters: ASCII letters, digits, `.`, `_`, `-`.
fn is_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')
}

/// A manifest path: `/`-separated, relative, non-empty segments of portable characters,
/// no `.` or `..` segment, no segment ending in `.` (Windows strips it).
fn valid_rel_path(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_PATH_BYTES
        && s.split('/').all(|seg| {
            !seg.is_empty() && seg != "." && seg != ".." && !seg.ends_with('.') && seg.chars().all(is_path_char)
        })
}

fn sha256(bytes: &[u8]) -> Digest {
    Digest::from_bytes(Sha256::digest(bytes).into())
}

fn string_field(t: &toml::Table, key: &str, ctx: &str) -> Result<String, PinError> {
    let s = match t.get(key) {
        Some(toml::Value::String(s)) => s,
        Some(_) => return Err(invalid(format!("{ctx}: `{key}` must be a string"))),
        None => return Err(invalid(format!("{ctx}: missing `{key}`"))),
    };
    if s.trim().is_empty() || s.len() > MAX_TEXT || s.chars().any(char::is_control) {
        return Err(invalid(format!("{ctx}: `{key}` must be non-empty, single-line, at most {MAX_TEXT} bytes")));
    }
    Ok(s.clone())
}

fn digest_field(t: &toml::Table, key: &str, ctx: &str) -> Result<Digest, PinError> {
    let s = string_field(t, key, ctx)?;
    Digest::parse(&s).map_err(|_| invalid(format!("{ctx}: `{key}` must be sha256: plus 64 lowercase hex digits")))
}

fn path_field(t: &toml::Table, key: &str, ctx: &str) -> Result<String, PinError> {
    let s = string_field(t, key, ctx)?;
    if !valid_rel_path(&s) {
        return Err(invalid(format!(
            "{ctx}: `{key}` {:?} must be a '/'-separated relative path of [A-Za-z0-9._-] segments without '.' or '..'",
            kept(&s)
        )));
    }
    Ok(s)
}

fn check_keys(t: &toml::Table, allowed: &[&str], ctx: &str) -> Result<(), PinError> {
    for k in t.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(invalid(format!("{ctx}: unknown key {:?}", kept(k))));
        }
    }
    Ok(())
}

fn parse_row(i: usize, t: &toml::Table) -> Result<PinnedFile, PinError> {
    let ctx = format!("file #{}", i + 1);
    check_keys(t, &FILE_KEYS, &ctx)?;
    let path = path_field(t, "path", &ctx)?;
    let bytes = match t.get("bytes") {
        Some(toml::Value::Integer(n)) => u64::try_from(*n).ok().filter(|n| *n <= MAX_FILE_BYTES),
        _ => None,
    }
    .ok_or_else(|| invalid(format!("{ctx}: `bytes` must be an integer from 0 to {MAX_FILE_BYTES}")))?;
    let sha256 = digest_field(t, "sha256", &ctx)?;
    let role_text = string_field(t, "role", &ctx)?;
    let role = Role::parse(&role_text).ok_or_else(|| invalid(format!("{ctx}: `role` must be \"xsd\" or \"xslt\"")))?;
    if Role::for_name(&path) != Some(role) {
        return Err(invalid(format!(
            "{ctx}: {path:?} does not have the extension of role {:?} (.xsd for xsd; .xsl or .xslt for xslt)",
            role.as_str()
        )));
    }
    Ok(PinnedFile { path, bytes, sha256, role })
}

impl Manifest {
    /// Parses and validates manifest text. Every problem fails `manifest_invalid`.
    pub fn parse(text: &str) -> Result<Manifest, PinError> {
        if text.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(invalid(format!("manifest larger than {MAX_MANIFEST_BYTES} bytes")));
        }
        let table: toml::Table = text.parse().map_err(|e: toml::de::Error| invalid(e.to_string().trim_end()))?;
        check_keys(&table, &TOP_KEYS, "manifest")?;
        match table.get("manifest_version") {
            Some(toml::Value::Integer(MANIFEST_VERSION)) => {}
            _ => return Err(invalid(format!("manifest: `manifest_version` must be {MANIFEST_VERSION}"))),
        }
        let licence_path = path_field(&table, "licence_path", "manifest")?;
        if Role::for_name(&licence_path).is_some() {
            return Err(invalid("manifest: `licence_path` must not be a schema file"));
        }
        let licence_sha256 = digest_field(&table, "licence_sha256", "manifest")?;
        let source = match table.get("source") {
            Some(toml::Value::Table(t)) => {
                check_keys(t, &SOURCE_KEYS, "[source]")?;
                Source { url: string_field(t, "url", "[source]")?, rev: string_field(t, "rev", "[source]")? }
            }
            _ => return Err(invalid("manifest: missing [source] table")),
        };
        let rows = match table.get("file") {
            Some(toml::Value::Array(rows)) if !rows.is_empty() => rows,
            _ => return Err(invalid("manifest: needs at least one [[file]] row")),
        };
        if rows.len() > MAX_FILES {
            return Err(invalid(format!("manifest: more than {MAX_FILES} [[file]] rows")));
        }
        // Keyed by ASCII-lowercased path: paths that differ only in case would name the
        // same file on Windows and macOS, so they are refused everywhere.
        let mut files: BTreeMap<String, PinnedFile> = BTreeMap::new();
        for (i, row) in rows.iter().enumerate() {
            let toml::Value::Table(t) = row else {
                return Err(invalid(format!("file #{}: not a table", i + 1)));
            };
            let file = parse_row(i, t)?;
            let folded = file.path.to_ascii_lowercase();
            if let Some(prev) = files.get(&folded) {
                return Err(invalid(format!(
                    "file #{}: {:?} duplicates {:?} (paths must differ in more than case)",
                    i + 1,
                    file.path,
                    prev.path
                )));
            }
            files.insert(folded, file);
        }
        // `licence_path` cannot collide with a row: rows are schema files and it is not.
        let mut files: Vec<PinnedFile> = files.into_values().collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Manifest { source, licence_path, licence_sha256, files })
    }

    /// Reads and parses `dir/qif-xsd.manifest.toml` (bounded, UTF-8, regular file only).
    pub fn load(dir: &Path) -> Result<Manifest, PinError> {
        let path = dir.join(MANIFEST_FILE);
        let meta = fs::symlink_metadata(&path).map_err(|e| unreadable(MANIFEST_FILE, e))?;
        if !meta.is_file() {
            return Err(unreadable(MANIFEST_FILE, "not a regular file"));
        }
        let (bytes, too_big) = read_bounded(&path, MAX_MANIFEST_BYTES).map_err(|e| unreadable(MANIFEST_FILE, e))?;
        if too_big {
            return Err(invalid(format!("manifest larger than {MAX_MANIFEST_BYTES} bytes")));
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| invalid("manifest is not UTF-8"))?;
        Manifest::parse(text)
    }
}

/// Reads at most `limit` bytes; the flag says whether the file was longer.
fn read_bounded(path: &Path, limit: u64) -> std::io::Result<(Vec<u8>, bool)> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    let too_big = bytes.len() as u64 > limit;
    Ok((bytes, too_big))
}

/// What a directory listing found.
struct Listing {
    /// Regular files, by `/`-separated relative path (UTF-8 names only).
    regular: BTreeSet<String>,
    /// Schema-named entries that are not readable regular files (symbolic links, special
    /// files, non-UTF-8 names shown lossily). They are never read.
    odd: BTreeSet<String>,
}

fn join_rel(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}/{name}")
    }
}

/// Lists `root` recursively in sorted order, never following symbolic links, so no file
/// outside `root` is ever reached (REQ-IO.QIF.GEN.PIN-005).
fn list(root: &Path) -> Result<Listing, PinError> {
    let mut listing = Listing { regular: BTreeSet::new(), odd: BTreeSet::new() };
    let mut stack: Vec<(PathBuf, String, usize)> = vec![(root.to_path_buf(), String::new(), 0)];
    let mut seen = 0usize;
    while let Some((dir, prefix, depth)) = stack.pop() {
        let shown = if prefix.is_empty() { "." } else { prefix.as_str() };
        let mut entries = Vec::new();
        for entry in fs::read_dir(&dir).map_err(|e| unreadable(shown, e))? {
            let entry = entry.map_err(|e| unreadable(shown, e))?;
            seen += 1;
            if seen > MAX_ENTRIES {
                return Err(unreadable(".", format!("more than {MAX_ENTRIES} directory entries")));
            }
            let kind = entry.file_type().map_err(|e| unreadable(shown, e))?;
            entries.push((entry.file_name(), kind));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, kind) in entries {
            let utf8 = name.to_str().map(str::to_string);
            let rel = join_rel(&prefix, &utf8.clone().unwrap_or_else(|| name.to_string_lossy().into_owned()));
            if kind.is_dir() {
                if utf8.is_none() {
                    return Err(unreadable(&rel, "directory name is not UTF-8"));
                }
                if depth + 1 > MAX_DEPTH {
                    return Err(unreadable(&rel, format!("nested deeper than {MAX_DEPTH} directories")));
                }
                stack.push((dir.join(&name), rel, depth + 1));
            } else if kind.is_file() && utf8.is_some() {
                listing.regular.insert(rel);
            } else if Role::for_name(&rel).is_some() {
                listing.odd.insert(rel);
            }
        }
    }
    Ok(listing)
}

/// Joins a validated manifest path below `root`, segment by segment.
fn on_disk(root: &Path, rel: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for seg in rel.split('/') {
        p.push(seg);
    }
    p
}

/// Reads one listed file and checks it; returns its bytes.
fn read_checked(
    root: &Path,
    listing: &Listing,
    path: &str,
    expected: Digest,
    expected_bytes: Option<u64>,
) -> Result<Vec<u8>, PinError> {
    let mismatch = |actual: Option<Digest>, actual_bytes: Option<u64>| {
        PinError::DigestMismatch(Box::new(Mismatch {
            path: path.to_string(),
            expected,
            expected_bytes,
            actual,
            actual_bytes,
        }))
    };
    if !listing.regular.contains(path) {
        return Err(mismatch(None, None));
    }
    let (bytes, too_big) = read_bounded(&on_disk(root, path), MAX_FILE_BYTES).map_err(|e| unreadable(path, e))?;
    if too_big {
        return Err(mismatch(None, Some(bytes.len() as u64)));
    }
    let actual = sha256(&bytes);
    let actual_bytes = bytes.len() as u64;
    if actual != expected || expected_bytes.is_some_and(|n| n != actual_bytes) {
        return Err(mismatch(Some(actual), Some(actual_bytes)));
    }
    Ok(bytes)
}

/// A pinned file whose bytes matched the manifest when read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedFile {
    path: String,
    role: Role,
    sha256: Digest,
    bytes: Vec<u8>,
}

impl VerifiedFile {
    /// `/`-separated path relative to the schema directory.
    pub fn path(&self) -> &str {
        &self.path
    }
    /// Role from the manifest.
    pub fn role(&self) -> Role {
        self.role
    }
    /// SHA-256 of [`VerifiedFile::bytes`].
    pub fn sha256(&self) -> Digest {
        self.sha256
    }
    /// The verified bytes (the only copy downstream code may parse).
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// The verified schema set: read access to verified bytes by relative path only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinnedSet {
    source: Source,
    licence_sha256: Digest,
    files: BTreeMap<String, VerifiedFile>,
}

impl PinnedSet {
    /// Loads `dir/qif-xsd.manifest.toml` and verifies `dir` against it.
    pub fn verify(dir: &Path) -> Result<PinnedSet, PinError> {
        let manifest = Manifest::load(dir)?;
        PinnedSet::verify_against(dir, &manifest)
    }

    /// Verifies `dir` against an already parsed manifest. Checks, in order: unlisted schema
    /// files (sorted by path), then each manifest row (sorted by path), then the licence file.
    pub fn verify_against(dir: &Path, manifest: &Manifest) -> Result<PinnedSet, PinError> {
        let listing = list(dir)?;
        let pinned: BTreeSet<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
        let candidates: BTreeSet<&String> = listing.regular.iter().chain(listing.odd.iter()).collect();
        for path in candidates {
            if Role::for_name(path).is_some() && !pinned.contains(path.as_str()) {
                return Err(PinError::UnpinnedFile { path: kept(path) });
            }
        }
        let mut files = BTreeMap::new();
        for row in &manifest.files {
            let bytes = read_checked(dir, &listing, &row.path, row.sha256, Some(row.bytes))?;
            files.insert(
                row.path.clone(),
                VerifiedFile { path: row.path.clone(), role: row.role, sha256: row.sha256, bytes },
            );
        }
        read_checked(dir, &listing, &manifest.licence_path, manifest.licence_sha256, None)?;
        Ok(PinnedSet { source: manifest.source.clone(), licence_sha256: manifest.licence_sha256, files })
    }

    /// Number of verified files (the licence file is not counted).
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether the set is empty (never true for a verified set).
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Verified files in path order.
    pub fn files(&self) -> impl Iterator<Item = &VerifiedFile> {
        self.files.values()
    }

    /// Upstream source reference from the manifest.
    pub fn source(&self) -> &Source {
        &self.source
    }

    /// SHA-256 of the verified upstream licence file.
    pub fn licence_sha256(&self) -> Digest {
        self.licence_sha256
    }

    /// Resolves a location relative to the schema directory root to a verified file
    /// (REQ-IO.QIF.GEN.PIN-004). `.` segments are dropped; URLs, absolute paths, drive
    /// letters, `\`, `%`, `?`, `#`, empty segments and any `..` segment all fail
    /// `xsd_unpinned_reference`, as does any path that is not a manifest row (exact case).
    pub fn resolve(&self, location: &str) -> Result<&VerifiedFile, PinError> {
        let mut segs: Vec<&str> = Vec::new();
        for seg in location_segments(location)? {
            match seg {
                "." => {}
                ".." => return Err(unpinned_ref(location)),
                s => segs.push(s),
            }
        }
        self.lookup(&segs, location)
    }

    /// Resolves an `xs:include`/`xs:import` `schemaLocation` written in the verified file
    /// `referrer`, relative to that file's directory. A `..` segment is allowed only while it
    /// stays inside the schema directory; everything else follows [`PinnedSet::resolve`].
    pub fn resolve_relative(&self, referrer: &str, location: &str) -> Result<&VerifiedFile, PinError> {
        if !self.files.contains_key(referrer) {
            return Err(unpinned_ref(referrer));
        }
        let mut segs: Vec<&str> = referrer.split('/').collect();
        segs.pop();
        for seg in location_segments(location)? {
            match seg {
                "." => {}
                ".." => {
                    if segs.pop().is_none() {
                        return Err(unpinned_ref(location));
                    }
                }
                s => segs.push(s),
            }
        }
        self.lookup(&segs, location)
    }

    fn lookup(&self, segs: &[&str], location: &str) -> Result<&VerifiedFile, PinError> {
        if segs.is_empty() {
            return Err(unpinned_ref(location));
        }
        self.files.get(&segs.join("/")).ok_or_else(|| unpinned_ref(location))
    }
}

/// Splits a location into segments after the character checks shared by both resolvers.
fn location_segments(location: &str) -> Result<Vec<&str>, PinError> {
    let ok = !location.is_empty()
        && location.len() <= MAX_PATH_BYTES
        && location.chars().all(|c| c == '/' || is_path_char(c))
        && location.split('/').all(|seg| !seg.is_empty());
    if !ok {
        return Err(unpinned_ref(location));
    }
    Ok(location.split('/').collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_answer() {
        // FIPS 180-2 example "abc".
        assert_eq!(sha256(b"abc").as_str(), "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn role_from_extension() {
        assert_eq!(Role::for_name("a/b.xsd"), Some(Role::Xsd));
        assert_eq!(Role::for_name("B.XSD"), Some(Role::Xsd));
        assert_eq!(Role::for_name("c.xsl"), Some(Role::Xslt));
        assert_eq!(Role::for_name("c.xslt"), Some(Role::Xslt));
        assert_eq!(Role::for_name("xsd"), None);
        assert_eq!(Role::for_name("dir.xsd/readme"), None);
        assert_eq!(Role::for_name("LICENSE.txt"), None);
    }

    #[test]
    fn rel_paths() {
        for ok in ["a.xsd", "lib/units.xsd", "QIFLibrary/Units-1_0.xsd", ".hidden/a.xsd"] {
            assert!(valid_rel_path(ok), "{ok}");
        }
        let bad_paths = [
            "",
            "/a.xsd",
            "a//b.xsd",
            "./a.xsd",
            "../a.xsd",
            "a/../b.xsd",
            "C:/a.xsd",
            "a\\b.xsd",
            "a b.xsd",
            "a.",
            "é.xsd",
        ];
        for bad in bad_paths {
            assert!(!valid_rel_path(bad), "{bad}");
        }
    }

    #[test]
    fn long_input_is_cut() {
        let long = "é".repeat(300);
        let k = kept(&long);
        assert!(k.len() <= KEEP && long.starts_with(&k));
    }
}
