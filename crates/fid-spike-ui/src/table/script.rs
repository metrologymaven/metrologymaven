//! Scripted inputs of the table spike: the keyboard sequence of AC-4 (read from the fixture,
//! with the state expected after each step), the exact-decimal check of AC-3, and the
//! benchmark schedule of AC-1 (scroll end to end for 30 s) and AC-2 (sort by deviation, type
//! a 6-character filter).

use super::data::{fnv1a, Column, Table};
use super::grid::{Action, Dir, Grid, Mode};
use std::fmt;

/// Largest accepted script file in bytes.
pub const MAX_SCRIPT_BYTES: usize = 1024 * 1024;
/// Largest accepted number of steps.
pub const MAX_STEPS: usize = 10_000;
/// Fields of the expected state per step.
pub const STATE_FIELDS: usize = 18;

/// Why a script was rejected (1-based line numbers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptError {
    /// Larger than [`MAX_SCRIPT_BYTES`] or more than [`MAX_STEPS`] steps.
    TooLarge,
    /// A line is malformed.
    Line(usize),
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScriptError::TooLarge => write!(f, "script too large"),
            ScriptError::Line(l) => write!(f, "script line {l}: malformed"),
        }
    }
}

impl std::error::Error for ScriptError {}

/// One scripted keyboard step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// 1-based step number.
    pub n: usize,
    /// The action.
    pub action: Action,
    /// The state expected after it, as the fixture writes it (see [`state`]).
    pub expect: Vec<String>,
}

fn action(key: &str, arg: &str) -> Option<Action> {
    let a = match key {
        "Up" => Action::Up,
        "Down" => Action::Down,
        "Left" => Action::Left,
        "Right" => Action::Right,
        "PageUp" => Action::PageUp,
        "PageDown" => Action::PageDown,
        "Home" => Action::Home,
        "End" => Action::End,
        "CtrlHome" => Action::CtrlHome,
        "CtrlEnd" => Action::CtrlEnd,
        "ShiftUp" => Action::ShiftUp,
        "ShiftDown" => Action::ShiftDown,
        "ShiftLeft" => Action::ShiftLeft,
        "ShiftRight" => Action::ShiftRight,
        "SortAsc" => Action::SortAsc,
        "SortDesc" => Action::SortDesc,
        "SortClear" => Action::SortClear,
        "FocusFilter" => Action::FocusFilter,
        "Backspace" => Action::Backspace,
        "Enter" => Action::Enter,
        "F2" => Action::F2,
        "Escape" => Action::Escape,
        "Copy" => Action::Copy,
        "MoveLeft" => Action::MoveLeft,
        "MoveRight" => Action::MoveRight,
        "Narrow" => Action::Narrow,
        "Widen" => Action::Widen,
        "FlyTo" => Action::FlyTo,
        "Type" => return (!arg.is_empty()).then(|| Action::Type(arg.to_string())),
        _ => return None,
    };
    arg.is_empty().then_some(a)
}

/// Parses `keyboard.tsv`: `#` comments, then `step, key, argument` and
/// [`STATE_FIELDS`] state fields separated by tabs.
pub fn parse_keyboard(text: &str) -> Result<Vec<Step>, ScriptError> {
    if text.len() > MAX_SCRIPT_BYTES {
        return Err(ScriptError::TooLarge);
    }
    let body = text.strip_suffix('\n').unwrap_or(text);
    let mut steps = Vec::new();
    for (k, line) in body.split('\n').enumerate() {
        if line.starts_with('#') {
            continue;
        }
        if steps.len() >= MAX_STEPS {
            return Err(ScriptError::TooLarge);
        }
        let bad = ScriptError::Line(k + 1);
        let f: Vec<&str> = line.split('\t').collect();
        let [n, key, arg, rest @ ..] = f.as_slice() else { return Err(bad) };
        if rest.len() != STATE_FIELDS || *n != (steps.len() + 1).to_string() {
            return Err(bad);
        }
        let action = action(key, arg).ok_or(bad)?;
        steps.push(Step { n: steps.len() + 1, action, expect: rest.iter().map(|s| s.to_string()).collect() });
    }
    Ok(steps)
}

