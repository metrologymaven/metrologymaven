//! Spec node MIG.LADDER.DEF: the ladder loader and check (AC-1 .. AC-5 at library level, plus
//! schema cases and the production ladder file). Fixtures are written by
//! `fixtures/mig/ladder/def/make_fixtures.py`; the expected findings are hand-labelled from the
//! leaf's rules and the expected digest comes from that script, not from this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_mig::ladder::def::{
    check, load, parse_manifest, Availability, EvaluationBasis, Finding, ManifestCommands, Prereq, SemVer, STAGE_IDS,
};
use std::path::{Path, PathBuf};

/// One edit of the base ladder.
type Edit = Box<dyn FnOnce(&mut CValue)>;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(root().join("fixtures/mig/ladder/def").join(name)).unwrap()
}

fn codes(findings: &[Finding]) -> Vec<String> {
    findings.iter().map(ToString::to_string).collect()
}

/// Parses the base ladder, lets `edit` change it, and returns canonical bytes.
fn mutated(edit: impl FnOnce(&mut CValue)) -> Vec<u8> {
    let mut v = fid_canon::parse(&fixture("ladder-ok.json")).unwrap();
    edit(&mut v);
    fid_canon::to_canonical_bytes(&v).unwrap()
}

fn obj(v: &mut CValue) -> &mut std::collections::BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        _ => panic!("not an object"),
    }
}

fn stage_mut(v: &mut CValue, i: usize) -> &mut std::collections::BTreeMap<String, CValue> {
    match obj(v).get_mut("stages").unwrap() {
        CValue::Arr(a) => obj(&mut a[i]),
        _ => panic!("stages not an array"),
    }
}

fn schema_findings(bytes: &[u8]) -> Vec<String> {
    let r = check(bytes, &fixture("manifest-ok.json"));
    assert!(r.ladder.is_none(), "{:?}", r.findings);
    assert!(!r.findings.is_empty());
    assert!(r.findings.iter().all(|f| f.code() == "LADDER-SCHEMA"), "{:?}", r.findings);
    codes(&r.findings)
}

/// AC-1: ladder-ok.json with manifest-ok.json passes; version and digest are exposed, and the
/// digest matches the reference computed by make_fixtures.py (REQ-005).
#[test]
fn ac1_ok_ladder_passes_with_version_and_digest() {
    let r = check(&fixture("ladder-ok.json"), &fixture("manifest-ok.json"));
    assert!(r.is_ok(), "{:?}", r.findings);
    let ladder = r.ladder.unwrap();
    assert_eq!(ladder.schema(), "mig-ladder@1");
    assert_eq!(ladder.version(), SemVer { major: 0, minor: 9, patch: 0 });
    assert_eq!(ladder.version().to_string(), "0.9.0");
    let want = String::from_utf8(fixture("ladder-ok.digest")).unwrap();
    assert_eq!(ladder.digest().as_str(), want.trim_end());
}

/// REQ-001: five stages in order 1 to 5, each with every field.
#[test]
fn req1_five_stages_with_all_fields() {
    let ladder = load(&fixture("ladder-ok.json")).unwrap();
    let ids: Vec<&str> = ladder.stages().iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, STAGE_IDS);
    for (i, s) in ladder.stages().iter().enumerate() {
        assert_eq!(usize::from(s.order), i + 1);
        assert!(!s.phase.is_empty() && !s.inputs.is_empty() && !s.outputs.is_empty());
        assert!(!s.commands.is_empty() && !s.basis.is_empty() && !s.completion_evidence.is_empty());
    }
    let templates = ladder.stage("templates").unwrap();
    assert_eq!(templates.commands[0].command_id.as_str(), "migrate.import_form@1");
    assert_eq!(templates.commands[0].spec_node, "MIG.TEMPL.MAP");
    assert_eq!(templates.prerequisites, vec![Prereq::Input("imported_results".into())]);
}

/// The digest is over the canonical form: whitespace and key order do not change it.
#[test]
fn req5_digest_is_of_canonical_json() {
    let pretty = load(&fixture("ladder-ok.json")).unwrap();
    let compact = load(&mutated(|_| {})).unwrap();
    assert_eq!(pretty.digest(), compact.digest());
    let other = load(&mutated(|v| {
        obj(v).insert("version".into(), CValue::Str("0.9.1".into()));
    }))
    .unwrap();
    assert_ne!(pretty.digest(), other.digest());
}

/// AC-2: stage 2 lists `incumbent_point_export` (an input only of stage 3) as a prerequisite.
#[test]
fn ac2_forward_prerequisite() {
    let r = check(&fixture("forward.json"), &fixture("manifest-ok.json"));
    assert_eq!(
        r.findings,
        vec![Finding::Forward {
            stage: "templates".into(),
            later: "re_evaluate".into(),
            prerequisite: Prereq::Input("incumbent_point_export".into()),
        }]
    );
    assert!(r.findings[0].to_string().starts_with("LADDER-FORWARD templates re_evaluate"));
}

