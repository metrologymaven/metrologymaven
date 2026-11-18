//! Acceptance criteria of spec node CORE.REG.DEF.PRED (AC-1 .. AC-4).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_registry::pred::{self, MAX_BOUND};
use fid_registry::{
    compile_command, evaluate, CommandDef, ContextTypes, FieldPath, ParamSchema, PredContext, PredError, Predicate,
    Type, Value, Verdict,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(rel: &str) -> BTreeMap<String, CValue> {
    let v = parse(&std::fs::read(root().join("fixtures/core/reg").join(rel)).unwrap()).unwrap();
    let CValue::Obj(o) = v else { panic!("{rel}: object expected") };
    o
}

/// Fixture encoding: integral JSON numbers are ints, `{"$float": "..."}` floats,
/// `{"$int": "..."}` large ints, `{"$ref": "..."}` entity references.
fn to_value(v: &CValue) -> Value {
    match v {
        CValue::Bool(b) => Value::Bool(*b),
        CValue::Str(s) => Value::Str(s.clone()),
        CValue::Num(f) => {
            assert!(f.fract() == 0.0 && f.abs() <= 9_007_199_254_740_992.0, "non-integral bare number {f}");
            Value::Int(*f as i64)
        }
        CValue::Arr(a) => Value::List(a.iter().map(to_value).collect()),
        CValue::Obj(o) => {
            if o.len() == 1 {
                if let Some(CValue::Str(s)) = o.get("$float") {
                    return Value::Float(s.parse().unwrap());
                }
                if let Some(CValue::Str(s)) = o.get("$int") {
                    return Value::Int(s.parse().unwrap());
                }
                if let Some(CValue::Str(s)) = o.get("$ref") {
                    return Value::Ref(s.clone());
                }
            }
            Value::Map(o.iter().map(|(k, v)| (k.clone(), to_value(v))).collect())
        }
        other => panic!("unsupported fixture value {other:?}"),
    }
}

/// The fake context of the fixtures: fixed maps, read by path.
struct MapCtx(Value);

impl PredContext for MapCtx {
    fn get(&self, path: &FieldPath) -> Result<Value, PredError> {
        let mut cur = &self.0;
        for seg in &path.0 {
            match cur {
                Value::Map(m) => match m.get(seg) {
                    Some(v) => cur = v,
                    None => return Err(PredError::MissingField { path: path.to_string() }),
                },
                _ => return Err(PredError::MissingField { path: path.to_string() }),
            }
        }
        Ok(cur.clone())
    }
}

fn types(o: &BTreeMap<String, CValue>) -> (ParamSchema, ContextTypes) {
    let params = ParamSchema::from_json_schema(&o["params_schema"]);
    let CValue::Obj(t) = &o["types"] else { panic!() };
    let mut ctx = ContextTypes::new();
    for (k, v) in t {
        let CValue::Str(name) = v else { panic!() };
        ctx.insert(k, Type::from_name(name).unwrap_or_else(|| panic!("type {name}")));
    }
    (params, ctx)
}

fn str_of<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> &'a str {
    match o.get(k) {
        Some(CValue::Str(s)) => s,
        other => panic!("{k}: {other:?}"),
    }
}

/// Runs every AC-1 case; returns one line per case for AC-4's comparison.
fn run_cases() -> Vec<String> {
    let o = fixture("def/pred/cases.json");
    let (params, ctx_types) = types(&o);
    let CValue::Arr(cases) = &o["cases"] else { panic!() };
    assert!(cases.len() >= 60, "AC-1 needs at least 60 cases");
    let mut lines = Vec::new();
    for c in cases {
        let CValue::Obj(c) = c else { panic!() };
        let name = str_of(c, "name");
        let pred = Predicate::compile(str_of(c, "expr"), "test.precondition_0", &params, &ctx_types)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let ctx = MapCtx(to_value(&c["context"]));
        let ev = evaluate(&pred, &ctx);
        let CValue::Obj(want) = &c["expect"] else { panic!() };
        let CValue::Num(steps) = want["steps"] else { panic!() };
        assert_eq!(f64::from(ev.steps), steps, "{name}: step count ({:?})", ev.result);
        match (&ev.result, want.get("error"), want.get("result")) {
            (Err(e), Some(CValue::Str(code)), None) => assert_eq!(e.code(), code, "{name}"),
            (Ok(Verdict::True), None, Some(CValue::Bool(true))) => {}
            (Ok(Verdict::False { message_key }), None, Some(CValue::Bool(false))) => {
                assert_eq!(message_key, str_of(want, "message_key"), "{name}")
            }
            (got, _, _) => panic!("{name}: got {got:?}, want {want:?}"),
        }
        lines.push(format!("{name} {:?} {}", ev.result, ev.steps));
    }
    lines
}

/// AC-1: every case's result and step count match the reference evaluator.
#[test]
fn ac1_cases() {
    run_cases();
}

