//! Acceptance criteria of spec node IO.CORP.MANIF (AC-1 .. AC-5).
//!
//! Nothing here touches the internet. `url` entries are served by an injected in-memory
//! fetcher (the "test server"), by `file://` URLs, or by a one-shot HTTP server on 127.0.0.1.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_corp::{
    cache_path, check_corpus, fetch_to_cache, parse_format_listing, CheckOptions, CorpError, FetchError, Fetcher,
    Fetching, FormatId, Report, SystemFetcher, DEFAULT_MAX_BYTES,
};
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/io/corp/manif");
const REMOTE_URL: &str = "https://corpus.example.invalid/mesh.stl/truncated_binary.stl";
// Hand-copied from fixtures/io/corp/manif/ok/mesh.stl.toml (computed by make_fixtures.py).
const REMOTE_SHA: &str = "sha256:6e13bb6b91678e9931a04c6fbe535f50e5057b59f8183dc1b91d8ed2e314cde6";
const TETRA_SHA: &str = "sha256:e8104cf29031ff92b5bf004c5a2bc6a255ae02448017d1697853bea72ab1bb9e";

fn fix(rel: &str) -> PathBuf {
    Path::new(FIX).join(rel)
}

fn registry(name: &str) -> Vec<FormatId> {
    parse_format_listing(&fs::read(fix(name)).unwrap()).unwrap()
}

fn digest(s: &str) -> Digest {
    Digest::parse(s).unwrap()
}

/// A fresh, empty directory under the system temp dir.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fid-io-corp-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for item in fs::read_dir(from).unwrap() {
        let item = item.unwrap();
        let target = to.join(item.file_name());
        if item.file_type().unwrap().is_dir() {
            copy_dir(&item.path(), &target);
        } else {
            fs::copy(item.path(), target).unwrap();
        }
    }
}

fn files_in(dir: &Path) -> Vec<String> {
    match fs::read_dir(dir) {
        Ok(r) => r.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect(),
        Err(_) => Vec::new(),
    }
}

/// In-memory stand-in for the test HTTP server: serves fixed bytes per URL, optionally altered.
struct TestServer {
    files: BTreeMap<String, Vec<u8>>,
    alter: bool,
}

impl TestServer {
    fn remote(alter: bool) -> Self {
        let bytes = fs::read(fix("remote/truncated_binary.stl")).unwrap();
        TestServer { files: [(REMOTE_URL.to_string(), bytes)].into_iter().collect(), alter }
    }
}

impl Fetcher for TestServer {
    fn fetch(&self, url: &str, sink: &mut dyn Write) -> Result<(), String> {
        let mut bytes = self.files.get(url).cloned().ok_or_else(|| format!("404 {url}"))?;
        if self.alter {
            bytes[0] ^= 1;
        }
        sink.write_all(&bytes).map_err(|e| e.to_string())
    }
}

fn check(dir: &Path, registry_file: &str, fetch: Option<Fetching<'_>>) -> Report {
    check_corpus(dir, &registry(registry_file), &CheckOptions { max_bytes: DEFAULT_MAX_BYTES, fetch })
}

fn codes(r: &Report) -> Vec<&'static str> {
    r.errors.iter().map(CorpError::code).collect()
}

/// Copies `ok/` to a scratch dir and points its url entry at `url`.
fn ok_copy_with_url(name: &str, url: &str) -> PathBuf {
    let dir = scratch(name);
    let corpus = dir.join("corpus");
    copy_dir(&fix("ok"), &corpus);
    let m = corpus.join("mesh.stl.toml");
    let text = fs::read_to_string(&m).unwrap();
    assert!(text.contains(REMOTE_URL));
    fs::write(&m, text.replace(REMOTE_URL, url)).unwrap();
    dir
}

/// One-shot HTTP/1.1 server on 127.0.0.1 answering every request with `body`.
fn local_http(body: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut s, _)) = listener.accept() {
            let mut req = Vec::new();
            let mut buf = [0u8; 1024];
            while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                match s.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => req.extend_from_slice(&buf[..n]),
                }
            }
            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(&body);
        }
    });
    format!("http://127.0.0.1:{port}/truncated_binary.stl")
}

