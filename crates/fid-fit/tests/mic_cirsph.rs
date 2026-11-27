//! Acceptance criteria of `KRN.ENV.MIC.CIRSPH`, through the kernel entry points
//! (`fit_mic_circle2d`, `fit_mic_sphere` and the `Fit` trait): AC-1 (inscribed radii of the
//! seeded lobed holes and sockets of `fixtures/fit/mic-cirsph-v1` against the exact oracle of
//! `make_mic_cirsph_fixtures.py` and the generator's analytic radius), AC-2 (the agreement job
//! against the Voronoi circle of `KRN.ENV.XCHK` on the same trust polygon, with injected
//! disagreements), AC-3 (a 60-degree arc is `unbounded_envelope` and `on_region_boundary`), AC-4
//! (output-bit pin, checked by CI on x86-64 Linux and Windows, ARM64 macOS and wasm32 under
//! wasmtime) and AC-5 (spec cards), plus error paths. Expected values come only from the Python
//! oracle; `OUTPUT_PIN` is a regression pin of this crate's own output, recorded from a local
//! x86-64 run (not an oracle). The points are regenerated here by the fixture's pinned rule and
//! checked against its digest, so a drift in the generator fails before any fit runs.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::result_large_err)]

use fid_fit::contract::{Constraint, Fit, FitError, FitParams, FittedFeature, Method, PointsView, Primitive, Weights};
use fid_fit::ls_rnd::Circle2Ls;
use fid_fit::mic_cirsph::{
    agreement_tol_floor, algorithm, algorithm_ids, check_agreement, command_rows, fit_mic_circle2d, fit_mic_sphere,
    trust_polygon, CrossOutcome, Disagreement, MicCircle2Fit, MicFit, MicParams, MicSphereFit, Optimality,
};
use fid_fit::mic_tr::TrustRegion;
use fid_fit::xchk::mic_circle2d_vor;
use fid_kci::{check, parse_card, InputArity};
use fid_testkit::Sha256Hasher;
use fid_types::{Determinism, SideEffect};
use std::sync::OnceLock;

const CASES_TXT: &str = include_str!("../../../fixtures/fit/mic-cirsph-v1/cases.txt");
const CARDS: [&str; 2] =
    [include_str!("../cards/fit.circle2.mi@1.0.0.toml"), include_str!("../cards/fit.sphere.mi@1.0.0.toml")];

/// The AC-1 command parameters: `--tr-fraction 0.2 --seed 3`.
const TR_FRACTION: f64 = 0.2;
const SEED: u64 = 3;
/// Boundary tolerance of the unbounded-envelope check (metres).
const BOUNDARY_TOL: f64 = 1e-9;
/// AC-1: radius against the analytic inscribed radius (metres).
const ANALYTIC_TOL: f64 = 1e-8;
/// Radius and centre against the exact oracle (metres).
const ORACLE_TOL: f64 = 1e-11;
/// REQ-004 / AC-2 agreement tolerance (metres) and the injected shift.
const AGREE_TOL: f64 = 1e-9;
const SHIFT: f64 = 1e-8;
/// The agreement job runs the O(n^2) Voronoi path only up to this many points.
const AGREE_MAX_POINTS: usize = 5000;

fn hex(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).expect("hex f64"))
}

struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn uniform(&mut self) -> f64 {
        (self.next() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

// The fixture's pinned generator (make_mic_cirsph_fixtures.py, module docstring).
fn direction(rng: &mut SplitMix64, dim: usize) -> [f64; 3] {
    loop {
        let mut x = [0.0; 3];
        for xj in x.iter_mut().take(dim) {
            *xj = 2.0 * rng.uniform() - 1.0;
        }
        let mut n2 = x[0] * x[0] + x[1] * x[1];
        if dim == 3 {
            n2 += x[2] * x[2];
        }
        if (0.0625..=1.0).contains(&n2) {
            let s = n2.sqrt();
            return [x[0] / s, x[1] / s, if dim == 3 { x[2] / s } else { 0.0 }];
        }
    }
}

fn form(u: &[f64; 3], dim: usize) -> f64 {
    if dim == 2 {
        let t = u[0] * u[0];
        1.0 + (4.0 * t - 3.0) * u[0]
    } else {
        1.0 + ((5.196152422706632 * u[0]) * u[1]) * u[2]
    }
}

fn valleys(dim: usize) -> Vec<[f64; 3]> {
    if dim == 2 {
        let h = 0.75_f64.sqrt();
        vec![[0.5, h, 0.0], [-1.0, 0.0, 0.0], [0.5, -h, 0.0]]
    } else {
        let s = 1.0 / 3.0_f64.sqrt();
        vec![[s, s, -s], [s, -s, s], [-s, s, s], [-s, -s, -s]]
    }
}

fn point(c: &[f64; 3], rho: f64, u: &[f64; 3], dim: usize) -> [f64; 3] {
    if dim == 2 {
        [c[0] + rho * u[0], c[1] + rho * u[1], c[2]]
    } else {
        [c[0] + rho * u[0], c[1] + rho * u[1], c[2] + rho * u[2]]
    }
}

struct Gen {
    n: usize,
    seed: u64,
    r: f64,
    a: f64,
    g: f64,
    c: [f64; 3],
}

fn gen_lobed(p: &Gen, dim: usize) -> Vec<[f64; 3]> {
    let mut rng = SplitMix64(p.seed);
    let ra = p.r - p.a;
    let vs = valleys(dim);
    let m = vs.len();
    let mut out = Vec::with_capacity(p.n);
    for i in 0..p.n {
        if let Some(k) = (0..m).find(|&k| (2 * k + 1) * p.n / (2 * m) == i) {
            out.push(point(&p.c, ra, &vs[k], dim));
            continue;
        }
        let u = direction(&mut rng, dim);
        let w = rng.uniform();
        let rho = (ra + p.a * form(&u, dim)) + p.g * (1.0 + w);
        out.push(point(&p.c, rho, &u, dim));
    }
    out
}

fn gen_arc(p: &Gen) -> Vec<[f64; 3]> {
    let mut rng = SplitMix64(p.seed);
    let h = 0.75_f64.sqrt();
    let mut out = Vec::with_capacity(p.n);
    while out.len() < p.n {
        let u = direction(&mut rng, 2);
        if u[0] * 0.6 + u[1] * 0.8 >= h {
            let w = rng.uniform();
            out.push(point(&p.c, p.r + p.g * (1.0 + w), &u, 2));
        }
    }
    out
}

enum Expect {
    Inscribed { r: f64, centre: Vec<f64>, analytic: f64, contacts: Vec<u32> },
    Unbounded,
}

struct Case {
    id: String,
    dim: usize,
    pts: Vec<[f64; 3]>,
    expect: Expect,
}

impl Case {
    fn xyz(&self) -> Vec<f64> {
        self.pts.iter().flatten().copied().collect()
    }

    fn xy(&self) -> Vec<[f64; 2]> {
        self.pts.iter().map(|p| [p[0], p[1]]).collect()
    }
}

/// The fixture's cases, loaded once.
fn cases() -> &'static [Case] {
    static CASES: OnceLock<Vec<Case>> = OnceLock::new();
    CASES.get_or_init(load)
}

/// Every case's fit at the AC-1 parameters, run once and shared by the tests.
fn results() -> &'static [Result<MicFit, FitError>] {
    static RESULTS: OnceLock<Vec<Result<MicFit, FitError>>> = OnceLock::new();
    RESULTS.get_or_init(|| cases().iter().map(run).collect())
}

/// Cases up to this many points also run through the `Fit` trait.
const TRAIT_MAX_POINTS: usize = 2000;

/// A case being read: id, dimension, arc flag, points and expectation.
type Partial = (String, usize, bool, Vec<[f64; 3]>, Option<Expect>);

