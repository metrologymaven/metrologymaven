//! Acceptance tests AC-1, AC-2 and AC-4 of spec node `AUTO.CLI.TREE` (help listing, help
//! goldens, edit-distance suggestions), which run the `fid` binary. Moved unchanged from
//! `crates/fid-cli/tests/tree.rs` with the binary (AUTO.SURF.COMPOSE AC-9); AC-3, AC-5 and the
//! embedded-tree test stay in `fid-cli`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fid_canon::{parse, CValue};
use fid_cli::{levenshtein, RESERVED_HOST_SUBCOMMANDS};
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
