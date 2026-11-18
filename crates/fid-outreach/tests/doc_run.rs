//! Acceptance criteria AC-1 .. AC-5 of spec node OUTR.DOCS.CONTRIB (`fid-outreach doc-run`),
//! plus parser and error paths. AC-6 (claim lint) runs as a CI step, because a tool crate may
//! not depend on another tool crate (`cargo xtask layers`).
//!
//! The fixtures under `fixtures/outr/contrib/` are hand-written and the expected lines and
//! outcomes below are hand-labelled from the spec's rules (see that folder's README), never
//! produced by running the checker. A guide can never state the ID of the commit that adds it,
//! so the tests copy a guide to a temporary file with its revision set to `git rev-parse HEAD`
//! ("given the guide stating revision R") before running the checker on this repository.
//!
//! Tests that run `python tools/scope.py` are skipped, with a note on stderr, when no `python`
//! is on PATH; the Linux CI job also runs the real guide after `setup-python`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_outreach::doc_run::{parse_guide, run, split_command, DocRunError, Options, Status};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn head() -> String {
    let out = Command::new("git").arg("-C").arg(root()).args(["rev-parse", "HEAD"]).output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn porcelain() -> String {
    let out = Command::new("git").arg("-C").arg(root()).args(["status", "--porcelain"]).output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

fn python_available(test: &str) -> bool {
    let ok = Command::new("python").arg("--version").output().map(|o| o.status.success()).unwrap_or(false);
    if !ok {
        eprintln!("{test}: skipped, no `python` on PATH");
    }
    ok
}

/// A private temporary folder for one test.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fid-doc-run-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Copies `text` to `<scratch>/guide.md` with the front matter `revision:` set to `rev`.
fn guide_with_revision(name: &str, text: &str, rev: &str) -> PathBuf {
    let mut done = false;
    let body: Vec<String> = text
        .lines()
        .map(|l| {
            if !done && l.starts_with("revision:") {
                done = true;
                format!("revision: {rev}")
            } else {
                l.to_string()
            }
        })
        .collect();
    assert!(done, "{name}: no revision line");
    let p = scratch(name).join("guide.md");
    std::fs::write(&p, body.join("\n") + "\n").unwrap();
    p
}

fn bin(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-outreach")).arg("doc-run").args(args).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("expected object, got {other:?}"),
    }
}

fn arr<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> &'a Vec<CValue> {
    match o.get(k) {
        Some(CValue::Arr(a)) => a,
        other => panic!("{k}: expected array, got {other:?}"),
    }
}

fn st(o: &BTreeMap<String, CValue>, k: &str) -> String {
    match o.get(k) {
        Some(CValue::Str(s)) => s.clone(),
        other => panic!("{k}: expected string, got {other:?}"),
    }
}

fn num(o: &BTreeMap<String, CValue>, k: &str) -> u32 {
    match o.get(k) {
        Some(CValue::Num(n)) => *n as u32,
        other => panic!("{k}: expected number, got {other:?}"),
    }
}

