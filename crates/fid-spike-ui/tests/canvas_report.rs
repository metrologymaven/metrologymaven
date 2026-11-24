#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Snapshot tests of the canvas report renderer (REQ-VIZ.SHELL.SPKRPT-005, AC-5) against
//! hand-written expected files in `fixtures/viz/report-canvas-200/`. Every number below is
//! synthetic test input, not a measurement (D-045).

use fid_hal_idl::UntrustedText;
use fid_spike_ui::report::{Environment, Outcome};
use fid_spike_ui::rpt::report::{render, timings_csv, Content, Results};
use fid_spike_ui::rpt::script::Phase;
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
    let p = format!("{}/../../fixtures/viz/report-canvas-200/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(p).unwrap().replace("\r\n", "\n")
}

const FULL: Content = Content { pages: 5, blocks: 200, tables: 32, frames: 1 };

/// AC-5: failing criteria say "fail" with the measured value, and the recommendation points
/// to D-005 option B or a web panel for the designer only.
#[test]
fn ac5_failing_report_snapshot() {
    let mut series = BTreeMap::new();
    series.insert(Phase::Drag, vec![10.0, 40.0, 20.0]);
    series.insert(Phase::Zoom, vec![6.0, 5.0]);
    series.insert(Phase::Typing, vec![33.0]);
    let r = Results {
        env: env("Synthetic test input | not a measurement \u{202e}", "iGPU (Vulkan)", false),
        fonts: vec!["C:\\Windows\\Fonts\\segoeui.ttf".into(), "C:\\Windows\\Fonts\\YuGothM.ttc".into()],
        content: Some(FULL),
        series,
        drag_seconds: Some(10.25),
        keyboard: Some((108, 109)),
        ime: Outcome::NotRun,
        screen_reader: Outcome::Pass,
        limitations: vec!["Blocks announce\u{200b} only on visible pages".into()],
    };
    assert_eq!(render(&r), expected("expected-fail.txt"));
    assert_eq!(render(&r), render(&r.clone()));
}

#[test]
fn pending_report_snapshot() {
    let r = Results {
        env: env("-", "-", true),
        fonts: vec![],
        content: None,
        series: BTreeMap::new(),
        drag_seconds: None,
        keyboard: None,
        ime: Outcome::NotRun,
        screen_reader: Outcome::NotRun,
        limitations: vec![],
    };
    assert_eq!(render(&r), expected("expected-pending.txt"));
}

#[test]
fn wrong_content_fails_and_passing_report_has_no_fallback() {
    let mut series = BTreeMap::new();
    for p in [Phase::Drag, Phase::Zoom, Phase::Typing] {
        series.insert(p, vec![1.0, 2.0]);
    }
    let mut r = Results {
        env: env("x", "y", true),
        fonts: vec!["f".into()],
        content: Some(FULL),
        series,
        drag_seconds: Some(10.0),
        keyboard: Some((109, 109)),
        ime: Outcome::Pass,
        screen_reader: Outcome::Pass,
        limitations: vec![],
    };
    let text = render(&r);
    assert!(text.contains("Overall: **pass**"));
    assert!(!text.contains("option B"));
    assert!(text.contains("Every criterion passes."));
    r.content = Some(Content { frames: 0, ..FULL });
    let text = render(&r);
    assert!(text.contains("Overall: **fail**"));
    assert!(text.contains("- REQ-001 content: fail, measured 5 pages, 200 blocks, 32 tables, 3D frames: 0."));
    assert!(text.contains("option B"));
}

/// Review of #577: a drag shorter than 10 s (or of unknown length) does not count for AC-1.
#[test]
fn short_drag_is_not_run() {
    let mut series = BTreeMap::new();
    for p in [Phase::Drag, Phase::Zoom, Phase::Typing] {
        series.insert(p, vec![1.0, 2.0]);
    }
    let mut r = Results {
        env: env("x", "y", true),
        fonts: vec!["f".into()],
        content: Some(FULL),
        series,
        drag_seconds: Some(4.5),
        keyboard: Some((109, 109)),
        ime: Outcome::Pass,
        screen_reader: Outcome::Pass,
        limitations: vec![],
    };
    let row = "| AC-1 drag a block for 10 s, p95 frame time | 2.000 ms (n = 2) over 4.500 s | < 33 ms | not run (needs 10 s) |";
    let text = render(&r);
    assert!(text.contains(row), "{text}");
    assert!(text.contains("Overall: **incomplete**"));
    assert!(text.contains("Incomplete: 1 criteria not run."));
    r.drag_seconds = None;
    assert!(render(&r).contains("| 2.000 ms (n = 2) over an unknown time | < 33 ms | not run (needs 10 s) |"));
    r.drag_seconds = Some(f64::NAN);
    assert!(render(&r).contains("not run (needs 10 s)"));
    // A slow drag that still lasted 10 s or more is judged, so a failing machine fails.
    r.drag_seconds = Some(12.0);
    r.series.insert(Phase::Drag, vec![50.0]);
    let text = render(&r);
    assert!(text.contains("| 50.000 ms (n = 1) over 12.000 s | < 33 ms | fail |"), "{text}");
    assert!(text.contains("Overall: **fail**"));
}

#[test]
fn csv_lists_every_sample() {
    let mut series = BTreeMap::new();
    series.insert(Phase::Typing, vec![1.5, 2.25]);
    series.insert(Phase::Drag, vec![3.0]);
    assert_eq!(timings_csv(&series), "series,index,ms\ndrag,0,3.000000\ntyping,0,1.500000\ntyping,1,2.250000\n");
}
