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
    cache_dir_name, check_run, check_run_with_driver_probe, clippy_toml, driver_version, finish, lint_args,
    parse_config, parse_messages, prune_cache_dirs, run, target_findings, unresolved_allowed, CrateRef, ItemKind,
    MapCtx, Options, TypedErrorCode, CACHE_STAMP, DEFAULT_TARGETS, KEEP_CACHE_DIRS, TYPED_TABLE, TYPED_TABLE_LEN,
};
use std::collections::BTreeMap;
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
    MapCtx { cwd: ws(), crates: dirs.iter().map(|d| crate_ref(d)).collect(), scratch: None }
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
            // No span inside the crate: never waivable.
            "KNP1-TYPED-NOSPAN typed-macro Cargo.toml:1:1",
        ]
    );
    assert!(!Rule::Typed("NOSPAN".into()).is_waivable());
    for f in &p.findings {
        f.validate().unwrap();
        assert!(f.message.contains("wasm32-unknown-unknown"), "{}", f.message);
    }
    // `typed-clean` has only a build-script artifact, which does not count.
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
fn unreadable_lines_and_unresolved_table_paths_are_driver_errors() {
    for (file, target) in [
        ("messages-garbage.jsonl", "x86_64-unknown-linux-gnu"),
        ("messages-unresolved.jsonl", "x86_64-unknown-linux-gnu"),
    ] {
        let e = parse_messages(&messages(file), target, &ctx(&["macro"])).unwrap_err();
        assert_eq!(e.code, TypedErrorCode::Driver, "{file}: {e}");
    }
    // The aarch64 detection macro may be unresolved off aarch64, never on it.
    assert!(unresolved_allowed("std::arch::is_aarch64_feature_detected", "x86_64-unknown-linux-gnu"));
    assert!(!unresolved_allowed("std::arch::is_aarch64_feature_detected", "aarch64-apple-darwin"));
    assert!(unresolved_allowed("std::is_x86_feature_detected", "wasm32-unknown-unknown"));
    assert!(!unresolved_allowed("std::is_x86_feature_detected", "x86_64-pc-windows-msvc"));
    assert!(!unresolved_allowed("f64::sin", "wasm32-unknown-unknown"));
    let e = parse_messages(&messages("messages.jsonl"), "aarch64-unknown-linux-gnu", &ctx(&["macro"])).unwrap_err();
    assert_eq!(e.code, TypedErrorCode::Driver);
}

#[test]
fn compile_errors_in_driver_wording_are_build_findings_not_driver_errors() {
    // E0277 says "expected an `FnMut(f64)` closure, found `f64`", the wording of a wrong-kind
    // configuration warning; with an error code and a span in the crate it is a compile error.
    let c = ctx(&["e0277", "macro"]);
    let p = parse_messages(&messages("messages-e0277.jsonl"), "x86_64-unknown-linux-gnu", &c).unwrap();
    assert!(p.unresolved_allowed.is_empty());
    assert_eq!(p.errors.keys().cloned().collect::<Vec<_>>(), ["typed-e0277"]);
    let all = target_findings(&p, &c, "x86_64-unknown-linux-gnu, default features", Some("driver failed"));
    assert_eq!(lines(&all), ["KNP1-TYPED-BUILD typed-e0277 Cargo.toml:1"]);
    assert!(all[0].message.contains("compile error: expected an `FnMut(f64)` closure"), "{}", all[0].message);
    // The same wording with no code and no span in a crate is still a driver configuration
    // warning naming a path the driver cannot resolve: fatal.
    let e = parse_messages(&messages("messages-wrong-kind.jsonl"), "x86_64-unknown-linux-gnu", &c).unwrap_err();
    assert_eq!(e.code, TypedErrorCode::Driver, "{e}");
    assert!(e.message.contains("knp1_wrong_kind"), "{e}");
}

