//! Acceptance criteria of spec node UNC.MODEL.DIST (AC-1 .. AC-6) plus requirement checks.
//!
//! Reference moments, inverse-normal values, AS241 coefficients, invalid parameter sets and
//! correlation labels come from the independent Python reference
//! `fixtures/unc/dist/make_fixtures.py` (exact fractions and 80-digit decimals), never from
//! this crate. `sample_digests.txt` pins the sample bytes for cross-target bit identity
//! (AC-2, REQ-006); it is produced by this test with `UNC_DIST_PRINT_DIGESTS=1` (see the
//! NODE.md Design) and CI runs this same file on wasm32-wasip1 (wasmtime), x86-64 (Linux,
//! Windows) and ARM64 (macOS), so a pass everywhere shows identical bytes on all three.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kunc::dist::{
    ppnd16, validate_correlation, Correlation, DistError, Distribution, Kind, MvNormal, Params, Quantity, UnitCode,
    AS241_A, AS241_B, AS241_C, AS241_D, AS241_E, AS241_F, MAX_GAMMA_INDEX, STREAM_LIMIT,
};
use fid_num::red;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/dist/");
/// AC-2 seed; stream 0 for every parameter set.
const SEED: u64 = 0xD_1570_0001;
const N: u64 = 1_000_000;

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{FIX}{name}")).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn csv(name: &str) -> Vec<Vec<String>> {
    read(name).lines().skip(1).filter(|l| !l.is_empty()).map(|l| l.split(',').map(str::to_owned).collect()).collect()
}

fn num(s: &str) -> f64 {
    s.parse::<f64>().unwrap_or_else(|e| panic!("{s}: {e}"))
}

fn unit(s: &str) -> UnitCode {
    UnitCode::parse(s).unwrap_or_else(|| panic!("unit {s}"))
}

/// Distance in units in the last place between two finite doubles.
fn ulps(a: f64, b: f64) -> u64 {
    let key = |x: f64| {
        let b = x.to_bits() as i64;
        if b < 0 {
            i64::MIN - b
        } else {
            b
        }
    };
    key(a).abs_diff(key(b))
}

fn build(kind: &str, q: &[Quantity]) -> Result<Distribution, DistError> {
    match kind {
        "normal" => Distribution::normal(q[0], q[1]),
        "rect" => Distribution::rect(q[0], q[1]),
        "tri" => Distribution::tri(q[0], q[1], q[2]),
        "arcsine" => Distribution::arcsine(q[0], q[1]),
        "t" => Distribution::scaled_t(q[0], q[1], q[2]),
        "trap" => Distribution::trap(q[0], q[1], q[2]),
        k => panic!("kind {k}"),
    }
}

struct Row {
    id: String,
    kind: String,
    dist: Distribution,
    mean: f64,
    sd: f64,
    kurt: f64,
}

fn moment_rows() -> Vec<Row> {
    csv("moments.csv")
        .into_iter()
        .map(|c| {
            let u = unit(&c[2]);
            let mut q: Vec<Quantity> =
                c[3..6].iter().filter(|s| !s.is_empty()).map(|s| Quantity::new(num(s), u)).collect();
            if c[1] == "t" || c[1] == "trap" {
                q[2].unit = UnitCode::One;
            }
            let dist = build(&c[1], &q).unwrap_or_else(|e| panic!("{}: {e}", c[0]));
            let kurt = if c[8] == "inf" { f64::INFINITY } else { num(&c[8]) };
            Row { id: c[0].clone(), kind: c[1].clone(), dist, mean: num(&c[6]), sd: num(&c[7]), kurt }
        })
        .collect()
}

// ------------------------------------------------------------------ REQ-001, AS241

#[test]
fn algorithm_ids_and_units() {
    let ids: Vec<&str> = Kind::ALL.iter().map(|k| k.algorithm_id()).collect();
    assert_eq!(
        ids,
        [
            "unc.dist.normal@1.0.0",
            "unc.dist.rect@1.0.0",
            "unc.dist.tri@1.0.0",
            "unc.dist.arcsine@1.0.0",
            "unc.dist.scaled_t@1.0.0",
            "unc.dist.trap@1.0.0",
            "unc.dist.mv_normal@1.0.0"
        ]
    );
    for s in ["si:m", "si:rad", "si:1"] {
        assert_eq!(unit(s).as_str(), s);
    }
    assert_eq!(UnitCode::parse("mm"), None);
    let d = Distribution::rect(Quantity::new(1.0, UnitCode::Radian), Quantity::new(2.0, UnitCode::Radian)).unwrap();
    assert_eq!((d.kind(), d.unit(), d.params()), (Kind::Rect, UnitCode::Radian, Params::Rect { a: 1.0, b: 2.0 }));
}

