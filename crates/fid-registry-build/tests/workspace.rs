//! The registry build step over the crates on main (spec node CORE.REG.MAN, Design "Build
//! step" and "Quantity catalogue"). Expected errors and the merged catalogue are hand-labelled
//! from the `fid-jobs` schemas and fragment (`crates/fid-jobs/src/commands.rs`: `timeout` is
//! `x-quantity: "time"`, `progress` is `x-quantity: "dimensionless"`) and from the GEO.FRAME
//! catalogue fixture, not taken from a run.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_registry::manifest::{check_manifest, ManifestError, SHOW_MANIFEST};
use fid_registry::{Catalogue, CommandDef, DefError, QuantityCatalogue, RuleCode};
use fid_registry_build::{
    build, build_workspace, declarations, manifest_in_force, merge_catalogues, settings, workspace_catalogue,
    workspace_settings, BuildError, QuantitySource, SettingsSource, Source, QUANTITY_SOURCES, SETTINGS_SOURCES,
    SOURCES,
};

fn geo() -> Catalogue {
    Catalogue::geo_frame().unwrap()
}

fn errors(e: BuildError) -> Vec<(String, RuleCode, Option<String>)> {
    match e {
        BuildError::Manifest(ManifestError::DeclarationsInvalid(v)) => {
            v.into_iter().map(|e| (e.command, e.rule, e.field)).collect()
        }
        other => panic!("unexpected {other}"),
    }
}

fn unknown(cmd: &str, field: &str) -> (String, RuleCode, Option<String>) {
    (cmd.to_string(), RuleCode::UnknownQuantity, Some(field.to_string()))
}

#[test]
fn sources_are_sorted_unique_and_each_declares_commands() {
    let names: Vec<_> = SOURCES.iter().map(|s| s.krate).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(names, sorted);
    for s in SOURCES {
        assert!(!(s.defs)().unwrap().is_empty(), "{}", s.krate);
    }
    let names: Vec<_> = QUANTITY_SOURCES.iter().map(|s| s.krate).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(names, sorted);
}

/// The merged workspace catalogue, typed by hand: the GEO.FRAME.UNITS catalogue fixture
/// (`fixtures/core/reg/def/schema/catalogue/geo_frame_units.json`) plus fid-jobs'
/// `dimensionless` (unit `1`) and `time` (`s`, `h` = 3600, `min` = 60, `ms` = 1/1000).
const WORKSPACE_CATALOGUE: &str = concat!(
    r#"{"format":"fid-quantity-catalogue/1","kinds":["#,
    r#"{"canonical":"rad","kind":"angle","units":[{"den":"10800","num":"1","pi":true,"symbol":"arcmin"},"#,
    r#"{"den":"648000","num":"1","pi":true,"symbol":"arcsec"},{"den":"180","num":"1","pi":true,"symbol":"deg"},"#,
    r#"{"den":"200","num":"1","pi":true,"symbol":"gon"},{"den":"1","num":"1","pi":false,"symbol":"rad"}]},"#,
    r#"{"canonical":"1","kind":"dimensionless","units":[{"den":"1","num":"1","pi":false,"symbol":"1"}]},"#,
    r#"{"canonical":"m","kind":"length","units":[{"den":"1250","num":"381","pi":false,"symbol":"ft"},"#,
    r#"{"den":"5000","num":"127","pi":false,"symbol":"in"},{"den":"1","num":"1","pi":false,"symbol":"m"},"#,
    r#"{"den":"1000","num":"1","pi":false,"symbol":"mm"},{"den":"1000000000","num":"1","pi":false,"symbol":"nm"},"#,
    r#"{"den":"5000000","num":"127","pi":false,"symbol":"thou"},"#,
    r#"{"den":"1000000","num":"1","pi":false,"symbol":"um"}]},"#,
    r#"{"canonical":"s","kind":"time","units":[{"den":"1","num":"3600","pi":false,"symbol":"h"},"#,
    r#"{"den":"1","num":"60","pi":false,"symbol":"min"},{"den":"1000","num":"1","pi":false,"symbol":"ms"},"#,
    r#"{"den":"1","num":"1","pi":false,"symbol":"s"}]}],"#,
    r#""pi_hex":"c90fdaa22168c234c4c6628b80dc1cd129024e088a67cc74020bbea63b139b22"}"#,
);

