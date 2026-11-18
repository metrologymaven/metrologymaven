//! `fid-outreach site import fetch`: adds one release's manifest and reports to the site's
//! vendored inputs (spec node `OUTR.SITE.GEN.IMPORT`, REQ-006 and REQ-007).
//!
//! The host of `--base-url` must be listed in `<site>/hosts.toml` (`[[host]] name, purpose`;
//! compared case-insensitively and exactly, no wildcard), checked before any connection.
//! `https://` is used for every host; plain `http://` only for loopback hosts (`127.0.0.1`,
//! `::1`, `localhost`), which the tests use. Downloads go through the `curl` program, as in
//! `fid-io-corp`, so the crate links no TLS stack; redirects are not followed.
//!
//! Each download streams through a hashing writer that kills `curl` past the size cap, so the
//! cap holds without a `Content-Length`; loopback fetches use `--noproxy '*'`.
//!
//! All files are downloaded into `<site>/.inputs-fetch.tmp/` and checked against the release
//! manifest first; only when every check passes are they moved into `inputs/<tag>/` and the
//! lock file replaced (sorted by path, through a temporary file). A new `inputs/<tag>/` arrives
//! in one rename of the scratch folder; an existing one is updated file by file with backups,
//! and a failure at any step (including the lock rename) rolls every move back. A symlink at
//! `inputs/<tag>` or at a file being replaced is refused. On any failure nothing is left under
//! `inputs/` and the lock file is unchanged.

use super::{
    is_tag, parse_err, parse_lock, parse_release_manifest, read_bounded, render_lock, tag_matches, Finding,
    ImportError, LockEntry, HOSTS_FILE, LOCK_FILE, MAX_HOSTS_BYTES, MAX_LOCK_BYTES, MAX_MANIFEST_BYTES,
    MAX_REPORT_BYTES, RELEASE_MANIFEST,
};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Report files fetched when the release manifest lists them.
pub const REPORTS: [&str; 4] = ["parity.json", "parity.md", "status.json", "status.md"];
/// Host purposes `hosts.toml` accepts.
pub const PURPOSES: [&str; 3] = ["release_host", "mirror", "site"];
/// Scratch folder inside the site folder (outside `inputs/`).
pub const SCRATCH: &str = ".inputs-fetch.tmp";

const LOOPBACK: [&str; 3] = ["127.0.0.1", "::1", "localhost"];

/// Arguments of [`fetch`].
#[derive(Debug, Clone, Copy)]
pub struct FetchArgs<'a> {
    pub site: &'a Path,
    pub release: &'a str,
    pub base_url: &'a str,
}

/// A parsed `--base-url`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseUrl {
    pub https: bool,
    /// Lowercase host, without brackets for IPv6.
    pub host: String,
    /// The URL with a trailing `/`, ready to have a file name appended.
    pub prefix: String,
}

/// Parses `http://` or `https://` URLs with a host, an optional port and a path; user info,
/// queries, fragments, whitespace and curl glob characters are refused.
pub fn parse_base_url(url: &str) -> Option<BaseUrl> {
    let (https, rest) =
        if let Some(r) = url.strip_prefix("https://") { (true, r) } else { (false, url.strip_prefix("http://")?) };
    if url.len() > 2048
        || url.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '{' | '}' | '?' | '#' | '@' | '\\'))
    {
        return None;
    }
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if path.contains(['[', ']']) {
        return None;
    }
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        // Brackets only around an IPv6 literal.
        let (h, after) = v6.split_once(']')?;
        if !h.bytes().all(|b| b.is_ascii_hexdigit() || b == b':') {
            return None;
        }
        (h, if after.is_empty() { None } else { Some(after.strip_prefix(':')?) })
    } else {
        if authority.contains(['[', ']']) {
            return None;
        }
        authority.split_once(':').map_or((authority, None), |(h, p)| (h, Some(p)))
    };
    if port.is_some_and(|p| !is_port(p))
        || host.is_empty()
        || !host.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':'))
    {
        return None;
    }
    let mut prefix = url.to_string();
    if !prefix.ends_with('/') {
        prefix.push('/');
    }
    Some(BaseUrl { https, host: host.to_ascii_lowercase(), prefix })
}

fn is_port(p: &str) -> bool {
    !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u16>().is_ok()
}

