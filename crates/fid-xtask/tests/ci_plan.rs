//! Acceptance and unit tests for `ECO.CI.GATES` (`cargo xtask ci-plan`, `fid_xtask::ci_plan`).
//!
//! Expected results come from the independent Python reference `fixtures/gates/make_cases.py`
//! (`expected.json`), never from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Command;

use fid_canon::CValue;
use fid_xtask::ci_plan::{
    normalise_path, parse_changed, plan, render_json, run, Config, ErrorCode, Glob, Outcome, Reason, MAX_CHANGED_PATHS,
    MAX_CONFIG_BYTES, MAX_GLOB_BYTES,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().expect("crates/").parent().expect("repo root").to_path_buf()
}

fn gates(rel: &str) -> PathBuf {
    repo_root().join("fixtures/gates").join(rel)
}

fn xtask(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask")).args(args).output().expect("run fid-xtask");
    (
        out.status.code().expect("exit code"),
        String::from_utf8(out.stdout).expect("utf-8 stdout"),
        String::from_utf8(out.stderr).expect("utf-8 stderr"),
    )
}

fn plan_cli(config: &str, changed: &str, json: bool) -> (i32, String, String) {
    let (c, ch) = (gates(config), gates(changed));
    let mut args = vec!["ci-plan", "--config", c.to_str().unwrap(), "--changed", ch.to_str().unwrap()];
    if json {
        args.push("--json");
    }
    xtask(&args)
}

fn expected() -> BTreeMap<String, CValue> {
    let bytes = std::fs::read(gates("expected.json")).expect("read expected.json");
    let CValue::Obj(o) = fid_canon::parse(&bytes).expect("parse expected.json") else { panic!("not an object") };
    o
}

fn s<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(v)) => v,
        other => panic!("{k}: expected a string, got {other:?}"),
    }
}

fn cases() -> BTreeMap<String, BTreeMap<String, CValue>> {
    let Some(CValue::Arr(items)) = expected().remove("cases") else { panic!("no cases") };
    items
        .into_iter()
        .map(|c| {
            let CValue::Obj(o) = c else { panic!("case is not an object") };
            (s(&o, "name").to_string(), o)
        })
        .collect()
}

/// Every reference case, through the binary, in both output modes: exit code, stdout and the
/// error code on stderr must equal the Python reference.
#[test]
fn every_reference_case_matches_the_oracle() {
    let all = cases();
    assert!(all.len() >= 30, "too few cases: {}", all.len());
    for (name, c) in &all {
        let want_exit: i32 = s(c, "exit").parse().unwrap();
        for json in [true, false] {
            let (code, out, err) = plan_cli(s(c, "config"), s(c, "changed"), json);
            assert_eq!(code, want_exit, "{name} (json={json}): exit; stderr: {err}");
            assert_eq!(out, s(c, if json { "json" } else { "text" }), "{name} (json={json}): stdout");
            match c.get("error") {
                Some(CValue::Str(e)) => {
                    assert!(err.starts_with(&format!("ci-plan: {e}: ")), "{name}: stderr {err:?} lacks {e}");
                }
                _ => assert!(err.is_empty(), "{name}: unexpected stderr {err:?}"),
            }
        }
    }
}

/// AC-1: a kernel crate's Cargo.toml selects exactly the layering gate.
#[test]
fn ac1_kernel_cargo_toml_selects_layers() {
    let (code, out, _) = plan_cli("ci-gates.toml", "kernel-change.txt", true);
    assert_eq!((code, out.as_str()), (0, "{\"gates\":[\"layers\"]}\n"));
}

/// AC-2: spec and decision paths select `spec` and not `layers`.
#[test]
fn ac2_spec_change_selects_spec_only() {
    let (code, out, _) = plan_cli("ci-gates.toml", "spec-change.txt", true);
    assert_eq!(code, 0);
    assert!(out.contains("\"spec\"") && !out.contains("\"layers\""), "{out}");
}

/// AC-3: a workflow change mixed with code exits 1 with `CI-MIXED src/lib.rs` and no plan.
#[test]
fn ac3_mixed_ci_change_is_refused() {
    let (code, out, _) = plan_cli("ci-gates.toml", "mixed.txt", true);
    assert_eq!((code, out.as_str()), (1, "CI-MIXED src/lib.rs\n"));
    assert!(!out.contains("gates"));
}

