//! KRN.SOLV.CONIC.PATCH AC-4 (REQ-004): the LDL ordering is a function of the sparsity
//! pattern only, with lowest-index tie-breaks. The expected permutations come from the
//! independent Python reference `fixtures/solv/patch/ldl-order-v1/make_orders.py`; CI runs
//! this test on x86-64 Linux and Windows, ARM64 macOS and wasm32-wasip1, so the
//! permutation is checked identical on every target.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_clarabel::algebra::CscMatrix;
use fid_clarabel::qdldl::min_degree_ordering;
use std::path::PathBuf;

struct Case {
    name: String,
    n: usize,
    colptr: Vec<usize>,
    rowval: Vec<usize>,
    perm: Vec<usize>,
}

fn ints(line: &str, key: &str) -> Vec<usize> {
    let rest = line.strip_prefix(key).unwrap_or_else(|| panic!("expected {key:?}, got {line:?}"));
    rest.split_whitespace().map(|t| t.parse().unwrap()).collect()
}

fn cases() -> Vec<Case> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/solv/patch/ldl-order-v1/orders.txt");
    let text = std::fs::read_to_string(path).unwrap();
    let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
    assert_eq!(lines.len() % 4, 0);
    lines
        .chunks(4)
        .map(|c| {
            let head: Vec<&str> = c[0].split_whitespace().collect();
            assert_eq!(head[0], "case");
            Case {
                name: head[1].to_string(),
                n: head[2].parse().unwrap(),
                colptr: ints(c[1], "colptr"),
                rowval: ints(c[2], "rowval"),
                perm: ints(c[3], "perm"),
            }
        })
        .collect()
}

#[test]
fn ordering_matches_the_reference_on_every_case() {
    let cases = cases();
    assert_eq!(cases.len(), 7);
    for c in cases {
        let nz = c.rowval.len();
        let a = CscMatrix::new(c.n, c.n, c.colptr.clone(), c.rowval.clone(), vec![1.0; nz]);
        assert_eq!(min_degree_ordering(&a).unwrap(), c.perm, "{}", c.name);
    }
}

#[test]
fn ordering_ignores_values() {
    // same pattern, different values (including zeros stored explicitly): same ordering
    for c in cases() {
        let nz = c.rowval.len();
        let vals: Vec<f64> = (0..nz).map(|k| if k % 3 == 0 { 0.0 } else { -(k as f64) * 1.5e3 }).collect();
        let a = CscMatrix::new(c.n, c.n, c.colptr.clone(), c.rowval.clone(), vals);
        assert_eq!(min_degree_ordering(&a).unwrap(), c.perm, "{}", c.name);
    }
}
