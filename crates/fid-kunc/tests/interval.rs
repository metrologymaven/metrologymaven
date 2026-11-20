//! Acceptance criteria of spec node UNC.PROP.MCM.INTVL (AC-1 .. AC-4, AC-H1) plus
//! requirement checks.
//!
//! Expected intervals, ranks, means and standard uncertainties come from the independent
//! Python reference `fixtures/unc/mcm/intvl/make_fixtures.py` (exact rationals, 60-digit
//! decimal square root, its own Philox4x32-10), never from this crate. AC-3 and AC-4 are
//! hand-labelled from the spec's rules.
//!
//! AC-H1 (as restated on #306) is measured by `ac_h1_interval_convergence`. Plain `cargo test` runs it at
//! M = 10 000 values per seed (a smoke run of the same seeds on every target, wasm32
//! included); `UNC_MCM_INTVL_FULL=1 cargo test --release -p fid-kunc --test interval
//! ac_h1 -- --nocapture` runs the hypothesis scale M = 10^6. Pass counts are printed with
//! their exact (Clopper-Pearson) two-sided 95 % bounds from the reference file.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kunc::dist::{open_unit, Distribution, Quantity, UnitCode};
use fid_kunc::prop::mcm::interval::{coverage_count, summarize, McmSummary, ALGORITHM_ID};
use fid_kunc::prop::mcm::McmError;
use fid_kunc::IntervalKind;
use fid_num::{f, rng, NumError};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/mcm/intvl/");
/// Every interval fixture produced by the oracle.
const FIXTURES: [&str; 5] =
    ["uniform_1000.json", "skewed.json", "ranks_m1012.json", "ranks_m51.json", "ranks_p6827.json"];

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{FIX}{name}")).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn hex(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap_or_else(|e| panic!("{s}: {e}")))
}

/// The text right after `"key": ` at or after byte `from`.
fn after<'a>(doc: &'a str, from: usize, key: &str) -> (&'a str, usize) {
    let pat = format!("\"{key}\": ");
    let at = doc[from..].find(&pat).unwrap_or_else(|| panic!("missing key {key}")) + from + pat.len();
    (&doc[at..], at)
}

/// A string field `"key": "<hex>"` as f64.
fn hex_field(doc: &str, from: usize, key: &str) -> f64 {
    let (rest, _) = after(doc, from, key);
    let rest = rest.strip_prefix('"').unwrap();
    hex(&rest[..rest.find('"').unwrap()])
}

/// A plain number or array field, as raw text up to the next `,` or `}` outside brackets.
fn raw_field<'a>(doc: &'a str, from: usize, key: &str) -> &'a str {
    let (rest, _) = after(doc, from, key);
    let mut depth = 0i32;
    for (i, c) in rest.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            ',' | '}' if depth == 0 => return rest[..i].trim(),
            _ => {}
        }
        if depth < 0 {
            return rest[..i].trim();
        }
    }
    rest.trim()
}

fn ranks(doc: &str, key: &str) -> (u64, u64) {
    let raw = raw_field(doc, 0, key);
    let v: Vec<u64> = raw.trim_matches(['[', ']']).split(',').map(|x| x.trim().parse().unwrap()).collect();
    (v[0], v[1])
}

/// All quoted strings of the array field `key`.
fn hex_array(doc: &str, key: &str) -> Vec<f64> {
    let (rest, _) = after(doc, 0, key);
    let body = &rest[1..rest.find(']').unwrap()];
    body.split(',').map(|s| hex(s.trim().trim_matches('"'))).collect()
}

/// Distance in units in the last place between two finite doubles of the same sign.
fn ulps(a: f64, b: f64) -> u64 {
    let (x, y) = (a.to_bits() as i64, b.to_bits() as i64);
    (x - y).unsigned_abs()
}

struct Case {
    p: f64,
    fx: f64,
    values: Vec<f64>,
    doc: String,
}

fn case(name: &str) -> Case {
    let doc = read(name);
    Case { p: hex_field(&doc, 0, "p"), fx: hex_field(&doc, 0, "fx"), values: hex_array(&doc, "values"), doc }
}

