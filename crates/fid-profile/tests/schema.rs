#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The published schemas (`schema/profile-v1.schema.json`, `schema/profile-tenant-v1.schema.json`)
//! and the code agree: the same members, enumerations, patterns and limits. The Python
//! fixture check (`fixtures/profile/make_fixtures.py --check`) separately validates every
//! expected document and the accepted tenant files against the schemas with `jsonschema`.

use std::path::PathBuf;

use fid_profile::defs::{
    DOCUMENT_KEYS, MAX_DESCRIPTION_CHARS, MAX_LABEL_CHARS, MAX_REQUIRED_MEANINGS, MAX_TENANT_MEANINGS,
    MEANING_CODE_PATTERN, PARAM_KEYS, PROFILE_VERSION, SCHEMA_ID, TENANT_CODE_PATTERN, TENANT_KEYS,
    TENANT_MEANING_KEYS, TENANT_SCHEMA_ID, TEXT_PATTERN,
};
use fid_profile::{AgentWrites, ProfileName, ProgramReleaseByOther};
use fid_trust_canon::Value;

fn schema(file: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schema").join(file);
    fid_trust_canon::parse(&std::fs::read(&p).unwrap()).unwrap()
}

/// Follows a JSON Pointer-like list of member names.
fn at<'a>(v: &'a Value, path: &[&str]) -> &'a Value {
    let mut cur = v;
    for key in path {
        cur = match cur {
            Value::Object(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v).unwrap_or_else(|| panic!("no {key}")),
            other => panic!("{key}: not an object: {other:?}"),
        };
    }
    cur
}

fn strings(v: &Value) -> Vec<String> {
    match v {
        Value::Array(items) => items
            .iter()
            .map(|i| match i {
                Value::String(s) => s.clone(),
                other => panic!("{other:?}"),
            })
            .collect(),
        other => panic!("{other:?}"),
    }
}

fn keys(v: &Value) -> Vec<String> {
    match v {
        Value::Object(m) => {
            let mut k: Vec<String> = m.iter().map(|(k, _)| k.clone()).collect();
            k.sort();
            k
        }
        other => panic!("{other:?}"),
    }
}

fn sorted(xs: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = xs.iter().map(|s| s.to_string()).collect();
    v.sort();
    v
}

fn num(v: &Value) -> usize {
    match v {
        Value::Number(x) => {
            assert!(*x >= 0.0 && x.fract() == 0.0);
            *x as usize
        }
        other => panic!("{other:?}"),
    }
}

fn string(v: &Value) -> &str {
    match v {
        Value::String(s) => s,
        other => panic!("{other:?}"),
    }
}

