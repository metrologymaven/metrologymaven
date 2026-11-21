//! Acceptance criteria of spec node CORE.REG.QRY (AC-1 .. AC-5; AC-6 is in
//! `fid-registry-build/tests/registry_query.rs`, which has the bus). Expected results come from
//! `fixtures/core/reg/qry/make_cases.py`, an independent Python reference, never from this
//! crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_registry::manifest::{ActivityEntry, ManifestInForce, Surface};
use fid_registry::query::{
    self, call, diff_manifests, invoke_cli, invoke_mcp, levenshtein, read_manifest, Audience, QueryError, Registry,
    DESCRIBE_COMMAND, DIFF_MANIFESTS, LIST_COMMANDS, MCP_DESCRIBE, MCP_DIFF, MCP_LIST,
};
use fid_registry::{validate, Catalogue, CommandDecl, MAX_MANIFEST_BYTES};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/core/reg")
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(rel)).unwrap()
}

fn decls(rel: &str) -> Vec<CommandDecl> {
    let CValue::Obj(mut o) = parse(&read(rel)).unwrap() else { panic!("{rel}: object expected") };
    let Some(CValue::Arr(items)) = o.remove("decls") else { panic!("{rel}: decls expected") };
    items.into_iter().map(CommandDecl).collect()
}

fn registry(source: &str) -> Registry {
    let rel = match source {
        "valid" => "decls/valid.json",
        "qry" => "qry/decls.json",
        other => panic!("unknown source {other}"),
    };
    let m = ManifestInForce::new(&decls(rel), Catalogue::geo_frame().unwrap()).unwrap();
    Registry::new(&m).unwrap()
}

fn get<'v>(v: &'v CValue, k: &str) -> &'v CValue {
    match v {
        CValue::Obj(o) => o.get(k).unwrap_or_else(|| panic!("missing {k}")),
        _ => panic!("not an object"),
    }
}

fn no_files(_: &str) -> Option<Vec<u8>> {
    None
}

fn manifests(p: &str) -> Option<Vec<u8>> {
    let name = p.strip_prefix("root:imports/").unwrap_or(p);
    match name {
        "v1.json" | "v2.json" | "tampered.json" | "valid.manifest.json" => {
            Some(std::fs::read(fixtures().join("manifest").join(name)).unwrap())
        }
        "audience_a.manifest.json" | "audience_b.manifest.json" => {
            Some(std::fs::read(fixtures().join("qry").join(name)).unwrap())
        }
        _ => None,
    }
}

// ------------------------------------------------------------------ the oracle's cases

#[test]
fn every_oracle_case_matches_for_its_audience() {
    let cases = parse(&read("qry/cases.json")).unwrap();
    let CValue::Arr(cases) = get(&cases, "cases") else { panic!() };
    assert!(cases.len() >= 40);
    let regs: BTreeMap<&str, Registry> = [("valid", registry("valid")), ("qry", registry("qry"))].into();
    for case in cases {
        let CValue::Str(name) = get(case, "name") else { panic!() };
        let CValue::Str(source) = get(case, "source") else { panic!() };
        let CValue::Str(audience) = get(case, "audience") else { panic!() };
        let CValue::Str(tool) = get(case, "tool") else { panic!() };
        let params = get(case, "params");
        let r = &regs[source.as_str()];
        let mut log = Vec::new();
        let (command, mcp) = match tool.as_str() {
            "list" => (LIST_COMMANDS, MCP_LIST),
            "describe" => (DESCRIBE_COMMAND, MCP_DESCRIBE),
            other => panic!("unknown tool {other}"),
        };
        let got = match audience.as_str() {
            "agent" => invoke_mcp(r, &mut log, mcp, &to_canonical_bytes(params).unwrap(), &no_files),
            "person" => call(r, &mut log, Surface::Cli, Audience::Person, command, params, &no_files),
            other => panic!("unknown audience {other}"),
        };
        assert_eq!(log.len(), 1, "{name}: one Activity Log record");
        let expect = get(case, "expect");
        match (expect, got) {
            (CValue::Obj(e), Ok(bytes)) if e.contains_key("ok") => {
                assert_eq!(parse(&bytes).unwrap(), e["ok"], "{name}");
                // Canonical bytes, so the output is byte-stable.
                assert_eq!(bytes, to_canonical_bytes(&e["ok"]).unwrap(), "{name}");
            }
            (CValue::Obj(e), Err(err)) if e.contains_key("error") => {
                let want = &e["error"];
                assert_eq!(&CValue::Str(err.code().into()), get(want, "code"), "{name}: {err}");
                if let CValue::Obj(w) = want {
                    if let Some(s) = w.get("suggestions") {
                        assert_eq!(get(&err.to_value(), "suggestions"), s, "{name}");
                    }
                }
            }
            (e, got) => panic!("{name}: expected {e:?}, got {got:?}"),
        }
    }
}

