//! Acceptance criteria of spec node AUTO.PROG.EXPR.TYPES. Programs, the manifest, the
//! catalogue and every expected type and diagnostic come from
//! `fixtures/program/types/make_fixtures.py` (hand-labelled from the spec's rules); nothing
//! here derives an expected value from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{digest_bytes, parse, to_canonical_bytes, CValue};
use fid_program::expr::types::{assignable, kind_dim, value_of_record, KIND_DIMS, LIBRARY, NO_DIMENSION_KINDS};
use fid_program::{check_types, parse_expr, type_of, DiagCode, Dim, Kind, Program, Type, TypeReport};
use fid_registry::{Catalogue, Factor, KindInfo, Manifest, QuantityCatalogue};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn expected() -> BTreeMap<String, CValue> {
    let v = parse(&std::fs::read(root().join("fixtures/program/types/expected.json")).unwrap()).unwrap();
    let CValue::Obj(m) = v else { panic!("expected.json root") };
    m
}

fn path_of(key: &str) -> PathBuf {
    let CValue::Str(rel) = expected().get(key).cloned().unwrap() else { panic!("{key}") };
    root().join(rel)
}

fn catalogue() -> Catalogue {
    Catalogue::from_json(&std::fs::read(path_of("catalogue")).unwrap()).unwrap()
}

fn manifest() -> Manifest {
    Manifest::from_json(&std::fs::read(path_of("manifest")).unwrap()).unwrap()
}

fn cases() -> BTreeMap<String, CValue> {
    let Some(CValue::Obj(c)) = expected().get("cases").cloned() else { panic!("cases") };
    c
}

