#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The generated fixture `fixtures/viz/report-canvas-200` (make_canvas.py, seed 13):
//! REQ-VIZ.SHELL.SPKRPT-001 content, AC-3 keyboard sequence, drag snapping and the
//! accessibility labels of REQ-VIZ.SHELL.SPKRPT-005.

use fid_spike_ui::rpt::canvas::{Canvas, Kind, Paper};
use fid_spike_ui::rpt::edit::{Action, Designer, GRID};
use fid_spike_ui::rpt::layout::Layout;
use fid_spike_ui::rpt::script::{parse_drag, parse_keyboard, run_drag, run_keyboard, Expect, Schedule};
use fid_spike_ui::rpt::{FIXTURE_A11Y, FIXTURE_CANVAS, FIXTURE_DRAG, FIXTURE_KEYBOARD};

#[test]
fn fixture_has_5_pages_200_blocks_tables_and_one_3d_frame() {
    let c = Canvas::parse(FIXTURE_CANVAS).unwrap();
    assert_eq!(c.pages(), [Paper::A4; 5]);
    assert_eq!(c.blocks().len(), 200);
    assert_eq!(c.count(Kind::Frame3d), 1);
    assert_eq!(c.count(Kind::Band), 10);
    assert!(c.count(Kind::Table) > 0);
    for p in 1..=5 {
        assert_eq!(c.on_page(p).count(), 40, "page {p}");
        assert!(c.on_page(p).any(|b| b.kind == Kind::Table), "page {p} has no table");
    }
    assert_eq!(c.get(2).unwrap().kind, Kind::Frame3d);
}

/// AC-3: the scripted keyboard sequence moves a block 10 grid steps and resizes it with no
/// pointer input, and every step reaches the state the reference model lists.
#[test]
fn ac3_keyboard_sequence_matches_reference() {
    let c = Canvas::parse(FIXTURE_CANVAS).unwrap();
    let steps = parse_keyboard(FIXTURE_KEYBOARD).unwrap();
    assert!(steps.len() >= 100);
    let ok = run_keyboard(&c, &steps);
    let failed: Vec<usize> = ok.iter().enumerate().filter(|(_, ok)| !**ok).map(|(i, _)| i + 1).collect();
    assert!(failed.is_empty(), "steps not matching the fixture: {failed:?}");

    // Independently of the fixture: 10 arrow presses move the block 10 grid steps, and
    // Shift+arrows resize it.
    let mut d = Designer::new(c);
    d.apply(&Action::PageDown);
    d.apply(&Action::Tab);
    let before = d.focused().unwrap().rect;
    for _ in 0..10 {
        d.apply(&Action::Right);
    }
    d.apply(&Action::ShiftRight);
    d.apply(&Action::ShiftDown);
    let after = d.focused().unwrap().rect;
    assert_eq!(after.x - before.x, 10 * GRID);
    assert_eq!((after.w - before.w, after.h - before.h), (GRID, GRID));
    assert_eq!(Expect::of(&d).unwrap().undo, 12);
}

#[test]
fn drag_cases_snap_like_the_reference() {
    let c = Canvas::parse(FIXTURE_CANVAS).unwrap();
    let cases = parse_drag(FIXTURE_DRAG).unwrap();
    assert_eq!(cases.len(), 200);
    assert!(cases.iter().filter(|k| k.guides.x.is_some() || k.guides.y.is_some()).count() > 10);
    let ok = run_drag(&c, &cases);
    let failed: Vec<usize> = ok.iter().enumerate().filter(|(_, ok)| !**ok).map(|(i, _)| i + 1).collect();
    assert!(failed.is_empty(), "drag cases not matching the fixture: {failed:?}");
}

/// REQ-005: each block announces its type, name and position (the screen-reader check
/// itself is manual).
#[test]
fn a11y_labels_match_reference() {
    let c = Canvas::parse(FIXTURE_CANVAS).unwrap();
    let mut n = 0;
    for line in FIXTURE_A11Y.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split('\t').collect();
        let [id, role, label] = f.as_slice() else { panic!("{line:?}") };
        let a = c.a11y(id.parse().unwrap()).unwrap();
        assert_eq!((a.role, a.label.as_str()), (*role, *label));
        n += 1;
    }
    assert_eq!(n, 200);
}

#[test]
fn every_block_is_hit_at_its_centre_at_every_zoom_step() {
    let c = Canvas::parse(FIXTURE_CANVAS).unwrap();
    for zoom in [25.0, 100.0, 400.0] {
        let l = Layout::new(&c, zoom);
        for b in c.blocks() {
            let r = l.to_px(b.page, b.rect).unwrap();
            let hit = l.hit(&c, r.x + r.w / 2.0, r.y + r.h / 2.0);
            assert_eq!(hit, Some(b.id), "block {} at {zoom} %", b.id);
        }
    }
}

#[test]
fn schedule_for_fixture() {
    let c = Canvas::parse(FIXTURE_CANVAS).unwrap();
    let s = Schedule::for_canvas(&c, 10.0).unwrap();
    let drag = c.get(s.drag_block).unwrap();
    let typing = c.get(s.typing_block).unwrap();
    assert_eq!((drag.page, drag.kind, typing.page, typing.kind), (1, Kind::Text, 1, Kind::Text));
    assert_eq!(s.drag_span, 2100 - drag.rect.w);
    assert_eq!(s.drag_seconds, 10.0);
    assert_eq!(s.typing_text.chars().count(), 200);
}
