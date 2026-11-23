//! Acceptance criteria of spec node OUTR.DOCS.QA (AC-1 .. AC-6) and edge cases.
//! All fixtures under `fixtures/outr/docs/qa/` are synthetic and written by `make_fixtures.py`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_outreach::link_check::{
    check_external, check_off_origin, select_lint_sources, DocsGatesConfig, ExternalOptions,
};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fix(rel: &str) -> PathBuf {
    repo_root().join("fixtures/outr/docs/qa").join(rel)
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn bin(args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-outreach")).args(args).output().unwrap()
}

fn show(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-link-check-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn parse_json_obj(bytes: &[u8]) -> BTreeMap<String, CValue> {
    let text = std::str::from_utf8(bytes).unwrap().trim();
    match fid_canon::parse(text.as_bytes()).unwrap() {
        CValue::Obj(m) => m,
        other => panic!("expected JSON object, got {other:?}"),
    }
}

fn as_str(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s.as_str(),
        other => panic!("expected string, got {other:?}"),
    }
}

fn as_num(v: &CValue) -> f64 {
    match v {
        CValue::Num(n) => *n,
        other => panic!("expected number, got {other:?}"),
    }
}

fn as_obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("expected object, got {other:?}"),
    }
}

fn as_arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a.as_slice(),
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn ac1_ok_edition_exits_0() {
    let o = bin(&["link-check".into(), "--edition".into(), s(&fix("ok")), "--json".into()]);
    assert_eq!(o.status.code(), Some(0), "{}", show(&o));
    let obj = parse_json_obj(&o.stdout);
    assert_eq!(obj.get("ok"), Some(&CValue::Bool(true)));
    assert_eq!(as_num(obj.get("pages").unwrap()), 2.0);
    assert!(as_arr(obj.get("findings").unwrap()).is_empty());
}

#[test]
fn ac2_broken_reports_missing_target_and_missing_fragment_with_page_and_line() {
    let o = bin(&["link-check".into(), "--edition".into(), s(&fix("broken")), "--json".into()]);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    let obj = parse_json_obj(&o.stdout);
    assert_eq!(obj.get("ok"), Some(&CValue::Bool(false)));
    let findings = as_arr(obj.get("findings").unwrap());
    assert_eq!(findings.len(), 2, "{findings:?}");

    let f0 = as_obj(&findings[0]);
    assert_eq!(as_str(f0.get("page").unwrap()), "book/index.html");
    assert_eq!(as_num(f0.get("line").unwrap()), 10.0);
    assert_eq!(as_str(f0.get("code").unwrap()), "link_target_missing");
    assert_eq!(as_str(f0.get("link").unwrap()), "ghost.html");

    let f1 = as_obj(&findings[1]);
    assert_eq!(as_str(f1.get("page").unwrap()), "book/index.html");
    assert_eq!(as_num(f1.get("line").unwrap()), 11.0);
    assert_eq!(as_str(f1.get("code").unwrap()), "fragment_missing");
    assert_eq!(as_str(f1.get("link").unwrap()), "units.html#nope");
}

/// Spawns a local loopback HTTP/1.1 test server answering:
/// - `/ok` -> 200 OK
/// - `/missing` -> 404 Not Found
/// - `/get-only` -> 405 on HEAD, 200 OK on GET
/// - `/hang` -> never answers until `stop` is signalled
fn start_test_server() -> (u16, Arc<AtomicBool>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_srv = Arc::clone(&stop);

    let handle = thread::spawn(move || {
        let mut workers = Vec::new();
        while !stop_srv.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let stop_conn = Arc::clone(&stop_srv);
                    workers.push(thread::spawn(move || {
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let mut buf = [0u8; 4096];
                        let n = stream.read(&mut buf).unwrap_or(0);
                        let req = String::from_utf8_lossy(&buf[..n]);
                        let first = req.lines().next().unwrap_or("");
                        let mut parts = first.split_whitespace();
                        let method = parts.next().unwrap_or("");
                        let path = parts.next().unwrap_or("/");
                        match (method, path) {
                            (_, "/ok") => {
                                let body = if method == "HEAD" { "" } else { "ok" };
                                let resp = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                    if method == "HEAD" { 2 } else { body.len() }
                                );
                                let _ = stream.write_all(resp.as_bytes());
                            }
                            (_, "/missing") => {
                                let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                                let _ = stream.write_all(resp.as_bytes());
                            }
                            ("HEAD", "/get-only") => {
                                let resp =
                                    "HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                                let _ = stream.write_all(resp.as_bytes());
                            }
                            ("GET", "/get-only") => {
                                let resp = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
                                let _ = stream.write_all(resp.as_bytes());
                            }
                            (_, "/hang") => {
                                while !stop_conn.load(Ordering::Relaxed) {
                                    thread::sleep(Duration::from_millis(50));
                                }
                            }
                            _ => {
                                let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                                let _ = stream.write_all(resp.as_bytes());
                            }
                        }
                        let _ = stream.flush();
                    }));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
        for w in workers {
            let _ = w.join();
        }
    });
    (port, stop, handle)
}

