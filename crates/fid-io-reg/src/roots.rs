//! Approved import and export root handles (spec node `IO.REG.ROOTS`, REQ-IO.REG-008,
//! REQ-IO.REG-009).
//!
//! Agents and scripts never pass raw file paths (CONSTITUTION §3): they name a file as
//! `root://<root-id>/<relative path>`, and [`RootSet`] resolves the handle against the
//! configured import and export roots. Resolution runs in three steps:
//!
//! 1. **Lexical check** ([`RootHandle::parse`], no file-system call): `..`, an absolute
//!    component, a drive letter or any `:`, a UNC prefix, an empty component and Windows
//!    device names fail `path_outside_root`; a non-handle from an agent or a script fails
//!    `raw_path_forbidden`; an unknown root, or one not configured for the direction, fails
//!    `root_not_found`.
//! 2. **Link walk**: the relative path is walked component by component below the
//!    canonical root, without following links. Every symbolic link or junction (Windows
//!    name-surrogate reparse point) is read and expanded in place; a target that leaves the
//!    root fails `path_outside_root`, one that stays inside resumes the walk. The result is
//!    a link-free path relative to the root.
//! 3. **Open relative to the root handle**: the root directory is opened once as a
//!    capability ([`cap_std::fs::Dir`]) and the final file (import) or its parent folder
//!    (export) is opened through it. `cap-std` refuses any open that would leave that
//!    directory, so a component swapped for an escaping link after step 2 makes the open
//!    fail instead of redirecting it (REQ-IO.REG.ROOTS-005).
//!
//! A human principal may also pass a raw path; roots apply to agents and scripts only.

use crate::error::{IoError, Position};
use crate::limits::Principal;
use cap_std::fs::Dir;
use std::collections::{BTreeMap, VecDeque};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::File;
use std::io;
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::atomic::{AtomicU64, Ordering};

/// The handle scheme.
pub const SCHEME: &str = "root://";

/// The most links one resolution may expand before it fails `io` (a link loop).
pub const MAX_LINK_HOPS: usize = 40;

/// Maximum length of a root ID.
pub const MAX_ROOT_ID_LEN: usize = 64;

/// The direction a root is used in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Direction {
    /// Reading a source file.
    Import,
    /// Writing a result file.
    Export,
}

impl Direction {
    /// The stable wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Import => "import",
            Direction::Export => "export",
        }
    }
}

/// The directions a configured root serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RootAccess {
    /// Import only.
    Import,
    /// Export only.
    Export,
    /// Both.
    ImportExport,
}

impl RootAccess {
    /// True if this root may be used for `dir`.
    pub fn allows(self, dir: Direction) -> bool {
        matches!(
            (self, dir),
            (RootAccess::ImportExport, _)
                | (RootAccess::Import, Direction::Import)
                | (RootAccess::Export, Direction::Export)
        )
    }
}

/// A root ID: 1 to [`MAX_ROOT_ID_LEN`] ASCII letters, digits, `-` or `_`, starting with a
/// letter or digit.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RootId(String);

impl RootId {
    /// Validates a root ID; `None` if it breaks the syntax.
    pub fn new(id: &str) -> Option<RootId> {
        let b = id.as_bytes();
        let ok = !b.is_empty()
            && b.len() <= MAX_ROOT_ID_LEN
            && b[0].is_ascii_alphanumeric()
            && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-' || *c == b'_');
        if ok {
            Some(RootId(id.to_string()))
        } else {
            None
        }
    }

    /// The ID text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RootId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A lexically checked path relative to a root: plain names only (no `..`, no `.`, no
/// separators, no drive or stream colons, no Windows device names). Empty means the root
/// itself.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelPath(Vec<String>);

impl RelPath {
    /// Checks the relative part of a handle. `/` and `\` both separate components on every
    /// platform; `.` components are dropped. Anything else that could leave the root fails
    /// `path_outside_root`. No file-system call is made.
    pub fn parse(rel: &str) -> Result<RelPath, IoError> {
        if rel.is_empty() {
            return Ok(RelPath(Vec::new()));
        }
        let mut out = Vec::new();
        for c in rel.split(['/', '\\']) {
            if c == "." {
                continue;
            }
            if !plain_name(c) {
                return Err(IoError::PathOutsideRoot);
            }
            out.push(c.to_string());
        }
        Ok(RelPath(out))
    }

