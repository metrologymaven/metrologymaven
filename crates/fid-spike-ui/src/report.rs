//! The results report (REQ-VIZ.SHELL.SPKTREE-005, AC-5): environment, pass or fail per
//! criterion, recommendation, frame-time summary, manual screen-reader log and limitations.
//! Rendering is deterministic: identical inputs give identical bytes.

use crate::script::Phase;
use crate::stats::{summarise, Summary, THRESHOLD_MS};
use fid_hal_idl::UntrustedText;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Result of one criterion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Measured and within the criterion.
    Pass,
    /// Measured and outside the criterion.
    Fail,
    /// Not measured (or no valid samples).
    NotRun,
}

impl Outcome {
    fn word(self) -> &'static str {
        match self {
            Outcome::Pass => "pass",
            Outcome::Fail => "fail",
            Outcome::NotRun => "not run",
        }
    }
}

/// Where the run happened. Hardware and adapter strings come from the person running the
/// harness and from the driver, so they are shown escaped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    /// CPU, RAM and OS as stated by the person running the harness.
    pub hardware: UntrustedText,
    /// GPU adapter as reported by wgpu.
    pub adapter: UntrustedText,
    /// Whether the machine is the PLAT H1 reference profile.
    pub h1: bool,
    /// egui version (egui-winit and egui-wgpu share it).
    pub egui: String,
    /// wgpu version.
    pub wgpu: String,
    /// winit version.
    pub winit: String,
}

/// Everything the report states.
#[derive(Debug, Clone, PartialEq)]
pub struct Results {
    /// Environment.
    pub env: Environment,
    /// Frame times in milliseconds per recorded phase.
    pub series: BTreeMap<Phase, Vec<f64>>,
    /// Keyboard sequence: (steps that matched, total steps); `None` if not run.
    pub keyboard: Option<(usize, usize)>,
    /// Manual screen-reader check (AC-4), as recorded by the person.
    pub screen_reader: Outcome,
    /// Observed limitations, one per bullet.
    pub limitations: Vec<String>,
}

struct Row {
    label: &'static str,
    short: &'static str,
    measured: String,
    threshold: String,
    outcome: Outcome,
}

const TIMED: [(Phase, &str, &str); 4] = [
    (Phase::Scroll, "AC-1 scroll, p95 frame time", "AC-1 scroll"),
    (Phase::ExpandAll, "AC-2 expand-all, p95 frame time", "AC-2 expand-all"),
    (Phase::CollapseAll, "AC-2 collapse-all, p95 frame time", "AC-2 collapse-all"),
    (Phase::FilterTyping, "REQ-002 filter typing, p95 frame time", "REQ-002 filter typing"),
];

fn ms(v: f64) -> String {
    format!("{v:.3} ms")
}

fn rows(r: &Results) -> (Vec<Row>, Vec<(Phase, Summary)>) {
    let mut out = Vec::new();
    let mut sums = Vec::new();
    for (phase, label, short) in TIMED {
        let s = r.series.get(&phase).and_then(|v| summarise(v).ok());
        let (measured, outcome) = match s {
            Some(s) => {
                sums.push((phase, s));
                (format!("{} (n = {})", ms(s.p95), s.n), if s.passes() { Outcome::Pass } else { Outcome::Fail })
            }
            None => ("-".to_string(), Outcome::NotRun),
        };
        out.push(Row { label, short, measured, threshold: format!("< {THRESHOLD_MS:.0} ms"), outcome });
    }
    let (measured, outcome) = match r.keyboard {
        Some((ok, total)) => {
            (format!("{ok} of {total} steps"), if ok == total && total > 0 { Outcome::Pass } else { Outcome::Fail })
        }
        None => ("-".to_string(), Outcome::NotRun),
    };
    out.push(Row {
        label: "AC-3 keyboard sequence",
        short: "AC-3 keyboard",
        measured,
        threshold: "all steps".to_string(),
        outcome,
    });
    out.push(Row {
        label: "AC-4 screen reader (manual)",
        short: "AC-4 screen reader",
        measured: "see log".to_string(),
        threshold: "role, name, level, expanded for 5 items".to_string(),
        outcome: r.screen_reader,
    });
    (out, sums)
}

