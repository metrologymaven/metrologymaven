//! Dataset fetch, cache and verify (spec node `ECO.DATA.FETCH`).
//!
//! A catalogue entry is installed at `<root>/<id>/<sha256 hex>/<filename>`. Bytes are
//! streamed into a uniquely named file under `<root>/.tmp/` while being hashed and
//! counted; reading stops one byte past the catalogue `size_bytes`; only a file whose
//! size and SHA-256 both match is fsynced and renamed to its final path. A cached file
//! that still verifies is returned without any network access, and offline mode never
//! opens a URL.
//!
//! URLs come only from the catalogue. `http://` and `https://` are fetched by the
//! `curl` program (as `fid-io-corp` does), so this crate links no HTTP or TLS stack; curl
//! is told to refuse every redirect, so no host outside the catalogue is ever contacted.
//! `file://` URLs (tests, local mirrors) are read directly.

use crate::cat::{self, CatalogueEntry};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const FETCH_DIGEST: &str = "FETCH-DIGEST";
pub const FETCH_SIZE: &str = "FETCH-SIZE";
pub const FETCH_OFFLINE: &str = "FETCH-OFFLINE";
pub const FETCH_ALL_URLS_FAILED: &str = "FETCH-ALL-URLS-FAILED";
pub const FETCH_UNKNOWN_ID: &str = "FETCH-UNKNOWN-ID";
pub const FETCH_IO: &str = "FETCH-IO";

/// `FID_DATA_OFFLINE=1` forbids any network access.
pub const ENV_OFFLINE: &str = "FID_DATA_OFFLINE";
/// Overrides the default per-user cache root.
pub const ENV_CACHE: &str = "FID_DATA_CACHE";
/// Overrides the default catalogue (the repository's `datasets.toml`).
pub const ENV_CATALOGUE: &str = "FID_DATA_CATALOGUE";
/// Folder under the cache root that holds downloads in progress.
pub const TMP_DIR: &str = ".tmp";

/// Subject of errors that concern no single dataset.
pub const NO_DATASET: &str = "-";

/// A failed fetch, path lookup or verification. Displays as `<CODE> <id>: <detail>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataError {
    pub code: &'static str,
    /// The dataset ID, or `-`.
    pub id: String,
    pub detail: String,
}

impl DataError {
    fn new(code: &'static str, id: &str, detail: impl Into<String>) -> Self {
        DataError { code, id: id.to_string(), detail: detail.into() }
    }
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.code, self.id, self.detail)
    }
}

impl std::error::Error for DataError {}

// ------------------------------------------------------------------ transports

/// Why opening or reading one URL failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The response is larger than the catalogue allows.
    TooLarge(String),
    /// Any other transfer failure (refused URL, HTTP error, redirect, I/O).
    Other(String),
}

/// The bytes of one response.
pub trait Body: Read {
    /// Called once the body has been read to its end; reports a transfer that failed.
    fn finish(&mut self) -> Result<(), Failure>;
}

/// Opens catalogue URLs. Tests can supply their own; [`SystemTransport`] is the default.
pub trait Transport {
    /// Opens `url`, whose catalogue size is `max_bytes`. The caller reads at most
    /// `max_bytes + 1` bytes from the body.
    fn open(&self, url: &str, max_bytes: u64) -> Result<Box<dyn Body>, Failure>;
}

/// `file://` URLs are read from disk; `http://` and `https://` URLs are downloaded by the
/// `curl` program without following redirects. Nothing else is accepted.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemTransport;

impl Transport for SystemTransport {
    fn open(&self, url: &str, max_bytes: u64) -> Result<Box<dyn Body>, Failure> {
        if url.starts_with("file://") {
            let path = file_url_path(url).ok_or_else(|| Failure::Other(format!("refused file URL {url:?}")))?;
            let file = fs::File::open(&path).map_err(|e| Failure::Other(format!("{}: {e}", path.display())))?;
            return Ok(Box::new(FileBody(file)));
        }
        if !is_http_url(url) {
            return Err(Failure::Other(format!("refused URL {url:?}: only http://, https:// and file:// are fetched")));
        }
        CurlBody::spawn(url, max_bytes).map(|b| Box::new(b) as Box<dyn Body>)
    }
}

struct FileBody(fs::File);

impl Read for FileBody {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Body for FileBody {
    fn finish(&mut self) -> Result<(), Failure> {
        Ok(())
    }
}

/// `https://` or `http://` with a non-empty rest, at most 2,048 bytes, and no whitespace,
/// control characters or curl glob characters.
fn is_http_url(url: &str) -> bool {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"));
    rest.is_some_and(|r| !r.is_empty())
        && url.len() <= 2048
        && !url.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '[' | ']' | '{' | '}'))
}

