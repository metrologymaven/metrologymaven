//! Scripted inputs of the canvas spike: the keyboard sequence of AC-3 (read from the
//! fixture, with the state expected after each step), the pointer-drag cases, and the
//! benchmark schedule of AC-1 (drag for 10 s) and AC-2 (zoom 25 % to 400 %, type 200
//! characters).

use super::canvas::{Canvas, Kind, Rect};
use super::edit::{Action, Designer, Guides, ZOOM_MAX, ZOOM_MIN};
use std::fmt;

/// Largest accepted script file in bytes.
pub const MAX_SCRIPT_BYTES: usize = 1024 * 1024;
/// Largest accepted number of steps or cases.
pub const MAX_STEPS: usize = 10_000;

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

/// The designer state expected after a keyboard step.
#[derive(Debug, Clone, PartialEq)]
pub struct Expect {
    /// Focused block.
    pub focus: u32,
    /// Its page.
    pub page: u32,
    /// Its rectangle.
    pub rect: Rect,
    /// Whether a text edit is open.
    pub editing: bool,
    /// Undo entries.
    pub undo: usize,
    /// Redo entries.
    pub redo: usize,
    /// Zoom in percent.
    pub zoom: f32,
    /// The focused block's text (bands and text blocks only).
    pub text: Option<String>,
}

impl Expect {
    /// The same facts read from a designer.
    pub fn of(d: &Designer) -> Option<Expect> {
        let b = d.focused()?;
        Some(Expect {
            focus: b.id,
            page: b.page,
            rect: b.rect,
            editing: d.editing().is_some(),
            undo: d.undo_depth(),
            redo: d.redo_depth(),
            zoom: d.zoom(),
            text: b.kind.editable().then(|| b.content.clone()),
        })
    }
}

/// One scripted keyboard step.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// 1-based step number.
    pub n: usize,
    /// The action.
    pub action: Action,
    /// What must hold after it.
    pub expect: Expect,
}

fn action(key: &str, arg: &str) -> Option<Action> {
    let a = match key {
        "Tab" => Action::Tab,
        "BackTab" => Action::BackTab,
        "PageDown" => Action::PageDown,
        "PageUp" => Action::PageUp,
        "Left" => Action::Left,
        "Right" => Action::Right,
        "Up" => Action::Up,
        "Down" => Action::Down,
        "ShiftLeft" => Action::ShiftLeft,
        "ShiftRight" => Action::ShiftRight,
        "ShiftUp" => Action::ShiftUp,
        "ShiftDown" => Action::ShiftDown,
        "Enter" => Action::Enter,
        "Escape" => Action::Escape,
        "Undo" => Action::Undo,
        "Redo" => Action::Redo,
        "ZoomIn" => Action::ZoomIn,
        "ZoomOut" => Action::ZoomOut,
        "Insert" => return (!arg.is_empty()).then(|| Action::Insert(arg.to_string())),
        "SetText" => return Some(Action::SetText(arg.to_string())),
        _ => return None,
    };
    arg.is_empty().then_some(a)
}

fn int<T: std::str::FromStr + ToString>(s: &str) -> Option<T> {
    let v: T = s.parse().ok()?;
    (v.to_string() == s).then_some(v)
}

