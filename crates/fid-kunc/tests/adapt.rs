//! Acceptance criteria of spec node UNC.PROP.MCM.ADAPT (AC-1 .. AC-4, AC-H1, AC-H2) plus
//! requirement checks.
//!
//! Expected decisions, standard deviations, tolerances and block sizes come from the
//! independent Python reference `fixtures/unc/mcm/adapt/make_fixtures.py` (exact rationals,
//! 60-digit decimal square root), never from this crate. AC-3 and AC-4 are also
//! hand-labelled from the spec's rules.
//!
//! AC-H1 and AC-H2 (as restated in the leaf's NODE.md) are measured by
//! `ac_h_adaptive_runs`. Plain `cargo test` runs a smoke of the first few seeds on every
//! target, wasm32 included; `UNC_MCM_ADAPT_FULL=1 cargo test --release -p fid-kunc --test
//! adapt ac_h -- --nocapture` runs all 100 seeds per example. Every number these tests print
//! is a simulated (Monte Carlo) result, not a measurement; pass counts are printed with
//! their exact (Clopper-Pearson) two-sided 95 % bounds from the reference file.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kunc::dist::{Distribution, Quantity, UnitCode};
use fid_kunc::prop::mcm::adapt::{
    block_size, decide, tolerance, AdaptProfile, AdaptStep, BlockSummary, Decision, GumSuiteProfile, Tolerance,
    ALGORITHM_ID, DEFAULT_CAP, DEFAULT_N_DIG, FLAG_NOT_CONVERGED, RULE,
};
use fid_kunc::prop::mcm::interval::{summarize, McmSummary};
use fid_kunc::prop::mcm::McmError;
use fid_num::{f, NumError};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/mcm/adapt/");

fn read(name: &str) -> String {
    std::fs::read_to_string(format!("{FIX}{name}")).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn hex(s: &str) -> f64 {
    let s = s.trim().trim_matches('"');
    f64::from_bits(u64::from_str_radix(s, 16).unwrap_or_else(|e| panic!("{s}: {e}")))
}

/// Raw text of field `key` in `doc` at or after byte `from`, up to the next `,` or `}`
/// outside brackets.
fn raw<'a>(doc: &'a str, from: usize, key: &str) -> &'a str {
    let pat = format!("\"{key}\": ");
    let at = doc[from..].find(&pat).unwrap_or_else(|| panic!("missing key {key}")) + from + pat.len();
    let rest = &doc[at..];
    let mut depth = 0i32;
    for (i, c) in rest.char_indices() {
        match c {
            '[' | '{' => depth += 1,
            ']' | '}' if depth == 0 => return rest[..i].trim(),
            ']' | '}' => depth -= 1,
            ',' if depth == 0 => return rest[..i].trim(),
            _ => {}
        }
    }
    rest.trim()
}

fn hex_list(raw: &str) -> Vec<f64> {
    raw.trim_matches(['[', ']']).split(',').map(hex).collect()
}

/// Distance in units in the last place between two finite doubles of the same sign.
fn ulps(a: f64, b: f64) -> u64 {
    (a.to_bits() as i64 - b.to_bits() as i64).unsigned_abs()
}

struct Seq {
    profile: AdaptProfile,
    blocks: Vec<BlockSummary>,
    /// One line per step.
    steps: Vec<String>,
}

fn seq(doc: &str) -> Seq {
    let p = hex(raw(doc, 0, "p"));
    let n_dig: u32 = raw(doc, 0, "n_dig").parse().unwrap();
    let cap: u64 = raw(doc, 0, "cap").parse().unwrap();
    let m: u64 = raw(doc, 0, "block_size").parse().unwrap();
    let profile = AdaptProfile::new(GumSuiteProfile::Jcgm101_2008, p, n_dig, cap).unwrap();
    assert_eq!(profile.block_size(), m, "block size");
    let blocks = doc
        .lines()
        .filter(|l| l.trim_start().starts_with("{\"y_mean\""))
        .map(|l| BlockSummary {
            y_mean: hex(raw(l, 0, "y_mean")),
            u: hex(raw(l, 0, "u")),
            low: hex(raw(l, 0, "low")),
            high: hex(raw(l, 0, "high")),
        })
        .collect();
    let steps = doc.lines().filter(|l| l.trim_start().starts_with("{\"h\"")).map(str::to_string).collect();
    Seq { profile, blocks, steps }
}

