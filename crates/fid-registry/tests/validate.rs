//! Acceptance criteria of spec node CORE.REG.VAL (AC-1 .. AC-5) and the pinned strictness
//! rules of its Design. Fixtures: `fixtures/core/reg/decls/` (hand-labelled, see
//! `make_fixtures.py` there).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_registry::{
    errors_to_value, validate, validate_json, validate_with, CommandDecl, CommandDef, GeoFrameUnits, InputError,
    Limits, RuleCode, SideEffect, ValidationError,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

type Obj = BTreeMap<String, CValue>;
type Codes = Vec<(&'static str, Option<String>)>;
type Pairs = Vec<(&'static str, CValue)>;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/core/reg/decls")
}

fn read(rel: &str) -> Obj {
    match parse(&std::fs::read(root().join(rel)).unwrap()).unwrap() {
        CValue::Obj(o) => o,
        _ => panic!("{rel}: object expected"),
    }
}

fn run_file(o: &Obj) -> Result<Vec<ValidationError>, InputError> {
    let bytes = to_canonical_bytes(&CValue::Obj(o.clone())).unwrap();
    validate_json(&bytes, &Limits::default(), &GeoFrameUnits)
}

fn decls_of(o: &Obj) -> Vec<CValue> {
    match &o["decls"] {
        CValue::Arr(a) => a.clone(),
        _ => panic!("decls must be an array"),
    }
}

/// A valid declaration (W2, simple schemas) from `valid.json`.
fn base() -> Obj {
    let all = decls_of(&read("valid.json"));
    let found = all.into_iter().find(|d| matches!(d, CValue::Obj(m) if m["id"] == s("align.run_bestfit")));
    match found {
        Some(CValue::Obj(m)) => m,
        _ => panic!("align.run_bestfit missing from valid.json"),
    }
}

fn s(x: &str) -> CValue {
    CValue::Str(x.to_string())
}

fn n(x: f64) -> CValue {
    CValue::Num(x)
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn run(decls: Vec<Obj>) -> Vec<ValidationError> {
    let decls: Vec<CommandDecl> = decls.into_iter().map(|d| CommandDecl::from_value(CValue::Obj(d))).collect();
    validate(&decls)
}

fn codes(errors: &[ValidationError]) -> Codes {
    errors.iter().map(|e| (e.rule.as_str(), e.field.clone())).collect()
}

fn one(d: Obj) -> Codes {
    codes(&run(vec![d]))
}

fn at(code: &'static str, field: &str) -> Codes {
    vec![(code, Some(field.to_string()))]
}

fn set_param(d: &mut Obj, name: &str, schema: CValue) {
    let CValue::Obj(p) = d.get_mut("params_schema").unwrap() else { panic!() };
    let CValue::Obj(props) = p.get_mut("properties").unwrap() else { panic!() };
    props.insert(name.to_string(), schema);
}

fn length() -> Pairs {
    vec![("type", s("number")), ("x-quantity", s("length")), ("x-unit", s("mm"))]
}

fn schema(pairs: Pairs) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

// ------------------------------------------------------------------ AC-1

/// AC-1: the valid fixture has one declaration per side-effect class and no error, through
/// both entry points; every declaration also parses as a DEF record.
#[test]
fn ac1_valid_fixture_has_no_errors() {
    let file = read("valid.json");
    assert_eq!(run_file(&file).unwrap(), vec![]);
    let decls = decls_of(&file);
    let typed: Vec<CommandDecl> = decls.iter().cloned().map(CommandDecl::from_value).collect();
    assert_eq!(validate(&typed), vec![]);
    let mut classes = BTreeSet::new();
    for d in decls {
        classes.insert(CommandDef::from_value(d).unwrap().side_effect);
    }
    for c in SideEffect::ALL {
        assert!(classes.contains(c), "no declaration of class {c}");
    }
}

// ------------------------------------------------------------------ AC-2

/// AC-2: each invalid file reports exactly the rule code in its `expect` field.
#[test]
fn ac2_each_invalid_file_reports_only_its_rule() {
    let mut names: Vec<String> = std::fs::read_dir(root().join("invalid"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    let want = [
        "a0_request_only.json",
        "bad_id.json",
        "duplicate_major.json",
        "long_tool_name.json",
        "m1_exposed.json",
        "missing_field.json",
        "number_without_quantity.json",
        "tool_name_collision.json",
        "w3_exposed.json",
    ];
    assert_eq!(names, want);
    for name in names {
        let mut o = read(&format!("invalid/{name}"));
        let Some(CValue::Str(expect)) = o.remove("expect") else { panic!("{name}: no expect") };
        let errors = run_file(&o).unwrap();
        assert!(!errors.is_empty(), "{name}: no error");
        for e in &errors {
            assert_eq!(e.rule.as_str(), expect, "{name}: {e}");
        }
    }
}

// ------------------------------------------------------------------ AC-3

/// AC-3: 12 errors in shuffled order; two runs are byte-identical and sorted per REQ-008.
#[test]
fn ac3_many_errors_sorted_and_reproducible() {
    let mut o = read("many_errors.json");
    let Some(CValue::Arr(expect)) = o.remove("expect_errors") else { panic!() };
    let first = run_file(&o).unwrap();
    let second = run_file(&o).unwrap();
    let bytes = |e: &[ValidationError]| to_canonical_bytes(&errors_to_value(e)).unwrap();
    assert_eq!(bytes(&first), bytes(&second));
    assert_eq!(first.len(), 12);
    let got: Vec<CValue> = first
        .iter()
        .map(|e| {
            CValue::Arr(vec![
                s(&e.command),
                n(f64::from(e.major)),
                s(e.rule.as_str()),
                e.field.as_deref().map_or(CValue::Null, s),
            ])
        })
        .collect();
    assert_eq!(got, expect);

    // The order of the declarations does not matter.
    let mut reversed = decls_of(&o);
    reversed.reverse();
    o.insert("decls".into(), CValue::Arr(reversed));
    assert_eq!(bytes(&run_file(&o).unwrap()), bytes(&first));
}

// ------------------------------------------------------------------ AC-4

/// AC-4: a 64-character tool name is accepted, 65 is `tool_name_too_long`; `_vN` counts.
#[test]
fn ac4_tool_name_length_boundary() {
    let with = |id: String, major: f64| {
        let mut d = base();
        d.insert("id".into(), CValue::Str(id));
        d.insert("major".into(), n(major));
        one(d)
    };
    // "a." + 62 -> "a_" + 62 = 64.
    assert_eq!(with(format!("a.{}", "b".repeat(62)), 1.0), vec![]);
    assert_eq!(with(format!("a.{}", "b".repeat(63)), 1.0), at("tool_name_too_long", "/id"));
    // "a." + 59 at major 2 -> "a_" + 59 + "_v2" = 64.
    assert_eq!(with(format!("a.{}", "b".repeat(59)), 2.0), vec![]);
    assert_eq!(with(format!("a.{}", "b".repeat(60)), 2.0), at("tool_name_too_long", "/id"));
}

// ------------------------------------------------------------------ AC-5

/// AC-5: a 100 MB file fails closed with `input_too_large` within 2 s.
#[test]
fn ac5_huge_input_is_refused() {
    let mut big = b"{\"decls\":[".to_vec();
    big.resize(100_000_000, b' ');
    let t = Instant::now();
    assert_eq!(validate_json(&big, &Limits::default(), &GeoFrameUnits), Err(InputError::InputTooLarge));
    assert!(t.elapsed() < Duration::from_secs(2));
}

/// AC-5: a schema nested 10,000 deep fails closed with `schema_too_deep` within 2 s.
#[test]
fn ac5_deep_schema_is_refused() {
    let mut text = String::from("{\"decls\":[{\"id\":\"a.b\",\"params_schema\":");
    text.push_str(&"{\"not\":".repeat(10_000));
    text.push_str("true");
    text.push_str(&"}".repeat(10_000));
    text.push_str("}]}");
    let t = Instant::now();
    let got = validate_json(text.as_bytes(), &Limits::default(), &GeoFrameUnits);
    assert_eq!(got, Err(InputError::SchemaTooDeep));
    assert_eq!(got.unwrap_err().code(), "schema_too_deep");
    assert!(t.elapsed() < Duration::from_secs(2));
}

/// Builds `{"not": ... {"type": "string"} ...}` with `levels` objects.
fn nested(levels: usize) -> CValue {
    let mut v = obj([("type", s("string"))]);
    for _ in 1..levels {
        v = obj([("not", v)]);
    }
    v
}

/// AC-5 / Design: the depth limit is 64 levels with the schema object as level 1, the same
/// in the file and in the typed entry point.
#[test]
fn ac5_depth_boundary() {
    for (levels, ok) in [(64, true), (65, false)] {
        let mut d = base();
        d.insert("params_schema".into(), nested(levels));
        let typed = one(d.clone());
        let file = run_file(&BTreeMap::from([("decls".to_string(), CValue::Arr(vec![CValue::Obj(d)]))]));
        if ok {
            assert_eq!(typed, vec![], "{levels}");
            assert_eq!(file.unwrap(), vec![], "{levels}");
        } else {
            assert_eq!(typed, at("schema_too_deep", "/params_schema"), "{levels}");
            assert_eq!(file, Err(InputError::SchemaTooDeep), "{levels}");
        }
    }
    // A tighter configured limit.
    let mut d = base();
    d.insert("params_schema".into(), nested(4));
    let limits = Limits { max_schema_depth: 3, ..Limits::default() };
    let errors = validate_with(&[CommandDecl::from_value(CValue::Obj(d))], &limits, &GeoFrameUnits);
    assert_eq!(codes(&errors), at("schema_too_deep", "/params_schema"));
}

/// AC-5: seeded byte mutations of the valid file never panic and always give a typed result.
#[test]
fn ac5_seeded_mutations_never_panic() {
    let good = std::fs::read(root().join("valid.json")).unwrap();
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let alphabet = b"{}[]\":,0123456789.eE-+ \\aZ\x00\xff";
    for _ in 0..300 {
        let mut b = good.clone();
        for _ in 0..1 + next() % 4 {
            let i = (next() % b.len() as u64) as usize;
            b[i] = alphabet[(next() % alphabet.len() as u64) as usize];
        }
        match validate_json(&b, &Limits::default(), &GeoFrameUnits) {
            Ok(errors) => assert!(errors.windows(2).all(|w| w[0].command <= w[1].command)),
            Err(e) => assert!(!e.code().is_empty()),
        }
    }
}

// ------------------------------------------------------------------ REQ-001 .. REQ-007

/// REQ-001: one error per missing field, naming the command.
#[test]
fn req001_names_command_and_each_missing_field() {
    let mut d = base();
    for f in ["major", "params_schema", "side_effect"] {
        d.remove(f);
    }
    let errors = run(vec![d]);
    assert_eq!(
        codes(&errors),
        vec![
            ("missing_field", Some("/major".to_string())),
            ("missing_field", Some("/params_schema".to_string())),
            ("missing_field", Some("/side_effect".to_string())),
        ]
    );
    assert!(errors.iter().all(|e| e.command == "align.run_bestfit" && e.major == 0));
}

/// REQ-002: ID syntax and major range (1..=99999, an integer).
#[test]
fn req002_id_and_major() {
    for bad in ["align", "Align.run", "align.Run", "align_x.run", "align.run.x", "align.run@1", "align.-run"] {
        let mut d = base();
        d.insert("id".into(), s(bad));
        assert_eq!(one(d), at("bad_id", "/id"), "{bad}");
    }
    for bad in [0.0, -1.0, 1.5, 100_000.0, -0.0] {
        let mut d = base();
        d.insert("major".into(), n(bad));
        assert_eq!(one(d), at("bad_id", "/major"), "{bad}");
    }
    let mut d = base();
    d.insert("major".into(), n(99_999.0));
    assert_eq!(one(d), vec![]);
    let mut d = base();
    d.insert("major".into(), s("1"));
    assert_eq!(one(d), at("bad_type", "/major"));
    let mut d = base();
    d.insert("major".into(), CValue::U64(1));
    assert_eq!(one(d), at("bad_type", "/major"));
    let mut d = base();
    d.insert("id".into(), n(1.0));
    assert_eq!(one(d), at("bad_type", "/id"));
}

/// REQ-004: every class at every projection: looser than the class maximum is an error,
/// equal or stricter is accepted.
#[test]
fn req004_projection_table() {
    let order = ["never", "request_only", "exposed"];
    let max = [
        ("R0", 2),
        ("R1", 2),
        ("I1", 2),
        ("W1", 2),
        ("W2", 2),
        ("W3", 1),
        ("X1", 1),
        ("D1", 1),
        ("M1", 0),
        ("S1", 0),
        ("A0", 0),
    ];
    for (class, m) in max {
        for (i, p) in order.iter().enumerate() {
            let mut d = base();
            d.insert("side_effect".into(), s(class));
            d.insert("mcp_projection".into(), s(p));
            let want = if i > m { at("projection_too_loose", "/mcp_projection") } else { vec![] };
            assert_eq!(one(d), want, "{class} {p}");
        }
    }
}

/// REQ-005: number fields anywhere in the schema need `x-quantity`; `integer` does not
/// (pinned, see Open questions); patternProperties keys may differ only in case.
#[test]
fn req005_quantity_in_every_subschema_position() {
    let bare = || obj([("type", s("number"))]);
    let cases = [
        (obj([("type", s("array")), ("items", bare())]), "/params_schema/properties/x/items"),
        (obj([("allOf", CValue::Arr(vec![bare()]))]), "/params_schema/properties/x/allOf/0"),
        (obj([("not", bare())]), "/params_schema/properties/x/not"),
        (obj([("$defs", obj([("d", bare())]))]), "/params_schema/properties/x/$defs/d"),
        (obj([("patternProperties", obj([("^a$", bare())]))]), "/params_schema/properties/x/patternProperties/^a$"),
        (obj([("type", CValue::Arr(vec![s("null"), s("number")]))]), "/params_schema/properties/x"),
        (obj([("additionalProperties", bare())]), "/params_schema/properties/x/additionalProperties"),
    ];
    for (sch, ptr) in cases {
        let mut d = base();
        set_param(&mut d, "x", sch);
        assert_eq!(one(d), at("missing_quantity", ptr), "{ptr}");
    }
    // A property literally named "default" or "const" is still a schema.
    let mut d = base();
    set_param(&mut d, "default", bare());
    assert_eq!(one(d), at("missing_quantity", "/params_schema/properties/default"));
    // Data keywords are not schemas.
    let mut d = base();
    set_param(&mut d, "x", schema(vec![("type", s("string")), ("examples", CValue::Arr(vec![bare()]))]));
    assert_eq!(one(d), vec![]);
    let mut d = base();
    set_param(&mut d, "n", obj([("type", s("integer"))]));
    assert_eq!(one(d), vec![]);
}

/// REQ-005 / Design: `x-quantity` must name a catalogue kind, `x-unit` must be one of its units.
#[test]
fn req005_quantity_kind_and_unit() {
    let mut d = base();
    set_param(&mut d, "t", schema(vec![("type", s("number")), ("x-quantity", s("time"))]));
    assert_eq!(one(d), at("unknown_quantity", "/params_schema/properties/t/x-quantity"));
    let mut d = base();
    set_param(&mut d, "t", schema(vec![("type", s("number")), ("x-quantity", s("Length"))]));
    assert_eq!(one(d), at("unknown_quantity", "/params_schema/properties/t/x-quantity"));
    let mut d = base();
    set_param(&mut d, "t", schema(vec![("type", s("number")), ("x-quantity", s("length")), ("x-unit", s("deg"))]));
    assert_eq!(one(d), at("unit_not_accepted", "/params_schema/properties/t/x-unit"));
    let mut d = base();
    set_param(&mut d, "t", schema(vec![("type", s("string")), ("x-unit", s("mm"))]));
    assert_eq!(one(d), at("unit_not_accepted", "/params_schema/properties/t/x-unit"));
    let mut d = base();
    set_param(&mut d, "t", schema(vec![("type", s("number")), ("x-quantity", n(1.0))]));
    assert_eq!(one(d), at("bad_schema", "/params_schema/properties/t/x-quantity"));
    for u in ["m", "mm", "um", "nm", "in", "ft", "thou"] {
        let mut d = base();
        set_param(&mut d, "t", schema(vec![("type", s("number")), ("x-quantity", s("length")), ("x-unit", s(u))]));
        assert_eq!(one(d), vec![], "{u}");
    }
    for u in ["rad", "deg", "arcmin", "arcsec", "gon"] {
        let mut d = base();
        set_param(&mut d, "t", schema(vec![("type", s("number")), ("x-quantity", s("angle")), ("x-unit", s(u))]));
        assert_eq!(one(d), vec![], "{u}");
    }
}

/// REQ-006 / REQ-007: the same ID at two majors is neither a duplicate nor a collision.
#[test]
fn req006_req007_majors_coexist() {
    let mut a = base();
    let mut b = base();
    a.insert("major".into(), n(1.0));
    b.insert("major".into(), n(2.0));
    assert_eq!(run(vec![a.clone(), b]), vec![]);
    let errors = run(vec![a.clone(), a.clone(), a]);
    assert_eq!(codes(&errors), vec![("duplicate_command", None)]);
    assert!(errors[0].message.starts_with("3 declarations"));
}

// ------------------------------------------------------------------ strictness

/// Unknown fields, case-only duplicate keys and exact duplicate keys are refused.
#[test]
fn unknown_and_duplicate_keys() {
    let mut d = base();
    d.insert("ID".into(), s("align.run_bestfit"));
    assert_eq!(one(d), vec![("duplicate_key", Some(String::new())), ("unknown_field", Some("/ID".to_string()))]);
    let mut d = base();
    set_param(&mut d, "Tolerance", obj([("type", s("string"))]));
    assert_eq!(one(d), at("duplicate_key", "/params_schema/properties"));
    let mut d = base();
    set_param(&mut d, "x", schema(vec![("type", s("string")), ("Type", s("number"))]));
    assert_eq!(one(d), at("duplicate_key", "/params_schema/properties/x"));
    let mut d = base();
    d.insert("lifecycle".into(), obj([("state", s("active")), ("State", s("removed")), ("since", s("2026-01-01"))]));
    assert_eq!(one(d), at("duplicate_key", "/lifecycle"));

    let text = br#"{"decls":[{"id":"a.b","id":"a.c"}]}"#;
    assert_eq!(validate_json(text, &Limits::default(), &GeoFrameUnits), Err(InputError::Json("duplicate_key")));
    let text = br#"{"decls":[],"Decls":[]}"#;
    assert_eq!(validate_json(text, &Limits::default(), &GeoFrameUnits).unwrap_err().code(), "bad_file");
    let text = br#"[]"#;
    assert_eq!(validate_json(text, &Limits::default(), &GeoFrameUnits).unwrap_err().code(), "bad_file");
    let text = br#"{"decls":{}}"#;
    assert_eq!(validate_json(text, &Limits::default(), &GeoFrameUnits).unwrap_err().code(), "bad_file");
    let text = br#"{"decls":[1]}"#;
    let errors = validate_json(text, &Limits::default(), &GeoFrameUnits).unwrap();
    assert_eq!(codes(&errors), vec![("bad_type", None)]);
}

/// Types and enum values of the policy fields; nothing is coerced.
#[test]
fn types_and_enums() {
    for (f, v, code) in [
        ("side_effect", s("r0"), "bad_enum"),
        ("side_effect", s("R0 "), "bad_enum"),
        ("mcp_projection", s("Exposed"), "bad_enum"),
        ("mcp_projection", s("named"), "bad_enum"),
        ("determinism", s("D4"), "bad_enum"),
        ("determinism", n(0.0), "bad_type"),
        ("side_effect", CValue::Null, "bad_type"),
        ("params_schema", CValue::Bool(true), "bad_type"),
        ("result_schema", CValue::Arr(vec![]), "bad_type"),
    ] {
        let mut d = base();
        d.insert(f.into(), v);
        assert_eq!(one(d), at(code, &format!("/{f}")), "{f}");
    }
}

/// The DEF record is the backstop for the fields VAL does not check itself.
#[test]
fn def_backstop() {
    let mut d = base();
    d.insert("algorithms".into(), CValue::Arr(vec![s("fit.cylinder@1")]));
    assert_eq!(one(d), at("bad_algorithm", "/algorithms"));
    let mut d = base();
    d.insert("web".into(), s("partial"));
    assert_eq!(one(d), at("web_note_missing", "/web_note"));
    let mut d = base();
    d.insert("title".into(), n(1.0));
    assert_eq!(one(d), at("bad_type", "/title"));
    let mut d = base();
    d.insert("network".into(), s("lan"));
    assert_eq!(one(d), at("bad_enum", "/network"));
    let mut d = base();
    d.remove("web");
    assert_eq!(one(d), at("missing_field", "/web"));
}

/// NaN, infinity and subnormals are refused wherever they occur; literals that underflow
/// are refused at the byte level.
#[test]
fn non_finite_and_subnormal_numbers() {
    for (v, ptr) in [
        (f64::NAN, "/params_schema/properties/tolerance/maximum"),
        (f64::INFINITY, "/params_schema/properties/tolerance/maximum"),
        (f64::MIN_POSITIVE / 2.0, "/params_schema/properties/tolerance/maximum"),
    ] {
        let mut d = base();
        let mut t = length();
        t.push(("maximum", n(v)));
        set_param(&mut d, "tolerance", schema(t));
        assert_eq!(one(d), at("bad_number", ptr), "{v}");
    }
    let mut d = base();
    d.insert("lifecycle".into(), obj([("state", s("active")), ("since", s("2026-01-01")), ("x", n(-5e-324))]));
    let errors = one(d);
    assert_eq!(errors[0], ("bad_number", Some("/lifecycle/x".to_string())));
    let mut d = base();
    d.insert("major".into(), n(f64::NAN));
    assert!(one(d).contains(&("bad_number", Some("/major".to_string()))));

    for lit in ["1e-400", "2.5e-310", "-4e-320", "1e400"] {
        let text = format!("{{\"decls\":[{{\"x\":{lit}}}]}}");
        let got = validate_json(text.as_bytes(), &Limits::default(), &GeoFrameUnits);
        assert_eq!(got.unwrap_err().code(), "bad_number", "{lit}");
    }
    // Zero written as zero is fine.
    let text = br#"{"decls":[{"x":0e-400}]}"#;
    assert!(validate_json(text, &Limits::default(), &GeoFrameUnits).is_ok());
}

/// Schema keyword values: types, ranges and references.
#[test]
fn schema_keyword_values() {
    let p = "/params_schema/properties/t";
    let cases: [(Pairs, Codes); 13] = [
        (vec![("type", s("Number"))], at("bad_schema", &format!("{p}/type"))),
        (vec![("type", CValue::Arr(vec![]))], at("bad_schema", &format!("{p}/type"))),
        (vec![("type", CValue::Arr(vec![s("string"), s("string")]))], at("bad_schema", &format!("{p}/type"))),
        (vec![("type", s("string")), ("minLength", n(-1.0))], at("bad_schema", &format!("{p}/minLength"))),
        (vec![("type", s("string")), ("minLength", n(1.5))], at("bad_schema", &format!("{p}/minLength"))),
        (vec![("type", s("string")), ("minLength", n(3.0)), ("maxLength", n(2.0))], at("bad_schema", p)),
        (vec![("type", s("array")), ("items", n(1.0))], at("bad_schema", &format!("{p}/items"))),
        (vec![("anyOf", CValue::Arr(vec![]))], at("bad_schema", &format!("{p}/anyOf"))),
        (vec![("properties", CValue::Arr(vec![]))], at("bad_schema", &format!("{p}/properties"))),
        (vec![("required", CValue::Arr(vec![s("a"), s("a")]))], at("bad_schema", &format!("{p}/required"))),
        (vec![("$ref", s("https://example.org/s.json"))], at("external_ref", &format!("{p}/$ref"))),
        (vec![("$ref", s("other.json#/x"))], at("external_ref", &format!("{p}/$ref"))),
        (vec![("$ref", s("#"))], vec![]),
    ];
    for (pairs, want) in cases {
        let mut d = base();
        set_param(&mut d, "t", schema(pairs.clone()));
        assert_eq!(one(d), want, "{pairs:?}");
    }
    let num = |extra: Pairs| {
        let mut t = length();
        t.extend(extra);
        let mut d = base();
        set_param(&mut d, "t", schema(t));
        one(d)
    };
    assert_eq!(num(vec![("minimum", n(2.0)), ("maximum", n(1.0))]), at("bad_schema", p));
    assert_eq!(num(vec![("minimum", n(1.0)), ("maximum", n(1.0))]), vec![]);
    assert_eq!(num(vec![("minimum", n(1.0)), ("exclusiveMaximum", n(1.0))]), at("bad_schema", p));
    assert_eq!(num(vec![("exclusiveMinimum", n(1.0)), ("maximum", n(1.0))]), at("bad_schema", p));
    assert_eq!(num(vec![("exclusiveMinimum", n(0.0)), ("exclusiveMaximum", n(0.0))]), at("bad_schema", p));
    assert_eq!(num(vec![("multipleOf", n(0.0))]), at("bad_schema", &format!("{p}/multipleOf")));
    assert_eq!(num(vec![("multipleOf", n(-1.0))]), at("bad_schema", &format!("{p}/multipleOf")));
    assert_eq!(num(vec![("minimum", s("0"))]), at("bad_schema", &format!("{p}/minimum")));
}

/// Every decision is deterministic and bounded: more than MAX_ERRORS errors end the run
/// with `too_many_errors`.
#[test]
fn error_cap() {
    let mut d = base();
    d.remove("determinism");
    d.remove("side_effect");
    let decls: Vec<CommandDecl> = (0..6_000)
        .map(|i| {
            let mut x = d.clone();
            x.insert("id".into(), s(&format!("a.b{i}")));
            CommandDecl::from_value(CValue::Obj(x))
        })
        .collect();
    let errors = validate(&decls);
    assert_eq!(errors.len(), fid_registry::validate::MAX_ERRORS + 1);
    assert_eq!(errors[0].rule, RuleCode::TooManyErrors);
    assert_eq!(validate(&decls), errors);
}

/// A parsed definition round-trips through the typed entry point.
#[test]
fn from_def_round_trip() {
    let def = CommandDef::from_value(CValue::Obj(base())).unwrap();
    assert_eq!(validate(&[CommandDecl::from_def(&def)]), vec![]);
}

/// Messages and fields echo hostile text only in bounded form.
#[test]
fn echoed_text_is_bounded() {
    let mut d = base();
    let long = "x".repeat(100_000);
    d.insert("id".into(), s(&long));
    d.insert(long.clone(), CValue::Null);
    for e in run(vec![d]) {
        assert!(e.command.len() <= 128, "{}", e.rule);
        assert!(e.field.as_deref().unwrap_or_default().len() <= 512, "{}", e.rule);
        assert!(e.message.len() <= 512, "{}", e.rule);
    }
}

fn printable(t: &str) -> bool {
    t.bytes().all(|b| (0x20..=0x7e).contains(&b))
}

/// Review of #233, items 2-4: hostile keys and IDs reach messages, fields and `Display`
/// only escaped (printable ASCII), never raw; an invalid id is not echoed as `command`.
#[test]
fn hostile_text_is_escaped() {
    let evil = "a\n\u{202e}\u{1b}[31m\"\\\u{200b}\u{0}z";
    let mut d = base();
    d.insert("id".into(), s(evil));
    d.insert(evil.to_string(), CValue::Null);
    d.insert("lifecycle".into(), obj([("state", s("active")), (evil, CValue::Null), ("State", CValue::Null)]));
    let errors = run(vec![d]);
    assert!(!errors.is_empty());
    for e in &errors {
        assert_eq!(e.command, "", "{}", e.rule);
        assert!(printable(&e.message), "{:?}", e.message);
        assert!(printable(e.field.as_deref().unwrap_or_default()), "{:?}", e.field);
        assert!(printable(&e.to_string()), "{e}");
        assert!(!e.message.contains('\u{202e}') && !e.message.contains('\u{1b}'));
    }
    let unknown = errors.iter().find(|e| e.rule == RuleCode::UnknownField).unwrap();
    assert_eq!(unknown.field.as_deref(), Some("/a\\u{a}\\u{202e}\\u{1b}[31m\"\\\\u{200b}\\u{0}z"));
    assert!(unknown.message.contains("\\\\u{202e}"), "{}", unknown.message);
    let bad_id = errors.iter().find(|e| e.rule == RuleCode::BadId).unwrap();
    assert_eq!(bad_id.to_string().split('@').next(), Some("\"\""));

    // A hostile key inside a precondition goes through the DEF backstop: fixed message only.
    let mut d = base();
    let pre = obj([("expr", s("true")), ("bound", n(1.0)), ("message_key", s("a.b")), (evil, CValue::Null)]);
    d.insert("preconditions".into(), CValue::Arr(vec![pre]));
    let errors = run(vec![d]);
    assert_eq!(codes(&errors), at("bad_precondition", "/preconditions"));
    assert_eq!(errors[0].message, "a precondition is malformed");

    // Valid ids are echoed and printed quoted.
    let mut d = base();
    d.insert("side_effect".into(), s("M1"));
    let e = &run(vec![d])[0];
    assert!(e.to_string().starts_with("\"align.run_bestfit\"@1: projection_too_loose"), "{e}");
}

/// Review of #233, item 1: a local `$ref` must land on a position the walk checks as a schema.
#[test]
fn refs_must_land_on_walked_schemas() {
    let p = "/params_schema/properties/a";
    let bare = || obj([("type", s("number"))]);
    let with = |extra: Vec<(&str, CValue)>, r: &str| {
        let mut d = base();
        let CValue::Obj(ps) = d.get_mut("params_schema").unwrap() else { panic!() };
        for (k, v) in extra {
            ps.insert(k.to_string(), v);
        }
        set_param(&mut d, "a", obj([("$ref", s(r))]));
        one(d)
    };
    // The reported hole: a number schema under an unknown keyword.
    assert_eq!(with(vec![("x-h", bare())], "#/x-h"), at("unwalked_ref", &format!("{p}/$ref")));
    // Data keywords, maps and missing targets are not schemas.
    assert_eq!(with(vec![("default", bare())], "#/default"), at("unwalked_ref", &format!("{p}/$ref")));
    assert_eq!(with(vec![], "#/properties"), at("unwalked_ref", &format!("{p}/$ref")));
    assert_eq!(with(vec![], "#/$defs/missing"), at("unwalked_ref", &format!("{p}/$ref")));
    assert_eq!(with(vec![], "#/properties/tolerance/x-unit"), at("unwalked_ref", &format!("{p}/$ref")));
    // Plain-name anchors and percent-encoded pointers are refused.
    assert_eq!(with(vec![], "#len"), at("unwalked_ref", &format!("{p}/$ref")));
    assert_eq!(with(vec![], "#/properties/t%6flerance"), at("unwalked_ref", &format!("{p}/$ref")));
    // Walked positions are fine, and their numbers are checked where they are defined.
    let defs = obj([("len", obj([("type", s("number")), ("x-quantity", s("length"))]))]);
    assert_eq!(with(vec![("$defs", defs.clone())], "#/$defs/len"), vec![]);
    assert_eq!(with(vec![], "#/properties/tolerance"), vec![]);
    assert_eq!(with(vec![], "#"), vec![]);
    let slash = obj([("a/b~c", obj([("type", s("string"))]))]);
    assert_eq!(with(vec![("$defs", slash)], "#/$defs/a~1b~0c"), vec![]);
    let all = CValue::Arr(vec![obj([("type", s("string"))])]);
    assert_eq!(with(vec![("allOf", all.clone())], "#/allOf/0"), vec![]);
    assert_eq!(with(vec![("allOf", all.clone())], "#/allOf/00"), at("unwalked_ref", &format!("{p}/$ref")));
    assert_eq!(with(vec![("allOf", all)], "#/allOf/1"), at("unwalked_ref", &format!("{p}/$ref")));
    // A ref resolves inside its own schema: params cannot reach into result.
    assert_eq!(with(vec![], "#/properties/angle"), at("unwalked_ref", &format!("{p}/$ref")));
    // $dynamicRef is refused outright; an embedded $id is refused because it moves the base.
    let mut d = base();
    set_param(&mut d, "a", obj([("$dynamicRef", s("#/$defs/len"))]));
    assert_eq!(one(d), at("unwalked_ref", &format!("{p}/$dynamicRef")));
    let mut d = base();
    set_param(&mut d, "a", obj([("$id", s("https://example.org/a")), ("type", s("string"))]));
    assert_eq!(one(d), at("unwalked_ref", &format!("{p}/$id")));
}

/// Review of #233, item 6: names under `properties` and `$defs` are printable ASCII
/// without space.
#[test]
fn property_names_are_printable_ascii() {
    for bad in ["", "a b", "to\u{200b}lerance", "\u{202e}x", "tab\t", "caf\u{e9}"] {
        let mut d = base();
        set_param(&mut d, bad, obj([("type", s("string"))]));
        assert_eq!(one(d), at("bad_property_name", "/params_schema/properties"), "{bad:?}");
    }
    let mut d = base();
    let CValue::Obj(ps) = d.get_mut("params_schema").unwrap() else { panic!() };
    ps.insert("$defs".into(), obj([("l\u{200d}en", obj([("type", s("string"))]))]));
    assert_eq!(one(d), at("bad_property_name", "/params_schema/$defs"));
    // Regex keys of patternProperties are exempt (they are checked as regexes elsewhere).
    let mut d = base();
    let CValue::Obj(ps) = d.get_mut("params_schema").unwrap() else { panic!() };
    ps.insert("patternProperties".into(), obj([("^a b$", obj([("type", s("string"))]))]));
    assert_eq!(one(d), vec![]);
}

/// Review of #233, item 5: 64-bit integers are checked exactly, never rounded through f64.
#[test]
fn big_integers_are_exact() {
    let p = "/params_schema/properties/t";
    let two53: u64 = 1 << 53;
    for (k, v, ok) in [
        ("maxLength", CValue::U64(two53), true),
        ("maxLength", CValue::U64(two53 + 1), false),
        ("maxLength", CValue::I64(-1), false),
        ("maxLength", CValue::U64(u64::MAX), false),
        ("minLength", CValue::I64(i64::MIN), false),
    ] {
        let mut d = base();
        set_param(&mut d, "t", schema(vec![("type", s("string")), (k, v.clone())]));
        let want = if ok { vec![] } else { at("bad_schema", &format!("{p}/{k}")) };
        assert_eq!(one(d), want, "{k} {v:?}");
    }
    for (v, ok) in
        [(CValue::I64(-(1 << 53)), true), (CValue::I64(-(1 << 53) - 1), false), (CValue::U64(two53 + 2), false)]
    {
        let mut t = length();
        t.push(("minimum", v.clone()));
        let mut d = base();
        set_param(&mut d, "t", schema(t));
        let want = if ok { vec![] } else { at("bad_schema", &format!("{p}/minimum")) };
        assert_eq!(one(d), want, "{v:?}");
    }
    // Integer literals beyond 2^53 would round when parsed: refused at the byte level.
    for (lit, ok) in [("9007199254740992", true), ("9007199254740993", false), ("-9007199254740993", false)] {
        let text = format!("{{\"decls\":[{{\"x\":{lit}}}]}}");
        let got = validate_json(text.as_bytes(), &Limits::default(), &GeoFrameUnits);
        assert_eq!(got.is_ok(), ok, "{lit}");
    }
}

/// Design performance budget (5,000 declarations in < 1 s on reference hardware); the bound
/// here is loose because CI runs unoptimised builds on shared runners.
#[test]
fn five_thousand_declarations() {
    let d = base();
    let decls: Vec<CommandDecl> = (0..5_000)
        .map(|i| {
            let mut x = d.clone();
            x.insert("id".into(), s(&format!("a.b{i}")));
            CommandDecl::from_value(CValue::Obj(x))
        })
        .collect();
    let t = Instant::now();
    assert_eq!(validate(&decls), vec![]);
    let took = t.elapsed();
    println!("5000 declarations: {took:?}");
    assert!(took < Duration::from_secs(10));
}

/// Follow-up of #233, item 1: `$schema` only at a schema root and only 2020-12; keywords of
/// earlier drafts are refused, so no number field can hide behind their semantics.
#[test]
fn dialect_is_pinned() {
    let dialect = fid_registry::validate::SCHEMA_DIALECT;
    let root = |v: CValue| {
        let mut d = base();
        let CValue::Obj(ps) = d.get_mut("params_schema").unwrap() else { panic!() };
        ps.insert("$schema".into(), v);
        one(d)
    };
    let r = "/params_schema/$schema";
    assert_eq!(root(s(dialect)), vec![]);
    assert_eq!(root(s("http://json-schema.org/draft-07/schema#")), at("bad_dialect", r));
    assert_eq!(root(s("https://json-schema.org/draft/2019-09/schema")), at("bad_dialect", r));
    assert_eq!(root(s(&format!("{dialect}#"))), at("bad_dialect", r));
    assert_eq!(root(s("HTTPS://json-schema.org/draft/2020-12/schema")), at("bad_dialect", r));
    assert_eq!(root(n(1.0)), at("bad_dialect", r));
    // In result_schema too.
    let mut d = base();
    let CValue::Obj(rs) = d.get_mut("result_schema").unwrap() else { panic!() };
    rs.insert("$schema".into(), s("http://json-schema.org/draft-07/schema#"));
    assert_eq!(one(d), at("bad_dialect", "/result_schema/$schema"));

    // Below the root even the exact URI is refused.
    let p = "/params_schema/properties/t";
    let mut d = base();
    set_param(&mut d, "t", obj([("$schema", s(dialect)), ("type", s("string"))]));
    assert_eq!(one(d), at("bad_dialect", &format!("{p}/$schema")));

    // Older-draft keywords, including one hiding an unannotated number field.
    let hidden = obj([("a", obj([("properties", obj([("x", obj([("type", s("number"))]))]))]))]);
    for (k, v) in [
        ("dependencies", hidden),
        ("dependencies", obj([("a", CValue::Arr(vec![s("b")]))])),
        ("$recursiveRef", s("#")),
        ("$recursiveAnchor", CValue::Bool(true)),
    ] {
        let mut d = base();
        set_param(&mut d, "t", schema(vec![("type", s("object")), (k, v.clone())]));
        assert_eq!(one(d), at("bad_dialect", &format!("{p}/{k}")), "{k}");
        let mut d = base();
        let CValue::Obj(ps) = d.get_mut("params_schema").unwrap() else { panic!() };
        ps.insert(k.to_string(), v);
        assert_eq!(one(d), at("bad_dialect", &format!("/params_schema/{k}")), "{k} at root");
    }
}

/// Follow-up of #233, item 2: a literal above 2^53 is refused whatever its notation; it is
/// never rounded.
#[test]
fn big_literals_in_any_notation_are_refused() {
    for (lit, ok) in [
        ("9007199254740992.0", true),
        ("9007199254740992e0", true),
        ("9.007199254740992e15", true),
        ("9007199254740993.0", false),
        ("9007199254740993e0", false),
        ("9.007199254740993e15", false),
        ("-9007199254740993.0", false),
        ("9007199254740994.0", false),
        ("1e300", false),
    ] {
        let text = format!("{{\"decls\":[{{\"x\":{lit}}}]}}");
        let got = validate_json(text.as_bytes(), &Limits::default(), &GeoFrameUnits);
        match got {
            Err(InputError::BadNumber(echo)) => assert!(!ok && echo == lit, "{lit}: {echo}"),
            Err(e) => panic!("{lit}: {e}"),
            Ok(_) => assert!(ok, "{lit} accepted"),
        }
    }
}

/// Follow-up of #233, item 3: `required` entries and `dependentRequired` names are
/// printable ASCII without space.
#[test]
fn required_and_dependent_required_names() {
    let p = "/params_schema/properties/t";
    let with = |pairs: Pairs| {
        let mut d = base();
        let mut t = vec![("type", s("object"))];
        t.extend(pairs);
        set_param(&mut d, "t", schema(t));
        one(d)
    };
    for bad in ["", "a b", "to\u{200b}lerance", "\u{202e}x", "caf\u{e9}"] {
        assert_eq!(
            with(vec![("required", CValue::Arr(vec![s(bad)]))]),
            at("bad_property_name", &format!("{p}/required"))
        );
        let key = obj([(bad, CValue::Arr(vec![s("b")]))]);
        assert_eq!(with(vec![("dependentRequired", key)]), at("bad_property_name", &format!("{p}/dependentRequired")));
        let entry = obj([("a", CValue::Arr(vec![s(bad)]))]);
        assert_eq!(
            with(vec![("dependentRequired", entry)]),
            at("bad_property_name", &format!("{p}/dependentRequired/a")),
            "{bad:?}"
        );
    }
    assert_eq!(with(vec![("required", CValue::Arr(vec![s("a"), s("b")]))]), vec![]);
    assert_eq!(with(vec![("dependentRequired", obj([("a", CValue::Arr(vec![s("b")]))]))]), vec![]);
    assert_eq!(with(vec![("dependentRequired", obj([("a", CValue::Arr(vec![]))]))]), vec![]);
    assert_eq!(
        with(vec![("dependentRequired", CValue::Arr(vec![]))]),
        at("bad_schema", &format!("{p}/dependentRequired"))
    );
    let dup = obj([("a", CValue::Arr(vec![s("b"), s("b")]))]);
    assert_eq!(with(vec![("dependentRequired", dup)]), at("bad_schema", &format!("{p}/dependentRequired/a")));
    let mixed = obj([("a", CValue::Arr(vec![n(1.0)]))]);
    assert_eq!(with(vec![("dependentRequired", mixed)]), at("bad_schema", &format!("{p}/dependentRequired/a")));
}

/// Follow-up of #233, item 4: a field pointer's segment cut applies to the escaped form.
#[test]
fn field_pointer_segments_are_cut_after_escaping() {
    let key = "\u{200b}".repeat(100);
    let mut d = base();
    d.insert(key, CValue::Null);
    let errors = run(vec![d]);
    let unknown = errors.iter().find(|e| e.rule == RuleCode::UnknownField).unwrap();
    assert_eq!(unknown.field.as_deref(), Some(format!("/{}", r"\u{200b}".repeat(16)).as_str()));
}

/// The branch events of CORE.JRN.BR.NAME are reserved to their commands: any other command
/// (another id, or the right id at another major) declaring one is `reserved_effect`, and
/// the owning command may declare it.
#[test]
fn branch_events_are_reserved_to_their_commands() {
    let owners = [
        ("BranchCreated", "journal.create_branch"),
        ("BranchProtected", "journal.protect_branch"),
        ("CoeditingSet", "journal.set_branch_coediting"),
    ];
    for (effect, owner) in owners {
        assert!(fid_registry::RESERVED_EFFECTS.contains(&(effect, Some((owner, 1)))), "{effect}");
        let mut d = base();
        d.insert("effects".into(), CValue::Arr(vec![s(effect)]));
        assert_eq!(one(d.clone()), at("reserved_effect", "/effects/0"), "{effect} on {}", "align.run_bestfit");
        let mut wrong_major = d.clone();
        wrong_major.insert("id".into(), s(owner));
        wrong_major.insert("major".into(), n(2.0));
        assert!(
            codes(&run(vec![wrong_major])).contains(&("reserved_effect", Some("/effects/0".to_string()))),
            "{effect}@2"
        );
        let mut right = d;
        right.insert("id".into(), s(owner));
        assert!(!codes(&run(vec![right])).iter().any(|(c, _)| *c == "reserved_effect"), "{effect} on its owner");
    }
}
