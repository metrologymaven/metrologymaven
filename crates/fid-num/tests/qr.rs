//! Acceptance criteria of spec node KRN.NUM.LA.QR (AC-1 .. AC-5) and the edge cases.
//!
//! Expected values come from `fixtures/num/la/make_qr_fixtures.py` (an exact rational oracle
//! plus a bit-exact Python model of the pinned rules), or are hand-labelled from the spec's
//! rules, never from this crate. CI runs these tests on Linux x86-64, Windows x86-64, macOS
//! ARM64 and wasm32-wasip1 under wasmtime, so a pass everywhere shows the R factors,
//! permutations and `lstsq` outputs (and their digests) are identical on every target (AC-5).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_num::la::{lstsq, mat_mul, qr, qr_pivoted, LaError, Mat, Qr};
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

/// The fixture's input stream (header of `qr-v1.txt`, same splitmix64 as `mat-ops-v1.txt`).
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
    fn int(&mut self, lo: i64, hi: i64) -> f64 {
        let span = (hi - lo + 1) as u64;
        (lo + (self.next() % span) as i64) as f64
    }
}

fn grade_exp(i: usize, cnt: usize, steps: usize) -> i32 {
    if cnt > 1 {
        ((2 * steps * i + (cnt - 1)) / (2 * (cnt - 1))) as i32
    } else {
        0
    }
}

/// Rebuilds `(A row-major, b)` for one `qr-v1` case.
fn gen_matrix(seed: u64, kind: &str, m: usize, n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut g = Gen(seed);
    let a: Vec<f64> = match kind {
        "general" => (0..m * n).map(|_| g.general()).collect(),
        "colgraded" | "rowgraded" => {
            let raw: Vec<f64> = (0..m * n).map(|_| g.general()).collect();
            let mut a = Vec::with_capacity(m * n);
            for i in 0..m {
                for j in 0..n {
                    let e = if kind == "colgraded" { grade_exp(j, n, 40) } else { grade_exp(i, m, 40) };
                    a.push(raw[i * n + j] * pow2(-e));
                }
            }
            a
        }
        "rankdef" => {
            let r = n.div_ceil(2);
            let mut cols: Vec<Vec<f64>> = (0..r).map(|_| (0..m).map(|_| g.int(-8, 8)).collect()).collect();
            for j in r..n {
                let (c1, c2) = (g.int(-3, 3), g.int(-3, 3));
                let col: Vec<f64> = (0..m).map(|i| cols[j % r][i] * c1 + cols[(j + 1) % r][i] * c2).collect();
                cols.push(col);
            }
            (0..m * n).map(|k| cols[k % n][k / n]).collect()
        }
        "tied" => {
            let mut base: Vec<f64> = (0..m).map(|_| g.int(-8, 8)).collect();
            base[0] = 9.0;
            let mut cols = Vec::new();
            for j in 0..n {
                let u = g.next();
                cols.push(
                    (0..m)
                        .map(|i| {
                            let v = base[(i + j) % m];
                            if (u >> (i % 64)) & 1 == 1 {
                                -v
                            } else {
                                v
                            }
                        })
                        .collect::<Vec<f64>>(),
                );
            }
            (0..m * n).map(|k| cols[k % n][k / n]).collect()
        }
        "scaled" => {
            let e = if g.next() & 1 == 1 { 400 } else { -400 };
            (0..m * n).map(|_| g.general() * pow2(e)).collect()
        }
        _ => panic!("unknown kind {kind}"),
    };
    let b = (0..m).map(|_| g.general()).collect();
    (a, b)
}

fn push_f(out: &mut Vec<u8>, v: &[f64]) {
    for x in v {
        out.extend_from_slice(&x.to_bits().to_le_bytes());
    }
}

fn push_u(out: &mut Vec<u8>, v: &[usize]) {
    for &x in v {
        out.extend_from_slice(&(x as u64).to_le_bytes());
    }
}

fn qr_bytes(q: &Qr, thin: &Mat) -> Vec<u8> {
    let mut out = Vec::new();
    push_f(&mut out, q.packed().data());
    push_f(&mut out, q.tau());
    push_u(&mut out, q.perm());
    push_u(&mut out, &[q.diag().rank]);
    push_f(&mut out, &[q.diag().cond]);
    push_f(&mut out, thin.data());
    out
}

