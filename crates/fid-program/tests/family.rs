//! Acceptance criteria of spec node AUTO.PROG.FAMILY. Fixtures and expected error labels come
//! from `fixtures/program/family/make_fixtures.py` (independent Python writer, codes labelled
//! by hand from the spec's rules); nothing here derives an expected value from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_program::family::{MAX_PARAMS, MAX_VARIANTS};
use fid_program::{
    binding_from_json, binding_from_value, binding_to_value, check_binding, check_decls, BindingError, BoundValue,
    Family, ParamDecl, ParamType, Program,
};
use fid_registry::{Catalogue, EntityType};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn dir() -> PathBuf {
    root().join("fixtures/program/family")
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(m) => m,
        _ => panic!("not an object"),
    }
}

fn expected() -> CValue {
    parse(&std::fs::read(dir().join("expected.json")).unwrap()).unwrap()
}

fn catalogue() -> Catalogue {
    let exp = expected();
    let CValue::Str(rel) = obj(&exp).get("catalogue").unwrap() else { panic!("catalogue path") };
    Catalogue::from_json(&std::fs::read(root().join(rel)).unwrap()).unwrap()
}

/// `(code, param, variant)` triples.
type Labels = Vec<(String, Option<String>, Option<String>)>;

fn labels(v: &CValue) -> Labels {
    let CValue::Arr(items) = v else { panic!("labels must be an array") };
    let s = |m: &BTreeMap<String, CValue>, k: &str| match m.get(k) {
        Some(CValue::Str(s)) => Some(s.clone()),
        Some(CValue::Null) | None => None,
        other => panic!("bad label {other:?}"),
    };
    items.iter().map(|i| (s(obj(i), "code").unwrap(), s(obj(i), "param"), s(obj(i), "variant"))).collect()
}

fn got(errors: &[BindingError]) -> Labels {
    errors
        .iter()
        .map(|e| (e.code().to_string(), e.param().map(str::to_string), e.variant().map(str::to_string)))
        .collect()
}

fn program(rel: &str) -> Program {
    Program::parse(&std::fs::read(dir().join(format!("{rel}.json"))).unwrap()).unwrap()
}

fn bracket() -> Family {
    Family::from_program(&program("bracket"), &catalogue()).unwrap()
}

fn binding_file(name: &str) -> Vec<u8> {
    std::fs::read(dir().join("bindings").join(format!("{name}.json"))).unwrap()
}

/// AC-1: the bracket family loads, and check_binding on variant B-120 returns no errors.
#[test]
fn ac1_variant_b120_is_clean() {
    let fam = bracket();
    let names: Vec<_> = fam.params().iter().map(|d| (d.name.as_str(), d.ty, d.required)).collect();
    assert_eq!(
        names,
        [
            ("hole_spacing", ParamType::Quantity, true),
            ("hole_count", ParamType::Integer, true),
            ("scan", ParamType::DataHandle, true),
            ("datum_hole", ParamType::Ref(EntityType::Element), false),
        ]
    );
    assert_eq!(fam.params()[0].quantity_kind.as_deref(), Some("length"));
    assert_eq!(fam.params()[1].default, Some(BoundValue::Number(4.0)));
    let b120 = fam.variant("B-120").unwrap();
    assert_eq!(check_binding(fam.params(), b120, &catalogue()), vec![]);
    assert_eq!(fam.check(b120, &catalogue()), vec![]);
    assert_eq!(fam.check(fam.variant("B-4IN").unwrap(), &catalogue()), vec![]);
}

/// Every program fixture gives exactly its hand-labelled errors.
#[test]
fn program_fixtures() {
    let exp = expected();
    let progs = obj(obj(&exp).get("programs").unwrap());
    assert!(progs.len() >= 20);
    let cat = catalogue();
    for (name, want) in progs {
        let p = program(name);
        let have = match Family::from_program(&p, &cat) {
            Ok(_) => vec![],
            Err(e) => {
                assert!(!e.is_empty(), "{name}: Err with no errors");
                got(&e)
            }
        };
        assert_eq!(have, labels(want), "{name}");
    }
    // Every fixture file is labelled.
    let mut files = BTreeSet::new();
    for sub in ["", "valid/", "invalid/"] {
        for e in std::fs::read_dir(dir().join(sub)).unwrap() {
            let p = e.unwrap().path();
            let stem = p.file_stem().unwrap().to_string_lossy().into_owned();
            if p.extension().is_some_and(|x| x == "json") && stem != "expected" {
                files.insert(format!("{sub}{stem}"));
            }
        }
    }
    assert_eq!(files, progs.keys().cloned().collect::<BTreeSet<_>>());
}

