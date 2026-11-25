//! Spec node GDT.LOC.DERIV.TYPES (`fid_gps::loc::derive`): AC-1 (a stub axis registered under
//! the chain's reduction operator, on `fixtures/gdt/loc/deriv_stub_points.json`, returns its
//! element with its algorithm ID), AC-2 (`UNSUPPORTED{alg}`), AC-3
//! (`derivation_failed{too_few_sections}`), AC-4 (`no_reduction_in_chain`) and AC-5 (the
//! serialised examples of all four variants in `fixtures/gdt/loc/deriv_elements.json`, which
//! `fixtures/gdt/loc/check_deriv_schema.py` validates against the exported JSON Schema, are
//! byte for byte what fid-gps serialises). Expected records come from the independent
//! reference `fixtures/gdt/loc/make_deriv_fixtures.py`; error paths are hand-labelled from the
//! leaf's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{to_canonical_bytes, CValue};
use fid_gps::dmt::{AlgId, Operator, OperatorChain, OperatorKind};
use fid_gps::loc::derive::{
    derive, Axis, DerivationReason, DeriveCall, DeriveError, DeriveFlag, DeriveRegistry, DeriveRequest, DerivedElement,
    ElementKind, FeatureKind, ImplError, MedianLine, MedianPoints, MedianSurface, Produced, RegistryError,
    DERIVED_ELEMENT_JSON_SCHEMA, DERIVED_ELEMENT_SCHEMA,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const STUB: &str = "syn.axis.stub@1.0.0";

// ------------------------------------------------------------------ helpers

fn fixture(name: &str) -> CValue {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/loc").join(name);
    fid_canon::parse(&std::fs::read(&path).unwrap()).unwrap()
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        _ => panic!("object"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("array"),
    }
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("string"),
    }
}

fn hexf(v: &CValue) -> f64 {
    let s = st(v);
    assert!(s.len() == 18 && s.starts_with("0x"), "{s}");
    f64::from_bits(u64::from_str_radix(&s[2..], 16).unwrap())
}

fn p3(v: &CValue) -> [f64; 3] {
    let a = arr(v);
    assert_eq!(a.len(), 3);
    [hexf(&a[0]), hexf(&a[1]), hexf(&a[2])]
}

fn list(v: &CValue) -> Vec<[f64; 3]> {
    arr(v).iter().map(p3).collect()
}

fn alg(s: &str) -> AlgId {
    AlgId::parse(s).unwrap()
}

fn chain(steps: &[(OperatorKind, &str)]) -> OperatorChain {
    let ops = steps.iter().map(|(k, a)| Operator::new(*k, alg(a), BTreeMap::new()).unwrap()).collect();
    OperatorChain::new("default", ops).unwrap()
}

fn chain_from(v: &CValue) -> OperatorChain {
    let c = obj(v);
    let steps = arr(&c["steps"])
        .iter()
        .map(|s| {
            let s = obj(s);
            assert!(obj(&s["params"]).is_empty());
            Operator::new(OperatorKind::parse(st(&s["kind"])).unwrap(), alg(st(&s["alg"])), BTreeMap::new()).unwrap()
        })
        .collect();
    OperatorChain::new(st(&c["variant"]), steps).unwrap()
}

/// A chain that associates a cylinder and reduces with `reduction`.
fn reducing(reduction: &str) -> OperatorChain {
    chain(&[
        (OperatorKind::Partition, "syn.partition.whole_feature@1.0.0"),
        (OperatorKind::Association, "syn.association.ls_cylinder@1.0.0"),
        (OperatorKind::Reduction, reduction),
        (OperatorKind::Evaluation, "syn.evaluation.zone@1.0.0"),
    ])
}

const PTS: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [0.001, 0.0, 0.01], [0.0, 0.001, 0.02]];

fn request(points: &[[f64; 3]]) -> DeriveRequest<'_> {
    DeriveRequest { points, feature_kind: FeatureKind::Cylinder, nominal_direction: None }
}

fn canon(v: &CValue) -> String {
    String::from_utf8(to_canonical_bytes(v).unwrap()).unwrap()
}

// ------------------------------------------------------------------ stubs

