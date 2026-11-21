//! ALN.QA.COND acceptance criteria AC-1, AC-2, AC-3 and AC-5 (AC-4, the read command, is in
//! `fid-align`'s `tests/diagnostics.rs`). The inputs are rebuilt here from the seeded rules of
//! `fixtures/aln/qa/cond-cases/make_cond_cases.py` and checked against its `inputs` digests;
//! every expected value comes from that independent Python reference (which runs the reviewed
//! bit-exact QR and SVD models of `fixtures/num/la/`), never from running this crate. Values
//! are compared bit for bit, which is AC-5 on every CI target (Linux, Windows, macOS, wasm32).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use fid_align_solve::{
    diagnose, diagnose_reduced, reduced_jacobian, validate, AlignmentDiagnostics, CondError, CondWarning, Dof,
    DofLocks, FrameContext, LockFrameRef, MotionKind, PivotKind, Threshold, ThresholdBasis, WeightSpec,
    DEFAULT_ILL_THRESHOLD,
};
use fid_geo_frame::{FrameId, FrameTree, Rigid};
use fid_kci::kci::{to_hex, Sha256};

// ------------------------------------------------------------------ expected values

fn expected() -> BTreeMap<(String, String), Vec<String>> {
    let path = format!("{}/fixtures/aln/qa/cond-cases/expected.txt", fid_testkit::WORKSPACE_ROOT);
    let text = std::fs::read_to_string(path).unwrap();
    let mut out = BTreeMap::new();
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let mut it = line.split(' ');
        let case = it.next().unwrap().to_string();
        let key = it.next().unwrap().to_string();
        assert!(out.insert((case, key), it.map(str::to_string).collect()).is_none(), "duplicate line {line}");
    }
    out
}

fn hex(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s.strip_prefix("0x").unwrap(), 16).unwrap())
}

fn bits(x: f64) -> String {
    format!("0x{:016x}", x.to_bits())
}

// ------------------------------------------------------------------ inputs (as the reference)

struct Stream(u64);

impl Stream {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn uni(&mut self) -> f64 {
        (self.next() >> 11) as f64 * f64::from_bits(0x3CA0_0000_0000_0000)
    }

    fn sym(&mut self) -> f64 {
        2.0 * self.uni() - 1.0
    }

    fn unit2(&mut self) -> (f64, f64) {
        loop {
            let (u, v) = (self.sym(), self.sym());
            let s2 = u * u + v * v;
            if s2 > 1.0 || s2 < 1.0 / 1024.0 {
                continue;
            }
            let s = sqrt(s2);
            return (u / s, v / s);
        }
    }

    fn unit3(&mut self) -> [f64; 3] {
        loop {
            let (u, v, w) = (self.sym(), self.sym(), self.sym());
            let s2 = (u * u + v * v) + w * w;
            if s2 > 1.0 || s2 < 1.0 / 1024.0 {
                continue;
            }
            let s = sqrt(s2);
            return [u / s, v / s, w / s];
        }
    }
}

fn sqrt(x: f64) -> f64 {
    fid_num::f::sqrt(x).unwrap()
}

type Sample = ([f64; 3], [f64; 3]);

fn cylinder(st: &mut Stream, half: bool) -> Sample {
    let r = 0.0625;
    let (nx, mut ny) = st.unit2();
    if half {
        ny = ny.abs();
    }
    let z = 0.1 * st.sym();
    ([r * nx, r * ny, z], [nx, ny, 0.0])
}

fn long_plane(st: &mut Stream) -> Sample {
    let x = 0.5 * st.sym();
    let y = 0.0005 * st.sym();
    let a = 1e-10 * st.sym();
    let b = 1e-9 * st.sym();
    let s = sqrt((a * a + b * b) + 1.0);
    ([x, y, 0.0], [a / s, b / s, 1.0 / s])
}

fn sphere(st: &mut Stream) -> Sample {
    let r = 0.125;
    let n = st.unit3();
    ([r * n[0], r * n[1], r * n[2]], n)
}

fn freeform(st: &mut Stream) -> Sample {
    let x = 0.1 * st.sym();
    let y = 0.1 * st.sym();
    let z = ((2.0 * x * x - 3.0 * y * y) + 5.0 * x * y) + 8.0 * x * x * x;
    let fx = (4.0 * x + 5.0 * y) + 24.0 * x * x;
    let fy = 5.0 * x - 6.0 * y;
    let s = sqrt((fx * fx + fy * fy) + 1.0);
    ([x, y, z], [-fx / s, -fy / s, 1.0 / s])
}

