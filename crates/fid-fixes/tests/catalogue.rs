//! Acceptance criteria of spec node FLOW.DEADEND.CATALOG (AC-1 .. AC-4) and the loader's
//! error paths (REQ-FLOW.DEADEND.CATALOG-005).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::to_canonical_bytes;
use fid_fixes::{catalogue_schema, load, load_path, shipped, FixError, StateKind, MAX_BYTES};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    root().join("fixtures/flow/fixes").join(name)
}

fn commands(cat: &fid_fixes::FixCatalogue, code: &str) -> Vec<String> {
    cat.get(code).unwrap_or_else(|| panic!("{code} missing")).fixes.iter().map(|f| f.command.to_string()).collect()
}

/// AC-1: the valid fixture loads 6 entries in reason_code order with the reference digest.
#[test]
fn ac1_valid_sorted_and_digest() {
    let cat = load_path(&fixture("catalogue-valid.json")).unwrap();
    let codes: Vec<&str> = cat.entries().iter().map(|e| e.reason_code.as_str()).collect();
    assert_eq!(
        codes,
        [
            "decide.rule_missing",
            "gdt.no_rule_bound",
            "gdt.unc_no_decision",
            "unc.instrument_missing",
            "unc.model_missing",
            "unc.u_missing"
        ]
    );
    let want = std::fs::read_to_string(fixture("catalogue-valid.digest")).unwrap();
    assert_eq!(cat.digest().as_str(), want.trim());
    assert_eq!(cat.version(), "fix-catalogue@1");
}

/// AC-1 (determinism): file order does not change the digest; loading twice is identical.
#[test]
fn ac1_digest_independent_of_file_order() {
    let a = load_path(&fixture("catalogue-valid.json")).unwrap();
    let b = load(&a.to_canonical_bytes().unwrap()).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.digest(), b.digest());
}

/// AC-2: duplicate reason codes are refused with the code named.
#[test]
fn ac2_duplicate() {
    let err = load_path(&fixture("catalogue-dup.json")).unwrap_err();
    assert_eq!(err.code(), "FIX-DUP");
    assert_eq!(err.to_string(), "FIX-DUP decide.rule_missing");
}

/// AC-3: a placeholder outside the closed set is refused.
#[test]
fn ac3_bad_placeholder() {
    let err = load_path(&fixture("catalogue-badph.json")).unwrap_err();
    assert_eq!(err.code(), "FIX-PLACEHOLDER");
    match err {
        FixError::Placeholder { reason_code, command, arg, placeholder } => {
            assert_eq!(reason_code, "decide.rule_missing");
            assert_eq!(command, "decide.bind_rule@1");
            assert_eq!(arg, "chars");
            assert_eq!(placeholder, "${path}");
        }
        other => panic!("unexpected {other:?}"),
    }
}

/// AC-4: the shipped catalogue holds exactly the REQ-004 mapping, fixes in the listed order.
#[test]
fn ac4_shipped_entries() {
    let cat = shipped().unwrap();
    let want: [(&str, &[&str]); 6] = [
        ("decide.rule_missing", &["decide.bind_rule@1", "decide.set_default_rule@1"]),
        ("gdt.no_rule_bound", &["decide.bind_rule@1", "decide.set_default_rule@1"]),
        ("gdt.unc_no_decision", &["unc.propagate_auto@1"]),
        ("unc.instrument_missing", &["unc.bind_instrument_model@1", "unc.list_profile@1"]),
        ("unc.model_missing", &["unc.bind_model@1"]),
        ("unc.u_missing", &["unc.propagate_auto@1", "unc.declare_uncertainty@1"]),
    ];
    assert_eq!(cat.entries().len(), want.len());
    for (code, cmds) in want {
        assert_eq!(commands(&cat, code), cmds, "{code}");
    }
    for e in cat.entries() {
        let want_kind = if e.reason_code.starts_with("unc.") { StateKind::Uec0 } else { StateKind::NotDecidable };
        assert_eq!(e.state_kind, want_kind, "{}", e.reason_code);
    }
}

/// REQ-003: entries built from a domain table keep that table's row order, not sorted order.
#[test]
fn req3_source_table_order_kept() {
    let cat = shipped().unwrap();
    let e = cat.get("unc.u_missing").unwrap();
    assert_eq!(e.source_table.as_deref(), Some("uec-fix@1"));
    // uec-fix@1 (REQ-UNC.UEC.EXPL-004) lists propagate_auto before declare_uncertainty.
    assert_eq!(commands(&cat, "unc.u_missing"), ["unc.propagate_auto@1", "unc.declare_uncertainty@1"]);
    assert!(cat.get("decide.rule_missing").unwrap().source_table.is_none());
}

