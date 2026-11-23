//! Acceptance criteria and edge-case tests for `TRUST.CHAIN.LINKS` (AC-1, AC-2, AC-4,
//! REQ-001..REQ-007, declaration/schema validation, and `PackageReader::stream_verified_member`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_chain::ckpt::CheckpointStatus;
use fid_chain::links::{
    check_package, declarations, invoke_cli, invoke_mcp, walk_segment_bytes, AnchorCheckpoint, ChainVerdict,
    CheckChainError, FindingKind, CLI_CHECK_CHAIN, MCP_CHECK_CHAIN, ZERO_DIGEST,
};
use fid_chain::reslink::LinkStatus;
use fid_pkg::reader::{FileOpener, HandleTable, PackageReader};
use fid_pkg::{build_package, EvidenceTier, MemberInput, PackageInput, PkgPath};
use fid_registry::decl::CommandDecl;
use fid_registry::{check_schema, tool_name, validate, Catalogue};
use fid_sign_core::troot::{Keyring, Revocations, SignatureStatus, TrustInputs, TrustRoot};
use fid_trust_canon::{entry_hash, parse as canon_parse, serialize as canon_serialize, Value};
use fid_types::Digest;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn pkg_goldens() -> PathBuf {
    repo_root().join("fixtures").join("pkg").join("golden")
}

fn tamper_fixtures() -> PathBuf {
    repo_root().join("fixtures").join("chain").join("tamper")
}

fn reslink_fixtures() -> PathBuf {
    repo_root().join("fixtures").join("chain").join("reslink")
}

fn both_trust_root_path() -> PathBuf {
    repo_root().join("fixtures").join("troot").join("roots").join("both.json")
}

fn load_both_trust_root() -> TrustRoot {
    let bytes = fs::read(both_trust_root_path()).expect("read both.json");
    TrustRoot::parse(&bytes).expect("parse both.json")
}

fn field<'a>(v: &'a Value, name: &str) -> Option<&'a Value> {
    let Value::Object(fields) = v else { return None };
    fields.iter().find(|(k, _)| k == name).map(|(_, val)| val)
}

fn as_str(v: &Value) -> &str {
    match v {
        Value::String(s) => s.as_str(),
        other => panic!("expected string, got {other:?}"),
    }
}

fn as_u64(v: &Value) -> u64 {
    match v {
        Value::Number(n) => *n as u64,
        other => panic!("expected number, got {other:?}"),
    }
}

fn as_arr(v: &Value) -> &[Value] {
    match v {
        Value::Array(a) => a.as_slice(),
        other => panic!("expected array, got {other:?}"),
    }
}

