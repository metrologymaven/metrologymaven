//! `cargo xtask rel sums`: the release checksum file and checksum statement (spec node
//! `ECO.REL.SIGN.SUMS`).
//!
//! Reads `release-manifest.json` (the tool reads only `version`, `artefacts[].name` and
//! `artefacts[].kind`), lists the artefact directory, requires the two lists to match exactly,
//! stream-hashes every artefact with SHA-256 and writes two files into the output directory:
//!
//! - `SHA256SUMS`: one line `<64 lowercase hex>  <name>` per artefact, sorted by name in byte
//!   order, LF line endings and nothing else, so `sha256sum -c` reads it;
//! - `sums-statement.json`: `{release, artefacts: [{name, sha256, size}]}` as RFC 8785
//!   canonical JSON through `fid-canon` (the DSSE payload signed by `ECO.REL.SIGN.CUSTODY`).
//!   `sha256` is the same 64 hex as in `SHA256SUMS`; `size` follows the `fid-canon` rule for
//!   64-bit integers and is a decimal string.
//!
//! The checksum file itself, its signatures, the statement and the SBOMs are release
//! artefacts but are never summed (REQ-ECO.REL.SIGN.SUMS-005). They are recognised by name
//! ([`is_unsummed_name`]) or by a manifest `kind` in [`UNSUMMED_KINDS`], and skipped on both
//! sides; the manifest file and its signatures are skipped in the directory too.
//!
//! Nothing is signed, nothing touches the network, and outputs are written only after every
//! check passes. The outputs are a pure function of the manifest and the artefact bytes (D0).

use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Name of the checksum file.
pub const SUMS_FILE: &str = "SHA256SUMS";
/// Name of the checksum statement.
pub const STATEMENT_FILE: &str = "sums-statement.json";
/// Name of the release manifest.
pub const MANIFEST_FILE: &str = "release-manifest.json";
/// Largest manifest read, in bytes.
pub const MAX_MANIFEST_BYTES: u64 = 16 << 20;
/// Hashing read buffer.
pub const BUFFER_BYTES: usize = 64 * 1024;
/// Manifest `kind` values that are never summed.
pub const UNSUMMED_KINDS: [&str; 3] = ["checksums", "signature", "sbom"];

/// A failure of `rel sums`. [`fmt::Display`] gives the line the tool prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SumsError {
    /// `SUMS-MISSING <name>`: listed in the manifest, absent from the directory.
    Missing(String),
    /// `SUMS-UNLISTED <name>`: in the directory (or not a regular file), absent from the manifest.
    Unlisted(String),
    /// `SUMS-NAME <name>`: a name outside `[A-Za-z0-9._+-]`, empty, `.` or `..`.
    Name(String),
    /// `SUMS-IO <path>: <reason>`: a file or directory could not be read or written.
    Io(String),
    /// `INPUT-PARSE: <reason>`: the manifest is not the expected JSON.
    Parse(String),
}

impl SumsError {
    /// Stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            SumsError::Missing(_) => "SUMS-MISSING",
            SumsError::Unlisted(_) => "SUMS-UNLISTED",
            SumsError::Name(_) => "SUMS-NAME",
            SumsError::Io(_) => "SUMS-IO",
            SumsError::Parse(_) => "INPUT-PARSE",
        }
    }

    /// Process exit code: 2 for an unreadable manifest structure, 1 otherwise.
    pub fn exit_code(&self) -> u8 {
        match self {
            SumsError::Parse(_) => 2,
            _ => 1,
        }
    }
}

impl fmt::Display for SumsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SumsError::Missing(n) | SumsError::Unlisted(n) => write!(f, "{} {n}", self.code()),
            // Debug quoting: a refused name may hold spaces, newlines or control characters.
            SumsError::Name(n) => write!(f, "{} {n:?}", self.code()),
            SumsError::Io(m) | SumsError::Parse(m) => write!(f, "{}: {m}", self.code()),
        }
    }
}

impl std::error::Error for SumsError {}

/// One summed artefact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub digest: Digest,
    pub size: u64,
}

/// The release version and the sorted artefact list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sums {
    pub release: String,
    pub entries: Vec<Entry>,
}