/// REQ-002 for commands: stage 1 requiring a stage 2 command fails; requiring its own
/// command, an earlier stage's output, or something no stage lists, does not.
#[test]
fn req2_forward_command_and_allowed_cases() {
    let r = check(
        &mutated(|v| {
            stage_mut(v, 0).insert(
                "prerequisites".into(),
                CValue::Arr(vec![CValue::Obj(
                    [("command".to_string(), CValue::Str("migrate.fill_form@1".into()))].into_iter().collect(),
                )]),
            );
        }),
        &fixture("manifest-ok.json"),
    );
    assert_eq!(codes(&r.findings), vec!["LADDER-FORWARD coexist templates command:migrate.fill_form@1"]);

    let r = check(
        &mutated(|v| {
            let prereqs = ["incumbent_report_form", "imported_results", "operator_badge"]
                .into_iter()
                .map(|k| CValue::Obj([("input".to_string(), CValue::Str(k.into()))].into_iter().collect()))
                .collect();
            stage_mut(v, 1).insert("prerequisites".into(), CValue::Arr(prereqs));
        }),
        &fixture("manifest-ok.json"),
    );
    assert!(r.is_ok(), "{:?}", r.findings);
}

/// AC-3: the manifest lacks `migrate.import_form@1`, which the ladder marks built.
#[test]
fn ac3_missing_built_command() {
    let r = check(&fixture("ladder-ok.json"), &fixture("manifest-missing.json"));
    assert_eq!(
        r.findings,
        vec![Finding::MissingCmd { command_id: "migrate.import_form@1".into(), stage: "templates".into() }]
    );
    assert!(r.findings[0].to_string().starts_with("LADDER-MISSING-CMD migrate.import_form@1"));
}

