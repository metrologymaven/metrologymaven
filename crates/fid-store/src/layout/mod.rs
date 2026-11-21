//! Project directory layout, write lock and cross-platform file behaviour (spec node
//! `CORE.STO.LAYOUT`).
//!
//! A project is a folder holding exactly these store-managed entries:
//!
//! | entry | kind |
//! |---|---|
//! | `format` | layout marker: `fid-project-layout@1` and the project UUID |
//! | `project.db` (+ `-wal`, `-shm`, `-journal`) | database, created empty here (schema: `CORE.STO.SQL`) |
//! | `blobs/` | content-addressed blobs (`CORE.STO.CAS`) |
//! | `kernels/` | kernel artifacts carried with the project |
//! | `tmp/` | scratch space |
//! | `lock` | write lock: OS lock plus holder PID, host and start time |
//!
//! Every name the store creates is one of these fixed lowercase ASCII names or a lowercase
//! hex digest ([`HexName`]), so no two names can differ only in case or Unicode
//! normalisation and no user text becomes a name.
//!
//! Paths: all work below the project root goes through a directory handle (`openat` family on
//! Linux and macOS, `\\?\` verbatim paths on Windows), so project roots of up to
//! [`MAX_PATH_CHARS`] characters work on all three systems, including macOS where the path
//! syscalls stop at 1,023 bytes.
//!
//! Locking: on Linux and macOS an exclusive `flock` on `lock`; on Windows the `lock` file is
//! held open with a share mode that denies other writers and renames (read sharing stays
//! allowed so a second process can read the holder's PID and host). Both are released by the
//! OS when the holder exits, however it exits. On Unix a lock only counts if the locked file is
//! still the one at `lock` (device and inode), so a file replaced by rename is not trusted.
//!
//! Links: the user's path to the project root may contain symbolic links; below the root the
//! store never follows a symbolic link (or, on Windows, a junction or reparse point) and fails
//! with `symlink_refused` instead.
//!
//! Recovery: a lock record from another host, or a garbled one, is never taken over
//! automatically; [`ProjectLocation::force_unlock`] clears it after a human confirms the holder.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
mod sys_unix;
#[cfg(unix)]
use sys_unix as sys;
#[cfg(windows)]
mod sys_windows;
#[cfg(windows)]
use sys_windows as sys;

mod folder;
pub use folder::{CopyTarget, FolderId, VerifiedFolder};
mod copy;
pub use copy::{set_copy_hook, CopyStep, COPY_PREFIX};

/// Layout version this build writes and the newest it can open.
pub const LAYOUT_VERSION: u32 = 1;
/// Prefix of the first line of the `format` file.
pub const FORMAT_TAG: &str = "fid-project-layout@";
/// Layout marker file.
pub const FORMAT_FILE: &str = "format";
/// Project database file.
pub const DB_FILE: &str = "project.db";
/// SQLite companion files of [`DB_FILE`] (created by the database, never by this module).
pub const DB_COMPANIONS: [&str; 3] = ["project.db-wal", "project.db-shm", "project.db-journal"];
/// Blob store directory.
pub const BLOBS_DIR: &str = "blobs";
/// Carried kernel artifacts directory.
pub const KERNELS_DIR: &str = "kernels";
/// Scratch directory.
pub const TMP_DIR: &str = "tmp";
/// Write lock file.
pub const LOCK_FILE: &str = "lock";
/// Longest supported absolute project path, in characters (REQ-CORE.STO.LAYOUT-006).
pub const MAX_PATH_CHARS: usize = 1024;
/// First line of the lock file.
pub const LOCK_TAG: &str = "fid-lock@1";

/// Upper bound for the small text files this module parses (`format`, `lock`).
const SMALL_FILE_LIMIT: u64 = 4096;
/// Deepest directory nesting [`ProjectLocation::list`] descends into.
const MAX_LIST_DEPTH: u32 = 16;

/// The entries a freshly created project holds (sorted).
pub const CREATED_ENTRIES: [&str; 6] = [BLOBS_DIR, FORMAT_FILE, KERNELS_DIR, LOCK_FILE, DB_FILE, TMP_DIR];

