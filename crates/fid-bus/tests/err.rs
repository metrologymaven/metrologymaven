//! Acceptance criteria of spec node CORE.BUS.PIPE.ERR (AC-1 .. AC-3).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{
    bus_error_schema, bus_errors_digest, bus_errors_json, catalog_value, check_compatible, current_catalog,
    parse_catalog, BusError, Code, ConflictReason, Detail, Removal, ValidationReason, MAX_DETAIL_CHARS,
};
use fid_canon::{parse, to_canonical_bytes, CValue};
use std::path::{Path, PathBuf};

const FIXTURE_DIR: &str = "fixtures/core/bus/pipe/err";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn check_generated(rel: &str, value: &CValue) {
    let mut want = to_canonical_bytes(value).unwrap();
    want.push(b'\n');
    let have = std::fs::read(root().join(rel)).unwrap_or_default();
    assert!(have == want, "{rel} is out of date. Replace its contents with:\n{}", String::from_utf8_lossy(&want));
}

/// The twelve codes of REQ-CORE.BUS.PIPE.ERR-001, written out by hand.
const REQ_001_CODES: [&str; 12] = [
    "validation",
    "policy",
    "precondition",
    "conflict",
    "not_found",
    "hidden_command",
    "undeclared_effect",
    "cancelled",
    "unauthenticated",
    "idempotency_mismatch",
    "upgrade_refused",
    "internal",
];

/// AC-1: the generated file lists exactly the twelve codes and its digest equals the
/// committed `bus-errors.v1.digest` (both fixtures come from `make_bus_errors.py`).
#[test]
fn ac1_generated_file_and_digest() {
    let bytes = bus_errors_json().unwrap();
    let mut fixture = read(&format!("{FIXTURE_DIR}/bus-errors.v1.json"));
    assert_eq!(fixture.pop(), Some(b'\n'));
    assert_eq!(String::from_utf8(bytes.clone()).unwrap(), String::from_utf8(fixture).unwrap());

    let want = String::from_utf8(read(&format!("{FIXTURE_DIR}/bus-errors.v1.digest"))).unwrap();
    assert_eq!(bus_errors_digest().unwrap().to_string(), want.trim_end());
    assert_eq!(fid_canon::digest_bytes(&bytes).to_string(), want.trim_end());

    let CValue::Obj(o) = parse(&bytes).unwrap() else { panic!("not an object") };
    let Some(CValue::Obj(codes)) = o.get("codes") else { panic!("no codes") };
    let mut listed: Vec<&str> = codes.keys().map(String::as_str).collect();
    let mut want_codes = REQ_001_CODES.to_vec();
    listed.sort_unstable();
    want_codes.sort_unstable();
    assert_eq!(listed, want_codes);
    assert_eq!(o.get("schema_version"), Some(&CValue::Num(1.0)));
    for code in codes.values() {
        let CValue::Arr(reasons) = code else { panic!("reasons not an array") };
        assert!(!reasons.is_empty(), "every code has at least one reason");
    }

    // The enum agrees with REQ-001 too, and every code round-trips.
    assert_eq!(Code::ALL.len(), 12);
    for c in Code::ALL {
        assert!(REQ_001_CODES.contains(&c.as_str()), "{c}");
        assert_eq!(Code::from_wire(c.as_str()), Some(*c));
    }
}

/// The published copy under `schema/` is generated from the Rust types.
#[test]
fn published_files_are_generated() {
    check_generated("schema/bus-errors.json", &catalog_value());
    check_generated("schema/bus-error.schema.json", &bus_error_schema());
}

/// REQ-003: the current build keeps every code and reason of v1.
#[test]
fn current_is_compatible_with_v1() {
    let v1 = parse_catalog(&read(&format!("{FIXTURE_DIR}/bus-errors.v1.json"))).unwrap();
    assert_eq!(check_compatible(&v1, &current_catalog()), Ok(()));
}

