#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Snapshot tests of the report renderer (REQ-VIZ.SHELL.SPKTREE-005, AC-5) against
//! hand-written expected files in `fixtures/viz/tree-10k/`.

use fid_hal_idl::UntrustedText;
use fid_spike_ui::report::{render, timings_csv, Environment, Outcome, Results};
use fid_spike_ui::script::Phase;
use std::collections::BTreeMap;

fn env(hardware: &str, adapter: &str, h1: bool) -> Environment {
    Environment {
        hardware: UntrustedText::new(hardware),
        adapter: UntrustedText::new(adapter),
        h1,
        egui: "0.33.3".into(),
        wgpu: "27.0.1".into(),
        winit: "0.30.13".into(),
    }
}

fn expected(name: &str) -> String {
    let p = format!("{}/../../fixtures/viz/tree-10k/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(p).unwrap()
}

/// AC-5: failing criteria say "fail" with the measured value, and the recommendation points
/// to D-005's option B.
#[test]
fn ac5_failing_report_snapshot() {
    let mut series = BTreeMap::new();
    series.insert(Phase::Scroll, vec![10.0, 40.0, 20.0]);
    series.insert(Phase::ExpandAll, vec![6.0, 5.0]);
    series.insert(Phase::FilterTyping, vec![33.0]);
    let r = Results {
        env: env("Test rig | 16 GB \u{202e}", "iGPU (Vulkan)", false),
        series,
        keyboard: Some((47, 48)),
        screen_reader: Outcome::NotRun,
        limitations: vec!["Rows announce\u{200b} only when visible".into()],
    };
    assert_eq!(render(&r), expected("expected-fail.txt"));
    assert_eq!(render(&r), render(&r.clone()));
}

#[test]
fn pending_report_snapshot() {
    let r = Results {
        env: env("H1", "-", true),
        series: BTreeMap::new(),
        keyboard: None,
        screen_reader: Outcome::NotRun,
        limitations: vec![],
    };
    assert_eq!(render(&r), expected("expected-pending.txt"));
}

#[test]
fn passing_report_has_no_fallback() {
    let mut series = BTreeMap::new();
    for p in [Phase::Scroll, Phase::ExpandAll, Phase::CollapseAll, Phase::FilterTyping] {
        series.insert(p, vec![1.0, 2.0]);
    }
    let r = Results {
        env: env("x", "y", true),
        series,
        keyboard: Some((48, 48)),
        screen_reader: Outcome::Pass,
        limitations: vec![],
    };
    let text = render(&r);
    assert!(text.contains("Overall: **pass**"));
    assert!(!text.contains("option B"));
    assert!(text.contains("Every criterion passes."));
}

#[test]
fn csv_lists_every_sample() {
    let mut series = BTreeMap::new();
    series.insert(Phase::Scroll, vec![1.5, 2.25]);
    series.insert(Phase::ExpandAll, vec![3.0]);
    assert_eq!(timings_csv(&series), "series,index,ms\nexpand-all,0,3.000000\nscroll,0,1.500000\nscroll,1,2.250000\n");
}