/// AC-4: a change to ci-gates.toml selects every configured gate.
#[test]
fn ac4_config_change_selects_every_gate() {
    let cfg = Config::from_bytes(&std::fs::read(gates("ci-gates.toml")).unwrap()).unwrap();
    let (code, out, _) = plan_cli("ci-gates.toml", "config-change.txt", true);
    assert_eq!(code, 0);
    let ids: Vec<String> = cfg.gates.keys().map(|k| format!("\"{k}\"")).collect();
    assert_eq!(out, format!("{{\"gates\":[{}]}}\n", ids.join(",")));
}

/// AC-5: a change that touches only CI files exits 0.
#[test]
fn ac5_ci_only_change_passes() {
    let (code, out, _) = plan_cli("ci-gates.toml", "ci-only.txt", true);
    // The fixture config lists the CI paths as shared tools, so every gate runs.
    assert_eq!((code, out.as_str()), (0, "{\"gates\":[\"apps\",\"layers\",\"rules\",\"spec\"]}\n"));
}

/// AC-6: the same inputs in shuffled orders (with duplicates and blank lines) give
/// byte-identical output, in both modes.
#[test]
fn ac6_shuffled_input_is_byte_identical() {
    for json in [true, false] {
        let a = plan_cli("ci-gates.toml", "multi.txt", json);
        let b = plan_cli("ci-gates.toml", "multi-shuffled.txt", json);
        assert_eq!(a, b);
        assert_eq!(a.0, 0);
    }
    // Every permutation of a small list, through the library.
    let cfg = Config::from_bytes(&std::fs::read(gates("ci-gates.toml")).unwrap()).unwrap();
    let lines = ["Cargo.lock", "AGENTS.md", "spec/a.md", "crates/x/src/lib.rs"];
    let mut outputs = BTreeSet::new();
    let mut idx = [0usize, 1, 2, 3];
    for _ in 0..24 {
        let text: String = idx.iter().map(|&i| format!("{}\n", lines[i])).collect();
        let Outcome::Plan(p) = plan(&cfg, &parse_changed(text.as_bytes()).unwrap()) else { panic!("mixed") };
        outputs.insert(render_json(&p));
        next_permutation(&mut idx);
    }
    assert_eq!(outputs.len(), 1, "{outputs:?}");
}

fn next_permutation(v: &mut [usize]) {
    let n = v.len();
    let Some(i) = (0..n - 1).rev().find(|&i| v[i] < v[i + 1]) else {
        v.reverse();
        return;
    };
    let j = (i + 1..n).rev().find(|&j| v[j] > v[i]).unwrap();
    v.swap(i, j);
    v[i + 1..].reverse();
}

/// The glob matcher agrees with the reference on every glob case, including long
/// backtracking-heavy inputs (which must finish quickly: the matcher never backtracks).
#[test]
fn glob_cases_match_the_oracle() {
    let Some(CValue::Arr(items)) = expected().remove("glob_cases") else { panic!("no glob cases") };
    assert!(items.len() >= 50);
    let start = std::time::Instant::now();
    for item in items {
        let CValue::Obj(o) = item else { panic!("glob case is not an object") };
        let want = matches!(o.get("match"), Some(CValue::Bool(true)));
        let g = Glob::parse(s(&o, "glob")).unwrap_or_else(|e| panic!("{:?}: {e}", s(&o, "glob")));
        assert_eq!(g.matches(s(&o, "path")), want, "glob {:?} path {:?}", s(&o, "glob"), s(&o, "path"));
    }
    assert!(start.elapsed().as_secs() < 10, "glob cases took {:?}", start.elapsed());
}

/// Globs outside the fixed syntax are refused, never reinterpreted.
#[test]
fn bad_globs_are_refused() {
    let Some(CValue::Arr(items)) = expected().remove("bad_globs") else { panic!("no bad globs") };
    assert!(items.len() >= 10);
    for item in items {
        let CValue::Str(g) = item else { panic!("bad glob is not a string") };
        assert!(Glob::parse(&g).is_err(), "accepted {g:?}");
    }
    assert!(Glob::parse(&"a".repeat(MAX_GLOB_BYTES)).is_ok());
    assert!(Glob::parse(&"a".repeat(MAX_GLOB_BYTES + 1)).is_err());
}

