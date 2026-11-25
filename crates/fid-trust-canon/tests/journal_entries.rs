//! AC-2 / REQ-TRUST.CHAIN.CANON-005 against CORE's journal writer. The golden
//! journals (`fixtures/journal/golden/*.jsonl`) are not published yet; until they
//! are, every header of CORE.JRN.ENTRY's vector set (`fixtures/core/jrn/entry/
//! vectors.json`, expected hashes from that node's own Python reference) is hashed
//! by this crate and by `fid-journal` (a dev-dependency only), and both must equal
//! the recorded hash.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_trust_canon::{
    entry_hash, entry_hash_value, inspect_entry_line, parse, serialize, CanonError, ConventionRule, EntryScratch,
    InspectError, InvalidKind, Limits, Value,
};

const VECTORS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/core/jrn/entry/vectors.json");
const CANON_ENTRY_VECTORS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/canon/entry/vectors.json");

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
        let replacement = match &*value {
            // Appending a digit keeps decimal-string fields valid.
            Value::String(s) => Value::String(format!("{s}0")),
            other => Value::Array(vec![other.clone()]),
        };
        *value = replacement;
        assert_ne!(entry_hash_value(&Value::Object(changed)).unwrap(), base, "{name}");
    }
}

fn opt_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    let Value::Object(m) = v else { return None };
    m.iter().find(|(k, _)| k == key).and_then(|(_, val)| match val {
        Value::String(s) => Some(s.as_str()),
        _ => None,
    })
}

