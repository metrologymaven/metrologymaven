//! Acceptance criteria of spec node KRN.SOLV.CERT.AS (AC-1 to AC-4) and the input checks.
//!
//! Expected values come from `fixtures/solv/cert/as-v1/make_cases.py` (exact rational
//! decisions, independent of this crate and of any solver) or are hand-labelled from the
//! spec's rules. The output-bit pins in `OUTPUT_PINS` are regression pins of this crate's own
//! output (not an oracle); CI checks them on x86-64 Linux and Windows, ARM64 macOS and wasm32.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_solv::cert::active::{extract, Proposal, RowLabel, Thresholds, MAX_CANDIDATES};
use fid_solv::cert::{CertError, Undetermined};
use fid_solv::conic::{solve_lp, Cone, ConicProblem, ConicProblemBuilder, ConicResiduals, ConicSolution, Csc};
use fid_testkit::Sha256Hasher;

const FIXTURE: &str = include_str!("../../../fixtures/solv/cert/as-v1/cases.txt");

fn hexes(s: &str) -> Vec<f64> {
    s.split_whitespace().map(|t| f64::from_bits(u64::from_str_radix(t, 16).unwrap())).collect()
}

fn usizes(s: &str) -> Vec<usize> {
    s.split_whitespace().map(|t| t.parse().unwrap()).collect()
}

fn labels(s: &str) -> Vec<RowLabel> {
    s.split_whitespace()
        .map(|t| {
            let (p, sd) = t.split_once(':').unwrap();
            RowLabel { point: p.parse().unwrap(), side: sd.parse().unwrap() }
        })
        .collect()
}

#[derive(Clone)]
struct Case {
    name: String,
    kind: String,
    n: usize,
    rows: Vec<Vec<f64>>,
    b: Vec<f64>,
    q: Vec<f64>,
    x: Vec<f64>,
    y: Vec<f64>,
    s: Vec<f64>,
    labels: Vec<RowLabel>,
    expected: usize,
    thresholds: Thresholds,
    scales: (f64, f64),
    strongly_active: Vec<RowLabel>,
    ambiguous: Vec<RowLabel>,
    result: Vec<String>,
    sets: Vec<Vec<RowLabel>>,
    known: Vec<Vec<RowLabel>>,
    solver_sets: Vec<Vec<RowLabel>>,
}

fn dense_rows(m: usize, n: usize, colptr: &[usize], rowval: &[usize], nzval: &[f64]) -> Vec<Vec<f64>> {
    let mut rows = vec![vec![0.0; n]; m];
    for j in 0..n {
        for k in colptr[j]..colptr[j + 1] {
            rows[rowval[k]][j] = nzval[k];
        }
    }
    rows
}

fn csc(rows: &[Vec<f64>], n: usize) -> Csc {
    let (mut colptr, mut rowval, mut nzval) = (vec![0], Vec::new(), Vec::new());
    for j in 0..n {
        for (i, r) in rows.iter().enumerate() {
            if r[j] != 0.0 {
                rowval.push(i);
                nzval.push(r[j]);
            }
        }
        colptr.push(rowval.len());
    }
    Csc::new(rows.len(), n, colptr, rowval, nzval)
}