/// The grid state in the fixture's encoding: mode, focus row id, focus column, view length,
/// sort, filter, edit buffer, focused cell, status, error, anchor, column order, focused
/// column width, clipboard (`bytes:fnv1a64`), fly-to, header label, cell label and cell
/// description; `-` for none.
pub fn state(g: &Grid, t: &Table) -> Vec<String> {
    let dash = || "-".to_string();
    let row = g.focus_row().and_then(|id| t.get(id));
    let fc = g.focus_col();
    let a11y = g.focus_pos().and_then(|p| g.a11y(t, p, fc));
    let order: Vec<&str> = g.order().iter().map(|c| c.key()).collect();
    vec![
        g.mode().name().to_string(),
        g.focus_row().map_or_else(dash, |r| r.to_string()),
        fc.key().to_string(),
        g.view().len().to_string(),
        g.shown_query().sort.map_or_else(dash, |(c, d)| format!("{}:{}", c.key(), d.name())),
        g.shown_query().filter.clone(),
        if g.mode() == Mode::Edit { g.buffer().to_string() } else { dash() },
        row.map_or_else(dash, |r| r.text(fc).into_owned()),
        row.map_or_else(dash, |r| r.status.word().to_string()),
        g.error().map_or_else(dash, |e| e.name().to_string()),
        g.anchor().map_or_else(dash, |(r, c)| format!("{r}:{}", c.key())),
        order.join(","),
        g.width(fc).to_string(),
        g.clipboard().map_or_else(dash, |c| format!("{}:{:016x}", c.len(), fnv1a(c.as_bytes()))),
        g.fly_to().map_or_else(dash, |r| r.to_string()),
        g.header_label(fc),
        a11y.as_ref().map_or_else(dash, |a| a.label()),
        a11y.as_ref().map_or_else(dash, |a| a.description()),
    ]
}

/// Runs the keyboard steps on a fresh grid over a copy of `table` (with the sort and filter
/// applied synchronously after each step) and returns the state after each step. The page
/// size is [`super::grid::DEFAULT_PAGE_ROWS`], as in the fixture.
pub fn run_keyboard(table: &Table, steps: &[Step]) -> Vec<Vec<String>> {
    let mut t = table.clone();
    let mut g = Grid::new(&t);
    steps
        .iter()
        .map(|s| {
            if g.apply_mut(&mut t, &s.action) {
                g.refresh(&t);
            }
            state(&g, &t)
        })
        .collect()
}

/// AC-3: on a fresh grid, types "0.0500" into the upper tolerance of the first row and
/// commits it. Returns the stored text (`None` if the edit did not commit).
pub fn exact_decimal(table: &Table) -> Option<String> {
    let mut t = table.clone();
    let mut g = Grid::new(&t);
    let target = g.column_pos(Column::Upper);
    for _ in 0..target {
        g.apply_mut(&mut t, &Action::Right);
    }
    g.apply_mut(&mut t, &Action::Type(EXACT_TEXT.to_string()));
    g.apply_mut(&mut t, &Action::Enter);
    if g.mode() != Mode::Grid {
        return None;
    }
    g.focus_row().and_then(|id| t.get(id)).map(|r| r.upper.clone())
}

/// The text AC-3 types.
pub const EXACT_TEXT: &str = "0.0500";

/// A benchmark phase. Each records its own frame-time series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    /// Frames not recorded (warm-up and set-up).
    Warmup,
    /// Scrolling end to end (AC-1).
    Scroll,
    /// From requesting the sort by deviation until it is shown, plus settle frames (AC-2).
    Sort,
    /// Typing the 6-character filter, one character per frame, until the result is shown,
    /// plus settle frames (AC-2).
    Filter,
}

impl Phase {
    /// Series name used in the report and the CSV.
    pub fn name(self) -> &'static str {
        match self {
            Phase::Warmup => "warmup",
            Phase::Scroll => "scroll",
            Phase::Sort => "sort",
            Phase::Filter => "filter",
        }
    }
}

