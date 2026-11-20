//! The tree view: the flattened visible-rows array (recomputed only on expand, collapse,
//! filter or reorder), keyboard and mouse rules, multi-select, rename in place and the
//! accessibility facts each row exposes.
//!
//! # Keyboard rules (AC-3; the WAI-ARIA tree pattern)
//!
//! Rows are the visible items in pre-order. Without a filter an item is visible when every
//! ancestor is expanded. With a filter, an item is visible when its name (ASCII
//! case-insensitive) contains the filter or a descendant's does; expanded flags are ignored.
//!
//! - Down / Up / Home / End: move the focus (no wrap); selection = {focus}; anchor = focus.
//! - Right: collapsed parent: expand. Expanded parent (or any parent while a filter is set):
//!   focus and select the next row if it is a child of the focused item. Leaf: nothing.
//! - Left: expanded parent with no filter set: collapse. Otherwise focus and select the
//!   parent (roots: nothing).
//! - Shift+Down / Shift+Up: move the focus one row; selection = the rows from the anchor to
//!   the focus (a hidden anchor is first reset to the old focus). No move: nothing.
//! - Ctrl+Down / Ctrl+Up: move the focus one row; selection unchanged.
//! - Space: toggle the focused item in the selection; anchor = focus.
//! - Expand all / Collapse all: every parent expanded / collapsed; collapse all moves the
//!   focus to the root ancestor of the focused item.
//! - Filter: set the filter; a focus that is no longer visible moves to the first row (none
//!   if there are no rows).
//! - F2: start renaming the focused item (buffer = [`crate::tree::Item::rename_seed`]: its
//!   name, or empty when the name needs escaping). While renaming only Type, Enter and
//!   Escape act. Type: replace the buffer. Enter: commit if the buffer passes
//!   [`crate::tree::valid_name`], else keep the old name (with a filter set the rows are
//!   rebuilt and the focus rule of Filter applies). Escape: cancel.
//! - With no focus, every move key focuses and selects the first row.
//!
//! `fixtures/viz/tree-10k/make_tree.py` implements the same rules independently and writes
//! the expected focus and selection of the scripted sequence.

use crate::tree::{valid_name, Tree};
use std::collections::BTreeSet;

/// Longest filter text in bytes; longer input is cut at a character boundary.
pub const MAX_FILTER_BYTES: usize = 256;

/// One keyboard (or toolbar) action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Arrow down.
    Down,
    /// Arrow up.
    Up,
    /// Home.
    Home,
    /// End.
    End,
    /// Arrow right.
    Right,
    /// Arrow left.
    Left,
    /// Shift + arrow down.
    ShiftDown,
    /// Shift + arrow up.
    ShiftUp,
    /// Ctrl + arrow down.
    CtrlDown,
    /// Ctrl + arrow up.
    CtrlUp,
    /// Space.
    Space,
    /// F2: start renaming.
    F2,
    /// Enter: commit the rename.
    Enter,
    /// Escape: cancel the rename.
    Escape,
    /// Expand every parent.
    ExpandAll,
    /// Collapse every parent.
    CollapseAll,
    /// Replace the rename buffer.
    Type(String),
    /// Set the filter text.
    Filter(String),
}

/// One visible row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    /// Item id.
    pub id: u32,
    /// Level, roots are 1.
    pub level: usize,
}

/// What the platform accessibility API is told about a row (REQ-VIZ.SHELL.SPKTREE-004).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A11yInfo {
    /// Always `"treeitem"`.
    pub role: &'static str,
    /// The escaped name.
    pub name: String,
    /// Level, roots are 1.
    pub level: usize,
    /// `Some(expanded)` for a parent, `None` for a leaf.
    pub expanded: Option<bool>,
    /// Whether the item is selected.
    pub selected: bool,
    /// 1-based position among its siblings.
    pub position_in_set: usize,
    /// Number of siblings, itself included.
    pub size_of_set: usize,
}

