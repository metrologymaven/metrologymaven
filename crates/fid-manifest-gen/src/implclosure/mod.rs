//! Per-algorithm implementation digest over the crate closure. Spec node `KRN.MAN.MANI.IMPL`.
//!
//! An algorithm's implementation digest covers exactly the source it is built from, at crate
//! level: the crate that registers it plus every crate that crate reaches over normal or build
//! dependency edges (any platform; dev-dependencies are not in the kernel graph), vendored
//! third-party crates included. Every value-affecting choice is pinned here and in the leaf's
//! Design:
//!
//! - **Closure.** From the resolved graph of a `cargo metadata --format-version 1` file
//!   ([`Metadata`]): normal (`kind: null`) and `build` edges, whatever their `target` gate;
//!   `dev` edges are skipped. Every crate in the closure, the registering crate included, must
//!   be on the allowlist, else [`ImplError::NotAllowlisted`] (REQ-KRN.MAN.MANI.IMPL-004).
//! - **Hashed set.** Only git-tracked files ([`TrackedFiles`]), read as committed bytes with
//!   no line-ending conversion. Per crate: its `Cargo.toml` and every file below `<crate>/src/`
//!   (any extension); paths are relative to the crate directory and `/`-separated
//!   (`Cargo.toml`, `src/lib.rs`), so they do not depend on the checkout path or the operating
//!   system. Per workspace: the root `Cargo.toml` and `.cargo/config.toml` and `.cargo/config`
//!   when present (an untracked one on disk is an error). Per crate, its `Cargo.lock` entry
//!   (name, version, source, checksum; absent fields as `""`). `tests/`, `benches/` and
//!   `examples/` lie outside `src/` and are not read.
//! - **Nothing escapes the hashed set** (fail closed, `MetadataInvalid`): a lib or bin target
//!   whose source lies outside `src/`; a build script or proc-macro target anywhere in the
//!   closure; in any hashed `.rs` file (and in any file it `include!`s), an `include!`,
//!   `include_str!`, `include_bytes!` or attribute `path = ..` whose argument is not one plain
//!   string literal, or whose path, resolved against the including file's directory, is not a
//!   hashed file of the same crate. An untracked file below `src/` is an `Io` error.
//! - **Encoding.** One object per crate,
//!   `{"features":[..],"files":[{"path":..,"sha256":"sha256:<hex>"},..],`
//!   `"lock":{"checksum":..,"name":..,"source":..,"version":..},"name":..,"version":..}`,
//!   features sorted and unique, files sorted by the UTF-8 bytes of their path (so `src/a-b.rs`
//!   < `src/a.rs` < `src/a/b.rs`), crates sorted by (name, version) bytes with ties broken by
//!   the crate object's canonical bytes. The digest is SHA-256 over the RFC 8785 canonical
//!   JSON of `{"crates":[..],"workspace":[{"path":..,"sha256":..},..]}` (workspace files sorted
//!   by path), written by the kernel's canonical JSON writer (`fid_kci::kci::json`) and hashed
//!   by the kernel's SHA-256 (`fid_kci::kci::sha256`), so the digest is computed over canonical
//!   bytes only (REQ-KRN.MAN.MANI.IMPL-001).
//! - **Table.** [`ImplDigestTable`], sorted by the [`AlgorithmId`] total order; its own
//!   canonical form is `[{"algorithm":"<id>","impl_digest":"sha256:<hex>"},..]` in that order.
//! - **Errors,** checked in this order: allowlist and registration input
//!   (`InputInvalid`), a registration for an algorithm not in the table (`UnknownAlgorithm`,
//!   first in ID order), the lockfile and the workspace files, then per algorithm in ID order: no registration or no such crate
//!   (`NoRegisteringCrate`), the closure's first crate in (name, version) order missing from
//!   the allowlist (`NotAllowlisted`), then layout and file errors (`MetadataInvalid`, `Io`).
//!
//! [`check_manifest`] reads a kernel manifest with the strict
//! [`KernelManifest::parse_canonical`] and checks that its implementation digests are the
//! table's, so a stale manifest is caught by the same code that computes the digests.

mod lockfile;
mod metadata;
mod scan;
mod tree;

use std::cmp::Ordering;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use fid_kci::kci::json::{to_canonical, Json, JsonError};
use fid_kci::kci::{sha256_digest, Digest};
use fid_kci::manifest::{KernelManifest, ManifestError};
use fid_kci::AlgorithmId;

pub use metadata::{Metadata, Package, MAX_METADATA_BYTES};
pub use tree::{
    git_tracked_files, FsTree, SourceTree, TrackedFiles, MAX_DIR_DEPTH, MAX_FILES_PER_DIR, MAX_FILE_BYTES,
    MAX_TRACKED_FILES,
};

