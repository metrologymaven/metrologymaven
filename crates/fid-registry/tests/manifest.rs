//! Acceptance criteria of spec node CORE.REG.MAN (AC-1 .. AC-5) and the pinned choices of its
//! Design. Expected bytes and digests come from `fixtures/core/reg/manifest/make_fixtures.py`
//! (an independent Python reference), never from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_registry::manifest::{
    check_manifest, command_def, export, export_with, invoke_cli, invoke_mcp, manifest_schema, mcp_projection, sees,
    show_manifest, ActivityEntry, Audience, ManifestError, ManifestInForce, ShowParams, Surface, AGENT_VIEW, MCP_SHOW,
    SHOW_MANIFEST,
};
use fid_registry::{validate, Catalogue, CommandDecl, QuantityCatalogue, RuleCode, MAX_COMMANDS};
use fid_registry::{CommandDef, Projection};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use Audience::{Agent, Person};

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

fn golden_digest(rel: &str) -> String {
    let text = String::from_utf8(read(rel)).unwrap();
    text.strip_suffix('\n').unwrap().to_string()
}

fn geo() -> Catalogue {
    Catalogue::geo_frame().unwrap()
}

fn with_time() -> Catalogue {
    Catalogue::from_json(&read("def/schema/catalogue/with_time.json")).unwrap()
}