#[test]
fn driver_wording_without_code_but_with_an_in_crate_span_is_not_a_driver_warning() {
    let c = ctx(&["macro"]);
    let p = parse_messages(&messages("messages-nocode-incrate.jsonl"), "x86_64-unknown-linux-gnu", &c).unwrap();
    assert!(p.unresolved_allowed.is_empty());
    assert_eq!(p.errors.keys().cloned().collect::<Vec<_>>(), ["typed-macro"]);
    let all = target_findings(&p, &c, "x86_64-unknown-linux-gnu", None);
    assert_eq!(lines(&all), ["KNP1-TYPED-BUILD typed-macro Cargo.toml:1"]);
}

/// A codeless "does not refer to" warning whose only span is `file`.
fn config_warning(manifest: &Path, file: &Path) -> Vec<u8> {
    let j = |p: &Path| p.to_string_lossy().replace('\\', "/");
    format!(
        "{{\"reason\":\"compiler-message\",\"manifest_path\":\"{}\",\"message\":{{\"message\":\"`std::knp1_scratch` does not refer to an existing function\",\"code\":null,\"level\":\"warning\",\"spans\":[{{\"file_name\":\"{}\",\"line_start\":1,\"column_start\":1,\"is_primary\":true,\"expansion\":null}}],\"children\":[]}}}}\n",
        j(manifest),
        j(file)
    )
    .into_bytes()
}

#[test]
fn spans_under_the_scratch_root_never_count_as_inside_a_crate() {
    // A crate whose root holds the scratch root (which check_run refuses; see below): the
    // generated clippy.toml lies under the crate root, but the warning still fails the run.
    let root = scratch("typed-scratch-span");
    let _ = std::fs::remove_dir_all(&root);
    let krate = root.join("k");
    let conf = krate.join("scratch").join("cfg-0000000000000000").join("clippy-conf");
    std::fs::create_dir_all(&conf).unwrap();
    std::fs::write(krate.join("Cargo.toml"), b"").unwrap();
    std::fs::write(conf.join("clippy.toml"), b"").unwrap();
    let krate = std::fs::canonicalize(&krate).unwrap();
    let stream = config_warning(&krate.join("Cargo.toml"), &conf.join("clippy.toml"));
    let cr = CrateRef { name: "k".into(), version: "0.0.1".into(), root: krate.clone() };
    let mut c = MapCtx { cwd: krate.clone(), crates: vec![cr], scratch: Some(krate.join("scratch")) };
    let e = parse_messages(&stream, "x86_64-unknown-linux-gnu", &c).unwrap_err();
    assert_eq!(e.code, TypedErrorCode::Driver, "{e}");
    assert!(e.message.contains("knp1_scratch"), "{e}");
    // Without the scratch root the span would count as crate code and the warning would be lost.
    c.scratch = None;
    assert!(parse_messages(&stream, "x86_64-unknown-linux-gnu", &c).is_ok());
}

#[test]
fn a_scratch_root_inside_a_linted_crate_is_refused() {
    let config = fix("ws/knp1-lint.toml");
    let text = String::from_utf8(read(&config)).unwrap();
    let cfg = parse_config(&text).unwrap();
    let syn_cfg = syn_scan::parse_config(&text).unwrap();
    let gcfg = graph::load_config(&config).unwrap();
    let closure = graph::compute_closure(&gcfg).unwrap();
    let inside = ws().join("clean").join("knp1-scratch-refused").join("deeper");
    let opts = Options { cargo: PathBuf::from("definitely-not-a-cargo-binary-knp1"), scratch: inside.clone() };
    let e = check_run(&closure, &gcfg.workspace, &cfg, &syn_cfg, &opts).unwrap_err();
    assert_eq!(e.code, TypedErrorCode::ConfigInvalid, "{e}");
    assert!(e.message.contains("typed-clean"), "{e}");
    // Refused before anything is written (and before the driver is looked for).
    assert!(!ws().join("clean").join("knp1-scratch-refused").exists());
    // Also through `..`.
    let dotted = ws().join("forms").join("..").join("clean").join("src").join("new");
    let opts = Options { cargo: PathBuf::from("definitely-not-a-cargo-binary-knp1"), scratch: dotted };
    let e = check_run(&closure, &gcfg.workspace, &cfg, &syn_cfg, &opts).unwrap_err();
    assert_eq!(e.code, TypedErrorCode::ConfigInvalid, "{e}");
}

