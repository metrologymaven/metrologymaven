//! Acceptance criteria of spec node OUTR.SITE.GEN.IMPORT (AC-1 .. AC-8) and error paths.
//! Fixtures and every expected output come from `fixtures/outr/site/import/make_fixtures.py`
//! (an independent Python reference), not from this crate. Fetch tests use a loopback HTTP
//! server started here; no test touches the network.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_outreach::site::import::fetch::parse_base_url;
use fid_outreach::site::import::{is_date, verify_and_stage, VerifyArgs};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

fn fix(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/outr/site/import").join(rel)
}

fn read(p: &Path) -> Vec<u8> {
    std::fs::read(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

static N: AtomicU32 = AtomicU32::new(0);

/// A fresh, empty temporary folder.
fn tmp(tag: &str) -> PathBuf {
    let n = N.fetch_add(1, Ordering::SeqCst);
    let p = std::env::temp_dir().join(format!("fid-site-import-{}-{tag}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), &to).unwrap();
        }
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-outreach")).args(args).output().unwrap()
}

fn verify(site: &Path, stage: &Path) -> Output {
    let (site, stage) = (site.to_str().unwrap(), stage.to_str().unwrap());
    run(&[
        "site",
        "import",
        "verify",
        "--site",
        site,
        "--stage",
        stage,
        "--revision",
        "abc123",
        "--revision-date",
        "2026-11-17",
    ])
}

fn fetch(site: &Path, base: &str) -> Output {
    run(&["site", "import", "fetch", "--site", site.to_str().unwrap(), "--release", "v0.2.0", "--base-url", base])
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// Serves `fixtures/.../remote` under `/good/` and `remote-bad` under `/bad/` on 127.0.0.1.
fn serve() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut req = Vec::new();
            let mut buf = [0u8; 1024];
            while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                match s.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => req.extend_from_slice(&buf[..n]),
                }
            }
            let line = String::from_utf8_lossy(&req).lines().next().unwrap_or("").to_string();
            let path = line.split(' ').nth(1).unwrap_or("/");
            if path.starts_with("/big/") {
                // No Content-Length: the body ends when the connection closes. 5 MiB of JSON
                // whitespace, over the 4 MiB manifest cap.
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
                let chunk = vec![b' '; 64 * 1024];
                for _ in 0..80 {
                    if s.write_all(&chunk).is_err() {
                        break;
                    }
                }
                continue;
            }
            let file = if let Some(r) = path.strip_prefix("/good/") {
                Some(fix("remote").join(r))
            } else {
                path.strip_prefix("/bad/").map(|r| fix("remote-bad").join(r))
            };
            let resp = match file.filter(|f| !path.contains("..") && f.is_file()) {
                Some(f) => {
                    let body = std::fs::read(f).unwrap();
                    let mut r =
                        format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len())
                            .into_bytes();
                    r.extend(body);
                    r
                }
                None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            };
            let _ = s.write_all(&resp);
        }
    });
    port
}

/// A copy of fixture `ok` with the fetch host allowlist.
fn work_site() -> PathBuf {
    let work = tmp("work").join("work");
    copy_tree(&fix("ok"), &work);
    std::fs::copy(fix("fetch/hosts.toml"), work.join("hosts.toml")).unwrap();
    work
}

fn files_under(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

#[test]
fn ac1_ok_verifies_and_writes_the_expected_build_manifest() {
    let stage = tmp("ac1");
    let o = verify(&fix("ok"), &stage);
    assert_eq!(o.status.code(), Some(0), "{}{}", stdout(&o), String::from_utf8_lossy(&o.stderr));
    assert_eq!(read(&stage.join("build-manifest.json")), read(&fix("expected/build-manifest.json")));
    // Every input is staged unchanged under data/<path>.
    let lock = String::from_utf8(read(&fix("ok/inputs.lock.toml"))).unwrap();
    let paths: Vec<&str> = lock.lines().filter_map(|l| l.strip_prefix("path = \"")?.strip_suffix('"')).collect();
    assert_eq!(paths.len(), 13);
    for p in paths {
        let rel: PathBuf = p.split('/').collect();
        assert_eq!(read(&stage.join("data").join(&rel)), read(&fix("ok").join(&rel)), "{p}");
    }
}

fn expect_failure(fixture: &str) {
    let stage = tmp(fixture);
    let o = verify(&fix(fixture), &stage);
    assert_eq!(o.status.code(), Some(1), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(stdout(&o), String::from_utf8(read(&fix(&format!("expected/{fixture}.out")))).unwrap());
    assert!(!stage.join("build-manifest.json").exists());
    assert!(!stage.join("data").exists());
}

#[test]
fn ac2_tampered_file_fails_with_both_digests() {
    expect_failure("tampered");
}

#[test]
fn ac3_asset_disagreeing_with_its_release_manifest_fails() {
    expect_failure("wrong-release");
}

#[test]
fn ac4_unlisted_file_fails() {
    expect_failure("stray");
}

#[test]
fn ac5_fetch_from_loopback_adds_five_files_and_entries() {
    let port = serve();
    let work = work_site();
    let o = fetch(&work, &format!("http://127.0.0.1:{port}/good/v0.2.0/"));
    assert_eq!(o.status.code(), Some(0), "{}{}", stdout(&o), String::from_utf8_lossy(&o.stderr));
    let dir = work.join("inputs").join("v0.2.0");
    let names = files_under(&dir);
    assert_eq!(names, ["parity.json", "parity.md", "release-manifest.json", "status.json", "status.md"]);
    for n in &names {
        assert_eq!(read(&dir.join(n)), read(&fix("remote/v0.2.0").join(n)), "{n}");
    }
    assert_eq!(read(&work.join("inputs.lock.toml")), read(&fix("expected/fetch-lock.toml")));
    assert!(!work.join(".inputs-fetch.tmp").exists());
    // The fetched site verifies.
    let o = verify(&work, &tmp("ac5-stage"));
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));
}

