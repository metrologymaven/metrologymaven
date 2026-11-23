//! Acceptance criteria of spec node `KRN.LSQ.PLAN2D.ASSIGN` (AC-1..AC-3,
//! REQ-KRN.LSQ.PLAN2D.ASSIGN-001..004, `PlaneFrame` 3D-to-2D projection, and error paths).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use core::f64::consts::PI;
use fid_fit::plan2d_assign::{assign, check_min_points, AssignError, PlaneFrame, Profile2, Seg2};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/fit/assign-v1/");

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{FIX}{name}")).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn hex_f64(s: &str) -> f64 {
    let s = s.trim().trim_matches('"');
    f64::from_bits(u64::from_str_radix(s, 16).unwrap_or_else(|e| panic!("bad hex {s}: {e}")))
}

fn parse_points_hex(doc: &str) -> Vec<[f64; 2]> {
    let start_key = "\"points_hex\":[";
    let start = doc.find(start_key).expect("points_hex") + start_key.len();
    let rest = &doc[start..];
    let mut pts = Vec::new();
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b']' => break,
            b'[' => {
                let close = rest[i..].find(']').expect("pair close") + i;
                let inner = &rest[i + 1..close];
                let mut parts = inner.split(',');
                let x = hex_f64(parts.next().expect("x"));
                let y = hex_f64(parts.next().expect("y"));
                pts.push([x, y]);
                i = close + 1;
            }
            _ => i += 1,
        }
    }
    pts
}

fn parse_expected_segment_of(doc: &str) -> Vec<usize> {
    let start_key = "\"expected_segment_of\":[";
    let start = doc.find(start_key).expect("expected_segment_of") + start_key.len();
    let end = doc[start..].find(']').expect("seg array end") + start;
    doc[start..end].split(',').map(|t| t.trim().parse::<usize>().expect("seg idx")).collect()
}

fn slot_profile() -> Profile2 {
    Profile2::new(vec![
        Seg2::Line { a: [-5.0, -2.0], b: [5.0, -2.0] },
        Seg2::Arc { c: [5.0, 0.0], r: 2.0, start: -0.5 * PI, sweep: PI },
        Seg2::Line { a: [5.0, 2.0], b: [-5.0, 2.0] },
        Seg2::Arc { c: [-5.0, 0.0], r: 2.0, start: 0.5 * PI, sweep: PI },
    ])
    .expect("slot profile")
}

fn rounded_rect_profile() -> Profile2 {
    Profile2::new(vec![
        Seg2::Line { a: [-7.0, -5.0], b: [7.0, -5.0] },
        Seg2::Arc { c: [7.0, -4.0], r: 1.0, start: -0.5 * PI, sweep: 0.5 * PI },
        Seg2::Line { a: [8.0, -4.0], b: [8.0, 4.0] },
        Seg2::Arc { c: [7.0, 4.0], r: 1.0, start: 0.0, sweep: 0.5 * PI },
        Seg2::Line { a: [7.0, 5.0], b: [-7.0, 5.0] },
        Seg2::Arc { c: [-7.0, 4.0], r: 1.0, start: 0.5 * PI, sweep: 0.5 * PI },
        Seg2::Line { a: [-8.0, 4.0], b: [-8.0, -4.0] },
        Seg2::Arc { c: [-7.0, -4.0], r: 1.0, start: PI, sweep: 0.5 * PI },
    ])
    .expect("rounded rect profile")
}

