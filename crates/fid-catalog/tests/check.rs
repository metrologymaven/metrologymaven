//! Spec node RPT.CATALOG.CHECK: the integrity check and the content digest (library level).
//!
//! Every expected status, count, stale list and digest comes from
//! `fixtures/rpt/catalog/check/make_cases.py` (Python's json and hashlib, written from the
//! leaf's rules), never from this crate. The source projects are synthetic journals read
//! through the `ProjectSources` trait; the journal-backed sources, the CLI and the rebuild land
//! with RPT.CATALOG.STORE.REG and RPT.CATALOG.STORE.INDEX (the leaf's open ACs).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_catalog::{
    check_catalog, content_digest, Availability, Catalog, CatalogEntryRow, CatalogError, CatalogText, CheckError,
    DecisionOutcome, EntryKey, EntryStatus, EntryValue, EvaluationBasis, EvidenceLabels, Interpretation, Mode,
    OpenOptions, ProjectCounts, ProjectRow, ProjectSources, RecordLookup, RecordStatus, SourceRecord, SourceUnreadable,
    UecClass, PLAN,
};
use fid_project::{Bits, IdError, ObservationSource};
use fid_types::Digest;
use fid_uuid::Uuid;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

const CASES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/rpt/catalog/check/check_cases.json");

// ------------------------------------------------------------------ helpers

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
        let name = format!("fid-catchk-{tag}-{}-{nanos}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
        let p = std::path::absolute(std::env::temp_dir().join(name)).unwrap();
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Fixed;

impl ObservationSource for Fixed {
    fn now_ms(&mut self) -> Result<u64, IdError> {
        Ok(1_763_700_000_000)
    }
    fn random_bits(&mut self, n: u32) -> Result<Bits, IdError> {
        Bits::new(n, 0x1234 & ((1u128 << n) - 1))
    }
}

fn open(path: &Path, mode: Mode) -> Catalog {
    let mut src = Fixed;
    Catalog::open_with(
        path,
        mode,
        OpenOptions { source: &mut src, owner: None, plan: PLAN, busy_timeout: Duration::from_millis(200) },
    )
    .unwrap()
}

fn cases() -> CValue {
    fid_canon::parse(&std::fs::read(CASES).unwrap()).unwrap()
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("object expected, got {other:?}"),
    }
}

fn arr(v: &CValue) -> &Vec<CValue> {
    match v {
        CValue::Arr(a) => a,
        other => panic!("array expected, got {other:?}"),
    }
}

fn s(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        other => panic!("string expected, got {other:?}"),
    }
}

fn b(v: &CValue) -> bool {
    match v {
        CValue::Bool(b) => *b,
        other => panic!("bool expected, got {other:?}"),
    }
}

fn n(v: &CValue) -> u64 {
    match v {
        CValue::Num(f) => {
            assert!(*f >= 0.0 && f.fract() == 0.0);
            *f as u64
        }
        other => panic!("number expected, got {other:?}"),
    }
}

fn get<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> &'a CValue {
    o.get(k).unwrap_or_else(|| panic!("missing key {k}"))
}

fn opt_text(v: &CValue) -> Option<CatalogText> {
    match v {
        CValue::Null => None,
        other => Some(CatalogText::new(s(other))),
    }
}

fn uuid(v: &CValue) -> Uuid {
    Uuid::parse(s(v)).unwrap()
}

fn digest(v: &CValue) -> Digest {
    Digest::parse(s(v)).unwrap()
}

