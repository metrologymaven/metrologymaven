#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The generated fixture `fixtures/viz/tree-10k` (make_tree.py, seed 5) and AC-3.

use fid_spike_ui::script::parse_keyboard;
use fid_spike_ui::tree::Tree;
use fid_spike_ui::view::{Action, TreeView};
use fid_spike_ui::{run_keyboard, FIXTURE_KEYBOARD, FIXTURE_TREE};

#[test]
fn fixture_has_10k_items_6_levels() {
    let t = Tree::parse(FIXTURE_TREE).unwrap();
    assert_eq!(t.len(), 10_000);
    assert_eq!(t.depth(), 6);
    assert_eq!(t.roots().len(), 4);
    // make_tree.py LEVEL_SIZES.
    let mut per_level = [0usize; 6];
    for i in t.items() {
        per_level[i.level - 1] += 1;
    }
    assert_eq!(per_level, [4, 16, 80, 400, 1900, 7600]);
}

#[test]
fn hostile_fixture_names_are_escaped() {
    let t = Tree::parse(FIXTURE_TREE).unwrap();
    let shown: Vec<&str> = t.items().iter().map(|i| i.display()).collect();
    assert!(shown.contains(&"point 8 \\u{202e}gnp.exe"));
    assert!(shown.contains(&"point\\u{200b}78"));
    assert!(shown.contains(&"C:\\\\probe\\\\point 778"));
    assert!(shown.iter().all(|s| !s.contains('\u{202e}') && !s.contains('\u{200b}')));
}

/// AC-3: the scripted keyboard sequence reaches the focus and selection the fixture lists.
#[test]
fn ac3_keyboard_sequence_reaches_expected_items() {
    let mut t = Tree::parse(FIXTURE_TREE).unwrap();
    let steps = parse_keyboard(FIXTURE_KEYBOARD).unwrap();
    assert_eq!(steps.len(), 48);
    let ok = run_keyboard(&mut t, &steps);
    let failed: Vec<usize> = ok.iter().enumerate().filter(|(_, ok)| !**ok).map(|(i, _)| i + 1).collect();
    assert!(failed.is_empty(), "steps not matching the fixture: {failed:?}");
}

#[test]
fn expand_all_shows_every_item_and_a11y_levels_match() {
    let mut t = Tree::parse(FIXTURE_TREE).unwrap();
    let mut v = TreeView::new(&t);
    assert_eq!(v.rows().len(), 4);
    v.apply(&mut t, &Action::ExpandAll);
    assert_eq!(v.rows().len(), 10_000);
    for (k, r) in v.rows().iter().enumerate().step_by(97) {
        let a = v.a11y(&t, k).unwrap();
        assert_eq!(a.role, "treeitem");
        assert_eq!(a.level, r.level);
        assert_eq!(a.expanded.is_some(), !t.children(r.id).is_empty());
        assert!(a.position_in_set >= 1 && a.position_in_set <= a.size_of_set);
    }
    // Pre-order ids: with everything expanded row k shows id k + 1.
    assert!(v.rows().iter().enumerate().all(|(k, r)| r.id as usize == k + 1));
    v.apply(&mut t, &Action::CollapseAll);
    assert_eq!(v.rows().len(), 4);
}

#[test]
fn drag_reorder_changes_row_order_only_among_siblings() {
    let mut t = Tree::parse(FIXTURE_TREE).unwrap();
    let roots = t.roots().to_vec();
    t.move_before(roots[3], roots[0]).unwrap();
    let mut v = TreeView::new(&t);
    let ids: Vec<u32> = v.rows().iter().map(|r| r.id).collect();
    assert_eq!(ids, [roots[3], roots[0], roots[1], roots[2]]);
    assert!(t.move_before(2, roots[1]).is_err());
    v.rebuild(&t);
    assert_eq!(v.rows().len(), 4);
}
