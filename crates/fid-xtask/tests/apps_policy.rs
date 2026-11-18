//! Acceptance criteria of spec node ECO.APPS.POLICY (AC-1 .. AC-7). The fixture workspaces
//! under `fixtures/policy/` and their `expected.txt` files are written by
//! `fixtures/policy/make_fixtures.py`, with findings labelled by hand from the spec rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_xtask::apps_policy::{
    code_only, parse_apps_status, parse_registry_manifest, token_lines, without_cfg_test, Config,
    DEFAULT_IDENTITY_TOKENS,
};
use std::path::{Path, PathBuf};
use std::process::Command;

const FIXTURES: [&str; 7] = ["ok", "defn", "leak", "ident", "size", "size-long", "input"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    root().join("fixtures/policy").join(name)
}

fn expected(name: &str) -> Vec<String> {
    std::fs::read_to_string(fixture(name).join("expected.txt")).unwrap().lines().map(String::from).collect()
}

/// Runs `cargo xtask apps-policy` (the built binary) on a fixture: (exit code, stdout).
fn cli_with(name: &str, extra: &[&str]) -> (i32, Vec<u8>) {
    let ws = fixture(name);
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
        .arg("apps-policy")
        .arg("--workspace")
        .arg(&ws)
        .arg("--registry-manifests")
        .arg(ws.join("manifests"))
        .arg("--apps-status")
        .arg(ws.join("apps-status.json"))
        .args(extra)
        .output()
        .unwrap();
    (out.status.code().unwrap(), out.stdout)
}

fn cli(name: &str) -> (i32, Vec<u8>) {
    cli_with(name, &[])
}

fn lines(out: &[u8]) -> Vec<String> {
    String::from_utf8(out.to_vec()).unwrap().lines().map(String::from).collect()
}

/// Every fixture's output matches its hand-labelled expected.txt, with exit 0 iff empty.
#[test]
fn fixtures_match_expected() {
    for name in FIXTURES {
        let want = expected(name);
        let (code, out) = cli(name);
        if want.is_empty() {
            assert_eq!((code, lines(&out)), (0, vec!["apps-policy: ok".to_string()]), "{name}");
        } else {
            assert_eq!((code, lines(&out)), (1, want), "{name}");
        }
    }
}

/// AC-1
#[test]
fn ac1_ok_exits_0() {
    let (code, out) = cli("ok");
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&out));
}

/// AC-2
#[test]
fn ac2_app_crate_declaring_a_command() {
    let (code, out) = cli("defn");
    assert_eq!(code, 1);
    let l = lines(&out);
    assert!(l.iter().all(|x| x.starts_with("POLICY-DEFN fid-app-a ")), "{l:?}");
    assert!(l.iter().any(|x| x.contains("fid-registry-macros")), "{l:?}");
    assert!(l.iter().any(|x| x.starts_with("POLICY-DEFN fid-app-a src/main.rs:8")), "{l:?}");
}

/// AC-3
#[test]
fn ac3_leaks_name_file_line_and_dependency() {
    let (code, out) = cli("leak");
    assert_eq!(code, 1);
    let l = lines(&out);
    assert!(l.iter().any(|x| x.starts_with("POLICY-LEAK fid-x src/lib.rs:12")), "{l:?}");
    assert!(l.iter().any(|x| x.starts_with("POLICY-LEAK fid-y ") && x.ends_with("fid-app-a")), "{l:?}");
    // Comments, strings, cfg(test) items, test files, dev-dependencies and tool crates do not count.
    assert_eq!(l.len(), 2, "{l:?}");
}

/// AC-4
#[test]
fn ac4_one_ident_finding_listing_both_apps() {
    let (code, out) = cli("ident");
    assert_eq!(code, 1);
    let l = lines(&out);
    let ident: Vec<&String> = l.iter().filter(|x| x.starts_with("POLICY-IDENT ")).collect();
    assert_eq!(ident.len(), 1, "{l:?}");
    let f = ident[0];
    assert!(f.starts_with("POLICY-IDENT x.do_thing@1"), "{f}");
    assert!(f.contains(&format!("a=sha256:{}", "a".repeat(64))), "{f}");
    assert!(f.contains(&format!("b=sha256:{}", "b".repeat(64))), "{f}");
}

/// AC-5
#[test]
fn ac5_extra_file_and_its_exception() {
    let (code, out) = cli("size");
    assert_eq!(code, 1);
    assert!(lines(&out).iter().any(|x| x.starts_with("POLICY-SIZE fid-app-a src/fit.rs")));
    let cfg = fixture("size").join("apps-policy-excepted.toml");
    let (code, out) = cli_with("size", &["--config", cfg.to_str().unwrap()]);
    assert_eq!((code, lines(&out)), (0, vec!["apps-policy: ok".to_string()]));
}

/// AC-5 (REQ-004): a 61-line main.rs is a finding, a 60-line one is not.
#[test]
fn ac5_main_rs_line_limit() {
    let (code, out) = cli("size-long");
    assert_eq!(code, 1);
    assert_eq!(lines(&out), ["POLICY-SIZE fid-app-a src/main.rs: 61 lines, limit 60"]);
}