/// The stub axis of `make_deriv_fixtures.py`: through (x0, y0, (zmin + zmax) / 2) along +z with
/// extent zmax - zmin.
fn stub_axis(call: &DeriveCall<'_>) -> Result<Produced, ImplError> {
    let pts = call.request.points;
    let first = pts.first().ok_or(ImplError::Failed(DerivationReason::TooFewPoints))?;
    let (mut zmin, mut zmax) = (first[2], first[2]);
    for p in pts {
        if p[2].total_cmp(&zmin).is_lt() {
            zmin = p[2];
        }
        if p[2].total_cmp(&zmax).is_gt() {
            zmax = p[2];
        }
    }
    let axis = Axis::new([first[0], first[1], (zmin + zmax) / 2.0], [0.0, 0.0, 1.0], zmax - zmin)
        .map_err(|_| ImplError::Failed(DerivationReason::FitFailed))?;
    Ok(Produced::new(DerivedElement::Axis(axis)))
}

fn stub_flagged(call: &DeriveCall<'_>) -> Result<Produced, ImplError> {
    let mut p = stub_axis(call)?;
    p.flags.insert(DeriveFlag::LocalOptimumOnly);
    Ok(p)
}

fn stub_line(call: &DeriveCall<'_>) -> Result<Produced, ImplError> {
    Ok(Produced::new(DerivedElement::MedianLine(MedianLine::new(call.request.points.to_vec()).unwrap())))
}

fn fail_sections(_: &DeriveCall<'_>) -> Result<Produced, ImplError> {
    Err(ImplError::Failed(DerivationReason::TooFewSections))
}

fn fail_points(_: &DeriveCall<'_>) -> Result<Produced, ImplError> {
    Err(ImplError::Failed(DerivationReason::TooFewPoints))
}

fn fail_fit(_: &DeriveCall<'_>) -> Result<Produced, ImplError> {
    Err(ImplError::Failed(DerivationReason::FitFailed))
}

fn fail_opposite(_: &DeriveCall<'_>) -> Result<Produced, ImplError> {
    Err(ImplError::Failed(DerivationReason::NoOppositePoint))
}

fn refuse_fit(_: &DeriveCall<'_>) -> Result<Produced, ImplError> {
    Err(ImplError::Unsupported { alg: "syn.association.mz_cylinder@1.0.0".to_owned() })
}

fn must_not_run(_: &DeriveCall<'_>) -> Result<Produced, ImplError> {
    panic!("the implementation must not be called")
}

/// Echoes the call: a median line whose points are the reduction step's index in the chain,
/// and +z or the nominal direction as the second point.
fn echo(call: &DeriveCall<'_>) -> Result<Produced, ImplError> {
    let idx = call.chain.steps().iter().position(|s| std::ptr::eq(s, call.reduction)).unwrap();
    let second = call.request.nominal_direction.unwrap_or([0.0, 0.0, 1.0]);
    let line = MedianLine::new(vec![[idx as f64, 0.0, 0.0], second]).unwrap();
    Ok(Produced::new(DerivedElement::MedianLine(line)))
}

thread_local! {
    static NEXT: RefCell<Option<Produced>> = const { RefCell::new(None) };
}

/// Returns the element the test put in [`NEXT`].
fn replay(_: &DeriveCall<'_>) -> Result<Produced, ImplError> {
    Ok(NEXT.with(|n| n.borrow_mut().take()).unwrap())
}

fn registry(entries: &[(&str, fid_gps::loc::derive::DeriveFn)]) -> DeriveRegistry {
    let mut r = DeriveRegistry::new();
    for (a, f) in entries {
        r.register(alg(a), *f).unwrap();
    }
    r
}

// ------------------------------------------------------------------ AC-1: dispatch

#[test]
fn ac1_stub_axis_on_fixture_returns_its_element_with_its_alg() {
    let fx = fixture("deriv_stub_points.json");
    let f = obj(&fx);
    let points = list(&f["points"]);
    let c = chain_from(&f["chain"]);
    let kind = FeatureKind::parse(st(&f["feature_kind"])).unwrap();
    // Other registered implementations must not be picked.
    let reg = registry(&[
        ("syn.axis.other@1.0.0", must_not_run),
        (STUB, stub_axis),
        ("syn.axis.stub@1.0.1", must_not_run),
        ("syn.association.ls_cylinder@1.0.0", must_not_run),
    ]);
    let req = DeriveRequest { points: &points, feature_kind: kind, nominal_direction: None };
    let d = derive(&reg, &req, &c).unwrap();
    assert_eq!(d.alg().as_str(), STUB);
    assert_eq!(d.element().kind(), ElementKind::Axis);
    assert!(d.flags().is_empty());
    assert_eq!(canon(&d.to_cvalue()), st(&f["expected_canonical"]));
    // The method form gives the same.
    assert_eq!(reg.derive(&req, &c).unwrap(), d);
}

