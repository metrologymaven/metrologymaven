//! Acceptance criteria of spec node KRN.NUM.LA.SVD (AC-1 .. AC-5) and the edge cases.
//!
//! Expected values come from `fixtures/num/la/make_svd_fixtures.py` (a fixed-point Jacobi
//! oracle, an exact integer reconstruction check and a bit-exact Python model of the pinned
//! rules), or are hand-labelled from the spec's rules, never from this crate. CI runs these
//! tests on Linux x86-64, Windows x86-64, macOS ARM64 and wasm32-wasip1 under wasmtime, so a
//! pass everywhere shows the factors (and their digest) are identical on every target (AC-5).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_num::la::{mat_mul, svd, svd_with_sweep_limit, LaError, Mat, Svd, MAX_SWEEPS};
use fid_num::{red, NumError};
use fid_testkit::{sha256_hex, Sha256Hasher};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/num/la/");
const U: f64 = 1.0 / 9_007_199_254_740_992.0; // 2^-53

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{FIX}{name}")).unwrap()
}

fn hexf(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn hex(v: &[f64]) -> String {
    v.iter().map(|x| format!("{:016x}", x.to_bits())).collect::<Vec<_>>().join(" ")
}

fn pow2(k: i32) -> f64 {
    f64::from_bits(((k + 1023) as u64) << 52)
}

/// The fixture's input stream (header of `svd-v1.txt`, same splitmix64 as `mat-ops-v1.txt`).
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
    fn int(&mut self, lo: i64, hi: i64) -> i64 {
        let span = (hi - lo + 1) as u64;
        lo + (self.next() % span) as i64
    }
}

fn grade_exp(i: usize, cnt: usize, steps: usize) -> i32 {
    if cnt > 1 {
        ((2 * steps * i + (cnt - 1)) / (2 * (cnt - 1))) as i32
    } else {
        0
    }
}

fn had(i: usize, k: usize) -> i64 {
    if (i & k).count_ones() & 1 == 1 {
        -1
    } else {
        1
    }
}

/// Rebuilds A (row-major) for one `svd-v1` case.
fn gen_matrix(seed: u64, kind: &str, m: usize, n: usize) -> Vec<f64> {
    let mut g = Gen(seed);
    match kind {
        "general" => (0..m * n).map(|_| g.general()).collect(),
        "graded" => {
            let raw: Vec<f64> = (0..m * n).map(|_| g.general()).collect();
            (0..m * n).map(|k| raw[k] * pow2(-grade_exp(k % n, n, 50))).collect()
        }
        "hadamard" => {
            let kk = m.min(n);
            let d: Vec<i64> = (0..kk).map(|_| g.int(-3, 3)).collect();
            (0..m * n)
                .map(|idx| (0..kk).map(|l| had(idx / n, l) * d[l] * had(idx % n, l)).sum::<i64>() as f64)
                .collect()
        }
        "rankdef" => {
            let r = n.div_ceil(2);
            let mut cols: Vec<Vec<f64>> = (0..r).map(|_| (0..m).map(|_| g.int(-8, 8) as f64).collect()).collect();
            for j in r..n {
                let (c1, c2) = (g.int(-3, 3) as f64, g.int(-3, 3) as f64);
                let col: Vec<f64> = (0..m).map(|i| cols[j % r][i] * c1 + cols[(j + 1) % r][i] * c2).collect();
                cols.push(col);
            }
            (0..m * n).map(|k| cols[k % n][k / n]).collect()
        }
        "scaled" => {
            let e = if g.next() & 1 == 1 { 400 } else { -400 };
            (0..m * n).map(|_| g.general() * pow2(e)).collect()
        }
        _ => panic!("unknown kind {kind}"),
    }
}

fn push_f(out: &mut Vec<u8>, v: &[f64]) {
    for x in v {
        out.extend_from_slice(&x.to_bits().to_le_bytes());
    }
}

fn push_u(out: &mut Vec<u8>, v: &[u64]) {
    for &x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
}

fn svd_bytes(s: &Svd) -> Vec<u8> {
    let mut out = Vec::new();
    push_f(&mut out, s.u().data());
    push_f(&mut out, s.sigma());
    push_f(&mut out, s.v().data());
    push_u(&mut out, &[s.diag().rank as u64]);
    push_f(&mut out, &[s.diag().cond]);
    push_u(&mut out, &[u64::from(s.sweeps()), u64::from(s.singular())]);
    out
}