/// AC-2: the registry build rejects each bad declaration, naming the command, the
/// precondition and the byte position.
#[test]
fn ac2_bad_syntax_fails_build() {
    let o = fixture("def/pred/bad_syntax.json");
    let (_, ctx_types) = types(&o);
    let CValue::Arr(defs) = parse(&std::fs::read(root().join("fixtures/core/reg/def/field/defs_valid.json")).unwrap())
        .unwrap()
    else {
        panic!()
    };
    let mut def = CommandDef::from_value(defs[0].clone()).unwrap();
    def.params_schema = o["params_schema"].clone();
    def.preconditions = vec!["true".to_string()];
    assert_eq!(compile_command(&def, &ctx_types).unwrap().len(), 1);

    let CValue::Arr(cases) = &o["cases"] else { panic!() };
    for c in cases {
        let CValue::Obj(c) = c else { panic!() };
        let name = str_of(c, "name");
        def.preconditions = vec!["params.n > 0".to_string(), str_of(c, "decl").to_string()];
        let err = compile_command(&def, &ctx_types).expect_err(name);
        assert_eq!(err.command, format!("{}@{}", def.id, def.major), "{name}");
        assert_eq!(err.index, 1, "{name}");
        assert_eq!(err.error.code, str_of(c, "code"), "{name}: {err}");
        let CValue::Num(pos) = c["pos"] else { panic!() };
        assert_eq!(err.error.pos as f64, pos, "{name}: {err}");
        let shown = err.to_string();
        assert!(shown.contains(&def.id) && shown.contains(&format!("byte {}", err.error.pos)), "{shown}");
    }
}

fn ids(n: usize) -> MapCtx {
    let ids = (0..n).map(|k| Value::Str(format!("id{k:03}"))).collect();
    let params = Value::Map([("ids".to_string(), Value::List(ids))].into_iter().collect());
    MapCtx(Value::Map([("params".to_string(), params)].into_iter().collect()))
}

fn ids_schema() -> ParamSchema {
    let schema = parse(br#"{"type":"object","properties":{"ids":{"type":"array","items":{"type":"string"}}}}"#).unwrap();
    ParamSchema::from_json_schema(&schema)
}

/// AC-3: the quadratic uniqueness check over 200 IDs stops at exactly 10,000 steps.
#[test]
fn ac3_budget() {
    let src = "all(x in params.ids, all(y in params.ids, x != y))";
    let pred = Predicate::compile(src, "k", &ids_schema(), &ContextTypes::new()).unwrap();
    assert_eq!(pred.bound, MAX_BOUND);
    let ev = evaluate(&pred, &ids(200));
    assert_eq!(ev.result, Err(PredError::Budget { steps: 10_000 }));
    assert_eq!(ev.result.as_ref().unwrap_err().code(), "precondition_budget");
    assert_eq!(ev.steps, 10_000);
}

/// AC-4: results are a pure function of the input. AC-1 compares every result and step
/// count with the same fixture on each CI target (x86-64 Linux and Windows, ARM64 macOS);
/// here repeated runs must also agree line for line.
#[test]
fn ac4_identical_runs() {
    assert_eq!(run_cases(), run_cases());
}

/// Bounds: declarations are size- and depth-limited, the bound annotation is capped, and the
/// evaluator never panics on hostile context values.
#[test]
fn limits_and_hostile_input() {
    let long = format!("true{}", " ".repeat(pred::MAX_SRC_BYTES));
    assert_eq!(pred::parse(&long).unwrap_err().code, "fpl_syntax");
    let deep = format!("{}true{}", "(".repeat(200), ")".repeat(200));
    assert_eq!(pred::parse(&deep).unwrap_err().code, "fpl_syntax");
    assert!(pred::parse(&format!("{}true{}", "(".repeat(40), ")".repeat(40))).is_ok());

    // A context that returns the wrong type for every path: typed errors, never a panic.
    struct Liar;
    impl PredContext for Liar {
        fn get(&self, _: &FieldPath) -> Result<Value, PredError> {
            Ok(Value::Float(f64::NAN))
        }
    }
    let pred = Predicate::compile("all(x in params.ids, x == \"a\")", "k", &ids_schema(), &ContextTypes::new()).unwrap();
    assert_eq!(evaluate(&pred, &Liar).result.unwrap_err().code(), "type_error");
    let pred = Predicate::compile("size(params.ids) > 0", "k", &ids_schema(), &ContextTypes::new()).unwrap();
    assert_eq!(evaluate(&pred, &Liar).result.unwrap_err().code(), "type_error");
    let pred = Predicate::compile("has(params.ids)", "k", &ids_schema(), &ContextTypes::new()).unwrap();
    assert_eq!(pred::eval(&pred, &Liar), Ok(true));

    // Non-ASCII input is rejected with a position, not a panic.
    assert_eq!(pred::parse("params.n > 1 \u{e9}").unwrap_err().pos, 13);
    assert_eq!(pred::parse("\"\u{e9}\\q\"").unwrap_err().code, "fpl_syntax");
}

/// The parameter schema maps JSON Schema types (and `x-ref`) to FPL types.
#[test]
fn param_schema_types() {
    let o = fixture("def/pred/cases.json");
    let (params, _) = types(&o);
    let env = &params.0 .0;
    assert_eq!(env["params"], Type::Map);
    assert_eq!(env["params.ids"], Type::List(Box::new(Type::String)));
    assert_eq!(env["params.feature"], Type::EntityRef);
    assert_eq!(env["params.x"], Type::Float);
    assert_eq!(env["params.items[].n"], Type::Int);
    assert_eq!(Type::from_name("list<list<int>>").unwrap().to_string(), "list<list<int>>");
}