#[test]
fn ac1_routes_each_reduction_to_its_own_implementation() {
    let reg = registry(&[(STUB, stub_axis), ("syn.mline.stub@1.0.0", stub_line), ("syn.mline.echo@1.0.0", echo)]);
    let a = derive(&reg, &request(&PTS), &reducing(STUB)).unwrap();
    assert_eq!(a.element().kind(), ElementKind::Axis);
    let l = derive(&reg, &request(&PTS), &reducing("syn.mline.stub@1.0.0")).unwrap();
    assert_eq!(l.alg().as_str(), "syn.mline.stub@1.0.0");
    match l.element() {
        DerivedElement::MedianLine(m) => assert_eq!(m.points(), &PTS),
        other => panic!("{other:?}"),
    }
    // The implementation sees the chain, its reduction step and the nominal direction.
    let mut req = request(&PTS);
    req.nominal_direction = Some([0.0, -1.0, 0.0]);
    let e = derive(&reg, &req, &reducing("syn.mline.echo@1.0.0")).unwrap();
    match e.element() {
        DerivedElement::MedianLine(m) => assert_eq!(m.points(), &[[2.0, 0.0, 0.0], [0.0, -1.0, 0.0]]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn ac1_flags_pass_through() {
    let reg = registry(&[(STUB, stub_flagged)]);
    let d = derive(&reg, &request(&PTS), &reducing(STUB)).unwrap();
    assert_eq!(d.flags().iter().copied().collect::<Vec<_>>(), vec![DeriveFlag::LocalOptimumOnly]);
    let CValue::Obj(m) = d.to_cvalue() else { panic!() };
    assert_eq!(m["flags"], CValue::Arr(vec![CValue::Str("local_optimum_only".to_owned())]));
}

// ------------------------------------------------------------------ AC-2: UNSUPPORTED

#[test]
fn ac2_unregistered_operator_is_unsupported() {
    let reg = registry(&[(STUB, must_not_run)]);
    for a in ["syn.axis.unregistered@1.0.0", "syn.axis.stub@1.0.1", "syn.axis.stub@2.0.0"] {
        let e = derive(&reg, &request(&PTS), &reducing(a)).unwrap_err();
        assert_eq!(e, DeriveError::Unsupported { alg: a.to_owned() });
        assert_eq!(e.code(), "UNSUPPORTED");
        assert_eq!(e.to_string(), format!("UNSUPPORTED{{{a}}}"));
    }
    // An empty registry supports nothing, whatever the data.
    let e = derive(&DeriveRegistry::new(), &request(&[]), &reducing(STUB)).unwrap_err();
    assert_eq!(e, DeriveError::Unsupported { alg: STUB.to_owned() });
    // The startup registry does not know the test stub.
    assert!(DeriveRegistry::builtin().get(&alg(STUB)).is_none());
    let e = derive(&DeriveRegistry::builtin(), &request(&PTS), &reducing(STUB)).unwrap_err();
    assert_eq!(e.code(), "UNSUPPORTED");
}

#[test]
fn ac2_implementation_refusal_is_unsupported_with_its_alg() {
    let reg = registry(&[(STUB, refuse_fit)]);
    let e = derive(&reg, &request(&PTS), &reducing(STUB)).unwrap_err();
    assert_eq!(e, DeriveError::Unsupported { alg: "syn.association.mz_cylinder@1.0.0".to_owned() });
}

// ------------------------------------------------------------------ AC-3: derivation_failed

#[test]
fn ac3_too_few_sections_is_derivation_failed() {
    let reg = registry(&[(STUB, fail_sections)]);
    let e = derive(&reg, &request(&PTS), &reducing(STUB)).unwrap_err();
    assert_eq!(e, DeriveError::DerivationFailed { reason: DerivationReason::TooFewSections });
    assert_eq!(e.code(), "derivation_failed");
    assert_eq!(e.to_string(), "derivation_failed{too_few_sections}");
}

#[test]
fn ac3_every_typed_reason_passes_through() {
    let cases: [(fid_gps::loc::derive::DeriveFn, &str); 4] = [
        (fail_sections, "too_few_sections"),
        (fail_points, "too_few_points"),
        (fail_fit, "fit_failed"),
        (fail_opposite, "no_opposite_point"),
    ];
    for (f, reason) in cases {
        let reg = registry(&[(STUB, f)]);
        let e = derive(&reg, &request(&PTS), &reducing(STUB)).unwrap_err();
        match e {
            DeriveError::DerivationFailed { reason: r } => assert_eq!(r.as_str(), reason),
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(DerivationReason::ALL.len(), 4);
}

// ------------------------------------------------------------------ AC-4: no_reduction_in_chain

#[test]
fn ac4_chain_without_reduction() {
    let reg = registry(&[(STUB, must_not_run), ("syn.association.ls_cylinder@1.0.0", must_not_run)]);
    let no_red = chain(&[
        (OperatorKind::Partition, "syn.partition.whole_feature@1.0.0"),
        (OperatorKind::Association, "syn.association.ls_cylinder@1.0.0"),
        (OperatorKind::Evaluation, "syn.evaluation.zone@1.0.0"),
    ]);
    let e = derive(&reg, &request(&PTS), &no_red).unwrap_err();
    assert_eq!(e, DeriveError::NoReductionInChain);
    assert_eq!(e.code(), "no_reduction_in_chain");
    assert_eq!(e.to_string(), "no_reduction_in_chain");
    // An empty chain (a placeholder row) has no reduction either, with any registry.
    let empty = OperatorChain::new("default", Vec::new()).unwrap();
    assert_eq!(derive(&DeriveRegistry::new(), &request(&PTS), &empty).unwrap_err(), DeriveError::NoReductionInChain);
}

// ------------------------------------------------------------------ other pinned rules

#[test]
fn two_reductions_are_ambiguous() {
    let reg = registry(&[(STUB, must_not_run), ("syn.mline.stub@1.0.0", must_not_run)]);
    let two = chain(&[
        (OperatorKind::Association, "syn.association.ls_cylinder@1.0.0"),
        (OperatorKind::Reduction, STUB),
        (OperatorKind::Reduction, "syn.mline.stub@1.0.0"),
    ]);
    let e = derive(&reg, &request(&PTS), &two).unwrap_err();
    assert_eq!(e, DeriveError::AmbiguousReduction { count: 2 });
    assert_eq!(e.to_string(), "ambiguous_reduction{2}");
}

#[test]
fn non_finite_inputs_are_refused_before_the_call() {
    let reg = registry(&[(STUB, must_not_run)]);
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let pts = [[0.0, 0.0, 0.0], [0.0, 0.0, 0.01], [0.0, bad, 0.02]];
        let e = derive(&reg, &request(&pts), &reducing(STUB)).unwrap_err();
        assert_eq!(e, DeriveError::NonFiniteInput { field: "points", index: 2 });
        assert_eq!(e.code(), "non_finite_input");
        let mut req = request(&PTS);
        req.nominal_direction = Some([0.0, 0.0, bad]);
        let e = derive(&reg, &req, &reducing(STUB)).unwrap_err();
        assert_eq!(e, DeriveError::NonFiniteInput { field: "nominal_direction", index: 0 });
    }
}

#[test]
fn duplicate_registration_is_refused() {
    let mut reg = DeriveRegistry::new();
    assert!(reg.is_empty());
    reg.register(alg(STUB), stub_axis).unwrap();
    let e = reg.register(alg(STUB), must_not_run).unwrap_err();
    assert_eq!(e, RegistryError::DuplicateAlg { alg: STUB.to_owned() });
    assert_eq!(e.code(), "duplicate_alg");
    assert_eq!(reg.len(), 1);
    // The first registration stands.
    assert!(derive(&reg, &request(&PTS), &reducing(STUB)).is_ok());
    reg.register(alg("syn.axis.alpha@1.0.0"), stub_axis).unwrap();
    assert_eq!(reg.algs().map(|a| a.as_str()).collect::<Vec<_>>(), vec!["syn.axis.alpha@1.0.0", STUB]);
}

#[test]
fn element_constructors_enforce_the_invariants() {
    let z = [0.0, 0.0, 1.0];
    assert!(Axis::new([0.0; 3], z, 0.0).is_ok());
    assert_eq!(Axis::new([f64::NAN, 0.0, 0.0], z, 1.0).unwrap_err().to_string(), "non_finite{point}");
    assert_eq!(Axis::new([0.0; 3], [0.0, 0.0, f64::INFINITY], 1.0).unwrap_err().to_string(), "non_finite{direction}");
    assert_eq!(Axis::new([0.0; 3], [0.0, 0.0, 2.0], 1.0).unwrap_err().to_string(), "not_unit{direction}");
    assert_eq!(Axis::new([0.0; 3], [0.0; 3], 1.0).unwrap_err().code(), "not_unit");
    assert_eq!(Axis::new([0.0; 3], z, f64::INFINITY).unwrap_err().to_string(), "non_finite{extent}");
    assert_eq!(Axis::new([0.0; 3], z, -1e-9).unwrap_err().code(), "negative_extent");

    assert_eq!(MedianLine::new(vec![[0.0; 3]]).unwrap_err().to_string(), "too_few_points{2:1}");
    assert_eq!(MedianLine::new(vec![[0.0; 3], [0.0, f64::NAN, 0.0]]).unwrap_err().to_string(), "non_finite{points[1]}");
    assert_eq!(MedianPoints::new(Vec::new()).unwrap_err().to_string(), "too_few_points{1:0}");
    assert_eq!(MedianSurface::new(Vec::new(), None).unwrap_err().code(), "too_few_points");
    assert_eq!(MedianSurface::new(vec![[0.0; 3]], Some(vec![z, z])).unwrap_err().to_string(), "normals_mismatch{1:2}");
    assert_eq!(
        MedianSurface::new(vec![[0.0; 3], [1.0; 3]], Some(vec![z, [0.0, 0.5, 0.0]])).unwrap_err().to_string(),
        "not_unit{normals[1]}"
    );
    assert_eq!(
        MedianSurface::new(vec![[0.0; 3]], Some(vec![[f64::NAN, 0.0, 1.0]])).unwrap_err().to_string(),
        "non_finite{normals[0]}"
    );
    let s = MedianSurface::new(vec![[0.0; 3]], None).unwrap();
    assert!(s.normals().is_none());
}

// ------------------------------------------------------------------ AC-5: schema and wire form

fn element_from(v: &CValue) -> DerivedElement {
    let e = obj(v);
    match st(&e["kind"]) {
        "axis" => DerivedElement::Axis(Axis::new(p3(&e["point"]), p3(&e["direction"]), hexf(&e["extent"])).unwrap()),
        "median_line" => DerivedElement::MedianLine(MedianLine::new(list(&e["points"])).unwrap()),
        "median_surface" => {
            DerivedElement::MedianSurface(MedianSurface::new(list(&e["points"]), e.get("normals").map(list)).unwrap())
        }
        "median_points" => DerivedElement::MedianPoints(MedianPoints::new(list(&e["points"])).unwrap()),
        other => panic!("{other}"),
    }
}

#[test]
fn ac5_examples_of_all_four_variants_serialise_byte_for_byte() {
    let fx = fixture("deriv_elements.json");
    let mut kinds = BTreeSet::new();
    let reg = registry(&[
        ("syn.axis.example@1.0.0", replay),
        ("syn.mline.example@1.0.0", replay),
        ("syn.msurf.example@1.0.0", replay),
        ("syn.mpts.example@2.1.0", replay),
    ]);
    for ex in arr(&obj(&fx)["examples"]) {
        let ex = obj(ex);
        let rec = obj(&ex["record"]);
        assert_eq!(st(&rec["schema"]), DERIVED_ELEMENT_SCHEMA);
        let element = element_from(&rec["element"]);
        kinds.insert(element.kind());
        let flags: BTreeSet<DeriveFlag> =
            arr(&rec["flags"]).iter().map(|f| DeriveFlag::parse(st(f)).unwrap()).collect();
        NEXT.with(|n| *n.borrow_mut() = Some(Produced { element, flags }));
        let d = derive(&reg, &request(&PTS), &reducing(st(&rec["alg"]))).unwrap();
        assert_eq!(canon(&d.to_cvalue()), st(&ex["canonical"]), "{}", st(&ex["name"]));
    }
    assert_eq!(kinds.len(), 4, "{kinds:?}");
}

#[test]
fn ac5_exported_json_schema_is_2020_12_and_closed() {
    let CValue::Obj(s) = fid_canon::parse(DERIVED_ELEMENT_JSON_SCHEMA.as_bytes()).unwrap() else { panic!() };
    assert_eq!(s.get("$schema"), Some(&CValue::Str("https://json-schema.org/draft/2020-12/schema".to_owned())));
    assert_eq!(s.get("additionalProperties"), Some(&CValue::Bool(false)));
    let file =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("schema/derived-element-v1.schema.json"))
            .unwrap();
    assert_eq!(DERIVED_ELEMENT_JSON_SCHEMA, file);
    // Validation of the examples with an independent validator: fixtures/gdt/loc/check_deriv_schema.py (CI).
}