/// Checks a summary against the expected block of a fixture.
fn check_expected(c: &Case, s: &McmSummary) {
    let e = c.doc.find("\"expected\"").unwrap();
    let d = &c.doc;
    assert_eq!(s.m, c.values.len() as u64);
    assert_eq!(s.sym.low.to_bits(), hex_field(d, e, "sym_low").to_bits(), "sym low");
    assert_eq!(s.sym.high.to_bits(), hex_field(d, e, "sym_high").to_bits(), "sym high");
    assert_eq!(s.shortest.low.to_bits(), hex_field(d, e, "shortest_low").to_bits(), "shortest low");
    assert_eq!(s.shortest.high.to_bits(), hex_field(d, e, "shortest_high").to_bits(), "shortest high");
    assert_eq!(s.sym_ranks, ranks(d, "sym_ranks"));
    assert_eq!(s.shortest_ranks, ranks(d, "shortest_ranks"));
    assert_eq!(s.sym.kind, IntervalKind::ProbSymmetric);
    assert_eq!(s.shortest.kind, IntervalKind::Shortest);
    assert_eq!(s.sym.p.to_bits(), c.p.to_bits());
    assert_eq!(s.shortest.p.to_bits(), c.p.to_bits());
    // Moments: the reference is the exact rational rounded once; the crate rounds the mean
    // once more on the way to u and mean - fx, so a few units in the last place are allowed.
    assert!(ulps(s.mean, hex_field(d, e, "mean")) <= 2, "mean {} vs {}", s.mean, hex_field(d, e, "mean"));
    assert!(ulps(s.u, hex_field(d, e, "u")) <= 4, "u {} vs {}", s.u, hex_field(d, e, "u"));
    let want = hex_field(d, e, "mean_minus_fx");
    assert!((s.mean_minus_fx - want).abs() <= 4.0 * f64::EPSILON * s.mean.abs(), "mean_minus_fx");
}

#[test]
fn algorithm_id() {
    assert_eq!(ALGORITHM_ID, "unc.prop.mcm_interval@1.0.0");
}

/// AC-1: values 1..1000, p = 0.95: symmetric [25th, 975th], shortest of the same length
/// with the lowest start ([1st, 951st]).
#[test]
fn ac1_uniform_1000() {
    let c = case("uniform_1000.json");
    assert_eq!(c.values.len(), 1000);
    let s = summarize(&c.values, c.p, c.fx).unwrap();
    assert_eq!((s.sym.low, s.sym.high), (25.0, 975.0));
    assert_eq!((s.shortest.low, s.shortest.high), (1.0, 951.0));
    assert_eq!(s.shortest.high - s.shortest.low, s.sym.high - s.sym.low);
    check_expected(&c, &s);
}

/// The input order does not change any output bit (sorting is total and stable).
#[test]
fn input_order_does_not_matter() {
    for name in ["uniform_1000.json", "skewed.json"] {
        let c = case(name);
        let a = summarize(&c.values, c.p, c.fx).unwrap();
        let mut v = c.values.clone();
        let mut cur = rng::Cursor::new(rng::Address { seed: 11, stream: 0 }, 0);
        rng::shuffle(&mut v, &mut cur).unwrap();
        v.reverse();
        let b = summarize(&v, c.p, c.fx).unwrap();
        assert_eq!(format!("{a:?}"), format!("{b:?}"), "{name}");
        assert_eq!(a.mean.to_bits(), b.mean.to_bits());
        assert_eq!(a.u.to_bits(), b.u.to_bits());
    }
}

/// AC-2: exponential sample (seed 3): the shortest interval is strictly shorter than the
/// symmetric one and both match the expected file.
#[test]
fn ac2_skewed() {
    let c = case("skewed.json");
    assert_eq!(c.values.len(), 2000);
    let s = summarize(&c.values, c.p, c.fx).unwrap();
    assert!(s.shortest.high - s.shortest.low < s.sym.high - s.sym.low);
    check_expected(&c, &s);
}

/// The fixture's exponential values are the project RNG's draws at (3, 0, i) through the
/// open-unit map of UNC.MODEL.DIST; the oracle computed them with its own Philox and
/// Python's log, so only the uniforms are compared bit for bit.
#[test]
fn ac2_values_come_from_the_project_rng() {
    let c = case("skewed.json");
    for (i, &x) in c.values.iter().enumerate() {
        let w = rng::block(3, 0, i as u64);
        let u = open_unit(w[1], w[0]);
        let y = -f::ln(u).unwrap();
        assert!((x - y).abs() <= 4.0 * f64::EPSILON * (1.0 + y), "value {i}: {x} vs {y}");
    }
}