#[test]
fn too_many_spans_on_a_driver_warning_fail_closed() {
    let span = "{\"file_name\":\"/elsewhere/x.rs\",\"line_start\":1,\"column_start\":1,\"is_primary\":false,\"expansion\":null}";
    let spans = vec![span; 5000].join(",");
    let ws = ws().to_string_lossy().replace('\\', "/");
    let ws = ws.strip_prefix("//?/").unwrap_or(&ws).to_string();
    let stream = format!(
        "{{\"reason\":\"compiler-message\",\"manifest_path\":\"{ws}/macro/Cargo.toml\",\"message\":{{\"message\":\"`f64::sin` does not refer to an existing function\",\"code\":null,\"level\":\"warning\",\"spans\":[{spans}],\"children\":[]}}}}\n"
    );
    let e = parse_messages(stream.as_bytes(), "x86_64-unknown-linux-gnu", &ctx(&["macro"])).unwrap_err();
    assert_eq!(e.code, TypedErrorCode::Driver, "{e}");
    assert!(e.message.contains("more than 4096 spans:"), "{e}");
}

/// A disallowed-method diagnostic whose primary span (line 5) sits under `levels` nested
/// expansions (line 8), the outermost at the call site (line 13).
fn deep_message(levels: usize) -> Vec<u8> {
    let ws = ws().to_string_lossy().replace('\\', "/");
    let ws = ws.strip_prefix("//?/").unwrap_or(&ws).to_string();
    let span = |line: u32, col: u32, exp: &str| {
        format!(
            "{{\"file_name\":\"deep/src/lib.rs\",\"line_start\":{line},\"column_start\":{col},\"is_primary\":true,\"expansion\":{exp}}}"
        )
    };
    let mut exp = format!("{{\"span\":{}}}", span(13, 5, "null"));
    for _ in 1..levels {
        exp = format!("{{\"span\":{}}}", span(8, 9, &exp));
    }
    let primary = span(5, 12, &exp);
    format!(
        "{{\"reason\":\"compiler-message\",\"manifest_path\":\"{ws}/deep/Cargo.toml\",\"message\":{{\"message\":\"use of a disallowed method `f64::mul_add`\",\"code\":{{\"code\":\"clippy::disallowed_methods\"}},\"level\":\"warning\",\"spans\":[{primary}],\"children\":[{{\"message\":\"KNP1-FMA\"}}]}}}}\n"
    )
    .into_bytes()
}

