//! The table's interaction model, independent of the toolkit: column order and widths with
//! two frozen columns, sort and filter (computed by [`compute_view`], which the example runs
//! on a background thread and swaps in), cell focus and rectangular selection, in-place
//! editing that stores the typed decimal string exactly, TSV copy, the fly-to stub and the
//! accessibility facts of the focused cell.
//!
//! Rules (the Python reference `fixtures/viz/char-table-100k/make_table.py` implements the same
//! rules independently):
//!
//! - The first [`FROZEN`] display positions are frozen; a column never moves across that
//!   boundary. Widths change by [`WIDTH_STEP`] within [[`MIN_WIDTH`], [`MAX_WIDTH`]].
//! - The view is the row ids in sort order (none: ascending id; ascending: key then id;
//!   descending: key descending, then ascending id), keeping rows where the filter (ASCII
//!   case-insensitive) is a substring of any of the row's 14 cell texts. Numeric columns
//!   compare as exact decimals with `-` after every number; status by Pass < Warn < Fail <
//!   Not measured; text by code point.
//! - Focus is (row id, column). After a sort or filter change (a refresh) the anchor is
//!   cleared and the focus keeps its row if it is still shown, else moves to the first shown
//!   row (none if nothing is shown).
//! - Grid mode: arrows, PageUp / PageDown ([`Grid::page_rows`] rows), Home / End (columns) and
//!   Ctrl+Home / Ctrl+End (rows) move, clamped. Shift+arrows set the anchor to the focus if
//!   unset and move. Every grid action except Shift moves, Copy and FlyTo clears the anchor.
//!   Enter or F2 on an editable cell starts editing with the stored text; typing on one starts
//!   editing with an empty buffer; on other cells both do nothing. Escape clears the anchor.
//! - Filter mode: typing appends (at most [`MAX_FILTER_CHARS`]), Backspace removes the last
//!   character, both refresh; Enter returns to the grid; Escape clears the filter, refreshes
//!   and returns.
//! - Edit mode: typing appends (at most [`MAX_DEC_CHARS`]), Backspace removes the last
//!   character, Escape cancels. Enter commits when the buffer is a decimal and upper is not
//!   below lower; the typed text is stored exactly, deviation and status are recomputed.
//!   Editing never re-sorts or re-filters the view.

use super::data::{Column, Row, Status, Table};
use super::dec::{Dec, MAX_DEC_CHARS};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::fmt;

/// Frozen display positions at the left.
pub const FROZEN: usize = 2;
/// Width change per Narrow / Widen.
pub const WIDTH_STEP: i32 = 10;
/// Narrowest column.
pub const MIN_WIDTH: i32 = 40;
/// Widest column.
pub const MAX_WIDTH: i32 = 600;
/// Longest filter text in characters.
pub const MAX_FILTER_CHARS: usize = 64;
/// Most cells one copy may hold.
pub const MAX_COPY_CELLS: usize = 2_000_000;
/// Default rows per PageUp / PageDown (the example sets the visible row count).
pub const DEFAULT_PAGE_ROWS: usize = 20;

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dir {
    /// Ascending.
    Asc,
    /// Descending.
    Desc,
}

impl Dir {
    /// Name used in the fixtures.
    pub fn name(self) -> &'static str {
        match self {
            Dir::Asc => "asc",
            Dir::Desc => "desc",
        }
    }
}

/// What the view shows: sort and filter.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    /// Sort column and direction; `None` is id order.
    pub sort: Option<(Column, Dir)>,
    /// Filter text.
    pub filter: String,
}