/// The quantity-catalogue gap is closed: the workspace exports against GEO.FRAME plus the
/// domain fragments, and the manifest carries the merged catalogue's digest.
#[test]
fn workspace_builds_against_the_merged_catalogue() {
    let merged = workspace_catalogue().unwrap();
    assert_eq!(merged, Catalogue::from_json(WORKSPACE_CATALOGUE.as_bytes()).unwrap());
    assert_eq!(merged.kind_names(), ["angle", "dimensionless", "length", "time"]);
    assert_ne!(merged.digest(), geo().digest());
    let export = build_workspace(&merged).unwrap();
    assert_eq!(check_manifest(&export.bytes, &merged).unwrap(), export.digest);
    let CValue::Obj(top) = parse(&export.bytes).unwrap() else { panic!() };
    assert_eq!(top.get("quantity_catalogue_digest"), Some(&CValue::Str(merged.digest().to_string())));
    let n: usize = SOURCES.iter().map(|s| (s.defs)().unwrap().len()).sum();
    assert_eq!(export.definitions.len(), n);
    assert!(manifest_in_force(merged).is_ok());
}

/// Fail-closed: without the domain fragments the kinds stay unknown, as typed VAL errors.
#[test]
fn geo_frame_alone_still_refuses_the_job_kinds() {
    let mut expected = vec![
        unknown("job.cancel_job", "/result_schema/properties/progress/x-quantity"),
        unknown("job.list_jobs", "/result_schema/properties/jobs/items/properties/progress/x-quantity"),
        unknown("job.show_status", "/result_schema/properties/progress/x-quantity"),
        unknown("job.wait_job", "/params_schema/properties/timeout/x-quantity"),
        unknown("job.wait_job", "/result_schema/properties/progress/x-quantity"),
    ];
    expected.sort();
    let mut got = errors(build_workspace(&geo()).unwrap_err());
    got.sort();
    assert_eq!(got, expected);
    assert_eq!(manifest_in_force(geo()).unwrap_err().code(), "declarations_invalid");
}

fn time_hours() -> &'static str {
    r#"{"format":"fid-quantity-kinds/1","kinds":[{"canonical":"s","kind":"time","units":[{"den":"1","num":"3600","pi":false,"symbol":"h"},{"den":"1","num":"1","pi":false,"symbol":"s"}]}]}"#
}

fn temperature() -> &'static str {
    r#"{"format":"fid-quantity-kinds/1","kinds":[{"canonical":"K","kind":"temperature","units":[{"den":"1","num":"1","pi":false,"symbol":"K"}]}]}"#
}

fn length_again() -> &'static str {
    r#"{"format":"fid-quantity-kinds/1","kinds":[{"canonical":"m","kind":"length","units":[{"den":"1250","num":"381","pi":false,"symbol":"ft"},{"den":"5000","num":"127","pi":false,"symbol":"in"},{"den":"1","num":"1","pi":false,"symbol":"m"},{"den":"1000","num":"1","pi":false,"symbol":"mm"},{"den":"1000000000","num":"1","pi":false,"symbol":"nm"},{"den":"5000000","num":"127","pi":false,"symbol":"thou"},{"den":"1000000","num":"1","pi":false,"symbol":"um"}]}]}"#
}

fn angle_turn() -> &'static str {
    r#"{"format":"fid-quantity-kinds/1","kinds":[{"canonical":"rad","kind":"angle","units":[{"den":"1","num":"1","pi":false,"symbol":"rad"},{"den":"1","num":"2","pi":true,"symbol":"turn"}]}]}"#
}