/// How a project is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    Read,
    Write,
}

/// The data areas below the project root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Blobs,
    Kernels,
    Tmp,
}

impl Area {
    /// The directory name of the area.
    pub fn dir_name(self) -> &'static str {
        match self {
            Area::Blobs => BLOBS_DIR,
            Area::Kernels => KERNELS_DIR,
            Area::Tmp => TMP_DIR,
        }
    }
}

/// A name the store may create inside an area: 2 to 128 lowercase hex digits.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct HexName(String);

impl HexName {
    /// Validates `s`; anything but 2..=128 characters of `[0-9a-f]` is `invalid_name`.
    pub fn new(s: &str) -> Result<HexName, StoreError> {
        let ok = (2..=128).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if ok {
            Ok(HexName(s.to_string()))
        } else {
            Err(StoreError::InvalidName(s.chars().take(64).collect()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// True if `name` is a name this store may create inside a project: a fixed entry name or a
/// [`HexName`].
pub fn is_store_name(name: &str) -> bool {
    CREATED_ENTRIES.contains(&name) || DB_COMPANIONS.contains(&name) || HexName::new(name).is_ok()
}

/// Who holds (or held) a project's write lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockInfo {
    pub pid: u32,
    pub host: String,
    /// Seconds since the Unix epoch at which the holder took the lock.
    pub started: u64,
}

/// Non-fatal events of an open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreWarning {
    /// The lock file named a holder on this host whose OS lock was gone (a crashed process);
    /// the lock was taken over. Carries the previous holder.
    StaleLockTaken(LockInfo),
}

impl StoreWarning {
    pub fn code(&self) -> &'static str {
        match self {
            StoreWarning::StaleLockTaken(_) => "stale_lock_taken",
        }
    }
}

/// Typed storage errors.
#[derive(Debug)]
pub enum StoreError {
    /// Another process holds the write lock. `None` if the holder's record could not be read.
    Locked(Option<LockInfo>),
    /// The project folder is on a network file system; write opens are refused.
    NetworkFsUnsupported,
    /// `format` names a newer layout than this build supports.
    LayoutTooNew {
        found: u32,
        supported: u32,
    },
    /// The absolute project path is longer than [`MAX_PATH_CHARS`].
    PathTooLong {
        chars: usize,
    },
    /// The folder is not a project (missing or malformed `format`, missing areas).
    NotAProject(String),
    /// `create` was given a folder that is not empty.
    NotEmpty,
    /// A name failed [`HexName`] validation.
    InvalidName(String),
    /// The lock file exists but cannot be parsed, so its holder's host is unknown.
    LockFileInvalid,
    /// An entry below the project root is a symbolic link (or, on Windows, a junction or other
    /// reparse point); the store never follows those inside a project.
    SymlinkRefused,
    /// `force_unlock` was confirmed with a holder record that is not the one in the lock file.
    LockMismatch(Option<LockInfo>),
    /// An entry below the project root that must be a regular file is something else (a FIFO,
    /// device or socket); it is refused rather than opened.
    NotRegularFile,
    /// The held lock file is no longer the file at `lock` (replaced by rename or deleted), so
    /// the lock no longer excludes other writers; writes stop.
    LockLost,
    /// The folder opened is not the folder `CORE.BUS.POL.FS` verified (another folder, or a
    /// link to one, was swapped in); code `outside_root` (REQ-011).
    FolderSwapped,
    /// A write open or a create on a folder resolved in an import root (REQ-011).
    ReadOnlyTarget,
    /// A read-only target whose database cannot be read without creating or writing a
    /// `-wal` or `-shm` file (REQ-011).
    ReadOnlyUnavailable,
    /// A copied object does not match its digest, or the source lacks an object its database
    /// references (REQ-012).
    BlobCorrupt(String),
    Io(io::Error),
}

impl StoreError {
    /// Stable snake_case code.
    pub fn code(&self) -> &'static str {
        match self {
            StoreError::Locked(_) => "locked",
            StoreError::NetworkFsUnsupported => "network_fs_unsupported",
            StoreError::LayoutTooNew { .. } => "layout_too_new",
            StoreError::PathTooLong { .. } => "path_too_long",
            StoreError::NotAProject(_) => "not_a_project",
            StoreError::NotEmpty => "not_empty",
            StoreError::InvalidName(_) => "invalid_name",
            StoreError::LockFileInvalid => "lock_file_invalid",
            StoreError::SymlinkRefused => "symlink_refused",
            StoreError::LockMismatch(_) => "lock_mismatch",
            StoreError::NotRegularFile => "not_regular_file",
            StoreError::LockLost => "lock_lost",
            StoreError::FolderSwapped => "outside_root",
            StoreError::ReadOnlyTarget => "read_only_target",
            StoreError::ReadOnlyUnavailable => "read_only_unavailable",
            StoreError::BlobCorrupt(_) => "blob_corrupt",
            StoreError::Io(_) => "io",
        }
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Locked(Some(i)) => write!(f, "locked: held by pid {} on host {}", i.pid, i.host),
            StoreError::Locked(None) => write!(f, "locked: held by another process"),
            StoreError::NetworkFsUnsupported => write!(f, "network_fs_unsupported: write opens need a local disk"),
            StoreError::LayoutTooNew { found, supported } => {
                write!(f, "layout_too_new: layout {found}, this build supports up to {supported}")
            }
            StoreError::PathTooLong { chars } => {
                write!(f, "path_too_long: {chars} characters, limit {MAX_PATH_CHARS}")
            }
            StoreError::NotAProject(why) => write!(f, "not_a_project: {why}"),
            StoreError::NotEmpty => write!(f, "not_empty: a project is created in an empty folder"),
            StoreError::InvalidName(n) => write!(f, "invalid_name: {n:?}"),
            StoreError::LockFileInvalid => write!(f, "lock_file_invalid: lock file is not readable"),
            StoreError::SymlinkRefused => write!(f, "symlink_refused: links are not followed inside a project"),
            StoreError::LockMismatch(_) => write!(f, "lock_mismatch: the lock file names a different holder"),
            StoreError::NotRegularFile => write!(f, "not_regular_file: expected a regular file"),
            StoreError::LockLost => write!(f, "lock_lost: the lock file was replaced or removed"),
            StoreError::FolderSwapped => write!(f, "outside_root: the folder is not the one that was verified"),
            StoreError::ReadOnlyTarget => write!(f, "read_only_target: the folder was resolved in an import root"),
            StoreError::ReadOnlyUnavailable => {
                write!(f, "read_only_unavailable: the database cannot be read without writing")
            }
            StoreError::BlobCorrupt(why) => write!(f, "blob_corrupt: {why}"),
            StoreError::Io(e) => write!(f, "io: {e}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StoreError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for StoreError {
    fn from(e: io::Error) -> Self {
        match e.get_ref().and_then(|inner| inner.downcast_ref::<Refusal>()) {
            Some(Refusal::Link) => StoreError::SymlinkRefused,
            Some(Refusal::NotRegular) => StoreError::NotRegularFile,
            None => StoreError::Io(e),
        }
    }
}

/// Marker carried inside an `io::Error` by the platform layer when it refuses an entry.
#[derive(Debug)]
enum Refusal {
    Link,
    NotRegular,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Link => write!(f, "link inside the project refused"),
            Refusal::NotRegular => write!(f, "not a regular file"),
        }
    }
}

impl std::error::Error for Refusal {}

fn link_refused() -> io::Error {
    io::Error::other(Refusal::Link)
}

fn not_regular() -> io::Error {
    io::Error::other(Refusal::NotRegular)
}

/// An opened project folder.
#[derive(Debug)]
pub struct ProjectLocation {
    root: PathBuf,
    uuid: String,
    layout_version: u32,
    dir: sys::Dir,
}

/// The write lock of a project. Dropping it clears the holder record and releases the OS lock.
#[derive(Debug)]
pub struct Locked {
    handle: Option<sys::LockHandle>,
    dir: sys::Dir,
    info: LockInfo,
    warnings: Vec<StoreWarning>,
}

impl ProjectLocation {
    /// Creates a project in `path` (created if missing; must be empty) and returns it with its
    /// write lock held.
    pub fn create(path: &Path) -> Result<(ProjectLocation, Locked), StoreError> {
        let root = absolute_checked(path)?;
        let dir = sys::Dir::open(&root, true)?;
        ProjectLocation::create_in(root, dir)
    }

