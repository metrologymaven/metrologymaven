//! The canonical form of a script bundle, so a registered script is frozen and identified by
//! content like a program.
//!
//! The bundle format itself (`AUTO.PY.SDK`) is not specified yet. Until it is, a bundle is a
//! set of UTF-8 text files with relative paths, and its canonical form is the RFC 8785 JSON
//!
//! ```text
//! {"files":{"<path>":"<content>",...},"format":"fid-script-bundle","format_version":1}
//! ```
//!
//! The version digest is SHA-256 of those bytes. Paths are 1 to 255 ASCII bytes of
//! `/`-separated segments of `[A-Za-z0-9._-]`, no empty, `.` or `..` segment; two paths that
//! differ only in ASCII case are refused (they collide on case-insensitive file systems).

use super::LifeError;
use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue};
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};

/// The `format` value.
pub const BUNDLE_FORMAT: &str = "fid-script-bundle";
/// The `format_version` value.
pub const BUNDLE_FORMAT_VERSION: u32 = 1;
/// Most files in a bundle.
pub const MAX_FILES: usize = 256;
/// Longest path, in bytes.
pub const MAX_PATH_BYTES: usize = 255;
/// Largest total content, in bytes.
pub const MAX_CONTENT_BYTES: usize = 4 * 1024 * 1024;
/// Largest canonical form, in bytes (content can grow by escaping, at most six times).
pub const MAX_BUNDLE_BYTES: usize = 6 * MAX_CONTENT_BYTES + 64 * 1024;
const MAX_BUNDLE_DEPTH: usize = 3;

/// A script bundle: path to UTF-8 content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptBundle {
    files: BTreeMap<String, String>,
}

fn valid_path(p: &str) -> bool {
    !p.is_empty()
        && p.len() <= MAX_PATH_BYTES
        && p.split('/').all(|seg| {
            !seg.is_empty()
                && seg != "."
                && seg != ".."
                && seg.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
        })
}

impl ScriptBundle {
    /// Builds a bundle from `(path, bytes)` pairs. Errors (`bad_params`): no files, too many
    /// files or bytes, a bad path, a path given twice or twice up to ASCII case, content that
    /// is not UTF-8.
    pub fn new(files: Vec<(String, Vec<u8>)>) -> Result<ScriptBundle, LifeError> {
        if files.is_empty() || files.len() > MAX_FILES {
            return Err(LifeError::bad("a bundle holds 1 to 256 files"));
        }
        let mut total = 0usize;
        let mut folded = BTreeSet::new();
        let mut out = BTreeMap::new();
        for (path, bytes) in files {
            if !valid_path(&path) {
                return Err(LifeError::bad("a bundle path is not a valid relative path"));
            }
            if !folded.insert(path.to_ascii_lowercase()) {
                return Err(LifeError::bad("a bundle path is given twice (ignoring case)"));
            }
            total = total.saturating_add(bytes.len());
            if total > MAX_CONTENT_BYTES {
                return Err(LifeError::bad("bundle content is too large"));
            }
            let text = String::from_utf8(bytes).map_err(|_| LifeError::bad("bundle content is not UTF-8"))?;
            out.insert(path, text);
        }
        Ok(ScriptBundle { files: out })
    }

    /// The files, by path.
    pub fn files(&self) -> &BTreeMap<String, String> {
        &self.files
    }

    fn to_value(&self) -> CValue {
        let files = self.files.iter().map(|(p, c)| (p.clone(), CValue::Str(c.clone()))).collect();
        let mut o = BTreeMap::new();
        o.insert("files".to_owned(), CValue::Obj(files));
        o.insert("format".to_owned(), CValue::Str(BUNDLE_FORMAT.to_owned()));
        o.insert("format_version".to_owned(), CValue::Num(f64::from(BUNDLE_FORMAT_VERSION)));
        CValue::Obj(o)
    }

    /// The canonical bytes. `None` only if canonicalisation fails (it cannot for strings).
    pub fn to_canonical_bytes(&self) -> Option<Vec<u8>> {
        to_canonical_bytes(&self.to_value()).ok()
    }

    /// SHA-256 of the canonical bytes: the version digest.
    pub fn digest(&self) -> Option<Digest> {
        self.to_canonical_bytes().map(|b| Digest::from_bytes(digest_bytes(&b).0))
    }

    /// Reads canonical bytes strictly: anything that is not exactly the canonical form of a
    /// valid bundle is refused.
    pub fn parse(bytes: &[u8]) -> Result<ScriptBundle, LifeError> {
        let bad = || LifeError::bad("not the canonical form of a script bundle");
        let v = parse_with_limits(bytes, MAX_BUNDLE_BYTES, MAX_BUNDLE_DEPTH).map_err(|_| bad())?;
        let CValue::Obj(o) = v else { return Err(bad()) };
        let Some(CValue::Obj(files)) = o.get("files") else { return Err(bad()) };
        let files = files
            .iter()
            .map(|(p, c)| match c {
                CValue::Str(c) => Ok((p.clone(), c.clone().into_bytes())),
                _ => Err(bad()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let b = ScriptBundle::new(files)?;
        if b.to_canonical_bytes().as_deref() != Some(bytes) {
            return Err(bad());
        }
        Ok(b)
    }
}