fn parse() -> (Thresholds, Vec<Case>) {
    let mut solver_th = None;
    let mut out = Vec::new();
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut head = String::new();
    for l in FIXTURE.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        if let Some(rest) = l.strip_prefix("solver_thresholds ") {
            let t = hexes(rest);
            solver_th = Some(Thresholds { slack_rel: t[0], mult_rel: t[1] });
        } else if let Some(rest) = l.strip_prefix("case ") {
            head = rest.to_string();
            fields.clear();
        } else if l == "end" {
            let get = |k: &str| fields.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone()).unwrap_or_default();
            let all = |k: &str| -> Vec<String> {
                fields.iter().filter(|(key, _)| key == k).map(|(_, v)| v.clone()).collect()
            };
            let h: Vec<&str> = head.split_whitespace().collect();
            let n: usize = get("n").parse().unwrap();
            let m: usize = get("m").parse().unwrap();
            let rows = dense_rows(m, n, &usizes(&get("a_colptr")), &usizes(&get("a_rowval")), &hexes(&get("a_nzval")));
            let th = hexes(&get("thresholds"));
            let sc = hexes(&get("scales"));
            out.push(Case {
                name: h[0].to_string(),
                kind: h[2].to_string(),
                n,
                rows,
                b: hexes(&get("b")),
                q: hexes(&get("q")),
                x: hexes(&get("x")),
                y: hexes(&get("y")),
                s: hexes(&get("s")),
                labels: labels(&get("labels")),
                expected: get("expected").parse().unwrap(),
                thresholds: Thresholds { slack_rel: th[0], mult_rel: th[1] },
                scales: (sc[0], sc[1]),
                strongly_active: labels(&get("strongly_active")),
                ambiguous: labels(&get("ambiguous")),
                result: get("result").split_whitespace().map(String::from).collect(),
                sets: all("set").iter().map(|s| labels(s)).collect(),
                known: all("known").iter().map(|s| labels(s)).collect(),
                solver_sets: all("solver_set").iter().map(|s| labels(s)).collect(),
            });
        } else {
            let (k, v) = l.split_once(' ').unwrap_or((l, ""));
            fields.push((k.to_string(), v.to_string()));
        }
    }
    (solver_th.unwrap(), out)
}

fn problem(c: &Case) -> ConicProblem {
    ConicProblemBuilder::new(c.q.clone(), csc(&c.rows, c.n), c.b.clone(), vec![Cone::Nonneg(c.b.len())])
        .build()
        .unwrap()
}

fn solution(c: &Case) -> ConicSolution {
    ConicSolution {
        x: c.x.clone(),
        y: c.y.clone(),
        s: c.s.clone(),
        objective: 0.0,
        iterations: 0,
        residuals: ConicResiduals { primal: 0.0, dual: 0.0, gap_abs: 0.0, gap_rel: 0.0 },
    }
}

fn run(c: &Case) -> Result<Proposal, CertError> {
    extract(&problem(c), &solution(c), &c.labels, c.expected, &c.thresholds)
}

fn bits(v: (f64, f64)) -> (u64, u64) {
    (v.0.to_bits(), v.1.to_bits())
}

fn check_case(c: &Case, got: &Result<Proposal, CertError>) {
    match c.result[0].as_str() {
        "ok" => {
            let g = got.as_ref().unwrap_or_else(|e| panic!("{}: {e:?}", c.name));
            assert_eq!(g.uncertified_sets(), &c.sets[..], "{}: sets", c.name);
            assert_eq!(g.strongly_active(), &c.strongly_active[..], "{}: strongly active", c.name);
            assert_eq!(g.ambiguous(), &c.ambiguous[..], "{}: ambiguous", c.name);
            assert_eq!(g.tie_broken(), c.result[2] == "1", "{}: tie_broken", c.name);
            assert_eq!(g.over_determined(), c.result[4] == "1", "{}: over_determined", c.name);
            assert_eq!(bits(g.scales()), bits(c.scales), "{}: scales", c.name);
            assert_eq!(
                bits((g.thresholds().slack_rel, g.thresholds().mult_rel)),
                bits((c.thresholds.slack_rel, c.thresholds.mult_rel)),
                "{}: thresholds recorded",
                c.name
            );
        }
        "undetermined" => {
            let cause = match c.result[1].as_str() {
                "TooFewRows" => Undetermined::TooFewRows,
                "OverCap" => Undetermined::OverCap,
                k => panic!("cause {k}"),
            };
            let want = CertError::ActiveSetUndetermined {
                strongly_active: c.strongly_active.clone(),
                ambiguous: c.ambiguous.clone(),
                cause,
            };
            assert_eq!(got.as_ref().err(), Some(&want), "{}", c.name);
        }
        r => panic!("result {r}"),
    }
}