    /// The components, in order.
    pub fn components(&self) -> &[String] {
        &self.0
    }

    /// True for the root itself.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.join("/"))
    }
}

/// True if `c` is a plain file or folder name that cannot name anything but a child of
/// its parent folder on any supported platform.
fn plain_name(c: &str) -> bool {
    if c.is_empty() || c.bytes().all(|b| b == b'.' || b == b' ') {
        // Empty (`//`, a leading separator: absolute or UNC) or `..`, and dot-and-space
        // names Windows may normalise to `..`.
        return false;
    }
    if c.bytes().any(|b| b == b':' || b < 0x20) {
        // Drive letters, alternate data streams, NUL and control bytes.
        return false;
    }
    if c.ends_with('.') || c.ends_with(' ') {
        // Windows strips trailing dots and spaces, so the name would alias another.
        return false;
    }
    !windows_device(c)
}

/// Windows reserved device names (with any extension), rejected on every platform so the
/// outcome of the lexical check does not depend on the OS.
fn windows_device(c: &str) -> bool {
    let stem = c.split('.').next().unwrap_or(c).trim_end_matches(' ').to_ascii_uppercase();
    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" => true,
        s => {
            let b = s.as_bytes();
            b.len() == 4 && (s.starts_with("COM") || s.starts_with("LPT")) && b[3].is_ascii_digit()
        }
    }
}

/// A parsed `root://<root-id>/<relative path>` handle.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RootHandle {
    /// The root.
    pub root: RootId,
    /// The path below it.
    pub rel: RelPath,
}

impl RootHandle {
    /// Parses a handle without touching the file system. Text without the `root://` scheme
    /// fails `raw_path_forbidden`; a bad relative part fails `path_outside_root` (checked
    /// first); a malformed root ID fails `root_not_found`.
    pub fn parse(text: &str) -> Result<RootHandle, IoError> {
        let rest = text.strip_prefix(SCHEME).ok_or(IoError::RawPathForbidden)?;
        let (id, rel) = match rest.find(['/', '\\']) {
            Some(i) => (&rest[..i], &rest[i + 1..]),
            None => (rest, ""),
        };
        let rel = RelPath::parse(rel)?;
        let root = RootId::new(id).ok_or(IoError::RootNotFound)?;
        Ok(RootHandle { root, rel })
    }
}

impl fmt::Display for RootHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{SCHEME}{}/{}", self.root, self.rel)
    }
}

/// A non-fatal finding about a resolved file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RootWarning {
    /// The file has more than one hard link. A hard link inside a root may share its data
    /// with a file outside it; that cannot be told by path, so it is reported, not refused.
    HardLinked {
        /// The link count.
        links: u64,
    },
}

impl RootWarning {
    /// The stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            RootWarning::HardLinked { .. } => "hard_linked",
        }
    }
}

/// A file opened for import.
#[derive(Debug)]
pub struct ImportFile {
    /// The open file, positioned at its start.
    pub file: File,
    /// The root it came from; `None` for a human's raw path.
    pub root: Option<RootId>,
    /// The link-free path relative to the root, or the raw path as given.
    pub path: PathBuf,
    /// Non-fatal findings.
    pub warnings: Vec<RootWarning>,
}