fn flag(s: &str) -> Option<bool> {
    match s {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

fn lines(text: &str) -> Result<Vec<(usize, &str)>, ScriptError> {
    if text.len() > MAX_SCRIPT_BYTES {
        return Err(ScriptError::TooLarge);
    }
    let body = text.strip_suffix('\n').unwrap_or(text);
    let out: Vec<(usize, &str)> =
        body.split('\n').enumerate().filter(|(_, l)| !l.starts_with('#')).map(|(k, l)| (k + 1, l)).collect();
    if out.len() > MAX_STEPS {
        return Err(ScriptError::TooLarge);
    }
    Ok(out)
}

/// Parses `keyboard.tsv`: `#` comments, then `step, key, argument, focus, page, x, y, w, h,
/// editing (0/1), undo depth, redo depth, zoom, text` separated by tabs, `-` for no text.
pub fn parse_keyboard(text: &str) -> Result<Vec<Step>, ScriptError> {
    let mut steps = Vec::new();
    for (ln, line) in lines(text)? {
        let bad = ScriptError::Line(ln);
        let f: Vec<&str> = line.splitn(14, '\t').collect();
        let [n, key, arg, focus, page, x, y, w, h, editing, undo, redo, zoom, txt] = f.as_slice() else {
            return Err(bad);
        };
        if *n != (steps.len() + 1).to_string() {
            return Err(bad);
        }
        let parsed = (|| {
            let rect = Rect { x: int(x)?, y: int(y)?, w: int(w)?, h: int(h)? };
            let zoom: u32 = int(zoom)?;
            let expect = Expect {
                focus: int(focus)?,
                page: int(page)?,
                rect,
                editing: flag(editing)?,
                undo: int(undo)?,
                redo: int(redo)?,
                zoom: zoom as f32,
                text: (*txt != "-").then(|| txt.to_string()),
            };
            Some(Step { n: steps.len() + 1, action: action(key, arg)?, expect })
        })();
        steps.push(parsed.ok_or(bad)?);
    }
    Ok(steps)
}

/// Runs the keyboard steps on a fresh designer and returns, per step, whether the state
/// matched.
pub fn run_keyboard(canvas: &Canvas, steps: &[Step]) -> Vec<bool> {
    let mut d = Designer::new(canvas.clone());
    steps
        .iter()
        .map(|s| {
            d.apply(&s.action);
            Expect::of(&d).as_ref() == Some(&s.expect)
        })
        .collect()
}

/// One pointer-drag case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DragCase {
    /// Block dragged.
    pub id: u32,
    /// Raw position (dmm).
    pub raw: (i32, i32),
    /// Whether snapping is on.
    pub snap: bool,
    /// Expected position (dmm).
    pub want: (i32, i32),
    /// Expected guides.
    pub guides: Guides,
}

/// Parses `drag.tsv`: `block, raw x, raw y, snap (0/1), x, y, guide x, guide y`, `-` for
/// no guide.
pub fn parse_drag(text: &str) -> Result<Vec<DragCase>, ScriptError> {
    let mut out = Vec::new();
    for (ln, line) in lines(text)? {
        let f: Vec<&str> = line.split('\t').collect();
        let guide = |s: &str| if s == "-" { Some(None) } else { int(s).map(Some) };
        let parsed = match f.as_slice() {
            [id, rx, ry, snap, x, y, gx, gy] => (|| {
                Some(DragCase {
                    id: int(id)?,
                    raw: (int(rx)?, int(ry)?),
                    snap: flag(snap)?,
                    want: (int(x)?, int(y)?),
                    guides: Guides { x: guide(gx)?, y: guide(gy)? },
                })
            })(),
            _ => None,
        };
        out.push(parsed.ok_or(ScriptError::Line(ln))?);
    }
    Ok(out)
}

/// Runs each drag case from the fixture canvas and returns, per case, whether the
/// position and guides matched.
pub fn run_drag(canvas: &Canvas, cases: &[DragCase]) -> Vec<bool> {
    cases
        .iter()
        .map(|c| {
            let mut d = Designer::new(canvas.clone());
            if !d.begin_drag(c.id) {
                return false;
            }
            let g = d.drag_to(c.raw.0, c.raw.1, c.snap);
            let r = d.canvas().get(c.id).map(|b| (b.rect.x, b.rect.y));
            g == c.guides && r == Some(c.want)
        })
        .collect()
}

/// A benchmark phase. Each records its own frame-time series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    /// Frames not recorded (warm-up and set-up).
    Warmup,
    /// Dragging a block across a page with snapping (AC-1).
    Drag,
    /// Zooming from 25 % to 400 % (AC-2).
    Zoom,
    /// Typing 200 characters into a text block, one per frame (AC-2).
    Typing,
}

impl Phase {
    /// Series name used in the report and the CSV.
    pub fn name(self) -> &'static str {
        match self {
            Phase::Warmup => "warmup",
            Phase::Drag => "drag",
            Phase::Zoom => "zoom",
            Phase::Typing => "typing",
        }
    }
}

/// What the harness does in one frame, before the UI runs.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Nothing; the frame is not recorded.
    Idle,
    /// Drag the block (starting the drag if needed) to the raw position, snapping on; record
    /// under [`Phase::Drag`].
    DragTo(u32, i32, i32),
    /// End the drag (not recorded).
    DragEnd,
    /// Set the zoom (percent) and record under the phase.
    Zoom(Phase, f32),
    /// Focus the block and start editing its text (not recorded).
    Edit(u32),
    /// Apply an action and record under the phase.
    Act(Phase, Action),
    /// The scripted run is over.
    Done,
}

/// The 200 characters typed in AC-2: Latin with CJK mixed in, so the fallback font is laid
/// out every frame. Filler text, not a measurement.
pub fn typing_text() -> String {
    "Lorem ipsum 文字 dolor sit 報告 amet そくてい ".chars().cycle().take(200).collect()
}