/// AC-2 to AC-5 and the further binding cases.
#[test]
fn binding_fixtures() {
    let exp = expected();
    let fam = bracket();
    let cat = catalogue();
    let bindings = obj(obj(&exp).get("bindings").unwrap());
    for (name, want) in bindings {
        let b = binding_from_json(&binding_file(name)).unwrap();
        assert_eq!(got(&check_binding(fam.params(), &b, &cat)), labels(want), "{name}");
    }
    let raw = obj(obj(&exp).get("raw_bindings").unwrap());
    for (name, reason) in raw {
        let CValue::Str(reason) = reason else { panic!("reason") };
        match binding_from_json(&binding_file(name)) {
            Err(BindingError::BadBinding { reason: r }) => assert_eq!(r, reason, "{name}"),
            other => panic!("{name}: {other:?}"),
        }
    }
    let all: BTreeSet<String> = std::fs::read_dir(dir().join("bindings"))
        .unwrap()
        .map(|e| e.unwrap().path().file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    let labelled: BTreeSet<String> = bindings.keys().chain(raw.keys()).cloned().collect();
    assert_eq!(all, labelled);
}

/// AC-2: a binding without hole_spacing.
#[test]
fn ac2_missing_parameter() {
    let b = binding_from_json(&binding_file("missing-hole-spacing")).unwrap();
    assert_eq!(
        bracket().check(&b, &catalogue()),
        vec![BindingError::MissingParameter { param: "hole_spacing".into() }]
    );
}

/// AC-3: 5 s for a length is wrong_unit; "four" for an integer is wrong_type.
#[test]
fn ac3_wrong_unit_and_wrong_type() {
    let b = binding_from_json(&binding_file("seconds")).unwrap();
    assert_eq!(
        bracket().check(&b, &catalogue()),
        vec![BindingError::WrongUnit { param: "hole_spacing".into(), unit: "s".into(), kind: "length".into() }]
    );
    let b = binding_from_json(&binding_file("hole-count-four")).unwrap();
    assert_eq!(
        bracket().check(&b, &catalogue()),
        vec![BindingError::WrongType { param: "hole_count".into(), expected: ParamType::Integer }]
    );
}

/// AC-4: an extra parameter.
#[test]
fn ac4_unknown_parameter() {
    let b = binding_from_json(&binding_file("colour")).unwrap();
    assert_eq!(bracket().check(&b, &catalogue()), vec![BindingError::UnknownParameter { param: "colour".into() }]);
}

/// AC-5: 4.724 in is accepted and kept exactly as authored.
#[test]
fn ac5_inches_kept_as_authored() {
    let bytes = binding_file("inches");
    let b = binding_from_json(&bytes).unwrap();
    assert_eq!(bracket().check(&b, &catalogue()), vec![]);
    assert_eq!(b.get("hole_spacing"), Some(&BoundValue::Quantity { value: "4.724".into(), unit: "in".into() }));
    // The binding writes back to the same bytes: nothing converted or filled in.
    assert_eq!(to_canonical_bytes(&binding_to_value(&b)).unwrap(), bytes);
    let fam = bracket();
    let v = fam.variant("B-4IN").unwrap();
    assert_eq!(v.get("hole_spacing"), Some(&BoundValue::Quantity { value: "4.724".into(), unit: "in".into() }));
}

/// AC-6: two variants named B-120.
#[test]
fn ac6_duplicate_variant() {
    let err = Family::from_program(&program("invalid/dup-variant"), &catalogue()).unwrap_err();
    assert_eq!(err, vec![BindingError::DuplicateVariant { index: 1, variant: "B-120".into() }]);
}

/// The variants field round-trips through the program format byte for byte.
#[test]
fn variants_round_trip_through_fmt() {
    for rel in ["bracket", "valid/no-variants", "valid/empty-variants"] {
        let bytes = std::fs::read(dir().join(format!("{rel}.json"))).unwrap();
        let p = Program::parse(&bytes).unwrap();
        assert_eq!(p.to_canonical().unwrap(), bytes, "{rel}");
    }
    assert_eq!(program("valid/no-variants").variants, None);
    assert_eq!(program("valid/empty-variants").variants, Some(vec![]));
    assert_eq!(program("bracket").variants.map(|v| v.len()), Some(2));
}

#[test]
fn variant_items_must_be_objects() {
    let bytes = std::fs::read(dir().join("valid/empty-variants.json")).unwrap();
    let text = String::from_utf8(bytes).unwrap().replace("\"variants\":[]", "\"variants\":[1]");
    let e = Program::parse(text.as_bytes()).unwrap_err();
    assert_eq!(e.code(), "schema_invalid");
    let text = text.replace("\"variants\":[1]", "\"variants\":{}");
    assert_eq!(Program::parse(text.as_bytes()).unwrap_err().code(), "schema_invalid");
}

#[test]
fn unknown_variant() {
    let fam = bracket();
    assert_eq!(fam.variant("b-120").unwrap_err(), BindingError::UnknownVariant { variant: "b-120".into() });
    assert_eq!(fam.variant("B-999").unwrap_err().code(), "unknown_variant");
}

#[test]
fn defaults_are_not_filled_in() {
    let fam = bracket();
    let b = fam.variant("B-120").unwrap();
    assert!(!b.contains_key("hole_count"));
}

fn decl(name: &str, ty: ParamType, kind: Option<&str>) -> ParamDecl {
    ParamDecl {
        name: name.into(),
        ty,
        quantity_kind: kind.map(str::to_string),
        required: true,
        default: None,
        description: None,
    }
}

/// Hand-built declarations are checked too, and their errors are returned alone.
#[test]
fn check_binding_checks_declarations_first() {
    let cat = catalogue();
    let decls = vec![decl("a", ParamType::Integer, None), decl("A", ParamType::Integer, None)];
    let b = binding_from_value(parse(br#"{"zzz":1}"#).unwrap()).unwrap();
    assert_eq!(check_binding(&decls, &b, &cat), vec![BindingError::DuplicateParameter { index: 1, param: "A".into() }]);
    let decls = vec![decl("q", ParamType::Quantity, None)];
    assert_eq!(got(&check_decls(&decls, &cat)), vec![("bad_declaration".into(), Some("q".into()), None)]);
    let decls = vec![decl("q", ParamType::Quantity, Some("time"))];
    assert_eq!(check_decls(&decls, &cat), vec![]);
    let b = binding_from_value(parse(br#"{"q":{"unit":"min","value":"2.50"}}"#).unwrap()).unwrap();
    assert_eq!(check_binding(&decls, &b, &cat), vec![]);
    let decls: Vec<ParamDecl> = (0..=MAX_PARAMS).map(|i| decl(&format!("p{i}"), ParamType::String, None)).collect();
    assert_eq!(got(&check_decls(&decls, &cat)), vec![("family_too_large".into(), None, None)]);
}

/// Geo-frame-only catalogue (no time kind): a time unit is still wrong_unit for a length.
#[test]
fn geo_frame_catalogue() {
    let cat = Catalogue::geo_frame().unwrap();
    let fam = Family::from_program(&program("bracket"), &cat).unwrap();
    let b = binding_from_json(&binding_file("seconds")).unwrap();
    assert_eq!(got(&fam.check(&b, &cat)), vec![("wrong_unit".into(), Some("hole_spacing".into()), None)]);
}

#[test]
fn size_bounds() {
    let cat = catalogue();
    let p: BTreeMap<String, CValue> = BTreeMap::new();
    let params = vec![p.clone(); MAX_PARAMS + 1];
    assert_eq!(
        got(&Family::from_parts(&params, &[], &cat).unwrap_err()),
        vec![("family_too_large".into(), None, None)]
    );
    let variants = vec![p; MAX_VARIANTS + 1];
    assert_eq!(
        got(&Family::from_parts(&[], &variants, &cat).unwrap_err()),
        vec![("family_too_large".into(), None, None)]
    );
    let big = vec![b' '; fid_program::family::MAX_BINDING_BYTES + 1];
    assert_eq!(binding_from_json(&big).unwrap_err().code(), "bad_binding");
    let deep = format!("{{\"a\":{}{}}}", "[".repeat(40), "]".repeat(40));
    assert_eq!(binding_from_json(deep.as_bytes()).unwrap_err().code(), "bad_binding");
}

/// Declarations write back to their wire form.
#[test]
fn decl_round_trip() {
    let p = program("bracket");
    let fam = bracket();
    for (d, wire) in fam.params().iter().zip(&p.params) {
        assert_eq!(d.to_value(), CValue::Obj(wire.clone()));
    }
    for (v, wire) in fam.variants().iter().zip(p.variants.as_deref().unwrap()) {
        assert_eq!(v.to_value(), CValue::Obj(wire.clone()));
    }
}

/// Messages never echo more than 64 characters of input.
#[test]
fn display_is_bounded() {
    let long = "x".repeat(10_000);
    let e = BindingError::UnknownParameter { param: long.clone() };
    assert!(e.to_string().len() < 100, "{e}");
    let e = BindingError::InVariant {
        variant: long.clone(),
        error: Box::new(BindingError::MissingParameter { param: long }),
    };
    assert!(e.to_string().len() < 200);
    assert_eq!(e.code(), "missing_parameter");
}

/// Arbitrary JSON in parameter and variant slots never panics and always fails closed or
/// succeeds with consistent results.
#[test]
fn hostile_shapes_do_not_panic() {
    let cat = catalogue();
    let samples = [
        "null",
        "1",
        "\"x\"",
        "[]",
        "{}",
        r#"{"name":1}"#,
        r#"{"name":"a","type":"quantity","required":true}"#,
        r#"{"name":"a","type":"integer","required":true,"default":null}"#,
        r#"{"name":"a","type":"string","required":false,"description":7}"#,
        r#"{"binding":[],"name":"V"}"#,
        r#"{"binding":{},"name":""}"#,
    ];
    for s in samples {
        let v = parse(s.as_bytes()).unwrap();
        let m = match v {
            CValue::Obj(m) => m,
            _ => continue,
        };
        let r1 = Family::from_parts(std::slice::from_ref(&m), &[], &cat);
        let r2 = Family::from_parts(&[], std::slice::from_ref(&m), &cat);
        if let Err(e) = r1 {
            assert!(!e.is_empty());
        }
        if let Err(e) = r2 {
            assert!(!e.is_empty());
        }
    }
}

/// `is_decimal` is the `GEO.FRAME.UNITS` authored-decimal grammar (checks 1 to 4). Cross-check
/// on a shared corpus against `fid-geo-frame`, whose `to_si` applies the same checks and then
/// converts: a text is in the grammar exactly when `to_si` succeeds or fails only on overflow
/// (check 5, which FAMILY cannot apply without converting).
#[test]
fn decimal_grammar_agrees_with_geo_frame() {
    use fid_geo_frame::units::to_si;
    use fid_geo_frame::{LengthUnit, Unit, UnitsError};
    use fid_program::family::is_decimal;
    let mut corpus: Vec<String> = [
        "0",
        "-0",
        "+0",
        "120",
        "4.724",
        "-1.5e-3",
        "+2E+10",
        "1e999999",
        "-1e999999",
        "1e-999999",
        "0.000",
        "1.000",
        "00012",
        "1e0",
        "1E-0",
        "",
        "+",
        "-",
        ".",
        "1,5",
        ".5",
        "1.",
        "1e",
        "1e+",
        " 1",
        "1 ",
        "nan",
        "NaN",
        "-inf",
        "Infinity",
        "--1",
        "+-1",
        "0x10",
        "1.2.3",
        "1_000",
        "١",
        "１",
        "1e1.5",
        "e5",
        "1ee5",
        "1.e5",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for n in [39, 40, 41] {
        corpus.push("7".repeat(n));
        corpus.push(format!("0.000{}", "3".repeat(n)));
        corpus.push(format!("{}.{}", "1".repeat(n / 2), "2".repeat(n - n / 2)));
    }
    corpus.push("1".repeat(1024));
    corpus.push("0".repeat(1024));
    corpus.push("0".repeat(1025));
    corpus.push(format!("0.{}1", "0".repeat(1021)));
    for t in &corpus {
        let geo = match to_si(t, Unit::Length(LengthUnit::M)) {
            Ok(_) | Err(UnitsError::Overflow) => true,
            Err(_) => false,
        };
        assert_eq!(is_decimal(t), geo, "{t:?}");
    }
}
