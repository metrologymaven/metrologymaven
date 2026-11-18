//! Building the per-run sandbox (REQ-AGENT.HARNESS.SANDBOX-001 to -004, -006).

use crate::path::{check_rel, confine, create_dirs, create_file, fold, is_same_or_ancestor};
use crate::snapshot::FsSnapshot;
use crate::tar::{self, Kind};
use crate::task::TaskSpec;
use crate::{read_bounded, ErrorCode, SandboxError};
use fid_types::Digest;
use fid_uuid::Uuid;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The `format` value of `roots.json`.
pub const ROOTS_FORMAT: &str = "fid-sandbox-roots@1";

/// One granted root: its name in the task and its folder, relative to the run directory.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RootDecl {
    pub root: String,
    pub dir: String,
}

/// A built sandbox. The run directory stays on disk (it is the run's record) until
/// [`Sandbox::remove`] is called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sandbox {
    run_id: Uuid,
    dir: PathBuf,
    project: PathBuf,
    home: PathBuf,
    artifacts: PathBuf,
    start_digest: Digest,
    read: Vec<RootDecl>,
    write: Vec<RootDecl>,
    env: Vec<(String, String)>,
}

fn err(code: ErrorCode, detail: impl Into<String>) -> SandboxError {
    SandboxError::new(code, detail)
}

fn io_err(p: &Path, e: std::io::Error) -> SandboxError {
    err(ErrorCode::Io, format!("{}: {e}", p.display()))
}

/// Creates one directory readable only by its owner (mode 0o700) on Unix; elsewhere a plain
/// directory (on Windows it inherits the work folder's access control list).
fn create_private_dir(p: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(p)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir(p)
    }
}

/// A fresh UUIDv7 (RFC 9562 §5.7): 48-bit Unix milliseconds and 74 random bits from the
/// operating system's CSPRNG.
pub fn new_run_id() -> Result<Uuid, SandboxError> {
    let unavailable = |d: &str| err(ErrorCode::RunIdUnavailable, d);
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| unavailable("clock before 1970"))?
        .as_millis();
    let ms = u64::try_from(ms).ok().filter(|m| *m < (1u64 << 48)).ok_or_else(|| unavailable("clock out of range"))?;
    let mut random = [0u8; 10];
    getrandom::getrandom(&mut random).map_err(|_| unavailable("no operating-system randomness"))?;
    let mut b = [0u8; 16];
    b[..6].copy_from_slice(&ms.to_be_bytes()[2..]);
    b[6..].copy_from_slice(&random);
    b[6] = 0x70 | (b[6] & 0x0f);
    b[8] = 0x80 | (b[8] & 0x3f);
    Ok(Uuid::from_bytes(b))
}

/// Checks one list of roots: each a portable relative path, no two equal or nested
/// (case-insensitively, so they cannot alias on any file system).
fn check_roots(roots: &[String], kind: &str) -> Result<(), SandboxError> {
    for (i, r) in roots.iter().enumerate() {
        check_rel(r).map_err(|why| err(ErrorCode::InvalidRoots, format!("{kind} root {r:?}: {why}")))?;
        for other in &roots[..i] {
            let (a, b) = (fold(other), fold(r));
            if is_same_or_ancestor(&a, &b) || is_same_or_ancestor(&b, &a) {
                return Err(err(
                    ErrorCode::InvalidRoots,
                    format!("{kind} roots {other:?} and {r:?} are equal or nested (ignoring case)"),
                ));
            }
        }
    }
    Ok(())
}