/// AC-2 (error path): renaming `conflict` to `head_conflict` fails, naming `conflict`.
#[test]
fn ac2_renamed_code_fails_naming_it() {
    let v1 = parse_catalog(&read(&format!("{FIXTURE_DIR}/bus-errors.v1.json"))).unwrap();
    let mut branch = current_catalog();
    let reasons = branch.remove("conflict").unwrap();
    branch.insert("head_conflict".to_string(), reasons);

    let removed = check_compatible(&v1, &branch).unwrap_err();
    assert_eq!(removed, vec![Removal::Code("conflict".to_string())]);
    assert!(removed[0].to_string().contains("`conflict`"), "{}", removed[0]);
}

/// REQ-003: renaming or dropping a reason fails too; additions pass.
#[test]
fn reason_removal_fails_and_additions_pass() {
    let v1 = parse_catalog(&read(&format!("{FIXTURE_DIR}/bus-errors.v1.json"))).unwrap();

    let mut branch = current_catalog();
    let policy = branch.get_mut("policy").unwrap();
    assert!(policy.remove("outside_root"));
    policy.insert("root_escape".to_string());
    branch.get_mut("validation").unwrap().remove("schema");
    let removed = check_compatible(&v1, &branch).unwrap_err();
    assert_eq!(
        removed,
        vec![
            Removal::Reason { code: "policy".into(), reason: "outside_root".into() },
            Removal::Reason { code: "validation".into(), reason: "schema".into() },
        ]
    );

    let mut grown = current_catalog();
    grown.insert("brand_new".to_string(), ["x".to_string()].into_iter().collect());
    grown.get_mut("conflict").unwrap().insert("another".to_string());
    assert_eq!(check_compatible(&v1, &grown), Ok(()));
}