/// Reads `hosts.toml` into the set of lowercase host names.
pub fn read_hosts(path: &Path) -> Result<BTreeSet<String>, ImportError> {
    let shown = path.display().to_string();
    let text = String::from_utf8(read_bounded(path, MAX_HOSTS_BYTES)?).map_err(|_| parse_err(&shown, "not UTF-8"))?;
    let table = text.parse::<toml::Table>().map_err(|e| parse_err(&shown, e.message()))?;
    let mut hosts = BTreeSet::new();
    for (key, value) in &table {
        if key != "host" {
            return Err(parse_err(&shown, format!("unknown top-level key `{key}`")));
        }
        let toml::Value::Array(items) = value else { return Err(parse_err(&shown, "`host` is not [[host]]")) };
        for (i, item) in items.iter().enumerate() {
            let toml::Value::Table(t) = item else {
                return Err(parse_err(&shown, format!("host #{} is not a table", i + 1)));
            };
            if let Some(k) = t.keys().find(|k| !matches!(k.as_str(), "name" | "purpose")) {
                return Err(parse_err(&shown, format!("host #{}: unknown key `{k}`", i + 1)));
            }
            let name = t.get("name").and_then(toml::Value::as_str).unwrap_or("");
            let purpose = t.get("purpose").and_then(toml::Value::as_str).unwrap_or("");
            if name.is_empty() || name.contains(['*', '/', ' ']) || !PURPOSES.contains(&purpose) {
                return Err(parse_err(&shown, format!("host #{}: bad `name` or `purpose`", i + 1)));
            }
            hosts.insert(name.to_ascii_lowercase());
        }
    }
    Ok(hosts)
}

/// A file writer that hashes what passes through and fails past `cap` bytes.
struct CapSink {
    file: std::fs::File,
    hasher: Sha256,
    written: u64,
    cap: u64,
    over_cap: bool,
}

impl Write for CapSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = u64::try_from(buf.len()).unwrap_or(u64::MAX);
        if self.written.saturating_add(n) > self.cap {
            self.over_cap = true;
            return Err(io::Error::other("size cap exceeded"));
        }
        let n = self.file.write(buf)?;
        self.hasher.update(buf.get(..n).unwrap_or_default());
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// Marker before the HTTP status curl writes to stderr.
const STATUS_MARK: &str = "fid-http-status:";

/// Streams `url` into `dst` with `curl`, hashing on the way and killing curl once more than
/// `cap` bytes arrive (the cap does not rely on a `Content-Length`). Returns the digest of the
/// bytes written, or the HTTP status or a short reason for an `IMPORT-FETCH` finding.
fn download(url: &str, https: bool, cap: u64, dst: &Path) -> Result<Digest, String> {
    let proto = if https { "=https" } else { "=http" };
    let file = std::fs::File::create(dst).map_err(|_| "scratch-write".to_string())?;
    let mut cmd = Command::new("curl");
    // `-q` must come first: it makes curl ignore any ~/.curlrc.
    cmd.args(["-q", "--globoff", "--silent", "--show-error", "--max-redirs", "0"])
        .args(["--proto", proto, "--proto-redir", proto, "--connect-timeout", "30", "--max-time", "600"])
        .args(["--write-out", "%{stderr}\nfid-http-status:%{http_code}\n", "--output", "-"]);
    if !https {
        // Loopback only: never send it through a proxy from the environment.
        cmd.args(["--noproxy", "*"]);
    }
    let mut child = cmd
        .args(["--", url])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "curl-unavailable".to_string())?;
    let mut sink = CapSink { file, hasher: Sha256::new(), written: 0, cap, over_cap: false };
    let copied = match child.stdout.take() {
        Some(mut out) => io::copy(&mut out, &mut sink).map(|_| ()),
        None => Err(io::Error::other("no pipe")),
    };
    if copied.is_err() {
        let _ = child.kill();
    }
    let out = child.wait_with_output().map_err(|_| "curl-wait".to_string())?;
    if sink.over_cap {
        return Err("too-large".into());
    }
    copied.map_err(|_| "read-error".to_string())?;
    sink.flush().map_err(|_| "scratch-write".to_string())?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    let code = stderr.lines().rev().find_map(|l| l.strip_prefix(STATUS_MARK)).unwrap_or("").trim().to_string();
    if !out.status.success() {
        return Err(format!("curl-exit-{}", out.status.code().unwrap_or(-1)));
    }
    if code != "200" {
        return Err(if code.is_empty() { "no-status".into() } else { code });
    }
    Ok(Digest::from_bytes(sink.hasher.finalize().into()))
}

/// Outcome of [`fetch`] beyond findings: the files written, for the caller's summary.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Fetched {
    pub entries: Vec<LockEntry>,
}