fn seconds_as_other_kind() -> &'static str {
    r#"{"format":"fid-quantity-kinds/1","kinds":[{"canonical":"s","kind":"duration","units":[{"den":"1","num":"1","pi":false,"symbol":"s"}]}]}"#
}

fn unsorted() -> &'static str {
    r#"{"format":"fid-quantity-kinds/1","kinds":[{"canonical":"s","kind":"time","units":[{"den":"1","num":"1","pi":false,"symbol":"s"}]},{"canonical":"K","kind":"temperature","units":[{"den":"1","num":"1","pi":false,"symbol":"K"}]}]}"#
}

fn src(krate: &'static str, kinds: fn() -> &'static str) -> QuantitySource {
    QuantitySource { krate, kinds }
}

#[test]
fn a_kind_defined_differently_by_two_crates_is_refused() {
    let jobs = QUANTITY_SOURCES[0];
    let err = merge_catalogues(&[jobs, src("fid-other", time_hours)]).unwrap_err();
    assert_eq!(err, BuildError::KindConflict { kind: "time".into(), first: "fid-jobs", second: "fid-other" });
    assert_eq!(err.code(), "quantity_kind_conflict");
    let CValue::Obj(o) = err.to_value() else { panic!() };
    assert_eq!(
        o.get("crates"),
        Some(&CValue::Arr(vec![CValue::Str("fid-jobs".into()), CValue::Str("fid-other".into())]))
    );
    // The same definition from a second crate is accepted once and changes nothing.
    let twice = merge_catalogues(&[jobs, src("fid-other", fid_jobs::quantity_catalogue)]).unwrap();
    assert_eq!(twice, workspace_catalogue().unwrap());
}

#[test]
fn a_redefined_geo_kind_is_refused() {
    // Even identical to GEO.FRAME's `length`: GEO kinds are locked.
    for (f, kind) in [(length_again as fn() -> &'static str, "length"), (angle_turn, "angle")] {
        let err = merge_catalogues(&[src("fid-geo-copy", f)]).unwrap_err();
        assert_eq!(err, BuildError::GeoKindRedefined { krate: "fid-geo-copy", kind: kind.into() });
        assert_eq!(err.code(), "geo_kind_redefined");
    }
}

#[test]
fn a_fragment_the_catalogue_refuses_names_its_crate() {
    let err = merge_catalogues(&[QUANTITY_SOURCES[0], src("fid-dur", seconds_as_other_kind)]).unwrap_err();
    assert_eq!(err, BuildError::Catalogue { krate: "fid-dur", catalogue_code: "bad_catalogue" });
    assert_eq!(err.code(), "catalogue_invalid");
    let err = merge_catalogues(&[src("fid-unsorted", unsorted)]).unwrap_err();
    assert_eq!(err, BuildError::Catalogue { krate: "fid-unsorted", catalogue_code: "bad_catalogue" });
}

/// The fixed crate order gives one digest; non-conflicting fragments in any order give the
/// same catalogue, because it is sorted by kind name.
#[test]
fn merge_order_gives_a_stable_digest() {
    let a = workspace_catalogue().unwrap();
    let b = workspace_catalogue().unwrap();
    assert_eq!(a.digest(), b.digest());
    let forward = merge_catalogues(&[QUANTITY_SOURCES[0], src("fid-temp", temperature)]).unwrap();
    let backward = merge_catalogues(&[src("fid-temp", temperature), QUANTITY_SOURCES[0]]).unwrap();
    assert_eq!(forward, backward);
    assert_eq!(forward.kind_names(), ["angle", "dimensionless", "length", "temperature", "time"]);
    assert_ne!(forward.digest(), a.digest());
    assert_eq!(merge_catalogues(&[]).unwrap(), geo());
}

fn time_h_3601() -> &'static str {
    r#"{"format":"fid-quantity-kinds/1","kinds":[{"canonical":"s","kind":"time","units":[{"den":"1","num":"3601","pi":false,"symbol":"h"},{"den":"1","num":"60","pi":false,"symbol":"min"},{"den":"1000","num":"1","pi":false,"symbol":"ms"},{"den":"1","num":"1","pi":false,"symbol":"s"}]}]}"#
}