/// Local path of a `file://` URL: `file:///abs/path` (or `file://localhost/...`), with
/// `%XX` escapes decoded. On Windows the path must start with a drive (`file:///C:/...`);
/// UNC hosts are refused everywhere.
pub fn file_url_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') || rest.starts_with("//") {
        return None;
    }
    let decoded = percent_decode(rest)?;
    if cfg!(windows) {
        let b = decoded.as_bytes();
        let drive = b.len() >= 4 && b[1].is_ascii_alphabetic() && b[2] == b':' && b[3] == b'/';
        return drive.then(|| PathBuf::from(&decoded[1..]));
    }
    Some(PathBuf::from(decoded))
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    let text = String::from_utf8(out).ok()?;
    (!text.chars().any(char::is_control)).then_some(text)
}

/// Seconds allowed to connect, and for a whole transfer.
const CONNECT_TIMEOUT_S: &str = "30";
const MAX_TIME_S: &str = "600";
/// curl's exit code for "maximum file size exceeded".
const CURL_FILESIZE_EXCEEDED: i32 = 63;
/// Most bytes of curl's error output kept for a message.
const MAX_STDERR: u64 = 4096;

struct CurlBody {
    child: Child,
    out: Option<ChildStdout>,
    err: Option<ChildStderr>,
    done: bool,
}

impl CurlBody {
    fn spawn(url: &str, max_bytes: u64) -> Result<CurlBody, Failure> {
        let max = max_bytes.to_string();
        // `-q` must come first: it makes curl ignore any ~/.curlrc. `--location` with
        // `--max-redirs 0` turns every redirect into an error, so only catalogue hosts are read.
        let mut child = Command::new("curl")
            .args(["-q", "--globoff", "--fail", "--silent", "--show-error", "--location", "--max-redirs", "0"])
            .args(["--proto", "=http,https", "--proto-redir", "=http,https"])
            .args(["--connect-timeout", CONNECT_TIMEOUT_S, "--max-time", MAX_TIME_S, "--max-filesize", &max])
            .args(["--", url])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Failure::Other(format!("cannot run curl: {e}")))?;
        let out = child.stdout.take();
        let err = child.stderr.take();
        Ok(CurlBody { child, out, err, done: false })
    }
}

impl Read for CurlBody {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.out.as_mut() {
            Some(out) => out.read(buf),
            None => Ok(0),
        }
    }
}

impl Body for CurlBody {
    fn finish(&mut self) -> Result<(), Failure> {
        self.out = None;
        let mut msg = Vec::new();
        if let Some(err) = self.err.take() {
            let _ = err.take(MAX_STDERR).read_to_end(&mut msg);
        }
        let status = self.child.wait().map_err(|e| Failure::Other(format!("curl: {e}")))?;
        self.done = true;
        let msg = String::from_utf8_lossy(&msg).trim().to_string();
        match status.code() {
            Some(0) => Ok(()),
            Some(CURL_FILESIZE_EXCEEDED) => Err(Failure::TooLarge(format!("curl refused the transfer: {msg}"))),
            _ => Err(Failure::Other(format!("curl failed ({status}): {msg}"))),
        }
    }
}