/// AC-3: mean 1.5 and f(x) = 1.0 give mean_minus_fx = 0.5.
#[test]
fn ac3_mean_minus_fx() {
    let v: Vec<f64> = (0..20).map(|i| if i % 2 == 0 { 1.0 } else { 2.0 }).collect();
    let s = summarize(&v, 0.95, 1.0).unwrap();
    assert_eq!(s.mean, 1.5);
    assert_eq!(s.mean_minus_fx, 0.5);
}

/// AC-4 (error path): 10 values at p = 0.95 are too few (need ceil(1 / 0.05) = 20); 19
/// fail and 20 pass.
#[test]
fn ac4_too_few_trials() {
    let v: Vec<f64> = (0..20).map(f64::from).collect();
    let e = summarize(&v[..10], 0.95, 0.0).unwrap_err();
    assert_eq!(e, McmError::TooFewTrials { m: 10, min: 20 });
    assert_eq!(e.code(), "too_few_trials");
    assert_eq!(summarize(&v[..19], 0.95, 0.0).unwrap_err(), McmError::TooFewTrials { m: 19, min: 20 });
    assert!(summarize(&v, 0.95, 0.0).is_ok());
    assert_eq!(summarize(&[], 0.5, 0.0).unwrap_err(), McmError::TooFewTrials { m: 0, min: 2 });
}

#[test]
fn invalid_p() {
    let v: Vec<f64> = (0..100).map(f64::from).collect();
    for p in [0.0, -0.0, 1.0, -0.5, 1.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let e = summarize(&v, p, 0.0).unwrap_err();
        assert_eq!(e, McmError::InvalidP, "p = {p}");
        assert_eq!(e.code(), "invalid_p");
    }
}

/// A NaN is always an error; non-finite values name the first offending index.
#[test]
fn non_finite_inputs() {
    let mut v: Vec<f64> = (0..100).map(f64::from).collect();
    v[7] = f64::INFINITY;
    v[42] = f64::NAN;
    assert_eq!(
        summarize(&v, 0.95, 0.0).unwrap_err(),
        McmError::Num(NumError::Infinite { op: "mcm.interval.values", index: Some(7) })
    );
    v[7] = 7.0;
    assert_eq!(
        summarize(&v, 0.95, 0.0).unwrap_err(),
        McmError::Num(NumError::NotANumber { op: "mcm.interval.values", index: Some(42) })
    );
    v[42] = 42.0;
    assert_eq!(
        summarize(&v, 0.95, f64::NAN).unwrap_err(),
        McmError::Num(NumError::NotANumber { op: "mcm.interval.fx", index: None })
    );
    // p is checked before the values.
    v[0] = f64::NAN;
    assert_eq!(summarize(&v, 2.0, 0.0).unwrap_err(), McmError::InvalidP);
}

/// Ties between windows go to the lowest start rank; signed zeros sort in total order.
#[test]
fn shortest_ties_take_lowest_start() {
    // 20 values, p = 0.9: q = 18. Windows [x1, x19] and [x2, x20] both have length 10.
    let mut v: Vec<f64> = vec![0.0; 20];
    v[0] = -5.0;
    for (i, x) in v.iter_mut().enumerate().skip(1).take(18) {
        *x = (i as f64) * 0.25;
    }
    v[19] = 5.0 + 0.25;
    // Sorted: -5, 0.25 .. 4.5, 5.25. Window 1: 4.5 - (-5) = 9.5; window 2: 5.25 - 0.25 = 5.
    let s = summarize(&v, 0.9, 0.0).unwrap();
    assert_eq!(s.shortest_ranks, (2, 20));
    let w: Vec<f64> = (0..20).map(f64::from).collect();
    let s = summarize(&w, 0.9, 0.0).unwrap();
    assert_eq!(s.shortest_ranks, (1, 19));
    let z = [0.0, -0.0, 1.0, 2.0];
    let s = summarize(&z, 0.5, 0.0).unwrap();
    assert_eq!(s.shortest.low.to_bits(), (-0.0f64).to_bits());
}