/// Benchmark schedule. Frame-driven except for the drag phase, which lasts a wall-clock
/// duration measured by the caller's monotonic clock.
#[derive(Debug, Clone, PartialEq)]
pub struct Schedule {
    /// Unrecorded warm-up frames at the start.
    pub warmup_frames: usize,
    /// Block dragged in AC-1.
    pub drag_block: u32,
    /// Its y (dmm), kept during the drag.
    pub drag_y: i32,
    /// Travel of its left edge across the page (page width minus block width, dmm).
    pub drag_span: i32,
    /// Raw pointer travel per frame (dmm).
    pub drag_step: i32,
    /// Drag phase length in seconds (AC-1: 10 s).
    pub drag_seconds: f64,
    /// Frames of the zoom sweep from [`ZOOM_MIN`] to [`ZOOM_MAX`].
    pub zoom_frames: usize,
    /// Text block typed into.
    pub typing_block: u32,
    /// Text typed, one character per frame.
    pub typing_text: String,
}

impl Schedule {
    /// The AC-1 / AC-2 schedule for a canvas: drag the first text block of page 1 and type
    /// into the second. `None` if page 1 has fewer than two text blocks.
    pub fn for_canvas(canvas: &Canvas, drag_seconds: f64) -> Option<Schedule> {
        let mut texts = canvas.on_page(1).filter(|b| b.kind == Kind::Text);
        let drag = texts.next()?;
        let typing = texts.next()?;
        let (pw, _) = canvas.page_size(1)?;
        Some(Schedule {
            warmup_frames: 60,
            drag_block: drag.id,
            drag_y: drag.rect.y,
            drag_span: pw - drag.rect.w,
            drag_step: 20,
            drag_seconds,
            zoom_frames: 300,
            typing_block: typing.id,
            typing_text: typing_text(),
        })
    }

    /// Raw x of the drag at frame `k` of the phase: a triangle wave between 0 and the span.
    pub fn drag_x(&self, k: usize) -> i32 {
        let span = i64::from(self.drag_span.max(0));
        if span == 0 {
            return 0;
        }
        let t = (k as i64).saturating_mul(i64::from(self.drag_step)).rem_euclid(2 * span);
        let x = if t <= span { t } else { 2 * span - t };
        i32::try_from(x).unwrap_or(0)
    }

    /// Zoom at frame `k` of the sweep: geometric from [`ZOOM_MIN`] to [`ZOOM_MAX`],
    /// endpoints exact.
    pub fn zoom_at(&self, k: usize) -> f32 {
        let n = self.zoom_frames;
        if n < 2 || k == 0 {
            return ZOOM_MIN;
        }
        if k + 1 >= n {
            return ZOOM_MAX;
        }
        let t = k as f64 / (n - 1) as f64;
        (f64::from(ZOOM_MIN) * (f64::from(ZOOM_MAX) / f64::from(ZOOM_MIN)).powf(t)) as f32
    }

    /// The command for the next frame. `drag_elapsed` is the seconds since the drag phase
    /// began (0 before it starts).
    pub fn next(&self, c: &mut Cursor, drag_elapsed: f64) -> Command {
        let k = c.k;
        c.k += 1;
        let advance = |c: &mut Cursor, stage: Stage| {
            c.stage = stage;
            c.k = 0;
        };
        match c.stage {
            Stage::Warmup => {
                if k + 1 >= self.warmup_frames {
                    advance(c, Stage::Drag);
                }
                if k < self.warmup_frames {
                    Command::Idle
                } else {
                    self.next(c, drag_elapsed)
                }
            }
            Stage::Drag => {
                if k > 0 && drag_elapsed >= self.drag_seconds {
                    advance(c, Stage::Zoom);
                    return Command::DragEnd;
                }
                Command::DragTo(self.drag_block, self.drag_x(k), self.drag_y)
            }
            Stage::Zoom => {
                if k >= self.zoom_frames {
                    advance(c, Stage::TypingSetup);
                    return Command::Zoom(Phase::Warmup, 100.0);
                }
                Command::Zoom(Phase::Zoom, self.zoom_at(k))
            }
            Stage::TypingSetup => {
                advance(c, Stage::Typing);
                Command::Edit(self.typing_block)
            }
            Stage::Typing => match self.typing_text.chars().nth(k) {
                Some(ch) => Command::Act(Phase::Typing, Action::Insert(ch.to_string())),
                None => {
                    advance(c, Stage::Done);
                    Command::Act(Phase::Warmup, Action::Escape)
                }
            },
            Stage::Done => Command::Done,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Stage {
    #[default]
    Warmup,
    Drag,
    Zoom,
    TypingSetup,
    Typing,
    Done,
}

/// Position in a [`Schedule`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cursor {
    stage: Stage,
    k: usize,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_lines() {
        let s = parse_keyboard(
            "# c\n1\tRight\t\t4\t1\t50\t0\t100\t60\t0\t1\t0\t100\t-\n\
                                2\tInsert\t a b\t4\t1\t50\t0\t100\t60\t1\t1\t0\t33\tx\ty\n",
        )
        .unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].action, Action::Right);
        assert_eq!(s[0].expect.rect, Rect { x: 50, y: 0, w: 100, h: 60 });
        assert_eq!(s[0].expect.text, None);
        assert_eq!(s[1].action, Action::Insert(" a b".into()));
        assert!(s[1].expect.editing);
        assert_eq!(s[1].expect.zoom, 33.0);
        assert_eq!(s[1].expect.text.as_deref(), Some("x\ty"));
        for bad in [
            "1\tRight\t\t4\t1\t50\t0\t100\t60\t0\t1\t0\t100",
            "2\tRight\t\t4\t1\t50\t0\t100\t60\t0\t1\t0\t100\t-",
            "1\tRight\tx\t4\t1\t50\t0\t100\t60\t0\t1\t0\t100\t-",
            "1\tJump\t\t4\t1\t50\t0\t100\t60\t0\t1\t0\t100\t-",
            "1\tInsert\t\t4\t1\t50\t0\t100\t60\t0\t1\t0\t100\t-",
            "1\tRight\t\t4\t1\t50\t0\t100\t60\t2\t1\t0\t100\t-",
            "1\tRight\t\t04\t1\t50\t0\t100\t60\t0\t1\t0\t100\t-",
            "1\tRight\t\t4\t1\t50\t0\t100\t60\t0\t1\t0\t100.5\t-",
        ] {
            assert_eq!(parse_keyboard(bad), Err(ScriptError::Line(1)), "{bad:?}");
        }
    }

