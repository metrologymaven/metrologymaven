//! The convergence rule of spec node KRN.SOLV.CERT.RECOMP at the rounding floor: nonlinear
//! problems whose residuals are evaluated in plain `f64` (no double-double) must recompute and
//! certify when their Gauss-Newton steps stall at the rounding floor, and an unconverged
//! problem must still be refused.
//!
//! Expected values come from `fixtures/solv/cert/recomp-floor-v1/make_cases.py` (90-digit
//! Newton-polished optima of minimum-zone partial arcs, long cylinders and cylinders 300 mm from
//! the origin, each verified a strict local minimum; independent of this crate, of any solver
//! and of the platform libm). All 64 generated candidates are emitted (no selection). The test
//! replays the recomputation's Gauss-Newton iteration bit for bit and counts the floor cases,
//! whose last step exceeds `4 u ||p||` (the step-only rule before the fix refused them); at
//! least `MIN_FLOOR` must be floor cases, and every candidate must recompute. `OUTPUT_PIN` is a
//! regression pin of this crate's own output (not an oracle); CI checks it on x86-64 Linux and
//! Windows, ARM64 macOS and wasm32.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_num::la::{lstsq, Mat};
use fid_num::{f, red};
use fid_solv::cert::active::{extract, Proposal, RowLabel, Thresholds};
use fid_solv::cert::kkt::{KktFail, KktStatus, Verdict};
use fid_solv::cert::recomp::{
    recompute, ActiveSetProblem, Interval, RecompConfig, Recomputed, DEFAULT_REL_TOL, STEP_TOL,
};
use fid_solv::cert::{certify, CertConfig, CertError, CertKind};
use fid_solv::conic::{Cone, ConicProblem, ConicProblemBuilder, ConicResiduals, ConicSolution, Csc};
use fid_testkit::Sha256Hasher;

const FIXTURE: &str = include_str!("../../../fixtures/solv/cert/recomp-floor-v1/cases.txt");

fn hexes(s: &str) -> Vec<f64> {
    s.split_whitespace().map(|t| f64::from_bits(u64::from_str_radix(t, 16).unwrap())).collect()
}

fn labels(s: &str) -> Vec<RowLabel> {
    s.split_whitespace()
        .map(|t| {
            let (p, sd) = t.split_once(':').unwrap();
            RowLabel { point: p.parse().unwrap(), side: sd.parse().unwrap() }
        })
        .collect()
}

fn ip(x: f64) -> Interval {
    Interval::point(x).unwrap()
}

// ------------------------------------------------------------------ test problem

/// Minimum-zone circle (`geo = 2`, points `(x, y)`, `p = (cx, cy, r_in, r_out)`) or cylinder axis
/// (`geo = 4`, points `(x, y, z)`, `p = (x0, y0, a, b, r_in, r_out)`, the axis through
/// `(x0, y0, 0)` with direction `(a, b, 1)`). Rows `(i, 0)`: `g = r_in - d_i`, `(i, 1)`:
/// `g = d_i - r_out`; the active-set residual is `d_i - r_side`, **in plain `f64`**. Every
/// enclosure the recomputation and the KKT check need is implemented. The circle start comes
/// from the set (perpendicular bisectors of each contact group, one QR solve); the cylinder
/// start is the fixture's nominal axis (problem data).
struct Geo {
    geo: usize,
    pts: Vec<[f64; 3]>,
    start: Vec<f64>,
}

impl Geo {
    fn new(kind: &str, flat: &[f64], start: Vec<f64>) -> Geo {
        let (geo, pts) = match kind {
            "arc" => (2, flat.chunks(2).map(|c| [c[0], c[1], 0.0]).collect()),
            "cyl" => (4, flat.chunks(3).map(|c| [c[0], c[1], c[2]]).collect()),
            k => panic!("kind {k}"),
        };
        Geo { geo, pts, start }
    }