/// One keyboard (or scripted) action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Previous row.
    Up,
    /// Next row.
    Down,
    /// Previous column.
    Left,
    /// Next column.
    Right,
    /// [`Grid::page_rows`] rows up.
    PageUp,
    /// [`Grid::page_rows`] rows down.
    PageDown,
    /// First column.
    Home,
    /// Last column.
    End,
    /// First row.
    CtrlHome,
    /// Last row.
    CtrlEnd,
    /// Extend the selection up.
    ShiftUp,
    /// Extend the selection down.
    ShiftDown,
    /// Extend the selection left.
    ShiftLeft,
    /// Extend the selection right.
    ShiftRight,
    /// Sort ascending by the focused column.
    SortAsc,
    /// Sort descending by the focused column.
    SortDesc,
    /// Remove the sort.
    SortClear,
    /// Move keyboard focus to the filter box.
    FocusFilter,
    /// Type text (into the filter, the edit buffer, or start an edit).
    Type(String),
    /// Remove the last character (filter or edit buffer).
    Backspace,
    /// Start editing; commit an edit; leave the filter box.
    Enter,
    /// Start editing.
    F2,
    /// Cancel an edit; clear the filter; clear the selection.
    Escape,
    /// Copy the selection as TSV.
    Copy,
    /// Swap the focused column with its left neighbour.
    MoveLeft,
    /// Swap the focused column with its right neighbour.
    MoveRight,
    /// Narrow the focused column.
    Narrow,
    /// Widen the focused column.
    Widen,
    /// Ask the 3D view to fly to the focused row's feature (stub).
    FlyTo,
}

/// Keyboard mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Navigating cells.
    Grid,
    /// Typing in the filter box.
    Filter,
    /// Editing a cell.
    Edit,
}

impl Mode {
    /// Name used in the fixture.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Grid => "grid",
            Mode::Filter => "filter",
            Mode::Edit => "edit",
        }
    }
}

/// Why a commit was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditError {
    /// The buffer is not a decimal.
    NotDecimal,
    /// The upper tolerance would be below the lower one.
    UpperBelowLower,
}

impl EditError {
    /// Name used in the fixture.
    pub fn name(self) -> &'static str {
        match self {
            EditError::NotDecimal => "not-decimal",
            EditError::UpperBelowLower => "upper-below-lower",
        }
    }
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::NotDecimal => write!(f, "not a decimal number"),
            EditError::UpperBelowLower => write!(f, "upper tolerance below lower tolerance"),
        }
    }
}

/// Sort key of one cell, borrowed from the row.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Key<'a> {
    Int(u32),
    /// `None` (`-`) sorts after every number.
    Num(Option<i128>),
    Status(Status),
    Text(&'a str),
}

fn key(r: &Row, c: Column) -> Key<'_> {
    match c {
        Column::Id => Key::Int(r.id),
        Column::Balloon => Key::Int(r.balloon),
        Column::Status => Key::Status(r.status),
        // `-` (and anything that is not a decimal) is `None`; `cmp_key` puts it last.
        c if c.numeric() => Key::Num(Dec::parse(&r.text(c)).map(|d| d.scaled)),
        Column::Characteristic => Key::Text(&r.characteristic),
        Column::Unit => Key::Text(&r.unit),
        Column::Method => Key::Text(&r.method),
        Column::Feature => Key::Text(&r.feature),
        _ => Key::Text(&r.note),
    }
}

fn cmp_key(a: &Key<'_>, b: &Key<'_>) -> Ordering {
    match (a, b) {
        (Key::Num(x), Key::Num(y)) => match (x, y) {
            (Some(x), Some(y)) => x.cmp(y),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        },
        _ => a.cmp(b),
    }
}

/// Whether `needle` (already ASCII-lowercased) occurs in `hay`, ignoring ASCII case.
pub fn contains_ascii_ci(hay: &str, needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    hay.as_bytes().windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
}

fn matches(r: &Row, needle: &[u8]) -> bool {
    Column::ALL.into_iter().any(|c| contains_ascii_ci(&r.text(c), needle))
}

/// The view (row ids in display order) of `table` under `q`. Pure: the example calls it on
/// a background thread.
pub fn compute_view(table: &Table, q: &Query) -> Vec<u32> {
    let rows = table.rows();
    let mut ids: Vec<u32> = match q.sort {
        None => rows.iter().map(|r| r.id).collect(),
        Some((c, dir)) => {
            let mut keyed: Vec<(Key<'_>, u32)> = rows.iter().map(|r| (key(r, c), r.id)).collect();
            keyed.sort_by(|(ka, ia), (kb, ib)| {
                let o = cmp_key(ka, kb);
                let o = if dir == Dir::Desc { o.reverse() } else { o };
                o.then(ia.cmp(ib))
            });
            keyed.into_iter().map(|(_, id)| id).collect()
        }
    };
    if !q.filter.is_empty() {
        let needle = q.filter.to_ascii_lowercase().into_bytes();
        ids.retain(|id| table.get(*id).is_some_and(|r| matches(r, &needle)));
    }
    ids
}

/// What an action changed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Applied {
    /// The requested sort or filter changed: refresh, or recompute the view in the
    /// background.
    pub query_changed: bool,
    /// A committed edit: the whole row to store in the table.
    pub edit: Option<Row>,
}

