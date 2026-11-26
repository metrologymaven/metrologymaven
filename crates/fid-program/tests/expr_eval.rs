//! Acceptance criteria of spec node AUTO.PROG.EXPR.EVAL. The 300-case table and every
//! expected value in it come from `fixtures/program/eval/make_cases.py`, an independent
//! reference evaluator (Python `fractions`, CPython floats, and the bit-exact libm model of
//! `fixtures/num/make_fn_fixtures.py`); the other expectations are hand-labelled from the
//! spec's rules. Nothing here derives an expected value from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_num::kdec;
use fid_program::expr::eval::{display, eval_expr, Env, EvalError, EvalKind, Value, MAX_VALUE_DEPTH, V};
use fid_program::{parse_expr, Kind, RecordType, Span, Type};
use fid_registry::{Catalogue, QuantityCatalogue};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn table() -> BTreeMap<String, CValue> {
    let v = parse(&std::fs::read(root().join("fixtures/program/eval/cases.json")).unwrap()).unwrap();
    let CValue::Obj(m) = v else { panic!("cases.json root") };
    m
}

fn catalogue() -> Catalogue {
    Catalogue::from_json(&std::fs::read(root().join("fixtures/program/types/catalogue.json")).unwrap()).unwrap()
}

fn field(v: &CValue, key: &str) -> CValue {
    match v {
        CValue::Obj(o) => o.get(key).cloned().unwrap_or_else(|| panic!("no field {key}")),
        other => panic!("not an object: {other:?}"),
    }
}

fn bytes(v: &CValue) -> String {
    String::from_utf8(to_canonical_bytes(v).unwrap()).unwrap()
}

fn str_of(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("not a string: {other:?}"),
    }
}

/// Test-side reader of the type syntax the fixture uses (`record<name>` looks the fields up
/// in the fixture's `records` table).
fn ty(src: &str, records: &BTreeMap<String, CValue>) -> Type {
    match src {
        "bool" => return Type::Bool,
        "int" => return Type::Int,
        "decimal" => return Type::Decimal,
        "string" => return Type::Str,
        _ => {}
    }
    let (head, rest) = src.split_once('<').unwrap();
    let inner = rest.strip_suffix('>').unwrap();
    match head {
        "quantity" => Type::Quantity(Kind::Named(inner.into())),
        "element" => Type::Element(inner.into()),
        "list" => Type::List(Box::new(ty(inner, records))),
        "enum" => Type::Enum(inner.split('|').map(str::to_string).collect::<BTreeSet<_>>()),
        "record" => {
            let CValue::Obj(fields) = &records[inner] else { panic!("record {inner}") };
            let fields = fields.iter().map(|(k, t)| (k.clone(), Some(ty(str_of(t), records)))).collect();
            Type::Record(Arc::new(RecordType { name: format!("{inner}@1"), fields }))
        }
        _ => panic!("type {src}"),
    }
}

fn run(src: &str, env: &Env, cat: &Catalogue) -> Result<Value, EvalError> {
    eval_expr(&parse_expr(src).unwrap_or_else(|e| panic!("{src}: {e}")), env, cat)
}

fn outcome(r: &Result<Value, EvalError>) -> CValue {
    match r {
        Ok(v) => CValue::Obj(BTreeMap::from([("ok".to_string(), v.to_value())])),
        Err(e) => CValue::Obj(BTreeMap::from([("err".to_string(), e.to_value())])),
    }
}