fn short(bytes: &[u8]) -> String {
    sha256_hex(bytes)["sha256:".len()..][..16].to_string()
}

/// `||A P - Q R||_F` computed in floating point (products by `red::dot`, which is far more
/// accurate than the bound), and `||A||_F`.
fn reconstruction(a: &Mat, q: &Qr, thin: &Mat) -> (f64, f64) {
    let (m, n) = (a.rows(), a.cols());
    let qr = mat_mul(thin, &q.r().unwrap()).unwrap();
    let mut diff = Vec::with_capacity(m * n);
    for i in 0..m {
        for j in 0..n {
            diff.push(a.get(i, q.perm()[j]).unwrap() - qr.get(i, j).unwrap());
        }
    }
    (red::norm2(&diff).unwrap(), red::norm2(a.data()).unwrap())
}

fn orthogonality(thin: &Mat) -> f64 {
    let n = thin.cols();
    let g = mat_mul(&thin.transpose().unwrap(), thin).unwrap();
    let diff: Vec<f64> = (0..n * n).map(|k| g.data()[k] - if k % (n + 1) == 0 { 1.0 } else { 0.0 }).collect();
    red::norm2(&diff).unwrap()
}

/// AC-1, AC-5: 300 seeded matrices up to 200 x 200. Every factor, thin Q, Q^T b and lstsq
/// result hashes to the Python model's value (whose factors the generator checked exactly
/// against the REQ-006 bounds) and the whole run to the golden digest; the bounds are checked
/// again here in floating point with an allowance of 2 u ||A||_F for the check's own rounding.
#[test]
fn ac1_ac5_qr_fixture() {
    let text = read("qr-v1.txt");
    let digest = text.lines().find_map(|l| l.strip_prefix("digest ")).unwrap().to_string();
    let mut h = Sha256Hasher::new();
    let (mut cases, mut largest, mut deficient, mut tied) = (0, 0, 0, 0);
    for line in text.lines().filter(|l| l.starts_with("case ")) {
        let t: Vec<&str> = line.split(' ').collect();
        let pos = |k: &str| t.iter().position(|f| *f == k).unwrap() + 1;
        let field = |k: &str| t[pos(k)];
        let (m, n): (usize, usize) = (field("m").parse().unwrap(), field("n").parse().unwrap());
        let kind = field("kind");
        let tol = hexf(field("tol"));
        largest = largest.max(m.max(n));
        tied += usize::from(kind == "tied");
        let (data, b) = gen_matrix(u64::from_str_radix(field("seed"), 16).unwrap(), kind, m, n);
        let a = Mat::new(m, n, data).unwrap();
        let ranks = [t[pos("rank")].parse::<usize>().unwrap(), t[pos("rank") + 1].parse::<usize>().unwrap()];
        let mut qp = None;
        for (k, (name, piv)) in [("qr", false), ("qrp", true)].into_iter().enumerate() {
            let q = if piv { qr_pivoted(&a, tol) } else { qr(&a, tol) }.unwrap();
            let thin = q.thin_q().unwrap();
            let bytes = qr_bytes(&q, &thin);
            assert_eq!(short(&bytes), field(name), "case {} {name}", t[1]);
            assert_eq!(q.diag().rank, ranks[k], "case {} {name} rank", t[1]);
            h.update(&bytes);
            let (rec, norm_a) = reconstruction(&a, &q, &thin);
            let nf = n as f64;
            assert!(rec <= (10.0 * nf + 2.0) * U * norm_a, "case {} {name}: reconstruction {rec}", t[1]);
            assert!(orthogonality(&thin) <= (10.0 * nf + 2.0) * U, "case {} {name}: orthogonality", t[1]);
            if piv {
                qp = Some(q);
            }
        }
        let qp = qp.unwrap();
        let qt = qp.apply_qt(&b).unwrap();
        let back = qp.apply_q(&qt).unwrap();
        let mut bytes = Vec::new();
        push_f(&mut bytes, &qt);
        push_f(&mut bytes, &back);
        assert_eq!(short(&bytes), field("qt"), "case {} qt", t[1]);
        h.update(&bytes);
        match lstsq(&a, &b, tol) {
            Ok(r) => {
                let mut bytes = Vec::new();
                push_f(&mut bytes, &r.x);
                push_f(&mut bytes, &[r.residual_norm]);
                push_u(&mut bytes, &[r.diag.rank]);
                push_f(&mut bytes, &[r.diag.cond]);
                assert_eq!(short(&bytes), field("lstsq"), "case {} lstsq", t[1]);
                assert_eq!(r.diag, qp.diag());
                h.update(&bytes);
            }
            Err(e) => {
                let Some(LaError::RankDeficient { rank }) = LaError::of(&e) else { panic!("case {}: {e:?}", t[1]) };
                assert_eq!(format!("rankdef:{rank}"), field("lstsq"), "case {}", t[1]);
                assert_eq!(rank, ranks[1]);
                assert_eq!(qp.diag().cond, f64::INFINITY);
                deficient += 1;
                h.update(&(rank as u64).to_le_bytes());
            }
        }
        cases += 1;
    }
    assert_eq!(cases, 300);
    assert_eq!(largest, 200);
    assert!(deficient >= 30 && tied >= 30, "{deficient} rank-deficient, {tied} tied");
    assert_eq!(h.finish(), digest);
}