/// The facts a screen reader gets for a cell (REQ-VIZ.SHELL.SPKTABLE-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellA11y {
    /// 0-based position of the row in the view.
    pub row_index: usize,
    /// Rows in the view.
    pub row_count: usize,
    /// 0-based display position of the column.
    pub column_index: usize,
    /// Columns.
    pub column_count: usize,
    /// Whether the cell is in a frozen column (exposed as a row header).
    pub row_header_cell: bool,
    /// Column header text.
    pub column_header: String,
    /// Row header text: balloon and characteristic.
    pub row_header: String,
    /// Cell value.
    pub value: String,
    /// Status word of the row.
    pub status: &'static str,
}

impl CellA11y {
    /// The label announced for the cell: row header, column header and value, then the
    /// status (left out on the status column itself).
    pub fn label(&self) -> String {
        let mut s = format!("{}, {}: {}", self.row_header, self.column_header, self.value);
        if self.column_header != Column::Status.title() {
            s.push_str(", status ");
            s.push_str(self.status);
        }
        s
    }

    /// Position, announced after the label.
    pub fn description(&self) -> String {
        format!(
            "Row {} of {}, column {} of {}",
            self.row_index + 1,
            self.row_count,
            self.column_index + 1,
            self.column_count
        )
    }
}

/// The grid state.
#[derive(Debug, Clone)]
pub struct Grid {
    order: Vec<Column>,
    widths: Vec<(Column, i32)>,
    query: Query,
    shown: Query,
    view: Vec<u32>,
    shown_set: HashSet<u32>,
    focus_row: Option<u32>,
    focus_col: Column,
    anchor: Option<(u32, Column)>,
    mode: Mode,
    buf: String,
    error: Option<EditError>,
    clipboard: Option<String>,
    fly_to: Option<u32>,
    /// Rows per PageUp / PageDown.
    pub page_rows: usize,
}

impl Grid {
    /// A grid over `table` in id order, focus on the first row and column.
    pub fn new(table: &Table) -> Grid {
        let query = Query::default();
        let view = compute_view(table, &query);
        let shown_set = view.iter().copied().collect();
        Grid {
            order: Column::ALL.to_vec(),
            widths: Column::ALL.iter().map(|c| (*c, c.default_width())).collect(),
            query: query.clone(),
            shown: query,
            focus_row: view.first().copied(),
            view,
            shown_set,
            focus_col: Column::Id,
            anchor: None,
            mode: Mode::Grid,
            buf: String::new(),
            error: None,
            clipboard: None,
            fly_to: None,
            page_rows: DEFAULT_PAGE_ROWS,
        }
    }

    /// Columns in display order.
    pub fn order(&self) -> &[Column] {
        &self.order
    }

    /// Width of a column in logical pixels.
    pub fn width(&self, c: Column) -> i32 {
        self.widths.iter().find(|(k, _)| *k == c).map_or(c.default_width(), |(_, w)| *w)
    }

    /// Sets a column's width (pointer resize), clamped.
    pub fn set_width(&mut self, c: Column, w: i32) {
        if let Some(e) = self.widths.iter_mut().find(|(k, _)| *k == c) {
            e.1 = w.clamp(MIN_WIDTH, MAX_WIDTH);
        }
    }

    /// Moves column `c` to display position `to` (pointer reorder) if both are on the same
    /// side of the frozen boundary. Returns whether it moved.
    pub fn move_column_to(&mut self, c: Column, to: usize) -> bool {
        let Some(from) = self.order.iter().position(|k| *k == c) else { return false };
        if to >= self.order.len() || (to < FROZEN) != (from < FROZEN) || to == from {
            return false;
        }
        self.order.remove(from);
        self.order.insert(to, c);
        true
    }