#[test]
fn document_schema_matches_the_code() {
    let s = schema("profile-v1.schema.json");
    assert_eq!(string(at(&s, &["$schema"])), "https://json-schema.org/draft/2020-12/schema");
    assert_eq!(string(at(&s, &["$id"])), SCHEMA_ID);
    assert_eq!(at(&s, &["additionalProperties"]), &Value::Bool(false));
    assert_eq!(sorted_strings(at(&s, &["required"])), sorted(&DOCUMENT_KEYS));
    assert_eq!(keys(at(&s, &["properties"])), sorted(&DOCUMENT_KEYS));

    let names: Vec<&str> = ProfileName::ALL.iter().map(|n| n.as_str()).collect();
    assert_eq!(strings(at(&s, &["properties", "name", "enum"])), names);
    assert_eq!(num(at(&s, &["properties", "version", "const"])), PROFILE_VERSION as usize);

    let params = at(&s, &["properties", "params"]);
    assert_eq!(at(params, &["additionalProperties"]), &Value::Bool(false));
    assert_eq!(strings(at(params, &["required"])), PARAM_KEYS.to_vec());
    assert_eq!(keys(at(params, &["properties"])), sorted(&PARAM_KEYS));
    let aw: Vec<&str> = AgentWrites::ALL.iter().map(|a| a.as_str()).collect();
    assert_eq!(strings(at(params, &["properties", "agent_writes", "enum"])), aw);
    let pr: Vec<&str> = ProgramReleaseByOther::ALL.iter().map(|a| a.as_str()).collect();
    assert_eq!(strings(at(params, &["properties", "program_release_by_other", "enum"])), pr);
    for b in ["allow_software_keys", "release_requires_signatures"] {
        assert_eq!(string(at(params, &["properties", b, "type"])), "boolean");
    }

    let defs = at(&s, &["$defs"]);
    assert_eq!(string(at(defs, &["meaningCode", "pattern"])), MEANING_CODE_PATTERN);
    assert_eq!(string(at(defs, &["tenantCode", "pattern"])), TENANT_CODE_PATTERN);
    assert_eq!(string(at(defs, &["text", "pattern"])), TEXT_PATTERN);
    assert_eq!(num(at(defs, &["text", "minLength"])), 1);
    assert_eq!(num(at(defs, &["requiredMeanings", "maxItems"])), MAX_REQUIRED_MEANINGS);
    assert_eq!(at(defs, &["requiredMeanings", "uniqueItems"]), &Value::Bool(true));
    assert_eq!(num(at(defs, &["tenantMeanings", "maxItems"])), MAX_TENANT_MEANINGS);
    let tm = at(defs, &["tenantMeaning"]);
    assert_eq!(at(tm, &["additionalProperties"]), &Value::Bool(false));
    assert_eq!(strings(at(tm, &["required"])), TENANT_MEANING_KEYS.to_vec());
    assert_eq!(keys(at(tm, &["properties"])), sorted(&TENANT_MEANING_KEYS));
    assert_eq!(max_length(at(tm, &["properties", "label"])), MAX_LABEL_CHARS);
    assert_eq!(max_length(at(tm, &["properties", "description"])), MAX_DESCRIPTION_CHARS);
}

fn sorted_strings(v: &Value) -> Vec<String> {
    let mut s = strings(v);
    s.sort();
    s
}

/// `maxLength` inside an `allOf` list.
fn max_length(v: &Value) -> usize {
    match at(v, &["allOf"]) {
        Value::Array(parts) => {
            let found: Vec<usize> = parts
                .iter()
                .filter_map(|p| match p {
                    Value::Object(m) => m.iter().find(|(k, _)| k == "maxLength").map(|(_, v)| num(v)),
                    _ => None,
                })
                .collect();
            assert_eq!(found.len(), 1);
            found[0]
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn tenant_schema_matches_the_code() {
    let s = schema("profile-tenant-v1.schema.json");
    assert_eq!(string(at(&s, &["$id"])), TENANT_SCHEMA_ID);
    assert_eq!(string(at(&s, &["type"])), "object");
    assert_eq!(at(&s, &["additionalProperties"]), &Value::Bool(false));
    assert_eq!(keys(at(&s, &["properties"])), sorted(&TENANT_KEYS));
    assert_eq!(
        string(at(&s, &["properties", "required_meanings", "$ref"])),
        "profile-v1.schema.json#/$defs/requiredMeanings"
    );
    assert_eq!(num(at(&s, &["properties", "required_meanings", "minItems"])), 1);
    assert_eq!(
        string(at(&s, &["properties", "tenant_meanings", "$ref"])),
        "profile-v1.schema.json#/$defs/tenantMeanings"
    );
}

#[test]
fn tenant_code_pattern_is_a_narrowing_of_the_meaning_code_pattern() {
    // Both patterns cap the code at 32 characters: "X_" plus at most 30.
    assert_eq!(MEANING_CODE_PATTERN, "^[A-Z][A-Z0-9_]{1,31}$");
    assert_eq!(TENANT_CODE_PATTERN, "^X_[A-Z0-9_]{0,30}$");
}