fn time_in_ms() -> &'static str {
    r#"{"format":"fid-quantity-kinds/1","kinds":[{"canonical":"ms","kind":"time","units":[{"den":"1","num":"3600000","pi":false,"symbol":"h"},{"den":"1","num":"60000","pi":false,"symbol":"min"},{"den":"1","num":"1","pi":false,"symbol":"ms"},{"den":"1","num":"1000","pi":false,"symbol":"s"}]}]}"#
}

fn with_pi_hex() -> &'static str {
    r#"{"format":"fid-quantity-kinds/1","kinds":[{"canonical":"K","kind":"temperature","units":[{"den":"1","num":"1","pi":false,"symbol":"K"}]}],"pi_hex":"c90fdaa22168c234c4c6628b80dc1cd129024e088a67cc74020bbea63b139b22"}"#
}

/// Same units and symbols, one SI factor differs (h = 3601 s); or the same kind with
/// another canonical unit: both are a different definition.
#[test]
fn one_differing_factor_or_canonical_unit_is_a_conflict() {
    for f in [time_h_3601 as fn() -> &'static str, time_in_ms] {
        let err = merge_catalogues(&[QUANTITY_SOURCES[0], src("fid-other", f)]).unwrap_err();
        assert_eq!(err, BuildError::KindConflict { kind: "time".into(), first: "fid-jobs", second: "fid-other" });
    }
}

#[test]
fn a_fragment_carrying_pi_hex_is_refused() {
    let err = merge_catalogues(&[src("fid-pi", with_pi_hex)]).unwrap_err();
    assert_eq!(err, BuildError::Catalogue { krate: "fid-pi", catalogue_code: "bad_catalogue" });
}

/// The merged workspace catalogue's digest, pinned: SHA-256 of `WORKSPACE_CATALOGUE`,
/// computed independently with Python `hashlib` (and by the #259 reviewer).
#[test]
fn the_workspace_catalogue_digest_is_pinned() {
    assert_eq!(
        workspace_catalogue().unwrap().digest().to_string(),
        "sha256:8c5b179d22187a8f7a85b7ad1acce2babe6a7cbd41ff7da18e9b9761a13a5ad4"
    );
}

fn without_jobs() -> Vec<Source> {
    SOURCES.iter().copied().filter(|s| s.krate != "fid-jobs").collect()
}

#[test]
fn every_other_crate_exports_deterministically_and_checks() {
    let sources = without_jobs();
    let a = build(&sources, &geo()).unwrap();
    let mut reversed = sources.clone();
    reversed.reverse();
    let b = build(&reversed, &geo()).unwrap();
    assert_eq!(a, b);
    assert_eq!(check_manifest(&a.bytes, &geo()).unwrap(), a.digest);
    let n: usize = sources.iter().map(|s| (s.defs)().unwrap().len()).sum();
    assert_eq!(a.definitions.len(), n);
    let CValue::Obj(top) = parse(&a.bytes).unwrap() else { panic!() };
    let Some(CValue::Arr(commands)) = top.get("commands") else { panic!() };
    let ids: Vec<String> = commands
        .iter()
        .map(|c| match c {
            CValue::Obj(o) => match (o.get("id"), o.get("major")) {
                (Some(CValue::Str(id)), Some(CValue::Num(m))) => format!("{id}@{m}"),
                _ => panic!(),
            },
            _ => panic!(),
        })
        .collect();
    assert!(ids.iter().any(|i| i == SHOW_MANIFEST));
    assert_eq!(top.get("quantity_catalogue_digest"), Some(&CValue::Str(geo().digest().to_string())));
}