use lockfile::Lockfile;
use scan::RefKind;

/// Which crate registers an algorithm (the algorithm table's registration data).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub algorithm: AlgorithmId,
    /// The registering crate's package name.
    pub crate_name: String,
}

/// Why a file could not be read. No operating-system message is kept, so errors are the
/// same on every platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoKind {
    NotFound,
    PermissionDenied,
    Read,
    Symlink,
    NotADirectory,
    NotAFile,
    NonUtf8Name,
    BadName,
    TooLarge,
    TooManyFiles,
    TooDeep,
    /// A file on disk below a hashed directory, or a Cargo configuration file, is not tracked.
    Untracked,
    /// `git ls-files` could not be run or its output is malformed.
    Git,
}

impl IoKind {
    /// Stable code.
    pub fn code(self) -> &'static str {
        match self {
            IoKind::NotFound => "not_found",
            IoKind::PermissionDenied => "permission_denied",
            IoKind::Read => "read",
            IoKind::Symlink => "symlink",
            IoKind::NotADirectory => "not_a_directory",
            IoKind::NotAFile => "not_a_file",
            IoKind::NonUtf8Name => "non_utf8_name",
            IoKind::BadName => "bad_name",
            IoKind::TooLarge => "too_large",
            IoKind::TooManyFiles => "too_many_files",
            IoKind::TooDeep => "too_deep",
            IoKind::Untracked => "untracked",
            IoKind::Git => "git",
        }
    }
}

/// Implementation-digest errors, each with a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImplError {
    /// A registration names an algorithm that is not in the algorithm table.
    UnknownAlgorithm { algorithm: AlgorithmId },
    /// An algorithm in the table has no registration, or its registering crate is not a
    /// package of the metadata.
    NoRegisteringCrate { algorithm: AlgorithmId },
    /// A crate in the algorithm's closure is not on the kernel dependency allowlist.
    NotAllowlisted { crate_name: String, algorithm: AlgorithmId },
    /// A source file or directory could not be read (build time only); `path` is relative to
    /// the workspace root, or the metadata file's path as given.
    Io { path: String, kind: IoKind },
    /// The `cargo metadata` file is malformed or describes a layout the digest cannot cover.
    MetadataInvalid { reason: &'static str },
    /// The registrations or the allowlist are malformed (duplicates).
    InputInvalid { reason: &'static str },
}

impl ImplError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            ImplError::UnknownAlgorithm { .. } => "unknown_algorithm",
            ImplError::NoRegisteringCrate { .. } => "no_registering_crate",
            ImplError::NotAllowlisted { .. } => "not_allowlisted",
            ImplError::Io { .. } => "io",
            ImplError::MetadataInvalid { .. } => "metadata_invalid",
            ImplError::InputInvalid { .. } => "input_invalid",
        }
    }
}

impl fmt::Display for ImplError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImplError::UnknownAlgorithm { algorithm } => write!(f, "unknown_algorithm: {algorithm}"),
            ImplError::NoRegisteringCrate { algorithm } => write!(f, "no_registering_crate: {algorithm}"),
            ImplError::NotAllowlisted { crate_name, algorithm } => {
                write!(f, "not_allowlisted: crate {crate_name:?} in the closure of {algorithm}")
            }
            ImplError::Io { path, kind } => write!(f, "io: {}: {path:?}", kind.code()),
            ImplError::MetadataInvalid { reason } => write!(f, "metadata_invalid: {reason}"),
            ImplError::InputInvalid { reason } => write!(f, "input_invalid: {reason}"),
        }
    }
}

impl std::error::Error for ImplError {}

/// One crate of a closure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateRef {
    pub name: String,
    pub version: String,
}

/// One algorithm's implementation digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplDigestEntry {
    pub algorithm: AlgorithmId,
    pub impl_digest: Digest,
    /// The crates digested, in digest order (diagnostics; not part of the table's encoding).
    pub closure: Vec<CrateRef>,
}

/// Implementation digests, sorted by algorithm ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplDigestTable {
    entries: Vec<ImplDigestEntry>,
}

impl ImplDigestTable {
    /// Entries, strictly increasing by algorithm ID.
    pub fn entries(&self) -> &[ImplDigestEntry] {
        &self.entries
    }

    /// The implementation digest of `algorithm`, if it is in the table.
    pub fn get(&self, algorithm: &AlgorithmId) -> Option<&Digest> {
        self.entries.binary_search_by(|e| e.algorithm.cmp(algorithm)).ok().map(|i| &self.entries[i].impl_digest)
    }