/// Whether `name` is a valid artefact file name: non-empty, not `.` or `..`, and only
/// `[A-Za-z0-9._+-]` (so no path separator, whitespace or newline).
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'))
}

/// Whether `name` is by its name never summed: the checksum file and its signatures, the
/// checksum statement and its envelopes, and SPDX SBOMs.
pub fn is_unsummed_name(name: &str) -> bool {
    let family = |base: &str| name == base || name.strip_prefix(base).is_some_and(|r| r.starts_with('.'));
    family(SUMS_FILE) || family(STATEMENT_FILE) || name.ends_with(".spdx.json")
}

/// The manifest's release version and its artefact names (`kind` read where present).
fn read_manifest(path: &Path) -> Result<(String, Vec<(String, Option<String>)>), SumsError> {
    let io = |e: std::io::Error| SumsError::Io(format!("{}: {e}", path.display()));
    let file = File::open(path).map_err(io)?;
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes).map_err(io)?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(SumsError::Parse(format!("{} is larger than {MAX_MANIFEST_BYTES} bytes", path.display())));
    }
    let value = parse_with_limits(&bytes, bytes.len(), 16)
        .map_err(|e| SumsError::Parse(format!("{}: {e}", path.display())))?;
    let CValue::Obj(top) = value else {
        return Err(SumsError::Parse("manifest: not an object".into()));
    };
    let release = match top.get("version") {
        Some(CValue::Str(s)) if !s.is_empty() => s.clone(),
        _ => return Err(SumsError::Parse("manifest: `version` must be a non-empty string".into())),
    };
    let Some(CValue::Arr(arts)) = top.get("artefacts") else {
        return Err(SumsError::Parse("manifest: `artefacts` must be an array".into()));
    };
    let mut out = Vec::with_capacity(arts.len());
    for (i, a) in arts.iter().enumerate() {
        let CValue::Obj(a) = a else {
            return Err(SumsError::Parse(format!("manifest: artefacts[{i}] is not an object")));
        };
        let Some(CValue::Str(name)) = a.get("name") else {
            return Err(SumsError::Parse(format!("manifest: artefacts[{i}].name must be a string")));
        };
        let kind = match a.get("kind") {
            None => None,
            Some(CValue::Str(k)) => Some(k.clone()),
            Some(_) => return Err(SumsError::Parse(format!("manifest: artefacts[{i}].kind must be a string"))),
        };
        out.push((name.clone(), kind));
    }
    Ok((release, out))
}

/// SHA-256 and length of a file, read in [`BUFFER_BYTES`] chunks.
pub fn hash_file(path: &Path) -> Result<(Digest, u64), SumsError> {
    let io = |e: std::io::Error| SumsError::Io(format!("{}: {e}", path.display()));
    let mut file = File::open(path).map_err(io)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; BUFFER_BYTES];
    let mut size: u64 = 0;
    loop {
        let n = match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(io(e)),
        };
        hasher.update(buf.get(..n).unwrap_or_default());
        size = size.saturating_add(n as u64);
    }
    Ok((Digest::from_bytes(hasher.finalize().into()), size))
}