// ------------------------------------------------------------------ AC-1, AC-2, AC-4

fn expected(name: &str) -> CValue {
    let cases = parse(&read("qry/cases.json")).unwrap();
    let CValue::Arr(cases) = get(&cases, "cases") else { panic!() };
    let case = cases.iter().find(|c| get(c, "name") == &CValue::Str(name.into())).unwrap();
    get(case, "expect").clone()
}

#[test]
fn ac1_cli_list_commands_lists_every_non_hidden_command_in_order() {
    let r = registry("valid");
    let mut log = Vec::new();
    let out = invoke_cli(&r, &mut log, Audience::Person, &["list-commands", "--json"], &no_files).unwrap();
    assert_eq!(out, to_canonical_bytes(get(&expected("ac1_valid_all"), "ok")).unwrap());
    assert_eq!(log, vec![ActivityEntry { command: LIST_COMMANDS, surface: Surface::Cli }]);
    // Hidden majors (qry fixture) are left out without --include-hidden.
    let q = registry("qry");
    let out = invoke_cli(&q, &mut log, Audience::Person, &["list-commands", "--json"], &no_files).unwrap();
    assert_eq!(out, to_canonical_bytes(get(&expected("qry_person_all"), "ok")).unwrap());
    let hidden =
        invoke_cli(&q, &mut log, Audience::Person, &["list-commands", "--include-hidden", "--json"], &no_files)
            .unwrap();
    assert_eq!(hidden, to_canonical_bytes(get(&expected("qry_person_hidden"), "ok")).unwrap());
}

#[test]
fn ac2_cli_side_effect_filter_lists_only_w2() {
    let r = registry("valid");
    let mut log = Vec::new();
    let out =
        invoke_cli(&r, &mut log, Audience::Person, &["list-commands", "--side-effect", "W2", "--json"], &no_files)
            .unwrap();
    assert_eq!(out, to_canonical_bytes(get(&expected("ac2_valid_w2"), "ok")).unwrap());
    let v = parse(&out).unwrap();
    let CValue::Arr(cmds) = get(&v, "commands") else { panic!() };
    assert!(cmds.iter().all(|c| get(c, "side_effect") == &CValue::Str("W2".into())));
    assert_eq!(cmds.len(), 1);
}

#[test]
fn ac4_describe_unknown_id_is_not_found_and_suggests_run_bestfit() {
    let r = registry("valid");
    let mut log = Vec::new();
    let err =
        invoke_cli(&r, &mut log, Audience::Person, &["describe-command", "align.run_bestfi@1"], &no_files).unwrap_err();
    assert_eq!(err.code(), "not_found");
    let QueryError::NotFound(s) = &err else { panic!() };
    assert_eq!(s.first().map(String::as_str), Some("align.run_bestfit"));
    let exp = expected("ac4_valid_bestfi");
    let want = get(get(&exp, "error"), "suggestions");
    assert_eq!(get(&err.to_value(), "suggestions"), want);
    assert_eq!(log, vec![ActivityEntry { command: DESCRIBE_COMMAND, surface: Surface::Cli }]);
    // The CLI and the parameter form agree, and the message echoes no input.
    assert!(!err.to_string().contains("bestfi"));
}