fn short(bytes: &[u8]) -> String {
    sha256_hex(bytes)["sha256:".len()..][..16].to_string()
}

/// `||A - U diag(sigma) V^T||_F` in floating point (products by `red::dot`) and `||A||_F`.
fn reconstruction(a: &Mat, s: &Svd) -> (f64, f64) {
    let (m, k) = (s.u().rows(), s.u().cols());
    let us: Vec<f64> = (0..m * k).map(|idx| s.u().data()[idx] * s.sigma()[idx % k]).collect();
    let usv = mat_mul(&Mat::new(m, k, us).unwrap(), &s.v().transpose().unwrap()).unwrap();
    let diff: Vec<f64> = a.data().iter().zip(usv.data()).map(|(x, y)| x - y).collect();
    (red::norm2(&diff).unwrap(), red::norm2(a.data()).unwrap())
}

/// AC-1, AC-5: 300 seeded matrices up to 200 x 200. Every factorization hashes to the Python
/// model's value and the whole run to the golden digest; each singular value is within
/// 10 n u sigma_max of the oracle and the reconstruction within 10 n u ||A||_F (checked here
/// in floating point with an allowance of 2 u for the check's own rounding; the generator
/// checked both exactly).
#[test]
fn ac1_ac5_svd_fixture() {
    let text = read("svd-v1.txt");
    let digest = text.lines().find_map(|l| l.strip_prefix("digest ")).unwrap().to_string();
    let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
    let mut h = Sha256Hasher::new();
    let (mut cases, mut largest, mut wide, mut deficient) = (0, 0, 0, 0);
    let mut kinds = std::collections::BTreeMap::new();
    for (li, line) in lines.iter().enumerate().filter(|(_, l)| l.starts_with("case ")) {
        let t: Vec<&str> = line.split(' ').collect();
        let field = |k: &str| t[t.iter().position(|f| *f == k).unwrap() + 1];
        let (m, n): (usize, usize) = (field("m").parse().unwrap(), field("n").parse().unwrap());
        let kind = field("kind");
        *kinds.entry(kind.to_string()).or_insert(0) += 1;
        largest = largest.max(m.max(n));
        wide += usize::from(m < n);
        let a = Mat::new(m, n, gen_matrix(u64::from_str_radix(field("seed"), 16).unwrap(), kind, m, n)).unwrap();
        let s = svd(&a, hexf(field("tol"))).unwrap();
        let bytes = svd_bytes(&s);
        assert_eq!(short(&bytes), field("svd"), "case {}", t[1]);
        assert_eq!(s.sweeps().to_string(), field("sweeps"), "case {}", t[1]);
        assert_eq!(s.diag().rank.to_string(), field("rank"), "case {}", t[1]);
        deficient += usize::from(s.diag().rank < m.min(n));
        h.update(&bytes);

        let refs: Vec<f64> =
            lines[li + 1].strip_prefix("ref ").unwrap().split(' ').map(|x| x.parse().unwrap()).collect();
        assert_eq!(refs.len(), m.min(n));
        let nf = n as f64;
        let bound = (10.0 * nf + 2.0) * U * refs[0];
        for (k, (&got, &want)) in s.sigma().iter().zip(&refs).enumerate() {
            assert!((got - want).abs() <= bound, "case {} sigma {k}: {got:e} vs oracle {want:e}", t[1]);
        }
        let (rec, norm_a) = reconstruction(&a, &s);
        assert!(rec <= (10.0 * nf + 2.0) * U * norm_a, "case {}: reconstruction {rec:e}", t[1]);
        cases += 1;
    }
    assert_eq!(cases, 300);
    assert_eq!(largest, 200);
    for kind in ["general", "graded", "hadamard", "rankdef", "scaled"] {
        assert!(kinds.get(kind).copied().unwrap_or(0) >= 30, "{kind}: {kinds:?}");
    }
    assert!(wide >= 30 && deficient >= 30, "{wide} wide, {deficient} rank-deficient");
    assert_eq!(h.finish(), digest);
}