/// Every reason code, owner and command in the shipped catalogue is defined in the spec tree:
/// the owner node exists, its NODE.md names the code, and some NODE.md names the command.
#[test]
fn shipped_references_exist_in_spec() {
    let spec = root().join("spec");
    let mut all = String::new();
    let mut stack = vec![spec.clone()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().is_some_and(|n| n == "NODE.md") {
                all.push_str(&std::fs::read_to_string(&p).unwrap());
            }
        }
    }
    for e in shipped().unwrap().entries() {
        let node = e.owner.split('.').fold(spec.clone(), |p, s| p.join(s.to_ascii_lowercase()));
        let text = std::fs::read_to_string(node.join("NODE.md")).unwrap_or_else(|_| panic!("no node {}", e.owner));
        let (_, code) = e.reason_code.split_once('.').unwrap();
        assert!(text.contains(&format!("`{code}`")), "{} does not name {}", e.owner, code);
        for f in &e.fixes {
            assert!(all.contains(&format!("`{}`", f.command)), "no spec node names {}", f.command);
        }
    }
}

#[test]
fn schema_is_generated() {
    let mut want = to_canonical_bytes(&catalogue_schema()).unwrap();
    want.push(b'\n');
    let have = std::fs::read(root().join("schema/fix-catalogue-v1.json")).unwrap_or_default();
    assert!(
        have == want,
        "schema/fix-catalogue-v1.json is out of date. Replace its contents with:\n{}",
        String::from_utf8_lossy(&want)
    );
}

const ONE: &str = r#"{"version":"fix-catalogue@1","entries":[{"reason_code":"decide.rule_missing","state_kind":"not_decidable","owner":"UNC.DEC.DEFAULT","fixes":[{"command":"decide.bind_rule@1","args":{"chars":"${characteristic_id}"},"label_key":"fix.decide.rule_missing.bind_rule"}]}]}"#;

#[test]
fn inline_minimal_loads() {
    let cat = load(ONE.as_bytes()).unwrap();
    assert_eq!(cat.entries().len(), 1);
    assert!(cat.get("unc.u_missing").is_none());
}

fn schema_err(text: &str) -> String {
    let err = load(text.as_bytes()).unwrap_err();
    assert_eq!(err.code(), "FIX-SCHEMA", "{err}");
    err.to_string()
}

#[test]
fn schema_errors() {
    assert!(schema_err("not json").contains("not valid JSON"));
    assert!(schema_err(&ONE.replace("fix-catalogue@1", "fix-catalogue@2")).contains("$.version"));
    assert!(schema_err(&ONE.replace("not_decidable", "NOT_DECIDABLE")).contains("state_kind"));
    assert!(schema_err(&ONE.replace("\"decide.rule_missing\"", "\"rule_missing\"")).contains("reason_code"));
    assert!(schema_err(&ONE.replace("UNC.DEC.DEFAULT", "unc.dec.default")).contains("owner"));
    assert!(schema_err(&ONE.replace("decide.bind_rule@1", "decide.bind_rule")).contains("command"));
    assert!(schema_err(&ONE.replace("\"label_key\":", "\"label\":")).contains("unknown property"));
    assert!(schema_err(&ONE.replace("\"${characteristic_id}\"", "1")).contains("expected a string"));
    let no_fixes = r#"{"version":"fix-catalogue@1","entries":[{"reason_code":"decide.rule_missing","state_kind":"not_decidable","owner":"UNC.DEC.DEFAULT","fixes":[]}]}"#;
    assert!(schema_err(no_fixes).contains("at least one fix"));
    assert!(schema_err(&ONE.replace("\"fixes\":", "\"source_table\":\"UEC\",\"fixes\":")).contains("source_table"));
    assert!(schema_err(r#"{"version":"fix-catalogue@1"}"#).contains("missing required property `entries`"));
}

#[test]
fn size_and_depth_limits() {
    let mut big = ONE.as_bytes().to_vec();
    big.resize(MAX_BYTES + 1, b' ');
    let err = load(&big).unwrap_err();
    assert_eq!(err.code(), "FIX-SIZE");
    // Exactly at the limit is accepted (trailing whitespace is valid JSON).
    big.truncate(MAX_BYTES);
    assert_eq!(load(&big).unwrap().entries().len(), 1);
    let deep = format!("{}{}", "[".repeat(40), "]".repeat(40));
    assert_eq!(load(deep.as_bytes()).unwrap_err().code(), "FIX-SIZE");
}

#[test]
fn io_error() {
    let err = load_path(&fixture("no-such-catalogue.json")).unwrap_err();
    assert_eq!(err.code(), "FIX-IO");
}