/// View state over a [`Tree`].
#[derive(Debug, Clone)]
pub struct TreeView {
    expanded: Vec<bool>,
    filter: String,
    rows: Vec<Row>,
    row_of: Vec<Option<usize>>,
    focus: Option<u32>,
    selection: BTreeSet<u32>,
    anchor: Option<u32>,
    editing: Option<(u32, String)>,
    rebuilds: u64,
}

fn idx(id: u32) -> usize {
    (id as usize).wrapping_sub(1)
}

impl TreeView {
    /// Everything collapsed, no filter, focus on the first row, empty selection.
    pub fn new(tree: &Tree) -> TreeView {
        let mut v = TreeView {
            expanded: vec![false; tree.len()],
            filter: String::new(),
            rows: Vec::new(),
            row_of: vec![None; tree.len()],
            focus: None,
            selection: BTreeSet::new(),
            anchor: None,
            editing: None,
            rebuilds: 0,
        };
        v.rebuild(tree);
        v.focus = v.rows.first().map(|r| r.id);
        v
    }

    /// Visible rows in display order.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Focused item.
    pub fn focus(&self) -> Option<u32> {
        self.focus
    }

    /// Selected items in id order.
    pub fn selection(&self) -> &BTreeSet<u32> {
        &self.selection
    }

    /// The current filter text.
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// The item being renamed and its buffer.
    pub fn editing(&self) -> Option<(u32, &str)> {
        self.editing.as_ref().map(|(id, s)| (*id, s.as_str()))
    }

    /// Row index of an item, if visible.
    pub fn row_of(&self, id: u32) -> Option<usize> {
        self.row_of.get(idx(id)).copied().flatten()
    }

    /// How many times the rows array was rebuilt (navigation never rebuilds it).
    pub fn rebuilds(&self) -> u64 {
        self.rebuilds
    }

    /// Whether `id` is expanded.
    pub fn is_expanded(&self, id: u32) -> bool {
        self.expanded.get(idx(id)).copied().unwrap_or(false)
    }

