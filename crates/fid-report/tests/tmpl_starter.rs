//! Integration tests for `RPT.MODEL.TMPL.STARTER` (built-in unlocked starter templates).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{digest_bytes, parse, CValue};
use fid_project::Uuid;
use fid_report::bind::expand::to_canonical_bytes;
use fid_report::bind::{
    expand, CharacteristicItem, EntityKind, JournalHead, MemoryJournal, MemoryModel, ResolveContext, ResultItem,
    RunItem, StageItem,
};
use fid_report::tmpl::format::{parse_template, serialise};
use fid_report::tmpl::slot::validate_template;
use fid_report::tmpl::starter::{
    lookup_label, parse_starter, resolve_starter_name, starter_name_for_id, starter_registry, CHAR_TABLE_NAME,
    DEVIATION_SUMMARY_NAME, MULTISTAGE_TABLE_NAME, STARTERS, STARTER_LABELS, STARTER_VERSION,
};
use fid_report::tmpl::ver::{list_template, show_template, BuiltinRegistry, MemoryTemplateBackend, ShowTemplate};
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn rpt_fixture(rel: &str) -> CValue {
    let p = repo_root().join("fixtures/rpt/model").join(rel);
    parse(&std::fs::read(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))).unwrap()
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(o) => o.get(k).unwrap_or_else(|| panic!("missing key {k}")),
        _ => panic!("expected object for {k}"),
    }
}

fn st<'a>(v: &'a CValue, k: &str) -> &'a str {
    match get(v, k) {
        CValue::Str(s) => s,
        _ => panic!("{k} is not a string"),
    }
}

fn arr(v: &CValue) -> &Vec<CValue> {
    match v {
        CValue::Arr(a) => a,
        _ => panic!("expected array"),
    }
}

fn uuid(s: &str) -> Uuid {
    Uuid::parse(s).unwrap()
}

fn load_bracket_journal() -> MemoryJournal {
    let mut j = MemoryJournal::new();
    for e in arr(get(&rpt_fixture("bracket_3stage/journal.json"), "entries")) {
        let kind = match st(e, "kind") {
            "result" => EntityKind::Result,
            "characteristic" => EntityKind::Characteristic,
            "named_view" => EntityKind::NamedView,
            "series" => EntityKind::Series,
            "statement" => EntityKind::Statement,
            k => panic!("unknown kind {k}"),
        };
        let id = match get(e, "id") {
            CValue::Str(s) => Some(uuid(s)),
            _ => None,
        };
        let head = JournalHead::parse(st(e, "head")).unwrap();
        j.append(head, uuid(st(e, "project")), kind, id, st(e, "text").as_bytes().to_vec()).unwrap();
    }
    j
}

fn load_bracket_model() -> (MemoryModel, Uuid) {
    let m = rpt_fixture("bind/expand/model.json");
    let project = uuid(st(&m, "project"));
    let characteristics = arr(get(&m, "characteristics"))
        .iter()
        .map(|c| CharacteristicItem {
            id: uuid(st(c, "id")),
            name: st(c, "name").to_owned(),
            char_type: st(c, "type").to_owned(),
            balloon: Some(st(c, "balloon").parse().unwrap()),
            element: Some(uuid(st(c, "element"))),
            tags: arr(get(c, "tags"))
                .iter()
                .map(|t| match t {
                    CValue::Str(s) => s.clone(),
                    _ => panic!("tag not string"),
                })
                .collect(),
        })
        .collect();
    let elements = arr(get(&m, "elements"))
        .iter()
        .map(|v| match v {
            CValue::Str(s) => uuid(s),
            _ => panic!("element id"),
        })
        .collect();
    let stages = arr(get(&m, "stages"))
        .iter()
        .map(|s| StageItem {
            id: uuid(st(s, "id")),
            run: uuid(st(s, "run")),
            removed: matches!(get(s, "removed"), CValue::Bool(true)),
        })
        .collect();
    let runs = arr(get(&m, "runs"))
        .iter()
        .map(|r| RunItem { id: uuid(st(r, "id")), start_ms: st(r, "start_ms").parse().unwrap() })
        .collect();
    let results = arr(get(&m, "results"))
        .iter()
        .map(|r| ResultItem {
            id: uuid(st(r, "id")),
            characteristic: uuid(st(r, "characteristic")),
            stage: uuid(st(r, "stage")),
        })
        .collect();

    let model = MemoryModel {
        head: JournalHead::parse(st(&m, "head")).unwrap(),
        characteristics,
        elements,
        stages,
        runs,
        results,
    };
    (model, project)
}

/// AC-1: `list_template` on a `BuiltinRegistry::with_starters()` lists all three starter IDs,
/// each unlocked (`locked == false`), built-in, at version 1.
#[test]
fn ac1_list_template_exposes_three_unlocked_version_1_starters() {
    let reg = BuiltinRegistry::with_starters().expect("register starters");
    let backend = MemoryTemplateBackend::new(uuid("019a9a9a-0000-7000-8000-0000000b3a5e"));
    let listed = list_template(&backend, &reg).expect("list_template");
    assert_eq!(listed.len(), 3);

    let expected_names = [CHAR_TABLE_NAME, DEVIATION_SUMMARY_NAME, MULTISTAGE_TABLE_NAME];
    for (info, name) in listed.iter().zip(expected_names.iter()) {
        let expected_id = resolve_starter_name(name).expect("resolve starter name");
        assert_eq!(info.template.id, expected_id.uuid());
        assert_eq!(info.template.version, STARTER_VERSION);
        assert!(info.template.builtin);
        assert!(!info.locked, "{name} must be unlocked");
        assert_eq!(starter_name_for_id(expected_id), Some(*name));

        // show_template returns the exact canonical bytes of the embedded starter.
        let (shown_ref, shown_val) = show_template(
            &backend,
            &reg,
            &ShowTemplate { template: expected_id, version: Some(STARTER_VERSION), builtin: true },
        )
        .expect("show starter template");
        assert_eq!(shown_ref, info.template);
        let desc = STARTERS.iter().find(|d| d.name == *name).unwrap();
        assert_eq!(fid_canon::to_canonical_bytes(&shown_val).unwrap(), desc.canonical_json);
    }
}