fn json(stdout: &str) -> BTreeMap<String, CValue> {
    obj(&parse(stdout.trim().as_bytes()).unwrap_or_else(|e| panic!("report JSON {e:?}: {stdout}"))).clone()
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn real_guide() -> String {
    std::fs::read_to_string(root().join("docs/contributor/guide.md")).unwrap()
}

// ------------------------------------------------------------------ AC-1 and AC-3

#[test]
fn ac1_ac3_real_guide_passes_and_leaves_the_repository_unchanged() {
    if !python_available("ac1_ac3") {
        return;
    }
    let guide = guide_with_revision("ac1", &real_guide(), &head());
    let before = porcelain();
    let (code, stdout, stderr) = bin(&["--guide", &s(&guide), "--repo", &s(&root()), "--json"]);
    let after = porcelain();
    assert_eq!(code, 0, "stdout: {stdout}\nstderr: {stderr}");
    let r = json(&stdout);
    assert_eq!(r.get("ok"), Some(&CValue::Bool(true)));
    assert_eq!(r.get("error"), Some(&CValue::Null));
    let commands: Vec<String> = arr(&r, "blocks")
        .iter()
        .map(|b| {
            let b = obj(b);
            assert_eq!(st(b, "status"), "passed", "{b:?}");
            st(b, "command")
        })
        .collect();
    for want in ["python tools/scope.py open", "python tools/scope.py check", "python tools/scope.py stats"] {
        assert!(commands.iter().any(|c| c == want), "missing `{want}` in {commands:?}");
    }
    // AC-3: the temporary copy is gone and the starting repository is unchanged.
    assert_eq!(r.get("workdir_removed"), Some(&CValue::Bool(true)));
    assert!(!Path::new(&st(&r, "workdir")).exists());
    assert_eq!(before, after, "git status --porcelain changed");
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_broken_command_fails_naming_file_and_line() {
    if !python_available("ac2") {
        return;
    }
    let src = std::fs::read_to_string(root().join("fixtures/outr/contrib/broken-cmd/guide.md")).unwrap();
    let guide = guide_with_revision("ac2", &src, &head());
    let g = s(&guide);
    let (code, stdout, stderr) = bin(&["--guide", &g, "--repo", &s(&root())]);
    assert_eq!(code, 1, "stdout: {stdout}\nstderr: {stderr}");
    // Hand-labelled: the expect-fail block opens on line 10, the failing block on line 16 and
    // its command is on line 17.
    assert!(stdout.contains(&format!("{g}:10: passed")), "{stdout}");
    assert!(stdout.contains(&format!("{g}:16: failed (line 17: `python tools/scope.py no-such-verb`")), "{stdout}");
    assert!(stdout.contains("doc-run: 2 block(s), 1 failed"), "{stdout}");

    let (code, stdout, _) = bin(&["--guide", &g, "--repo", &s(&root()), "--json"]);
    assert_eq!(code, 1);
    let r = json(&stdout);
    assert_eq!(st(&r, "error"), "block_failed");
    let blocks = arr(&r, "blocks");
    assert_eq!(blocks.len(), 2);
    let (b0, b1) = (obj(&blocks[0]), obj(&blocks[1]));
    assert_eq!((num(b0, "line"), st(b0, "status").as_str()), (10, "passed"));
    assert_eq!((num(b1, "line"), num(b1, "command_line"), st(b1, "status").as_str()), (16, 17, "failed"));
    assert_ne!(num(b1, "exit_code"), 0);
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_wrong_revision_stops_before_any_block() {
    let g = s(&root().join("fixtures/outr/contrib/wrong-rev/guide.md"));
    let (code, stdout, stderr) = bin(&["--guide", &g, "--repo", &s(&root()), "--json"]);
    assert_eq!(code, 1, "stdout: {stdout}\nstderr: {stderr}");
    let r = json(&stdout);
    assert_eq!(st(&r, "error"), "revision_mismatch");
    assert_eq!(st(&r, "revision"), "0000000000000000000000000000000000000000");
    assert_eq!(st(&r, "head"), head());
    assert!(arr(&r, "blocks").is_empty());
    assert_eq!(r.get("workdir"), Some(&CValue::Null));

    let (code, stdout, _) = bin(&["--guide", &g, "--repo", &s(&root())]);
    assert_eq!(code, 1);
    assert!(stdout.contains("revision_mismatch"), "{stdout}");
    assert!(stdout.contains("no block was run"), "{stdout}");
}

// ------------------------------------------------------------------ AC-5

#[test]
fn ac5_guide_has_find_claim_expand_validate_naming_their_sources() {
    let text = real_guide();
    let mut sections: Vec<(String, String)> = Vec::new();
    for l in text.lines() {
        if let Some(h) = l.strip_prefix("## ") {
            sections.push((h.trim().to_string(), String::new()));
        } else if let Some(last) = sections.last_mut() {
            last.1.push_str(l);
            last.1.push('\n');
        }
    }
    let names: Vec<&str> = sections.iter().map(|(h, _)| h.as_str()).collect();
    let mut at = 0;
    for want in ["Find", "Claim", "Expand", "Validate"] {
        let i = names.iter().position(|h| *h == want).unwrap_or_else(|| panic!("no `## {want}` in {names:?}"));
        assert!(i >= at, "`## {want}` out of order in {names:?}");
        at = i;
        let body = &sections[i].1;
        assert!(
            ["tools/scope.py", "prompts/expand-node.md", "AGENTS.md"].iter().any(|n| body.contains(n)),
            "`## {want}` names none of the source files"
        );
    }
    for want in ["House rules", "Building from source"] {
        assert!(names.contains(&want), "no `## {want}` in {names:?}");
    }
    // REQ-005: the guide states a full commit ID; REQ-002: the runnable blocks parse.
    let g = parse_guide(&text).unwrap();
    assert_eq!(g.revision.len(), 40);
    assert!(g.blocks.iter().any(|b| b.expect_fail));
}

// ------------------------------------------------------------------ other outcomes

#[test]
fn expect_fail_spawn_failure_and_timeout() {
    if !python_available("outcomes") {
        return;
    }
    // Hand-written guide; block lines are counted from the text below.
    let text = "---\nrevision: R\n---\n\n\
```sh run expect-fail\npython --version\n```\n\n\
```sh run\nno-such-program-for-doc-run-tests --x\n```\n\n\
```sh run\npython -c \"import time; time.sleep(30)\"\n```\n\n\
```sh run\n# a comment line is skipped\npython --version\npython --version\n```\n";
    let guide = guide_with_revision("outcomes", text, &head());
    let opts = Options { guide, repo: root(), timeout: Duration::from_secs(3) };
    let r = run(&opts).unwrap();
    let got: Vec<(u32, Status, u32)> = r.blocks.iter().map(|b| (b.line, b.status, b.command_line)).collect();
    assert_eq!(
        got,
        vec![
            (5, Status::Failed, 6),
            (9, Status::SpawnFailed, 10),
            (13, Status::TimedOut, 14),
            (17, Status::Passed, 20)
        ]
    );
    assert_eq!(r.exit_code(), 1);
    assert!(r.workdir_removed);
    assert!(r.render().contains("expected a non-zero exit"));
}

#[test]
fn guide_errors_exit_2() {
    // The broken-cmd fixture states the `@HEAD@` placeholder, which is not a commit ID.
    let g = s(&root().join("fixtures/outr/contrib/broken-cmd/guide.md"));
    let (code, stdout, stderr) = bin(&["--guide", &g, "--repo", &s(&root()), "--json"]);
    assert_eq!(code, 2, "{stdout}");
    assert_eq!(st(&json(&stdout), "error"), "guide_invalid");
    assert!(stderr.contains(&format!("{g}:3: guide_invalid")), "{stderr}");

    let missing = s(&root().join("fixtures/outr/contrib/no-such-guide.md"));
    let (code, stdout, _) = bin(&["--guide", &missing, "--repo", &s(&root()), "--json"]);
    assert_eq!(code, 2);
    assert_eq!(st(&json(&stdout), "error"), "guide_unreadable");

    // A folder that is not a git repository.
    let not_repo = scratch("not-repo");
    let wrong = s(&root().join("fixtures/outr/contrib/wrong-rev/guide.md"));
    let (code, stdout, _) = bin(&["--guide", &wrong, "--repo", &s(&not_repo), "--json"]);
    assert_eq!(code, 2, "{stdout}");
    assert_eq!(st(&json(&stdout), "error"), "repo_unreadable");

    let w = wrong.as_str();
    let bad_usages: [&[&str]; 3] = [&["--guide", w], &["--repo", "."], &["--guide", w, "--repo", ".", "--x"]];
    for bad in bad_usages {
        assert_eq!(bin(bad).0, 2, "{bad:?}");
    }
}

// ------------------------------------------------------------------ parser

const HEAD40: &str = "0123456789abcdef0123456789abcdef01234567";

fn parse_err(body: &str) -> (u32, String) {
    parse_guide(&format!("---\nrevision: {HEAD40}\n---\n{body}")).unwrap_err()
}

#[test]
fn parser_finds_runnable_blocks_only() {
    let text = format!(
        "---\ntitle: t\nrevision: \"{HEAD40}\"\n---\n\
```sh\nnot run\n```\n\
~~~sh run\npython 'a b' \"c|d\" e\n\n~~~\n\
````text\n```sh run\ninside a longer fence: not a block\n````\n\
```sh run expect-fail\nx\n```\n\
\x20\x20\x20\x20```sh run\nindented four spaces: not a fence\n"
    );
    let g = parse_guide(&text).unwrap();
    assert_eq!(g.revision, HEAD40);
    assert_eq!(g.blocks.len(), 2);
    assert_eq!((g.blocks[0].line, g.blocks[0].expect_fail), (8, false));
    assert_eq!(g.blocks[0].lines[0].argv, vec!["python", "a b", "c|d", "e"]);
    assert_eq!(g.blocks[0].lines[0].line, 9);
    // Line 13 is content of the four-backtick fence, so the next block opens on line 16.
    assert_eq!((g.blocks[1].line, g.blocks[1].expect_fail), (16, true));
    assert_eq!(g.blocks[1].lines.len(), 1);
}

#[test]
fn parser_rejects_bad_guides() {
    assert_eq!(parse_guide("no front matter\n").unwrap_err().0, 1);
    assert_eq!(parse_guide("---\ntitle: t\n---\n").unwrap_err().0, 1);
    assert_eq!(parse_guide("---\nrevision: abc123\n---\n").unwrap_err().0, 2);
    assert_eq!(parse_guide(&format!("---\nrevision: {}\n---\n", HEAD40.to_uppercase())).unwrap_err().0, 2);
    assert_eq!(parse_err("```sh run\npython x\n").0, 4);
    assert_eq!(parse_err("x\n```sh run quietly\npython x\n```\n").0, 5);
    assert_eq!(parse_err("```sh run\n\n# only a comment\n```\n").0, 4);
    assert_eq!(parse_err("```sh run\npython x | head\n```\n").0, 5);
    for bad in ["a > b", "a && b", "a $HOME", "a *.md", "FOO=1 a", "cd spec", "a 'open", "a b\\c", "a # c"] {
        assert!(split_command(bad).is_err(), "{bad}");
    }
    assert_eq!(split_command("a  '' \"x y\"").unwrap(), vec!["a", "", "x y"]);
}

#[test]
fn error_codes_are_stable() {
    let e = DocRunError::RepoUnreadable { path: "r".into(), reason: "x".into() };
    assert_eq!(e.code(), "repo_unreadable");
    assert_eq!(e.to_string(), "r: repo_unreadable: x");
}