fn load() -> Vec<Case> {
    let mut out = Vec::new();
    let mut cur: Option<Partial> = None;
    for line in CASES_TXT.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let t: Vec<&str> = line.split_whitespace().collect();
        match t[0] {
            "case" => {
                let dim = if t[2] == "circle" { 2 } else { 3 };
                cur = Some((t[1].to_string(), dim, t[3] == "arc", Vec::new(), None));
            }
            "gen" => {
                let c = cur.as_mut().unwrap();
                let g = Gen {
                    n: t[1].parse().unwrap(),
                    seed: t[2].parse().unwrap(),
                    r: hex(t[3]),
                    a: hex(t[4]),
                    g: hex(t[5]),
                    c: [hex(t[6]), hex(t[7]), hex(t[8])],
                };
                c.3 = if c.2 { gen_arc(&g) } else { gen_lobed(&g, c.1) };
            }
            "digest" => {
                let c = cur.as_ref().unwrap();
                let mut h = Sha256Hasher::new();
                for p in &c.3 {
                    for x in p {
                        h.update(&x.to_bits().to_le_bytes());
                    }
                }
                assert_eq!(h.finish(), format!("sha256:{}", t[1]), "{}: generator drift", c.0);
            }
            "expect" => {
                let c = cur.as_mut().unwrap();
                c.4 = Some(if t[1] == "unbounded" {
                    Expect::Unbounded
                } else {
                    let d = c.1;
                    assert_eq!(t[3 + d], "analytic");
                    assert_eq!(t[5 + d], "contacts");
                    let m: usize = t[6 + d].parse().unwrap();
                    let contacts: Vec<u32> = t[7 + d..].iter().map(|s| s.parse().unwrap()).collect();
                    assert_eq!(contacts.len(), m);
                    Expect::Inscribed {
                        r: hex(t[2]),
                        centre: (0..d).map(|j| hex(t[3 + j])).collect(),
                        analytic: hex(t[4 + d]),
                        contacts,
                    }
                });
            }
            "end" => {
                let (id, dim, _, pts, expect) = cur.take().unwrap();
                out.push(Case { id, dim, pts, expect: expect.unwrap() });
            }
            other => panic!("unknown line {other}"),
        }
    }
    assert_eq!(out.len(), 11);
    out
}

fn params() -> FitParams {
    FitParams::new(Method::MaximumInscribed)
}

fn mic() -> MicParams {
    MicParams::new(TR_FRACTION, BOUNDARY_TOL)
}

fn run(c: &Case) -> Result<MicFit, FitError> {
    let xyz = c.xyz();
    let view = PointsView::new(&xyz, None).unwrap();
    if c.dim == 2 {
        fit_mic_circle2d(&view, &params(), &mic(), Some(SEED))
    } else {
        fit_mic_sphere(&view, &params(), &mic(), Some(SEED))
    }
}

fn centre_radius(f: &FittedFeature) -> (Vec<f64>, f64) {
    match f {
        FittedFeature::Circle2 { c, r } => (c.to_vec(), *r),
        FittedFeature::Sphere { c, r } => (c.to_vec(), *r),
        other => panic!("unexpected feature {other:?}"),
    }
}

