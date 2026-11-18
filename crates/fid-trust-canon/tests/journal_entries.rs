//! AC-2 / REQ-TRUST.CHAIN.CANON-005 against CORE's journal writer. The golden
//! journals (`fixtures/journal/golden/*.jsonl`) are not published yet; until they
//! are, every header of CORE.JRN.ENTRY's vector set (`fixtures/core/jrn/entry/
//! vectors.json`, expected hashes from that node's own Python reference) is hashed
//! by this crate and by `fid-journal` (a dev-dependency only), and both must equal
//! the recorded hash.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_trust_canon::{entry_hash, entry_hash_value, parse, serialize, Value};

const VECTORS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/jrn/entry/vectors.json");

fn get<'a>(v: &'a Value, key: &str) -> &'a Value {
    match v {
        Value::Object(m) => m.iter().find(|(k, _)| k == key).map(|(_, x)| x).unwrap_or_else(|| panic!("no {key}")),
        other => panic!("{key} of {other:?}"),
    }
}

#[test]
fn core_entry_vectors_hash_identically() {
    let Value::Array(vectors) = parse(&std::fs::read(VECTORS).unwrap()).unwrap() else { panic!("not an array") };
    assert!(vectors.len() >= 30, "{}", vectors.len());
    for v in &vectors {
        let Value::String(name) = get(v, "name") else { panic!("name") };
        let Value::String(recorded) = get(v, "hash") else { panic!("hash") };
        let header = get(v, "header");

        // This crate, from the parsed tree and from JSON text.
        let ours = entry_hash_value(header).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(ours.as_str(), recorded, "{name}");
        let text = serialize(header).unwrap();
        assert_eq!(entry_hash(&text).unwrap(), ours, "{name}");

        // CORE's writer: schema check plus hash recomputation over the same header.
        let cvalue = fid_canon::parse(&text).unwrap();
        let core = fid_journal::verify_header_value(&cvalue).unwrap_or_else(|e| panic!("{name}: fid-journal {e:?}"));
        assert_eq!(core.0, ours.to_bytes(), "{name}");
    }
}

/// A one-byte change to any header changes our hash (no field is silently ignored
/// except the top-level `hash`).
#[test]
fn every_header_field_is_hashed() {
    let Value::Array(vectors) = parse(&std::fs::read(VECTORS).unwrap()).unwrap() else { panic!("not an array") };
    let Value::Object(members) = get(&vectors[0], "header").clone() else { panic!("header") };
    let base = entry_hash_value(&Value::Object(members.clone())).unwrap();
    for (i, (name, _)) in members.iter().enumerate() {
        let mut changed = members.clone();
        let value = &mut changed[i].1;
        if name == "hash" {
            *value = Value::String("anything".into());
            assert_eq!(entry_hash_value(&Value::Object(changed)).unwrap(), base);
            continue;
        }
        *value = match value {
            // Appending a digit keeps decimal-string fields valid.
            Value::String(s) => Value::String(format!("{s}0")),
            other => Value::Array(vec![other.clone()]),
        };
        assert_ne!(entry_hash_value(&Value::Object(changed)).unwrap(), base, "{name}");
    }
}