    /// The body of [`ProjectLocation::create`] once the folder is open.
    fn create_in(root: PathBuf, dir: sys::Dir) -> Result<(ProjectLocation, Locked), StoreError> {
        if !dir.entries()?.is_empty() {
            return Err(StoreError::NotEmpty);
        }
        if dir.is_network()? {
            return Err(StoreError::NetworkFsUnsupported);
        }
        let locked = acquire(&dir)?;
        for d in [BLOBS_DIR, KERNELS_DIR, TMP_DIR] {
            dir.sub(d, true)?;
        }
        dir.write_file(DB_FILE, b"")?;
        let uuid = new_uuid();
        // Written last: a folder without `format` is not a project.
        dir.write_file(FORMAT_FILE, format_text(LAYOUT_VERSION, &uuid).as_bytes())?;
        Ok((ProjectLocation { root, uuid, layout_version: LAYOUT_VERSION, dir }, locked))
    }

    /// Opens the project in `path`. `Write` takes the write lock and fails with `locked`,
    /// `network_fs_unsupported` or `layout_too_new` as the spec requires; `Read` takes no lock.
    pub fn open(path: &Path, mode: OpenMode) -> Result<(ProjectLocation, Option<Locked>), StoreError> {
        let root = absolute_checked(path)?;
        let dir = sys::Dir::open(&root, false)?;
        let text = match dir.read_file(FORMAT_FILE, SMALL_FILE_LIMIT) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(StoreError::NotAProject(format!("no {FORMAT_FILE} file")))
            }
            Err(e) => return Err(StoreError::from(e)),
        };
        let (layout_version, uuid) = parse_format(&text)?;
        for d in [BLOBS_DIR, KERNELS_DIR, TMP_DIR] {
            match dir.sub(d, false).map_err(StoreError::from) {
                Ok(_) => {}
                Err(StoreError::SymlinkRefused) => return Err(StoreError::SymlinkRefused),
                Err(_) => return Err(StoreError::NotAProject(format!("missing {d}/"))),
            }
        }
        let loc = ProjectLocation { root, uuid, layout_version, dir };
        match mode {
            OpenMode::Read => Ok((loc, None)),
            OpenMode::Write => {
                if loc.dir.is_network()? {
                    return Err(StoreError::NetworkFsUnsupported);
                }
                let locked = acquire(&loc.dir)?;
                Ok((loc, Some(locked)))
            }
        }
    }

    /// Manual recovery for a lock the store will not take over by itself: a record from another
    /// host (for example after the machine was renamed, or a project folder copied between
    /// machines) or a garbled record. A human reads the holder from the `locked` error, makes
    /// sure that process is really gone, and confirms by passing that same record back
    /// (`None` for a garbled record). Fails with `locked` while any process holds the OS lock,
    /// and with `lock_mismatch` if the record changed. Never called automatically.
    pub fn force_unlock(path: &Path, confirm: Option<&LockInfo>) -> Result<(), StoreError> {
        let root = absolute_checked(path)?;
        let dir = sys::Dir::open(&root, false)?;
        // Only ever touch the lock of something that is a project.
        match dir.read_file(FORMAT_FILE, SMALL_FILE_LIMIT) {
            Ok(text) => {
                parse_format(&text)?;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(StoreError::NotAProject(format!("no {FORMAT_FILE} file")))
            }
            Err(e) => return Err(StoreError::from(e)),
        }
        let mut handle = match dir.try_lock(LOCK_FILE)? {
            sys::TryLock::Acquired(h) => h,
            sys::TryLock::Busy => {
                let info = dir.read_file(LOCK_FILE, SMALL_FILE_LIMIT).ok().and_then(|b| parse_lock_info(&b).ok());
                return Err(StoreError::Locked(info));
            }
        };
        let current = handle.read_all(SMALL_FILE_LIMIT)?;
        let recorded = parse_lock_info(&current).ok();
        let empty = current.iter().all(u8::is_ascii_whitespace);
        let matches = match confirm {
            Some(c) => recorded.as_ref() == Some(c),
            None => recorded.is_none() && !empty,
        };
        if !matches {
            handle.release();
            return Err(StoreError::LockMismatch(recorded));
        }
        let cleared = handle.replace(b"");
        handle.release();
        cleared.map_err(StoreError::from)
    }

    /// Absolute project root as opened.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Project UUID (lowercase, hyphenated) from `format`.
    pub fn uuid(&self) -> &str {
        &self.uuid
    }

    /// Layout version from `format`.
    pub fn layout_version(&self) -> u32 {
        self.layout_version
    }

    /// `<per-user cache dir>/fiducial/cache/<project uuid>/` (not created). `None` if the
    /// per-user cache directory is unknown or would lie inside the project.
    pub fn cache_dir(&self) -> Option<PathBuf> {
        let base = user_cache_dir(std::env::consts::OS, |k| std::env::var_os(k))?;
        let dir = base.join("fiducial").join("cache").join(&self.uuid);
        if dir.starts_with(&self.root) {
            None
        } else {
            Some(dir)
        }
    }

    /// Reads `<area>/<subdirs...>/<name>`, failing if it is larger than `limit` bytes.
    pub fn read_file(
        &self,
        area: Area,
        subdirs: &[HexName],
        name: &HexName,
        limit: u64,
    ) -> Result<Vec<u8>, StoreError> {
        let d = descend(&self.dir, area, subdirs, false)?;
        Ok(d.read_file(name.as_str(), limit)?)
    }

    /// Recursive listing of the project folder: relative paths joined with `/`, sorted.
    /// Symbolic links are listed but not followed.
    pub fn list(&self) -> Result<Vec<String>, StoreError> {
        let mut out = Vec::new();
        list_into(&self.dir, "", 0, &mut out)?;
        out.sort();
        Ok(out)
    }
}