/// Rank rule spot checks (REQ-UNC.PROP.MCM.INTVL-002), hand-computed: M = 2000, p = 0.95
/// gives r = 50, q = 1900; M = 20, p = 0.95 gives r = round(0.5) = 1, q = 19.
#[test]
fn rank_rule() {
    let v: Vec<f64> = (1..=2000).map(f64::from).collect();
    let s = summarize(&v, 0.95, 0.0).unwrap();
    assert_eq!(s.sym_ranks, (50, 1950));
    assert_eq!(s.shortest_ranks, (1, 1901));
    let s = summarize(&v[..20], 0.95, 0.0).unwrap();
    assert_eq!(s.sym_ranks, (1, 20));
    assert_eq!(s.shortest_ranks, (1, 20));
}

/// Rank cases where p M is not an integer (JCGM 101:2008 §7.7.2): M = 1012 gives
/// (26, 987), M = 51 gives (2, 50), M = 1000 at p = 0.6827 gives (159, 842).
#[test]
fn rank_rule_non_integer_pm() {
    for (name, want) in [("ranks_m1012.json", (26, 987)), ("ranks_m51.json", (2, 50)), ("ranks_p6827.json", (159, 842))]
    {
        let c = case(name);
        let s = summarize(&c.values, c.p, c.fx).unwrap();
        assert_eq!(s.sym_ranks, want, "{name}");
        check_expected(&c, &s);
    }
}

/// q = floor(p M + 1/2) exactly from the binary64 value of p.
#[test]
fn coverage_count_is_exact() {
    assert_eq!(coverage_count(0.95, 1012), 961);
    assert_eq!(coverage_count(0.95, 1000), 950);
    assert_eq!(coverage_count(0.5, 3), 2); // 1.5 + 0.5 = 2 exactly
    assert_eq!(coverage_count(0.5, 1), 1);
    assert_eq!(coverage_count(0.25, 2), 1); // 0.5 + 0.5
                                            // The binary64 0.95 is slightly below 19/20, so 0.95 * 10 + 1/2 = 9.999... and q = 9.
    assert_eq!(coverage_count(0.95, 10), 9);
    assert_eq!(coverage_count(f64::from_bits(1), u64::MAX), 0);
    // (1 - 2^-53) * 2^20 + 1/2 = 2^20 + 1/2 - 2^-33, so q = 2^20.
    assert_eq!(coverage_count(1.0 - f64::EPSILON / 2.0, 1 << 20), 1 << 20);
    assert_eq!(coverage_count(1.0 - f64::EPSILON / 2.0, u64::MAX), u64::MAX - 2048);
}

/// M >= 2 always (REQ-UNC.PROP.MCM.INTVL-003), even when ceil(1 / (1 - p)) rounds to 1.
#[test]
fn at_least_two_values() {
    assert_eq!(summarize(&[1.0], 1e-20, 0.0).unwrap_err(), McmError::TooFewTrials { m: 1, min: 2 });
    assert!(summarize(&[1.0, 2.0], 1e-20, 0.0).is_ok());
}

// ------------------------------------------------------------------------ AC-H1

struct Endpoint {
    q: f64,
    x: f64,
    density: f64,
}

fn endpoint(doc: &str, dist: &str, key: &str) -> Endpoint {
    let d = doc.find(&format!("\"{dist}\": {{")).unwrap();
    let (_, at) = after(doc, d, key);
    Endpoint {
        q: raw_field(doc, at, "q").parse().unwrap(),
        x: hex_field(doc, at, "x"),
        density: hex_field(doc, at, "density"),
    }
}

/// Tolerance 3 x the asymptotic standard error of the order statistic at level q,
/// sqrt(q (1 - q) / M) / f(x_q). Used for the symmetric endpoints only.
fn tolerance(e: &Endpoint, m: usize) -> f64 {
    let m = m as f64;
    3.0 * f::sqrt(e.q * (1.0 - e.q) / m).unwrap() / e.density
}

fn rect4_values(seed: u64, m: usize) -> Vec<f64> {
    let h = f::sqrt(3.0).unwrap();
    let d = Distribution::rect(Quantity::new(-h, UnitCode::One), Quantity::new(h, UnitCode::One)).unwrap();
    let cols: Vec<Vec<f64>> = (0..4).map(|k| d.sample(seed, k, 0..m as u64).unwrap()).collect();
    (0..m).map(|i| ((cols[0][i] + cols[1][i]) + cols[2][i]) + cols[3][i]).collect()
}