/// What the harness does in one frame, before the UI runs.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Nothing; the frame is not recorded.
    Idle,
    /// Scroll so that this (fractional) row is at the top; record under [`Phase::Scroll`].
    ScrollTo(f64),
    /// Request a sort and record under the phase.
    Sort(Phase, Column, Dir),
    /// Apply an action and record under the phase.
    Act(Phase, Action),
    /// Do nothing but record the frame under the phase (waiting for a background result).
    Await(Phase),
    /// The scripted run is over.
    Done,
}

/// The 6 characters typed in AC-2 (matches "Hole 1..." features).
pub const FILTER_TEXT: &str = "hole 1";

/// Benchmark schedule: frame-driven except the scroll phase, which lasts a wall-clock
/// duration measured by the caller's monotonic clock.
#[derive(Debug, Clone, PartialEq)]
pub struct Schedule {
    /// Unrecorded warm-up frames at the start.
    pub warmup_frames: usize,
    /// Scroll phase length in seconds (AC-1: 30 s).
    pub scroll_seconds: f64,
    /// Frames recorded after a background result is shown.
    pub settle_frames: usize,
    /// Most frames to wait for a background result before moving on.
    pub max_wait_frames: usize,
    /// Text typed into the filter, one character per frame.
    pub filter_text: String,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule {
            warmup_frames: 60,
            scroll_seconds: 30.0,
            settle_frames: 10,
            max_wait_frames: 100_000,
            filter_text: FILTER_TEXT.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Stage {
    #[default]
    Warmup,
    Scroll,
    Sort,
    SortWait,
    FilterOpen,
    FilterType,
    FilterWait,
    Done,
}

/// Position in a [`Schedule`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cursor {
    stage: Stage,
    k: usize,
    settled: usize,
}

impl Schedule {
    /// Top row at `elapsed` seconds into the scroll: linear from the first row to the last
    /// page over [`Schedule::scroll_seconds`], so the whole table passes once.
    pub fn scroll_top(&self, elapsed: f64, rows: usize, visible: usize) -> f64 {
        let span = rows.saturating_sub(visible) as f64;
        if self.scroll_seconds.is_nan() || self.scroll_seconds <= 0.0 || !elapsed.is_finite() {
            return span;
        }
        span * (elapsed / self.scroll_seconds).clamp(0.0, 1.0)
    }

    fn wait(&self, c: &mut Cursor, phase: Phase, busy: bool, next: Stage) -> Command {
        if busy {
            c.settled = 0;
        } else {
            c.settled += 1;
        }
        if c.settled > self.settle_frames || c.k > self.max_wait_frames {
            c.stage = next;
            c.k = 0;
            c.settled = 0;
        }
        Command::Await(phase)
    }

    /// The command for the next frame. `scroll_elapsed` is the seconds since the scroll
    /// phase began (0 before it starts); `busy` is whether a sort or filter is still being
    /// computed or not yet shown.
    pub fn next(&self, c: &mut Cursor, scroll_elapsed: f64, rows: usize, visible: usize, busy: bool) -> Command {
        let k = c.k;
        c.k += 1;
        match c.stage {
            Stage::Warmup => {
                if k + 1 >= self.warmup_frames {
                    c.stage = Stage::Scroll;
                    c.k = 0;
                }
                if k < self.warmup_frames {
                    Command::Idle
                } else {
                    self.next(c, scroll_elapsed, rows, visible, busy)
                }
            }
            Stage::Scroll => {
                if k > 0 && scroll_elapsed >= self.scroll_seconds {
                    c.stage = Stage::Sort;
                    c.k = 0;
                    return Command::ScrollTo(self.scroll_top(self.scroll_seconds, rows, visible));
                }
                Command::ScrollTo(self.scroll_top(scroll_elapsed, rows, visible))
            }
            Stage::Sort => {
                c.stage = Stage::SortWait;
                c.k = 0;
                Command::Sort(Phase::Sort, Column::Deviation, Dir::Asc)
            }
            Stage::SortWait => self.wait(c, Phase::Sort, busy, Stage::FilterOpen),
            Stage::FilterOpen => {
                c.stage = Stage::FilterType;
                c.k = 0;
                Command::Act(Phase::Warmup, Action::FocusFilter)
            }
            Stage::FilterType => match self.filter_text.chars().nth(k) {
                Some(ch) => Command::Act(Phase::Filter, Action::Type(ch.to_string())),
                None => {
                    c.stage = Stage::FilterWait;
                    c.k = 0;
                    self.wait(c, Phase::Filter, busy, Stage::Done)
                }
            },
            Stage::FilterWait => self.wait(c, Phase::Filter, busy, Stage::Done),
            Stage::Done => Command::Done,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_lines() {
        let state = vec!["x"; STATE_FIELDS].join("\t");
        let s = parse_keyboard(&format!("# c\n1\tDown\t\t{state}\n2\tType\t0.5\t{state}\n")).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].action, Action::Down);
        assert_eq!(s[1].action, Action::Type("0.5".into()));
        assert_eq!(s[1].expect.len(), STATE_FIELDS);
        for bad in [
            format!("2\tDown\t\t{state}"),
            format!("1\tDown\tx\t{state}"),
            format!("1\tType\t\t{state}"),
            format!("1\tJump\t\t{state}"),
            format!("1\tDown\t\t{state}\tx"),
            "1\tDown\t".to_string(),
        ] {
            assert_eq!(parse_keyboard(&bad), Err(ScriptError::Line(1)), "{bad:?}");
        }
    }

