//! A bounded directory walk that never follows symbolic links. What it cannot walk is a
//! `REVIEW-SCHEMA` finding, so a tree that cannot be fully read never passes.

use super::SCHEMA;
use crate::finding::Finding;
use std::path::{Path, PathBuf};

/// Most files and directories walked in one call.
pub const MAX_ENTRIES: usize = 100_000;
/// Deepest directory nesting walked.
pub const MAX_DEPTH: usize = 32;

/// One file or directory found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// `/`-separated path: `rel_dir` joined with the names below it.
    pub rel: String,
    pub path: PathBuf,
    pub is_dir: bool,
    /// File length in bytes (0 for a directory).
    pub len: u64,
}

/// Walks `dir` (whose own path is `rel_dir`, empty for the top) and returns its regular files
/// and directories, sorted by path. Symbolic links, special files, non-UTF-8 names, nesting
/// deeper than [`MAX_DEPTH`] and more than [`MAX_ENTRIES`] entries are findings. `Err` is an
/// I/O failure (exit 2).
pub fn walk(dir: &Path, rel_dir: &str, f: &mut Vec<Finding>) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, String, usize)> = vec![(dir.to_path_buf(), rel_dir.to_string(), 0)];
    while let Some((dir, rel_dir, depth)) = stack.pop() {
        let rd = std::fs::read_dir(&dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let mut entries = Vec::new();
        for e in rd {
            entries.push(e.map_err(|e| format!("cannot read {}: {e}", dir.display()))?);
        }
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for e in entries {
            if out.len() >= MAX_ENTRIES {
                f.push(Finding::new(
                    SCHEMA,
                    or_dot(&rel_dir),
                    format!("more than {MAX_ENTRIES} entries; not fully read"),
                ));
                out.sort_by(|a: &Entry, b: &Entry| a.rel.cmp(&b.rel));
                return Ok(out);
            }
            let name = e.file_name();
            let Some(name) = name.to_str() else {
                f.push(Finding::new(SCHEMA, or_dot(&rel_dir), "a file name is not UTF-8; not read"));
                continue;
            };
            let rel = if rel_dir.is_empty() { name.to_string() } else { format!("{rel_dir}/{name}") };
            let md = std::fs::symlink_metadata(e.path()).map_err(|e2| format!("cannot read {rel}: {e2}"))?;
            if md.file_type().is_symlink() {
                f.push(Finding::new(SCHEMA, &rel, "symbolic link; not followed"));
            } else if md.is_dir() {
                if depth + 1 > MAX_DEPTH {
                    f.push(Finding::new(SCHEMA, &rel, format!("nested deeper than {MAX_DEPTH}; not read")));
                } else {
                    out.push(Entry { rel: rel.clone(), path: e.path(), is_dir: true, len: 0 });
                    stack.push((e.path(), rel, depth + 1));
                }
            } else if md.is_file() {
                out.push(Entry { rel, path: e.path(), is_dir: false, len: md.len() });
            } else {
                f.push(Finding::new(SCHEMA, &rel, "not a regular file or directory; not read"));
            }
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(out)
}

fn or_dot(s: &str) -> &str {
    if s.is_empty() {
        "."
    } else {
        s
    }
}