    /// The requested query.
    pub fn query(&self) -> &Query {
        &self.query
    }

    /// The query the current view reflects.
    pub fn shown_query(&self) -> &Query {
        &self.shown
    }

    /// Requests a sort (the benchmark harness); shown after the next refresh or install.
    pub fn set_sort(&mut self, sort: Option<(Column, Dir)>) {
        self.query.sort = sort;
    }

    /// Whether the view is out of date (a sort or filter is pending).
    pub fn is_stale(&self) -> bool {
        self.query != self.shown
    }

    /// Row ids in display order.
    pub fn view(&self) -> &[u32] {
        &self.view
    }

    /// Focused row id.
    pub fn focus_row(&self) -> Option<u32> {
        self.focus_row
    }

    /// Focused column.
    pub fn focus_col(&self) -> Column {
        self.focus_col
    }

    /// Selection anchor.
    pub fn anchor(&self) -> Option<(u32, Column)> {
        self.anchor
    }

    /// Keyboard mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Edit buffer (meaningful in edit mode).
    pub fn buffer(&self) -> &str {
        &self.buf
    }

    /// Last refused commit, if the edit is still open.
    pub fn error(&self) -> Option<EditError> {
        self.error
    }

    /// Text of the last copy.
    pub fn clipboard(&self) -> Option<&str> {
        self.clipboard.as_deref()
    }

    /// Takes the text of the last copy (the example hands it to the platform once).
    pub fn take_clipboard(&mut self) -> Option<String> {
        self.clipboard.take()
    }

    /// Row id of the last fly-to request (the stub's whole effect).
    pub fn fly_to(&self) -> Option<u32> {
        self.fly_to
    }

    /// Display position of the focused row.
    pub fn focus_pos(&self) -> Option<usize> {
        let f = self.focus_row?;
        self.view.iter().position(|id| *id == f)
    }

    /// Display position of a column.
    pub fn column_pos(&self, c: Column) -> usize {
        self.order.iter().position(|k| *k == c).unwrap_or(0)
    }

    /// Installs a view computed for `q` (by [`compute_view`], possibly on another thread).
    /// Ignored unless `q` is the requested query. Returns whether it was installed.
    pub fn install(&mut self, q: &Query, view: Vec<u32>) -> bool {
        if *q != self.query {
            return false;
        }
        self.shown = q.clone();
        self.shown_set = view.iter().copied().collect();
        self.view = view;
        self.anchor = None;
        if !self.focus_row.is_some_and(|f| self.shown_set.contains(&f)) {
            self.focus_row = self.view.first().copied();
        }
        true
    }

    /// Recomputes the view now (the synchronous path used by tests and the script).
    pub fn refresh(&mut self, table: &Table) {
        let q = self.query.clone();
        let v = compute_view(table, &q);
        self.install(&q, v);
    }

    fn move_row(&mut self, to: isize) {
        if self.view.is_empty() || self.focus_row.is_none() {
            return;
        }
        let last = self.view.len() as isize - 1;
        let k = to.clamp(0, last) as usize;
        self.focus_row = self.view.get(k).copied();
    }

    fn move_col(&mut self, to: isize) {
        let last = self.order.len() as isize - 1;
        let k = to.clamp(0, last) as usize;
        if let Some(c) = self.order.get(k) {
            self.focus_col = *c;
        }
    }

    fn editable(&self) -> bool {
        self.focus_row.is_some() && self.focus_col.editable()
    }

    fn push_limited(s: &mut String, text: &str, limit: usize) {
        let mut n = s.chars().count();
        for ch in text.chars() {
            if n < limit {
                s.push(ch);
                n += 1;
            }
        }
    }