/// Fetches one release into `<site>/inputs/<tag>/` and updates the lock file. Returns findings
/// (exit 1) with nothing written, or the new lock entries.
pub fn fetch(args: &FetchArgs<'_>) -> Result<Result<Fetched, Vec<Finding>>, ImportError> {
    if !is_tag(args.release) {
        return Err(parse_err("--release", "must be 1 to 64 of A-Z a-z 0-9 . _ -"));
    }
    let Some(base) = parse_base_url(args.base_url) else {
        return Err(parse_err("--base-url", "must be an http:// or https:// URL without user, query or fragment"));
    };
    // Host check first: nothing is read from or sent to a host that is not allowlisted.
    let hosts = read_hosts(&args.site.join(HOSTS_FILE))?;
    if !hosts.contains(&base.host) {
        return Ok(Err(vec![Finding::Host { host: base.host }]));
    }
    if !base.https && !LOOPBACK.contains(&base.host.as_str()) {
        return Ok(Err(vec![Finding::Fetch { url: base.prefix, status: "http-not-loopback".into() }]));
    }
    // Current lock (absent means empty).
    let lock_path = args.site.join(LOCK_FILE);
    let (old, kind_findings) = if lock_path.exists() {
        let shown = lock_path.display().to_string();
        let text =
            String::from_utf8(read_bounded(&lock_path, MAX_LOCK_BYTES)?).map_err(|_| parse_err(&shown, "not UTF-8"))?;
        parse_lock(&text, &shown)?
    } else {
        (Vec::new(), Vec::new())
    };
    if !kind_findings.is_empty() {
        return Ok(Err(kind_findings));
    }

    let scratch = args.site.join(SCRATCH);
    let staged = match download_all(args.release, &base, &scratch) {
        Ok(Ok(staged)) => staged,
        Ok(Err(findings)) => {
            let _ = std::fs::remove_dir_all(&scratch);
            return Ok(Err(findings));
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&scratch);
            return Err(e);
        }
    };
    let committed = commit(args.site, args.release, &scratch, &old, staged);
    let _ = std::fs::remove_dir_all(&scratch);
    committed.map(|entries| Ok(Fetched { entries }))
}

/// Downloaded files: (file name, bytes path in scratch, lock entry).
type Staged = Vec<(String, PathBuf, LockEntry)>;

fn download_all(tag: &str, base: &BaseUrl, scratch: &Path) -> Result<Result<Staged, Vec<Finding>>, ImportError> {
    if scratch.exists() {
        std::fs::remove_dir_all(scratch).map_err(|e| super::io_err(scratch, &e))?;
    }
    std::fs::create_dir_all(scratch).map_err(|e| super::io_err(scratch, &e))?;
    let url = format!("{}{RELEASE_MANIFEST}", base.prefix);
    let mpath = scratch.join(RELEASE_MANIFEST);
    let mdigest = match download(&url, base.https, MAX_MANIFEST_BYTES, &mpath) {
        Ok(d) => d,
        Err(status) => return Ok(Err(vec![Finding::Fetch { url, status }])),
    };
    let rm = parse_release_manifest(&read_bounded(&mpath, MAX_MANIFEST_BYTES)?, &url)?;
    if !tag_matches(tag, &rm.version) {
        return Ok(Err(vec![Finding::Release { name: RELEASE_MANIFEST.into(), tag: tag.into() }]));
    }
    let Some(date) = rm.date.clone() else {
        return Err(parse_err(&url, "release manifest: `date` missing or not a date"));
    };
    let entry = |name: &str, kind: &str, sha256: Digest| LockEntry {
        name: name.into(),
        kind: kind.into(),
        release: Some(tag.into()),
        source_rev: None,
        date: date.clone(),
        path: format!("inputs/{tag}/{name}"),
        sha256,
    };
    let mut staged = vec![(RELEASE_MANIFEST.to_string(), mpath, entry(RELEASE_MANIFEST, "release_manifest", mdigest))];
    let mut findings = Vec::new();
    for name in REPORTS {
        let Some(expected) = rm.artefacts.get(name) else { continue };
        let url = format!("{}{name}", base.prefix);
        let path = scratch.join(name);
        match download(&url, base.https, MAX_REPORT_BYTES, &path) {
            Ok(digest) => {
                let e = entry(name, "release_asset", digest);
                if e.sha256 == *expected {
                    staged.push((name.to_string(), path, e));
                } else {
                    findings.push(Finding::Digest { name: name.into(), expected: *expected, actual: e.sha256 });
                }
            }
            Err(status) => findings.push(Finding::Fetch { url, status }),
        }
    }
    if findings.is_empty() {
        Ok(Ok(staged))
    } else {
        findings.sort();
        Ok(Err(findings))
    }
}

/// Lock file written next to the real one before it is replaced.
const LOCK_TMP: &str = ".inputs.lock.toml.tmp";
/// Backups of replaced files while a release folder is updated in place.
const BACKUP: &str = ".inputs-fetch.bak";

