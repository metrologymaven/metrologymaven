//! The table spike's results report (REQ-VIZ.SHELL.SPKTABLE-005): environment, pass or fail
//! per criterion, recommendation, frame-time summary, the manual screen-reader log and
//! limitations. Rendering is deterministic: identical inputs give identical bytes.
//!
//! Nothing here measures anything: every number comes from the caller's [`Results`], and a
//! criterion without data is "not run" (D-045: claimed values never look measured).

use super::script::{Phase, EXACT_TEXT};
use crate::report::{Environment, Outcome};
use crate::stats::{summarise, Summary, THRESHOLD_MS};
use fid_hal_idl::UntrustedText;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// What the prototype rendered (REQ-VIZ.SHELL.SPKTABLE-001), counted from the loaded table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Content {
    /// Rows.
    pub rows: usize,
    /// Columns.
    pub columns: usize,
    /// Frozen columns.
    pub frozen: usize,
    /// Whether every row's status chip has an icon and a text.
    pub chips: bool,
}

impl Content {
    /// Whether it is what REQ-001 asks for: 10^5 rows, 14 columns, 2 frozen, chips.
    pub fn passes(&self) -> bool {
        self.rows == 100_000 && self.columns == 14 && self.frozen == 2 && self.chips
    }
}

/// Everything the report states.
#[derive(Debug, Clone, PartialEq)]
pub struct Results {
    /// Environment (hardware, adapter, H1 flag, toolkit versions).
    pub env: Environment,
    /// What was rendered; `None` if the harness did not run.
    pub content: Option<Content>,
    /// Frame times in milliseconds per recorded phase.
    pub series: BTreeMap<Phase, Vec<f64>>,
    /// How long the scroll phase really lasted, in seconds, by the harness's monotonic
    /// clock. AC-1 asks for at least [`SCROLL_SECONDS`]; a shorter (or unknown) scroll is
    /// "not run".
    pub scroll_seconds: Option<f64>,
    /// AC-2: seconds from requesting the sort by deviation until its result was shown;
    /// `None` if it was never shown (the sort row is then "not run").
    pub sort_shown: Option<f64>,
    /// AC-2: seconds from typing the last filter character until the filter result was
    /// shown; `None` if it was never shown (the filter row is then "not run").
    pub filter_shown: Option<f64>,
    /// AC-3: the text stored after committing "0.0500"; `None` if not run.
    pub exact: Option<String>,
    /// Keyboard sequence: (steps that matched, total steps); `None` if not run.
    pub keyboard: Option<(usize, usize)>,
    /// Manual screen-reader check (AC-5), as recorded by the person.
    pub screen_reader: Outcome,
    /// Observed limitations, one per bullet.
    pub limitations: Vec<String>,
}

/// What a manual criterion's "measured" cell says.
const MANUAL: &str = "see log";

/// Scroll length AC-1 asks for, in seconds.
pub const SCROLL_SECONDS: f64 = 30.0;

fn word(o: Outcome) -> &'static str {
    match o {
        Outcome::Pass => "pass",
        Outcome::Fail => "fail",
        Outcome::NotRun => "not run",
    }
}

struct Row {
    label: &'static str,
    short: &'static str,
    measured: String,
    threshold: String,
    outcome: Outcome,
    /// Appended to the result word (for "not run (needs 30 s)").
    note: &'static str,
}

const TIMED: [(Phase, &str, &str); 3] = [
    (Phase::Scroll, "AC-1 scroll end to end for 30 s, p95 frame time", "AC-1 scroll"),
    (
        Phase::Sort,
        "AC-2 sort by deviation: UI-thread frame time while the background sort is pending, p95",
        "AC-2 sort",
    ),
    (
        Phase::Filter,
        "AC-2 type a 6-character filter: UI-thread frame time while the background filter is pending, p95",
        "AC-2 filter",
    ),
];

fn ms(v: f64) -> String {
    format!("{v:.3} ms")
}

