//! Acceptance tests of the construction contract (spec node `KRN.CONS.DECL`, AC-1 .. AC-5).
//!
//! Expected values come from `fixtures/cons/make_decl_fixtures.py` (stdlib Python, never the
//! Rust code). The midpoint and line-plane constructions below are test fixtures only.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::Cell;

use fid_construct::contract::{
    harness, is_construct_command, run, validate_fields, CommandRow, Condition, ConstructAlgorithmId, ConstructError,
    ConstructParams, ConstructedElement, Construction, Declared, DependencyDecl, ElementGeom, ElementId, ElementRef,
    Field, InputKind, Inputs, Measure, QuantityKind, Role, RowError, UndefinedReason, ValueType, COMMON_PARAMS,
    JOURNAL_FIELDS, MAX_INPUTS,
};
use fid_fit::contract::{FittedFeature, Primitive, V3};
use fid_num::f;
use fid_types::{CommandId, Determinism};

const PAIRS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/cons/decl-toy-v1/pairs.txt");
const CANON: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/cons/decl-toy-v1/canon.txt");
const NEAR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/cons/decl-toy-v1/near-parallel.txt");

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}

fn data_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines().filter(|l| !l.is_empty() && !l.starts_with('#'))
}

fn unhex(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap())
}

fn canon(key: &str) -> String {
    let text = read(CANON);
    let prefix = format!("{key}=");
    let found = data_lines(&text).find_map(|l| l.strip_prefix(&prefix).map(str::to_string));
    found.unwrap()
}

fn params() -> ConstructParams {
    ConstructParams::new(1e-9, 1e-6).unwrap()
}

fn bits3(v: V3) -> [u64; 3] {
    [v[0].to_bits(), v[1].to_bits(), v[2].to_bits()]
}

fn point(c: V3) -> ElementGeom<'static> {
    ElementGeom::Feature(FittedFeature::Point { c })
}

fn eid(n: u8) -> ElementId {
    let mut b = [0u8; 16];
    b[15] = n;
    ElementId::from_bytes(b).unwrap()
}

// ------------------------------------------------------------------ test-only constructions

/// Test-only midpoint of two points: `(a + b) * 0.5` per coordinate.
struct Midpoint {
    declared: Declared,
    a: ElementRef,
    b: ElementRef,
}

impl Midpoint {
    fn new(a: ElementId, b: ElementId) -> Midpoint {
        let a = ElementRef::new(a, Role::new("a").unwrap());
        let b = ElementRef::new(b, Role::new("b").unwrap());
        let alg = ConstructAlgorithmId::parse("construct.midpoint.point_point@1.0.0").unwrap();
        Midpoint { declared: Declared::new(alg, &[a, b]).unwrap(), a, b }
    }
}

impl Construction for Midpoint {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, _: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let a = inputs.point(&self.a)?;
        let b = inputs.point(&self.b)?;
        let c = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, (a[2] + b[2]) * 0.5];
        ConstructedElement::feature(&FittedFeature::Point { c })
    }
}

/// Test-only line-plane intersection. Undefined when `d . n` is exactly zero; otherwise the
/// condition measure is the angle `asin(|d . n|)` between line and plane, checked against
/// the angular threshold.
struct LinePlane {
    declared: Declared,
    line: ElementRef,
    plane: ElementRef,
}

impl LinePlane {
    fn new() -> LinePlane {
        let line = ElementRef::new(eid(1), Role::new("line").unwrap());
        let plane = ElementRef::new(eid(2), Role::new("plane").unwrap());
        let alg = ConstructAlgorithmId::new("intersect", "line_plane", (1, 0, 0)).unwrap();
        LinePlane { declared: Declared::new(alg, &[line, plane]).unwrap(), line, plane }
    }
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

impl Construction for LinePlane {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let (lp, ld) = match inputs.feature(&self.line, InputKind::Feature(Primitive::Line3))? {
            FittedFeature::Line3 { p, d } => (*p, *d),
            _ => unreachable!(),
        };
        let (pp, pn) = match inputs.feature(&self.plane, InputKind::Feature(Primitive::Plane))? {
            FittedFeature::Plane { p, n } => (*p, *n),
            _ => unreachable!(),
        };
        let dn = dot(ld, pn);
        if dn == 0.0 {
            return Err(ConstructError::Undefined { reason: UndefinedReason::Parallel });
        }
        let angle = f::asin(dn.abs())?;
        Condition::check(Measure::Angle, angle, params.angular_threshold())?;
        let t = dot([pp[0] - lp[0], pp[1] - lp[1], pp[2] - lp[2]], pn) / dn;
        ConstructedElement::feature(&FittedFeature::Point {
            c: [lp[0] + t * ld[0], lp[1] + t * ld[1], lp[2] + t * ld[2]],
        })
    }
}