#[test]
fn a_command_declared_twice_across_crates_is_refused() {
    let mut sources = without_jobs();
    sources.push(Source { krate: "fid-align-copy", defs: fid_align::command_defs });
    let got = errors(build(&sources, &geo()).unwrap_err());
    assert!(!got.is_empty());
    assert!(got.iter().all(|(c, r, _)| *r == RuleCode::DuplicateCommand && c.starts_with("align.")), "{got:?}");
}

fn broken() -> Result<Vec<CommandDef>, DefError> {
    Err(DefError::WebMissing)
}

#[test]
fn a_failing_crate_is_named() {
    let sources = [SOURCES[0], Source { krate: "fid-broken", defs: broken }];
    let err = declarations(&sources).unwrap_err();
    assert_eq!(err, BuildError::Declaration { krate: "fid-broken", code: DefError::WebMissing.code() });
    assert_eq!(err.code(), "declaration_build_failed");
    let CValue::Obj(o) = err.to_value() else { panic!() };
    assert_eq!(o.get("crate"), Some(&CValue::Str("fid-broken".into())));
}

/// The commands of the exported workspace manifest, keyed `id@major`, as parsed back from its
/// exact bytes.
fn workspace_commands() -> Vec<(String, std::collections::BTreeMap<String, CValue>)> {
    let export = build_workspace(&workspace_catalogue().unwrap()).unwrap();
    let CValue::Obj(top) = parse(&export.bytes).unwrap() else { panic!() };
    let Some(CValue::Arr(commands)) = top.get("commands") else { panic!() };
    commands
        .iter()
        .map(|c| {
            let CValue::Obj(o) = c else { panic!() };
            let (Some(CValue::Str(id)), Some(CValue::Num(m))) = (o.get("id"), o.get("major")) else { panic!() };
            (format!("{id}@{m}"), o.clone())
        })
        .collect()
}

fn strs(v: Option<&CValue>) -> Vec<String> {
    let Some(CValue::Arr(a)) = v else { panic!("{v:?}") };
    a.iter()
        .map(|x| match x {
            CValue::Str(s) => s.clone(),
            other => panic!("{other:?}"),
        })
        .collect()
}

/// Whether a manifest command is projected to MCP: `mcp` among its surfaces and a projection
/// other than `never`. Read from the manifest only.
fn on_mcp(o: &std::collections::BTreeMap<String, CValue>) -> bool {
    strs(o.get("exposure")).iter().any(|s| s == "mcp")
        && !matches!(o.get("mcp_projection"), Some(CValue::Str(p)) if p == "never")
}

/// The Activity Log writes job lifecycle changes as `command` entries with the reserved ID
/// `fid_journal::stream::LIFECYCLE_COMMAND` (`job.lifecycle`, CORE.JRN.STREAM.ALOG). No
/// registry command may take that ID, or a log reader could not tell the two apart.
#[test]
fn no_command_takes_the_lifecycle_record_id() {
    let reserved = fid_journal::stream::LIFECYCLE_COMMAND;
    assert_eq!(reserved, "job.lifecycle");
    let commands = workspace_commands();
    assert!(!commands.is_empty());
    for (key, o) in &commands {
        assert_ne!(
            o.get("id"),
            Some(&CValue::Str(reserved.to_string())),
            "{key} collides with the lifecycle record ID"
        );
    }
}

/// GEO.TYPES.REG AC-6 (hand-labelled from spec/geo/types/reg/NODE.md, Design): the workspace
/// manifest carries `geo.describe_element@1`, class R0, projected to MCP as `exposed`, so the
/// tool `geo_describe_element` is generated from it.
#[test]
fn geo_describe_element_is_exposed_in_the_workspace_manifest() {
    let commands = workspace_commands();
    let (_, c) = commands.iter().find(|(id, _)| id == "geo.describe_element@1").expect("collected");
    assert_eq!(c.get("mcp_projection"), Some(&CValue::Str("exposed".into())));
    assert_eq!(c.get("side_effect"), Some(&CValue::Str("R0".into())));
    assert!(on_mcp(c));
    assert_eq!(fid_registry::tool_name("geo.describe_element", 1), "geo_describe_element");
}

