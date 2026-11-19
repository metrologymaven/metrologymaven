//! Acceptance criteria of spec node KRN.NUM.LINT.TYPED (AC-1 .. AC-4), the diagnostic mapping,
//! the fail-closed paths and the pinned table. Expected results are hand-labelled
//! (`fixtures/knp1/typed/ws/expected-*.txt`, the assertions on `messages.jsonl`) or come from the
//! independent Python reference `fixtures/knp1/typed/make_expected.py`; none is produced by
//! this crate. The end-to-end tests run the toolchain's `cargo clippy` (CI installs the three
//! kernel targets).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_knp1_lint::find::{Finding, Rule};
use fid_knp1_lint::graph;
use fid_knp1_lint::syn_scan;
use fid_knp1_lint::typed::{
    check, clippy_toml, driver_version, finish, parse_config, parse_messages, run, target_findings, CrateRef, ItemKind,
    MapCtx, Options, TypedErrorCode, DEFAULT_TARGETS, TYPED_TABLE, TYPED_TABLE_LEN,
};
use std::path::{Path, PathBuf};

fn fix(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/knp1/typed").join(rel)
}

fn read(p: &Path) -> Vec<u8> {
    std::fs::read(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn cargo() -> PathBuf {
    std::env::var_os("CARGO").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("cargo"))
}

fn scratch(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)
}

/// `RULE crate file:line`.
fn lines(fs: &[Finding]) -> Vec<String> {
    fs.iter().map(|f| format!("{} {} {}:{}", f.rule, f.crate_name, f.file, f.line)).collect()
}

fn text_lines(p: &Path) -> Vec<String> {
    String::from_utf8(read(p)).unwrap().lines().filter(|l| !l.is_empty()).map(str::to_string).collect()
}

fn ws() -> PathBuf {
    std::fs::canonicalize(fix("ws")).unwrap()
}

fn crate_ref(dir: &str) -> CrateRef {
    CrateRef {
        name: format!("typed-{dir}"),
        version: "0.0.1".into(),
        root: std::fs::canonicalize(ws().join(dir)).unwrap(),
    }
}

fn ctx(dirs: &[&str]) -> MapCtx {
    MapCtx { cwd: ws(), crates: dirs.iter().map(|d| crate_ref(d)).collect() }
}

/// The message fixture with `@WS@` replaced by the absolute workspace path.
fn messages(name: &str) -> Vec<u8> {
    let ws = ws().to_string_lossy().replace('\\', "/");
    let ws = ws.strip_prefix("//?/").unwrap_or(&ws).to_string();
    String::from_utf8(read(&fix(name))).unwrap().replace("@WS@", &ws).into_bytes()
}

// ------------------------------------------------------------------ generated config

#[test]
fn generated_clippy_toml_matches_the_python_reference() {
    assert_eq!(clippy_toml().into_bytes(), read(&fix("expected-clippy.toml")));
}

#[test]
fn table_is_pinned_in_node_md() {
    assert_eq!(TYPED_TABLE.len(), TYPED_TABLE_LEN);
    let node = String::from_utf8(read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/krn/num/lint/typed/NODE.md"),
    ))
    .unwrap()
    .replace('\r', "");
    for (rule, kind, path) in &TYPED_TABLE {
        let k = match kind {
            ItemKind::Method => "method",
            ItemKind::Type => "type",
            ItemKind::Macro => "macro",
        };
        let row = format!("| `{}` | {k} | `{path}` |", rule.id());
        assert!(node.contains(&row), "NODE.md Design does not pin {row}");
        assert!(rule.is_waivable(), "{row}");
    }
    let pinned = node.lines().filter(|l| l.starts_with("| `KNP1-")).count();
    assert_eq!(pinned, TYPED_TABLE_LEN);
}

#[test]
fn config_is_validated() {
    let ok = parse_config("[typed]\nclippy_version = \"0.1.89\"\n").unwrap();
    assert_eq!(ok.targets, DEFAULT_TARGETS.iter().map(|s| (*s).to_string()).collect::<Vec<_>>());
    for bad in [
        "",
        "[typed]\n",
        "[typed]\nclippy_version = \"\"\n",
        "[typed]\nclippy_version = \"0.1 89\"\n",
        "[typed]\nclippy_version = \"0.1.89\"\ntargets = []\n",
        "[typed]\nclippy_version = \"0.1.89\"\ntargets = [\"a\", \"a\"]\n",
        "[typed]\nclippy_version = \"0.1.89\"\ntargets = [\"X86\"]\n",
        "[typed]\nclippy_version = \"0.1.89\"\ntimeout_secs = 0\n",
        "[typed]\nclippy_version = \"0.1.89\"\nother = 1\n",
    ] {
        assert_eq!(parse_config(bad).unwrap_err().code, TypedErrorCode::ConfigInvalid, "{bad:?}");
    }
}

// ------------------------------------------------------------------ mapping diagnostics