    /// Applies one action. Sort and filter changes only update the requested query; call
    /// [`Grid::refresh`] (or [`Grid::install`] with a background result) to show them. A
    /// committed edit is returned for the caller to store (the example shares the table with
    /// its background sorter).
    pub fn apply(&mut self, table: &Table, a: &Action) -> Applied {
        let before = self.query.clone();
        let edit = match self.mode {
            Mode::Filter => {
                self.apply_filter(a);
                None
            }
            Mode::Edit => self.apply_edit(table, a),
            Mode::Grid => {
                self.apply_grid(table, a);
                None
            }
        };
        Applied { query_changed: self.query != before, edit }
    }

    /// [`Grid::apply`], storing a committed edit in `table`. Returns whether the query
    /// changed.
    pub fn apply_mut(&mut self, table: &mut Table, a: &Action) -> bool {
        let out = self.apply(table, a);
        if let Some(row) = out.edit {
            if let Some(slot) = table.get_mut(row.id) {
                *slot = row;
            }
        }
        out.query_changed
    }

    /// Sets the filter text (typed into the filter box by the example), at most
    /// [`MAX_FILTER_CHARS`] characters.
    pub fn set_filter(&mut self, text: &str) {
        self.query.filter = text.chars().take(MAX_FILTER_CHARS).collect();
    }

    /// Sets the edit buffer (typed into the cell editor by the example), at most
    /// [`MAX_DEC_CHARS`] characters. Ignored outside edit mode.
    pub fn set_buffer(&mut self, text: &str) {
        if self.mode == Mode::Edit {
            self.buf = text.chars().take(MAX_DEC_CHARS).collect();
            self.error = None;
        }
    }

    fn apply_filter(&mut self, a: &Action) {
        match a {
            Action::Type(t) => Self::push_limited(&mut self.query.filter, t, MAX_FILTER_CHARS),
            Action::Backspace => {
                self.query.filter.pop();
            }
            Action::Enter => self.mode = Mode::Grid,
            Action::Escape => {
                self.query.filter.clear();
                self.mode = Mode::Grid;
            }
            _ => {}
        }
    }

    fn apply_edit(&mut self, table: &Table, a: &Action) -> Option<Row> {
        match a {
            Action::Type(t) => {
                Self::push_limited(&mut self.buf, t, MAX_DEC_CHARS);
                self.error = None;
            }
            Action::Backspace => {
                self.buf.pop();
                self.error = None;
            }
            Action::Enter => match self.commit(table) {
                Ok(row) => {
                    self.mode = Mode::Grid;
                    self.buf.clear();
                    self.error = None;
                    return Some(row);
                }
                Err(e) => self.error = Some(e),
            },
            Action::Escape => {
                self.mode = Mode::Grid;
                self.buf.clear();
                self.error = None;
            }
            _ => {}
        }
        None
    }

    fn commit(&self, table: &Table) -> Result<Row, EditError> {
        let Some(row) = self.focus_row.and_then(|f| table.get(f)) else { return Err(EditError::NotDecimal) };
        if Dec::parse(&self.buf).is_none() {
            return Err(EditError::NotDecimal);
        }
        let mut next = row.clone();
        match self.focus_col {
            Column::Nominal => next.nominal.clone_from(&self.buf),
            Column::Upper => next.upper.clone_from(&self.buf),
            Column::Lower => next.lower.clone_from(&self.buf),
            _ => return Err(EditError::NotDecimal),
        }
        let (Some(u), Some(l)) = (Dec::parse(&next.upper), Dec::parse(&next.lower)) else {
            return Err(EditError::NotDecimal);
        };
        if u.cmp_value(&l) == Ordering::Less {
            return Err(EditError::UpperBelowLower);
        }
        if !next.recompute() {
            return Err(EditError::NotDecimal);
        }
        Ok(next)
    }