/// AC-4: stage 5 (and stage 4) commands are planned and absent from the manifest; the check
/// passes.
#[test]
fn ac4_planned_commands_are_skipped() {
    let ladder = load(&fixture("ladder-ok.json")).unwrap();
    let manifest = parse_manifest(&fixture("manifest-ok.json")).unwrap();
    for id in ["programs", "history"] {
        for c in &ladder.stage(id).unwrap().commands {
            assert_eq!(c.availability, Availability::Planned);
            assert!(!manifest.contains(&c.command_id));
        }
    }
    assert!(check(&fixture("ladder-ok.json"), &fixture("manifest-ok.json")).is_ok());
    // A built command with the right name but another major is missing.
    let r = check(&fixture("ladder-ok.json"), br#"{"commands":[{"id":"migrate.import_form","major":"2"}]}"#);
    assert!(codes(&r.findings).contains(&"LADDER-MISSING-CMD migrate.import_form@1 templates".to_string()));
}

/// AC-5: stage 4 declares POINTS.
#[test]
fn ac5_history_basis_points() {
    let r = check(&fixture("basis.json"), &fixture("manifest-ok.json"));
    assert_eq!(r.findings, vec![Finding::Basis { stage: "history".into(), basis: EvaluationBasis::Points }]);
    assert!(r.findings[0].to_string().starts_with("LADDER-BASIS"));
}

/// REQ-004 for stages 1 and 3; stages 2 and 5 are unconstrained.
#[test]
fn req4_basis_rules_for_stages_1_and_3() {
    let set_basis = |i: usize, b: &[&str]| {
        let b: Vec<CValue> = b.iter().map(|s| CValue::Str((*s).into())).collect();
        mutated(move |v| {
            stage_mut(v, i).insert("basis".into(), CValue::Arr(b));
        })
    };
    let m = fixture("manifest-ok.json");
    assert_eq!(
        codes(&check(&set_basis(0, &["RESULTS_ONLY", "FEATURES"]), &m).findings),
        vec!["LADDER-BASIS coexist FEATURES"]
    );
    assert_eq!(
        codes(&check(&set_basis(2, &["DIRECT_READING"]), &m).findings),
        vec!["LADDER-BASIS re_evaluate DIRECT_READING"]
    );
    assert_eq!(
        codes(&check(&set_basis(2, &["RESULTS_ONLY"]), &m).findings),
        vec!["LADDER-BASIS re_evaluate RESULTS_ONLY"]
    );
    assert!(check(&set_basis(2, &["FEATURES"]), &m).is_ok());
    assert!(check(&set_basis(1, &["POINTS", "DIRECT_READING"]), &m).is_ok());
    assert!(check(&set_basis(4, &["FEATURES"]), &m).is_ok());
}

/// Findings of several kinds come in algorithm order: forward, manifest, basis.
#[test]
fn findings_are_ordered_and_deterministic() {
    let both = mutated(|v| {
        if let Some(CValue::Arr(a)) = stage_mut(v, 1).get_mut("prerequisites") {
            a.push(CValue::Obj(
                [("input".to_string(), CValue::Str("incumbent_point_export".into()))].into_iter().collect(),
            ));
        }
        stage_mut(v, 3).insert("basis".into(), CValue::Arr(vec![CValue::Str("POINTS".into())]));
    });
    let r = check(&both, &fixture("manifest-missing.json"));
    let got: Vec<&str> = r.findings.iter().map(Finding::code).collect();
    assert_eq!(got, vec!["LADDER-FORWARD", "LADDER-MISSING-CMD", "LADDER-BASIS"]);
    assert_eq!(check(&both, &fixture("manifest-missing.json")), r);
}

#[test]
fn schema_wrong_stage_count_and_order() {
    let four = mutated(|v| {
        if let Some(CValue::Arr(a)) = obj(v).get_mut("stages") {
            a.pop();
        }
    });
    assert_eq!(schema_findings(&four), vec!["LADDER-SCHEMA stages: must be an array of exactly 5 stages"]);
    let swapped = mutated(|v| {
        if let Some(CValue::Arr(a)) = obj(v).get_mut("stages") {
            a.swap(0, 1);
        }
    });
    let f = schema_findings(&swapped);
    assert!(
        f.contains(&"LADDER-SCHEMA stages[0].order: must be 1 (stages are listed in order 1 to 5)".to_string()),
        "{f:?}"
    );
    assert!(f.contains(&"LADDER-SCHEMA stages[0].id: must be coexist".to_string()), "{f:?}");
}

#[test]
fn schema_field_rules() {
    let cases: Vec<(Edit, &str)> = vec![
        (
            Box::new(|v: &mut CValue| {
                obj(v).insert("schema".into(), CValue::Str("mig-ladder@2".into()));
            }),
            "LADDER-SCHEMA schema: must be mig-ladder@1",
        ),
        (
            Box::new(|v: &mut CValue| {
                obj(v).insert("version".into(), CValue::Str("1.0".into()));
            }),
            "LADDER-SCHEMA version: must be MAJOR.MINOR.PATCH",
        ),
        (
            Box::new(|v: &mut CValue| {
                obj(v).insert("extra".into(), CValue::Null);
            }),
            "LADDER-SCHEMA ladder: unknown field extra",
        ),
        (
            Box::new(|v: &mut CValue| {
                stage_mut(v, 0).remove("phase");
            }),
            "LADDER-SCHEMA stages[0]: missing field phase",
        ),
        (
            Box::new(|v: &mut CValue| {
                stage_mut(v, 0).insert("phase".into(), CValue::Str("phase one".into()));
            }),
            "LADDER-SCHEMA stages[0].phase: must be a phase tag such as P1a",
        ),
        (
            Box::new(|v: &mut CValue| {
                stage_mut(v, 1).insert("outputs".into(), CValue::Arr(vec![]));
            }),
            "LADDER-SCHEMA stages[1].outputs: must not be empty",
        ),
        (
            Box::new(|v: &mut CValue| {
                stage_mut(v, 1).insert("inputs".into(), CValue::Arr(vec![CValue::Str("Form".into())]));
            }),
            "LADDER-SCHEMA stages[1].inputs[0]: must be a lowercase snake_case identifier",
        ),
        (
            Box::new(|v: &mut CValue| {
                stage_mut(v, 3).insert("basis".into(), CValue::Arr(vec![CValue::Str("GUESS".into())]));
            }),
            "LADDER-SCHEMA stages[3].basis[0]: must be one of POINTS, FEATURES, RESULTS_ONLY, DIRECT_READING",
        ),
        (
            Box::new(|v: &mut CValue| {
                stage_mut(v, 4)
                    .insert("completion_evidence".into(), CValue::Arr(vec![CValue::Str("migrate.fill_form@1".into())]));
            }),
            "LADDER-SCHEMA stages[4].completion_evidence[0]: migrate.fill_form@1 is not one of this stage's commands",
        ),
        (
            Box::new(|v: &mut CValue| {
                stage_mut(v, 4).insert("prerequisites".into(), CValue::Arr(vec![CValue::Str("x".into())]));
            }),
            "LADDER-SCHEMA stages[4].prerequisites[0]: must be {\"input\": <kind>} or {\"command\": <id>}",
        ),
    ];
    for (edit, want) in cases {
        let f = schema_findings(&mutated(edit));
        assert!(f.iter().any(|l| l == want), "want {want:?} in {f:?}");
    }
}

#[test]
fn schema_command_entries() {
    let bad = |c: CValue| {
        mutated(move |v| {
            stage_mut(v, 3).insert("commands".into(), CValue::Arr(vec![c]));
        })
    };
    let entry = |id: &str, node: &str, avail: &str| {
        CValue::Obj(
            [
                ("command_id".to_string(), CValue::Str(id.into())),
                ("spec_node".to_string(), CValue::Str(node.into())),
                ("availability".to_string(), CValue::Str(avail.into())),
            ]
            .into_iter()
            .collect(),
        )
    };
    let f = schema_findings(&bad(entry("stats.import_history", "RPT.CATALOG.IMPORT.BATCH", "planned")));
    assert!(
        f.contains(&"LADDER-SCHEMA stages[3].commands[0].command_id: not a command ID (name@major)".to_string()),
        "{f:?}"
    );
    let f = schema_findings(&bad(entry("stats.import_history@1", "rpt.catalog", "planned")));
    assert!(f.iter().any(|l| l.starts_with("LADDER-SCHEMA stages[3].commands[0].spec_node")), "{f:?}");
    let f = schema_findings(&bad(entry("stats.import_history@1", "RPT.CATALOG.IMPORT.BATCH", "soon")));
    assert!(
        f.contains(&"LADDER-SCHEMA stages[3].commands[0].availability: must be built or planned".to_string()),
        "{f:?}"
    );
}

/// Bounded parsing: invalid JSON, duplicate keys, oversize and over-deep input are schema
/// findings, never a crash.
#[test]
fn bounded_parse() {
    for bytes in [
        b"not json".to_vec(),
        br#"{"schema":"mig-ladder@1","schema":"mig-ladder@1"}"#.to_vec(),
        vec![b' '; fid_mig::ladder::def::MAX_LADDER_BYTES + 1],
        format!("{}{}", "[".repeat(40), "]".repeat(40)).into_bytes(),
        b"[]".to_vec(),
    ] {
        let f = schema_findings(&bytes);
        assert_eq!(f.len(), 1, "{f:?}");
    }
}

#[test]
fn manifest_parsing() {
    let m = parse_manifest(&fixture("manifest-ok.json")).unwrap();
    assert_eq!(m.len(), 8);
    assert!(parse_manifest(br#"{"commands":[]}"#).unwrap().is_empty());
    assert_eq!(parse_manifest(br#"{"commands":[]}"#).unwrap(), ManifestCommands::default());
    let bads: [&[u8]; 4] =
        [b"[]", br#"{"format":1}"#, br#"{"commands":[{"id":"x"}]}"#, br#"{"commands":[{"id":"x","major":1.5}]}"#];
    for bad in bads {
        assert_eq!(parse_manifest(bad).unwrap_err().code(), "LADDER-SCHEMA");
    }
    // A bad manifest is reported after the forward findings and before the basis findings.
    let r = check(&fixture("basis.json"), b"{}");
    let got: Vec<&str> = r.findings.iter().map(Finding::code).collect();
    assert_eq!(got, vec!["LADDER-SCHEMA", "LADDER-BASIS"]);
}

#[test]
fn semver_parse() {
    assert_eq!(SemVer::parse("1.2.3"), Some(SemVer { major: 1, minor: 2, patch: 3 }));
    for bad in ["1.2", "1.2.3.4", "01.2.3", "1.2.3-rc.1", "", "a.b.c", "1..3"] {
        assert_eq!(SemVer::parse(bad), None, "{bad}");
    }
}

/// The production ladder (`data/mig/ladder.json`) loads, passes with an empty manifest (every
/// command is still planned), and every command is listed in its owning spec node's
/// `journal:` frontmatter, so command IDs and owners are verified against the spec tree.
#[test]
fn production_ladder_matches_spec_tree() {
    let bytes = std::fs::read(root().join("data/mig/ladder.json")).unwrap();
    let r = check(&bytes, br#"{"commands":[]}"#);
    assert!(r.is_ok(), "{:?}", r.findings);
    let ladder = r.ladder.unwrap();
    assert_eq!(ladder.version().to_string(), "1.0.0");
    for s in ladder.stages() {
        for c in &s.commands {
            let mut path = root().join("spec");
            for seg in c.spec_node.split('.') {
                path.push(seg.to_lowercase());
            }
            path.push("NODE.md");
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let journal = text.lines().find(|l| l.starts_with("journal:")).unwrap_or_default();
            assert!(
                journal.contains(c.command_id.as_str()),
                "{} is not in the journal list of {} ({journal})",
                c.command_id.as_str(),
                c.spec_node
            );
        }
    }
}