fn decision_code(d: Decision) -> &'static str {
    d.code()
}

/// Replays a sequence block by block against the oracle's steps; returns the crate's steps.
fn replay(s: &Seq) -> Vec<AdaptStep> {
    let mut out = Vec::new();
    for line in &s.steps {
        let h: usize = raw(line, 0, "h").parse().unwrap();
        let u_all = hex(raw(line, 0, "u_all"));
        let got = decide(&s.profile, &s.blocks[..h], u_all).unwrap();
        let want = raw(line, 0, "decision").trim_matches('"');
        assert_eq!(decision_code(got.decision), want, "h = {h}");
        assert_eq!(got.blocks, h as u64);
        assert_eq!(got.trials, h as u64 * s.profile.block_size());
        let sd = raw(line, 0, "sd");
        if sd == "null" {
            assert_eq!(got.sd, None);
            assert_eq!(got.tolerance, None);
        } else {
            let want_sd = hex_list(sd);
            let got_sd = got.sd.unwrap();
            for k in 0..4 {
                // The oracle rounds the exact s once; the crate rounds the mean, the norm and
                // the division, so a few units in the last place are allowed.
                assert!(ulps(got_sd[k], want_sd[k]) <= 8, "h = {h} sd[{k}]: {} vs {}", got_sd[k], want_sd[k]);
            }
            let tol = got.tolerance.unwrap();
            let e = raw(line, 0, "exp10");
            let want_e = if e == "null" { None } else { Some(e.parse::<i32>().unwrap()) };
            assert_eq!(tol.exp10, want_e, "h = {h} exp10");
            assert_eq!(tol.delta.to_bits(), hex(raw(line, 0, "delta")).to_bits(), "h = {h} delta");
        }
        out.push(got);
    }
    out
}

#[test]
fn identifiers() {
    assert_eq!(ALGORITHM_ID, "unc.prop.mcm_adapt@1.0.0");
    assert_eq!(RULE, "mcm-adapt@1");
    assert_eq!(GumSuiteProfile::default().as_str(), "jcgm101-2008");
    assert_eq!(DEFAULT_N_DIG, 2);
    assert_eq!(DEFAULT_CAP, 1_000_000);
}

/// The spec card `mcm-adapt@1` parses (KRN.MAN.CARD schema and standards-text check) and
/// matches the algorithm ID.
#[test]
fn spec_card() {
    let card = fid_kci::parse_card(include_str!("../cards/unc.prop.mcm_adapt@1.0.0.toml")).unwrap();
    let id = fid_kci::AlgorithmId::parse(ALGORITHM_ID).unwrap();
    let report = fid_kci::check(&[id], std::slice::from_ref(&card.id));
    assert!(report.is_clean(), "{report:?}");
    assert_eq!(card.determinism, "D0");
    assert!(card.references.iter().all(|r| r.starts_with("JCGM 101:2008")));
}

/// AC-1: the converging sequence stops at block 5, after four `continue`s.
#[test]
fn ac1_converging_sequence_stops() {
    let s = seq(&read("summaries_converge.json"));
    let steps = replay(&s);
    let codes: Vec<&str> = steps.iter().map(|x| x.decision.code()).collect();
    assert_eq!(codes, ["continue", "continue", "continue", "continue", "stop"]);
    let last = steps.last().unwrap();
    assert_eq!((last.blocks, last.trials), (5, 50_000));
    assert!(!last.not_converged());
    assert!(last.flags().is_empty());
    // Step 3 has u(y) = 0.9951, which rounds to 1.0 at two digits: delta stays 0.05.
    assert_eq!(steps[2].tolerance.unwrap().exp10, Some(-1));
}

