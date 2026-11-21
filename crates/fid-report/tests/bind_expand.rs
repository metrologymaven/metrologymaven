//! Acceptance criteria of spec node RPT.MODEL.BIND.EXPAND (AC-1 .. AC-5) and the pinned
//! rules of `fid_report::bind::expand`. Expected expansions come from the independent Python
//! reference `fixtures/rpt/model/bind/expand/make_fixtures.py`; error codes are hand-labelled.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::disallowed_types)]

use fid_canon::{parse, CValue};
use fid_project::Uuid;
use fid_report::bind::expand::{to_canonical_bytes, MAX_PAGES};
use fid_report::bind::*;
use fid_report::tmpl::format::{parse_template, Template};
use fid_report::tmpl::slot::SavedQueryId;
use std::path::PathBuf;

fn path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rpt/model").join(rel)
}

fn fixture(rel: &str) -> CValue {
    parse(&std::fs::read(path(rel)).unwrap()).unwrap()
}

fn get<'a>(v: &'a CValue, k: &str) -> &'a CValue {
    match v {
        CValue::Obj(o) => o.get(k).unwrap_or_else(|| panic!("no key {k}")),
        _ => panic!("not an object"),
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
        _ => panic!("not an array"),
    }
}

fn uuid(s: &str) -> Uuid {
    Uuid::parse(s).unwrap()
}

fn journal() -> MemoryJournal {
    let mut j = MemoryJournal::new();
    for e in arr(get(&fixture("bracket_3stage/journal.json"), "entries")) {
        let kind = match st(e, "kind") {
            "result" => EntityKind::Result,
            "characteristic" => EntityKind::Characteristic,
            "named_view" => EntityKind::NamedView,
            "series" => EntityKind::Series,
            "statement" => EntityKind::Statement,
            k => panic!("kind {k}"),
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

fn model() -> MemoryModel {
    let m = fixture("bind/expand/model.json");
    let characteristics = arr(get(&m, "characteristics"))
        .iter()
        .map(|c| CharacteristicItem {
            id: uuid(st(c, "id")),
            name: st(c, "name").to_string(),
            char_type: st(c, "type").to_string(),
            balloon: Some(st(c, "balloon").parse().unwrap()),
            element: Some(uuid(st(c, "element"))),
            tags: arr(get(c, "tags"))
                .iter()
                .map(|t| match t {
                    CValue::Str(s) => s.clone(),
                    _ => panic!("tag"),
                })
                .collect(),
        })
        .collect();
    let ids = |k: &str| -> Vec<Uuid> {
        arr(get(&m, k))
            .iter()
            .map(|v| match v {
                CValue::Str(s) => uuid(s),
                _ => panic!("id"),
            })
            .collect()
    };
    MemoryModel {
        head: JournalHead::parse(st(&m, "head")).unwrap(),
        characteristics,
        elements: ids("elements"),
        stages: arr(get(&m, "stages"))
            .iter()
            .map(|s| StageItem {
                id: uuid(st(s, "id")),
                run: uuid(st(s, "run")),
                removed: matches!(get(s, "removed"), CValue::Bool(true)),
            })
            .collect(),
        runs: arr(get(&m, "runs"))
            .iter()
            .map(|r| RunItem { id: uuid(st(r, "id")), start_ms: st(r, "start_ms").parse().unwrap() })
            .collect(),
        results: arr(get(&m, "results"))
            .iter()
            .map(|r| ResultItem {
                id: uuid(st(r, "id")),
                characteristic: uuid(st(r, "characteristic")),
                stage: uuid(st(r, "stage")),
            })
            .collect(),
    }
}

fn template(name: &str) -> Template {
    parse_template(&std::fs::read(path(&format!("bind/expand/templates/{name}.json"))).unwrap()).unwrap()
}

fn ctx(m: &MemoryModel) -> ResolveContext<'static> {
    let project = uuid(st(&fixture("bind/expand/model.json"), "project"));
    ResolveContext { project, head: m.head, stages: Vec::new(), runs: Vec::new(), sources: Vec::new() }
}

fn run(name: &str, m: &MemoryModel) -> Result<Vec<ExpandedBlock>, ExpandError> {
    expand(&template(name), &ctx(m), m, &journal(), None)
}

fn expected(name: &str) -> String {
    st(get(&fixture("bind/expand/expected.json"), name), "canonical").to_string()
}

fn canonical(blocks: &[ExpandedBlock]) -> String {
    String::from_utf8(to_canonical_bytes(blocks).unwrap()).unwrap()
}

fn check(name: &str) -> Vec<ExpandedBlock> {
    let m = model();
    let out = run(name, &m).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(canonical(&out), expected(name), "{name}");
    out
}

/// AC-1: 50 rows in ascending characteristic ID, three stage columns in project stage order
/// (which differs from stage ID order), the removed stage left out.
#[test]
fn ac1_multistage_rows_and_stage_columns() {
    let out = check("multistage");
    let m = model();
    let table = out.iter().find(|b| b.role == Role::Table).unwrap();
    let live: Vec<String> = m.stages.iter().filter(|s| !s.removed).map(|s| s.id.to_string()).collect();
    assert_eq!(table.columns.iter().map(|c| c.id.clone()).collect::<Vec<_>>(), live);
    let mut sorted = live.clone();
    sorted.sort();
    assert_ne!(sorted, live, "the fixture's project order must differ from ID order");
    let rows: Vec<&ExpandedBlock> = out.iter().filter(|b| b.role == Role::Row).collect();
    assert_eq!(rows.len(), 50);
    let ids: Vec<&str> = rows.iter().map(|r| r.item.as_ref().unwrap().id.as_str()).collect();
    let mut asc = ids.clone();
    asc.sort();
    assert_eq!(ids, asc);
    assert!(rows.iter().all(|r| r.cells.len() == 3 && r.cells.iter().all(Option::is_some)));
    assert!(rows.iter().all(|r| r.binding.refs().len() == 4));
}

/// AC-2: equal sort keys fall back to ascending stable ID, and shuffling the model's
/// insertion order (SplitMix64, seed 11) changes nothing.
#[test]
fn ac2_equal_keys_and_shuffled_input() {
    let out = check("by_balloon");
    let rows: Vec<&ExpandedBlock> = out.iter().filter(|b| b.role == Role::Row).collect();
    let first_two: Vec<&str> = rows.iter().take(2).map(|r| r.item.as_ref().unwrap().id.as_str()).collect();
    assert!(first_two[0] < first_two[1]);
    let mut m = model();
    let mut x: u64 = 11;
    let mut next = || {
        x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = x;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    };
    for i in (1..m.characteristics.len()).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        m.characteristics.swap(i, j);
    }
    for i in (1..m.results.len()).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        m.results.swap(i, j);
    }
    m.elements.reverse();
    m.runs.reverse();
    for name in ["by_balloon", "multistage", "page_group", "tags"] {
        assert_eq!(canonical(&run(name, &m).unwrap()), expected(name), "{name} after shuffling");
    }
}