fn full_row(p: [f64; 3], n: [f64; 3]) -> [f64; 6] {
    let [x, y, z] = p;
    let [nx, ny, nz] = n;
    [nx, ny, nz, y * nz - z * ny, z * nx - x * nz, x * ny - y * nx]
}

struct Geometry {
    points: Vec<[f64; 3]>,
    rows: Vec<[f64; 6]>,
}

fn build(count: usize, mut gen: impl FnMut() -> Sample) -> Geometry {
    let (mut points, mut rows) = (Vec::new(), Vec::new());
    for _ in 0..count {
        let (p, n) = gen();
        points.push(p);
        rows.push(full_row(p, n));
    }
    Geometry { points, rows }
}

fn geometries() -> BTreeMap<&'static str, Geometry> {
    let mut st = Stream(14);
    let mut g = BTreeMap::new();
    g.insert("full_cylinder", build(10_000, || cylinder(&mut st, false)));
    g.insert("half_cylinder", build(10_000, || cylinder(&mut st, true)));
    g.insert("long_plane", build(10_000, || long_plane(&mut st)));
    g.insert("sphere", build(10_000, || sphere(&mut st)));
    g.insert("freeform", build(10_000, || freeform(&mut st)));
    let mut small = Stream(15);
    g.insert("freeform_small", build(50, || freeform(&mut small)));
    g
}

fn dof(name: &str) -> Dof {
    *Dof::ALL.iter().find(|d| d.name() == name).unwrap()
}

/// A case: its parameters, threshold, points and kept Jacobian columns (row-major).
struct Case {
    params: Vec<Dof>,
    threshold: Threshold,
    points: Vec<[f64; 3]>,
    jacobian: Vec<f64>,
}

fn case(name: &str, exp: &BTreeMap<(String, String), Vec<String>>, geo: &BTreeMap<&str, Geometry>) -> Case {
    let get = |k: &str| exp.get(&(name.to_string(), k.to_string())).unwrap_or_else(|| panic!("{name} {k}"));
    let params: Vec<Dof> = get("params")[0].split(',').map(dof).collect();
    let t = hex(&get("threshold")[0]);
    let threshold = if t == DEFAULT_ILL_THRESHOLD { Threshold::DEFAULT } else { Threshold::caller(t).unwrap() };
    let (points, jacobian) = if name == "wide" {
        (vec![[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]], vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.5])
    } else {
        let src = match name {
            "long_plane_1e12" => "long_plane",
            "small_translations" | "small_tilt" => "freeform_small",
            other => other,
        };
        let g = &geo[src];
        let jac = g.rows.iter().flat_map(|r| params.iter().map(move |d| r[d.index()])).collect();
        (g.points.clone(), jac)
    };
    // The inputs are the reference's, bit for bit.
    let mut h = Sha256::new();
    for p in &points {
        for c in p {
            h.update(&c.to_le_bytes());
        }
    }
    for x in &jacobian {
        h.update(&x.to_le_bytes());
    }
    let inputs = get("inputs");
    assert_eq!(inputs[0], points.len().to_string(), "{name} points");
    assert_eq!(inputs[1], (jacobian.len() / params.len()).to_string(), "{name} rows");
    assert_eq!(to_hex(&h.finish()), inputs[2], "{name} inputs digest");
    Case { params, threshold, points, jacobian }
}

fn opt(x: Option<f64>) -> String {
    x.map_or("inf".to_string(), bits)
}

fn all(xs: &[f64]) -> Vec<String> {
    xs.iter().map(|x| bits(*x)).collect()
}

