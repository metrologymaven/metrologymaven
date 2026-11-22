//! Acceptance criteria of spec node KRN.NUM.LA.EIG (AC-1 .. AC-5) and edge cases.
//!
//! Expected values come from `fixtures/num/la/make_eig_fixtures.py` (a 400-bit fixed-point
//! Jacobi oracle, exact integer residual and orthogonality checks, and a bit-exact Python
//! model of the pinned rules), or are hand-labelled from the spec's rules, never from this
//! crate. CI runs these tests on Linux x86-64, Windows x86-64, macOS ARM64 and
//! wasm32-wasip1 under wasmtime, so a pass everywhere shows the outputs (and their digest)
//! are bit-identical on every target (AC-5).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_num::la::{mat_mul, sym_eig, sym_eig_with_sweep_limit, LaError, Mat, SymEig, EIG_MAX_SWEEPS};
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

/// The fixture's input stream (same splitmix64 as `mat-ops-v1.txt`).
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

/// Rebuilds symmetric A (row-major `n x n`) for one `eig-v1` case.
fn gen_matrix(seed: u64, kind: &str, n: usize) -> Vec<f64> {
    let mut g = Gen(seed);
    match kind {
        "general" => {
            let mut a = vec![0.0; n * n];
            for i in 0..n {
                a[i * n + i] = g.general();
                for j in (i + 1)..n {
                    let v = g.general();
                    a[i * n + j] = v;
                    a[j * n + i] = v;
                }
            }
            a
        }
        "hadamard" => {
            let d: Vec<i64> = (0..n).map(|_| g.int(-3, 3)).collect();
            (0..n * n).map(|idx| (0..n).map(|l| had(idx / n, l) * d[l] * had(idx % n, l)).sum::<i64>() as f64).collect()
        }
        "clustered" => {
            let mut a = vec![0.0; n * n];
            for i in 0..n {
                let center = g.int(-3, 3) as f64;
                let eps = g.int(-8, 8) as f64 * pow2(-24);
                a[i * n + i] = center + eps;
            }
            let steps = 3 * n;
            for s in 0..steps {
                let mut p = (g.next() % (n as u64)) as usize;
                let mut q = (g.next() % ((n - 1) as u64)) as usize;
                if q >= p {
                    q += 1;
                }
                if p > q {
                    core::mem::swap(&mut p, &mut q);
                }
                let (cs, sn) = if s & 1 == 0 { (0.6_f64, 0.8_f64) } else { (0.28_f64, -0.96_f64) };
                for j in 0..n {
                    let ap = a[p * n + j];
                    let aq = a[q * n + j];
                    a[p * n + j] = cs * ap - sn * aq;
                    a[q * n + j] = sn * ap + cs * aq;
                }
                for i in 0..n {
                    let ap = a[i * n + p];
                    let aq = a[i * n + q];
                    a[i * n + p] = cs * ap - sn * aq;
                    a[i * n + q] = sn * ap + cs * aq;
                }
            }
            for i in 0..n {
                for j in (i + 1)..n {
                    a[j * n + i] = a[i * n + j];
                }
            }
            a
        }
        "graded" => {
            let mut a = vec![0.0; n * n];
            for i in 0..n {
                let gi = grade_exp(i, n, 50);
                a[i * n + i] = g.general() * pow2(-gi);
                for j in (i + 1)..n {
                    let gj = grade_exp(j, n, 50);
                    let v = g.general() * pow2(-((gi + gj) / 2));
                    a[i * n + j] = v;
                    a[j * n + i] = v;
                }
            }
            a
        }
        "scatter3" => {
            assert_eq!(n, 3);
            let e1 = [g.general(), g.general(), g.general()];
            let e2 = [g.general(), g.general(), g.general()];
            let e3 = [g.general(), g.general(), g.general()];
            let noise_exp = if g.next() & 1 == 0 { -24 } else { -40 };
            let noise_scale = pow2(noise_exp);
            let mut pts = Vec::with_capacity(24);
            for _ in 0..24 {
                let u = g.general();
                let v = g.general();
                let w = g.general() * noise_scale;
                pts.push([
                    u * e1[0] + v * e2[0] + w * e3[0],
                    u * e1[1] + v * e2[1] + w * e3[1],
                    u * e1[2] + v * e2[2] + w * e3[2],
                ]);
            }
            let mut a = vec![0.0; 9];
            for i in 0..3 {
                for j in i..3 {
                    let mut s = 0.0_f64;
                    for pt in &pts {
                        s += pt[i] * pt[j];
                    }
                    a[i * 3 + j] = s;
                    a[j * 3 + i] = s;
                }
            }
            a
        }
        "scaled" => {
            let e = if g.next() & 1 == 1 { 400 } else { -400 };
            let sc = pow2(e);
            let mut a = vec![0.0; n * n];
            for i in 0..n {
                a[i * n + i] = g.general() * sc;
                for j in (i + 1)..n {
                    let v = g.general() * sc;
                    a[i * n + j] = v;
                    a[j * n + i] = v;
                }
            }
            a
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

fn eig_bytes(s: &SymEig) -> Vec<u8> {
    let mut out = Vec::new();
    push_f(&mut out, s.values());
    push_f(&mut out, s.vectors().data());
    push_u(&mut out, &[s.diag().rank as u64]);
    push_f(&mut out, &[s.diag().cond]);
    push_u(&mut out, &[u64::from(s.sweeps())]);
    out
}

fn short(bytes: &[u8]) -> String {
    sha256_hex(bytes)["sha256:".len()..][..16].to_string()
}

/// `||A V - V Lambda||_F` and `||A||_F` in floating point.
fn residual(a: &Mat, s: &SymEig) -> (f64, f64) {
    let n = a.rows();
    let av = mat_mul(a, s.vectors()).unwrap();
    let mut diff = Vec::with_capacity(n * n);
    for i in 0..n {
        for j in 0..n {
            let vl = s.vectors().data()[i * n + j] * s.values()[j];
            diff.push(av.data()[i * n + j] - vl);
        }
    }
    (red::norm2(&diff).unwrap(), red::norm2(a.data()).unwrap())
}

/// `||V^T V - I||_F` in floating point.
fn orth_fro(v: &Mat) -> f64 {
    let g = mat_mul(&v.transpose().unwrap(), v).unwrap();
    let n = v.cols();
    let mut diff = Vec::with_capacity(n * n);
    for i in 0..n {
        for j in 0..n {
            diff.push(g.data()[i * n + j] - if i == j { 1.0 } else { 0.0 });
        }
    }
    red::norm2(&diff).unwrap()
}

/// AC-1, AC-5: 300 seeded symmetric matrices 2x2 to 200x200 (general, repeated/Hadamard,
/// clustered, graded, 3x3 near-planar scatter, scaled). Every decomposition hashes to the
/// Python model's value and the whole run to the golden digest; eigenvalues, residual and
/// orthogonality satisfy REQ-006 bounds (with 2 u slack for the floating-point check; exact
/// integer checks run in `make_eig_fixtures.py`).
#[test]
fn ac1_ac5_eig_fixture() {
    let text = read("eig-v1.txt");
    let digest = text.lines().find_map(|l| l.strip_prefix("digest ")).unwrap().to_string();
    let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
    let mut h = Sha256Hasher::new();
    let (mut cases, mut smallest, mut largest) = (0, usize::MAX, 0);
    let mut kinds = std::collections::BTreeMap::new();
    for (li, line) in lines.iter().enumerate().filter(|(_, l)| l.starts_with("case ")) {
        let t: Vec<&str> = line.split(' ').collect();
        let field = |k: &str| t[t.iter().position(|f| *f == k).unwrap() + 1];
        let n: usize = field("n").parse().unwrap();
        let kind = field("kind");
        *kinds.entry(kind.to_string()).or_insert(0) += 1;
        smallest = smallest.min(n);
        largest = largest.max(n);
        let a = Mat::new(n, n, gen_matrix(u64::from_str_radix(field("seed"), 16).unwrap(), kind, n)).unwrap();
        let s = sym_eig(&a, hexf(field("tol"))).unwrap();
        let bytes = eig_bytes(&s);
        assert_eq!(short(&bytes), field("eig"), "case {}", t[1]);
        assert_eq!(s.sweeps().to_string(), field("sweeps"), "case {}", t[1]);
        assert_eq!(s.diag().rank.to_string(), field("rank"), "case {}", t[1]);
        h.update(&bytes);

        let refs: Vec<f64> =
            lines[li + 1].strip_prefix("ref ").unwrap().split(' ').map(|x| x.parse().unwrap()).collect();
        assert_eq!(refs.len(), n);
        let norm2 = refs.iter().fold(0.0_f64, |acc, &x| acc.max(x.abs()));
        let nf = n as f64;
        let eig_bound = (10.0 * nf + 2.0) * U * norm2;
        for (k, (&got, &want)) in s.values().iter().zip(&refs).enumerate() {
            assert!((got - want).abs() <= eig_bound, "case {} eig {k}: {got:e} vs oracle {want:e}", t[1]);
        }
        let (res, norm_a) = residual(&a, &s);
        assert!(res <= (10.0 * nf + 2.0) * U * norm_a, "case {}: residual {res:e}", t[1]);
        let orth = orth_fro(s.vectors());
        assert!(orth <= (10.0 * nf + 2.0) * U, "case {}: ||V^T V - I||_F = {orth:e}", t[1]);
        cases += 1;
    }
    assert_eq!(cases, 300);
    assert_eq!(smallest, 2);
    assert_eq!(largest, 200);
    for kind in ["general", "hadamard", "clustered", "graded", "scatter3", "scaled"] {
        assert!(kinds.get(kind).copied().unwrap_or(0) >= 30, "{kind}: {kinds:?}");
    }
    assert_eq!(h.finish(), digest);
}

fn show_err(e: NumError) -> String {
    let idx = |i: Option<usize>| i.map_or("none".to_string(), |i| i.to_string());
    let (op, s) = match e {
        NumError::NotANumber { op, index } => (op, format!("NotANumber {}", idx(index))),
        NumError::Infinite { op, index } => (op, format!("Infinite {}", idx(index))),
        NumError::Domain { op, detail } => (op, format!("Domain {detail}")),
        NumError::NoConvergence { op, sweeps } => (op, format!("NoConvergence {sweeps}")),
        NumError::NotSymmetric { op, i, j } => (op, format!("NotSymmetric {i} {j}")),
        other => panic!("unexpected error {other:?}"),
    };
    assert_eq!(op, "la.sym_eig", "{e:?}");
    format!("err {s}")
}

fn show(s: &SymEig) -> String {
    format!(
        "ok values {} vectors {} rank {} cond {:016x} sweeps {}",
        hex(s.values()),
        hex(s.vectors().data()),
        s.diag().rank,
        s.diag().cond.to_bits(),
        s.sweeps(),
    )
}

#[test]
fn edge_fixture() {
    let text = read("eig-edge-v1.txt");
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
        let shown = match sym_eig_with_sweep_limit(&a, tol, limit) {
            Ok(s) => {
                let bound = (10.0 * a.rows() as f64 + 2.0) * U;
                assert!(orth_fro(s.vectors()) <= bound, "{name}: ||V^T V - I||_F");
                show(&s)
            }
            Err(e) => show_err(e),
        };
        assert_eq!(shown, get("expect").unwrap().join(" "), "{name}");
        if limit == EIG_MAX_SWEEPS {
            assert_eq!(sym_eig(&a, tol), sym_eig_with_sweep_limit(&a, tol, limit), "{name}");
        }
    }
    assert!(blocks.len() >= 40, "fixture too small: {}", blocks.len());
}

/// AC-2: 3 x 3 scatter matrix of points on a perfect plane z = 0 sampled on a grid ->
/// smallest eigenvalue <= 1e-15 * largest and its eigenvector is (0, 0, 1).
#[test]
fn ac2_plane_scatter_z0() {
    let mut pts = Vec::new();
    for ix in -5..=5 {
        for iy in -3..=3 {
            let x = f64::from(ix) * 0.7 + f64::from(iy) * 0.2;
            let y = f64::from(iy) * 1.1 - f64::from(ix) * 0.15;
            pts.push([x, y, 0.0]);
        }
    }
    let mut s = [0.0_f64; 9];
    for i in 0..3 {
        for j in 0..3 {
            let mut acc = 0.0_f64;
            for p in &pts {
                acc += p[i] * p[j];
            }
            s[i * 3 + j] = acc;
        }
    }
    let a = Mat::new(3, 3, s.to_vec()).unwrap();
    let eig = sym_eig(&a, 1e-12).unwrap();
    assert!(eig.values()[0].abs() <= 1e-15 * eig.values()[2].abs(), "values = {:?}", eig.values());
    let v0 = [eig.vectors().get(0, 0).unwrap(), eig.vectors().get(1, 0).unwrap(), eig.vectors().get(2, 0).unwrap()];
    assert_eq!(v0, [0.0, 0.0, 1.0]);
    assert_eq!(eig.diag().rank, 2);
}

/// AC-3: asymmetric matrix returns `NotSymmetric { i: 0, j: 1 }`; NaN entry returns a typed
/// non-finite error; non-square returns `BadShape`; sweep limit 1 on 50 x 50 returns
/// `NoConvergence`.
#[test]
fn ac3_error_paths() {
    let a = Mat::from_rows(&[[1.0, 1.0], [1.1, 2.0]]).unwrap();
    let e = sym_eig(&a, 1e-12).unwrap_err();
    assert_eq!(e, NumError::NotSymmetric { op: "la.sym_eig", i: 0, j: 1 });
    assert_eq!(LaError::of(&e), Some(LaError::NotSymmetric { i: 0, j: 1 }));
    assert_eq!(e.to_string(), "la.sym_eig: not symmetric at (0, 1)");

    let e = Mat::from_rows(&[[1.0, f64::NAN], [f64::NAN, 1.0]]).unwrap_err();
    assert_eq!(e, NumError::NotANumber { op: "la.mat", index: Some(1) });

    let rect = Mat::from_rows(&[[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]).unwrap();
    assert_eq!(LaError::of(&sym_eig(&rect, 1e-12).unwrap_err()), Some(LaError::BadShape));

    let id = Mat::from_rows(&[[1.0]]).unwrap();
    assert_eq!(sym_eig(&id, f64::NAN).unwrap_err(), NumError::NotANumber { op: "la.sym_eig", index: None });
    assert_eq!(sym_eig(&id, f64::INFINITY).unwrap_err(), NumError::Infinite { op: "la.sym_eig", index: None });
    assert_eq!(LaError::of(&sym_eig(&id, -1.0).unwrap_err()), Some(LaError::BadTolerance));

    let a50 = Mat::new(50, 50, gen_matrix(0xDEAD_BEEF, "general", 50)).unwrap();
    let nc = sym_eig_with_sweep_limit(&a50, 1e-12, 1).unwrap_err();
    assert_eq!(nc, NumError::NoConvergence { op: "la.sym_eig", sweeps: 1 });
    assert_eq!(LaError::of(&nc), Some(LaError::NoConvergence { sweeps: 1 }));
    assert_eq!(EIG_MAX_SWEEPS, 50);
    let ok = sym_eig(&a50, 1e-12).unwrap();
    assert!(ok.sweeps() > 1 && ok.sweeps() <= EIG_MAX_SWEEPS);
}

/// AC-4: identity matrix -> eigenvalues [1, 1, 1] and unit basis in order on every run.
#[test]
fn ac4_identity_determinism() {
    let id = Mat::from_rows(&[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]).unwrap();
    let expected = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    for _ in 0..4 {
        let s = sym_eig(&id, 1e-12).unwrap();
        assert_eq!(s.values(), &[1.0, 1.0, 1.0]);
        assert_eq!(s.vectors().data(), &expected);
        assert_eq!(s.sweeps(), 1);
        assert_eq!((s.diag().rank, s.diag().cond), (3, 1.0));
    }
}

/// REQ-001: when |a_ij - a_ji| <= n u max|a|, only the upper triangle is used.
#[test]
fn upper_triangle_only_when_within_symmetry_tol() {
    let a1 = Mat::from_rows(&[[2.0, 1.0, 0.5], [1.0, 3.0, -0.25], [0.5, -0.25, 4.0]]).unwrap();
    let a2 = Mat::from_rows(&[[2.0, 1.0, 0.5], [1.0 + 2.0 * U, 3.0, -0.25], [0.5 - 2.0 * U, -0.25, 4.0]]).unwrap();
    let r1 = sym_eig(&a1, 1e-12).unwrap();
    let r2 = sym_eig(&a2, 1e-12).unwrap();
    assert_eq!(hex(r1.values()), hex(r2.values()));
    assert_eq!(hex(r1.vectors().data()), hex(r2.vectors().data()));
}

#[test]
fn error_kinds_round_trip() {
    let ns = LaError::NotSymmetric { i: 2, j: 5 };
    assert_eq!(ns.detail(), "not_symmetric");
    assert_eq!(LaError::of(&NumError::NotSymmetric { op: "la.sym_eig", i: 2, j: 5 }), Some(ns));
    assert_eq!(LaError::of(&NumError::NotSymmetric { op: "other", i: 2, j: 5 }), None);
    let tiny = Mat::from_rows(&[[1e-310, 0.0], [0.0, -2e-310]]).unwrap();
    assert_eq!(LaError::of(&sym_eig(&tiny, 0.0).unwrap_err()), Some(LaError::Subnormal));
}

/// Every result buffer has exact capacity (no slack).
#[test]
fn allocation_exact() {
    let f = core::mem::size_of::<f64>();
    let a = Mat::new(7, 7, gen_matrix(42, "general", 7)).unwrap();
    let s = sym_eig(&a, 1e-12).unwrap();
    assert_eq!(s.vectors().heap_bytes(), 49 * f);
    assert_eq!((s.vectors().rows(), s.vectors().cols()), (7, 7));
    let (values, _) = s.into_parts();
    assert_eq!(values.capacity(), 7);
}