#[test]
fn ac3_external_reports_404_and_timeout_and_writes_issue_md() {
    let (port, stop, server) = start_test_server();
    let root = tmp("ac3");
    let edition = root.join("external");
    let out_dir = root.join("out");
    std::fs::create_dir_all(edition.join("book")).unwrap();

    // Populate from the committed `external` fixture, pointing to the local test server port.
    let template = std::fs::read_to_string(fix("external/book/links.html")).unwrap();
    let html = template.replace("127.0.0.1:1", &format!("127.0.0.1:{port}"));
    std::fs::write(edition.join("book/links.html"), html).unwrap();

    // Run the CLI end-to-end (default 10 s timeout, max 4 concurrent, generic user agent).
    let o = bin(&[
        "link-check".into(),
        "--edition".into(),
        s(&edition),
        "--external".into(),
        "--out".into(),
        s(&out_dir),
        "--json".into(),
    ]);
    stop.store(true, Ordering::Relaxed);
    let _ = server.join();

    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    let obj = parse_json_obj(&o.stdout);
    assert_eq!(obj.get("ok"), Some(&CValue::Bool(false)));
    assert_eq!(as_num(obj.get("checked").unwrap()), 3.0);
    assert_eq!(as_num(obj.get("failed").unwrap()), 2.0);

    let failures = as_arr(obj.get("failures").unwrap());
    assert_eq!(failures.len(), 2, "{failures:?}");

    // Sorted by URL: .../hang before .../missing.
    let hang = as_obj(&failures[0]);
    assert_eq!(as_str(hang.get("url").unwrap()), format!("http://127.0.0.1:{port}/hang"));
    assert_eq!(as_str(hang.get("error").unwrap()), "timeout");
    assert_eq!(hang.get("status"), Some(&CValue::Null));
    assert_eq!(as_arr(hang.get("pages").unwrap()), &[CValue::Str("book/links.html".into())]);

    let missing = as_obj(&failures[1]);
    assert_eq!(as_str(missing.get("url").unwrap()), format!("http://127.0.0.1:{port}/missing"));
    assert_eq!(as_num(missing.get("status").unwrap()), 404.0);
    assert_eq!(as_arr(missing.get("pages").unwrap()), &[CValue::Str("book/links.html".into())]);

    // `external.json` and `issue.md` are written and contain both failures and linking pages.
    let ext_json = std::fs::read(out_dir.join("external.json")).unwrap();
    assert_eq!(parse_json_obj(&ext_json), obj);

    let issue_md = std::fs::read_to_string(out_dir.join("issue.md")).unwrap();
    assert!(issue_md.contains(&format!("http://127.0.0.1:{port}/missing")), "{issue_md}");
    assert!(issue_md.contains("HTTP 404"), "{issue_md}");
    assert!(issue_md.contains(&format!("http://127.0.0.1:{port}/hang")), "{issue_md}");
    assert!(issue_md.contains("timeout"), "{issue_md}");
    assert!(issue_md.contains("book/links.html"), "{issue_md}");
}

#[test]
fn external_head_falls_back_to_get_on_405() {
    let (port, stop, server) = start_test_server();
    let root = tmp("get-fallback");
    let edition = root.join("ed");
    std::fs::create_dir_all(edition.join("book")).unwrap();
    std::fs::write(
        edition.join("book/index.html"),
        format!(
            "<!doctype html><html><body><a href=\"http://127.0.0.1:{port}/ok\">ok</a>\
             <a href=\"http://127.0.0.1:{port}/get-only\">get</a></body></html>"
        ),
    )
    .unwrap();

    let opts = ExternalOptions { timeout: Duration::from_secs(3), ..ExternalOptions::default() };
    let report = check_external(&edition, &opts).unwrap();
    stop.store(true, Ordering::Relaxed);
    let _ = server.join();

    assert_eq!(report.exit_code(), 0, "{}", report.render());
    assert_eq!(report.results.len(), 2);
    assert!(report.results.iter().all(|r| r.ok && r.status == Some(200)));
    assert!(report.to_issue_md().contains("can be closed"));
}

