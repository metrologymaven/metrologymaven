//! IO.REG.ENV.IMPORT: the per-entity commit cost is measured, not guessed. A counting global
//! allocator records the real heap peak of preparing and committing imports of many tiny
//! entities, and the test fails unless the one budget of the import charged at least that
//! peak: for 1,000 and 5,000 entities into an empty project, per entity (the growth between
//! the two runs against `entity_cost`), and for a small import into a project that already
//! holds 5,000 elements (the checked copy of the tree). Own test binary, because it installs a
//! counting global allocator; one test function, so nothing else allocates concurrently.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use cap::Cap;
use fid_canon::{parse, CValue, Digest as CanonDigest};
use fid_io_reg::env::import::{
    entity_cost, import_file_id, register_import_events, ImportCommand, ImportEnv, ImportParams, ImportRun,
    ImportSource, Reader, StageSink,
};
use fid_io_reg::prov::{LengthUnit, PrincipalCommit, PrincipalKind, ReaderId, ReaderInfo, VersionToken};
use fid_io_reg::{
    Direction, FormatDescriptor, FormatId, IoError, Matcher, Origin, Phase, Position, Principal, Registry,
    ReportBuilder, RootAccess, RootId, RootSet, Signature, UntrustedText, VersionRange,
};
use fid_project::{ElementTypeDef, ElementTypeRegistry, Tree, Uuid, STAGE_ADDED};
use std::alloc::System;
use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;

#[global_allocator]
static ALLOC: Cap<System> = Cap::new(System, usize::MAX);

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/io/reg/env/import/");

/// `fid-test-tiny`: a header line `FTT1`, then one entity per line, its name the line and its
/// payload the line's bytes.
struct Tiny(FormatId, ReaderInfo);

fn read_all(src: &mut ImportSource<'_>) -> Result<Vec<u8>, IoError> {
    let mut data = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = src.read_bounded(&mut chunk)?;
        if n == 0 {
            return Ok(data);
        }
        let need = data.len() + n;
        if need > data.capacity() {
            let cap = need.max(data.capacity() * 2);
            src.budget_mut().charge_alloc((cap - data.capacity()) as u64, Position::byte(data.len() as u64))?;
            data.reserve_exact(cap - data.len());
        }
        data.extend_from_slice(&chunk[..n]);
    }
}

impl Reader for Tiny {
    fn format(&self) -> &FormatId {
        &self.0
    }
    fn info(&self) -> &ReaderInfo {
        &self.1
    }
    fn read(&self, _opts: &CValue, sink: &mut StageSink<'_, '_>, report: &mut ReportBuilder) -> Result<(), IoError> {
        let data = read_all(sink.source())?;
        sink.declare_unit(LengthUnit::Mm);
        let mut off = 0u64;
        for (i, line) in data.split(|b| *b == b'\n').enumerate() {
            let at = Position::byte(off);
            off += line.len() as u64 + 1;
            if i == 0 || line.is_empty() {
                continue;
            }
            let name = std::str::from_utf8(line).map_err(|_| IoError::Malformed { at, reason: "name" })?;
            sink.add_entity("test.point_set", &UntrustedText::new(name), line, at, report)?;
        }
        Ok(())
    }
}

fn tiny() -> Tiny {
    Tiny(
        FormatId::new("fid-test-tiny").unwrap(),
        ReaderInfo { id: ReaderId::new("fid-test-tiny").unwrap(), version: VersionToken::new("1").unwrap() },
    )
}

fn registry() -> Registry {
    let mut r = Registry::new();
    r.register(
        FormatDescriptor::new(
            FormatId::new("fid-test-tiny").unwrap(),
            VersionRange::new("1", "1").unwrap(),
            vec![Signature::new(0, Matcher::Bytes(b"FTT1".to_vec()), 50).unwrap()],
            vec!["ftt".to_string()],
            CValue::Obj(BTreeMap::new()),
            Phase::new(0).unwrap(),
            Origin::FirstParty,
            Direction::Import,
        )
        .unwrap(),
    )
    .unwrap();
    r
}

fn field(o: &BTreeMap<String, CValue>, k: &str) -> String {
    match o.get(k) {
        Some(CValue::Str(s)) => s.clone(),
        other => panic!("{k}: {other:?}"),
    }
}

