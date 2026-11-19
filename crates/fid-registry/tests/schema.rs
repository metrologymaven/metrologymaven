//! Acceptance criteria of spec node CORE.REG.DEF.SCHEMA (AC-1 .. AC-4) and the pinned rules
//! of its Design. Fixtures: `fixtures/core/reg/def/schema/` (hand-labelled, see
//! `make_fixtures.py` there).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_registry::schema::{canonical_len, MAX_ISSUES, STANDARDS_REF_PATTERN};
use fid_registry::{
    check_schema, check_schema_json, registry_keywords_schema, validate_with, Catalogue, CatalogueError, CommandDecl,
    EntityType, GeoFrameUnits, Limits, ProjectKeyword, QuantityCatalogue, QuantityKinds, Rational, RuleCode,
    SchemaCode, SchemaIssue, SelectorForm, SCHEMA_MAX_BYTES,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fix() -> PathBuf {
    repo().join("fixtures/core/reg/def/schema")
}

fn bytes(rel: &str) -> Vec<u8> {
    std::fs::read(fix().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn geo() -> Catalogue {
    Catalogue::geo_frame().unwrap()
}

fn json(text: &str) -> CValue {
    parse(text.as_bytes()).unwrap_or_else(|e| panic!("{text}: {e}"))
}

/// (code, pointer) pairs of a check.
fn codes(issues: &[SchemaIssue]) -> Vec<(&'static str, String)> {
    issues.iter().map(|i| (i.code.as_str(), i.pointer.clone())).collect()
}

fn check_text(text: &str) -> Vec<(&'static str, String)> {
    codes(&check_schema(&json(text), &geo()))
}

fn one(code: &'static str, pointer: &str) -> Vec<(&'static str, String)> {
    vec![(code, pointer.to_string())]
}

fn sorted_files(dir: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(fix().join(dir))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    files
}

// ------------------------------------------------------------------ AC-1

/// AC-1: the valid schemas use every project keyword and report nothing, through both entry
/// points.
#[test]
fn ac1_valid_fixtures_have_no_issues() {
    let files = sorted_files("valid");
    assert!(files.len() >= 2);
    let mut seen = std::collections::BTreeSet::new();
    for f in files {
        let b = std::fs::read(&f).unwrap();
        assert_eq!(check_schema_json(&b, &geo()), vec![], "{}", f.display());
        assert_eq!(check_schema(&parse(&b).unwrap(), &geo()), vec![], "{}", f.display());
        let text = String::from_utf8(b).unwrap();
        for p in ProjectKeyword::ALL {
            if text.contains(&format!("\"{}\"", p.as_str())) {
                seen.insert(*p);
            }
        }
    }
    assert_eq!(seen.into_iter().collect::<Vec<_>>(), ProjectKeyword::ALL.to_vec(), "every project keyword used");
}

/// AC-1 / REQ-001: the checked-in meta-schema is generated from the Rust tables. Full JSON
/// Schema validation of the fixtures against it: `python tools/check_registry_keywords.py`.
#[test]
fn ac1_meta_schema_is_generated() {
    let mut want = to_canonical_bytes(&registry_keywords_schema()).unwrap();
    want.push(b'\n');
    let have = std::fs::read(repo().join("schemas/registry-keywords.schema.json")).unwrap_or_default();
    assert!(
        have == want,
        "schemas/registry-keywords.schema.json is out of date. Replace its contents with:\n{}",
        String::from_utf8_lossy(&want)
    );
}

/// REQ-001 / REQ-008: the meta-schema admits exactly the seven project keywords, with the
/// closed lists and the standards-reference pattern of the checker.
#[test]
fn ac1_meta_schema_matches_the_checker() {
    let CValue::Obj(meta) = registry_keywords_schema() else { panic!() };
    assert_eq!(meta["unevaluatedProperties"], CValue::Bool(false));
    assert_eq!(meta["$schema"], CValue::Str("https://json-schema.org/draft/2020-12/schema".into()));
    let CValue::Obj(props) = &meta["properties"] else { panic!() };
    let x: Vec<&str> = props.keys().filter(|k| k.starts_with("x-")).map(String::as_str).collect();
    let want: Vec<&str> = ProjectKeyword::ALL.iter().map(|p| p.as_str()).collect();
    assert_eq!(x, want);
    let strs = |v: &CValue| match v {
        CValue::Arr(a) => a
            .iter()
            .map(|s| match s {
                CValue::Str(s) => s.clone(),
                _ => panic!(),
            })
            .collect::<Vec<_>>(),
        _ => panic!(),
    };
    let CValue::Obj(xref) = &props["x-ref"] else { panic!() };
    assert_eq!(strs(&xref["enum"]), EntityType::ALL.iter().map(|e| e.as_str().to_string()).collect::<Vec<_>>());
    let CValue::Obj(xsel) = &props["x-selector"] else { panic!() };
    assert_eq!(strs(&xsel["enum"]), SelectorForm::ALL.iter().map(|e| e.as_str().to_string()).collect::<Vec<_>>());
    let CValue::Obj(xstd) = &props["x-standards-ref"] else { panic!() };
    assert_eq!(xstd["pattern"], CValue::Str(STANDARDS_REF_PATTERN.into()));
    for old in ["$dynamicRef", "$recursiveAnchor", "$recursiveRef", "definitions", "dependencies"] {
        assert_eq!(props[old], CValue::Bool(false), "{old}");
    }
}

// ------------------------------------------------------------------ AC-2

/// AC-2 (error path): each invalid fixture reports exactly its `expect` code and pointer.
#[test]
fn ac2_invalid_fixtures_report_exactly_their_code() {
    let names: Vec<String> =
        sorted_files("invalid").iter().map(|p| p.file_stem().unwrap().to_string_lossy().into_owned()).collect();
    let want = [
        "angle_unit_on_length",
        "depth_65",
        "external_ref",
        "standards_ref_with_text",
        "unknown_quantity",
        "untrusted_on_number",
        "x_foo_keyword",
    ];
    assert_eq!(names, want);
    for name in want {
        let CValue::Obj(case) = parse(&bytes(&format!("invalid/{name}.json"))).unwrap() else { panic!() };
        let CValue::Obj(expect) = &case["expect"] else { panic!() };
        let (CValue::Str(code), CValue::Str(ptr)) = (&expect["code"], &expect["pointer"]) else { panic!() };
        let schema = &case["schema"];
        let got = check_schema(schema, &geo());
        assert_eq!(codes(&got), vec![(code.as_str(), ptr.clone())], "{name}: {got:?}");
        let again = check_schema_json(&to_canonical_bytes(schema).unwrap(), &geo());
        assert_eq!(again, got, "{name}: both entry points agree");
        for i in &got {
            assert!(i.message.bytes().all(|b| (b' '..=b'~').contains(&b)) && i.message.len() <= 512);
        }
    }
}

// ------------------------------------------------------------------ AC-3

/// AC-3: the facade lists `length` exactly as GEO.FRAME.UNITS publishes it, one for one; the
/// catalogue fixture is typed by hand from the factors pinned in that node.
#[test]
fn ac3_length_units_one_for_one() {
    let cat = geo();
    let fixture = Catalogue::from_json(&bytes("catalogue/geo_frame_units.json")).unwrap();
    assert_eq!(cat.kind_names(), vec!["angle", "length"]);
    assert_eq!(cat.kind("length"), fixture.kind("length"));
    assert_eq!(cat.kind("angle"), fixture.kind("angle"));
    let length = cat.kind("length").unwrap();
    assert_eq!(length.canonical, "m");
    // Hand-labelled from spec/geo/frame/units/NODE.md (as written there, before reduction).
    let table: [(&str, u64, u64); 7] = [
        ("ft", 381, 1250),
        ("in", 254, 10000),
        ("m", 1, 1),
        ("mm", 1, 1000),
        ("nm", 1, 1_000_000_000),
        ("thou", 127, 5_000_000),
        ("um", 1, 1_000_000),
    ];
    assert_eq!(length.units.len(), table.len());
    for (u, (sym, n, d)) in length.units.iter().zip(table) {
        assert_eq!(u.symbol, sym);
        assert_eq!(u.factor.ratio, Rational::reduced(n, d).unwrap(), "{sym}");
        assert!(!u.factor.times_pi);
        assert_eq!(cat.factor(sym), Some(u.factor));
        assert!(QuantityCatalogue::accepts(&cat, "length", sym));
        assert!(!QuantityCatalogue::accepts(&cat, "angle", sym));
    }
    let inch = cat.factor("in").unwrap();
    assert_eq!((inch.ratio.num(), inch.ratio.den()), (127, 5000));
    let deg = cat.factor("deg").unwrap();
    assert!(deg.times_pi && deg.ratio == Rational::new(1, 180).unwrap());
    assert!(cat.factor("rad").unwrap().is_one());
    assert_eq!(cat.factor("s"), None);
    assert_eq!(cat.pi_hex(), "c90fdaa22168c234c4c6628b80dc1cd129024e088a67cc74020bbea63b139b22");
}

// ------------------------------------------------------------------ AC-4

/// AC-4 / REQ-005: the digest is stable, equals the SHA-256 computed independently by
/// `make_fixtures.py`, and changes when the catalogue changes. The one-unit change of
/// `one_unit_changed.json` is refused by `from_json` (REQ-004); its digest change is tested
/// on the test-only unchecked path in `catalogue.rs`.
#[test]
fn ac4_digest_stable_and_sensitive() {
    let a = geo();
    let b = geo();
    assert_eq!(a.digest(), b.digest());
    let fixture_bytes = bytes("catalogue/geo_frame_units.json");
    let fixture = Catalogue::from_json(&fixture_bytes).unwrap();
    assert_eq!(fixture.digest(), a.digest());
    assert_eq!(to_canonical_bytes(&a.to_value()).unwrap(), fixture_bytes.strip_suffix(b"\n").unwrap());
    let want = String::from_utf8(bytes("catalogue/geo_frame_units.sha256")).unwrap();
    assert_eq!(a.digest().as_str(), want.trim());
    let timed = Catalogue::from_json(&bytes("catalogue/with_time.json")).unwrap();
    assert_ne!(timed.digest(), a.digest());
    assert_eq!(
        Catalogue::from_json(&bytes("catalogue/one_unit_changed.json")),
        Err(CatalogueError::GeoKindAltered("length"))
    );
}

// ------------------------------------------------------------------ catalogue facade

/// A catalogue with a `time` kind (test data) is served as published, and both the checker
/// and `CORE.REG.VAL` accept `x-quantity: time` through it; the GEO.FRAME.UNITS catalogue
/// does not have the kind.
#[test]
fn time_kind_through_the_facade() {
    let timed = Catalogue::from_json(&bytes("catalogue/with_time.json")).unwrap();
    assert_eq!(timed.kind_names(), vec!["angle", "length", "time"]);
    assert_eq!(timed.kind("length"), geo().kind("length"));
    assert_eq!(timed.factor("min").unwrap().ratio, Rational::new(60, 1).unwrap());
    let schema = json(r#"{"type":"object","properties":{"t":{"type":"number","x-quantity":"time","x-unit":"ms"}}}"#);
    assert_eq!(check_schema(&schema, &timed), vec![]);
    assert_eq!(codes(&check_schema(&schema, &geo())), one("unknown_quantity", "/properties/t/x-quantity"));

    // CORE.REG.VAL takes the facade in place of GeoFrameUnits.
    let file = parse(&std::fs::read(repo().join("fixtures/core/reg/decls/valid.json")).unwrap()).unwrap();
    let CValue::Obj(file) = file else { panic!() };
    let CValue::Arr(decls) = &file["decls"] else { panic!() };
    let mut d = decls[0].clone();
    let CValue::Obj(dm) = &mut d else { panic!() };
    let CValue::Obj(ps) = dm.get_mut("params_schema").unwrap() else { panic!() };
    let CValue::Obj(props) = ps.get_mut("properties").unwrap() else { panic!() };
    props.insert("dwell".into(), json(r#"{"type":"number","x-quantity":"time","x-unit":"s"}"#));
    let decls_v: Vec<CommandDecl> = vec![CommandDecl(d)];
    assert_eq!(validate_with(&decls_v, &Limits::default(), &timed), vec![]);
    let errs = validate_with(&decls_v, &Limits::default(), &GeoFrameUnits);
    assert_eq!(errs.iter().map(|e| e.rule).collect::<Vec<_>>(), vec![RuleCode::UnknownQuantity]);
    assert!(QuantityKinds::has_kind(&timed, "time") && !QuantityKinds::has_kind(&geo(), "time"));
}

/// The facade over GEO.FRAME.UNITS gives VAL exactly the answers of `GeoFrameUnits`.
#[test]
fn geo_facade_equals_geo_frame_units_for_val() {
    let cat = geo();
    for file in ["valid.json", "many_errors.json"] {
        let v = parse(&std::fs::read(repo().join("fixtures/core/reg/decls").join(file)).unwrap()).unwrap();
        let CValue::Obj(o) = v else { panic!() };
        let CValue::Arr(items) = &o["decls"] else { panic!() };
        let decls: Vec<CommandDecl> = items.iter().cloned().map(CommandDecl).collect();
        assert_eq!(
            validate_with(&decls, &Limits::default(), &cat),
            validate_with(&decls, &Limits::default(), &GeoFrameUnits),
            "{file}"
        );
    }
    for kind in ["length", "angle", "time", "Length", ""] {
        assert_eq!(QuantityKinds::has_kind(&cat, kind), GeoFrameUnits.has_kind(kind), "{kind}");
        for unit in ["m", "mm", "deg", "rad", "um", "µm", "M", "s"] {
            assert_eq!(QuantityKinds::accepts(&cat, kind, unit), GeoFrameUnits.accepts(kind, unit), "{kind} {unit}");
        }
    }
}

fn shape_err(text: &str) -> CatalogueError {
    Catalogue::from_json(text.as_bytes()).unwrap_err()
}

/// The GEO.FRAME.UNITS catalogue document as text (canonical, no newline).
fn geo_text() -> String {
    String::from_utf8(bytes("catalogue/geo_frame_units.json")).unwrap().trim_end().to_string()
}

/// The GEO.FRAME.UNITS catalogue with `extra` appended to its kinds (kinds after `length`).
fn cat_text(extra: &str) -> String {
    geo_text().replacen("]}],\"pi_hex\"", &format!("]}},{extra}],\"pi_hex\""), 1)
}

/// REQ-004: a catalogue document must carry every GEO.FRAME.UNITS kind and Π unchanged; it
/// may only add kinds. Each alteration is refused with its typed error.
#[test]
fn catalogue_must_extend_geo_frame_units() {
    let geo = geo_text();
    assert!(Catalogue::from_json(geo.as_bytes()).is_ok());
    let inch = r#"{"den":"5000","num":"127","pi":false,"symbol":"in"}"#;
    let nm = r#"{"den":"1000000000","num":"1","pi":false,"symbol":"nm"}"#;
    let thou = r#",{"den":"5000000","num":"127","pi":false,"symbol":"thou"}"#;
    let pm = r#",{"den":"1000000000000","num":"1","pi":false,"symbol":"pm"}"#;
    let pi = "c90fdaa22168c234c4c6628b80dc1cd129024e088a67cc74020bbea63b139b22";
    for needle in [inch, nm, thou, pi] {
        assert_eq!(geo.matches(needle).count(), 1, "{needle}");
    }
    let altered = CatalogueError::GeoKindAltered("length");
    let cases: Vec<(&str, String, CatalogueError)> = vec![
        ("in = 1/39", geo.replace(inch, r#"{"den":"39","num":"1","pi":false,"symbol":"in"}"#), altered.clone()),
        ("an extra length unit", geo.replace(nm, &format!("{nm}{pm}")), altered.clone()),
        ("a dropped unit", geo.replace(thou, ""), altered.clone()),
        (
            "another canonical unit",
            geo.replace(r#""canonical":"m""#, r#""canonical":"mm""#),
            CatalogueError::Shape(String::new()),
        ),
        ("a changed pi", geo.replace(pi, &pi.replace("9b22", "9b23")), CatalogueError::PiAltered),
        (
            "an extra field in a GEO kind",
            geo.replacen(r#""kinds":[{"canonical":"rad""#, r#""kinds":[{"canonical":"rad","x":1"#, 1),
            CatalogueError::Shape(String::new()),
        ),
    ];
    for (why, text, want) in cases {
        let got = shape_err(&text);
        match want {
            CatalogueError::Shape(_) => assert_eq!(got.code(), "bad_catalogue", "{why}: {got}"),
            want => assert_eq!(got, want, "{why}"),
        }
        assert!(got.to_string().is_ascii(), "{why}");
    }
    // A whole kind missing.
    let start = geo.find(r#"{"canonical":"rad""#).unwrap();
    let end = geo.find(r#"{"canonical":"m""#).unwrap();
    let no_angle = format!("{}{}", &geo[..start], &geo[end..]);
    assert_eq!(shape_err(&no_angle), CatalogueError::GeoKindMissing("angle"));
    assert_eq!(shape_err(&no_angle).code(), "geo_kind_missing");
    // A new kind is fine; its symbols may not reuse a GEO.FRAME.UNITS symbol.
    let time = r#"{"canonical":"s","kind":"time","units":[{"den":"1","num":"1","pi":false,"symbol":"s"}]}"#;
    assert!(Catalogue::from_json(cat_text(time).as_bytes()).is_ok());
    let reuse = r#"{"canonical":"s","kind":"time","units":[{"den":"60","num":"1","pi":false,"symbol":"m"},{"den":"1","num":"1","pi":false,"symbol":"s"}]}"#;
    assert_eq!(shape_err(&cat_text(reuse)).code(), "bad_catalogue");
}

/// Catalogue documents are refused, not repaired: every pinned shape rule fails closed.
#[test]
fn catalogue_refusals() {
    let unit = |sym: &str, n: &str, d: &str| format!(r#"{{"den":"{d}","num":"{n}","pi":false,"symbol":"{sym}"}}"#);
    let kind = |name: &str, canonical: &str, units: &[String]| {
        format!(r#"{{"canonical":"{canonical}","kind":"{name}","units":[{}]}}"#, units.join(","))
    };
    let ok = cat_text(&kind("time", "s", &[unit("ms", "1", "1000"), unit("s", "1", "1")]));
    assert!(Catalogue::from_json(ok.as_bytes()).is_ok(), "{ok}");
    let cases: Vec<(&str, String)> = vec![
        ("unsorted units", cat_text(&kind("time", "s", &[unit("s", "1", "1"), unit("ms", "1", "1000")]))),
        ("not lowest terms", cat_text(&kind("time", "s", &[unit("ms", "2", "2000"), unit("s", "1", "1")]))),
        ("leading zero", cat_text(&kind("time", "s", &[unit("ms", "1", "01000"), unit("s", "1", "1")]))),
        ("zero factor", cat_text(&kind("time", "s", &[unit("s", "1", "1"), unit("z", "0", "1")]))),
        ("u64 overflow", cat_text(&kind("time", "s", &[unit("s", "1", "1"), unit("z", "1", "18446744073709551616")]))),
        ("case-only symbol twin", cat_text(&kind("time", "s", &[unit("S", "1000", "1"), unit("s", "1", "1")]))),
        ("two factor-1 units", cat_text(&kind("time", "s", &[unit("s", "1", "1"), unit("x", "1", "1")]))),
        ("canonical mismatch", cat_text(&kind("time", "ms", &[unit("ms", "1", "1000"), unit("s", "1", "1")]))),
        ("no units", cat_text(&kind("time", "s", &[]))),
        ("bad kind name", cat_text(&kind("tIme", "s", &[unit("s", "1", "1")]))),
        ("non-ascii symbol", cat_text(&kind("time", "s", &[unit("s", "1", "1"), unit("\u{b5}s", "1", "1000000")]))),
        ("unsorted kinds", cat_text(&kind("aaa", "s", &[unit("s", "1", "1")]))),
        ("duplicate kind", cat_text(&kind("length", "s", &[unit("s", "1", "1")]))),
        ("extra field", ok.replacen(r#""format""#, r#""extra":1,"format""#, 1)),
        ("wrong format", ok.replace("fid-quantity-catalogue/1", "fid-quantity-catalogue/2")),
        ("bad pi_hex", ok.replace("c90fdaa2", "C90FDAA2")),
        ("pi_hex top bit clear", ok.replace("c90fdaa2", "490fdaa2")),
        ("num as number", ok.replacen(r#""num":"1""#, r#""num":1"#, 1)),
    ];
    for (why, text) in cases {
        let e = shape_err(&text);
        assert_eq!(e.code(), "bad_catalogue", "{why}: {e}");
        assert!(e.to_string().is_ascii(), "{why}");
    }
    assert_eq!(shape_err(r#"{"format":1,"format":2}"#).code(), "duplicate_key");
    assert_eq!(shape_err("{").code(), "syntax");
    let big = format!("{}{}", ok, " ".repeat(1024 * 1024));
    assert_eq!(Catalogue::from_json(big.as_bytes()).unwrap_err(), CatalogueError::TooLarge);
    let deep = format!("{}{}", "[".repeat(10), "]".repeat(10));
    assert!(Catalogue::from_json(deep.as_bytes()).is_err());
}

// ------------------------------------------------------------------ keyword rules

/// REQ-002: the closed list. Old-draft keywords are `bad_dialect`; anything else outside the
/// list (including case variants and `additionalItems`) is `unknown_keyword`. Keys of
/// `properties`, `x-ui` and data keywords are not keywords.
#[test]
fn closed_keyword_list() {
    assert_eq!(check_text(r#"{"Type":"string"}"#), one("unknown_keyword", "/Type"));
    assert_eq!(check_text(r#"{"additionalItems":false}"#), one("unknown_keyword", "/additionalItems"));
    assert_eq!(check_text(r#"{"x-quantities":"length"}"#), one("unknown_keyword", "/x-quantities"));
    for old in ["definitions", "dependencies", "$recursiveAnchor", "$recursiveRef"] {
        let doc = format!(r#"{{"{old}":{{}}}}"#);
        assert_eq!(check_text(&doc), one("bad_dialect", &format!("/{old}")), "{old}");
    }
    let ok = r#"{"properties":{"x-foo":{"type":"string"}},"x-ui":{"x-foo":1,"widget":"slider"},
        "const":{"x-foo":1},"default":{"x-bar":2},"examples":[{"zzz":1}],"enum":[{"q":1}]}"#;
    assert_eq!(check_text(ok), vec![]);
    // Keywords inside an unknown keyword are data: not walked.
    assert_eq!(check_text(r#"{"x-foo":{"type":"nonsense"}}"#), one("unknown_keyword", "/x-foo"));
}

/// REQ-002 and the VAL conventions for references.
#[test]
fn references() {
    assert_eq!(check_text(r##"{"$ref":"other.json#/a"}"##), one("external_ref", "/$ref"));
    assert_eq!(check_text(r##"{"$ref":"#"}"##), vec![]);
    let ok = r##"{"$defs":{"a/b":{"type":"string"}},"properties":{"x":{"$ref":"#/$defs/a~1b"},"y":{"$ref":"#/properties/x"}}}"##;
    assert_eq!(check_text(ok), vec![]);
    let into_list = r##"{"anyOf":[{"type":"string"}],"properties":{"x":{"$ref":"#/anyOf/0"}}}"##;
    assert_eq!(check_text(into_list), vec![]);
    for (doc, at) in [
        (r##"{"properties":{"x":{"$ref":"#/properties/x/default"},"y":{}}}"##, "/properties/x/$ref"),
        (r##"{"properties":{"x":{"$ref":"#/properties"}}}"##, "/properties/x/$ref"),
        (r##"{"properties":{"x":{"$ref":"#/missing"}}}"##, "/properties/x/$ref"),
        (r##"{"properties":{"x":{"$ref":"#/x-ui"}},"x-ui":{}}"##, "/properties/x/$ref"),
        (r##"{"properties":{"x":{"$ref":"#name"}}}"##, "/properties/x/$ref"),
        (r##"{"properties":{"x":{"$ref":"#/%24defs/a"}}}"##, "/properties/x/$ref"),
        (r##"{"anyOf":[{}],"properties":{"x":{"$ref":"#/anyOf/00"}}}"##, "/properties/x/$ref"),
        (r##"{"properties":{"x":{"$ref":"#/properties/~2"}}}"##, "/properties/x/$ref"),
        (r##"{"properties":{"x":{"$dynamicRef":"#meta"}}}"##, "/properties/x/$dynamicRef"),
        (r##"{"properties":{"x":{"$id":"https://example.com/x"}}}"##, "/properties/x/$id"),
    ] {
        assert_eq!(check_text(doc), one("unwalked_ref", at), "{doc}");
    }
    // A reference into an unknown keyword lands on data.
    let r = check_text(r##"{"x-foo":{"type":"string"},"properties":{"x":{"$ref":"#/x-foo"}}}"##);
    assert_eq!(r, vec![("unwalked_ref", "/properties/x/$ref".to_string()), ("unknown_keyword", "/x-foo".to_string())]);
    assert_eq!(check_text(r#"{"$ref":5}"#), one("bad_schema", "/$ref"));
    assert_eq!(check_text(r#"{"$id":"https://example.com/s.json"}"#), vec![]);
    assert_eq!(check_text(r#"{"$id":"https://example.com/s.json#frag"}"#), one("bad_schema", "/$id"));
}

/// REQ-003: catalogue membership; `x-unit` against the sibling `x-quantity`.
#[test]
fn quantities_and_units() {
    assert_eq!(check_text(r#"{"type":"number","x-quantity":"length","x-unit":"in"}"#), vec![]);
    assert_eq!(check_text(r#"{"type":"string","x-quantity":"angle","x-unit":"gon"}"#), vec![]);
    assert_eq!(check_text(r#"{"x-quantity":"Length"}"#), one("unknown_quantity", "/x-quantity"));
    assert_eq!(check_text(r#"{"x-quantity":"length","x-unit":"MM"}"#), one("unit_not_accepted", "/x-unit"));
    assert_eq!(check_text(r#"{"x-unit":"mm"}"#), one("unit_not_accepted", "/x-unit"));
    // The parent's x-quantity does not govern a subschema's x-unit.
    let nested = r#"{"x-quantity":"length","properties":{"a":{"x-unit":"mm"}}}"#;
    assert_eq!(check_text(nested), one("unit_not_accepted", "/properties/a/x-unit"));
    // An invalid x-quantity is reported once; its x-unit is not judged.
    assert_eq!(check_text(r#"{"x-quantity":"lenght","x-unit":"mm"}"#), one("unknown_quantity", "/x-quantity"));
    assert_eq!(check_text(r#"{"x-quantity":3}"#), one("bad_keyword_value", "/x-quantity"));
    assert_eq!(check_text(r#"{"x-quantity":"length","x-unit":3}"#), one("bad_keyword_value", "/x-unit"));
    // Numbers without x-quantity are CORE.REG.VAL's rule, not this checker's.
    assert_eq!(check_text(r#"{"type":"number"}"#), vec![]);
}

/// REQ-006 to REQ-008: values of the other project keywords.
#[test]
fn project_keyword_values() {
    assert_eq!(check_text(r#"{"type":"string","x-untrusted":true}"#), vec![]);
    assert_eq!(check_text(r#"{"type":["null","string"],"x-untrusted":true}"#), vec![]);
    for t in [r#""integer""#, r#"["string","number"]"#, r#"["null"]"#, "[]"] {
        let doc = format!(r#"{{"type":{t},"x-untrusted":true}}"#);
        let got = check_text(&doc);
        assert!(got.contains(&("untrusted_on_non_string", "/x-untrusted".to_string())), "{doc}: {got:?}");
    }
    assert_eq!(check_text(r#"{"x-untrusted":true}"#), one("untrusted_on_non_string", "/x-untrusted"));
    assert_eq!(check_text(r#"{"type":"string","x-untrusted":"yes"}"#), one("bad_keyword_value", "/x-untrusted"));
    for e in EntityType::ALL {
        assert_eq!(check_text(&format!(r#"{{"type":"string","x-ref":"{e}"}}"#)), vec![]);
    }
    assert_eq!(check_text(r#"{"x-ref":"Feature"}"#), one("bad_keyword_value", "/x-ref"));
    assert_eq!(check_text(r#"{"x-ref":["feature"]}"#), one("bad_keyword_value", "/x-ref"));
    for f in SelectorForm::ALL {
        assert_eq!(check_text(&format!(r#"{{"x-selector":"{f}"}}"#)), vec![]);
    }
    assert_eq!(check_text(r#"{"x-selector":"all"}"#), one("bad_keyword_value", "/x-selector"));
    assert_eq!(check_text(r#"{"x-ui":"slider"}"#), one("bad_keyword_value", "/x-ui"));
    assert_eq!(check_text(r#"{"x-standards-ref":"ASME-Y14.5:2018 cl. 7"}"#), vec![]);
    assert_eq!(check_text(r#"{"x-standards-ref":["ASME-Y14.5:2018"]}"#), one("bad_standards_ref", "/x-standards-ref"));
    let long = format!(r#"{{"x-standards-ref":"ISO-1101:2017 cl. {}"}}"#, "1".repeat(17));
    assert_eq!(check_text(&long), one("bad_standards_ref", "/x-standards-ref"));
    // x-ui never changes validation: a schema inside it is data.
    assert_eq!(check_text(r#"{"x-ui":{"schema":{"type":"bogus","x-foo":1}}}"#), vec![]);
}

/// Standard keyword values, the dialect and property names (VAL conventions).
#[test]
fn standard_keyword_values() {
    let cases: [(&str, &str, &str); 20] = [
        (r#"{"type":"float"}"#, "bad_schema", "/type"),
        (r#"{"type":["string","string"]}"#, "bad_schema", "/type"),
        (r#"{"type":[]}"#, "bad_schema", "/type"),
        (r#"{"items":[{}]}"#, "bad_schema", "/items"),
        (r#"{"allOf":[]}"#, "bad_schema", "/allOf"),
        (r#"{"properties":[]}"#, "bad_schema", "/properties"),
        (r#"{"not":"x"}"#, "bad_schema", "/not"),
        (r#"{"minLength":-1}"#, "bad_schema", "/minLength"),
        (r#"{"maxItems":1.5}"#, "bad_schema", "/maxItems"),
        (r#"{"multipleOf":0}"#, "bad_schema", "/multipleOf"),
        (r#"{"minimum":"0"}"#, "bad_schema", "/minimum"),
        (r#"{"uniqueItems":1}"#, "bad_schema", "/uniqueItems"),
        (r#"{"pattern":1}"#, "bad_schema", "/pattern"),
        (r#"{"enum":{}}"#, "bad_schema", "/enum"),
        (r#"{"required":["a","a"]}"#, "bad_schema", "/required"),
        (r#"{"$anchor":"1a"}"#, "bad_schema", "/$anchor"),
        (r#"{"$vocabulary":{"x":1}}"#, "bad_schema", "/$vocabulary"),
        (r#"{"$schema":"http://json-schema.org/draft-07/schema#"}"#, "bad_dialect", "/$schema"),
        (r#"{"not":{"$schema":"https://json-schema.org/draft/2020-12/schema"}}"#, "bad_dialect", "/not/$schema"),
        (r#"{"required":["a b"]}"#, "bad_property_name", "/required"),
    ];
    for (doc, code, at) in cases {
        assert_eq!(check_text(doc), one(code, at), "{doc}");
    }
    assert_eq!(check_text(r#"{"properties":{"\u200bid":{}}}"#), one("bad_property_name", "/properties"));
    assert_eq!(check_text(r#"{"dependentRequired":{"a":["b","b"]}}"#), one("bad_schema", "/dependentRequired/a"));
    assert_eq!(check_text(r#"{"properties":{"a":{},"A":{}}}"#), one("duplicate_key", "/properties"));
    assert_eq!(check_text(r#"{"patternProperties":{"^a$":{},"^A$":{}}}"#), vec![]);
    // Integers above 2^53 are not numbers (no rounding through f64).
    let big = CValue::Obj(BTreeMap::from([("maximum".to_string(), CValue::U64((1 << 53) + 1))]));
    assert_eq!(codes(&check_schema(&big, &geo())), one("bad_schema", "/maximum"));
    let edge = CValue::Obj(BTreeMap::from([("maxLength".to_string(), CValue::U64(1 << 53))]));
    assert_eq!(check_schema(&edge, &geo()), vec![]);
    let sub = CValue::Obj(BTreeMap::from([("minimum".to_string(), CValue::Num(f64::MIN_POSITIVE / 4.0))]));
    assert_eq!(codes(&check_schema(&sub, &geo())), one("bad_number", "/minimum"));
    let nan = CValue::Obj(BTreeMap::from([("default".to_string(), CValue::Num(f64::NAN))]));
    assert_eq!(codes(&check_schema(&nan, &geo())), one("bad_number", "/default"));
    assert_eq!(codes(&check_schema(&CValue::Bool(true), &geo())), one("bad_schema", ""));
}

/// Echoed input is escaped, cut and printable ASCII.
#[test]
fn echoes_are_bounded_and_escaped() {
    let key = format!("x-\u{202e}{}", "k".repeat(5000));
    let doc = CValue::Obj(BTreeMap::from([(key, CValue::Null)]));
    let got = check_schema(&doc, &geo());
    assert_eq!(got.len(), 1);
    let i = &got[0];
    assert_eq!(i.code, SchemaCode::UnknownKeyword);
    assert!(i.message.len() <= 512 && i.message.bytes().all(|b| (b' '..=b'~').contains(&b)), "{}", i.message);
    assert!(i.pointer.len() <= 512 && i.pointer.starts_with("/x-\\u{202e}"), "{}", i.pointer);
    assert!(i.to_string().is_ascii());
    assert!(matches!(i.to_value(), CValue::Obj(_)));
}

// ------------------------------------------------------------------ REQ-009 and bounds

/// REQ-009: 64 levels pass, 65 do not; reported once, at the first container too deep.
#[test]
fn depth_limit() {
    fn nested(levels: usize) -> String {
        format!("{}{{}}{}", r#"{"not":"#.repeat(levels - 1), "}".repeat(levels - 1))
    }
    assert_eq!(check_text(&nested(64)), vec![]);
    assert_eq!(check_text(&nested(65)), one("schema_too_deep", &"/not".repeat(64)));
    assert_eq!(check_text(&nested(100)), one("schema_too_deep", &"/not".repeat(64)));
    // Arrays count as levels too, and data is measured like schemas.
    let arrays = format!(r#"{{"default":{}{}}}"#, "[".repeat(64), "]".repeat(64));
    assert_eq!(check_text(&arrays), one("schema_too_deep", &format!("/default{}", "/0".repeat(63))));
    // Bytes deeper than fid-canon's 128 levels are refused before parsing.
    let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
    assert_eq!(codes(&check_schema_json(deep.as_bytes(), &geo())), one("schema_too_deep", ""));
    // A deep in-memory value (built iteratively) does not overflow the stack.
    let mut v = CValue::Obj(BTreeMap::new());
    for _ in 0..1000 {
        v = CValue::Obj(BTreeMap::from([("not".to_string(), v)]));
    }
    assert_eq!(codes(&check_schema(&v, &geo())), one("schema_too_deep", &"/not".repeat(64)));
}

/// REQ-009: 1 MiB, for bytes and for values (measured as their canonical form).
#[test]
fn size_limit() {
    let fill = |n: usize| format!(r#"{{"description":"{}"}}"#, "a".repeat(n));
    let at_limit = fill(SCHEMA_MAX_BYTES - r#"{"description":""}"#.len());
    assert_eq!(at_limit.len(), SCHEMA_MAX_BYTES);
    assert_eq!(check_schema_json(at_limit.as_bytes(), &geo()), vec![]);
    assert_eq!(check_schema(&json(&at_limit), &geo()), vec![]);
    let over = fill(SCHEMA_MAX_BYTES - r#"{"description":""}"#.len() + 1);
    assert_eq!(codes(&check_schema_json(over.as_bytes(), &geo())), one("schema_too_large", ""));
    assert_eq!(codes(&check_schema(&json(&over), &geo())), one("schema_too_large", ""));
    // Whitespace counts for bytes but not for the canonical form of a value.
    let spaced = format!("{}{}", at_limit, " ");
    assert_eq!(codes(&check_schema_json(spaced.as_bytes(), &geo())), one("schema_too_large", ""));
    assert_eq!(canonical_len(&json(&at_limit), usize::MAX), SCHEMA_MAX_BYTES);
}

/// Byte-level refusals of `check_schema_json`.
#[test]
fn byte_refusals() {
    let r = |t: &str| codes(&check_schema_json(t.as_bytes(), &geo()));
    assert_eq!(r("{"), one("bad_json", ""));
    assert_eq!(r("\u{feff}{}"), one("bad_json", ""));
    assert_eq!(r(r#"{"type":"string","type":"number"}"#), one("duplicate_key", ""));
    assert_eq!(r(r#"{"maximum":1e400}"#), one("bad_number", ""));
    assert_eq!(r(r#"{"maximum":9007199254740993}"#), one("bad_number", ""));
    assert_eq!(r(r#"{"maximum":1e-400}"#), one("bad_number", ""));
    assert_eq!(r(r#"{"maximum":9007199254740992}"#), vec![]);
    assert_eq!(r("[]"), one("bad_schema", ""));
}

/// At most 10,000 issues plus one `too_many_issues`: exactly the sorted first 10,000,
/// whatever order the walk finds them in; the output is sorted and the same for the same
/// input.
#[test]
fn issue_cap_and_order() {
    let mut m: BTreeMap<String, CValue> = (0..MAX_ISSUES + 50).map(|i| (format!("x-k{i:06}"), CValue::Null)).collect();
    let got = check_schema(&CValue::Obj(m.clone()), &geo());
    assert_eq!(got.len(), MAX_ISSUES + 1);
    assert_eq!(got[0].code, SchemaCode::TooManyIssues);
    assert_eq!(got[0].pointer, "");
    assert_eq!(got[1].pointer, "/x-k000000");
    assert_eq!(got[MAX_ISSUES].pointer, format!("/x-k{:06}", MAX_ISSUES - 1));

    // Found last (references resolve after the walk) but sorted first: it must be kept, and
    // the largest unknown keyword is the one evicted.
    m.insert("properties".into(), json(r##"{"a":{"$ref":"#/nowhere"}}"##));
    let got = check_schema(&CValue::Obj(m), &geo());
    assert_eq!(got.len(), MAX_ISSUES + 1);
    assert_eq!(
        codes(&got[..2]),
        vec![("too_many_issues", String::new()), ("unwalked_ref", "/properties/a/$ref".into())]
    );
    assert_eq!(got[MAX_ISSUES].pointer, format!("/x-k{:06}", MAX_ISSUES - 2));

    // Found in reverse pointer order: "/a" (and its child) is walked before "/a!", but
    // "/a!..." sorts first; with more than the cap, the kept set is still the sorted first.
    let mut rev: BTreeMap<String, CValue> = BTreeMap::new();
    rev.insert("properties".into(), {
        let mut p: BTreeMap<String, CValue> = BTreeMap::new();
        p.insert("a".into(), json(r#"{"x-zzz":1}"#));
        for i in 0..MAX_ISSUES {
            p.insert(format!("a!{i:05}"), json(r#"{"x-q":1}"#));
        }
        CValue::Obj(p)
    });
    let got = check_schema(&CValue::Obj(rev), &geo());
    assert_eq!(got.len(), MAX_ISSUES + 1);
    assert!(got.iter().all(|i| i.pointer != "/properties/a/x-zzz"));
    assert_eq!(got[MAX_ISSUES].pointer, format!("/properties/a!{:05}/x-q", MAX_ISSUES - 1));
    let doc = json(
        r##"{"x-b":1,"properties":{"z":{"x-unit":"mm","$ref":"x.json"},"a":{"x-ref":"no"}},"x-a":2,"type":"bad"}"##,
    );
    let got = check_schema(&doc, &geo());
    let keys: Vec<(String, &str)> = got.iter().map(|i| (i.pointer.clone(), i.code.as_str())).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    assert_eq!(check_schema(&doc, &geo()), got);
    assert_eq!(
        codes(&got),
        vec![
            ("bad_keyword_value", "/properties/a/x-ref".to_string()),
            ("external_ref", "/properties/z/$ref".to_string()),
            ("unit_not_accepted", "/properties/z/x-unit".to_string()),
            ("bad_schema", "/type".to_string()),
            ("unknown_keyword", "/x-a".to_string()),
            ("unknown_keyword", "/x-b".to_string()),
        ]
    );
}

/// Performance budget: 5,000 schemas; prints the time (the < 1 s budget is for release builds).
#[test]
fn perf_5000_schemas() {
    let docs: Vec<CValue> =
        sorted_files("valid").iter().map(|f| parse(&std::fs::read(f).unwrap()).unwrap()).cycle().take(5000).collect();
    let cat = geo();
    let t = Instant::now();
    let mut n = 0;
    for d in &docs {
        n += check_schema(d, &cat).len();
    }
    assert_eq!(n, 0);
    println!("5000 schemas checked in {:?}", t.elapsed());
}