/// Validates the archive entries as a whole: safe paths, and no two entries that collide
/// or nest below a file on a case-insensitive file system. Returns folded path -> kind.
fn check_entries(entries: &[tar::Entry<'_>]) -> Result<BTreeMap<String, Kind>, SandboxError> {
    let bad = |d: String| err(ErrorCode::BadArchive, d);
    let mut seen: BTreeMap<String, Kind> = BTreeMap::new();
    for e in entries {
        check_rel(&e.path).map_err(|why| bad(format!("entry {:?}: {why}", e.path)))?;
        let key = fold(&e.path);
        let segs: Vec<&str> = key.split('/').collect();
        for n in 1..segs.len() {
            let anc = segs[..n].join("/");
            match seen.get(&anc) {
                Some(Kind::File) => return Err(bad(format!("entry {:?} lies below a file", e.path))),
                Some(Kind::Dir) => {}
                None => {
                    seen.insert(anc, Kind::Dir);
                }
            }
        }
        match (seen.get(&key), e.kind) {
            (None, k) => {
                seen.insert(key, k);
            }
            (Some(Kind::Dir), Kind::Dir) => {}
            _ => return Err(bad(format!("entry {:?} collides with another entry (ignoring case)", e.path))),
        }
    }
    Ok(seen)
}

/// The child-process environment: private home and configuration folders (user and, through
/// `XDG_CONFIG_DIRS` / `XDG_DATA_DIRS`, the XDG system search paths), plus only the variables
/// needed to start programs (`PATH`; on Windows also the system folder variables).
///
/// This isolates a program only if it finds its configuration through these variables. `fid`
/// must therefore resolve its folders from the environment: on Windows the known-folder API
/// (`SHGetKnownFolderPath`) ignores `APPDATA`, and machine-wide configuration (for example
/// under `ProgramData` or `/etc` when `XDG_CONFIG_DIRS` is ignored) is not hidden by an
/// environment at all; see the leaf's open questions.
fn build_env(home: &Path) -> Result<Vec<(String, String)>, SandboxError> {
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    let dirs: [(&str, &[&str]); 13] = [
        ("HOME", &[]),
        ("USERPROFILE", &[]),
        ("XDG_CONFIG_HOME", &[".config"]),
        ("XDG_DATA_HOME", &[".local", "share"]),
        ("XDG_STATE_HOME", &[".local", "state"]),
        ("XDG_CACHE_HOME", &[".cache"]),
        ("XDG_CONFIG_DIRS", &["etc", "xdg"]),
        ("XDG_DATA_DIRS", &["usr", "share"]),
        ("APPDATA", &["AppData", "Roaming"]),
        ("LOCALAPPDATA", &["AppData", "Local"]),
        ("TMPDIR", &["tmp"]),
        ("TMP", &["tmp"]),
        ("TEMP", &["tmp"]),
    ];
    for (key, segs) in dirs {
        let rel = segs.join("/");
        let p = if rel.is_empty() { home.to_path_buf() } else { create_dirs(home, &rel)? };
        let s = p.to_str().ok_or_else(|| err(ErrorCode::InvalidWorkDir, "work folder path is not UTF-8"))?;
        env.insert(key.to_string(), s.to_string());
    }
    let mut pass = vec!["PATH"];
    if cfg!(windows) {
        pass.extend(["SystemRoot", "SystemDrive", "windir", "ComSpec", "PATHEXT"]);
    }
    for key in pass {
        if let Some(v) = std::env::var_os(key).and_then(|v| v.into_string().ok()) {
            env.insert(key.to_string(), v);
        }
    }
    Ok(env.into_iter().collect())
}

/// `roots.json`: canonical JSON (RFC 8785) of the root configuration. Root names and folders
/// pass [`check_rel`], so they need no escaping.
fn roots_json(read: &[RootDecl], write: &[RootDecl]) -> String {
    let list = |v: &[RootDecl]| {
        v.iter().map(|d| format!("{{\"dir\":\"{}\",\"root\":\"{}\"}}", d.dir, d.root)).collect::<Vec<_>>().join(",")
    };
    format!("{{\"format\":\"{ROOTS_FORMAT}\",\"read\":[{}],\"write\":[{}]}}", list(read), list(write))
}

impl Sandbox {
    /// Builds a sandbox for `task` under `<work>/run/` with a fresh UUIDv7 run ID.
    pub fn build(work: &Path, task: &TaskSpec) -> Result<Sandbox, SandboxError> {
        Sandbox::build_with_run_id(work, task, new_run_id()?)
    }

    /// [`Sandbox::build`] with a caller-chosen run ID (for example a replayed run). Fails with
    /// `run_dir_exists` if `<work>/run/<run_id>` exists, and leaves it untouched.
    ///
    /// Everything that can be checked without writing (roots, archive digest, archive
    /// contents) is checked first, so a failing task creates no run directory. A failure
    /// after the run directory is created removes it again.
    pub fn build_with_run_id(work: &Path, task: &TaskSpec, run_id: Uuid) -> Result<Sandbox, SandboxError> {
        check_roots(&task.read_roots, "read")?;
        check_roots(&task.write_roots, "write")?;
        let archive = read_bounded(&task.start_archive, ErrorCode::BadArchive)?;
        let digest = Digest::from_bytes(fid_canon::digest_bytes(&archive).0);
        if digest != task.start_digest {
            return Err(err(
                ErrorCode::StartStateMismatch,
                format!("start state is {digest}, task names {}", task.start_digest),
            ));
        }
        let entries = tar::parse(&archive).map_err(|d| err(ErrorCode::BadArchive, d))?;
        let kinds = check_entries(&entries)?;
        for r in &task.read_roots {
            let key = fold(r);
            if kinds.iter().any(|(p, k)| *k == Kind::File && is_same_or_ancestor(p, &key)) {
                return Err(err(ErrorCode::InvalidRoots, format!("read root {r:?} is a file in the start state")));
            }
        }

        if !work.is_absolute() {
            return Err(err(ErrorCode::InvalidWorkDir, format!("{}: not an absolute path", work.display())));
        }
        let wm =
            std::fs::metadata(work).map_err(|e| err(ErrorCode::InvalidWorkDir, format!("{}: {e}", work.display())))?;
        if !wm.is_dir() {
            return Err(err(ErrorCode::InvalidWorkDir, format!("{}: not a directory", work.display())));
        }
        let runs = work.join("run");
        match std::fs::create_dir(&runs) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => return Err(io_err(&runs, e)),
        }
        let rm = std::fs::symlink_metadata(&runs).map_err(|e| io_err(&runs, e))?;
        if rm.file_type().is_symlink() || !rm.is_dir() {
            return Err(err(ErrorCode::InvalidWorkDir, format!("{}: not a real directory", runs.display())));
        }
        let dir = runs.join(run_id.to_string());
        match create_private_dir(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                return Err(err(ErrorCode::RunDirExists, format!("{} exists", dir.display())));
            }
            Err(e) => return Err(io_err(&dir, e)),
        }
        match Sandbox::populate(&dir, task, &entries, run_id, digest) {
            Ok(s) => Ok(s),
            Err(e) => {
                // Best effort: the error being reported matters more than a failed cleanup.
                let _ = std::fs::remove_dir_all(dir.as_path());
                Err(e)
            }
        }
    }

    fn populate(
        dir: &Path,
        task: &TaskSpec,
        entries: &[tar::Entry<'_>],
        run_id: Uuid,
        start_digest: Digest,
    ) -> Result<Sandbox, SandboxError> {
        let project = create_dirs(dir, "project")?;
        for e in entries {
            match e.kind {
                Kind::Dir => {
                    create_dirs(&project, &e.path)?;
                }
                Kind::File => create_file(&project, &e.path, e.data)?,
            }
        }
        create_dirs(dir, "roots/read")?;
        create_dirs(dir, "roots/write")?;
        let mut read = Vec::new();
        for r in &task.read_roots {
            let rel = format!("roots/read/{r}");
            let root_dir = create_dirs(dir, &rel)?;
            let prefix = format!("{}/", fold(r));
            for e in entries.iter().filter(|e| e.kind == Kind::File) {
                if fold(&e.path).starts_with(&prefix) {
                    let rest = e.path.get(prefix.len()..).unwrap_or_default();
                    create_file(&root_dir, rest, e.data)?;
                }
            }
            read.push(RootDecl { root: r.clone(), dir: rel });
        }
        let mut write = Vec::new();
        for w in &task.write_roots {
            let rel = format!("roots/write/{w}");
            create_dirs(dir, &rel)?;
            write.push(RootDecl { root: w.clone(), dir: rel });
        }
        create_file(dir, "roots.json", roots_json(&read, &write).as_bytes())?;
        let home = dir.join("home");
        create_private_dir(&home).map_err(|e| io_err(&home, e))?;
        let artifacts = create_dirs(dir, "artifacts")?;
        let env = build_env(&home)?;
        Ok(Sandbox { run_id, dir: dir.to_path_buf(), project, home, artifacts, start_digest, read, write, env })
    }

    pub fn run_id(&self) -> Uuid {
        self.run_id
    }

    /// The run directory, `<work>/run/<run-id>`.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The sandbox project (the unpacked start state).
    pub fn project(&self) -> &Path {
        &self.project
    }

    /// The private home folder.
    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn artifacts(&self) -> &Path {
        &self.artifacts
    }

    /// The verified start-state digest (equal to the task's).
    pub fn start_digest(&self) -> Digest {
        self.start_digest
    }

    /// Read roots, in task order.
    pub fn read_roots(&self) -> &[RootDecl] {
        &self.read
    }

    /// Write roots, in task order.
    pub fn write_roots(&self) -> &[RootDecl] {
        &self.write
    }

    /// The folder of the read root named `root` exactly as in the task.
    pub fn read_root(&self, root: &str) -> Option<PathBuf> {
        self.read.iter().find(|d| d.root == root).map(|d| self.native(&d.dir))
    }

    /// The folder of the write root named `root` exactly as in the task.
    pub fn write_root(&self, root: &str) -> Option<PathBuf> {
        self.write.iter().find(|d| d.root == root).map(|d| self.native(&d.dir))
    }

    /// A run-relative `/`-separated path as a native path (one component per segment).
    fn native(&self, rel: &str) -> PathBuf {
        rel.split('/').fold(self.dir.clone(), |p, seg| p.join(seg))
    }

    /// The complete child-process environment, sorted by name.
    pub fn env(&self) -> &[(String, String)] {
        &self.env
    }

    /// A command that runs `program` in the project folder with exactly [`Sandbox::env`]
    /// (the parent's environment is cleared).
    pub fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let mut c = Command::new(program);
        c.env_clear().envs(self.env.iter().map(|(k, v)| (k.as_str(), v.as_str()))).current_dir(&self.project);
        c
    }

    /// Resolves a run-directory-relative path without leaving the run directory: `..`,
    /// absolute paths, backslashes and symbolic links or junctions on the way are refused
    /// with `path_escape`.
    ///
    /// This is check-then-use: the components are checked when it is called, and a process
    /// that can write in the run directory can swap a component for a link before the path
    /// is used. It is therefore not safe while an agent (or a leftover agent process) is
    /// running; root enforcement at run time belongs to AUTO.SURF.ROOTS.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, SandboxError> {
        confine(&self.dir, rel)
    }

    /// Path and SHA-256 of every file in the run directory (REQ-005).
    pub fn snapshot(&self) -> Result<FsSnapshot, SandboxError> {
        FsSnapshot::take(&self.dir)
    }

    /// Deletes the run directory. Symbolic links and junctions inside are removed, never
    /// followed.
    pub fn remove(self) -> Result<(), SandboxError> {
        std::fs::remove_dir_all(&self.dir).map_err(|e| io_err(&self.dir, e))
    }
}