impl Locked {
    /// The record this process wrote into `lock`.
    pub fn info(&self) -> &LockInfo {
        &self.info
    }

    /// Checks that the lock still excludes other writers: the held lock file must still be the
    /// file at `lock` (on Unix a sync tool or user could rename another file over it, after
    /// which a second process could lock that one). Every mutating call runs this first.
    pub fn verify(&self) -> Result<(), StoreError> {
        let Some(h) = self.handle.as_ref() else { return Err(StoreError::LockLost) };
        if h.is_current(&self.dir, LOCK_FILE)? {
            Ok(())
        } else {
            Err(StoreError::LockLost)
        }
    }

    /// Warnings raised while taking the lock.
    pub fn warnings(&self) -> &[StoreWarning] {
        &self.warnings
    }

    /// Writes `<area>/<subdirs...>/<name>` (creating the subdirectories), replacing any
    /// previous content, and flushes it. Crash-safe commits are `CORE.STO.SQL`'s job.
    pub fn write_file(&self, area: Area, subdirs: &[HexName], name: &HexName, bytes: &[u8]) -> Result<(), StoreError> {
        self.verify()?;
        let d = descend(&self.dir, area, subdirs, true)?;
        Ok(d.write_file(name.as_str(), bytes)?)
    }
}

impl Drop for Locked {
    fn drop(&mut self) {
        if let Some(mut h) = self.handle.take() {
            // A cleared record marks a clean release; errors cannot be reported from drop and
            // the OS lock goes away with the handle either way.
            let _ = h.replace(b"");
            h.release();
        }
    }
}

