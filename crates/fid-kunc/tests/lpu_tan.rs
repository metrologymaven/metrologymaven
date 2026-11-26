//! Acceptance criteria of spec node `UNC.PROP.LPU.TAN` (AC-1..AC-5, the registered-sample-point
//! part of AC-H1, REQ-UNC.PROP.LPU.TAN-001..005).
//!
//! Fixtures and every expected value (the affine matrix bits, the AC-2 error, the coverage
//! report bytes) come from the independent stdlib Python reference
//! `fixtures/unc/lpu/tan/make_fixtures.py`, never from this crate. The test providers below
//! are hand-written analytic derivatives of hand-written test operators.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use fid_kci::kci::json::to_canonical;
use fid_kci::kci::sha256_digest;
use fid_kci::{AlgorithmId, KernelManifest};
use fid_kunc::dist::{Params, UnitCode};
use fid_kunc::model::{OperatorNode, UncertaintyModel};
use fid_kunc::prop::lpu::tan::{
    coverage_report, Kink, OperatorTangent, ProviderError, SamplePoint, ScalarView, TanError, TangentOut, TangentTable,
    COVERAGE_VERSION, MAX_KINKS,
};
use fid_kunc::tangent_harness::{check_provider, check_provider_at, HarnessError, REL_TOL};
use fid_num::la::Mat;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/lpu/tan/");

fn read(name: &str) -> Vec<u8> {
    std::fs::read(format!("{FIX}{name}")).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn expected() -> Vec<Vec<String>> {
    String::from_utf8(read("expected.txt"))
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| l.split_whitespace().map(str::to_owned).collect())
        .collect()
}

fn id(s: &str) -> AlgorithmId {
    AlgorithmId::parse(s).unwrap()
}

fn m(v: f64) -> ScalarView {
    ScalarView { value: v, unit: UnitCode::Metre }
}