/// Whether `curl` can run. Off CI a missing curl skips the HTTP tests; on CI (`CI` set) it fails them.
fn curl_available() -> bool {
    let ok = Command::new("curl").arg("--version").output().map(|o| o.status.success()).unwrap_or(false);
    if !ok && std::env::var_os("CI").is_some() {
        panic!("curl is required for the HTTP tests on CI");
    }
    if !ok {
        eprintln!("skipped: curl not on PATH");
    }
    ok
}

#[test]
fn ac1_ok_corpus_passes_with_url_served_by_test_server() {
    let cache = scratch("ac1-cache");
    let server = TestServer::remote(false);
    let r = check(&fix("ok"), "registry_ok.json", Some(Fetching { cache: &cache, fetcher: &server }));
    assert!(r.is_ok(), "{:?}", r.errors);
    assert_eq!(r.manifests.len(), 2);
    assert_eq!((r.files_checked, r.fetched, r.cached, r.urls_skipped), (2, 1, 0, 0));
    assert_eq!(files_in(&cache), [digest(REMOTE_SHA).hex()]);
    // Second run is served from the verified cache.
    let r = check(&fix("ok"), "registry_ok.json", Some(Fetching { cache: &cache, fetcher: &server }));
    assert!(r.is_ok(), "{:?}", r.errors);
    assert_eq!((r.fetched, r.cached), (0, 1));
    // Without fetching, the url entry is skipped and the rest still passes.
    let r = check(&fix("ok"), "registry_ok.json", None);
    assert!(r.is_ok(), "{:?}", r.errors);
    assert_eq!(r.urls_skipped, 1);
}