/// AC-1: the inscribed radius matches the analytic one within 1e-8 m and the exact oracle
/// within 1e-11 m, the contacts are the oracle's, no point lies inside by more than 1e-12 m,
/// global optimality is proven, and the `Fit` trait returns the same feature.
#[test]
fn ac1_inscribed_radii_match_the_oracle() {
    for (c, res) in cases().iter().zip(results()) {
        let Expect::Inscribed { r, centre, analytic, contacts } = &c.expect else { continue };
        let m = res.as_ref().unwrap_or_else(|e| panic!("{}: {e:?}", c.id));
        let (mc, mr) = centre_radius(&m.feature);
        assert!((mr - analytic).abs() <= ANALYTIC_TOL, "{}: radius {mr} vs analytic {analytic}", c.id);
        assert!((mr - r).abs() <= ORACLE_TOL, "{}: radius {mr} vs oracle {r}", c.id);
        for (j, (x, y)) in mc.iter().zip(centre).enumerate() {
            assert!((x - y).abs() <= ORACLE_TOL, "{}: centre[{j}] {x} vs {y}", c.id);
        }
        assert_eq!(&m.contacts, contacts, "{}", c.id);
        assert!(m.max_violation <= 1e-12, "{}: {}", c.id, m.max_violation);
        assert!(m.residuals.iter().all(|&e| e >= -1e-12), "{}", c.id);
        assert_eq!(m.cross_check.outcome, CrossOutcome::Passed, "{}: {:?}", c.id, m.cross_check);
        assert_eq!(m.optimality, Optimality::Global, "{}", c.id);
        assert!(!m.local_optimum_only && !m.certificate.flags().local_optimum_only, "{}", c.id);
        assert!(m.boundary_distance_m > BOUNDARY_TOL, "{}", c.id);
        assert_eq!(m.trust_region.fraction(), TR_FRACTION);
        assert_eq!(m.rescue_starts, 0, "{}", c.id);
        assert_eq!(m.algorithm, algorithm_ids()[c.dim - 2]);
        if c.pts.len() > TRAIT_MAX_POINTS {
            continue;
        }
        let xyz = c.xyz();
        let view = PointsView::new(&xyz, None).unwrap();
        let res = if c.dim == 2 {
            MicCircle2Fit { mic: mic() }.fit(&view, &params(), Some(SEED))
        } else {
            MicSphereFit { mic: mic() }.fit(&view, &params(), Some(SEED))
        }
        .unwrap();
        assert_eq!(res.feature, m.feature, "{}", c.id);
        let cert = res.certificate.unwrap();
        assert_eq!(cert.support, contacts.iter().map(|&i| i as usize).collect::<Vec<_>>());
    }
}

/// AC-3: a 60-degree arc has no bounded inscribed circle: the conic path returns
/// `unbounded_envelope` (through the record entry point and the trait) and the Voronoi path on
/// the same trust polygon reports `on_region_boundary`.
#[test]
fn ac3_sixty_degree_arc_is_unbounded_on_both_paths() {
    let mut arcs = 0;
    for (c, res) in cases().iter().zip(results()) {
        if !matches!(c.expect, Expect::Unbounded) {
            continue;
        }
        arcs += 1;
        let e = *res.as_ref().unwrap_err();
        let FitError::UnboundedEnvelope { boundary_distance_m } = e else { panic!("{}: {e:?}", c.id) };
        assert!(boundary_distance_m <= BOUNDARY_TOL, "{}", c.id);
        assert_eq!(e.code(), "unbounded_envelope");
        let xyz = c.xyz();
        let view = PointsView::new(&xyz, None).unwrap();
        let t = MicCircle2Fit { mic: mic() }.fit(&view, &params(), Some(SEED));
        assert!(matches!(t, Err(FitError::UnboundedEnvelope { .. })), "{}", c.id);
        let vor = xchk_on_trust_polygon(c);
        assert!(vor.on_region_boundary, "{}: Voronoi path {} m from the boundary", c.id, vor.boundary_distance_m);
    }
    assert_eq!(arcs, 2);
}

fn trust_region(c: &Case) -> TrustRegion {
    let xyz = c.xyz();
    let view = PointsView::new(&xyz, None).unwrap();
    let ls = Circle2Ls.fit(&view, &FitParams::new(Method::LeastSquares), None).unwrap();
    TrustRegion::from_feature(&ls.feature, TR_FRACTION).unwrap()
}

fn xchk_on_trust_polygon(c: &Case) -> fid_fit::xchk::Inscribed {
    let poly = trust_polygon(&trust_region(c)).unwrap();
    mic_circle2d_vor(&c.xy(), &poly, BOUNDARY_TOL).unwrap()
}

