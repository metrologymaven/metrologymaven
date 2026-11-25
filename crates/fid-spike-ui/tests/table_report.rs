#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Snapshot tests of the table report renderer (REQ-VIZ.SHELL.SPKTABLE-005, AC-5) against
//! hand-written expected files in `fixtures/viz/char-table-100k/`. Every number below is
//! synthetic test input, not a measurement (D-045).

use fid_hal_idl::UntrustedText;
use fid_spike_ui::report::{Environment, Outcome};
use fid_spike_ui::table::report::{render, timings_csv, Content, Results};
use fid_spike_ui::table::script::Phase;
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
    let p = format!("{}/../../fixtures/viz/char-table-100k/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(p).unwrap().replace("\r\n", "\n")
}

const FULL: Content = Content { rows: 100_000, columns: 14, frozen: 2, chips: true };

/// AC-5: failing criteria say "fail" with the measured value, a failed screen-reader check
/// says "fail", and the recommendation points to D-005 option B for this screen.
#[test]
fn ac5_failing_report_snapshot() {
    let mut series = BTreeMap::new();
    series.insert(Phase::Scroll, vec![10.0, 40.0, 20.0]);
    series.insert(Phase::Sort, vec![6.0, 5.0]);
    series.insert(Phase::Filter, vec![33.0]);
    let r = Results {
        env: env("Synthetic test input | not a measurement \u{202e}", "iGPU (Vulkan)", false),
        content: Some(FULL),
        series,
        scroll_seconds: Some(30.25),
        sort_shown: Some(0.5),
        filter_shown: Some(0.125),
        exact: Some("0.05".into()),
        keyboard: Some((107, 108)),
        screen_reader: Outcome::Fail,
        limitations: vec!["Cells announce\u{200b} only on screen".into()],
    };
    assert_eq!(render(&r), expected("expected-fail.txt"));
    assert_eq!(render(&r), render(&r.clone()));
}

#[test]
fn pending_report_snapshot() {
    let r = Results {
        env: env("-", "-", true),
        content: None,
        series: BTreeMap::new(),
        scroll_seconds: None,
        sort_shown: None,
        filter_shown: None,
        exact: None,
        keyboard: None,
        screen_reader: Outcome::NotRun,
        limitations: vec![],
    };
    assert_eq!(render(&r), expected("expected-pending.txt"));
}

#[test]
fn short_scroll_is_not_run_and_passing_report_has_no_fallback() {
    let mut series = BTreeMap::new();
    for p in [Phase::Scroll, Phase::Sort, Phase::Filter] {
        series.insert(p, vec![1.0, 2.0]);
    }
    let mut r = Results {
        env: env("x", "y", true),
        content: Some(FULL),
        series,
        scroll_seconds: Some(30.0),
        sort_shown: Some(0.75),
        filter_shown: Some(0.25),
        exact: Some("0.0500".into()),
        keyboard: Some((108, 108)),
        screen_reader: Outcome::Pass,
        limitations: vec![],
    };
    let pass = render(&r);
    assert!(pass.contains("Overall: **pass**"), "{pass}");
    assert!(!pass.contains("option B"));
    r.scroll_seconds = Some(29.999);
    let short = render(&r);
    assert!(short.contains("Overall: **incomplete**"));
    assert!(short.contains("| AC-1 scroll end to end for 30 s, p95 frame time | 2.000 ms (n = 2) over 29.999 s | < 33 ms | not run (needs 30 s) |"), "{short}");
    r.scroll_seconds = None;
    assert!(render(&r).contains("over an unknown time | < 33 ms | not run (needs 30 s) |"));
    r.scroll_seconds = Some(30.0);
    r.content = Some(Content { frozen: 1, ..FULL });
    let wrong = render(&r);
    assert!(wrong.contains("Overall: **fail**") && wrong.contains("REQ-001 content: fail"), "{wrong}");
    // A non-finite or negative sample voids the series rather than passing it.
    r.content = Some(FULL);
    r.series.insert(Phase::Sort, vec![1.0, f64::NAN]);
    assert!(render(&r).contains("| AC-2 sort by deviation: UI-thread frame time while the background sort is pending, p95 | - | < 33 ms | not run |"));
}

/// AC-2: a sort or filter whose result was never shown is "not run (result not shown)",
/// however good its frame times were.
#[test]
fn result_not_shown_snapshot() {
    let mut series = BTreeMap::new();
    for p in [Phase::Scroll, Phase::Sort, Phase::Filter] {
        series.insert(p, vec![1.0, 2.0]);
    }
    let r = Results {
        env: env("x", "y", true),
        content: Some(FULL),
        series,
        scroll_seconds: Some(30.0),
        sort_shown: None,
        filter_shown: Some(f64::NAN),
        exact: Some("0.0500".into()),
        keyboard: Some((95, 95)),
        screen_reader: Outcome::Pass,
        limitations: vec![],
    };
    assert_eq!(render(&r), expected("expected-not-shown.txt"));
}

#[test]
fn timings_csv_lists_every_sample() {
    let mut series = BTreeMap::new();
    series.insert(Phase::Filter, vec![1.5]);
    series.insert(Phase::Scroll, vec![2.0, 3.25]);
    assert_eq!(timings_csv(&series), "series,index,ms\nscroll,0,2.000000\nscroll,1,3.250000\nfilter,0,1.500000\n");
}
