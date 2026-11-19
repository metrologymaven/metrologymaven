//! Acceptance criteria of spec node KRN.NUM.RED (AC-1 .. AC-4) and the adversarial cases.
//!
//! Expected values come from `fixtures/num/make_red_fixtures.py` (an exact rational oracle
//! plus a bit-exact Python model of the documented rule), never from this crate. CI runs these
//! tests on Linux x86-64, Windows x86-64, macOS ARM64 and wasm32-wasip1 under wasmtime, so a
//! pass everywhere shows the result bits are identical on every target (AC-2).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_num::red::{self, BLOCK};
use fid_num::NumError;
use fid_testkit::Sha256Hasher;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/num/");

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{FIX}{name}")).unwrap()
}

fn hexf(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

/// Position of a double on the line of all doubles (+0 and -0 both 0).
fn ordered(x: f64) -> i128 {
    let b = x.to_bits();
    if b >> 63 == 1 {
        -i128::from(b & !(1 << 63))
    } else {
        i128::from(b)
    }
}

fn ulp_dist(a: f64, b: f64) -> u128 {
    (ordered(a) - ordered(b)).unsigned_abs()
}

/// Same notation as the fixture's `expect` column.
fn show(r: Result<f64, NumError>, op: &str) -> String {
    let idx = |i: Option<usize>| i.map_or("none".to_string(), |i| i.to_string());
    let check_op = |o: &str| assert_eq!(o, format!("red.{op}"), "op name");
    match r {
        Ok(x) => format!("ok:{:016x}", x.to_bits()),
        Err(NumError::NotANumber { op: o, index }) => {
            check_op(o);
            format!("err:NotANumber:{}", idx(index))
        }
        Err(NumError::Infinite { op: o, index }) => {
            check_op(o);
            format!("err:Infinite:{}", idx(index))
        }
        Err(NumError::LengthMismatch { op: o, a, b }) => {
            check_op(o);
            format!("err:LengthMismatch:{a}:{b}")
        }
        Err(NumError::EmptyInput { op: o }) => {
            check_op(o);
            "err:EmptyInput".to_string()
        }
        Err(e) => panic!("unexpected error {e:?}"),
    }
}

/// Expands `hex` and `hex*count` tokens.
fn values<'a>(toks: impl Iterator<Item = &'a str>) -> Vec<f64> {
    let mut v = Vec::new();
    for t in toks {
        match t.split_once('*') {
            Some((h, n)) => v.extend(std::iter::repeat_n(hexf(h), n.parse().unwrap())),
            None => v.push(hexf(t)),
        }
    }
    v
}

fn splitmix_values(seed: u64, n: usize) -> Vec<f64> {
    let mut st = seed;
    (0..n)
        .map(|_| {
            st = st.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = st;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            let u = z ^ (z >> 31);
            let e = 993 + ((u >> 52) & 63);
            f64::from_bits(((u >> 63) << 63) | (e << 52) | (u & ((1 << 52) - 1)))
        })
        .collect()
}

/// AC-3: an independent test-side implementation of the block-and-tree rule, written from
/// the spec text: all block pairs first, then an explicit-stack pairwise reduction.
mod reference {
    fn neumaier(xs: &[f64]) -> (f64, f64) {
        let (mut s, mut c) = (-0.0_f64, -0.0_f64);
        for &x in xs {
            let t = s + x;
            let e = if s.abs() >= x.abs() { (s - t) + x } else { (x - t) + s };
            c += e;
            s = t;
        }
        (s, c)
    }

    fn reduce(pairs: &[(f64, f64)]) -> (f64, f64) {
        match pairs {
            [p] => *p,
            _ => {
                let (l, r) = pairs.split_at(pairs.len() / 2);
                let ((s1, c1), (s2, c2)) = (reduce(l), reduce(r));
                let s = s1 + s2;
                let e = if s1.abs() >= s2.abs() { (s1 - s) + s2 } else { (s2 - s) + s1 };
                (s, (c1 + c2) + e)
            }
        }
    }

    pub fn sum(xs: &[f64]) -> f64 {
        if xs.is_empty() {
            return 0.0;
        }
        let pairs: Vec<(f64, f64)> = xs.chunks(256).map(neumaier).collect();
        let (s, c) = reduce(&pairs);
        if c == 0.0 {
            s
        } else {
            s + c
        }
    }
}

/// AC-1, AC-2: ill-conditioned sums match the Python model bit for bit, stay within 2 ulp of
/// the exact sum for condition numbers up to 1e16, and hash to the golden digest.
#[test]
fn ac1_ac2_illcond_fixture() {
    let text = read("red-illcond-v1.txt");
    let mut digest = None;
    let mut cases: Vec<(Vec<String>, Vec<f64>)> = Vec::new();
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        if let Some(d) = line.strip_prefix("digest ") {
            digest = Some(d.to_string());
        } else if line.starts_with("case ") {
            cases.push((line.split(' ').map(str::to_string).collect(), Vec::new()));
        } else {
            cases.last_mut().unwrap().1.extend(line.split(' ').map(hexf));
        }
    }
    assert!(cases.len() >= 30, "fixture too small: {}", cases.len());
    let mut h = Sha256Hasher::new();
    let mut checked_bound = 0;
    for (head, xs) in &cases {
        let field = |k: &str| head.iter().position(|f| f == k).map(|i| head[i + 1].as_str()).unwrap();
        assert_eq!(xs.len(), field("n").parse::<usize>().unwrap(), "{}", head[1]);
        let got = red::sum(xs).unwrap();
        assert_eq!(got.to_bits(), hexf(field("ref")).to_bits(), "{}", head[1]);
        let cond: f64 = field("cond").parse().unwrap();
        if cond <= 1e16 {
            assert!(ulp_dist(got, hexf(field("exact"))) <= 2, "{}", head[1]);
            checked_bound += 1;
        }
        assert_eq!(got.to_bits(), reference::sum(xs).to_bits(), "{}", head[1]);
        h.update(&got.to_bits().to_le_bytes());
    }
    assert!(checked_bound >= 20, "too few cases with cond <= 1e16: {checked_bound}");
    assert_eq!(h.finish(), digest.unwrap());
}

