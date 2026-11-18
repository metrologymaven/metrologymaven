//! Acceptance criteria of spec node ECO.DATA.FETCH (AC-1 .. AC-7) and its requirements.
//!
//! The catalogue template, the served files and every digest come from the independent
//! Python script `fixtures/fetch/make_fixtures.py`. HTTP is served by an in-process server
//! bound to 127.0.0.1 that logs every request; no test touches any other host.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_data::cat::{entries, CatalogueEntry};
use fid_data::fetch::{
    fetch_entry, installed_path, remove_stale_parts, verify_cache, Body, Failure, Transport, FETCH_ALL_URLS_FAILED,
    FETCH_IO, FETCH_OFFLINE, FETCH_SIZE, STALE_PART_AGE, TMP_DIR,
};
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// From `python fixtures/fetch/make_fixtures.py`.
const TINY_SHA: &str = "2bebb9539f557a177967a2563c170629380904ad42b316e874b55d46db5a2321";
const MIRRORED_SHA: &str = "baccbe6270d9e62a483a95027154a9cd57441303b55475cbc02b00958f615e88";
const OVERSIZE_LIMIT: u64 = 512;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/fetch")
}

// ------------------------------------------------------------------ test HTTP server

struct Server {
    base: String,
    log: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Server {
    fn start() -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let log = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (log2, stop2, base2) = (log.clone(), stop.clone(), base.clone());
        let handle = std::thread::spawn(move || {
            for conn in listener.incoming() {
                if stop2.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(s) = conn {
                    serve(s, &log2, &base2);
                }
            }
        });
        Server { base, log, stop, handle: Some(handle) }
    }

    fn requests(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.base.trim_start_matches("http://"));
        if let Some(h) = self.handle.take() {
            h.join().unwrap();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

fn serve(mut s: TcpStream, log: &Mutex<Vec<String>>, base: &str) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let mut req = Vec::new();
    let mut buf = [0u8; 1024];
    while !req.windows(4).any(|w| w == b"\r\n\r\n") && req.len() < 16 * 1024 {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => req.extend_from_slice(&buf[..n]),
        }
    }
    let text = String::from_utf8_lossy(&req);
    let path = text.split_whitespace().nth(1).unwrap_or("").to_string();
    log.lock().unwrap().push(path.clone());
    let names = ["tiny.bin", "corrupt.bin", "mirrored.bin", "oversize.bin"];
    let file = path.strip_prefix('/').filter(|n| names.contains(n));
    let _ = if let Some(name) = file {
        // No Content-Length: the body runs to the close, so only the reader's cap stops it.
        let body = std::fs::read(fixtures().join(name)).unwrap();
        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n")
            .and_then(|()| s.write_all(&body))
    } else if let Some(rest) = path.strip_prefix("/redirect/") {
        let location = format!("Location: {base}/{rest}\r\n");
        let head = format!("HTTP/1.1 302 Found\r\n{location}Content-Length: 0\r\nConnection: close\r\n\r\n");
        s.write_all(head.as_bytes())
    } else {
        s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    };
    let _ = s.flush();
    let _ = s.shutdown(Shutdown::Write);
}

