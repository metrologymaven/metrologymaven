//! The canvas spike's results report (REQ-VIZ.SHELL.SPKRPT-005, AC-5): environment, pass or
//! fail per criterion, recommendation, frame-time summary, the manual IME and screen-reader
//! logs and limitations. Rendering is deterministic: identical inputs give identical bytes.
//!
//! Nothing here measures anything: every number comes from the caller's [`Results`], and a
//! criterion without data is "not run" (D-045: claimed values never look measured).

use super::script::Phase;
use crate::report::{Environment, Outcome};
use crate::stats::{summarise, Summary, THRESHOLD_MS};
use fid_hal_idl::UntrustedText;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// What the prototype rendered (REQ-VIZ.SHELL.SPKRPT-001), counted from the loaded canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Content {
    /// Pages.
    pub pages: usize,
    /// Blocks.
    pub blocks: usize,
    /// Table blocks.
    pub tables: usize,
    /// Live 3D frames.
    pub frames: usize,
}

impl Content {
    /// Whether it is what REQ-001 asks for: 5 pages, 200 blocks, tables and one 3D frame.
    pub fn passes(&self) -> bool {
        self.pages == 5 && self.blocks == 200 && self.tables > 0 && self.frames == 1
    }
}

/// Everything the report states.
#[derive(Debug, Clone, PartialEq)]
pub struct Results {
    /// Environment (hardware, adapter, H1 flag, toolkit versions).
    pub env: Environment,
    /// Font files loaded, primary first, then the fallbacks (paths as given).
    pub fonts: Vec<String>,
    /// What was rendered; `None` if the harness did not run.
    pub content: Option<Content>,
    /// Frame times in milliseconds per recorded phase.
    pub series: BTreeMap<Phase, Vec<f64>>,
    /// How long the drag phase really lasted, in seconds, by the harness's monotonic clock.
    /// AC-1 asks for at least [`DRAG_SECONDS`]; a shorter (or unknown) drag is "not run".
    pub drag_seconds: Option<f64>,
    /// Keyboard sequence: (steps that matched, total steps); `None` if not run.
    pub keyboard: Option<(usize, usize)>,
    /// Manual IME check (AC-4), as recorded by the person.
    pub ime: Outcome,
    /// Manual screen-reader check (REQ-005), as recorded by the person.
    pub screen_reader: Outcome,
    /// Observed limitations, one per bullet.
    pub limitations: Vec<String>,
}

/// Drag length AC-1 asks for, in seconds.
pub const DRAG_SECONDS: f64 = 10.0;

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
    /// Appended to the result word (for "not run (needs 10 s)").
    note: &'static str,
}

const TIMED: [(Phase, &str, &str); 3] = [
    (Phase::Drag, "AC-1 drag a block for 10 s, p95 frame time", "AC-1 drag"),
    (Phase::Zoom, "AC-2 zoom 25 % to 400 %, p95 frame time", "AC-2 zoom"),
    (Phase::Typing, "AC-2 type 200 characters, p95 frame time", "AC-2 typing"),
];

fn ms(v: f64) -> String {
    format!("{v:.3} ms")
}