/// GEO.TYPES.REG errors map onto the bus error catalogue (v2) only: every (code, reason) pair
/// `fid_geo_cmd::GeoCmdError::bus_error` returns is a published pair, and `id_collision` is
/// the shared `conflict/id_collision` (hand-labelled from crates/fid-bus/src/error.rs).
#[test]
fn geo_cmd_errors_map_onto_the_bus_catalogue() {
    use fid_geo_cmd::GeoCmdError as E;
    let id = fid_uuid::Uuid::from_u128(7);
    let all = [
        E::NotFound(id),
        E::NotGeometry(id),
        E::BadParams("x"),
        E::UnknownCommand("x".into()),
        E::IdCollision(id),
        E::BadSourceId("x"),
        E::DuplicateInput(id),
        E::DigestMismatch(id),
        E::BadOutput("x"),
        E::Geometry(fid_geo_types::GeoTypeError::Empty),
        E::KindMismatch { expected: fid_geo_types::ElementKind::Mesh, found: fid_geo_types::ElementKind::PointSet },
        E::InputLimit("x"),
        E::Edge(fid_dep::EdgeError::UnknownElement(id)),
        E::Edge(fid_dep::EdgeError::Cycle(vec![id])),
        E::Prop(fid_dep::PropError::NotInPass(id)),
        E::Id(fid_project::IdError::BadId),
        E::Internal("x"),
    ];
    for e in &all {
        let (code, reason) = e.bus_error();
        let bus = fid_bus::error::BusError::from_wire(code, reason, "d");
        assert!(bus.is_some(), "{e}: {code}/{reason} is not in the bus catalogue");
    }
    let bus = fid_bus::error::BusError::from_wire("conflict", "id_collision", "d").unwrap();
    assert_eq!(E::IdCollision(id).bus_error(), (bus.code().as_str(), bus.reason()));
}

/// CORE.STO.SET's four commands are collected into the workspace manifest (hand-labelled
/// from spec/core/sto/set/NODE.md, Design).
#[test]
fn the_settings_commands_are_in_the_workspace_manifest() {
    let commands = workspace_commands();
    let mut settings: Vec<(String, String)> = commands
        .iter()
        .filter(|(id, _)| id.starts_with("settings."))
        .map(|(id, o)| match o.get("side_effect") {
            Some(CValue::Str(c)) => (id.clone(), c.clone()),
            other => panic!("{other:?}"),
        })
        .collect();
    settings.sort();
    let expected = [
        ("settings.list_values@1", "R0"),
        ("settings.reset_value@1", "W2"),
        ("settings.set_admin_value@1", "A0"),
        ("settings.set_value@1", "W2"),
    ];
    let expected: Vec<(String, String)> = expected.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    assert_eq!(settings, expected);
}

/// REQ-CORE.STO.SET-006 / AC-5, read from the exported manifest itself: the admin command has
/// no MCP exposure (surfaces cli and gui only, projection `never`, `human_only`), the only
/// settings tools an MCP client can see are the three ordinary ones, and no A0 command of the
/// workspace is projected to MCP.
#[test]
fn set_admin_value_has_no_mcp_exposure_in_the_exported_manifest() {
    let commands = workspace_commands();
    let (_, admin) = commands.iter().find(|(id, _)| id == "settings.set_admin_value@1").expect("collected");
    assert_eq!(strs(admin.get("exposure")), ["cli", "gui"]);
    assert_eq!(admin.get("mcp_projection"), Some(&CValue::Str("never".into())));
    assert_eq!(admin.get("human_only"), Some(&CValue::Bool(true)));
    assert_eq!(admin.get("side_effect"), Some(&CValue::Str("A0".into())));
    assert!(!on_mcp(admin));
    let settings_on_mcp: Vec<&str> =
        commands.iter().filter(|(id, o)| id.starts_with("settings.") && on_mcp(o)).map(|(id, _)| id.as_str()).collect();
    assert_eq!(settings_on_mcp, ["settings.list_values@1", "settings.reset_value@1", "settings.set_value@1"]);
    let a0_on_mcp: Vec<&str> = commands
        .iter()
        .filter(|(_, o)| on_mcp(o) && matches!(o.get("side_effect"), Some(CValue::Str(c)) if c == "A0"))
        .map(|(id, _)| id.as_str())
        .collect();
    assert!(a0_on_mcp.is_empty(), "{a0_on_mcp:?}");
}

