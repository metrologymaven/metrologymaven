//! Acceptance criteria of spec node CORE.BUS.LINT (AC-1 .. AC-4) and its requirements.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_xtask::crate_graph::{check, run, run_with_warnings, warnings, Allowlist};
use fid_xtask::Graph;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const FIXTURES: [&str; 6] = ["clean", "side_door", "sqlite_leak", "surface_core", "surface_store", "optional_dep"];

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/core/bus/lint").join(name)
}

fn expected(name: &str) -> Vec<String> {
    std::fs::read_to_string(fixture(name).join("expected.txt")).unwrap().lines().map(String::from).collect()
}

fn findings(name: &str) -> Vec<String> {
    let ws = fixture(name);
    run(&ws, &ws.join("lint-allowlist.toml"), &ws.join("storage-writers.toml"))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Runs `cargo xtask lint-crate-graph` (the built binary) on a fixture: (exit code, stdout).
fn cli(name: &str) -> (i32, Vec<u8>) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
        .arg("lint-crate-graph")
        .arg("--workspace")
        .arg(fixture(name))
        .output()
        .unwrap();
    (out.status.code().unwrap(), out.stdout)
}

fn stdout_lines(out: &[u8]) -> Vec<String> {
    String::from_utf8(out.to_vec()).unwrap().lines().map(String::from).collect()
}

/// Every fixture's findings match its hand-labelled expected.txt.
#[test]
fn fixtures_match_expected() {
    for name in FIXTURES {
        assert_eq!(findings(name), expected(name), "{name}");
    }
}

/// AC-1
#[test]
fn ac1_clean_exits_0() {
    assert!(expected("clean").is_empty());
    let (code, out) = cli("clean");
    assert_eq!(code, 0);
    assert_eq!(stdout_lines(&out), ["lint-crate-graph: ok"]);
}

/// AC-2
#[test]
fn ac2_side_door_prints_path() {
    let (code, out) = cli("side_door");
    assert_eq!(code, 1);
    let lines = stdout_lines(&out);
    assert_eq!(lines, expected("side_door"));
    assert!(lines.iter().any(|l| l.ends_with(" cli -> helper -> fid-state")), "{lines:?}");
}

/// AC-3
#[test]
fn ac3_sqlite_leak_names_crate() {
    let (code, out) = cli("sqlite_leak");
    assert_eq!(code, 1);
    let lines = stdout_lines(&out);
    assert_eq!(lines, expected("sqlite_leak"));
    assert!(lines.iter().all(|l| l.starts_with("fid-viewer: ")), "{lines:?}");
}

/// AC-4, and REQ-005: two runs give byte-identical output.
#[test]
fn ac4_runs_are_byte_identical() {
    for name in FIXTURES {
        let first = cli(name);
        let second = cli(name);
        assert_eq!(first, second, "{name}");
        let lines = stdout_lines(&first.1);
        let mut sorted = lines.clone();
        sorted.sort();
        assert_eq!(lines, sorted, "{name}: not sorted by crate name");
    }
}

/// REQ-004: a surface may depend on no CORE crate but the bus API.
#[test]
fn req4_surface_depends_on_core() {
    let (code, out) = cli("surface_core");
    assert_eq!(code, 1);
    assert_eq!(stdout_lines(&out), ["cli: surface_dep cli -> fid-bus"]);
}

/// REQ-004 is transitive: a surface may not reach a CORE crate through a non-CORE helper,
/// even when that CORE crate is a door for the state and SQLite rules.
#[test]
fn req4_surface_reaches_core_through_helper() {
    let (code, out) = cli("surface_store");
    assert_eq!(code, 1);
    assert_eq!(stdout_lines(&out), ["cli: surface_dep cli -> helper -> fid-store"]);
}