/// AC-1: every case of the table, bit for bit, every value tagged `not_of_record`.
#[test]
fn ac1_table_matches_the_reference_evaluator() {
    let t = table();
    let cat = catalogue();
    assert_eq!(str_of(&t["format"]), "fid-program-eval-cases/1");
    assert_eq!(str_of(&t["catalogue"]), "fixtures/program/types/catalogue.json");
    let CValue::Obj(records) = &t["records"] else { panic!("records") };
    let CValue::Arr(cases) = &t["cases"] else { panic!("cases") };
    assert_eq!(cases.len(), 300);
    let mut failures = Vec::new();
    let mut ids = BTreeSet::new();
    for c in cases {
        let CValue::Obj(c) = c else { panic!("case") };
        let id = str_of(&c["id"]).to_string();
        assert!(ids.insert(id.clone()), "duplicate case {id}");
        let src = str_of(&c["src"]);
        let mut env = Env::new();
        let CValue::Obj(vars) = &c["env"] else { panic!("env") };
        for (name, b) in vars {
            let CValue::Obj(b) = b else { panic!("binding") };
            let value = Value::from_value(&b["value"]).unwrap_or_else(|e| panic!("{id} {name}: {e}"));
            env.bind(name, ty(str_of(&b["type"]), records), value, &cat).unwrap_or_else(|e| panic!("{id} {name}: {e}"));
        }
        let got = run(src, &env, &cat);
        if let Ok(v) = &got {
            assert!(v.not_of_record(), "{id}");
        }
        let (got, want) = (bytes(&outcome(&got)), bytes(&c["expect"]));
        if got != want {
            failures.push(format!("{id} {src}\n  got  {got}\n  want {want}"));
        }
    }
    assert!(failures.is_empty(), "{} of 300 cases differ:\n{}", failures.len(), failures.join("\n"));
}

/// AC-2: exact decimals in one unit; an f64 against an authored decimal is compared exactly.
#[test]
fn ac2_exact_decimals_and_exact_comparison() {
    let cat = catalogue();
    let v = run("0.1[mm] + 0.2[mm] == 0.3[mm]", &Env::new(), &cat).unwrap();
    assert_eq!(v.v(), &V::Bool(true));
    // An f64 result 0.0003 m from a command, against 0.3[mm] = 0.0003 m exactly.
    let x = 0.0003_f64;
    let mut env = Env::new();
    let q = Type::Quantity(Kind::Named("length".into()));
    env.bind("r", q, Value::real(x, Some(Kind::Named("length".into()))).unwrap(), &cat).unwrap();
    let want = kdec::cmp(x, &kdec::Decimal::parse("0.0003").unwrap()).unwrap();
    // Hand check: the nearest double to 0.0003 lies below it.
    assert_eq!(want, Ordering::Less);
    let eq = run("r == 0.3[mm]", &env, &cat).unwrap();
    let lt = run("r < 0.3[mm]", &env, &cat).unwrap();
    assert_eq!(eq.v(), &V::Bool(want == Ordering::Equal));
    assert_eq!(lt.v(), &V::Bool(want == Ordering::Less));
    // The same decimal converted once (REQ-003) is that nearest double.
    let sum = run("r + 0.0[mm]", &env, &cat).unwrap();
    assert_eq!(sum.v(), &V::Real(x, Some(Kind::Named("length".into()))));
}