// ------------------------------------------------------------------ AC-1

struct Pair {
    ids: [ElementId; 2],
    pts: [V3; 4],
    mids: [V3; 3],
}

fn pairs() -> Vec<Pair> {
    let text = read(PAIRS);
    data_lines(&text)
        .map(|l| {
            let t: Vec<&str> = l.split(' ').collect();
            assert_eq!(t.len(), 2 + 21);
            let v: Vec<f64> = t[2..].iter().map(|s| unhex(s)).collect();
            let p = |i: usize| [v[3 * i], v[3 * i + 1], v[3 * i + 2]];
            Pair {
                ids: [ElementId::parse(t[0]).unwrap(), ElementId::parse(t[1]).unwrap()],
                pts: [p(0), p(1), p(2), p(3)],
                mids: [p(4), p(5), p(6)],
            }
        })
        .collect()
}

#[test]
fn ac1_midpoint_repeat_and_replace_are_bit_identical() {
    let all = pairs();
    assert_eq!(all.len(), 100);
    let params = params();
    for (k, pr) in all.iter().enumerate() {
        let [a, b, c, d] = pr.pts;
        let inputs = [point(a), point(b)];
        let replacements = [point(c), point(d)];
        let make = || Midpoint::new(pr.ids[0], pr.ids[1]);
        let eval = harness::check(make, &params, &inputs, &replacements).unwrap().unwrap();
        let FittedFeature::Point { c: m } = eval.output().as_feature().unwrap() else { panic!("pair {k}") };
        assert_eq!(bits3(*m), bits3(pr.mids[0]), "pair {k}: mid(a, b)");
        // Replace-then-evaluate against the independent reference too.
        for (inputs, want) in [([point(c), point(b)], pr.mids[1]), ([point(a), point(d)], pr.mids[2])] {
            let e = run(&make(), &inputs, &params).unwrap();
            let FittedFeature::Point { c: m } = e.output().as_feature().unwrap() else { panic!("pair {k}") };
            assert_eq!(bits3(*m), bits3(want), "pair {k}: replaced input");
        }
    }
}

/// A construction whose output depends on how often it ran: the harness must reject it.
struct Counting {
    inner: Midpoint,
    calls: Cell<u32>,
}

impl Construction for Counting {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        self.inner.declare(params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        self.calls.set(self.calls.get() + 1);
        let out = self.inner.evaluate(inputs, params)?;
        let FittedFeature::Point { c } = out.as_feature().unwrap() else { unreachable!() };
        ConstructedElement::feature(&FittedFeature::Point { c: [c[0] + f64::from(self.calls.get()), c[1], c[2]] })
    }
}

/// A construction that caches its first result and ignores later inputs.
struct Caching {
    inner: Midpoint,
    cache: std::cell::RefCell<Option<ConstructedElement>>,
}

impl Construction for Caching {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        self.inner.declare(params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, params: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        if let Some(c) = self.cache.borrow().as_ref() {
            return Ok(c.clone());
        }
        let out = self.inner.evaluate(inputs, params)?;
        *self.cache.borrow_mut() = Some(out.clone());
        Ok(out)
    }
}