#[test]
fn diagnostics_map_to_the_outermost_in_crate_span() {
    let c = ctx(&["clean", "macro", "minmax-var", "nowasm"]);
    let p = parse_messages(&messages("messages.jsonl"), "wasm32-unknown-unknown", &c).unwrap();
    let got: Vec<String> =
        p.findings.iter().map(|f| format!("{} {} {}:{}:{}", f.rule, f.crate_name, f.file, f.line, f.column)).collect();
    assert_eq!(
        got,
        [
            // In a local macro: the call site (line 20), not the body (line 5).
            "KNP1-FMA typed-macro src/lib.rs:20:5",
            // An absolute span path.
            "KNP1-MINMAX typed-minmax-var src/lib.rs:9:37",
            // A span in std whose expansion lands in the crate.
            "KNP1-MINMAX typed-minmax-var src/lib.rs:5:15",
        ]
    );
    for f in &p.findings {
        f.validate().unwrap();
        assert!(f.message.contains("wasm32-unknown-unknown"), "{}", f.message);
    }
    assert!(p.unreadable);
    assert_eq!(p.built.iter().cloned().collect::<Vec<_>>(), ["typed-macro", "typed-minmax-var"]);
    assert_eq!(p.errors.keys().cloned().collect::<Vec<_>>(), ["typed-nowasm"]);

    // Crates with a compile error or without an artifact are KNP1-TYPED-BUILD.
    let all = target_findings(&p, &c, "wasm32-unknown-unknown", None);
    let builds: Vec<String> =
        all.iter().filter(|f| f.rule == Rule::Typed("BUILD".into())).map(|f| f.crate_name.clone()).collect();
    assert_eq!(builds, ["typed-clean", "typed-nowasm"]);
    for f in &all {
        f.validate().unwrap();
    }
    assert!(!Rule::Typed("BUILD".into()).is_waivable());
}

#[test]
fn unmapped_disallowed_diagnostic_is_a_driver_error() {
    let e =
        parse_messages(&messages("messages-unmapped.jsonl"), "x86_64-unknown-linux-gnu", &ctx(&["macro"])).unwrap_err();
    assert_eq!(e.code, TypedErrorCode::Driver);
    assert_eq!(e.code.as_str(), "KNP1-TYPED-DRIVER");
}

#[test]
fn facade_files_turn_libm_into_notes_and_targets_are_deduplicated() {
    let syn_cfg = syn_scan::parse_config(
        "[[syn.facade_files]]\ncrate = \"typed-macro\"\nversion = \"0.0.1\"\nfile = \"src/lib.rs\"\nreason = \"r\"\n",
    )
    .unwrap();
    let f = |rule: Rule, line: u32| Finding {
        rule,
        crate_name: "typed-macro".into(),
        crate_version: "0.0.1".into(),
        file: "src/lib.rs".into(),
        line,
        column: 5,
        excerpt_digest: format!("sha256:{}", "0".repeat(64)),
        message: "m".into(),
    };
    let out = finish(vec![f(Rule::Libm, 5), f(Rule::Fma, 20), f(Rule::Fma, 20), f(Rule::Libm, 5)], &syn_cfg);
    assert_eq!(lines(&out.findings), ["KNP1-FMA typed-macro src/lib.rs:20"]);
    assert_eq!(lines(&out.notes), ["KNP1-LIBM typed-macro src/lib.rs:5"]);
    assert!(out.notes[0].message.starts_with("opt-out (facade file: r)"));
}

// ------------------------------------------------------------------ the driver

#[test]
fn missing_driver_is_fatal() {
    let e = driver_version(Path::new("definitely-not-a-cargo-binary-knp1")).unwrap_err();
    assert_eq!(e.code, TypedErrorCode::Driver);
}

#[test]
fn wrong_clippy_version_is_fatal() {
    // The fixture pins 0.0.0, which no toolchain has.
    let opts = Options { cargo: cargo(), scratch: scratch("typed-version") };
    let e = run(&fix("ws/knp1-lint.toml"), &opts).unwrap_err();
    assert_eq!(e.code, TypedErrorCode::Driver, "{e}");
}

#[test]
fn ac1_to_ac4_end_to_end_on_three_targets() {
    let config = fix("ws/knp1-lint.toml");
    let text = String::from_utf8(read(&config)).unwrap();
    let mut cfg = parse_config(&text).unwrap();
    cfg.clippy_version = driver_version(&cargo()).unwrap();
    let syn_cfg = syn_scan::parse_config(&text).unwrap();
    let gcfg = graph::load_config(&config).unwrap();
    let closure = graph::compute_closure(&gcfg).unwrap();
    let opts = Options { cargo: cargo(), scratch: scratch("typed-ws") };
    let out = check(&closure, &gcfg.workspace, &cfg, &syn_cfg, &opts).unwrap();
    for f in out.findings.iter().chain(&out.notes) {
        f.validate().unwrap();
    }
    // AC-2..AC-4 and the bypass cases, hand-labelled; the aliased HashMap is checked below.
    let mut got: Vec<String> = lines(&out.findings).into_iter().filter(|l| !l.contains(" typed-alias ")).collect();
    got.dedup();
    assert_eq!(got, text_lines(&fix("ws/expected-findings.txt")), "{:#?}", out.findings);
    assert_eq!(lines(&out.notes), text_lines(&fix("ws/expected-notes.txt")));
    // AC-4: the build failure names the crate and the target.
    let b = out.findings.iter().find(|f| f.rule == Rule::Typed("BUILD".into())).unwrap();
    assert!(b.message.contains("typed-nowasm") && b.message.contains("wasm32-unknown-unknown"), "{}", b.message);
    // REQ-004: the aliased HashMap is seen.
    let alias: Vec<&Finding> = out.findings.iter().filter(|f| f.crate_name == "typed-alias").collect();
    assert!(!alias.is_empty() && alias.iter().all(|f| f.rule == Rule::Hash), "{alias:#?}");
    assert!(alias.iter().all(|f| f.line == 3 || f.line == 6), "{alias:#?}");
    // AC-3: nothing for the clean crate.
    assert!(out.findings.iter().all(|f| f.crate_name != "typed-clean"));
}