#[test]
fn ac1_corpus_check_binary_exits_0() {
    if !curl_available() {
        return;
    }
    let body = fs::read(fix("remote/truncated_binary.stl")).unwrap();
    let dir = ok_copy_with_url("ac1-bin", &local_http(body));
    let out = Command::new(env!("CARGO_BIN_EXE_corpus-check"))
        .arg("--corpus")
        .arg(dir.join("corpus"))
        .arg("--formats")
        .arg(fix("registry_ok.json"))
        .arg("--cache")
        .arg(dir.join("cache"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}\n{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("1 fetched"), "{stdout}");
    assert!(cache_path(&dir.join("cache"), &digest(REMOTE_SHA)).is_file());
}

#[test]
fn ac1_url_entry_over_local_http_server() {
    if !curl_available() {
        return;
    }
    let body = fs::read(fix("remote/truncated_binary.stl")).unwrap();
    let dir = ok_copy_with_url("ac1-http", &local_http(body));
    let cache = dir.join("cache");
    let fetcher = SystemFetcher { max_bytes: DEFAULT_MAX_BYTES };
    let r = check(&dir.join("corpus"), "registry_ok.json", Some(Fetching { cache: &cache, fetcher: &fetcher }));
    assert!(r.is_ok(), "{:?}", r.errors);
    assert_eq!(r.fetched, 1);
}

#[test]
fn ac2_registered_format_without_manifest() {
    assert_eq!(
        registry("registry_obj.json").iter().map(FormatId::as_str).collect::<Vec<_>>(),
        ["mesh.obj", "mesh.ply", "mesh.stl"]
    );
    let r = check(&fix("ok"), "registry_obj.json", None);
    assert_eq!(r.errors, [CorpError::ManifestMissing { format: "mesh.obj".into() }]);
    assert_eq!(r.errors[0].to_string().split(':').next(), Some("manifest_missing{mesh.obj}"));
}

#[test]
fn ac3_in_repo_file_with_unredistributable_licence() {
    let r = check(&fix("bad_licence"), "registry_ok.json", None);
    assert_eq!(
        r.errors,
        [
            CorpError::LicenceNotRedistributable {
                format: "mesh.stl".into(),
                entry: "files/tetra.stl".into(),
                licence: "unknown".into()
            },
            CorpError::LicenceNotRedistributable {
                format: "mesh.stl".into(),
                entry: "files/other.stl".into(),
                licence: "GPL-3.0-only".into()
            },
        ]
    );
}

#[test]
fn ac4_altered_bytes_fail_and_leave_no_cache_file() {
    let cache = scratch("ac4-cache");
    let server = TestServer::remote(true);
    let r = check(&fix("ok"), "registry_ok.json", Some(Fetching { cache: &cache, fetcher: &server }));
    assert_eq!(codes(&r), ["digest_mismatch"]);
    match &r.errors[0] {
        CorpError::DigestMismatch { format, entry, expected, actual } => {
            assert_eq!((format.as_str(), entry.as_str()), ("mesh.stl", REMOTE_URL));
            assert_eq!(**expected, digest(REMOTE_SHA));
            assert_ne!(actual, expected);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(files_in(&cache), Vec::<String>::new());
}

#[test]
fn manifest_urls_other_than_http_are_refused() {
    for url in ["file:///etc/passwd", "file:////host/share/x", "https://x.invalid/[1-9]", "https://x.invalid/a b"] {
        let dir = ok_copy_with_url("bad-url", url);
        let cache = dir.join("cache");
        let fetcher = SystemFetcher { max_bytes: 1 << 20 };
        let r = check(&dir.join("corpus"), "registry_ok.json", Some(Fetching { cache: &cache, fetcher: &fetcher }));
        assert_eq!(codes(&r), ["malformed"], "{url}: {:?}", r.errors);
        assert_eq!(files_in(&cache), Vec::<String>::new());
    }
    // The system fetcher refuses them too, even when called directly.
    let cache = scratch("bad-url-direct");
    let sys = SystemFetcher { max_bytes: 1 << 20 };
    let got = fetch_to_cache(&sys, "file:///etc/passwd", &digest(TETRA_SHA), &cache, 1 << 20);
    assert!(matches!(got, Err(FetchError::Failed(_))), "{got:?}");
    assert_eq!(files_in(&cache), Vec::<String>::new());
}

#[cfg(unix)]
#[test]
fn symlinks_are_refused() {
    let dir = scratch("symlink");
    copy_dir(&fix("ok"), &dir);
    // Same bytes, so only the symlink rule can fail it.
    fs::remove_file(dir.join("files/tri.ply")).unwrap();
    std::os::unix::fs::symlink(fix("ok/files/tri.ply"), dir.join("files/tri.ply")).unwrap();
    let r = check(&dir, "registry_ok.json", None);
    assert_eq!(codes(&r), ["io"], "{:?}", r.errors);
    // A symlinked directory on the way is refused as well.
    let dir = scratch("symlink-dir");
    copy_dir(&fix("ok"), &dir);
    fs::remove_dir_all(dir.join("files")).unwrap();
    std::os::unix::fs::symlink(fix("ok/files"), dir.join("files")).unwrap();
    let r = check(&dir, "registry_ok.json", None);
    assert_eq!(codes(&r), ["io", "io"], "{:?}", r.errors);
    // A symlinked manifest is malformed.
    let dir = scratch("symlink-manifest");
    copy_dir(&fix("ok"), &dir);
    fs::remove_file(dir.join("mesh.ply.toml")).unwrap();
    std::os::unix::fs::symlink(fix("ok/mesh.ply.toml"), dir.join("mesh.ply.toml")).unwrap();
    let r = check(&dir, "registry_ok.json", None);
    assert_eq!(codes(&r), ["malformed"], "{:?}", r.errors);
}

#[test]
fn ac4_local_http_server_with_altered_bytes() {
    if !curl_available() {
        return;
    }
    let mut body = fs::read(fix("remote/truncated_binary.stl")).unwrap();
    body[100] ^= 0x80;
    let dir = ok_copy_with_url("ac4-http", &local_http(body));
    let cache = dir.join("cache");
    let fetcher = SystemFetcher { max_bytes: DEFAULT_MAX_BYTES };
    let r = check(&dir.join("corpus"), "registry_ok.json", Some(Fetching { cache: &cache, fetcher: &fetcher }));
    assert_eq!(codes(&r), ["digest_mismatch"]);
    assert_eq!(files_in(&cache), Vec::<String>::new());
}

#[test]
fn ac4_size_cap_and_failed_fetch_leave_no_cache_file() {
    let cache = scratch("ac4-cap");
    let server = TestServer::remote(false);
    let got = fetch_to_cache(&server, REMOTE_URL, &digest(REMOTE_SHA), &cache, 10);
    assert!(matches!(got, Err(FetchError::Failed(ref d)) if d.contains("size cap")), "{got:?}");
    let got = fetch_to_cache(&server, "https://other.invalid/x", &digest(REMOTE_SHA), &cache, 1 << 20);
    assert!(matches!(got, Err(FetchError::Failed(_))), "{got:?}");
    assert_eq!(files_in(&cache), Vec::<String>::new());
}

#[test]
fn corrupt_cache_file_is_replaced() {
    let cache = scratch("corrupt-cache");
    let target = cache_path(&cache, &digest(REMOTE_SHA));
    fs::write(&target, b"stale").unwrap();
    let got = fetch_to_cache(&TestServer::remote(false), REMOTE_URL, &digest(REMOTE_SHA), &cache, 1 << 20);
    assert_eq!(got, Ok((target.clone(), true)));
    assert_eq!(fs::read(&target).unwrap(), fs::read(fix("remote/truncated_binary.stl")).unwrap());
}

#[test]
fn ac5_committed_file_edited_after_digest_was_recorded() {
    let dir = scratch("ac5");
    copy_dir(&fix("ok"), &dir);
    let f = dir.join("files/tetra.stl");
    let mut text = fs::read(&f).unwrap();
    text.extend_from_slice(b"\n");
    fs::write(&f, text).unwrap();
    let r = check(&dir, "registry_ok.json", None);
    assert_eq!(codes(&r), ["digest_mismatch"]);
    match &r.errors[0] {
        CorpError::DigestMismatch { format, entry, expected, .. } => {
            assert_eq!((format.as_str(), entry.as_str()), ("mesh.stl", "files/tetra.stl"));
            assert_eq!(**expected, digest(TETRA_SHA));
        }
        other => panic!("{other:?}"),
    }
    let out = Command::new(env!("CARGO_BIN_EXE_corpus-check"))
        .arg("--corpus")
        .arg(&dir)
        .arg("--formats")
        .arg(fix("registry_ok.json"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("digest_mismatch{mesh.stl: files/tetra.stl}"));
}

#[test]
fn missing_file_duplicate_digest_and_malformed_manifest() {
    let dir = scratch("misc");
    copy_dir(&fix("ok"), &dir);
    fs::remove_file(dir.join("files/tri.ply")).unwrap();
    let r = check(&dir, "registry_ok.json", None);
    assert_eq!(codes(&r), ["io"]);

    // The same digest under a second entry is refused.
    let dir = scratch("dup");
    copy_dir(&fix("ok"), &dir);
    let ply = dir.join("mesh.ply.toml");
    let text = fs::read_to_string(&ply).unwrap();
    let extra = format!(
        "\n[[entry]]\npath = \"files/tetra.stl\"\nsource = \"s\"\nlicence = \"MIT\"\nsha256 = \"{TETRA_SHA}\"\n\
         expect = {{ reject = {{ error = \"format_mismatch\" }} }}\n"
    );
    fs::write(&ply, format!("{text}{extra}")).unwrap();
    let r = check(&dir, "registry_ok.json", None);
    assert_eq!(codes(&r), ["malformed"]);

    // A malformed manifest is reported once, not also as missing.
    fs::write(&ply, "format = \"mesh.ply\"\nbogus = 1\n").unwrap();
    let r = check(&dir, "registry_ok.json", None);
    assert_eq!(codes(&r), ["malformed"], "{:?}", r.errors);
}

#[test]
fn binary_usage_errors_exit_2() {
    let out = Command::new(env!("CARGO_BIN_EXE_corpus-check")).arg("--corpus").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let out = Command::new(env!("CARGO_BIN_EXE_corpus-check"))
        .args(["--corpus", FIX, "--formats"])
        .arg(fix("ok/mesh.stl.toml"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}