    /// `[{"algorithm":"<id>","impl_digest":"sha256:<hex>"},..]` in ID order, canonical JSON.
    pub fn canonical_json(&self) -> Result<Vec<u8>, JsonError> {
        let rows = self
            .entries
            .iter()
            .map(|e| {
                Json::Obj(vec![
                    ("algorithm".to_string(), Json::Str(e.algorithm.to_string())),
                    ("impl_digest".to_string(), Json::Str(e.impl_digest.as_str().to_string())),
                ])
            })
            .collect();
        to_canonical(&Json::Arr(rows))
    }
}

fn s(v: &str) -> Json {
    Json::Str(v.to_string())
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

fn canonical(v: &Json) -> Result<Vec<u8>, ImplError> {
    to_canonical(v).map_err(|e| ImplError::InputInvalid { reason: e.code() })
}

/// One crate's digest object and its canonical bytes (the tie-break sort key).
struct CrateEntry {
    json: Json,
    bytes: Vec<u8>,
}

fn file_list(files: &[(String, Vec<u8>)]) -> Json {
    Json::Arr(
        files
            .iter()
            .map(|(path, bytes)| {
                Json::Obj(vec![("path".to_string(), s(path)), ("sha256".to_string(), s(sha256_digest(bytes).as_str()))])
            })
            .collect(),
    )
}

/// `lit` resolved against the directory of `file` (both relative to the crate directory);
/// `None` when it is absolute, uses `\` or `:`, or climbs out of the crate directory.
fn resolve(file: &str, lit: &str) -> Option<String> {
    if lit.starts_with('/') || lit.contains('\\') || lit.contains(':') {
        return None;
    }
    let mut parts: Vec<&str> = file.split('/').collect();
    parts.pop();
    for c in lit.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            c => parts.push(c),
        }
    }
    Some(parts.join("/"))
}

/// Checks that no hashed source of one crate makes the compiler read a file outside the
/// crate's hashed files: every `.rs` file is scanned, and so is every file reached through
/// `include!` (it is compiled as Rust whatever its extension).
fn check_escapes(files: &BTreeMap<String, Vec<u8>>) -> Result<(), ImplError> {
    let bad = |reason: &'static str| ImplError::MetadataInvalid { reason };
    let mut queue: Vec<&str> = files.keys().map(String::as_str).filter(|p| p.ends_with(".rs")).collect();
    let mut seen: BTreeSet<&str> = queue.iter().copied().collect();
    while let Some(file) = queue.pop() {
        let bytes = files.get(file).ok_or_else(|| bad("include_outside_hashed_set"))?;
        let text = std::str::from_utf8(bytes).map_err(|_| bad("source_not_utf8"))?;
        for (kind, lit) in scan::references(text).map_err(bad)? {
            let target = resolve(file, &lit).filter(|t| files.contains_key(t.as_str()));
            let Some(target) = target else {
                return Err(bad(match kind {
                    RefKind::ModPath => "path_attr_outside_hashed_set",
                    RefKind::Code | RefKind::Data => "include_outside_hashed_set",
                }));
            };
            if kind == RefKind::Code {
                if let Some((key, _)) = files.get_key_value(target.as_str()) {
                    if seen.insert(key.as_str()) {
                        queue.push(key.as_str());
                    }
                }
            }
        }
    }
    Ok(())
}

fn crate_entry(md: &Metadata, lock: &Lockfile, p: &Package, tree: &dyn SourceTree) -> Result<CrateEntry, ImplError> {
    let layout = md.layout(p)?;
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    files.insert("Cargo.toml".to_string(), tree.read_file(&join(&layout.dir, "Cargo.toml"))?);
    for (rel, bytes) in tree.files_under(&join(&layout.dir, "src"))? {
        if files.insert(format!("src/{rel}"), bytes).is_some() {
            return Err(ImplError::MetadataInvalid { reason: "duplicate_file" });
        }
    }
    check_escapes(&files)?;
    // BTreeMap<String, _> iterates in byte order of the path.
    let files: Vec<(String, Vec<u8>)> = files.into_iter().collect();
    let entry = lock.get(p.name(), p.version(), p.source())?;
    let lock_json = Json::Obj(vec![
        ("checksum".to_string(), s(&entry.checksum)),
        ("name".to_string(), s(&entry.name)),
        ("source".to_string(), s(&entry.source)),
        ("version".to_string(), s(&entry.version)),
    ]);
    let features = md.features(p.id())?.iter().map(|f| s(f)).collect();
    let json = Json::Obj(vec![
        ("features".to_string(), Json::Arr(features)),
        ("files".to_string(), file_list(&files)),
        ("lock".to_string(), lock_json),
        ("name".to_string(), s(p.name())),
        ("version".to_string(), s(p.version())),
    ]);
    let bytes = canonical(&json)?;
    Ok(CrateEntry { json, bytes })
}