#[test]
fn describe_cli_parses_id_at_major() {
    let r = registry("qry");
    let mut log = Vec::new();
    let cli =
        invoke_cli(&r, &mut log, Audience::Person, &["describe-command", "align.run_bestfit@2", "--json"], &no_files)
            .unwrap();
    assert_eq!(cli, to_canonical_bytes(get(&expected("describe_hidden_by_major"), "ok")).unwrap());
    let latest =
        invoke_cli(&r, &mut log, Audience::Person, &["describe-command", "gdt.evaluate_flatness"], &no_files).unwrap();
    assert_eq!(latest, to_canonical_bytes(get(&expected("describe_latest_active"), "ok")).unwrap());
    for bad in [
        vec!["describe-command"],
        vec!["describe-command", "a@"],
        vec!["describe-command", "a@x"],
        vec!["describe-command", "a@-1"],
        vec!["describe-command", "a@4294967296"],
        vec!["describe-command", "a@99999999999"],
        vec!["describe-command", "a", "b"],
        vec!["describe-command", "a", "--json", "--json"],
        vec!["describe-command", "a", "--major", "1"],
        vec!["list-commands", "--domain"],
        vec!["list-commands", "--domain", "a", "--domain", "b"],
        vec!["list-commands", "--include-hidden", "--include-hidden"],
        vec!["list-commands", "extra"],
        vec!["list-commands", "--side-effect", "W9"],
        vec!["diff-manifests", "v1.json"],
        vec![],
    ] {
        let err = invoke_cli(&r, &mut log, Audience::Person, &bad, &no_files).unwrap_err();
        assert_eq!(err.code(), "bad_params", "{bad:?}");
    }
    let err = invoke_cli(&r, &mut log, Audience::Person, &["show-manifest"], &no_files).unwrap_err();
    assert_eq!(err, QueryError::UnknownCommand);
    let many = vec!["list-commands"; 13];
    assert_eq!(invoke_cli(&r, &mut log, Audience::Person, &many, &no_files).unwrap_err().code(), "bad_params");
}

// ------------------------------------------------------------------ AC-3, AC-5

#[test]
fn ac3_diff_v1_v2_has_exactly_the_five_entries() {
    let r = registry("valid");
    let mut log = Vec::new();
    let out =
        invoke_cli(&r, &mut log, Audience::Person, &["diff-manifests", "v1.json", "v2.json", "--json"], &manifests)
            .unwrap();
    let want = parse(&read("qry/v1_v2.changes.json")).unwrap();
    assert_eq!(parse(&out).unwrap(), want);
    assert_eq!(out, to_canonical_bytes(&want).unwrap());
    let CValue::Arr(changes) = get(&want, "changes") else { panic!() };
    assert_eq!(changes.len(), 5);
    let scan = changes.iter().find(|c| get(c, "id") == &CValue::Str("demo.scan".into())).unwrap();
    assert_eq!(get(scan, "compat"), &CValue::Str("breaking".into()));
    let measure = changes.iter().find(|c| get(c, "id") == &CValue::Str("demo.measure".into())).unwrap();
    assert_eq!(get(measure, "compat"), &CValue::Str("additive".into()));
    assert_eq!(log, vec![ActivityEntry { command: DIFF_MANIFESTS, surface: Surface::Cli }]);
    // A manifest against itself has no change; the same call twice gives the same bytes.
    let same = diff_manifests(Audience::Person, &read("manifest/v1.json"), &read("manifest/v1.json")).unwrap();
    assert_eq!(get(&same, "changes"), &CValue::Arr(Vec::new()));
    assert_eq!(get(&same, "a_manifest_digest"), get(&want, "a_manifest_digest"));
    assert_eq!(get(&same, "b_manifest_digest"), get(&want, "a_manifest_digest"));
    let again =
        invoke_cli(&r, &mut log, Audience::Person, &["diff-manifests", "v1.json", "v2.json"], &manifests).unwrap();
    assert_eq!(again, out);
}

#[test]
fn ac5_tampered_manifest_is_manifest_invalid_and_only_the_activity_log_is_written() {
    let r = registry("valid");
    let mut log = Vec::new();
    for args in [["diff-manifests", "tampered.json", "v2.json"], ["diff-manifests", "v1.json", "tampered.json"]] {
        let err = invoke_cli(&r, &mut log, Audience::Person, &args, &manifests).unwrap_err();
        assert_eq!(err.code(), "manifest_invalid", "{err}");
    }
    // The registry API has no journal: the only record of either call is its Activity Log
    // entry (REQ-007); R0 calls never reach the Record Journal.
    assert_eq!(log, vec![ActivityEntry { command: DIFF_MANIFESTS, surface: Surface::Cli }; 2]);
    assert_eq!(read_manifest(&read("manifest/tampered.json")).unwrap_err().code(), "manifest_invalid");
    // The untampered source of tampered.json passes.
    assert!(read_manifest(&read("manifest/valid.manifest.json")).is_ok());
}