// ------------------------------------------------------------------ helpers

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("fid-data-fetch-{}-{name}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `file://` URL of the fixtures folder.
fn file_base() -> String {
    let canon = std::fs::canonicalize(fixtures()).unwrap();
    let s = canon.to_string_lossy().replace('\\', "/");
    let s = s.strip_prefix("//?/").unwrap_or(&s).to_string();
    let s = if s.starts_with('/') { s } else { format!("/{s}") };
    format!("file://{}", s.replace('%', "%25").replace(' ', "%20"))
}

/// The fixture catalogue with its placeholders filled, written to `<dir>/catalogue.toml`.
fn catalogue(dir: &Path, base: &str) -> PathBuf {
    let text = std::fs::read_to_string(fixtures().join("catalogue.toml")).unwrap();
    let text = text.replace("@BASE@", base).replace("@FILEBASE@", &file_base());
    let p = dir.join("catalogue.toml");
    std::fs::write(&p, text).unwrap();
    p
}

/// Runs `fid-data data <args>` with no proxy and no FID_DATA_* variables from the caller.
fn run(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fid-data"));
    cmd.arg("data").args(args);
    for k in ["http_proxy", "HTTP_PROXY", "https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"] {
        cmd.env_remove(k);
    }
    cmd.env("NO_PROXY", "*").env("no_proxy", "*");
    for k in ["FID_DATA_OFFLINE", "FID_DATA_CACHE", "FID_DATA_CATALOGUE"] {
        cmd.env_remove(k);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn fetch_cli(id: &str, cat: &Path, cache: &Path, extra: &[&str]) -> Output {
    let mut args = vec!["fetch", id, "--catalogue", cat.to_str().unwrap(), "--cache", cache.to_str().unwrap()];
    args.extend_from_slice(extra);
    run(&args, &[])
}

/// Every file under `dir`, relative, sorted.
fn files_under(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, d: &Path, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                out.push(p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn assert_tmp_empty(cache: &Path) {
    assert_eq!(files_under(&cache.join(TMP_DIR)), Vec::<String>::new(), "leftover temporary files");
}

// ------------------------------------------------------------------ acceptance criteria

/// AC-1, REQ-002 and AC-2: fetch installs the verified file; a second fetch and an offline
/// fetch with the server stopped make no request.
#[test]
fn ac1_ac2_fetch_then_offline() {
    let mut server = Server::start();
    let tmp = TempDir::new("ac1");
    let cache = tmp.0.join("cache");
    let cat = catalogue(&tmp.0, &server.base);

    let out = fetch_cli("tiny", &cat, &cache, &[]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let installed = cache.join("tiny").join(TINY_SHA).join("tiny.bin");
    assert!(installed.is_file());
    assert_eq!(std::fs::read(&installed).unwrap(), std::fs::read(fixtures().join("tiny.bin")).unwrap());
    assert_eq!(stdout(&out).trim(), installed.display().to_string());
    assert_eq!(server.requests(), vec!["/tiny.bin"]);
    assert_tmp_empty(&cache);

    // REQ-002: cached and verified, so no network access even when online.
    let again = fetch_cli("tiny", &cat, &cache, &[]);
    assert_eq!(code(&again), 0, "{}", stderr(&again));
    assert_eq!(server.requests(), vec!["/tiny.bin"]);

    // AC-2: offline with the server stopped.
    server.stop();
    let offline = fetch_cli("tiny", &cat, &cache, &["--offline"]);
    assert_eq!(code(&offline), 0, "{}", stderr(&offline));
    assert_eq!(stdout(&offline).trim(), installed.display().to_string());
    assert_eq!(server.requests(), vec!["/tiny.bin"]);
}

/// AC-3 / REQ-001: wrong bytes are rejected, named with dataset and URL, and nothing is cached.
#[test]
fn ac3_digest_mismatch_leaves_cache_unchanged() {
    let server = Server::start();
    let tmp = TempDir::new("ac3");
    let cache = tmp.0.join("cache");
    let cat = catalogue(&tmp.0, &server.base);
    let out = fetch_cli("corrupt", &cat, &cache, &[]);
    assert_eq!(code(&out), 1);
    let err = stderr(&out);
    assert!(err.contains("FETCH-DIGEST corrupt"), "{err}");
    assert!(err.contains(&format!("{}/corrupt.bin", server.base)), "{err}");
    assert!(!cache.join("corrupt").exists());
    assert_eq!(files_under(&cache), Vec::<String>::new());
}

/// AC-4 / REQ-004: URLs are tried in catalogue order; the first that verifies wins.
#[test]
fn ac4_mirror_fallback_in_order() {
    let server = Server::start();
    let tmp = TempDir::new("ac4");
    let cache = tmp.0.join("cache");
    let cat = catalogue(&tmp.0, &server.base);
    let out = fetch_cli("mirrored", &cat, &cache, &[]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(server.requests(), vec!["/missing/mirrored.bin", "/mirrored.bin"]);
    let err = stderr(&out);
    let first = err.find(&format!("trying {}/missing/mirrored.bin", server.base)).expect(&err);
    let second = err.find(&format!("trying {}/mirrored.bin", server.base)).expect(&err);
    assert!(first < second, "{err}");
    assert!(cache.join("mirrored").join(MIRRORED_SHA).join("mirrored.bin").is_file());
    assert_tmp_empty(&cache);
}

/// AC-5 / REQ-003: offline with an empty cache fails and makes no request, by flag or variable.
#[test]
fn ac5_offline_empty_cache() {
    let server = Server::start();
    let tmp = TempDir::new("ac5");
    let cache = tmp.0.join("cache");
    let cat = catalogue(&tmp.0, &server.base);
    let out = fetch_cli("tiny", &cat, &cache, &["--offline"]);
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("FETCH-OFFLINE tiny"), "{}", stderr(&out));

    let args = ["fetch", "tiny", "--catalogue", cat.to_str().unwrap(), "--cache", cache.to_str().unwrap()];
    let by_env = run(&args, &[("FID_DATA_OFFLINE", "1")]);
    assert_eq!(code(&by_env), 1);
    assert!(stderr(&by_env).contains("FETCH-OFFLINE tiny"), "{}", stderr(&by_env));
    assert_eq!(server.requests(), Vec::<String>::new());
    assert!(!cache.join("tiny").exists());
}

/// AC-6 / REQ-006: a response longer than size_bytes is refused and nothing is cached.
#[test]
fn ac6_oversize_refused() {
    let server = Server::start();
    let tmp = TempDir::new("ac6");
    let cache = tmp.0.join("cache");
    let cat = catalogue(&tmp.0, &server.base);
    let out = fetch_cli("oversize", &cat, &cache, &[]);
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("FETCH-SIZE oversize"), "{}", stderr(&out));
    assert!(!cache.join("oversize").exists());
    assert_tmp_empty(&cache);
}

/// A transport that serves an endless stream and counts the bytes handed out.
struct Endless(Arc<AtomicU64>);

impl Read for Endless {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        buf.fill(0xa5);
        self.0.fetch_add(buf.len() as u64, Ordering::SeqCst);
        Ok(buf.len())
    }
}

impl Body for Endless {
    fn finish(&mut self) -> Result<(), Failure> {
        Ok(())
    }
}

struct EndlessTransport(Arc<AtomicU64>);

impl Transport for EndlessTransport {
    fn open(&self, _url: &str, _max: u64) -> Result<Box<dyn Body>, Failure> {
        Ok(Box::new(Endless(self.0.clone())))
    }
}

fn fixture_entry(id: &str) -> CatalogueEntry {
    let text = std::fs::read_to_string(fixtures().join("catalogue.toml")).unwrap();
    let text = text.replace("@BASE@", "http://127.0.0.1:9").replace("@FILEBASE@", "file:///nonexistent");
    entries(&text, true).unwrap().into_iter().find(|e| e.id == id).unwrap()
}

/// AC-6: the reader stops after at most size_bytes + 1 bytes.
#[test]
fn ac6_reads_at_most_size_plus_one() {
    let tmp = TempDir::new("ac6lib");
    let count = Arc::new(AtomicU64::new(0));
    let entry = fixture_entry("oversize");
    assert_eq!(entry.size_bytes, OVERSIZE_LIMIT);
    let err = fetch_entry(&entry, &tmp.0, false, &EndlessTransport(count.clone()), &mut io::sink()).unwrap_err();
    assert_eq!(err.code, FETCH_SIZE);
    assert!(count.load(Ordering::SeqCst) <= OVERSIZE_LIMIT + 1, "read {}", count.load(Ordering::SeqCst));
    assert!(!installed_path(&tmp.0, &entry).exists());
    assert_tmp_empty(&tmp.0);
}

/// AC-7: a cache file altered after install fails `data verify`, which names the dataset.
#[test]
fn ac7_verify_detects_altered_file() {
    let server = Server::start();
    let tmp = TempDir::new("ac7");
    let cache = tmp.0.join("cache");
    let cat = catalogue(&tmp.0, &server.base);
    assert_eq!(code(&fetch_cli("tiny", &cat, &cache, &[])), 0);
    let ok = run(&["verify", "--cache", cache.to_str().unwrap()], &[]);
    assert_eq!(code(&ok), 0, "{}", stderr(&ok));
    assert!(stdout(&ok).contains("ok (1 files"), "{}", stdout(&ok));

    let installed = cache.join("tiny").join(TINY_SHA).join("tiny.bin");
    let mut bytes = std::fs::read(&installed).unwrap();
    bytes[100] ^= 1;
    std::fs::write(&installed, bytes).unwrap();
    let bad = run(&["verify", "--cache", cache.to_str().unwrap()], &[]);
    assert_eq!(code(&bad), 1);
    assert!(stderr(&bad).contains("FETCH-DIGEST tiny"), "{}", stderr(&bad));

    // The library reports the same, and a later fetch repairs the file from a verified download.
    let (n, errors) = verify_cache(&cache);
    assert_eq!((n, errors.len()), (0, 1));
    assert_eq!(code(&fetch_cli("tiny", &cat, &cache, &["--offline"])), 1);
    assert_eq!(code(&fetch_cli("tiny", &cat, &cache, &[])), 0);
    assert_eq!(verify_cache(&cache), (1, vec![]));
}

// ------------------------------------------------------------------ further requirements

/// Redirects are refused, so no host outside the catalogue URL is contacted.
#[test]
fn redirect_is_refused() {
    let server = Server::start();
    let tmp = TempDir::new("redirect");
    let cache = tmp.0.join("cache");
    let cat = catalogue(&tmp.0, &server.base);
    let out = fetch_cli("redirect", &cat, &cache, &[]);
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    assert!(stderr(&out).contains("FETCH-ALL-URLS-FAILED redirect"), "{}", stderr(&out));
    assert_eq!(server.requests(), vec!["/redirect/tiny.bin"]);
    assert!(!cache.join("redirect").exists());
}

/// `file://` URLs (local mirrors) need no server.
#[test]
fn file_url_mirror() {
    let tmp = TempDir::new("file");
    let cache = tmp.0.join("cache");
    let cat = catalogue(&tmp.0, "http://127.0.0.1:9");
    let out = fetch_cli("local", &cat, &cache, &[]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(cache.join("local").join(TINY_SHA).join("tiny.bin").is_file());
}

/// Unknown IDs, `data path` with the environment variables, and usage errors.
#[test]
fn unknown_id_path_and_usage() {
    let tmp = TempDir::new("path");
    let cache = tmp.0.join("cache");
    let cat = catalogue(&tmp.0, "http://127.0.0.1:9");
    let out = fetch_cli("no-such-id", &cat, &cache, &[]);
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("FETCH-UNKNOWN-ID no-such-id"), "{}", stderr(&out));

    let env = [("FID_DATA_CACHE", cache.to_str().unwrap()), ("FID_DATA_OFFLINE", "1")];
    let path_args = ["path", "local", "--catalogue", cat.to_str().unwrap()];
    let missing = run(&path_args, &env);
    assert_eq!(code(&missing), 1);
    assert!(stderr(&missing).contains("FETCH-OFFLINE local"), "{}", stderr(&missing));
    assert_eq!(code(&fetch_cli("local", &cat, &cache, &[])), 0);
    let found = run(&path_args, &env);
    assert_eq!(code(&found), 0, "{}", stderr(&found));
    assert_eq!(stdout(&found).trim(), cache.join("local").join(TINY_SHA).join("tiny.bin").display().to_string());

    let bads: [&[&str]; 5] = [&["fetch"], &["fetch", "--all", "tiny"], &["path"], &["verify", "tiny"], &["frob"]];
    for bad in bads {
        assert_eq!(code(&run(bad, &[])), 2, "{bad:?}");
    }
}

/// A transport whose first URL breaks off mid-body and whose second URL serves the bytes.
struct Flaky {
    tiny: Vec<u8>,
}

struct Broken(Vec<u8>, usize);

impl Read for Broken {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.1 >= 100 {
            return Err(io::Error::new(io::ErrorKind::ConnectionReset, "interrupted"));
        }
        let n = buf.len().min(100 - self.1);
        buf[..n].copy_from_slice(&self.0[self.1..self.1 + n]);
        self.1 += n;
        Ok(n)
    }
}

impl Body for Broken {
    fn finish(&mut self) -> Result<(), Failure> {
        Ok(())
    }
}

struct Whole(io::Cursor<Vec<u8>>);

impl Read for Whole {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Body for Whole {
    fn finish(&mut self) -> Result<(), Failure> {
        Ok(())
    }
}

impl Transport for Flaky {
    fn open(&self, url: &str, _max: u64) -> Result<Box<dyn Body>, Failure> {
        match url {
            "mem://broken" => Ok(Box::new(Broken(self.tiny.clone(), 0))),
            "mem://whole" => Ok(Box::new(Whole(io::Cursor::new(self.tiny.clone())))),
            _ => Err(Failure::Other("no such URL".into())),
        }
    }
}

/// REQ-005: an interrupted download leaves no file at the final path and no temporary file.
#[test]
fn interrupted_fetch_leaves_nothing() {
    let tmp = TempDir::new("interrupt");
    let tiny = std::fs::read(fixtures().join("tiny.bin")).unwrap();
    let mut entry = fixture_entry("tiny");
    entry.urls = vec!["mem://broken".into()];
    let flaky = Flaky { tiny };
    let err = fetch_entry(&entry, &tmp.0, false, &flaky, &mut io::sink()).unwrap_err();
    assert_eq!(err.code, FETCH_ALL_URLS_FAILED);
    assert!(err.detail.contains("mem://broken"), "{err}");
    assert!(!installed_path(&tmp.0, &entry).exists());
    assert_tmp_empty(&tmp.0);

    entry.urls = vec!["mem://broken".into(), "mem://whole".into()];
    let mut log = Vec::new();
    let path = fetch_entry(&entry, &tmp.0, false, &flaky, &mut log).unwrap();
    // `mem://broken` has no path segment, so the file is named after the dataset ID.
    assert_eq!(path, tmp.0.join("tiny").join(TINY_SHA).join("tiny"));
    assert!(String::from_utf8(log).unwrap().contains("trying mem://whole"));
    assert_tmp_empty(&tmp.0);
}

/// Offline mode never asks the transport for anything.
#[test]
fn offline_never_opens() {
    struct Refuse;
    impl Transport for Refuse {
        fn open(&self, url: &str, _max: u64) -> Result<Box<dyn Body>, Failure> {
            panic!("offline fetch opened {url}");
        }
    }
    let tmp = TempDir::new("offline-lib");
    let err = fetch_entry(&fixture_entry("tiny"), &tmp.0, true, &Refuse, &mut io::sink()).unwrap_err();
    assert_eq!(err.code, FETCH_OFFLINE);
    assert_ne!(err.code, FETCH_IO);
}

/// The repository catalogue loads for fetching (no download happens here).
#[test]
fn repository_catalogue_loads() {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../datasets.toml")).unwrap();
    let all = entries(&text, false).unwrap();
    assert!(!all.is_empty());
    assert!(all.iter().all(|e| e.urls.iter().all(|u| u.starts_with("https://"))));
}

/// `file://` URLs are accepted only from a catalogue named explicitly, never by default.
#[test]
fn file_urls_need_an_explicit_catalogue() {
    let tmp = TempDir::new("explicit");
    let cat = catalogue(&tmp.0, "http://127.0.0.1:9");
    let text = std::fs::read_to_string(&cat).unwrap();
    assert!(entries(&text, true).is_ok());
    let err = entries(&text, false).unwrap_err();
    assert!(err.contains("urls"), "{err}");

    // FID_DATA_CATALOGUE counts as explicit, like --catalogue (file_url_mirror).
    let cache = tmp.0.join("cache");
    let env = [("FID_DATA_CATALOGUE", cat.to_str().unwrap()), ("FID_DATA_CACHE", cache.to_str().unwrap())];
    let out = run(&["fetch", "local"], &env);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(cache.join("local").join(TINY_SHA).join("tiny.bin").is_file());
}

/// Proxy variables are ignored: curl connects to the catalogue host only.
#[test]
fn proxy_variables_are_ignored() {
    let server = Server::start();
    let proxy = Server::start();
    let tmp = TempDir::new("proxy");
    let cache = tmp.0.join("cache");
    let cat = catalogue(&tmp.0, &server.base);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fid-data"));
    cmd.args(["data", "fetch", "tiny", "--catalogue", cat.to_str().unwrap(), "--cache", cache.to_str().unwrap()]);
    for k in ["NO_PROXY", "no_proxy", "FID_DATA_OFFLINE", "FID_DATA_CACHE", "FID_DATA_CATALOGUE"] {
        cmd.env_remove(k);
    }
    for k in ["http_proxy", "HTTP_PROXY", "https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"] {
        cmd.env(k, &proxy.base);
    }
    let out = cmd.output().unwrap();
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(proxy.requests(), Vec::<String>::new());
    assert_eq!(server.requests(), vec!["/tiny.bin"]);
}

/// `--all` fetches every entry in catalogue order and reports each failure.
#[test]
fn fetch_all() {
    let server = Server::start();
    let tmp = TempDir::new("all");
    let cache = tmp.0.join("cache");
    let cat = catalogue(&tmp.0, &server.base);
    let args = ["fetch", "--all", "--catalogue", cat.to_str().unwrap(), "--cache", cache.to_str().unwrap()];
    let out = run(&args, &[]);
    assert_eq!(code(&out), 1, "{}", stderr(&out));
    let err = stderr(&out);
    for want in ["FETCH-DIGEST corrupt", "FETCH-SIZE oversize", "FETCH-ALL-URLS-FAILED redirect"] {
        assert!(err.contains(want), "{want}: {err}");
    }
    let printed: Vec<String> = stdout(&out).lines().map(String::from).collect();
    let tiny = ("tiny", TINY_SHA, "tiny.bin");
    let installed = [tiny, ("mirrored", MIRRORED_SHA, "mirrored.bin"), ("local", TINY_SHA, "tiny.bin")];
    let want: Vec<String> = installed
        .iter()
        .map(|(id, sha, name)| cache.join(id).join(sha).join(name).display().to_string())
        .collect();
    assert_eq!(printed, want);
    let requested = ["/tiny.bin", "/corrupt.bin", "/missing/mirrored.bin", "/mirrored.bin", "/oversize.bin"];
    assert_eq!(server.requests(), [&requested[..], &["/redirect/tiny.bin"][..]].concat());
    for id in ["corrupt", "oversize", "redirect"] {
        assert!(!cache.join(id).exists(), "{id}");
    }
    assert_tmp_empty(&cache);
}

/// `--owner` fetches only the entries of that domain; an unmatched owner fetches nothing.
#[test]
fn fetch_owner() {
    let server = Server::start();
    let tmp = TempDir::new("owner");
    let cache = tmp.0.join("cache");
    let cat = catalogue(&tmp.0, &server.base);
    let base = ["fetch", "--catalogue", cat.to_str().unwrap(), "--cache", cache.to_str().unwrap(), "--owner"];
    let krn = run(&[&base[..], &["KRN"][..]].concat(), &[]);
    assert_eq!(code(&krn), 0, "{}", stderr(&krn));
    assert_eq!(stdout(&krn).trim(), cache.join("local").join(TINY_SHA).join("tiny.bin").display().to_string());
    assert_eq!(files_under(&cache), vec![format!("local/{TINY_SHA}/tiny.bin")]);
    let none = run(&[&base[..], &["UNC"][..]].concat(), &[]);
    assert_eq!(code(&none), 0, "{}", stderr(&none));
    assert_eq!(stdout(&none), "");
    assert_eq!(server.requests(), Vec::<String>::new());
}

/// Stale `.part` files in `.tmp/` are removed at the start of a fetch; fresh ones stay.
#[test]
fn stale_parts_are_removed() {
    let tmp = TempDir::new("stale");
    let tmp_dir = tmp.0.join(TMP_DIR);
    std::fs::create_dir_all(&tmp_dir).unwrap();
    let stale = tmp_dir.join("old.1.2.3.part");
    let fresh = tmp_dir.join("new.1.2.3.part");
    let other = tmp_dir.join("keep.txt");
    for p in [&stale, &fresh, &other] {
        std::fs::write(p, b"x").unwrap();
    }
    let old = std::time::SystemTime::now() - STALE_PART_AGE - Duration::from_secs(3600);
    for p in [&stale, &other] {
        std::fs::OpenOptions::new().write(true).open(p).unwrap().set_modified(old).unwrap();
    }
    assert_eq!(remove_stale_parts(&tmp_dir, STALE_PART_AGE), 1);
    assert!(!stale.exists() && fresh.exists() && other.exists());

    // fetch_entry runs the same cleanup before downloading.
    std::fs::write(&stale, b"x").unwrap();
    std::fs::OpenOptions::new().write(true).open(&stale).unwrap().set_modified(old).unwrap();
    let mut tiny_entry = fixture_entry("local");
    tiny_entry.urls = vec!["mem://whole".into()];
    let flaky = Flaky { tiny: std::fs::read(fixtures().join("tiny.bin")).unwrap() };
    fetch_entry(&tiny_entry, &tmp.0, false, &flaky, &mut io::sink()).unwrap();
    assert!(!stale.exists() && fresh.exists());
}