/// Every recorded value of `d` against the reference, bit for bit.
fn check(name: &str, d: &AlignmentDiagnostics, exp: &BTreeMap<(String, String), Vec<String>>) {
    let get = |k: &str| exp[&(name.to_string(), k.to_string())].clone();
    assert_eq!(vec![bits(d.length_scale())], get("length_scale"), "{name} length_scale");
    assert_eq!(all(&d.centroid()), get("centroid"), "{name} centroid");
    let mut pivot = vec![d.pivot().as_str().to_string()];
    pivot.extend(all(&d.pivot_point()));
    assert_eq!(pivot, get("pivot"), "{name} pivot");
    assert_eq!(all(d.singular_values()), get("sigma"), "{name} singular values");
    assert_eq!(vec![opt(d.condition_number())], get("cond"), "{name} condition number");
    assert_eq!(vec![d.n_unobservable().to_string()], get("n_unobservable"), "{name} n_unobservable");
    let w = d.weakest_dof();
    let axis = w.kind().axis().map_or("-", |a| a.as_str());
    assert_eq!(vec![w.kind().kind_str().to_string(), axis.to_string()], get("weakest"), "{name} weakest");
    assert_eq!(all(w.vector()), get("vector"), "{name} vector");
    assert_eq!(all(&w.angular()), get("angular"), "{name} angular");
    assert_eq!(all(&w.linear()), get("linear"), "{name} linear");
    let warning = d.warnings().first().map_or("none", |w| w.code());
    assert_eq!(vec![warning.to_string()], get("warning"), "{name} warning");
    assert!(d.warnings().len() <= 1);
    assert!(d.history().is_none());
}

