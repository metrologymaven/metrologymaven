//! Source access for the implementation digest: the [`SourceTree`] trait and its file-system
//! implementation [`FsTree`]. This is the only place the module touches files (build time
//! only, Design of `KRN.MAN.MANI.IMPL`).
//!
//! Files are read as they are on disk, byte for byte: no line-ending or encoding conversion.
//! That the working tree holds the committed bytes is checked in CI (REQ-KRN.MAN.MANI.IMPL-003).
//! [`FsTree`] fails closed on symbolic links, non-UTF-8 or `\`-containing names, special
//! files and anything over the bounds below, so the file list does not depend on the
//! operating system's link or name handling.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use super::{ImplError, IoKind};

/// Largest source file read, in bytes.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// Most files read below one crate's `src/`.
pub const MAX_FILES_PER_DIR: usize = 20_000;
/// Deepest directory nesting below `src/`.
pub const MAX_DIR_DEPTH: usize = 32;

/// Read access to the sources of a workspace. Paths are `/`-separated and relative to the
/// workspace root.
pub trait SourceTree {
    /// Every regular file below directory `dir`, recursively, as (path relative to `dir`,
    /// bytes). Order is free: the caller sorts.
    fn files_under(&self, dir: &str) -> Result<Vec<(String, Vec<u8>)>, ImplError>;
    /// The bytes of the regular file `path`.
    fn read_file(&self, path: &str) -> Result<Vec<u8>, ImplError>;
}

/// A workspace checked out at a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsTree {
    root: PathBuf,
}

fn io(path: &str, kind: IoKind) -> ImplError {
    ImplError::Io { path: path.to_string(), kind }
}

fn kind_of(e: &std::io::Error) -> IoKind {
    match e.kind() {
        std::io::ErrorKind::NotFound => IoKind::NotFound,
        std::io::ErrorKind::PermissionDenied => IoKind::PermissionDenied,
        _ => IoKind::Read,
    }
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// Reads at most [`MAX_FILE_BYTES`]; `shown` is the path used in errors.
pub(crate) fn read_bounded(path: &Path, shown: &str, limit: u64) -> Result<Vec<u8>, ImplError> {
    let f = File::open(path).map_err(|e| io(shown, kind_of(&e)))?;
    let mut buf = Vec::new();
    f.take(limit.saturating_add(1)).read_to_end(&mut buf).map_err(|e| io(shown, kind_of(&e)))?;
    if buf.len() as u64 > limit {
        return Err(io(shown, IoKind::TooLarge));
    }
    Ok(buf)
}

impl FsTree {
    /// The workspace checked out at `root`.
    pub fn new(root: impl Into<PathBuf>) -> FsTree {
        FsTree { root: root.into() }
    }

    fn path(&self, rel: &str) -> PathBuf {
        let mut p = self.root.clone();
        for c in rel.split('/').filter(|c| !c.is_empty()) {
            p.push(c);
        }
        p
    }
}

impl SourceTree for FsTree {
    fn files_under(&self, dir: &str) -> Result<Vec<(String, Vec<u8>)>, ImplError> {
        let base = self.path(dir);
        let md = fs::symlink_metadata(&base).map_err(|e| io(dir, kind_of(&e)))?;
        if md.file_type().is_symlink() {
            return Err(io(dir, IoKind::Symlink));
        }
        if !md.is_dir() {
            return Err(io(dir, IoKind::NotADirectory));
        }
        let mut out = Vec::new();
        // (directory on disk, its path relative to `dir`, depth below `dir`)
        let mut stack: Vec<(PathBuf, String, usize)> = vec![(base, String::new(), 0)];
        while let Some((path, rel, depth)) = stack.pop() {
            let shown = join(dir, &rel);
            let mut names = Vec::new();
            for e in fs::read_dir(&path).map_err(|e| io(&shown, kind_of(&e)))? {
                let e = e.map_err(|e| io(&shown, kind_of(&e)))?;
                let ft = e.file_type().map_err(|e| io(&shown, kind_of(&e)))?;
                let name = e.file_name().into_string().map_err(|_| io(&shown, IoKind::NonUtf8Name))?;
                names.push((name, ft));
            }
            // Sorted so that the first error reported does not depend on directory order.
            names.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, ft) in names.into_iter().rev() {
                let child = join(&rel, &name);
                let child_shown = join(dir, &child);
                if name.contains('\\') || name.contains('/') {
                    return Err(io(&child_shown, IoKind::BadName));
                }
                if ft.is_symlink() {
                    return Err(io(&child_shown, IoKind::Symlink));
                } else if ft.is_dir() {
                    if depth + 1 > MAX_DIR_DEPTH {
                        return Err(io(&child_shown, IoKind::TooDeep));
                    }
                    stack.push((path.join(&name), child, depth + 1));
                } else if ft.is_file() {
                    if out.len() >= MAX_FILES_PER_DIR {
                        return Err(io(&child_shown, IoKind::TooManyFiles));
                    }
                    let bytes = read_bounded(&path.join(&name), &child_shown, MAX_FILE_BYTES)?;
                    out.push((child, bytes));
                } else {
                    return Err(io(&child_shown, IoKind::NotAFile));
                }
            }
        }
        Ok(out)
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>, ImplError> {
        let p = self.path(path);
        let md = fs::symlink_metadata(&p).map_err(|e| io(path, kind_of(&e)))?;
        if md.file_type().is_symlink() {
            return Err(io(path, IoKind::Symlink));
        }
        if !md.is_file() {
            return Err(io(path, IoKind::NotAFile));
        }
        read_bounded(&p, path, MAX_FILE_BYTES)
    }
}