/// The fixture's `err` notation; checks the operation name.
fn show_err(e: NumError) -> String {
    let idx = |i: Option<usize>| i.map_or("none".to_string(), |i| i.to_string());
    let (op, s) = match e {
        NumError::NotANumber { op, index } => (op, format!("NotANumber {}", idx(index))),
        NumError::Infinite { op, index } => (op, format!("Infinite {}", idx(index))),
        NumError::Domain { op, detail } => (op, format!("Domain {detail}")),
        NumError::NoConvergence { op, sweeps } => (op, format!("NoConvergence {sweeps}")),
        other => panic!("unexpected error {other:?}"),
    };
    assert_eq!(op, "la.svd", "{e:?}");
    format!("err {s}")
}

fn show(s: &Svd) -> String {
    format!(
        "ok u {} sigma {} v {} rank {} cond {:016x} sweeps {} singular {}",
        hex(s.u().data()),
        hex(s.sigma()),
        hex(s.v().data()),
        s.diag().rank,
        s.diag().cond.to_bits(),
        s.sweeps(),
        u8::from(s.singular())
    )
}

/// Signed zeros, zero rows and columns, ties, tolerance edges, the scaling extremes, overflow,
/// subnormal-scale input, infinite condition, error order and the sweep limit.
#[test]
fn edge_fixture() {
    let text = read("svd-edge-v1.txt");
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        if line.starts_with("case ") {
            blocks.push(Vec::new());
        }
        blocks.last_mut().unwrap().push(line);
    }
    for b in &blocks {
        let name = b[0].split(' ').nth(1).unwrap();
        let get = |k: &str| {
            b.iter().find_map(|l| {
                let t: Vec<&str> = l.split(' ').collect();
                (t[0] == k).then(|| t[1..].to_vec())
            })
        };
        let at = get("a").unwrap();
        let a = Mat::new(at[0].parse().unwrap(), at[1].parse().unwrap(), at[2..].iter().map(|s| hexf(s)).collect())
            .unwrap();
        let tol = hexf(get("tol").unwrap()[0]);
        let limit: u16 = get("limit").unwrap()[0].parse().unwrap();
        let shown = match svd_with_sweep_limit(&a, tol, limit) {
            Ok(s) => show(&s),
            Err(e) => show_err(e),
        };
        assert_eq!(shown, get("expect").unwrap().join(" "), "{name}");
        if limit == MAX_SWEEPS {
            assert_eq!(svd(&a, tol), svd_with_sweep_limit(&a, tol, limit), "{name}");
        }
    }
    assert!(blocks.len() >= 40, "fixture too small: {}", blocks.len());
}