    /// Distance of point `i` and its gradient in the geometric parameters.
    fn dist(&self, i: usize, p: &[f64]) -> (f64, Vec<f64>) {
        let [x, y, z] = self.pts[i];
        if self.geo == 2 {
            let (dx, dy) = (x - p[0], y - p[1]);
            let d = f::sqrt(dx * dx + dy * dy).unwrap();
            return (d, vec![-dx / d, -dy / d]);
        }
        let (a, b) = (p[2], p[3]);
        let (wx, wy, wz) = (x - p[0], y - p[1], z);
        let (cx, cy, cz) = (wy - wz * b, wz * a - wx, wx * b - wy * a);
        let nc = f::sqrt(cx * cx + cy * cy + cz * cz).unwrap();
        let vv = a * a + b * b + 1.0;
        let nv = f::sqrt(vv).unwrap();
        let k = nc * nv;
        let g = vec![
            (cy - b * cz) / k,
            (a * cz - cx) / k,
            (wz * cy - wy * cz) / k - nc * a / (nv * vv),
            (wx * cz - wz * cx) / k - nc * b / (nv * vv),
        ];
        (nc / nv, g)
    }

    /// Interval distance of point `i` and its gradient over the box `b`.
    fn dist_iv(&self, i: usize, b: &[Interval]) -> Result<(Interval, Vec<Interval>), CertError> {
        let [x, y, z] = self.pts[i];
        if self.geo == 2 {
            let dx = ip(x).sub(b[0])?;
            let dy = ip(y).sub(b[1])?;
            let d = dx.sqr()?.add(dy.sqr()?)?.sqrt()?;
            return Ok((d, vec![dx.div(d)?.neg(), dy.div(d)?.neg()]));
        }
        let (a, bb) = (b[2], b[3]);
        let (wx, wy, wz) = (ip(x).sub(b[0])?, ip(y).sub(b[1])?, ip(z));
        let cx = wy.sub(wz.mul(bb)?)?;
        let cy = wz.mul(a)?.sub(wx)?;
        let cz = wx.mul(bb)?.sub(wy.mul(a)?)?;
        let nc = cx.sqr()?.add(cy.sqr()?)?.add(cz.sqr()?)?.sqrt()?;
        let vv = a.sqr()?.add(bb.sqr()?)?.add(ip(1.0))?;
        let nv = vv.sqrt()?;
        let k = nc.mul(nv)?;
        let nvv = nv.mul(vv)?;
        let g = vec![
            cy.sub(bb.mul(cz)?)?.div(k)?,
            a.mul(cz)?.sub(cx)?.div(k)?,
            wz.mul(cy)?.sub(wy.mul(cz)?)?.div(k)?.sub(nc.mul(a)?.div(nvv)?)?,
            wx.mul(cz)?.sub(wz.mul(cx)?)?.div(k)?.sub(nc.mul(bb)?.div(nvv)?)?,
        ];
        Ok((nc.div(nv)?, g))
    }

    fn grad_row(&self, side: u8, gd: &[Interval]) -> Vec<Interval> {
        let mut row = vec![ip(0.0); self.n_params()];
        for (k, g) in gd.iter().enumerate() {
            row[k] = if side == 0 { g.neg() } else { *g };
        }
        row[self.geo + side as usize] = ip(if side == 0 { 1.0 } else { -1.0 });
        row
    }

    fn obj_grad(&self) -> Vec<f64> {
        let mut g = vec![0.0; self.n_params()];
        g[self.geo] = -1.0;
        g[self.geo + 1] = 1.0;
        g
    }
}