/// Checks the manifest against the directory and hashes every summed artefact.
///
/// Errors come back all at once, in a fixed order: name errors first (and alone, since a bad
/// name makes the comparison meaningless), then missing and unlisted files, each sorted.
pub fn compute(manifest: &Path, dir: &Path) -> Result<Sums, Vec<SumsError>> {
    let (release, listed) = read_manifest(manifest).map_err(|e| vec![e])?;

    // Names never summed: by name, or by their manifest kind.
    let mut unsummed: BTreeSet<String> = BTreeSet::new();
    let mut names: BTreeSet<String> = BTreeSet::new();
    let mut errors: BTreeSet<String> = BTreeSet::new();
    for (name, kind) in &listed {
        if kind.as_deref().is_some_and(|k| UNSUMMED_KINDS.contains(&k)) || is_unsummed_name(name) {
            unsummed.insert(name.clone());
            continue;
        }
        if !is_valid_name(name) {
            errors.insert(name.clone());
        } else if !names.insert(name.clone()) {
            return Err(vec![SumsError::Parse(format!("manifest: artefact {name} is listed twice"))]);
        }
    }

    // Directory entries; non-UTF-8 names are refused as SUMS-NAME.
    let io = |e: std::io::Error| vec![SumsError::Io(format!("{}: {e}", dir.display()))];
    let mut present: BTreeMap<String, bool> = BTreeMap::new(); // name -> is a regular file
    for entry in std::fs::read_dir(dir).map_err(io)? {
        let entry = entry.map_err(io)?;
        let raw = entry.file_name();
        let Some(name) = raw.to_str().map(str::to_string) else {
            errors.insert(raw.to_string_lossy().into_owned());
            continue;
        };
        let manifest_family = name == MANIFEST_FILE || name.strip_prefix(MANIFEST_FILE).is_some_and(|r| r.starts_with('.'));
        if unsummed.contains(&name) || is_unsummed_name(&name) || manifest_family {
            continue;
        }
        if !is_valid_name(&name) {
            errors.insert(name);
            continue;
        }
        // Follows symbolic links: a link to a regular file counts as that file.
        let is_file = std::fs::metadata(entry.path()).map_err(io)?.is_file();
        present.insert(name, is_file);
    }
    if !errors.is_empty() {
        return Err(errors.into_iter().map(SumsError::Name).collect());
    }

    let mut out: Vec<SumsError> = Vec::new();
    out.extend(names.iter().filter(|n| !present.contains_key(*n)).cloned().map(SumsError::Missing));
    out.extend(
        present.iter().filter(|(n, is_file)| !names.contains(*n) || !**is_file).map(|(n, _)| SumsError::Unlisted(n.clone())),
    );
    if !out.is_empty() {
        return Err(out);
    }

    // BTreeSet<String> iterates in byte order of the names.
    let mut entries = Vec::with_capacity(names.len());
    for name in names {
        let (digest, size) = hash_file(&dir.join(&name)).map_err(|e| vec![e])?;
        entries.push(Entry { name, digest, size });
    }
    Ok(Sums { release, entries })
}

/// The bytes of `SHA256SUMS`.
pub fn sums_file(sums: &Sums) -> Vec<u8> {
    let mut out = String::with_capacity(sums.entries.len() * 100);
    for e in &sums.entries {
        out.push_str(e.digest.hex());
        out.push_str("  ");
        out.push_str(&e.name);
        out.push('\n');
    }
    out.into_bytes()
}

/// The bytes of `sums-statement.json` (RFC 8785 canonical JSON, no trailing newline).
pub fn statement(sums: &Sums) -> Result<Vec<u8>, SumsError> {
    let arts = sums
        .entries
        .iter()
        .map(|e| {
            let mut o = BTreeMap::new();
            o.insert("name".to_string(), CValue::Str(e.name.clone()));
            o.insert("sha256".to_string(), CValue::Str(e.digest.hex().to_string()));
            o.insert("size".to_string(), CValue::U64(e.size));
            CValue::Obj(o)
        })
        .collect();
    let mut top = BTreeMap::new();
    top.insert("release".to_string(), CValue::Str(sums.release.clone()));
    top.insert("artefacts".to_string(), CValue::Arr(arts));
    to_canonical_bytes(&CValue::Obj(top)).map_err(|e| SumsError::Parse(format!("statement: {e}")))
}

/// `rel sums`: checks, hashes and writes `SHA256SUMS` and `sums-statement.json` into `out`
/// (created if absent). Nothing is written unless every check passes.
pub fn run(manifest: &Path, dir: &Path, out: &Path) -> Result<Sums, Vec<SumsError>> {
    let sums = compute(manifest, dir)?;
    let file = sums_file(&sums);
    let stmt = statement(&sums).map_err(|e| vec![e])?;
    let io = |p: &Path, e: std::io::Error| vec![SumsError::Io(format!("{}: {e}", p.display()))];
    std::fs::create_dir_all(out).map_err(|e| io(out, e))?;
    let p = out.join(SUMS_FILE);
    std::fs::write(&p, file).map_err(|e| io(&p, e))?;
    let p = out.join(STATEMENT_FILE);
    std::fs::write(&p, stmt).map_err(|e| io(&p, e))?;
    Ok(sums)
}
