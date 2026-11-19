//! AC-4 of spec node GEO.SEL.MODEL: 10^8 indexes forming 10 ranges are stored in
//! O(ranges) memory, under 1 KiB. Kept apart from `tests/model.rs` so the wasm32 job, which
//! runs the fixture tests, does not also run the 10^8-push loop under an interpreter.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_geo_sel::{Domain, IndexSet, IndexSetBuilder};

#[test]
fn ac4_hundred_million_indexes_in_ten_ranges() {
    const RUN: u64 = 10_000_000;
    let mut b = IndexSetBuilder::new(Domain::Points, 20 * RUN);
    // Ranges [2kR, (2k+1)R) for k = 9, 8, ..., 0 (descending range order, ascending within).
    for k in (0..10u64).rev() {
        for i in 2 * k * RUN..(2 * k + 1) * RUN {
            b.push(i).unwrap();
        }
    }
    let s = b.finish();
    assert_eq!(s.len(), 100_000_000);
    assert_eq!(s.ranges().len(), 10);
    for (k, &(start, end)) in s.ranges().iter().enumerate() {
        assert_eq!((start, end), (2 * k as u64 * RUN, (2 * k as u64 + 1) * RUN));
    }
    assert!(s.stored_bytes() < 1024, "{} bytes", s.stored_bytes());
    assert!(s.contains(0) && !s.contains(RUN) && s.contains(19 * RUN - 1) && !s.contains(19 * RUN));
}

#[test]
fn ac4_explicit_ranges_need_no_member_storage() {
    let ranges: Vec<(u64, u64)> = (0..10u64).map(|k| (k * 1_000_000_000, k * 1_000_000_000 + 10_000_000)).collect();
    let s = IndexSet::from_ranges(Domain::Triangles, &ranges, u64::MAX).unwrap();
    assert_eq!(s.len(), 100_000_000);
    assert_eq!(s.ranges().len(), 10);
    assert!(s.stored_bytes() < 1024);
}