impl Drop for CurlBody {
    fn drop(&mut self) {
        if !self.done {
            // Reading stopped early (size cap) or failed: end the transfer.
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

// ------------------------------------------------------------------ cache layout

/// The file name a dataset is installed under: the last path segment of its first URL
/// when that is a plain name (`[A-Za-z0-9._-]`, 1 to 128 characters, not starting with
/// `.`), otherwise the dataset ID.
pub fn file_name(entry: &CatalogueEntry) -> String {
    let first = entry.urls.first().map(String::as_str);
    first.and_then(last_segment).filter(|s| is_plain_name(s)).unwrap_or_else(|| entry.id.clone())
}

fn last_segment(url: &str) -> Option<String> {
    let no_query = url.split(['?', '#']).next()?;
    let after_scheme = no_query.split_once("://")?.1;
    let path = after_scheme.split_once('/')?.1;
    path.rsplit('/').next().map(str::to_string)
}

fn is_plain_name(s: &str) -> bool {
    (1..=128).contains(&s.len())
        && !s.starts_with('.')
        && s.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

/// Where `entry` lives in the cache: `<root>/<id>/<sha256 hex>/<filename>`.
pub fn installed_path(root: &Path, entry: &CatalogueEntry) -> PathBuf {
    root.join(&entry.id).join(entry.sha256.hex()).join(file_name(entry))
}

/// SHA-256 and length of a file, reading at most `limit` bytes.
fn hash_file(path: &Path, limit: u64) -> io::Result<(Digest, u64)> {
    let file = fs::File::open(path)?;
    let mut reader = file.take(limit);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut n: u64 = 0;
    loop {
        let k = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(k) => k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        hasher.update(&buf[..k]);
        n += k as u64;
    }
    Ok((Digest::from_bytes(hasher.finalize().into()), n))
}

/// Whether the file at `path` has exactly `size` bytes and the SHA-256 `expected`.
fn verifies(path: &Path, expected: &Digest, size: u64) -> bool {
    path.is_file() && hash_file(path, size.saturating_add(1)).is_ok_and(|(d, n)| d == *expected && n == size)
}

/// A file under `<root>/.tmp/` that is deleted when dropped unless it was installed.
struct TempFile {
    path: PathBuf,
    installed: bool,
}

impl TempFile {
    fn create(tmp_dir: &Path, id: &str) -> io::Result<(TempFile, fs::File)> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let path = tmp_dir.join(format!("{id}.{}.{nanos}.{n}.part", std::process::id()));
        // create_new: never write through an existing file or symlink at this name.
        let file = fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
        Ok((TempFile { path, installed: false }, file))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.installed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

// ------------------------------------------------------------------ fetch

/// Returns the verified cache path of `entry`, fetching it first if needed.
///
/// A cached copy that verifies is returned with no network access (REQ-002). With
/// `offline`, a missing or failing copy is `FETCH-OFFLINE` and no URL is opened
/// (REQ-003). Otherwise each URL is tried in catalogue order until one verifies
/// (REQ-004); progress lines go to `log`. Nothing reaches the final path except by a
/// rename after verification (REQ-001, REQ-005), and no more than `size_bytes + 1`
/// bytes are read from any response (REQ-006).
pub fn fetch_entry(
    entry: &CatalogueEntry,
    root: &Path,
    offline: bool,
    transport: &dyn Transport,
    log: &mut dyn Write,
) -> Result<PathBuf, DataError> {
    let id = entry.id.as_str();
    let target = installed_path(root, entry);
    if verifies(&target, &entry.sha256, entry.size_bytes) {
        return Ok(target);
    }
    if offline {
        let why = if target.exists() { "the cached copy fails verification" } else { "not in the cache" };
        let detail = format!("{why} ({}) and offline mode forbids a fetch", target.display());
        return Err(DataError::new(FETCH_OFFLINE, id, detail));
    }
    let tmp_dir = root.join(TMP_DIR);
    fs::create_dir_all(&tmp_dir)
        .map_err(|e| DataError::new(FETCH_IO, id, format!("cannot create {}: {e}", tmp_dir.display())))?;
    let mut failures: Vec<(&'static str, String)> = Vec::new();
    for url in &entry.urls {
        let _ = writeln!(log, "data fetch {id}: trying {url}");
        let outcome = download(entry, url, &tmp_dir, transport).and_then(|tmp| install(tmp, &target));
        match outcome {
            Ok(()) => {
                let _ = writeln!(log, "data fetch {id}: installed {}", target.display());
                return Ok(target);
            }
            Err((code, msg)) => {
                let _ = writeln!(log, "data fetch {id}: {url}: {code} {msg}");
                failures.push((code, format!("{url}: {msg}")));
            }
        }
    }
    let first = failures.first().map_or(FETCH_ALL_URLS_FAILED, |f| f.0);
    let code = if failures.iter().all(|f| f.0 == first) { first } else { FETCH_ALL_URLS_FAILED };
    let detail: Vec<String> = failures.into_iter().map(|f| f.1).collect();
    Err(DataError::new(code, id, detail.join("; ")))
}

type Attempt<T> = Result<T, (&'static str, String)>;

fn failure(f: Failure) -> (&'static str, String) {
    match f {
        Failure::TooLarge(m) => (FETCH_SIZE, m),
        Failure::Other(m) => (FETCH_ALL_URLS_FAILED, m),
    }
}

/// Streams one URL into a temporary file and verifies it; returns the fsynced file.
fn download(entry: &CatalogueEntry, url: &str, tmp_dir: &Path, transport: &dyn Transport) -> Attempt<TempFile> {
    let io_err = |e: io::Error| (FETCH_IO, format!("cannot write {}: {e}", tmp_dir.display()));
    let (tmp, file) = TempFile::create(tmp_dir, &entry.id).map_err(io_err)?;
    let mut body = transport.open(url, entry.size_bytes).map_err(failure)?;
    let size = entry.size_bytes;
    let mut writer = io::BufWriter::new(file);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut n: u64 = 0;
    {
        let mut limited = Read::by_ref(&mut body).take(size.saturating_add(1));
        loop {
            let k = match limited.read(&mut buf) {
                Ok(0) => break,
                Ok(k) => k,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err((FETCH_ALL_URLS_FAILED, format!("read failed: {e}"))),
            };
            n += k as u64;
            if n > size {
                return Err((FETCH_SIZE, format!("response is larger than size_bytes {size}; stopped reading")));
            }
            hasher.update(&buf[..k]);
            writer.write_all(&buf[..k]).map_err(io_err)?;
        }
    }
    body.finish().map_err(failure)?;
    drop(body);
    let file = writer.into_inner().map_err(|e| io_err(e.into_error()))?;
    let actual = Digest::from_bytes(hasher.finalize().into());
    if n != size || actual != entry.sha256 {
        return Err((
            FETCH_DIGEST,
            format!("got {actual} ({n} bytes), catalogue says {} ({size} bytes)", entry.sha256),
        ));
    }
    file.sync_all().map_err(io_err)?;
    Ok(tmp)
}

/// Atomically renames a verified temporary file to its final path.
fn install(mut tmp: TempFile, target: &Path) -> Attempt<()> {
    let io_err = |e: io::Error| (FETCH_IO, format!("cannot install {}: {e}", target.display()));
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir).map_err(io_err)?;
    }
    fs::rename(&tmp.path, target).map_err(io_err)?;
    tmp.installed = true;
    Ok(())
}

// ------------------------------------------------------------------ verify

/// Re-hashes every file in the cache. A file at `<root>/<id>/<hex>/<name>` must hash to
/// `<hex>`; anything that does not is a `FETCH-DIGEST` error naming the dataset, and any
/// other file, symlink or unexpected folder is `FETCH-IO`. `.tmp` is skipped. Returns the
/// number of files that verified and the errors, sorted. A missing root is an empty cache.
pub fn verify_cache(root: &Path) -> (usize, Vec<DataError>) {
    let mut ok = 0;
    let mut errors = Vec::new();
    if !root.exists() {
        return (0, errors);
    }
    let io = |id: &str, p: &Path, what: &str| DataError::new(FETCH_IO, id, format!("{}: {what}", p.display()));
    let ids = match sorted_dir(root) {
        Ok(v) => v,
        Err(e) => return (0, vec![io(NO_DATASET, root, &e.to_string())]),
    };
    for (id, id_path, id_type) in ids {
        if id == TMP_DIR {
            continue;
        }
        if !id_type.is_dir() {
            errors.push(io(NO_DATASET, &id_path, "not a dataset folder"));
            continue;
        }
        let hexes = match sorted_dir(&id_path) {
            Ok(v) => v,
            Err(e) => {
                errors.push(io(&id, &id_path, &e.to_string()));
                continue;
            }
        };
        for (hex, hex_path, hex_type) in hexes {
            let expected = Digest::parse(&format!("sha256:{hex}")).ok();
            let Some(expected) = expected.filter(|_| hex_type.is_dir()) else {
                errors.push(io(&id, &hex_path, "not a <sha256 hex> folder"));
                continue;
            };
            let files = match sorted_dir(&hex_path) {
                Ok(v) => v,
                Err(e) => {
                    errors.push(io(&id, &hex_path, &e.to_string()));
                    continue;
                }
            };
            for (_, file_path, file_type) in files {
                if !file_type.is_file() {
                    errors.push(io(&id, &file_path, "not a regular file"));
                    continue;
                }
                match hash_file(&file_path, u64::MAX) {
                    Ok((d, _)) if d == expected => ok += 1,
                    Ok((d, _)) => errors.push(DataError::new(
                        FETCH_DIGEST,
                        &id,
                        format!("{} hashes to {d}, its folder says {expected}", file_path.display()),
                    )),
                    Err(e) => errors.push(io(&id, &file_path, &e.to_string())),
                }
            }
        }
    }
    errors.sort_by(|a, b| (&a.id, a.code, &a.detail).cmp(&(&b.id, b.code, &b.detail)));
    (ok, errors)
}

/// Directory entries sorted by name, without following symlinks.
fn sorted_dir(dir: &Path) -> io::Result<Vec<(String, PathBuf, fs::FileType)>> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir)? {
        let e = e?;
        out.push((e.file_name().to_string_lossy().into_owned(), e.path(), e.file_type()?));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

// ------------------------------------------------------------------ defaults and lookup

/// Whether `FID_DATA_OFFLINE=1` is set.
pub fn env_offline() -> bool {
    std::env::var(ENV_OFFLINE).is_ok_and(|v| v == "1")
}

/// `FID_DATA_CATALOGUE`, else the repository's `datasets.toml`.
pub fn default_catalogue() -> PathBuf {
    match std::env::var_os(ENV_CATALOGUE) {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../datasets.toml")),
    }
}

/// `FID_DATA_CACHE`, else `metrologymaven/datasets` in the per-user cache directory:
/// `%LOCALAPPDATA%` on Windows, `~/Library/Caches` on macOS, `$XDG_CACHE_HOME` or
/// `~/.cache` elsewhere.
pub fn default_cache() -> Result<PathBuf, DataError> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(p) = var(ENV_CACHE) {
        return Ok(p);
    }
    let base = if cfg!(windows) {
        var("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library").join("Caches"))
    } else {
        var("XDG_CACHE_HOME").or_else(|| var("HOME").map(|h| h.join(".cache")))
    };
    base.map(|b| b.join("metrologymaven").join("datasets"))
        .ok_or_else(|| DataError::new(FETCH_IO, NO_DATASET, format!("no per-user cache directory; set {ENV_CACHE}")))
}

/// Reads the fetchable entries of a catalogue file.
pub fn load_catalogue(path: &Path) -> Result<Vec<CatalogueEntry>, DataError> {
    cat::read_bounded(path)
        .and_then(|text| cat::entries(&text).map_err(|e| format!("{}: {e}", path.display())))
        .map_err(|e| DataError::new(FETCH_IO, NO_DATASET, e))
}

/// The entry with ID `id`, or `FETCH-UNKNOWN-ID`.
pub fn find<'a>(entries: &'a [CatalogueEntry], id: &str) -> Result<&'a CatalogueEntry, DataError> {
    entries.iter().find(|e| e.id == id).ok_or_else(|| DataError::new(FETCH_UNKNOWN_ID, id, "not in the catalogue"))
}

/// Path of a verified dataset for tests, benchmarks and examples: reads the default
/// catalogue and cache, fetches the dataset if it is not cached, and never touches the
/// network while `FID_DATA_OFFLINE=1`.
pub fn dataset_path(id: &str) -> Result<PathBuf, DataError> {
    let entries = load_catalogue(&default_catalogue())?;
    let entry = find(&entries, id)?;
    fetch_entry(entry, &default_cache()?, env_offline(), &SystemTransport, &mut io::sink())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn entry(urls: &[&str]) -> CatalogueEntry {
        CatalogueEntry {
            id: "x1".into(),
            owner: "ECO".into(),
            size_bytes: 1,
            sha256: Digest::TOMBSTONE,
            urls: urls.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn file_names() {
        assert_eq!(file_name(&entry(&["https://h/a/b.bin?x=1#f"])), "b.bin");
        assert_eq!(file_name(&entry(&["https://h/a/"])), "x1");
        assert_eq!(file_name(&entry(&["https://h"])), "x1");
        assert_eq!(file_name(&entry(&["https://h/.hidden"])), "x1");
        assert_eq!(file_name(&entry(&["https://h/a%2Fb"])), "x1");
        assert_eq!(file_name(&entry(&["file:///tmp/t.bin"])), "t.bin");
    }

    #[test]
    fn urls() {
        assert!(is_http_url("https://example.invalid/a"));
        assert!(!is_http_url("https://x/[1-9]"));
        assert!(!is_http_url("ftp://x/a"));
        assert!(file_url_path("file://host/share/x").is_none());
        assert!(file_url_path("file:////host/share/x").is_none());
        assert!(file_url_path("file:///a%0Ab").is_none());
        if cfg!(windows) {
            assert_eq!(file_url_path("file:///C:/a%20b/c.bin"), Some(PathBuf::from("C:/a b/c.bin")));
            assert!(file_url_path("file:///a/b").is_none());
        } else {
            assert_eq!(file_url_path("file:///a%20b/c.bin"), Some(PathBuf::from("/a b/c.bin")));
            assert_eq!(file_url_path("file://localhost/a"), Some(PathBuf::from("/a")));
        }
        assert!(SystemTransport.open("ftp://x/a", 1).is_err());
    }
}
