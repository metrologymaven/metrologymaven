//! Hashing with a size cap, and fetching `url` entries into a content-addressed cache.

use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Source of the bytes behind a `url` entry. Tests inject their own; [`SystemFetcher`] is the default.
pub trait Fetcher {
    /// Streams the bytes at `url` into `sink`. The sink enforces the size cap and fails once it
    /// is exceeded. Errors are human-readable details.
    fn fetch(&self, url: &str, sink: &mut dyn Write) -> Result<(), String>;
}

/// The default fetcher: `https://` and `http://` URLs are downloaded by the `curl` program
/// (present on every CI runner), so this crate links no TLS stack. No other scheme is accepted:
/// a manifest can never make the tool read a local or UNC path.
#[derive(Debug, Clone, Copy)]
pub struct SystemFetcher {
    /// Passed to `curl --max-filesize`; the sink enforces the same cap.
    pub max_bytes: u64,
}

impl Fetcher for SystemFetcher {
    fn fetch(&self, url: &str, sink: &mut dyn Write) -> Result<(), String> {
        if is_fetchable_url(url) {
            curl(url, self.max_bytes, sink)
        } else {
            Err(format!("refused URL {url:?}: only plain http:// and https:// URLs are fetched"))
        }
    }
}

/// Whether `url` may be fetched: `https://` or `http://` with a non-empty rest, at most 2,048
/// bytes, and no whitespace, control characters or curl glob characters (`[]{}`).
pub fn is_fetchable_url(url: &str) -> bool {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"));
    rest.is_some_and(|r| !r.is_empty())
        && url.len() <= 2048
        && !url.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '[' | ']' | '{' | '}'))
}

/// Seconds allowed to connect, and for a whole transfer.
const CONNECT_TIMEOUT_S: &str = "30";
const MAX_TIME_S: &str = "600";

fn curl(url: &str, max_bytes: u64, sink: &mut dyn Write) -> Result<(), String> {
    let max = max_bytes.to_string();
    // `-q` must come first: it makes curl ignore any ~/.curlrc.
    let mut child = Command::new("curl")
        .args(["-q", "--globoff", "--fail", "--silent", "--show-error", "--location", "--max-redirs", "5"])
        .args(["--proto", "=http,https", "--proto-redir", "=http,https"])
        .args(["--connect-timeout", CONNECT_TIMEOUT_S, "--max-time", MAX_TIME_S, "--max-filesize", &max])
        .args(["--", url])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run curl: {e}"))?;
    let copied = match child.stdout.take() {
        Some(mut out) => io::copy(&mut out, sink).map(|_| ()).map_err(|e| e.to_string()),
        None => Err("curl gave no output pipe".to_string()),
    };
    if copied.is_err() {
        let _ = child.kill();
    }
    let out = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;
    copied?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("curl failed ({}): {}", out.status, String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// A writer that hashes and counts what passes through and fails past `max` bytes.
struct HashSink<W> {
    inner: W,
    hasher: Sha256,
    written: u64,
    max: u64,
    over_cap: bool,
}

impl<W: Write> HashSink<W> {
    fn new(inner: W, max: u64) -> Self {
        HashSink { inner, hasher: Sha256::new(), written: 0, max, over_cap: false }
    }

    fn digest(&self) -> Digest {
        Digest::from_bytes(self.hasher.clone().finalize().into())
    }
}

impl<W: Write> Write for HashSink<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = u64::try_from(buf.len()).unwrap_or(u64::MAX);
        if self.written.saturating_add(n) > self.max {
            self.over_cap = true;
            return Err(io::Error::other(format!("size cap of {} bytes exceeded", self.max)));
        }
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// SHA-256 of everything `r` yields, failing past `max` bytes.
pub fn hash_reader(r: &mut dyn Read, max: u64) -> Result<Digest, String> {
    let mut sink = HashSink::new(io::sink(), max);
    io::copy(r, &mut sink).map_err(|e| e.to_string())?;
    Ok(sink.digest())
}

/// SHA-256 of a file, failing past `max` bytes.
pub fn hash_file(path: &Path, max: u64) -> Result<Digest, String> {
    let mut f = fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    hash_reader(&mut f, max)
}

/// Why [`fetch_to_cache`] failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// The fetched bytes hash to this digest instead of the expected one. Nothing is cached.
    DigestMismatch(Digest),
    /// The fetch or the cache write failed. Nothing is cached.
    Failed(String),
}

/// Where [`fetch_to_cache`] keeps the verified bytes of `expected`: `<cache>/<64 hex digits>`.
pub fn cache_path(cache: &Path, expected: &Digest) -> PathBuf {
    cache.join(expected.hex())
}

/// Fetches `url` into `<cache>/<hex>` and verifies its SHA-256 against `expected`
/// (REQ-IO.CORP.MANIF-003). Bytes go to `<hex>.part` first and are renamed only after the
/// digest matches, so a failed or mismatching fetch leaves no file for the entry. A cached
/// file whose digest still matches is reused; a corrupted one is replaced.
///
/// Returns the cache path and whether a fetch happened.
pub fn fetch_to_cache(
    fetcher: &dyn Fetcher,
    url: &str,
    expected: &Digest,
    cache: &Path,
    max_bytes: u64,
) -> Result<(PathBuf, bool), FetchError> {
    let target = cache_path(cache, expected);
    if target.is_file() {
        if hash_file(&target, max_bytes).as_ref() == Ok(expected) {
            return Ok((target, false));
        }
        fs::remove_file(&target).map_err(|e| FetchError::Failed(format!("cannot replace corrupt cache file: {e}")))?;
    }
    fs::create_dir_all(cache).map_err(|e| FetchError::Failed(format!("cannot create cache: {e}")))?;
    let part = cache.join(part_name(expected));
    let result = fetch_part(fetcher, url, &part, max_bytes);
    let outcome = match result {
        Ok(actual) if actual == *expected => {
            fs::rename(&part, &target).map(|()| (target, true)).map_err(|e| FetchError::Failed(e.to_string()))
        }
        Ok(actual) => Err(FetchError::DigestMismatch(actual)),
        Err(e) => Err(FetchError::Failed(e)),
    };
    if outcome.is_err() {
        let _ = fs::remove_file(&part);
    }
    outcome
}

/// A temporary name no other process or call uses: `<hex>.<pid>.<nanos>.<counter>.part`.
fn part_name(expected: &Digest) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    format!("{}.{}.{nanos}.{n}.part", expected.hex(), std::process::id())
}

