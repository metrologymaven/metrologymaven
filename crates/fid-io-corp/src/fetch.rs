//! Hashing with a size cap, and fetching `url` entries into a content-addressed cache.

use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Source of the bytes behind a `url` entry. Tests inject their own; [`SystemFetcher`] is the default.
pub trait Fetcher {
    /// Streams the bytes at `url` into `sink`. The sink enforces the size cap and fails once it
    /// is exceeded. Errors are human-readable details.
    fn fetch(&self, url: &str, sink: &mut dyn Write) -> Result<(), String>;
}

/// The default fetcher: `file://` URLs are read directly; `http://` and `https://` URLs are
/// downloaded by the `curl` program (present on every CI runner), so this crate links no TLS stack.
#[derive(Debug, Clone, Copy)]
pub struct SystemFetcher {
    /// Passed to `curl --max-filesize`; the sink enforces the same cap on every scheme.
    pub max_bytes: u64,
}

impl Fetcher for SystemFetcher {
    fn fetch(&self, url: &str, sink: &mut dyn Write) -> Result<(), String> {
        if url.starts_with("file://") {
            let path = file_url_to_path(url)?;
            let mut f = fs::File::open(&path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
            io::copy(&mut f, sink).map(|_| ()).map_err(|e| e.to_string())
        } else if url.starts_with("https://") || url.starts_with("http://") {
            curl(url, self.max_bytes, sink)
        } else {
            Err(format!("unsupported URL scheme in {url:?}"))
        }
    }
}

fn curl(url: &str, max_bytes: u64, sink: &mut dyn Write) -> Result<(), String> {
    let max = max_bytes.to_string();
    let mut child = Command::new("curl")
        .args(["--fail", "--silent", "--show-error", "--location", "--proto", "=https,http"])
        .args(["--proto-redir", "=https,http", "--max-filesize", &max, url])
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

/// Decodes `%XX` escapes; fails on a bad escape or non-UTF-8 result.
fn percent_decode(s: &str) -> Result<String, String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok());
            let Some(v) = hex else {
                return Err(format!("bad percent escape in {s:?}"));
            };
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| format!("non-UTF-8 path in {s:?}"))
}

/// The local path of a `file://` URL (empty host or `localhost` only). On Windows,
/// `file:///C:/dir/f` becomes `C:/dir/f`.
pub fn file_url_to_path(url: &str) -> Result<PathBuf, String> {
    let rest = url.strip_prefix("file://").ok_or_else(|| format!("not a file URL: {url:?}"))?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return Err(format!("file URL with a remote host: {url:?}"));
    }
    let path = percent_decode(rest)?;
    let b = path.as_bytes();
    let drive = b.len() >= 3 && b[1].is_ascii_alphabetic() && b[2] == b':';
    if cfg!(windows) && drive {
        return Ok(PathBuf::from(&path[1..]));
    }
    Ok(PathBuf::from(path))
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
    let part = cache.join(format!("{}.part", expected.hex()));
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

fn fetch_part(fetcher: &dyn Fetcher, url: &str, part: &Path, max_bytes: u64) -> Result<Digest, String> {
    let file = fs::File::create(part).map_err(|e| format!("cannot write cache: {e}"))?;
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
    fn percent_and_file_urls() {
        assert_eq!(percent_decode("a%20b%2Fc").unwrap(), "a b/c");
        assert!(percent_decode("a%2").is_err());
        assert!(percent_decode("a%zz").is_err());
        assert!(percent_decode("%ff").is_err());
        assert!(file_url_to_path("file://host/x").is_err());
        assert!(file_url_to_path("https://x/y").is_err());
        assert_eq!(file_url_to_path("file://localhost/tmp/a%20b").unwrap(), PathBuf::from("/tmp/a b"));
        let drive = file_url_to_path("file:///C:/x/y").unwrap();
        if cfg!(windows) {
            assert_eq!(drive, PathBuf::from("C:/x/y"));
        } else {
            assert_eq!(drive, PathBuf::from("/C:/x/y"));
        }
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