/// Validates a `fid-trust-canon::Value` against a `fid-registry` JSON Schema (`CValue`).
fn check_conforms(schema: &fid_registry::decl::rt::CValue, value: &Value, path: &str) -> Result<(), String> {
    use fid_registry::decl::rt::CValue;
    let CValue::Obj(s) = schema else {
        return Err(format!("{path}: schema is not an object"));
    };
    if let Some(CValue::Arr(allowed)) = s.get("enum") {
        let Value::String(got) = value else {
            return Err(format!("{path}: expected enum string, got {value:?}"));
        };
        if !allowed.iter().any(|a| matches!(a, CValue::Str(x) if x == got)) {
            return Err(format!("{path}: {got:?} not in enum {allowed:?}"));
        }
        return Ok(());
    }
    let Some(CValue::Str(ty)) = s.get("type") else {
        return Ok(());
    };
    match ty.as_str() {
        "object" => {
            let Value::Object(fields) = value else {
                return Err(format!("{path}: expected object, got {value:?}"));
            };
            let props = match s.get("properties") {
                Some(CValue::Obj(p)) => p,
                _ => return Err(format!("{path}: object schema missing properties")),
            };
            if let Some(CValue::Arr(req)) = s.get("required") {
                for r in req {
                    let CValue::Str(name) = r else { continue };
                    if !fields.iter().any(|(k, _)| k == name) {
                        return Err(format!("{path}: missing required field {name:?}"));
                    }
                }
            }
            for (k, v) in fields {
                let Some(prop_schema) = props.get(k) else {
                    if matches!(s.get("additionalProperties"), Some(CValue::Bool(false))) {
                        return Err(format!("{path}: unexpected property {k:?}"));
                    }
                    continue;
                };
                check_conforms(prop_schema, v, &format!("{path}.{k}"))?;
            }
            Ok(())
        }
        "array" => {
            let Value::Array(items) = value else {
                return Err(format!("{path}: expected array, got {value:?}"));
            };
            if let Some(item_schema) = s.get("items") {
                for (i, item) in items.iter().enumerate() {
                    check_conforms(item_schema, item, &format!("{path}[{i}]"))?;
                }
            }
            Ok(())
        }
        "string" => {
            let Value::String(got) = value else {
                return Err(format!("{path}: expected string, got {value:?}"));
            };
            if let Some(CValue::Num(min)) = s.get("minLength") {
                if (got.chars().count() as f64) < *min {
                    return Err(format!("{path}: string shorter than {min}"));
                }
            }
            if let Some(CValue::Num(max)) = s.get("maxLength") {
                if (got.chars().count() as f64) > *max {
                    return Err(format!("{path}: string longer than {max}"));
                }
            }
            if let Some(CValue::Str(pat)) = s.get("pattern") {
                let ok = match pat.as_str() {
                    "^(0|[1-9][0-9]*)$" => {
                        !got.is_empty()
                            && got.bytes().all(|b| b.is_ascii_digit())
                            && (got == "0" || !got.starts_with('0'))
                    }
                    "^sha256:[0-9a-f]{64}$" => got.strip_prefix("sha256:").is_some_and(|h| {
                        h.len() == 64 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    }),
                    other => panic!("unhandled schema pattern {other:?}"),
                };
                if !ok {
                    return Err(format!("{path}: {got:?} does not match pattern {pat:?}"));
                }
            }
            Ok(())
        }
        "integer" => {
            let Value::Number(n) = value else {
                return Err(format!("{path}: expected integer, got {value:?}"));
            };
            if n.fract() != 0.0 {
                return Err(format!("{path}: expected integer, got float {n}"));
            }
            if let Some(CValue::Num(min)) = s.get("minimum") {
                if *n < *min {
                    return Err(format!("{path}: {n} < minimum {min}"));
                }
            }
            if let Some(CValue::Num(max)) = s.get("maximum") {
                if *n > *max {
                    return Err(format!("{path}: {n} > maximum {max}"));
                }
            }
            Ok(())
        }
        "boolean" => {
            if !matches!(value, Value::Bool(_)) {
                return Err(format!("{path}: expected boolean, got {value:?}"));
            }
            Ok(())
        }
        other => Err(format!("{path}: unsupported schema type {other:?}")),
    }
}

/// Builds a synthetic journal entry line with valid canonical `header.hash`.
fn make_entry_line(seq: u64, prev: Digest, kind: &str) -> (String, Digest) {
    let mut header = vec![
        ("ai_assistance".to_owned(), Value::String("none".to_owned())),
        ("branch".to_owned(), Value::String("main".to_owned())),
        ("kind".to_owned(), Value::String(kind.to_owned())),
        ("logical_parent".to_owned(), Value::Null),
        ("prev".to_owned(), Value::String(prev.to_string())),
        ("schema_version".to_owned(), Value::Number(1.0)),
        ("seq".to_owned(), Value::String(seq.to_string())),
        ("stream".to_owned(), Value::String("main".to_owned())),
        (
            "time".to_owned(),
            Value::Object(vec![
                ("mono_ns".to_owned(), Value::String((seq * 1000).to_string())),
                ("sync".to_owned(), Value::String("test".to_owned())),
                ("utc".to_owned(), Value::String("2026-01-01T00:00:00Z".to_owned())),
            ]),
        ),
    ];
    if kind == "command" {
        header.push((
            "command".to_owned(),
            Value::Object(vec![
                ("algorithms".to_owned(), Value::Array(Vec::new())),
                ("class".to_owned(), Value::String("W2".to_owned())),
                ("determinism".to_owned(), Value::String("D0".to_owned())),
                ("id".to_owned(), Value::String("project.step".to_owned())),
                ("inputs".to_owned(), Value::Array(Vec::new())),
                ("major".to_owned(), Value::Number(1.0)),
                ("outputs".to_owned(), Value::Array(Vec::new())),
                ("seed".to_owned(), Value::String("0".to_owned())),
            ]),
        ));
    }
    let header_bytes = canon_serialize(&Value::Object(header.clone())).unwrap();
    let hash = entry_hash(&header_bytes).unwrap();
    header.push(("hash".to_owned(), Value::String(hash.to_string())));
    let line_bytes = canon_serialize(&Value::Object(vec![("header".to_owned(), Value::Object(header))])).unwrap();
    (String::from_utf8(line_bytes).unwrap(), hash)
}