    fn apply_grid(&mut self, table: &Table, a: &Action) {
        let keeps_anchor = matches!(
            a,
            Action::ShiftUp | Action::ShiftDown | Action::ShiftLeft | Action::ShiftRight | Action::Copy | Action::FlyTo
        );
        if !keeps_anchor {
            self.anchor = None;
        }
        let shifted = match a {
            Action::ShiftUp => Some(Action::Up),
            Action::ShiftDown => Some(Action::Down),
            Action::ShiftLeft => Some(Action::Left),
            Action::ShiftRight => Some(Action::Right),
            _ => None,
        };
        if let Some(m) = shifted {
            let Some(f) = self.focus_row else { return };
            self.anchor.get_or_insert((f, self.focus_col));
            self.navigate(&m);
            return;
        }
        let cpos = self.column_pos(self.focus_col) as isize;
        match a {
            Action::Up
            | Action::Down
            | Action::Left
            | Action::Right
            | Action::PageUp
            | Action::PageDown
            | Action::Home
            | Action::End
            | Action::CtrlHome
            | Action::CtrlEnd => self.navigate(a),
            Action::SortAsc => self.query.sort = Some((self.focus_col, Dir::Asc)),
            Action::SortDesc => self.query.sort = Some((self.focus_col, Dir::Desc)),
            Action::SortClear => self.query.sort = None,
            Action::FocusFilter => self.mode = Mode::Filter,
            Action::Type(t) => {
                if self.editable() {
                    self.mode = Mode::Edit;
                    self.buf.clear();
                    self.error = None;
                    Self::push_limited(&mut self.buf, t, MAX_DEC_CHARS);
                }
            }
            Action::Enter | Action::F2 => {
                if self.editable() {
                    let text = self.focus_row.and_then(|f| table.get(f)).map(|r| r.text(self.focus_col).into_owned());
                    if let Some(text) = text {
                        self.mode = Mode::Edit;
                        self.buf = text;
                        self.error = None;
                    }
                }
            }
            Action::Copy => self.copy(table),
            Action::FlyTo => {
                if self.focus_row.is_some() {
                    self.fly_to = self.focus_row;
                }
            }
            Action::MoveLeft | Action::MoveRight => {
                let to = if *a == Action::MoveLeft { cpos - 1 } else { cpos + 1 };
                if let Ok(to) = usize::try_from(to) {
                    self.move_column_to(self.focus_col, to);
                }
            }
            Action::Narrow | Action::Widen => {
                let step = if *a == Action::Widen { WIDTH_STEP } else { -WIDTH_STEP };
                self.set_width(self.focus_col, self.width(self.focus_col) + step);
            }
            Action::ShiftUp
            | Action::ShiftDown
            | Action::ShiftLeft
            | Action::ShiftRight
            | Action::Backspace
            | Action::Escape => {}
        }
    }

    fn navigate(&mut self, a: &Action) {
        let p = self.focus_pos().map_or(0, |p| p as isize);
        let page = self.page_rows as isize;
        let cpos = self.column_pos(self.focus_col) as isize;
        match a {
            Action::Up => self.move_row(p - 1),
            Action::Down => self.move_row(p + 1),
            Action::PageUp => self.move_row(p - page),
            Action::PageDown => self.move_row(p + page),
            Action::CtrlHome => self.move_row(0),
            Action::CtrlEnd => self.move_row(self.view.len() as isize - 1),
            Action::Left => self.move_col(cpos - 1),
            Action::Right => self.move_col(cpos + 1),
            Action::Home => self.move_col(0),
            Action::End => self.move_col(self.order.len() as isize - 1),
            _ => {}
        }
    }

    /// Focuses a cell (pointer click). Clears the anchor unless `extend`, which sets it to
    /// the old focus if unset. Ignored in filter and edit mode.
    pub fn click(&mut self, id: u32, c: Column, extend: bool) {
        if self.mode != Mode::Grid || !self.shown_set.contains(&id) {
            return;
        }
        if extend {
            if let Some(f) = self.focus_row {
                self.anchor.get_or_insert((f, self.focus_col));
            }
        } else {
            self.anchor = None;
        }
        self.focus_row = Some(id);
        self.focus_col = c;
    }

    /// Whether a cell lies in the selection rectangle (anchor to focus, or the focus alone).
    pub fn selected(&self, pos: usize, c: Column) -> bool {
        self.selection().is_some_and(|((r0, r1), (c0, c1))| {
            let cp = self.column_pos(c);
            (r0..=r1).contains(&pos) && (c0..=c1).contains(&cp)
        })
    }

