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
        (vec![("$ref", s("#/$defs/x"))], vec![]),
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
