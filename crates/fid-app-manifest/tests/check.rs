#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! ECO.APPS.MANIFEST.SCHEMA AC-1..AC-8 and the REQ-001..009 edge cases.
//!
//! Inputs are under `fixtures/manifest/`: the hand-written manifests (`*.toml`), `apps.toml`,
//! `seeds/fixture.md`, and the catalogues written by the independent Python reference
//! `fixtures/manifest/make_app_fixtures.py`. Every expected finding below is hand-labelled
//! from the spec's rules; none is produced by running this crate.

use fid_app_manifest::cli::run_cli;
use fid_app_manifest::{
    load_manifest, load_manifest_bytes, parse_manifest, read_app_ids, read_id_list, seed_ids, validate, Catalogues,
    Finding, RegistryView, Status,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixtures() -> PathBuf {
    repo().join("fixtures/manifest")
}

fn fixture(name: &str) -> PathBuf {
    fixtures().join(name)
}

fn catalogues(seeds: bool) -> Catalogues {
    let s = fixture("seeds");
    Catalogues::from_dir(&fixtures(), seeds.then_some(s.as_path())).unwrap()
}

fn lines(findings: &[Finding]) -> Vec<String> {
    findings.iter().map(ToString::to_string).collect()
}

fn findings_of(name: &str, seeds: bool) -> Vec<String> {
    match load_manifest(&fixture(name), &catalogues(seeds)) {
        Ok(_) => Vec::new(),
        Err(f) => lines(&f),
    }
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn check_args(manifest: &str) -> Vec<String> {
    let m = fixture(manifest);
    let c = fixtures();
    args(&["check", "--manifest", m.to_str().unwrap(), "--catalogues", c.to_str().unwrap()])
}

fn bin(a: &[String]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fid-app-manifest")).args(a).output().unwrap()
}

static TMP: AtomicUsize = AtomicUsize::new(0);

/// A fresh empty directory under the system temp dir.
fn tmp_dir() -> PathBuf {
    let n = TMP.fetch_add(1, Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("fid-app-manifest-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A copy of the fixture catalogues, for tests that break one file.
fn catalogue_copy() -> PathBuf {
    let d = tmp_dir();
    for f in ["registry-manifest.json", "settings.json", "panels.json", "layouts.json", "templates.json", "apps.toml"] {
        std::fs::copy(fixture(f), d.join(f)).unwrap();
    }
    d
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_valid_manifest_passes_the_check_binary() {
    let out = bin(&check_args("valid.toml"));
    assert_eq!(out.status.code(), Some(0), "stdout: {}", String::from_utf8_lossy(&out.stdout));
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}

#[test]
fn ac1_valid_manifest_loads_with_its_fields() {
    let loaded = load_manifest(&fixture("valid.toml"), &catalogues(true)).unwrap();
    let m = loaded.manifest();
    assert_eq!(m.id, "report");
    assert_eq!(m.status, Status::Alpha);
    assert_eq!(m.namespaces.iter().map(String::as_str).collect::<Vec<_>>(), ["project", "report", "view"]);
    assert_eq!(m.panels, ["report.outline", "report.preview"]);
    assert_eq!(m.layout, "report.default");
    assert_eq!(m.templates.len(), 1);
    assert_eq!(m.defaults.len(), 2);
    assert_eq!(m.known_gaps.len(), 2);
    // Effective commands: the registry commands of project, report and view (not cmm, settings).
    let eff: Vec<&str> = loaded.effective_commands().iter().map(|c| c.as_str()).collect();
    assert_eq!(
        eff,
        ["project.open_project@1", "report.generate_report@1", "report.show_report@1", "view.apply_layout@1"]
    );
    // REQ-ECO.APPS.MANIFEST-007: release packaging can detect placeholder branding.
    assert!(loaded.branding_is_placeholder());
}

// ------------------------------------------------------------------ AC-2

const AC2: [&str; 3] =
    ["MAN-UNKNOWN-NS sketch", "MAN-UNKNOWN-PANEL report.chart", "MAN-UNKNOWN-SETTING report.page_size"];

#[test]
fn ac2_unknown_names_are_all_reported_sorted_by_code() {
    let out = bin(&check_args("unknown-names.toml"));
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(text, format!("{}\n", AC2.join("\n")));
}

/// A minimal app: it starts only when the loader accepts its manifest.
struct TestApp {
    started: bool,
}

fn start_test_app(manifest: &Path, catalogues: &Catalogues) -> (TestApp, Vec<Finding>) {
    match load_manifest(manifest, catalogues) {
        Ok(_) => (TestApp { started: true }, Vec::new()),
        Err(f) => (TestApp { started: false }, f),
    }
}

#[test]
fn ac2_loader_refuses_to_start_the_app_with_the_same_findings() {
    let (app, findings) = start_test_app(&fixture("unknown-names.toml"), &catalogues(false));
    assert!(!app.started);
    assert_eq!(lines(&findings), AC2);
    let (app, findings) = start_test_app(&fixture("valid.toml"), &catalogues(false));
    assert!(app.started);
    assert!(findings.is_empty());
}

// ------------------------------------------------------------------ AC-3 .. AC-7

#[test]
fn ac3_forbidden_class_in_an_enabled_namespace() {
    assert_eq!(findings_of("forbidden.toml", false), ["MAN-FORBIDDEN-CLASS cmm.run_program@1 M1"]);
}

#[test]
fn ac4_extra_key_is_a_schema_error() {
    assert_eq!(findings_of("extra-key.toml", false), ["MAN-SCHEMA mcp_projection"]);
}

#[test]
fn ac5_status_app_id_and_branding() {
    assert_eq!(findings_of("beta-no-scope.toml", false), ["MAN-STATUS status_scope missing"]);
    assert_eq!(findings_of("bad-id.toml", false), ["MAN-APP-ID suite"]);
    assert_eq!(findings_of("real-brand.toml", false), ["MAN-BRANDING branding.internal"]);
}

#[test]
fn ac6_default_for_an_admin_setting() {
    assert_eq!(findings_of("admin-default.toml", false), ["MAN-DEFAULT net.allowed_roots admin"]);
}

#[test]
fn req003_default_failing_the_setting_schema() {
    assert_eq!(findings_of("bad-default.toml", false), ["MAN-DEFAULT report.decimal_places schema"]);
}

#[test]
fn ac7_unknown_seed_gap_with_a_seed_list() {
    let m = fixture("gaps.toml");
    let c = fixtures();
    let s = fixture("seeds");
    let a = args(&[
        "check",
        "--manifest",
        m.to_str().unwrap(),
        "--catalogues",
        c.to_str().unwrap(),
        "--seeds",
        s.to_str().unwrap(),
    ]);
    let out = bin(&a);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "MAN-GAP PW.RPT.99\n");
    // Without a seed list the seed check is skipped.
    assert!(findings_of("gaps.toml", false).is_empty());
}

// ------------------------------------------------------------------ AC-8

#[test]
fn ac8_repeated_runs_are_byte_identical() {
    let a = check_args("unknown-names.toml");
    let first = bin(&a);
    let second = bin(&a);
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(first.stderr, second.stderr);
    assert_eq!(first.status.code(), second.status.code());
    assert_eq!(run_cli(&a), run_cli(&a));
}

// ------------------------------------------------------------------ REQ-009: one pass

#[test]
fn req009_every_problem_in_one_pass_sorted() {
    let expected = [
        "MAN-APP-ID suite",
        "MAN-BRANDING branding.name",
        "MAN-DEFAULT report.decimal_places schema",
        "MAN-DEFAULT report.when value",
        "MAN-FORBIDDEN-CLASS cmm.run_program@1 M1",
        "MAN-GAP known_gaps[0]",
        "MAN-SCHEMA branding.side_effect",
        "MAN-SCHEMA forbidden_classes[1] value",
        "MAN-SCHEMA mcp_projection",
        "MAN-SCHEMA namespaces duplicate",
        "MAN-STATUS status value",
        "MAN-UNKNOWN-LAYOUT report.wide",
        "MAN-UNKNOWN-NS sketch",
        "MAN-UNKNOWN-PANEL report.chart",
        "MAN-UNKNOWN-TEMPLATE 0193a5b0-7c1e-7d2a-9f00-3b5c6d7e8f99",
    ];
    assert_eq!(findings_of("many.toml", true), expected);
}

#[test]
fn req009_oversized_manifest_is_refused_before_parsing() {
    let mut big = std::fs::read(fixture("valid.toml")).unwrap();
    big.extend(std::iter::repeat_n(b'#', 256 * 1024));
    let c = catalogues(false);
    assert_eq!(lines(&load_manifest_bytes(&big, &c).unwrap_err()), ["MAN-IO manifest too_large"]);
    let d = tmp_dir();
    std::fs::write(d.join("big.toml"), &big).unwrap();
    assert_eq!(lines(&load_manifest(&d.join("big.toml"), &c).unwrap_err()), ["MAN-IO manifest too_large"]);
    assert_eq!(lines(&load_manifest(&d.join("absent.toml"), &c).unwrap_err()), ["MAN-IO manifest missing"]);
}

// ------------------------------------------------------------------ REQ-001 and parser edges

fn valid_text() -> String {
    std::fs::read_to_string(fixture("valid.toml")).unwrap()
}

fn findings_text(text: &str) -> Vec<String> {
    match load_manifest_bytes(text.as_bytes(), &catalogues(false)) {
        Ok(_) => Vec::new(),
        Err(f) => lines(&f),
    }
}

#[test]
fn req001_duplicate_key_and_bad_toml_are_refused() {
    let dup = valid_text().replacen("layout = \"report.default\"", "layout = \"report.default\"\nlayout = \"x.y\"", 1);
    assert_eq!(findings_text(&dup), ["MAN-SCHEMA toml syntax"]);
    assert_eq!(load_manifest_bytes(&[0xff, 0xfe], &catalogues(false)).unwrap_err().len(), 1);
}

#[test]
fn req001_missing_required_keys_and_wrong_schema() {
    let f = findings_text("schema = 2\n");
    assert_eq!(
        f,
        [
            "MAN-SCHEMA branding missing",
            "MAN-SCHEMA id missing",
            "MAN-SCHEMA known_gaps missing",
            "MAN-SCHEMA layout missing",
            "MAN-SCHEMA namespaces missing",
            "MAN-SCHEMA panels missing",
            "MAN-SCHEMA schema not_1",
            "MAN-STATUS status missing",
        ]
    );
}

#[test]
fn req001_no_key_can_alter_a_command() {
    for key in ["side_effect", "mcp_projection", "commands", "exposure", "human_only"] {
        let text = format!("{key} = \"x\"\n{}", valid_text());
        assert_eq!(findings_text(&text), [format!("MAN-SCHEMA {key}")], "{key}");
    }
}

#[test]
fn req003_default_values_without_a_json_form() {
    for v in ["2026-11-25", "nan", "inf", "9007199254740993"] {
        let text =
            valid_text().replacen("\"report.decimal_places\" = 3", &format!("\"report.decimal_places\" = {v}"), 1);
        assert_eq!(findings_text(&text), ["MAN-DEFAULT report.decimal_places value"], "{v}");
    }
    // An unquoted dotted key is a nested table, not the setting `report.decimal_places`.
    let text = valid_text().replacen("\"report.decimal_places\" = 3", "report.decimal_places = 3", 1);
    assert_eq!(findings_text(&text), ["MAN-UNKNOWN-SETTING report"]);
}

#[test]
fn req006_status_scope_rules() {
    let long = "x".repeat(121);
    let text = valid_text().replacen(
        "status_scope = \"Report authoring and preview for single-stage FAI; no SPC yet\"",
        &format!("status_scope = \"{long}\""),
        1,
    );
    assert_eq!(findings_text(&text), ["MAN-STATUS status_scope length"]);
    let exact = valid_text().replacen(
        "status_scope = \"Report authoring and preview for single-stage FAI; no SPC yet\"",
        &format!("status_scope = \"{}\"", "é".repeat(120)),
        1,
    );
    assert!(findings_text(&exact).is_empty());
    let typed = valid_text().replacen(
        "status_scope = \"Report authoring and preview for single-stage FAI; no SPC yet\"",
        "status_scope = 3",
        1,
    );
    assert_eq!(findings_text(&typed), ["MAN-STATUS status_scope type"]);
    // A stable app needs no scope.
    let stable = valid_text().replacen(
        "status = \"alpha\"\nstatus_scope = \"Report authoring and preview for single-stage FAI; no SPC yet\"\n",
        "status = \"stable\"\n",
        1,
    );
    assert!(findings_text(&stable).is_empty());
}

#[test]
fn req007_gap_shapes() {
    for (gap, expect) in [
        ("{ seed = \"pw.rpt.01\" }", true),
        ("{ text = \"t\" }", true),
        ("{ text = \"t\", node = \"rpt.model\" }", true),
        ("{ text = \"\", node = \"RPT.MODEL\" }", true),
        ("{ text = \"t\", node = \"RPT.MODEL\", seed = \"PW.RPT.01\" }", true),
        ("\"PW.RPT.01\"", true),
        ("{ text = \"t\", node = \"RPT.MODEL\" }", false),
    ] {
        let text = valid_text().replacen("  { seed = \"PW.RPT.01\" },\n", &format!("  {gap},\n"), 1);
        let want: Vec<String> = if expect { vec!["MAN-GAP known_gaps[0]".into()] } else { Vec::new() };
        assert_eq!(findings_text(&text), want, "{gap}");
    }
}

#[test]
fn req008_branding_placeholders() {
    let name = valid_text().replacen("name = \"placeholder-report\"", "name = \"Metrology Maven Report\"", 1);
    assert_eq!(findings_text(&name), ["MAN-BRANDING branding.name"]);
    let icon = valid_text().replacen("icon = \"placeholder-report-icon\"", "icon = \"placeholder-\"", 1);
    assert_eq!(findings_text(&icon), ["MAN-BRANDING branding.icon"]);
    let colour = valid_text().replacen("colours = [\"placeholder-accent\"]", "colours = [\"#0055ff\"]", 1);
    assert_eq!(findings_text(&colour), ["MAN-BRANDING branding.colours"]);
    let missing = valid_text().replacen("internal = true\n", "", 1);
    assert_eq!(findings_text(&missing), ["MAN-BRANDING branding.internal missing"]);
}

#[test]
fn unprintable_names_are_never_echoed() {
    let text = format!("\"bad key\\u0007\" = 1\n{}", valid_text().replacen("\"project\"", "\"pro ject\"", 1));
    assert_eq!(findings_text(&text), ["MAN-SCHEMA key unprintable", "MAN-SCHEMA namespaces[0] type"]);
}

// ------------------------------------------------------------------ in-process use

#[test]
fn parse_then_validate_matches_the_loader() {
    let c = catalogues(false);
    for name in ["valid.toml", "unknown-names.toml", "forbidden.toml", "bad-id.toml", "real-brand.toml"] {
        let bytes = std::fs::read(fixture(name)).unwrap();
        let m = parse_manifest(&bytes).unwrap();
        let via_validate = lines(&validate(&m, &c));
        assert_eq!(via_validate, findings_of(name, false), "{name}");
    }
    let mut m = parse_manifest(&std::fs::read(fixture("valid.toml")).unwrap()).unwrap();
    m.schema = 2;
    m.known_gaps.push(fid_app_manifest::Gap::Seed("not-a-seed".into()));
    assert_eq!(lines(&validate(&m, &c)), ["MAN-GAP known_gaps[2]", "MAN-SCHEMA schema not_1"]);
}

#[test]
fn registry_view_from_in_process_commands() {
    use fid_types::{CommandId, SideEffect};
    let mut c = catalogues(false);
    c.registry = RegistryView::from_commands([
        (CommandId::parse("report.show_report@1").unwrap(), SideEffect::R0),
        (CommandId::parse("report.publish_report@1").unwrap(), SideEffect::W3),
        (CommandId::parse("project.open_project@1").unwrap(), SideEffect::W2),
        (CommandId::parse("view.apply_layout@1").unwrap(), SideEffect::W2),
    ]);
    let text = valid_text().replacen("forbidden_classes = [\"M1\", \"S1\"]", "forbidden_classes = [\"W3\"]", 1);
    let f = load_manifest_bytes(text.as_bytes(), &c).unwrap_err();
    assert_eq!(lines(&f), ["MAN-FORBIDDEN-CLASS report.publish_report@1 W3"]);
}

// ------------------------------------------------------------------ catalogue files

#[test]
fn registry_manifest_with_a_wrong_digest_is_refused() {
    let bytes = std::fs::read(fixture("registry-manifest.json")).unwrap();
    assert_eq!(RegistryView::from_manifest_bytes(&bytes).unwrap().commands().len(), 7);
    let text = String::from_utf8(bytes).unwrap().replacen(
        "Fixture definition of class M1.",
        "Fixture definition of class R0.",
        1,
    );
    assert_eq!(RegistryView::from_manifest_bytes(text.as_bytes()), Err("digest"));
    assert_eq!(RegistryView::from_manifest_bytes(b"{\"format\":1}"), Err("shape"));
    assert_eq!(RegistryView::from_manifest_bytes(b"{\"a\":1,\"a\":2}"), Err("json"));
}

#[test]
fn every_broken_catalogue_file_is_reported_at_once() {
    let d = catalogue_copy();
    std::fs::remove_file(d.join("layouts.json")).unwrap();
    std::fs::write(
        d.join("panels.json"),
        br#"{"format":"fid.panels","panels":[{"id":"a.b"},{"id":"a.b"}],"version":1}"#,
    )
    .unwrap();
    std::fs::write(d.join("apps.toml"), "[[app]]\nid = \"x\"\n[[app]]\nid = \"x\"\n").unwrap();
    std::fs::write(d.join("settings.json"), b"{}").unwrap();
    let err = Catalogues::from_dir(&d, Some(&d.join("no-seeds"))).err().unwrap();
    assert_eq!(
        lines(&err),
        [
            "MAN-IO apps.toml duplicate",
            "MAN-IO layouts.json missing",
            "MAN-IO panels.json duplicate",
            "MAN-IO seeds unreadable",
            "MAN-IO settings.json invalid",
        ]
    );
    let out = run_cli(&args(&[
        "check",
        "--manifest",
        fixture("valid.toml").to_str().unwrap(),
        "--catalogues",
        d.to_str().unwrap(),
    ]));
    assert_eq!(out.0, 1);
    assert!(out.1.starts_with("MAN-IO apps.toml duplicate\n"));
    assert!(!out.1.contains(d.to_str().unwrap()), "no paths in output");
}

#[test]
fn list_catalogue_shapes() {
    assert_eq!(
        read_id_list(br#"{"format":"fid.panels","panels":[],"version":1}"#, "fid.panels", "panels").unwrap().len(),
        0
    );
    assert_eq!(
        read_id_list(br#"{"format":"fid.panels","panels":[],"version":2}"#, "fid.panels", "panels"),
        Err("format")
    );
    assert_eq!(
        read_id_list(br#"{"format":"fid.layouts","panels":[],"version":1}"#, "fid.panels", "panels"),
        Err("format")
    );
    assert_eq!(
        read_id_list(br#"{"extra":0,"format":"fid.panels","panels":[],"version":1}"#, "fid.panels", "panels"),
        Err("shape")
    );
    assert_eq!(
        read_id_list(br#"{"format":"fid.panels","panels":[{"id":"a b"}],"version":1}"#, "fid.panels", "panels"),
        Err("entry")
    );
    assert_eq!(read_app_ids(b"[[app]]\nid = \"report\"\n").unwrap().len(), 1);
    assert_eq!(read_app_ids(b"[app]\nid = \"report\"\n"), Err("shape"));
}

#[test]
fn seed_files_are_read_by_their_table_rows() {
    let ids = seed_ids(&std::fs::read_to_string(fixture("seeds/fixture.md")).unwrap());
    assert_eq!(ids.into_iter().collect::<Vec<_>>(), ["PW.RPT.01", "PW.RPT.02"]);
    // The real seed lists parse the same way.
    let pw = seed_ids(&std::fs::read_to_string(repo().join("seeds/polyworks.md")).unwrap());
    assert!(pw.contains("PW.PRJ.01"));
}

// ------------------------------------------------------------------ CLI

#[test]
fn cli_usage_errors_exit_2() {
    for a in [
        args(&[]),
        args(&["lint"]),
        args(&["check"]),
        args(&["check", "--manifest"]),
        args(&["check", "--manifest", "a", "--manifest", "b", "--catalogues", "c"]),
        args(&["check", "--manifest", "a", "--other", "b"]),
    ] {
        let (code, out, err) = run_cli(&a);
        assert_eq!(code, 2, "{a:?}");
        assert!(out.is_empty());
        assert!(err.contains("usage: fid-app-manifest check"));
    }
}