    /// Recomputes the visible rows. O(items).
    pub fn rebuild(&mut self, tree: &Tree) {
        self.rebuilds += 1;
        let n = tree.len();
        self.expanded.resize(n, false);
        let keep: Option<Vec<bool>> = if self.filter.is_empty() {
            None
        } else {
            let f = self.filter.to_ascii_lowercase();
            let mut keep = vec![false; n];
            // A parent always has a smaller id than its children, so one pass from the end
            // carries every match up to its ancestors.
            for item in tree.items().iter().rev() {
                let i = idx(item.id);
                if item.search_key().contains(f.as_str()) {
                    keep[i] = true;
                }
                if keep[i] {
                    if let Some(p) = item.parent {
                        keep[idx(p)] = true;
                    }
                }
            }
            Some(keep)
        };
        self.rows.clear();
        self.row_of.clear();
        self.row_of.resize(n, None);
        let mut stack: Vec<u32> = tree.roots().iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            let i = idx(id);
            if let Some(k) = &keep {
                if !k.get(i).copied().unwrap_or(false) {
                    continue;
                }
            }
            let level = tree.get(id).map_or(1, |it| it.level);
            if let Some(slot) = self.row_of.get_mut(i) {
                *slot = Some(self.rows.len());
            }
            self.rows.push(Row { id, level });
            if keep.is_some() || self.expanded.get(i).copied().unwrap_or(false) {
                stack.extend(tree.children(id).iter().rev());
            }
        }
    }

    fn step(&self, delta: isize) -> Option<u32> {
        let first = self.rows.first()?.id;
        let Some(k) = self.focus.and_then(|f| self.row_of(f)) else {
            return Some(first);
        };
        let t = k.checked_add_signed(delta)?;
        self.rows.get(t).map(|r| r.id)
    }

    fn focus_select(&mut self, id: u32) {
        self.focus = Some(id);
        self.selection.clear();
        self.selection.insert(id);
        self.anchor = Some(id);
    }

    fn refocus_if_hidden(&mut self) {
        if self.focus.and_then(|f| self.row_of(f)).is_none() {
            self.focus = self.rows.first().map(|r| r.id);
        }
    }

    /// With no focus, every move key focuses and selects the first row. Returns true when
    /// it did.
    fn focus_first_if_none(&mut self) -> bool {
        if self.focus.is_some() {
            return false;
        }
        if let Some(r) = self.rows.first() {
            let id = r.id;
            self.focus_select(id);
        }
        true
    }

    /// Applies one action.
    pub fn apply(&mut self, tree: &mut Tree, action: &Action) {
        if let Some((eid, buf)) = self.editing.as_mut() {
            match action {
                Action::Type(t) => *buf = t.clone(),
                Action::Enter => {
                    let (eid, buf) = (*eid, std::mem::take(buf));
                    self.editing = None;
                    if valid_name(&buf) && tree.rename(eid, &buf).is_ok() && !self.filter.is_empty() {
                        self.rebuild(tree);
                        self.refocus_if_hidden();
                    }
                }
                Action::Escape => self.editing = None,
                _ => {}
            }
            return;
        }
        match action {
            Action::Down | Action::Up => {
                if self.focus_first_if_none() {
                    return;
                }
                if let Some(t) = self.step(if *action == Action::Down { 1 } else { -1 }) {
                    self.focus_select(t);
                }
            }
            Action::Home | Action::End => {
                let r = if *action == Action::Home { self.rows.first() } else { self.rows.last() };
                if let Some(r) = r.copied() {
                    self.focus_select(r.id);
                }
            }
            Action::Right => {
                if self.focus_first_if_none() {
                    return;
                }
                let Some(f) = self.focus else { return };
                if tree.children(f).is_empty() {
                    return;
                }
                if !self.filter.is_empty() || self.is_expanded(f) {
                    if let Some(t) = self.step(1) {
                        if tree.get(t).and_then(|i| i.parent) == Some(f) {
                            self.focus_select(t);
                        }
                    }
                } else {
                    if let Some(e) = self.expanded.get_mut(idx(f)) {
                        *e = true;
                    }
                    self.rebuild(tree);
                }
            }
            Action::Left => {
                if self.focus_first_if_none() {
                    return;
                }
                let Some(f) = self.focus else { return };
                if self.filter.is_empty() && !tree.children(f).is_empty() && self.is_expanded(f) {
                    if let Some(e) = self.expanded.get_mut(idx(f)) {
                        *e = false;
                    }
                    self.rebuild(tree);
                } else if let Some(p) = tree.get(f).and_then(|i| i.parent) {
                    self.focus_select(p);
                }
            }
            Action::ShiftDown | Action::ShiftUp => {
                if self.focus_first_if_none() {
                    return;
                }
                let Some(t) = self.step(if *action == Action::ShiftDown { 1 } else { -1 }) else { return };
                if self.anchor.and_then(|a| self.row_of(a)).is_none() {
                    self.anchor = self.focus;
                }
                self.focus = Some(t);
                if let (Some(a), Some(b)) = (self.anchor.and_then(|a| self.row_of(a)), self.row_of(t)) {
                    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                    self.selection = self.rows.get(lo..=hi).unwrap_or(&[]).iter().map(|r| r.id).collect();
                }
            }
            Action::CtrlDown | Action::CtrlUp => {
                if self.focus_first_if_none() {
                    return;
                }
                if let Some(t) = self.step(if *action == Action::CtrlDown { 1 } else { -1 }) {
                    self.focus = Some(t);
                }
            }
            Action::Space => {
                if let Some(f) = self.focus {
                    if !self.selection.remove(&f) {
                        self.selection.insert(f);
                    }
                    self.anchor = Some(f);
                }
            }
            Action::ExpandAll | Action::CollapseAll => {
                let on = *action == Action::ExpandAll;
                for (i, e) in self.expanded.iter_mut().enumerate() {
                    let id = u32::try_from(i + 1).unwrap_or(u32::MAX);
                    *e = on && !tree.children(id).is_empty();
                }
                if !on {
                    // The root ancestor; levels bound the walk.
                    let mut f = self.focus;
                    while let Some(p) = f.and_then(|id| tree.get(id)).and_then(|i| i.parent) {
                        f = Some(p);
                    }
                    self.focus = f;
                }
                self.rebuild(tree);
            }
            Action::Filter(text) => {
                self.set_filter(tree, text);
            }
            Action::F2 => {
                if let Some(f) = self.focus {
                    let seed = tree.get(f).map(|i| i.rename_seed().to_string()).unwrap_or_default();
                    self.editing = Some((f, seed));
                }
            }
            Action::Type(_) | Action::Enter | Action::Escape => {}
        }
    }

    /// Sets the filter (cut to [`MAX_FILTER_BYTES`] at a character boundary).
    pub fn set_filter(&mut self, tree: &Tree, text: &str) {
        let mut end = text.len().min(MAX_FILTER_BYTES);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        self.filter = text.get(..end).unwrap_or("").to_string();
        self.rebuild(tree);
        self.refocus_if_hidden();
    }

    /// Toggles a parent's expanded flag (the disclosure triangle).
    pub fn toggle(&mut self, tree: &Tree, id: u32) {
        if tree.children(id).is_empty() {
            return;
        }
        if let Some(e) = self.expanded.get_mut(idx(id)) {
            *e = !*e;
            self.rebuild(tree);
            self.refocus_if_hidden();
        }
    }

    /// Mouse click on a row: plain = select only it; ctrl = toggle it; shift = range from
    /// the anchor. The focus moves to the row in every case.
    pub fn click(&mut self, id: u32, ctrl: bool, shift: bool) {
        if self.row_of(id).is_none() {
            return;
        }
        if shift {
            if self.anchor.and_then(|a| self.row_of(a)).is_none() {
                self.anchor = self.focus.or(Some(id));
            }
            self.focus = Some(id);
            if let (Some(a), Some(b)) = (self.anchor.and_then(|a| self.row_of(a)), self.row_of(id)) {
                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                self.selection = self.rows.get(lo..=hi).unwrap_or(&[]).iter().map(|r| r.id).collect();
            }
        } else if ctrl {
            self.focus = Some(id);
            if !self.selection.remove(&id) {
                self.selection.insert(id);
            }
            self.anchor = Some(id);
        } else {
            self.focus_select(id);
        }
    }

    /// Accessibility facts for the row at `row` (None past the end).
    pub fn a11y(&self, tree: &Tree, row: usize) -> Option<A11yInfo> {
        let r = self.rows.get(row)?;
        let item = tree.get(r.id)?;
        let sibs = tree.siblings(r.id);
        let pos = sibs.iter().position(|&s| s == r.id).map_or(1, |p| p + 1);
        let expanded =
            if tree.children(r.id).is_empty() { None } else { Some(!self.filter.is_empty() || self.is_expanded(r.id)) };
        Some(A11yInfo {
            role: "treeitem",
            name: item.display().to_string(),
            level: r.level,
            expanded,
            selected: self.selection.contains(&r.id),
            position_in_set: pos,
            size_of_set: sibs.len(),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    // 1 A
    //   2 B
    //     3 c
    //   4 D
    // 5 E
    const T: &str =
        "1\t0\tpart\tpass\tA\n2\t1\tgroup\tpass\tB\n3\t2\tplane\tpass\tc\n4\t1\tplane\tpass\tD\n5\t0\tpart\tpass\tE\n";

    fn ids(v: &TreeView) -> Vec<u32> {
        v.rows().iter().map(|r| r.id).collect()
    }

    #[test]
    fn expand_collapse_and_navigation() {
        let mut t = Tree::parse(T).unwrap();
        let mut v = TreeView::new(&t);
        assert_eq!(ids(&v), [1, 5]);
        assert_eq!(v.focus(), Some(1));
        v.apply(&mut t, &Action::Right);
        assert_eq!(ids(&v), [1, 2, 4, 5]);
        let before = v.rebuilds();
        v.apply(&mut t, &Action::Right);
        v.apply(&mut t, &Action::Down);
        v.apply(&mut t, &Action::Up);
        assert_eq!(v.rebuilds(), before, "navigation never rebuilds the rows");
        assert_eq!(v.focus(), Some(2));
        v.apply(&mut t, &Action::ExpandAll);
        assert_eq!(ids(&v), [1, 2, 3, 4, 5]);
        v.apply(&mut t, &Action::End);
        v.apply(&mut t, &Action::ShiftUp);
        v.apply(&mut t, &Action::ShiftUp);
        assert_eq!(v.selection().iter().copied().collect::<Vec<_>>(), [3, 4, 5]);
        v.apply(&mut t, &Action::CollapseAll);
        assert_eq!(v.focus(), Some(1), "focus 3 moves to its root ancestor");
        assert_eq!(ids(&v), [1, 5]);
    }

    #[test]
    fn filter_keeps_ancestors_and_refocuses() {
        let mut t = Tree::parse(T).unwrap();
        let mut v = TreeView::new(&t);
        v.apply(&mut t, &Action::End);
        v.apply(&mut t, &Action::Filter("C".into()));
        assert_eq!(ids(&v), [1, 2, 3]);
        assert_eq!(v.focus(), Some(1));
        let a = v.a11y(&t, 1).unwrap();
        assert_eq!(a.expanded, Some(true));
        v.apply(&mut t, &Action::Filter("zz".into()));
        assert!(v.rows().is_empty());
        assert_eq!(v.focus(), None);
        v.apply(&mut t, &Action::Down);
        assert_eq!(v.focus(), None);
        v.set_filter(&t, &"é".repeat(200));
        assert!(v.filter().len() <= MAX_FILTER_BYTES);
    }

    #[test]
    fn rename_in_place() {
        let mut t = Tree::parse(T).unwrap();
        let mut v = TreeView::new(&t);
        v.apply(&mut t, &Action::F2);
        assert_eq!(v.editing(), Some((1, "A")));
        v.apply(&mut t, &Action::Down);
        assert_eq!(v.focus(), Some(1), "keys other than Type/Enter/Escape are ignored");
        v.apply(&mut t, &Action::Type("Probe \u{202e}x".into()));
        v.apply(&mut t, &Action::Enter);
        assert_eq!(v.editing(), None);
        assert_eq!(t.get(1).unwrap().display(), "Probe \\u{202e}x");
        v.apply(&mut t, &Action::F2);
        v.apply(&mut t, &Action::Type(String::new()));
        v.apply(&mut t, &Action::Enter);
        assert_eq!(t.get(1).unwrap().display(), "Probe \\u{202e}x");
    }

    #[test]
    fn mouse_multi_select_and_a11y() {
        let mut t = Tree::parse(T).unwrap();
        let mut v = TreeView::new(&t);
        v.apply(&mut t, &Action::ExpandAll);
        v.click(2, false, false);
        v.click(5, false, true);
        assert_eq!(v.selection().iter().copied().collect::<Vec<_>>(), [2, 3, 4, 5]);
        v.click(3, true, false);
        assert_eq!(v.selection().iter().copied().collect::<Vec<_>>(), [2, 4, 5]);
        v.click(99, false, false);
        assert_eq!(v.focus(), Some(3));
        let a = v.a11y(&t, 3).unwrap();
        assert_eq!(
            a,
            A11yInfo {
                role: "treeitem",
                name: "D".into(),
                level: 2,
                expanded: None,
                selected: true,
                position_in_set: 2,
                size_of_set: 2
            }
        );
        assert_eq!(v.a11y(&t, 0).unwrap().expanded, Some(true));
        assert_eq!(v.a11y(&t, 9), None);
        v.toggle(&t, 1);
        assert_eq!(ids(&v), [1, 5]);
        assert_eq!(v.focus(), Some(1), "hidden focus moves to the first row");
    }
}