/// AC-1, and every boundary and undetermined case of the fixture.
#[test]
fn ac1_fixture_cases() {
    let (_, cases) = parse();
    let count = |k: &str| cases.iter().filter(|c| c.kind == k).count();
    assert_eq!(count("nondegenerate"), 30);
    assert_eq!(count("tie-ambiguous") + count("tie-overdetermined"), 10);
    for c in &cases {
        check_case(c, &run(c));
        if c.kind == "nondegenerate" {
            assert_eq!(c.sets, c.known, "{}: the known contact set", c.name);
        }
        if c.kind.starts_with("tie-") {
            // every planted contact basis is listed, in lexicographic order
            let at: Vec<usize> = c.known.iter().map(|k| c.sets.iter().position(|s| s == k).unwrap()).collect();
            assert!(at.windows(2).all(|w| w[0] < w[1]), "{}", c.name);
        }
    }
}

/// AC-1 on solver output: each planted LP solved by the conic wrapper, then extracted. The
/// candidates equal the fixture's (path independence: the oracle's vertex, sub-threshold and
/// interior duals and the solver's own multipliers give one list). Every contact row comes out
/// strongly active (the interior-point method converges to the analytic centre of the optimal
/// dual face), so the tied cases are over-determined.
#[test]
fn ac1_solved_by_the_wrapper() {
    let (th, cases) = parse();
    let mut solved = 0;
    for c in cases.iter().filter(|c| !c.solver_sets.is_empty()) {
        let p = problem(c);
        let sol = solve_lp(&p).unwrap();
        let got = extract(&p, &sol, &c.labels, c.expected, &th).unwrap();
        assert_eq!(got.uncertified_sets(), &c.solver_sets[..], "{}", c.name);
        assert_eq!(got.uncertified_sets(), &c.sets[..], "{}: path independence", c.name);
        assert!(got.ambiguous().is_empty(), "{}", c.name);
        assert_eq!(got.tie_broken(), c.solver_sets.len() > 1, "{}", c.name);
        assert_eq!(got.over_determined(), c.solver_sets.len() > 1, "{}", c.name);
        if c.kind == "nondegenerate" {
            assert_eq!(got.uncertified_sets(), &c.known[..], "{}", c.name);
        }
        solved += 1;
    }
    assert_eq!(solved, 42);
}

fn by_points(sets: &[Vec<RowLabel>]) -> Vec<Vec<u32>> {
    sets.iter().map(|s| s.iter().map(|l| l.point).collect()).collect()
}

/// AC-2's problem with the given multipliers on rows 0:0, 1:1, 3:1, 4:0 (all slacks exact).
fn ac2_problem(mult: [f64; 4]) -> (ConicProblem, ConicSolution, Vec<RowLabel>) {
    let v = [-1.0, 1.0, 0.0, 1.0, -1.0];
    let (mut rows, mut b, mut s, mut y, mut lab) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (i, &vi) in v.iter().enumerate() {
        let u = i as f64;
        for side in 0..2u8 {
            let lower = side == 0;
            rows.push(if lower { vec![1.0, u, -1.0] } else { vec![-1.0, -u, -1.0] });
            b.push(if lower { vi } else { -vi });
            s.push(if lower { vi + 1.0 } else { 1.0 - vi });
            y.push(match (i, side) {
                (0, 0) => mult[0],
                (1, 1) => mult[1],
                (3, 1) => mult[2],
                (4, 0) => mult[3],
                _ => 0.0,
            });
            lab.push(RowLabel { point: i as u32, side });
        }
    }
    let p = ConicProblemBuilder::new(vec![0.0, 0.0, 1.0], csc(&rows, 3), b, vec![Cone::Nonneg(10)]).build().unwrap();
    let sol = ConicSolution {
        x: vec![0.0, 0.0, 1.0],
        y,
        s,
        objective: 1.0,
        iterations: 0,
        residuals: ConicResiduals { primal: 0.0, dual: 0.0, gap_abs: 0.0, gap_rel: 0.0 },
    };
    (p, sol, lab)
}

