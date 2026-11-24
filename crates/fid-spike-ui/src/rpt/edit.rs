//! The designer state over a [`Canvas`]: focus, keyboard move and resize, text editing,
//! pointer drag with snapping to the grid and to guides, zoom, and undo / redo.
//!
//! # Rules (AC-3; `fixtures/viz/report-canvas-200/make_canvas.py` implements them independently)
//!
//! Lengths are dmm (0.1 mm). Grid step [`GRID`] (5 mm), guide tolerance [`GUIDE_TOL`] (2 mm),
//! smallest side [`MIN_SIDE`] (5 mm), margin guides [`MARGIN`] (15 mm) from each page edge.
//!
//! - Focus starts on block 1 and is never empty while the canvas has blocks.
//! - Tab / Shift+Tab: next / previous block id (no wrap). PageDown / PageUp: the lowest block
//!   id on the next / previous page (none: nothing).
//! - Arrows: move the focused block one grid step, clamped to the page.
//! - Shift+Right / Shift+Left: one grid step wider / narrower; Shift+Down / Shift+Up: taller /
//!   shorter; clamped to `[MIN_SIDE, page_w - x]` (heights alike).
//! - A change pushes one undo entry (at most [`UNDO_DEPTH`], oldest dropped) and clears the
//!   redo stack; an action that changes nothing pushes nothing.
//! - Enter on a band or text block starts editing with the block's text as buffer; on a
//!   table or 3D frame it does nothing. While editing only Insert (append, at most
//!   [`MAX_TEXT_CHARS`] characters in all), SetText (replace) and Escape act. Escape ends
//!   editing and commits the buffer as one undo entry if it differs from the block's text.
//! - Undo pops the newest entry, restores the block, pushes the entry on the redo stack and
//!   focuses the block. Redo is the reverse.
//! - Zoom in / out: the next of [`ZOOM_STEPS`] above / below the current zoom, clamped.
//!   Zoom needs no focused block, so it also works on an empty canvas.
//! - While a pointer drag or resize is in progress every keyboard action is ignored.
//! - Changing the paper clears undo and redo: their rectangles belong to the old page size.
//! - Drag ([`Designer::drag_to`]) with snapping: the raw position snaps to a guide when the
//!   block's left edge, centre (`x + w / 2`) or right edge lies within [`GUIDE_TOL`] of one.
//!   Guides, ascending: 0, the margins, the page centre and the left edge, centre and right
//!   edge of every other block on the page. The nearest guide wins; on a tie the earlier
//!   edge (left, centre, right), then the smaller guide. Without a guide the position rounds
//!   to the nearest grid line (halves up). The result is clamped to the page; a guide is
//!   reported only if the clamp did not move the block. y alike with heights. Without
//!   snapping the raw position is only clamped. A whole drag is one undo entry.

use super::canvas::{fits, Block, Canvas, Paper, Rect, MAX_TEXT_CHARS, MIN_SIDE};
use std::collections::VecDeque;

/// Grid step in dmm (5 mm).
pub const GRID: i32 = 50;
/// Distance within which an edge snaps to a guide, in dmm (2 mm).
pub const GUIDE_TOL: i32 = 20;
/// Margin guides, in dmm from each page edge (15 mm).
pub const MARGIN: i32 = 150;
/// Undo entries kept.
pub const UNDO_DEPTH: usize = 1000;
/// Keyboard zoom steps in percent.
pub const ZOOM_STEPS: [f32; 11] = [25.0, 33.0, 50.0, 67.0, 75.0, 100.0, 125.0, 150.0, 200.0, 300.0, 400.0];
/// Smallest zoom in percent.
pub const ZOOM_MIN: f32 = 25.0;
/// Largest zoom in percent.
pub const ZOOM_MAX: f32 = 400.0;