/// Reasons carried by the plan (REQ-001/002) and the text output naming them.
#[test]
fn reasons_name_the_path_and_glob() {
    let cfg = Config::from_bytes(&std::fs::read(gates("ci-gates-always.toml")).unwrap()).unwrap();
    let paths = parse_changed(b"zz/Cargo.toml\nCargo.toml\n").unwrap();
    let Outcome::Plan(p) = plan(&cfg, &paths) else { panic!("mixed") };
    assert_eq!(p.len(), 2);
    assert_eq!(p[0].reason, Reason::Always);
    assert_eq!(p[1].reason, Reason::Matched { path: "Cargo.toml".into(), glob: "**/Cargo.toml".into() });
    assert_eq!(p[1].run, "cargo xtask layers");
    let paths = parse_changed(b"tools/check-x.py\n").unwrap();
    let Outcome::Plan(p) = plan(&cfg, &paths) else { panic!("mixed") };
    assert!(p.iter().skip(1).all(|g| g.reason == Reason::AllSelected { path: "tools/check-x.py".into() }));
}

/// Changed-path lines: normalisation and fail-closed refusals (`GATES-INPUT`).
#[test]
fn changed_paths_are_normalised_or_refused() {
    assert_eq!(normalise_path("./spec/a.md").unwrap(), "spec/a.md");
    assert_eq!(normalise_path("crates\\a\\Cargo.toml").unwrap(), "crates/a/Cargo.toml");
    for bad in [
        "/etc/x",
        "C:/x",
        "c:\\x",
        "a//b",
        "a/",
        "../a",
        "a/./b",
        " a",
        "a ",
        "\"a\\303\"",
        "a\tb",
        "./",
        "\u{feff}a",
        "a/\u{feff}b",
    ] {
        assert!(normalise_path(bad).is_err(), "accepted {bad:?}");
    }
    assert_eq!(parse_changed(b"b\r\na\r\n\r\nb\n").unwrap().into_iter().collect::<Vec<_>>(), ["a", "b"]);
    for bad in [&b"\n\n"[..], b"", b"\xff\n", b"a\n/b\n"] {
        assert_eq!(parse_changed(bad).unwrap_err().code, ErrorCode::Input, "{bad:?}");
    }
    let mut many = String::new();
    for i in 0..=MAX_CHANGED_PATHS {
        many.push_str(&format!("p/{i}\n"));
    }
    let e = parse_changed(many.as_bytes()).unwrap_err();
    assert_eq!(e.code, ErrorCode::Input);
    assert!(e.detail.contains("more than"), "{e}");
}

/// Configuration refusals (`GATES-CONFIG`) and the size bound.
#[test]
fn bad_configurations_are_refused() {
    let base = "[[gate]]\nid = \"spec\"\nrun = \"x\"\nglobs = [\"spec/**\"]\n";
    assert!(Config::parse(base).is_ok());
    for bad in [
        "",
        "gate = 1\n",
        "[[gate]]\nid = \"spec\"\nglobs = [\"spec/**\"]\n",
        "[[gate]]\nid = \"spec\"\nrun = \"  \"\nglobs = [\"spec/**\"]\n",
        "[[gate]]\nid = \"spec\"\nrun = \"x\"\n",
        "[[gate]]\nid = \"spec\"\nrun = \"x\"\nglobs = \"spec/**\"\n",
        "[[gate]]\nid = \"spec\"\nrun = \"x\"\nglobs = [\"spec/**\", \"spec/**\"]\n",
        "[[gate]]\nid = \"spec\"\nrun = \"x\"\nglobs = [\"spec/**\"]\nalways = \"yes\"\n",
        "[[gate]]\nid = \"-spec\"\nrun = \"x\"\nglobs = [\"spec/**\"]\n",
        "[[gate]]\nid = \"spec\"\nrun = \"x\\ny\"\nglobs = [\"spec/**\"]\n",
        format!("{base}[isolation]\nci = [\"x/**\"]\n").as_str(),
        format!("{base}[shared]\ntool_globs = [\"a\", 1]\n").as_str(),
        format!("{base}[shared]\ntools = []\n").as_str(),
        format!("{base}isolation = []\n").as_str(),
        format!("{base}version = 1\n").as_str(),
    ] {
        assert_eq!(Config::parse(bad).unwrap_err().code, ErrorCode::Config, "{bad:?}");
    }
    let big = format!("{base}# {}\n", "x".repeat(MAX_CONFIG_BYTES));
    assert_eq!(Config::from_bytes(big.as_bytes()).unwrap_err().code, ErrorCode::Config);
    assert_eq!(Config::from_bytes(b"\xff").unwrap_err().code, ErrorCode::Config);
    // Defaults apply when [isolation] / [shared] are absent.
    let c = Config::parse(base).unwrap();
    let ci: Vec<&str> = c.ci_globs.iter().map(Glob::as_str).collect();
    assert_eq!(ci, [".github/workflows/**", ".github/actions/**", "ci/**"]);
    assert_eq!(c.tool_globs.iter().map(Glob::as_str).collect::<Vec<_>>(), ["xtask/**"]);
}