/// Optional (feature-gated) dependencies are checked: the lint resolves with
/// `--all-features`, while the default resolve (as `layers` uses) does not see the edge.
#[test]
fn optional_dependencies_are_checked() {
    let ws = fixture("optional_dep");
    assert_eq!(findings("optional_dep"), ["fid-viewer: sqlite_driver fid-viewer -> rusqlite"]);
    let a = Allowlist::load(&ws.join("lint-allowlist.toml"), &ws.join("storage-writers.toml")).unwrap();
    let default_features = Graph::load(&ws).unwrap();
    assert!(check(&a, &default_features).is_empty());
}

/// A mutable-state crate missing from the graph is a warning on stderr, not a finding.
#[test]
fn missing_state_crate_is_a_warning() {
    let ws = fixture("clean");
    let (f, w) = run_with_warnings(&ws, &ws.join("lint-allowlist.toml"), &ws.join("storage-writers.toml")).unwrap();
    assert!(f.is_empty() && w.is_empty(), "{f:?} {w:?}");
    let text = std::fs::read_to_string(ws.join("lint-allowlist.toml"))
        .unwrap()
        .replace("crate = \"fid-state\"", "crate = \"fid-gone\"");
    let a = Allowlist::parse(&text, None).unwrap();
    let g = Graph::load(&ws).unwrap();
    assert_eq!(warnings(&a, &g), ["warning: mutable-state crate fid-gone is not in the crate graph"]);
}

/// REQ-003: the storage writers come from storage-writers.toml (CORE.STO.LAYOUT); without
/// it, the store is not a writer: it is reported, and so is the bus, which now reaches the
/// driver through the store (the store is no longer a door for this rule).
#[test]
fn req3_writers_file_is_read() {
    let ws = fixture("clean");
    let got = run(&ws, &ws.join("lint-allowlist.toml"), &ws.join("no-such-writers.toml")).unwrap();
    assert_eq!(
        got,
        ["fid-bus: sqlite_driver fid-bus -> fid-store -> rusqlite", "fid-store: sqlite_driver fid-store -> rusqlite"]
    );
}

