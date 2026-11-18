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
            // COM0-9, LPT0-9 and the superscript forms COM¹-³, LPT¹-³ (U+00B9, U+00B2, U+00B3).
            let tail = s.strip_prefix("COM").or_else(|| s.strip_prefix("LPT"));
            match tail {
                Some(t) => {
                    (t.len() == 1 && t.as_bytes()[0].is_ascii_digit()) || ["\u{b9}", "\u{b2}", "\u{b3}"].contains(&t)
                }
                None => false,
            }
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

/// Where an export may be written: the target's folder, held open as a capability, and the
/// file name inside it. The folder handle is not exposed; the only write operation offered
/// here is [`ExportTarget::create_new`]. Overwrite, atomic rename and permissions are
/// `IO.REG.WRITE`'s job and must go through this folder handle.
#[derive(Debug)]
pub struct ExportTarget {
    dir: Dir,
    name: OsString,
    root: Option<RootId>,
    path: PathBuf,
}

impl ExportTarget {
    /// The file name inside the target folder.
    pub fn name(&self) -> &OsStr {
        &self.name
    }

    /// The root it belongs to; `None` for a human's raw path.
    pub fn root(&self) -> Option<&RootId> {
        self.root.as_ref()
    }

    /// The link-free path relative to the root, or the raw path as given.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Creates the target file for writing through the held folder handle. It must not
    /// exist yet (`target_exists` otherwise, also for a link of that name, which is never
    /// followed).
    pub fn create_new(&self) -> Result<File, IoError> {
        let mut opts = cap_std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        match self.dir.open_with(&self.name, &opts) {
            Ok(f) => Ok(f.into_std()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(IoError::TargetExists),
            Err(e) => Err(io_err(e)),
        }
    }
}

#[derive(Debug, Clone)]
struct RootEntry {
    access: RootAccess,
    path: PathBuf,
}

/// A test hook run between the link walk and the open.
type RaceHook = Box<dyn Fn() + Send + Sync>;

/// The configured roots (read-only here: configuration belongs to CORE settings) and the
/// resolver.
#[derive(Default)]
pub struct RootSet {
    roots: BTreeMap<RootId, RootEntry>,
    fs_calls: AtomicU64,
    race_hook: Option<RaceHook>,
}

impl fmt::Debug for RootSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RootSet")
            .field("roots", &self.roots)
            .field("fs_calls", &self.fs_calls)
            .field("race_hook", &self.race_hook.is_some())
            .finish()
    }
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

    /// Test seam for REQ-IO.REG.ROOTS-005: `hook` runs after the link walk and before the
    /// open, so a test can swap a checked component for an escaping link in between.
    #[doc(hidden)]
    pub fn set_race_hook(&mut self, hook: impl Fn() + Send + Sync + 'static) {
        self.race_hook = Some(Box::new(hook));
    }

    fn before_open(&self) {
        if let Some(h) = &self.race_hook {
            h();
        }
    }

    /// Maps a failed open through the root handle. `cap-std` refuses an escape with its
    /// own error; that error, or a fresh walk that now finds an escaping link (the path was
    /// swapped after the first walk), gives `path_outside_root`.
    fn open_failed(&self, e: io::Error, canon: &Path, rel: &[String]) -> IoError {
        if is_escape(&e) {
            return IoError::PathOutsideRoot;
        }
        match self.walk(canon, rel) {
            Err(IoError::PathOutsideRoot) => IoError::PathOutsideRoot,
            _ => io_err(e),
        }
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
                // A human's raw path follows links, but must still name a regular file:
                // checked before the open and again on the handle.
                self.tick();
                if !std::fs::metadata(input).map_err(io_err)?.is_file() {
                    return Err(not_a_file());
                }
                let mut opts = std::fs::OpenOptions::new();
                opts.read(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    opts.custom_flags(NO_BLOCK_FLAGS);
                }
                self.tick();
                let file = opts.open(input).map_err(io_err)?;
                let warnings = self.check_file(&file)?;
                return Ok(ImportFile { file, root: None, path: PathBuf::from(input), warnings });
            }
        };
        let (dir, canon, comps) = self.resolve(Direction::Import, &handle)?;
        if comps.is_empty() {
            return Err(not_a_file());
        }
        let rel: PathBuf = comps.iter().collect();
        self.before_open();
        // Only a regular file is opened: a FIFO or device would block or have side effects.
        self.tick();
        match dir.symlink_metadata(&rel) {
            Ok(m) if m.is_file() => {}
            Ok(_) => return Err(not_a_file()),
            Err(e) => return Err(self.open_failed(e, &canon, handle.rel.components())),
        }
        let mut opts = cap_std::fs::OpenOptions::new();
        opts.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // Never block on a FIFO swapped in after the check; never take a controlling
            // terminal.
            opts.custom_flags(NO_BLOCK_FLAGS);
        }
        self.tick();
        let file = match dir.open_with(&rel, &opts) {
            Ok(f) => f.into_std(),
            Err(e) => return Err(self.open_failed(e, &canon, handle.rel.components())),
        };
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
        let (root_dir, canon, mut comps) = self.resolve(Direction::Export, &handle)?;
        let name = comps.pop().ok_or_else(not_a_file)?;
        let parent: PathBuf = comps.iter().collect();
        self.before_open();
        let dir = if comps.is_empty() {
            root_dir
        } else {
            self.tick();
            match root_dir.open_dir(&parent) {
                Ok(d) => d,
                Err(e) => return Err(self.open_failed(e, &canon, handle.rel.components())),
            }
        };
        let path = parent.join(&name);
        Ok(ExportTarget { dir, name, root: Some(handle.root), path })
    }

    /// Looks up the root, opens it, and walks the handle's path to a link-free path below
    /// it.
    fn resolve(&self, dir: Direction, handle: &RootHandle) -> Result<(Dir, PathBuf, Vec<OsString>), IoError> {
        let entry = self.roots.get(&handle.root).filter(|e| e.access.allows(dir)).ok_or(IoError::RootNotFound)?;
        self.tick();
        let canon = std::fs::canonicalize(&entry.path).map_err(io_err)?;
        self.tick();
        let root_dir = Dir::open_ambient_dir(&canon, cap_std::ambient_authority()).map_err(io_err)?;
        let comps = self.walk(&canon, handle.rel.components())?;
        Ok((root_dir, canon, comps))
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

/// `O_NONBLOCK | O_NOCTTY`: an open that cannot hang on a FIFO or adopt a terminal.
#[cfg(unix)]
const NO_BLOCK_FLAGS: i32 = libc::O_NONBLOCK | libc::O_NOCTTY;

/// True for `cap-std`'s refusal of a path that would leave the directory it was opened
/// through (`PermissionDenied`, "a path led outside of the filesystem"), and for Linux's
/// `EXDEV` from `openat2(RESOLVE_BENEATH)`.
fn is_escape(e: &io::Error) -> bool {
    if e.kind() == io::ErrorKind::PermissionDenied && e.to_string().contains("outside of the filesystem") {
        return true;
    }
    #[cfg(target_os = "linux")]
    {
        if e.raw_os_error() == Some(libc::EXDEV) {
            return true;
        }
    }
    false
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

/// Name equality for the containment check: on Windows, ASCII letters compare without
/// case and every other code point must match exactly; elsewhere names must match
/// exactly. Any fold the file system does beyond that (Unicode case, a case-insensitive
/// macOS volume, 8.3 short names) can only make an in-root target look outside, which
/// fails closed; it never makes an outside target look inside.
fn same_part(a: &KeyPart, b: &KeyPart) -> bool {
    match (a, b) {
        (KeyPart::Head(x), KeyPart::Head(y)) => x.eq_ignore_ascii_case(y),
        (KeyPart::Name(x), KeyPart::Name(y)) => {
            if cfg!(windows) {
                x.as_encoded_bytes().eq_ignore_ascii_case(y.as_encoded_bytes())
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
    fn superscript_device_names() {
        for c in ["COM\u{b9}", "lpt\u{b2}.txt", "Com\u{b3}"] {
            assert!(windows_device(c), "{c}");
            assert!(!plain_name(c), "{c}");
        }
        assert!(!windows_device("COM\u{b4}"));
    }

    #[test]
    fn names_fold_ascii_only() {
        let n = |s: &str| KeyPart::Name(OsString::from(s));
        // KELVIN SIGN folds to `k` under Unicode lower-casing; it must not match here.
        assert!(!same_part(&n("\u{212a}"), &n("k")));
        assert!(!same_part(&n("\u{212a}"), &n("K")));
        assert!(!same_part(&n("\u{c4}"), &n("\u{e4}")));
        assert!(same_part(&n("\u{c4}"), &n("\u{c4}")));
        assert_eq!(same_part(&n("ABC"), &n("abc")), cfg!(windows));
        assert!(same_part(&n("abc"), &n("abc")));
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
