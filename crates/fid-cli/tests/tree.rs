//! Acceptance tests AC-1..AC-5 for spec node `AUTO.CLI.TREE` (generated CLI command tree,
//! help rendering, `--major` dispatch, edit-distance suggestions, and source-scan lint).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fid_canon::{parse, CValue};
use fid_cli::{
    embedded_tree, emit, invoke_with_tree, levenshtein, parse_tree, Bus, ExitCode, Output, RESERVED_HOST_SUBCOMMANDS,
};
use fid_surface::{project, Envelope, ErrorCode, Outcome};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> Vec<u8> {
    fs::read(repo_root().join(rel)).unwrap()
}

fn run_fid(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid")).args(args).output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
}

/// AC-1: `fid --help --json` lists exactly the projection's CLI paths for every command in
/// `fixtures/manifest/min-classes.json`, and nothing else besides `mcp` and `ctrl`.
#[test]
fn ac1_help_json_lists_exact_projection_paths_and_host_subcommands() {
    let proj = project(&read("fixtures/manifest/min-classes.json")).unwrap();
    let (code, stdout, stderr) = run_fid(&["--help", "--json"]);
    assert_eq!(code, 0, "stderr: {stderr}");

    let CValue::Obj(root) = parse(stdout.as_bytes()).unwrap() else { panic!("expected JSON object") };
    assert_eq!(root.get("manifest_digest"), Some(&CValue::Str(proj.manifest_digest.as_str().to_string())));

    // Host subcommands are strictly `ctrl` and `mcp`.
    let CValue::Arr(hosts) = &root["host_subcommands"] else { panic!("host_subcommands") };
    let host_names: Vec<&str> = hosts
        .iter()
        .map(|v| match v {
            CValue::Str(s) => s.as_str(),
            _ => panic!("string expected"),
        })
        .collect();
    assert_eq!(host_names, RESERVED_HOST_SUBCOMMANDS);

    // Command paths equal the unique CLI paths from the projection, in byte order.
    let CValue::Arr(cmds) = &root["commands"] else { panic!("commands") };
    let got_paths: Vec<Vec<String>> = cmds
        .iter()
        .map(|v| {
            let CValue::Obj(m) = v else { panic!("command object") };
            let CValue::Arr(p) = &m["path"] else { panic!("path array") };
            p.iter()
                .map(|s| match s {
                    CValue::Str(t) => t.clone(),
                    _ => panic!("path string"),
                })
                .collect()
        })
        .collect();

    let want_paths: Vec<Vec<String>> =
        proj.commands.iter().map(|c| c.cli_path.clone()).collect::<BTreeSet<_>>().into_iter().collect();
    assert_eq!(got_paths, want_paths);

    // `fid --version` prints binary version and embedded manifest digest (REQ-005).
    let (vcode, vout, verr) = run_fid(&["--version"]);
    assert_eq!(vcode, 0, "stderr: {verr}");
    assert!(vout.contains(env!("CARGO_PKG_VERSION")), "{vout}");
    assert!(vout.contains(proj.manifest_digest.as_str()), "{vout}");
}