/// A fixture entry as a catalog row and its tags.
fn row(v: &CValue) -> (CatalogEntryRow, Vec<CatalogText>) {
    let o = obj(v);
    let t = |k: &str| CatalogText::new(s(get(o, k)));
    let value = match get(o, "value") {
        CValue::Null => None,
        val => {
            let vo = obj(val);
            Some(EntryValue {
                literal: CatalogText::new(s(get(vo, "literal"))),
                unit: CatalogText::new(s(get(vo, "unit"))),
                si: s(get(vo, "si")).parse::<f64>().unwrap(),
            })
        }
    };
    let row = CatalogEntryRow {
        project_id: uuid(get(o, "project_id")),
        record_id: t("record_id"),
        part: t("part"),
        revision: opt_text(get(o, "revision")),
        plan: t("plan"),
        run: t("run"),
        stage: t("stage"),
        characteristic: t("characteristic"),
        timestamp: t("timestamp"),
        value,
        decision_outcome: DecisionOutcome::parse(s(get(o, "decision_outcome"))).unwrap(),
        rule_id: opt_text(get(o, "rule_id")),
        evaluation_basis: EvaluationBasis::parse(s(get(o, "evaluation_basis"))).unwrap(),
        uec_class: UecClass::parse(s(get(o, "uec_class"))).unwrap(),
        labels: EvidenceLabels {
            claimed: b(get(o, "claimed")),
            declared_input: b(get(o, "declared_input")),
            uncharacterized_input: b(get(o, "uncharacterized_input")),
        },
        interpretation: Interpretation::parse(s(get(o, "interpretation"))).unwrap(),
        record_status: RecordStatus::parse(s(get(o, "record_status"))).unwrap(),
        journal_seq: s(get(o, "journal_seq")).parse().unwrap(),
        journal_entry_hash: digest(get(o, "journal_entry_hash")),
        entry_status: EntryStatus::parse(s(get(o, "entry_status"))).unwrap(),
    };
    let tags = arr(get(o, "tags")).iter().map(|t| CatalogText::new(s(t))).collect();
    (row, tags)
}

fn project_row(v: &CValue) -> ProjectRow {
    let o = obj(v);
    ProjectRow {
        project_id: uuid(get(o, "project_id")),
        handle: CatalogText::new(s(get(o, "handle"))),
        indexed: None,
        availability: Availability::Available,
    }
}

/// Creates a catalog with `projects` registered and `entries` inserted, in the given orders.
fn build(dir: &TempDir, projects: &[ProjectRow], entries: &[(CatalogEntryRow, Vec<CatalogText>)]) -> PathBuf {
    let path = dir.0.join("catalog.sqlite");
    let mut c = open(&path, Mode::ReadWrite);
    for p in projects {
        c.insert_project(p).unwrap();
    }
    for (e, tags) in entries {
        c.insert_entry(e).unwrap();
        for t in tags {
            c.insert_tag(&e.project_id, &e.record_id, t).unwrap();
        }
    }
    path
}

fn fixture_catalog(dir: &TempDir, c: &CValue) -> PathBuf {
    let o = obj(c);
    let projects: Vec<ProjectRow> = arr(get(o, "projects")).iter().map(project_row).collect();
    let entries: Vec<_> = arr(get(o, "entries")).iter().map(row).collect();
    build(dir, &projects, &entries)
}

// ------------------------------------------------------------------ synthetic sources

#[derive(Clone)]
struct Pos {
    hash: Digest,
    records: Vec<CatalogText>,
    unreadable: bool,
}

/// Synthetic source journals: project -> journal position -> entry. Counts opens and lookups.
#[derive(Default)]
struct Synth {
    journals: BTreeMap<Uuid, BTreeMap<u64, Pos>>,
    opens: Vec<Uuid>,
}

struct Lookup<'a>(&'a BTreeMap<u64, Pos>);

impl RecordLookup for Lookup<'_> {
    fn record_at(&mut self, seq: u64, record: &CatalogText) -> Result<SourceRecord, SourceUnreadable> {
        match self.0.get(&seq) {
            None => Ok(SourceRecord::Absent),
            Some(p) if p.unreadable => Err(SourceUnreadable),
            Some(p) if p.records.contains(record) => Ok(SourceRecord::Present { entry_hash: p.hash }),
            Some(_) => Ok(SourceRecord::Absent),
        }
    }
}

impl ProjectSources for Synth {
    fn open(&mut self, project: &ProjectRow) -> Option<Box<dyn RecordLookup + '_>> {
        self.opens.push(project.project_id);
        let j = self.journals.get(&project.project_id)?;
        Some(Box::new(Lookup(j)))
    }
}

fn fixture_sources(c: &CValue) -> Synth {
    let mut out = Synth::default();
    for (p, list) in obj(get(obj(c), "sources")) {
        let mut j = BTreeMap::new();
        for pos in arr(list) {
            let po = obj(pos);
            j.insert(
                s(get(po, "seq")).parse::<u64>().unwrap(),
                Pos {
                    hash: digest(get(po, "hash")),
                    records: arr(get(po, "records")).iter().map(|r| CatalogText::new(s(r))).collect(),
                    unreadable: b(get(po, "unreadable")),
                },
            );
        }
        out.journals.insert(Uuid::parse(p).unwrap(), j);
    }
    out
}