#[test]
fn ac5_fetch_with_a_bad_digest_writes_nothing() {
    let port = serve();
    let work = work_site();
    let before = read(&work.join("inputs.lock.toml"));
    let o = fetch(&work, &format!("http://127.0.0.1:{port}/bad/v0.2.0/"));
    assert_eq!(o.status.code(), Some(1), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(stdout(&o).starts_with("IMPORT-DIGEST parity.md sha256:"), "{}", stdout(&o));
    assert_eq!(read(&work.join("inputs.lock.toml")), before);
    assert!(!work.join("inputs").join("v0.2.0").exists());
    assert!(!work.join(".inputs-fetch.tmp").exists());
}

#[test]
fn fetch_enforces_the_size_cap_without_content_length() {
    let port = serve();
    let work = work_site();
    let before = read(&work.join("inputs.lock.toml"));
    let base = format!("http://127.0.0.1:{port}/big/v0.2.0/");
    let o = fetch(&work, &base);
    assert_eq!(o.status.code(), Some(1), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(stdout(&o), format!("IMPORT-FETCH {base}release-manifest.json too-large\n"));
    assert_eq!(read(&work.join("inputs.lock.toml")), before);
    assert!(!work.join("inputs").join("v0.2.0").exists());
    assert!(!work.join(".inputs-fetch.tmp").exists());
}

#[test]
fn fetch_into_an_existing_release_folder_replaces_its_files() {
    let port = serve();
    let work = work_site();
    let base = format!("http://127.0.0.1:{port}/good/v0.2.0/");
    assert_eq!(fetch(&work, &base).status.code(), Some(0));
    // Damage one file; a second fetch replaces it in place and leaves no backups behind.
    std::fs::write(work.join("inputs/v0.2.0/parity.md"), b"stale").unwrap();
    let o = fetch(&work, &base);
    assert_eq!(o.status.code(), Some(0), "{}{}", stdout(&o), String::from_utf8_lossy(&o.stderr));
    assert_eq!(read(&work.join("inputs/v0.2.0/parity.md")), read(&fix("remote/v0.2.0/parity.md")));
    assert_eq!(read(&work.join("inputs.lock.toml")), read(&fix("expected/fetch-lock.toml")));
    for scratch in [".inputs-fetch.tmp", ".inputs-fetch.bak", ".inputs.lock.toml.tmp"] {
        assert!(!work.join(scratch).exists(), "{scratch}");
    }
    assert_eq!(verify(&work, &tmp("refetch-stage")).status.code(), Some(0));
}

#[cfg(unix)]
#[test]
fn fetch_refuses_a_symlinked_release_folder() {
    let port = serve();
    let work = work_site();
    let before = read(&work.join("inputs.lock.toml"));
    let elsewhere = tmp("elsewhere");
    std::os::unix::fs::symlink(&elsewhere, work.join("inputs/v0.2.0")).unwrap();
    let o = fetch(&work, &format!("http://127.0.0.1:{port}/good/v0.2.0/"));
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(read(&work.join("inputs.lock.toml")), before);
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
    assert!(!work.join(".inputs.lock.toml.tmp").exists());
}

#[test]
fn fetch_of_a_missing_release_reports_the_status() {
    let port = serve();
    let work = work_site();
    let before = read(&work.join("inputs.lock.toml"));
    let base = format!("http://127.0.0.1:{port}/good/v9.9.9/");
    let o = fetch(&work, &base);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stdout(&o), format!("IMPORT-FETCH {base}release-manifest.json 404\n"));
    assert_eq!(read(&work.join("inputs.lock.toml")), before);
}

#[test]
fn ac6_host_not_in_allowlist_is_refused_before_any_request() {
    let work = work_site();
    let before = read(&work.join("inputs.lock.toml"));
    let o = fetch(&work, "https://example.org/v0.2.0/");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stdout(&o), "IMPORT-HOST example.org\n");
    assert_eq!(read(&work.join("inputs.lock.toml")), before);
    assert!(!work.join("inputs").join("v0.2.0").exists());
}