fn obj(pairs: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn set(d: &CommandDecl, key: &str, v: CValue) -> CommandDecl {
    let CValue::Obj(mut o) = d.0.clone() else { panic!() };
    o.insert(key.to_string(), v);
    CommandDecl(CValue::Obj(o))
}

// ------------------------------------------------------------------ AC-1, AC-2

#[test]
fn ac1_export_matches_the_independent_golden_bytes_and_digest() {
    let (bytes, digest) = export(&decls("decls/valid.json")).unwrap();
    assert_eq!(bytes, read("manifest/valid.manifest.json"));
    assert_eq!(digest.to_string(), golden_digest("manifest/valid.digest"));
    let (again, digest2) = export(&decls("decls/valid.json")).unwrap();
    assert_eq!((again, digest2), (bytes, digest));
}

/// Prints the digest when run as the child of [`ac1_two_processes_give_identical_digests`].
#[test]
fn child_print_digest() {
    if std::env::var_os("FID_REG_MAN_CHILD").is_none() {
        return;
    }
    let (bytes, digest) = export(&decls("decls/valid.json")).unwrap();
    println!("FID_REG_MAN_DIGEST={digest} LEN={}", bytes.len());
}

#[test]
fn ac1_two_processes_give_identical_digests() {
    let exe = std::env::current_exe().unwrap();
    let run = || {
        let out = std::process::Command::new(&exe)
            .args(["child_print_digest", "--exact", "--nocapture", "--test-threads=1"])
            .env("FID_REG_MAN_CHILD", "1")
            .output()
            .unwrap();
        assert!(out.status.success());
        let stdout = String::from_utf8(out.stdout).unwrap();
        // The harness may print `test child_print_digest ... ` on the same line.
        stdout.split_whitespace().find(|w| w.starts_with("FID_REG_MAN_DIGEST=")).unwrap().to_string()
    };
    let (a, b) = (run(), run());
    assert_eq!(a, b);
    assert!(a.contains(&golden_digest("manifest/valid.digest")));
}

#[test]
fn ac2_reversed_declarations_give_identical_bytes() {
    let mut d = decls("decls/valid.json");
    let (forward, _) = export(&d).unwrap();
    d.reverse();
    let (reversed, _) = export(&d).unwrap();
    assert_eq!(forward, reversed);
}

#[test]
fn normal_form_and_order_match_the_golden() {
    // Exposure out of surface order, optional fields omitted, a precondition, an algorithm,
    // two majors of one ID, shuffled.
    let d = decls("manifest/order.json");
    let (bytes, digest) = export(&d).unwrap();
    assert_eq!(bytes, read("manifest/order.manifest.json"));
    assert_eq!(digest.to_string(), golden_digest("manifest/order.digest"));
    for perm in [[0, 1, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
        let shuffled: Vec<_> = perm.iter().map(|&i| d[i].clone()).collect();
        assert_eq!(export(&shuffled).unwrap().0, bytes);
    }
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_definition_digests_match_the_golden_and_the_export_record() {
    let export = export_with(&decls("decls/valid.json"), &geo()).unwrap();
    let CValue::Obj(top) = parse(&read("manifest/valid.manifest.json")).unwrap() else { panic!() };
    let Some(CValue::Arr(commands)) = top.get("commands") else { panic!() };
    assert_eq!(commands.len(), export.definitions.len());
    for c in commands {
        let CValue::Obj(o) = c else { panic!() };
        let (Some(CValue::Str(id)), Some(CValue::Num(major)), Some(CValue::Str(stated))) =
            (o.get("id"), o.get("major"), o.get("definition_digest"))
        else {
            panic!()
        };
        let mut body = o.clone();
        body.remove("definition_digest");
        let recomputed = fid_canon::digest(&CValue::Obj(body)).unwrap().to_string();
        assert_eq!(&recomputed, stated);
        assert_eq!(export.definitions[&(id.clone(), *major as u32)].to_string(), *stated);
    }
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_published_schema_is_the_generated_one() {
    let mut expected = to_canonical_bytes(&manifest_schema()).unwrap();
    expected.push(b'\n');
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/manifest.schema.json");
    if std::env::var_os("FID_REGEN_MANIFEST_SCHEMA").is_some() {
        std::fs::write(&path, &expected).unwrap();
    }
    assert_eq!(std::fs::read(path).unwrap(), expected, "regenerate schema/manifest.schema.json");
}

#[test]
fn ac4_check_accepts_the_golden_and_refuses_the_tampered_manifest() {
    let golden = read("manifest/valid.manifest.json");
    assert_eq!(check_manifest(&golden, &geo()).unwrap().to_string(), golden_digest("manifest/valid.digest"));
    let order = read("manifest/order.manifest.json");
    assert_eq!(check_manifest(&order, &geo()).unwrap().to_string(), golden_digest("manifest/order.digest"));
    let err = check_manifest(&read("manifest/tampered.json"), &geo()).unwrap_err();
    assert_eq!(err.code(), "manifest_invalid");
    assert!(err.to_string().contains("definition_digest mismatch"), "{err}");
}

fn invalid(bytes: &[u8], catalogue: &Catalogue) -> String {
    let err = check_manifest(bytes, catalogue).unwrap_err();
    assert_eq!(err.code(), "manifest_invalid", "{err}");
    err.to_string()
}

fn golden_top() -> BTreeMap<String, CValue> {
    let CValue::Obj(top) = parse(&read("manifest/valid.manifest.json")).unwrap() else { panic!() };
    top
}

fn bytes_of(top: BTreeMap<String, CValue>) -> Vec<u8> {
    to_canonical_bytes(&CValue::Obj(top)).unwrap()
}

#[test]
fn check_refuses_every_departure_from_an_export() {
    let golden = read("manifest/valid.manifest.json");
    // Not canonical: a trailing newline, pretty-printing.
    let mut nl = golden.clone();
    nl.push(b'\n');
    assert!(invalid(&nl, &geo()).contains("canonical"));
    // Not JSON / duplicate key.
    assert!(invalid(b"{\"format\":1,\"format\":1}", &geo()).contains("duplicate_key"));
    assert!(invalid(b"[", &geo()).contains("json"));
    // Another catalogue than the one in force.
    assert!(invalid(&golden, &with_time()).contains("catalogue"));
    // Extra or missing top-level field, wrong format.
    let mut t = golden_top();
    t.insert("built_at".into(), CValue::Str("2026-11-19".into()));
    assert!(invalid(&bytes_of(t), &geo()).contains("exactly"));
    let mut t = golden_top();
    t.insert("format".into(), CValue::Num(2.0));
    assert!(invalid(&bytes_of(t), &geo()).contains("format"));
    // Commands out of order, and a repeated command.
    let mut t = golden_top();
    let Some(CValue::Arr(c)) = t.get_mut("commands") else { panic!() };
    c.swap(0, 1);
    assert!(invalid(&bytes_of(t), &geo()).contains("sorted"));
    let mut t = golden_top();
    let Some(CValue::Arr(c)) = t.get_mut("commands") else { panic!() };
    let first = c[0].clone();
    c.insert(0, first);
    assert!(invalid(&bytes_of(t), &geo()).contains("sorted"));
    // A definition missing its digest.
    let mut t = golden_top();
    let Some(CValue::Arr(c)) = t.get_mut("commands") else { panic!() };
    let CValue::Obj(o) = &mut c[0] else { panic!() };
    o.remove("definition_digest");
    assert!(invalid(&bytes_of(t), &geo()).contains("definition_digest"));
    // A definition that fails VAL, with a matching digest: an unknown quantity kind.
    let mut t = golden_top();
    let Some(CValue::Arr(c)) = t.get_mut("commands") else { panic!() };
    let CValue::Obj(o) = &mut c[0] else { panic!() };
    o.remove("definition_digest");
    let mut text = String::from_utf8(to_canonical_bytes(&CValue::Obj(o.clone())).unwrap()).unwrap();
    text = text.replacen("\"x-quantity\":\"length\"", "\"x-quantity\":\"lenght\"", 1);
    let mut body = match parse(text.as_bytes()).unwrap() {
        CValue::Obj(b) => b,
        _ => panic!(),
    };
    let d = fid_canon::digest(&CValue::Obj(body.clone())).unwrap().to_string();
    body.insert("definition_digest".into(), CValue::Str(d));
    *o = body;
    assert!(invalid(&bytes_of(t), &geo()).contains("unknown_quantity"));
    // A definition not in normal form (an optional field left out), digest matching.
    let mut t = golden_top();
    let Some(CValue::Arr(c)) = t.get_mut("commands") else { panic!() };
    let CValue::Obj(o) = &mut c[0] else { panic!() };
    o.remove("definition_digest");
    o.remove("cli_bare");
    let d = fid_canon::digest(&CValue::Obj(o.clone())).unwrap().to_string();
    o.insert("definition_digest".into(), CValue::Str(d));
    assert!(invalid(&bytes_of(t), &geo()).contains("normal form"));
    // Too large.
    let huge = vec![b' '; fid_registry::MAX_MANIFEST_BYTES + 1];
    assert!(invalid(&huge, &geo()).contains("limit"));
}

// ------------------------------------------------------------------ AC-5

fn core() -> ManifestInForce {
    let mut d = decls("decls/valid.json");
    d.push(CommandDecl::from_def(&command_def().unwrap()));
    ManifestInForce::new(&d, geo()).unwrap()
}

fn no_files(_: &str) -> Option<Vec<u8>> {
    None
}

#[test]
fn ac5_show_manifest_prints_the_digest_and_logs_one_activity_entry() {
    let m = core();
    let mut log: Vec<ActivityEntry> = Vec::new();
    let out = invoke_cli(&m, &mut log, Person, &["show-manifest", "--json"], &no_files).unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), format!("{{\"digest\":\"{}\"}}", m.digest()));
    assert_eq!(log, vec![ActivityEntry { command: SHOW_MANIFEST, surface: Surface::Cli }]);
    // The digest is that of the exact bytes.
    assert_eq!(m.digest().to_string(), fid_canon::digest_bytes(m.bytes()).to_string());
}

#[test]
fn ac5_full_returns_the_bytes_and_mcp_matches_an_agent_on_the_cli() {
    let m = core();
    let mut log = Vec::new();
    let cli = invoke_cli(&m, &mut log, Person, &["show-manifest", "--full", "--json"], &no_files).unwrap();
    let CValue::Obj(o) = parse(&cli).unwrap() else { panic!() };
    assert_eq!(o.get("bytes"), Some(&CValue::Str(String::from_utf8(m.bytes().to_vec()).unwrap())));
    assert_eq!(o.get("view"), None);
    // MCP is always the agent audience, and an agent on the CLI gets the same result.
    let mcp = invoke_mcp(&m, &mut log, MCP_SHOW, b"{\"full\":true}").unwrap();
    let agent_cli = invoke_cli(&m, &mut log, Agent, &["show-manifest", "--full", "--json"], &no_files).unwrap();
    assert_eq!(mcp, agent_cli);
    let CValue::Obj(a) = parse(&mcp).unwrap() else { panic!() };
    let view = m.agent_view();
    assert_eq!(a.get("bytes"), Some(&CValue::Str(String::from_utf8(view.bytes.clone()).unwrap())));
    assert_eq!(a.get("digest"), Some(&CValue::Str(view.digest.to_string())));
    assert_eq!(a.get("view"), Some(&CValue::Str(AGENT_VIEW.to_string())));
    assert_eq!(log.len(), 3);
    assert_eq!(log[1], ActivityEntry { command: SHOW_MANIFEST, surface: Surface::Mcp });
}

// ------------------------------------------------------------------ registry visibility (D-035)

fn visibility() -> ManifestInForce {
    ManifestInForce::new(&decls("manifest/visibility.json"), geo()).unwrap()
}

fn text_of(result: &[u8], key: &str) -> String {
    let CValue::Obj(o) = parse(result).unwrap() else { panic!() };
    let Some(CValue::Str(s)) = o.get(key) else { panic!("{key} missing") };
    s.clone()
}

/// The definitions of a manifest's text, in order (without `definition_digest`).
fn defs_in(manifest: &[u8]) -> Vec<CommandDef> {
    let CValue::Obj(top) = parse(manifest).unwrap() else { panic!() };
    let Some(CValue::Arr(commands)) = top.get("commands") else { panic!() };
    commands
        .iter()
        .map(|c| {
            let CValue::Obj(mut o) = c.clone() else { panic!() };
            assert!(o.remove("definition_digest").is_some());
            CommandDef::from_value(CValue::Obj(o)).unwrap()
        })
        .collect()
}

/// Every (id, major) of a manifest's text, in order, and every `replaced_by` in it.
fn ids_in(manifest: &[u8]) -> (Vec<(String, u32)>, Vec<String>) {
    let mut ids = Vec::new();
    let mut replaced = Vec::new();
    for def in defs_in(manifest) {
        if let Some(CValue::Str(r)) = def.lifecycle.get("replaced_by") {
            replaced.push(r.clone());
        }
        ids.push((def.id, def.major));
    }
    (ids, replaced)
}

/// The independent oracle: full and agent-view bytes, digests and the visible set, all from
/// make_fixtures.py.
#[test]
fn agent_view_matches_the_independent_oracle() {
    let m = visibility();
    assert_eq!(m.bytes(), &read("manifest/visibility.manifest.json")[..]);
    assert_eq!(m.digest().to_string(), golden_digest("manifest/visibility.digest"));
    assert_eq!(m.agent_view().bytes, read("manifest/visibility.agent.manifest.json"));
    assert_eq!(m.agent_view().digest.to_string(), golden_digest("manifest/visibility.agent.digest"));
    let CValue::Obj(o) = parse(&read("manifest/visibility.agent.json")).unwrap() else { panic!() };
    let Some(CValue::Arr(pairs)) = o.get("visible") else { panic!() };
    let want: Vec<(String, u32)> = pairs
        .iter()
        .map(|p| match p {
            CValue::Arr(x) => match (&x[0], &x[1]) {
                (CValue::Str(id), CValue::Num(n)) => (id.clone(), *n as u32),
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(ids_in(&m.agent_view().bytes).0, want);
    // The agent view is itself a valid manifest under the catalogue in force.
    assert_eq!(check_manifest(&m.agent_view().bytes, &geo()).unwrap(), m.agent_view().digest);
}

#[test]
fn an_agents_full_manifest_never_contains_a_hidden_command() {
    let m = visibility();
    let mut log = Vec::new();
    let out = invoke_mcp(&m, &mut log, MCP_SHOW, b"{\"full\":true}").unwrap();
    let bytes = text_of(&out, "bytes");
    let hidden: Vec<CommandDef> = decls("manifest/visibility.json")
        .into_iter()
        .map(|d| CommandDef::from_value(d.0).unwrap())
        .filter(|d| !sees(Agent, d))
        .collect();
    // The class ceilings of never (M1, S1, A0, one of them human_only), declared never (D1
    // and an R0) and no mcp surface.
    assert_eq!(hidden.len(), 6);
    assert!(hidden.iter().any(|d| d.human_only));
    let (ids, replaced) = ids_in(bytes.as_bytes());
    for d in &hidden {
        assert_eq!(mcp_projection(d), Projection::Never);
        assert!(!ids.contains(&(d.id.clone(), d.major)), "{} leaked", d.id);
        // Not as a command, not inside another command (replaced_by), nowhere in the text.
        assert!(!bytes.contains(&format!("\"{}\"", d.id)), "{} named", d.id);
        assert!(!bytes.contains(&format!("{}@{}", d.id, d.major)), "{} named", d.id);
        assert!(!replaced.iter().any(|r| r.starts_with(&format!("{}@", d.id))));
    }
    // The digest an agent gets is the agent view's, labelled; not the full manifest's.
    assert_eq!(text_of(&out, "digest"), m.agent_view().digest.to_string());
    assert_ne!(m.agent_view().digest, m.digest());
    assert_eq!(text_of(&out, "view"), AGENT_VIEW);
    let short = invoke_mcp(&m, &mut log, MCP_SHOW, b"{}").unwrap();
    assert_eq!(
        String::from_utf8(short).unwrap(),
        format!("{{\"digest\":\"{}\",\"view\":\"agent\"}}", m.agent_view().digest)
    );
    // The per-definition digests of the view cover exactly the visible commands.
    assert!(m.agent_view().definitions.keys().all(|k| ids.contains(k)));
    assert_eq!(m.agent_view().definitions.len(), ids.len());
}

#[test]
fn a_persons_full_manifest_contains_every_command() {
    let m = visibility();
    let all = decls("manifest/visibility.json");
    let out = show_manifest(&m, Person, &ShowParams { full: true, check: None }).unwrap();
    let CValue::Obj(o) = &out else { panic!() };
    assert_eq!(o.get("view"), None);
    assert_eq!(o.get("digest"), Some(&CValue::Str(m.digest().to_string())));
    let bytes = match o.get("bytes") {
        Some(CValue::Str(s)) => s.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(bytes.as_bytes(), m.bytes());
    let (ids, replaced) = ids_in(bytes.as_bytes());
    assert_eq!(ids.len(), all.len());
    for id in [
        "cmm.move_to",
        "trust.sign_record",
        "settings.set_admin_value",
        "hal.connect_device",
        "gdt.evaluate_never",
        "gdt.evaluate_cli_only",
    ] {
        assert!(ids.iter().any(|(i, _)| i == id), "{id}");
    }
    for r in ["cmm.move_to@1", "gdt.evaluate_never@1", "report.set_heading@1", "project.list_features@2"] {
        assert!(replaced.iter().any(|x| x == r), "{r}");
    }
}

#[test]
fn replaced_by_a_hidden_command_is_dropped_for_agents() {
    let m = visibility();
    let (_, agent) = ids_in(&m.agent_view().bytes);
    // A visible replacement stays; a hidden one (class never, declared never) and an absent
    // one are removed.
    assert_eq!(agent, vec!["project.list_features@2".to_string()]);
    let mut seen = 0;
    for def in defs_in(&m.agent_view().bytes) {
        if ["sim.read_probe", "gdt.evaluate_old", "report.set_title"].contains(&def.id.as_str()) {
            seen += 1;
            assert_eq!(def.lifecycle.get("state"), Some(&CValue::Str("deprecated".into())), "{}", def.id);
            assert_eq!(def.lifecycle.get("replaced_by"), None, "{}", def.id);
        }
    }
    assert_eq!(seen, 3);
    // The person still sees it.
    let (_, person) = ids_in(m.bytes());
    assert!(person.contains(&"cmm.move_to@1".to_string()));
}

#[test]
fn check_is_the_same_for_both_audiences() {
    let m = visibility();
    let text = String::from_utf8(read("manifest/visibility.manifest.json")).unwrap();
    let p = ShowParams { full: false, check: Some(text) };
    let want = format!("{{\"digest\":\"{}\"}}", golden_digest("manifest/visibility.digest"));
    for audience in [Person, Agent] {
        let got = to_canonical_bytes(&show_manifest(&m, audience, &p).unwrap()).unwrap();
        assert_eq!(String::from_utf8(got).unwrap(), want);
    }
}

#[test]
fn ac4_cli_check_reports_manifest_invalid_for_the_tampered_file() {
    let m = ManifestInForce::new(&decls("decls/valid.json"), geo()).unwrap();
    let files = |p: &str| match p {
        "tampered.json" => Some(read("manifest/tampered.json")),
        "valid.json" => Some(read("manifest/valid.manifest.json")),
        _ => None,
    };
    let mut log = Vec::new();
    let err =
        invoke_cli(&m, &mut log, Person, &["show-manifest", "--check", "tampered.json", "--json"], &files).unwrap_err();
    assert_eq!(err.code(), "manifest_invalid");
    let CValue::Obj(v) = err.to_value() else { panic!() };
    assert_eq!(v.get("code"), Some(&CValue::Str("manifest_invalid".into())));
    let ok = invoke_cli(&m, &mut log, Person, &["show-manifest", "--check", "valid.json", "--json"], &files).unwrap();
    assert_eq!(
        String::from_utf8(ok).unwrap(),
        format!("{{\"digest\":\"{}\"}}", golden_digest("manifest/valid.digest"))
    );
    assert_eq!(log.len(), 2);
}

#[test]
fn show_manifest_refuses_bad_arguments_and_parameters() {
    let m = core();
    let mut log = Vec::new();
    for args in [
        &["show-manifest", "--full", "--full"][..],
        &["show-manifest", "--bogus"],
        &["show-manifest", "--check"],
        &["show-manifest", "--check", "missing.json"],
        &["show-manifest", "x", "x", "x", "x", "x", "x", "x", "x"],
        &[],
    ] {
        assert_eq!(invoke_cli(&m, &mut log, Person, args, &no_files).unwrap_err().code(), "bad_params", "{args:?}");
    }
    assert_eq!(invoke_cli(&m, &mut log, Person, &["list-commands"], &no_files).unwrap_err().code(), "unknown_command");
    for params in [&b"{\"full\":1}"[..], b"{\"other\":true}", b"[]", b"{\"full\":true,\"full\":true}", b"{"] {
        assert_eq!(invoke_mcp(&m, &mut log, MCP_SHOW, params).unwrap_err().code(), "bad_params");
    }
    assert_eq!(invoke_mcp(&m, &mut log, "registry_other", b"{}").unwrap_err().code(), "unknown_command");
}

#[test]
fn show_manifest_definition_is_valid_r0_exposed() {
    let def = command_def().unwrap();
    assert_eq!(format!("{}@{}", def.id, def.major), SHOW_MANIFEST);
    assert_eq!(def.side_effect.as_str(), "R0");
    assert_eq!(def.mcp_projection.as_str(), "exposed");
    assert!(validate(&[CommandDecl::from_def(&def)]).is_empty());
}

// ------------------------------------------------------------------ validation, no bypass

fn rules(err: ManifestError) -> Vec<RuleCode> {
    match err {
        ManifestError::DeclarationsInvalid(e) => e.into_iter().map(|e| e.rule).collect(),
        other => panic!("unexpected {other}"),
    }
}

#[test]
fn duplicate_ids_and_tool_name_collisions_are_refused() {
    let d = decls("decls/valid.json");
    let mut dup = d.clone();
    dup.push(d[0].clone());
    assert_eq!(rules(export(&dup).unwrap_err()), vec![RuleCode::DuplicateCommand]);
    // `x.y_v2@1` and `x.y@2` derive the same tool name `x_y_v2`.
    let a = set(&set(&d[0], "id", CValue::Str("gdt.flat_v2".into())), "major", CValue::Num(1.0));
    let b = set(&set(&d[0], "id", CValue::Str("gdt.flat".into())), "major", CValue::Num(2.0));
    let r = rules(export(&[a, b]).unwrap_err());
    assert!(!r.is_empty() && r.iter().all(|c| *c == RuleCode::ToolNameCollision), "{r:?}");
}

#[test]
fn an_unknown_quantity_kind_is_a_typed_error_until_a_catalogue_holds_it() {
    let d = decls("decls/valid.json");
    let time = obj(vec![
        ("type", CValue::Str("object".into())),
        (
            "properties",
            obj(vec![(
                "timeout",
                obj(vec![
                    ("type", CValue::Str("number".into())),
                    ("x-quantity", CValue::Str("time".into())),
                    ("x-unit", CValue::Str("s".into())),
                ]),
            )]),
        ),
    ]);
    let job = set(&d[0], "params_schema", time);
    let err = export(std::slice::from_ref(&job)).unwrap_err();
    assert_eq!(err.code(), "declarations_invalid");
    let CValue::Obj(v) = err.to_value() else { panic!() };
    assert!(matches!(v.get("errors"), Some(CValue::Arr(e)) if e.len() == 1));
    assert_eq!(rules(err), vec![RuleCode::UnknownQuantity]);
    // With a catalogue that holds `time`, the same declaration exports, and the manifest
    // names that catalogue.
    let cat = with_time();
    let e = export_with(&[job], &cat).unwrap();
    let CValue::Obj(top) = parse(&e.bytes).unwrap() else { panic!() };
    assert_eq!(top.get("quantity_catalogue_digest"), Some(&CValue::Str(cat.digest().to_string())));
    assert_ne!(cat.digest(), geo().digest());
}

#[test]
fn malformed_and_oversized_inputs_fail_closed() {
    assert_eq!(rules(export(&[CommandDecl(CValue::Null)]).unwrap_err()), vec![RuleCode::BadType]);
    let nan = set(&decls("decls/valid.json")[0], "major", CValue::Num(f64::NAN));
    assert_eq!(export(&[nan]).unwrap_err().code(), "declarations_invalid");
    let many = vec![CommandDecl(CValue::Null); MAX_COMMANDS + 1];
    assert_eq!(export(&many).unwrap_err(), ManifestError::TooManyCommands(MAX_COMMANDS + 1));
    // An empty registry is a valid (empty) manifest.
    let (bytes, _) = export(&[]).unwrap();
    assert!(check_manifest(&bytes, &geo()).is_ok());
}

#[test]
fn performance_5000_definitions() {
    let base = decls("decls/valid.json")[0].clone();
    let d: Vec<_> = (0..5000).map(|i| set(&base, "id", CValue::Str(format!("perf.cmd_{i:05}")))).collect();
    let start = Instant::now();
    let (bytes, _) = export(&d).unwrap();
    let took = start.elapsed();
    eprintln!("export of 5000 definitions: {took:?}, {} bytes", bytes.len());
    // The 500 ms budget is for a release build; a debug test build only has to finish.
    assert!(took < Duration::from_secs(60));
}