/// AC-3: synthetic Longley-like, polynomial and random problems; `lstsq` reproduces the pinned
/// bits and, on every certified case, has at least 7 significant digits in every component
/// against the exact rational solution.
#[test]
fn ac3_lstsq_fixture() {
    let text = read("lstsq-v1.txt");
    let digest = text.lines().find_map(|l| l.strip_prefix("digest ")).unwrap().to_string();
    let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
    let mut h = Sha256Hasher::new();
    let (mut cases, mut certified) = (0, 0);
    for (i, line) in lines.iter().enumerate().filter(|(_, l)| l.starts_with("case ")) {
        let t: Vec<&str> = line.split(' ').collect();
        let field = |k: &str| t[t.iter().position(|f| *f == k).unwrap() + 1];
        let (m, n): (usize, usize) = (field("m").parse().unwrap(), field("n").parse().unwrap());
        let vec_of = |tag: &str| -> Vec<f64> {
            let l = lines[i + 1..].iter().find(|l| l.starts_with(&format!("{tag} "))).unwrap();
            l.split(' ').skip(1).map(hexf).collect()
        };
        let a = Mat::new(m, n, vec_of("a")).unwrap();
        let (b, reference, x_expect) = (vec_of("b"), vec_of("ref"), vec_of("x"));
        let tail: Vec<&str> = lines[i + 1..].iter().find(|l| l.starts_with("rnorm ")).unwrap().split(' ').collect();
        let r = lstsq(&a, &b, hexf(field("tol"))).unwrap();
        assert_eq!(hex(&r.x), hex(&x_expect), "{}", t[1]);
        assert_eq!(r.residual_norm.to_bits(), hexf(tail[1]).to_bits(), "{}", t[1]);
        assert_eq!(r.diag.rank.to_string(), tail[3]);
        assert_eq!(r.diag.cond.to_bits(), hexf(tail[5]).to_bits(), "{}", t[1]);
        if field("certify") == "1" {
            for (x, c) in r.x.iter().zip(&reference) {
                assert!((x - c).abs() <= 1e-7 * c.abs(), "{}: {x} vs certified {c}", t[1]);
            }
            certified += 1;
        }
        let mut bytes = Vec::new();
        push_f(&mut bytes, &r.x);
        push_f(&mut bytes, &[r.residual_norm]);
        push_u(&mut bytes, &[r.diag.rank]);
        push_f(&mut bytes, &[r.diag.cond]);
        h.update(&bytes);
        cases += 1;
    }
    assert!(cases >= 20 && certified >= 18, "{cases} cases, {certified} certified");
    assert_eq!(h.finish(), digest);
}

/// The fixture's `err` notation; checks the operation name.
fn show_err(e: NumError, want_op: &str) -> String {
    let idx = |i: Option<usize>| i.map_or("none".to_string(), |i| i.to_string());
    let (op, s) = match e {
        NumError::NotANumber { op, index } => (op, format!("NotANumber {}", idx(index))),
        NumError::Infinite { op, index } => (op, format!("Infinite {}", idx(index))),
        NumError::Domain { op, detail } => (op, format!("Domain {detail}")),
        NumError::Singular { op, rank } if op == "la.lstsq" => (op, format!("RankDeficient {rank}")),
        other => panic!("unexpected error {other:?}"),
    };
    assert_eq!(op, want_op, "{e:?}");
    format!("err {s}")
}

