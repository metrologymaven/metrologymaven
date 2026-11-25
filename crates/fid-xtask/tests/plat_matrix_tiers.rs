//! Acceptance criteria of spec node PLAT.MATRIX.POLICY.DEFS (AC-1 .. AC-3). Expected
//! findings are hand-labelled from the spec's rules and written at the top of each fixture.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_xtask::plat_matrix_tiers::{lint, Column, Tier, TierError, DEFAULT_PATH};
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    root().join("fixtures/plat/matrix").join(name)
}

fn lint_file(path: &Path) -> Vec<TierError> {
    lint(&std::fs::read_to_string(path).unwrap())
}

/// Runs `fid-xtask plat-matrix lint-tiers [file]` from the workspace root (what
/// `cargo xtask plat-matrix lint-tiers` runs); returns (exit code, stdout).
fn cli(file: Option<&Path>) -> (i32, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fid-xtask"));
    cmd.current_dir(root()).args(["plat-matrix", "lint-tiers"]);
    if let Some(f) = file {
        cmd.arg(f);
    }
    let out = cmd.output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap())
}

/// AC-1: the shipped document passes, through the library and the command.
#[test]
fn ac1_shipped_document_passes() {
    assert_eq!(lint_file(&root().join(DEFAULT_PATH)), vec![]);
    let (code, out) = cli(None);
    assert_eq!((code, out.as_str()), (0, "lint-tiers: ok\n"));
}

/// REQ-002: the shipped rows say what the spec requires of each tier.
#[test]
fn req2_shipped_content() {
    let text = std::fs::read_to_string(root().join(DEFAULT_PATH)).unwrap();
    let row = |label: &str| {
        let prefix = format!("| {label} |");
        text.lines().find(|l| l.starts_with(&prefix)).unwrap().to_lowercase()
    };
    let t1 = row("Tier 1");
    assert!(t1.contains("full test suite on every release"), "{t1}");
    assert!(t1.contains("blocks the release"), "{t1}");
    let t2 = row("Tier 2");
    assert!(t2.contains("builds on every release"), "{t2}");
    assert!(t2.contains("release notes") && t2.contains("does not block"), "{t2}");
    let t3 = row("Tier 3");
    assert!(t3.contains("no ci job"), "{t3}");
    assert!(t3.contains("community-maintained"), "{t3}");
}

/// AC-2: the table after the next heading does not count.
#[test]
fn ac2_missing_tier2() {
    assert_eq!(lint_file(&fixture("tiers-no-tier2.md")), vec![TierError::MissingTier { tier: Tier::Tier2 }]);
    let (code, out) = cli(Some(&fixture("tiers-no-tier2.md")));
    assert_eq!((code, out.as_str()), (1, "MissingTier: tier2\n"));
}

/// AC-3: a blank cell and an absent trailing cell are both empty.
#[test]
fn ac3_empty_cell() {
    assert_eq!(
        lint_file(&fixture("tiers-empty-cell.md")),
        vec![
            TierError::EmptyCell { tier: Tier::Tier2, column: Column::SupportResponse },
            TierError::EmptyCell { tier: Tier::Tier3, column: Column::PromotionDemotion },
        ]
    );
}

/// REQ-003: a missing column.
#[test]
fn req3_missing_column() {
    assert_eq!(
        lint_file(&fixture("tiers-missing-column.md")),
        vec![TierError::MissingColumn { column: Column::ReleaseBlocking }]
    );
}

/// REQ-003: no table in the section, and no section at all.
#[test]
fn req3_no_table() {
    assert_eq!(lint_file(&fixture("tiers-no-table.md")), vec![TierError::NoTable]);
    assert_eq!(lint("# Tiers\n\n| Tier |\n|---|\n| Tier 1 |\n"), vec![TierError::NoTable]);
}

/// Review follow-up: tables and headings inside fenced code blocks are ignored.
#[test]
fn fenced_blocks_are_ignored() {
    assert_eq!(lint_file(&fixture("tiers-fenced.md")), vec![TierError::MissingTier { tier: Tier::Tier3 }]);
}

/// Review follow-up: a second row for the same tier fails.
#[test]
fn duplicate_tier_row() {
    assert_eq!(lint_file(&fixture("tiers-duplicate.md")), vec![TierError::DuplicateTier { tier: Tier::Tier2 }]);
    let (code, out) = cli(Some(&fixture("tiers-duplicate.md")));
    assert_eq!((code, out.as_str()), (1, "DuplicateTier: tier2\n"));
}

/// Review follow-up: cells holding only `-`, `**`, a dash or code/emphasis marks are unfilled.
#[test]
fn placeholder_cells_are_unfilled() {
    assert_eq!(
        lint_file(&fixture("tiers-placeholder-cells.md")),
        vec![
            TierError::EmptyCell { tier: Tier::Tier1, column: Column::CiObligation },
            TierError::EmptyCell { tier: Tier::Tier2, column: Column::ReleaseBlocking },
            TierError::EmptyCell { tier: Tier::Tier3, column: Column::SupportResponse },
            TierError::EmptyCell { tier: Tier::Tier3, column: Column::PromotionDemotion },
        ]
    );
}

/// Review follow-up: setext headings (`---` and `===` underlines) end the section.
#[test]
fn setext_heading_ends_section() {
    assert_eq!(lint_file(&fixture("tiers-setext.md")), vec![TierError::NoTable]);
    let text = "## Tiers\n\nOther\n=====\n\n| Tier |\n|---|\n| Tier 1 |\n";
    assert_eq!(lint(text), vec![TierError::NoTable]);
}

/// Review follow-up: the local `Tier` names must match `fid_platform::matrix::Tier`
/// (`fid-xtask` cannot depend on that `tool` crate, so the source is read instead; the module
/// lives in `fid-platform-core` and `fid-platform` re-exports it).
#[test]
fn tier_names_match_fid_platform() {
    let src = std::fs::read_to_string(root().join("crates/fid-platform-core/src/matrix.rs")).unwrap();
    let line = src.lines().map(str::trim).find(|l| l.starts_with("Tier {")).expect("Tier enum in matrix.rs");
    let names: Vec<&str> = line.split('"').skip(1).step_by(2).collect();
    let ours: Vec<&str> = Tier::ALL.iter().map(|t| t.name()).collect();
    assert_eq!(names, ours, "{line}");
}

/// Without a Tier column no row can be matched: only the missing columns are reported.
#[test]
fn no_tier_column() {
    let text = "## Tiers\n\n| Level | CI obligation | Release-blocking | Support response | Promotion/demotion |\n\
                |---|---|---|---|---|\n| Tier 1 | a | b | c | d |\n";
    assert_eq!(lint(text), vec![TierError::MissingColumn { column: Column::Tier }]);
}

/// Bad usage and unreadable files exit 2.
#[test]
fn usage_and_io_errors() {
    let out = Command::new(env!("CARGO_BIN_EXE_fid-xtask"))
        .current_dir(root())
        .args(["plat-matrix", "lint-tiers", "a", "b"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(cli(Some(&fixture("no-such-file.md"))).0, 2);
}
