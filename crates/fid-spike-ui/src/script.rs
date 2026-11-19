//! Scripted inputs: the keyboard sequence of AC-3 (read from the fixture) and the
//! benchmark schedule of AC-1 and AC-2 (scroll, expand-all / collapse-all, filter typing).

use crate::view::Action;
use std::fmt;

/// Largest accepted keyboard script in bytes.
pub const MAX_SCRIPT_BYTES: usize = 1024 * 1024;
/// Largest accepted number of steps.
pub const MAX_STEPS: usize = 10_000;

/// One scripted keyboard step and what must hold after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// 1-based step number.
    pub n: usize,
    /// The action.
    pub action: Action,
    /// Expected focus.
    pub focus: Option<u32>,
    /// Expected selection, ascending.
    pub selection: Vec<u32>,
    /// Expected raw name of the focused item (`None` with no focus).
    pub name: Option<String>,
}

/// Why a keyboard script was rejected (1-based line numbers).
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
            ScriptError::TooLarge => write!(f, "keyboard script too large"),
            ScriptError::Line(l) => write!(f, "keyboard script line {l}: malformed"),
        }
    }
}

impl std::error::Error for ScriptError {}

fn action(key: &str, arg: &str) -> Option<Action> {
    let a = match key {
        "Down" => Action::Down,
        "Up" => Action::Up,
        "Home" => Action::Home,
        "End" => Action::End,
        "Right" => Action::Right,
        "Left" => Action::Left,
        "ShiftDown" => Action::ShiftDown,
        "ShiftUp" => Action::ShiftUp,
        "CtrlDown" => Action::CtrlDown,
        "CtrlUp" => Action::CtrlUp,
        "Space" => Action::Space,
        "F2" => Action::F2,
        "Enter" => Action::Enter,
        "Escape" => Action::Escape,
        "ExpandAll" => Action::ExpandAll,
        "CollapseAll" => Action::CollapseAll,
        "Type" => return Some(Action::Type(arg.to_string())),
        "Filter" => return Some(Action::Filter(arg.to_string())),
        _ => return None,
    };
    arg.is_empty().then_some(a)
}

fn id(s: &str) -> Option<u32> {
    let v: u32 = s.parse().ok()?;
    (v > 0 && v.to_string() == s).then_some(v)
}

/// Parses `keyboard.tsv`: `#` comments, then
/// `step<TAB>key<TAB>arg<TAB>focus<TAB>selection<TAB>name` with `-` for none and the
/// selection as ascending comma-separated ids.
pub fn parse_keyboard(text: &str) -> Result<Vec<Step>, ScriptError> {
    if text.len() > MAX_SCRIPT_BYTES {
        return Err(ScriptError::TooLarge);
    }
    let mut steps = Vec::new();
    let body = text.strip_suffix('\n').unwrap_or(text);
    for (k, line) in body.split('\n').enumerate() {
        let ln = k + 1;
        if line.starts_with('#') {
            continue;
        }
        if steps.len() >= MAX_STEPS {
            return Err(ScriptError::TooLarge);
        }
        let f: Vec<&str> = line.splitn(6, '\t').collect();
        let bad = ScriptError::Line(ln);
        let [n, key, arg, focus, sel, name] = f.as_slice() else { return Err(bad) };
        if *n != (steps.len() + 1).to_string() {
            return Err(bad);
        }
        let action = action(key, arg).ok_or(bad.clone())?;
        let focus = if *focus == "-" { None } else { Some(id(focus).ok_or(bad.clone())?) };
        let selection = if *sel == "-" {
            Vec::new()
        } else {
            let v = sel.split(',').map(id).collect::<Option<Vec<u32>>>().ok_or(bad.clone())?;
            if !v.windows(2).all(|w| w[0] < w[1]) {
                return Err(bad);
            }
            v
        };
        let name = match (focus, *name) {
            (None, "-") => None,
            (Some(_), nm) if !nm.is_empty() => Some(nm.to_string()),
            _ => return Err(bad),
        };
        steps.push(Step { n: steps.len() + 1, action, focus, selection, name });
    }
    Ok(steps)
}