/// AC-2: cap 30 000 and M = 10 000: cap_reached after block 3 with the achieved values.
#[test]
fn ac2_slow_sequence_reaches_the_cap() {
    let s = seq(&read("summaries_slow.json"));
    assert_eq!((s.profile.cap(), s.profile.block_size()), (30_000, 10_000));
    let steps = replay(&s);
    let last = steps.last().unwrap();
    assert_eq!(last.decision, Decision::CapReached);
    assert_eq!((last.blocks, last.trials), (3, 30_000));
    assert!(last.not_converged());
    assert_eq!(last.flags(), [FLAG_NOT_CONVERGED]);
    let tol = last.tolerance.unwrap();
    assert_eq!(tol.delta, 0.05);
    // The achieved values do not meet the tolerance: never `stop` on a cap.
    assert!(last.sd.unwrap().iter().any(|&x| !tol.admits(x)));
    assert_eq!(steps[1].decision, Decision::Continue);
}

/// Boundary steps of the oracle's edge file.
#[test]
fn edge_sequences() {
    let doc = read("summaries_edge.json");
    let parts: Vec<&str> = doc.split("\"title\": ").skip(1).collect();
    assert_eq!(parts.len(), 4);
    let mut last_codes = Vec::new();
    for part in parts {
        let s = seq(part);
        let steps = replay(&s);
        last_codes.push(steps.last().unwrap().decision);
    }
    assert_eq!(last_codes, [Decision::Stop, Decision::Continue, Decision::Stop, Decision::Stop]);
}

/// AC-3: u(y) = 0.0123456, n_dig = 2 gives delta = 0.0005.
#[test]
fn ac3_tolerance() {
    let t = tolerance(0.0123456, 2).unwrap();
    assert_eq!(t.exp10, Some(-3));
    assert_eq!(t.delta, 0.0005);
}

/// The oracle's tolerance table, bit for bit, and the exact `2 s <= delta` test.
#[test]
fn tolerance_table() {
    let doc = read("tolerance.json");
    let mut n = 0;
    for line in doc.lines().filter(|l| l.trim_start().starts_with("{\"u\"")) {
        let u = hex(raw(line, 0, "u"));
        let n_dig: u32 = raw(line, 0, "n_dig").parse().unwrap();
        let t = tolerance(u, n_dig).unwrap();
        let e = raw(line, 0, "exp10");
        let want = if e == "null" { None } else { Some(e.parse::<i32>().unwrap()) };
        assert_eq!(t.exp10, want, "u = {u}, n_dig = {n_dig}");
        assert_eq!(t.delta.to_bits(), hex(raw(line, 0, "delta")).to_bits(), "u = {u}, n_dig = {n_dig}");
        n += 1;
    }
    assert!(n >= 30);
    let mut n = 0;
    for line in doc.lines().filter(|l| l.trim_start().starts_with("{\"s\"")) {
        let s = hex(raw(line, 0, "s"));
        let l: i32 = raw(line, 0, "exp10").parse().unwrap();
        let want: bool = raw(line, 0, "admits").parse().unwrap();
        let t = Tolerance { exp10: Some(l), delta: 0.0 };
        assert_eq!(t.admits(s), want, "s = {s}, l = {l}");
        n += 1;
    }
    assert!(n >= 14);
    // u(y) = 0 (and -0) give delta = 0, admitting only s = 0.
    for z in [0.0, -0.0] {
        let t = tolerance(z, 2).unwrap();
        assert_eq!((t.exp10, t.delta), (None, 0.0));
        assert!(t.admits(0.0));
        assert!(!t.admits(f64::from_bits(1)));
    }
}

/// AC-4: p = 0.99 gives M = 10 000 and p = 0.9999 gives M = 1 000 000 (p as written),
/// plus the oracle's table.
#[test]
fn ac4_block_size() {
    assert_eq!(block_size(0.99).unwrap(), 10_000);
    assert_eq!(block_size(0.9999).unwrap(), 1_000_000);
    let doc = read("block_size.json");
    let mut n = 0;
    for line in doc.lines().filter(|l| l.trim_start().starts_with("{\"p\"")) {
        let p = hex(raw(line, 0, "p"));
        let want: u64 = raw(line, 0, "block_size").parse().unwrap();
        assert_eq!(block_size(p).unwrap(), want, "p = {p}");
        n += 1;
    }
    assert!(n >= 16);
}

