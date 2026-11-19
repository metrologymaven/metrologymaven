//! Acceptance criteria of spec node KRN.NUM.LA.MAT (AC-1 .. AC-5) and the edge cases.
//!
//! Expected values come from `fixtures/num/la/make_la_fixtures.py` (an exact rational oracle
//! plus a bit-exact Python model of the pinned rules), never from this crate. CI runs these
//! tests on Linux x86-64, Windows x86-64, macOS ARM64 and wasm32-wasip1 under wasmtime, so a
//! pass everywhere shows the output bits and digests are identical on every target.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_num::la::{
    cond_triangular, mat_mul, mat_t_vec, mat_vec, rank_by_rule, solve_lower, solve_upper, Factorization, LaError,
    Mat, COND_ITERATIONS, MAX_DIM, MAX_MAT_BYTES,
};
use fid_num::NumError;
use fid_testkit::{sha256_hex, Sha256Hasher};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/num/la/");

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{FIX}{name}")).unwrap()
}

fn hexf(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn pow2(k: i32) -> f64 {
    f64::from_bits(((k + 1023) as u64) << 52)
}

/// The fixture's input stream (see the header of `mat-ops-v1.txt`).
struct Gen(u64);

impl Gen {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn general(&mut self) -> f64 {
        let u = self.next();
        ((u >> 11) as f64 * pow2(-52) - 1.0) * pow2((u & 15) as i32 - 8)
    }
    fn diag(&mut self) -> f64 {
        let u = self.next();
        let v = (u >> 12) as f64 * pow2(-51) + 1.0;
        if (u >> 11) & 1 == 1 {
            -v
        } else {
            v
        }
    }
    fn off(&mut self, k: usize) -> f64 {
        let u = self.next();
        ((u >> 11) as f64 * pow2(-52) - 1.0) * pow2(-((usize::BITS - k.leading_zeros()) as i32))
    }
    fn vec(&mut self, n: usize) -> Vec<f64> {
        (0..n).map(|_| self.general()).collect()
    }
    fn tri(&mut self, k: usize, upper: bool) -> Vec<f64> {
        let mut d = Vec::with_capacity(k * k);
        for i in 0..k {
            for j in 0..k {
                d.push(if i == j {
                    self.diag()
                } else if (j > i) == upper {
                    self.off(k)
                } else {
                    self.general()
                });
            }
        }
        d
    }
}

fn bits_of(v: &[f64]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_bits().to_le_bytes()).collect()
}

/// AC-1: 500 seeded cases from 1x1 to 200x200; every result hashes to the Python model's
/// value (which the generator checked against the exact oracle within 2 n u), and the whole
/// run to the golden digest.
#[test]
fn ac1_mat_ops_fixture() {
    let text = read("mat-ops-v1.txt");
    let digest = text.lines().find_map(|l| l.strip_prefix("digest ")).unwrap().to_string();
    let mut h = Sha256Hasher::new();
    let mut n_cases = 0;
    let mut largest = 0;
    for line in text.lines().filter(|l| l.starts_with("case ")) {
        let t: Vec<&str> = line.split(' ').collect();
        let field = |k: &str| t[t.iter().position(|f| *f == k).unwrap() + 1];
        let num = |k: &str| field(k).parse::<usize>().unwrap();
        let (m, k, n) = (num("m"), num("k"), num("n"));
        largest = largest.max(m.max(k).max(n));
        let mut g = Gen(u64::from_str_radix(field("seed"), 16).unwrap());
        let a = Mat::new(m, k, g.vec(m * k)).unwrap();
        let x = g.vec(k);
        let w = g.vec(m);
        let b = Mat::new(k, n, g.vec(k * n)).unwrap();
        let up = Mat::new(k, k, g.tri(k, true)).unwrap();
        let bu = g.vec(k);
        let lo = Mat::new(k, k, g.tri(k, false)).unwrap();
        let bl = g.vec(k);
        let results = [
            ("mat_vec", mat_vec(&a, &x).unwrap()),
            ("mat_t_vec", mat_t_vec(&a, &w).unwrap()),
            ("mat_mul", mat_mul(&a, &b).unwrap().into_data()),
            ("solve_upper", solve_upper(&up, &bu, 0.0).unwrap()),
            ("solve_lower", solve_lower(&lo, &bl, 0.0).unwrap()),
        ];
        for (op, v) in &results {
            let bytes = bits_of(v);
            assert_eq!(&sha256_hex(&bytes)[..16], field(op), "case {} {op}", t[1]);
            h.update(&bytes);
        }
        n_cases += 1;
    }
    assert_eq!(n_cases, 500);
    assert_eq!(largest, 200);
    assert_eq!(format!("sha256:{}", h.finish()), digest);
}