fn model(name: &str) -> UncertaintyModel {
    UncertaintyModel::parse(&read(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn the_operator(model: &UncertaintyModel) -> OperatorNode {
    assert_eq!(model.operators().len(), 1);
    model.operators()[0].clone()
}

/// The model's input means, in the operator's port order (x0, x1, x2).
fn model_inputs(model: &UncertaintyModel) -> Vec<ScalarView> {
    let means: Vec<f64> = model
        .inputs()
        .iter()
        .map(|n| match n.distribution.params() {
            Params::Normal { mu, .. } => mu,
            other => panic!("{}: {other:?}", n.id),
        })
        .collect();
    assert_eq!(model.inputs().iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), ["x0", "x1", "x2"]);
    means.into_iter().map(m).collect()
}

// ------------------------------------------------------------------ test operators

/// Every `"<16 hex digits>"` string of canonical parameter bytes, in byte order. For the
/// affine parameters `{"a":[[..],[..],[..]],"b":[..]}` that is A row-major, then b.
fn hex_values(params: &[u8]) -> Result<Vec<f64>, ProviderError> {
    let text = std::str::from_utf8(params).map_err(|_| ProviderError::new("bad_params", "utf-8"))?;
    let mut out = Vec::new();
    for (k, part) in text.split('"').enumerate() {
        if k % 2 == 1 && part.len() == 16 && part.bytes().all(|b| b.is_ascii_hexdigit()) {
            out.push(f64::from_bits(u64::from_str_radix(part, 16).unwrap()));
        }
    }
    if out.len() != 12 {
        return Err(ProviderError::new("bad_params", format!("{} numbers, expected 12", out.len())));
    }
    Ok(out)
}

fn affine_eval(params: &[u8], x: &[f64]) -> Result<Vec<f64>, String> {
    let v = hex_values(params).map_err(|e| e.detail)?;
    if x.len() != 3 {
        return Err("arity".into());
    }
    Ok((0..3).map(|i| v[3 * i] * x[0] + v[3 * i + 1] * x[1] + v[3 * i + 2] * x[2] + v[9 + i]).collect())
}

/// The analytic tangent of `y = A x + b`: the matrix A itself.
struct Affine {
    id: AlgorithmId,
    sample_params: Vec<u8>,
}

impl OperatorTangent for Affine {
    fn alg_id(&self) -> &AlgorithmId {
        &self.id
    }
    fn jacobian(&self, params: &[u8], inputs: &[ScalarView]) -> Result<TangentOut, ProviderError> {
        if inputs.len() != 3 {
            return Err(ProviderError::new("arity", "3 inputs"));
        }
        let v = hex_values(params)?;
        let jac = Mat::new(3, 3, v[..9].to_vec()).map_err(|e| ProviderError::new("mat", e.to_string()))?;
        Ok(TangentOut { jac, kinks: vec![] })
    }
    fn sample_points(&self) -> Vec<SamplePoint> {
        [[0.012, -0.034, 0.056], [1.5, -2.25, 3.0], [-40.0, 0.125, 7.5]]
            .iter()
            .map(|p| SamplePoint { params: self.sample_params.clone(), inputs: p.iter().copied().map(m).collect() })
            .collect()
    }
}

/// `test.poly.map@1.0.0`: f0 = x0 x1 + 2 x2, f1 = x1^2 x2 - x0, f2 = x0^3 / 4 + x1.
fn poly_eval(_params: &[u8], x: &[f64]) -> Result<Vec<f64>, String> {
    if x.len() != 3 {
        return Err("arity".into());
    }
    let (a, b, c) = (x[0], x[1], x[2]);
    Ok(vec![a * b + 2.0 * c, b * b * c - a, 0.25 * a * a * a + b])
}

struct Poly {
    id: AlgorithmId,
}

impl OperatorTangent for Poly {
    fn alg_id(&self) -> &AlgorithmId {
        &self.id
    }
    fn jacobian(&self, _params: &[u8], inputs: &[ScalarView]) -> Result<TangentOut, ProviderError> {
        let [a, b, c] = match inputs {
            [a, b, c] => [a.value, b.value, c.value],
            _ => return Err(ProviderError::new("arity", "3 inputs")),
        };
        let rows = [[b, a, 2.0], [-1.0, 2.0 * b * c, b * b], [0.75 * a * a, 1.0, 0.0]];
        Ok(TangentOut { jac: Mat::from_rows(&rows).unwrap(), kinks: vec![] })
    }
    fn sample_points(&self) -> Vec<SamplePoint> {
        [[1.5, -0.75, 2.25], [0.3, 1.2, -0.8], [-2.0, 0.5, 1.0]]
            .iter()
            .map(|p| SamplePoint { params: b"{}".to_vec(), inputs: p.iter().copied().map(m).collect() })
            .collect()
    }
}

fn poly() -> Poly {
    Poly { id: id("test.poly.map@1.0.0") }
}

fn affine(version: &str) -> Affine {
    let op = the_operator(&model("affine_model.json"));
    Affine { id: id(&format!("test.affine.map@{version}")), sample_params: to_canonical(&op.params).unwrap() }
}

/// A provider that returns a fixed outcome, for error paths and coverage.
struct Fixed {
    id: AlgorithmId,
    out: Result<TangentOut, ProviderError>,
}

impl OperatorTangent for Fixed {
    fn alg_id(&self) -> &AlgorithmId {
        &self.id
    }
    fn jacobian(&self, _params: &[u8], _inputs: &[ScalarView]) -> Result<TangentOut, ProviderError> {
        self.out.clone()
    }
    fn sample_points(&self) -> Vec<SamplePoint> {
        vec![SamplePoint { params: b"{}".to_vec(), inputs: vec![m(1.0)] }]
    }
}

fn fixed(alg: &str, out: Result<TangentOut, ProviderError>) -> Box<Fixed> {
    Box::new(Fixed { id: id(alg), out })
}

fn mat(rows: usize, cols: usize) -> Mat {
    Mat::new(rows, cols, (0..rows * cols).map(|k| k as f64 + 1.0).collect()).unwrap()
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_affine_jacobian_equals_the_matrix_bit_for_bit() {
    let mdl = model("affine_model.json");
    let op = the_operator(&mdl);
    assert_eq!(op.algorithm().unwrap(), id("test.affine.map@1.0.0"));
    let mut table = TangentTable::new();
    table.register(Box::new(affine("1.0.0"))).unwrap();
    let out = table.operator_jacobian(&op, &model_inputs(&mdl)).unwrap();
    assert_eq!((out.jac.rows(), out.jac.cols()), (3, 3));
    assert!(out.kinks.is_empty());
    let cells: Vec<Vec<String>> = expected().into_iter().filter(|l| l[0] == "jacobian").collect();
    assert_eq!(cells.len(), 9);
    for l in cells {
        let (i, j): (usize, usize) = (l[1].parse().unwrap(), l[2].parse().unwrap());
        let want = u64::from_str_radix(&l[3], 16).unwrap();
        assert_eq!(out.jac.get(i, j).unwrap().to_bits(), want, "entry ({i}, {j})");
    }
}

#[test]
fn ac1_params_are_the_rfc8785_bytes_the_model_digests() {
    // The provider receives exactly the bytes `params_digest` is taken over.
    struct Spy {
        id: AlgorithmId,
        seen: std::cell::RefCell<Vec<u8>>,
    }
    impl OperatorTangent for Spy {
        fn alg_id(&self) -> &AlgorithmId {
            &self.id
        }
        fn jacobian(&self, params: &[u8], _inputs: &[ScalarView]) -> Result<TangentOut, ProviderError> {
            *self.seen.borrow_mut() = params.to_vec();
            Ok(TangentOut { jac: mat(3, 3), kinks: vec![] })
        }
        fn sample_points(&self) -> Vec<SamplePoint> {
            vec![]
        }
    }
    let mdl = model("affine_model.json");
    let op = the_operator(&mdl);
    let spy = std::rc::Rc::new(Spy { id: id("test.affine.map@1.0.0"), seen: Default::default() });
    struct Shared(std::rc::Rc<Spy>);
    impl OperatorTangent for Shared {
        fn alg_id(&self) -> &AlgorithmId {
            self.0.alg_id()
        }
        fn jacobian(&self, p: &[u8], i: &[ScalarView]) -> Result<TangentOut, ProviderError> {
            self.0.jacobian(p, i)
        }
        fn sample_points(&self) -> Vec<SamplePoint> {
            vec![]
        }
    }
    let mut table = TangentTable::new();
    table.register(Box::new(Shared(spy.clone()))).unwrap();
    table.operator_jacobian(&op, &model_inputs(&mdl)).unwrap();
    let seen = spy.seen.borrow().clone();
    assert_eq!(sha256_digest(&seen), op.params_digest().unwrap());
}

// ------------------------------------------------------------------ AC-2 (REQ-001)

#[test]
fn ac2_other_semver_is_no_derivative() {
    let want: Vec<Vec<String>> = expected().into_iter().filter(|l| l[0] == "no_derivative").collect();
    assert_eq!(want.len(), 1);
    let mdl = model("affine_model_v110.json");
    let op = the_operator(&mdl);
    let mut table = TangentTable::new();
    table.register(Box::new(affine("1.0.0"))).unwrap();
    let err = table.operator_jacobian(&op, &model_inputs(&mdl)).unwrap_err();
    assert_eq!(err, TanError::NoDerivative { operator: want[0][1].clone() });
    assert_eq!(err.code(), "no_derivative");
    assert_eq!(err.operator(), "test.affine.map@1.1.0");
    assert_eq!(err.to_string(), "no_derivative: test.affine.map@1.1.0");
}

#[test]
fn ac2_lookup_is_exact_in_every_version_part() {
    let mut table = TangentTable::new();
    table.register(Box::new(affine("1.0.0"))).unwrap();
    assert!(table.lookup(&id("test.affine.map@1.0.0")).is_ok());
    for near in ["test.affine.map@1.0.1", "test.affine.map@0.1.0", "test.affine.map@2.0.0", "test.affine.mapx@1.0.0"] {
        let err = table.lookup(&id(near)).err().unwrap();
        assert_eq!(err, TanError::NoDerivative { operator: near.to_owned() }, "{near}");
    }
    // No provider is ever consulted for an unregistered operator, so the input checks after
    // the lookup do not hide `no_derivative`.
    let err = table.jacobian(&id("test.affine.map@1.1.0"), b"{}", &[], 0).unwrap_err();
    assert_eq!(err.code(), "no_derivative");
}

// ------------------------------------------------------------------ AC-3 (REQ-002)

#[test]
fn ac3_two_by_three_for_a_three_by_three_operator_is_a_shape_mismatch() {
    let mdl = model("affine_model.json");
    let op = the_operator(&mdl);
    let mut table = TangentTable::new();
    table.register(fixed("test.affine.map@1.0.0", Ok(TangentOut { jac: mat(2, 3), kinks: vec![] }))).unwrap();
    let err = table.operator_jacobian(&op, &model_inputs(&mdl)).unwrap_err();
    assert_eq!(
        err,
        TanError::TangentShapeMismatch { operator: "test.affine.map@1.0.0".into(), expected: (3, 3), actual: (2, 3) }
    );
    assert_eq!(err.code(), "tangent_shape_mismatch");
    // Transposed shape and the right element count in the wrong shape fail too.
    for (r, c) in [(3, 2), (1, 9), (9, 1), (3, 4)] {
        let mut t = TangentTable::new();
        t.register(fixed("test.affine.map@1.0.0", Ok(TangentOut { jac: mat(r, c), kinks: vec![] }))).unwrap();
        let err = t.operator_jacobian(&op, &model_inputs(&mdl)).unwrap_err();
        assert_eq!(err.code(), "tangent_shape_mismatch", "{r}x{c}");
    }
}

// ------------------------------------------------------------------ AC-4 (REQ-004)

#[test]
fn ac4_coverage_lists_missing_and_orphan_in_id_order() {
    let bytes = read("manifest_min.json");
    let manifest = KernelManifest::parse_canonical(&bytes).unwrap();
    assert_eq!(manifest.core().algorithms.len(), 5);
    let mut table = TangentTable::new();
    table.register(Box::new(affine("1.0.0"))).unwrap();
    table.register(Box::new(poly())).unwrap();
    table.register(fixed("fit.line.ls@1.0.0", Err(ProviderError::new("stub", "")))).unwrap();
    table.register(fixed("test.affine.map@1.1.0", Err(ProviderError::new("stub", "")))).unwrap();
    let report = coverage_report(&manifest, &table);
    let exp = expected();
    let list = |tag: &str| exp.iter().filter(|l| l[0] == tag).map(|l| l[1].clone()).collect::<Vec<_>>();
    assert_eq!(report.missing.iter().map(|i| i.to_string()).collect::<Vec<_>>(), list("missing"));
    assert_eq!(report.orphan.iter().map(|i| i.to_string()).collect::<Vec<_>>(), list("orphan"));
    assert_eq!(report.missing.len(), 2);
    assert_eq!(report.orphan.len(), 1);
    assert!(!report.is_complete());
    assert_eq!(report.manifest, sha256_digest(&bytes));
    assert_eq!(report.canonical_json().unwrap(), read("coverage_expected.json"));
    assert_eq!(report.digest().unwrap(), sha256_digest(&read("coverage_expected.json")));
    assert!(String::from_utf8(report.canonical_json().unwrap()).unwrap().contains(COVERAGE_VERSION));
}

#[test]
fn ac4_registration_order_does_not_change_the_report() {
    let manifest = KernelManifest::parse_canonical(&read("manifest_min.json")).unwrap();
    let mut table = TangentTable::new();
    table.register(fixed("test.affine.map@1.1.0", Err(ProviderError::new("stub", "")))).unwrap();
    table.register(fixed("fit.line.ls@1.0.0", Err(ProviderError::new("stub", "")))).unwrap();
    table.register(Box::new(poly())).unwrap();
    table.register(Box::new(affine("1.0.0"))).unwrap();
    assert_eq!(coverage_report(&manifest, &table).canonical_json().unwrap(), read("coverage_expected.json"));
    // An empty table lists every operator as missing, in manifest order; nothing is orphan.
    let empty = coverage_report(&manifest, &TangentTable::new());
    let listed: Vec<String> = manifest.core().algorithms.iter().map(|a| a.id.to_string()).collect();
    assert_eq!(empty.missing.iter().map(|i| i.to_string()).collect::<Vec<_>>(), listed);
    assert!(empty.orphan.is_empty());
}

// ------------------------------------------------------------------ AC-5 (REQ-005)

/// Seeded single-entry faults for the mutation tests.
#[derive(Debug, Clone, Copy)]
enum Fault {
    SignFlip(usize, usize),
    Double(usize, usize),
    Transpose(usize, usize),
    Drop(usize, usize),
}

struct Mutant<'a> {
    inner: &'a dyn OperatorTangent,
    fault: Fault,
}

impl OperatorTangent for Mutant<'_> {
    fn alg_id(&self) -> &AlgorithmId {
        self.inner.alg_id()
    }
    fn jacobian(&self, params: &[u8], inputs: &[ScalarView]) -> Result<TangentOut, ProviderError> {
        let out = self.inner.jacobian(params, inputs)?;
        let (r, c) = (out.jac.rows(), out.jac.cols());
        let mut d = out.jac.into_data();
        match self.fault {
            Fault::SignFlip(i, j) => d[i * c + j] = -d[i * c + j],
            Fault::Double(i, j) => d[i * c + j] *= 2.0,
            Fault::Drop(i, j) => d[i * c + j] = 0.0,
            Fault::Transpose(i, j) => d.swap(i * c + j, j * c + i),
        }
        Ok(TangentOut { jac: Mat::new(r, c, d).unwrap(), kinks: out.kinks })
    }
    fn sample_points(&self) -> Vec<SamplePoint> {
        self.inner.sample_points()
    }
}

#[test]
fn ac5_harness_rejects_a_wrong_sign_naming_the_entry() {
    let p = poly();
    let report = check_provider(&p, &poly_eval).unwrap();
    assert_eq!((report.points, report.entries), (3, 27));
    let bad = Mutant { inner: &p, fault: Fault::SignFlip(1, 2) };
    match check_provider(&bad, &poly_eval) {
        Err(HarnessError::Entry { point: 0, row: 1, col: 2, provider, oracle }) => {
            // b^2 at (1.5, -0.75, 2.25) is 0.5625; the mutant says -0.5625.
            assert_eq!(provider, -0.5625);
            assert!((oracle - 0.5625).abs() < 1e-9, "{oracle}");
        }
        other => panic!("expected the (1, 2) entry, got {other:?}"),
    }
    let a = affine("1.0.0");
    assert_eq!(check_provider(&a, &affine_eval).unwrap().entries, 27);
    let bad = Mutant { inner: &a, fault: Fault::SignFlip(2, 0) };
    assert!(matches!(check_provider(&bad, &affine_eval), Err(HarnessError::Entry { row: 2, col: 0, .. })));
}

#[test]
fn ac5_harness_contract_errors() {
    let p = poly();
    assert_eq!(check_provider_at(&p, &[], &poly_eval).unwrap_err(), HarnessError::NoSamplePoints);
    let nan = SamplePoint { params: b"{}".to_vec(), inputs: vec![m(f64::NAN), m(1.0), m(1.0)] };
    assert_eq!(check_provider_at(&p, &[nan], &poly_eval).unwrap_err(), HarnessError::NonFinite { point: 0 });
    let two = |_: &[u8], _: &[f64]| -> Result<Vec<f64>, String> { Ok(vec![1.0, 2.0]) };
    assert!(matches!(
        check_provider(&p, &two).unwrap_err(),
        HarnessError::Shape { point: 0, expected: (2, 3), actual: (3, 3) }
    ));
    let failing = |_: &[u8], _: &[f64]| -> Result<Vec<f64>, String> { Err("boom".into()) };
    assert!(matches!(check_provider(&p, &failing).unwrap_err(), HarnessError::Eval { point: 0, .. }));
    let broken = fixed("test.poly.map@1.0.0", Err(ProviderError::new("stub", "no")));
    let got = check_provider_at(broken.as_ref(), &p.sample_points(), &poly_eval).unwrap_err();
    assert!(matches!(got, HarnessError::Provider { point: 0, .. }), "{got:?}");
}

// ------------------------------------------------------------------ AC-H1 (registered sample points)

/// Every seeded single-entry fault that changes an entry at a registered sample point.
fn faults(jac: &Mat) -> Vec<Fault> {
    let (r, c) = (jac.rows(), jac.cols());
    let g = |i, j| jac.get(i, j).unwrap();
    let mut out = Vec::new();
    for i in 0..r {
        for j in 0..c {
            if g(i, j) != 0.0 {
                out.extend([Fault::SignFlip(i, j), Fault::Double(i, j), Fault::Drop(i, j)]);
            }
            if i < j && j < r && i < c && g(i, j) != g(j, i) {
                out.push(Fault::Transpose(i, j));
            }
        }
    }
    out
}

#[test]
fn ach1_every_seeded_fault_is_rejected_and_every_correct_provider_accepted() {
    // The registered-sample-point part of AC-H1. The frozen rdl-release LSNULL points are
    // not available yet (UNC.RDL.GEN.LSNULL unbuilt), see the leaf's Open questions.
    type EvalFn = fn(&[u8], &[f64]) -> Result<Vec<f64>, String>;
    let a = affine("1.0.0");
    let p = poly();
    let cases: [(&dyn OperatorTangent, EvalFn); 2] = [(&a, affine_eval), (&p, poly_eval)];
    let mut killed = 0;
    for (provider, eval) in cases {
        check_provider(provider, &eval).unwrap();
        for (k, sp) in provider.sample_points().into_iter().enumerate() {
            let jac = provider.jacobian(&sp.params, &sp.inputs).unwrap().jac;
            for fault in faults(&jac) {
                let mutant = Mutant { inner: provider, fault };
                match check_provider_at(&mutant, std::slice::from_ref(&sp), &eval) {
                    Err(HarnessError::Entry { .. }) => killed += 1,
                    other => panic!("{} point {k}: {fault:?} survived: {other:?}", provider.alg_id()),
                }
            }
        }
    }
    // Affine: 8 non-zero entries x 3 + 3 transpositions, at 3 points; poly: per point 8
    // non-zero entries x 3 + 3 transpositions. Counted, not computed, so a fault list that
    // silently shrinks fails here.
    assert_eq!(killed, 3 * (8 * 3 + 3) + 3 * (8 * 3 + 3));
    assert_eq!(REL_TOL, 1e-6);
}

// ------------------------------------------------------------------ table contract

#[test]
fn duplicate_provider_is_refused_and_the_first_stays() {
    let mut table = TangentTable::new();
    table.register(Box::new(poly())).unwrap();
    let err = table.register(fixed("test.poly.map@1.0.0", Err(ProviderError::new("stub", "")))).unwrap_err();
    assert_eq!(err, TanError::DuplicateProvider { operator: "test.poly.map@1.0.0".into() });
    assert_eq!(table.len(), 1);
    let out = table.jacobian(&id("test.poly.map@1.0.0"), b"{}", &[m(1.0), m(2.0), m(3.0)], 3).unwrap();
    assert_eq!(out.jac.get(0, 0).unwrap(), 2.0);
    assert_eq!(table.ids(), vec![id("test.poly.map@1.0.0")]);
}

#[test]
fn inputs_are_checked_before_the_provider_runs() {
    let mdl = model("affine_model.json");
    let op = the_operator(&mdl);
    let mut table = TangentTable::new();
    table.register(Box::new(affine("1.0.0"))).unwrap();
    let alg = "test.affine.map@1.0.0".to_string();
    let err = table.operator_jacobian(&op, &[m(1.0), m(2.0)]).unwrap_err();
    assert_eq!(err, TanError::BadInput { operator: alg.clone(), reason: "count", index: 2 });
    let rad = ScalarView { value: 1.0, unit: UnitCode::Radian };
    let err = table.operator_jacobian(&op, &[m(1.0), rad, m(2.0)]).unwrap_err();
    assert_eq!(err, TanError::BadInput { operator: alg.clone(), reason: "unit", index: 1 });
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let err = table.operator_jacobian(&op, &[m(1.0), m(2.0), m(bad)]).unwrap_err();
        assert_eq!(err, TanError::BadInput { operator: alg.clone(), reason: "non_finite", index: 2 });
        assert_eq!(err.code(), "tangent_bad_input");
    }
    let mut no_out = op.clone();
    no_out.ports_out.clear();
    let err = table.operator_jacobian(&no_out, &model_inputs(&mdl)).unwrap_err();
    assert_eq!(err, TanError::BadCall { operator: alg.clone(), reason: "ports_out" });
    let mut no_in = op.clone();
    no_in.ports_in.clear();
    assert_eq!(table.operator_jacobian(&no_in, &[]).unwrap_err().code(), "tangent_bad_call");
    let mut huge = op.clone();
    huge.ports_in[0].arity = u32::MAX;
    let err = table.operator_jacobian(&huge, &model_inputs(&mdl)).unwrap_err();
    assert_eq!(err, TanError::BadCall { operator: alg.clone(), reason: "too_large" });
    let mut bad_id = op.clone();
    bad_id.algorithm_version = "1.0".into();
    let err = table.operator_jacobian(&bad_id, &model_inputs(&mdl)).unwrap_err();
    assert_eq!(err, TanError::BadCall { operator: "test.affine.map@1.0".into(), reason: "algorithm_id" });
    let err = table.jacobian(&id(&alg), b"{}", &[m(1.0)], 0).unwrap_err();
    assert_eq!(err, TanError::BadCall { operator: alg, reason: "ports_out" });
}