/// Profile validation (`invalid_profile`) and the recorded fields (REQ-004).
#[test]
fn profile_validation_and_record() {
    let g = GumSuiteProfile::Jcgm101_2008;
    for p in [0.0, -0.0, 1.0, 1.5, -0.1, f64::NAN, f64::INFINITY] {
        let e = AdaptProfile::new(g, p, 2, DEFAULT_CAP).unwrap_err();
        assert_eq!(e, McmError::InvalidProfile { detail: "p" }, "p = {p}");
        assert_eq!(e.code(), "invalid_profile");
    }
    for n in [0, 18] {
        assert_eq!(
            AdaptProfile::new(g, 0.95, n, DEFAULT_CAP).unwrap_err(),
            McmError::InvalidProfile { detail: "n_dig" }
        );
    }
    assert!(AdaptProfile::new(g, 0.95, 17, DEFAULT_CAP).is_ok());
    // cap < 2 M.
    assert_eq!(AdaptProfile::new(g, 0.95, 2, 19_999).unwrap_err(), McmError::InvalidProfile { detail: "cap" });
    assert!(AdaptProfile::new(g, 0.95, 2, 20_000).is_ok());
    // p = 0.9999 needs a cap of at least 2 * 10^6; the default 10^6 is refused.
    assert_eq!(AdaptProfile::with_defaults(0.9999).unwrap_err(), McmError::InvalidProfile { detail: "cap" });
    assert_eq!(AdaptProfile::new(g, 0.9999, 2, 2_000_000).unwrap().block_size(), 1_000_000);

    let pr = AdaptProfile::with_defaults(0.95).unwrap();
    let blocks = [BlockSummary { y_mean: 1.0, u: 1.0, low: 0.0, high: 2.0 }; 2];
    let step = decide(&pr, &blocks, 1.0).unwrap();
    assert_eq!(step.profile.version(), "mcm-adapt@1");
    assert_eq!(step.profile.gum_suite_profile().as_str(), "jcgm101-2008");
    assert_eq!(step.profile.p(), 0.95);
    assert_eq!(step.profile.block_size(), 10_000);
    assert_eq!(step.profile.n_dig(), 2);
    assert_eq!(step.profile.cap(), 1_000_000);
    assert_eq!(step.decision, Decision::Stop);
}

/// Input errors of `decide`.
#[test]
fn decide_errors() {
    let pr = AdaptProfile::new(GumSuiteProfile::Jcgm101_2008, 0.95, 2, 30_000).unwrap();
    let ok = BlockSummary { y_mean: 1.0, u: 1.0, low: 0.0, high: 2.0 };
    assert_eq!(decide(&pr, &[], 1.0).unwrap_err(), McmError::Num(NumError::EmptyInput { op: "mcm.adapt.blocks" }));
    let e = decide(&pr, &[ok; 4], 1.0).unwrap_err();
    assert_eq!(e, McmError::CapExceeded { blocks: 4, max: 3 });
    assert_eq!(e.code(), "cap_exceeded");
    let mut bad = [ok; 2];
    bad[1].low = f64::NAN;
    assert_eq!(
        decide(&pr, &bad, 1.0).unwrap_err(),
        McmError::Num(NumError::NotANumber { op: "mcm.adapt.blocks", index: Some(1) })
    );
    bad[1].low = f64::NEG_INFINITY;
    assert_eq!(
        decide(&pr, &bad, 1.0).unwrap_err(),
        McmError::Num(NumError::Infinite { op: "mcm.adapt.blocks", index: Some(1) })
    );
    bad[1] = ok;
    bad[0].u = -1.0;
    let e = decide(&pr, &bad, 1.0).unwrap_err();
    assert_eq!(e, McmError::InvalidSummary { block: Some(0) });
    assert_eq!(e.code(), "invalid_summary");
    bad[0] = BlockSummary { low: 3.0, ..ok };
    assert_eq!(decide(&pr, &bad, 1.0).unwrap_err(), McmError::InvalidSummary { block: Some(0) });
    assert_eq!(decide(&pr, &[ok; 2], -1.0).unwrap_err(), McmError::InvalidSummary { block: None });
    assert_eq!(
        decide(&pr, &[ok; 2], f64::NAN).unwrap_err(),
        McmError::Num(NumError::NotANumber { op: "mcm.adapt.u", index: None })
    );
    // One block always continues, even with identical summaries.
    assert_eq!(decide(&pr, &[ok], 1.0).unwrap().decision, Decision::Continue);
}

