//! Acceptance criteria of spec node ECO.CI.BOTS.BLOCK (AC-1 .. AC-5). Fixtures and their
//! expected output under `fixtures/block/` are hand-labelled from the spec's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_xtask::bots::block::check;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fx(p: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/block").join(p)
}

fn expected(name: &str) -> String {
    std::fs::read_to_string(fx(&format!("expected-{name}.txt"))).unwrap()
}

/// Runs `fid-xtask bot-blocking` on a thread export path and returns (exit code, stdout).
fn run_path(threads: &Path) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
        .arg("bot-blocking")
        .arg("--threads")
        .arg(threads)
        .arg("--register")
        .arg(fx("bots.toml"))
        .arg("--maintainers")
        .arg(fx("maintainers.txt"))
        .output()
        .unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap())
}

fn run(name: &str) -> (i32, String) {
    run_path(&fx(&format!("threads-{name}.json")))
}

/// The library and the binary agree with the hand-labelled output.
fn assert_case(name: &str, code: i32) {
    let want = expected(name);
    assert_eq!(run(name), (code, want.clone()), "{name}");
    let report = check(&fx(&format!("threads-{name}.json")), &fx("bots.toml"), &fx("maintainers.txt")).unwrap();
    let lines: String = report.lines().iter().map(|l| format!("{l}\n")).collect();
    assert_eq!(lines, want, "{name}");
    assert_eq!(i32::from(report.exit_code()), code, "{name}");
}

/// AC-1: two bot threads without marks pass.
#[test]
fn ac1_advisory_passes() {
    assert_case("advisory", 0);
}

/// AC-2: a maintainer's `/blocking` on an unresolved bot thread fails and names both.
#[test]
fn ac2_blocking_fails() {
    assert_case("blocking", 1);
    assert!(expected("blocking").starts_with("blocking PRRT_a1 crates/fid-demo/src/lib.rs:12 by ada-quill\n"));
}

/// AC-3: a later `/advisory` by the same maintainer, or resolving the thread, clears it.
#[test]
fn ac3_unmarked_or_resolved_passes() {
    assert_case("unmarked", 0);
    assert_case("resolved", 0);
}

/// AC-4: a non-maintainer's `/blocking` is ignored with one note.
#[test]
fn ac4_non_maintainer_mark_is_a_note() {
    assert_case("non-maintainer", 0);
    let (_, out) = run("non-maintainer");
    assert_eq!(out.lines().filter(|l| l.starts_with("ignored-mark ")).count(), 1);
}

/// AC-5: a truncated export fails closed.
#[test]
fn ac5_truncated_fails_closed() {
    let (code, out) = run("truncated");
    assert_eq!(code, 1);
    assert!(out.starts_with("BLOCK-INPUT threads: not valid JSON"), "{out}");
    assert_eq!(out.lines().count(), 1, "{out}");
}

/// REQ-001..004 together; output sorted by thread ID and byte-identical across runs.
#[test]
fn mixed_sorted_and_deterministic() {
    assert_case("mixed", 1);
    let first = run("mixed");
    assert_eq!(first, run("mixed"));
}

/// REQ-005: missing and oversized exports, and bad register or maintainer inputs, fail closed.
#[test]
fn input_errors_fail_closed() {
    let (code, out) = run("absent");
    assert_eq!(code, 1);
    assert!(out.starts_with("BLOCK-INPUT "), "{out}");

    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("bot-blocking");
    std::fs::create_dir_all(&dir).unwrap();
    // A valid export padded with whitespace to just over 10 MiB.
    let mut big = std::fs::read(fx("threads-advisory.json")).unwrap();
    big.resize(10 * 1024 * 1024 + 1, b' ');
    let big_path = dir.join("big.json");
    std::fs::write(&big_path, &big).unwrap();
    let (code, out) = run_path(&big_path);
    assert_eq!(code, 1);
    assert!(out.starts_with("BLOCK-INPUT ") && out.contains("larger than"), "{out}");
    // Exactly 10 MiB is accepted.
    big.truncate(10 * 1024 * 1024);
    std::fs::write(&big_path, &big).unwrap();
    assert_eq!(run_path(&big_path), (0, expected("advisory")));

    let empty = dir.join("maintainers-empty.txt");
    std::fs::write(&empty, "# nobody\n").unwrap();
    let r = check(&fx("threads-advisory.json"), &fx("bots.toml"), &empty);
    assert!(r.is_err());
    let bad_register = dir.join("bots-bad.toml");
    std::fs::write(&bad_register, "[[bot]]\nid = \"x\"\n").unwrap();
    let r = check(&fx("threads-advisory.json"), &bad_register, &fx("maintainers.txt"));
    assert!(r.unwrap_err().starts_with("register: BOTS-SCHEMA x "));
    // A register without bots would make every thread advisory.
    let no_bots = dir.join("bots-none.toml");
    std::fs::write(&no_bots, "# no bots
").unwrap();
    let r = check(&fx("threads-blocking.json"), &no_bots, &fx("maintainers.txt"));
    assert_eq!(r.unwrap_err(), "register: no bots");
    // Maintainer handles that differ only in case.
    let twice = dir.join("maintainers-twice.txt");
    std::fs::write(&twice, "ada-quill
Ada-Quill
").unwrap();
    assert!(check(&fx("threads-blocking.json"), &fx("bots.toml"), &twice).is_err());
}

#[test]
fn usage_errors() {
    let bin = env!("CARGO_BIN_EXE_fid-xtask");
    let out = Command::new(bin).args(["bot-blocking", "--threads", "x.json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let out = Command::new(bin)
        .args(["bot-blocking", "--threads", "a", "--threads", "b", "--register", "r", "--maintainers", "m"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

/// Logins are case-insensitive: a maintainer list and register in other cases still block.
#[test]
fn accounts_ignore_case() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("bot-blocking-case");
    std::fs::create_dir_all(&dir).unwrap();
    let register = std::fs::read_to_string(fx("bots.toml")).unwrap().replace("fid-review[bot]", "Fid-Review[bot]");
    let register_path = dir.join("bots.toml");
    std::fs::write(&register_path, register).unwrap();
    let maintainers = dir.join("maintainers.txt");
    std::fs::write(&maintainers, "ADA-QUILL
boris-penn
").unwrap();
    let report = check(&fx("threads-blocking.json"), &register_path, &maintainers).unwrap();
    let lines: String = report.lines().iter().map(|l| format!("{l}
")).collect();
    assert_eq!(lines, expected("blocking"));
}

/// The repository's own register is accepted as `--register`.
#[test]
fn repository_register_is_usable() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let report = check(&fx("threads-blocking.json"), &root.join("bots.toml"), &fx("maintainers.txt")).unwrap();
    // fid-review[bot] and lint-x[bot] are not in the repository register, so nothing blocks.
    assert!(report.blocking.is_empty());
}