/// AC-2 (hand-labelled): diag(1, 3, 2) gives sigma = [3, 2, 1]; V (and U) has the columns
/// e1, e2, e0, each a unit vector with a positive largest entry; no pair rotates.
#[test]
fn ac2_diagonal() {
    let a = Mat::from_rows(&[[1.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 2.0]]).unwrap();
    let s = svd(&a, 1e-12).unwrap();
    assert_eq!(s.sigma(), &[3.0, 2.0, 1.0]);
    let perm = [0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
    assert_eq!(s.v().data(), &perm);
    assert_eq!(s.u().data(), &perm);
    assert_eq!(s.sweeps(), 1);
    assert_eq!((s.diag().rank, s.diag().cond, s.singular()), (3, 3.0, false));
    // Negative diagonal entries: V keeps positive entries, U carries the signs.
    let a = Mat::from_rows(&[[-1.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, -2.0]]).unwrap();
    let s = svd(&a, 1e-12).unwrap();
    assert_eq!(s.sigma(), &[3.0, 2.0, 1.0]);
    assert_eq!(s.v().data(), &perm);
    assert_eq!(s.u().data(), &[0.0, 0.0, -1.0, 1.0, 0.0, 0.0, 0.0, -1.0, 0.0]);
}

/// AC-3: A and -A give identical sigma (bits) and V (values), and U columns that differ only
/// in sign, for tall, square and wide matrices.
#[test]
fn ac3_sign() {
    for (seed, m, n) in [(1_u64, 9, 4), (2, 6, 6), (3, 4, 9), (4, 1, 5), (5, 30, 30)] {
        let mut g = Gen(seed);
        let d: Vec<f64> = (0..m * n).map(|_| g.general()).collect();
        let a = Mat::new(m, n, d.clone()).unwrap();
        let na = Mat::new(m, n, d.iter().map(|x| -x).collect()).unwrap();
        let (s, t) = (svd(&a, 1e-12).unwrap(), svd(&na, 1e-12).unwrap());
        assert_eq!(hex(s.sigma()), hex(t.sigma()), "{m}x{n}");
        assert_eq!(s.v().data(), t.v().data(), "{m}x{n}");
        let k = m.min(n);
        for j in 0..k {
            let cu: Vec<f64> = (0..m).map(|i| s.u().data()[i * k + j]).collect();
            let cn: Vec<f64> = (0..m).map(|i| t.u().data()[i * k + j]).collect();
            let same = cu == cn;
            let neg = cu.iter().zip(&cn).all(|(x, y)| *x == -*y);
            assert!(same || neg, "{m}x{n} column {j}");
        }
        // V columns: largest-magnitude entry (lowest index on ties) is positive.
        for j in 0..k {
            let col: Vec<f64> = (0..n).map(|i| s.v().data()[i * k + j]).collect();
            let mut best = 0;
            for i in 0..n {
                if col[i].abs() > col[best].abs() {
                    best = i;
                }
            }
            assert!(col[best] > 0.0, "{m}x{n} column {j}");
        }
    }
}

/// AC-4: a matrix with an infinity cannot be built (`la.mat`, typed error), so `svd` never
/// sees one; a NaN tolerance is refused; a 50 x 50 random matrix with the sweep limit set to
/// 1 gives `NoConvergence { sweeps: 1 }` and no factors.
#[test]
fn ac4_error_paths() {
    let e = Mat::from_rows(&[[1.0, f64::INFINITY], [0.0, 1.0]]).unwrap_err();
    assert_eq!(e, NumError::Infinite { op: "la.mat", index: Some(1) });
    let e = Mat::new(1, 2, vec![f64::NAN, 1.0]).unwrap_err();
    assert_eq!(e, NumError::NotANumber { op: "la.mat", index: Some(0) });
    let a = Mat::from_rows(&[[1.0]]).unwrap();
    assert_eq!(svd(&a, f64::NAN).unwrap_err(), NumError::NotANumber { op: "la.svd", index: None });
    assert_eq!(svd(&a, f64::INFINITY).unwrap_err(), NumError::Infinite { op: "la.svd", index: None });
    assert_eq!(LaError::of(&svd(&a, -1.0).unwrap_err()), Some(LaError::BadTolerance));

    let mut g = Gen(50);
    let a = Mat::new(50, 50, (0..2500).map(|_| g.general()).collect()).unwrap();
    let e = svd_with_sweep_limit(&a, 1e-12, 1).unwrap_err();
    assert_eq!(e, NumError::NoConvergence { op: "la.svd", sweeps: 1 });
    assert_eq!(LaError::of(&e), Some(LaError::NoConvergence { sweeps: 1 }));
    assert_eq!(e.to_string(), "la.svd: no convergence after 1 sweeps");
    // The default limit converges on the same matrix.
    let s = svd(&a, 1e-12).unwrap();
    assert!(s.sweeps() > 1 && s.sweeps() <= MAX_SWEEPS);
    assert_eq!(MAX_SWEEPS, 60);
}

#[test]
fn error_kinds_round_trip() {
    let nc = LaError::NoConvergence { sweeps: 7 };
    assert_eq!(nc.detail(), "no_convergence");
    assert_eq!(LaError::of(&NumError::NoConvergence { op: "la.svd", sweeps: 7 }), Some(nc));
    // Not a linear-algebra operation.
    assert_eq!(LaError::of(&NumError::NoConvergence { op: "other", sweeps: 7 }), None);
    let tiny = Mat::from_rows(&[[1e-310], [-2e-310]]).unwrap();
    assert_eq!(LaError::of(&svd(&tiny, 0.0).unwrap_err()), Some(LaError::Subnormal));
}

/// Every result buffer has exact capacity (the allocation bound of `KRN.NUM.LA.MAT`).
#[test]
fn allocation_exact() {
    let f = core::mem::size_of::<f64>();
    for (m, n) in [(7, 3), (3, 7)] {
        let a = Mat::new(m, n, (0..m * n).map(|i| ((i * i % 11) as f64) - 5.0).collect()).unwrap();
        let s = svd(&a, 1e-12).unwrap();
        assert_eq!(s.u().heap_bytes(), m * 3 * f);
        assert_eq!(s.v().heap_bytes(), n * 3 * f);
        assert_eq!((s.u().rows(), s.u().cols(), s.v().rows(), s.v().cols()), (m, 3, n, 3));
        let (_, sigma, _) = s.into_parts();
        assert_eq!(sigma.capacity(), 3);
    }
}