/// A benchmark phase. Each records its own frame-time series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    /// Frames not recorded (font atlas, pipeline and buffer upload settle).
    Warmup,
    /// Expand-all frames (AC-2).
    ExpandAll,
    /// Collapse-all frames (AC-2).
    CollapseAll,
    /// Continuous scrolling with every item visible (AC-1).
    Scroll,
    /// Typing into the filter box, one character per frame (REQ-VIZ.SHELL.SPKTREE-002).
    FilterTyping,
}

impl Phase {
    /// Series name used in the report and the CSV.
    pub fn name(self) -> &'static str {
        match self {
            Phase::Warmup => "warmup",
            Phase::ExpandAll => "expand-all",
            Phase::CollapseAll => "collapse-all",
            Phase::Scroll => "scroll",
            Phase::FilterTyping => "filter-typing",
        }
    }
}

/// What the harness does in one frame, before the UI runs.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Nothing; the frame is not recorded.
    Idle,
    /// Apply an action and record the frame under the phase.
    Act(Phase, Action),
    /// Scroll so that this row is at the top and record the frame under [`Phase::Scroll`].
    ScrollTo(usize),
    /// The scripted run is over.
    Done,
}

/// Benchmark schedule. Frame-driven except for the scroll phase, which lasts a wall-clock
/// duration measured by the caller's monotonic clock (`scroll_elapsed`).
#[derive(Debug, Clone, PartialEq)]
pub struct Schedule {
    /// Unrecorded warm-up frames at the start.
    pub warmup_frames: usize,
    /// Expand-all / collapse-all pairs.
    pub expand_cycles: usize,
    /// Rows advanced per scroll frame.
    pub scroll_rows_per_frame: usize,
    /// Scroll phase length in seconds (AC-1: 30 s).
    pub scroll_seconds: f64,
    /// Text typed into the filter, then erased, one character per frame.
    pub filter_text: String,
    /// Typing repetitions.
    pub filter_cycles: usize,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule {
            warmup_frames: 60,
            expand_cycles: 100,
            scroll_rows_per_frame: 3,
            scroll_seconds: 30.0,
            filter_text: "cylinder 12".to_string(),
            filter_cycles: 20,
        }
    }
}

/// Position in a [`Schedule`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cursor {
    frame: usize,
    scroll_step: usize,
    scroll_done: bool,
}

impl Schedule {
    fn filter_frames(&self) -> usize {
        let chars = self.filter_text.chars().count();
        // Type each character, then erase back to empty.
        2 * chars * self.filter_cycles
    }

    fn filter_action(&self, k: usize) -> Action {
        let chars: Vec<char> = self.filter_text.chars().collect();
        let len = chars.len().max(1);
        let in_cycle = k % (2 * len);
        let typed = if in_cycle < len { in_cycle + 1 } else { 2 * len - in_cycle - 1 };
        Action::Filter(chars.iter().take(typed).collect())
    }

    /// The command for the next frame. `rows` is the current number of visible rows and
    /// `scroll_elapsed` the seconds since the scroll phase began (0 before it starts).
    pub fn next(&self, c: &mut Cursor, rows: usize, scroll_elapsed: f64) -> Command {
        let f = c.frame;
        let expand_end = self.warmup_frames + 2 * self.expand_cycles;
        let cmd = if f < self.warmup_frames {
            Command::Idle
        } else if f < expand_end {
            let k = f - self.warmup_frames;
            if k % 2 == 0 {
                Command::Act(Phase::ExpandAll, Action::ExpandAll)
            } else {
                Command::Act(Phase::CollapseAll, Action::CollapseAll)
            }
        } else if f == expand_end {
            // Unrecorded set-up frame: everything visible for the scroll phase.
            Command::Act(Phase::Warmup, Action::ExpandAll)
        } else if !c.scroll_done {
            if scroll_elapsed >= self.scroll_seconds {
                c.scroll_done = true;
                c.frame = expand_end + 1;
                return self.next_filter(c);
            }
            let top = if rows == 0 { 0 } else { (c.scroll_step * self.scroll_rows_per_frame) % rows };
            c.scroll_step += 1;
            Command::ScrollTo(top)
        } else {
            return self.next_filter(c);
        };
        c.frame += 1;
        cmd
    }