impl ActiveSetProblem for Geo {
    fn n_params(&self) -> usize {
        self.geo + 2
    }
    fn expected_active(&self) -> usize {
        self.geo + 2
    }
    fn row_labels(&self) -> Vec<RowLabel> {
        (0..self.pts.len() as u32).flat_map(|point| [0, 1].map(|side| RowLabel { point, side })).collect()
    }
    fn is_linear(&self) -> bool {
        false
    }
    fn start_from_set(&self, set: &[RowLabel]) -> Result<Vec<f64>, CertError> {
        if self.geo == 4 {
            return Ok(self.start.clone());
        }
        let mut rows = Vec::new();
        let mut rhs = Vec::new();
        let mut radii = Vec::new();
        for side in [0, 1] {
            let group: Vec<usize> = set.iter().filter(|l| l.side == side).map(|l| l.point as usize).collect();
            let Some((&g0, rest)) = group.split_first() else {
                return Err(CertError::Callback { reason: "arc set needs an inner and an outer point" });
            };
            radii.push(g0);
            let [x0, y0, _] = self.pts[g0];
            for &k in rest {
                let [x1, y1, _] = self.pts[k];
                rows.push(vec![2.0 * (x1 - x0), 2.0 * (y1 - y0)]);
                rhs.push((x1 * x1 + y1 * y1) - (x0 * x0 + y0 * y0));
            }
        }
        let s = lstsq(&Mat::from_rows(&rows)?, &rhs, 0.0)?;
        let c = [s.x[0], s.x[1]];
        let mut p = c.to_vec();
        for i in radii {
            p.push(self.dist(i, &c).0);
        }
        Ok(p)
    }
    fn residuals_and_jacobian(&self, set: &[RowLabel], p: &[f64]) -> Result<(Vec<f64>, Mat), CertError> {
        let mut r = Vec::new();
        let mut rows = Vec::new();
        for l in set {
            let (d, gd) = self.dist(l.point as usize, p);
            r.push(d - p[self.geo + l.side as usize]);
            let mut row = vec![0.0; self.n_params()];
            row[..gd.len()].copy_from_slice(&gd);
            row[self.geo + l.side as usize] = -1.0;
            rows.push(row);
        }
        Ok((r, Mat::from_rows(&rows)?))
    }
    fn value(&self, p: &[f64]) -> Result<f64, CertError> {
        Ok(p[self.geo + 1] - p[self.geo])
    }
    fn constraint_values(&self, p: &[f64]) -> Result<Vec<f64>, CertError> {
        Ok(self
            .row_labels()
            .iter()
            .map(|l| {
                let d = self.dist(l.point as usize, p).0;
                let r = p[self.geo + l.side as usize];
                if l.side == 0 {
                    r - d
                } else {
                    d - r
                }
            })
            .collect())
    }
    fn objective_gradient(&self, _: &[f64]) -> Result<Vec<f64>, CertError> {
        Ok(self.obj_grad())
    }
    fn constraint_gradients(&self, set: &[RowLabel], p: &[f64]) -> Result<Mat, CertError> {
        let rows: Vec<Vec<f64>> = set
            .iter()
            .map(|l| {
                let gd = self.dist(l.point as usize, p).1;
                let mut row = vec![0.0; self.n_params()];
                for (k, g) in gd.iter().enumerate() {
                    row[k] = if l.side == 0 { -g } else { *g };
                }
                row[self.geo + l.side as usize] = if l.side == 0 { 1.0 } else { -1.0 };
                row
            })
            .collect();
        Ok(Mat::from_rows(&rows)?)
    }
    fn residual_enclosure(&self, set: &[RowLabel], p: &[f64]) -> Result<Option<Vec<Interval>>, CertError> {
        let b: Vec<Interval> = p.iter().map(|&x| ip(x)).collect();
        let mut out = Vec::new();
        for l in set {
            let (d, _) = self.dist_iv(l.point as usize, &b)?;
            out.push(d.sub(b[self.geo + l.side as usize])?);
        }
        Ok(Some(out))
    }
    fn jacobian_enclosure(&self, set: &[RowLabel], b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        let mut out = Vec::new();
        for l in set {
            let (_, gd) = self.dist_iv(l.point as usize, b)?;
            let mut row = vec![ip(0.0); self.n_params()];
            row[..gd.len()].copy_from_slice(&gd);
            row[self.geo + l.side as usize] = ip(-1.0);
            out.extend(row);
        }
        Ok(Some(out))
    }
    fn value_enclosure(&self, b: &[Interval]) -> Result<Option<Interval>, CertError> {
        Ok(Some(b[self.geo + 1].sub(b[self.geo])?))
    }
    fn data_scale(&self) -> Result<f64, CertError> {
        let mut m = 0.0_f64;
        for v in self.pts.iter().flatten() {
            m = f::total_max(m, v.abs())?;
        }
        Ok(m)
    }
    fn constraint_enclosure(&self, b: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        let mut out = Vec::new();
        for l in self.row_labels() {
            let (d, _) = self.dist_iv(l.point as usize, b)?;
            let r = b[self.geo + l.side as usize];
            out.push(if l.side == 0 { r.sub(d)? } else { d.sub(r)? });
        }
        Ok(Some(out))
    }
    fn objective_gradient_enclosure(&self, _: &[Interval]) -> Result<Option<Vec<Interval>>, CertError> {
        Ok(Some(self.obj_grad().into_iter().map(ip).collect()))
    }
    fn constraint_gradient_enclosure(
        &self,
        set: &[RowLabel],
        b: &[Interval],
    ) -> Result<Option<Vec<Interval>>, CertError> {
        let mut out = Vec::new();
        for l in set {
            let (_, gd) = self.dist_iv(l.point as usize, b)?;
            out.extend(self.grad_row(l.side, &gd));
        }
        Ok(Some(out))
    }
}