#[test]
fn deep_expansion_chains_are_read_or_fail_closed() {
    let c = ctx(&["deep"]);
    // 70 nested expansions: read, and placed at the outermost call site.
    let p = parse_messages(&deep_message(70), "x86_64-unknown-linux-gnu", &c).unwrap();
    let got: Vec<String> =
        p.findings.iter().map(|f| format!("{} {}:{}:{}", f.rule, f.file, f.line, f.column)).collect();
    assert_eq!(got, ["KNP1-FMA src/lib.rs:13:5"]);
    // Deeper than the parser's limit: the whole run fails, nothing is dropped.
    let e = parse_messages(&deep_message(400), "x86_64-unknown-linux-gnu", &c).unwrap_err();
    assert_eq!(e.code, TypedErrorCode::Driver);
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

// ------------------------------------------------------------------ cache directories

#[test]
fn cache_dir_name_covers_driver_version_lints_and_config() {
    let v = "clippy 0.1.89 (29483883ee 2025-08-04)";
    let toml = clippy_toml();
    let name = cache_dir_name(v, &toml);
    assert_eq!(name, cache_dir_name(v, &toml));
    assert!(name.starts_with("cfg-") && name.len() == 20, "{name}");
    assert!(name[4..].bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "{name}");
    // Another driver build (same second word), another configuration: another directory.
    assert_ne!(name, cache_dir_name("clippy 0.1.89 (0000000000 2025-08-05)", &toml));
    assert_ne!(name, cache_dir_name(v, &format!("{toml}# x\n")));
    // The length prefixes keep the fields apart.
    assert_ne!(cache_dir_name("a", "b\nc"), cache_dir_name("a\nb", "c"));
    // The lints the key covers are the ones the driver is given.
    assert_eq!(
        lint_args(),
        [
            "-A",
            "clippy::all",
            "--force-warn",
            "clippy::disallowed_methods",
            "--force-warn",
            "clippy::disallowed_types",
            "--force-warn",
            "clippy::disallowed_macros"
        ]
    );
}

#[test]
fn pruning_keeps_the_current_and_the_most_recent_cache_dirs_inside_the_scratch_root() {
    assert_eq!(KEEP_CACHE_DIRS, 2);
    let root = scratch("typed-prune");
    let _ = std::fs::remove_dir_all(&root);
    let scratch_root = root.join("scratch");
    std::fs::create_dir_all(&scratch_root).unwrap();
    let base = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    let dir = |i: u64| format!("cfg-{i:016x}");
    // cfg-…1 is the oldest, cfg-…5 the newest; cfg-…3 is the current one.
    for i in 1..=5u64 {
        let d = scratch_root.join(dir(i));
        std::fs::create_dir_all(d.join("target")).unwrap();
        std::fs::write(d.join("target").join("x"), b"x").unwrap();
        let stamp = d.join(CACHE_STAMP);
        std::fs::write(&stamp, b"clippy").unwrap();
        let f = std::fs::File::options().write(true).open(&stamp).unwrap();
        f.set_modified(base + std::time::Duration::from_secs(i * 60)).unwrap();
    }
    // Without a stamp: oldest of all.
    std::fs::create_dir_all(scratch_root.join(dir(6))).unwrap();
    // Not cache directories: left alone.
    for other in ["cfg-ABCDEF0123456789", "cfg-0123", "keep-me", "cfg-0123456789abcdeg"] {
        std::fs::create_dir_all(scratch_root.join(other)).unwrap();
    }
    std::fs::write(scratch_root.join("cfg-00000000000000ff"), b"a file").unwrap();
    // A cache-like directory outside the scratch root is never touched.
    std::fs::create_dir_all(root.join(dir(7))).unwrap();

    let removed = prune_cache_dirs(&scratch_root, &dir(3), KEEP_CACHE_DIRS);
    assert_eq!(removed, [dir(1), dir(2), dir(6)]);
    let mut left: Vec<String> = std::fs::read_dir(&scratch_root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(
        left,
        [
            "cfg-0000000000000003",
            "cfg-0000000000000004",
            "cfg-0000000000000005",
            "cfg-00000000000000ff",
            "cfg-0123",
            "cfg-0123456789abcdeg",
            "cfg-ABCDEF0123456789",
            "keep-me"
        ]
    );
    assert!(root.join(dir(7)).is_dir());
    // Idempotent; a missing scratch root is not an error.
    assert!(prune_cache_dirs(&scratch_root, &dir(3), KEEP_CACHE_DIRS).is_empty());
    assert!(prune_cache_dirs(&root.join("missing"), &dir(3), KEEP_CACHE_DIRS).is_empty());
}

// ------------------------------------------------------------------ the canary (kind guard)

/// `(rule, kind, path)` of each `// row NNN: RULE kind path` comment of the canary, in order.
fn canary_rows() -> Vec<(String, String, String)> {
    let src = String::from_utf8(read(&fix("canary/src/lib.rs"))).unwrap();
    src.lines()
        .filter_map(|l| l.split_once(" // row ").map(|(_, r)| r))
        .map(|r| {
            let w: Vec<&str> = r.split_whitespace().collect();
            assert_eq!(w.len(), 4, "{r}");
            (w[1].to_string(), w[2].to_string(), w[3].to_string())
        })
        .collect()
}

#[test]
fn canary_uses_every_table_row_once() {
    let kind = |k: ItemKind| match k {
        ItemKind::Method => "method",
        ItemKind::Type => "type",
        ItemKind::Macro => "macro",
    };
    let want: Vec<(String, String, String)> =
        TYPED_TABLE.iter().map(|(r, k, p)| (r.id(), kind(*k).to_string(), (*p).to_string())).collect();
    assert_eq!(canary_rows(), want);
}

#[test]
fn every_table_row_is_guarded_by_some_lint() {
    // Each canary row line needs an expected hit with its rule: from the driver on some target,
    // or from the syntax scan (`syn`). No row may be left unguarded.
    let src = String::from_utf8(read(&fix("canary/src/lib.rs"))).unwrap();
    let hits = text_lines(&fix("canary/expected-hits.txt"));
    let mut guards: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (n, l) in src.lines().enumerate() {
        let Some((_, row)) = l.split_once(" // row ") else { continue };
        let w: Vec<&str> = row.split_whitespace().collect();
        let (rule, kind, line) = (w[1], w[2], (n + 1).to_string());
        let g: Vec<String> = hits
            .iter()
            .filter_map(|h| {
                let x: Vec<&str> = h.split_whitespace().collect();
                // A `syn` line carries the syntax scan's rule, which may differ from the row's.
                ((x[0] == "syn" || x[1] == rule) && x[2] == kind && x[3] == line).then(|| x[0].to_string())
            })
            .collect();
        assert!(!g.is_empty(), "row {row} (line {line}) is guarded by no lint");
        guards.insert(w[3].to_string(), g);
    }
    assert_eq!(guards.len(), TYPED_TABLE_LEN);
    // The one row clippy cannot see is guarded by the syntax scan only.
    assert_eq!(guards["std::include"], ["syn"]);
    assert_eq!(guards.values().filter(|g| g.iter().any(|t| t == "syn")).count(), 1);
}

/// The kind a disallowed-item finding's message names (`method`, `type` or `macro`).
fn kind_of(message: &str) -> &'static str {
    ["method", "type", "macro"].into_iter().find(|k| message.contains(&format!("disallowed {k} "))).unwrap_or("?")
}

#[test]
fn canary_hits_every_table_row_on_every_target() {
    // The kind guard: clippy ignores an entry filed under the wrong kind without a word, so
    // every row must produce exactly its expected hit, per target, as rendered by the
    // independent make_canary.py.
    let config = fix("canary/knp1-lint.toml");
    let text = String::from_utf8(read(&config)).unwrap();
    let mut cfg = parse_config(&text).unwrap();
    cfg.clippy_version = driver_version(&cargo()).unwrap();
    let syn_cfg = syn_scan::parse_config(&text).unwrap();
    let gcfg = graph::load_config(&config).unwrap();
    let closure = graph::compute_closure(&gcfg).unwrap();
    let opts = Options { cargo: cargo(), scratch: scratch("typed-canary") };
    let mut got = Vec::new();
    for target in DEFAULT_TARGETS {
        cfg.targets = vec![target.to_string()];
        let run = check_run(&closure, &gcfg.workspace, &cfg, &syn_cfg, &opts).unwrap();
        assert!(run.outcome.notes.is_empty(), "{target}: {:#?}", run.outcome.notes);
        for f in &run.outcome.findings {
            f.validate().unwrap();
            assert_eq!((f.crate_name.as_str(), f.file.as_str()), ("typed-canary", "src/lib.rs"), "{target}: {f:?}");
            got.push(format!("{target} {} {} {}", f.rule, kind_of(&f.message), f.line));
        }
    }
    got.sort();
    // `syn …` rows are guarded by KRN.NUM.LINT.SYN (tests/syn.rs), not by the driver.
    let mut want: Vec<String> =
        text_lines(&fix("canary/expected-hits.txt")).into_iter().filter(|l| !l.starts_with("syn ")).collect();
    want.sort();
    let missing: Vec<&String> = want.iter().filter(|w| !got.contains(w)).collect();
    let extra: Vec<&String> = got.iter().filter(|g| !want.contains(g)).collect();
    assert!(missing.is_empty() && extra.is_empty(), "missing hits: {missing:#?}\nunexpected hits: {extra:#?}");
    assert_eq!(got, want);
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
fn an_unresolvable_table_path_fails_the_real_driver_run() {
    // Proves the real driver's wording matches the detector: a path that exists nowhere must
    // make the run fail with KNP1-TYPED-DRIVER, not lint silently without it.
    let config = fix("ws/knp1-lint.toml");
    let text = String::from_utf8(read(&config)).unwrap();
    let mut cfg = parse_config(&text).unwrap();
    cfg.clippy_version = driver_version(&cargo()).unwrap();
    cfg.targets = vec!["x86_64-unknown-linux-gnu".to_string()];
    let syn_cfg = syn_scan::parse_config(&text).unwrap();
    let gcfg = graph::load_config(&config).unwrap();
    let mut closure = graph::compute_closure(&gcfg).unwrap();
    closure.crates.retain(|c| c.name == "typed-clean");
    let opts = Options { cargo: cargo(), scratch: scratch("typed-unresolved") };
    // A path that exists nowhere fails the run with KNP1-TYPED-DRIVER.
    let probe = "std::knp1_no_such_function";
    let e = match check_run_with_driver_probe(&closure, &gcfg.workspace, &cfg, &syn_cfg, &opts, &[probe.to_string()]) {
        Ok(r) => panic!("probe {probe}: the driver accepted it silently: {r:?}"),
        Err(e) => e,
    };
    assert_eq!(e.code, TypedErrorCode::Driver, "{e}");
    assert!(e.message.contains("knp1_no_such_function"), "{e}");
    // A path of the wrong kind (a type listed as a method) is accepted silently by the driver,
    // with no wording to detect. Pinned here so a driver change shows up; NODE.md records that
    // the kinds are guarded by the pinned table and the end-to-end fixtures, not by the driver.
    let wrong = "std::collections::BTreeMap";
    let r = check_run_with_driver_probe(&closure, &gcfg.workspace, &cfg, &syn_cfg, &opts, &[wrong.to_string()]);
    assert!(matches!(&r, Ok(run) if run.outcome.findings.is_empty()), "{wrong}: {r:?}");
    // Probe paths can never relabel or shadow a table entry, nor inject TOML.
    for bad in ["f64::mul_add", "std::x\", reason = \"KNP1-LIBM", ""] {
        let e = check_run_with_driver_probe(&closure, &gcfg.workspace, &cfg, &syn_cfg, &opts, &[bad.to_string()])
            .unwrap_err();
        assert_eq!(e.code, TypedErrorCode::ConfigInvalid, "{bad}: {e}");
    }
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
    // The workspace's .cargo/config.toml sets a missing wrapper, a missing rustc and a bad
    // codegen flag: the check must override them, or nothing builds.
    let run = check_run(&closure, &gcfg.workspace, &cfg, &syn_cfg, &opts).unwrap();
    let out = run.outcome;
    for f in out.findings.iter().chain(&out.notes) {
        f.validate().unwrap();
    }
    // AC-2..AC-4 and the bypass cases, hand-labelled; the aliased HashMap is checked below.
    let mut got: Vec<String> = lines(&out.findings).into_iter().filter(|l| !l.contains(" typed-alias ")).collect();
    got.dedup();
    assert_eq!(got, text_lines(&fix("ws/expected-findings.txt")), "{:#?}", out.findings);
    assert_eq!(lines(&out.notes), text_lines(&fix("ws/expected-notes.txt")));
    // AC-4: the build failure names the crate and the target.
    let b =
        out.findings.iter().find(|f| f.rule == Rule::Typed("BUILD".into()) && f.crate_name == "typed-nowasm").unwrap();
    assert!(b.message.contains("typed-nowasm") && b.message.contains("wasm32-unknown-unknown"), "{}", b.message);
    // #235 follow-up: a real E0277 ("expected an `FnMut(f64)` closure, found `f64`") is a build
    // finding for its crate; it did not abort the run as KNP1-TYPED-DRIVER.
    let e =
        out.findings.iter().find(|f| f.rule == Rule::Typed("BUILD".into()) && f.crate_name == "typed-e0277").unwrap();
    assert!(e.message.contains("compile error"), "{}", e.message);
    // REQ-004: the aliased HashMap is seen.
    let alias: Vec<&Finding> = out.findings.iter().filter(|f| f.crate_name == "typed-alias").collect();
    assert!(!alias.is_empty() && alias.iter().all(|f| f.rule == Rule::Hash), "{alias:#?}");
    assert!(alias.iter().all(|f| f.line == 3 || f.line == 6), "{alias:#?}");
    // AC-3: nothing for the clean crate.
    assert!(out.findings.iter().all(|f| f.crate_name != "typed-clean"));
}