/// Renders `REPORT.md`.
pub fn render(r: &Results) -> String {
    let (rows, sums) = rows(r);
    let overall = if rows.iter().any(|x| x.outcome == Outcome::Fail) {
        "fail"
    } else if rows.iter().any(|x| x.outcome == Outcome::NotRun) {
        "incomplete"
    } else {
        "pass"
    };
    let mut s = String::new();
    s.push_str("# VIZ.SHELL.SPKTREE results: element tree with 10k items\n\n");
    let _ = writeln!(s, "Overall: **{overall}**\n");
    s.push_str("## Environment\n\n");
    let _ = writeln!(s, "- Hardware: {}", r.env.hardware.as_display());
    let _ = writeln!(s, "- GPU adapter: {}", r.env.adapter.as_display());
    if r.env.h1 {
        s.push_str("- Profile: PLAT H1 reference machine\n");
    } else {
        s.push_str("- Profile: not the PLAT H1 reference machine; timings are [unverified] for H1\n");
    }
    let _ = writeln!(s, "- Versions: egui {}, wgpu {}, winit {}\n", r.env.egui, r.env.wgpu, r.env.winit);
    s.push_str("## Criteria\n\n| Criterion | Measured | Threshold | Result |\n|---|---|---|---|\n");
    for x in &rows {
        let _ = writeln!(s, "| {} | {} | {} | {} |", x.label, x.measured, x.threshold, x.outcome.word());
    }
    s.push_str("\n## Recommendation\n\n");
    let failing: Vec<&Row> = rows.iter().filter(|x| x.outcome == Outcome::Fail).collect();
    let not_run = rows.iter().filter(|x| x.outcome == Outcome::NotRun).count();
    if !failing.is_empty() {
        for x in failing {
            let _ = writeln!(s, "- {}: fail, measured {}.", x.short, x.measured);
        }
        s.push_str(
            "\nFall back to D-005 option B for the element tree screen. The spike adopts nothing; \
             the maintainers confirm or reopen D-005.\n",
        );
    } else if not_run > 0 {
        let _ = writeln!(s, "Incomplete: {not_run} criteria not run. No recommendation until every criterion is measured.");
    } else {
        s.push_str(
            "Every criterion passes. This is input for the maintainers' confirmation of D-005; \
             the spike adopts nothing.\n",
        );
    }
    s.push_str("\n## Frame-time summary\n\n");
    if sums.is_empty() {
        s.push_str("No frame times recorded.\n");
    } else {
        s.push_str("| Series | n | min | median | p95 | max |\n|---|---|---|---|---|---|\n");
        for (phase, m) in &sums {
            let _ = writeln!(
                s,
                "| {} | {} | {} | {} | {} | {} |",
                phase.name(),
                m.n,
                ms(m.min),
                ms(m.median),
                ms(m.p95),
                ms(m.max)
            );
        }
        s.push_str("\nRaw per-frame timings: `timings.csv` (series, index, ms).\n");
    }
    s.push_str(
        "\n## Manual screen-reader check (AC-4)\n\nScreen reader and version:\n\n\
         | Item | Role | Name | Level | Expanded |\n|---|---|---|---|---|\n",
    );
    for i in 1..=5 {
        let _ = writeln!(s, "| {i} | | | | |");
    }
    s.push_str("\n## Limitations\n\n");
    if r.limitations.is_empty() {
        s.push_str("None recorded.\n");
    }
    for l in &r.limitations {
        let _ = writeln!(s, "- {}", UntrustedText::new(l).as_display());
    }
    s
}

/// Writes the per-frame timings as CSV: `series,index,ms` with a header line.
pub fn timings_csv(series: &BTreeMap<Phase, Vec<f64>>) -> String {
    let mut s = String::from("series,index,ms\n");
    for (phase, v) in series {
        for (i, t) in v.iter().enumerate() {
            let _ = writeln!(s, "{},{},{:.6}", phase.name(), i, t);
        }
    }
    s
}