fn vals(toks: &[&str]) -> Vec<f64> {
    toks.iter().map(|t| hexf(t)).collect()
}

fn mat_tok(toks: &[&str]) -> Mat {
    let (r, c) = (toks[0].parse().unwrap(), toks[1].parse().unwrap());
    Mat::new(r, c, vals(&toks[2..])).unwrap()
}

/// Same notation as the fixture's `expect` line; checks the operation name of every error.
fn show(r: Result<Vec<f64>, NumError>, op: &str) -> String {
    let name = match op {
        "rank" => "la.rank_by_rule",
        "cond" => "la.cond_triangular",
        _ => "",
    };
    let want = if name.is_empty() { format!("la.{op}") } else { name.to_string() };
    let idx = |i: Option<usize>| i.map_or("none".to_string(), |i| i.to_string());
    match r {
        Ok(v) if v.is_empty() => "ok".to_string(),
        Ok(v) => format!("ok {}", v.iter().map(|x| format!("{:016x}", x.to_bits())).collect::<Vec<_>>().join(" ")),
        Err(e) => {
            let (o, s) = match e {
                NumError::NotANumber { op, index } => (op, format!("NotANumber {}", idx(index))),
                NumError::Infinite { op, index } => (op, format!("Infinite {}", idx(index))),
                NumError::Domain { op, detail } => (op, format!("Domain {detail}")),
                NumError::Singular { op, rank } => (op, format!("Singular {rank}")),
                NumError::EmptyInput { op } => (op, "EmptyInput none".to_string()),
                other => panic!("unexpected error {other:?}"),
            };
            assert_eq!(o, want, "{e:?}");
            format!("err {s}")
        }
    }
}

/// Signed zeros, subnormals, cancellation, overflow positions, error order, the ignored
/// triangle, Kahan and Hilbert systems, the rank rule and the estimator's error paths.
#[test]
fn edge_fixture() {
    let text = read("mat-edge-v1.txt");
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        if line.starts_with("case ") {
            blocks.push(Vec::new());
        }
        blocks.last_mut().unwrap().push(line);
    }
    let mut seen = 0;
    for b in &blocks {
        let head: Vec<&str> = b[0].split(' ').collect();
        let (name, op) = (head[1], head[2]);
        let get = |k: &str| {
            b.iter().find_map(|l| {
                let t: Vec<&str> = l.split(' ').collect();
                (t[0] == k).then(|| t[1..].to_vec())
            })
        };
        let a = get("a").unwrap();
        let x = get("x");
        let tol = get("tol").map(|t| hexf(t[0]));
        let got = match op {
            "rank" => rank_by_rule(&vals(&a), tol.unwrap()).map(|r| vec![r as f64]),
            "mat_vec" => mat_vec(&mat_tok(&a), &vals(&x.unwrap())),
            "mat_t_vec" => mat_t_vec(&mat_tok(&a), &vals(&x.unwrap())),
            "mat_mul" => mat_mul(&mat_tok(&a), &mat_tok(&x.unwrap())).map(Mat::into_data),
            "solve_upper" => solve_upper(&mat_tok(&a), &vals(&x.unwrap()), tol.unwrap()),
            "solve_lower" => solve_lower(&mat_tok(&a), &vals(&x.unwrap()), tol.unwrap()),
            "cond" => cond_triangular(&mat_tok(&a), tol.unwrap()).map(|c| vec![c]),
            _ => panic!("unknown op {op}"),
        };
        let expect = get("expect").unwrap().join(" ");
        let shown = match (op, &got) {
            ("rank", Ok(v)) => format!("rank {}", v[0]),
            _ => show(got, op),
        };
        assert_eq!(shown, expect, "{name} ({op})");
        seen += 1;
    }
    assert!(seen >= 60, "fixture too small: {seen}");
}