/// AC-6
#[test]
fn ac6_checklist_exists_and_template_links_it() {
    let checklist = std::fs::read_to_string(root().join("docs/review/app-crate-checklist.md")).unwrap();
    for topic in ["numeric logic", "configuration", "copied"] {
        assert!(checklist.to_lowercase().contains(topic), "checklist misses {topic}");
    }
    let template = std::fs::read_to_string(root().join(".github/PULL_REQUEST_TEMPLATE.md")).unwrap();
    assert!(template.contains("docs/review/app-crate-checklist.md"));
}

/// AC-7, and REQ-006: two runs give byte-identical output.
#[test]
fn ac7_runs_are_byte_identical() {
    let first = cli("leak");
    let second = cli("leak");
    assert_eq!(first, second);
}

/// REQ-006: bad usage exits 2.
#[test]
fn usage_error_exits_2() {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask")).args(["apps-policy", "--workspace"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let ws = fixture("ok");
    let out =
        Command::new(env!("CARGO_BIN_EXE_fid-xtask")).arg("apps-policy").arg("--workspace").arg(&ws).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

/// A missing apps-status file is a POLICY-INPUT finding with exit 1.
#[test]
fn missing_apps_status_is_input_finding() {
    let ws = fixture("ok");
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
        .arg("apps-policy")
        .arg("--workspace")
        .arg(&ws)
        .arg("--registry-manifests")
        .arg(ws.join("manifests"))
        .arg("--apps-status")
        .arg(ws.join("no-such.json"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(lines(&out.stdout).iter().any(|l| l.starts_with("POLICY-INPUT apps-status.json: ")));
}

#[test]
fn lexer_blanks_comments_strings_and_chars() {
    let src = "a // x\nb /* y /* z */ w */ c\n\"s\\\"t\" r#\"u\"v\"# '\"' 'q' '\\'' &'static d\n";
    let code = code_only(src);
    assert_eq!(code.lines().count(), 3);
    let words: Vec<&str> = code.split_whitespace().collect();
    assert_eq!(words, ["a", "b", "c", "&'static", "d"]);
}

#[test]
fn lexer_keeps_raw_identifiers_and_lifetimes() {
    let code = code_only("let r#type = 1; fn f<'a>(x: &'a str) {}\n");
    assert!(code.contains("r#type"));
    assert!(code.contains("&'a str"));
}

#[test]
fn cfg_test_items_are_blanked() {
    let src = concat!(
        "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn b() { current_app() }\n}\n",
        "#[cfg(test)]\nuse x::current_app;\nfn c() { current_app() }\n",
    );
    let code = without_cfg_test(&code_only(src));
    assert_eq!(token_lines(&code, "current_app"), [8]);
}

#[test]
fn tokens_respect_identifier_boundaries() {
    let code = "current_app()\nrecurrent_app\ncurrent_app_name\nAppId::from(x)\nMyAppId::from\nx.FID_APP\n";
    assert_eq!(token_lines(code, "current_app"), [1]);
    assert_eq!(token_lines(code, "AppId::from"), [4]);
    assert_eq!(token_lines(code, "FID_APP"), [6]);
    assert_eq!(token_lines(code, "#[fid_command"), Vec::<usize>::new());
}

#[test]
fn config_defaults_and_validation() {
    let d = Config::default();
    assert_eq!(d.identity_tokens.len(), DEFAULT_IDENTITY_TOKENS.len());
    assert!(d.macro_crates.is_empty());
    let c = Config::parse("macro_crates = [\"m\"]\n").unwrap();
    assert_eq!(c.identity_tokens, d.identity_tokens);
    assert!(Config::parse("bogus = 1\n").is_err());
    assert!(Config::parse("[[exception]]\ncrate = \"a\"\nfile = \"src/x.rs\"\nreason = \"r\"\n").is_err());
    assert!(Config::parse("identity_tokens = [\"\"]\n").is_err());
}

#[test]
fn manifests_use_shared_types() {
    let d = format!("sha256:{}", "a".repeat(64));
    let ok = format!("{{\"commands\":[{{\"id\":\"x.y@1\",\"definition_digest\":\"{d}\",\"side_effect\":\"W2\"}}]}}");
    assert_eq!(parse_registry_manifest(&ok, "m").unwrap().len(), 1);
    // Side-effect classes are exactly those of CONSTITUTION §3.
    let bad_class = ok.replace("W2", "W9");
    assert!(parse_registry_manifest(&bad_class, "m").is_err());
    let bad_id = ok.replace("x.y@1", "X.Y");
    assert!(parse_registry_manifest(&bad_id, "m").is_err());
    let bad_digest = ok.replace(&d, "aa");
    assert!(parse_registry_manifest(&bad_digest, "m").is_err());
    let status = r#"{"apps":[{"id":"a","manifest":true,"commands":[{"id":"x.y@1"}]},{"id":"b","manifest":false}]}"#;
    let eff = parse_apps_status(status).unwrap();
    assert_eq!(eff.keys().collect::<Vec<_>>(), ["a"]);
}