// ------------------------------------------------------------------ fixture

struct Case {
    name: String,
    kind: String,
    points: Vec<f64>,
    start: Vec<f64>,
    set: Vec<RowLabel>,
    params: Vec<f64>,
    value: f64,
    expect: String,
    gn_steps: Option<u32>,
}

impl Case {
    fn problem(&self) -> Geo {
        Geo::new(&self.kind, &self.points, self.start.clone())
    }
}

fn parse() -> Vec<Case> {
    let mut out = Vec::new();
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut head = String::new();
    for l in FIXTURE.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        if let Some(rest) = l.strip_prefix("case ") {
            head = rest.to_string();
            fields.clear();
        } else if l == "end" {
            let get = |k: &str| fields.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone()).unwrap_or_default();
            let h: Vec<&str> = head.split_whitespace().collect();
            let steps = get("gn_steps");
            out.push(Case {
                name: h[0].to_string(),
                kind: h[2].to_string(),
                points: hexes(&get("points")),
                start: hexes(&get("start")),
                set: labels(&get("set")),
                params: hexes(&get("params")),
                value: hexes(get("value").split_whitespace().next().unwrap_or("")).first().copied().unwrap_or(0.0),
                expect: get("expect"),
                gn_steps: if steps.is_empty() { None } else { Some(steps.parse().unwrap()) },
            });
        } else {
            let (k, v) = l.split_once(' ').unwrap_or((l, ""));
            fields.push((k.to_string(), v.to_string()));
        }
    }
    out
}

/// A conic problem with one nonnegative row per label and a solution in which exactly the rows
/// of `set` have zero slack and positive multipliers (the extraction only classifies rows).
fn conic(all: &[RowLabel], set: &[RowLabel]) -> (ConicProblem, ConicSolution) {
    let m = all.len();
    let p = ConicProblemBuilder::new(vec![1.0], Csc::zeros(m, 1), vec![1.0; m], vec![Cone::Nonneg(m)]).build().unwrap();
    let (mut s, mut y) = (Vec::new(), Vec::new());
    for (i, l) in all.iter().enumerate() {
        if set.contains(l) {
            s.push(0.0);
            y.push(0.25);
        } else {
            s.push(0.5 + i as f64);
            y.push(0.0);
        }
    }
    let sol = ConicSolution {
        x: vec![0.0],
        y,
        s,
        objective: 0.0,
        iterations: 7,
        residuals: ConicResiduals { primal: 0.0, dual: 0.0, gap_abs: 0.0, gap_rel: 0.0 },
    };
    (p, sol)
}

fn proposal(all: &[RowLabel], set: &[RowLabel]) -> Proposal {
    let (p, sol) = conic(all, set);
    let th = Thresholds { slack_rel: 1e-9, mult_rel: 1e-9 };
    let pr = extract(&p, &sol, all, set.len(), &th).unwrap();
    assert_eq!(pr.uncertified_sets().len(), 1);
    pr
}

fn run(prob: &Geo, set: &[RowLabel], cfg: &RecompConfig) -> Result<Recomputed, CertError> {
    recompute(prob, &proposal(&prob.row_labels(), set), 0, cfg)
}

