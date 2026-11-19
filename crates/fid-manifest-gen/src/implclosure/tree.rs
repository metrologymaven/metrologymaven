//! Source access for the implementation digest: the [`SourceTree`] trait, the git-tracked file
//! list [`TrackedFiles`] and the file-system implementation [`FsTree`]. This is the only place
//! the module touches files or runs a process (build time only, Design of `KRN.MAN.MANI.IMPL`).
//!
//! Only git-tracked files are hashed (REQ-KRN.MAN.MANI.IMPL-003). The tracked list comes from
//! `git ls-files -z --stage` ([`git_tracked_files`], run with a hardened environment) or from
//! the caller ([`TrackedFiles::new`]). It is rejected when two paths are equal under ASCII case
//! folding (they cannot both be checked out on a case-insensitive file system) and when an
//! entry is a symbolic link or a submodule. On disk, a file below a hashed directory that is
//! not tracked is an error rather than being skipped, because the compiler would still read it;
//! a tracked file that is missing is an error too.
//!
//! Files are read as they are on disk, byte for byte: no line-ending or encoding conversion.
//! That the working tree holds the committed bytes is checked in CI. Symbolic links, special
//! files, non-UTF-8 or `\`-containing names and anything over the bounds below are typed
//! errors, so the result does not depend on the operating system's link or name handling.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{ImplError, IoKind};

/// Largest source file read, in bytes.
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// Most files read below one hashed directory.
pub const MAX_FILES_PER_DIR: usize = 20_000;
/// Deepest directory nesting below a hashed directory.
pub const MAX_DIR_DEPTH: usize = 32;
/// Most entries in a tracked-file list.
pub const MAX_TRACKED_FILES: usize = 1_000_000;

/// Read access to the git-tracked sources of a workspace. Paths are `/`-separated and relative
/// to the workspace root.
pub trait SourceTree {
    /// Every tracked file below directory `dir`, recursively, as (path relative to `dir`,
    /// bytes). Order is free: the caller sorts. An untracked file below `dir` is an error.
    fn files_under(&self, dir: &str) -> Result<Vec<(String, Vec<u8>)>, ImplError>;
    /// The bytes of the tracked regular file `path`.
    fn read_file(&self, path: &str) -> Result<Vec<u8>, ImplError>;
    /// The bytes of `path` if it is tracked, `None` if it exists neither in the tracked list nor
    /// on disk; an untracked file on disk is an error.
    fn read_optional(&self, path: &str) -> Result<Option<Vec<u8>>, ImplError>;
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

/// Reads at most `limit` bytes; `shown` is the path used in errors.
pub(crate) fn read_bounded(path: &Path, shown: &str, limit: u64) -> Result<Vec<u8>, ImplError> {
    let f = File::open(path).map_err(|e| io(shown, kind_of(&e)))?;
    let mut buf = Vec::new();
    f.take(limit.saturating_add(1)).read_to_end(&mut buf).map_err(|e| io(shown, kind_of(&e)))?;
    if buf.len() as u64 > limit {
        return Err(io(shown, IoKind::TooLarge));
    }
    Ok(buf)
}

/// The git-tracked regular files of a workspace, `/`-separated and relative to its root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedFiles {
    paths: BTreeSet<String>,
}

fn path_ok(p: &str) -> bool {
    !p.contains('\\') && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
}

impl TrackedFiles {
    /// A tracked list given by the caller (regular files only). Fails on a malformed path, on
    /// two paths equal under ASCII case folding (only `A`-`Z` are folded; non-ASCII letters are
    /// compared as they are) and on more than [`MAX_TRACKED_FILES`].
    pub fn new<I: IntoIterator<Item = String>>(paths: I) -> Result<TrackedFiles, ImplError> {
        let mut set = BTreeSet::new();
        let mut folded: BTreeMap<String, String> = BTreeMap::new();
        for p in paths {
            if !path_ok(&p) {
                return Err(io(&p, IoKind::BadName));
            }
            if let Some(other) = folded.insert(p.to_ascii_lowercase(), p.clone()) {
                if other != p {
                    return Err(ImplError::InputInvalid { reason: "tracked_case_collision" });
                }
            }
            set.insert(p);
            if set.len() > MAX_TRACKED_FILES {
                return Err(ImplError::InputInvalid { reason: "too_many_tracked_files" });
            }
        }
        Ok(TrackedFiles { paths: set })
    }