/// Unreadable files are `GATES-IO`; usage errors exit 2.
#[test]
fn io_and_usage_errors() {
    let missing = gates("does-not-exist.txt");
    assert_eq!(run(&gates("ci-gates.toml"), &missing).unwrap_err().code, ErrorCode::Io);
    assert_eq!(run(&missing, &gates("kernel-change.txt")).unwrap_err().code, ErrorCode::Io);
    let (code, out, err) = xtask(&["ci-plan", "--config", gates("ci-gates.toml").to_str().unwrap()]);
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(err.contains("usage: cargo xtask ci-plan"), "{err}");
    let c = gates("ci-gates.toml");
    let ch = gates("kernel-change.txt");
    let (c, ch) = (c.to_str().unwrap(), ch.to_str().unwrap());
    for args in [
        vec!["ci-plan", "--config", c, "--changed", ch, "--json", "--json"],
        vec!["ci-plan", "--config", c, "--config", c, "--changed", ch],
        vec!["ci-plan", "--config", c, "--changed", ch, "--bogus"],
        vec!["ci-plan", "--config", c, "--changed"],
    ] {
        assert_eq!(xtask(&args).0, 2, "{args:?}");
    }
}

/// The repository's own ci-gates.toml is valid and maps the paths REQ-ECO.CI.GATES-004 names
/// to the layering, rules, spec-tree and app-list gates.
#[test]
fn repository_config_implements_the_default_gate_set() {
    let cfg = Config::from_bytes(&std::fs::read(repo_root().join("ci-gates.toml")).unwrap()).unwrap();
    assert_eq!(cfg.gates.keys().map(String::as_str).collect::<Vec<_>>(), ["apps", "layers", "rules", "spec"]);
    let one = |p: &str| -> Vec<String> {
        let Outcome::Plan(planned) = plan(&cfg, &parse_changed(format!("{p}\n").as_bytes()).unwrap()) else {
            panic!("{p}: mixed")
        };
        planned.into_iter().map(|s| s.id).collect()
    };
    for (path, gate) in [
        ("layers.toml", "layers"),
        ("kernel-allowlist.toml", "layers"),
        ("Cargo.toml", "layers"),
        ("crates/fid-num/Cargo.toml", "layers"),
        ("AGENTS.md", "rules"),
        ("CLAUDE.md", "rules"),
        ("house-rules/core.md", "rules"),
        ("spec/eco/ci/gates/NODE.md", "spec"),
        ("seeds/eco.md", "spec"),
        ("decisions/D-001-licence.md", "spec"),
        ("apps.toml", "apps"),
    ] {
        assert_eq!(one(path), [gate], "{path}");
    }
    assert!(one("crates/fid-num/src/lib.rs").is_empty());
    for all in [
        "ci-gates.toml",
        "crates/fid-xtask/src/ci_plan.rs",
        "Cargo.lock",
        ".github/workflows/rust.yml",
        ".github/actions/setup/action.yml",
        "ci/fixture-checks.txt",
    ] {
        assert_eq!(one(all).len(), cfg.gates.len(), "{all}");
    }
    // Workflow files are CI paths here; the ci/ lists are not (they travel with leaf PRs), but
    // they select every gate as shared tools (checked above).
    let mixed = parse_changed(b".github/workflows/rust.yml\nspec/a.md\n").unwrap();
    assert_eq!(plan(&cfg, &mixed), Outcome::CiMixed { non_ci: vec!["spec/a.md".into()] });
    let with_code = parse_changed(b"ci/fixture-checks.txt\ncrates/fid-num/src/lib.rs\n").unwrap();
    assert!(matches!(plan(&cfg, &with_code), Outcome::Plan(p) if p.len() == cfg.gates.len()));
}
