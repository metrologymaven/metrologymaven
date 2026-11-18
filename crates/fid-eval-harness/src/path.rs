//! Relative-path rules and confinement.

use crate::{ErrorCode, SandboxError};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Longest accepted relative path, in bytes (the ustar name limit).
pub(crate) const MAX_PATH_BYTES: usize = 255;

/// Windows device names, reserved with or without an extension.
const DEVICES: [&str; 4] = ["con", "prn", "aux", "nul"];

fn is_device_name(seg: &str) -> bool {
    let stem = seg.split('.').next().unwrap_or(seg).to_ascii_lowercase();
    let b = stem.as_bytes();
    DEVICES.contains(&stem.as_str())
        || (b.len() == 4 && (stem.starts_with("com") || stem.starts_with("lpt")) && b[3].is_ascii_digit())
}

/// Checks that `s` is a portable relative path: `/`-separated segments of
/// `[A-Za-z0-9._-]`, never empty, `.` or `..`, never ending in a dot, never a Windows device
/// name, at most 255 bytes. Absolute paths, drive letters, backslashes, alternate data
/// streams and 8.3 short names cannot be expressed. Returns the reason on failure.
pub fn check_rel(s: &str) -> Result<(), &'static str> {
    if s.is_empty() {
        return Err("empty path");
    }
    if s.len() > MAX_PATH_BYTES {
        return Err("path longer than 255 bytes");
    }
    for seg in s.split('/') {
        if seg.is_empty() {
            return Err("empty segment (absolute path or doubled slash)");
        }
        if seg == "." || seg == ".." {
            return Err("`.` or `..` segment");
        }
        if !seg.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-')) {
            return Err("character outside [A-Za-z0-9._-]");
        }
        if seg.ends_with('.') {
            return Err("segment ends with a dot");
        }
        if is_device_name(seg) {
            return Err("Windows device name");
        }
    }
    Ok(())
}

/// The case-folded key under which two paths collide on a case-insensitive file system.
/// Paths are ASCII (see [`check_rel`]), so ASCII folding is exact.
pub(crate) fn fold(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// True when `a` is `b` or an ancestor of `b` (both folded).
pub(crate) fn is_same_or_ancestor(a: &str, b: &str) -> bool {
    b == a || (b.len() > a.len() && b.starts_with(a) && b.as_bytes().get(a.len()) == Some(&b'/'))
}

fn io_err(p: &Path, e: std::io::Error) -> SandboxError {
    SandboxError::new(ErrorCode::Io, format!("{}: {e}", p.display()))
}

/// Resolves `rel` below `base` without leaving it: `rel` must pass [`check_rel`], and no
/// existing component may be a symbolic link or junction, nor a non-directory before the
/// last segment. Components that do not exist yet are allowed.
pub(crate) fn confine(base: &Path, rel: &str) -> Result<PathBuf, SandboxError> {
    check_rel(rel).map_err(|why| SandboxError::new(ErrorCode::PathEscape, format!("{rel}: {why}")))?;
    let segs: Vec<&str> = rel.split('/').collect();
    let mut p = base.to_path_buf();
    let mut exists = true;
    for (i, seg) in segs.iter().enumerate() {
        p.push(seg);
        if !exists {
            continue;
        }
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(SandboxError::new(
                    ErrorCode::PathEscape,
                    format!("{rel}: passes through a symbolic link or junction"),
                ));
            }
            Ok(m) if !m.is_dir() && i + 1 < segs.len() => {
                return Err(SandboxError::new(ErrorCode::PathEscape, format!("{rel}: passes through a non-directory")));
            }
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => exists = false,
            Err(e) => return Err(io_err(&p, e)),
        }
    }
    Ok(p)
}

/// Creates the folders of `rel` below `base` one segment at a time. An existing segment must
/// be a real directory (never a symbolic link or junction). Returns the full path.
pub(crate) fn create_dirs(base: &Path, rel: &str) -> Result<PathBuf, SandboxError> {
    check_rel(rel).map_err(|why| SandboxError::new(ErrorCode::PathEscape, format!("{rel}: {why}")))?;
    let mut p = base.to_path_buf();
    for seg in rel.split('/') {
        p.push(seg);
        match std::fs::create_dir(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                let m = std::fs::symlink_metadata(&p).map_err(|e| io_err(&p, e))?;
                if m.file_type().is_symlink() || !m.is_dir() {
                    return Err(SandboxError::new(
                        ErrorCode::PathEscape,
                        format!("{rel}: an existing component is not a real directory"),
                    ));
                }
            }
            Err(e) => return Err(io_err(&p, e)),
        }
    }
    Ok(p)
}

/// Creates the new file `rel` below `base` (its folders are created first) and writes `data`.
/// Fails if anything already exists at that path, including a dangling link.
pub(crate) fn create_file(base: &Path, rel: &str, data: &[u8]) -> Result<(), SandboxError> {
    use std::io::Write;
    check_rel(rel).map_err(|why| SandboxError::new(ErrorCode::PathEscape, format!("{rel}: {why}")))?;
    let p = match rel.rsplit_once('/') {
        Some((dir, name)) => create_dirs(base, dir)?.join(name),
        None => base.join(rel),
    };
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&p).map_err(|e| io_err(&p, e))?;
    f.write_all(data).map_err(|e| io_err(&p, e))?;
    f.sync_all().map_err(|e| io_err(&p, e))
}