/// AC-2: the estimate matches the Python model bit for bit and lies within a factor of 10
/// of the exact condition number (a rigorous interval [lo, hi] from the oracle).
#[test]
fn ac2_cond_fixture() {
    let text = read("cond-v1.txt");
    let digest = text.lines().find_map(|l| l.strip_prefix("digest ")).unwrap().to_string();
    let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
    let mut h = Sha256Hasher::new();
    let (mut cases, mut max_cond) = (0, 0.0_f64);
    for (i, line) in lines.iter().enumerate().filter(|(_, l)| l.starts_with("case ")) {
        let t: Vec<&str> = line.split(' ').collect();
        let field = |k: &str| t[t.iter().position(|f| *f == k).unwrap() + 1];
        let n: usize = field("n").parse().unwrap();
        let data: Vec<f64> = lines[i + 1..=i + n].iter().flat_map(|l| l.split(' ').map(hexf)).collect();
        let r = Mat::new(n, n, data).unwrap();
        let est = cond_triangular(&r, hexf(field("tol"))).unwrap();
        assert_eq!(est.to_bits(), hexf(field("est")).to_bits(), "{}", t[1]);
        let (lo, hi) = (hexf(field("lo")), hexf(field("hi")));
        assert!(lo <= hi && est >= lo / 10.0 && est <= hi * 10.0, "{}: {est} vs [{lo}, {hi}]", t[1]);
        if lo > max_cond {
            max_cond = lo;
        }
        h.update(&est.to_bits().to_le_bytes());
        cases += 1;
    }
    assert!(cases >= 80, "fixture too small: {cases}");
    assert!(max_cond >= 1e14, "largest condition number {max_cond}");
    assert_eq!(format!("sha256:{}", h.finish()), digest);
}

fn domain(op: &'static str, k: LaError) -> NumError {
    NumError::Domain { op, detail: k.detail() }
}

/// AC-3: a NaN, an infinite entry, 0 rows or 5000 columns is a typed error (hand-labelled
/// from the spec's rules), in the pinned check order.
#[test]
fn ac3_construction_errors() {
    let op = "la.mat";
    assert_eq!(
        Mat::from_rows(&[[1.0, 2.0], [f64::NAN, 4.0]]),
        Err(NumError::NotANumber { op, index: Some(2) })
    );
    assert_eq!(
        Mat::new(2, 2, vec![1.0, f64::NEG_INFINITY, f64::NAN, 0.0]),
        Err(NumError::Infinite { op, index: Some(1) })
    );
    let none: [[f64; 3]; 0] = [];
    assert_eq!(Mat::from_rows(&none), Err(domain(op, LaError::BadShape)));
    assert_eq!(Mat::new(0, 3, vec![]), Err(domain(op, LaError::BadShape)));
    assert_eq!(Mat::from_rows(&[vec![0.0; 5000]]), Err(domain(op, LaError::TooLarge)));
    assert_eq!(Mat::new(1, 5000, vec![0.0; 5000]), Err(domain(op, LaError::TooLarge)));
    assert_eq!(Mat::new(MAX_DIM + 1, 1, vec![0.0; MAX_DIM + 1]), Err(domain(op, LaError::TooLarge)));
    assert!(Mat::new(1, MAX_DIM, vec![0.0; MAX_DIM]).is_ok());
    // Zero dimension is checked before size; size before shape; shape before finiteness.
    assert_eq!(Mat::new(0, 5000, vec![]), Err(domain(op, LaError::BadShape)));
    assert_eq!(Mat::new(1, 5000, vec![f64::NAN]), Err(domain(op, LaError::TooLarge)));
    assert_eq!(Mat::new(2, 2, vec![f64::NAN; 3]), Err(domain(op, LaError::BadShape)));
    assert_eq!(Mat::from_rows(&[vec![1.0, 2.0], vec![f64::NAN]]), Err(domain(op, LaError::BadShape)));
    assert_eq!(Mat::from_rows(&[Vec::<f64>::new()]), Err(domain(op, LaError::BadShape)));
}

/// AC-4: r_33 = 1e-20, r_11 = 1, rel_tol = 1e-12: `Singular { index: 2 }` and no solution.
#[test]
fn ac4_singular_index() {
    let r = Mat::from_rows(&[[1.0, 0.5, 0.25], [0.0, 1.0, 0.5], [0.0, 0.0, 1e-20]]).unwrap();
    let e = solve_upper(&r, &[1.0, 2.0, 3.0], 1e-12).unwrap_err();
    assert_eq!(e, NumError::Singular { op: "la.solve_upper", rank: 2 });
    assert_eq!(LaError::of(&e), Some(LaError::Singular { index: 2 }));
    let e = cond_triangular(&r, 1e-12).unwrap_err();
    assert_eq!(LaError::of(&e), Some(LaError::Singular { index: 2 }));
    // The same factor stored as lower triangular.
    let e = solve_lower(&r.transpose(), &[1.0, 2.0, 3.0], 1e-12).unwrap_err();
    assert_eq!(LaError::of(&e), Some(LaError::Singular { index: 2 }));
}

