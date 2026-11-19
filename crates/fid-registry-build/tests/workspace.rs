//! The registry build step over the crates on main (spec node CORE.REG.MAN, Design "Build
//! step"). The expected errors are hand-labelled from the `fid-jobs` schemas
//! (`crates/fid-jobs/src/commands.rs`: `progress` is a `number` without `x-quantity`, and
//! `timeout` declares `x-quantity: "time"`), not taken from a run.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, CValue};
use fid_registry::manifest::{check_manifest, ManifestError, SHOW_MANIFEST};
use fid_registry::{Catalogue, CommandDef, DefError, QuantityCatalogue, RuleCode};
use fid_registry_build::{build, build_workspace, declarations, manifest_in_force, BuildError, Source, SOURCES};
use std::path::Path;

fn geo() -> Catalogue {
    Catalogue::geo_frame().unwrap()
}

fn with_time() -> Catalogue {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/core/reg/def/schema/catalogue/with_time.json");
    Catalogue::from_json(&std::fs::read(p).unwrap()).unwrap()
}

fn errors(e: BuildError) -> Vec<(String, RuleCode, Option<String>)> {
    match e {
        BuildError::Manifest(ManifestError::DeclarationsInvalid(v)) => {
            v.into_iter().map(|e| (e.command, e.rule, e.field)).collect()
        }
        other => panic!("unexpected {other}"),
    }
}

fn progress(cmd: &str, field: &str) -> (String, RuleCode, Option<String>) {
    (cmd.to_string(), RuleCode::MissingQuantity, Some(field.to_string()))
}

fn missing_progress() -> Vec<(String, RuleCode, Option<String>)> {
    vec![
        progress("job.cancel_job", "/result_schema/properties/progress"),
        progress("job.list_jobs", "/result_schema/properties/jobs/items/properties/progress"),
        progress("job.show_status", "/result_schema/properties/progress"),
        progress("job.wait_job", "/result_schema/properties/progress"),
    ]
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
}

/// The open blocker, surfaced as typed errors and never skipped: with the GEO.FRAME.UNITS
/// catalogue the workspace does not export.
#[test]
fn workspace_is_blocked_on_the_quantity_catalogue() {
    let mut expected = missing_progress();
    expected.push((
        "job.wait_job".to_string(),
        RuleCode::UnknownQuantity,
        Some("/params_schema/properties/timeout/x-quantity".to_string()),
    ));
    expected.sort();
    let mut got = errors(build_workspace(&geo()).unwrap_err());
    got.sort();
    assert_eq!(got, expected);
    assert_eq!(manifest_in_force(geo()).unwrap_err().code(), "declarations_invalid");
}

/// A catalogue that holds `time` (test data) clears the `time` error; `progress` still needs a
/// `dimensionless` kind and its `x-quantity`.
#[test]
fn a_time_kind_leaves_only_the_progress_fields() {
    let mut got = errors(build_workspace(&with_time()).unwrap_err());
    got.sort();
    assert_eq!(got, missing_progress());
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