/// Differential cross-check of `inspect_entry_line` (both the single-pass canonical ASCII fast
/// path and the general arena fallback) against `parse` + `entry_hash_value` across every core
/// entry vector, every canon entry vector, and edge cases.
#[test]
fn inspect_entry_line_matches_entry_hash_across_vectors_and_edge_cases() {
    let mut scratch = EntryScratch::new();
    let limits = Limits::default();

    // 1. All CORE.JRN.ENTRY vectors in canonical wrapped, payload-wrapped, whitespace/unsorted,
    //    and bare-header forms.
    let Value::Array(core_vecs) = parse(&std::fs::read(VECTORS).unwrap()).unwrap() else { panic!("not an array") };
    for v in &core_vecs {
        let Value::String(name) = get(v, "name") else { panic!("name") };
        let header = get(v, "header");
        let want_hash = entry_hash_value(header).unwrap();
        let canon_hdr = String::from_utf8(serialize(header).unwrap()).unwrap();

        // Canonical wrapped -> hits single-pass canonical fast path when ASCII.
        let wrapped = format!(r#"{{"header":{canon_hdr}}}"#);
        let ins = inspect_entry_line(wrapped.as_bytes(), limits, &mut scratch)
            .unwrap_or_else(|e| panic!("{name} wrapped: {e:?}"));
        assert_eq!(ins.recomputed_hash, want_hash, "{name}");
        assert_eq!(ins.recomputed_bytes, want_hash.to_bytes(), "{name}");
        assert_eq!(ins.stored_hash.as_deref(), opt_str(header, "hash"), "{name}");
        assert_eq!(ins.stored_prev.as_deref(), opt_str(header, "prev"), "{name}");
        assert_eq!(ins.seq.as_deref(), opt_str(header, "seq"), "{name}");
        assert_eq!(ins.kind.as_deref(), opt_str(header, "kind"), "{name}");
        assert_eq!(ins.stream.as_deref(), opt_str(header, "stream"), "{name}");

        // Wrapped with trailing canonical sibling (`"payload"`).
        let with_payload = format!(r#"{{"header":{canon_hdr},"payload":{{"ok":true,"v":[0,1,2]}}}}"#);
        let ins_p = inspect_entry_line(with_payload.as_bytes(), limits, &mut scratch)
            .unwrap_or_else(|e| panic!("{name} payload: {e:?}"));
        assert_eq!(ins_p.recomputed_hash, want_hash, "{name} payload");

        // Pretty/unsorted wrapper -> forces ArenaParser fallback.
        let pretty = format!(" {{ \"z_extra\" : 1 , \"header\" : {canon_hdr} }} ");
        let ins_fb = inspect_entry_line(pretty.as_bytes(), limits, &mut scratch)
            .unwrap_or_else(|e| panic!("{name} fallback: {e:?}"));
        assert_eq!(ins_fb.recomputed_hash, want_hash, "{name} fallback");
        assert_eq!(ins_fb.recomputed_bytes, want_hash.to_bytes(), "{name} fallback");

        // Bare header (has `seq` and `prev` at top level).
        if opt_str(header, "seq").is_some() && opt_str(header, "prev").is_some() {
            let ins_bare = inspect_entry_line(canon_hdr.as_bytes(), limits, &mut scratch)
                .unwrap_or_else(|e| panic!("{name} bare: {e:?}"));
            assert_eq!(ins_bare.recomputed_hash, want_hash, "{name} bare");
        }
    }

    // 2. All TRUST.CHAIN.CANON entry vectors (valid hashes, canon.convention paths, canon.invalid).
    let Value::Array(canon_vecs) = parse(&std::fs::read(CANON_ENTRY_VECTORS).unwrap()).unwrap() else {
        panic!("not an array")
    };
    for v in &canon_vecs {
        let Value::String(name) = get(v, "name") else { panic!("name") };
        let Value::String(header_raw) = get(v, "header") else { panic!("header") };
        let ref_res = entry_hash(header_raw.as_bytes());
        let wrapped = format!(r#"{{"header":{header_raw}}}"#);
        let got_res = inspect_entry_line(wrapped.as_bytes(), limits, &mut scratch);
        match (ref_res, got_res) {
            (Ok(want), Ok(got)) => {
                assert_eq!(got.recomputed_hash, want, "{name}");
                assert_eq!(got.recomputed_bytes, want.to_bytes(), "{name}");
            }
            (
                Err(CanonError::Convention { rule: ConventionRule::HeaderNotObject, .. }),
                Err(InspectError::Structure { reason: "header_not_object", .. }),
            ) => {}
            (Err(want_err), Err(InspectError::Canon(got_err))) => {
                assert_eq!(got_err.code(), want_err.code(), "{name}: got {got_err:?}, want {want_err:?}");
                match (&want_err, &got_err) {
                    (CanonError::Convention { path: wp, rule: wr }, CanonError::Convention { path: gp, rule: gr }) => {
                        assert_eq!((gp, gr), (wp, wr), "{name}");
                    }
                    (CanonError::Invalid { kind: wk, .. }, CanonError::Invalid { kind: gk, .. }) => {
                        assert_eq!(gk, wk, "{name}");
                    }
                    _ => panic!("{name}: mismatch {got_err:?} vs {want_err:?}"),
                }
            }
            (r, g) => panic!("{name}: ref={r:?}, got={g:?}"),
        }
    }

    // 3. Targeted edge cases for canonical fast-path `hash` placement, nested `hash`, non-string
    //    `hash`, escapes, Unicode, numeric edge forms, conventions, duplicates, depth & structure.
    let valid_cases = [
        // `"hash"` as only member
        r#"{"header":{"hash":"sha256:0000000000000000000000000000000000000000000000000000000000000000"}}"#,
        // `"hash"` as first member with following member
        r#"{"header":{"hash":"x","seq":"1"}}"#,
        // `"hash"` in the middle
        r#"{"header":{"a":0,"hash":"x","seq":"1"}}"#,
        // `"hash"` as last member
        r#"{"header":{"a":-42,"b":[true,false,null,9007199254740991],"hash":"x"}}"#,
        // No `"hash"` member
        r#"{"header":{"a":1,"seq":"1","time":{"mono_ns":"0"}}}"#,
        // Non-string `"hash"` (skipped by convention check even with fractional float!)
        r#"{"header":{"a":1,"hash":123,"seq":"1"}}"#,
        r#"{"header":{"a":1,"hash":[1.5],"seq":"1"}}"#,
        // Non-canonical numeric forms that fall back to ArenaParser
        r#"{"header":{"a":-0,"b":1.0,"c":1e2,"hash":"x","seq":"1"}}"#,
        // Escaped strings, non-ASCII UTF-8, `~` and `/` in keys, unsorted keys
        r#"{"header":{"z/a~b":"caf\u00e9\"\\","a":"héllo","hash":"x","seq":"1"}}"#,
        // Nested `"hash"` inside `command` (MUST be hashed)
        r#"{"header":{"command":{"hash":"nested-1","seed":"7"},"hash":"top","seq":"1"}}"#,
        r#"{"header":{"command":{"hash":"nested-2","seed":"7"},"hash":"top","seq":"1"}}"#,
    ];
    let mut nested_hashes = Vec::new();
    for raw in valid_cases {
        let parsed = parse(raw.as_bytes()).unwrap();
        let hdr = get(&parsed, "header");
        let want = entry_hash_value(hdr).unwrap();
        let got = inspect_entry_line(raw.as_bytes(), limits, &mut scratch).unwrap_or_else(|e| panic!("{raw}: {e:?}"));
        assert_eq!(got.recomputed_hash, want, "{raw}");
        assert_eq!(got.recomputed_bytes, want.to_bytes(), "{raw}");
        if raw.contains("nested-") {
            nested_hashes.push(got.recomputed_hash);
        }
    }
    assert_ne!(nested_hashes[0], nested_hashes[1], "nested hash field must affect entry hash");

    // Non-string `hash` flags.
    let ns = inspect_entry_line(br#"{"header":{"a":1,"hash":123,"seq":"1"}}"#, limits, &mut scratch).unwrap();
    assert!(ns.hash_non_string);
    assert!(ns.stored_hash.is_none());

    // Canonical-looking convention violations MUST still return exact CanonError::Convention paths.
    for (raw, want_path, want_rule) in [
        (r#"{"header":{"seq":1}}"#, "/seq", ConventionRule::DecimalString),
        (r#"{"header":{"seq":"01"}}"#, "/seq", ConventionRule::DecimalString),
        (r#"{"header":{"seq":"1","time":{"mono_ns":42}}}"#, "/time/mono_ns", ConventionRule::DecimalString),
        (r#"{"header":{"seq":"1","time":{"mono_ns":"00"}}}"#, "/time/mono_ns", ConventionRule::DecimalString),
        (r#"{"header":{"command":{"seed":0},"seq":"1"}}"#, "/command/seed", ConventionRule::DecimalString),
        (r#"{"header":{"command":{"seed":"01"},"seq":"1"}}"#, "/command/seed", ConventionRule::DecimalString),
        (r#"{"header":{"a":1.5,"seq":"1"}}"#, "/a", ConventionRule::RawNumber),
        (r#"{"header":{"a":9007199254740992,"seq":"1"}}"#, "/a", ConventionRule::RawNumber),
        (r#"{"header":{"command":{"hash":1.5,"seed":"1"},"seq":"1"}}"#, "/command/hash", ConventionRule::RawNumber),
    ] {
        let err = inspect_entry_line(raw.as_bytes(), limits, &mut scratch).unwrap_err();
        assert_eq!(
            err,
            InspectError::Canon(CanonError::Convention { path: want_path.to_owned(), rule: want_rule }),
            "{raw}"
        );
    }

    // Duplicate keys (in header, nested object, and outer wrapper), lone surrogates, tight depth.
    for (raw, want_kind) in [
        (r#"{"header":{"a":1,"a":2}}"#, InvalidKind::DuplicateKey),
        (r#"{"header":{"a":{"x":1,"x":2}}}"#, InvalidKind::DuplicateKey),
        (r#"{"header":{"a":1},"header":{"a":1}}"#, InvalidKind::DuplicateKey),
        (r#"{"header":{"a":1},"p":1,"p":2}"#, InvalidKind::DuplicateKey),
        (r#"{"header":{"a":"\ud800"}}"#, InvalidKind::LoneSurrogate),
    ] {
        let err = inspect_entry_line(raw.as_bytes(), limits, &mut scratch).unwrap_err();
        assert!(
            matches!(err, InspectError::Canon(CanonError::Invalid { kind, .. }) if kind == want_kind),
            "{raw}: {err:?}"
        );
    }

    let deep_err =
        inspect_entry_line(br#"{"header":{"a":{"b":1}}}"#, Limits { max_bytes: 1024, max_depth: 2 }, &mut scratch)
            .unwrap_err();
    assert!(matches!(deep_err, InspectError::Canon(CanonError::Invalid { kind: InvalidKind::TooDeep, .. })));

    // Structure errors.
    assert!(matches!(
        inspect_entry_line(b"[1,2]", limits, &mut scratch),
        Err(InspectError::Structure { reason: "entry_not_object", .. })
    ));
    assert!(matches!(
        inspect_entry_line(br#"{"header":123}"#, limits, &mut scratch),
        Err(InspectError::Structure { reason: "header_not_object", .. })
    ));
    assert!(matches!(
        inspect_entry_line(br#"{"other":1}"#, limits, &mut scratch),
        Err(InspectError::Structure { reason: "missing_header", .. })
    ));
}