fn statuses(c: &Catalog) -> BTreeMap<String, EntryStatus> {
    let mut out = BTreeMap::new();
    let mut after: Option<(Uuid, CatalogText)> = None;
    loop {
        let page = c.entries_after(after.as_ref().map(|(p, r)| (p, r)), 10_000).unwrap();
        let Some(last) = page.last() else { break };
        after = Some((last.project_id, last.record_id.clone()));
        for e in &page {
            // Record IDs in the fixture are plain ASCII, so the display form is the raw text.
            out.insert(format!("{}/{}", e.project_id, e.record_id.as_display()), e.entry_status);
        }
    }
    out
}

fn expected_report(exp: &BTreeMap<String, CValue>) -> (Vec<ProjectCounts>, Vec<EntryKey>, Digest) {
    let per = arr(get(exp, "per_project"))
        .iter()
        .map(|p| {
            let o = obj(p);
            ProjectCounts {
                project_id: uuid(get(o, "project_id")),
                ok: n(get(o, "ok")),
                stale: n(get(o, "stale")),
                unavailable: n(get(o, "unavailable")),
            }
        })
        .collect();
    let stale = arr(get(exp, "stale"))
        .iter()
        .map(|k| {
            let k = arr(k);
            EntryKey { project_id: uuid(&k[0]), record_id: CatalogText::new(s(&k[1])) }
        })
        .collect();
    (per, stale, digest(get(exp, "digest")))
}

/// Alters a stored journal entry hash with a plain SQLite connection, as an edit of the
/// catalog file from outside would (AC-2).
fn alter_hash(path: &Path, project: &str, record: &str, hash: &str) {
    let conn = rusqlite::Connection::open(path).unwrap();
    let n = conn
        .execute(
            "UPDATE entry SET journal_entry_hash = ?1 WHERE project_id = ?2 AND record_id = ?3",
            rusqlite::params![hash, project, record],
        )
        .unwrap();
    assert_eq!(n, 1);
}

// ------------------------------------------------------------------ scenarios

/// AC-1, AC-2, AC-3 (library level) and REQ-001/-002: every hand-labelled scenario.
#[test]
fn scenarios_match_the_reference() {
    let c = cases();
    let mut ran = 0;
    for sc in arr(get(obj(&c), "scenarios")) {
        let so = obj(sc);
        let name = s(get(so, "name"));
        let dir = TempDir::new("sc");
        let path = fixture_catalog(&dir, &c);
        for e in arr(get(so, "catalog_edits")) {
            let eo = obj(e);
            alter_hash(&path, s(get(eo, "project_id")), s(get(eo, "record_id")), s(get(eo, "journal_entry_hash")));
        }
        let mut src = fixture_sources(&c);
        for p in arr(get(so, "remove_projects")) {
            src.journals.remove(&uuid(p));
        }
        for d in arr(get(so, "drop_records")) {
            let o = obj(d);
            let pos = src.journals.get_mut(&uuid(get(o, "project_id"))).unwrap();
            let pos = pos.get_mut(&s(get(o, "seq")).parse().unwrap()).unwrap();
            let r = CatalogText::new(s(get(o, "record_id")));
            pos.records.retain(|x| *x != r);
        }
        for d in arr(get(so, "unreadable")) {
            let o = obj(d);
            let pos = src.journals.get_mut(&uuid(get(o, "project_id"))).unwrap();
            pos.get_mut(&s(get(o, "seq")).parse().unwrap()).unwrap().unreadable = true;
        }
        for d in arr(get(so, "rehash")) {
            let o = obj(d);
            let pos = src.journals.get_mut(&uuid(get(o, "project_id"))).unwrap();
            pos.get_mut(&s(get(o, "seq")).parse().unwrap()).unwrap().hash = digest(get(o, "hash"));
        }
        let filter = match get(so, "filter") {
            CValue::Null => None,
            v => Some(uuid(v)),
        };
        let mut cat = open(&path, Mode::ReadWrite);
        let report = check_catalog(&mut cat, &mut src, filter.as_ref()).unwrap();
        let exp = obj(get(so, "expect"));
        let (per, stale, dig) = expected_report(exp);
        assert_eq!(report.per_project, per, "{name}: per-project counts");
        assert_eq!(report.stale, stale, "{name}: stale list");
        assert_eq!(report.digest, dig, "{name}: content digest");
        // Each checked project is opened exactly once, in project-ID order.
        let opened: Vec<Uuid> = per.iter().map(|p| p.project_id).collect();
        assert_eq!(src.opens, opened, "{name}: opens");
        // The statuses were written back.
        let want: BTreeMap<String, EntryStatus> =
            obj(get(exp, "statuses")).iter().map(|(k, v)| (k.clone(), EntryStatus::parse(s(v)).unwrap())).collect();
        assert_eq!(statuses(&cat), want, "{name}: stored statuses");
        assert_eq!(content_digest(&cat).unwrap(), dig, "{name}: digest after the check");
        // Checking again changes nothing.
        drop(cat);
        let mut cat = open(&path, Mode::ReadWrite);
        src.opens.clear();
        let again = check_catalog(&mut cat, &mut src, filter.as_ref()).unwrap();
        assert_eq!(again, report, "{name}: second check");
        assert_eq!(statuses(&cat), want, "{name}: stored statuses after a second check");
        ran += 1;
    }
    assert_eq!(ran, 7);
}

