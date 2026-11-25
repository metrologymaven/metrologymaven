//! Acceptance tests for spec node `AUTO.CLI.TREE`: AC-3 (`--major` dispatch), AC-5 (source-scan
//! lint) and the embedded tree. AC-1, AC-2 and AC-4 run the `fid` binary, which is built by
//! `fid-compose` (AUTO.SURF.COMPOSE), so they live in `crates/fid-compose/tests/tree.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use fid_canon::CValue;
use fid_cli::{embedded_tree, emit, invoke_with_tree, parse_tree, Bus, ExitCode, Output};
use fid_surface::{project, Envelope, ErrorCode, Outcome};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> Vec<u8> {
    fs::read(repo_root().join(rel)).unwrap()
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