#[test]
fn provider_errors_pass_through_typed() {
    let mut table = TangentTable::new();
    let e = ProviderError::new("singular_point", "derivative undefined at the apex");
    table.register(fixed("test.poly.map@1.0.0", Err(e.clone()))).unwrap();
    let err = table.jacobian(&id("test.poly.map@1.0.0"), b"{}", &[m(1.0)], 1).unwrap_err();
    assert_eq!(err, TanError::Provider { operator: "test.poly.map@1.0.0".into(), error: e });
    assert_eq!(err.code(), "tangent_provider_error");
    assert_eq!(
        err.to_string(),
        "tangent_provider_error: test.poly.map@1.0.0: singular_point: derivative undefined at the apex"
    );
}

#[test]
fn kink_hints_are_checked() {
    let k = |input_component, margin| Kink { input_component, margin };
    let ok = vec![k(0, 0.0), k(1, 1e-3)];
    let cases: Vec<(Vec<Kink>, Option<usize>)> = vec![
        (ok, None),
        (vec![k(0, 0.5), k(2, 0.5)], Some(1)),
        (vec![k(0, -1e-9)], Some(0)),
        (vec![k(1, f64::NAN)], Some(0)),
        (vec![k(1, f64::INFINITY)], Some(0)),
        (vec![k(0, 1.0); MAX_KINKS + 1], Some(MAX_KINKS)),
    ];
    for (kinks, bad) in cases {
        let mut table = TangentTable::new();
        table.register(fixed("test.poly.map@1.0.0", Ok(TangentOut { jac: mat(1, 2), kinks: kinks.clone() }))).unwrap();
        let got = table.jacobian(&id("test.poly.map@1.0.0"), b"{}", &[m(1.0), m(2.0)], 1);
        match bad {
            None => assert_eq!(got.unwrap().kinks, kinks),
            Some(index) => {
                assert_eq!(got.unwrap_err(), TanError::BadKink { operator: "test.poly.map@1.0.0".into(), index })
            }
        }
    }
}