fn run(name: &str) -> AlignmentDiagnostics {
    let exp = expected();
    let geo = geometries();
    let c = case(name, &exp, &geo);
    let d = diagnose(&c.jacobian, &c.params, &c.points, c.threshold).unwrap();
    check(name, &d, &exp);
    d
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_every_reference_case_matches_bit_for_bit() {
    let exp = expected();
    let geo = geometries();
    let names: std::collections::BTreeSet<&String> = exp.keys().map(|(c, _)| c).collect();
    assert_eq!(names.len(), 9);
    for name in names {
        let c = case(name, &exp, &geo);
        let d = diagnose(&c.jacobian, &c.params, &c.points, c.threshold).unwrap();
        check(name, &d, &exp);
    }
}

#[test]
fn ac1_full_and_half_cylinder_name_the_rotation_about_the_axis_as_unobservable() {
    for name in ["full_cylinder", "half_cylinder"] {
        let d = run(name);
        assert_eq!(d.warnings(), &[CondWarning::UnobservableDof { count: 1 }][..], "{name}");
        assert_eq!(d.weakest_dof().kind(), MotionKind::Rotation(fid_align_solve::Axis::Z));
        assert_eq!(d.pivot(), PivotKind::FrameOrigin, "tz is locked");
        assert_eq!(d.weakest_dof().name(), "rotation about z through the frame origin");
        assert_eq!(d.condition_number(), None, "the smallest singular value is exactly 0");
    }
}

#[test]
fn ac1_sphere_reports_three_unobservable_rotations_as_the_weakest_plus_the_count() {
    let d = run("sphere");
    assert_eq!(d.warnings(), &[CondWarning::UnobservableDof { count: 3 }][..]);
    // The weakest motion is a rotation: its linear part is negligible.
    let w = d.weakest_dof();
    assert!(w.linear().iter().all(|x| x.abs() < 1e-2), "{:?}", w.linear());
    assert_eq!(d.pivot(), PivotKind::Centroid);
}

#[test]
fn ac1_freeform_patch_gives_no_warning() {
    let d = run("freeform");
    assert!(d.warnings().is_empty());
    assert!(d.condition_number().unwrap() < 1e3);
}

// ------------------------------------------------------------------ AC-2, AC-3

#[test]
fn ac2_long_thin_plane_is_ill_conditioned_at_the_default_and_names_the_slide() {
    let d = run("long_plane");
    let cond = d.condition_number().unwrap();
    assert!(cond > 1e8 && cond < 1e12, "{cond:e}");
    assert_eq!(
        d.warnings(),
        &[CondWarning::IllConditioned { condition_number: Some(cond), threshold: Threshold::DEFAULT }][..]
    );
    assert_eq!(d.weakest_dof().name(), "translation along x");
    // D-045: the default threshold is a claimed value and says so.
    assert_eq!(d.threshold().basis(), ThresholdBasis::DefaultUnverified);
    assert_eq!(d.threshold().basis().as_str(), "default_unverified");
}

#[test]
fn ac3_a_1e12_threshold_attaches_no_warning() {
    let d = run("long_plane_1e12");
    assert!(d.warnings().is_empty());
    assert_eq!(d.threshold().value(), 1e12);
    assert_eq!(d.threshold().basis(), ThresholdBasis::Caller);
}

// ------------------------------------------------------------------ reduced Jacobian

#[test]
fn diagnose_reduced_equals_diagnose_on_the_kept_columns() {
    let geo = geometries();
    let g = &geo["full_cylinder"];
    let tree = FrameTree::new();
    let ctx = FrameContext { tree: &tree, parent: FrameId::WORLD };
    let locks = DofLocks::NONE.with(Dof::Tz);
    let cs =
        validate(locks, LockFrameRef::Parent, &ctx, &Rigid::IDENTITY, &WeightSpec::default(), g.points.len()).unwrap();
    let j = reduced_jacobian(&cs, &g.rows).unwrap();
    let a = diagnose_reduced(&j, locks, &g.points, Threshold::DEFAULT).unwrap();
    let exp = expected();
    check("full_cylinder", &a, &exp);
    // A Jacobian whose column count disagrees with the locks is refused.
    let err = diagnose_reduced(&j, DofLocks::NONE, &g.points, Threshold::DEFAULT).unwrap_err();
    assert_eq!(err.code(), "bad_shape");
}

// ------------------------------------------------------------------ refusals (hand-labelled)

#[test]
fn refusals_are_typed_and_in_the_pinned_order() {
    let pts = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let t = Threshold::DEFAULT;
    let code = |j: &[f64], p: &[Dof], pts: &[[f64; 3]]| diagnose(j, p, pts, t).unwrap_err().code();
    assert_eq!(code(&[1.0], &[], &pts), "bad_params");
    assert_eq!(code(&[1.0, 0.0], &[Dof::Tx, Dof::Tx], &pts), "bad_params");
    assert_eq!(code(&[1.0, 0.0], &[Dof::Ty, Dof::Tx], &pts), "bad_params");
    assert_eq!(code(&[], &[Dof::Tx], &pts), "empty_jacobian");
    assert_eq!(code(&[1.0, 2.0, 3.0], &[Dof::Tx, Dof::Ty], &pts), "bad_shape");
    assert_eq!(
        diagnose(&[1.0, f64::NAN], &[Dof::Tx], &pts, t).unwrap_err(),
        CondError::NanInput { what: "jacobian", index: 1 }
    );
    assert_eq!(code(&[f64::INFINITY], &[Dof::Tx], &pts), "infinite_input");
    assert_eq!(code(&[1.0], &[Dof::Tx], &[]), "empty_points");
    assert_eq!(
        diagnose(&[1.0], &[Dof::Tx], &[[0.0, 0.0, 0.0], [0.0, f64::NAN, 0.0]], t).unwrap_err(),
        CondError::NanInput { what: "points", index: 4 }
    );
    // A bad parameter list is reported before a bad Jacobian.
    assert_eq!(code(&[f64::NAN], &[], &[]), "bad_params");
    // Rotations with every point at the centroid: no length scale.
    assert_eq!(code(&[1.0], &[Dof::Rx], &[[1.0, 2.0, 3.0]]), "degenerate_scale");
    // Without rotations L == 0 is recorded, not refused.
    let d = diagnose(&[2.0], &[Dof::Tx], &[[1.0, 2.0, 3.0]], t).unwrap();
    assert_eq!(d.length_scale(), 0.0);
    assert_eq!(d.singular_values(), &[2.0][..]);
    assert!(d.warnings().is_empty());
    // Thresholds.
    for bad in [f64::NAN, f64::INFINITY, 0.5, -1.0] {
        assert_eq!(Threshold::caller(bad).unwrap_err(), CondError::BadThreshold);
    }
    assert_eq!(Threshold::caller(1.0).unwrap().value(), 1.0);
    // Overflow while scaling.
    let e = diagnose(&[1.0, f64::MAX], &[Dof::Tx, Dof::Rx], &[[0.0, 0.0, 0.0], [1e-300, 0.0, 0.0]], t).unwrap_err();
    assert_eq!(e, CondError::NonFinite { op: "scale" });
}

#[test]
fn an_all_zero_jacobian_is_entirely_unobservable() {
    let pts = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
    let d = diagnose(&[0.0; 12], &[Dof::Tx, Dof::Ty, Dof::Rz], &pts, Threshold::DEFAULT).unwrap();
    assert_eq!(d.warnings(), &[CondWarning::UnobservableDof { count: 3 }][..]);
    assert_eq!(d.condition_number(), None);
    assert_eq!(d.singular_values(), &[0.0, 0.0, 0.0][..]);
}