fn edit(rel: &str, f: impl Fn(&mut BTreeMap<String, CValue>)) -> Vec<u8> {
    let CValue::Obj(mut o) = parse(&read(rel)).unwrap() else { panic!() };
    f(&mut o);
    to_canonical_bytes(&CValue::Obj(o)).unwrap()
}

fn first_command(o: &mut BTreeMap<String, CValue>) -> &mut BTreeMap<String, CValue> {
    let Some(CValue::Arr(items)) = o.get_mut("commands") else { panic!() };
    let Some(CValue::Obj(c)) = items.first_mut() else { panic!() };
    c
}

#[test]
fn diff_refuses_every_malformed_manifest() {
    let invalid = |bytes: &[u8]| read_manifest(bytes).unwrap_err().code();
    assert_eq!(invalid(b"not json"), "manifest_invalid");
    assert_eq!(invalid(b"[]"), "manifest_invalid");
    assert_eq!(
        invalid(b"{\"commands\":[],\"format\":1,\"format\":1,\"quantity_catalogue_digest\":\"x\"}"),
        "manifest_invalid"
    );
    let base = "manifest/v1.json";
    let cases: Vec<Vec<u8>> = vec![
        edit(base, |o| {
            o.insert("format".into(), CValue::Num(2.0));
        }),
        edit(base, |o| {
            o.insert("extra".into(), CValue::Bool(true));
        }),
        edit(base, |o| {
            o.remove("quantity_catalogue_digest");
        }),
        edit(base, |o| {
            o.insert("quantity_catalogue_digest".into(), CValue::Str("sha256:00".into()));
        }),
        edit(base, |o| {
            o.insert("commands".into(), CValue::Obj(BTreeMap::new()));
        }),
        edit(base, |o| {
            o.insert("commands".into(), CValue::Arr(vec![CValue::Num(1.0)]));
        }),
        // A required normal-form field missing (manifest.schema.json), digest still matching
        // what is left would not help: the field check comes first.
        edit(base, |o| {
            first_command(o).remove("cli_bare");
        }),
        edit(base, |o| {
            first_command(o).remove("definition_digest");
        }),
        edit(base, |o| {
            first_command(o).insert("definition_digest".into(), CValue::Str("sha256:zz".into()));
        }),
        edit(base, |o| {
            first_command(o).insert("unknown".into(), CValue::Bool(true));
        }),
        // A repeated (ID, major).
        edit(base, |o| {
            let Some(CValue::Arr(items)) = o.get_mut("commands") else { panic!() };
            let first = items[0].clone();
            items.push(first);
        }),
    ];
    for (i, bytes) in cases.iter().enumerate() {
        assert_eq!(invalid(bytes), "manifest_invalid", "case {i}");
    }
    let big = vec![b' '; MAX_MANIFEST_BYTES + 1];
    assert_eq!(invalid(&big), "input_too_large");
    let r = registry("valid");
    let mut log = Vec::new();
    let huge = |_: &str| Some(vec![b' '; MAX_MANIFEST_BYTES + 1]);
    let err = invoke_cli(&r, &mut log, Audience::Person, &["diff-manifests", "a", "b"], &huge).unwrap_err();
    assert_eq!(err.code(), "input_too_large");
    let err = invoke_cli(&r, &mut log, Audience::Person, &["diff-manifests", "a", "b"], &no_files).unwrap_err();
    assert_eq!(err.code(), "bad_params");
}

// ------------------------------------------------------------------ MCP