#[test]
fn ac4_tracker_fails_off_origin_gate_with_docs_gates_toml() {
    let gates = DocsGatesConfig::load(&repo_root().join("docs/qa/docs-gates.toml")).unwrap();
    assert_eq!(gates.origin, "https://docs.metrologymaven.io");
    assert_eq!(gates.a11y_level, "WCAG22AA");
    assert_eq!(gates.size_budget_kb, 500);

    // Clean edition has zero off-origin requests.
    let ok_findings = check_off_origin(&fix("ok"), &gates.origin).unwrap();
    assert!(ok_findings.is_empty(), "{ok_findings:?}");

    // Tracker edition fails naming the page and third-party script URL.
    let findings = check_off_origin(&fix("tracker"), &gates.origin).unwrap();
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].page, "book/index.html");
    assert_eq!(findings[0].element, "script");
    assert_eq!(findings[0].url, "https://tracker.example/analytics.js");
}

#[test]
fn ac5_claim_lint_runs_only_on_hand_written_sources_not_ref_pages() {
    let gates = DocsGatesConfig::load(&repo_root().join("docs/qa/docs-gates.toml")).unwrap();
    let selected = select_lint_sources(&fix("lint"), &gates).unwrap();
    assert_eq!(selected.len(), 1, "only hand-written book source must be selected: {selected:?}");
    assert!(selected[0].ends_with("book/intro.md"));

    // Read the real OUTR.BRAND.LINT rule file (`claim-rules.toml`) and verify that CL001 matches
    // the selected hand-written book source, while the generated reference page under `ref/`
    // (which also contains "certified") is excluded by `docs-gates.toml`.
    let rules_text = std::fs::read_to_string(repo_root().join("crates/fid-brand-lint/rules/claim-rules.toml")).unwrap();
    let rules_tbl: toml::Value = toml::from_str(&rules_text).unwrap();
    let cl001_words: Vec<String> = rules_tbl
        .get("rule")
        .and_then(toml::Value::as_array)
        .unwrap()
        .iter()
        .find(|r| r.get("id").and_then(toml::Value::as_str) == Some("CL001"))
        .unwrap()
        .get("words")
        .and_then(toml::Value::as_array)
        .unwrap()
        .iter()
        .filter_map(toml::Value::as_str)
        .map(str::to_ascii_lowercase)
        .collect();
    assert!(cl001_words.contains(&"certified".to_string()));

    let mut errors: Vec<(String, &'static str)> = Vec::new();
    for path in &selected {
        let text = std::fs::read_to_string(path).unwrap().to_ascii_lowercase();
        if cl001_words.iter().any(|w| text.split(|c: char| !c.is_ascii_alphanumeric()).any(|tok| tok == w)) {
            errors.push((s(path), "CL001"));
        }
    }
    assert_eq!(errors.len(), 1, "expected exactly one CL001 finding, got {errors:?}");
    assert!(errors[0].0.replace('\\', "/").ends_with("book/intro.md"));
    assert_eq!(errors[0].1, "CL001");

    // Confirm the excluded ref page really does contain "certified" so exclusion is meaningful.
    let ref_text = std::fs::read_to_string(fix("lint/ref/cli/demo.read_item@1.html")).unwrap();
    assert!(ref_text.contains("certified"));
}

#[test]
fn ac6_internal_check_reports_are_byte_identical() {
    let a = bin(&["link-check".into(), "--edition".into(), s(&fix("broken")), "--json".into()]);
    let b = bin(&["link-check".into(), "--edition".into(), s(&fix("broken")), "--json".into()]);
    assert_eq!(a.status.code(), Some(1));
    assert_eq!(b.status.code(), Some(1));
    assert_eq!(a.stdout, b.stdout);
}

#[test]
fn link_escapes_edition_and_error_paths() {
    let root = tmp("escape");
    let edition = root.join("ed");
    std::fs::create_dir_all(edition.join("book")).unwrap();
    std::fs::write(
        edition.join("book/index.html"),
        "<!doctype html><html><body><a href=\"../../secret.html\">escape</a></body></html>",
    )
    .unwrap();

    let o = bin(&["link-check".into(), "--edition".into(), s(&edition), "--json".into()]);
    assert_eq!(o.status.code(), Some(1), "{}", show(&o));
    let obj = parse_json_obj(&o.stdout);
    let findings = as_arr(obj.get("findings").unwrap());
    assert_eq!(findings.len(), 1);
    assert_eq!(as_str(as_obj(&findings[0]).get("code").unwrap()), "link_escapes_edition");

    // Missing edition exits 2.
    let missing = bin(&["link-check".into(), "--edition".into(), s(&root.join("nope")), "--json".into()]);
    assert_eq!(missing.status.code(), Some(2));
    assert_eq!(parse_json_obj(&missing.stdout).get("ok"), Some(&CValue::Bool(false)));

    // `--out` without `--external` is bad usage (exit 2).
    let bad = bin(&["link-check".into(), "--edition".into(), s(&fix("ok")), "--out".into(), s(&root.join("out"))]);
    assert_eq!(bad.status.code(), Some(2));
}