/// The recomputation's Gauss-Newton iteration replayed with the same calls in the same order
/// (sorted set, `start_from_set`, `lstsq(J, r, rel_tol)`, `p_j -= x_j`): the final parameters
/// and the last step, bit for bit.
fn replay(prob: &Geo, set: &[RowLabel], steps: u32) -> (Vec<f64>, Vec<f64>) {
    let mut sorted = set.to_vec();
    sorted.sort_unstable();
    let mut p = prob.start_from_set(&sorted).unwrap();
    let mut step = Vec::new();
    for _ in 0..steps {
        let (r, j) = prob.residuals_and_jacobian(&sorted, &p).unwrap();
        step = lstsq(&j, &r, DEFAULT_REL_TOL).unwrap().x;
        for (pi, xi) in p.iter_mut().zip(&step) {
            *pi -= xi;
        }
    }
    (p, step)
}

fn ulp(x: f64) -> f64 {
    let a = x.abs();
    f64::from_bits(a.to_bits() + 1) - a
}

/// The proven bound must contain the exact optimum; the reference is the exact optimum rounded
/// to nearest, so `|got - ref| <= bound + ulp(ref)`.
fn check_bound(name: &str, what: &str, got: f64, reference: f64, bound: f64) {
    let err = (got - reference).abs();
    assert!(bound.is_finite() && bound >= 0.0, "{name}: {what} bound {bound}");
    assert!(err <= bound + ulp(reference), "{name}: {what} error {err:e} outside proven bound {bound:e}");
}

fn bits(v: &[f64]) -> Vec<u64> {
    v.iter().map(|x| x.to_bits()).collect()
}

// ------------------------------------------------------------------ tests

#[test]
fn fixture_counts() {
    let cases = parse();
    let count = |p: &str| cases.iter().filter(|c| c.name.starts_with(p)).count();
    assert_eq!((count("arc-"), count("arc-far-"), count("cyl-long-"), count("cyl-far-")), (32, 16, 16, 16));
    assert_eq!(cases.iter().filter(|c| c.expect == "no_convergence").count(), 1);
    assert_eq!(cases.len(), 65);
}

/// At least this many candidates are floor cases (the step-only rule refused them).
const MIN_FLOOR: usize = 19;

/// Every generated candidate (no selection) recomputes with the replayed parameters and a proven
/// bound that contains the exact optimum, and at least [`MIN_FLOOR`] of them are floor cases: the
/// step-only rule (`s <= 4 u ||p||`) would refuse them. A floor case is accepted only within the
/// new allowance. Then `certify` runs end to end: it passes, or stops only at the KKT
/// stationarity check (see `kkt_stationarity_limit`).
#[test]
fn all_candidates_recompute() {
    let cfg = RecompConfig::default();
    let mut floor = Vec::new();
    let mut kkt_limited = Vec::new();
    for c in parse().iter().filter(|c| c.expect == "certify") {
        let prob = c.problem();
        let (p, step) = replay(&prob, &c.set, cfg.gn_steps);
        let s = red::norm2(&step).unwrap();
        let old_thr = STEP_TOL * red::norm2(&p).unwrap();

        let r = run(&prob, &c.set, &cfg).unwrap_or_else(|e| panic!("{}: {e:?}", c.name));
        assert_eq!(bits(r.params()), bits(&p), "{}", c.name);
        let scale = c.params.iter().fold(0.0_f64, |a, v| if v.abs() > a { v.abs() } else { a });
        for (i, (&g, &w)) in r.params().iter().zip(&c.params).enumerate() {
            check_bound(&c.name, &format!("param {i}"), g, w, r.param_bounds()[i]);
        }
        check_bound(&c.name, "value", r.value(), c.value, r.value_bound());
        assert!(r.param_bounds().iter().all(|&b| b <= 1e-9 * scale), "{}: {:?}", c.name, r.param_bounds());
        if s > old_thr {
            floor.push(c.name.clone());
            // Accepted only within the allowance 4 u ||p|| + 2 ||k||.
            let k = red::norm2(r.param_bounds()).unwrap();
            assert!(s <= old_thr + 2.0 * k, "{}", c.name);
        }

        let (conic_p, sol) = conic(&prob.row_labels(), &c.set);
        match certify(&prob, &conic_p, &sol, CertKind::Zone, &CertConfig::default()) {
            Ok(cert) => assert_eq!(bits(cert.params()), bits(r.params()), "{}", c.name),
            Err(CertError::CertificateFailed { tried: 1, first_report: Some(rep), first_error: None }) => {
                assert_eq!(rep.status(), KktStatus::Undecidable, "{}", c.name);
                assert_eq!(rep.reason(), Some(KktFail::Stationarity), "{}", c.name);
                for cond in [rep.primal(), rep.dual(), rep.complementarity()] {
                    assert_eq!(cond.verdict(), Verdict::Satisfied, "{}", c.name);
                }
                kkt_limited.push(c.name.clone());
            }
            Err(e) => panic!("{}: certify {e:?}", c.name),
        }
    }
    eprintln!("floor cases ({}): {floor:?}", floor.len());
    eprintln!("KKT stationarity undecidable ({}): {kkt_limited:?}", kkt_limited.len());
    assert!(floor.len() >= MIN_FLOOR, "only {} floor cases: {floor:?}", floor.len());
    // Only cylinders far from the origin (gradients in (a, b) of about 300) hit the KKT limit.
    assert!(kkt_limited.iter().all(|n| n.starts_with("cyl-far-")), "{kkt_limited:?}");
}