#[test]
fn mcp_diff_needs_handles_and_bounded_params() {
    let r = registry("valid");
    let mut log = Vec::new();
    let raw = br#"{"a":"v1.json","b":"v2.json"}"#;
    assert_eq!(invoke_mcp(&r, &mut log, MCP_DIFF, raw, &manifests).unwrap_err().code(), "bad_params");
    let handles = br#"{"a":"root:imports/v1.json","b":"root:imports/v2.json"}"#;
    let out = invoke_mcp(&r, &mut log, MCP_DIFF, handles, &manifests).unwrap();
    assert_eq!(parse(&out).unwrap(), parse(&read("qry/v1_v2.changes.json")).unwrap());
    for bad in [
        &br#"{"a":"root:imports/v1.json"}"#[..],
        br#"{"a":"root:imports/v1.json","b":"root:imports/v2.json","c":"x"}"#,
        br#"{"a":1,"b":"root:imports/v2.json"}"#,
        br#"{"a":"root:imports/v1.json","b":"root:imports/v2.json","a":"x"}"#,
        br#"{"a":[[[[["deep"]]]]],"b":""}"#,
        b"not json",
    ] {
        assert_eq!(invoke_mcp(&r, &mut log, MCP_DIFF, bad, &manifests).unwrap_err().code(), "bad_params");
    }
    let long = format!("{{\"a\":\"root:imports/{}\",\"b\":\"root:imports/v2.json\"}}", "x".repeat(2100));
    assert_eq!(invoke_mcp(&r, &mut log, MCP_DIFF, long.as_bytes(), &manifests).unwrap_err().code(), "bad_params");
    let huge = vec![b' '; query::MAX_PARAMS_BYTES + 1];
    assert_eq!(invoke_mcp(&r, &mut log, MCP_LIST, &huge, &manifests).unwrap_err().code(), "bad_params");
    assert_eq!(
        invoke_mcp(&r, &mut log, "registry_show_manifest", b"{}", &manifests).unwrap_err().code(),
        "unknown_command"
    );
    // Every call but the unknown tool was logged, all as MCP.
    assert!(log.iter().all(|e| e.surface == Surface::Mcp));
    assert_eq!(log.len(), 10);
}