// ------------------------------------------------------------------------ AC-H runs

#[derive(Clone, Copy, PartialEq)]
enum Example {
    Normal4,
    Rect4,
}

impl Example {
    fn name(self) -> &'static str {
        match self {
            Example::Normal4 => "normal4",
            Example::Rect4 => "rect4",
        }
    }
    fn base_seed(self) -> u64 {
        match self {
            Example::Normal4 => 0xAD_0000,
            Example::Rect4 => 0xAD_1000,
        }
    }
    fn input(self) -> Distribution {
        let one = |x| Quantity::new(x, UnitCode::One);
        match self {
            Example::Normal4 => Distribution::normal(one(0.0), one(1.0)).unwrap(),
            Example::Rect4 => {
                let h = f::sqrt(3.0).unwrap();
                Distribution::rect(one(-h), one(h)).unwrap()
            }
        }
    }
}

struct Run {
    steps: Vec<AdaptStep>,
    fin: McmSummary,
}

/// One adaptive run: block `b` holds trials `b M .. (b + 1) M`, and trial `t` draws input
/// `k` at RNG address `(seed, k, t)`, so every block is a pure function of the seed and
/// its index and the run replays exactly. Y = ((X1 + X2) + X3) + X4; summaries use the
/// probabilistically symmetric interval and f(x) = 0.
fn run(ex: Example, seed: u64, profile: &AdaptProfile) -> Run {
    let d = ex.input();
    let m = profile.block_size();
    let p = profile.p();
    let mut all: Vec<f64> = Vec::new();
    let mut blocks = Vec::new();
    let mut steps = Vec::new();
    loop {
        let b = blocks.len() as u64;
        let cols: Vec<Vec<f64>> = (0..4).map(|k| d.sample(seed, k, b * m..(b + 1) * m).unwrap()).collect();
        let v: Vec<f64> = (0..m as usize).map(|i| ((cols[0][i] + cols[1][i]) + cols[2][i]) + cols[3][i]).collect();
        blocks.push(BlockSummary::symmetric(&summarize(&v, p, 0.0).unwrap()));
        all.extend_from_slice(&v);
        let fin = summarize(&all, p, 0.0).unwrap();
        let step = decide(profile, &blocks, fin.u).unwrap();
        steps.push(step);
        if step.decision != Decision::Continue {
            return Run { steps, fin };
        }
    }
}

struct Reference {
    u: f64,
    low: f64,
    high: f64,
    trials_pred: u64,
}

fn reference(doc: &str, ex: Example) -> Reference {
    let at = doc.find(&format!("\"{}\": {{", ex.name())).unwrap();
    let num = |k: &str| raw(doc, at, k).trim_matches('"').parse::<f64>().unwrap();
    Reference {
        u: num("u"),
        low: num("sym_low"),
        high: num("sym_high"),
        trials_pred: raw(doc, at, "trials_pred").parse().unwrap(),
    }
}

/// Verdict of a one-sided exact binomial test of "pass rate >= 0.95" from the oracle's
/// thresholds: `falsified` if the one-sided 95 % Clopper-Pearson upper bound is below 0.95,
/// `supported` if the one-sided lower bound is at least 0.95, else `inconclusive`.
fn verdict(pass: u64, falsified_at_most: u64, supported_at_least: u64) -> &'static str {
    if pass <= falsified_at_most {
        "falsified"
    } else if pass >= supported_at_least {
        "supported"
    } else {
        "inconclusive"
    }
}

/// Verdicts recorded in NODE.md for the hypothesis-scale run (1000 seeds per example,
/// release build): (example, quantity, pass count, verdict). The run is deterministic
/// (D0), so a full run must reproduce these counts exactly.
const RECORDED: [(&str, &str, u64, &str); 6] = [
    ("normal4", "u", 1000, "supported"),
    ("normal4", "sym_low", 935, "falsified"),
    ("normal4", "sym_high", 940, "inconclusive"),
    ("rect4", "u", 1000, "supported"),
    ("rect4", "sym_low", 938, "inconclusive"),
    ("rect4", "sym_high", 950, "inconclusive"),
];