/// Where an export may be written: an open handle to the target's folder and the file name
/// inside it. Writing is `IO.REG.WRITE`'s job; it must create the file through [`Self::dir`]
/// so the write stays relative to the checked folder.
#[derive(Debug)]
pub struct ExportTarget {
    /// The target's folder, opened as a capability.
    pub dir: Dir,
    /// The file name inside `dir`.
    pub name: OsString,
    /// The root it belongs to; `None` for a human's raw path.
    pub root: Option<RootId>,
    /// The link-free path relative to the root, or the raw path as given.
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
struct RootEntry {
    access: RootAccess,
    path: PathBuf,
}

/// The configured roots (read-only here: configuration belongs to CORE settings) and the
/// resolver.
#[derive(Debug, Default)]
pub struct RootSet {
    roots: BTreeMap<RootId, RootEntry>,
    fs_calls: AtomicU64,
}

impl RootSet {
    /// An empty set.
    pub fn new() -> RootSet {
        RootSet::default()
    }

    /// Adds or replaces a root.
    pub fn insert(&mut self, id: RootId, access: RootAccess, path: impl Into<PathBuf>) {
        self.roots.insert(id, RootEntry { access, path: path.into() });
    }

    /// Number of file-system calls made by this set so far (for tests that check a
    /// rejection happened before any file-system access).
    pub fn fs_calls(&self) -> u64 {
        self.fs_calls.load(Ordering::Relaxed)
    }

    fn tick(&self) {
        self.fs_calls.fetch_add(1, Ordering::Relaxed);
    }

    /// Classifies the input: `Some(handle)`, `None` for a raw path a human may use, or
    /// `raw_path_forbidden`.
    fn classify(principal: Principal, input: &str) -> Result<Option<RootHandle>, IoError> {
        if input.starts_with(SCHEME) {
            return RootHandle::parse(input).map(Some);
        }
        match principal {
            Principal::Human => Ok(None),
            Principal::Agent | Principal::Script => Err(IoError::RawPathForbidden),
        }
    }

    /// Resolves `input` (a handle, or for a human also a raw path) and opens the file for
    /// reading.
    pub fn resolve_import(&self, principal: Principal, input: &str) -> Result<ImportFile, IoError> {
        let handle = match RootSet::classify(principal, input)? {
            Some(h) => h,
            None => {
                self.tick();
                let file = File::open(input).map_err(io_err)?;
                let warnings = self.check_file(&file)?;
                return Ok(ImportFile { file, root: None, path: PathBuf::from(input), warnings });
            }
        };
        let (dir, comps) = self.resolve(Direction::Import, &handle)?;
        if comps.is_empty() {
            return Err(not_a_file());
        }
        let rel: PathBuf = comps.iter().collect();
        self.tick();
        let file = dir.open(&rel).map_err(io_err)?.into_std();
        let warnings = self.check_file(&file)?;
        Ok(ImportFile { file, root: Some(handle.root), path: rel, warnings })
    }

    /// Resolves `input` (a handle, or for a human also a raw path) to the folder an export
    /// is written into and the file name inside it. The folder must exist.
    pub fn resolve_export(&self, principal: Principal, input: &str) -> Result<ExportTarget, IoError> {
        let handle = match RootSet::classify(principal, input)? {
            Some(h) => h,
            None => {
                let p = Path::new(input);
                let name = p.file_name().ok_or_else(not_a_file)?.to_os_string();
                let parent = match p.parent() {
                    Some(q) if !q.as_os_str().is_empty() => q,
                    _ => Path::new("."),
                };
                self.tick();
                let dir = Dir::open_ambient_dir(parent, cap_std::ambient_authority()).map_err(io_err)?;
                return Ok(ExportTarget { dir, name, root: None, path: p.to_path_buf() });
            }
        };
        let (root_dir, mut comps) = self.resolve(Direction::Export, &handle)?;
        let name = comps.pop().ok_or_else(not_a_file)?;
        let parent: PathBuf = comps.iter().collect();
        let dir = if comps.is_empty() {
            root_dir
        } else {
            self.tick();
            root_dir.open_dir(&parent).map_err(io_err)?
        };
        let path = parent.join(&name);
        Ok(ExportTarget { dir, name, root: Some(handle.root), path })
    }

