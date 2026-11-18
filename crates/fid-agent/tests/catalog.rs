//! Acceptance criteria of spec node AGENT.SKILLS.CATALOG (AC-1 .. AC-5) and its error codes.
//! Fixtures and the golden index come from `fixtures/agent/skills/make_fixtures.py`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_agent::{check, parse_catalog, CatalogIndex, Code, Inputs};
use std::path::{Path, PathBuf};

fn fx() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/agent/skills")
}

fn inputs(catalog: &str, skills: &str) -> Inputs {
    Inputs {
        manifest: fx().join("manifest-ok.json"),
        profiles: fx().join("profiles-ok.json"),
        catalog: fx().join(catalog),
        skills: fx().join(skills),
        recipes: fx().join("recipes"),
    }
}

fn findings(i: &Inputs) -> Vec<(Code, String)> {
    check(i).expect_err("check should fail").into_iter().map(|f| (f.code, f.subject)).collect()
}

fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fid-agent-catalog").join(name);
    if d.exists() {
        std::fs::remove_dir_all(&d).unwrap();
    }
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn copy_dir(src: &Path, dst: &Path, skip: &str) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        if e.file_name() == skip {
            continue;
        }
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to, skip);
        } else {
            std::fs::copy(e.path(), to).unwrap();
        }
    }
}

fn version_of<'a>(ix: &'a CatalogIndex, wf: &str) -> &'a str {
    ix.rows.iter().find(|r| r.workflow_id == wf).unwrap().catalog_version.as_str()
}

/// AC-1: the check passes and its index equals the golden file, through the library and the binary.
#[test]
fn ac1_ok_matches_golden() {
    let golden = std::fs::read(fx().join("catalog-index.json")).unwrap();
    let ix = check(&inputs("catalog-ok.json", "skills-ok")).unwrap();
    assert_eq!(
        String::from_utf8(ix.to_canonical_bytes().unwrap()).unwrap(),
        String::from_utf8(golden.clone()).unwrap()
    );
    // D0: a second run gives the same bytes.
    assert_eq!(check(&inputs("catalog-ok.json", "skills-ok")).unwrap(), ix);

    let out = scratch("ac1").join("catalog-index.json");
    let i = inputs("catalog-ok.json", "skills-ok");
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_agent-catalog-check"))
        .arg("--manifest")
        .arg(&i.manifest)
        .arg("--profiles")
        .arg(&i.profiles)
        .arg("--catalog")
        .arg(&i.catalog)
        .arg("--skills")
        .arg(&i.skills)
        .arg("--recipes")
        .arg(&i.recipes)
        .arg("--out")
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(std::fs::read(&out).unwrap(), golden);
}

/// AC-2
#[test]
fn ac2_missing_guide() {
    assert_eq!(
        findings(&inputs("catalog-missing-guide.json", "skills-ok")),
        vec![(Code::MissingGuide, "inspect.ghost".to_string())]
    );
}

/// AC-3
#[test]
fn ac3_stale_skill() {
    assert_eq!(
        findings(&inputs("catalog-ok.json", "skills-stale")),
        vec![(Code::StaleSkill, "inspect.measure_bore".to_string())]
    );
}

/// AC-4
#[test]
fn ac4_too_few_workflows() {
    assert_eq!(
        findings(&inputs("catalog-too-few.json", "skills-ok")),
        vec![(Code::TooFewWorkflows, "inspect".to_string())]
    );
}

/// AC-5: a one-character change to a recipe file of W changes only W's catalog version.
#[test]
fn ac5_recipe_change_moves_only_its_workflow() {
    let before = check(&inputs("catalog-ok.json", "skills-ok")).unwrap();
    let recipes = scratch("ac5");
    copy_dir(&fx().join("recipes"), &recipes, "");
    let p = recipes.join("demo.measure_bore_deep.json");
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.contains("Deep bore"));
    std::fs::write(&p, text.replacen("Deep bore", "deep bore", 1)).unwrap();
    let mut i = inputs("catalog-ok.json", "skills-ok");
    i.recipes = recipes;
    let after = check(&i).unwrap();
    assert_eq!(after.manifest_digest, before.manifest_digest);
    assert_eq!(after.rows.len(), before.rows.len());
    for r in &before.rows {
        let changed = version_of(&after, &r.workflow_id) != r.catalog_version.as_str();
        assert_eq!(changed, r.workflow_id == "inspect.measure_bore", "{}", r.workflow_id);
    }
}

/// REQ-005
#[test]
fn duplicate_workflow_and_unknown_profile() {
    assert_eq!(
        findings(&inputs("catalog-duplicate.json", "skills-ok")),
        vec![(Code::DuplicateWorkflow, "inspect.align_scan".to_string())]
    );
    assert_eq!(
        findings(&inputs("catalog-unknown-profile.json", "skills-ok")),
        vec![(Code::UnknownProfile, "measure".to_string()), (Code::ProfileMismatch, "measure.scan".to_string())]
    );
}

/// REQ-003: guide of another profile; Skill absent.
#[test]
fn profile_mismatch_and_missing_skill() {
    assert_eq!(
        findings(&inputs("catalog-profile-mismatch.json", "skills-ok")),
        vec![(Code::ProfileMismatch, "inspect.canon".to_string())]
    );
    let skills = scratch("missing-skill");
    copy_dir(&fx().join("skills-ok"), &skills, "demo-export-report");
    let mut i = inputs("catalog-ok.json", "skills-ok");
    i.skills = skills;
    assert_eq!(findings(&i), vec![(Code::MissingSkill, "inspect.export_report".to_string())]);
}

/// A recipe named by a workflow must exist.
#[test]
fn missing_recipe() {
    let recipes = scratch("missing-recipe");
    copy_dir(&fx().join("recipes"), &recipes, "demo.align_scan_basic.json");
    let mut i = inputs("catalog-ok.json", "skills-ok");
    i.recipes = recipes;
    assert_eq!(findings(&i), vec![(Code::MissingRecipe, "inspect.align_scan".to_string())]);
}

/// Unreadable inputs are typed, not panics.
#[test]
fn bad_inputs() {
    let mut i = inputs("catalog-ok.json", "skills-ok");
    i.manifest = fx().join("no-such-manifest.json");
    assert_eq!(findings(&i), vec![(Code::BadInput, "manifest".to_string())]);
    let mut i = inputs("catalog-ok.json", "skills-ok");
    i.catalog = fx().join("profiles-ok.json");
    assert_eq!(findings(&i), vec![(Code::BadCatalog, "catalog".to_string())]);
}

/// The repository catalog `agent/catalog.json` stays well formed.
#[test]
fn repository_catalog_parses() {
    let bytes = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../agent/catalog.json")).unwrap();
    parse_catalog(&bytes).unwrap();
}