fn exp1_values(seed: u64, m: usize) -> Vec<f64> {
    (0..m as u64)
        .map(|i| {
            let w = rng::block(seed, 0, i);
            -f::ln(open_unit(w[1], w[0])).unwrap()
        })
        .collect()
}

/// Analytic CDF of rect4, Y = 2 sqrt(3) S - 4 sqrt(3) with S Irwin-Hall(4):
/// F_S(s) = (1/24) sum_{k <= floor(s)} (-1)^k C(4, k) (s - k)^4 (the textbook closed form).
fn rect4_cdf(y: f64) -> f64 {
    let h = f::sqrt(3.0).unwrap();
    let s = (y + 4.0 * h) / (2.0 * h);
    if s <= 0.0 {
        return 0.0;
    }
    if s >= 4.0 {
        return 1.0;
    }
    let c = [1.0, -4.0, 6.0, -4.0, 1.0];
    let mut acc = 0.0;
    for (k, ck) in c.iter().enumerate() {
        let d = s - k as f64;
        if d <= 0.0 {
            break;
        }
        acc += ck * (d * d) * (d * d);
    }
    acc / 24.0
}

/// Analytic CDF of exp1.
fn exp1_cdf(x: f64) -> f64 {
    if x <= 0.0 {
        0.0
    } else {
        1.0 - f::exp(-x).unwrap()
    }
}

/// AC-H1 as restated (maintainer decision on #306): for the probabilistically symmetric
/// interval, each endpoint is within 3 asymptotic order-statistic standard errors of the
/// analytic endpoint; for the shortest interval (whose endpoints converge at about M^(-1/3),
/// shorth behaviour, so a per-endpoint test does not apply), the width is within
/// 3 sqrt(p (1 - p) / M) / f(xi) of the analytic shortest width (xi the upper analytic
/// endpoint) and the realised coverage F(hi) - F(lo) is within 3 sqrt(p (1 - p) / M) of p.
/// Each of the four checks must pass for at least 99 of 100 seeds. The shortest endpoints
/// themselves are matched bit for bit against the oracle by AC-1, AC-2 and the rank cases.
/// Seeds: rect4 0x1A7_0000 + s (streams 0..3), exp1 0x1A7_1000 + s (stream 0), s = 0..99.
#[test]
fn ac_h1_interval_convergence() {
    let full = std::env::var("UNC_MCM_INTVL_FULL").map(|v| v == "1").unwrap_or(false);
    let m = if full { 1_000_000 } else { 10_000 };
    let p = 0.95;
    let doc = read("ac_h1_reference.json");
    let cp_raw = raw_field(&doc, 0, "clopper_pearson_95_n100");
    let cp: Vec<&str> = cp_raw.trim_matches(['[', ']']).split("], [").collect();
    assert_eq!(cp.len(), 101);
    let checks = ["sym_low", "sym_high", "shortest_width", "shortest_coverage"];
    let cov_tol = 3.0 * f::sqrt(p * (1.0 - p) / m as f64).unwrap();
    let mut report = Vec::new();
    for (dist, base) in [("rect4", 0x1A7_0000u64), ("exp1", 0x1A7_1000u64)] {
        let lo = endpoint(&doc, dist, "sym_low");
        let hi = endpoint(&doc, dist, "sym_high");
        let sh_lo = endpoint(&doc, dist, "shortest_low");
        let sh_hi = endpoint(&doc, dist, "shortest_high");
        let width_ref = sh_hi.x - sh_lo.x;
        let width_tol = cov_tol / sh_hi.density;
        let cdf = if dist == "rect4" { rect4_cdf } else { exp1_cdf };
        let mut pass = [0u32; 4];
        let mut worst = [0.0f64; 4];
        for s in 0..100u64 {
            let v = if dist == "rect4" { rect4_values(base + s, m) } else { exp1_values(base + s, m) };
            let sum = summarize(&v, p, 0.0).unwrap();
            let z = [
                (sum.sym.low - lo.x).abs() / tolerance(&lo, m),
                (sum.sym.high - hi.x).abs() / tolerance(&hi, m),
                ((sum.shortest.high - sum.shortest.low) - width_ref).abs() / width_tol,
                ((cdf(sum.shortest.high) - cdf(sum.shortest.low)) - p).abs() / cov_tol,
            ];
            for k in 0..4 {
                if z[k] <= 1.0 {
                    pass[k] += 1;
                }
                if z[k] > worst[k] {
                    worst[k] = z[k];
                }
            }
        }
        for k in 0..4 {
            let bounds = cp[pass[k] as usize].replace('"', "");
            println!(
                "AC-H1 M={m} {dist} {}: {}/100 seeds within tolerance, exact 95 % bounds [{bounds}], worst |err|/tol = {:.3}",
                checks[k], pass[k], worst[k]
            );
            report.push((dist, checks[k], pass[k]));
        }
    }
    assert_eq!(report.len(), 8);
    // The hypothesis is stated at M = 10^6, where every check must pass for >= 99 seeds
    // (measured: 99 or 100 each). At the M = 10 000 smoke scale the symmetric checks keep
    // the same bar, and the shortest-interval checks are a regression guard at >= 95: the
    // shortest window is selected from the sample, which perturbs its realised coverage by
    // an amount that only becomes negligible at large M (measured: rect4 coverage 98/100).
    for (dist, check, pass) in &report {
        let bar = if full || check.starts_with("sym_") { 99 } else { 95 };
        assert!(*pass >= bar, "AC-H1 M={m} {dist} {check}: {pass}/100, need {bar}");
    }
}