fn disagreement(id: &str, conic: &Result<MicFit, FitError>, vor: &fid_fit::xchk::Inscribed) -> Result<(), String> {
    check_agreement(conic, vor, AGREE_TOL).map_err(|d| format!("{id}: {d:?}"))
}

/// AC-2 / REQ-004: the agreement job on every circle case of the fixture up to
/// `AGREE_MAX_POINTS` points (the frozen `UNC.RDL.FREEZE` set is not available yet): it passes,
/// and an injected 1e-8 m shift of the centre or radius, or a flipped boundary status, fails it
/// with the case ID.
#[test]
fn ac2_agreement_job_and_injected_failures() {
    let mut ran = 0;
    for (c, conic) in cases().iter().zip(results()) {
        if c.dim != 2 || c.pts.len() > AGREE_MAX_POINTS {
            continue;
        }
        ran += 1;
        let vor = xchk_on_trust_polygon(c);
        disagreement(&c.id, conic, &vor).unwrap();
        match conic {
            Ok(m) => {
                for shift in [[SHIFT, 0.0, 0.0], [0.0, SHIFT, 0.0], [0.0, 0.0, SHIFT]] {
                    let mut bad = m.clone();
                    let FittedFeature::Circle2 { c: cc, r } = m.feature else { panic!() };
                    bad.feature = FittedFeature::Circle2 { c: [cc[0] + shift[0], cc[1] + shift[1]], r: r + shift[2] };
                    let msg = disagreement(&c.id, &Ok(bad), &vor).unwrap_err();
                    assert!(msg.starts_with(&format!("{}: ", c.id)), "{msg}");
                    assert!(msg.contains("Centre") || msg.contains("Radius"), "{msg}");
                }
                // The centre is compared by Euclidean distance: a diagonal shift of 0.8 tol per
                // coordinate (1.13 tol away) fails, though each coordinate is within tol.
                let mut diag = m.clone();
                let FittedFeature::Circle2 { c: cc, r } = m.feature else { panic!() };
                let d = 0.8 * AGREE_TOL;
                diag.feature = FittedFeature::Circle2 { c: [cc[0] + d, cc[1] + d], r };
                let msg = disagreement(&c.id, &Ok(diag), &vor).unwrap_err();
                assert!(msg.starts_with(&format!("{}: Centre", c.id)), "{msg}");
                // A tolerance below the values' rounding floor is refused.
                let floor = agreement_tol_floor(conic, &vor).unwrap();
                assert!(floor > 0.0 && floor < 1e-13, "{}: {floor}", c.id);
                assert_eq!(check_agreement(conic, &vor, floor / 2.0), Err(Disagreement::InvalidTolerance));
                assert_eq!(check_agreement(conic, &vor, floor), Ok(()));
                let mut flipped = vor.clone();
                flipped.on_region_boundary = true;
                let msg = disagreement(&c.id, conic, &flipped).unwrap_err();
                assert!(msg.starts_with(&format!("{}: Boundary", c.id)), "{msg}");
            }
            Err(_) => {
                let mut flipped = vor.clone();
                flipped.on_region_boundary = false;
                let msg = disagreement(&c.id, conic, &flipped).unwrap_err();
                assert!(msg.starts_with(&format!("{}: Boundary", c.id)), "{msg}");
            }
        }
    }
    assert_eq!(ran, 5);
    // A conic failure other than the boundary is a disagreement, and the tolerance is checked.
    let c = &cases()[0];
    let vor = xchk_on_trust_polygon(c);
    let failed = Err(FitError::TooFewPoints { n: 2, need: 3 });
    assert_eq!(check_agreement(&failed, &vor, AGREE_TOL), Err(Disagreement::ConicFailed { code: "too_few_points" }));
    for tol in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(check_agreement(&results()[0], &vor, tol), Err(Disagreement::InvalidTolerance));
    }
}

fn put(h: &mut Sha256Hasher, xs: &[f64]) {
    for x in xs {
        h.update(&x.to_bits().to_le_bytes());
    }
}