fn descend(root: &sys::Dir, area: Area, subdirs: &[HexName], create: bool) -> Result<sys::Dir, StoreError> {
    let mut d = root.sub(area.dir_name(), false)?;
    for s in subdirs {
        d = d.sub(s.as_str(), create)?;
    }
    Ok(d)
}

fn list_into(dir: &sys::Dir, prefix: &str, depth: u32, out: &mut Vec<String>) -> io::Result<()> {
    if depth > MAX_LIST_DEPTH {
        return Err(io::Error::other("project listing nested too deeply"));
    }
    for (name, is_dir) in dir.entries()? {
        let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        if is_dir {
            let sub = dir.sub(&name, false)?;
            list_into(&sub, &rel, depth + 1, out)?;
        }
        out.push(rel);
    }
    Ok(())
}

/// Takes the write lock on `dir/lock` (REQ-003..005).
fn acquire(dir: &sys::Dir) -> Result<Locked, StoreError> {
    let mut handle = match dir.try_lock(LOCK_FILE)? {
        sys::TryLock::Acquired(h) => h,
        sys::TryLock::Busy => {
            let info = dir.read_file(LOCK_FILE, SMALL_FILE_LIMIT).ok().and_then(|b| parse_lock_info(&b).ok());
            return Err(StoreError::Locked(info));
        }
    };
    let me = LockInfo { pid: std::process::id(), host: host_name(), started: unix_now() };
    let previous = handle.read_all(SMALL_FILE_LIMIT)?;
    let mut warnings = Vec::new();
    if !previous.iter().all(u8::is_ascii_whitespace) {
        // The OS lock is free, so the recorded holder is gone (it would still hold the OS lock
        // otherwise). Take it over only if it was on this host (REQ-005).
        let Ok(prev) = parse_lock_info(&previous) else {
            handle.release();
            return Err(StoreError::LockFileInvalid);
        };
        if prev.host != me.host {
            handle.release();
            return Err(StoreError::Locked(Some(prev)));
        }
        warnings.push(StoreWarning::StaleLockTaken(prev));
    }
    handle.replace(lock_text(&me).as_bytes())?;
    Ok(Locked { handle: Some(handle), dir: dir.try_clone()?, info: me, warnings })
}