/// AC-H1 and AC-H2 as restated in NODE.md (simulated runs, not measurements).
///
/// AC-H1: on the two additive-model examples with closed-form output distributions, the
/// stopped u(y) and symmetric 95 % endpoints (from all trials of the run) lie within delta
/// of the analytic values for at least 95 % of runs, per quantity, judged over 1000 seeds
/// by the exact one-sided binomial test of [`verdict`].
/// AC-H2: the median trial count at stop over the same seeds is within a factor 2 of the
/// trial count predicted from the asymptotic per-block standard deviations (oracle).
/// Seeds: normal4 0xAD_0000 + s, rect4 0xAD_1000 + s (streams 0..3), s = 0..999.
#[test]
fn ac_h_adaptive_runs() {
    let full = std::env::var("UNC_MCM_ADAPT_FULL").map(|v| v == "1").unwrap_or(false);
    let doc = read("ac_h_reference.json");
    let n_full: u64 = raw(&doc, 0, "seeds").parse().unwrap();
    let k_f: u64 = raw(&doc, 0, "falsified_if_at_most").parse().unwrap();
    let k_s: u64 = raw(&doc, 0, "supported_if_at_least").parse().unwrap();
    let cp_first: u64 = raw(&doc, 0, "clopper_pearson_95_first_k").parse().unwrap();
    let cp_raw = raw(&doc, 0, "clopper_pearson_95");
    let cp: Vec<String> = cp_raw.trim_matches(['[', ']']).split("], [").map(|x| x.replace('"', "")).collect();
    assert_eq!(cp.len() as u64, n_full - cp_first + 1);
    let seeds = if full { n_full } else { 4 };
    let scale = if full { "hypothesis scale" } else { "smoke, not the hypothesis scale" };
    let profile = AdaptProfile::with_defaults(0.95).unwrap();
    let mut measured = Vec::new();
    for ex in [Example::Normal4, Example::Rect4] {
        let r = reference(&doc, ex);
        let mut pass = [0u64; 3];
        let mut caps = 0u64;
        let mut trials = Vec::new();
        for s in 0..seeds {
            let run = run(ex, ex.base_seed() + s, &profile);
            let last = run.steps.last().unwrap();
            if last.decision == Decision::CapReached {
                caps += 1;
            }
            trials.push(last.trials);
            let delta = last.tolerance.unwrap().delta;
            let errs = [(run.fin.u - r.u).abs(), (run.fin.sym.low - r.low).abs(), (run.fin.sym.high - r.high).abs()];
            for k in 0..3 {
                if errs[k] <= delta {
                    pass[k] += 1;
                }
            }
        }
        trials.sort_unstable();
        let n = trials.len();
        let median = if n % 2 == 1 { trials[n / 2] as f64 } else { (trials[n / 2 - 1] + trials[n / 2]) as f64 / 2.0 };
        for (k, name) in ["u", "sym_low", "sym_high"].iter().enumerate() {
            let (bounds, v) = if full {
                let b = if pass[k] >= cp_first { cp[(pass[k] - cp_first) as usize].clone() } else { "< 0.8".into() };
                (b, verdict(pass[k], k_f, k_s))
            } else {
                ("n/a".to_string(), "n/a")
            };
            println!(
                "AC-H1 simulated {} {name}: {}/{seeds} runs within delta ({scale}), exact two-sided 95 % bounds [{bounds}], verdict {v}",
                ex.name(),
                pass[k]
            );
            measured.push((ex.name(), *name, pass[k], v));
        }
        let pred = r.trials_pred as f64;
        let h2 = if median >= pred / 2.0 && median <= pred * 2.0 { "supported" } else { "falsified" };
        println!(
            "AC-H2 simulated {}: median trials at stop {median} (predicted {pred}, {scale}), verdict {h2}; cap reached {caps}/{seeds}; min {} max {}",
            ex.name(),
            trials[0],
            trials[n - 1]
        );
        assert_eq!(caps, 0, "{}: a run reached the cap", ex.name());
        if full {
            assert_eq!(h2, "supported", "AC-H2 {}", ex.name());
        }
    }
    if full {
        assert_eq!(measured, RECORDED, "the hypothesis-scale run no longer reproduces the counts in NODE.md");
    }
}