// ------------------------------------------------------------------------ D0 bits

/// Little-endian bytes of the summary fields, in a fixed order.
fn summary_bytes(s: &McmSummary, out: &mut Vec<u8>) {
    for x in [s.mean, s.u, s.sym.low, s.sym.high, s.shortest.low, s.shortest.high, s.mean_minus_fx] {
        out.extend_from_slice(&x.to_le_bytes());
    }
}

fn hex_bytes(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// D0 bit identity across targets. `crate_bits.txt` pins this crate's exact `to_bits()` of
/// mean, u and mean_minus_fx for every interval fixture, and a SHA-256 over the summaries
/// of AC-H1 seeds 0..3 of both distributions at M = 10 000. It is produced by this test
/// with `UNC_MCM_INTVL_PRINT_BITS=1` (crate output, not an oracle: the correctness of
/// these values is checked against the oracle by the other tests) and CI runs this file on
/// wasm32-wasip1, x86-64 (Linux, Windows) and ARM64 (macOS), so a pass everywhere shows
/// identical bits on all of them.
#[test]
fn d0_bits_are_pinned() {
    let mut lines = vec![
        "# UNC.PROP.MCM.INTVL D0 bit pins: crate output, regenerate with UNC_MCM_INTVL_PRINT_BITS=1".to_string(),
        "# fixture mean u mean_minus_fx (IEEE-754 bits, hex)".to_string(),
    ];
    for name in FIXTURES {
        let c = case(name);
        let s = summarize(&c.values, c.p, c.fx).unwrap();
        lines.push(format!(
            "{name} {:016x} {:016x} {:016x}",
            s.mean.to_bits(),
            s.u.to_bits(),
            s.mean_minus_fx.to_bits()
        ));
    }
    let mut bytes = Vec::new();
    for (dist, base) in [("rect4", 0x1A7_0000u64), ("exp1", 0x1A7_1000u64)] {
        for s in 0..4u64 {
            let v = if dist == "rect4" { rect4_values(base + s, 10_000) } else { exp1_values(base + s, 10_000) };
            summary_bytes(&summarize(&v, 0.95, 0.0).unwrap(), &mut bytes);
        }
    }
    lines.push(format!("ac_h1_seeds_0_3_m10000 sha256:{}", hex_bytes(&fid_kci::kci::sha256::sha256(&bytes))));
    let text = lines.join("\n") + "\n";
    if std::env::var("UNC_MCM_INTVL_PRINT_BITS").map(|v| v == "1").unwrap_or(false) {
        std::fs::write(format!("{FIX}crate_bits.txt"), &text).unwrap();
    }
    assert_eq!(text, read("crate_bits.txt"));
}