/// REQ-003, REQ-006: the digest ignores entry statuses (the fixture's initial statuses are mixed).
#[test]
fn digest_matches_reference_before_any_check() {
    let c = cases();
    let dir = TempDir::new("dig");
    let path = fixture_catalog(&dir, &c);
    let cat = open(&path, Mode::ReadOnly);
    assert_eq!(content_digest(&cat).unwrap(), digest(get(obj(&c), "digest")));
}

/// AC-5: projects registered and entries inserted in another order give the same digest.
#[test]
fn ac5_digest_independent_of_registration_and_insertion_order() {
    let c = cases();
    let o = obj(&c);
    let mut projects: Vec<ProjectRow> = arr(get(o, "projects")).iter().map(project_row).collect();
    let mut entries: Vec<_> = arr(get(o, "entries")).iter().map(row).collect();
    projects.reverse();
    entries.reverse();
    for (_, tags) in &mut entries {
        tags.reverse();
    }
    let dir = TempDir::new("ac5");
    let path = build(&dir, &projects, &entries);
    let cat = open(&path, Mode::ReadOnly);
    assert_eq!(content_digest(&cat).unwrap(), digest(get(o, "digest")));
}

#[test]
fn empty_catalog_digest() {
    let c = cases();
    let dir = TempDir::new("empty");
    let path = build(&dir, &[], &[]);
    let mut cat = open(&path, Mode::ReadWrite);
    assert_eq!(content_digest(&cat).unwrap(), digest(get(obj(&c), "empty_digest")));
    let report = check_catalog(&mut cat, &mut Synth::default(), None).unwrap();
    assert!(report.per_project.is_empty() && report.stale.is_empty());
    assert_eq!(report.digest, digest(get(obj(&c), "empty_digest")));
}