fn program(name: &str) -> Program {
    let p = root().join("fixtures/program/types").join(format!("{name}.json"));
    Program::parse(&std::fs::read(&p).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn run(name: &str) -> TypeReport {
    check_types(&program(name), &manifest(), &catalogue())
}

fn bytes(v: &CValue) -> String {
    String::from_utf8(to_canonical_bytes(v).unwrap()).unwrap()
}

fn assert_case(name: &str) {
    let want = cases().get(name).cloned().unwrap_or_else(|| panic!("no expected report for {name}"));
    let got = run(name);
    assert_eq!(bytes(&got.to_value()), bytes(&want), "{name}");
}

#[test]
fn manifest_names_the_catalogue() {
    assert_eq!(manifest().quantity_catalogue_digest, catalogue().digest());
}

/// AC-1: every valid program has no diagnostic and the expected types.
#[test]
fn ac1_valid_programs() {
    let dir = root().join("fixtures/program/types/valid");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter_map(|f| f.strip_suffix(".json").map(str::to_string))
        .collect();
    names.sort();
    assert_eq!(names, ["area", "bore"]);
    for n in names {
        let name = format!("valid/{n}");
        let r = run(&name);
        assert!(r.is_clean(), "{name}: {:?}", r.diagnostics);
        assert!(r.types.values().all(|t| t.not_of_record), "{name}");
        assert_case(&name);
    }
}

/// AC-2: `d1 + 5[deg]` with `d1` a length.
#[test]
fn ac2_unit_mismatch() {
    let r = run("unit-mismatch");
    assert_eq!(r.diagnostics.len(), 1);
    let d = &r.diagnostics[0];
    assert_eq!(d.code, DiagCode::UnitMismatch);
    assert_eq!(d.names, ["length", "angle"]);
    assert!(d.step.is_some());
    assert_case("unit-mismatch");
}

/// AC-3: `w * h` is an area, `w * h + w` a mismatch.
#[test]
fn ac3_area() {
    let r = run("valid/area");
    let area = r.types.values().find(|t| t.ty == Type::Quantity(Kind::Named("area".into())));
    assert!(area.is_some());
    let r = run("area-mismatch");
    assert_eq!(r.diagnostics.len(), 1);
    assert_eq!(r.diagnostics[0].code, DiagCode::UnitMismatch);
    assert_case("area-mismatch");
}

/// AC-4: an expression for a `value_of_record` parameter.
#[test]
fn ac4_value_of_record() {
    let r = run("value-of-record");
    let codes: Vec<_> = r.diagnostics.iter().map(|d| (d.code, d.slot.as_str())).collect();
    assert_eq!(
        codes,
        [(DiagCode::ExpressionValueOfRecord, "args.nominal"), (DiagCode::ExpressionValueOfRecord, "args.result")]
    );
    assert_case("value-of-record");
}

/// AC-5: an unknown unit; a binding used after its block.
#[test]
fn ac5_unknown_unit_and_scope() {
    assert_eq!(run("unknown-unit").diagnostics[0].code, DiagCode::UnknownUnit);
    assert_case("unknown-unit");
    assert_eq!(run("out-of-block").diagnostics[0].code, DiagCode::Undeclared);
    assert_case("out-of-block");
}

/// REQ-005 and REQ-007 error paths, and REQ-001 for unknown functions and commands.
#[test]
fn other_diagnostics() {
    assert_case("redeclared");
    assert_case("type-mismatch");
}

/// Enum equality and assignment take members only; read-only names, loop bounds, a record
/// of a major the manifest lacks, a kind the catalogue lacks, and a subroutine that reads
/// `main`'s bindings (review of PR #418).
#[test]
fn enums_and_scoping() {
    assert_case("enums");
    assert_case("scoping");
    let r = run("scoping");
    let codes: Vec<_> = r.diagnostics.iter().map(|d| (d.code.as_str(), d.slot.as_str())).collect();
    assert_eq!(
        codes,
        [
            ("type_unknown", "variables.r"),
            ("type_unknown", "variables.k"),
            ("type_mismatch", "from"),
            ("type_mismatch", "to"),
            ("read_only", "var"),
            ("read_only", "var"),
            ("undeclared", "expr"),
        ]
    );
}

/// A catalogue other than the manifest's: one declaration diagnostic naming both digests,
/// nothing typed.
#[test]
fn catalogue_digest_mismatch() {
    let geo_bytes = std::fs::read(root().join("fixtures/core/reg/def/schema/catalogue/geo_frame_units.json")).unwrap();
    let geo = Catalogue::from_json(&geo_bytes).unwrap();
    let r = check_types(&program("valid/bore"), &manifest(), &geo);
    assert!(r.types.is_empty());
    assert_eq!(r.diagnostics.len(), 1);
    let d = &r.diagnostics[0];
    assert_eq!(d.code, DiagCode::CatalogueMismatch);
    assert_eq!(d.step, None);
    let canonical = to_canonical_bytes(&parse(&geo_bytes).unwrap()).unwrap();
    assert_eq!(d.names, [manifest().quantity_catalogue_digest.to_string(), digest_bytes(&canonical).to_string()]);
}

/// A catalogue that gives one symbol to two kinds is refused, never resolved last-wins.
struct DupCatalogue {
    kinds: Vec<KindInfo>,
    digest: fid_types::Digest,
}

impl QuantityCatalogue for DupCatalogue {
    fn kind_names(&self) -> Vec<&str> {
        self.kinds.iter().map(|k| k.name.as_str()).collect()
    }
    fn kind(&self, name: &str) -> Option<&KindInfo> {
        self.kinds.iter().find(|k| k.name == name)
    }
    fn factor(&self, unit: &str) -> Option<Factor> {
        self.kinds.iter().flat_map(|k| &k.units).find(|u| u.symbol == unit).map(|u| u.factor)
    }
    fn digest(&self) -> fid_types::Digest {
        self.digest.clone()
    }
}

#[test]
fn duplicate_unit_symbol_refused() {
    let base = catalogue();
    let mut kinds: Vec<KindInfo> = base.kind_names().iter().map(|n| base.kind(n).unwrap().clone()).collect();
    let mm =
        kinds.iter().find(|k| k.name == "length").unwrap().units.iter().find(|u| u.symbol == "mm").unwrap().clone();
    kinds.iter_mut().find(|k| k.name == "time").unwrap().units.push(mm);
    let dup = DupCatalogue { kinds, digest: manifest().quantity_catalogue_digest };
    let r = check_types(&program("valid/bore"), &manifest(), &dup);
    assert!(r.types.is_empty());
    assert_eq!(r.diagnostics.len(), 1);
    assert_eq!(r.diagnostics[0].code, DiagCode::CatalogueInvalid);
    assert_eq!(r.diagnostics[0].names, ["mm"]);
    let (t, d) = type_of(&parse_expr("1[s]").unwrap(), &BTreeMap::new(), &dup);
    assert_eq!(t, None);
    assert_eq!(d[0].code, DiagCode::CatalogueInvalid);
}

fn json(s: &str) -> CValue {
    parse(s.as_bytes()).unwrap()
}

/// `value_of_record` is found through `items`, `allOf` and local `$ref`s, and fails closed.
#[test]
fn value_of_record_nested() {
    let root = json(r##"{"$defs":{"nom":{"type":"number","value_of_record":true},"plain":{"type":"number"}}}"##);
    let vor = |s: &str| value_of_record(&json(s), &root, 0);
    assert!(vor(r#"{"type":"number","value_of_record":true}"#));
    assert!(vor(r#"{"type":"string","x-ref":"result"}"#));
    assert!(vor(r#"{"items":{"type":"number","value_of_record":true},"type":"array"}"#));
    assert!(vor(r#"{"allOf":[{"type":"number"},{"value_of_record":true}]}"#));
    assert!(vor(r##"{"$ref":"#/$defs/nom"}"##));
    assert!(vor(r##"{"items":{"$ref":"#/$defs/nom"},"type":"array"}"##));
    assert!(vor(r##"{"$ref":"#/$defs/missing"}"##));
    assert!(vor(r#"{"$ref":"https://example.org/x.json"}"#));
    assert!(vor(r#"{"properties":{"a":{"value_of_record":true}},"type":"object"}"#));
    assert!(!vor(r##"{"$ref":"#/$defs/plain"}"##));
    assert!(!vor(r#"{"items":{"type":"number"},"type":"array"}"#));
    assert!(!vor(r#"{"type":"number","value_of_record":false}"#));
}

/// Every kind of every catalogue in the workspace has a pinned dimension or is listed as
/// having none on purpose.
#[test]
fn workspace_kinds_have_dimensions() {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "json") && p.metadata().is_ok_and(|m| m.len() <= 1024 * 1024) {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(&root().join("fixtures"), &mut files);
    let mut kinds: Vec<String> = Catalogue::geo_frame().unwrap().kind_names().iter().map(|s| s.to_string()).collect();
    let mut docs = 0;
    for f in files {
        let bytes = std::fs::read(&f).unwrap();
        let needle: &[u8] = b"\"fid-quantity-catalogue/1\"";
        if !bytes.windows(needle.len()).any(|w| w == needle) {
            continue;
        }
        if let Ok(c) = Catalogue::from_json(&bytes) {
            docs += 1;
            kinds.extend(c.kind_names().iter().map(|s| s.to_string()));
        }
    }
    assert!(docs >= 4, "found {docs} catalogue documents");
    for k in kinds {
        assert!(kind_dim(&k).is_some() || NO_DIMENSION_KINDS.contains(&k.as_str()), "kind {k} has no dimension");
    }
    assert_eq!(kind_dim("dimensionless"), Some(Dim::NONE));
}

/// `dimensionless` is a kind, not a plain number; an empty product is `decimal`.
#[test]
fn dimensionless_kind() {
    let mut v = Catalogue::geo_frame().unwrap().to_value();
    let CValue::Obj(m) = &mut v else { panic!() };
    let Some(CValue::Arr(ks)) = m.get_mut("kinds") else { panic!() };
    ks.insert(
        1,
        json(r#"{"canonical":"1","kind":"dimensionless","units":[{"den":"1","num":"1","pi":false,"symbol":"1"},{"den":"1000000","num":"1","pi":false,"symbol":"ppm"}]}"#),
    );
    let cat = Catalogue::from_json(&to_canonical_bytes(&v).unwrap()).unwrap();
    let vars = vars(&[("r", Type::Quantity(Kind::Named("dimensionless".into()))), ("l", len())]);
    let ty = |src: &str| {
        let (t, d) = type_of(&parse_expr(src).unwrap(), &vars, &cat);
        (t.map(|t| t.to_string()), d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>())
    };
    assert_eq!(ty("r * l"), (Some("quantity<length>".into()), vec![]));
    assert_eq!(ty("l / l"), (Some("decimal".into()), vec![]));
    assert_eq!(ty("r + 1"), (None, vec!["unit_mismatch"]));
    assert_eq!(ty("r + 3[ppm]"), (Some("quantity<dimensionless>".into()), vec![]));
    assert!(!assignable(&Type::Decimal, &Type::Quantity(Kind::Named("dimensionless".into()))));
}

/// AC-6: the same program and manifest give byte-identical reports; the committed expected
/// bytes pin them across platforms (CI runs this on Linux, Windows and macOS).
#[test]
fn ac6_deterministic() {
    for name in cases().keys() {
        let a = to_canonical_bytes(&run(name).to_value()).unwrap();
        let b = to_canonical_bytes(&run(name).to_value()).unwrap();
        assert_eq!(a, b, "{name}");
    }
    assert_eq!(cases().len(), 11);
}

fn vars(pairs: &[(&str, Type)]) -> BTreeMap<String, Type> {
    pairs.iter().map(|(n, t)| (n.to_string(), t.clone())).collect()
}

fn len() -> Type {
    Type::Quantity(Kind::Named("length".into()))
}

/// The parser's deepest tree (a flat chain near the token bound) types without recursion.
#[test]
fn deep_chain() {
    let src = vec!["x"; 500].join("+");
    let e = parse_expr(&src).unwrap();
    let (t, d) = type_of(&e, &vars(&[("x", len())]), &catalogue());
    assert!(d.is_empty());
    assert_eq!(t, Some(len()));
    let src = vec!["x"; 500].join("*");
    let e = parse_expr(&src).unwrap();
    let (t, d) = type_of(&e, &vars(&[("x", len())]), &catalogue());
    // Exponent 128 does not fit `i8`: reported, never wrapped.
    assert_eq!(t, None);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].code, DiagCode::TypeUnknown);
    assert_eq!(d[0].names, ["dimension_overflow"]);
}

/// Hand-labelled single expressions (REQ-002, REQ-003, the library signatures).
#[test]
fn expressions() {
    let cat = catalogue();
    let v = vars(&[
        ("l", len()),
        ("a", Type::Quantity(Kind::Named("angle".into()))),
        ("t", Type::Quantity(Kind::Named("time".into()))),
        ("n", Type::Int),
        ("x", Type::Decimal),
        ("xs", Type::List(Box::new(len()))),
        ("s", Type::Str),
    ]);
    let ok = |src: &str, want: &str| {
        let (t, d) = type_of(&parse_expr(src).unwrap(), &v, &cat);
        assert!(d.is_empty(), "{src}: {d:?}");
        assert_eq!(t.map(|t| t.to_string()).as_deref(), Some(want), "{src}");
    };
    let bad = |src: &str, code: DiagCode, names: &[&str]| {
        let (t, d) = type_of(&parse_expr(src).unwrap(), &v, &cat);
        assert_eq!(t, None, "{src}");
        assert_eq!(d.len(), 1, "{src}: {d:?}");
        assert_eq!(d[0].code, code, "{src}");
        assert_eq!(d[0].names, names, "{src}");
    };
    ok("n + 1", "int");
    ok("n + 1.5", "decimal");
    ok("n / 2", "decimal");
    ok("floor(n / 2)", "int");
    ok("n % 2", "int");
    ok("l * 2", "quantity<length>");
    ok("2 * l", "quantity<length>");
    ok("l / t", "quantity<dim:m*s^-1>");
    ok("1 / t", "quantity<dim:s^-1>");
    ok("l * l * l / (l * l)", "quantity<length>");
    ok("l / l", "decimal");
    ok("a / t", "quantity<dim:s^-1*rad>");
    ok("l / t < l / t", "bool");
    ok("-l", "quantity<length>");
    ok("min(l, 2[mm])", "quantity<length>");
    ok("max(n, 2)", "int");
    ok("floor(x)", "int");
    ok("sqrt(x)", "decimal");
    ok("tan(a)", "decimal");
    ok("atan2(l, 2[mm])", "quantity<angle>");
    ok("len(s)", "int");
    ok("contains(xs, l)", "bool");
    ok("format(\"{}\", l, n)", "string");
    ok("xs[n]", "quantity<length>");
    ok("if n > 1 then l else 3[mm]", "quantity<length>");
    ok("if n > 1 then n else x", "decimal");
    ok("s == \"a\" or not (n != 2)", "bool");
    ok("to_unit(l, \"in\")", "quantity<length>");
    bad("l + 1", DiagCode::UnitMismatch, &["length", "number"]);
    bad("l < a", DiagCode::UnitMismatch, &["length", "angle"]);
    bad("max(l, a)", DiagCode::UnitMismatch, &["length", "angle"]);
    bad("l / t + l / t", DiagCode::TypeMismatch, &["quantity of a catalogue kind", "quantity<dim:m*s^-1>"]);
    bad("sqrt(l)", DiagCode::TypeMismatch, &["quantity with even exponents", "quantity<length>"]);
    bad("sin(l)", DiagCode::UnitMismatch, &["angle", "length"]);
    bad("to_unit(l, \"deg\")", DiagCode::UnitMismatch, &["length", "angle"]);
    bad("to_unit(l, \"furlong\")", DiagCode::UnknownUnit, &["furlong"]);
    bad("abs(1, 2)", DiagCode::TypeMismatch, &["abs/1", "abs/2"]);
    bad("xs[x]", DiagCode::TypeMismatch, &["int", "decimal"]);
    bad("n.field", DiagCode::TypeUnknown, &["int", "field"]);
    bad("not n", DiagCode::TypeMismatch, &["bool", "int"]);
    bad("zz + 1", DiagCode::Undeclared, &["zz"]);
    bad("2[µm]", DiagCode::UnknownUnit, &["µm"]);
}

/// REQ-007: only `int` widens to `decimal`; an enum fits a superset.
#[test]
fn assignability() {
    let e = |v: &[&str]| Type::Enum(v.iter().map(|s| s.to_string()).collect());
    assert!(assignable(&Type::Decimal, &Type::Int));
    assert!(!assignable(&Type::Int, &Type::Decimal));
    assert!(assignable(&e(&["a", "b"]), &e(&["a"])));
    assert!(!assignable(&e(&["a"]), &e(&["a", "b"])));
    assert!(!assignable(&e(&["a"]), &Type::Str));
    assert!(!assignable(&Type::List(Box::new(Type::Decimal)), &Type::List(Box::new(Type::Int))));
}

#[test]
fn tables_sorted() {
    assert!(KIND_DIMS.windows(2).all(|w| w[0].0 < w[1].0));
    assert!(LIBRARY.windows(2).all(|w| w[0].0 < w[1].0));
    assert_eq!(Dim::ANGLE.to_string(), "rad");
}
