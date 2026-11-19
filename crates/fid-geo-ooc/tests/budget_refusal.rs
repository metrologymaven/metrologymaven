//! AC-4 (REQ-GEO.OOC.LAYOUT-005): `to_memory` with a budget smaller than the cloud fails
//! with `BudgetExceeded { needed }` and allocates nothing. Own test binary, because it
//! installs a counting global allocator; one test function, so nothing else allocates
//! concurrently.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use cap::Cap;
use fid_geo_ooc::{memory_needed, to_chunks, to_memory, LayoutParams, MemStore, OocError};
use fid_geo_types::{AttributeSet, FrameRef, PointBuffer, PointCloud};
use std::alloc::System;

#[global_allocator]
static ALLOC: Cap<System> = Cap::new(System, usize::MAX);

#[test]
fn ac4_budget_refusal_allocates_nothing() {
    let n = 20_000usize;
    let pts: Vec<[f64; 3]> = (0..n).map(|i| [(i % 100) as f64, (i / 100) as f64, (i % 7) as f64 * 0.5]).collect();
    let attrs = AttributeSet {
        normal: Some([vec![0.0; n], vec![1.0; n], vec![0.0; n]]),
        intensity: Some((0..n).map(|i| i as f32).collect()),
        ..AttributeSet::default()
    };
    let cloud = PointCloud::new(FrameRef::from_bytes([4; 16]), PointBuffer::from_points(&pts), attrs).unwrap();
    let store = MemStore::new();
    let m = to_chunks(&cloud, &store, &LayoutParams { max_chunk_points: 1000 }).unwrap();
    let needed = memory_needed(&m);
    // The cloud's columns alone are 20 000 * 52 bytes.
    assert!(needed > 20_000 * 52);
    for budget in [0, 1, needed / 2, needed - 1] {
        let before = ALLOC.total_allocated();
        let r = to_memory(&m, &store, budget);
        let after = ALLOC.total_allocated();
        let refused = matches!(r, Err(OocError::BudgetExceeded { needed: x }) if x == needed);
        assert_eq!(after, before, "no allocation before the refusal (budget {budget})");
        assert!(refused, "budget {budget}: {:?}", r.err());
    }
    let back = to_memory(&m, &store, needed).unwrap();
    assert_eq!(back, cloud);
}