#[test]
fn plain_http_is_refused_for_a_non_loopback_host() {
    // Listed (case-insensitively) in hosts.toml, but plain http is only for loopback.
    let work = work_site();
    let o = fetch(&work, "http://releases.metrologymaven.EXAMPLE/v0.2.0/");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stdout(&o), "IMPORT-FETCH http://releases.metrologymaven.EXAMPLE/v0.2.0/ http-not-loopback\n");
}

#[test]
fn ac7_verify_has_no_network_code_and_runs_in_process() {
    // The verify module holds no network or process code (fetch lives in its own file); CI also
    // runs verify on fixture `ok` with the network cut (rust.yml, Linux).
    let src =
        String::from_utf8(read(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/site/import/mod.rs"))).unwrap();
    for banned in ["std::net", "TcpStream", "UdpSocket", "Command", "curl"] {
        assert!(!src.contains(banned), "verify module mentions {banned}");
    }
    let stage = tmp("ac7");
    let args = VerifyArgs { site: &fix("ok"), stage: &stage, revision: "abc123", revision_date: "2026-11-17" };
    assert_eq!(verify_and_stage(&args).unwrap(), vec![]);
}

#[test]
fn ac8_two_runs_give_identical_bytes() {
    let (a, b) = (tmp("ac8a"), tmp("ac8b"));
    assert_eq!(verify(&fix("ok"), &a).status.code(), Some(0));
    assert_eq!(verify(&fix("ok"), &b).status.code(), Some(0));
    // Re-running into a used stage replaces it.
    assert_eq!(verify(&fix("ok"), &a).status.code(), Some(0));
    let ma = read(&a.join("build-manifest.json"));
    assert_eq!(ma, read(&b.join("build-manifest.json")));
    // The expected file is shared by the Linux, Windows and macOS CI runs.
    assert_eq!(ma, read(&fix("expected/build-manifest.json")));
}

/// A copy of `ok` whose lock text is edited by `f`.
fn edited(tag: &str, f: impl Fn(String) -> String) -> PathBuf {
    let site = tmp(tag).join("site");
    copy_tree(&fix("ok"), &site);
    let lock = site.join("inputs.lock.toml");
    let text = String::from_utf8(read(&lock)).unwrap();
    std::fs::write(&lock, f(text)).unwrap();
    site
}

#[test]
fn unknown_kind_is_rejected() {
    let site = edited("kind", |t| t.replacen("kind = \"tokens\"", "kind = \"palette\"", 1));
    let o = verify(&site, &tmp("kind-stage"));
    assert_eq!(o.status.code(), Some(1));
    // The entry is dropped, so its file is also unlisted.
    assert_eq!(stdout(&o), "IMPORT-UNLISTED inputs/tokens/tokens.json\nIMPORT-KIND palette\n");
}

#[test]
fn entry_without_release_or_source_rev_does_not_parse() {
    let site = edited("nosource", |t| t.replacen("source_rev = \"4444444444444444444444444444444444444444\"\n", "", 1));
    let o = verify(&site, &tmp("nosource-stage"));
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).starts_with("INPUT-PARSE "));
}

#[test]
fn missing_file_is_named() {
    let site = edited("missing", |t| t);
    std::fs::remove_file(site.join("inputs/apps/apps.toml")).unwrap();
    let o = verify(&site, &tmp("missing-stage"));
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stdout(&o), "IMPORT-MISSING apps.toml\n");
}

#[test]
fn bad_usage_exits_2() {
    assert_eq!(run(&["site", "import", "verify", "--site", "x"]).status.code(), Some(2));
    assert_eq!(run(&["site", "import", "nope"]).status.code(), Some(2));
    let stage = tmp("baddate");
    let o = run(&[
        "site",
        "import",
        "verify",
        "--site",
        fix("ok").to_str().unwrap(),
        "--stage",
        stage.to_str().unwrap(),
        "--revision",
        "abc123",
        "--revision-date",
        "2026-02-30",
    ]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn base_url_and_date_rules() {
    let u = parse_base_url("https://Releases.Example.org/v0.2.0").unwrap();
    assert!(u.https);
    assert_eq!(u.host, "releases.example.org");
    assert_eq!(u.prefix, "https://Releases.Example.org/v0.2.0/");
    assert_eq!(parse_base_url("http://127.0.0.1:8080/x/").unwrap().host, "127.0.0.1");
    assert_eq!(parse_base_url("http://[::1]:8080/x/").unwrap().host, "::1");
    for bad in [
        "ftp://h/x",
        "https://user@h/x",
        "https://h/x?y",
        "https://h:99999/",
        "https:///x",
        "https://h/[x]",
        "file:///c",
    ] {
        assert!(parse_base_url(bad).is_none(), "{bad}");
    }
    assert!(is_date("2028-02-29") && !is_date("2027-02-29") && !is_date("2026-13-01") && !is_date("2026-1-01"));
}