/// Workspace-level files that change how every crate builds: the root manifest (profiles,
/// patches) and the Cargo configuration (source replacement, rustflags), sorted by path.
const WORKSPACE_FILES: [(&str, bool); 3] = [(".cargo/config", false), (".cargo/config.toml", false), ("Cargo.toml", true)];

fn workspace_files(tree: &dyn SourceTree) -> Result<Json, ImplError> {
    let mut files = Vec::new();
    for (path, required) in WORKSPACE_FILES {
        let bytes = if required { Some(tree.read_file(path)?) } else { tree.read_optional(path)? };
        if let Some(bytes) = bytes {
            files.push((path.to_string(), bytes));
        }
    }
    Ok(file_list(&files))
}

/// Crate names that differ only in ASCII case or `-` versus `_` name the same crate to Cargo's
/// registry rules; an allowlist holding two such entries is ambiguous.
fn folded(name: &str) -> String {
    name.to_ascii_lowercase().replace('-', "_")
}

/// Computes the implementation digest of every algorithm in `algorithms` (the algorithm
/// table's IDs; repeats are ignored). `registrations` map algorithm IDs to their registering
/// crate; `allowlist` holds every crate name allowed in a kernel closure (the kernel-layer
/// workspace crates and the third-party crates of `kernel-allowlist.toml`).
pub fn impl_digests(
    metadata: &Metadata,
    algorithms: &[AlgorithmId],
    registrations: &[Registration],
    allowlist: &[String],
    tree: &dyn SourceTree,
) -> Result<ImplDigestTable, ImplError> {
    let mut allow: BTreeMap<String, &str> = BTreeMap::new();
    for name in allowlist {
        if name.is_empty() || allow.insert(folded(name), name).is_some() {
            return Err(ImplError::InputInvalid { reason: "allowlist_duplicate_or_empty" });
        }
    }
    let allow: BTreeSet<&str> = allow.into_values().collect();

    let mut algs: Vec<&AlgorithmId> = algorithms.iter().collect();
    algs.sort();
    algs.dedup();
    let mut regs: Vec<&Registration> = registrations.iter().collect();
    regs.sort_by(|a, b| a.algorithm.cmp(&b.algorithm));
    if regs.windows(2).any(|w| w[0].algorithm == w[1].algorithm) {
        return Err(ImplError::InputInvalid { reason: "duplicate_registration" });
    }
    for r in &regs {
        if algs.binary_search(&&r.algorithm).is_err() {
            return Err(ImplError::UnknownAlgorithm { algorithm: r.algorithm.clone() });
        }
    }

    let lock = Lockfile::parse(&tree.read_file("Cargo.lock")?)?;
    let workspace = workspace_files(tree)?;
    let mut cache: BTreeMap<String, CrateEntry> = BTreeMap::new();
    let mut entries = Vec::with_capacity(algs.len());
    for alg in algs {
        let none = || ImplError::NoRegisteringCrate { algorithm: alg.clone() };
        let reg = regs.binary_search_by(|r| r.algorithm.cmp(alg)).map(|i| regs[i]).map_err(|_| none())?;
        let root = metadata.package_named(&reg.crate_name)?.ok_or_else(none)?;
        let mut crates: Vec<&Package> =
            metadata.closure(root.id())?.into_iter().map(|id| metadata.package(id)).collect::<Result<_, _>>()?;
        crates.sort_by(|a, b| {
            (a.name().as_bytes(), a.version().as_bytes()).cmp(&(b.name().as_bytes(), b.version().as_bytes()))
        });
        if let Some(p) = crates.iter().find(|p| !allow.contains(p.name())) {
            return Err(ImplError::NotAllowlisted { crate_name: p.name().to_string(), algorithm: alg.clone() });
        }
        for p in &crates {
            if let Entry::Vacant(slot) = cache.entry(p.id().to_string()) {
                slot.insert(crate_entry(metadata, &lock, p, tree)?);
            }
        }
        let mut items: Vec<(&Package, &CrateEntry)> = Vec::with_capacity(crates.len());
        for p in &crates {
            let e = cache.get(p.id()).ok_or(ImplError::MetadataInvalid { reason: "dangling_package_id" })?;
            items.push((*p, e));
        }
        items.sort_by(|a, b| {
            (a.0.name().as_bytes(), a.0.version().as_bytes())
                .cmp(&(b.0.name().as_bytes(), b.0.version().as_bytes()))
                .then_with(|| a.1.bytes.cmp(&b.1.bytes))
        });
        let list = Json::Obj(vec![
            ("crates".to_string(), Json::Arr(items.iter().map(|(_, e)| e.json.clone()).collect())),
            ("workspace".to_string(), workspace.clone()),
        ]);
        entries.push(ImplDigestEntry {
            algorithm: alg.clone(),
            impl_digest: sha256_digest(&canonical(&list)?),
            closure: items
                .iter()
                .map(|(p, _)| CrateRef { name: p.name().to_string(), version: p.version().to_string() })
                .collect(),
        });
    }
    Ok(ImplDigestTable { entries })
}