const AC2_SETS: [[u32; 3]; 4] = [[0, 1, 3], [0, 1, 4], [0, 3, 4], [1, 3, 4]];

/// AC-2 (amended): 5 points, v = (-1, 1, 0, 1, -1), points 1 and 3 both on the upper boundary
/// of the zone (line 0, half-width 1), expected 3. The small-slack rows are 0:0, 1:1, 3:1,
/// 4:0, so the candidates are their four 3-subsets in lexicographic order; the two bases
/// {0, 1, 4} and {0, 3, 4} are among them in that order ({0, 1, 3} and {1, 3, 4} have no
/// nonnegative dual and fail the KKT check in `KRN.SOLV.CERT.CT`).
#[test]
fn ac2_five_point_tie() {
    let (p, sol, lab) = ac2_problem([0.25, 1e-15, 1e-15, 0.25]);
    let th = Thresholds { slack_rel: 1e-9, mult_rel: 1e-9 };
    let got = extract(&p, &sol, &lab, 3, &th).unwrap();
    assert_eq!(by_points(got.uncertified_sets()), AC2_SETS.map(Vec::from).to_vec());
    assert!(got.tie_broken() && !got.over_determined());
    // the same problem in the fixture (oracle-checked) and solved by the wrapper
    let (sth, cases) = parse();
    let c = cases.iter().find(|c| c.name == "ac2-five-point-tie").unwrap();
    assert_eq!(by_points(run(c).unwrap().uncertified_sets()), AC2_SETS.map(Vec::from).to_vec());
    let pc = problem(c);
    let solved = extract(&pc, &solve_lp(&pc).unwrap(), &c.labels, 3, &sth).unwrap();
    assert_eq!(by_points(solved.uncertified_sets()), AC2_SETS.map(Vec::from).to_vec());
    assert!(solved.over_determined());
}

/// Path independence: a vertex solution and an interior solution of the same tie LP (they
/// differ only in the multipliers) give the same candidate list. AC-2's problem: the two
/// vertex duals (3/8, 1/2, 0, 1/8) and (1/8, 0, 1/2, 3/8), their mean (an interior point of
/// the dual face) and a sub-threshold iterate. Also every tied fixture LP against the
/// wrapper's own (interior-point) solution.
#[test]
fn vertex_and_interior_solutions_agree() {
    let th = Thresholds { slack_rel: 1e-9, mult_rel: 1e-9 };
    let duals =
        [[0.375, 0.5, 0.0, 0.125], [0.125, 0.0, 0.5, 0.375], [0.25, 0.25, 0.25, 0.25], [0.25, 1e-15, 1e-15, 0.25]];
    let lists: Vec<(Vec<Vec<u32>>, bool)> = duals
        .iter()
        .map(|d| {
            let (p, sol, lab) = ac2_problem(*d);
            let got = extract(&p, &sol, &lab, 3, &th).unwrap();
            (by_points(got.uncertified_sets()), got.over_determined())
        })
        .collect();
    for (sets, _) in &lists {
        assert_eq!(sets, &AC2_SETS.map(Vec::from).to_vec());
    }
    // the diagnostic flag does depend on the path (vertex: no, interior: yes)
    assert_eq!(lists.iter().map(|l| l.1).collect::<Vec<_>>(), vec![false, false, true, false]);

    let (sth, cases) = parse();
    for c in cases.iter().filter(|c| c.kind.starts_with("tie-")) {
        let p = problem(c);
        let solved = extract(&p, &solve_lp(&p).unwrap(), &c.labels, c.expected, &sth).unwrap();
        let given = run(c).unwrap();
        assert_eq!(solved.uncertified_sets(), given.uncertified_sets(), "{}", c.name);
        assert!(solved.tie_broken() && given.tie_broken(), "{}", c.name);
    }
}