/// One keyboard action on the canvas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Tab: next block.
    Tab,
    /// Shift+Tab: previous block.
    BackTab,
    /// Page Down: first block of the next page.
    PageDown,
    /// Page Up: first block of the previous page.
    PageUp,
    /// Arrow left: move left one grid step.
    Left,
    /// Arrow right: move right one grid step.
    Right,
    /// Arrow up: move up one grid step.
    Up,
    /// Arrow down: move down one grid step.
    Down,
    /// Shift + arrow left: one grid step narrower.
    ShiftLeft,
    /// Shift + arrow right: one grid step wider.
    ShiftRight,
    /// Shift + arrow up: one grid step shorter.
    ShiftUp,
    /// Shift + arrow down: one grid step taller.
    ShiftDown,
    /// Enter: start editing the text.
    Enter,
    /// Escape: end editing and commit.
    Escape,
    /// Ctrl+Z.
    Undo,
    /// Ctrl+Y or Ctrl+Shift+Z.
    Redo,
    /// Ctrl+Plus: next zoom step.
    ZoomIn,
    /// Ctrl+Minus: previous zoom step.
    ZoomOut,
    /// Committed text (typing or an IME commit) appended to the editing buffer.
    Insert(String),
    /// Replace the editing buffer (the text widget's whole content).
    SetText(String),
}