/// Replay: the same seed gives the same decisions and bits; blocks do not depend on the
/// blocks before them (the RNG is addressed by trial index).
#[test]
fn runs_replay_exactly() {
    let profile = AdaptProfile::with_defaults(0.95).unwrap();
    let a = run(Example::Rect4, 0xAD_2000, &profile);
    let b = run(Example::Rect4, 0xAD_2000, &profile);
    assert_eq!(format!("{:?}", a.steps), format!("{:?}", b.steps));
    assert_eq!(a.fin.u.to_bits(), b.fin.u.to_bits());
    let d = Example::Rect4.input();
    let whole = d.sample(7, 2, 0..30_000).unwrap();
    let third = d.sample(7, 2, 20_000..30_000).unwrap();
    assert_eq!(&whole[20_000..], &third[..]);
}

// ------------------------------------------------------------------------ D0 bits

fn hex_bytes(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// D0 bit identity across targets. `crate_bits.txt` pins this crate's exact output bits:
/// the four `s` of every step of the AC-1 and AC-2 sequences, and a SHA-256 over the runs
/// of seeds 0 and 1 of both AC-H examples (stop block, trials, final u and endpoints, the
/// four `s`). It is produced by this test with `UNC_MCM_ADAPT_PRINT_BITS=1` (crate output,
/// not an oracle: correctness is checked against the oracle by the other tests), and CI
/// runs this file on wasm32-wasip1, x86-64 (Linux, Windows) and ARM64 (macOS), so a pass
/// everywhere shows identical bits on all of them.
#[test]
fn d0_bits_are_pinned() {
    let mut lines = vec![
        "# UNC.PROP.MCM.ADAPT D0 bit pins: crate output, regenerate with UNC_MCM_ADAPT_PRINT_BITS=1".to_string(),
        "# sequence h decision s_estimate s_u s_low s_high (IEEE-754 bits, hex)".to_string(),
    ];
    for name in ["summaries_converge.json", "summaries_slow.json"] {
        let s = seq(&read(name));
        for st in replay(&s) {
            if let Some(sd) = st.sd {
                lines.push(format!(
                    "{name} {} {} {:016x} {:016x} {:016x} {:016x}",
                    st.blocks,
                    st.decision.code(),
                    sd[0].to_bits(),
                    sd[1].to_bits(),
                    sd[2].to_bits(),
                    sd[3].to_bits()
                ));
            }
        }
    }
    let profile = AdaptProfile::with_defaults(0.95).unwrap();
    let mut bytes = Vec::new();
    let mut stops = Vec::new();
    for ex in [Example::Normal4, Example::Rect4] {
        for s in 0..2u64 {
            let r = run(ex, ex.base_seed() + s, &profile);
            let last = r.steps.last().unwrap();
            stops.push(format!("{}:{}", ex.name(), last.blocks));
            bytes.extend_from_slice(&last.blocks.to_le_bytes());
            bytes.extend_from_slice(&last.trials.to_le_bytes());
            for x in [r.fin.u, r.fin.sym.low, r.fin.sym.high] {
                bytes.extend_from_slice(&x.to_le_bytes());
            }
            for st in &r.steps {
                for x in st.sd.unwrap_or([0.0; 4]) {
                    bytes.extend_from_slice(&x.to_le_bytes());
                }
            }
        }
    }
    lines.push(format!("ac_h_seeds_0_1 stop_blocks {}", stops.join(" ")));
    lines.push(format!("ac_h_seeds_0_1 sha256:{}", hex_bytes(&fid_kci::kci::sha256::sha256(&bytes))));
    let text = lines.join("\n") + "\n";
    if std::env::var("UNC_MCM_ADAPT_PRINT_BITS").map(|v| v == "1").unwrap_or(false) {
        std::fs::write(format!("{FIX}crate_bits.txt"), &text).unwrap();
    }
    assert_eq!(text, read("crate_bits.txt"));
}