    /// Parses `git ls-files -z --stage` output (`<mode> <object> <stage>\t<path>\0` records).
    /// Only mode `100644` and `100755` entries are accepted; a symbolic link (`120000`) or a
    /// submodule (`160000`) is an error, as is a non-UTF-8 path.
    pub fn from_ls_files_stage(output: &[u8]) -> Result<TrackedFiles, ImplError> {
        let mut paths = Vec::new();
        for rec in output.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            let tab = rec.iter().position(|b| *b == b'\t').ok_or_else(|| io("git ls-files", IoKind::Git))?;
            let raw = &rec[tab + 1..];
            let path = std::str::from_utf8(raw).map_err(|_| io(&String::from_utf8_lossy(raw), IoKind::NonUtf8Name))?;
            match rec.get(..7) {
                Some(b"100644 ") | Some(b"100755 ") => paths.push(path.to_string()),
                Some(b"120000 ") => return Err(io(path, IoKind::Symlink)),
                _ => return Err(io(path, IoKind::NotAFile)),
            }
        }
        TrackedFiles::new(paths)
    }

    /// Whether `path` is tracked.
    pub fn contains(&self, path: &str) -> bool {
        self.paths.contains(path)
    }

    /// Every tracked path, sorted.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.paths.iter().map(String::as_str)
    }

    /// Tracked paths strictly below directory `dir` (`""` for the root).
    fn under(&self, dir: &str) -> Vec<&str> {
        let prefix = join(dir, "");
        self.paths
            .range::<str, _>((Bound::Included(prefix.as_str()), Bound::Unbounded))
            .map(String::as_str)
            .take_while(|p| p.starts_with(prefix.as_str()))
            .collect()
    }
}

/// Runs `git ls-files -z --stage` in `workspace` under the hardened rules for repositories the
/// tool did not create: an empty environment apart from `PATH` and `SYSTEMROOT`, no system or
/// global configuration, no prompts, no lazy fetch, every transport protocol disabled, no
/// fsmonitor and no hooks. `safe.directory` is never widened. Paths are relative to
/// `workspace`.
pub fn git_tracked_files(workspace: &Path) -> Result<TrackedFiles, ImplError> {
    let devnull = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let mut cmd = Command::new("git");
    cmd.env_clear();
    for key in ["PATH", "SYSTEMROOT"] {
        if let Some(v) = std::env::var_os(key) {
            cmd.env(key, v);
        }
    }
    cmd.env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", devnull)
        .env("GIT_TERMINAL_PROMPT", "0")
        .arg("--no-lazy-fetch");
    for proto in ["", ".file", ".ext", ".ssh", ".git", ".http", ".https"] {
        cmd.arg("-c").arg(format!("protocol{proto}.allow=never"));
    }
    cmd.arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg(format!("core.hooksPath={devnull}"))
        .arg("-C")
        .arg(workspace)
        .args(["ls-files", "-z", "--stage"])
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    let shown = workspace.display().to_string();
    let out = cmd.output().map_err(|_| io(&shown, IoKind::Git))?;
    if !out.status.success() {
        return Err(io(&shown, IoKind::Git));
    }
    TrackedFiles::from_ls_files_stage(&out.stdout)
}

/// A workspace checked out at a directory, with its tracked-file list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsTree {
    root: PathBuf,
    tracked: TrackedFiles,
}

impl FsTree {
    /// The workspace checked out at `root` whose tracked files are `tracked`.
    pub fn new(root: impl Into<PathBuf>, tracked: TrackedFiles) -> FsTree {
        FsTree { root: root.into(), tracked }
    }