fn show_qr(q: &Qr) -> String {
    let perm: Vec<String> = q.perm().iter().map(ToString::to_string).collect();
    format!(
        "ok packed {} tau {} perm {} rank {} cond {:016x}",
        hex(q.packed().data()),
        hex(q.tau()),
        perm.join(" "),
        q.diag().rank,
        q.diag().cond.to_bits()
    )
}

/// Signed zeros, the reflector sign, zero columns, tolerance edges, subnormal-scale inputs,
/// overflow positions, error order, infinite condition, Hilbert, and the apply operations.
#[test]
fn edge_fixture() {
    let text = read("qr-edge-v1.txt");
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        if line.starts_with("case ") {
            blocks.push(Vec::new());
        }
        blocks.last_mut().unwrap().push(line);
    }
    for b in &blocks {
        let head: Vec<&str> = b[0].split(' ').collect();
        let (name, op) = (head[1], head[2]);
        let get = |k: &str| {
            b.iter().find_map(|l| {
                let t: Vec<&str> = l.split(' ').collect();
                (t[0] == k).then(|| t[1..].to_vec())
            })
        };
        let at = get("a").unwrap();
        let a = Mat::new(at[0].parse().unwrap(), at[1].parse().unwrap(), at[2..].iter().map(|s| hexf(s)).collect())
            .unwrap();
        let x: Option<Vec<f64>> = get("x").map(|t| t.iter().map(|s| hexf(s)).collect());
        let tol = hexf(get("tol").unwrap()[0]);
        let want_op = format!("la.{op}");
        let shown = match op {
            "qr" | "qr_pivoted" => match if op == "qr" { qr(&a, tol) } else { qr_pivoted(&a, tol) } {
                Ok(q) => show_qr(&q),
                Err(e) => show_err(e, &want_op),
            },
            "lstsq" => match lstsq(&a, &x.unwrap(), tol) {
                Ok(r) => format!(
                    "ok x {} rnorm {:016x} rank {} cond {:016x}",
                    hex(&r.x),
                    r.residual_norm.to_bits(),
                    r.diag.rank,
                    r.diag.cond.to_bits()
                ),
                Err(e) => show_err(e, &want_op),
            },
            "apply_qt" | "apply_q" | "thin_q" => {
                let q = qr_pivoted(&a, tol).unwrap();
                let r = match op {
                    "apply_qt" => q.apply_qt(&x.unwrap()),
                    "apply_q" => q.apply_q(&x.unwrap()),
                    _ => q.thin_q().map(Mat::into_data),
                };
                match r {
                    Ok(v) => format!("ok {}", hex(&v)),
                    Err(e) => show_err(e, &want_op),
                }
            }
            _ => panic!("unknown op {op}"),
        };
        assert_eq!(shown, get("expect").unwrap().join(" "), "{name} ({op})");
    }
    assert!(blocks.len() >= 50, "fixture too small: {}", blocks.len());
}

/// AC-2 (hand-labelled): a 4 x 3 matrix whose three columns all have norm 5 pivots column 0
/// first, and repeated runs give the same permutation.
#[test]
fn ac2_tie_break() {
    let a = Mat::from_rows(&[[3.0, 0.0, 4.0], [4.0, 0.0, 0.0], [0.0, 4.0, 3.0], [0.0, 3.0, 0.0]]).unwrap();
    for j in 0..3 {
        assert_eq!(red::norm2(&a.col(j).unwrap()).unwrap(), 5.0);
    }
    let first = qr_pivoted(&a, 1e-12).unwrap();
    assert_eq!(first.perm()[0], 0);
    for _ in 0..5 {
        let again = qr_pivoted(&a, 1e-12).unwrap();
        assert_eq!(again.perm(), first.perm());
        assert_eq!(again, first);
    }
    // Unpivoted QR keeps the identity permutation.
    assert_eq!(qr(&a, 1e-12).unwrap().perm(), &[0, 1, 2]);
}