    /// Looks up the root, opens it, and walks the handle's path to a link-free path below
    /// it.
    fn resolve(&self, dir: Direction, handle: &RootHandle) -> Result<(Dir, Vec<OsString>), IoError> {
        let entry = self.roots.get(&handle.root).filter(|e| e.access.allows(dir)).ok_or(IoError::RootNotFound)?;
        self.tick();
        let canon = std::fs::canonicalize(&entry.path).map_err(io_err)?;
        self.tick();
        let root_dir = Dir::open_ambient_dir(&canon, cap_std::ambient_authority()).map_err(io_err)?;
        let comps = self.walk(&canon, handle.rel.components())?;
        Ok((root_dir, comps))
    }

    /// Walks `rel` below `root` (canonical) without following links, expanding each link in
    /// place and refusing any whose target leaves the root.
    fn walk(&self, root: &Path, rel: &[String]) -> Result<Vec<OsString>, IoError> {
        let root_key = path_key(root).ok_or_else(|| io_err(io::Error::from(io::ErrorKind::InvalidInput)))?;
        let mut done: Vec<OsString> = Vec::new();
        // Leading components of `done` that were seen on disk; below a missing folder
        // nothing exists, so nothing more is looked up.
        let mut seen = 0usize;
        let mut todo: VecDeque<OsString> = rel.iter().map(OsString::from).collect();
        let mut hops = 0usize;
        while let Some(c) = todo.pop_front() {
            if c.as_os_str() == OsStr::new(".") {
                continue;
            }
            if c.as_os_str() == OsStr::new("..") {
                // Every component in `done` is a real folder (links were expanded), so
                // `..` is its parent.
                if done.pop().is_none() {
                    return Err(IoError::PathOutsideRoot);
                }
                seen = seen.min(done.len());
                continue;
            }
            if done.len() > seen {
                done.push(c);
                continue;
            }
            let mut p = root.to_path_buf();
            p.extend(done.iter());
            p.push(&c);
            self.tick();
            let meta = match std::fs::symlink_metadata(&p) {
                Ok(m) => m,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    done.push(c);
                    continue;
                }
                Err(e) => return Err(io_err(e)),
            };
            if !meta.file_type().is_symlink() {
                done.push(c);
                seen = done.len();
                continue;
            }
            hops += 1;
            if hops > MAX_LINK_HOPS {
                return Err(io_err(io::Error::from(io::ErrorKind::Other)));
            }
            self.tick();
            let target = std::fs::read_link(&p).map_err(io_err)?;
            let (from_root, comps) = link_components(&target, &root_key)?;
            if from_root {
                done.clear();
                seen = 0;
            }
            for x in comps.into_iter().rev() {
                todo.push_front(x);
            }
        }
        Ok(done)
    }

    /// Refuses anything but a regular file and reports a hard-link count above one.
    fn check_file(&self, file: &File) -> Result<Vec<RootWarning>, IoError> {
        self.tick();
        let meta = file.metadata().map_err(io_err)?;
        if !meta.is_file() {
            return Err(not_a_file());
        }
        Ok(match link_count(&meta) {
            Some(links) if links > 1 => vec![RootWarning::HardLinked { links }],
            _ => Vec::new(),
        })
    }
}

#[cfg(unix)]
fn link_count(meta: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(meta.nlink())
}

/// Windows exposes the link count only through an unstable std API; not reported there.
#[cfg(not(unix))]
fn link_count(_meta: &std::fs::Metadata) -> Option<u64> {
    None
}

fn io_err(e: io::Error) -> IoError {
    IoError::Io { kind: e.kind(), at: Position::default() }
}

fn not_a_file() -> IoError {
    io_err(io::Error::from(io::ErrorKind::InvalidInput))
}

/// One element of an absolute path's comparison key: the head (drive, UNC share or `/`)
/// or a name.
#[derive(Debug, Clone)]
enum KeyPart {
    Head(String),
    Name(OsString),
}