/// The reference project (S1 adopting the default stage, S2), as in `tests/import.rs`.
fn project(cmd: &fid_types::CommandId) -> (Tree, Uuid) {
    let e = parse(&fs::read(format!("{DIR}expected.json")).unwrap()).unwrap();
    let CValue::Obj(o) = e else { panic!() };
    let ns = Uuid::parse(&field(&o, "namespace")).unwrap();
    let mut reg = ElementTypeRegistry::new();
    let mut schema = BTreeMap::new();
    schema.insert("type".to_string(), CValue::Str("object".into()));
    reg.register(ElementTypeDef::new("test.point_set", CValue::Obj(schema), "point_cloud").unwrap()).unwrap();
    register_import_events(&mut reg, &[&import_file_id().unwrap(), cmd]).unwrap();
    let mut t = Tree::new(ns, Arc::new(reg)).unwrap();
    let run = field(&o, "run");
    for (id, label, adopts) in
        [(field(&o, "s1"), "S1", format!("\"{}\"", field(&o, "default_stage"))), (field(&o, "s2"), "S2", "null".into())]
    {
        let b = format!(
            r#"{{"adopts":{adopts},"id":"{id}","label":"{label}","metadata":{{}},"run":"{run}","serial":null}}"#
        );
        t.apply(STAGE_ADDED, &parse(b.as_bytes()).unwrap()).unwrap();
    }
    (t, ns)
}

fn write(dir: &std::path::Path, name: &str, n: u64, tag: &str) {
    let mut s = String::from("FTT1\n");
    for i in 0..n {
        s.push_str(&format!("{tag}{i}\n"));
    }
    fs::write(dir.join(name), s).unwrap();
}

/// Runs `f` and returns the heap peak above the level before it, and `f`'s result.
fn measure(f: impl FnOnce() -> u64) -> (u64, u64) {
    // Raise the current level to the earlier peak, so the next peak is this run's own.
    let ballast: Vec<u8> = vec![0u8; ALLOC.max_allocated() - ALLOC.allocated()];
    let base = ALLOC.allocated();
    let charged = f();
    let peak = (ALLOC.max_allocated() - base) as u64;
    drop(ballast);
    (peak, charged)
}

#[test]
fn the_budget_covers_the_real_peak() {
    let raw = std::env::temp_dir().join(format!("fid-io-import-alloc-{}", std::process::id()));
    let _ = fs::remove_dir_all(&raw);
    fs::create_dir_all(raw.join("in")).unwrap();
    for (name, n, tag) in [("a.ftt", 1000, "e"), ("b.ftt", 5000, "e"), ("c.ftt", 10, "late")] {
        write(&raw.join("in"), name, n, tag);
    }
    let mut roots = RootSet::new();
    roots.insert(RootId::new("in").unwrap(), RootAccess::Import, raw.join("in"));
    let reg = registry();
    let cmd = ImportCommand::new(tiny(), &[]).unwrap();
    let (tree, ns) = project(cmd.id());
    let commit =
        PrincipalCommit { kind: PrincipalKind::Agent, commit: CanonDigest([0x5a; 32]), on_behalf_of_commit: None };

    let run = |tree: &Tree, src: &str, target: &mut Tree| -> u64 {
        let env = ImportEnv::new(&reg, &roots, Principal::Agent, commit, tree, ns);
        let p = cmd.prepare(&env, &ImportParams::new(src), None).unwrap();
        let charged = p.alloc_charged();
        p.commit(target).unwrap();
        charged
    };

    let mut report = Vec::new();
    let mut peaks = Vec::new();
    for (src, n) in [("root://in/a.ftt", 1000u64), ("root://in/b.ftt", 5000)] {
        let mut target = tree.clone();
        let (peak, charged) = measure(|| run(&tree, src, &mut target));
        report.push(format!("{n} entities: peak {peak} B, charged {charged} B"));
        peaks.push((n, peak, charged, target));
    }
    let per_entity = peaks[1].1.saturating_sub(peaks[0].1) / (peaks[1].0 - peaks[0].0);
    let ok = peaks.iter().all(|(_, peak, charged, _)| charged >= peak);
    // Name `e<i>` (at most 5 bytes) and type `test.point_set` (14 bytes); payload the name.
    let cost = entity_cost(5 + 14, 5);
    report.push(format!("measured per entity: {per_entity} B, charged per entity: {cost} B"));

    // A small import into a project holding 5,000 elements: the checked copy is charged.
    let big = peaks.pop().unwrap().3;
    let mut target = big.clone();
    let (peak, charged) = measure(|| run(&big, "root://in/c.ftt", &mut target));
    report.push(format!("10 entities into 5,000: peak {peak} B, charged {charged} B"));
    let report = report.join("\n");
    eprintln!("{report}");
    assert!(ok, "an import was charged less than its peak:\n{report}");
    assert!(cost >= per_entity, "entity cost {cost} < measured {per_entity}:\n{report}");
    assert!(charged >= peak, "into a big project: charged {charged} < peak {peak}:\n{report}");
    let _ = fs::remove_dir_all(&raw);
}