#[test]
fn as241_coefficients_equal_the_reference_table() {
    let tables = [("a", AS241_A), ("b", AS241_B), ("c", AS241_C), ("d", AS241_D), ("e", AS241_E), ("f", AS241_F)];
    let rows = csv("as241.csv");
    assert_eq!(rows.len(), 48);
    for r in rows {
        let t = tables.iter().find(|(n, _)| *n == r[0]).unwrap().1;
        let i: usize = r[1].parse().unwrap();
        assert_eq!(t[i].to_bits(), num(&r[2]).to_bits(), "{}{}", r[0], r[1]);
    }
}

#[test]
fn ppnd16_matches_the_decimal_inverse_normal() {
    let rows = csv("ppnd.csv");
    assert!(rows.len() > 200);
    let mut worst = 0.0_f64;
    for r in rows {
        let p = num(&r[0]);
        let want = num(&r[1]);
        let got = ppnd16(p).unwrap();
        if want == 0.0 {
            assert_eq!(got, 0.0);
            continue;
        }
        let rel = ((got - want) / want).abs();
        worst = if rel > worst { rel } else { worst };
        assert!(rel <= 1e-15, "p={p} got {got} want {want} rel {rel:e}");
    }
    eprintln!("ppnd16 worst relative error {worst:e}");
    assert!(ppnd16(0.0).is_err() && ppnd16(1.0).is_err() && ppnd16(f64::NAN).is_err() && ppnd16(-0.5).is_err());
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_moments_within_4_ulp_of_the_50_digit_reference() {
    let rows = moment_rows();
    assert_eq!(rows.len(), 40);
    for k in ["normal", "rect", "tri", "arcsine", "t", "trap"] {
        assert!(rows.iter().filter(|r| r.kind == k).count() >= 6, "{k}");
    }
    for r in &rows {
        let e = r.dist.expectation().unwrap();
        let u = r.dist.std_uncertainty().unwrap();
        assert!(ulps(e, r.mean) <= 4, "{} expectation {e:e} vs {:e} ({} ulp)", r.id, r.mean, ulps(e, r.mean));
        assert!(ulps(u, r.sd) <= 4, "{} std {u:e} vs {:e} ({} ulp)", r.id, r.sd, ulps(u, r.sd));
    }
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_invalid_parameter_sets_name_the_parameter() {
    let rows = csv("invalid.csv");
    assert!(rows.len() >= 30);
    for c in rows {
        let q: Vec<Quantity> = (0..3)
            .filter(|i| !c[2 + 2 * i].is_empty())
            .map(|i| Quantity::new(num(&c[2 + 2 * i]), unit(&c[3 + 2 * i])))
            .collect();
        let e = build(&c[1], &q).unwrap_err();
        assert_eq!(e, DistError::InvalidParameter { name: Box::leak(c[8].clone().into_boxed_str()) }, "{}", c[0]);
        assert_eq!(e.code(), "invalid_parameter");
    }
}

// ------------------------------------------------------------------ AC-5

/// A minimal JSON reader for the fixture shape (objects, arrays, strings without escapes,
/// numbers).
#[derive(Debug)]
enum J {
    Obj(Vec<(String, J)>),
    Arr(Vec<J>),
    Str(String),
    Num(f64),
}

impl J {
    fn get(&self, k: &str) -> &J {
        match self {
            J::Obj(m) => &m.iter().find(|(n, _)| n == k).unwrap().1,
            _ => panic!("not an object"),
        }
    }
    fn arr(&self) -> &[J] {
        match self {
            J::Arr(v) => v,
            _ => panic!("not an array"),
        }
    }
    fn str(&self) -> &str {
        match self {
            J::Str(s) => s,
            _ => panic!("not a string"),
        }
    }
    fn num(&self) -> f64 {
        match self {
            J::Num(x) => *x,
            _ => panic!("not a number"),
        }
    }
}

fn parse_json(s: &[u8], i: &mut usize) -> J {
    while s[*i].is_ascii_whitespace() {
        *i += 1;
    }
    match s[*i] {
        b'{' => {
            *i += 1;
            let mut m = Vec::new();
            loop {
                while s[*i].is_ascii_whitespace() || s[*i] == b',' {
                    *i += 1;
                }
                if s[*i] == b'}' {
                    *i += 1;
                    return J::Obj(m);
                }
                let k = parse_json(s, i).str().to_owned();
                while s[*i] != b':' {
                    *i += 1;
                }
                *i += 1;
                m.push((k, parse_json(s, i)));
            }
        }
        b'[' => {
            *i += 1;
            let mut v = Vec::new();
            loop {
                while s[*i].is_ascii_whitespace() || s[*i] == b',' {
                    *i += 1;
                }
                if s[*i] == b']' {
                    *i += 1;
                    return J::Arr(v);
                }
                v.push(parse_json(s, i));
            }
        }
        b'"' => {
            let start = *i + 1;
            *i = start;
            while s[*i] != b'"' {
                *i += 1;
            }
            *i += 1;
            J::Str(String::from_utf8(s[start..*i - 1].to_vec()).unwrap())
        }
        _ => {
            let start = *i;
            while *i < s.len() && !matches!(s[*i], b',' | b']' | b'}') && !s[*i].is_ascii_whitespace() {
                *i += 1;
            }
            J::Num(num(std::str::from_utf8(&s[start..*i]).unwrap()))
        }
    }
}

fn corr_cases(name: &str) -> Vec<(String, usize, Vec<f64>, String)> {
    let text = read(name);
    let j = parse_json(text.as_bytes(), &mut 0);
    j.get("cases")
        .arr()
        .iter()
        .map(|c| {
            (
                c.get("id").str().to_owned(),
                c.get("n").num() as usize,
                c.get("data").arr().iter().map(J::num).collect(),
                c.get("expect").str().to_owned(),
            )
        })
        .collect()
}

#[test]
fn ac5_correlation_fixtures() {
    let valid = corr_cases("corr_valid.json");
    assert!(valid.len() >= 3 && valid.iter().any(|c| c.1 == 3) && valid.iter().any(|c| c.1 == 6));
    for (id, n, data, expect) in valid {
        assert_eq!(expect, "valid");
        validate_correlation(n, &data).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(Correlation::new(n, data).unwrap().dim(), n);
    }
    let invalid = corr_cases("corr_invalid.json");
    for want in ["asymmetric", "diagonal", "indefinite"] {
        assert!(invalid.iter().any(|c| c.3 == want), "{want}");
    }
    for (id, n, data, expect) in invalid {
        let e = validate_correlation(n, &data).unwrap_err();
        assert_eq!(e.code(), "invalid_correlation", "{id}");
        assert_eq!(e, DistError::InvalidCorrelation { reason: Box::leak(expect.into_boxed_str()) }, "{id}");
    }
    // Shape and non-finite entries (hand-labelled).
    let shape = |n: usize, d: &[f64]| validate_correlation(n, d).unwrap_err();
    assert_eq!(shape(0, &[]), DistError::InvalidCorrelation { reason: "shape" });
    assert_eq!(shape(2, &[1.0, 0.0, 0.0]), DistError::InvalidCorrelation { reason: "shape" });
    assert_eq!(shape(2, &[1.0, f64::NAN, f64::NAN, 1.0]), DistError::InvalidCorrelation { reason: "non_finite" });
}

// ------------------------------------------------------------------ AC-2, AC-3

fn bytes(xs: &[f64]) -> Vec<u8> {
    xs.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn digest(xs: &[f64]) -> String {
    fid_kci::kci::sha256_digest(&bytes(xs)).to_string()
}

fn pinned_digest(id: &str) -> Option<String> {
    read("sample_digests.txt").lines().filter(|l| !l.starts_with('#')).find_map(|l| {
        let mut it = l.split_whitespace();
        (it.next() == Some(id)).then(|| it.next().unwrap().to_owned())
    })
}

fn check_digest(id: &str, d: &str) {
    if std::env::var_os("UNC_DIST_PRINT_DIGESTS").is_some() {
        println!("DIGEST {id} {d}");
        return;
    }
    assert_eq!(pinned_digest(id).as_deref(), Some(d), "{id}: sample digest differs from sample_digests.txt");
}

/// Sample mean and (n - 1) standard deviation through fixed-order reductions.
fn mean_sd(xs: &[f64]) -> (f64, f64) {
    let m = red::mean(xs).unwrap();
    let dev: Vec<f64> = xs.iter().map(|x| x - m).collect();
    let var = red::sum_sq(&dev).unwrap() / (xs.len() as f64 - 1.0);
    (m, var.sqrt())
}

fn ac2_ac3(kind: &str) {
    let rows: Vec<Row> = moment_rows().into_iter().filter(|r| r.kind == kind).collect();
    assert!(!rows.is_empty());
    let n = N as f64;
    for r in rows {
        let xs = r.dist.sample(SEED, 0, 0..N).unwrap();
        assert_eq!(xs.len(), N as usize);
        // AC-2 statistics; see the Design for which scaled-t rows are tested.
        let (m, sd) = mean_sd(&xs);
        if r.kurt.is_finite() {
            let se_mean = r.sd / n.sqrt();
            assert!((m - r.mean).abs() <= 5.0 * se_mean, "{}: mean {m} vs {} (5 SE = {})", r.id, r.mean, 5.0 * se_mean);
        }
        let nu_ok = match r.dist.params() {
            Params::ScaledT { nu, .. } => nu > 8.0,
            _ => true,
        };
        if nu_ok {
            let se_sd = r.sd * ((r.kurt - 1.0) / (4.0 * n)).sqrt();
            assert!((sd - r.sd).abs() <= 5.0 * se_sd, "{}: sd {sd} vs {} (5 SE = {})", r.id, r.sd, 5.0 * se_sd);
        }
        let d = digest(&xs);
        check_digest(&r.id, &d);
        // AC-3: the second half first, then the first half, gives identical bytes.
        let hi = r.dist.sample(SEED, 0, N / 2..N).unwrap();
        let lo = r.dist.sample(SEED, 0, 0..N / 2).unwrap();
        let mut cat = lo;
        cat.extend_from_slice(&hi);
        assert!(cat == xs, "{}: out-of-order draw differs", r.id);
        // REQ-004: any single index regenerates on its own.
        for i in [0, 1, 4_999, N / 2, N - 1] {
            assert_eq!(r.dist.sample_at(SEED, 0, i).unwrap().to_bits(), xs[i as usize].to_bits(), "{} index {i}", r.id);
        }
    }
}

#[test]
fn ac2_ac3_normal() {
    ac2_ac3("normal");
}

#[test]
fn ac2_ac3_rect() {
    ac2_ac3("rect");
}

#[test]
fn ac2_ac3_tri() {
    ac2_ac3("tri");
}

#[test]
fn ac2_ac3_arcsine() {
    ac2_ac3("arcsine");
}

#[test]
fn ac2_ac3_scaled_t() {
    ac2_ac3("t");
}

#[test]
fn ac2_ac3_trap() {
    ac2_ac3("trap");
}

// ------------------------------------------------------------------ AC-6

#[test]
fn ac6_correlated_normals_recover_rho() {
    let (_, n, data, _) = corr_cases("corr_valid.json").into_iter().find(|c| c.0 == "rho12_08_3x3").unwrap();
    assert_eq!(data[1], 0.8);
    let corr = Correlation::new(n, data).unwrap();
    let mu = [
        Quantity::new(1.0, UnitCode::Metre),
        Quantity::new(-2.0, UnitCode::Metre),
        Quantity::new(0.5, UnitCode::Radian),
    ];
    let sg = [
        Quantity::new(0.5, UnitCode::Metre),
        Quantity::new(3.0, UnitCode::Metre),
        Quantity::new(1e-3, UnitCode::Radian),
    ];
    let mv = MvNormal::new(&mu, &sg, corr).unwrap();
    assert_eq!((mv.dim(), mv.rank(), mv.kind()), (3, 3, Kind::MvNormal));
    let xs = mv.sample(SEED, 0, 0..N).unwrap();
    let col = |j: usize| -> Vec<f64> { xs.iter().skip(j).step_by(3).copied().collect() };
    let (c0, c1) = (col(0), col(1));
    let (m0, s0) = mean_sd(&c0);
    let (m1, s1) = mean_sd(&c1);
    let prod: Vec<f64> = c0.iter().zip(&c1).map(|(a, b)| (a - m0) * (b - m1)).collect();
    let rho = red::sum(&prod).unwrap() / (N as f64 - 1.0) / (s0 * s1);
    assert!((rho - 0.8).abs() <= 0.005, "rho12 = {rho}");
    for (j, (mj, sj)) in [(1.0, 0.5), (-2.0, 3.0), (0.5, 1e-3)].into_iter().enumerate() {
        let (m, s) = mean_sd(&col(j));
        assert!((m - mj).abs() <= 5.0 * sj / (N as f64).sqrt(), "mean {j}");
        assert!((s - sj).abs() <= 5.0 * sj * (0.5 / N as f64).sqrt(), "sd {j}");
    }
    check_digest("mv_rho12", &digest(&xs));
    // Order independence of the multivariate path.
    let mut cat = mv.sample(SEED, 0, 0..10).unwrap();
    cat.extend(mv.sample(SEED, 0, 10..20).unwrap());
    assert_eq!(bytes(&cat), bytes(&xs[..60]));
}

#[test]
fn mv_normal_singular_correlation_and_parameter_errors() {
    let corr = Correlation::new(2, vec![1.0, 1.0, 1.0, 1.0]).unwrap();
    let m = [Quantity::new(0.0, UnitCode::Metre), Quantity::new(10.0, UnitCode::Metre)];
    let s = [Quantity::new(1.0, UnitCode::Metre), Quantity::new(2.0, UnitCode::Metre)];
    let mv = MvNormal::new(&m, &s, corr.clone()).unwrap();
    assert_eq!(mv.rank(), 1);
    // Perfect correlation: x1 - 10 = 2 (x0 - 0) for every sample.
    let xs = mv.sample(7, 3, 0..1000).unwrap();
    for p in xs.chunks(2) {
        assert!(((p[1] - 10.0) - 2.0 * p[0]).abs() <= 1e-12 * (1.0 + p[0].abs()));
    }
    assert_eq!(mv.covariance().unwrap(), vec![1.0, 2.0, 2.0, 4.0]);
    let bad = |m: &[Quantity], s: &[Quantity], c: Correlation| MvNormal::new(m, s, c).unwrap_err();
    assert_eq!(bad(&[], &[], corr.clone()), DistError::InvalidParameter { name: "mu" });
    assert_eq!(bad(&m, &s[..1], corr.clone()), DistError::InvalidParameter { name: "sigma" });
    let one = Correlation::new(1, vec![1.0]).unwrap();
    assert_eq!(bad(&m, &s, one), DistError::InvalidParameter { name: "corr" });
    let s_bad = [s[0], Quantity::new(2.0, UnitCode::Radian)];
    assert_eq!(bad(&m, &s_bad, corr.clone()), DistError::InvalidParameter { name: "sigma" });
    let s_zero = [s[0], Quantity::new(0.0, UnitCode::Metre)];
    assert_eq!(bad(&m, &s_zero, corr.clone()), DistError::InvalidParameter { name: "sigma" });
    let m_nan = [Quantity::new(f64::NAN, UnitCode::Metre), m[1]];
    assert_eq!(bad(&m_nan, &s, corr), DistError::InvalidParameter { name: "mu" });
}

// ------------------------------------------------------------------ REQ-004 address limits

#[test]
fn address_limits() {
    let one = |v| Quantity::new(v, UnitCode::One);
    let n = Distribution::normal(one(0.0), one(1.0)).unwrap();
    assert_eq!(n.sample_at(1, STREAM_LIMIT, 0), Err(DistError::InvalidParameter { name: "stream" }));
    assert!(n.sample_at(1, STREAM_LIMIT - 1, u64::MAX).is_ok());
    assert_eq!(
        n.sample(1, 0, core::ops::Range { start: 5, end: 4 }),
        Err(DistError::InvalidParameter { name: "index" })
    );
    assert_eq!(n.sample(1, 0, core::ops::Range { start: 4, end: 4 }), Ok(vec![]));
    let t = Distribution::scaled_t(one(0.0), one(1.0), one(5.0)).unwrap();
    assert!(t.sample_at(1, 0, MAX_GAMMA_INDEX).is_ok());
    assert_eq!(t.sample_at(1, 0, MAX_GAMMA_INDEX + 1), Err(DistError::InvalidParameter { name: "index" }));
    // Different streams and seeds give different draws.
    assert_ne!(n.sample_at(1, 0, 0), n.sample_at(1, 1, 0));
    assert_ne!(n.sample_at(1, 0, 0), n.sample_at(2, 0, 0));
}