/// AC-5: singular values [3, 1, 1e-13, 0] with rel_tol 1e-12 have rank 2.
#[test]
fn ac5_rank_rule() {
    assert_eq!(rank_by_rule(&[3.0, 1.0, 1e-13, 0.0], 1e-12), Ok(2));
    let f = Factorization { rank: 2, cond: 3e13, rel_tol: 1e-12 };
    assert_eq!((f.rank, f.cond.to_bits(), f.rel_tol), (2, 3e13_f64.to_bits(), 1e-12));
    assert_eq!(rank_by_rule(&[], 1e-12), Err(NumError::EmptyInput { op: "la.rank_by_rule" }));
}

#[test]
fn error_kinds_round_trip() {
    for k in [LaError::BadShape, LaError::TooLarge, LaError::BadTolerance, LaError::BadSpectrum] {
        assert_eq!(LaError::of(&domain("la.mat_vec", k)), Some(k));
    }
    assert_eq!(LaError::of(&NumError::Domain { op: "fmt", detail: "bad_shape" }), None);
    assert_eq!(LaError::of(&NumError::Singular { op: "other", rank: 1 }), None);
    assert_eq!(LaError::of(&NumError::Infinite { op: "la.mat_vec", index: Some(0) }), None);
}

#[test]
fn views_and_transpose() {
    let a = Mat::from_rows(&[[1.0, 2.0, 3.0], [4.0, 5.0, -0.0]]).unwrap();
    assert_eq!((a.rows(), a.cols()), (2, 3));
    assert_eq!(a.row(1).unwrap(), &[4.0, 5.0, -0.0]);
    assert_eq!(a.col(2).unwrap()[1].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(a.get(0, 1), Ok(2.0));
    assert_eq!(a.row(2), Err(domain("la.row", LaError::BadShape)));
    assert_eq!(a.col(3), Err(domain("la.col", LaError::BadShape)));
    assert_eq!(a.get(2, 0), Err(domain("la.get", LaError::BadShape)));
    let t = a.transpose();
    assert_eq!((t.rows(), t.cols()), (3, 2));
    assert_eq!(t.data(), &[1.0, 4.0, 2.0, 5.0, 3.0, -0.0]);
    assert_eq!(t.transpose(), a);
    // mat_t_vec(A, x) is mat_vec(A^T, x) bit for bit (same dot over the same order).
    let x = [0.1, -0.7];
    assert_eq!(mat_t_vec(&a, &x).unwrap(), mat_vec(&t, &x).unwrap());
}

/// Allocation bounds: the per-matrix byte bound is charged at `size_of::<f64>()` per entry,
/// and matrices hold exactly `rows * cols` entries (no growth slack).
#[test]
fn allocation_bounds() {
    assert_eq!(MAX_DIM, 4096);
    assert!(MAX_MAT_BYTES >= MAX_DIM * MAX_DIM * core::mem::size_of::<f64>());
    let a = Mat::from_rows(&[vec![1.0; 37], vec![2.0; 37]]).unwrap();
    assert_eq!(a.heap_bytes(), 2 * 37 * core::mem::size_of::<f64>());
    let c = mat_mul(&a.transpose(), &a).unwrap();
    assert_eq!(c.heap_bytes(), 37 * 37 * core::mem::size_of::<f64>());
    assert!(c.heap_bytes() <= MAX_MAT_BYTES);
    assert_eq!(COND_ITERATIONS, 8);
}

/// The estimator is invariant under power-of-two scaling (exact when no entry turns
/// subnormal) and reads only the upper triangle.
#[test]
fn cond_scaling_and_triangle() {
    let rows = [[2.0, -1.0, 0.5], [0.0, 1e-3, 3.0], [0.0, 0.0, 7.0]];
    let r = Mat::from_rows(&rows).unwrap();
    let c = cond_triangular(&r, 1e-14).unwrap();
    for k in [-900, -1, 1, 600] {
        let s: Vec<f64> = r.data().iter().map(|v| v * pow2(k)).collect();
        assert_eq!(cond_triangular(&Mat::new(3, 3, s).unwrap(), 1e-14).unwrap().to_bits(), c.to_bits(), "2^{k}");
    }
    let mut noisy = r.data().to_vec();
    noisy[3] = 1e300;
    noisy[6] = -5.0;
    noisy[7] = f64::MAX;
    assert_eq!(cond_triangular(&Mat::new(3, 3, noisy).unwrap(), 1e-14).unwrap().to_bits(), c.to_bits());
}