// ------------------------------------------------------------------ settings manifest

/// The workspace settings manifest collects `fid-bus`'s `bus.fs_roots`, and the exported
/// `settings-manifest.json` declares it admin and machine scope only (CORE.BUS.POL.FS REQ-006:
/// roots are an A0 admin setting). Expected values from the CORE.BUS.POL.FS Design.
#[test]
fn fs_roots_is_admin_and_machine_only_in_the_settings_manifest() {
    let names: Vec<_> = SETTINGS_SOURCES.iter().map(|s| s.krate).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(names, sorted);

    let catalogue = workspace_catalogue().unwrap();
    let registry = workspace_settings(&catalogue).unwrap();
    let bytes = registry.manifest().unwrap();
    let CValue::Obj(doc) = parse(&bytes).unwrap() else { panic!("object") };
    assert_eq!(doc["format"], CValue::Str("fid-settings-manifest/1".into()));
    let CValue::Arr(items) = &doc["settings"] else { panic!("settings") };
    let entry = items
        .iter()
        .find_map(|i| match i {
            CValue::Obj(o) if o.get("key") == Some(&CValue::Str("bus.fs_roots".into())) => Some(o),
            _ => None,
        })
        .expect("bus.fs_roots in the settings manifest");
    assert_eq!(entry["admin"], CValue::Bool(true));
    assert_eq!(entry["scopes"], CValue::Arr(vec![CValue::Str("machine".into())]));
    assert_eq!(entry["owner"], CValue::Str("bus".into()));
    assert_eq!(entry["default"], CValue::Arr(vec![]));
    // The exported manifest reads back to the same registry.
    let again = fid_store::settings::SettingsRegistry::from_manifest(&bytes, &catalogue).unwrap();
    assert_eq!(again, registry);
}

/// A key declared by two crates is refused as a whole.
#[test]
fn a_setting_declared_twice_is_refused() {
    let twice = [SETTINGS_SOURCES[0], SettingsSource { krate: "fid-other", settings: SETTINGS_SOURCES[0].settings }];
    let e = settings(&twice, &geo()).unwrap_err();
    assert_eq!(e.code(), "settings_invalid");
    assert_eq!(e, BuildError::Settings { key: Some("bus.fs_roots".into()), settings_code: "duplicate_key" });
}

/// AUTO.REC.ADOPT's two adoption commands are collected into the workspace manifest as W2,
/// `human_only` and request-only on MCP (hand-labelled from spec/auto/rec/adopt/NODE.md,
/// REQ-001 and D-031).
#[test]
fn the_adoption_commands_are_human_only_and_request_only_in_the_manifest() {
    let commands = workspace_commands();
    for id in ["program.adopt_steps@1", "script.adopt_version@1"] {
        let (_, c) = commands.iter().find(|(i, _)| i == id).unwrap_or_else(|| panic!("{id} collected"));
        assert_eq!(c.get("side_effect"), Some(&CValue::Str("W2".into())), "{id}");
        assert_eq!(c.get("human_only"), Some(&CValue::Bool(true)), "{id}");
        assert_eq!(c.get("mcp_projection"), Some(&CValue::Str("request_only".into())), "{id}");
        let mut exposure = strs(c.get("exposure"));
        exposure.sort();
        assert_eq!(exposure, ["cli", "gui", "mcp"], "{id}");
    }
}