    #[test]
    fn schedule_hand_labelled() {
        let s = Schedule {
            warmup_frames: 1,
            scroll_seconds: 2.0,
            settle_frames: 1,
            max_wait_frames: 5,
            filter_text: "ab".into(),
        };
        let mut c = Cursor::default();
        assert_eq!(s.next(&mut c, 0.0, 110, 10, false), Command::Idle);
        assert_eq!(s.next(&mut c, 0.0, 110, 10, false), Command::ScrollTo(0.0));
        assert_eq!(s.next(&mut c, 1.0, 110, 10, false), Command::ScrollTo(50.0));
        assert_eq!(s.next(&mut c, 2.5, 110, 10, false), Command::ScrollTo(100.0));
        assert_eq!(s.next(&mut c, 2.5, 110, 10, false), Command::Sort(Phase::Sort, Column::Deviation, Dir::Asc));
        // Busy for two frames, then two idle frames (the second ends the wait).
        assert_eq!(s.next(&mut c, 0.0, 0, 0, true), Command::Await(Phase::Sort));
        assert_eq!(s.next(&mut c, 0.0, 0, 0, true), Command::Await(Phase::Sort));
        assert_eq!(s.next(&mut c, 0.0, 0, 0, false), Command::Await(Phase::Sort));
        assert_eq!(s.next(&mut c, 0.0, 0, 0, false), Command::Await(Phase::Sort));
        assert_eq!(s.next(&mut c, 0.0, 0, 0, false), Command::Act(Phase::Warmup, Action::FocusFilter));
        assert_eq!(s.next(&mut c, 0.0, 0, 0, false), Command::Act(Phase::Filter, Action::Type("a".into())));
        assert_eq!(s.next(&mut c, 0.0, 0, 0, true), Command::Act(Phase::Filter, Action::Type("b".into())));
        assert_eq!(s.next(&mut c, 0.0, 0, 0, true), Command::Await(Phase::Filter));
        assert_eq!(s.next(&mut c, 0.0, 0, 0, false), Command::Await(Phase::Filter));
        assert_eq!(s.next(&mut c, 0.0, 0, 0, false), Command::Await(Phase::Filter));
        assert_eq!(s.next(&mut c, 0.0, 0, 0, false), Command::Done);
        assert_eq!(s.next(&mut c, 0.0, 0, 0, false), Command::Done);
        // A result that never arrives ends the wait after max_wait_frames.
        let mut c = Cursor { stage: Stage::SortWait, k: 0, settled: 0 };
        for _ in 0..6 {
            assert_eq!(s.next(&mut c, 0.0, 0, 0, true), Command::Await(Phase::Sort));
        }
        assert_eq!(s.next(&mut c, 0.0, 0, 0, true), Command::Act(Phase::Warmup, Action::FocusFilter));
        assert_eq!(s.scroll_top(0.0, 5, 10), 0.0);
        assert_eq!(FILTER_TEXT.chars().count(), 6);
    }
}