    #[test]
    fn drag_lines() {
        let c = parse_drag("# c\n3\t-5\t7\t1\t0\t0\t0\t-\n").unwrap();
        assert_eq!(
            c[0],
            DragCase { id: 3, raw: (-5, 7), snap: true, want: (0, 0), guides: Guides { x: Some(0), y: None } }
        );
        assert_eq!(parse_drag("3\t-5\t7\t2\t0\t0\t0\t-\n"), Err(ScriptError::Line(1)));
        assert_eq!(parse_drag("3\t-5\t7\t1\t0\t0\t0\n"), Err(ScriptError::Line(1)));
    }

    #[test]
    fn schedule_hand_labelled() {
        let s = Schedule {
            warmup_frames: 1,
            drag_block: 4,
            drag_y: 300,
            drag_span: 50,
            drag_step: 20,
            drag_seconds: 1.0,
            zoom_frames: 3,
            typing_block: 7,
            typing_text: "ab".into(),
        };
        let mut c = Cursor::default();
        assert_eq!(s.next(&mut c, 0.0), Command::Idle);
        assert_eq!(s.next(&mut c, 0.0), Command::DragTo(4, 0, 300));
        assert_eq!(s.next(&mut c, 0.3), Command::DragTo(4, 20, 300));
        assert_eq!(s.next(&mut c, 0.6), Command::DragTo(4, 40, 300));
        // 60 mod 100 = 60 > 50: back to 40, then 20, 0, 20.
        assert_eq!(s.next(&mut c, 0.9), Command::DragTo(4, 40, 300));
        assert_eq!(s.next(&mut c, 1.0), Command::DragEnd);
        assert_eq!(s.next(&mut c, 1.0), Command::Zoom(Phase::Zoom, 25.0));
        // Midpoint of the geometric sweep: 25 * 16^0.5 = 100 (libm rounding aside).
        match s.next(&mut c, 1.0) {
            Command::Zoom(Phase::Zoom, z) => assert!((z - 100.0).abs() < 1e-3, "{z}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(s.next(&mut c, 1.0), Command::Zoom(Phase::Zoom, 400.0));
        assert_eq!(s.next(&mut c, 1.0), Command::Zoom(Phase::Warmup, 100.0));
        assert_eq!(s.next(&mut c, 1.0), Command::Edit(7));
        assert_eq!(s.next(&mut c, 1.0), Command::Act(Phase::Typing, Action::Insert("a".into())));
        assert_eq!(s.next(&mut c, 1.0), Command::Act(Phase::Typing, Action::Insert("b".into())));
        assert_eq!(s.next(&mut c, 1.0), Command::Act(Phase::Warmup, Action::Escape));
        assert_eq!(s.next(&mut c, 1.0), Command::Done);
        assert_eq!(s.next(&mut c, 1.0), Command::Done);
        assert_eq!((s.drag_x(5), s.drag_x(6), s.drag_x(10)), (0, 20, 0));
    }

    #[test]
    fn typing_text_is_200_characters_with_cjk() {
        let t = typing_text();
        assert_eq!(t.chars().count(), 200);
        assert!(t.contains('文') && t.contains('そ'));
    }
}