#[test]
fn ac1_harness_detects_hidden_state() {
    let params = params();
    let inputs = [point([1.0, 2.0, 3.0]), point([3.0, 2.0, 1.0])];
    let repl = [point([5.0, 5.0, 5.0]), point([7.0, 7.0, 7.0])];
    let counting = || Counting { inner: Midpoint::new(eid(1), eid(2)), calls: Cell::new(0) };
    assert_eq!(harness::check(counting, &params, &inputs, &repl), Err(harness::HarnessError::RepeatDiffers));
    let caching = || Caching { inner: Midpoint::new(eid(1), eid(2)), cache: Default::default() };
    assert_eq!(
        harness::check(caching, &params, &inputs, &repl),
        Err(harness::HarnessError::ReplaceDiffers { index: 0 })
    );
    let make = || Midpoint::new(eid(1), eid(2));
    assert_eq!(
        harness::check(make, &params, &inputs, &repl[..1]),
        Err(harness::HarnessError::ReplacementCount { expected: 2, got: 1 })
    );
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_declaration_bytes_match_reference_and_are_stable() {
    let pr = &pairs()[0];
    let m = Midpoint::new(pr.ids[0], pr.ids[1]);
    let p = params();
    assert_eq!(String::from_utf8(p.canonical_bytes()).unwrap(), canon("params_bytes"));
    assert_eq!(p.digest().as_str(), canon("params_digest"));
    let d1 = m.declare(&p).canonical_bytes();
    let d2 = Midpoint::new(pr.ids[0], pr.ids[1]).declare(&params()).canonical_bytes();
    assert_eq!(d1, d2);
    assert_eq!(String::from_utf8(d1).unwrap(), canon("decl_bytes"));
}

#[test]
fn ac2_changing_any_threshold_changes_the_digest() {
    let base = params().digest();
    let ang = ConstructParams::new(2e-9, 1e-6).unwrap().digest();
    let dist = ConstructParams::new(1e-9, 2e-6).unwrap().digest();
    assert_ne!(base, ang);
    assert_ne!(base, dist);
    assert_eq!(ang.as_str(), canon("params_angular_2e-9_digest"));
    assert_eq!(dist.as_str(), canon("params_distance_2e-6_digest"));
    // Named values: insertion order does not matter, the bytes are sorted by name.
    let v =
        params().with("offset", QuantityKind::Length, 0.5).unwrap().with("branch", QuantityKind::Count, 1.0).unwrap();
    assert_eq!(String::from_utf8(v.canonical_bytes()).unwrap(), canon("params_values_bytes"));
    assert_eq!(v.digest().as_str(), canon("params_values_digest"));
    let w = params().with("offset", QuantityKind::Length, 0.5 + f64::EPSILON).unwrap();
    assert_ne!(w.digest(), params().with("offset", QuantityKind::Length, 0.5).unwrap().digest());
    // A construction declared under other parameters declares another digest.
    let m = Midpoint::new(eid(1), eid(2));
    assert_ne!(
        m.declare(&params()).canonical_bytes(),
        m.declare(&ConstructParams::new(2e-9, 1e-6).unwrap()).canonical_bytes()
    );
}

#[test]
fn ac2_output_and_event_match_reference() {
    let pr = &pairs()[0];
    let m = Midpoint::new(pr.ids[0], pr.ids[1]);
    let e = run(&m, &[point(pr.pts[0]), point(pr.pts[1])], &params()).unwrap();
    assert_eq!(String::from_utf8(e.output().canonical_bytes()).unwrap(), canon("output_bytes"));
    assert_eq!(e.output_digest().as_str(), canon("output_digest"));
    let cmd = CommandId::parse(&canon("event_type")).unwrap();
    let ev = e.event(&cmd).unwrap();
    assert_eq!(ev.event_type(), canon("event_type"));
    assert_eq!(String::from_utf8(ev.payload_bytes()).unwrap(), canon("event_payload"));
    assert_eq!(ev.inputs(), m.declare(&params()).inputs());
    assert_eq!(ev.params_digest(), &params().digest());
    // Only construct.* commands journal a construct event.
    assert_eq!(
        e.event(&CommandId::parse("fit.ls_plane@1").unwrap()),
        Err(ConstructError::InvalidDecl { field: "command" })
    );
    let v = ConstructedElement::value(0.25, QuantityKind::Angle).unwrap();
    assert_eq!(String::from_utf8(v.canonical_bytes()).unwrap(), canon("value_output_bytes"));
    assert_eq!(v.as_value().unwrap().unit(), "rad");
}

// ------------------------------------------------------------------ AC-3

fn near_cases() -> Vec<(String, [V3; 4], f64, String, Vec<String>)> {
    let text = read(NEAR);
    data_lines(&text)
        .map(|l| {
            let t: Vec<&str> = l.split(' ').collect();
            let v: Vec<f64> = t[1..14].iter().map(|s| unhex(s)).collect();
            let p = |i: usize| [v[3 * i], v[3 * i + 1], v[3 * i + 2]];
            (
                t[0].to_string(),
                [p(0), p(1), p(2), p(3)],
                v[12],
                t[14].to_string(),
                t[15..].iter().map(|s| s.to_string()).collect(),
            )
        })
        .collect()
}

#[test]
fn ac3_near_parallel_is_ill_conditioned_with_no_element() {
    let cases = near_cases();
    assert_eq!(cases.len(), 3);
    for (name, [lp, ld, pp, pn], thr, expected, value) in cases {
        let params = ConstructParams::new(thr, 1e-6).unwrap();
        let inputs = [
            ElementGeom::Feature(FittedFeature::Line3 { p: lp, d: ld }),
            ElementGeom::Feature(FittedFeature::Plane { p: pp, n: pn }),
        ];
        let got = run(&LinePlane::new(), &inputs, &params);
        match expected.as_str() {
            "ill_conditioned" => {
                let Err(ConstructError::IllConditioned(c)) = got else { panic!("{name}: {got:?}") };
                assert_eq!(c.measure(), Measure::Angle);
                assert_eq!(c.measure().as_str(), "angle");
                assert_eq!(c.value().to_bits(), unhex(&value[0]).to_bits(), "{name}");
                assert_eq!(c.threshold().to_bits(), 1e-9f64.to_bits(), "{name}");
            }
            "undefined" => {
                assert_eq!(value[0], "parallel");
                assert_eq!(got, Err(ConstructError::Undefined { reason: UndefinedReason::Parallel }), "{name}");
            }
            "ok" => {
                let e = got.unwrap();
                let FittedFeature::Point { c } = e.output().as_feature().unwrap() else { panic!("{name}") };
                let want = [unhex(&value[0]), unhex(&value[1]), unhex(&value[2])];
                assert_eq!(bits3(*c), bits3(want), "{name}");
            }
            other => panic!("{other}"),
        }
    }
    // The same near-parallel line passes once the threshold is below the measured angle.
    let line = ElementGeom::Feature(FittedFeature::Line3 { p: [0.0, 0.0, 1.0], d: [1.0, 0.0, 1e-12] });
    let plane = ElementGeom::Feature(FittedFeature::Plane { p: [0.0; 3], n: [0.0, 0.0, 1.0] });
    assert!(run(&LinePlane::new(), &[line, plane], &ConstructParams::new(1e-13, 1e-6).unwrap()).is_ok());
}

#[test]
fn condition_check_semantics_are_pinned() {
    // Strict comparison: equal to the threshold passes.
    assert_eq!(Condition::check(Measure::Distance, 1e-6, 1e-6), Ok(()));
    let Err(ConstructError::IllConditioned(c)) = Condition::check(Measure::Distance, -0.0, 1e-6) else { panic!() };
    assert_eq!(c.value().to_bits(), 0.0f64.to_bits());
    assert_eq!(c.measure().unit(), "m");
    // Zero threshold refuses nothing.
    assert_eq!(Condition::check(Measure::Ratio, 0.0, 0.0), Ok(()));
    for bad in [f64::NAN, f64::INFINITY, -1.0] {
        assert_eq!(Condition::check(Measure::Angle, bad, 1e-9).map_err(|e| e.code()), Err("numeric"));
        assert_eq!(Condition::check(Measure::Angle, 1.0, bad).map_err(|e| e.code()), Err("numeric"));
    }
    let codes: Vec<&str> = UndefinedReason::ALL.iter().map(|r| r.as_str()).collect();
    assert_eq!(
        codes,
        ["parallel", "coincident", "collinear", "zero_length", "no_intersection", "skew", "not_supported"]
    );
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_wrong_input_count_is_reported() {
    let m = Midpoint::new(eid(1), eid(2));
    let p = params();
    assert_eq!(run(&m, &[point([1.0; 3])], &p).map(|_| ()), Err(ConstructError::InputCount { expected: 2, got: 1 }));
    let three = [point([1.0; 3]), point([2.0; 3]), point([3.0; 3])];
    assert_eq!(run(&m, &three, &p).map(|_| ()), Err(ConstructError::InputCount { expected: 2, got: 3 }));
    // Wrong kind, with the declared index.
    let plane = ElementGeom::Feature(FittedFeature::Plane { p: [0.0; 3], n: [0.0, 0.0, 1.0] });
    assert_eq!(
        run(&m, &[point([1.0; 3]), plane], &p).map(|_| ()),
        Err(ConstructError::WrongInputKind {
            index: 1,
            expected: InputKind::Feature(Primitive::Point),
            got: InputKind::Feature(Primitive::Plane)
        })
    );
    // An invalid input feature never reaches the construction.
    assert_eq!(
        run(&m, &[point([f64::NAN, 0.0, 0.0]), point([1.0; 3])], &p).map(|_| ()).map_err(|e| e.code()),
        Err("invalid_feature")
    );
}

/// Declares only `a` but reads `b` too: refused, because `b` is not in its declaration.
struct Sneaky {
    declared: Declared,
    a: ElementRef,
    b: ElementRef,
}

impl Construction for Sneaky {
    fn declare(&self, params: &ConstructParams) -> DependencyDecl {
        DependencyDecl::new(&self.declared, params)
    }

    fn evaluate(&self, inputs: &Inputs<'_>, _: &ConstructParams) -> Result<ConstructedElement, ConstructError> {
        let a = inputs.point(&self.a)?;
        let b = inputs.point(&self.b)?;
        ConstructedElement::feature(&FittedFeature::Point { c: [a[0] + b[0], a[1], a[2]] })
    }
}

#[test]
fn undeclared_input_cannot_be_read() {
    let a = ElementRef::new(eid(1), Role::new("a").unwrap());
    let b = ElementRef::new(eid(2), Role::new("b").unwrap());
    let alg = ConstructAlgorithmId::parse("construct.test.sneaky@0.1.0").unwrap();
    let s = Sneaky { declared: Declared::new(alg, &[a]).unwrap(), a, b };
    assert_eq!(
        run(&s, &[point([1.0; 3])], &params()).map(|_| ()),
        Err(ConstructError::UndeclaredInput { role: Role::new("b").unwrap() })
    );
    // A reference with a declared role but another element is not the declared input either.
    let s2 = Sneaky {
        declared: Declared::new(ConstructAlgorithmId::parse("construct.test.sneaky@0.1.0").unwrap(), &[a, b]).unwrap(),
        a,
        b: ElementRef::new(eid(3), Role::new("b").unwrap()),
    };
    assert_eq!(
        run(&s2, &[point([1.0; 3]), point([2.0; 3])], &params()).map(|_| ()).map_err(|e| e.code()),
        Err("undeclared_input")
    );
}

// ------------------------------------------------------------------ AC-5

fn midpoint_row() -> Result<CommandRow, RowError> {
    CommandRow::new(
        CommandId::parse("construct.midpoint@1").unwrap(),
        &[ConstructAlgorithmId::parse("construct.midpoint.point_point@1.0.0").unwrap()],
        Determinism::D0,
        &[],
        &[Field::new("element.c", ValueType::NumberArray, Some(QuantityKind::Length), true)],
    )
}

#[test]
fn ac5_rows_declare_quantity_and_unit_on_every_numeric_field() {
    let row = midpoint_row().unwrap();
    assert_eq!(String::from_utf8(row.to_json()).unwrap(), canon("row_bytes"));
    for f in row.params().iter().chain(row.result()) {
        assert_eq!(f.ty.is_numeric(), f.quantity.is_some(), "{}", f.name);
    }
    assert_eq!(validate_fields(&COMMON_PARAMS), Ok(()));
    assert_eq!(validate_fields(&JOURNAL_FIELDS), Ok(()));
    // Sample rows that break the rule are refused.
    let cmd = || CommandId::parse("construct.offset@1").unwrap();
    let alg = [ConstructAlgorithmId::parse("construct.offset.plane@1.0.0").unwrap()];
    let no_q = [Field::new("params.offset", ValueType::Number, None, true)];
    assert_eq!(
        CommandRow::new(cmd(), &alg, Determinism::D0, &no_q, &[]),
        Err(RowError::MissingQuantity { field: "params.offset" })
    );
    let extra_q = [Field::new("params.rule", ValueType::String, Some(QuantityKind::Length), true)];
    assert_eq!(
        CommandRow::new(cmd(), &alg, Determinism::D0, &extra_q, &[]),
        Err(RowError::UnexpectedQuantity { field: "params.rule" })
    );
    let clash = [Field::new("thresholds.angular", ValueType::Number, Some(QuantityKind::Angle), true)];
    assert_eq!(
        CommandRow::new(cmd(), &alg, Determinism::D0, &clash, &[]),
        Err(RowError::DuplicateField { field: "thresholds.angular" })
    );
    assert_eq!(CommandRow::new(cmd(), &alg, Determinism::N, &[], &[]), Err(RowError::Determinism));
    assert_eq!(CommandRow::new(cmd(), &[], Determinism::D0, &[], &[]), Err(RowError::Algorithms));
    let two = [alg[0].clone(), alg[0].clone()];
    assert_eq!(CommandRow::new(cmd(), &two, Determinism::D0, &[], &[]), Err(RowError::Algorithms));
    assert_eq!(
        CommandRow::new(CommandId::parse("fit.ls_plane@1").unwrap(), &alg, Determinism::D0, &[], &[]),
        Err(RowError::Command)
    );
    assert!(is_construct_command(&CommandId::parse("construct.intersect@1").unwrap()));
    assert!(!is_construct_command(&CommandId::parse("construct.a.b@1").unwrap()));
}

// ------------------------------------------------------------------ IDs and parameters

#[test]
fn algorithm_id_grammar_is_pinned() {
    let id = ConstructAlgorithmId::parse("construct.intersect.line_plane@1.2.3").unwrap();
    assert_eq!((id.op(), id.variant(), id.version()), ("intersect", "line_plane", (1, 2, 3)));
    assert_eq!(id.to_string(), "construct.intersect.line_plane@1.2.3");
    assert_eq!(id.as_kci().module, "construct");
    let long = "a".repeat(32);
    assert!(ConstructAlgorithmId::parse(&format!("construct.{long}.{long}@4294967295.4294967295.4294967295")).is_ok());
    for bad in [
        "fit.plane.ls@1.0.0",
        "construct.intersect@1.0.0",
        "construct.intersect.line_plane",
        "construct.intersect.line_plane@1.0",
        "construct.intersect.line_plane@01.0.0",
        "construct.intersect.line_plane@1.0.4294967296",
        "construct.Intersect.line_plane@1.0.0",
        "construct.intersect.1line@1.0.0",
        "construct.intersect.line-plane@1.0.0",
        "construct.intersect.line_plane.x@1.0.0",
        " construct.intersect.line_plane@1.0.0",
        "",
    ] {
        assert_eq!(
            ConstructAlgorithmId::parse(bad),
            Err(ConstructError::InvalidDecl { field: "algorithm" }),
            "{bad:?}"
        );
    }
    assert!(ConstructAlgorithmId::parse(&format!("construct.{long}a.x@1.0.0")).is_err());
    assert!(ConstructAlgorithmId::new("intersect", "Line", (1, 0, 0)).is_err());
}

#[test]
fn element_ids_and_roles() {
    let text = "cb7bb484-b6c5-9c37-f9f2-d8054190feb1";
    let id = ElementId::parse(text).unwrap();
    assert_eq!(id.to_text(), text);
    assert_eq!(ElementId::from_bytes(id.to_bytes()), Ok(id));
    for bad in [
        "CB7BB484-B6C5-9C37-F9F2-D8054190FEB1",
        "cb7bb484b6c59c37f9f2d8054190feb1",
        "{cb7bb484-b6c5-9c37-f9f2-d8054190feb1}",
        "cb7bb484-b6c5-9c37-f9f2_d8054190feb1",
        "00000000-0000-0000-0000-000000000000",
        "cb7bb484-b6c5-9c37-f9f2-d8054190feb",
    ] {
        assert!(ElementId::parse(bad).is_err(), "{bad}");
    }
    assert!(ElementId::from_bytes([0; 16]).is_err());
    for bad in ["", "A", "1a", "a-b", "a b", "abcdefghijklmnopqrstuvwxyz0123456"] {
        assert!(Role::new(bad).is_err(), "{bad:?}");
    }
    // Roles are unique within a declaration; elements may repeat.
    let alg = || ConstructAlgorithmId::parse("construct.midpoint.point_point@1.0.0").unwrap();
    let r = |n, role| ElementRef::new(eid(n), Role::new(role).unwrap());
    assert_eq!(
        Declared::new(alg(), &[r(1, "a"), r(2, "a")]),
        Err(ConstructError::InvalidDecl { field: "inputs.role" })
    );
    let d = Declared::new(alg(), &[r(9, "a"), r(3, "b"), r(9, "c")]).unwrap();
    let decl = DependencyDecl::new(&d, &params());
    assert_eq!(decl.edge_inputs(), vec![eid(3), eid(9)]);
    let many: Vec<ElementRef> = (0..=MAX_INPUTS).map(|_| r(1, "a")).collect();
    assert_eq!(Declared::new(alg(), &many), Err(ConstructError::InvalidDecl { field: "inputs" }));
}

#[test]
fn params_are_checked() {
    for (a, d, field) in [
        (f64::NAN, 0.0, "angular_threshold"),
        (-1e-9, 0.0, "angular_threshold"),
        (-0.0, 0.0, "angular_threshold"),
        (4.0, 0.0, "angular_threshold"),
        (0.0, f64::INFINITY, "distance_threshold"),
        (0.0, -0.0, "distance_threshold"),
    ] {
        assert_eq!(ConstructParams::new(a, d), Err(ConstructError::InvalidParams { field }), "{a} {d}");
    }
    assert!(ConstructParams::new(core::f64::consts::PI, 0.0).is_ok());
    let p = params().with("offset", QuantityKind::Length, -0.0).unwrap();
    assert_eq!(p.get("offset", QuantityKind::Length).unwrap().to_bits(), 0.0f64.to_bits());
    assert_eq!(p.get("offset", QuantityKind::Angle), Err(ConstructError::MissingParam { name: "offset" }));
    assert_eq!(p.get("branch", QuantityKind::Count), Err(ConstructError::MissingParam { name: "branch" }));
    let dup = p.clone().with("offset", QuantityKind::Length, 1.0);
    assert_eq!(dup, Err(ConstructError::InvalidParams { field: "values" }));
    assert!(params().with("n", QuantityKind::Count, 1.5).is_err());
    assert!(params().with("n", QuantityKind::Count, -1.0).is_err());
    assert!(params().with("x", QuantityKind::Length, f64::NAN).is_err());
    assert!(params().with("Bad", QuantityKind::Length, 1.0).is_err());
    let mut full = params();
    for name in NAMES {
        full = full.with(name, QuantityKind::Dimensionless, 1.0).unwrap();
    }
    assert!(full.with("zz", QuantityKind::Dimensionless, 1.0).is_err());
    assert!(ConstructedElement::value(f64::INFINITY, QuantityKind::Length).is_err());
    assert!(ConstructedElement::feature(&FittedFeature::Line3 { p: [0.0; 3], d: [0.0; 3] }).is_err());
}

const NAMES: [&str; 64] = [
    "v00", "v01", "v02", "v03", "v04", "v05", "v06", "v07", "v08", "v09", "v10", "v11", "v12", "v13", "v14", "v15",
    "v16", "v17", "v18", "v19", "v20", "v21", "v22", "v23", "v24", "v25", "v26", "v27", "v28", "v29", "v30", "v31",
    "v32", "v33", "v34", "v35", "v36", "v37", "v38", "v39", "v40", "v41", "v42", "v43", "v44", "v45", "v46", "v47",
    "v48", "v49", "v50", "v51", "v52", "v53", "v54", "v55", "v56", "v57", "v58", "v59", "v60", "v61", "v62", "v63",
];