/// This machine's host name as recorded in lock files.
pub fn host_name() -> String {
    let h: String = sys::host_name().chars().filter(|c| !c.is_control()).take(255).collect();
    if h.is_empty() {
        "unknown-host".to_string()
    } else {
        h
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The text written into `lock`.
pub fn lock_text(info: &LockInfo) -> String {
    format!("{LOCK_TAG}\npid={}\nhost={}\nstarted={}\n", info.pid, info.host, info.started)
}

/// Parses a lock file (bounded, fails closed on anything unexpected).
pub fn parse_lock_info(bytes: &[u8]) -> Result<LockInfo, StoreError> {
    let bad = || StoreError::LockFileInvalid;
    if bytes.len() as u64 > SMALL_FILE_LIMIT {
        return Err(bad());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| bad())?;
    let mut lines = text.lines().map(|l| l.trim_end_matches('\r'));
    if lines.next() != Some(LOCK_TAG) {
        return Err(bad());
    }
    let (mut pid, mut host, mut started) = (None, None, None);
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (k, v) = line.split_once('=').ok_or_else(bad)?;
        let slot_was_empty = match k {
            "pid" => pid.replace(parse_decimal(v).and_then(|n| u32::try_from(n).ok()).ok_or_else(bad)?).is_none(),
            "host" => {
                if v.is_empty() || v.len() > 255 || v.chars().any(char::is_control) {
                    return Err(bad());
                }
                host.replace(v.to_string()).is_none()
            }
            "started" => started.replace(parse_decimal(v).ok_or_else(bad)?).is_none(),
            _ => return Err(bad()),
        };
        if !slot_was_empty {
            return Err(bad());
        }
    }
    match (pid, host, started) {
        (Some(pid), Some(host), Some(started)) => Ok(LockInfo { pid, host, started }),
        _ => Err(bad()),
    }
}

fn parse_decimal(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 20 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// The text written into `format`.
pub fn format_text(version: u32, uuid: &str) -> String {
    format!("{FORMAT_TAG}{version}\nuuid={uuid}\n")
}

/// Parses `format`: returns the layout version and the project UUID. A version newer than
/// [`LAYOUT_VERSION`] is `layout_too_new` (its remaining lines are not interpreted).
pub fn parse_format(bytes: &[u8]) -> Result<(u32, String), StoreError> {
    let bad = |why: &str| StoreError::NotAProject(format!("{FORMAT_FILE}: {why}"));
    if bytes.len() as u64 > SMALL_FILE_LIMIT {
        return Err(bad("too large"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| bad("not UTF-8"))?;
    let mut lines = text.lines().map(|l| l.trim_end_matches('\r'));
    let first = lines.next().unwrap_or("");
    let digits = first.strip_prefix(FORMAT_TAG).ok_or_else(|| bad("missing layout tag"))?;
    let version = parse_decimal(digits)
        .filter(|_| digits.len() <= 9)
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n >= 1)
        .ok_or_else(|| bad("bad layout version"))?;
    if version > LAYOUT_VERSION {
        return Err(StoreError::LayoutTooNew { found: version, supported: LAYOUT_VERSION });
    }
    let uuid = lines.next().and_then(|l| l.strip_prefix("uuid=")).ok_or_else(|| bad("missing uuid"))?;
    if !is_uuid(uuid) {
        return Err(bad("bad uuid"));
    }
    if lines.any(|l| !l.is_empty()) {
        return Err(bad("unexpected content"));
    }
    Ok((version, uuid.to_string()))
}

/// True for a lowercase hyphenated UUID (`8-4-4-4-12` hex digits). The UUID becomes a cache
/// path component, so nothing else is accepted.
pub fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        })
}