fn fetch_part(fetcher: &dyn Fetcher, url: &str, part: &Path, max_bytes: u64) -> Result<Digest, String> {
    // create_new: never write through an existing file or symlink at this name.
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(part)
        .map_err(|e| format!("cannot write cache: {e}"))?;
    let mut sink = HashSink::new(io::BufWriter::new(file), max_bytes);
    let fetched = fetcher.fetch(url, &mut sink);
    if sink.over_cap {
        return Err(format!("size cap of {max_bytes} bytes exceeded"));
    }
    fetched?;
    sink.flush().map_err(|e| e.to_string())?;
    Ok(sink.digest())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn fetchable_urls() {
        assert!(is_fetchable_url("https://example.invalid/a/b.stl?x=1"));
        assert!(is_fetchable_url("http://127.0.0.1:8080/a"));
        for bad in [
            "file:///etc/passwd",
            "file:////host/share/x",
            "ftp://x/a",
            "https://",
            "https://x/a b",
            "https://x/a\tb",
            "https://x/a\n",
            "https://x/[1-9]",
            "https://x/{a,b}",
            "-o/tmp/x",
            "HTTPS://x/a",
        ] {
            assert!(!is_fetchable_url(bad), "{bad:?}");
        }
        let refused = SystemFetcher { max_bytes: 10 }.fetch("file:///etc/passwd", &mut io::sink());
        assert!(refused.is_err());
    }

    #[test]
    fn part_names_are_unique() {
        let d = Digest::TOMBSTONE;
        assert_ne!(part_name(&d), part_name(&d));
        assert!(part_name(&d).ends_with(".part"));
    }

    #[test]
    fn hash_cap() {
        let empty = "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(hash_reader(&mut &b""[..], 0).unwrap().as_str(), empty);
        let abc = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(hash_reader(&mut &b"abc"[..], 3).unwrap().as_str(), abc);
        assert!(hash_reader(&mut &b"abc"[..], 2).is_err());
    }
}