fn rows(r: &Results) -> (Vec<Row>, Vec<(Phase, Summary)>) {
    let mut out = Vec::new();
    let mut sums = Vec::new();
    let (measured, outcome) = match r.content {
        Some(c) => (
            format!(
                "{} rows, {} columns, {} frozen, status chips: {}",
                c.rows,
                c.columns,
                c.frozen,
                if c.chips { "icon and text" } else { "incomplete" }
            ),
            if c.passes() { Outcome::Pass } else { Outcome::Fail },
        ),
        None => ("-".to_string(), Outcome::NotRun),
    };
    out.push(Row {
        label: "REQ-001 content loaded",
        short: "REQ-001 content",
        measured,
        threshold: "100000 rows, 14 columns, 2 frozen, chips with icon and text".to_string(),
        outcome,
        note: "",
    });
    for (phase, label, short) in TIMED {
        let s = r.series.get(&phase).and_then(|v| summarise(v).ok());
        let (mut measured, mut outcome) = match s {
            Some(s) => {
                sums.push((phase, s));
                (format!("{} (n = {})", ms(s.p95), s.n), if s.passes() { Outcome::Pass } else { Outcome::Fail })
            }
            None => ("-".to_string(), Outcome::NotRun),
        };
        let mut note = "";
        if phase == Phase::Scroll && s.is_some() {
            let d = r.scroll_seconds.filter(|d| d.is_finite());
            match d {
                Some(d) => {
                    let _ = write!(measured, " over {d:.3} s");
                }
                None => measured.push_str(" over an unknown time"),
            }
            if !d.is_some_and(|d| d >= SCROLL_SECONDS) {
                outcome = Outcome::NotRun;
                note = " (needs 30 s)";
            }
        }
        let shown = match phase {
            Phase::Sort => Some(r.sort_shown),
            Phase::Filter => Some(r.filter_shown),
            _ => None,
        };
        if let (Some(shown), Some(_)) = (shown, s) {
            match shown.filter(|t| t.is_finite() && *t >= 0.0) {
                Some(t) => {
                    let _ = write!(measured, ", result shown after {t:.3} s");
                }
                None => {
                    measured.push_str(", result not shown");
                    outcome = Outcome::NotRun;
                    note = " (result not shown)";
                }
            }
        }
        out.push(Row { label, short, measured, threshold: format!("< {THRESHOLD_MS:.0} ms"), outcome, note });
    }
    let (measured, outcome) = match &r.exact {
        Some(s) => (
            format!("stored \"{}\"", UntrustedText::new(s).as_display()),
            if s == EXACT_TEXT { Outcome::Pass } else { Outcome::Fail },
        ),
        None => ("-".to_string(), Outcome::NotRun),
    };
    out.push(Row {
        label: "AC-3 exact decimal storage",
        short: "AC-3 exact decimal",
        measured,
        threshold: format!("stored \"{EXACT_TEXT}\""),
        outcome,
        note: "",
    });
    let (measured, outcome) = match r.keyboard {
        Some((ok, total)) => {
            (format!("{ok} of {total} steps"), if ok == total && total > 0 { Outcome::Pass } else { Outcome::Fail })
        }
        None => ("-".to_string(), Outcome::NotRun),
    };
    out.push(Row {
        label: "AC-4 keyboard sequence (model)",
        short: "AC-4 keyboard",
        measured,
        threshold: "all steps".to_string(),
        outcome,
        note: "",
    });
    out.push(Row {
        label: "AC-5 screen-reader announcements (manual)",
        short: "AC-5 screen reader",
        measured: MANUAL.to_string(),
        threshold: "table, headers, value and status for 5 cells".to_string(),
        outcome: r.screen_reader,
        note: "",
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
    s.push_str("# VIZ.SHELL.SPKTABLE results: characteristic table\n\n");
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
        let _ = writeln!(s, "| {} | {} | {} | {}{} |", x.label, x.measured, x.threshold, word(x.outcome), x.note);
    }
    s.push_str("\n## Recommendation\n\n");
    let failing: Vec<&Row> = rows.iter().filter(|x| x.outcome == Outcome::Fail).collect();
    let not_run = rows.iter().filter(|x| x.outcome == Outcome::NotRun).count();
    if !failing.is_empty() {
        for x in failing {
            if x.measured == MANUAL {
                let _ = writeln!(s, "- {}: fail (manual check; see the log below).", x.short);
            } else {
                let _ = writeln!(s, "- {}: fail, measured {}.", x.short, x.measured);
            }
        }
        s.push_str(
            "\nFall back to D-005 option B for the characteristic table screen. The spike adopts \
             nothing; the maintainers confirm or reopen D-005.\n",
        );
    } else if not_run > 0 {
        let _ =
            writeln!(s, "Incomplete: {not_run} criteria not run. No recommendation until every criterion is measured.");
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
        "\n## Manual screen-reader check (AC-5)\n\nOn Windows with NVDA or Narrator, focus a cell \
         and move focus across one row with the arrow keys. Record what is announced for 5 \
         cells; a missing table, header, value or status is a fail.\n\nScreen reader and \
         version:\n\n| Cell | Table announced | Row header | Column header | Value | Status text |\n\
         |---|---|---|---|---|---|\n",
    );
    for i in 1..=5 {
        let _ = writeln!(s, "| {i} | | | | | |");
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