/// AC-4: output-bit pin of every case (feature, contacts, boundary distance, certificate digest,
/// residuals, or the error's boundary distance), recorded from a local x86-64 run and checked by
/// CI on x86-64 Linux and Windows, ARM64 macOS and wasm32 under wasmtime.
const OUTPUT_PIN: &str = "sha256:7638af08a11178038125d2b57820c6303cc578eb2a655c2f30a389bb576f8d6b";

#[test]
fn ac4_output_bits_pinned() {
    let mut h = Sha256Hasher::new();
    for (c, res) in cases().iter().zip(results()) {
        h.update(c.id.as_bytes());
        match res {
            Ok(m) => {
                let (cc, r) = centre_radius(&m.feature);
                put(&mut h, &cc);
                put(&mut h, &[r, m.boundary_distance_m, m.max_violation]);
                put(&mut h, &m.residuals);
                for k in &m.contacts {
                    h.update(&k.to_le_bytes());
                }
                h.update(m.certificate.digest().to_string().as_bytes());
                h.update(&[u8::from(m.agreement), u8::from(m.local_optimum_only)]);
                h.update(&m.cross_check.cells.to_le_bytes());
            }
            Err(FitError::UnboundedEnvelope { boundary_distance_m }) => put(&mut h, &[*boundary_distance_m]),
            Err(e) => panic!("{}: {e:?}", c.id),
        }
    }
    assert_eq!(h.finish(), OUTPUT_PIN, "output bits changed on {}", std::env::consts::ARCH);
}

/// The fit is D0 for a seed: the same seed gives the same record; another seed may visit other
/// starts but the proven global optimum is the same feature.
#[test]
fn seed_determinism() {
    let c = &cases()[1];
    let a = results()[1].as_ref().unwrap();
    assert_eq!(&run(c).unwrap(), a);
    let xyz = c.xyz();
    let view = PointsView::new(&xyz, None).unwrap();
    let b = fit_mic_circle2d(&view, &params(), &mic(), Some(SEED + 1)).unwrap();
    assert_eq!(b.feature, a.feature);
    assert_eq!(b.optimality, Optimality::Global);
}

/// AC-5: a spec card per algorithm ID and a clean card check; the command data rows (W2, dry
/// run R1, D0 per seed).
#[test]
fn ac5_spec_cards_and_command_rows() {
    let mut ids = Vec::new();
    for text in CARDS {
        let card = parse_card(text).expect("card parses");
        assert_eq!(card.determinism, "D0");
        assert_eq!(card.input_arity, InputArity::exactly(1));
        ids.push(card.id);
    }
    assert_eq!(ids, algorithm_ids().to_vec());
    let report = check(&algorithm_ids(), &ids);
    assert!(report.is_clean(), "{report:?}");
    // `algorithm` is fallible: no maximum inscribed 3D circle (method_undefined), and the
    // cylinder belongs to KRN.ENV.MIC.CYL, not this module.
    assert_eq!(algorithm(Primitive::Circle2).unwrap(), algorithm_ids()[0]);
    assert_eq!(algorithm(Primitive::Sphere).unwrap(), algorithm_ids()[1]);
    let e = algorithm(Primitive::Circle3).unwrap_err();
    assert_eq!(e, FitError::MethodUndefined { primitive: Primitive::Circle3, method: Method::MaximumInscribed });
    assert_eq!(e.code(), "method_undefined");
    assert_eq!(algorithm(Primitive::Cylinder), Err(FitError::InvalidParams { field: "primitive" }));
    let strs: Vec<String> = algorithm_ids().iter().map(|a| a.to_string()).collect();
    assert_eq!(strs, ["fit.circle2.mi@1.0.0", "fit.sphere.mi@1.0.0"]);
    let rows = command_rows().unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r.command.as_str()).collect();
    assert_eq!(names, ["fit.mi_circle2@1", "fit.mi_sphere@1"]);
    for row in &rows {
        row.validate().unwrap();
        assert_eq!((row.side_effect, row.dry_run, row.determinism), (SideEffect::W2, SideEffect::R1, Determinism::D0));
    }
}

