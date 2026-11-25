#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The characteristic table model (VIZ.SHELL.SPKTABLE) against the independent Python
//! reference in `fixtures/viz/char-table-100k/make_table.py`: the generated table (digest
//! and sample rows), sort and filter results, the AC-4 keyboard sequence with the state after
//! every step (including the AC-3 exact-decimal commit, the copied TSV and the accessibility
//! label of the focused cell).

use fid_spike_ui::table::data::{fnv1a, Column, Status, Table, FIXTURE_ROWS};
use fid_spike_ui::table::grid::{compute_view, Action, Dir, Grid, Query, FROZEN};
use fid_spike_ui::table::script::{exact_decimal, parse_keyboard, run_keyboard, state, EXACT_TEXT};
use fid_spike_ui::table::FIXTURE_KEYBOARD;
use std::sync::OnceLock;

fn fixture(name: &str) -> String {
    let p = format!("{}/../../fixtures/viz/char-table-100k/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(p).unwrap().replace("\r\n", "\n")
}

fn table() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(Table::fixture)
}

fn ids_digest(ids: &[u32]) -> String {
    let text: String = ids.iter().map(|i| format!("{i}\n")).collect();
    format!("{:016x}", fnv1a(text.as_bytes()))
}

fn ids_line(ids: &[u32]) -> String {
    if ids.is_empty() {
        return "-".into();
    }
    ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
}

fn parse_sort(s: &str) -> Option<(Column, Dir)> {
    if s == "-" {
        return None;
    }
    let (c, d) = s.split_once(':').unwrap();
    Some((Column::from_key(c).unwrap(), if d == "asc" { Dir::Asc } else { Dir::Desc }))
}

/// REQ-001 and the fixture: 10^5 rows x 14 columns, byte-identical to the reference.
#[test]
fn generated_table_matches_reference_digest() {
    let t = table();
    assert_eq!(t.len(), FIXTURE_ROWS as usize);
    assert_eq!(Column::ALL.len(), 14);
    let tsv = t.to_tsv();
    let want = fixture("digest.txt");
    let got = format!("rows\t{}\nbytes\t{}\nfnv1a64\t{:016x}\n", t.len(), tsv.len(), fnv1a(tsv.as_bytes()));
    assert_eq!(got, want);
}

#[test]
fn sample_rows_match_reference() {
    let t = table();
    let sample = Table::parse_tsv(&fixture("sample.tsv")).unwrap();
    assert!(sample.len() > 100);
    for r in &sample {
        assert_eq!(t.get(r.id), Some(r), "row {}", r.id);
    }
}

/// REQ-001: every row has a status chip with an icon and a word; all four statuses occur.
#[test]
fn status_chips_and_frozen_columns() {
    let t = table();
    assert!(t.status_counts().iter().all(|n| *n > 0), "{:?}", t.status_counts());
    for s in [Status::Pass, Status::Warn, Status::Fail, Status::NotMeasured] {
        assert!(!s.icon().is_empty() && !s.word().is_empty());
    }
    assert_eq!(FROZEN, 2);
    let g = Grid::new(t);
    assert_eq!(&g.order()[..FROZEN], &[Column::Id, Column::Balloon]);
}

#[test]
fn sorts_match_reference() {
    let t = table();
    let text = fixture("sort.tsv");
    let mut n = 0;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split('\t').collect();
        let sort = parse_sort(&format!("{}:{}", f[0], f[1]));
        let ids = compute_view(t, &Query { sort, filter: String::new() });
        assert_eq!(ids.len().to_string(), f[2], "{line}");
        assert_eq!(ids_digest(&ids), f[3], "{}:{}", f[0], f[1]);
        assert_eq!(ids_line(&ids[..20]), f[4]);
        assert_eq!(ids_line(&ids[ids.len() - 20..]), f[5]);
        n += 1;
    }
    assert_eq!(n, 7);
}

#[test]
fn filters_match_reference() {
    let t = table();
    let text = fixture("filter.tsv");
    let mut n = 0;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split('\t').collect();
        let q = Query { sort: parse_sort(f[0]), filter: f[1].to_string() };
        let ids = compute_view(t, &q);
        assert_eq!(ids.len().to_string(), f[2], "{line}");
        assert_eq!(ids_digest(&ids), f[3], "{line}");
        assert_eq!(ids_line(&ids[..ids.len().min(20)]), f[4], "{line}");
        n += 1;
    }
    assert_eq!(n, 12);
}

/// AC-4 (with AC-3 and the copy, fly-to, reorder and resize steps inside it): every step of
/// the scripted keyboard sequence leaves the state the reference model expects.
#[test]
fn ac4_keyboard_sequence_matches_reference() {
    let steps = parse_keyboard(FIXTURE_KEYBOARD).unwrap();
    assert_eq!(steps.len(), 95);
    let got = run_keyboard(table(), &steps);
    for (s, g) in steps.iter().zip(&got) {
        assert_eq!(g, &s.expect, "step {} ({:?})", s.n, s.action);
    }
    // Every operation REQ-003 lists is in the sequence: navigate, sort, filter, edit,
    // commit, cancel, copy.
    let has = |p: &dyn Fn(&Action) -> bool| steps.iter().any(|s| p(&s.action));
    assert!(has(&|a| matches!(a, Action::Down | Action::Right | Action::PageUp | Action::CtrlEnd)));
    assert!(has(&|a| matches!(a, Action::SortAsc)) && has(&|a| matches!(a, Action::SortDesc)));
    assert!(has(&|a| matches!(a, Action::FocusFilter)));
    assert!(has(&|a| matches!(a, Action::F2)) && has(&|a| matches!(a, Action::Type(_))));
    assert!(has(&|a| matches!(a, Action::Enter)) && has(&|a| matches!(a, Action::Escape)));
    assert!(has(&|a| matches!(a, Action::Copy)));
}

/// The first Copy of the sequence puts exactly the reference's TSV on the clipboard.
#[test]
fn copy_text_matches_reference() {
    let steps = parse_keyboard(FIXTURE_KEYBOARD).unwrap();
    let mut t = table().clone();
    let mut g = Grid::new(&t);
    for s in &steps {
        if g.apply_mut(&mut t, &s.action) {
            g.refresh(&t);
        }
        if s.action == Action::Copy {
            break;
        }
    }
    assert_eq!(g.clipboard(), Some(fixture("copy.tsv").as_str()));
}

/// AC-3: "0.0500" is stored exactly, with no binary round trip.
#[test]
fn ac3_exact_decimal() {
    assert_eq!(exact_decimal(table()).as_deref(), Some(EXACT_TEXT));
    // Other spellings of the same value are kept as typed too.
    let mut t = Table::generate(9, 5).unwrap();
    let mut g = Grid::new(&t);
    for _ in 0..g.column_pos(Column::Upper) {
        g.apply_mut(&mut t, &Action::Right);
    }
    for typed in ["0.05", "0.050000000000", "00.0500"] {
        g.apply_mut(&mut t, &Action::Type(typed.into()));
        g.apply_mut(&mut t, &Action::Enter);
        assert_eq!(t.get(1).unwrap().upper, typed);
    }
    let st = state(&g, &t);
    assert_eq!(st[7], "00.0500");
}