/// AC-3: a page group over 4 elements gives 4 page copies, with the same derived UUIDv5 IDs
/// when expanded twice.
#[test]
fn ac3_page_group_ids_are_stable() {
    let a = check("page_group");
    let b = run("page_group", &model()).unwrap();
    assert_eq!(a, b);
    let copies: std::collections::BTreeSet<Uuid> = a.iter().filter(|x| x.page_item.is_some()).map(|x| x.page).collect();
    assert_eq!(copies.len(), 4);
    for x in a.iter().filter(|x| x.page_item.is_some()) {
        let item = x.page_item.as_ref().unwrap();
        assert_eq!(x.page, fid_project::uuid_v5(&x.source_page.uuid(), item.id.as_bytes()));
    }
}

/// REQ-003: columns over stages in project order, two per group.
#[test]
fn stage_column_groups_follow_project_order() {
    let out = check("stage_columns");
    let groups: Vec<usize> = out.iter().filter(|b| b.role == Role::ColumnGroup).map(|b| b.columns.len()).collect();
    assert_eq!(groups, [2, 1]);
}

/// Tags select by bytes; the outcome filter reads decisions from the journaled records.
#[test]
fn tags_and_outcome_filter() {
    let tags = check("tags");
    assert_eq!(tags.iter().filter(|b| b.role == Role::Row).count(), 7);
    let none = check("nonconforming");
    assert_eq!(none.iter().filter(|b| b.role == Role::Row).count(), 0);
}

struct Query(Vec<Ref>);

impl QueryProvider for Query {
    fn run(&self, _: SavedQueryId, _: &JournalHead) -> Result<Vec<Ref>, BindError> {
        Ok(self.0.clone())
    }
}