    fn path(&self, rel: &str) -> PathBuf {
        let mut p = self.root.clone();
        for c in rel.split('/').filter(|c| !c.is_empty()) {
            p.push(c);
        }
        p
    }

    /// Every file on disk below `dir` (paths relative to the root). Fails on links, special
    /// files, odd names and the bounds; a missing `dir` gives an empty list.
    fn disk_files(&self, dir: &str) -> Result<Vec<String>, ImplError> {
        let base = self.path(dir);
        let md = match fs::symlink_metadata(&base) {
            Ok(md) => md,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io(dir, kind_of(&e))),
        };
        if md.file_type().is_symlink() {
            return Err(io(dir, IoKind::Symlink));
        }
        if !md.is_dir() {
            return Err(io(dir, IoKind::NotADirectory));
        }
        let mut out = Vec::new();
        // (directory on disk, its path relative to the root, depth below `dir`)
        let mut stack: Vec<(PathBuf, String, usize)> = vec![(base, dir.to_string(), 0)];
        while let Some((path, rel, depth)) = stack.pop() {
            let mut names = Vec::new();
            for e in fs::read_dir(&path).map_err(|e| io(&rel, kind_of(&e)))? {
                let e = e.map_err(|e| io(&rel, kind_of(&e)))?;
                let ft = e.file_type().map_err(|e| io(&rel, kind_of(&e)))?;
                let name = e.file_name().into_string().map_err(|_| io(&rel, IoKind::NonUtf8Name))?;
                names.push((name, ft));
            }
            // Sorted so that the first error reported does not depend on directory order.
            names.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, ft) in names {
                let child = join(&rel, &name);
                if name.contains('\\') || name.contains('/') {
                    return Err(io(&child, IoKind::BadName));
                }
                if ft.is_symlink() {
                    return Err(io(&child, IoKind::Symlink));
                } else if ft.is_dir() {
                    if depth + 1 > MAX_DIR_DEPTH {
                        return Err(io(&child, IoKind::TooDeep));
                    }
                    stack.push((path.join(&name), child, depth + 1));
                } else if ft.is_file() {
                    if out.len() >= MAX_FILES_PER_DIR {
                        return Err(io(&child, IoKind::TooManyFiles));
                    }
                    out.push(child);
                } else {
                    return Err(io(&child, IoKind::NotAFile));
                }
            }
        }
        Ok(out)
    }

    /// Reads a tracked path from disk: it must be a regular file, not a link.
    fn read_tracked(&self, path: &str) -> Result<Vec<u8>, ImplError> {
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

impl SourceTree for FsTree {
    fn files_under(&self, dir: &str) -> Result<Vec<(String, Vec<u8>)>, ImplError> {
        for f in self.disk_files(dir)? {
            if !self.tracked.contains(&f) {
                return Err(io(&f, IoKind::Untracked));
            }
        }
        let tracked = self.tracked.under(dir);
        if tracked.is_empty() {
            return Err(io(dir, IoKind::NotFound));
        }
        if tracked.len() > MAX_FILES_PER_DIR {
            return Err(io(dir, IoKind::TooManyFiles));
        }
        let skip = join(dir, "").len();
        let mut out = Vec::with_capacity(tracked.len());
        for path in tracked {
            let rel = path.get(skip..).unwrap_or_default();
            if rel.split('/').count() > MAX_DIR_DEPTH + 1 {
                return Err(io(path, IoKind::TooDeep));
            }
            out.push((rel.to_string(), self.read_tracked(path)?));
        }
        Ok(out)
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>, ImplError> {
        match self.read_optional(path)? {
            Some(bytes) => Ok(bytes),
            None => Err(io(path, IoKind::NotFound)),
        }
    }

    fn read_optional(&self, path: &str) -> Result<Option<Vec<u8>>, ImplError> {
        if self.tracked.contains(path) {
            return self.read_tracked(path).map(Some);
        }
        match fs::symlink_metadata(self.path(path)) {
            Ok(_) => Err(io(path, IoKind::Untracked)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io(path, kind_of(&e))),
        }
    }
}