/// The head and the following components of an absolute path, or `None` if `p` is not
/// absolute (Windows drive-relative and root-relative paths are not). `.` is dropped; `..`
/// is kept as a name so the walk can apply it after expanding links.
fn path_key(p: &Path) -> Option<Vec<KeyPart>> {
    let mut parts = Vec::new();
    let mut it = p.components();
    match it.next()? {
        Component::RootDir => parts.push(KeyPart::Head("/".to_string())),
        Component::Prefix(pre) => {
            let head = match pre.kind() {
                Prefix::Disk(d) | Prefix::VerbatimDisk(d) => format!("{}:", char::from(d).to_ascii_uppercase()),
                Prefix::UNC(s, sh) | Prefix::VerbatimUNC(s, sh) => {
                    format!("\\\\{}\\{}", s.to_string_lossy(), sh.to_string_lossy())
                }
                // Device and other verbatim namespaces are never inside a root.
                Prefix::Verbatim(_) | Prefix::DeviceNS(_) => return None,
            };
            if it.next() != Some(Component::RootDir) {
                return None;
            }
            parts.push(KeyPart::Head(head));
        }
        _ => return None,
    }
    for c in it {
        match c {
            Component::Normal(n) => parts.push(KeyPart::Name(n.to_os_string())),
            Component::ParentDir => parts.push(KeyPart::Name(OsString::from(".."))),
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir => return None,
        }
    }
    Some(parts)
}

/// Name equality as the platform's default file systems see it: ASCII case-insensitive
/// on Windows, exact elsewhere (a case-insensitive macOS volume can only make an in-root
/// target look outside, which fails closed).
fn same_part(a: &KeyPart, b: &KeyPart) -> bool {
    match (a, b) {
        (KeyPart::Head(x), KeyPart::Head(y)) => x.eq_ignore_ascii_case(y),
        (KeyPart::Name(x), KeyPart::Name(y)) => {
            if cfg!(windows) {
                x.to_string_lossy().to_lowercase() == y.to_string_lossy().to_lowercase()
            } else {
                x == y
            }
        }
        _ => false,
    }
}

/// The components a link target contributes to the walk, and whether they start at the
/// root (absolute target) or at the link's folder (relative target). An absolute target
/// must start with the canonical root's own components, with no `..` before the root is
/// reached; otherwise `path_outside_root`.
fn link_components(target: &Path, root_key: &[KeyPart]) -> Result<(bool, Vec<OsString>), IoError> {
    let first = target.components().next();
    let absolute = matches!(first, Some(Component::Prefix(_)) | Some(Component::RootDir));
    if !absolute {
        let mut out = Vec::new();
        for c in target.components() {
            match c {
                Component::Normal(n) => out.push(n.to_os_string()),
                Component::ParentDir => out.push(OsString::from("..")),
                Component::CurDir => {}
                Component::Prefix(_) | Component::RootDir => return Err(IoError::PathOutsideRoot),
            }
        }
        return Ok((false, out));
    }
    let key = path_key(target).ok_or(IoError::PathOutsideRoot)?;
    if key.len() < root_key.len() || !root_key.iter().zip(key.iter()).all(|(a, b)| same_part(a, b)) {
        return Err(IoError::PathOutsideRoot);
    }
    let mut out = Vec::new();
    for part in key.into_iter().skip(root_key.len()) {
        match part {
            KeyPart::Name(n) => out.push(n),
            KeyPart::Head(_) => return Err(IoError::PathOutsideRoot),
        }
    }
    Ok((true, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names() {
        for c in ["CON", "nul", "Com1", "lpt9.txt", "aux.tar.gz", "CONIN$"] {
            assert!(windows_device(c), "{c}");
        }
        for c in ["console", "com", "lpt10", "nullx", "a"] {
            assert!(!windows_device(c), "{c}");
        }
    }

    #[test]
    fn root_ids() {
        assert!(RootId::new("imp").is_some());
        assert!(RootId::new("a-b_9").is_some());
        for bad in ["", "-a", "a b", "a.b", "a/b", "ä"] {
            assert!(RootId::new(bad).is_none(), "{bad}");
        }
        assert!(RootId::new(&"a".repeat(MAX_ROOT_ID_LEN + 1)).is_none());
    }
}