#[test]
fn parse_catalog_rejects_bad_input() {
    let cases: [(&[u8], &str); 6] = [
        (br#"[]"#, "bad_shape"),
        (br#"{"codes":{},"schema_version":2}"#, "schema_version_unsupported"),
        (br#"{"codes":{"a":"b"},"schema_version":1}"#, "bad_shape"),
        (br#"{"codes":{"a":["b","b"]},"schema_version":1}"#, "bad_shape"),
        (br#"{"codes":{},"extra":1,"schema_version":1}"#, "bad_shape"),
        (br#"{"codes":{},"codes":{},"schema_version":1}"#, "duplicate_key"),
    ];
    for (bytes, code) in cases {
        assert_eq!(parse_catalog(bytes).unwrap_err().code(), code, "{}", String::from_utf8_lossy(bytes));
    }
}

// ------------------------------------------------------------------ AC-3

/// A JSON Schema checker for exactly the keywords `bus-error.schema.json` uses:
/// `type`, `required`, `additionalProperties: false`, `properties`, `enum`, `const`,
/// `maxLength` and `oneOf`. Any other keyword fails the test so the checker never
/// silently ignores part of the schema.
fn validates(schema: &CValue, v: &CValue) -> bool {
    let CValue::Obj(s) = schema else { panic!("schema is not an object") };
    for (k, kw) in s {
        let ok = match k.as_str() {
            "$schema" | "$id" | "title" => true,
            "type" => match (kw, v) {
                (CValue::Str(t), CValue::Obj(_)) => t == "object",
                (CValue::Str(t), CValue::Str(_)) => t == "string",
                _ => false,
            },
            "required" => {
                let CValue::Arr(req) = kw else { panic!("required") };
                match v {
                    CValue::Obj(o) => req.iter().all(|r| matches!(r, CValue::Str(r) if o.contains_key(r))),
                    _ => true,
                }
            }
            "additionalProperties" => {
                assert_eq!(kw, &CValue::Bool(false));
                let Some(CValue::Obj(props)) = s.get("properties") else { panic!("properties") };
                match v {
                    CValue::Obj(o) => o.keys().all(|k| props.contains_key(k)),
                    _ => true,
                }
            }
            "properties" => {
                let CValue::Obj(props) = kw else { panic!("properties") };
                match v {
                    CValue::Obj(o) => props.iter().all(|(k, sub)| match o.get(k) {
                        None => true,
                        Some(x) => validates(sub, x),
                    }),
                    _ => true,
                }
            }
            "enum" => {
                let CValue::Arr(items) = kw else { panic!("enum") };
                items.contains(v)
            }
            "const" => kw == v,
            "maxLength" => {
                let CValue::Num(n) = kw else { panic!("maxLength") };
                match v {
                    CValue::Str(t) => (t.chars().count() as f64) <= *n,
                    _ => true,
                }
            }
            "oneOf" => {
                let CValue::Arr(subs) = kw else { panic!("oneOf") };
                subs.iter().filter(|sub| validates(sub, v)).count() == 1
            }
            other => panic!("schema keyword {other} not supported by this checker"),
        };
        if !ok {
            return false;
        }
    }
    true
}

/// AC-3: every code, with every reason, serialises to JSON that validates against the
/// committed `schema/bus-error.schema.json`.
#[test]
fn ac3_every_error_validates_against_schema() {
    let schema = parse(&read("schema/bus-error.schema.json")).unwrap();
    let long = "é".repeat(MAX_DETAIL_CHARS + 50);
    let mut n = 0;
    for code in Code::ALL {
        for reason in code.reasons() {
            for detail in ["", "plain text", long.as_str()] {
                let e = BusError::from_wire(code.as_str(), reason, detail).unwrap();
                assert_eq!(e.code(), *code);
                assert_eq!(e.reason(), reason);
                let json = e.to_json().unwrap();
                let v = parse(&json).unwrap();
                assert!(validates(&schema, &v), "{}", String::from_utf8_lossy(&json));
                let CValue::Obj(o) = &v else { panic!() };
                assert_eq!(o.len(), 3, "exactly code, reason, detail (REQ-004)");
                n += 1;
            }
        }
    }
    assert!(n >= 12 * 3);

    // The checker is not vacuous: these must fail.
    let bad = [
        r#"{"code":"conflict","detail":"","reason":"schema"}"#,
        r#"{"code":"head_conflict","detail":"","reason":"head_moved"}"#,
        r#"{"code":"conflict","reason":"head_moved"}"#,
        r#"{"code":"conflict","detail":"","extra":1,"reason":"head_moved"}"#,
    ];
    for b in bad {
        assert!(!validates(&schema, &parse(b.as_bytes()).unwrap()), "{b}");
    }
    let detail = "x".repeat(MAX_DETAIL_CHARS + 1);
    let too_long = format!(r#"{{"code":"conflict","detail":"{detail}","reason":"head_moved"}}"#);
    assert!(!validates(&schema, &parse(too_long.as_bytes()).unwrap()));
}

#[test]
fn detail_is_bounded_untrusted_text() {
    let d = Detail::new(&"ab".repeat(MAX_DETAIL_CHARS));
    assert_eq!(d.as_untrusted_str().chars().count(), MAX_DETAIL_CHARS);
    let d = Detail::new(&"€".repeat(MAX_DETAIL_CHARS + 1));
    assert_eq!(d.as_untrusted_str().chars().count(), MAX_DETAIL_CHARS);
    assert_eq!(Detail::new("short").as_untrusted_str(), "short");
    assert_eq!(Detail::empty().as_untrusted_str(), "");

    // Detail text that looks like JSON stays an opaque string.
    let e = BusError::Validation(ValidationReason::Schema, Detail::new(r#"{"code":"internal"}"#));
    assert_eq!(
        String::from_utf8(e.to_json().unwrap()).unwrap(),
        r#"{"code":"validation","detail":"{\"code\":\"internal\"}","reason":"schema"}"#
    );
    // Display leaves the untrusted detail out.
    assert_eq!(e.to_string(), "validation/schema");
}

#[test]
fn from_wire_rejects_unknown_and_mismatched() {
    assert!(BusError::from_wire("head_conflict", "head_moved", "").is_none());
    assert!(BusError::from_wire("conflict", "schema", "").is_none());
    assert_eq!(
        BusError::from_wire("conflict", "head_moved", "d"),
        Some(BusError::Conflict(ConflictReason::HeadMoved, Detail::new("d")))
    );
}

#[test]
fn reasons_are_unique_per_code() {
    for c in Code::ALL {
        let mut r = c.reasons();
        let n = r.len();
        r.sort_unstable();
        r.dedup();
        assert_eq!(r.len(), n, "duplicate reason in {c}");
    }
    // `fid_bus::err` is the same module.
    assert_eq!(fid_bus::err::Code::ALL.len(), 12);
}
