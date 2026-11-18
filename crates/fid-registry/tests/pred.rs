//! Acceptance criteria of spec node CORE.REG.DEF.PRED (AC-1 .. AC-4).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_registry::pred::{self, MAX_BOUND};
use fid_registry::{
    compile_command, evaluate, CommandDef, ContextTypes, FieldPath, ParamSchema, PredContext, PredError, Precondition,
    Predicate, Type, Value, Verdict,
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
        let bound = match c.get("bound") {
            Some(CValue::Num(b)) => *b as u32,
            _ => MAX_BOUND,
        };
        let key = c.get("message_key").map_or("test.precondition_0", |_| str_of(c, "message_key"));
        let pred = Predicate::compile(str_of(c, "expr"), bound, key, &params, &ctx_types)
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

fn decl(expr: &str) -> Precondition {
    Precondition { expr: expr.to_string(), bound: MAX_BOUND, message_key: "test.ok".to_string() }
}

fn base_def(params_schema: &CValue) -> CommandDef {
    let CValue::Arr(defs) =
        parse(&std::fs::read(root().join("fixtures/core/reg/def/field/defs_valid.json")).unwrap()).unwrap()
    else {
        panic!()
    };
    let mut def = CommandDef::from_value(defs[0].clone()).unwrap();
    def.params_schema = params_schema.clone();
    def
}

/// AC-2: the registry build rejects each bad declaration, naming the command, the
/// precondition and the byte position.
#[test]
fn ac2_bad_syntax_fails_build() {
    let o = fixture("def/pred/bad_syntax.json");
    let (_, ctx_types) = types(&o);
    let mut def = base_def(&o["params_schema"]);
    def.preconditions = vec![decl("true")];
    assert_eq!(compile_command(&def, &ctx_types).unwrap().len(), 1);

    let CValue::Arr(cases) = &o["cases"] else { panic!() };
    for c in cases {
        let CValue::Obj(c) = c else { panic!() };
        let name = str_of(c, "name");
        def.preconditions = vec![decl("params.n > 0"), decl(str_of(c, "expr"))];
        let err = compile_command(&def, &ctx_types).expect_err(name);
        assert_eq!(err.command, format!("{}@{}", def.id, def.major), "{name}");
        assert_eq!(err.index, 1, "{name}");
        assert_eq!(err.error.code, str_of(c, "code"), "{name}: {err}");
        let CValue::Num(pos) = c["pos"] else { panic!() };
        assert_eq!(err.error.pos as f64, pos, "{name}: {err}");
        let shown = err.to_string();
        assert!(shown.contains(&def.id) && shown.contains(&format!("byte {}", err.error.pos)), "{shown}");
    }

    // A declared bound above FPL's cap fails the build too.
    def.preconditions = vec![Precondition { bound: 20_000, ..decl("true") }];
    let err = compile_command(&def, &ctx_types).unwrap_err();
    assert_eq!((err.index, err.error.code), (0, "fpl_bound"));
}

fn ids(n: usize) -> MapCtx {
    let ids = (0..n).map(|k| Value::Str(format!("id{k:04}"))).collect();
    let params = Value::Map([("ids".to_string(), Value::List(ids))].into_iter().collect());
    MapCtx(Value::Map([("params".to_string(), params)].into_iter().collect()))
}

fn ids_schema() -> ParamSchema {
    let schema =
        parse(br#"{"type":"object","properties":{"ids":{"type":"array","items":{"type":"string"}}}}"#).unwrap();
    ParamSchema::from_json_schema(&schema)
}

fn compile(expr: &str) -> Predicate {
    Predicate::compile(expr, MAX_BOUND, "k", &ids_schema(), &ContextTypes::new()).unwrap()
}

/// AC-3: a quadratic membership check over 1,000 distinct IDs stops with
/// `precondition_budget` after exactly 10,000 steps, even with short-circuiting.
#[test]
fn ac3_budget() {
    let pred = compile("all(x in params.ids, exists(y in params.ids, y == x))");
    let ev = evaluate(&pred, &ids(1000));
    assert_eq!(ev.result, Err(PredError::Budget { steps: 10_000 }));
    assert_eq!(ev.result.as_ref().unwrap_err().code(), "precondition_budget");
    assert_eq!(ev.steps, 10_000);
    // The same check over 10 IDs finishes: true, within budget.
    assert_eq!(evaluate(&pred, &ids(10)).result, Ok(Verdict::True));
}

/// AC-4: results are a pure function of the input. AC-1 compares every result and step
/// count with the same fixture on each CI target (x86-64 Linux and Windows, ARM64 macOS);
/// here repeated runs must also agree line for line.
#[test]
fn ac4_identical_runs() {
    assert_eq!(run_cases(), run_cases());
}

/// Words refused at the start of an expression are ordinary field names after `.`.
#[test]
fn keywords_are_field_names_after_dot() {
    for src in ["params.load > 0", "params.repeat", "doc.now", "params.for.while == 1", "params.in"] {
        assert!(pred::parse(src).is_ok(), "{src}");
    }
    for src in ["load > 0", "while", "now", "all(let in params.ids, true)"] {
        assert_eq!(pred::parse(src).unwrap_err().code, "fpl_syntax", "{src}");
    }
}

/// `-0.0` is normalised to `+0.0` on entry, from literals and from context reads.
#[test]
fn negative_zero_is_normalised() {
    let schema = parse(br#"{"type":"object","properties":{"tol":{"type":"number"}}}"#).unwrap();
    let params = ParamSchema::from_json_schema(&schema);
    let ctx = MapCtx(Value::Map(
        [("params".to_string(), Value::Map([("tol".to_string(), Value::Float(-0.0))].into_iter().collect()))]
            .into_iter()
            .collect(),
    ));
    for (src, want) in [("0.0 == -0.0", true), ("-0.0 < 0.0", false), ("params.tol >= 0.0", true)] {
        let pred = Predicate::compile(src, MAX_BOUND, "k", &params, &ContextTypes::new()).unwrap();
        assert_eq!(pred::eval(&pred, &ctx), Ok(want), "{src}");
    }
}

/// A missing field is reported with its full path.
#[test]
fn missing_field_names_full_path() {
    let schema = parse(
        br#"{"type":"object","properties":{"items":{"type":"array","items":{"type":"object","properties":{"n":{"type":"integer"}}}}}}"#,
    )
    .unwrap();
    let params = ParamSchema::from_json_schema(&schema);
    let item = |n: Option<i64>| Value::Map(n.map(|n| ("n".to_string(), Value::Int(n))).into_iter().collect());
    let items = Value::List(vec![item(Some(1)), item(None)]);
    let ctx = MapCtx(Value::Map(
        [("params".to_string(), Value::Map([("items".to_string(), items)].into_iter().collect()))].into_iter().collect(),
    ));
    let pred = Predicate::compile("all(it in params.items, it.n > 0)", MAX_BOUND, "k", &params, &ContextTypes::new())
        .unwrap();
    assert_eq!(evaluate(&pred, &ctx).result, Err(PredError::MissingField { path: "params.items[1].n".to_string() }));
    let pred = Predicate::compile("params.items.n > 0", MAX_BOUND, "k", &params, &ContextTypes::new());
    assert_eq!(pred.unwrap_err().code, "fpl_type");
    let pred = Predicate::compile("params.gone.n > 0", MAX_BOUND, "k", &params, &ContextTypes::new());
    assert_eq!(pred.unwrap_err().code, "fpl_type");
}

/// Only real nesting counts toward the depth limit: flat `&&`/`||` chains do not.
#[test]
fn flat_chains_are_not_nesting() {
    let flat = vec!["true"; 200].join(" && ");
    assert_eq!(pred::parse(&flat).unwrap().depth, 2);
    let flat = vec!["true"; 200].join(" || ");
    assert_eq!(pred::parse(&flat).unwrap().depth, 2);
    let nested = format!("{}true{}", "true && (".repeat(70), ")".repeat(70));
    assert_eq!(pred::parse(&nested).unwrap_err().code, "fpl_syntax");
}

/// Bounds: expressions are size- and depth-limited, and the evaluator never panics on
/// hostile context values.
#[test]
fn limits_and_hostile_input() {
    let long = format!("true{}", " ".repeat(pred::MAX_SRC_BYTES));
    assert_eq!(pred::parse(&long).unwrap_err().code, "fpl_syntax");
    let deep = format!("{}true{}", "(".repeat(200), ")".repeat(200));
    assert_eq!(pred::parse(&deep).unwrap_err().code, "fpl_syntax");
    assert!(pred::parse(&format!("{}true{}", "(".repeat(40), ")".repeat(40))).is_ok());
    for bad in [0, MAX_BOUND + 1] {
        let err = Predicate::compile("true", bad, "k", &ids_schema(), &ContextTypes::new()).unwrap_err();
        assert_eq!(err.code, "fpl_bound");
    }

    // A context that returns the wrong type for every path: typed errors, never a panic.
    struct Liar;
    impl PredContext for Liar {
        fn get(&self, _: &FieldPath) -> Result<Value, PredError> {
            Ok(Value::Float(f64::NAN))
        }
    }
    assert_eq!(evaluate(&compile("all(x in params.ids, x == \"a\")"), &Liar).result.unwrap_err().code(), "type_error");
    assert_eq!(evaluate(&compile("size(params.ids) > 0"), &Liar).result.unwrap_err().code(), "type_error");
    assert_eq!(pred::eval(&compile("has(params.ids)"), &Liar), Ok(true));

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