/// AC-4: a saved query without a provider is `report.query_unavailable` naming the slot;
/// with one, its characteristics are the rows; a non-characteristic answer is refused.
#[test]
fn ac4_saved_query_needs_a_provider() {
    let m = model();
    let want = get(&fixture("bind/expand/expected.json"), "saved_query").clone();
    let e = run("saved_query", &m).unwrap_err();
    assert_eq!(e.code(), st(&want, "error"));
    assert_eq!(e.block().unwrap().uuid().to_string(), st(&want, "block"));
    let c = m.characteristics[4].id;
    let q = Query(vec![Ref::Characteristic(c)]);
    let out = expand(&template("saved_query"), &ctx(&m), &m, &journal(), Some(&q)).unwrap();
    let rows: Vec<&ExpandedBlock> = out.iter().filter(|b| b.role == Role::Row).collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].item.as_ref().unwrap().id, c.to_string());
    let q = Query(vec![Ref::Result(m.results[0].id)]);
    let e = expand(&template("saved_query"), &ctx(&m), &m, &journal(), Some(&q)).unwrap_err();
    assert_eq!(e.code(), "report.query_result_invalid");
}

/// AC-5: 100 001 selected rows is `report.expansion_too_large`, refused before any item is
/// looked up; 100 000 is within the bound. Pages are bounded the same way.
#[test]
fn ac5_expansion_too_large() {
    let base = model();
    let many = |n: usize| -> Vec<CharacteristicItem> {
        (0..n)
            .map(|i| CharacteristicItem {
                id: uuid(&format!("019a9a9b-00ff-7000-8000-{:012x}", i + 1)),
                name: String::new(),
                char_type: "DiameterCharacteristic".to_string(),
                balloon: None,
                element: None,
                tags: Vec::new(),
            })
            .collect()
    };
    let mut m = base.clone();
    m.characteristics = many(100_001);
    m.results.clear();
    let e = run("multistage", &m).unwrap_err();
    assert_eq!(e.code(), "report.expansion_too_large");
    assert_eq!(e.to_value(), {
        let mut o = std::collections::BTreeMap::new();
        o.insert("block".to_string(), CValue::Str(e.block().unwrap().uuid().to_string()));
        o.insert("code".to_string(), CValue::Str("report.expansion_too_large".to_string()));
        o.insert("limit".to_string(), CValue::Str("rows".to_string()));
        CValue::Obj(o)
    });
    // At the bound the rows pass the limit check (they then fail the journal lookup: these
    // synthetic characteristics are not journaled).
    m.characteristics = many(100_000);
    assert_eq!(run("multistage", &m).unwrap_err().code(), "report.not_found");
    // Pages: one cover page plus one copy per element.
    let mut m = base;
    m.elements = (0..MAX_PAGES).map(|i| uuid(&format!("019a9a9b-00fe-7000-8000-{:012x}", i + 1))).collect();
    assert_eq!(run("page_group", &m).unwrap_err().code(), "report.expansion_too_large");
    m.elements.pop();
    assert_eq!(run("page_group", &m).unwrap().iter().filter(|b| b.role == Role::Copy).count(), 1 + 2 * (MAX_PAGES - 1));
}

/// The stage selection of the context: named stages (in multi-stage order whatever the
/// order given), runs narrowing them, and unknown or removed stages refused.
#[test]
fn context_stage_and_run_selection() {
    let m = model();
    let live: Vec<Uuid> = m.stages.iter().filter(|s| !s.removed).map(|s| s.id).collect();
    let j = journal();
    let t = template("multistage");
    let mut c = ctx(&m);
    c.stages = vec![live[2], live[0]];
    let out = expand(&t, &c, &m, &j, None).unwrap();
    let table = out.iter().find(|b| b.role == Role::Table).unwrap();
    assert_eq!(table.columns.iter().map(|x| uuid(&x.id)).collect::<Vec<_>>(), [live[0], live[2]]);
    let mut c = ctx(&m);
    c.runs = vec![m.stages[3].run];
    let out = expand(&t, &c, &m, &j, None).unwrap();
    assert_eq!(out.iter().find(|b| b.role == Role::Table).unwrap().columns.len(), 1);
    let mut c = ctx(&m);
    c.stages = vec![m.stages[1].id]; // removed
    assert_eq!(expand(&t, &c, &m, &j, None).unwrap_err().code(), "report.not_found");
    // An unknown head is refused.
    let mut c = ctx(&m);
    c.head = JournalHead::parse(&format!("sha256:{}", "7".repeat(64))).unwrap();
    assert_eq!(expand(&t, &c, &m, &j, None).unwrap_err().code(), "report.not_found");
}

/// Expansion copies no value: no expanded block holds a number or value text.
#[test]
fn expansion_holds_no_value() {
    let out = check("multistage");
    let v = parse(&to_canonical_bytes(&out).unwrap()).unwrap();
    assert_eq!(find_value(&v, true), None);
}

/// A repeated model ID is refused.
#[test]
fn repeated_model_ids_are_refused() {
    let mut m = model();
    let dup = m.characteristics[0].clone();
    m.characteristics.push(dup);
    assert_eq!(run("multistage", &m).unwrap_err().code(), "report.store_invalid");
}