/// AC-3: runtime errors with their kinds and spans; no value is returned.
#[test]
fn ac3_error_kinds_and_spans() {
    let cat = catalogue();
    let mut env = Env::new();
    let xs = Value::list(vec![Value::int(1), Value::int(2), Value::int(3)]).unwrap();
    env.bind("xs", Type::List(Box::new(Type::Int)), xs, &cat).unwrap();
    let cases = [
        ("1 / 0", EvalKind::DivZero, 0, 5),
        ("xs[10]", EvalKind::IndexRange, 0, 6),
        ("9223372036854775807 + 1", EvalKind::IntOverflow, 0, 23),
        ("2 + xs[-1]", EvalKind::IndexRange, 4, 10),
        ("sqrt(-2.0)", EvalKind::Domain, 0, 10),
    ];
    for (src, kind, start, end) in cases {
        let got = run(src, &env, &cat);
        assert_eq!(got, Err(EvalError::Eval { kind, span: Span::new(start, end) }), "{src}");
        let e = got.unwrap_err();
        assert_eq!(e.code(), "eval_error");
        assert_eq!(e.to_value(), {
            let mut o = BTreeMap::new();
            o.insert("code".to_string(), CValue::Str("eval_error".into()));
            o.insert("kind".to_string(), CValue::Str(kind.as_str().into()));
            let span = BTreeMap::from([
                ("end".to_string(), CValue::I64(end as i64)),
                ("start".to_string(), CValue::I64(start as i64)),
            ]);
            o.insert("span".to_string(), CValue::Obj(span));
            CValue::Obj(o)
        });
    }
    // 38 digits fit; 39 do not (REQ-002).
    let ok = run("99999999999999999999999999999999999.999 + 0.000", &env, &cat).unwrap();
    assert_eq!(field(&ok.to_value(), "v"), CValue::Str("99999999999999999999999999999999999.999".into()));
    let over = run("99999999999999999999999999999999999.999 + 0.0001", &env, &cat);
    assert_eq!(over, Err(EvalError::DecimalOverflow { span: Span::new(0, 48) }));
}