/// AC-4 (hand-labelled): two equal columns give `RankDeficient { rank: 2 }` and no solution;
/// a 2 x 3 matrix is `BadShape`.
#[test]
fn ac4_error_paths() {
    let a = Mat::from_rows(&[[1.0, 2.0, 1.0], [3.0, 1.0, 3.0], [2.0, 5.0, 2.0]]).unwrap();
    let e = lstsq(&a, &[1.0, 2.0, 3.0], 1e-12).unwrap_err();
    assert_eq!(LaError::of(&e), Some(LaError::RankDeficient { rank: 2 }));
    assert_eq!(e, NumError::Singular { op: "la.lstsq", rank: 2 });
    // The factorization itself succeeds and reports the rank explicitly.
    let q = qr_pivoted(&a, 1e-12).unwrap();
    assert_eq!((q.diag().rank, q.diag().cond), (2, f64::INFINITY));
    let wide = Mat::from_rows(&[[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]).unwrap();
    for e in [qr(&wide, 1e-12).unwrap_err(), qr_pivoted(&wide, 1e-12).unwrap_err()] {
        assert_eq!(LaError::of(&e), Some(LaError::BadShape));
    }
    assert_eq!(LaError::of(&lstsq(&wide, &[1.0, 2.0], 1e-12).unwrap_err()), Some(LaError::BadShape));
}

/// REQ-001 (hand-labelled): r_kk has the opposite sign of a_kk, positive when a_kk is zero.
#[test]
fn reflector_sign() {
    let r00 = |rows: &[[f64; 2]]| qr(&Mat::from_rows(rows).unwrap(), 0.0).unwrap().packed().data()[0];
    assert_eq!(r00(&[[3.0, 1.0], [4.0, 2.0]]), -5.0);
    assert_eq!(r00(&[[-3.0, 1.0], [4.0, 2.0]]), 5.0);
    assert_eq!(r00(&[[0.0, 1.0], [4.0, 2.0]]), 4.0);
    assert_eq!(r00(&[[-0.0, 1.0], [4.0, 2.0]]), 4.0);
    // 1 x 1: [-3] = Q R with Q = [-1] (tau = 2), R = [3].
    let q = qr(&Mat::from_rows(&[[-3.0]]).unwrap(), 0.0).unwrap();
    assert_eq!((q.packed().data(), q.tau()), (&[3.0][..], &[2.0][..]));
    assert_eq!(q.thin_q().unwrap().data(), &[-1.0]);
}

#[test]
fn error_kinds_round_trip() {
    let sub = NumError::Domain { op: "la.qr", detail: LaError::Subnormal.detail() };
    assert_eq!(LaError::of(&sub), Some(LaError::Subnormal));
    assert_eq!(LaError::of(&NumError::Singular { op: "la.lstsq", rank: 3 }), Some(LaError::RankDeficient { rank: 3 }));
    let singular = NumError::Singular { op: "la.solve_upper", rank: 3 };
    assert_eq!(LaError::of(&singular), Some(LaError::Singular { index: 3 }));
    assert_eq!(LaError::RankDeficient { rank: 0 }.detail(), "rank_deficient");
    // Subnormal-scale input is refused; one subnormal entry at normal scale is accepted.
    let tiny = Mat::from_rows(&[[1e-310], [-2e-310]]).unwrap();
    assert_eq!(LaError::of(&qr(&tiny, 0.0).unwrap_err()), Some(LaError::Subnormal));
    assert!(qr(&Mat::from_rows(&[[1.0], [1e-310]]).unwrap(), 0.0).is_ok());
}

/// Every result buffer has exact capacity (the allocation bound of `KRN.NUM.LA.MAT`).
#[test]
fn allocation_exact() {
    let a = Mat::new(7, 3, (0..21).map(|i| f64::from(i * i % 11) - 5.0).collect()).unwrap();
    let q = qr_pivoted(&a, 1e-12).unwrap();
    let f = core::mem::size_of::<f64>();
    assert_eq!(q.packed().heap_bytes(), 21 * f);
    assert_eq!(q.thin_q().unwrap().heap_bytes(), 21 * f);
    assert_eq!(q.r().unwrap().heap_bytes(), 9 * f);
    let r = lstsq(&a, &[1.0; 7], 1e-12).unwrap();
    assert_eq!(r.x.capacity(), 3);
    assert_eq!(q.apply_qt(&[1.0; 7]).unwrap().capacity(), 7);
}