/// The unconverged case: its step budget leaves a last step far above both `4 u ||p||` and twice
/// any proven radius around the final point, so it is still refused.
#[test]
fn unconverged_case_still_fails() {
    let c = parse().into_iter().find(|c| c.expect == "no_convergence").unwrap();
    let cfg = RecompConfig { gn_steps: c.gn_steps.unwrap(), ..RecompConfig::default() };
    let prob = c.problem();
    let (_, step) = replay(&prob, &c.set, cfg.gn_steps);
    let s = red::norm2(&step).unwrap();
    assert_eq!(run(&prob, &c.set, &cfg), Err(CertError::RecomputeNoConvergence { step_norm: s }));
    // With the default budget the same start converges and certifies.
    let r = run(&prob, &c.set, &RecompConfig::default()).unwrap();
    for (i, (&g, &w)) in r.params().iter().zip(&c.params).enumerate() {
        check_bound(&c.name, &format!("param {i}"), g, w, r.param_bounds()[i]);
    }
}

fn digest(name: &str, r: &Result<Recomputed, CertError>, h: &mut Sha256Hasher) {
    h.update(name.as_bytes());
    match r {
        Ok(r) => {
            for l in r.set() {
                h.update(&l.point.to_le_bytes());
                h.update(&[l.side]);
            }
            for v in r.params().iter().chain(r.param_bounds()).chain(&[r.value(), r.value_bound(), r.cond()]) {
                h.update(&v.to_bits().to_le_bytes());
            }
            h.update(&r.steps().to_le_bytes());
        }
        Err(e) => h.update(format!("{e:?}").as_bytes()),
    }
}

/// Output-bit pin of every case (its own step budget), recorded from a local x86-64 run and
/// checked by CI on x86-64 Linux and Windows, ARM64 macOS and wasm32 under wasmtime
/// (REQ-KRN.SOLV.CERT.RECOMP-005: a target that diverges is a failure, not a new pin).
const OUTPUT_PIN: &str = "sha256:ac75dc549609915e1aee1f471ea8611c44927fdbc046cdaf9aa21621af82b714";

#[test]
fn output_bits_pinned() {
    let mut h = Sha256Hasher::new();
    for c in &parse() {
        let cfg = RecompConfig {
            gn_steps: c.gn_steps.unwrap_or(RecompConfig::default().gn_steps),
            ..RecompConfig::default()
        };
        digest(&c.name, &run(&c.problem(), &c.set, &cfg), &mut h);
    }
    assert_eq!(h.finish(), OUTPUT_PIN, "output bits changed on {}", std::env::consts::ARCH);
}