#[test]
fn agent_on_cli_gets_the_same_restricted_view_as_mcp() {
    let r = registry("qry");
    let mut log = Vec::new();
    let cli = invoke_cli(&r, &mut log, Audience::Agent, &["list-commands", "--include-hidden"], &no_files).unwrap();
    let mcp = invoke_mcp(&r, &mut log, MCP_LIST, br#"{"include_hidden":true}"#, &no_files).unwrap();
    assert_eq!(cli, mcp);
    assert_eq!(cli, to_canonical_bytes(get(&expected("qry_agent_hidden"), "ok")).unwrap());
    let err =
        invoke_cli(&r, &mut log, Audience::Agent, &["describe-command", "sign.set_profile@1"], &no_files).unwrap_err();
    assert_eq!(err.code(), "not_found");
    let s = get(&err.to_value(), "suggestions").clone();
    let CValue::Arr(s) = s else { panic!() };
    for never in ["sign.set_profile", "cmm.move_to", "align.set_datum"] {
        assert!(!s.contains(&CValue::Str(never.into())), "{never} leaked");
    }
    let err =
        invoke_cli(&r, &mut log, Audience::Agent, &["diff-manifests", "v1.json", "v2.json"], &manifests).unwrap_err();
    assert_eq!(err.code(), "bad_params");
}

// ------------------------------------------------------------------ definitions, helpers

#[test]
fn the_three_definitions_pass_val_and_export() {
    let defs = query::command_defs().unwrap();
    let ids: Vec<(String, u32)> = defs.iter().map(|d| (d.id.clone(), d.major)).collect();
    assert_eq!(
        ids,
        [("registry.describe_command", 1), ("registry.diff_manifests", 1), ("registry.list_commands", 1)]
            .map(|(i, m)| (i.to_string(), m))
    );
    let decls: Vec<CommandDecl> = defs.iter().map(CommandDecl::from_def).collect();
    assert_eq!(validate(&decls), vec![]);
    for d in &defs {
        assert_eq!(d.side_effect.as_str(), "R0");
        assert_eq!(query::mcp_projection(d).as_str(), "exposed");
    }
    assert_eq!(defs[1].fs_effect.as_str(), "read_root");
    let m = ManifestInForce::new(&decls, Catalogue::geo_frame().unwrap()).unwrap();
    let r = Registry::new(&m).unwrap();
    assert_eq!(r.len(), 3);
    // The registry describes its own query commands.
    let mut log = Vec::new();
    let out = invoke_mcp(&r, &mut log, MCP_DESCRIBE, br#"{"id":"registry.diff_manifests"}"#, &no_files).unwrap();
    let v = parse(&out).unwrap();
    assert_eq!(get(&v, "definition"), &defs[1].to_value());
}

#[test]
fn levenshtein_matches_hand_values() {
    assert_eq!(levenshtein("", ""), 0);
    assert_eq!(levenshtein("abc", ""), 3);
    assert_eq!(levenshtein("", "abc"), 3);
    assert_eq!(levenshtein("kitten", "sitting"), 3);
    assert_eq!(levenshtein("align.run_bestfi", "align.run_bestfit"), 1);
    assert_eq!(levenshtein("r\u{e9}ad", "read"), 1);
    assert_eq!(levenshtein("flaw", "lawn"), 2);
}

// ------------------------------------------------------------------ #394 review

#[test]
fn diff_is_filtered_by_audience_and_carries_both_manifest_digests() {
    let r = registry("qry");
    let want = parse(&read("qry/audience.changes.json")).unwrap();
    let mut log = Vec::new();
    let person = invoke_cli(
        &r,
        &mut log,
        Audience::Person,
        &["diff-manifests", "audience_a.manifest.json", "audience_b.manifest.json", "--json"],
        &manifests,
    )
    .unwrap();
    assert_eq!(parse(&person).unwrap(), *get(&want, "person"));
    let params = br#"{"a":"root:imports/audience_a.manifest.json","b":"root:imports/audience_b.manifest.json"}"#;
    let agent = invoke_mcp(&r, &mut log, MCP_DIFF, params, &manifests).unwrap();
    assert_eq!(parse(&agent).unwrap(), *get(&want, "agent"));
    let cli_agent = invoke_cli(
        &r,
        &mut log,
        Audience::Agent,
        &["diff-manifests", "root:imports/audience_a.manifest.json", "root:imports/audience_b.manifest.json"],
        &manifests,
    )
    .unwrap();
    assert_eq!(cli_agent, agent);
    // An exported manifest's digest is the SHA-256 of its exact (canonical) bytes.
    let a = read("qry/audience_a.manifest.json");
    assert_eq!(get(get(&want, "person"), "a_manifest_digest"), &CValue::Str(fid_canon::digest_bytes(&a).to_string()));
}

/// Every class x declared projection x human_only x mcp-in-exposure combination against the
/// oracle's table (`qry/projection.json`).
#[test]
fn mcp_projection_matches_the_oracle_on_every_combination() {
    let table = parse(&read("qry/projection.json")).unwrap();
    let CValue::Arr(rows) = get(&table, "rows") else { panic!() };
    assert_eq!(rows.len(), 11 * 3 * 2 * 2);
    for row in rows {
        let s = |k: &str| match get(row, k) {
            CValue::Str(x) => x.clone(),
            _ => panic!("{k}"),
        };
        let b = |k: &str| get(row, k) == &CValue::Bool(true);
        let exposure = if b("on_mcp") { r#"["cli","mcp"]"# } else { r#"["cli"]"# };
        let json = format!(
            r#"{{"id":"test.combo","major":1,"title":"T","description":"D","params_schema":{{"type":"object"}},
"result_schema":{{"type":"object"}},"side_effect":"{}","mcp_projection":"{}","determinism":"D0",
"idempotency":"none","fs_effect":"none","undo":"not_undoable","preconditions":[],"effects":[],"algorithms":[],
"exposure":{exposure},"lifecycle":{{"state":"active","since":"2026-01-01"}},"web":"available","human_only":{}}}"#,
            s("side_effect"),
            s("declared"),
            b("human_only")
        );
        let def = fid_registry::CommandDef::from_json(json.as_bytes()).unwrap();
        assert_eq!(query::mcp_projection(&def).as_str(), s("expect"), "{row:?}");
        assert_eq!(fid_registry::mcp_projection(&def), query::mcp_projection(&def));
    }
}