    fn next_filter(&self, c: &mut Cursor) -> Command {
        let start = self.warmup_frames + 2 * self.expand_cycles + 1;
        let k = c.frame - start;
        c.frame += 1;
        if k < self.filter_frames() {
            Command::Act(Phase::FilterTyping, self.filter_action(k))
        } else {
            Command::Done
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_lines() {
        let s = parse_keyboard("# c\n1\tDown\t\t2\t2\tB\n2\tFilter\tx y\t-\t1,3\t-\n").unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].action, Action::Down);
        assert_eq!(s[1].action, Action::Filter("x y".into()));
        assert_eq!(s[1].selection, vec![1, 3]);
        assert_eq!(s[1].name, None);
        for bad in [
            "1\tDown\t\t2\t2",
            "2\tDown\t\t2\t2\tB",
            "1\tDown\tx\t2\t2\tB",
            "1\tJump\t\t2\t2\tB",
            "1\tDown\t\t0\t2\tB",
            "1\tDown\t\t2\t3,2\tB",
            "1\tDown\t\t2\t2,2\tB",
            "1\tDown\t\t-\t2\tB",
            "1\tDown\t\t2\t2\t",
        ] {
            assert_eq!(parse_keyboard(bad), Err(ScriptError::Line(1)), "{bad:?}");
        }
    }

    #[test]
    fn schedule_hand_labelled() {
        let s = Schedule {
            warmup_frames: 1,
            expand_cycles: 1,
            scroll_rows_per_frame: 4,
            scroll_seconds: 1.0,
            filter_text: "ab".into(),
            filter_cycles: 1,
        };
        let mut c = Cursor::default();
        let f = |a: &str| Action::Filter(a.into());
        assert_eq!(s.next(&mut c, 10, 0.0), Command::Idle);
        assert_eq!(s.next(&mut c, 10, 0.0), Command::Act(Phase::ExpandAll, Action::ExpandAll));
        assert_eq!(s.next(&mut c, 10, 0.0), Command::Act(Phase::CollapseAll, Action::CollapseAll));
        assert_eq!(s.next(&mut c, 10, 0.0), Command::Act(Phase::Warmup, Action::ExpandAll));
        assert_eq!(s.next(&mut c, 10, 0.0), Command::ScrollTo(0));
        assert_eq!(s.next(&mut c, 10, 0.2), Command::ScrollTo(4));
        assert_eq!(s.next(&mut c, 10, 0.4), Command::ScrollTo(8));
        assert_eq!(s.next(&mut c, 10, 0.6), Command::ScrollTo(2));
        assert_eq!(s.next(&mut c, 0, 0.8), Command::ScrollTo(0));
        assert_eq!(s.next(&mut c, 10, 1.0), Command::Act(Phase::FilterTyping, f("a")));
        assert_eq!(s.next(&mut c, 10, 1.0), Command::Act(Phase::FilterTyping, f("ab")));
        assert_eq!(s.next(&mut c, 10, 1.0), Command::Act(Phase::FilterTyping, f("a")));
        assert_eq!(s.next(&mut c, 10, 1.0), Command::Act(Phase::FilterTyping, f("")));
        assert_eq!(s.next(&mut c, 10, 1.0), Command::Done);
        assert_eq!(s.next(&mut c, 10, 1.0), Command::Done);
    }
}