/// AC-3: only 2 rows of small slack, expected size 4.
#[test]
fn ac3_too_few_rows() {
    let rows: Vec<Vec<f64>> = (0..6).map(|_| vec![1.0]).collect();
    let p = ConicProblemBuilder::new(vec![1.0], csc(&rows, 1), vec![1.0; 6], vec![Cone::Nonneg(6)]).build().unwrap();
    let sol = ConicSolution {
        x: vec![1.0],
        y: vec![0.5, 0.5, 0.0, 0.0, 0.0, 0.0],
        s: vec![0.0, 0.0, 0.5, 0.5, 0.5, 0.5],
        objective: 1.0,
        iterations: 0,
        residuals: ConicResiduals { primal: 0.0, dual: 0.0, gap_abs: 0.0, gap_rel: 0.0 },
    };
    let lab: Vec<RowLabel> = (0..6).map(|i| RowLabel { point: i, side: 0 }).collect();
    let th = Thresholds { slack_rel: 1e-9, mult_rel: 1e-9 };
    let got = extract(&p, &sol, &lab, 4, &th);
    assert_eq!(
        got,
        Err(CertError::ActiveSetUndetermined {
            strongly_active: lab[..2].to_vec(),
            ambiguous: Vec::new(),
            cause: Undetermined::TooFewRows,
        })
    );
}

fn reversed(c: &Case) -> Case {
    let mut r = c.clone();
    r.rows.reverse();
    r.b.reverse();
    r.y.reverse();
    r.s.reverse();
    r.labels.reverse();
    r
}

/// AC-4: rows presented in reverse order, labels permuted with them, give the same result
/// (every fixture case, including the errors).
#[test]
fn ac4_row_order_invariance() {
    let (_, cases) = parse();
    for c in &cases {
        let a = run(c);
        let b = run(&reversed(c));
        assert_eq!(a, b, "{}", c.name);
        check_case(c, &b);
    }
}

fn tiny() -> (ConicProblem, ConicSolution, Vec<RowLabel>) {
    let rows = vec![vec![1.0], vec![-1.0]];
    let p = ConicProblemBuilder::new(vec![1.0], csc(&rows, 1), vec![1.0, 0.0], vec![Cone::Nonneg(2)]).build().unwrap();
    let sol = ConicSolution {
        x: vec![0.0],
        y: vec![0.0, 1.0],
        s: vec![1.0, 0.0],
        objective: 0.0,
        iterations: 0,
        residuals: ConicResiduals { primal: 0.0, dual: 0.0, gap_abs: 0.0, gap_rel: 0.0 },
    };
    (p, sol, vec![RowLabel { point: 0, side: 1 }, RowLabel { point: 0, side: 0 }])
}

const TH: Thresholds = Thresholds { slack_rel: 1e-9, mult_rel: 1e-9 };

#[test]
fn single_candidate_req_002() {
    let (p, sol, lab) = tiny();
    let got = extract(&p, &sol, &lab, 1, &TH).unwrap();
    assert_eq!(got.uncertified_sets(), &[vec![RowLabel { point: 0, side: 0 }]]);
    assert!(!got.tie_broken() && !got.over_determined() && got.ambiguous().is_empty());
    assert_eq!(bits(got.scales()), bits((1.0, 1.0)));
}

