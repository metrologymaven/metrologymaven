//! The validator behind `corpus-check`.

use crate::error::CorpError;
use crate::fetch::{fetch_to_cache, hash_file, FetchError, Fetcher};
use crate::manifest::{FormatId, Location, Manifest};
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;

/// SPDX IDs whose files may be stored in the repository (REQ-IO.CORP.MANIF-002). Starting list
/// from the leaf's Test fixtures section [unverified: to align with ECO.RULES under D-001].
pub const REDISTRIBUTION_ALLOWLIST: [&str; 6] =
    ["CC0-1.0", "CC-BY-4.0", "MIT", "Apache-2.0", "BSL-1.0", "BSD-3-Clause"];

/// Default size cap per corpus file: the `IO.REG.LIMITS` default byte limit, 64 GiB.
pub const DEFAULT_MAX_BYTES: u64 = 64 * 1024 * 1024 * 1024;

/// Largest accepted manifest file.
pub const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

/// Whether an in-repository file under `licence` may be redistributed. `unknown` never may.
pub fn is_redistributable(licence: &str) -> bool {
    REDISTRIBUTION_ALLOWLIST.contains(&licence)
}

/// Fetching of `url` entries: where verified bytes are cached and who fetches them.
#[derive(Clone, Copy)]
pub struct Fetching<'a> {
    pub cache: &'a Path,
    pub fetcher: &'a dyn Fetcher,
}

/// Options of [`check_corpus`].
#[derive(Clone, Copy)]
pub struct CheckOptions<'a> {
    /// Size cap per file, committed or fetched.
    pub max_bytes: u64,
    /// `None` checks manifests and committed files only; `url` entries are then counted as skipped.
    pub fetch: Option<Fetching<'a>>,
}

impl Default for CheckOptions<'_> {
    fn default() -> Self {
        CheckOptions { max_bytes: DEFAULT_MAX_BYTES, fetch: None }
    }
}

/// What a check found.
#[derive(Debug, Default)]
pub struct Report {
    /// Parsed manifests by format.
    pub manifests: BTreeMap<FormatId, Manifest>,
    /// Every failure, in a deterministic order (manifests by name, then entries in file order).
    pub errors: Vec<CorpError>,
    /// Committed files whose digest was checked.
    pub files_checked: usize,
    /// `url` entries fetched now, and those served from a valid cache file.
    pub fetched: usize,
    pub cached: usize,
    /// `url` entries not fetched because fetching was off.
    pub urls_skipped: usize,
}

impl Report {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

fn read_capped(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let f = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    f.take(max.saturating_add(1)).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    if buf.len() as u64 > max {
        return Err(format!("larger than {max} bytes"));
    }
    Ok(buf)
}

/// Reads every `<format-id>.toml` in `dir`, sorted by name. Other files are ignored.
pub fn load_manifests(dir: &Path) -> (BTreeMap<FormatId, Manifest>, Vec<CorpError>) {
    let mut manifests = BTreeMap::new();
    let mut errors = Vec::new();
    let read = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) => {
            errors.push(CorpError::malformed(dir.display().to_string(), format!("cannot read corpus directory: {e}")));
            return (manifests, errors);
        }
    };
    let mut files = BTreeMap::new();
    for item in read {
        let Ok(item) = item else { continue };
        let name = item.file_name();
        let Some(name) = name.to_str() else { continue };
        if let Some(stem) = name.strip_suffix(".toml") {
            files.insert(stem.to_string(), item.path());
        }
    }
    for (stem, path) in files {
        let text = read_capped(&path, MAX_MANIFEST_BYTES)
            .and_then(|b| String::from_utf8(b).map_err(|_| "not UTF-8".to_string()));
        let parsed = match text {
            Ok(text) => Manifest::parse(&text, &stem),
            Err(e) => Err(CorpError::malformed(format!("{stem}.toml"), e)),
        };
        match parsed {
            Ok(m) => {
                manifests.insert(m.format.clone(), m);
            }
            Err(e) => errors.push(e),
        }
    }
    (manifests, errors)
}

/// Checks the corpus in `dir` against the registered first-party `formats`:
///
/// - every format has a manifest (`manifest_missing`, REQ-IO.CORP.MANIF-001);
/// - manifests parse (`malformed`) and no digest appears twice (`malformed`);
/// - in-repository entries carry an allowlisted licence (`licence_not_redistributable`,
///   REQ-IO.CORP.MANIF-002) and match their digest (`digest_mismatch`, REQ-IO.CORP.MANIF-004);
/// - with fetching on, `url` entries are fetched and verified (`digest_mismatch`,
///   REQ-IO.CORP.MANIF-003; `fetch_failed`).
///
/// Manifests for formats that are not registered are still checked.
pub fn check_corpus(dir: &Path, formats: &[FormatId], opts: &CheckOptions<'_>) -> Report {
    let (manifests, mut errors) = load_manifests(dir);
    let mut report = Report::default();
    let malformed_stems: Vec<String> = errors
        .iter()
        .filter_map(|e| match e {
            CorpError::Malformed { at, .. } => at.split(".toml").next().map(str::to_string),
            _ => None,
        })
        .collect();
    for f in formats {
        // A format whose manifest exists but is malformed is reported once, as malformed.
        if !manifests.contains_key(f) && !malformed_stems.iter().any(|s| s == f.as_str()) {
            errors.push(CorpError::ManifestMissing { format: f.to_string() });
        }
    }
    let mut seen: BTreeMap<Digest, String> = BTreeMap::new();
    for m in manifests.values() {
        for e in &m.entries {
            let name = format!("{}: {}", m.format, e.location.as_str());
            if let Some(first) = seen.get(&e.sha256) {
                errors.push(CorpError::malformed(name, format!("sha256 {} repeats entry {first}", e.sha256)));
            } else {
                seen.insert(e.sha256, name);
            }
        }
    }
    for m in manifests.values() {
        for e in &m.entries {
            let format = m.format.to_string();
            let entry = e.location.as_str().to_string();
            match &e.location {
                Location::Path(rel) => {
                    if !is_redistributable(&e.licence) {
                        errors.push(CorpError::LicenceNotRedistributable {
                            format: format.clone(),
                            entry: entry.clone(),
                            licence: e.licence.clone(),
                        });
                    }
                    report.files_checked += 1;
                    match hash_file(&dir.join(rel), opts.max_bytes) {
                        Ok(actual) if actual == e.sha256 => {}
                        Ok(actual) => errors.push(CorpError::DigestMismatch {
                            format,
                            entry,
                            expected: Box::new(e.sha256),
                            actual: Box::new(actual),
                        }),
                        Err(detail) => errors.push(CorpError::Io { format, entry, detail }),
                    }
                }
                Location::Url(url) => match opts.fetch {
                    None => report.urls_skipped += 1,
                    Some(f) => match fetch_to_cache(f.fetcher, url, &e.sha256, f.cache, opts.max_bytes) {
                        Ok((_, true)) => report.fetched += 1,
                        Ok((_, false)) => report.cached += 1,
                        Err(FetchError::DigestMismatch(actual)) => errors.push(CorpError::DigestMismatch {
                            format,
                            entry,
                            expected: Box::new(e.sha256),
                            actual: Box::new(actual),
                        }),
                        Err(FetchError::Failed(detail)) => {
                            errors.push(CorpError::FetchFailed { format, entry, detail })
                        }
                    },
                },
            }
        }
    }
    report.manifests = manifests;
    report.errors = errors;
    report
}