#[test]
fn errors_are_typed() {
    let ws = fixture("clean");
    let e = run(&ws, &ws.join("no-such-allowlist.toml"), &ws.join("storage-writers.toml")).unwrap_err();
    assert!(e.starts_with("allowlist_invalid"), "{e}");
    let e = run(&fixture("no-such-workspace"), &ws.join("lint-allowlist.toml"), &ws.join("storage-writers.toml"))
        .unwrap_err();
    assert!(e.starts_with("metadata_unavailable"), "{e}");
    // Errors go to stderr, not stdout.
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
        .args(["lint-crate-graph", "--workspace"])
        .arg(fixture("no-such-workspace"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("metadata_unavailable"));
}

const GOOD: &str = "[mutable_state]\ncrate = \"s\"\nallowed = [\"b\"]\n[sqlite]\ndrivers = [\"d\"]\n\
                    [surfaces]\ncrates = [\"c\"]\nbus_api = \"api\"\n[core]\ncrates = [\"b\", \"api\"]\n";

#[test]
fn allowlist_parses() {
    let a = Allowlist::parse(GOOD, Some("# comment\n[storage_writers]\nallowed = [\"w\"] # trailing\n")).unwrap();
    assert_eq!(a.mutable_state, "s");
    assert_eq!(a.bus_api, "api");
    assert!(a.storage_writers.contains("w") && a.storage_writers.len() == 1);
    assert!(a.core.contains("api") && a.core.contains("b"));
    // Writers listed in the allowlist itself are merged with the file's.
    let both = format!("{GOOD}[storage_writers]\nallowed = [\"x\"]\n");
    let a = Allowlist::parse(&both, Some("[storage_writers]\nallowed = [\"w\"]\n")).unwrap();
    assert_eq!(a.storage_writers.iter().collect::<Vec<_>>(), ["w", "x"]);
    assert!(Allowlist::parse(GOOD, None).unwrap().storage_writers.is_empty());
}

#[test]
fn bad_allowlists_are_refused() {
    for (text, writers, why) in [
        (GOOD.replace("crate = \"s\"\n", ""), None, "missing [mutable_state] crate"),
        (GOOD.replace("bus_api = \"api\"", "bus_api = [\"api\"]"), None, "must be a string"),
        (format!("{GOOD}[extra]\n"), None, "unknown section"),
        (format!("{GOOD}[core]\n"), None, "appears twice"),
        (GOOD.replace("drivers", "driver"), None, "unknown key"),
        (GOOD.replace("[\"d\"]", "[\"d\",\n"), None, "one line"),
        (format!("x = \"y\"\n{GOOD}"), None, "outside a section"),
        (GOOD.replace("[\"d\"]", "[d]"), None, "non-empty strings"),
        (GOOD.to_string(), Some("[storage_writers]\n"), "missing [storage_writers] allowed"),
        (GOOD.to_string(), Some("[storage]\nallowed = []\n"), "unknown section"),
    ] {
        let err = Allowlist::parse(&text, writers).unwrap_err();
        assert!(err.starts_with("allowlist_invalid") && err.contains(why), "{why}: {err}");
    }
}

/// Builds `cargo metadata`-shaped JSON for a synthetic workspace: `n` members, member `i`
/// depending on members `i+1` and `i+2`, the last one on the state crate.
fn synthetic_metadata(n: usize) -> String {
    let id = |i: usize| format!("path+file:///ws/c{i:03}#c{i:03}@0.0.0");
    let mut packages = vec![r#"{"id":"path+file:///ws/state#fid-state@0.0.0","name":"fid-state"}"#.to_string()];
    let mut members = Vec::new();
    let mut nodes = vec![r#"{"id":"path+file:///ws/state#fid-state@0.0.0","deps":[]}"#.to_string()];
    for i in 0..n {
        packages.push(format!(r#"{{"id":"{}","name":"c{i:03}"}}"#, id(i)));
        members.push(format!(r#""{}""#, id(i)));
        let mut deps: Vec<String> = (i + 1..(i + 3).min(n)).map(id).collect();
        if i + 1 == n {
            deps.push("path+file:///ws/state#fid-state@0.0.0".to_string());
        }
        let deps: Vec<String> =
            deps.iter().map(|d| format!(r#"{{"pkg":"{d}","dep_kinds":[{{"kind":null,"target":null}}]}}"#)).collect();
        nodes.push(format!(r#"{{"id":"{}","deps":[{}]}}"#, id(i), deps.join(",")));
    }
    format!(
        r#"{{"packages":[{}],"workspace_members":[{}],"resolve":{{"nodes":[{}]}}}}"#,
        packages.join(","),
        members.join(","),
        nodes.join(",")
    )
}

/// REQ-006 (algorithm part): 200 crates are checked well under 30 s. The `cargo metadata`
/// call itself is not timed here [unverified until measured on a real 200-crate workspace].
#[test]
fn req6_two_hundred_crates() {
    let g = Graph::from_metadata(synthetic_metadata(200).as_bytes()).unwrap();
    let a = Allowlist::parse(GOOD.replace("\"s\"", "\"fid-state\"").as_str(), None).unwrap();
    let start = Instant::now();
    let got = check(&a, &g);
    assert!(start.elapsed().as_secs() < 30);
    assert_eq!(got.len(), 200);
    // Shortest path: 100 dependency hops to c199, then fid-state.
    assert!(got[0].starts_with("c000: mutable_state c000 -> "), "{}", got[0]);
    assert!(got[0].ends_with(" -> c199 -> fid-state"), "{}", got[0]);
    assert_eq!(got[0].matches(" -> ").count(), 101, "{}", got[0]);
    assert_eq!(got[199], "c199: mutable_state c199 -> fid-state");
}

/// The repository's own workspace has no side doors.
#[test]
fn repository_has_no_side_doors() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let got = run(&root, &root.join("lint-allowlist.toml"), &root.join("storage-writers.toml")).unwrap();
    assert!(got.is_empty(), "{got:?}");
}