fn rows(r: &Results) -> (Vec<Row>, Vec<(Phase, Summary)>) {
    let mut out = Vec::new();
    let mut sums = Vec::new();
    let (measured, outcome) = match r.content {
        Some(c) => (
            format!("{} pages, {} blocks, {} tables, 3D frames: {}", c.pages, c.blocks, c.tables, c.frames),
            if c.passes() { Outcome::Pass } else { Outcome::Fail },
        ),
        None => ("-".to_string(), Outcome::NotRun),
    };
    out.push(Row {
        label: "REQ-001 content loaded",
        short: "REQ-001 content",
        measured,
        threshold: "5 pages, 200 blocks, tables, 1 3D frame".to_string(),
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
        if phase == Phase::Drag && s.is_some() {
            let d = r.drag_seconds.filter(|d| d.is_finite());
            match d {
                Some(d) => {
                    let _ = write!(measured, " over {d:.3} s");
                }
                None => measured.push_str(" over an unknown time"),
            }
            if !d.is_some_and(|d| d >= DRAG_SECONDS) {
                outcome = Outcome::NotRun;
                note = " (needs 10 s)";
            }
        }
        out.push(Row { label, short, measured, threshold: format!("< {THRESHOLD_MS:.0} ms"), outcome, note });
    }
    let (measured, outcome) = match r.keyboard {
        Some((ok, total)) => {
            (format!("{ok} of {total} steps"), if ok == total && total > 0 { Outcome::Pass } else { Outcome::Fail })
        }
        None => ("-".to_string(), Outcome::NotRun),
    };
    out.push(Row {
        label: "AC-3 keyboard move and resize",
        short: "AC-3 keyboard",
        measured,
        threshold: "all steps".to_string(),
        outcome,
        note: "",
    });
    out.push(Row {
        label: "AC-4 Japanese IME commit (manual)",
        short: "AC-4 IME",
        measured: "see log".to_string(),
        threshold: "committed text in the block".to_string(),
        outcome: r.ime,
        note: "",
    });
    out.push(Row {
        label: "REQ-005 screen-reader announcements (manual)",
        short: "REQ-005 screen reader",
        measured: "see log".to_string(),
        threshold: "type, name and position for 5 blocks".to_string(),
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
    s.push_str("# VIZ.SHELL.SPKRPT results: report designer canvas\n\n");
    let _ = writeln!(s, "Overall: **{overall}**\n");
    s.push_str("## Environment\n\n");
    let _ = writeln!(s, "- Hardware: {}", r.env.hardware.as_display());
    let _ = writeln!(s, "- GPU adapter: {}", r.env.adapter.as_display());
    if r.env.h1 {
        s.push_str("- Profile: PLAT H1 reference machine\n");
    } else {
        s.push_str("- Profile: not the PLAT H1 reference machine; timings are [unverified] for H1\n");
    }
    let _ = writeln!(s, "- Versions: egui {}, wgpu {}, winit {}", r.env.egui, r.env.wgpu, r.env.winit);
    if r.fonts.is_empty() {
        s.push_str("- Fonts: none recorded\n\n");
    } else {
        let fonts: Vec<String> = r.fonts.iter().map(|f| UntrustedText::new(f).as_display()).collect();
        let _ = writeln!(s, "- Fonts (primary, then fallbacks): {}\n", fonts.join("; "));
    }
    s.push_str("## Criteria\n\n| Criterion | Measured | Threshold | Result |\n|---|---|---|---|\n");
    for x in &rows {
        let _ = writeln!(s, "| {} | {} | {} | {}{} |", x.label, x.measured, x.threshold, word(x.outcome), x.note);
    }
    s.push_str("\n## Recommendation\n\n");
    let failing: Vec<&Row> = rows.iter().filter(|x| x.outcome == Outcome::Fail).collect();
    let not_run = rows.iter().filter(|x| x.outcome == Outcome::NotRun).count();
    if !failing.is_empty() {
        for x in failing {
            let _ = writeln!(s, "- {}: fail, measured {}.", x.short, x.measured);
        }
        s.push_str(
            "\nFor the report designer canvas, fall back to D-005 option B, or to a web panel for the \
             designer only (the concept doc's migration triggers). The spike adopts nothing; the \
             maintainers confirm or reopen D-005.\n",
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
        "\n## Manual IME check (AC-4)\n\nOn Windows with a Japanese IME, select a text block, press \
         Enter, compose そくてい and commit it. Then compose again and press Escape before \
         committing.\n\n- Input method and version:\n- Committed text shown in the block:\n- Escape \
         during composition cancels the composition without ending the edit (yes or no):\n",
    );
    s.push_str(
        "\n## Manual screen-reader check (REQ-005)\n\nScreen reader and version:\n\n\
         | Block | Type announced | Name announced | Position announced |\n|---|---|---|---|\n",
    );
    for i in 1..=5 {
        let _ = writeln!(s, "| {i} | | | |");
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