/// Writes the new lock file to a temporary file, moves the files into `inputs/<tag>/`, then
/// replaces the lock file. A new release folder arrives in one rename of the scratch folder; an
/// existing one is updated file by file, and every step is rolled back if a later one fails.
fn commit(
    site: &Path,
    tag: &str,
    scratch: &Path,
    old: &[LockEntry],
    staged: Staged,
) -> Result<Vec<LockEntry>, ImportError> {
    let dir = site.join("inputs").join(tag);
    let existing = match std::fs::symlink_metadata(&dir) {
        Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
            return Err(ImportError::Io {
                path: dir.display().to_string(),
                reason: "exists and is not a plain folder (symlinks are refused)".into(),
            });
        }
        Ok(_) => true,
        Err(_) => false,
    };
    let new_paths: BTreeSet<&str> = staged.iter().map(|(_, _, e)| e.path.as_str()).collect();
    let mut merged: Vec<LockEntry> = old.iter().filter(|e| !new_paths.contains(e.path.as_str())).cloned().collect();
    merged.extend(staged.iter().map(|(_, _, e)| e.clone()));
    let lock_tmp = site.join(LOCK_TMP);
    std::fs::write(&lock_tmp, render_lock(&merged)).map_err(|e| super::io_err(&lock_tmp, &e))?;
    let lock = site.join(LOCK_FILE);
    let moved = if existing { replace_files(&dir, site, &staged) } else { move_folder(scratch, &dir) };
    let undo = match moved {
        Ok(undo) => undo,
        Err(e) => {
            let _ = std::fs::remove_file(&lock_tmp);
            return Err(e);
        }
    };
    if let Err(e) = std::fs::rename(&lock_tmp, &lock) {
        let _ = std::fs::remove_file(&lock_tmp);
        undo.roll_back();
        return Err(super::io_err(&lock, &e));
    }
    undo.finish();
    let mut entries: Vec<LockEntry> = staged.into_iter().map(|(_, _, e)| e).collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

/// How to undo a move into `inputs/<tag>/`.
enum Undo {
    /// The scratch folder became the release folder.
    Folder { from: PathBuf, to: PathBuf },
    /// Files moved in place: (destination, source, backup of the replaced file).
    Files { moved: Vec<(PathBuf, PathBuf, Option<PathBuf>)>, backup: PathBuf },
}

impl Undo {
    fn roll_back(self) {
        match self {
            Undo::Folder { from, to } => {
                let _ = std::fs::rename(&to, &from);
            }
            Undo::Files { moved, backup } => {
                for (dst, src, old) in moved.into_iter().rev() {
                    let _ = std::fs::rename(&dst, &src);
                    if let Some(old) = old {
                        let _ = std::fs::rename(&old, &dst);
                    }
                }
                let _ = std::fs::remove_dir_all(&backup);
            }
        }
    }

    fn finish(self) {
        if let Undo::Files { backup, .. } = self {
            let _ = std::fs::remove_dir_all(&backup);
        }
    }
}

fn move_folder(scratch: &Path, dir: &Path) -> Result<Undo, ImportError> {
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| super::io_err(parent, &e))?;
    }
    std::fs::rename(scratch, dir).map_err(|e| super::io_err(dir, &e))?;
    Ok(Undo::Folder { from: scratch.to_path_buf(), to: dir.to_path_buf() })
}

fn replace_files(dir: &Path, site: &Path, staged: &Staged) -> Result<Undo, ImportError> {
    let backup = site.join(BACKUP);
    if backup.exists() {
        std::fs::remove_dir_all(&backup).map_err(|e| super::io_err(&backup, &e))?;
    }
    std::fs::create_dir_all(&backup).map_err(|e| super::io_err(&backup, &e))?;
    let mut undo = Undo::Files { moved: Vec::new(), backup: backup.clone() };
    for (name, src, _) in staged {
        let dst = dir.join(name);
        let step = back_up(&dst, &backup.join(name));
        let old = match step {
            Ok(old) => old,
            Err(e) => {
                undo.roll_back();
                return Err(e);
            }
        };
        if let Err(e) = std::fs::rename(src, &dst) {
            if let Some(b) = &old {
                let _ = std::fs::rename(b, &dst);
            }
            undo.roll_back();
            return Err(super::io_err(&dst, &e));
        }
        if let Undo::Files { moved, .. } = &mut undo {
            moved.push((dst, src.clone(), old));
        }
    }
    Ok(undo)
}

/// Moves an existing plain file at `dst` to `backup`; refuses anything that is not a plain file.
fn back_up(dst: &Path, backup: &Path) -> Result<Option<PathBuf>, ImportError> {
    match std::fs::symlink_metadata(dst) {
        Ok(m) if !m.is_file() => {
            Err(ImportError::Io { path: dst.display().to_string(), reason: "exists and is not a plain file".into() })
        }
        Ok(_) => {
            std::fs::rename(dst, backup).map_err(|e| super::io_err(dst, &e))?;
            Ok(Some(backup.to_path_buf()))
        }
        Err(_) => Ok(None),
    }
}