// ------------------------------------------------------------------ AC-1

/// AC-1: `fid verify check-chain fixtures/pkg/golden/points.zip --json` returns `VALID`
/// with zero findings, and every other golden Result Package also verifies `VALID`.
#[test]
fn ac1_golden_points_and_all_goldens_are_valid() {
    let points = pkg_goldens().join("points.zip");
    let p_str = points.to_str().unwrap();
    let out = invoke_cli(&FileOpener, &[CLI_CHECK_CHAIN, p_str, "--json"]);
    assert_eq!(out.code, 0, "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(out.stderr.is_empty());
    assert!(out.stdout.ends_with(b"\n"));

    let json = canon_parse(out.stdout.trim_ascii_end()).unwrap();
    assert_eq!(as_str(field(&json, "verdict").unwrap()), "VALID");
    assert_eq!(as_u64(field(&json, "entries_verified").unwrap()), 7);
    assert!(as_arr(field(&json, "findings").unwrap()).is_empty());
    assert!(as_arr(field(&json, "checkpoints").unwrap()).is_empty());
    assert!(as_arr(field(&json, "unlinked").unwrap()).is_empty());
    assert!(as_arr(field(&json, "mislinked").unwrap()).is_empty());
    assert_eq!(as_arr(field(&json, "links").unwrap()).len(), 3);

    // Validate against command result schema.
    let decls = declarations();
    check_conforms(&decls[0].def.result_schema, &json, "points.zip").unwrap();

    // Also confirm all other golden Result Packages with journal segments verify VALID.
    for name in ["features.zip", "results.zip", "uec0.zip", "two-signatures.zip"] {
        let path = pkg_goldens().join(name);
        let bytes = fs::read(&path).unwrap();
        let report = check_package(Cursor::new(bytes), None).unwrap();
        assert_eq!(report.verdict, ChainVerdict::Valid, "{name}: {:?}", report.findings);
        assert!(report.findings.is_empty(), "{name}");
        assert!(report.unlinked.is_empty(), "{name}");
        assert!(report.mislinked.is_empty(), "{name}");
        assert!(!report.links.is_empty(), "{name}");
        let v = canon_parse(&report.to_canonical_bytes()).unwrap();
        check_conforms(&decls[0].def.result_schema, &v, name).unwrap();
    }
}

// ------------------------------------------------------------------ AC-2

/// AC-2: Every tamper fixture in `fixtures/chain/tamper/{modified,removed,inserted,reordered,truncated}.zip`
/// reports `BROKEN` at index 17 with the exact expected finding kind(s) from `expect.json`,
/// while `base.zip` and `anchored-ckpt.zip` report `VALID` and `unanchored.zip` reports `UNANCHORED`.
#[test]
fn ac2_tamper_fixtures_match_expect_json() {
    let dir = tamper_fixtures();
    let expect_bytes = fs::read(dir.join("expect.json")).unwrap();
    let expect_val = canon_parse(&expect_bytes).unwrap();
    let Value::Object(cases) = field(&expect_val, "cases").unwrap() else {
        panic!("expect.json missing cases object");
    };

    let root = load_both_trust_root();
    let rev = Revocations::default();
    let ring = Keyring::default();
    let trust = TrustInputs { trust_root: &root, revocations: &rev, keyring: &ring, as_of: None };
    let decls = declarations();
    let trust_str = both_trust_root_path().to_str().unwrap().to_owned();

    // Ensure all 5 required AC-2 fixtures are present in expect.json.
    for required in ["modified.zip", "removed.zip", "inserted.zip", "reordered.zip", "truncated.zip"] {
        assert!(cases.iter().any(|(k, _)| k == required), "missing {required} in expect.json");
    }

    for (zip_name, exp) in cases {
        let zip_path = dir.join(zip_name);
        let zip_bytes = fs::read(&zip_path).expect(zip_name);
        let report = check_package(Cursor::new(zip_bytes), Some(&trust)).expect(zip_name);

        let exp_verdict = as_str(field(exp, "verdict").unwrap());
        let exp_verified = as_u64(field(exp, "entries_verified").unwrap());
        let exp_kinds: Vec<&str> = as_arr(field(exp, "expected_kinds").unwrap()).iter().map(as_str).collect();
        let exp_unlinked = as_u64(field(exp, "unlinked_count").unwrap()) as usize;

        assert_eq!(report.verdict.as_str(), exp_verdict, "{zip_name}");
        assert_eq!(report.entries_verified, exp_verified, "{zip_name}");
        assert_eq!(report.unlinked.len(), exp_unlinked, "{zip_name}");

        let got_kinds: Vec<&str> = report.findings.iter().map(|f| f.kind.as_str()).collect();
        assert_eq!(got_kinds, exp_kinds, "{zip_name}: findings = {:?}", report.findings);

        match field(exp, "first_failing_index").unwrap() {
            Value::Null => assert!(report.findings.is_empty(), "{zip_name}"),
            Value::Number(n) => {
                let want_idx = *n as u64;
                assert!(!report.findings.is_empty(), "{zip_name}");
                for f in &report.findings {
                    assert_eq!(f.index, want_idx, "{zip_name}: {f:?}");
                }
            }
            other => panic!("{zip_name}: unexpected first_failing_index {other:?}"),
        }

        // Check CLI JSON conforms to result_schema and matches library verdict.
        let cli_out = invoke_cli(
            &FileOpener,
            &[CLI_CHECK_CHAIN, zip_path.to_str().unwrap(), "--trust-root", &trust_str, "--json"],
        );
        let want_code = if exp_verdict == "VALID" && exp_unlinked == 0 { 0 } else { 1 };
        assert_eq!(cli_out.code, want_code, "{zip_name}");
        let cli_json = canon_parse(cli_out.stdout.trim_ascii_end()).unwrap();
        check_conforms(&decls[0].def.result_schema, &cli_json, zip_name).unwrap();
        assert_eq!(as_str(field(&cli_json, "verdict").unwrap()), exp_verdict, "{zip_name}");
    }

    // Specifically verify signed checkpoints in base.zip (seq 12) and anchored-ckpt.zip (seq 6).
    let base_report = check_package(Cursor::new(fs::read(dir.join("base.zip")).unwrap()), Some(&trust)).unwrap();
    assert_eq!(base_report.checkpoints.len(), 1);
    assert_eq!(base_report.checkpoints[0].status, CheckpointStatus::Match);
    assert_eq!(base_report.checkpoints[0].signature.as_ref().map(|s| s.status), Some(SignatureStatus::ValidTrusted));

    let anchored = check_package(Cursor::new(fs::read(dir.join("anchored-ckpt.zip")).unwrap()), Some(&trust)).unwrap();
    assert_eq!(anchored.verdict, ChainVerdict::Valid);
    assert_eq!(anchored.checkpoints.len(), 1);
    assert_eq!(anchored.checkpoints[0].index, Some(6));
    assert_eq!(anchored.checkpoints[0].status, CheckpointStatus::Match);
    assert_eq!(anchored.checkpoints[0].signature.as_ref().map(|s| s.status), Some(SignatureStatus::ValidTrusted));
}

// ------------------------------------------------------------------ AC-4

/// AC-4: `verify_check_chain` via MCP with opaque handles returns bytes identical to
/// `fid verify check-chain --json`.
#[test]
fn ac4_mcp_matches_cli_json_byte_for_byte() {
    let points = pkg_goldens().join("points.zip");
    let cli = invoke_cli(&FileOpener, &[CLI_CHECK_CHAIN, points.to_str().unwrap(), "--json"]);
    assert_eq!(cli.code, 0);

    let mut handles = HandleTable::new();
    handles.insert("pkg-points", &points);
    let mcp = invoke_mcp(&handles, MCP_CHECK_CHAIN, br#"{"package":"pkg-points"}"#).expect("mcp ok");
    assert_eq!(mcp.as_slice(), cli.stdout.trim_ascii_end());

    // Also verify with optional trust_root handle on tamper/base.zip.
    let base = tamper_fixtures().join("base.zip");
    let root = both_trust_root_path();
    let cli_signed = invoke_cli(
        &FileOpener,
        &[CLI_CHECK_CHAIN, base.to_str().unwrap(), "--trust-root", root.to_str().unwrap(), "--json"],
    );
    assert_eq!(cli_signed.code, 0);
    handles.insert("pkg-base", &base);
    handles.insert("root-both", &root);
    let mcp_signed =
        invoke_mcp(&handles, MCP_CHECK_CHAIN, br#"{"package":"pkg-base","trust_root":"root-both"}"#).expect("mcp ok");
    assert_eq!(mcp_signed.as_slice(), cli_signed.stdout.trim_ascii_end());

    // MCP rejects raw paths not in HandleTable, unknown tools, and unknown params.
    let err_raw = invoke_mcp(&handles, MCP_CHECK_CHAIN, format!(r#"{{"package":"{}"}}"#, points.display()).as_bytes())
        .unwrap_err();
    let err_val = canon_parse(&err_raw).unwrap();
    assert_eq!(as_str(field(field(&err_val, "error").unwrap(), "code").unwrap()), "pkg.io");

    let err_tool = invoke_mcp(&handles, "verify_unknown", br#"{"package":"pkg-points"}"#).unwrap_err();
    let err_tool_v = canon_parse(&err_tool).unwrap();
    assert_eq!(as_str(field(field(&err_tool_v, "error").unwrap(), "code").unwrap()), "mcp.unknown_tool");

    let err_extra = invoke_mcp(&handles, MCP_CHECK_CHAIN, br#"{"package":"pkg-points","extra":"1"}"#).unwrap_err();
    let err_extra_v = canon_parse(&err_extra).unwrap();
    assert_eq!(as_str(field(field(&err_extra_v, "error").unwrap(), "code").unwrap()), "params.invalid");
}

// ------------------------------------------------------------------ RESLINK composition & edge cases

#[test]
fn composes_with_reslink_fixtures() {
    // foreign-hash.zip: chain VALID, hole-2 UNLINKED.
    let foreign =
        check_package(Cursor::new(fs::read(reslink_fixtures().join("foreign-hash.zip")).unwrap()), None).unwrap();
    assert_eq!(foreign.verdict, ChainVerdict::Valid);
    assert_eq!(foreign.unlinked, vec!["hole-2"]);
    assert!(foreign.mislinked.is_empty());

    // broken-at-5.zip: chain BROKEN at index 5 (HASH_MISMATCH), hole-early LINKED, hole-late UNLINKED.
    let broken =
        check_package(Cursor::new(fs::read(reslink_fixtures().join("broken-at-5.zip")).unwrap()), None).unwrap();
    assert_eq!(broken.verdict, ChainVerdict::Broken);
    assert_eq!(broken.entries_verified, 5);
    assert_eq!(broken.findings.len(), 1);
    assert_eq!(broken.findings[0].index, 5);
    assert_eq!(broken.findings[0].kind, FindingKind::HashMismatch);
    assert_eq!(broken.unlinked, vec!["hole-late"]);
    assert!(broken.links.iter().any(|l| l.record_id == "hole-early" && l.status == LinkStatus::Linked));

    // wrong-entry.zip: chain VALID, hole-2 & hole-3 MISLINKED.
    let wrong =
        check_package(Cursor::new(fs::read(reslink_fixtures().join("wrong-entry.zip")).unwrap()), None).unwrap();
    assert_eq!(wrong.verdict, ChainVerdict::Valid);
    assert!(wrong.unlinked.is_empty());
    assert_eq!(wrong.mislinked, vec!["hole-2", "hole-3"]);
}

#[test]
fn isolated_finding_kinds_and_truncation_edge_cases() {
    let (l0, h0) = make_entry_line(1, ZERO_DIGEST, "session_header");
    let (l1, h1) = make_entry_line(2, h0, "command");
    let (l2, _h2) = make_entry_line(3, h1, "command");

    // 1. Valid segment with and without trailing newline.
    let ok_nl = walk_segment_bytes(format!("{l0}\n{l1}\n{l2}\n").as_bytes(), &[]).unwrap();
    assert_eq!(ok_nl.verdict, ChainVerdict::Valid);
    assert_eq!(ok_nl.entries_verified, 3);
    let ok_no_nl = walk_segment_bytes(format!("{l0}\n{l1}\n{l2}").as_bytes(), &[]).unwrap();
    assert_eq!(ok_no_nl.verdict, ChainVerdict::Valid);
    assert_eq!(ok_no_nl.entries_verified, 3);

    // 2. Pure LINK_MISMATCH (seq == 3 matches, prev != h1).
    let (bad_prev, _) = make_entry_line(3, Digest::from_bytes([0xee; 32]), "command");
    let r_link = walk_segment_bytes(format!("{l0}\n{l1}\n{bad_prev}\n").as_bytes(), &[]).unwrap();
    assert_eq!(r_link.verdict, ChainVerdict::Broken);
    assert_eq!(r_link.first_failing_index, Some(2));
    assert_eq!(r_link.findings.len(), 1);
    assert_eq!(r_link.findings[0].kind, FindingKind::LinkMismatch);

    // 3. Pure SEQUENCE_GAP (prev == h1 matches, seq == 4 skips 3).
    let (gap, _) = make_entry_line(4, h1, "command");
    let r_gap = walk_segment_bytes(format!("{l0}\n{l1}\n{gap}\n").as_bytes(), &[]).unwrap();
    assert_eq!(r_gap.verdict, ChainVerdict::Broken);
    assert_eq!(r_gap.first_failing_index, Some(2));
    assert_eq!(r_gap.findings.len(), 1);
    assert_eq!(r_gap.findings[0].kind, FindingKind::SequenceGap);

    // 4. Pure SEQUENCE_REPEAT (prev == h1 matches, seq == 2 repeats).
    let (rep, _) = make_entry_line(2, h1, "command");
    let r_rep = walk_segment_bytes(format!("{l0}\n{l1}\n{rep}\n").as_bytes(), &[]).unwrap();
    assert_eq!(r_rep.verdict, ChainVerdict::Broken);
    assert_eq!(r_rep.first_failing_index, Some(2));
    assert_eq!(r_rep.findings.len(), 1);
    assert_eq!(r_rep.findings[0].kind, FindingKind::SequenceRepeat);

    // 5. Truncated last line WITH trailing newline -> TRUNCATED (REQ-005).
    let r_trunc_nl = walk_segment_bytes(format!("{l0}\n{l1}\n{{\"header\":{{\"seq\":\"3\"\n").as_bytes(), &[]).unwrap();
    assert_eq!(r_trunc_nl.verdict, ChainVerdict::Broken);
    assert_eq!(r_trunc_nl.first_failing_index, Some(2));
    assert_eq!(r_trunc_nl.findings[0].kind, FindingKind::Truncated);

    // 6. Corrupted MIDDLE line followed by another line -> canon.invalid passthrough error.
    let err_mid = walk_segment_bytes(format!("{l0}\n{{\"bad\":\n{l2}\n").as_bytes(), &[]).unwrap_err();
    assert_eq!(err_mid.code(), "canon.invalid");
    assert!(matches!(err_mid, CheckChainError::Canon { index: 1, .. }));

    // 7. Empty segment -> UNANCHORED at index 0.
    let r_empty = walk_segment_bytes(b"", &[]).unwrap();
    assert_eq!(r_empty.verdict, ChainVerdict::Broken);
    assert_eq!(r_empty.first_failing_index, Some(0));
    assert_eq!(r_empty.findings[0].kind, FindingKind::Unanchored);

    // 8. Checkpoint-anchored start at seq 10 without session_header.
    let (c10, h10) = make_entry_line(10, Digest::from_bytes([0x11; 32]), "command");
    let (c11, _) = make_entry_line(11, h10, "command");
    let unanch = walk_segment_bytes(format!("{c10}\n{c11}\n").as_bytes(), &[]).unwrap();
    assert_eq!(unanch.verdict, ChainVerdict::Broken);
    assert_eq!(unanch.findings[0].kind, FindingKind::Unanchored);

    let anchor = AnchorCheckpoint { seq: 10, head: h10, stream: "main".to_owned() };
    let anch = walk_segment_bytes(format!("{c10}\n{c11}\n").as_bytes(), &[anchor]).unwrap();
    assert_eq!(anch.verdict, ChainVerdict::Valid);
    assert_eq!(anch.entries_verified, 2);
}

// ------------------------------------------------------------------ declaration & error passthrough

#[test]
fn declaration_validates_and_schema_checks() {
    let decls = declarations();
    assert_eq!(decls.len(), 1);
    let d = &decls[0].def;
    assert_eq!((d.id.as_str(), d.major), ("verify.check_chain", 1));
    assert_eq!(tool_name(&d.id, d.major), MCP_CHECK_CHAIN);
    let raw: Vec<_> = decls.iter().map(CommandDecl::to_validation).collect();
    assert_eq!(validate(&raw), Vec::new());
    let catalogue = Catalogue::geo_frame().unwrap();
    for schema in [&d.params_schema, &d.result_schema] {
        assert_eq!(check_schema(schema, &catalogue), Vec::new());
    }
    check_conforms(&d.params_schema, &canon_parse(br#"{"package":"pkg-1"}"#).unwrap(), "params").unwrap();
    check_conforms(
        &d.params_schema,
        &canon_parse(br#"{"package":"pkg-1","trust_root":"root-1"}"#).unwrap(),
        "params_with_root",
    )
    .unwrap();
    assert!(check_conforms(&d.params_schema, &canon_parse(br#"{"package":""}"#).unwrap(), "empty_pkg").is_err());
    assert!(
        check_conforms(&d.params_schema, &canon_parse(br#"{"package":"p","extra":"x"}"#).unwrap(), "extra").is_err()
    );
}

#[test]
fn package_errors_and_stream_verified_invariant() {
    // Missing journal/segment.jsonl -> pkg.malformed.
    let minimal = pkg_goldens().join("minimal.zip");
    let err = check_package(Cursor::new(fs::read(&minimal).unwrap()), None).unwrap_err();
    assert_eq!(err.code(), "pkg.malformed");

    // Build a valid package, tamper with a byte inside journal/segment.jsonl after manifest
    // sealing, and confirm stream_verified_member exposes ZERO bytes before failing.
    let (l0, _) = make_entry_line(1, ZERO_DIGEST, "session_header");
    let seg_body = format!("{l0}\n").into_bytes();
    let built = build_package(&PackageInput {
        created_by: "stream test".into(),
        evidence_tier: EvidenceTier::Results,
        profile_digest: Digest::from_bytes([0xaa; 32]),
        kernel_manifest_digest: Digest::from_bytes([0xbb; 32]),
        result_count: 0,
        members: vec![MemberInput {
            path: PkgPath::parse("journal/segment.jsonl").unwrap(),
            media_type: "application/x-ndjson".into(),
            body: seg_body.clone(),
        }],
    })
    .unwrap();

    // Verify clean streaming delivers exact bytes.
    let mut reader = PackageReader::open(Cursor::new(built.container.clone())).unwrap();
    let mut streamed = Vec::new();
    let found = reader
        .stream_verified_member("journal/segment.jsonl", &mut |chunk| {
            streamed.extend_from_slice(chunk);
            Ok(())
        })
        .unwrap();
    assert_eq!(found, Some(()));
    assert_eq!(streamed, seg_body);

    // Flip one byte inside the stored-block payload of journal/segment.jsonl in the ZIP.
    let needle = b"\"session_header\"";
    let pos = built.container.windows(needle.len()).position(|w| w == needle).expect("find needle in container");
    let mut tampered = built.container.clone();
    tampered[pos + 1] ^= 0x01;

    let mut reader2 = PackageReader::open(Cursor::new(tampered)).unwrap();
    let mut saw_bytes = 0usize;
    let res = reader2.stream_verified_member("journal/segment.jsonl", &mut |chunk| {
        saw_bytes += chunk.len();
        Ok(())
    });
    assert!(res.is_err());
    assert_eq!(res.unwrap_err().code(), "pkg.digest_mismatch");
    assert_eq!(saw_bytes, 0, "stream_verified_member must never expose unverified bytes");

    // CLI text output and usage errors.
    let usage = invoke_cli(&FileOpener, &[CLI_CHECK_CHAIN]);
    assert_eq!(usage.code, 2);
    let points = pkg_goldens().join("points.zip");
    let human = invoke_cli(&FileOpener, &[CLI_CHECK_CHAIN, points.to_str().unwrap()]);
    assert_eq!(human.code, 0);
    let human_str = String::from_utf8(human.stdout).unwrap();
    assert!(human_str.contains("verdict VALID"));
    assert!(human_str.contains("entries_verified 7"));
}

#[test]
fn cli_reports_bad_trust_root_as_troot_malformed() {
    let points = pkg_goldens().join("points.zip");
    let bad_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let out = invoke_cli(
        &FileOpener,
        &[CLI_CHECK_CHAIN, points.to_str().unwrap(), "--trust-root", bad_root.to_str().unwrap(), "--json"],
    );
    assert_eq!(out.code, 1);
    let v = canon_parse(out.stdout.trim_ascii_end()).unwrap();
    assert_eq!(as_str(field(field(&v, "error").unwrap(), "code").unwrap()), "troot.malformed");
}