/// A random (version 4 layout) UUID from the standard library's randomly keyed hasher.
fn new_uuid() -> String {
    use std::hash::BuildHasher;
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let pid = std::process::id();
    let a = std::collections::hash_map::RandomState::new().hash_one((nanos, pid, 1u8));
    let b = std::collections::hash_map::RandomState::new().hash_one((nanos, pid, 2u8, a));
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&a.to_le_bytes());
    bytes[8..].copy_from_slice(&b.to_le_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

/// Per-user cache directory for `os` (`std::env::consts::OS` values), from environment
/// variables: Windows `%LOCALAPPDATA%`; macOS `$HOME/Library/Caches`; other Unix
/// `$XDG_CACHE_HOME` (if absolute) or `$HOME/.cache`.
pub fn user_cache_dir(os: &str, env: impl Fn(&str) -> Option<std::ffi::OsString>) -> Option<PathBuf> {
    // Judged by `os`, not by the running system, so the rules are testable everywhere.
    let abs = |k: &str| env(k).map(PathBuf::from).filter(|p| usable_cache_base(os, p));
    match os {
        "windows" => abs("LOCALAPPDATA"),
        "macos" | "ios" => abs("HOME").map(|h| h.join("Library").join("Caches")),
        _ => abs("XDG_CACHE_HOME").or_else(|| abs("HOME").map(|h| h.join(".cache"))),
    }
}

fn usable_cache_base(os: &str, p: &Path) -> bool {
    if os == "windows" {
        !p.as_os_str().is_empty()
    } else {
        p.to_string_lossy().starts_with('/')
    }
}

/// Makes `path` absolute (no symlink resolution) and enforces [`MAX_PATH_CHARS`].
fn absolute_checked(path: &Path) -> Result<PathBuf, StoreError> {
    let abs = std::path::absolute(path)?;
    let chars = path_chars(&abs.as_os_str().to_string_lossy());
    if chars > MAX_PATH_CHARS {
        return Err(StoreError::PathTooLong { chars });
    }
    Ok(abs)
}

/// Length in characters of an absolute path, not counting a Windows `\\?\` prefix.
pub fn path_chars(s: &str) -> usize {
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        2 + rest.chars().count()
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.chars().count()
    } else {
        s.chars().count()
    }
}