#[test]
fn error_paths() {
    let cases = cases();
    let c = &cases[0];
    let xyz = c.xyz();
    let view = PointsView::new(&xyz, None).unwrap();
    // The trust-region fraction is required: no default, every non-positive or non-finite value
    // refused.
    for f in [0.0, -0.2, f64::NAN, f64::INFINITY] {
        let e = fit_mic_circle2d(&view, &params(), &MicParams::new(f, BOUNDARY_TOL), None);
        assert_eq!(e, Err(FitError::InvalidParams { field: "tr_fraction" }), "{f}");
    }
    // boundary_tol: non-finite, zero and below the region's rounding floor are refused.
    for t in [f64::NAN, f64::INFINITY, 0.0, -0.0, 1e-30] {
        let e = fit_mic_circle2d(&view, &params(), &MicParams::new(TR_FRACTION, t), None);
        assert_eq!(e, Err(FitError::InvalidParams { field: "boundary_tol" }), "{t}");
    }
    let mut m = mic();
    m.starts = 0;
    assert_eq!(fit_mic_circle2d(&view, &params(), &m, None), Err(FitError::InvalidParams { field: "starts" }));
    let mut m = mic();
    m.seq.kind = fid_solv::cert::CertKind::Zone;
    assert_eq!(fit_mic_circle2d(&view, &params(), &m, None), Err(FitError::InvalidParams { field: "seq" }));
    // Method, weights and constraints.
    let ls = FitParams::new(Method::LeastSquares);
    assert_eq!(fit_mic_circle2d(&view, &ls, &mic(), None), Err(FitError::InvalidParams { field: "method" }));
    let mut w = params();
    w.weights = Weights::PerPoint(vec![1.0; c.pts.len()]);
    assert_eq!(fit_mic_circle2d(&view, &w, &mic(), None), Err(FitError::InvalidParams { field: "weights" }));
    let mut fixed = params();
    fixed.constraints.push(Constraint::FixedRadius(0.001));
    assert_eq!(fit_mic_circle2d(&view, &fixed, &mic(), None), Err(FitError::InvalidParams { field: "constraints" }));
    let mut plane = params();
    plane.constraints.push(Constraint::ProjectionPlane { p: [0.0; 3], n: [0.0, 0.0, 1.0] });
    assert!(fit_mic_circle2d(&view, &plane, &mic(), Some(SEED)).is_ok());
    let s = &cases[5];
    let sxyz = s.xyz();
    let sview = PointsView::new(&sxyz, None).unwrap();
    let mut splane = params();
    splane.constraints.push(Constraint::ProjectionPlane { p: [0.0; 3], n: [0.0, 0.0, 1.0] });
    assert_eq!(fit_mic_sphere(&sview, &splane, &mic(), None), Err(FitError::InvalidParams { field: "constraints" }));
    // Too few points, collinear points (the least-squares start's errors pass through).
    let two = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let v2 = PointsView::new(&two, None).unwrap();
    assert!(matches!(fit_mic_circle2d(&v2, &params(), &mic(), None), Err(FitError::TooFewPoints { .. })));
    let line: Vec<f64> = (0..6).flat_map(|i| [0.25 * i as f64, 0.5 * i as f64, 0.0]).collect();
    let vl = PointsView::new(&line, None).unwrap();
    assert!(matches!(fit_mic_circle2d(&vl, &params(), &mic(), None), Err(FitError::Collinear { .. })));
    // The trust polygon is defined for circles only.
    let sph = TrustRegion::from_feature(&FittedFeature::Sphere { c: [0.0; 3], r: 1.0 }, 0.2).unwrap();
    assert_eq!(trust_polygon(&sph), Err(FitError::InvalidParams { field: "primitive" }));
}
