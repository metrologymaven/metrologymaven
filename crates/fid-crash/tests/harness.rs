//! Acceptance criteria AC-1, AC-2 and AC-4 of CORE.STO.SQL.CRASH: the seeded kill harness
//! against fid-store's transactions, its self-test with a writer that acknowledges before
//! COMMIT, and reproduction of the failure from the printed seed and instant. AC-3 (power
//! loss) runs on Linux in `.github/workflows/crash-power-loss.yml`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use fid_crash::adapter::{StoreAdapter, WriterMode};
use fid_crash::{run, Config, FailureKind, KillInstant};
use std::process::Command;

/// Kills per run: 1,000 (AC-1); `FID_CRASH_KILLS` may raise or lower it for local runs.
fn kills() -> u64 {
    std::env::var("FID_CRASH_KILLS").ok().and_then(|v| v.parse().ok()).unwrap_or(1000)
}

/// AC-1: seed 1, 1,000 kills on this OS, every check passes.
#[test]
fn ac1_seed_1_every_check_passes() {
    let t = TempDir::new("ac1");
    let config = Config::new(1, kills(), t.0.join("work"), writer("correct", &[]));
    let mut log = Vec::new();
    let started = std::time::Instant::now();
    let result = run(&config, &StoreAdapter::new(WriterMode::Correct), &mut log);
    let log = String::from_utf8(log).unwrap();
    eprintln!("{log}took {:?}", started.elapsed());
    let summary = result.unwrap_or_else(|f| panic!("harness failed: {f}\n{log}"));
    assert_eq!(summary.kills, kills());
    // No acknowledgement was lost on the way to the harness: every row a check found is either
    // acknowledged or the one commit that was in flight when its writer was killed.
    assert!(summary.max_unacked <= 1, "{} unacknowledged commits after one kill", summary.max_unacked);
    assert_eq!(summary.recovered, summary.acked + summary.unacked);
    // Liveness, bounded on the work done rather than on a per-OS commit rate: the kills landed
    // both after acknowledged commits and inside the window between a commit taking effect and
    // its acknowledgement, not only before the first commit. (Fixed floors of kills / 10, then
    // kills / 40, assumed a commit rate per kill; Windows runners acknowledged 82, 88 and 91
    // while every check passed, because FlushFileBuffers can outlast the 30 ms delay range.)
    assert!(summary.acked > 0, "no commit was acknowledged before a kill");
    assert!(summary.unacked > 0, "no kill landed between a commit and its acknowledgement");
}

/// AC-2 and AC-4 through the command line: the broken writer is caught with seed 1 within
/// 1,000 kills, the seed and instant are printed, and re-running that instant reproduces the
/// same failure (D2: the instant is re-tried on fresh stores, up to 50 times).
#[test]
fn ac2_ack_before_commit_is_caught_and_ac4_reproduces_it() {
    let t = TempDir::new("ac2");
    let out = Command::new(BIN)
        .args(["run", "--mode", "ack-before-commit", "--seed", "1", "--kills", "1000", "--dir"])
        .arg(t.0.join("run"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert_eq!(out.status.code(), Some(1), "{stdout}");
    let fail = stdout.lines().find(|l| l.starts_with("FAIL ")).unwrap_or_else(|| panic!("no FAIL line: {stdout}"));
    assert!(fail.starts_with("FAIL ack_missing: "), "{fail}");
    assert!(fail.contains("(seed 1, instant "), "{fail}");
    assert!(stdout.contains("bus error: internal/invariant_violated"), "{stdout}");
    let repro = stdout.lines().find_map(|l| l.strip_prefix("reproduce: --seed 1 --instant ")).unwrap();
    let instant = KillInstant::parse(repro.trim()).unwrap();
    assert!(instant.kill < 1000);

    let out = Command::new(BIN)
        .args(["run", "--mode", "ack-before-commit", "--seed", "1", "--attempts", "50", "--instant"])
        .arg(repro.trim())
        .arg("--dir")
        .arg(t.0.join("repro"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert_eq!(out.status.code(), Some(1), "did not reproduce: {stdout}");
    assert!(stdout.contains("FAIL ack_missing: "), "{stdout}");
    assert!(stdout.contains(&format!("instant {instant}")), "{stdout}");
}

/// The library run reports the same failure kind, seed and instant.
#[test]
fn library_run_reports_seed_and_instant() {
    let t = TempDir::new("lib");
    let config = Config::new(1, 1000, t.0.join("work"), writer("ack-before-commit", &[]));
    let mut log = Vec::new();
    let f = run(&config, &StoreAdapter::new(WriterMode::AckBeforeCommit), &mut log).unwrap_err();
    assert_eq!(f.kind, FailureKind::AckMissing, "{f}");
    assert_eq!(f.seed, Some(1));
    let instant = f.instant.unwrap();
    assert_eq!(instant, fid_crash::KillPlan { seed: 1, max_delay_us: config.max_delay_us }.instant(instant.kill));
    assert_refused(&f, "ack before commit");
}

/// Bad command lines are usage errors (exit 2), and power-loss mode exists only on Linux.
#[test]
fn command_line_errors() {
    for args in [
        vec!["run", "--kills", "x"],
        vec!["run", "--mode", "nope"],
        vec!["run", "--instant", "12"],
        vec!["run", "--bogus", "1"],
        vec!["writer", "--mode", "correct"],
        vec!["nothing"],
    ] {
        let out = Command::new(BIN).args(&args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
    if !cfg!(target_os = "linux") {
        let out = Command::new(BIN)
            .args(["run", "--power-loss", "--dm-name", "x", "--dm-dev", "y", "--dm-sectors", "1", "--mount", "z"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2));
    }
}