/// Windows verbatim form of an absolute path: `C:\x` -> `\\?\C:\x`, `\\srv\share\x` ->
/// `\\?\UNC\srv\share\x`; verbatim and device paths are returned unchanged.
pub fn windows_verbatim(abs: &str) -> String {
    if abs.starts_with(r"\\?\") || abs.starts_with(r"\\.\") {
        abs.to_string()
    } else if let Some(rest) = abs.strip_prefix(r"\\") {
        format!(r"\\?\UNC\{rest}")
    } else {
        format!(r"\\?\{abs}")
    }
}

/// True if a canonical Windows path lies on a network share (`\\?\UNC\...`; mapped drive
/// letters canonicalise to this form [unverified for every redirector]).
pub fn is_network_windows_path(canonical: &str) -> bool {
    canonical.starts_with(r"\\?\UNC\") || (canonical.starts_with(r"\\") && !canonical.starts_with(r"\\?\"))
}

/// Linux `statfs` `f_type` magic numbers of network file systems, in order: NFS, SMB, CIFS,
/// SMB2, OpenAFS, kAFS, Coda, NCP, Ceph, 9P.
pub const NETWORK_FS_MAGIC: [u64; 10] =
    [0x6969, 0x517B, 0xFF53_4D42, 0xFE53_4D42, 0x5346_414F, 0x6B41_4653, 0x7375_7245, 0x564C, 0x00C3_6400, 0x0102_1997];

/// True if a Linux `f_type` (low 32 bits) is a network file system [unverified completeness].
pub fn is_network_fs_magic(f_type: u64) -> bool {
    NETWORK_FS_MAGIC.contains(&(f_type & 0xFFFF_FFFF))
}

/// macOS `f_fstypename` values of network file systems.
pub const NETWORK_FS_NAMES: [&str; 6] = ["nfs", "smbfs", "afpfs", "webdav", "cifs", "ftp"];

/// True if a macOS `f_fstypename` is a network file system [unverified completeness].
pub fn is_network_fs_name(name: &str) -> bool {
    NETWORK_FS_NAMES.contains(&name)
}

/// Parses `storage-writers.toml` (REQ-010): the crates allowed to write project files, from
/// `[storage_writers]` `allowed = ["..."]` (one-line array). Bounded, fails closed.
pub fn parse_storage_writers(text: &str) -> Result<Vec<String>, String> {
    if text.len() > 64 * 1024 {
        return Err("storage-writers.toml: too large".to_string());
    }
    let mut section = None::<String>;
    let mut allowed: Option<Vec<String>> = None;
    for (i, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(s) = line.strip_prefix('[') {
            section =
                Some(s.strip_suffix(']').ok_or_else(|| format!("line {}: bad section", i + 1))?.trim().to_string());
            continue;
        }
        let (k, v) = line.split_once('=').ok_or_else(|| format!("line {}: expected key = value", i + 1))?;
        if section.as_deref() != Some("storage_writers") || k.trim() != "allowed" {
            return Err(format!("line {}: unknown key {}", i + 1, k.trim()));
        }
        if allowed.is_some() {
            return Err(format!("line {}: allowed given twice", i + 1));
        }
        let inner = v
            .trim()
            .strip_prefix('[')
            .and_then(|x| x.strip_suffix(']'))
            .ok_or_else(|| format!("line {}: allowed must be a one-line array", i + 1))?;
        let mut names = Vec::new();
        for item in inner.split(',').map(str::trim).filter(|x| !x.is_empty()) {
            let name = item
                .strip_prefix('"')
                .and_then(|x| x.strip_suffix('"'))
                .filter(|x| !x.is_empty() && x.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
                .ok_or_else(|| format!("line {}: bad crate name {item}", i + 1))?;
            names.push(name.to_string());
        }
        allowed = Some(names);
    }
    allowed.ok_or_else(|| "missing [storage_writers] allowed".to_string())
}