// ------------------------------------------------------------------ REQ-003 lint

/// Identifiers and fragments that mean "evaluate an operator" or "perturb an input". LPU
/// modules get derivatives from providers only; the kernel call interface, evaluation
/// callbacks and step sizes have no business there.
const FORBIDDEN_IDENTS: &[&str] =
    &["evaluate", "eval", "Eval", "KernelAlgorithm", "KernelCall", "AlgorithmTable", "tangent_harness", "STEP"];
const FORBIDDEN_IDENT_PARTS: &[&str] = &["perturb", "finite_diff", "central_diff", "forward_diff", "backward_diff"];
const FORBIDDEN_FRAGMENTS: &[&str] = &[".run(", "Fn(", "FnMut(", "FnOnce(", "fn(", "fn ("];

/// The code of a Rust file with `//` comments removed (doc comments describe the rule).
fn code_only(text: &str) -> String {
    text.lines().map(|l| l.find("//").map_or(l, |at| &l[..at])).collect::<Vec<_>>().join("\n")
}

/// Every forbidden use in `text` (file name for messages).
fn lint(name: &str, text: &str) -> Vec<String> {
    let code = code_only(text);
    let mut found = Vec::new();
    for ident in code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        let lower = ident.to_ascii_lowercase();
        if FORBIDDEN_IDENTS.contains(&ident) || FORBIDDEN_IDENT_PARTS.iter().any(|p| lower.contains(p)) {
            found.push(format!("{name}: identifier {ident}"));
        }
    }
    for frag in FORBIDDEN_FRAGMENTS {
        if code.contains(frag) {
            found.push(format!("{name}: {frag}"));
        }
    }
    found
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let p = e.unwrap().path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn req003_lpu_modules_have_no_finite_difference_path() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/prop/lpu");
    let mut files = Vec::new();
    rust_files(&dir, &mut files);
    files.sort();
    assert!(files.iter().any(|f| f.ends_with("tan.rs")), "{files:?}");
    let mut found = Vec::new();
    for f in &files {
        found.extend(lint(&f.display().to_string(), &std::fs::read_to_string(f).unwrap()));
    }
    assert!(found.is_empty(), "finite-difference or evaluation path in LPU modules: {found:?}");
}

#[test]
fn req003_lint_catches_a_finite_difference_path() {
    // The harness itself differentiates numerically: the lint must flag it.
    let harness =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tangent_harness.rs")).unwrap();
    assert!(!lint("tangent_harness.rs", &harness).is_empty());
    let samples = [
        "fn sens(t: &AlgorithmTable, c: &KernelCall) { let r = fid_kci::evaluate(t, c); }",
        "fn d(f: &dyn Fn(&[f64]) -> f64, x: &mut [f64]) { x[0] += 1e-6; }",
        "fn d(a: &dyn KernelAlgorithm, p: &[u8], x: &[&[u8]]) { let _ = a.run(p, None, x); }",
        "let perturbed = x + h;",
        "fn central_difference() {}",
    ];
    for s in samples {
        assert!(!lint("sample", s).is_empty(), "{s}");
    }
    // Comments are not code.
    assert!(lint("sample", "// never evaluate an operator at a perturbed input\nlet a = 1;").is_empty());
}