/// AC-2: `fid align run-bestfit --help` matches golden `fixtures/cli/help/align-run-bestfit.txt`
/// byte-for-byte, and `fid align set-datum --help` matches golden `fixtures/cli/help/align-set-datum.txt`
/// (class `W2`), both naming the manifest digest, side-effect class, and dry-run support.
#[test]
fn ac2_command_help_matches_goldens() {
    let (code, stdout, stderr) = run_fid(&["align", "run-bestfit", "--help"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    let golden = String::from_utf8(read("fixtures/cli/help/align-run-bestfit.txt")).unwrap();
    assert_eq!(stdout, golden);
    assert!(stdout.contains("Manifest: sha256:"));
    assert!(stdout.contains("Side-effect class: R1"));
    assert!(stdout.contains("Dry run: unsupported"));

    let (code2, stdout2, stderr2) = run_fid(&["align", "set-datum", "--help"]);
    assert_eq!(code2, 0, "stderr: {stderr2}");
    let golden2 = String::from_utf8(read("fixtures/cli/help/align-set-datum.txt")).unwrap();
    assert_eq!(stdout2, golden2);
    assert!(stdout2.contains("Side-effect class: W2"));
}

struct RecordingBus {
    called: Vec<String>,
}

impl Bus for RecordingBus {
    fn resolve(&self, _domain: &str, _verb_object: &str) -> Option<String> {
        None
    }

    fn call(&mut self, command_id: &str, _out: &mut Output<'_>) -> Envelope {
        self.called.push(command_id.to_string());
        let mut r = BTreeMap::new();
        r.insert("invoked".to_string(), CValue::Str(command_id.to_string()));
        Envelope::computed(command_id, CValue::Obj(r))
    }
}

/// AC-3: Given `fixtures/manifest/two-majors.json` (`demo.show_thing@1` and `@2`),
/// `fid demo show-thing` invokes major 2 and `fid demo show-thing --major 1` invokes major 1.
#[test]
fn ac3_two_majors_defaults_to_newest_and_honours_major_flag() {
    let proj = project(&read("fixtures/manifest/two-majors.json")).unwrap();
    let emitted = emit(&proj).unwrap();
    let tree = parse_tree(&emitted.bytes).unwrap();

    // Also verify the emitted tree matches fixtures/cli/two-majors.tree.json (modulo _header).
    let fixture_tree = parse_tree(&read("fixtures/cli/two-majors.tree.json")).unwrap();
    assert_eq!(tree, fixture_tree);

    let mut bus = RecordingBus { called: Vec::new() };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    // Plain path invokes newest major (2).
    let args_v2: Vec<String> = ["demo", "show-thing", "--json"].iter().map(|s| (*s).to_string()).collect();
    let rc2 = invoke_with_tree(&tree, "0.0.1", &args_v2, &mut bus, &mut stdout, &mut stderr);
    assert_eq!(rc2, ExitCode::Success, "stderr: {}", String::from_utf8_lossy(&stderr));
    let env2 = Envelope::decode(String::from_utf8_lossy(&stdout).trim_end().as_bytes()).unwrap();
    assert_eq!(env2.outcome, Outcome::Computed);
    assert_eq!(env2.command_id, "demo.show_thing@2");

    // Explicit `--major 1` invokes major 1.
    stdout.clear();
    stderr.clear();
    let args_v1: Vec<String> =
        ["demo", "show-thing", "--major", "1", "--json"].iter().map(|s| (*s).to_string()).collect();
    let rc1 = invoke_with_tree(&tree, "0.0.1", &args_v1, &mut bus, &mut stdout, &mut stderr);
    assert_eq!(rc1, ExitCode::Success, "stderr: {}", String::from_utf8_lossy(&stderr));
    let env1 = Envelope::decode(String::from_utf8_lossy(&stdout).trim_end().as_bytes()).unwrap();
    assert_eq!(env1.outcome, Outcome::Computed);
    assert_eq!(env1.command_id, "demo.show_thing@1");

    assert_eq!(bus.called, vec!["demo.show_thing@2", "demo.show_thing@1"]);

    // Non-existent `--major 9` fails with exit 2 (`usage_error`).
    stdout.clear();
    stderr.clear();
    let args_bad: Vec<String> =
        ["demo", "show-thing", "--major", "9", "--json"].iter().map(|s| (*s).to_string()).collect();
    let rc_bad = invoke_with_tree(&tree, "0.0.1", &args_bad, &mut bus, &mut stdout, &mut stderr);
    assert_eq!(rc_bad, ExitCode::Usage);
    let env_bad = Envelope::decode(String::from_utf8_lossy(&stdout).trim_end().as_bytes()).unwrap();
    assert_eq!(env_bad.error.unwrap().code, ErrorCode::UsageError);
}

/// AC-4 (error path): `fid align run-bestfitt` fails with exit code 2 (`usage_error`)
/// and suggests `run-bestfit`.
#[test]
fn ac4_typo_fails_with_usage_error_and_suggests_nearest_match() {
    let (code, stdout, stderr) = run_fid(&["align", "run-bestfitt"]);
    assert_eq!(code, 2);
    assert_eq!(stdout, "");
    assert!(stderr.contains("run-bestfit"), "stderr must suggest run-bestfit: {stderr}");

    // Same under `--json`: exit 2, single failed envelope with UsageError naming `run-bestfit`.
    let (jcode, jout, _jerr) = run_fid(&["align", "run-bestfitt", "--json"]);
    assert_eq!(jcode, 2);
    let env = Envelope::decode(jout.trim_end().as_bytes()).unwrap();
    assert_eq!(env.outcome, Outcome::Failed);
    let err = env.error.unwrap();
    assert_eq!(err.code, ErrorCode::UsageError);
    assert!(err.message.contains("run-bestfit"), "{}", err.message);

    // Typo on domain also suggests nearest domain.
    let (dcode, _dout, derr) = run_fid(&["algin"]);
    assert_eq!(dcode, 2);
    assert!(derr.contains("align"), "stderr must suggest align: {derr}");

    assert_eq!(levenshtein("run-bestfitt", "run-bestfit"), 1);
}

/// AC-5: Source scan of `crates/fid-cli/src/` proves no string literal equals any registry CLI
/// command path, domain, or verb-object from `min-classes.json` (REQ-AUTO.CLI.TREE-002).
#[test]
fn ac5_no_hardcoded_registry_cli_paths_in_fid_cli_src() {
    let proj = project(&read("fixtures/manifest/min-classes.json")).unwrap();
    let mut forbidden: BTreeSet<String> = BTreeSet::new();
    for cmd in &proj.commands {
        forbidden.insert(cmd.cli_path.join(" "));
        forbidden.insert(cmd.cli_path[1..].join(" "));
        forbidden.insert(cmd.cli_path[2].clone());
        forbidden.insert(cmd.command_id.name().to_string());
    }

    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut checked = 0usize;
    for entry in fs::read_dir(&src_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        checked += 1;
        // Extract every double-quoted string literal in the source file.
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '"' {
                let mut lit = String::new();
                while let Some(ch) = chars.next() {
                    if ch == '\\' {
                        if let Some(esc) = chars.next() {
                            lit.push(esc);
                        }
                        continue;
                    }
                    if ch == '"' {
                        break;
                    }
                    lit.push(ch);
                }
                assert!(
                    !forbidden.contains(&lit),
                    "forbidden hardcoded registry CLI literal {lit:?} found in {}",
                    path.display()
                );
            }
        }
    }
    assert!(checked >= 4, "expected at least 4 Rust source files under crates/fid-cli/src");
}

/// Embedded tree matches `min-classes.json` emission and `parse_tree` rejects malformed input.
#[test]
fn embedded_tree_roundtrip_and_malformed_refusal() {
    let proj = project(&read("fixtures/manifest/min-classes.json")).unwrap();
    let emitted = emit(&proj).unwrap();
    let from_emit = parse_tree(&emitted.bytes).unwrap();
    let from_embedded = embedded_tree().unwrap();
    assert_eq!(from_emit, from_embedded);

    for bad in [&b"not json"[..], b"{}", b"{\"schema\":\"wrong\",\"manifest_digest\":\"sha256:00\",\"domains\":{}}"] {
        assert!(parse_tree(bad).is_err(), "expected error for {bad:?}");
    }
}

#[test]
fn tier5_resolve_args_preserves_flag_values_and_leading_json() {
    let tree = embedded_tree().unwrap();
    let to_vec = |xs: &[&str]| xs.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();

    // --reason --help must NOT trigger CommandHelp.
    match resolve_args(&tree, &to_vec(&["project", "list-features", "--reason", "--help"])) {
        ResolveOutcome::Invoke { command_id, rest, .. } => {
            assert_eq!(command_id, "project.list_features@1");
            assert_eq!(rest, to_vec(&["--reason", "--help"]));
        }
        other => panic!("expected Invoke, got {other:?}"),
    }

    // --reason --major 2 must NOT consume --major as the major selector.
    match resolve_args(&tree, &to_vec(&["project", "list-features", "--reason", "--major", "2"])) {
        ResolveOutcome::Invoke { major, rest, .. } => {
            assert_eq!(major, 1);
            assert_eq!(rest, to_vec(&["--reason", "--major", "2"]));
        }
        other => panic!("expected Invoke, got {other:?}"),
    }

    // Leading --json before domain or ctrl subcommand is accepted.
    match resolve_args(&tree, &to_vec(&["--json", "project", "list-features"])) {
        ResolveOutcome::Invoke { command_id, json, .. } => {
            assert_eq!(command_id, "project.list_features@1");
            assert!(json);
        }
        other => panic!("expected Invoke with json=true, got {other:?}"),
    }
    match resolve_args(&tree, &to_vec(&["--json", "ctrl", "status"])) {
        ResolveOutcome::HostSubcommand { name, rest } => {
            assert_eq!(name, "ctrl");
            assert!(rest.iter().any(|s| s == "--json"));
            assert!(rest.iter().any(|s| s == "status"));
        }
        other => panic!("expected HostSubcommand, got {other:?}"),
    }
}