/// AC-2 & REQ-002: every starter passes format (`parse_template`), canonical serialisation,
/// slot validation (`validate_template`), unlocked check, and has catalog-key labels backed by
/// English source text in `STARTER_LABELS` and `labels.en.json`.
#[test]
fn ac2_starters_pass_format_slot_and_catalog_label_validation() {
    let labels_path = repo_root().join("crates/fid-report/templates/starter/labels.en.json");
    let labels_val = parse(&std::fs::read(&labels_path).expect("read labels.en.json")).unwrap();
    let CValue::Obj(labels_map) = &labels_val else {
        panic!("labels.en.json must be a JSON object");
    };

    // STARTER_LABELS must be sorted and match labels.en.json 1-to-1.
    assert_eq!(STARTER_LABELS.len(), labels_map.len());
    for (k, v) in STARTER_LABELS {
        assert_eq!(lookup_label(k), Some(*v));
        assert_eq!(labels_map.get(*k), Some(&CValue::Str((*v).to_owned())));
    }

    for desc in &STARTERS {
        let tmpl = parse_starter(desc.name).expect("parse_starter");
        assert!(!tmpl.locked(), "{} must be unlocked", desc.name);
        assert_eq!(tmpl.template_id().uuid().to_string(), desc.id);
        // Canonical JSON bytes must match serialise(&tmpl) identically (D0).
        let re_encoded = serialise(&tmpl).expect("serialise");
        assert_eq!(re_encoded.as_bytes(), desc.canonical_json);

        // Slot validation must succeed.
        let slots = validate_template(&tmpl).expect("validate_template");
        assert!(!slots.is_empty(), "{} must define at least one slot", desc.name);

        // Title and every payload key ending with `text` / `title_key` / `heading_key` must be
        // a known `rpt.starter.*` catalog key with English source text.
        let title_val = tmpl.title().to_value();
        let title_key = st(&title_val, "text");
        assert!(title_key.starts_with("rpt.starter."));
        assert!(lookup_label(title_key).is_some(), "missing label for {title_key}");

        let doc = parse(desc.canonical_json).unwrap();
        check_catalog_keys(&doc);
    }

    // Starter registry registration is idempotent and deterministic.
    let r1 = starter_registry().expect("r1");
    let r2 = starter_registry().expect("r2");
    assert_eq!(r1, r2);
}

fn check_catalog_keys(v: &CValue) {
    match v {
        CValue::Obj(o) => {
            for (k, val) in o {
                if matches!(k.as_str(), "title_key" | "heading_key") {
                    let CValue::Str(key) = val else {
                        panic!("{k} must be a string");
                    };
                    assert!(key.starts_with("rpt.starter."), "expected catalog key: {key}");
                    assert!(lookup_label(key).is_some(), "missing English source text for {key}");
                } else if k == "text" {
                    if let CValue::Str(key) = val {
                        if key.starts_with("rpt.starter.") {
                            assert!(lookup_label(key).is_some(), "missing English source text for {key}");
                        }
                    }
                }
                check_catalog_keys(val);
            }
        }
        CValue::Arr(a) => {
            for item in a {
                check_catalog_keys(item);
            }
        }
        _ => {}
    }
}

/// AC-3 (expansion against `fixtures/rpt/model/bracket_3stage`): each starter template expands
/// cleanly against the 3-stage bracket fixture with zero errors and matches its committed
/// deterministic golden expansion digest (`fixtures/rpt/model/golden/starter-*.digest`).
#[test]
fn ac3_starters_expand_cleanly_against_bracket_3stage_with_golden_digests() {
    let journal = load_bracket_journal();
    let (model, project) = load_bracket_model();
    let ctx = ResolveContext { project, head: model.head, stages: Vec::new(), runs: Vec::new(), sources: Vec::new() };

    let cases = [
        (CHAR_TABLE_NAME, "starter-char-table.digest"),
        (DEVIATION_SUMMARY_NAME, "starter-deviation-summary.digest"),
        (MULTISTAGE_TABLE_NAME, "starter-multistage-table.digest"),
    ];

    let golden_dir = repo_root().join("fixtures/rpt/model/golden");
    for (name, digest_file) in cases {
        let tmpl =
            parse_template(STARTERS.iter().find(|d| d.name == name).unwrap().canonical_json).expect("parse starter");
        let expanded =
            expand(&tmpl, &ctx, &model, &journal, None).unwrap_or_else(|e| panic!("expand {name} failed: {e}"));
        assert!(!expanded.is_empty(), "{name} expanded to zero blocks");

        let bytes = to_canonical_bytes(&expanded).expect("canonical expansion bytes");
        let digest = digest_bytes(&bytes).to_string();

        let expected_path = golden_dir.join(digest_file);
        let expected =
            std::fs::read_to_string(&expected_path).unwrap_or_else(|e| panic!("read {}: {e}", expected_path.display()));
        assert_eq!(digest.trim(), expected.trim(), "golden digest mismatch for {name} (computed {digest})");
    }
}