#[test]
fn invalid_inputs_are_typed() {
    let (p, sol, lab) = tiny();
    let inv = |field| Err(CertError::InvalidInput { field });
    for bad in [f64::NAN, f64::INFINITY, -1e-300, -1.0] {
        assert_eq!(extract(&p, &sol, &lab, 1, &Thresholds { slack_rel: bad, mult_rel: 1e-9 }), inv("thresholds"));
        assert_eq!(extract(&p, &sol, &lab, 1, &Thresholds { slack_rel: 1e-9, mult_rel: bad }), inv("thresholds"));
    }
    assert_eq!(extract(&p, &sol, &lab, 0, &TH), inv("expected"));

    let mut s2 = sol.clone();
    s2.y[1] = f64::NAN;
    assert_eq!(extract(&p, &s2, &lab, 1, &TH), inv("y"));
    let mut s3 = sol.clone();
    s3.s[0] = f64::NEG_INFINITY;
    assert_eq!(extract(&p, &s3, &lab, 1, &TH), inv("s"));

    assert_eq!(extract(&p, &sol, &lab[..1], 1, &TH), Err(CertError::Shape { field: "labels", expected: 2, got: 1 }));
    let mut s4 = sol.clone();
    s4.y.push(0.0);
    assert_eq!(extract(&p, &s4, &lab, 1, &TH), Err(CertError::Shape { field: "y", expected: 2, got: 3 }));
    let mut s5 = sol.clone();
    s5.s.pop();
    assert_eq!(extract(&p, &s5, &lab, 1, &TH), Err(CertError::Shape { field: "s", expected: 2, got: 1 }));

    let dup = vec![RowLabel { point: 3, side: 1 }, RowLabel { point: 3, side: 1 }];
    assert_eq!(extract(&p, &sol, &dup, 1, &TH), Err(CertError::DuplicateLabel { label: dup[0] }));

    // equality rows and second-order cones are not classified (fail closed)
    let rows = vec![vec![1.0], vec![-1.0]];
    for cones in [vec![Cone::Zero(1), Cone::Nonneg(1)], vec![Cone::Soc(2)]] {
        let q = ConicProblemBuilder::new(vec![1.0], csc(&rows, 1), vec![1.0, 0.0], cones).build().unwrap();
        assert_eq!(extract(&q, &sol, &lab, 1, &TH), inv("cones"));
    }
}

#[test]
fn cap_is_256() {
    assert_eq!(MAX_CANDIDATES, 256);
    let (_, cases) = parse();
    let c = cases.iter().find(|c| c.name == "at-cap-252").unwrap();
    assert_eq!(run(c).unwrap().uncertified_sets().len(), 252);
}

fn digest(name: &str, r: &Result<Proposal, CertError>, h: &mut Sha256Hasher) {
    h.update(name.as_bytes());
    match r {
        Ok(c) => {
            let (sc, th) = (c.scales(), c.thresholds());
            for v in [sc.0, sc.1, th.slack_rel, th.mult_rel] {
                h.update(&v.to_bits().to_le_bytes());
            }
            h.update(&[u8::from(c.tie_broken()), u8::from(c.over_determined())]);
            let mut put = |ls: &[RowLabel], tag: u8| {
                h.update(&[tag]);
                for l in ls {
                    h.update(&l.point.to_le_bytes());
                    h.update(&[l.side]);
                }
            };
            put(c.strongly_active(), 1);
            put(c.ambiguous(), 2);
            for s in c.uncertified_sets() {
                put(s, 3);
            }
        }
        Err(e) => h.update(format!("{e:?}").as_bytes()),
    }
}

/// Output-bit pins: regression pins of this crate's own output (fixture solutions, then the
/// wrapper-solved LPs), recorded from a local x86-64 run and checked by CI on every target.
/// One value serves all targets because they agree bit for bit; a target that diverged would
/// get its own `#[cfg(target_arch)]` value here.
const OUTPUT_PIN: &str = "sha256:151983945983a1091ac92bb6545c4fc7026a6a6299a275e6f6f1c102005f5f1f";

#[test]
fn output_bits_pinned() {
    let (th, cases) = parse();
    let mut h = Sha256Hasher::new();
    for c in &cases {
        digest(&c.name, &run(c), &mut h);
    }
    for c in cases.iter().filter(|c| !c.solver_sets.is_empty()) {
        let p = problem(c);
        let sol = solve_lp(&p).unwrap();
        digest(&c.name, &extract(&p, &sol, &c.labels, c.expected, &th), &mut h);
    }
    assert_eq!(h.finish(), OUTPUT_PIN, "output bits changed on {}", std::env::consts::ARCH);
}