/// Reads the metadata file at `metadata_file` (bounded) and computes [`impl_digests`] over the
/// workspace checked out at `workspace`, whose git-tracked files are `tracked` (from
/// [`git_tracked_files`]). Paths in the metadata are taken relative to its `workspace_root`, so
/// `workspace` may be any checkout of the same sources.
pub fn impl_digests_from_files(
    metadata_file: &Path,
    workspace: &Path,
    tracked: TrackedFiles,
    algorithms: &[AlgorithmId],
    registrations: &[Registration],
    allowlist: &[String],
) -> Result<ImplDigestTable, ImplError> {
    let shown = metadata_file.display().to_string();
    let bytes = tree::read_bounded(metadata_file, &shown, MAX_METADATA_BYTES as u64)?;
    let md = Metadata::parse(&bytes)?;
    impl_digests(&md, algorithms, registrations, allowlist, &FsTree::new(workspace, tracked))
}

/// Why a manifest disagrees with an implementation-digest table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestCheckError {
    /// The manifest is not a valid canonical manifest (strict read).
    Manifest(ManifestError),
    /// The manifest lists an algorithm the table does not.
    NotInTable { algorithm: AlgorithmId },
    /// The table lists an algorithm the manifest does not.
    NotInManifest { algorithm: AlgorithmId },
    /// The manifest's implementation digest differs from the table's.
    ImplDigestMismatch { algorithm: AlgorithmId },
}

impl ManifestCheckError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            ManifestCheckError::Manifest(e) => e.code(),
            ManifestCheckError::NotInTable { .. } => "not_in_table",
            ManifestCheckError::NotInManifest { .. } => "not_in_manifest",
            ManifestCheckError::ImplDigestMismatch { .. } => "impl_digest_mismatch",
        }
    }
}

impl fmt::Display for ManifestCheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestCheckError::Manifest(e) => write!(f, "manifest: {e}"),
            ManifestCheckError::NotInTable { algorithm } => write!(f, "not_in_table: {algorithm}"),
            ManifestCheckError::NotInManifest { algorithm } => write!(f, "not_in_manifest: {algorithm}"),
            ManifestCheckError::ImplDigestMismatch { algorithm } => write!(f, "impl_digest_mismatch: {algorithm}"),
        }
    }
}

impl std::error::Error for ManifestCheckError {}

/// Reads `manifest` strictly ([`KernelManifest::parse_canonical`]: the bytes must be the
/// canonical encoding) and checks that it lists exactly the table's algorithms with the
/// table's implementation digests. The first difference in ID order is reported.
pub fn check_manifest(manifest: &[u8], table: &ImplDigestTable) -> Result<KernelManifest, ManifestCheckError> {
    let m = KernelManifest::parse_canonical(manifest).map_err(ManifestCheckError::Manifest)?;
    let listed = &m.core().algorithms;
    let rows = table.entries();
    let (mut i, mut j) = (0, 0);
    loop {
        match (listed.get(i), rows.get(j)) {
            (None, None) => break,
            (Some(a), None) => return Err(ManifestCheckError::NotInTable { algorithm: a.id.clone() }),
            (None, Some(t)) => return Err(ManifestCheckError::NotInManifest { algorithm: t.algorithm.clone() }),
            (Some(a), Some(t)) => match a.id.cmp(&t.algorithm) {
                Ordering::Less => return Err(ManifestCheckError::NotInTable { algorithm: a.id.clone() }),
                Ordering::Greater => return Err(ManifestCheckError::NotInManifest { algorithm: t.algorithm.clone() }),
                Ordering::Equal => {
                    if a.impl_digest != t.impl_digest {
                        return Err(ManifestCheckError::ImplDigestMismatch { algorithm: a.id.clone() });
                    }
                    i += 1;
                    j += 1;
                }
            },
        }
    }
    Ok(m)
}