    /// The selection as (row positions, column positions), inclusive.
    pub fn selection(&self) -> Option<((usize, usize), (usize, usize))> {
        let fp = self.focus_pos()?;
        let fc = self.column_pos(self.focus_col);
        let (ap, ac) = match self.anchor {
            Some((id, c)) => (self.view.iter().position(|x| *x == id).unwrap_or(fp), self.column_pos(c)),
            None => (fp, fc),
        };
        Some(((ap.min(fp), ap.max(fp)), (ac.min(fc), ac.max(fc))))
    }

    fn copy(&mut self, table: &Table) {
        let Some(((r0, r1), (c0, c1))) = self.selection() else { return };
        let cols: Vec<Column> = self.order.get(c0..=c1).map(<[Column]>::to_vec).unwrap_or_default();
        if (r1 - r0 + 1).saturating_mul(cols.len()) > MAX_COPY_CELLS {
            return;
        }
        let clean = |s: &str| -> String { s.chars().map(|ch| if ch.is_control() { ' ' } else { ch }).collect() };
        let mut out = String::new();
        let titles: Vec<&str> = cols.iter().map(|c| c.title()).collect();
        out.push_str(&titles.join("\t"));
        out.push('\n');
        for id in self.view.get(r0..=r1).unwrap_or(&[]) {
            let Some(r) = table.get(*id) else { continue };
            let cells: Vec<String> = cols.iter().map(|c| clean(&r.text(*c))).collect();
            out.push_str(&cells.join("\t"));
            out.push('\n');
        }
        self.clipboard = Some(out);
    }

    /// Header text of a column: its title, plus the sort state.
    pub fn header_label(&self, c: Column) -> String {
        match self.shown.sort {
            Some((k, Dir::Asc)) if k == c => format!("{}, sorted ascending", c.title()),
            Some((k, Dir::Desc)) if k == c => format!("{}, sorted descending", c.title()),
            _ => c.title().to_string(),
        }
    }

    /// Accessibility facts of the cell at view position `pos` and column `c`.
    pub fn a11y(&self, table: &Table, pos: usize, c: Column) -> Option<CellA11y> {
        let r = table.get(*self.view.get(pos)?)?;
        let column_index = self.column_pos(c);
        Some(CellA11y {
            row_index: pos,
            row_count: self.view.len(),
            column_index,
            column_count: self.order.len(),
            row_header_cell: column_index < FROZEN,
            column_header: c.title().to_string(),
            row_header: format!("Balloon {} {}", r.balloon, r.characteristic),
            value: r.text(c).into_owned(),
            status: r.status.word(),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn small() -> Table {
        Table::generate(9, 30).unwrap()
    }

    #[test]
    fn frozen_boundary_and_widths() {
        let t = small();
        let mut g = Grid::new(&t);
        assert!(g.move_column_to(Column::Id, 1));
        assert_eq!(&g.order()[..3], &[Column::Balloon, Column::Id, Column::Characteristic]);
        assert!(!g.move_column_to(Column::Id, 2));
        assert!(!g.move_column_to(Column::Note, 1));
        assert!(g.move_column_to(Column::Note, 2));
        g.set_width(Column::Note, 5);
        assert_eq!(g.width(Column::Note), MIN_WIDTH);
        g.set_width(Column::Note, 5000);
        assert_eq!(g.width(Column::Note), MAX_WIDTH);
    }

    #[test]
    fn stale_results_are_not_installed() {
        let mut t = small();
        let mut g = Grid::new(&t);
        g.apply_mut(&mut t, &Action::SortDesc);
        assert!(g.is_stale());
        let old = g.query().clone();
        g.apply_mut(&mut t, &Action::SortClear);
        assert!(!g.install(&old, vec![3, 2, 1]));
        g.refresh(&t);
        assert!(!g.is_stale());
        assert_eq!(g.view().len(), 30);
    }

    #[test]
    fn ascii_case_insensitive_contains() {
        assert!(contains_ascii_ci("Hole 12", b"hole 1"));
        assert!(contains_ascii_ci("x", b""));
        assert!(!contains_ascii_ci("Hol", b"hole"));
        assert!(contains_ascii_ci("Prüfung", "ü".as_bytes()));
    }
}