#[test]
fn ac1_slot_and_rounded_rect_10000_points_match_expected_and_ties_go_lower() {
    let slot_doc = read("slot.json");
    let slot_pts = parse_points_hex(&slot_doc);
    let slot_want = parse_expected_segment_of(&slot_doc);
    assert_eq!(slot_pts.len(), 5_000);

    let slot = slot_profile();
    let slot_res = assign(&slot_pts, &slot).expect("slot assign");
    assert_eq!(slot_res.segment_of, slot_want);
    // Explicit equidistant tie points:
    assert_eq!(&slot_res.segment_of[..5], &[0, 0, 1, 2, 0]);
    check_min_points(&slot_res, &slot.default_min_points(false)).expect("slot min points");

    let rect_doc = read("rounded_rect.json");
    let rect_pts = parse_points_hex(&rect_doc);
    let rect_want = parse_expected_segment_of(&rect_doc);
    assert_eq!(rect_pts.len(), 5_000);

    let rect = rounded_rect_profile();
    let rect_res = assign(&rect_pts, &rect).expect("rect assign");
    assert_eq!(rect_res.segment_of, rect_want);
    assert_eq!(&rect_res.segment_of[..9], &[0, 0, 1, 2, 3, 4, 5, 6, 0]);
    check_min_points(&rect_res, &rect.default_min_points(false)).expect("rect min points");

    // Verify per-segment residual signs & statistics sanity (outside positive, inside negative)
    assert!((slot.segments[0].signed_distance([0.0, -3.0]).unwrap() - 1.0).abs() < 1e-12);
    assert!((slot.segments[0].signed_distance([0.0, -1.0]).unwrap() + 1.0).abs() < 1e-12);
    assert!((slot.segments[1].signed_distance([8.0, 0.0]).unwrap() - 1.0).abs() < 1e-12);
    assert!((slot.segments[1].signed_distance([6.0, 0.0]).unwrap() + 1.0).abs() < 1e-12);
}

#[test]
fn ac2_undersampled_second_arc_returns_segment_1() {
    let slot = slot_profile();
    // Seg 0 (line): 3 pts; Seg 1 (arc): ONLY 2 pts (< 3); Seg 2 (line): 3 pts; Seg 3 (arc): 4 pts
    let pts = vec![
        [-2.0, -2.0],
        [0.0, -2.0],
        [2.0, -2.0],
        [7.0, 0.0],
        [6.414, 1.414],
        [2.0, 2.0],
        [0.0, 2.0],
        [-2.0, 2.0],
        [-7.0, 0.0],
        [-6.414, 1.414],
        [-6.414, -1.414],
        [-6.732, 1.0],
    ];
    let assigned = assign(&pts, &slot).expect("assign");
    assert_eq!(assigned.stats[1].count, 2);

    // Default (free arc radius -> arcs need 3): fails with UnderSampled { segment: 1 }
    let err = check_min_points(&assigned, &slot.default_min_points(false)).unwrap_err();
    assert_eq!(err, AssignError::UnderSampled { segment: 1, required: 3, actual: 2 });

    // Fixed arc radius -> arcs need 2: passes!
    check_min_points(&assigned, &slot.default_min_points(true)).expect("fixed radius allows 2 pts on arc");
}

#[test]
fn ac3_canonical_encoding_is_deterministic_and_frame_projection_works() {
    let slot_pts = parse_points_hex(&read("slot.json"));
    let slot = slot_profile();
    let a1 = assign(&slot_pts, &slot).expect("assign 1");
    let a2 = assign(&slot_pts, &slot).expect("assign 2");
    assert_eq!(a1.to_canonical_bytes().unwrap(), a2.to_canonical_bytes().unwrap());
    assert_eq!(a1.digest().unwrap(), a2.digest().unwrap());

    // PlaneFrame 3D -> 2D projection
    let frame = PlaneFrame::new([10.0, 20.0, 30.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]).expect("frame");
    assert_eq!(frame.n, [0.0, 0.0, 1.0]);
    let p2 = frame.project_points(&[[10.0, 20.0, 35.0], [15.0, 18.0, 30.0]]).expect("project");
    assert_eq!(p2, vec![[0.0, 0.0], [5.0, -2.0]]);

    // Non-finite point rejected with Num error
    assert!(matches!(assign(&[[f64::NAN, 0.0]], &slot), Err(AssignError::Num(_))));
    assert!(matches!(assign(&[[0.0, f64::INFINITY]], &slot), Err(AssignError::Num(_))));
}