/// Adversarial cases for every reduction: catastrophic cancellation, huge/tiny mixes,
/// subnormals, signed zeros, block boundaries, overflow and the error paths.
#[test]
fn edge_fixture() {
    let text = read("red-edge-v1.txt");
    let mut n = 0;
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty() && !l.starts_with("gen ")) {
        let toks: Vec<&str> = line.split(' ').collect();
        let (op, name, expect) = (toks[0], toks[1], toks[2]);
        assert_eq!((toks[3], toks[5], toks[7]), ("exact", "maxulp", "a"), "{line}");
        let b_at = toks.iter().position(|t| *t == "b");
        let a = values(toks[8..b_at.unwrap_or(toks.len())].iter().copied());
        let b = b_at.map(|i| values(toks[i + 1..].iter().copied()));
        let got = match op {
            "sum" => red::sum(&a),
            "dot" => red::dot(&a, b.as_deref().unwrap()),
            "sum_sq" => red::sum_sq(&a),
            "norm2" => red::norm2(&a),
            "mean" => red::mean(&a),
            _ => panic!("unknown op {op}"),
        };
        assert_eq!(show(got, op), expect, "{op} {name}");
        if let (Ok(x), "-") = (got, toks[4]) {
            panic!("{op} {name}: ok result {x} without an oracle value");
        }
        if let Ok(x) = got {
            let bound: u128 = toks[6].parse().unwrap();
            assert!(ulp_dist(x, hexf(toks[4])) <= bound, "{op} {name}: beyond {bound} ulp of the oracle");
            if op == "sum" {
                assert_eq!(x.to_bits(), reference::sum(&a).to_bits(), "{op} {name}: test-side reference");
            }
        }
        n += 1;
    }
    assert!(n >= 60, "fixture too small: {n}");
}

/// AC-3: on 10^6 elements the result equals the test-side reimplementation bit for bit, and
/// the Python model's result and the oracle from the fixture.
#[test]
fn ac3_million_elements() {
    let text = read("red-edge-v1.txt");
    let line = text.lines().find(|l| l.starts_with("gen sum splitmix64 ")).unwrap();
    let toks: Vec<&str> = line.split(' ').collect();
    let seed = u64::from_str_radix(toks[3].trim_start_matches("0x"), 16).unwrap();
    let n: usize = toks[4].parse().unwrap();
    assert_eq!(n, 1_000_000);
    let xs = splitmix_values(seed, n);
    let got = red::sum(&xs).unwrap();
    assert_eq!(got.to_bits(), reference::sum(&xs).to_bits());
    assert_eq!(format!("ok:{:016x}", got.to_bits()), toks[5]);
    assert_eq!(toks[6], "exact");
    assert!(ulp_dist(got, hexf(toks[7])) <= 2);
    // A second, differently shaped input of the same length (not a multiple of BLOCK).
    let ys: Vec<f64> = splitmix_values(seed ^ 0xFFFF, n + 77).into_iter().map(|y| y * 1e-3).collect();
    assert_eq!(red::sum(&ys).unwrap().to_bits(), reference::sum(&ys).to_bits());
}

/// AC-4: error paths named in the spec.
#[test]
fn ac4_error_paths() {
    let mut xs = vec![1.0; 20];
    xs[9] = f64::INFINITY;
    assert_eq!(red::sum(&xs), Err(NumError::Infinite { op: "red.sum", index: Some(9) }));
    assert_eq!(
        red::dot(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0, 4.0]),
        Err(NumError::LengthMismatch { op: "red.dot", a: 3, b: 4 })
    );
    assert_eq!(red::mean(&[]), Err(NumError::EmptyInput { op: "red.mean" }));
    xs[9] = f64::NAN;
    assert_eq!(red::sum_sq(&xs), Err(NumError::NotANumber { op: "red.sum_sq", index: Some(9) }));
    assert_eq!(red::norm2(&xs), Err(NumError::NotANumber { op: "red.norm2", index: Some(9) }));
}

/// The partition is the one the spec fixes, and results depend only on the values and order.
#[test]
fn partition_and_repeatability() {
    assert_eq!(BLOCK, 256);
    let xs = splitmix_values(42, 3 * BLOCK + 5);
    let first = red::sum(&xs).unwrap().to_bits();
    for _ in 0..3 {
        assert_eq!(red::sum(&xs.clone()).unwrap().to_bits(), first);
    }
    assert_eq!(red::dot(&xs, &xs).unwrap().to_bits(), red::sum_sq(&xs).unwrap().to_bits());
    let ones = vec![1.0; 1000];
    assert_eq!(red::dot(&xs[..1000], &ones).unwrap().to_bits(), red::sum(&xs[..1000]).unwrap().to_bits());
}