/// AC-4: decimal text is preserved by `format`.
#[test]
fn ac4_format_keeps_decimal_text() {
    let cat = catalogue();
    let v = run("format(\"{} mm\", 2.50)", &Env::new(), &cat).unwrap();
    assert_eq!(v.v(), &V::Str("2.50 mm".into()));
    let v = run("format(\"{{{}}} {}\", 2.50[mm], 10.0 - 7.5)", &Env::new(), &cat).unwrap();
    assert_eq!(v.v(), &V::Str("{2.50 mm} 2.5".into()));
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Strips comments (nested block comments too), string, byte-string, raw-string and char
/// literals, so only code is scanned. A literal becomes `""`.
fn code_only(src: &str) -> String {
    let b: Vec<char> = src.chars().collect();
    let at = |i: usize| b.get(i).copied();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let next = at(i + 1);
        let prev = if i == 0 { None } else { at(i - 1) };
        let boundary = |p: Option<char>| !p.is_some_and(is_ident);
        let raw_start =
            c == 'r' && (boundary(prev) || (prev == Some('b') && boundary(if i < 2 { None } else { at(i - 2) })));
        let mut j = i + 1;
        while at(j) == Some('#') {
            j += 1;
        }
        let raw = raw_start && at(j) == Some('"');
        if c == '/' && next == Some('/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            let mut depth = 0;
            while i < b.len() {
                if at(i) == Some('/') && at(i + 1) == Some('*') {
                    depth += 1;
                    i += 2;
                } else if at(i) == Some('*') && at(i + 1) == Some('/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            out.push(' ');
        } else if raw {
            let mut hashes = 0;
            i += 1;
            while at(i) == Some('#') {
                hashes += 1;
                i += 1;
            }
            i += 1;
            loop {
                if i >= b.len() {
                    break;
                }
                if b[i] == '"' && (1..=hashes).all(|k| at(i + k) == Some('#')) {
                    i += 1 + hashes;
                    break;
                }
                i += 1;
            }
            out.push_str("\"\"");
        } else if c == '"' {
            i += 1;
            while i < b.len() && b[i] != '"' {
                i += if b[i] == '\\' { 2 } else { 1 };
            }
            i += 1;
            out.push_str("\"\"");
        } else if c == '\'' && at(i + 2) == Some('\'') {
            i += 3;
        } else if c == '\'' && next == Some('\\') {
            i += 2;
            while i < b.len() && b[i] != '\'' {
                i += 1;
            }
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// Removes whitespace next to `::`, so `std :: fs` reads as `std::fs`.
fn squash_paths(code: &str) -> String {
    let b: Vec<char> = code.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_whitespace() {
            let mut j = i;
            while j < b.len() && b[j].is_whitespace() {
                j += 1;
            }
            let next_colons = b.get(j) == Some(&':') && b.get(j + 1) == Some(&':');
            if !(next_colons || out.ends_with("::")) {
                out.push(' ');
            }
            i = j;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// The AC-5 allowlist lint of one source file of `expr::eval` (`name` is its file name).
/// Returns every problem found; empty means clean.
fn lint(name: &str, src: &str) -> Vec<String> {
    let is_mod = name == "mod.rs";
    let code = squash_paths(&code_only(src));
    let compact: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    let mut problems = Vec::new();
    let banned = [
        "env!",
        "option_env!",
        "include!",
        "include_str!",
        "include_bytes!",
        "print!",
        "println!",
        "eprint!",
        "eprintln!",
        "dbg!",
        "#[path",
        "#![path",
    ];
    for m in banned {
        if compact.contains(m) {
            problems.push(format!("{name}: {m}"));
        }
    }
    let words: Vec<&str> = code.split(|c: char| !is_ident(c)).filter(|w| !w.is_empty()).collect();
    for (k, w) in words.iter().enumerate() {
        let next = words.get(k + 1).copied().unwrap_or("");
        match *w {
            "unsafe" | "extern" => problems.push(format!("{name}: {w}")),
            "static" if next == "mut" => problems.push(format!("{name}: static mut")),
            "mod" if !(is_mod && ["dec", "func", "num", "walk"].contains(&next)) => {
                problems.push(format!("{name}: mod {next}"))
            }
            _ => {}
        }
    }
    // No renames in `use` (`use std as f;` would make an allowed root name anything).
    let mut rest = code.as_str();
    while let Some(p) = rest.find("use ") {
        let boundary = p == 0 || !rest[..p].chars().last().is_some_and(is_ident);
        let stmt_end = rest[p..].find(';').map_or(rest.len(), |e| p + e);
        let stmt = &rest[p..stmt_end];
        if boundary && stmt.split(|c: char| !is_ident(c)).any(|w| w == "as") {
            problems.push(format!("{name}: rename in `{stmt}`"));
        }
        rest = &rest[(p + 4).min(rest.len())..];
    }
    let roots: BTreeSet<&str> = [
        "crate",
        "super",
        "self",
        "Self",
        "std",
        "core",
        "fid_bus",
        "fid_canon",
        "fid_geo_frame",
        "fid_num",
        "fid_registry",
        "fid_types",
        "f",
        "kdec",
        "dec",
        "func",
        "num",
        "walk",
        "i8",
        "i16",
        "i32",
        "i64",
        "i128",
        "u8",
        "u16",
        "u32",
        "u64",
        "u128",
        "usize",
        "isize",
        "f32",
        "f64",
        "str",
    ]
    .into_iter()
    .collect();
    let allowed_paths = [
        "std::cmp",
        "std::collections",
        "std::fmt",
        "std::error::Error",
        "core::cmp",
        "fid_bus::unit::{normalise, AuthoredLiteral}",
        "fid_geo_frame::units::{PI_256, PI_256_SHIFT}",
        "fid_num::kdec",
        "fid_num::f",
        "fid_num::NumError",
        "fid_canon::CValue",
        "fid_registry::QuantityCatalogue",
        "fid_types::Digest",
        "crate::expr::syntax",
        "crate::expr::types",
    ];
    let chars: Vec<char> = code.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // A `::` with no path segment before it (`::std::fs`) is a root of its own.
        if chars[i] == ':' && chars.get(i + 1) == Some(&':') {
            let before = if i == 0 { None } else { chars.get(i - 1).copied() };
            if !before.is_some_and(|c| is_ident(c) || c == '>') {
                problems.push(format!("{name}: leading ::"));
            }
            i += 2;
            continue;
        }
        if is_ident(chars[i]) && (i == 0 || !is_ident(chars[i - 1])) {
            let start = i;
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            // Not a root: after `::` (inside a path) or after `.` (a method turbofish).
            let after_path = (start >= 2 && chars[start - 1] == ':' && chars[start - 2] == ':')
                || (start >= 1 && chars[start - 1] == '.');
            let is_root = chars.get(i) == Some(&':') && chars.get(i + 1) == Some(&':') && !after_path;
            let lower = word.chars().next().is_some_and(|c| c.is_lowercase());
            if !(is_root && lower) {
                continue;
            }
            let path: String = chars[start..].iter().take(80).collect();
            if !roots.contains(word.as_str()) {
                problems.push(format!("{name}: path root {word}::"));
            } else if word == "super" {
                if is_mod || path.starts_with("super::super") {
                    problems.push(format!("{name}: path {path}"));
                }
            } else if [
                "std",
                "core",
                "crate",
                "fid_bus",
                "fid_geo_frame",
                "fid_num",
                "fid_canon",
                "fid_registry",
                "fid_types",
            ]
            .contains(&word.as_str())
                && !allowed_paths.iter().any(|p| path.starts_with(p))
            {
                problems.push(format!("{name}: path {path}"));
            }
        } else {
            i += 1;
        }
    }
    problems
}

/// AC-5 (REQ-007): module `expr::eval` uses only an allowlist of paths: no I/O, time,
/// environment, process, thread, network, randomness or bus API (only the pure
/// `fid_bus::unit` conversion), no crate module outside `expr::{syntax, types}`, no macro
/// that reads the build environment or prints, no `#[path]`, no `use` rename.
#[test]
fn ac5_eval_module_is_pure() {
    let dir = root().join("crates/fid-program/src/expr/eval");
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    assert_eq!(
        files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>(),
        ["dec.rs", "func.rs", "mod.rs", "num.rs", "walk.rs"],
        "a new file in expr::eval must be added to this lint"
    );
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        let problems = lint(&name, &std::fs::read_to_string(f).unwrap());
        assert!(problems.is_empty(), "{problems:?}");
    }
}

/// The AC-5 lint itself: each known evasion is caught, and ordinary code passes.
#[test]
fn ac5_lint_catches_evasions() {
    let clean = "use super::dec::Dec;\nuse std::cmp::Ordering;\nfn a(d: &Dec) -> String {\n    let s = r#\"x\"#;\n    let o = Ordering::Less;\n    format!(\"{s}{o:?}{}\", Vec::<u8>::new().len())\n}\n";
    assert_eq!(lint("walk.rs", clean), Vec::<String>::new());
    let evasions = [
        ("whitespace around ::", "walk.rs", "fn a() { let _ = std :: fs :: read(\"x\"); }"),
        ("leading ::", "walk.rs", "fn a() { let _ = ::std::fs::read(\"x\"); }"),
        ("#[path]", "walk.rs", "#[path = \"../../check.rs\"]\nmod c;"),
        ("use rename", "walk.rs", "use std as f;\nfn a() { let _ = f::fs::read(\"x\"); }"),
        ("crate path outside expr", "walk.rs", "use crate::check::check_program;"),
        ("super beyond the module", "walk.rs", "use super::super::fmt::Program;"),
        ("super from mod.rs", "mod.rs", "use super::types::Type;"),
        ("raw string", "walk.rs", "fn a() { let s = r#\"a\"b\"#; let t = std::fs::read(\"x\"); let u = \"\"; }"),
    ];
    for (what, name, src) in evasions {
        assert!(!lint(name, src).is_empty(), "{what} not caught: {src}");
    }
}

/// REQ-001: the tag survives storage in a variable and use as an argument, and is part of
/// the canonical form; a value without it is refused.
#[test]
fn req001_not_of_record_survives() {
    let cat = catalogue();
    let v = run("2.5 * 2", &Env::new(), &cat).unwrap();
    assert!(v.not_of_record());
    let mut env = Env::new();
    env.bind("v", Type::Decimal, v.clone(), &cat).unwrap();
    let w = run("max(v, 1)", &env, &cat).unwrap();
    assert!(w.not_of_record());
    assert_eq!(w, v);
    let c = v.to_value();
    assert_eq!(field(&c, "not_of_record"), CValue::Bool(true));
    assert_eq!(Value::from_value(&c).unwrap(), v);
    let CValue::Obj(mut o) = c else { panic!() };
    o.insert("not_of_record".into(), CValue::Bool(false));
    assert!(Value::from_value(&CValue::Obj(o.clone())).is_err());
    o.remove("not_of_record");
    assert!(Value::from_value(&CValue::Obj(o)).is_err());
}

/// Fail closed: ill-typed expressions and bad bindings evaluate nothing.
#[test]
fn fails_closed() {
    let cat = catalogue();
    let env = Env::new();
    for src in ["1 + \"a\"", "nope + 1", "1[mm] + 1", "f(1)"] {
        assert!(matches!(run(src, &env, &cat), Err(EvalError::Type { .. })), "{src}");
    }
    let mut env = Env::new();
    let len = Type::Quantity(Kind::Named("length".into()));
    let refuse = |e: Result<(), EvalError>| match e {
        Err(EvalError::Env { reason, .. }) => reason,
        other => panic!("{other:?}"),
    };
    assert_eq!(refuse(env.bind("q", len.clone(), Value::int(1), &cat)), "type");
    assert_eq!(refuse(env.bind("q", len.clone(), Value::dec("1", Some("s")).unwrap(), &cat)), "type");
    assert_eq!(refuse(env.bind("q", len.clone(), Value::real(1.0, None).unwrap(), &cat)), "type");
    assert_eq!(refuse(env.bind("not", Type::Int, Value::int(1), &cat)), "name");
    assert_eq!(refuse(env.bind("f", Type::Formula(Kind::Named("length".into())), Value::int(1), &cat)), "type");
    env.bind("q", len.clone(), Value::dec("1", Some("mm")).unwrap(), &cat).unwrap();
    let geo = Catalogue::geo_frame().unwrap();
    assert_ne!(geo.digest(), cat.digest());
    assert_eq!(refuse(env.bind("p", len, Value::dec("1", Some("mm")).unwrap(), &geo)), "catalogue");
    assert!(matches!(run("q", &env, &geo), Err(EvalError::Env { .. })));
    // Values refuse NaN, infinities, bad decimals and deep nesting.
    assert!(Value::real(f64::NAN, None).is_err());
    assert!(Value::real(f64::INFINITY, None).is_err());
    assert!(Value::dec("1.", None).is_err());
    assert!(Value::dec("1e5", None).is_err());
    let mut v = Value::int(0);
    for _ in 0..MAX_VALUE_DEPTH {
        v = Value::list(vec![v]).unwrap();
    }
    assert!(Value::list(vec![v]).is_err());
}

/// Hand-labelled edge cases of the pinned rules.
#[test]
fn pinned_rules() {
    let cat = catalogue();
    let mut env = Env::new();
    env.bind("z", Type::Decimal, Value::real(-0.0, None).unwrap(), &cat).unwrap();
    env.bind("o", Type::Decimal, Value::real(0.0, None).unwrap(), &cat).unwrap();
    let text = |src: &str| display(&cat, &run(src, &env, &cat).unwrap()).unwrap();
    let canon = |src: &str| bytes(&run(src, &env, &cat).unwrap().to_value());
    // Scales: sum takes the larger, product the sum; remainder keeps the dividend's sign.
    assert_eq!(text("1.50 + 2.5"), "4.00");
    assert_eq!(text("1.50 * 2.5"), "3.750");
    assert_eq!(text("-7.5 % 2"), "-1.5");
    assert_eq!(text("-7 % 2"), "-1");
    // Trailing zeros are dropped only when 38 digits are exceeded.
    assert_eq!(text("1000000000000000000000.0000000000000000 * 10.0"), "10000000000000000000000.000000000000000");
    // A unit decimal times a plain number stays exact in its unit.
    assert_eq!(text("2 * 0.3[mm]"), "0.6 mm");
    // Rounding half to even; floor and ceil.
    assert_eq!(text("round(2.5)"), "2");
    assert_eq!(text("round(-3.5)"), "-4");
    assert_eq!(text("floor(-0.5)"), "-1");
    assert_eq!(text("ceil(0.0001)"), "1");
    // Total order for min and max of reals.
    assert_eq!(canon("min(o, z)"), canon("min(z, o)"));
    assert_eq!(run("min(o, z)", &env, &cat).unwrap().v(), &V::Real(-0.0, None));
    assert_eq!(run("max(z, o)", &env, &cat).unwrap().v(), &V::Real(0.0, None));
    assert_eq!(text("min(1, 2.5)"), "1");
    assert_eq!(field(&run("min(1, 2.5)", &env, &cat).unwrap().to_value(), "t"), CValue::Str("dec".into()));
    // `if` typed decimal turns an int branch into a decimal.
    assert_eq!(field(&run("if true then 1 else 2.5", &env, &cat).unwrap().to_value(), "t"), CValue::Str("dec".into()));
    assert_eq!(field(&run("if true then 1 else 2", &env, &cat).unwrap().to_value(), "t"), CValue::Str("int".into()));
    // Short circuits.
    assert_eq!(text("false and 1 / 0 == 1.0"), "false");
    assert_eq!(text("true or 1 / 0 == 1.0"), "true");
    assert_eq!(text("if false then 1 / 0 else 2.0"), "2.0");
    // to_unit: exact for power-of-ten ratios, one rounding otherwise.
    assert_eq!(text("to_unit(2.50[mm], \"m\")"), "0.00250 m");
    assert_eq!(text("to_unit(1[in], \"mm\")"), "25.4 mm");
    assert_eq!(text("to_unit(1[mm], \"in\")"), "0.001 m");
    assert_eq!(text("to_unit(1[ft], \"in\")"), "12 in");
    // Products of unit decimals stay exact in the catalogue unit of factor fu * fv, or in the
    // canonical unit when fu * fv is a power of ten (REQ-002); f64 only when neither exists.
    assert_eq!(text("2[mm] * 3[mm] == 6[mm^2]"), "true");
    assert_eq!(text("2[mm] * 3[mm]"), "6 mm^2");
    assert_eq!(text("2.0[cm] * 3[mm]"), "0.000060 m^2");
    assert_eq!(field(&run("2[in] * 3[in]", &env, &cat).unwrap().to_value(), "t"), CValue::Str("real".into()));
    // Decimals in different units compare exactly (REQ-004).
    assert_eq!(text("1[in] == 25.4[mm]"), "true");
    assert_eq!(text("1[ft] == 12[in]"), "true");
    assert_eq!(text("180[deg] == 200[gon]"), "true");
    assert_eq!(text("0.1[mm] < 0.0001000000000000000000000000000001[m]"), "true");
    // `i64::MIN % -1` is 0.
    assert_eq!(text("(-9223372036854775807 - 1) % -1"), "0");
    assert_eq!(text("1000[mm] == 1[m]"), "true");
    // Strings, enums and lists.
    assert_eq!(text("len(\"aé\")"), "2");
    assert_eq!(text("contains(\"probe\", \"\")"), "true");
    // Literal display of reals.
    assert_eq!(text("1 / 4"), "0.25");
    assert_eq!(text("z"), "-0");
}

/// Determinism: evaluating twice gives byte-identical canonical results.
#[test]
fn deterministic() {
    let cat = catalogue();
    let src = "format(\"{} {}\", sqrt(2.0) * sin(0.5), atan2(1[mm], 3[in]))";
    let a = bytes(&run(src, &Env::new(), &cat).unwrap().to_value());
    let b = bytes(&run(src, &Env::new(), &cat).unwrap().to_value());
    assert_eq!(a, b);
}