/// Paging: more entries than one catalog page, followed by another project's entry.
#[test]
fn more_entries_than_one_page() {
    let c = cases();
    let bulk = obj(get(obj(&c), "bulk"));
    let count = n(get(bulk, "count"));
    let hash = digest(get(bulk, "hash"));
    let (template, _) = row(get(bulk, "template"));
    let (other, other_tags) = row(get(bulk, "other"));
    let dir = TempDir::new("bulk");
    let path = dir.0.join("catalog.sqlite");
    {
        let mut cat = open(&path, Mode::ReadWrite);
        for p in arr(get(obj(&c), "projects")) {
            cat.insert_project(&project_row(p)).unwrap();
        }
    }
    {
        // One transaction for speed; the rows still meet every table check.
        let mut conn = rusqlite::Connection::open(&path).unwrap();
        let tx = conn.transaction().unwrap();
        {
            let mut st = tx
                .prepare(
                    "INSERT INTO entry (project_id, record_id, part, revision, plan, run, stage, characteristic, \
                     timestamp, value_literal, unit, si_value, decision_outcome, rule_id, evaluation_basis, \
                     uec_class, claimed, declared_input, uncharacterized_input, interpretation, record_status, \
                     journal_seq, journal_entry_hash, entry_status) \
                     VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, ?8, NULL, NULL, NULL, 'NOT_DECIDABLE', NULL, \
                     ?9, 'UEC-0', 0, 0, 0, ?10, ?11, ?12, ?13, 'unverified')",
                )
                .unwrap();
            let t = &template;
            for i in 0..count {
                st.execute(rusqlite::params![
                    t.project_id.to_string(),
                    format!("r-{i:05}"),
                    "BRACKET-100",
                    "plan-main",
                    "run-1",
                    "final",
                    "D1 diameter",
                    "2026-11-20T08:00:00Z",
                    t.evaluation_basis.as_str(),
                    t.interpretation.as_str(),
                    t.record_status.as_str(),
                    i64::try_from(i + 1).unwrap(),
                    hash.as_str(),
                ])
                .unwrap();
            }
        }
        tx.commit().unwrap();
    }
    let mut cat = open(&path, Mode::ReadWrite);
    cat.insert_entry(&other).unwrap();
    assert!(other_tags.is_empty());
    assert_eq!(cat.count_entries().unwrap(), count + 1);
    // The template agrees with the generated rows.
    assert_eq!(cat.entry(&template.project_id, &template.record_id).unwrap().unwrap(), template);
    assert_eq!(content_digest(&cat).unwrap(), digest(get(bulk, "digest")));

    // A source holding every generated record at its position.
    struct All {
        hash: Digest,
        count: u64,
        other: CatalogEntryRow,
    }
    struct AllLookup<'a>(&'a All, Uuid);
    impl RecordLookup for AllLookup<'_> {
        fn record_at(&mut self, seq: u64, record: &CatalogText) -> Result<SourceRecord, SourceUnreadable> {
            if self.1 == self.0.other.project_id {
                let o = &self.0.other;
                return Ok(if seq == o.journal_seq && *record == o.record_id {
                    SourceRecord::Present { entry_hash: o.journal_entry_hash }
                } else {
                    SourceRecord::Absent
                });
            }
            let want = CatalogText::new(&format!("r-{:05}", seq.wrapping_sub(1)));
            Ok(if seq >= 1 && seq <= self.0.count && *record == want {
                SourceRecord::Present { entry_hash: self.0.hash }
            } else {
                SourceRecord::Absent
            })
        }
    }
    impl ProjectSources for All {
        fn open(&mut self, project: &ProjectRow) -> Option<Box<dyn RecordLookup + '_>> {
            Some(Box::new(AllLookup(self, project.project_id)))
        }
    }
    let mut src = All { hash, count, other: other.clone() };
    let report = check_catalog(&mut cat, &mut src, None).unwrap();
    let counts: Vec<(u64, u64, u64)> = report.per_project.iter().map(|p| (p.ok, p.stale, p.unavailable)).collect();
    // P1 (bulk), P2 (one entry), P3 (no entries in this catalog).
    assert_eq!(counts, vec![(count, 0, 0), (1, 0, 0), (0, 0, 0)]);
    assert!(report.stale.is_empty());
    assert_eq!(report.digest, digest(get(bulk, "digest")));
    assert!(statuses(&cat).values().all(|s| *s == EntryStatus::Ok));
}

// ------------------------------------------------------------------ refusals

#[test]
fn read_only_catalog_is_refused_before_reading() {
    let c = cases();
    let dir = TempDir::new("ro");
    let path = fixture_catalog(&dir, &c);
    let mut cat = open(&path, Mode::ReadOnly);
    let mut src = fixture_sources(&c);
    let err = check_catalog(&mut cat, &mut src, None).unwrap_err();
    assert_eq!(err.code(), "stats.catalog_read_only");
    assert!(src.opens.is_empty());
}

#[test]
fn unknown_project_filter_is_refused_and_changes_nothing() {
    let c = cases();
    let dir = TempDir::new("unk");
    let path = fixture_catalog(&dir, &c);
    let mut cat = open(&path, Mode::ReadWrite);
    let before = statuses(&cat);
    let mut src = fixture_sources(&c);
    let stranger = Uuid::parse("01936f3a-0000-7000-8000-0000000000ff").unwrap();
    let err = check_catalog(&mut cat, &mut src, Some(&stranger)).unwrap_err();
    assert_eq!(err, CheckError::UnknownProject(stranger));
    assert_eq!(err.code(), "stats.catalog_unknown_project");
    assert!(src.opens.is_empty());
    assert_eq!(statuses(&cat), before);
    // The catalog is usable afterwards (the transaction was rolled back).
    check_catalog(&mut cat, &mut src, None).unwrap();
}

#[test]
fn locked_catalog_is_refused_and_changes_nothing() {
    let c = cases();
    let dir = TempDir::new("lock");
    let path = fixture_catalog(&dir, &c);
    let mut cat = open(&path, Mode::ReadWrite);
    let before = statuses(&cat);
    let other = rusqlite::Connection::open(&path).unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut src = fixture_sources(&c);
    let err = check_catalog(&mut cat, &mut src, None).unwrap_err();
    assert_eq!(err, CheckError::Catalog(CatalogError::Locked));
    assert_eq!(err.code(), "stats.catalog_locked");
    other.execute_batch("ROLLBACK").unwrap();
    assert_eq!(statuses(&cat), before);
}