/// Guides a drag snapped to, in dmm on the block's page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Guides {
    /// Vertical guide line (x).
    pub x: Option<i32>,
    /// Horizontal guide line (y).
    pub y: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct State {
    rect: Rect,
    content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Edit {
    id: u32,
    before: State,
    after: State,
}

/// The designer.
#[derive(Debug, Clone)]
pub struct Designer {
    canvas: Canvas,
    focus: Option<u32>,
    editing: Option<(u32, String)>,
    undo: VecDeque<Edit>,
    redo: Vec<Edit>,
    zoom: f32,
    drag: Option<(u32, State)>,
}

fn state(b: &Block) -> State {
    State { rect: b.rect, content: b.content.clone() }
}

fn cut(s: &str) -> String {
    s.chars().take(MAX_TEXT_CHARS).collect()
}

fn clamp(v: i64, lo: i64, hi: i64) -> i32 {
    // hi >= lo whenever the block fits its page; prefer lo otherwise.
    let v = if v > hi { hi } else { v };
    let v = if v < lo { lo } else { v };
    i32::try_from(v).unwrap_or(0)
}

/// Snaps one axis (see the module rules): `raw` is the dragged edge position, `size` the
/// block's extent on that axis, `extent` the page's, `guides` ascending. Returns the
/// position and the guide used.
pub fn snap_axis(raw: i32, size: i32, extent: i32, guides: &[i32]) -> (i32, Option<i32>) {
    let raw = i64::from(raw);
    let mut best: Option<(i64, i64, i64)> = None; // (distance, guide, edge offset)
    for off in [0, i64::from(size) / 2, i64::from(size)] {
        for &g in guides {
            let g = i64::from(g);
            let d = (raw + off - g).abs();
            if d <= i64::from(GUIDE_TOL) && best.is_none_or(|b| d < b.0) {
                best = Some((d, g, off));
            }
        }
    }
    let grid = i64::from(GRID);
    let (pos, guide) = match best {
        Some((_, g, off)) => (g - off, Some(g)),
        None => ((raw + grid / 2).div_euclid(grid) * grid, None),
    };
    let clamped = clamp(pos, 0, i64::from(extent) - i64::from(size));
    let guide = guide.filter(|_| i64::from(clamped) == pos).and_then(|g| i32::try_from(g).ok());
    (clamped, guide)
}

impl Designer {
    /// A designer over `canvas`: focus on block 1, zoom 100 %, nothing to undo.
    pub fn new(canvas: Canvas) -> Designer {
        let focus = canvas.blocks().first().map(|b| b.id);
        Designer { canvas, focus, editing: None, undo: VecDeque::new(), redo: Vec::new(), zoom: 100.0, drag: None }
    }

    /// The canvas.
    pub fn canvas(&self) -> &Canvas {
        &self.canvas
    }

    /// The focused block id.
    pub fn focus(&self) -> Option<u32> {
        self.focus
    }

    /// The focused block.
    pub fn focused(&self) -> Option<&Block> {
        self.focus.and_then(|f| self.canvas.get(f))
    }

    /// The block being edited and the editing buffer.
    pub fn editing(&self) -> Option<(u32, &str)> {
        self.editing.as_ref().map(|(id, s)| (*id, s.as_str()))
    }

    /// The block being dragged or resized by the pointer.
    pub fn dragging(&self) -> Option<u32> {
        self.drag.as_ref().map(|d| d.0)
    }

    /// Undo entries.
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// Redo entries.
    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// Zoom in percent.
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Sets the zoom (pointer zoom), clamped to [`ZOOM_MIN`]..=[`ZOOM_MAX`]; NaN is ignored.
    pub fn set_zoom(&mut self, percent: f32) {
        if !percent.is_nan() {
            self.zoom = percent.clamp(ZOOM_MIN, ZOOM_MAX);
        }
    }

    /// Sets the paper of every page (see [`Canvas::set_paper`]). Whenever the paper of any
    /// page actually changes, undo and redo are cleared: their rectangles were valid for the
    /// old page size and could put a block off the new page.
    pub fn set_paper(&mut self, paper: Paper) {
        self.finish_edit();
        self.end_drag();
        if self.canvas.pages().iter().any(|p| *p != paper) {
            self.canvas.set_paper(paper);
            self.undo.clear();
            self.redo.clear();
        }
    }

    fn push(&mut self, e: Edit) {
        if e.before == e.after {
            return;
        }
        if self.undo.len() >= UNDO_DEPTH {
            self.undo.pop_front();
        }
        self.undo.push_back(e);
        self.redo.clear();
    }

    fn finish_edit(&mut self) {
        let Some((id, buf)) = self.editing.take() else { return };
        let Some(b) = self.canvas.get_mut(id) else { return };
        let before = state(b);
        b.content = buf;
        let after = state(b);
        self.push(Edit { id, before, after });
    }

    fn restore(&mut self, id: u32, s: &State) {
        if let Some(b) = self.canvas.get_mut(id) {
            b.rect = s.rect;
            b.content.clone_from(&s.content);
            self.focus = Some(id);
        }
    }

    /// Focuses a block (pointer click). An open text edit is committed first.
    pub fn select(&mut self, id: u32) {
        if self.canvas.get(id).is_some() && self.editing.as_ref().map(|e| e.0) != Some(id) {
            self.finish_edit();
            self.focus = Some(id);
        }
    }

    /// Applies one keyboard action. Ignored while a pointer drag or resize is in progress.
    pub fn apply(&mut self, a: &Action) {
        if self.drag.is_some() {
            return;
        }
        if let Some((_, buf)) = self.editing.as_mut() {
            match a {
                Action::Insert(s) => {
                    buf.push_str(s);
                    *buf = cut(buf);
                }
                Action::SetText(s) => *buf = cut(s),
                Action::Escape => self.finish_edit(),
                _ => {}
            }
            return;
        }
        match a {
            Action::ZoomIn => {
                self.zoom = ZOOM_STEPS.iter().copied().find(|z| *z > self.zoom).unwrap_or(ZOOM_MAX);
                return;
            }
            Action::ZoomOut => {
                self.zoom = ZOOM_STEPS.iter().rev().copied().find(|z| *z < self.zoom).unwrap_or(ZOOM_MIN);
                return;
            }
            _ => {}
        }
        let Some(id) = self.focus else { return };
        let Some(b) = self.canvas.get(id) else { return };
        let (page, kind, r) = (b.page, b.kind, b.rect);
        let Some((pw, ph)) = self.canvas.page_size(page) else { return };
        let last = self.canvas.blocks().len() as u32;
        let (mut dx, mut dy, mut dw, mut dh) = (0, 0, 0, 0);
        match a {
            Action::Tab => self.focus = Some((id + 1).min(last)),
            Action::BackTab => self.focus = Some(id.saturating_sub(1).max(1)),
            Action::PageDown | Action::PageUp => {
                let target = if *a == Action::PageDown { page.checked_add(1) } else { page.checked_sub(1) };
                if let Some(first) = target.and_then(|p| self.canvas.on_page(p).next()) {
                    self.focus = Some(first.id);
                }
            }
            Action::Left => dx = -GRID,
            Action::Right => dx = GRID,
            Action::Up => dy = -GRID,
            Action::Down => dy = GRID,
            Action::ShiftLeft => dw = -GRID,
            Action::ShiftRight => dw = GRID,
            Action::ShiftUp => dh = -GRID,
            Action::ShiftDown => dh = GRID,
            Action::Enter => {
                if kind.editable() {
                    self.editing = Some((id, b.content.clone()));
                }
            }
            Action::Undo => {
                if let Some(e) = self.undo.pop_back() {
                    self.restore(e.id, &e.before);
                    self.redo.push(e);
                }
            }
            Action::Redo => {
                if let Some(e) = self.redo.pop() {
                    self.restore(e.id, &e.after);
                    self.undo.push_back(e);
                }
            }
            Action::Escape | Action::Insert(_) | Action::SetText(_) | Action::ZoomIn | Action::ZoomOut => {}
        }
        if (dx, dy, dw, dh) == (0, 0, 0, 0) {
            return;
        }
        let (x, y, w, h, pw, ph) =
            (i64::from(r.x), i64::from(r.y), i64::from(r.w), i64::from(r.h), i64::from(pw), i64::from(ph));
        let min = i64::from(MIN_SIDE);
        let nx = clamp(x + i64::from(dx), 0, pw - w);
        let ny = clamp(y + i64::from(dy), 0, ph - h);
        let nw = clamp(w + i64::from(dw), min, pw - x);
        let nh = clamp(h + i64::from(dh), min, ph - y);
        let rect = if dx != 0 || dy != 0 { Rect { x: nx, y: ny, ..r } } else { Rect { w: nw, h: nh, ..r } };
        self.set_rect(id, rect);
    }

    fn set_rect(&mut self, id: u32, rect: Rect) {
        let Some(b) = self.canvas.get_mut(id) else { return };
        let before = state(b);
        b.rect = rect;
        let after = state(b);
        self.push(Edit { id, before, after });
    }

    /// Starts a pointer drag or resize of `id` (which also focuses it). Returns false for an
    /// unknown block or while that block's text is being edited.
    pub fn begin_drag(&mut self, id: u32) -> bool {
        if self.editing.as_ref().is_some_and(|e| e.0 == id) {
            return false;
        }
        self.end_drag();
        self.select(id);
        match self.canvas.get(id) {
            Some(b) => {
                self.drag = Some((id, state(b)));
                true
            }
            None => false,
        }
    }

    /// Moves the dragged block so its top-left corner is at the raw position (dmm, page
    /// coordinates), snapping as the module rules say. No drag: nothing.
    pub fn drag_to(&mut self, raw_x: i32, raw_y: i32, snap: bool) -> Guides {
        let Some(id) = self.drag.as_ref().map(|d| d.0) else { return Guides::default() };
        let Some(b) = self.canvas.get(id) else { return Guides::default() };
        let Some((pw, ph)) = self.canvas.page_size(b.page) else { return Guides::default() };
        let r = b.rect;
        let (x, y, guides) = if snap {
            let mut gx = vec![0, MARGIN, pw - MARGIN, pw / 2];
            let mut gy = vec![0, MARGIN, ph - MARGIN, ph / 2];
            for o in self.canvas.on_page(b.page).filter(|o| o.id != id) {
                let o = o.rect;
                gx.extend([o.x, o.x + o.w / 2, o.x + o.w]);
                gy.extend([o.y, o.y + o.h / 2, o.y + o.h]);
            }
            for g in [&mut gx, &mut gy] {
                g.sort_unstable();
                g.dedup();
            }
            let (x, sx) = snap_axis(raw_x, r.w, pw, &gx);
            let (y, sy) = snap_axis(raw_y, r.h, ph, &gy);
            (x, y, Guides { x: sx, y: sy })
        } else {
            let x = clamp(i64::from(raw_x), 0, i64::from(pw) - i64::from(r.w));
            let y = clamp(i64::from(raw_y), 0, i64::from(ph) - i64::from(r.h));
            (x, y, Guides::default())
        };
        if let Some(b) = self.canvas.get_mut(id) {
            b.rect = Rect { x, y, ..r };
        }
        guides
    }

    /// Resizes the dragged block to the raw size (dmm): rounded to the grid when snapping
    /// (halves up), then clamped to `[MIN_SIDE, page - position]`. No drag: nothing.
    pub fn resize_to(&mut self, raw_w: i32, raw_h: i32, snap: bool) {
        let Some(id) = self.drag.as_ref().map(|d| d.0) else { return };
        let Some(b) = self.canvas.get(id) else { return };
        let Some((pw, ph)) = self.canvas.page_size(b.page) else { return };
        let r = b.rect;
        let grid = i64::from(GRID);
        let round = |v: i32| if snap { (i64::from(v) + grid / 2).div_euclid(grid) * grid } else { i64::from(v) };
        let min = i64::from(MIN_SIDE);
        let w = clamp(round(raw_w), min, i64::from(pw) - i64::from(r.x));
        let h = clamp(round(raw_h), min, i64::from(ph) - i64::from(r.y));
        if let Some(b) = self.canvas.get_mut(id) {
            b.rect = Rect { w, h, ..r };
        }
    }

    /// Ends the pointer drag or resize: one undo entry if the block changed.
    pub fn end_drag(&mut self) {
        let Some((id, before)) = self.drag.take() else { return };
        let Some(b) = self.canvas.get(id) else { return };
        debug_assert!(self.canvas.page_size(b.page).is_some_and(|s| fits(b.rect, s)));
        let after = state(b);
        self.push(Edit { id, before, after });
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const SMALL: &str = "page\t1\tA4\npage\t2\tA4\n\
                         block\t1\t1\ttext\t200\t300\t400\t100\tText 1\tabc\n\
                         block\t2\t1\ttable\t1000\t300\t500\t200\tTable 1\t2x2\n\
                         block\t3\t2\tband\t150\t100\t1800\t150\tHeader 2\tTitle\n";

    fn designer() -> Designer {
        Designer::new(Canvas::parse(SMALL).unwrap())
    }

    fn rect(d: &Designer, id: u32) -> Rect {
        d.canvas().get(id).unwrap().rect
    }

    #[test]
    fn snap_axis_hand_labelled() {
        // Grid: 74 -> 50, 75 -> 100 (halves up), -26 -> -50 -> clamped to 0.
        assert_eq!(snap_axis(74, 100, 2100, &[]), (50, None));
        assert_eq!(snap_axis(75, 100, 2100, &[]), (100, None));
        assert_eq!(snap_axis(-26, 100, 2100, &[]), (0, None));
        // Left edge 20 from guide 500: snaps; 21 away: grid.
        assert_eq!(snap_axis(480, 100, 2100, &[500]), (500, Some(500)));
        assert_eq!(snap_axis(479, 100, 2100, &[500]), (500, None));
        // Centre (x + 50) nearer than the left edge: 452 + 50 = 502 (2 away) beats 452 -> 450.
        assert_eq!(snap_axis(452, 100, 2100, &[450, 502]), (452, Some(502)));
        // Tie between left edge (to 470, 10 away) and right edge (to 590, 10 away): left.
        assert_eq!(snap_axis(480, 100, 2100, &[470, 590]), (470, Some(470)));
        // Tie at one edge between guides 470 and 490: the smaller guide.
        assert_eq!(snap_axis(480, 100, 2100, &[470, 490]), (470, Some(470)));
        // Clamped away from the guide: no guide reported.
        assert_eq!(snap_axis(2010, 100, 2100, &[2010]), (2000, None));
        assert_eq!(snap_axis(i32::MIN, 100, 2100, &[]), (0, None));
        assert_eq!(snap_axis(i32::MAX, 100, 2100, &[]), (2000, None));
    }

    #[test]
    fn keyboard_move_resize_undo_redo() {
        let mut d = designer();
        assert_eq!(d.focus(), Some(1));
        for _ in 0..10 {
            d.apply(&Action::Right);
        }
        assert_eq!(rect(&d, 1), Rect { x: 700, y: 300, w: 400, h: 100 });
        d.apply(&Action::ShiftRight);
        d.apply(&Action::ShiftDown);
        assert_eq!(rect(&d, 1), Rect { x: 700, y: 300, w: 450, h: 150 });
        assert_eq!((d.undo_depth(), d.redo_depth()), (12, 0));
        d.apply(&Action::Undo);
        d.apply(&Action::Undo);
        assert_eq!(rect(&d, 1), Rect { x: 700, y: 300, w: 400, h: 100 });
        d.apply(&Action::Redo);
        assert_eq!(rect(&d, 1).w, 450);
        assert_eq!((d.undo_depth(), d.redo_depth()), (11, 1));
        d.apply(&Action::Up);
        assert_eq!(d.redo_depth(), 0);
        // Clamp at the top: y 250 -> 200 ... -> 0, then no more entries.
        for _ in 0..10 {
            d.apply(&Action::Up);
        }
        assert_eq!(rect(&d, 1).y, 0);
        assert_eq!(d.undo_depth(), 17);
        // Shrink to the smallest side (h 100 -> 50: the redo above restored only the
        // width), then nothing.
        for _ in 0..20 {
            d.apply(&Action::ShiftUp);
        }
        assert_eq!(rect(&d, 1).h, MIN_SIDE);
        assert_eq!(d.undo_depth(), 18);
    }

    #[test]
    fn focus_moves_and_page_keys() {
        let mut d = designer();
        d.apply(&Action::BackTab);
        assert_eq!(d.focus(), Some(1));
        d.apply(&Action::PageDown);
        assert_eq!(d.focus(), Some(3));
        d.apply(&Action::PageDown);
        assert_eq!(d.focus(), Some(3));
        d.apply(&Action::Tab);
        assert_eq!(d.focus(), Some(3));
        d.apply(&Action::PageUp);
        assert_eq!(d.focus(), Some(1));
        d.apply(&Action::Tab);
        assert_eq!(d.focus(), Some(2));
        // Enter on a table does nothing.
        d.apply(&Action::Enter);
        assert_eq!(d.editing(), None);
    }

    #[test]
    fn editing_commits_once_and_ignores_other_keys() {
        let mut d = designer();
        d.apply(&Action::Enter);
        assert_eq!(d.editing(), Some((1, "abc")));
        d.apply(&Action::Right);
        d.apply(&Action::Insert("そくてい".into()));
        assert_eq!(d.editing(), Some((1, "abcそくてい")));
        assert_eq!(rect(&d, 1).x, 200);
        d.apply(&Action::Escape);
        assert_eq!(d.canvas().get(1).unwrap().content, "abcそくてい");
        assert_eq!(d.undo_depth(), 1);
        d.apply(&Action::Undo);
        assert_eq!(d.canvas().get(1).unwrap().content, "abc");
        // Unchanged text: no entry.
        d.apply(&Action::Enter);
        d.apply(&Action::SetText("abc".into()));
        d.apply(&Action::Escape);
        assert_eq!((d.undo_depth(), d.redo_depth()), (0, 1));
        // The buffer is capped.
        d.apply(&Action::Enter);
        d.apply(&Action::SetText("x".repeat(MAX_TEXT_CHARS + 5)));
        d.apply(&Action::Insert("y".into()));
        assert_eq!(d.editing().unwrap().1.chars().count(), MAX_TEXT_CHARS);
        // Selecting another block commits.
        d.select(2);
        assert_eq!(d.editing(), None);
        assert_eq!(d.focus(), Some(2));
        assert_eq!(d.undo_depth(), 1);
    }

    #[test]
    fn zoom_steps_and_pointer_zoom() {
        let mut d = designer();
        d.apply(&Action::ZoomIn);
        assert_eq!(d.zoom(), 125.0);
        d.set_zoom(110.0);
        d.apply(&Action::ZoomOut);
        assert_eq!(d.zoom(), 100.0);
        d.set_zoom(1000.0);
        assert_eq!(d.zoom(), ZOOM_MAX);
        d.set_zoom(f32::NAN);
        assert_eq!(d.zoom(), ZOOM_MAX);
        d.apply(&Action::ZoomIn);
        assert_eq!(d.zoom(), ZOOM_MAX);
        d.set_zoom(0.0);
        d.apply(&Action::ZoomOut);
        assert_eq!(d.zoom(), ZOOM_MIN);
    }

    #[test]
    fn drag_snaps_and_is_one_undo_entry() {
        let mut d = designer();
        assert!(d.begin_drag(1));
        // Guides on page 1 (x): 0, 150, 1050, 1950, block 2: 1000, 1250, 1500.
        // Raw 590: right edge 990 is 10 from 1000 -> x 600.
        assert_eq!(d.drag_to(590, 300, true), Guides { x: Some(1000), y: Some(300) });
        assert_eq!(rect(&d, 1).x, 600);
        // No guide near 333 / 333+200 / 333+400; y 777: 777+50 = 827? no guide -> grid 350 / 800.
        assert_eq!(d.drag_to(333, 777, true), Guides::default());
        assert_eq!((rect(&d, 1).x, rect(&d, 1).y), (350, 800));
        assert_eq!(d.drag_to(-5, 5000, false), Guides::default());
        assert_eq!((rect(&d, 1).x, rect(&d, 1).y), (0, 2870));
        d.resize_to(1234, 20, true);
        assert_eq!((rect(&d, 1).w, rect(&d, 1).h), (1250, MIN_SIDE));
        d.end_drag();
        assert_eq!(d.undo_depth(), 1);
        d.apply(&Action::Undo);
        assert_eq!(rect(&d, 1), Rect { x: 200, y: 300, w: 400, h: 100 });
        // Dragging the block being edited is refused.
        d.apply(&Action::Enter);
        assert!(!d.begin_drag(1));
        assert!(!d.begin_drag(9));
        assert_eq!(d.drag_to(0, 0, true), Guides::default());
    }

    /// Review of #577: A4, Letter, A4, move, Letter, then undo must not put a block off the
    /// page (Letter to A4 moves no block, so a clear tied to moved blocks kept stale entries).
    #[test]
    fn paper_change_clears_history_so_undo_stays_on_the_page() {
        // A footer near the bottom of A4: it must move up to fit Letter (2794 high).
        let text = "page	1	A4
block	1	1	band	150	2720	1800	150	Footer 1	Page 1
";
        let mut d = Designer::new(Canvas::parse(text).unwrap());
        d.apply(&Action::Up);
        assert_eq!(d.undo_depth(), 1);
        d.set_paper(Paper::Letter);
        assert_eq!((d.undo_depth(), d.redo_depth()), (0, 0));
        d.apply(&Action::Undo);
        d.set_paper(Paper::A4);
        // No block moved, but the paper changed: still cleared.
        assert_eq!((d.undo_depth(), d.redo_depth()), (0, 0));
        for _ in 0..10 {
            d.apply(&Action::Down);
        }
        assert_eq!(rect(&d, 1).y, 2970 - 150);
        // Back up to where the block also fits Letter, so the switch below moves nothing;
        // the undo entries still hold y 2820, below the Letter page.
        for _ in 0..5 {
            d.apply(&Action::Up);
        }
        assert_eq!(rect(&d, 1).y, 2570);
        d.set_paper(Paper::Letter);
        assert_eq!(rect(&d, 1).y, 2570);
        assert_eq!((d.undo_depth(), d.redo_depth()), (0, 0));
        for _ in 0..20 {
            d.apply(&Action::Undo);
        }
        let size = d.canvas().page_size(1).unwrap();
        assert!(d.canvas().blocks().iter().all(|b| fits(b.rect, size)), "{:?}", rect(&d, 1));
        // Setting the same paper again keeps the history.
        d.apply(&Action::Up);
        d.set_paper(Paper::Letter);
        assert_eq!(d.undo_depth(), 1);
    }

    #[test]
    fn keys_are_ignored_while_dragging_and_zoom_needs_no_focus() {
        let mut d = designer();
        assert!(d.begin_drag(1));
        d.drag_to(700, 300, false);
        for a in [Action::Right, Action::Tab, Action::Undo, Action::ZoomIn, Action::Enter] {
            d.apply(&a);
        }
        assert_eq!((rect(&d, 1).x, d.focus(), d.zoom(), d.editing()), (700, Some(1), 100.0, None));
        d.end_drag();
        assert_eq!(d.undo_depth(), 1);
        d.apply(&Action::Undo);
        assert_eq!(rect(&d, 1).x, 200);
        // An empty canvas: no focus, zoom still works.
        let mut e = Designer::new(
            Canvas::parse(
                "page	1	A4
",
            )
            .unwrap(),
        );
        assert_eq!(e.focus(), None);
        e.apply(&Action::ZoomIn);
        e.apply(&Action::ZoomIn);
        assert_eq!(e.zoom(), 150.0);
        e.apply(&Action::ZoomOut);
        assert_eq!(e.zoom(), 125.0);
        e.apply(&Action::Right);
        e.set_paper(Paper::Letter);
        assert_eq!(e.canvas().pages(), [Paper::Letter]);
    }

    #[test]
    fn undo_depth_is_capped() {
        let mut d = designer();
        for k in 0..(UNDO_DEPTH + 10) {
            d.apply(if k % 2 == 0 { &Action::Right } else { &Action::Left });
        }
        assert_eq!(d.undo_depth(), UNDO_DEPTH);
    }
}
